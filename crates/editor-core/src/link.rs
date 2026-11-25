//! A live connection to an engine and the thread pumping its events.
//!
//! The editor never knows which kind of engine it has: a [`Link`] holds any
//! [`Connection`] (in-process, a separate process, HTTP), long-polls `events.poll` on its own
//! thread (spec 3.4), and hands the events to whoever drains them (the egui shell each frame, the
//! FFI's core thread), which feeds them to the session
//! ([`plumcraft_session::RunState::apply_event`]). The pump calls a [`Waker`] when events arrive.
//!
//! Ported from `plumcraft-view`'s `connect()` and pump thread; choosing and starting the engine
//! stays with the app. Moved out of `plumcraft-ui` (stage N1), where the wake was
//! `egui::Context::request_repaint`; the egui shell now passes a waker that calls it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use plumcraft_host::Connection;
use plumcraft_proto::event::Envelope;
use plumcraft_proto::session::EngineState;
use serde::Deserialize;
use serde_json::json;

/// A wake with no payload, callable from any thread: the pump's "events are waiting".
///
/// A shared closure rather than a trait: every shell has one already (egui's
/// `Context::request_repaint`, a C function and its context behind the FFI, a channel send), so a
/// closure adapts each in one line with nothing to implement, and the `Arc` makes it cheap to
/// hand to several pumps. It carries no data, so nothing crosses on the pump's thread but the
/// call (`docs/research/24-native-shells.md` 4.2: "one wake, no payload").
#[derive(Clone)]
pub struct Waker(Arc<dyn Fn() + Send + Sync>);

impl Waker {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Waker {
        Waker(Arc::new(wake))
    }

    /// Call it. It should only note that something is waiting and return.
    pub fn wake(&self) {
        (self.0)()
    }
}

impl std::fmt::Debug for Waker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Waker")
    }
}

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
    /// Start pumping `conn`'s events. `waker` is called when events arrive.
    pub fn start(conn: Arc<dyn Connection>, label: impl Into<String>, waker: Option<Waker>) -> Result<Link, String> {
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
                        if let Some(w) = &waker {
                            w.wake();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    use plumcraft_proto::data::DataRef;
    use plumcraft_proto::error::ProtoError;
    use plumcraft_proto::event::Event;
    use plumcraft_store::Blob;
    use serde_json::Value;

    /// An engine with two events waiting, then none.
    struct TwoEvents(AtomicBool);

    impl Connection for TwoEvents {
        fn call(&self, method: &str, _: Value) -> Result<Value, ProtoError> {
            assert_eq!(method, "events.poll");
            if self.0.swap(true, Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(20));
                return Ok(json!({"events": [], "last_seq": 2}));
            }
            let events = [1, 2].map(|seq| Envelope::new(seq, 0.0, None, Event::SchemaChanged { schema_digest: format!("d{seq}") }));
            Ok(json!({"events": events, "last_seq": 2}))
        }
        fn state(&self) -> EngineState {
            EngineState::Off
        }
        fn resolve(&self, _: &DataRef) -> Option<Blob> {
            None
        }
    }

    #[test]
    fn events_are_held_until_drained_and_wake_the_shell() {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let link = Link::start(
            Arc::new(TwoEvents(AtomicBool::new(false))),
            "fake",
            Some(Waker::new(move || {
                w.fetch_add(1, Ordering::SeqCst);
            })),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while woken.load(Ordering::SeqCst) == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(woken.load(Ordering::SeqCst), 1, "one wake for one batch");
        assert_eq!(link.drain().iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2]);
        assert!(link.drain().is_empty(), "drained once");
        assert_eq!(link.state(), EngineState::Off);
        assert_eq!(link.label, "fake");
        assert_eq!(format!("{:?}", Waker::new(|| {})), "Waker");
        // no waker is fine too, and dropping the link stops its pump
        drop(Link::start(Arc::new(TwoEvents(AtomicBool::new(false))), "quiet", None).unwrap());
    }
}
