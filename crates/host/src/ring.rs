//! The UI-side event ring for bindings that push: envelopes are re-sequenced on receipt (spec
//! 9.1 allows a binding to assign `seq`), so polling and resume keep working across engine
//! restarts, and synthesised failures join the same stream.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use plumcraft_engine::host::PollReply;
use plumcraft_engine::now_ts;
use plumcraft_proto::event::{Envelope, Event, Resync};

const RING: usize = 4096;
const MAX_WAIT_MS: u64 = 30_000;

#[derive(Default)]
struct Inner {
    seq: u64,
    events: VecDeque<Envelope>,
}

#[derive(Default)]
pub struct Ring {
    inner: Mutex<Inner>,
    cv: Condvar,
}

impl Ring {
    pub fn push(&self, mut e: Envelope) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.seq += 1;
        e.seq = g.seq;
        g.events.push_back(e);
        while g.events.len() > RING {
            g.events.pop_front();
        }
        self.cv.notify_all();
    }

    pub fn push_event(&self, run: Option<(&str, u64)>, event: Event) {
        self.push(Envelope::new(0, now_ts(), run, event));
    }

    pub fn poll(&self, since: u64, wait_ms: u64, run_id: Option<&str>) -> PollReply {
        let deadline = Instant::now() + Duration::from_millis(wait_ms.min(MAX_WAIT_MS));
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let gap = g.events.front().is_some_and(|e| e.seq > since.saturating_add(1));
            let mut events: Vec<Envelope> =
                g.events.iter().filter(|e| e.seq > since && run_id.is_none_or(|r| e.run_id.as_deref() == Some(r))).cloned().collect();
            let left = deadline.saturating_duration_since(Instant::now());
            if !events.is_empty() || gap || left.is_zero() {
                if gap {
                    events.insert(
                        0,
                        Envelope::new(g.seq, now_ts(), None, Event::Resync(Resync { run_id: None, reason: "events older than the ring were dropped".into() })),
                    );
                }
                return PollReply { events, last_seq: g.seq, gap };
            }
            g = self.cv.wait_timeout(g, left).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
        }
    }
}
