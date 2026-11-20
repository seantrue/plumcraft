//! `plumcraft-ffi`: the C ABI the native shells call (`docs/research/24-native-shells.md`
//! section 7; the header is `include/plumcraft.h`, kept by hand). Stage N0(b): a spike that
//! proves the boundary with numbers.
//!
//! - **One core per `plum_open`.** A core thread owns the session and the in-process text
//!   engine ([`core`]). A core is named by a number, not a pointer, so a double close, a call
//!   after close or a made-up core is an error code, never a use after free.
//! - **Asynchronous requests.** `plum_request` queues the control protocol's
//!   `{id, method, params}` and returns the request's number at once; the reply,
//!   `{id, ok, result | error, tag?}`, comes out of `plum_poll` with that number as `id` (the
//!   caller's own `id` comes back as `tag`).
//! - **One wake, no payload,** from the core thread, coalesced: after a wake, none until the
//!   shell has polled. The shell hops to its own thread and calls `plum_poll`.
//! - **Borrowed bytes.** `plum_blob` lends a store blob's bytes (pointer and length) and holds
//!   them until `plum_release(token)`, which may come after `plum_close`. It never waits for an
//!   engine: what must be read through one (an HTTP engine's value) or made (a thumbnail, a
//!   decoded picture: a `derive:` key, `media.get`) is `PLUM_PENDING` until the media service has
//!   it, which `plum_poll`'s `media` says.
//!
//! Every entry catches unwinds. Every pointer from the caller is checked for null; lengths are
//! believed (C cannot do better), and the bytes behind them are hostile input.
//!
//! This crate is golden rule 2's second exception: `unsafe` here, and only here, each block
//! with a `SAFETY:` comment (enforced by `clippy::undocumented_unsafe_blocks`).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod core;
#[cfg(feature = "jni")]
pub mod jni;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use crate::core::{Config, Core, Lease, WakeFn};

/// The ABI's version; it changes only when a signature does.
pub const PLUM_ABI_VERSION: u32 = 1;

/// Status codes (`plum_bytes.status`, `plum_release`, `plum_close`).
pub const PLUM_OK: i32 = 0;
/// No such core: never opened, or closed.
pub const PLUM_ERR_CORE: i32 = -1;
/// A null pointer, an empty or non-UTF-8 argument.
pub const PLUM_ERR_ARG: i32 = -2;
/// Nothing there: no bytes for that handle, no such token.
pub const PLUM_ERR_MISSING: i32 = -3;
/// A panic was caught at the boundary.
pub const PLUM_ERR_PANIC: i32 = -4;
/// Its bytes were read and aren't what they claim (a picture that won't decode).
pub const PLUM_ERR_UNREADABLE: i32 = -5;
/// Not here yet: the media service is reading or making it; `plum_poll`'s `media` says when.
pub const PLUM_PENDING: i32 = 1;

/// A buffer the library made (JSON from `plum_poll`, `plum_last_error`). Free it with
/// `plum_free`, once. `ptr` is null and `len` 0 when there is nothing.
#[repr(C)]
#[derive(Debug)]
pub struct PlumBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl PlumBuf {
    const EMPTY: PlumBuf = PlumBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 };

    fn from_vec(v: Vec<u8>) -> PlumBuf {
        if v.is_empty() {
            return PlumBuf::EMPTY;
        }
        let mut v = std::mem::ManuallyDrop::new(v);
        PlumBuf { ptr: v.as_mut_ptr(), len: v.len(), cap: v.capacity() }
    }
}

/// Bytes lent by `plum_blob`: valid, unchanged, until `plum_release(token)`. Read only.
#[repr(C)]
#[derive(Debug)]
pub struct PlumBytes {
    pub ptr: *const u8,
    pub len: usize,
    /// For `plum_release`; 0 when `status` is not `PLUM_OK`.
    pub token: u64,
    pub status: i32,
}

impl PlumBytes {
    fn err(status: i32) -> PlumBytes {
        PlumBytes { ptr: std::ptr::null(), len: 0, token: 0, status }
    }
}

