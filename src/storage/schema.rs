use super::*;

impl Storage {
    pub(super) fn init(&self) -> Result<()> {
        let connection = self.connect()?;
        connection.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS servers (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL UNIQUE,
                ws_url TEXT
            );

            CREATE TABLE IF NOT EXISTS profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                server_id INTEGER NOT NULL,
                username TEXT NOT NULL,
                inbox_id TEXT NOT NULL,
                registered INTEGER NOT NULL,
                keys_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(server_id, username),
                FOREIGN KEY(server_id) REFERENCES servers(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS contacts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                profile_id INTEGER NOT NULL,
                username TEXT,
                inbox_id TEXT,
                display_name TEXT NOT NULL,
                identity_key TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY(profile_id) REFERENCES profiles(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS conversations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                profile_id INTEGER NOT NULL,
                contact_id INTEGER NOT NULL,
                session_json TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(profile_id, contact_id),
                FOREIGN KEY(profile_id) REFERENCES profiles(id) ON DELETE CASCADE,
                FOREIGN KEY(contact_id) REFERENCES contacts(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                conversation_id INTEGER NOT NULL,
                relay_message_id TEXT UNIQUE,
                client_message_id TEXT NOT NULL,
                direction TEXT NOT NULL,
                body TEXT,
                header TEXT NOT NULL,
                ciphertext TEXT NOT NULL,
                relay_kind TEXT NOT NULL,
                created_at TEXT NOT NULL,
                status TEXT NOT NULL,
                error_reason TEXT,
                FOREIGN KEY(conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            );
            "#,
        )?;
        let _ = connection.execute("ALTER TABLE messages ADD COLUMN error_reason TEXT", []);
        Ok(())
    }
}
