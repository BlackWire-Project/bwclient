use super::*;

impl App {
    pub(super) fn ingest_messages(&mut self, items: Vec<crate::relay::RelayMessage>) -> Result<()> {
        self.last_poll_count = items.len();
        let mut inserted = 0usize;
        let mut unresolved = 0usize;
        for item in items {
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
            "prekey_message" => self.ingest_prekey_message(profile, header, item),
            "ratchet_message" => self.ingest_ratchet_message(profile, header, item),
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

    fn ingest_prekey_message(
        &mut self,
        profile: LocalProfileRecord,
        header: MessageHeader,
        item: crate::relay::RelayMessage,
    ) -> Result<IngestOutcome> {
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

    fn ingest_ratchet_message(
        &mut self,
        profile: LocalProfileRecord,
        header: MessageHeader,
        item: crate::relay::RelayMessage,
    ) -> Result<IngestOutcome> {
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
        let decrypted = match receive_session_message(&session, &item.header, &item.ciphertext) {
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
}
