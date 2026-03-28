use super::*;

impl App {
    pub(crate) fn consume_sync_events(&mut self) {
        let active_profile = self.active_profile.as_ref().map(|profile| profile.id);
        for event in self.sync.drain() {
            match event {
                SyncEvent::Message { profile_id, item } if Some(profile_id) == active_profile => {
                    if let Err(error) = self.ingest_messages(vec![item]) {
                        self.show_sync_error_toast(error.to_string(), ToastMode::AutoDismiss);
                    }
                }
                SyncEvent::Status {
                    profile_id,
                    message,
                } if Some(profile_id) == active_profile => {
                    self.status = message;
                }
                SyncEvent::Error {
                    profile_id,
                    message,
                } if Some(profile_id) == active_profile => {
                    self.show_sync_error_toast(message, ToastMode::AutoDismiss);
                }
                _ => {}
            }
        }
    }

    fn ingest_messages(&mut self, items: Vec<crate::relay::RelayMessage>) -> Result<()> {
        self.last_poll_count = items.len();
        let mut inserted = 0usize;
        let mut unresolved = 0usize;
        let mut last_seen_relay_message_id = None;
        for item in items {
            last_seen_relay_message_id = Some(item.id.clone());
            if self.storage.has_relay_message(&item.id)? {
                continue;
            }
            match self.ingest_message(item)? {
                IngestOutcome::Stored { conversation_id } => {
                    inserted += 1;
                    self.unread_conversations.insert(conversation_id);
                }
                IngestOutcome::Unresolved {
                    conversation_id,
                    reason,
                } => {
                    unresolved += 1;
                    self.unread_conversations.insert(conversation_id);
                    self.last_receive_error = Some(reason);
                }
                IngestOutcome::Ignored => {}
            }
        }
        if let Some(profile) = self.active_profile.as_mut() {
            if let Some(last_seen_relay_message_id) = last_seen_relay_message_id.as_deref() {
                self.storage
                    .update_profile_sync_cursor(profile.id, Some(last_seen_relay_message_id))?;
                profile.last_synced_relay_message_id = Some(last_seen_relay_message_id.to_string());
            }
        }
        self.last_ingest_stored = inserted;
        self.last_ingest_unresolved = unresolved;
        self.reload_conversations()?;
        self.replenish_prekeys_if_needed()?;
        if inserted > 0 || unresolved > 0 {
            self.status = format!(
                "sync ok: stored={inserted} unresolved={unresolved} from poll={}",
                self.last_poll_count
            );
        }
        Ok(())
    }

