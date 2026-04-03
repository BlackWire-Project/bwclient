use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, OptionalExtension, params};

use crate::state::{
    ContactRecord, ConversationRecord, LocalProfileRecord, MessageDirection, MessageRecord,
    MessageStatus, ServerRecord, SessionState, StoredProfileKeys,
};

mod contacts;
mod conversations;
mod messages;
mod profiles;
mod schema;
mod servers;

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

    fn map_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalProfileRecord> {
        let keys_json: String = row.get(5)?;
        let keys = serde_json::from_str(&keys_json).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err))
        })?;
        Ok(LocalProfileRecord {
            id: row.get(0)?,
            server_id: row.get(1)?,
            username: row.get(2)?,
            inbox_id: row.get(3)?,
            registered: row.get::<_, i64>(4)? == 1,
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
