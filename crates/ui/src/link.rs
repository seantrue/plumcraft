//! A live connection to an engine and the thread pumping its events.
//!
//! The editor never knows which kind of engine it has: a [`Link`] holds any
//! [`Connection`] (in-process, a separate process, HTTP), long-polls `events.poll` on its own
//! thread (spec 3.4), and hands the events to the UI thread, which drains them each frame and
//! feeds them to the session ([`plumcraft_session::RunState::apply_event`]). The pump wakes the
//! UI (a repaint) when events arrive.
//!
//! Ported from `plumcraft-view`'s `connect()` and pump thread; choosing and starting the engine
//! stays with the app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use plumcraft_host::Connection;
use plumcraft_proto::event::Envelope;
use plumcraft_proto::session::EngineState;
use serde::Deserialize;
use serde_json::json;

/// The most events kept waiting for the UI; older ones are dropped (the reducer orders what
/// arrives, and the run's reply is authoritative).
pub const MAX_WAITING: usize = 20_000;

/// The reply to `events.poll`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Poll {
    events: Vec<Envelope>,
    last_seq: u64,
}

/// A connection and its event pump. Dropping it stops the pump and waits for it (at most one
/// `events.poll`, 250 ms), so the pump's hold on the connection is gone too: a spawned engine
/// is stopped once the last holder drops (`ProcessHost`'s shutdown, `HttpHost`'s lifeline).
pub struct Link {
    pub conn: Arc<dyn Connection>,
    /// How the engine is hosted, for the status bar ("Rust · mode 1 · in-process").
    pub label: String,
    inbox: Arc<Mutex<Vec<Envelope>>>,
    stop: Arc<AtomicBool>,
    pump: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(pump) = self.pump.take() {
            let _ = pump.join();
        }
    }
}

impl Link {
    /// Start pumping `conn`'s events. `repaint` is woken when events arrive.
    pub fn start(conn: Arc<dyn Connection>, label: impl Into<String>, repaint: Option<egui::Context>) -> Result<Link, String> {
        let inbox = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (pump_conn, pump_inbox, pump_stop) = (Arc::clone(&conn), Arc::clone(&inbox), Arc::clone(&stop));
        let pump = std::thread::Builder::new()
            .name("plumcraft-editor-pump".into())
            .spawn(move || {
                let mut since = 0;
                while !pump_stop.load(Ordering::SeqCst) {
                    let Ok(Ok(p)) = pump_conn.call("events.poll", json!({"since": since, "wait_ms": 250})).map(serde_json::from_value::<Poll>) else {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    };
                    since = p.last_seq;
                    if !p.events.is_empty() {
                        let mut inbox = pump_inbox.lock().unwrap_or_else(|e| e.into_inner());
                        inbox.extend(p.events);
                        let excess = inbox.len().saturating_sub(MAX_WAITING);
                        inbox.drain(..excess);
                        drop(inbox);
                        if let Some(ctx) = &repaint {
                            ctx.request_repaint();
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Link { conn, label: label.into(), inbox, stop, pump: Some(pump) })
    }

    /// The events that arrived since the last drain.
    pub fn drain(&self) -> Vec<Envelope> {
        std::mem::take(&mut *self.inbox.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// What the host knows about the engine.
    pub fn state(&self) -> EngineState {
        self.conn.state()
    }
}

/// An engine state in a few words (the status bar).
pub fn state_text(s: &EngineState) -> String {
    match s {
        EngineState::Off => "off".into(),
        EngineState::Connecting { url } => format!("connecting {url}"),
        EngineState::Unreachable { reason, .. } => format!("unreachable: {reason}"),
        EngineState::EngineDown { reason, .. } => format!("down: {reason}"),
        EngineState::LoadingSchema => "loading schema".into(),
        EngineState::Unavailable { reason } => format!("unavailable: {reason}"),
        EngineState::Busy { .. } => "busy".into(),
        EngineState::Connected { nodes, .. } => format!("connected · {nodes} nodes"),
    }
}
