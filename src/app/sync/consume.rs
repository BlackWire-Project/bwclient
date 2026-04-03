use super::*;

impl App {
    pub(crate) fn consume_sync_events(&mut self) {
        let active_profile = self.active_profile.as_ref().map(|profile| profile.id);
        for event in self.sync.drain() {
            match event {
                SyncEvent::Messages { profile_id, items } if Some(profile_id) == active_profile => {
                    if let Err(error) = self.ingest_messages(items) {
                        self.show_sync_error_toast(error.to_string(), ToastMode::AutoDismiss);
                    }
                }
                SyncEvent::Status {
                    profile_id,
                    message,
                } if Some(profile_id) == active_profile => {
                    self.status = message;
                }
                SyncEvent::Error {
                    profile_id,
                    message,
                } if Some(profile_id) == active_profile => {
                    self.show_sync_error_toast(message, ToastMode::AutoDismiss);
                }
                _ => {}
            }
        }
    }
}
