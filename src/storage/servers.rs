use super::*;

impl Storage {
    pub fn list_servers(&self) -> Result<Vec<ServerRecord>> {
        let connection = self.connect()?;
        let mut stmt =
            connection.prepare("SELECT id, name, base_url, ws_url FROM servers ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            Ok(ServerRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                base_url: row.get(2)?,
                ws_url: row.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to load servers")
    }

    pub fn create_server(&self, name: &str, base_url: &str, ws_url: Option<&str>) -> Result<i64> {
        let connection = self.connect()?;
        connection.execute(
            "INSERT INTO servers (name, base_url, ws_url) VALUES (?, ?, ?)",
            params![name, base_url, ws_url],
        )?;
        Ok(connection.last_insert_rowid())
    }

    pub fn get_server(&self, server_id: i64) -> Result<ServerRecord> {
        let connection = self.connect()?;
        connection
            .query_row(
                "SELECT id, name, base_url, ws_url FROM servers WHERE id = ?",
                [server_id],
                |row| {
                    Ok(ServerRecord {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        base_url: row.get(2)?,
                        ws_url: row.get(3)?,
                    })
                },
            )
            .context("server not found")
    }
}
