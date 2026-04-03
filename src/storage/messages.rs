use super::*;

impl Storage {
    pub fn get_messages(&self, conversation_id: i64) -> Result<Vec<MessageRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            "SELECT id, conversation_id, relay_message_id, client_message_id, direction, body,
                    header, ciphertext, relay_kind, created_at, status, error_reason
             FROM messages
             WHERE conversation_id = ?
             ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([conversation_id], Self::map_message)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load messages")
    }

    pub fn insert_message(&self, conversation_id: i64, message: &NewMessage) -> Result<i64> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO messages (
                 conversation_id, relay_message_id, client_message_id, direction, body,
                 header, ciphertext, relay_kind, created_at, status, error_reason
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                conversation_id,
                message.relay_message_id,
                message.client_message_id,
                message.direction.as_str(),
                message.body,
                message.header,
                message.ciphertext,
                message.relay_kind,
                message.created_at,
                message.status.as_str(),
                message.error_reason,
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    pub fn update_message_status(&self, message_id: i64, status: MessageStatus) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            "UPDATE messages SET status = ? WHERE id = ?",
            params![status.as_str(), message_id],
        )?;
        Ok(())
    }

    pub fn has_relay_message(&self, relay_message_id: &str) -> Result<bool> {
        let connection = self.connect()?;
        let exists = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM messages WHERE relay_message_id = ?)",
            [relay_message_id],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(exists == 1)
    }
}