/// The open cores, by number.
static CORES: LazyLock<Mutex<HashMap<u64, Arc<Core>>>> = LazyLock::new(Mutex::default);
static NEXT_CORE: AtomicU64 = AtomicU64::new(1);
/// Bytes lent out, by token: process-wide, so bytes outlive the core that lent them.
static LEASES: LazyLock<Mutex<HashMap<u64, Lease>>> = LazyLock::new(Mutex::default);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// Why the last `plum_open` on this thread failed.
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn core(id: u64) -> Option<Arc<Core>> {
    lock(&CORES).get(&id).cloned()
}

fn set_error(e: impl Into<String>) {
    let e = e.into();
    LAST_ERROR.with(|l| *l.borrow_mut() = e);
}

/// Run `f`, turning a panic into `fallback`.
fn guard<T>(fallback: impl FnOnce() -> T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| fallback())
}

/// The caller's bytes, borrowed for this call; empty for a null pointer or a zero length.
///
/// # Safety
///
/// Unless `ptr` is null, `ptr..ptr+len` must be readable and unchanged for the call.
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 || len > isize::MAX as usize {
        return &[];
    }
    // SAFETY: not null and within isize::MAX (checked above); readable and unchanged for this
    // call is the caller's promise, the one every C function taking (ptr, len) asks for.
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// The ABI version this library speaks (`PLUM_ABI_VERSION`).
#[unsafe(no_mangle)]
pub extern "C" fn plum_abi_version() -> u32 {
    PLUM_ABI_VERSION
}

/// Open a core: start the engine and the session. `config_json` may be null (defaults);
/// `wake` may be null (the shell polls on its own). Returns the core's number, or 0 on failure
/// (`plum_last_error` says why).
///
/// `wake(ctx)` is called on a thread of the core's, never the caller's, with no payload and at
/// most once until the next `plum_poll`. It must only schedule (hop to the shell's thread) and
/// return: it must not call back into this library.
///
/// # Safety
///
/// `config_json..config_json+len` readable for the call, unless null. `ctx` is passed back to
/// `wake` as it is and never read here; it must stay valid until `plum_close` returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plum_open(config_json: *const u8, len: usize, wake: Option<WakeFn>, ctx: *mut c_void) -> u64 {
    // SAFETY: the caller's promise about (config_json, len), passed on.
    let config = unsafe { bytes(config_json, len) };
    guard(
        || {
            set_error("plum_open panicked");
            0
        },
        || {
            let opened = Config::parse(config).and_then(|c| Core::open(&c, wake, ctx));
            match opened {
                Ok(c) => {
                    let id = NEXT_CORE.fetch_add(1, Ordering::Relaxed);
                    lock(&CORES).insert(id, Arc::new(c));
                    id
                }
                Err(e) => {
                    set_error(e);
                    0
                }
            }
        },
    )
}

/// Why the last `plum_open` on this thread failed, as UTF-8 text (free it with `plum_free`).
#[unsafe(no_mangle)]
pub extern "C" fn plum_last_error() -> PlumBuf {
    guard(|| PlumBuf::EMPTY, || PlumBuf::from_vec(LAST_ERROR.with(|l| l.borrow().clone()).into_bytes()))
}

/// Close a core: cancel its run, stop its threads, drop its session. Waits for the core thread
/// (not when called from the wake, which runs on it). Bytes it lent stay valid until released.
/// `PLUM_OK`, or `PLUM_ERR_CORE` for a core that isn't open (a second close).
#[unsafe(no_mangle)]
pub extern "C" fn plum_close(core: u64) -> i32 {
    guard(
        || PLUM_ERR_PANIC,
        || {
            let Some(c) = lock(&CORES).remove(&core) else { return PLUM_ERR_CORE };
            c.close();
            PLUM_OK
        },
    )
}

/// Queue a request, the control protocol's `{id, method, params}` as UTF-8 JSON, copied before
/// this returns. Returns the request's number (its reply's `id`), or 0 when the core isn't open.
/// Anything else, even bytes that aren't JSON, is answered, with an error reply.
///
/// # Safety
///
/// `json..json+len` readable for the call, unless null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plum_request(core: u64, json: *const u8, len: usize) -> u64 {
    // SAFETY: the caller's promise about (json, len), passed on.
    let body = unsafe { bytes(json, len) };
    guard(|| 0, || self::core(core).and_then(|c| c.request(body.to_vec())).unwrap_or(0))
}

