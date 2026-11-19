//! The JNI shims (feature `jni`): the C ABI's calls as the native methods of
//! `com.plumcraft.ffi.PlumNative` (`native/android/`), the way the Swift package calls them.
//!
//! - JSON crosses as UTF-8 byte arrays, not `jstring`s: JNI strings are "modified UTF-8" and
//!   mangle characters outside the BMP (the native virtimp.com app's lesson).
//! - **No callback into the VM.** A core opened here gets a wake of this module's own that
//!   signals a condition variable; a Kotlin thread blocks in `nativeAwait` and hops to its own
//!   main thread. So no foreign thread is ever attached to the VM, and the wake does nothing but
//!   schedule, as on Apple.
//! - Lent bytes come back as a direct `ByteBuffer` over the lent memory (no copy); Kotlin makes
//!   it read-only and calls `nativeRelease` with the token.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::Duration;

use jni_sys::{JNIEnv, jbyte, jbyteArray, jclass, jint, jlong, jlongArray, jobject, jsize};

use crate::{PLUM_OK, lock};

/// A core's wake, as a flag and a condition variable.
#[derive(Default)]
struct Signal {
    woken: Mutex<Option<bool>>,
    cv: Condvar,
}

/// Wake signals by number (the number is the wake's `ctx`, so the wake reads no pointer), and
/// which signal each core has.
static SIGNALS: LazyLock<Mutex<HashMap<u64, Arc<Signal>>>> = LazyLock::new(Mutex::default);
static BY_CORE: LazyLock<Mutex<HashMap<u64, u64>>> = LazyLock::new(Mutex::default);
static NEXT_SIGNAL: AtomicU64 = AtomicU64::new(1);

/// The wake a JNI core gets: set the flag, notify. Runs on the core thread.
extern "C" fn wake(ctx: *mut c_void) {
    let id = ctx as usize as u64;
    if let Some(s) = lock(&SIGNALS).get(&id).cloned() {
        let mut w = lock(&s.woken);
        if w.is_some() {
            *w = Some(true);
        }
        s.cv.notify_all();
    }
}

/// The JNI function table.
///
/// # Safety
///
/// `env` is the `JNIEnv` the VM passed to this native method.
unsafe fn fns(env: *mut JNIEnv) -> &'static jni_sys::JNINativeInterface__1_4 {
    // SAFETY: the VM passes a valid env pointing at its function table, which lives as long as
    // the VM; JNI 1.4's table (NewDirectByteBuffer) is in every VM since Java 1.4 and Android.
    unsafe { &(**env).v1_4 }
}

/// A Java byte array's contents, copied; empty for null.
///
/// # Safety
///
/// `env` as for [`fns`]; `a` a local reference to a `byte[]` or null.
unsafe fn read(env: *mut JNIEnv, a: jbyteArray) -> Vec<u8> {
    if a.is_null() {
        return Vec::new();
    }
    // SAFETY: env is valid (the caller's promise) and `a` a live byte[] reference.
    unsafe {
        let f = fns(env);
        let len = (f.GetArrayLength)(env, a).max(0);
        let mut buf = vec![0u8; len as usize];
        (f.GetByteArrayRegion)(env, a, 0, len, buf.as_mut_ptr().cast::<jbyte>());
        buf
    }
}

/// A new Java byte array holding `bytes`; null when the VM can't make one (an exception is then
/// pending, which Kotlin sees).
///
/// # Safety
///
/// `env` as for [`fns`].
unsafe fn new_array(env: *mut JNIEnv, bytes: &[u8]) -> jbyteArray {
    let Ok(n) = jsize::try_from(bytes.len()) else { return std::ptr::null_mut() };
    // SAFETY: env is valid; the array is made with n elements and n bytes are copied into it.
    unsafe {
        let f = fns(env);
        let a = (f.NewByteArray)(env, n);
        if !a.is_null() {
            (f.SetByteArrayRegion)(env, a, 0, n, bytes.as_ptr().cast::<jbyte>());
        }
        a
    }
}

/// `catch_unwind` for a native method.
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(fallback)
}

fn core_id(core: jlong) -> u64 {
    u64::try_from(core).unwrap_or(0)
}

/// `PlumNative.nativeOpen(config: ByteArray?): Long`: `plum_open` with this module's wake; 0 on
/// failure (`nativeLastError`).
///
/// # Safety
///
/// Called by the VM with its env.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativeOpen(env: *mut JNIEnv, _: jclass, config: jbyteArray) -> jlong {
    // SAFETY: the VM's env and a byte[] (or null) it passed.
    let config = unsafe { read(env, config) };
    guard(0, || {
        let sid = NEXT_SIGNAL.fetch_add(1, Ordering::Relaxed);
        let signal = Arc::new(Signal { woken: Mutex::new(Some(false)), cv: Condvar::new() });
        lock(&SIGNALS).insert(sid, signal);
        // SAFETY: the config is a live Vec for the call; the ctx is a number, never read as a
        // pointer (see `wake`).
        let core = unsafe { crate::plum_open(config.as_ptr(), config.len(), Some(wake), sid as usize as *mut c_void) };
        if core == 0 {
            lock(&SIGNALS).remove(&sid);
            return 0;
        }
        lock(&BY_CORE).insert(core, sid);
        jlong::try_from(core).unwrap_or(0)
    })
}

