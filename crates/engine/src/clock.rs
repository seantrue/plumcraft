//! The clock a node's own work is measured on (spec 14.5: whether a result is worth persisting
//! is weighed against that work). An engine uses the real one; a test gives a virtual one, on
//! which a node's work is exactly the time it sleeps, so what it decides doesn't depend on how
//! fast the machine is (B37).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Now and sleep, for measuring a node's work.
pub trait Clock: Send + Sync {
    /// Time since some fixed moment of this clock's.
    fn now(&self) -> Duration;
    /// Wait `d` on this clock.
    fn sleep(&self, d: Duration);
}

/// The machine's monotonic clock.
#[derive(Debug, Clone, Copy)]
pub struct RealClock {
    epoch: Instant,
}

impl Default for RealClock {
    fn default() -> Self {
        RealClock { epoch: Instant::now() }
    }
}

impl Clock for RealClock {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// A clock that moves only when something sleeps on it (or [`VirtualClock::advance`]): what a
/// test measures work on.
#[derive(Debug, Default)]
pub struct VirtualClock {
    now: Mutex<Duration>,
}

impl VirtualClock {
    pub fn new() -> Arc<VirtualClock> {
        Arc::new(VirtualClock::default())
    }

    /// Move it on by `d`, sleeping no real time.
    pub fn advance(&self, d: Duration) {
        let mut now = self.now.lock().unwrap_or_else(|e| e.into_inner());
        *now = now.saturating_add(d);
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> Duration {
        *self.now.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sleep(&self, d: Duration) {
        self.advance(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_virtual_clock_moves_only_when_slept_on() {
        let c = VirtualClock::new();
        assert_eq!(c.now(), Duration::ZERO);
        c.sleep(Duration::from_millis(60));
        c.advance(Duration::from_millis(1));
        assert_eq!(c.now(), Duration::from_millis(61));
        let r = RealClock::default();
        let a = r.now();
        r.sleep(Duration::from_millis(2));
        assert!(r.now() >= a + Duration::from_millis(2));
    }
}
