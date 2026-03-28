use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};

use crate::state::{
    ContactRecord, ConversationRecord, LocalProfileRecord, MessageDirection, MessageRecord,
    MessageStatus, ServerRecord, SessionState, StoredProfileKeys,
};

pub struct Storage {
    path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct UpsertContact {
    pub username: Option<String>,
    pub inbox_id: Option<String>,
    pub display_name: String,
    pub identity_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NewMessage {
    pub relay_message_id: Option<String>,
    pub client_message_id: String,
    pub direction: MessageDirection,
    pub body: Option<String>,
    pub header: String,
    pub ciphertext: String,
    pub relay_kind: String,
    pub created_at: String,
    pub status: MessageStatus,
    pub error_reason: Option<String>,
}

impl Storage {
    pub fn open_default() -> Result<Self> {
        let path = std::env::current_dir()
            .context("failed to resolve current directory")?
            .join("bwclient.db");
        Self::open(path)
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let storage = Self { path: path.into() };
        storage.init()?;
        Ok(storage)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connect(&self) -> Result<Connection> {
        Connection::open(&self.path).context("failed to open sqlite database")
    }

    fn init(&self) -> Result<()> {
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
                 last_synced_relay_message_id TEXT,
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
        let _ = connection.execute(
            "ALTER TABLE profiles ADD COLUMN last_synced_relay_message_id TEXT",
            [],
        );
        let _ = connection.execute("ALTER TABLE messages ADD COLUMN error_reason TEXT", []);
        Ok(())
    }

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

    pub fn list_profiles_by_server(&self, server_id: i64) -> Result<Vec<LocalProfileRecord>> {
        let connection = self.connect()?;
        let mut stmt = connection.prepare(
            "SELECT id, server_id, username, inbox_id, registered, last_synced_relay_message_id, keys_json
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
            "INSERT INTO profiles (server_id, username, inbox_id, registered, last_synced_relay_message_id, keys_json)
             VALUES (?, ?, ?, ?, NULL, ?)",
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
                "SELECT id, server_id, username, inbox_id, registered, last_synced_relay_message_id, keys_json
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

    pub fn update_profile_sync_cursor(
        &self,
        profile_id: i64,
        relay_message_id: Option<&str>,
    ) -> Result<()> {
        let connection = self.connect()?;
        connection.execute(
            "UPDATE profiles SET last_synced_relay_message_id = ? WHERE id = ?",
            params![relay_message_id, profile_id],
        )?;
        Ok(())
    }

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

    fn map_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalProfileRecord> {
        let keys_json: String = row.get(6)?;
        let keys = serde_json::from_str(&keys_json).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(err))
        })?;
        Ok(LocalProfileRecord {
            id: row.get(0)?,
            server_id: row.get(1)?,
            username: row.get(2)?,
            inbox_id: row.get(3)?,
            registered: row.get::<_, i64>(4)? == 1,
            last_synced_relay_message_id: row.get(5)?,
            keys,
        })
    }

    fn map_contact(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContactRecord> {
        Ok(ContactRecord {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            username: row.get(2)?,
            inbox_id: row.get(3)?,
            display_name: row.get(4)?,
            identity_key: row.get(5)?,
        })
    }

    fn map_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationRecord> {
        let session = row
            .get::<_, Option<String>>(3)?
            .map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
        Ok(ConversationRecord {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            contact_id: row.get(2)?,
            session,
        })
    }

    fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
        Ok(MessageRecord {
            id: row.get(0)?,
            conversation_id: row.get(1)?,
            relay_message_id: row.get(2)?,
            client_message_id: row.get(3)?,
            direction: MessageDirection::from_db(&row.get::<_, String>(4)?),
            body: row.get(5)?,
            header: row.get(6)?,
            ciphertext: row.get(7)?,
            relay_kind: row.get(8)?,
            created_at: row.get(9)?,
            status: MessageStatus::from_db(&row.get::<_, String>(10)?),
            error_reason: row.get(11)?,
        })
    }

    pub fn default_ws_url(base_url: &str) -> Result<String> {
        if let Some(rest) = base_url.strip_prefix("https://") {
            return Ok(format!("wss://{rest}/ws"));
        }
        if let Some(rest) = base_url.strip_prefix("http://") {
            return Ok(format!("ws://{rest}/ws"));
        }
        Err(anyhow!("base_url must start with http:// or https://"))
    }
}
