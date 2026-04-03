mod client;
mod types;

pub use client::{RelayClient, ensure_server_health};
pub use types::{
    AddPrekeysRequest, AddPrekeysResponse, ListMessagesResponse, PostMessageRequest,
    PostMessageResponse, RegisterUserRequest, RegisterUserResponse, RelayBundle, RelayMessage,
    WsNotification,
};
