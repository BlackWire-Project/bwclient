use serde::{Deserialize, Serialize};

use crate::state::SessionState;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityBundlePublic {
    pub sign: String,
    pub dh: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedPayload {
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageHeader {
    pub version: u8,
    pub sender_username: String,
    pub sender_inbox_id: String,
    pub message_type: String,
    pub session_id: String,
    pub sender_identity_sign: String,
    pub sender_identity_dh: String,
    pub dh_pub: String,
    pub recipient_signed_prekey: Option<String>,
    pub used_one_time_prekey: Option<String>,
    pub pn: u32,
    pub n: u32,
    pub timestamp: String,
}

#[derive(Clone, Debug)]
pub struct RemoteBundleKeys {
    pub username: String,
    pub inbox_id: String,
    pub identity_sign_public: String,
    pub identity_dh_public: String,
    pub signed_prekey_public: String,
    pub one_time_prekey_public: Option<String>,
    pub prekey_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PreparedMessage {
    pub relay_kind: String,
    pub relay_inbox_id: String,
    pub header_json: String,
    pub ciphertext: String,
    pub client_message_id: String,
    pub used_prekey_id: Option<String>,
    pub plaintext: String,
    pub session: SessionState,
}

#[derive(Clone, Debug)]
pub struct DecryptedIncoming {
    pub header: MessageHeader,
    pub plaintext: String,
    pub session: SessionState,
    pub sender_identity_json: String,
}
