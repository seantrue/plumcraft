//! The editor core: everything about editing that isn't drawing or input recognition, with no
//! GUI toolkit in it (`docs/research/24-native-shells.md` sections 3 and 4). Every shell sits on
//! it: the egui editor (`plumcraft-ui`) today, the Swift and Kotlin shells later, and the C ABI
//! (`plumcraft-ffi`) between.
//!
//! Stage N1 cuts it out from under the egui editor in slices, with no change a user can see.
//! It holds the pure logic that was stranded in `plumcraft-ui`, the engine event pump, and (slice
//! 2) starting and switching engines, which was the windowed app's and `plumcraft-ui`'s:
//!
//! - [`plain`]: the core's own colour, point and rect, in place of a toolkit's;
//! - [`link`]: a connection to an engine and the thread pumping its events, with a [`link::Waker`];
//! - [`engines`]: which engines there are, how each may be hosted on this platform
//!   ([`engines::Capabilities`]), and starting one ([`engines::Launcher`]);
//! - [`switchboard`]: switching, adding, removing, attaching and clearing engines while editing;
//! - [`document`]: one open document, owning its session, engine link, worker, asks and media,
//!   with the per-turn work every shell calls (slice 7);
//! - [`worker`]: a document's engine calls (runs, cancels, releases, a switch's connect), off the
//!   shell's thread, in order, handed back through an inbox;
//! - [`install`]: installing an engine from GitHub, off the shell's thread;
//! - [`headless`]: the windowless commands `run`, `check`, `tidy` and `nodes`;
//! - [`asks`], [`invoke`]: what the core asks the shell, and running a command as its menu item
//!   does (slice 4);
//! - [`control`]: the control channel's requests, for every shell, over a [`control::Shell`]
//!   trait for what only a shell can do, and engine switching without waiting (slice 5);
//! - [`prefs`], [`appearance`], [`theme`]: preferences, the user's look, and themes as data;
//! - [`run_anim`], [`run_text`]: the run animation's model and run states as words;
//! - [`wire_path`]: a bent wire's curve and hit testing on it;
//! - [`menu`]: every menu as data (the bar, context menus, shortcuts, roles), for every shell
//!   (slice 8); [`rows`]: a node's input rows and a collapsed group's outputs, which it lists;
//! - [`log`]: one log for the core and every shell: a bounded ring, levels, an optional file
//!   (slice 8);
//! - [`selection`], [`canvas_cmds`]: the canvas's selection and the group entered as document
//!   state, and the canvas's commands (select, Select All, Delete, enter a group, fit, the
//!   palette, reveal) (N2 slice 1);
//! - [`palette`]: the node palette's search index;
//! - [`probe`], [`diff`], [`dtype`], [`raster`]: exact readings and differences of arrays;
//! - [`values`]: a document's values as pictures, through the media service
//!   ([`plumcraft_media`]), which reads and decodes them off the shell's thread (slice 6);
//! - [`yaml`]: JSON values as YAML, for display.
//!
//! `plumcraft-ui` re-exports each at its old path, so code written against it still compiles;
//! `plumcraft-editor` re-exports [`engines::PackEngine`] as before.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod appearance;
pub mod asks;
pub mod autosave;
pub mod bundle;
pub mod canvas_cmds;
pub mod conformance;
pub mod control;
pub mod diff;
pub mod document;
pub mod dtype;
pub mod engines;
pub mod geometry;
pub mod gesture;
pub mod headless;
pub mod icons;
pub mod inline;
pub mod install;
pub mod invoke;
pub mod link;
pub mod log;
pub mod menu;
pub mod naming;
pub mod palette;
pub mod plain;
pub mod prefs;
pub mod probe;
pub mod raster;
pub mod rows;
pub mod run_anim;
pub mod run_text;
pub mod scene;
pub mod selection;
pub mod settings;
pub mod shell_schema;
pub mod switchboard;
pub mod theme;
pub mod values;
pub mod widget_text;
pub mod windows;
pub mod wire_path;
pub mod worker;
pub mod yaml;

/// The media crate, for shells that reach it through the core.
pub use plumcraft_media as media;
