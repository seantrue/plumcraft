//! A live connection to an engine and the thread pumping its events: the core's
//! [`plumcraft_editor_core::link::Link`] (stage N1), woken by an egui repaint.
//!
//! The pump, its inbox and its stop live in the core; this shell only says how to wake it:
//! `ctx.request_repaint()`. [`Link`] derefs to the core's, so `link.conn`, `link.label`,
//! `link.drain()` and `link.state()` read as before, and a windowless caller uses the core's
//! directly and builds no egui.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use plumcraft_editor_core::link as core;
use plumcraft_host::Connection;

pub use plumcraft_editor_core::link::{MAX_WAITING, Waker, state_text};

/// A connection and its event pump, waking an egui context when events arrive. Dropping it
/// stops the pump and waits for it (at most one `events.poll`, 250 ms).
pub struct Link(pub core::Link);

impl Link {
    /// Start pumping `conn`'s events. `repaint` is woken when events arrive.
    pub fn start(conn: Arc<dyn Connection>, label: impl Into<String>, repaint: Option<egui::Context>) -> Result<Link, String> {
        core::Link::start(conn, label, repaint.map(repaint_waker)).map(Link)
    }

    /// The core's link, for a caller that holds it without a window.
    pub fn into_core(self) -> core::Link {
        self.0
    }
}

/// The wake that repaints `ctx`.
pub fn repaint_waker(ctx: egui::Context) -> Waker {
    Waker::new(move || ctx.request_repaint())
}

impl From<core::Link> for Link {
    fn from(link: core::Link) -> Link {
        Link(link)
    }
}

impl Deref for Link {
    type Target = core::Link;
    fn deref(&self) -> &core::Link {
        &self.0
    }
}

impl DerefMut for Link {
    fn deref_mut(&mut self) -> &mut core::Link {
        &mut self.0
    }
}
