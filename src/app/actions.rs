use super::*;

impl App {
    pub(crate) fn submit_add_server(&mut self, form: &FormState) -> Result<()> {
        let name = form.fields[0].value.trim();
        let base_url = form.fields[1].value.trim().trim_end_matches('/');
        if name.is_empty() || base_url.is_empty() {
            return Err(anyhow!("server name and base_url are required"));
        }
        ensure_server_health(base_url)?;
        let ws_url = Storage::default_ws_url(base_url)?;
        self.storage.create_server(name, base_url, Some(&ws_url))?;
        self.refresh_login_data()?;
        self.status = format!("Server '{name}' added.");
        self.screen = Screen::Login;
        Ok(())
    }

    pub(crate) fn submit_add_profile(&mut self, form: &FormState) -> Result<()> {
        let username = form.fields[0].value.trim();
        if username.is_empty() {
            return Err(anyhow!("username is required"));
        }
        let server = self
            .selected_server_record()
            .cloned()
            .ok_or_else(|| anyhow!("select a server first"))?;
        let relay = RelayClient::new(server.base_url.clone())?;
        let (keys, inbox_id) = generate_profile_material(16)?;
        relay.register_user(&RegisterUserRequest {
            username: username.to_string(),
            identity_key: identity_json(&keys)?,
            signed_prekey: keys.signed_prekey_public.clone(),
            signed_prekey_signature: keys.signed_prekey_signature.clone(),
            inbox_id: inbox_id.clone(),
            one_time_prekeys: keys
                .one_time_prekeys
                .iter()
                .map(|prekey| prekey.public_key.clone())
                .collect(),
        })?;
        self.storage
            .create_profile(server.id, username, &inbox_id, true, &keys)?;
        self.refresh_profiles()?;
        self.status = format!("Profile '{username}' registered on {}.", server.name);
        self.screen = Screen::Login;
        Ok(())
    }

    pub(crate) fn submit_add_contact(&mut self, form: &ContactFormState) -> Result<()> {
        let profile = self
            .active_profile
            .clone()
            .ok_or_else(|| anyhow!("no active profile"))?;
        let server = self
            .active_server
            .clone()
            .ok_or_else(|| anyhow!("no active server"))?;
        let relay = RelayClient::new(server.base_url.clone())?;
        let display_name = if form.display_name.trim().is_empty() {
            if form.use_username {
                form.value.trim().to_string()
            } else {
                super::view::shorten(form.value.trim(), 18)
            }
        } else {
            form.display_name.trim().to_string()
        };
        if form.value.trim().is_empty() {
            return Err(anyhow!("username or inbox_id is required"));
        }

        let contact = if form.use_username {
            let bundle = relay.get_bundle(form.value.trim())?;
            self.storage.upsert_contact(
                profile.id,
                &UpsertContact {
                    username: Some(bundle.username.clone()),
                    inbox_id: Some(bundle.inbox_id.clone()),
                    display_name,
                    identity_key: Some(bundle.identity_key.clone()),
                },
            )?
        } else {
            self.storage.upsert_contact(
                profile.id,
                &UpsertContact {
                    username: None,
                    inbox_id: Some(form.value.trim().to_string()),
                    display_name,
                    identity_key: None,
                },
            )?
        };
        self.storage.ensure_conversation(profile.id, contact.id)?;
        self.reload_conversations()?;
        self.status = format!("Contact '{}' added.", contact.display_name);
        self.screen = Screen::Main;
        Ok(())
    }

    pub(crate) fn activate_profile(&mut self, profile_id: i64) -> Result<()> {
        let profile = self.storage.get_profile(profile_id)?;
        let server = self.storage.get_server(profile.server_id)?;
        self.active_profile = Some(profile.clone());
        self.active_server = Some(server.clone());
        self.screen = Screen::Main;
        self.input_mode = InputMode::Command;
        self.composer.clear();
        self.reload_conversations()?;
        self.replenish_prekeys_if_needed()?;
        self.sync.start(server, profile);
        self.sync.trigger();
        self.status = "Profile activated.".to_string();
        Ok(())
    }

    pub(crate) fn refresh_login_data(&mut self) -> Result<()> {
        self.servers = self.storage.list_servers()?;
        if self.servers.is_empty() {
            self.selected_server = 0;
            self.profiles.clear();
            self.selected_profile = 0;
            return Ok(());
        }
        self.selected_server = self
            .selected_server
            .min(self.servers.len().saturating_sub(1));
        self.refresh_profiles()
    }

