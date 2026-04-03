use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::Duration,
};

use tungstenite::{Message, connect};

use crate::state::{LocalProfileRecord, ServerRecord};

pub(super) fn spawn_websocket(
    stop: Arc<AtomicBool>,
    trigger_tx: Sender<()>,
    server: ServerRecord,
    profile: LocalProfileRecord,
) {
    thread::spawn(move || {
        let ws_url = server
            .ws_url
            .clone()
            .unwrap_or_else(|| format!("{}/ws", server.base_url.replace("http", "ws")));
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }

            match connect(format!("{ws_url}?inbox_id={}", profile.inbox_id).as_str()) {
                Ok((mut socket, _)) => {
                    let _ = trigger_tx.send(());
                    while !stop.load(Ordering::Relaxed) {
                        match socket.read() {
                            Ok(Message::Text(_))
                            | Ok(Message::Binary(_))
                            | Ok(Message::Ping(_))
                            | Ok(Message::Pong(_)) => {
                                let _ = trigger_tx.send(());
                            }
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
}
