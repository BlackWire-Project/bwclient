use super::*;

impl Storage {
    pub fn ensure_conversation(
        &self,
        profile_id: i64,
        contact_id: i64,
    ) -> Result<ConversationRecord> {
        let connection = self.connect()?;
        let existing = connection
            .query_row(
                "SELECT id, profile_id, contact_id, session_json
                 FROM conversations
                 WHERE profile_id = ? AND contact_id = ?",
                params![profile_id, contact_id],
                Self::map_conversation,
            )
            .optional()?;
        if let Some(conversation) = existing {
            return Ok(conversation);
        }

        connection.execute(
            "INSERT INTO conversations (profile_id, contact_id) VALUES (?, ?)",
            params![profile_id, contact_id],
        )?;
        self.get_conversation(connection.last_insert_rowid())
    }

    pub fn get_conversation(&self, conversation_id: i64) -> Result<ConversationRecord> {
        let connection = self.connect()?;
        connection
            .query_row(
                "SELECT id, profile_id, contact_id, session_json
                 FROM conversations
                 WHERE id = ?",
                [conversation_id],
                Self::map_conversation,
            )
            .context("conversation not found")
    }

    pub fn list_conversations(
        &self,
        profile_id: i64,
    ) -> Result<Vec<(ConversationRecord, ContactRecord)>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            "SELECT
                 c.id, c.profile_id, c.contact_id, c.session_json,
                 ct.id, ct.profile_id, ct.username, ct.inbox_id, ct.display_name, ct.identity_key
             FROM conversations c
             JOIN contacts ct ON ct.id = c.contact_id
             WHERE c.profile_id = ?
             ORDER BY ct.display_name",
        )?;
        let rows = stmt.query_map([profile_id], |row| {
            Ok((
                ConversationRecord {
                    id: row.get(0)?,
                    profile_id: row.get(1)?,
                    contact_id: row.get(2)?,
                    session: row
                        .get::<_, Option<String>>(3)?
                        .map(|json| serde_json::from_str(&json))
                        .transpose()
                        .map_err(|err| {
                            rusqlite::Error::FromSqlConversionFailure(
                                3,
                                rusqlite::types::Type::Text,
                                Box::new(err),
                            )
                        })?,
                },
                ContactRecord {
                    id: row.get(4)?,
                    profile_id: row.get(5)?,
                    username: row.get(6)?,
                    inbox_id: row.get(7)?,
                    display_name: row.get(8)?,
                    identity_key: row.get(9)?,
                },
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load conversations")
    }

    pub fn set_session(&self, conversation_id: i64, session: &SessionState) -> Result<()> {
        let connection = self.connect()?;
        let session_json = serde_json::to_string(session)?;
        connection.execute(
            "UPDATE conversations SET session_json = ? WHERE id = ?",
            params![session_json, conversation_id],
        )?;
        Ok(())
    }

    pub fn get_session(&self, conversation_id: i64) -> Result<Option<SessionState>> {
        let connection = self.connect()?;
        let session = connection
            .query_row(
                "SELECT session_json FROM conversations WHERE id = ?",
                [conversation_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        session
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }
}