/// Everything since the last poll: `{"replies": [...], "events": [...], "dropped": n}` (UTF-8
/// JSON; `events` are the engine's event envelopes, `dropped` how many were dropped unpolled).
/// An empty buffer (null, 0) when there is nothing, or the core isn't open. Free with `plum_free`.
#[unsafe(no_mangle)]
pub extern "C" fn plum_poll(core: u64) -> PlumBuf {
    guard(|| PlumBuf::EMPTY, || self::core(core).and_then(|c| c.shared.poll()).map_or(PlumBuf::EMPTY, PlumBuf::from_vec))
}

/// Free a buffer this library returned. A null buffer is ignored.
///
/// # Safety
///
/// `buf` exactly as `plum_poll` or `plum_last_error` returned it, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plum_free(buf: PlumBuf) {
    if buf.ptr.is_null() || buf.cap == 0 || buf.len > buf.cap {
        return;
    }
    // SAFETY: (ptr, len, cap) came from a Vec<u8> PlumBuf::from_vec forgot, and the caller frees
    // it once; dropping it here gives the memory back to the allocator that made it.
    drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
}

/// Lend the bytes of a store blob by handle (UTF-8, as in a DataRef: `md5:raw:…`), without a
/// copy, or what the media service made by a `derive:` key (`media.get`'s `key`: an RGBA8
/// picture, peaks, a file's numbers). A blob already lent is shared. Read the bytes, never write
/// them (they may be a mapped file); release with `plum_release(token)`. Never waits for an
/// engine: `PLUM_PENDING` while the bytes are on their way (`plum_poll`'s `media` says when).
///
/// # Safety
///
/// `handle..handle+len` readable for the call, unless null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plum_blob(core: u64, handle: *const u8, len: usize) -> PlumBytes {
    // SAFETY: the caller's promise about (handle, len), passed on.
    let handle = unsafe { bytes(handle, len) };
    guard(
        || PlumBytes::err(PLUM_ERR_PANIC),
        || {
            let Ok(handle) = std::str::from_utf8(handle) else { return PlumBytes::err(PLUM_ERR_ARG) };
            if handle.is_empty() {
                return PlumBytes::err(PLUM_ERR_ARG);
            }
            let Some(c) = self::core(core) else { return PlumBytes::err(PLUM_ERR_CORE) };
            let lease = match c.shared.blob(handle) {
                Ok(l) => l,
                Err(status) => return PlumBytes::err(status),
            };
            let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
            let (ptr, len) = (lease.bytes().as_ptr(), lease.bytes().len());
            lock(&LEASES).insert(token, lease);
            PlumBytes { ptr, len, token, status: PLUM_OK }
        },
    )
}

/// Give back bytes `plum_blob` lent. `PLUM_OK`, or `PLUM_ERR_MISSING` for a token not lent
/// (or already released). Allowed after the core that lent them is closed.
#[unsafe(no_mangle)]
pub extern "C" fn plum_release(token: u64) -> i32 {
    guard(|| PLUM_ERR_PANIC, || if lock(&LEASES).remove(&token).is_some() { PLUM_OK } else { PLUM_ERR_MISSING })
}

/// How many blobs are lent and not released (the measurements and leak checks).
#[unsafe(no_mangle)]
pub extern "C" fn plum_leases() -> u64 {
    guard(|| 0, || lock(&LEASES).len() as u64)
}

// ---------- hot calls (N2 slice 3; docs/shell-interface.md section 9) ----------

