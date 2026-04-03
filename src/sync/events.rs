use crate::relay::RelayMessage;

#[derive(Clone, Debug)]
pub enum SyncEvent {
    Messages {
        profile_id: i64,
        items: Vec<RelayMessage>,
    },
    Status { profile_id: i64, message: String },
    Error { profile_id: i64, message: String },
}
