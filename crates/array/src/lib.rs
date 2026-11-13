//! The array engine (`array`): the research software's general array nodes, on the CPU, in Rust.
//!
//! Arrays are the core ARRAY type (`plumcraft_proto::catalogue::core_types`): any numeric
//! dtype in, `f32` out (integer labels as `<i4`), with an optional colour space and channel
//! names. The nodes keep the research software's titles, ports, defaults and descriptions; their arithmetic
//! is ported from yapocis's numba kernels (`yapocis3/src/yapocis/kernels/numbax`, without
//! `fastmath`, so results are the same on every machine) and the yapocis library code around
//! them, with npexact for the steps that are numpy's own (draws, reductions, histograms,
//! sorts). They match the research software closely, not bit for bit: the research software runs yapocis's GPU kernels.
//! Nothing from yapocis_external: what the research software took from there is written here plainly, and
//! Mean Shift and Posterize Hue are left out.
//!
//! Groups (each a module with its own `nodes()`): [`channels`] (files, channels, stacks),
//! [`math`], [`filters`] (filters, edges, geometry), [`generate`] (generators, regions, drawing,
//! frequency) and [`colour`].

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod channels;
pub mod colour;
pub mod filters;
pub mod generate;
pub mod img;
pub mod math;
pub mod op;

use plumcraft_engine::{Engine, Node};
use plumcraft_proto::catalogue::{Media, WireType, core_types};
use plumcraft_proto::session::EngineInfo;

/// The engine's demo workflow.
pub const DEMO_WORKFLOW: &str = include_str!("demo.json");

/// Every node, in palette order.
pub fn nodes() -> Vec<Box<dyn Node>> {
    let mut v = channels::nodes();
    v.extend(colour::nodes());
    v.extend(math::nodes());
    v.extend(filters::nodes());
    v.extend(generate::nodes());
    v
}

/// The engine, ready to host.
pub fn engine() -> Engine {
    let version = env!("CARGO_PKG_VERSION");
    let info = EngineInfo {
        id: "array".into(),
        name: "Array".into(),
        version: version.into(),
        salt: plumcraft_engine::salt(&format!("rs:plumcraft-array+{version}"), &plumcraft_engine::code_digest!()),
        ..Default::default()
    };
    let mut e = Engine::new(info).with_wire_type(core_types::array()).with_svg().with_wire_type(core_types::image());
    // PDF, as the craft packs declare it, so their exports cross into Rasterize PDF
    e = e.with_wire_type(WireType { id: "PDF".into(), media: Media::Bytes, ..Default::default() });
    // and video (FilmCraft's exports), into Video to Array
    for id in channels::video_types() {
        e = e.with_wire_type(WireType { id: id.into(), media: Media::Bytes, ..Default::default() });
    }
    for (id, media) in [("FLOAT", Media::Scalar), ("INT", Media::Scalar), ("BOOL", Media::Scalar), ("STRING", Media::Scalar), ("ENUM", Media::Scalar)] {
        e = e.with_wire_type(WireType { id: id.into(), media, ..Default::default() });
    }
    for n in nodes() {
        e = e.with_node(n);
    }
    e.with_building_blocks()
}
