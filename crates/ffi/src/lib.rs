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
//!   them until `plum_release(token)`, which may come after `plum_close`.
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

use plumcraft_store::Blob;

use crate::core::{Config, Core, WakeFn};

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
/// Blobs lent out, by token: process-wide, so bytes outlive the core that lent them.
static LEASES: LazyLock<Mutex<HashMap<u64, Arc<Blob>>>> = LazyLock::new(Mutex::default);
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
/// copy. A blob already lent is shared. Read the bytes, never write them (they may be a mapped
/// file); release with `plum_release(token)`.
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
            let Ok(blob) = c.shared.blob(handle) else { return PlumBytes::err(PLUM_ERR_MISSING) };
            let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
            let (ptr, len) = (blob.bytes.as_ptr(), blob.bytes.len());
            lock(&LEASES).insert(token, blob);
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
