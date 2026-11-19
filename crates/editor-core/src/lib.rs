//! The editor core: everything about editing that isn't drawing or input recognition, with no
//! GUI toolkit in it (`docs/research/24-native-shells.md` sections 3 and 4). Every shell sits on
//! it: the egui editor (`plumcraft-ui`) today, the Swift and Kotlin shells later, and the C ABI
//! (`plumcraft-ffi`) between.
//!
//! Stage N1 cuts it out from under the egui editor in slices, with no change a user can see.
//! This slice holds the pure logic that was stranded in `plumcraft-ui` and the engine event pump:
//!
//! - [`plain`]: the core's own colour, point and rect, in place of a toolkit's;
//! - [`link`]: a connection to an engine and the thread pumping its events, with a [`link::Waker`];
//! - [`prefs`], [`appearance`], [`theme`]: preferences, the user's look, and themes as data;
//! - [`run_anim`], [`run_text`]: the run animation's model and run states as words;
//! - [`wire_path`]: a bent wire's curve and hit testing on it;
//! - [`palette`]: the node palette's search index;
//! - [`probe`], [`diff`], [`dtype`], [`raster`]: exact readings and differences of arrays;
//! - [`yaml`]: JSON values as YAML, for display.
//!
//! `plumcraft-ui` re-exports each at its old path, so code written against it still compiles.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod appearance;
pub mod diff;
pub mod dtype;
pub mod link;
pub mod palette;
pub mod plain;
pub mod prefs;
pub mod probe;
pub mod raster;
pub mod run_anim;
pub mod run_text;
pub mod theme;
pub mod wire_path;
pub mod yaml;