    fn ingest_message(&mut self, item: crate::relay::RelayMessage) -> Result<IngestOutcome> {
        let profile = self
            .active_profile
            .clone()
            .ok_or_else(|| anyhow!("no active profile"))?;
        let header: MessageHeader = match serde_json::from_str(&item.header) {
            Ok(header) => header,
            Err(error) => {
                let conversation_id = self.store_unresolved_message(
                    &profile,
                    "Relay Diagnostics",
                    None,
                    None,
                    &item,
                    MessageStatus::UnsupportedHeader,
                    format!("header parse failed: {error}"),
                )?;
                return Ok(IngestOutcome::Unresolved {
                    conversation_id,
                    reason: "header parse failed".to_string(),
                });
            }
        };

        match header.message_type.as_str() {
            "prekey_message" => {
                let mut profile_with_keys = profile.clone();
                let decrypted = match receive_prekey_message(
                    &mut profile_with_keys.keys,
                    &profile.username,
                    &profile.inbox_id,
                    &item.header,
                    &item.ciphertext,
                ) {
                    Ok(decrypted) => decrypted,
                    Err(error) => {
                        let conversation_id = self.store_unresolved_message(
                            &profile,
                            &header.sender_username,
                            Some(header.sender_username.clone()),
                            Some(header.sender_inbox_id.clone()),
                            &item,
                            MessageStatus::DecryptFailed,
                            format!("prekey decrypt failed: {error}"),
                        )?;
                        return Ok(IngestOutcome::Unresolved {
                            conversation_id,
                            reason: "prekey decrypt failed".to_string(),
                        });
                    }
                };
                let mut session = decrypted.session;
                session.session_id = header.session_id.clone();
                session.peer_username = Some(header.sender_username.clone());
                session.peer_inbox_id = header.sender_inbox_id.clone();

                self.storage
                    .update_profile_keys(profile.id, &profile_with_keys.keys)?;
                self.active_profile = Some(profile_with_keys.clone());

                let contact = self.storage.upsert_contact(
                    profile.id,
                    &UpsertContact {
                        username: Some(header.sender_username.clone()),
                        inbox_id: Some(header.sender_inbox_id.clone()),
                        display_name: header.sender_username.clone(),
                        identity_key: Some(decrypted.sender_identity_json),
                    },
                )?;
                let conversation = self.storage.ensure_conversation(profile.id, contact.id)?;
                self.storage.set_session(conversation.id, &session)?;
                self.storage.insert_message(
                    conversation.id,
                    &NewMessage {
                        relay_message_id: Some(item.id),
                        client_message_id: item.client_message_id,
                        direction: MessageDirection::Incoming,
                        body: Some(decrypted.plaintext),
                        header: item.header,
                        ciphertext: item.ciphertext,
                        relay_kind: item.kind,
                        created_at: item.created_at,
                        status: MessageStatus::Received,
                        error_reason: None,
                    },
                )?;
                Ok(IngestOutcome::Stored {
                    conversation_id: conversation.id,
                })
            }
            "ratchet_message" => {
                let conversations = self.storage.list_conversations(profile.id)?;
                let Some((conversation, _)) = conversations
                    .iter()
                    .find(|(conversation, contact)| {
                        conversation
                            .session
                            .as_ref()
                            .map(|session| session.session_id == header.session_id)
                            .unwrap_or(false)
                            || contact
                                .inbox_id
                                .as_ref()
                                .map(|inbox_id| inbox_id == &header.sender_inbox_id)
                                .unwrap_or(false)
                    })
                    .cloned()
                else {
                    let conversation_id = self.store_unresolved_message(
                        &profile,
                        &header.sender_username,
                        Some(header.sender_username.clone()),
                        Some(header.sender_inbox_id.clone()),
                        &item,
                        MessageStatus::SessionMissing,
                        format!(
                            "session missing for ratchet {} / {}",
                            header.session_id, header.sender_inbox_id
                        ),
                    )?;
                    return Ok(IngestOutcome::Unresolved {
                        conversation_id,
                        reason: "session missing".to_string(),
                    });
                };
                let session = conversation
                    .session
                    .clone()
                    .ok_or_else(|| anyhow!("missing session for ratchet message"))?;
                let decrypted =
                    match receive_session_message(&session, &item.header, &item.ciphertext) {
                        Ok(decrypted) => decrypted,
                        Err(error) => {
                            let conversation_id = self.store_unresolved_message(
                                &profile,
                                &header.sender_username,
                                Some(header.sender_username.clone()),
                                Some(header.sender_inbox_id.clone()),
                                &item,
                                MessageStatus::DecryptFailed,
                                format!("ratchet decrypt failed: {error}"),
                            )?;
                            return Ok(IngestOutcome::Unresolved {
                                conversation_id,
                                reason: "ratchet decrypt failed".to_string(),
                            });
                        }
                    };
                self.storage
                    .set_session(conversation.id, &decrypted.session)?;
                self.storage.insert_message(
                    conversation.id,
                    &NewMessage {
                        relay_message_id: Some(item.id),
                        client_message_id: item.client_message_id,
                        direction: MessageDirection::Incoming,
                        body: Some(decrypted.plaintext),
                        header: item.header,
                        ciphertext: item.ciphertext,
                        relay_kind: item.kind,
                        created_at: item.created_at,
                        status: MessageStatus::Received,
                        error_reason: None,
                    },
                )?;
                Ok(IngestOutcome::Stored {
                    conversation_id: conversation.id,
                })
            }
            _ => {
                let conversation_id = self.store_unresolved_message(
                    &profile,
                    "Relay Diagnostics",
                    Some(header.sender_username),
                    Some(header.sender_inbox_id),
                    &item,
                    MessageStatus::UnsupportedHeader,
                    format!("unsupported message type: {}", header.message_type),
                )?;
                Ok(IngestOutcome::Unresolved {
                    conversation_id,
                    reason: "unsupported message type".to_string(),
                })
            }
        }
    }

    pub(crate) fn replenish_prekeys_if_needed(&mut self) -> Result<()> {
        let Some(profile) = self.active_profile.clone() else {
            return Ok(());
        };
        if available_prekeys(&profile.keys) >= 5 {
            return Ok(());
        }
        let Some(server) = self.active_server.clone() else {
            return Ok(());
        };
        let relay = RelayClient::new(server.base_url.clone())?;
        let mut profile = profile;
        let new_prekeys = generate_more_prekeys(12);
        let public_keys: Vec<String> = new_prekeys
            .iter()
            .map(|prekey| prekey.public_key.clone())
            .collect();
        relay.add_prekeys(&profile.username, &public_keys)?;
        profile.keys.one_time_prekeys.extend(new_prekeys);
        self.storage
            .update_profile_keys(profile.id, &profile.keys)?;
        self.active_profile = Some(profile);
        self.status = "Uploaded more one-time prekeys.".to_string();
        Ok(())
    }

    fn store_unresolved_message(
        &mut self,
        profile: &LocalProfileRecord,
        display_name: &str,
        username: Option<String>,
        inbox_id: Option<String>,
        item: &crate::relay::RelayMessage,
        status: MessageStatus,
        reason: String,
    ) -> Result<i64> {
        let contact = self.storage.upsert_contact(
            profile.id,
            &UpsertContact {
                username,
                inbox_id,
                display_name: display_name.to_string(),
                identity_key: None,
            },
        )?;
        let conversation = self.storage.ensure_conversation(profile.id, contact.id)?;
        self.storage.insert_message(
            conversation.id,
            &NewMessage {
                relay_message_id: Some(item.id.clone()),
                client_message_id: item.client_message_id.clone(),
                direction: MessageDirection::Incoming,
                body: None,
                header: item.header.clone(),
                ciphertext: item.ciphertext.clone(),
                relay_kind: item.kind.clone(),
                created_at: item.created_at.clone(),
                status,
                error_reason: Some(reason),
            },
        )?;
        Ok(conversation.id)
    }
}
