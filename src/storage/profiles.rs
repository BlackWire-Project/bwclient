use super::*;

impl Storage {
    pub fn list_profiles_by_server(&self, server_id: i64) -> Result<Vec<LocalProfileRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            "SELECT id, server_id, username, inbox_id, registered, keys_json
             FROM profiles
             WHERE server_id = ?
             ORDER BY username",
        )?;
        let rows = stmt.query_map([server_id], Self::map_profile)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load profiles")
    }

    pub fn create_profile(
        &self,
        server_id: i64,
        username: &str,
        inbox_id: &str,
        registered: bool,
        keys: &StoredProfileKeys,
    ) -> Result<i64> {
        let connection = self.connect()?;
        let keys_json = serde_json::to_string(keys)?;
        connection.execute(
            "INSERT INTO profiles (server_id, username, inbox_id, registered, keys_json)
             VALUES (?, ?, ?, ?, ?)",
            params![
                server_id,
                username,
                inbox_id,
                i64::from(registered),
                keys_json
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    pub fn get_profile(&self, profile_id: i64) -> Result<LocalProfileRecord> {
        let connection = self.connect()?;
        connection
            .query_row(
                "SELECT id, server_id, username, inbox_id, registered, keys_json
                 FROM profiles
                 WHERE id = ?",
                [profile_id],
                Self::map_profile,
            )
            .context("profile not found")
    }

    pub fn update_profile_keys(&self, profile_id: i64, keys: &StoredProfileKeys) -> Result<()> {
        let connection = self.connect()?;
        let keys_json = serde_json::to_string(keys)?;
        connection.execute(
            "UPDATE profiles SET keys_json = ? WHERE id = ?",
            params![keys_json, profile_id],
        )?;
        Ok(())
    }
}
