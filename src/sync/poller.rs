use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
    },
    thread,
    time::Duration,
};

use crate::{
    relay::RelayClient,
    state::{LocalProfileRecord, ServerRecord},
    sync::SyncEvent,
};

pub(super) fn spawn_poller(
    stop: Arc<AtomicBool>,
    trigger_rx: Receiver<()>,
    events_tx: Sender<SyncEvent>,
    server: ServerRecord,
    profile: LocalProfileRecord,
) {
    thread::spawn(move || {
        let client = match RelayClient::new(server.base_url.clone()) {
            Ok(client) => client,
            Err(error) => {
                let _ = events_tx.send(SyncEvent::Error {
                    profile_id: profile.id,
                    message: error.to_string(),
                });
                return;
            }
        };

        let _ = events_tx.send(SyncEvent::Status {
            profile_id: profile.id,
            message: "sync worker online".to_string(),
        });

        let _ = trigger_rx.recv_timeout(Duration::from_millis(10));
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }

            match client.list_messages(&profile.inbox_id, 200, None) {
                Ok(response) => {
                    let _ = events_tx.send(SyncEvent::Messages {
                        profile_id: profile.id,
                        items: response.items,
                    });
                }
                Err(error) => {
                    let _ = events_tx.send(SyncEvent::Error {
                        profile_id: profile.id,
                        message: format!("poll failed: {error}"),
                    });
                }
            }

            match trigger_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(_) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}
