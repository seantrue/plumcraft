package com.plumcraft.ffi

import java.nio.ByteBuffer

/**
 * The JNI shims of `crates/ffi/src/jni.rs` (feature `jni`). JSON crosses as UTF-8 byte arrays,
 * never `String`s (JNI's modified UTF-8 would mangle characters outside the BMP).
 *
 * The library is `libplumcraft_ffi` (`jniLibs/<abi>/libplumcraft_ffi.so` on Android, the host
 * build on `java.library.path` in the JVM tests).
 */
object PlumNative {
    init {
        System.loadLibrary("plumcraft_ffi")
    }

    /** `plum_open` with the library's own wake (see [nativeAwait]); 0 on failure. */
    @JvmStatic external fun nativeOpen(config: ByteArray?): Long

    /** Why the last [nativeOpen] on this thread failed. */
    @JvmStatic external fun nativeLastError(): ByteArray

    @JvmStatic external fun nativeClose(core: Long): Int

    /** Queue a request; its number, which its reply carries as `id` (0: the core isn't open). */
    @JvmStatic external fun nativeRequest(core: Long, json: ByteArray?): Long

    /** `{"replies": [...], "events": [...], "dropped": n}`, or null when there is nothing. */
    @JvmStatic external fun nativePoll(core: Long): ByteArray?

    /** Block until the core wakes (true), [timeoutMs] passes or it closes (false). Never on the main thread. */
    @JvmStatic external fun nativeAwait(core: Long, timeoutMs: Long): Boolean

    /** A direct buffer over lent bytes (no copy), its token in `token[0]`; null when there are none. */
    @JvmStatic external fun nativeBlob(core: Long, handle: ByteArray, token: LongArray): ByteBuffer?

    @JvmStatic external fun nativeRelease(token: Long): Int

    @JvmStatic external fun nativeLeases(): Long
}