    pub(crate) fn refresh_profiles(&mut self) -> Result<()> {
        if let Some(server) = self.selected_server_record() {
            self.profiles = self.storage.list_profiles_by_server(server.id)?;
            self.selected_profile = self
                .selected_profile
                .min(self.profiles.len().saturating_sub(1));
        } else {
            self.profiles.clear();
            self.selected_profile = 0;
        }
        Ok(())
    }

    pub(crate) fn reload_conversations(&mut self) -> Result<()> {
        let profile = self
            .active_profile
            .clone()
            .ok_or_else(|| anyhow!("no active profile"))?;
        self.conversations = self.storage.list_conversations(profile.id)?;
        self.selected_conversation = self
            .selected_conversation
            .min(self.conversations.len().saturating_sub(1));
        self.reload_messages()
    }

    pub(crate) fn reload_messages(&mut self) -> Result<()> {
        let was_auto_follow = self.message_auto_follow;
        self.messages.clear();
        if let Some(conversation_id) = self
            .current_conversation()
            .map(|(conversation, _)| conversation.id)
        {
            self.unread_conversations.remove(&conversation_id);
            self.messages = self.storage.get_messages(conversation_id)?;
        }
        if was_auto_follow {
            self.message_scroll = u16::MAX;
        }
        self.normalize_message_scroll();
        Ok(())
    }

    pub(crate) fn send_current_message(&mut self) -> Result<()> {
        let plaintext = self.composer.trim().to_string();
        if plaintext.is_empty() {
            return Ok(());
        }
        let previous_composer = std::mem::take(&mut self.composer);
        let profile = self
            .active_profile
            .clone()
            .ok_or_else(|| anyhow!("no active profile"))?;
        let server = self
            .active_server
            .clone()
            .ok_or_else(|| anyhow!("no active server"))?;
        let (conversation, contact) = self
            .current_conversation()
            .cloned()
            .ok_or_else(|| anyhow!("select a conversation"))?;
        let relay = RelayClient::new(server.base_url.clone())?;

        let send_result: Result<PostMessageResponse> = (|| {
            let prepared = if let Some(session) = conversation.session.clone() {
                let mut session = session;
                if session.pending_send_ratchet {
                    rotate_local_ratchet(&mut session)?;
                }
                prepare_session_message(&session, &profile.username, &profile.inbox_id, &plaintext)?
            } else {
                let username = contact.username.clone().ok_or_else(|| {
                    anyhow!("this contact has no username; bootstrap requires bundle discovery")
                })?;
                let bundle = relay.get_bundle(&username)?;
                let prepared = prepare_initial_message(
                    &profile.username,
                    &profile.inbox_id,
                    &profile.keys,
                    &bundle,
                    &plaintext,
                )?;
                let identity = parse_identity_json(&bundle.identity_key)?;
                self.storage.upsert_contact(
                    profile.id,
                    &UpsertContact {
                        username: Some(bundle.username.clone()),
                        inbox_id: Some(bundle.inbox_id.clone()),
                        display_name: contact.display_name.clone(),
                        identity_key: Some(serde_json::to_string(&identity)?),
                    },
                )?;
                prepared
            };

            let response = relay.post_message(&PostMessageRequest {
                inbox_id: prepared.relay_inbox_id.clone(),
                kind: prepared.relay_kind.clone(),
                header: prepared.header_json.clone(),
                ciphertext: prepared.ciphertext.clone(),
                used_prekey_id: prepared.used_prekey_id.clone(),
                client_message_id: prepared.client_message_id.clone(),
            })?;

            self.storage
                .set_session(conversation.id, &prepared.session)?;
            self.storage.insert_message(
                conversation.id,
                &NewMessage {
                    relay_message_id: Some(response.id.clone()),
                    client_message_id: prepared.client_message_id,
                    direction: MessageDirection::Outgoing,
                    body: Some(plaintext.clone()),
                    header: prepared.header_json,
                    ciphertext: prepared.ciphertext,
                    relay_kind: prepared.relay_kind,
                    created_at: response.created_at.clone(),
                    status: MessageStatus::Sent,
                    error_reason: None,
                },
            )?;

            Ok(response)
        })();

        match send_result {
            Ok(response) => {
                self.reload_conversations()?;
                self.sync.trigger();
                self.status = format!("Message sent. Expires at {}.", response.expires_at);
                Ok(())
            }
            Err(error) => {
                self.composer = previous_composer;
                Err(error)
            }
        }
    }
}
