//! The UI ↔ engine interface, protocol 2: the types every binding carries (in-process, AMQP,
//! socket JSON-RPC, HTTP), as `docs/interface.md` ("the spec")
//! defines them.
//!
//! **Everything here parses hostile input** (spec section 18): engine replies, events, workflow
//! fields and handles. Parsing never panics. Every field has a default, unknown fields are
//! ignored, and an unknown event kind, locator, state or rendition is skipped rather than
//! failing the message around it (spec section 3, rule 5).
//!
//! **Bytes never travel in these types** except an `inline` locator under the session's limits
//! (spec section 3, rule 7): values are named by [`Handle`] and reached through [`DataRef`]
//! locators.
//!
//! This crate knows no engine. Pack data (`x.vi`, …) stays opaque JSON.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod caps;
pub mod catalogue;
pub mod data;
pub mod de;
pub mod display;
pub mod error;
pub mod event;
pub mod expansion;
pub mod handle;
pub mod manifest;
pub mod media;
pub mod memo;
pub mod reducer;
pub mod run;
#[cfg(feature = "schema")]
pub mod schema;
pub mod session;
pub mod slice;

pub use caps::Caps;
pub use data::{BlobMeta, DataRef, Form, Locator, PortValue};
pub use error::{ErrorCode, ProtoError};
pub use handle::{Algo, Handle, HandleError, Recipe};

/// The protocol major this crate speaks (`"v": 2`). Protocol 1 is today's the research software protocol,
/// mapped into these types by its pack (spec section 17).
pub const PROTOCOL: u32 = 2;
