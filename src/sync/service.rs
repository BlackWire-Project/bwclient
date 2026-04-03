use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
};

use crate::state::{LocalProfileRecord, ServerRecord};

use super::{events::SyncEvent, poller::spawn_poller, websocket::spawn_websocket};

pub struct SyncService {
    events_rx: Receiver<SyncEvent>,
    events_tx: Sender<SyncEvent>,
    current: Option<SyncHandle>,
}

struct SyncHandle {
    profile_id: i64,
    stop: Arc<AtomicBool>,
    trigger_tx: Sender<()>,
}

impl SyncService {
    pub fn new() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            events_rx,
            events_tx,
            current: None,
        }
    }

    pub fn start(&mut self, server: ServerRecord, profile: LocalProfileRecord) {
        self.stop();

        let stop = Arc::new(AtomicBool::new(false));
        let (trigger_tx, trigger_rx) = mpsc::channel();
        spawn_poller(
            stop.clone(),
            trigger_rx,
            self.events_tx.clone(),
            server.clone(),
            profile.clone(),
        );
        spawn_websocket(
            stop.clone(),
            trigger_tx.clone(),
            server,
            profile.clone(),
        );

        self.current = Some(SyncHandle {
            profile_id: profile.id,
            stop,
            trigger_tx,
        });
    }

    pub fn trigger(&self) {
        if let Some(handle) = &self.current {
            let _ = handle.trigger_tx.send(());
        }
    }

    pub fn active_profile_id(&self) -> Option<i64> {
        self.current.as_ref().map(|handle| handle.profile_id)
    }

    pub fn stop(&mut self) {
        if let Some(handle) = self.current.take() {
            handle.stop.store(true, Ordering::Relaxed);
            let _ = handle.trigger_tx.send(());
        }
    }

    pub fn drain(&self) -> Vec<SyncEvent> {
        let mut items = Vec::new();
        while let Ok(item) = self.events_rx.try_recv() {
            items.push(item);
        }
        items
    }
}