/// What a hot call found (`plum_hit`): what is at a point (`plum_hit_test`), or where a dragged
/// wire may land (`plum_wire_target`). The kinds and parts are
/// `plumcraft_editor_core::geometry::{kind, part}`'s; `docs/shell-interface.md` 9.2 lists them.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlumHit {
    /// `PLUM_OK`; `PLUM_ERR_CORE`, `PLUM_ERR_MISSING` (no such document, or no scene published
    /// for it yet), `PLUM_ERR_ARG`, `PLUM_ERR_PANIC`, with `kind` 0.
    pub status: i32,
    /// 0 none, 1 node, 2 pin, 3 wire, 4 bend point, 5 net tag, 6 frame, 7 group.
    pub kind: u32,
    pub part: u32,
    /// A row, footer item or bend point; -1 for none.
    pub index: i32,
    /// The nid of what was hit (a face, wire, net, frame or group, by `kind`).
    pub item: u64,
    /// The face (kinds 1 and 2, a tag's).
    pub node: u64,
    /// The wire (kinds 3 and 4, a use's tag).
    pub wire: u64,
    /// The pin's index on its side (kind 2).
    pub pin: u32,
    /// `plum_wire_target`: 0 doesn't fit, 1 `wire.connect` takes it, 2 only through a Convert.
    pub fits: u32,
    /// The pin's point (kind 2), else the point asked (graph units).
    pub x: f32,
    pub y: f32,
}

impl PlumHit {
    fn err(status: i32) -> PlumHit {
        PlumHit { status, kind: 0, part: 0, index: -1, item: 0, node: 0, wire: 0, pin: 0, fits: 0, x: 0.0, y: 0.0 }
    }

    fn of(h: plumcraft_editor_core::geometry::Hit) -> PlumHit {
        PlumHit {
            status: PLUM_OK,
            kind: h.kind,
            part: h.part,
            index: h.index,
            item: h.item,
            node: h.node,
            wire: h.wire,
            pin: h.pin,
            fits: h.fits,
            x: h.x,
            y: h.y,
        }
    }
}

/// One face's measurements for `plum_geometry_set` (graph units, at zoom 1).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PlumFace {
    /// The face's nid (the scene's).
    pub face: u64,
    /// The face's `rev` the shell measured; 0: not said.
    pub rev: u64,
    pub w: f32,
    pub h: f32,
    /// The header's height; NaN or below 0: not said.
    pub header: f32,
    /// Each input pin's height from the face's top, in row order (NaN: not measured).
    pub inputs: *const f32,
    pub n_inputs: usize,
    pub outputs: *const f32,
    pub n_outputs: usize,
    /// Each input row's widget rect, four floats a row (x0, y0, x1, y1, face coordinates; a NaN
    /// x0: no widget). `n_rows` counts rows, not floats.
    pub rows: *const f32,
    pub n_rows: usize,
}

/// The caller's floats, borrowed for this call; empty for a null pointer or a zero count.
///
/// # Safety
///
/// Unless `ptr` is null, `ptr..ptr+n` floats must be readable and unchanged for the call.
unsafe fn floats<'a>(ptr: *const f32, n: usize) -> Option<&'a [f32]> {
    if ptr.is_null() || n == 0 {
        return Some(&[]);
    }
    if n > plumcraft_editor_core::geometry::MAX_PINS * 4 || !ptr.is_aligned() {
        return None;
    }
    // SAFETY: not null, aligned and a bounded count (checked above); readable and unchanged for
    // this call is the caller's promise.
    Some(unsafe { std::slice::from_raw_parts(ptr, n) })
}

fn hot(core: u64, doc: u64, f: impl FnOnce(&plumcraft_editor_core::geometry::Snapshot) -> PlumHit) -> PlumHit {
    let Some(c) = self::core(core) else { return PlumHit::err(PLUM_ERR_CORE) };
    match c.shared.snapshot(doc) {
        Some(s) => f(&s),
        None => PlumHit::err(PLUM_ERR_MISSING),
    }
}

/// What is at `(x, y)` (graph units) in document `doc` (0: the first), within `slop` graph units
/// (the shell's target size over its zoom): a pin first, then a face and its part, a bend point,
/// a net tag, a frame's tab or handle, a wire, an open group's band, a frame's body. Reads the
/// latest snapshot without waiting for the core; it may be a publish behind a request just sent.
#[unsafe(no_mangle)]
pub extern "C" fn plum_hit_test(core: u64, doc: u64, x: f32, y: f32, slop: f32) -> PlumHit {
    guard(|| PlumHit::err(PLUM_ERR_PANIC), || hot(core, doc, |s| PlumHit::of(s.hit(x, y, slop))))
}

