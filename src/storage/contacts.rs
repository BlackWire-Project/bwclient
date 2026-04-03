use super::*;

impl Storage {
    pub fn upsert_contact(&self, profile_id: i64, input: &UpsertContact) -> Result<ContactRecord> {
        let connection = self.connect()?;

        let mut existing = None;
        if let Some(username) = &input.username {
            existing = connection
                .query_row(
                    "SELECT id, profile_id, username, inbox_id, display_name, identity_key
                     FROM contacts
                     WHERE profile_id = ? AND username = ?",
                    params![profile_id, username],
                    Self::map_contact,
                )
                .optional()?;
        }
        if existing.is_none() {
            if let Some(inbox_id) = &input.inbox_id {
                existing = connection
                    .query_row(
                        "SELECT id, profile_id, username, inbox_id, display_name, identity_key
                         FROM contacts
                         WHERE profile_id = ? AND inbox_id = ?",
                        params![profile_id, inbox_id],
                        Self::map_contact,
                    )
                    .optional()?;
            }
        }

        if let Some(contact) = existing {
            connection.execute(
                "UPDATE contacts
                 SET username = COALESCE(?, username),
                     inbox_id = COALESCE(?, inbox_id),
                     display_name = ?,
                     identity_key = COALESCE(?, identity_key)
                 WHERE id = ?",
                params![
                    input.username,
                    input.inbox_id,
                    input.display_name,
                    input.identity_key,
                    contact.id
                ],
            )?;
            return self.get_contact(contact.id);
        }

        connection.execute(
            "INSERT INTO contacts (profile_id, username, inbox_id, display_name, identity_key)
             VALUES (?, ?, ?, ?, ?)",
            params![
                profile_id,
                input.username,
                input.inbox_id,
                input.display_name,
                input.identity_key
            ],
        )?;
        self.get_contact(connection.last_insert_rowid())
    }

    pub fn list_contacts(&self, profile_id: i64) -> Result<Vec<ContactRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            "SELECT id, profile_id, username, inbox_id, display_name, identity_key
             FROM contacts
             WHERE profile_id = ?
             ORDER BY display_name",
        )?;
        let rows = stmt.query_map([profile_id], Self::map_contact)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load contacts")
    }

    pub fn get_contact(&self, contact_id: i64) -> Result<ContactRecord> {
        let connection = self.connect()?;
        connection
            .query_row(
                "SELECT id, profile_id, username, inbox_id, display_name, identity_key
                 FROM contacts
                 WHERE id = ?",
                [contact_id],
                Self::map_contact,
            )
            .context("contact not found")
    }
}
