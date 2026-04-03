use super::*;

impl App {
    pub(super) fn store_unresolved_message(
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