/// Where a wire dragged from pin `pin` of face `face` (`side` 1 an input, 2 an output) may land
/// at `(x, y)`: the nearest pin on the other side within `slop` that fits (`fits` 1, or 2 only
/// through a Convert), else the nearest that doesn't (`fits` 0), else `kind` 0. `PLUM_ERR_ARG`
/// for another side.
#[unsafe(no_mangle)]
pub extern "C" fn plum_wire_target(core: u64, doc: u64, face: u64, side: u32, pin: u32, x: f32, y: f32, slop: f32) -> PlumHit {
    guard(
        || PlumHit::err(PLUM_ERR_PANIC),
        || {
            let output = match side {
                1 => false,
                2 => true,
                _ => return PlumHit::err(PLUM_ERR_ARG),
            };
            hot(core, doc, |s| PlumHit::of(s.wire_target(face, output, pin, x, y, slop)))
        },
    )
}

/// The paths of every wire with an end on one of the `n` faces at `faces` (nids), as if those
/// faces were moved by `(dx, dy)` graph units: per wire its nid's low and high 32 bits and its
/// point count (each a `uint32_t` bit-cast into a float), then that many points as `x, y`, a cubic chain
/// (the start, then three points a segment). Returns the floats needed; when that is more than
/// `cap` (or `out` is null) nothing is written. 0 for a core or document that isn't there.
///
/// # Safety
///
/// `faces..faces+n` readable for the call unless null; `out..out+cap` writable unless null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plum_wire_paths(core: u64, doc: u64, faces: *const u64, n: usize, dx: f32, dy: f32, out: *mut f32, cap: usize) -> usize {
    guard(
        || 0,
        || {
            let ids: &[u64] = if faces.is_null() || n == 0 || n > plumcraft_editor_core::geometry::MAX_FACES || !faces.is_aligned() {
                &[]
            } else {
                // SAFETY: not null, aligned and a bounded count (checked above); readable and
                // unchanged for this call is the caller's promise.
                unsafe { std::slice::from_raw_parts(faces, n) }
            };
            let Some(s) = self::core(core).and_then(|c| c.shared.snapshot(doc)) else { return 0 };
            let mut v = Vec::new();
            s.wire_paths(ids, dx, dy, &mut v);
            if !out.is_null() && v.len() <= cap && out.is_aligned() {
                // SAFETY: `out..out+cap` is writable (the caller's promise), aligned (checked),
                // and `v.len() <= cap` floats are written; `v` is ours, so the two don't overlap.
                unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len()) };
            }
            v.len()
        },
    )
}

/// Where a drag under way is (`plum_gesture_move`, N2 slice 4): what `ui.gesture {kind: "drag",
/// phase: "move"}` answers, typed.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlumDrag {
    /// `PLUM_OK`; `PLUM_ERR_CORE`; `PLUM_ERR_MISSING` (no such document, no drag under way, or no
    /// scene published); `PLUM_ERR_PANIC`; with `kind` 0.
    pub status: i32,
    /// 1 move (faces), 2 wire, 3 marquee, 4 bend point, 5 frame, 6 frame's size, 7 open group.
    pub kind: u32,
    /// The drag's number (`ui.gesture`'s `begin` reply says it): a shell checks it is its own.
    pub serial: u64,
    /// The offset from where the drag began (graph units).
    pub dx: f32,
    pub dy: f32,
    /// A marquee's rect (x0, y0, x1, y1); zeros for the others.
    pub rect: [f32; 4],
    /// A wire's target: where it would land (`kind` 2, a pin, `x, y` its point) and whether it
    /// fits (`fits`), as `plum_wire_target` answers.
    pub target: PlumHit,
}

impl PlumDrag {
    fn err(status: i32) -> PlumDrag {
        PlumDrag { status, kind: 0, serial: 0, dx: 0.0, dy: 0.0, rect: [0.0; 4], target: PlumHit::err(status) }
    }
}

