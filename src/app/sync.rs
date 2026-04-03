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
                if let Some(reason) =
                    self.prekey_identity_mismatch_reason(profile.id, &header, &decrypted.sender_identity_json)?
                {
                    let conversation_id = self.store_unresolved_message(
                        &profile,
                        &header.sender_username,
                        Some(header.sender_username.clone()),
                        Some(header.sender_inbox_id.clone()),
                        &item,
                        MessageStatus::IdentityMismatch,
                        reason,
                    )?;
                    return Ok(IngestOutcome::Unresolved {
                        conversation_id,
                        reason: "identity mismatch".to_string(),
                    });
                }
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

    fn prekey_identity_mismatch_reason(
        &self,
        profile_id: i64,
        header: &MessageHeader,
        incoming_identity_json: &str,
    ) -> Result<Option<String>> {
        let incoming_identity = parse_identity_json(incoming_identity_json)?;
        for contact in self.storage.list_contacts(profile_id)? {
            let matches_contact = contact.username.as_deref() == Some(header.sender_username.as_str())
                || contact.inbox_id.as_deref() == Some(header.sender_inbox_id.as_str());
            if !matches_contact {
                continue;
            }
            let Some(existing_identity_json) = contact.identity_key.as_deref() else {
                continue;
            };
            let existing_identity = parse_identity_json(existing_identity_json)?;
            if existing_identity.sign != incoming_identity.sign
                || existing_identity.dh != incoming_identity.dh
            {
                return Ok(Some(format!(
                    "identity mismatch for contact '{}' / inbox '{}'",
                    header.sender_username, header.sender_inbox_id
                )));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::PathBuf, time::Instant};

    use super::*;
    use crate::{
        app::{InputMode, LoginFocus, Screen},
        crypto::{MessageHeader, generate_profile_material, identity_json, prepare_initial_message, prepare_session_message, rotate_local_ratchet},
        relay::RelayMessage,
        state::{MessageStatus, StoredProfileKeys},
        storage::Storage,
        sync::SyncService,
    };
    use uuid::Uuid;

    struct TestDb {
        path: PathBuf,
    }

    impl TestDb {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("bwclient-{name}-{}.db", Uuid::new_v4()));
            Self { path }
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn test_app(username: &str, keys: &StoredProfileKeys, inbox_id: &str) -> (App, TestDb) {
        let db = TestDb::new(username);
        let storage = Storage::open(&db.path).unwrap();
        let server_id = storage
            .create_server("test", "http://127.0.0.1:8080", Some("ws://127.0.0.1:8080/ws"))
            .unwrap();
        let profile_id = storage
            .create_profile(server_id, username, inbox_id, true, keys)
            .unwrap();
        let server = storage.get_server(server_id).unwrap();
        let profile = storage.get_profile(profile_id).unwrap();
        let app = App {
            storage,
            sync: SyncService::new(),
            exit: false,
            status: String::new(),
            show_technical: false,
            screen: Screen::Main,
            login_focus: LoginFocus::Servers,
            servers: vec![server.clone()],
            selected_server: 0,
            profiles: vec![profile.clone()],
            selected_profile: 0,
            active_server: Some(server),
            active_profile: Some(profile),
            conversations: Vec::new(),
            selected_conversation: 0,
            messages: Vec::new(),
            message_scroll: 0,
            message_auto_follow: true,
            composer: String::new(),
            input_mode: InputMode::Command,
            unread_conversations: BTreeSet::new(),
            last_poll_count: 0,
            last_ingest_stored: 0,
            last_ingest_unresolved: 0,
            last_receive_error: None,
            toast: None,
            started_at: Instant::now(),
        };
        (app, db)
    }

    fn relay_message(kind: &str, inbox_id: &str, header: String, ciphertext: String) -> RelayMessage {
        RelayMessage {
            id: Uuid::new_v4().to_string(),
            inbox_id: inbox_id.to_string(),
            kind: kind.to_string(),
            header,
            ciphertext,
            used_prekey_id: None,
            client_message_id: Uuid::new_v4().to_string(),
            created_at: "1234567890".to_string(),
            expires_at: "1234569999".to_string(),
        }
    }

    fn relay_bundle(username: &str, inbox_id: &str, keys: &StoredProfileKeys) -> crate::relay::RelayBundle {
        crate::relay::RelayBundle {
            username: username.to_string(),
            identity_key: identity_json(keys).unwrap(),
            signed_prekey: keys.signed_prekey_public.clone(),
            signed_prekey_signature: keys.signed_prekey_signature.clone(),
            inbox_id: inbox_id.to_string(),
            prekey_id: Some(Uuid::new_v4().to_string()),
            one_time_prekey: Some(keys.one_time_prekeys[0].public_key.clone()),
        }
    }

    fn prepared_prekey_message(
        sender_username: &str,
        sender_inbox_id: &str,
        sender_keys: &StoredProfileKeys,
        recipient_username: &str,
        recipient_inbox_id: &str,
        recipient_keys: &StoredProfileKeys,
        plaintext: &str,
    ) -> crate::crypto::PreparedMessage {
        prepare_initial_message(
            sender_username,
            sender_inbox_id,
            sender_keys,
            &relay_bundle(recipient_username, recipient_inbox_id, recipient_keys),
            plaintext,
        )
        .unwrap()
    }

    #[test]
    fn ingest_message_stores_invalid_header_as_unresolved_diagnostics() {
        let (keys, inbox_id) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("bob", &keys, &inbox_id);
        let item = relay_message(
            "prekey_message",
            &inbox_id,
            "{not-json".to_string(),
            "ciphertext".to_string(),
        );

        let outcome = app.ingest_message(item).unwrap();
        assert!(matches!(outcome, IngestOutcome::Unresolved { .. }));

        let conversations = app
            .storage
            .list_conversations(app.active_profile.as_ref().unwrap().id)
            .unwrap();
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].1.display_name, "Relay Diagnostics");

        let messages = app.storage.get_messages(conversations[0].0.id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].status, MessageStatus::UnsupportedHeader);
    }

    #[test]
    fn ingest_message_marks_ratchet_without_session_as_unresolved() {
        let (keys, inbox_id) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("alice", &keys, &inbox_id);
        let header = serde_json::to_string(&MessageHeader {
            version: 1,
            sender_username: "bob".to_string(),
            sender_inbox_id: "bob-inbox".to_string(),
            message_type: "ratchet_message".to_string(),
            session_id: Uuid::new_v4().to_string(),
            sender_identity_sign: String::new(),
            sender_identity_dh: String::new(),
            dh_pub: keys.identity_dh_public.clone(),
            recipient_signed_prekey: None,
            used_one_time_prekey: None,
            pn: 0,
            n: 0,
            timestamp: "1234567890".to_string(),
        })
        .unwrap();
        let item = relay_message("ratchet_message", &inbox_id, header, "junk".to_string());

        let outcome = app.ingest_message(item).unwrap();
        assert!(matches!(outcome, IngestOutcome::Unresolved { .. }));

        let conversations = app
            .storage
            .list_conversations(app.active_profile.as_ref().unwrap().id)
            .unwrap();
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].1.username.as_deref(), Some("bob"));

        let messages = app.storage.get_messages(conversations[0].0.id).unwrap();
        assert_eq!(messages[0].status, MessageStatus::SessionMissing);
    }

    #[test]
    fn ingest_message_marks_ratchet_decrypt_failure_when_session_matches_but_ciphertext_is_invalid() {
        let (alice_keys, alice_inbox) = generate_profile_material(8).unwrap();
        let (mut bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("alice", &alice_keys, &alice_inbox);
        let bob_identity = identity_json(&bob_keys).unwrap();
        let contact = app
            .storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: Some("bob".to_string()),
                    inbox_id: Some(bob_inbox.clone()),
                    display_name: "bob".to_string(),
                    identity_key: Some(bob_identity),
                },
            )
            .unwrap();
        let conversation = app
            .storage
            .ensure_conversation(app.active_profile.as_ref().unwrap().id, contact.id)
            .unwrap();

        let bob_bundle = crate::relay::RelayBundle {
            username: "bob".to_string(),
            identity_key: identity_json(&bob_keys).unwrap(),
            signed_prekey: bob_keys.signed_prekey_public.clone(),
            signed_prekey_signature: bob_keys.signed_prekey_signature.clone(),
            inbox_id: bob_inbox.clone(),
            prekey_id: Some(Uuid::new_v4().to_string()),
            one_time_prekey: Some(bob_keys.one_time_prekeys[0].public_key.clone()),
        };
        let alice_out = prepare_initial_message(
            "alice",
            &alice_inbox,
            &alice_keys,
            &bob_bundle,
            "hello",
        )
        .unwrap();
        app.storage
            .set_session(conversation.id, &alice_out.session)
            .unwrap();
        let bob_in = receive_prekey_message(
            &mut bob_keys,
            "bob",
            &bob_inbox,
            &alice_out.header_json,
            &alice_out.ciphertext,
        )
        .unwrap();
        let mut bob_session = bob_in.session;
        bob_session.session_id = bob_in.header.session_id.clone();
        bob_session.peer_username = Some("alice".to_string());
        bob_session.peer_inbox_id = alice_inbox.clone();
        rotate_local_ratchet(&mut bob_session).unwrap();
        let bob_reply = prepare_session_message(&bob_session, "bob", &bob_inbox, "reply").unwrap();
        let item = relay_message(
            "ratchet_message",
            &alice_inbox,
            bob_reply.header_json,
            "not-valid-ciphertext".to_string(),
        );

        let outcome = app.ingest_message(item).unwrap();
        assert!(matches!(outcome, IngestOutcome::Unresolved { .. }));

        let messages = app.storage.get_messages(conversation.id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].status, MessageStatus::DecryptFailed);
    }

    #[test]
    fn ingest_prekey_message_accepts_known_contact_when_identity_matches() {
        let (alice_keys, alice_inbox) = generate_profile_material(8).unwrap();
        let (bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("bob", &bob_keys, &bob_inbox);
        let original_identity = identity_json(&alice_keys).unwrap();
        let contact = app
            .storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: Some("alice".to_string()),
                    inbox_id: Some(alice_inbox.clone()),
                    display_name: "alice".to_string(),
                    identity_key: Some(original_identity.clone()),
                },
            )
            .unwrap();
        let message = prepared_prekey_message(
            "alice",
            &alice_inbox,
            &alice_keys,
            "bob",
            &bob_inbox,
            &bob_keys,
            "hello",
        );
        let item = relay_message(
            "prekey_message",
            &bob_inbox,
            message.header_json,
            message.ciphertext,
        );

        let outcome = app.ingest_message(item).unwrap();
        let conversation_id = match outcome {
            IngestOutcome::Stored { conversation_id } => conversation_id,
            _ => panic!("expected stored message"),
        };

        let updated_contact = app.storage.get_contact(contact.id).unwrap();
        assert_eq!(updated_contact.identity_key.as_deref(), Some(original_identity.as_str()));

        let messages = app.storage.get_messages(conversation_id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].status, MessageStatus::Received);
        assert_eq!(messages[0].body.as_deref(), Some("hello"));
    }

    #[test]
    fn ingest_prekey_message_rejects_spoofed_username_for_existing_contact_with_different_identity() {
        let (alice_keys, alice_inbox) = generate_profile_material(8).unwrap();
        let (mallory_keys, _mallory_inbox) = generate_profile_material(8).unwrap();
        let (bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("bob", &bob_keys, &bob_inbox);
        let original_identity = identity_json(&alice_keys).unwrap();
        let contact = app
            .storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: Some("alice".to_string()),
                    inbox_id: Some(alice_inbox.clone()),
                    display_name: "alice".to_string(),
                    identity_key: Some(original_identity.clone()),
                },
            )
            .unwrap();
        let spoofed = prepared_prekey_message(
            "alice",
            &alice_inbox,
            &mallory_keys,
            "bob",
            &bob_inbox,
            &bob_keys,
            "spoofed hello",
        );
        let item = relay_message(
            "prekey_message",
            &bob_inbox,
            spoofed.header_json,
            spoofed.ciphertext,
        );

        let outcome = app.ingest_message(item).unwrap();
        let conversation_id = match outcome {
            IngestOutcome::Unresolved { conversation_id, reason } => {
                assert_eq!(reason, "identity mismatch");
                conversation_id
            }
            _ => panic!("expected unresolved message"),
        };

        let updated_contact = app.storage.get_contact(contact.id).unwrap();
        assert_eq!(updated_contact.username.as_deref(), Some("alice"));
        assert_eq!(updated_contact.identity_key.as_deref(), Some(original_identity.as_str()));

        let messages = app.storage.get_messages(conversation_id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].status, MessageStatus::IdentityMismatch);
        assert_eq!(messages[0].body.as_deref(), None);
        assert!(messages[0]
            .error_reason
            .as_deref()
            .unwrap_or_default()
            .contains("identity mismatch"));
    }

    #[test]
    fn ingest_prekey_message_accepts_new_identity_for_contact_without_saved_identity_key() {
        let (alice_keys, alice_inbox) = generate_profile_material(8).unwrap();
        let (bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("bob", &bob_keys, &bob_inbox);
        let contact = app
            .storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: Some("alice".to_string()),
                    inbox_id: Some(alice_inbox.clone()),
                    display_name: "alice".to_string(),
                    identity_key: None,
                },
            )
            .unwrap();
        let message = prepared_prekey_message(
            "alice",
            &alice_inbox,
            &alice_keys,
            "bob",
            &bob_inbox,
            &bob_keys,
            "hello",
        );

        let outcome = app
            .ingest_message(relay_message("prekey_message", &bob_inbox, message.header_json, message.ciphertext))
            .unwrap();
        assert!(matches!(outcome, IngestOutcome::Stored { .. }));

        let updated_contact = app.storage.get_contact(contact.id).unwrap();
        assert_eq!(
            updated_contact.identity_key.as_deref(),
            Some(identity_json(&alice_keys).unwrap().as_str())
        );
    }

    #[test]
    fn ingest_prekey_message_rejects_mismatch_when_contact_matches_by_username_only() {
        let (alice_keys, alice_inbox) = generate_profile_material(8).unwrap();
        let (mallory_keys, mallory_inbox) = generate_profile_material(8).unwrap();
        let (bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("bob", &bob_keys, &bob_inbox);
        let original_identity = identity_json(&alice_keys).unwrap();
        let contact = app
            .storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: Some("alice".to_string()),
                    inbox_id: None,
                    display_name: "alice".to_string(),
                    identity_key: Some(original_identity.clone()),
                },
            )
            .unwrap();
        let spoofed = prepared_prekey_message(
            "alice",
            &mallory_inbox,
            &mallory_keys,
            "bob",
            &bob_inbox,
            &bob_keys,
            "spoofed",
        );

        let outcome = app
            .ingest_message(relay_message("prekey_message", &bob_inbox, spoofed.header_json, spoofed.ciphertext))
            .unwrap();
        assert!(matches!(outcome, IngestOutcome::Unresolved { reason, .. } if reason == "identity mismatch"));

        let updated_contact = app.storage.get_contact(contact.id).unwrap();
        assert_eq!(updated_contact.username.as_deref(), Some("alice"));
        assert_eq!(updated_contact.inbox_id.as_deref(), Some(mallory_inbox.as_str()));
        assert_eq!(updated_contact.identity_key.as_deref(), Some(original_identity.as_str()));
    }

    #[test]
    fn ingest_prekey_message_rejects_mismatch_when_contact_matches_by_inbox_only() {
        let (alice_keys, _alice_inbox) = generate_profile_material(8).unwrap();
        let (mallory_keys, shared_inbox) = generate_profile_material(8).unwrap();
        let (bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("bob", &bob_keys, &bob_inbox);
        let original_identity = identity_json(&alice_keys).unwrap();
        let contact = app
            .storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: None,
                    inbox_id: Some(shared_inbox.clone()),
                    display_name: "mystery".to_string(),
                    identity_key: Some(original_identity.clone()),
                },
            )
            .unwrap();
        let spoofed = prepared_prekey_message(
            "mallory",
            &shared_inbox,
            &mallory_keys,
            "bob",
            &bob_inbox,
            &bob_keys,
            "spoofed",
        );

        let outcome = app
            .ingest_message(relay_message("prekey_message", &bob_inbox, spoofed.header_json, spoofed.ciphertext))
            .unwrap();
        assert!(matches!(outcome, IngestOutcome::Unresolved { reason, .. } if reason == "identity mismatch"));

        let updated_contact = app.storage.get_contact(contact.id).unwrap();
        assert_eq!(updated_contact.username.as_deref(), Some("mallory"));
        assert_eq!(updated_contact.inbox_id.as_deref(), Some(shared_inbox.as_str()));
        assert_eq!(updated_contact.identity_key.as_deref(), Some(original_identity.as_str()));
    }

    #[test]
    fn ingest_prekey_message_rejects_if_any_matching_contact_has_different_identity() {
        let (alice_keys, alice_inbox) = generate_profile_material(8).unwrap();
        let (mallory_keys, _mallory_inbox) = generate_profile_material(8).unwrap();
        let (bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("bob", &bob_keys, &bob_inbox);
        app.storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: Some("alice".to_string()),
                    inbox_id: None,
                    display_name: "alice-by-name".to_string(),
                    identity_key: Some(identity_json(&alice_keys).unwrap()),
                },
            )
            .unwrap();
        app.storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: None,
                    inbox_id: Some(alice_inbox.clone()),
                    display_name: "alice-by-inbox".to_string(),
                    identity_key: Some(identity_json(&mallory_keys).unwrap()),
                },
            )
            .unwrap();
        let message = prepared_prekey_message(
            "alice",
            &alice_inbox,
            &alice_keys,
            "bob",
            &bob_inbox,
            &bob_keys,
            "hello",
        );

        let outcome = app
            .ingest_message(relay_message("prekey_message", &bob_inbox, message.header_json, message.ciphertext))
            .unwrap();
        assert!(matches!(outcome, IngestOutcome::Unresolved { reason, .. } if reason == "identity mismatch"));
    }

    #[test]
    fn ingest_prekey_message_allows_unknown_sender_and_persists_identity() {
        let (alice_keys, alice_inbox) = generate_profile_material(8).unwrap();
        let (bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("bob", &bob_keys, &bob_inbox);
        let message = prepared_prekey_message(
            "alice",
            &alice_inbox,
            &alice_keys,
            "bob",
            &bob_inbox,
            &bob_keys,
            "first contact",
        );

        let outcome = app
            .ingest_message(relay_message("prekey_message", &bob_inbox, message.header_json, message.ciphertext))
            .unwrap();
        let conversation_id = match outcome {
            IngestOutcome::Stored { conversation_id } => conversation_id,
            _ => panic!("expected stored message"),
        };

        let conversations = app
            .storage
            .list_conversations(app.active_profile.as_ref().unwrap().id)
            .unwrap();
        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].1.username.as_deref(), Some("alice"));
        assert_eq!(conversations[0].1.inbox_id.as_deref(), Some(alice_inbox.as_str()));
        assert_eq!(
            conversations[0].1.identity_key.as_deref(),
            Some(identity_json(&alice_keys).unwrap().as_str())
        );
        assert_eq!(app.storage.get_messages(conversation_id).unwrap().len(), 1);
    }

    #[test]
    fn ingest_ratchet_message_with_spoofed_header_fails_without_overwriting_contact_identity() {
        let (alice_keys, alice_inbox) = generate_profile_material(8).unwrap();
        let (bob_keys, bob_inbox) = generate_profile_material(8).unwrap();
        let (mut app, _db) = test_app("alice", &alice_keys, &alice_inbox);
        let identity = identity_json(&bob_keys).unwrap();
        let contact = app
            .storage
            .upsert_contact(
                app.active_profile.as_ref().unwrap().id,
                &UpsertContact {
                    username: Some("bob".to_string()),
                    inbox_id: Some(bob_inbox.clone()),
                    display_name: "bob".to_string(),
                    identity_key: Some(identity.clone()),
                },
            )
            .unwrap();
        let conversation = app
            .storage
            .ensure_conversation(app.active_profile.as_ref().unwrap().id, contact.id)
            .unwrap();
        let alice_out = prepared_prekey_message(
            "alice",
            &alice_inbox,
            &alice_keys,
            "bob",
            &bob_inbox,
            &bob_keys,
            "hello",
        );
        app.storage.set_session(conversation.id, &alice_out.session).unwrap();
        let mut bob_keys_for_receive = bob_keys.clone();
        let bob_in = receive_prekey_message(
            &mut bob_keys_for_receive,
            "bob",
            &bob_inbox,
            &alice_out.header_json,
            &alice_out.ciphertext,
        )
        .unwrap();
        let mut bob_session = bob_in.session;
        bob_session.session_id = bob_in.header.session_id.clone();
        bob_session.peer_username = Some("alice".to_string());
        bob_session.peer_inbox_id = alice_inbox.clone();
        rotate_local_ratchet(&mut bob_session).unwrap();
        let mut bob_reply = prepare_session_message(&bob_session, "bob", &bob_inbox, "reply").unwrap();
        let mut header: MessageHeader = serde_json::from_str(&bob_reply.header_json).unwrap();
        header.sender_username = "mallory".to_string();
        header.sender_inbox_id = "fake-inbox".to_string();
        bob_reply.header_json = serde_json::to_string(&header).unwrap();
        let item = relay_message("ratchet_message", &alice_inbox, bob_reply.header_json, bob_reply.ciphertext);

        let outcome = app.ingest_message(item).unwrap();
        assert!(matches!(outcome, IngestOutcome::Unresolved { reason, .. } if reason == "ratchet decrypt failed"));

        let updated_contact = app.storage.get_contact(contact.id).unwrap();
        assert_eq!(updated_contact.username.as_deref(), Some("bob"));
        assert_eq!(updated_contact.inbox_id.as_deref(), Some(bob_inbox.as_str()));
        assert_eq!(updated_contact.identity_key.as_deref(), Some(identity.as_str()));
    }
}