/// `PlumNative.nativeLastError(): ByteArray`.
///
/// # Safety
///
/// Called by the VM with its env.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativeLastError(env: *mut JNIEnv, _: jclass) -> jbyteArray {
    let text = guard(String::new(), || crate::LAST_ERROR.with(|l| l.borrow().clone()));
    // SAFETY: the VM's env.
    unsafe { new_array(env, text.as_bytes()) }
}

/// `PlumNative.nativeClose(core: Long): Int`: `plum_close`; a thread in `nativeAwait` returns.
///
/// # Safety
///
/// Called by the VM.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativeClose(_: *mut JNIEnv, _: jclass, core: jlong) -> jint {
    let core = core_id(core);
    let status = crate::plum_close(core);
    guard((), || {
        if let Some(sid) = lock(&BY_CORE).remove(&core)
            && let Some(s) = lock(&SIGNALS).remove(&sid)
        {
            *lock(&s.woken) = None;
            s.cv.notify_all();
        }
    });
    status
}

/// `PlumNative.nativeRequest(core: Long, json: ByteArray?): Long`: `plum_request`.
///
/// # Safety
///
/// Called by the VM with its env.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativeRequest(env: *mut JNIEnv, _: jclass, core: jlong, json: jbyteArray) -> jlong {
    // SAFETY: the VM's env and a byte[] (or null) it passed.
    let body = unsafe { read(env, json) };
    // SAFETY: body is a live Vec for the call.
    let id = unsafe { crate::plum_request(core_id(core), body.as_ptr(), body.len()) };
    jlong::try_from(id).unwrap_or(0)
}

/// `PlumNative.nativePoll(core: Long): ByteArray?`: `plum_poll`, copied into a Java array (the
/// one copy JNI needs) and freed; null when there is nothing.
///
/// # Safety
///
/// Called by the VM with its env.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativePoll(env: *mut JNIEnv, _: jclass, core: jlong) -> jbyteArray {
    let buf = crate::plum_poll(core_id(core));
    if buf.ptr.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: plum_poll's buffer is len initialised bytes until plum_free, called once below;
    // env is the VM's.
    unsafe {
        let a = new_array(env, std::slice::from_raw_parts(buf.ptr, buf.len));
        crate::plum_free(buf);
        a
    }
}

/// `PlumNative.nativeAwait(core: Long, timeoutMs: Long): Boolean`: block until the core wakes
/// (true), the timeout passes or the core is closed (false). Never on the main thread.
///
/// # Safety
///
/// Called by the VM.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativeAwait(_: *mut JNIEnv, _: jclass, core: jlong, timeout_ms: jlong) -> u8 {
    guard(0, || {
        let Some(s) = lock(&BY_CORE).get(&core_id(core)).and_then(|sid| lock(&SIGNALS).get(sid).cloned()) else { return 0 };
        let timeout = Duration::from_millis(u64::try_from(timeout_ms).unwrap_or(0));
        let w = lock(&s.woken);
        let (mut w, _) = s.cv.wait_timeout_while(w, timeout, |w| *w == Some(false)).unwrap_or_else(std::sync::PoisonError::into_inner);
        match *w {
            Some(true) => {
                *w = Some(false);
                1
            }
            _ => 0,
        }
    })
}

/// `PlumNative.nativeBlob(core: Long, handle: ByteArray, token: LongArray): ByteBuffer?`:
/// `plum_blob` as a direct buffer over the lent bytes (no copy), its token in `token[0]`; null
/// when there are none.
///
/// # Safety
///
/// Called by the VM with its env; `token` a `long[]` of at least one element, or null.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativeBlob(
    env: *mut JNIEnv,
    _: jclass,
    core: jlong,
    handle: jbyteArray,
    token: jlongArray,
) -> jobject {
    // SAFETY: the VM's env and a byte[] (or null) it passed.
    let handle = unsafe { read(env, handle) };
    // SAFETY: handle is a live Vec for the call.
    let b = unsafe { crate::plum_blob(core_id(core), handle.as_ptr(), handle.len()) };
    if b.status != PLUM_OK {
        return std::ptr::null_mut();
    }
    let Ok(len) = jlong::try_from(b.len) else {
        crate::plum_release(b.token);
        return std::ptr::null_mut();
    };
    // SAFETY: the lent bytes stay valid until plum_release(token), which Kotlin calls when it
    // drops the buffer; Kotlin only reads them (asReadOnlyBuffer). The token array, when given,
    // has room for one long (the caller's promise) and is written in bounds.
    unsafe {
        let f = fns(env);
        if !token.is_null() && (f.GetArrayLength)(env, token) >= 1 {
            let t = jlong::try_from(b.token).unwrap_or(0);
            (f.SetLongArrayRegion)(env, token, 0, 1, &t);
        }
        let buf = (f.NewDirectByteBuffer)(env, b.ptr.cast_mut().cast::<c_void>(), len);
        if buf.is_null() {
            crate::plum_release(b.token);
        }
        buf
    }
}

/// `PlumNative.nativeRelease(token: Long): Int`: `plum_release`.
///
/// # Safety
///
/// Called by the VM.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativeRelease(_: *mut JNIEnv, _: jclass, token: jlong) -> jint {
    crate::plum_release(u64::try_from(token).unwrap_or(0))
}

/// `PlumNative.nativeLeases(): Long`: `plum_leases`.
///
/// # Safety
///
/// Called by the VM.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_plumcraft_ffi_PlumNative_nativeLeases(_: *mut JNIEnv, _: jclass) -> jlong {
    jlong::try_from(crate::plum_leases()).unwrap_or(0)
}
