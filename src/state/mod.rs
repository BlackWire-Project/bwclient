use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct ServerRecord {
    pub id: i64,
    pub name: String,
    pub base_url: String,
    pub ws_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LocalProfileRecord {
    pub id: i64,
    pub server_id: i64,
    pub username: String,
    pub inbox_id: String,
    pub registered: bool,
    pub last_synced_relay_message_id: Option<String>,
    pub keys: StoredProfileKeys,
}

#[derive(Clone, Debug)]
pub struct ContactRecord {
    pub id: i64,
    pub profile_id: i64,
    pub username: Option<String>,
    pub inbox_id: Option<String>,
    pub display_name: String,
    pub identity_key: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ConversationRecord {
    pub id: i64,
    pub profile_id: i64,
    pub contact_id: i64,
    pub session: Option<SessionState>,
}

#[derive(Clone, Debug)]
pub struct MessageRecord {
    pub id: i64,
    pub conversation_id: i64,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredProfileKeys {
    pub identity_sign_secret: String,
    pub identity_sign_public: String,
    pub identity_dh_secret: String,
    pub identity_dh_public: String,
    pub signed_prekey_secret: String,
    pub signed_prekey_public: String,
    pub signed_prekey_signature: String,
    pub one_time_prekeys: Vec<StoredOneTimePrekey>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredOneTimePrekey {
    pub public_key: String,
    pub private_key: String,
    pub uploaded: bool,
    pub consumed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionState {
    pub version: u8,
    pub session_id: String,
    pub peer_username: Option<String>,
    pub peer_inbox_id: String,
    pub peer_identity_sign_public: String,
    pub peer_identity_dh_public: String,
    pub root_key: String,
    pub sending_chain_key: Option<String>,
    pub receiving_chain_key: Option<String>,
    pub local_ratchet_private: String,
    pub local_ratchet_public: String,
    pub remote_ratchet_public: Option<String>,
    pub send_count: u32,
    pub receive_count: u32,
    pub previous_send_count: u32,
    pub pending_send_ratchet: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageDirection {
    Incoming,
    Outgoing,
}

impl MessageDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incoming => "incoming",
            Self::Outgoing => "outgoing",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "outgoing" => Self::Outgoing,
            _ => Self::Incoming,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageStatus {
    LocalOnly,
    Sending,
    Sent,
    Received,
    Failed,
    RawUnresolved,
    DecryptFailed,
    IdentityMismatch,
    SessionMissing,
    UnsupportedHeader,
}

impl MessageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalOnly => "local_only",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Received => "received",
            Self::Failed => "failed",
            Self::RawUnresolved => "raw_unresolved",
            Self::DecryptFailed => "decrypt_failed",
            Self::IdentityMismatch => "identity_mismatch",
            Self::SessionMissing => "session_missing",
            Self::UnsupportedHeader => "unsupported_header",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "sending" => Self::Sending,
            "sent" => Self::Sent,
            "failed" => Self::Failed,
            "local_only" => Self::LocalOnly,
            "raw_unresolved" => Self::RawUnresolved,
            "decrypt_failed" => Self::DecryptFailed,
            "identity_mismatch" => Self::IdentityMismatch,
            "session_missing" => Self::SessionMissing,
            "unsupported_header" => Self::UnsupportedHeader,
            _ => Self::Received,
        }
    }
}