/// Where the drag under way in document `doc` (0: the first) is with the pointer at `(x, y)`
/// (graph units), within `slop`: the offset, a marquee's rect, a wire's target and whether it
/// fits. Nothing changes; the drag began, and ends or is cancelled, with `ui.gesture`. Reads the
/// document's drag and its latest snapshot without waiting for the core.
#[unsafe(no_mangle)]
pub extern "C" fn plum_gesture_move(core: u64, doc: u64, x: f32, y: f32, slop: f32) -> PlumDrag {
    guard(
        || PlumDrag::err(PLUM_ERR_PANIC),
        || {
            let Some(c) = self::core(core) else { return PlumDrag::err(PLUM_ERR_CORE) };
            let (Some(d), Some(s)) = (c.shared.drag(doc), c.shared.snapshot(doc)) else { return PlumDrag::err(PLUM_ERR_MISSING) };
            if !(x.is_finite() && y.is_finite()) {
                return PlumDrag::err(PLUM_ERR_ARG);
            }
            let slop = if slop.is_nan() { 0.0 } else { slop.clamp(0.0, 1.0e4) };
            let at = plumcraft_editor_core::plain::Point::new(plumcraft_editor_core::wire_path::coord(x), plumcraft_editor_core::wire_path::coord(y));
            let u = d.update(&s, at, slop);
            let rect = u.rect.map_or([0.0; 4], |r| [r.min.x, r.min.y, r.max.x, r.max.y]);
            PlumDrag { status: PLUM_OK, kind: u.code, serial: u.serial, dx: u.dx, dy: u.dy, rect, target: PlumHit::of(u.target) }
        },
    )
}

/// What the shell measured of `n` faces at `faces`, for document `doc` (0: the first): what
/// `geometry.set` takes, typed. Copied before this returns, kept on the core's thread (the next
/// snapshot has it); faces by a nid the scene doesn't have are skipped. `PLUM_OK`,
/// `PLUM_ERR_CORE`, or `PLUM_ERR_ARG` (a null `faces` with `n` above 0, more than
/// 100,000 faces, a list of pins or rows past 4,096, a misaligned pointer).
///
/// # Safety
///
/// `faces..faces+n` readable for the call, and every pointer in each face readable for its
/// count, unless null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plum_geometry_set(core: u64, doc: u64, faces: *const PlumFace, n: usize) -> i32 {
    use plumcraft_editor_core::geometry::{FaceGeometry, MAX_FACES, MAX_PINS, MAX_SIZE};
    use plumcraft_editor_core::wire_path::coord;
    guard(
        || PLUM_ERR_PANIC,
        || {
            let Some(c) = self::core(core) else { return PLUM_ERR_CORE };
            if n == 0 {
                return PLUM_OK;
            }
            if faces.is_null() || n > MAX_FACES || !faces.is_aligned() {
                return PLUM_ERR_ARG;
            }
            // SAFETY: not null, aligned and a bounded count (checked above); readable for the
            // call is the caller's promise.
            let list = unsafe { std::slice::from_raw_parts(faces, n) };
            let mut out = Vec::with_capacity(n);
            for f in list {
                if f.n_inputs > MAX_PINS || f.n_outputs > MAX_PINS || f.n_rows > MAX_PINS {
                    return PLUM_ERR_ARG;
                }
                // SAFETY: the face's inputs pointer and count, the caller's promise; bounded above.
                let inputs = unsafe { floats(f.inputs, f.n_inputs) };
                // SAFETY: the face's outputs pointer and count, as for the inputs.
                let outputs = unsafe { floats(f.outputs, f.n_outputs) };
                // SAFETY: the face's rows pointer, four floats a row, as for the inputs.
                let rows = unsafe { floats(f.rows, f.n_rows.saturating_mul(4)) };
                let (Some(i), Some(o), Some(r)) = (inputs, outputs, rows) else { return PLUM_ERR_ARG };
                let pin = |y: &f32| y.is_finite().then(|| coord(*y));
                let geometry = FaceGeometry {
                    size: [f.w, f.h].map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, MAX_SIZE) }),
                    header: (f.header.is_finite() && f.header >= 0.0).then(|| f.header.min(MAX_SIZE)),
                    inputs: i.iter().map(pin).collect(),
                    outputs: o.iter().map(pin).collect(),
                    rows: r.as_chunks::<4>().0.iter().map(|q| q.iter().all(|v| v.is_finite()).then(|| q.map(coord))).collect(),
                    parts: Vec::new(),
                    offset: [0.0; 2],
                };
                out.push((f.face, (f.rev > 0).then_some(f.rev), geometry));
            }
            if c.geometry(doc, out) { PLUM_OK } else { PLUM_ERR_CORE }
        },
    )
}
