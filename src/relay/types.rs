use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub struct RegisterUserRequest {
    pub username: String,
    pub identity_key: String,
    pub signed_prekey: String,
    pub signed_prekey_signature: String,
    pub inbox_id: String,
    pub one_time_prekeys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RegisterUserResponse {
    pub id: String,
    pub username: String,
    pub inbox_id: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RelayBundle {
    pub username: String,
    pub identity_key: String,
    pub signed_prekey: String,
    pub signed_prekey_signature: String,
    pub inbox_id: String,
    pub prekey_id: Option<String>,
    pub one_time_prekey: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AddPrekeysRequest {
    pub one_time_prekeys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AddPrekeysResponse {
    pub count: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PostMessageRequest {
    pub inbox_id: String,
    pub kind: String,
    pub header: String,
    pub ciphertext: String,
    pub used_prekey_id: Option<String>,
    pub client_message_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PostMessageResponse {
    pub id: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RelayMessage {
    pub id: String,
    pub inbox_id: String,
    pub kind: String,
    pub header: String,
    pub ciphertext: String,
    pub used_prekey_id: Option<String>,
    pub client_message_id: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ListMessagesResponse {
    pub items: Vec<RelayMessage>,
    pub next_after_id: Option<String>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WsNotification {
    pub r#type: String,
    pub message_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ErrorEnvelope {
    pub(crate) error: String,
}
