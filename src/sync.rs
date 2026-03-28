use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use serde_json::from_str;
use tungstenite::{Message, connect};

use crate::{
    relay::{RelayClient, RelayMessage, WsNotification},
    state::{LocalProfileRecord, ServerRecord},
};

#[derive(Clone, Debug)]
pub enum SyncEvent {
    Message { profile_id: i64, item: RelayMessage },
    Status { profile_id: i64, message: String },
    Error { profile_id: i64, message: String },
}

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
        let events_tx = self.events_tx.clone();
        let poll_stop = stop.clone();
        let ws_stop = stop.clone();
        let profile_for_poll = profile.clone();
        let server_for_poll = server.clone();
        let profile_for_ws = profile.clone();
        let server_for_ws = server.clone();
        let trigger_tx_for_ws = trigger_tx.clone();
        let events_tx_for_poll = events_tx.clone();
        let events_tx_for_ws = events_tx.clone();

        thread::spawn(move || {
            let client = match RelayClient::new(server_for_poll.base_url.clone()) {
                Ok(client) => client,
                Err(error) => {
                    let _ = events_tx_for_poll.send(SyncEvent::Error {
                        profile_id: profile_for_poll.id,
                        message: error.to_string(),
                    });
                    return;
                }
            };

            let _ = events_tx_for_poll.send(SyncEvent::Status {
                profile_id: profile_for_poll.id,
                message: "sync worker online".to_string(),
            });

            let _ = trigger_rx.recv_timeout(Duration::from_millis(10));
            let mut after_id = profile_for_poll.last_synced_relay_message_id.clone();
            loop {
                if poll_stop.load(Ordering::Relaxed) {
                    break;
                }

                match client.list_messages(&profile_for_poll.inbox_id, 100, after_id.as_deref()) {
                    Ok(mut response) => loop {
                        for item in response.items {
                            after_id = Some(item.id.clone());
                            let _ = events_tx_for_poll.send(SyncEvent::Message {
                                profile_id: profile_for_poll.id,
                                item,
                            });
                        }

                        if !response.has_more {
                            break;
                        }

                        response = match client.list_messages(
                            &profile_for_poll.inbox_id,
                            100,
                            response.next_after_id.as_deref(),
                        ) {
                            Ok(response) => response,
                            Err(error) => {
                                let _ = events_tx_for_poll.send(SyncEvent::Error {
                                    profile_id: profile_for_poll.id,
                                    message: format!("poll failed: {error}"),
                                });
                                break;
                            }
                        };
                    },
                    Err(error) => {
                        let _ = events_tx_for_poll.send(SyncEvent::Error {
                            profile_id: profile_for_poll.id,
                            message: format!("poll failed: {error}"),
                        });
                    }
                }

                match trigger_rx.recv_timeout(Duration::from_secs(15)) {
                    Ok(_) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        thread::spawn(move || {
            let ws_url = server_for_ws
                .ws_url
                .clone()
                .unwrap_or_else(|| format!("{}/ws", server_for_ws.base_url.replace("http", "ws")));
            loop {
                if ws_stop.load(Ordering::Relaxed) {
                    break;
                }

                match connect(format!("{ws_url}?inbox_id={}", profile_for_ws.inbox_id).as_str()) {
                    Ok((mut socket, _)) => {
                        while !ws_stop.load(Ordering::Relaxed) {
                            match socket.read() {
                                Ok(Message::Text(text)) => {
                                    match from_str::<WsNotification>(&text) {
                                        Ok(notification)
                                            if notification.r#type == "messages_available" =>
                                        {
                                            if let Some(message_id) = notification.message_id {
                                                match RelayClient::new(
                                                    server_for_ws.base_url.clone(),
                                                )
                                                .and_then(|client| {
                                                    client.get_message(
                                                        &profile_for_ws.inbox_id,
                                                        &message_id,
                                                    )
                                                }) {
                                                    Ok(item) => {
                                                        let _ = events_tx_for_ws.send(
                                                            SyncEvent::Message {
                                                                profile_id: profile_for_ws.id,
                                                                item,
                                                            },
                                                        );
                                                    }
                                                    Err(_) => {
                                                        let _ = trigger_tx_for_ws.send(());
                                                    }
                                                }
                                            } else {
                                                let _ = trigger_tx_for_ws.send(());
                                            }
                                        }
                                        _ => {
                                            let _ = trigger_tx_for_ws.send(());
                                        }
                                    }
                                }
                                Ok(Message::Binary(_))
                                | Ok(Message::Ping(_))
                                | Ok(Message::Pong(_)) => {}
                                Ok(Message::Close(_)) => break,
                                Ok(Message::Frame(_)) => {}
                                Err(_) => break,
                            }
                        }
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        });

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
