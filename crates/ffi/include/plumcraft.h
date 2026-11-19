/*
 * plumcraft.h: the C ABI of plumcraft-ffi (docs/research/24-native-shells.md section 7).
 * Kept by hand; crates/ffi/tests/header.rs fails when its functions drift from the library's.
 *
 * Threads: every function may be called from any thread. A core runs on threads of its own;
 * plum_request only queues. Replies and engine events come out of plum_poll, after a wake.
 *
 * Strings and JSON are UTF-8, passed as (pointer, length), never NUL-terminated.
 * Buffers the library returns are freed with plum_free, once.
 */
#ifndef PLUMCRAFT_H
#define PLUMCRAFT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define PLUM_ABI_VERSION 1

#define PLUM_OK 0
#define PLUM_ERR_CORE (-1)    /* no such core: never opened, or closed */
#define PLUM_ERR_ARG (-2)     /* a null, empty or non-UTF-8 argument */
#define PLUM_ERR_MISSING (-3) /* no bytes for that handle; no such token */
#define PLUM_ERR_PANIC (-4)   /* a panic was caught at the boundary */
#define PLUM_ERR_UNREADABLE (-5) /* read, and not what it claims (a picture that won't decode) */
#define PLUM_PENDING 1        /* plum_blob: on its way; plum_poll's "media" says when */

/* A core, by number (0 is none). A number, not a pointer: a second close or a call after
 * close is an error code, never a use after free. */
typedef uint64_t plum_core;

/* Called on a core's thread, with no payload, at most once until the next plum_poll.
 * It must only schedule work on the shell's own thread and return; it must not call
 * back into this library. */
typedef void (*plum_wake_fn)(void *ctx);

/* A buffer made by the library: free with plum_free. ptr is NULL and len 0 when empty. */
typedef struct {
    uint8_t *ptr;
    size_t len;
    size_t cap;
} plum_buf;

/* Bytes lent by plum_blob: read only, valid until plum_release(token). */
typedef struct {
    const uint8_t *ptr;
    size_t len;
    uint64_t token;  /* 0 unless status is PLUM_OK */
    int32_t status;
} plum_bytes;

/* PLUM_ABI_VERSION of the library linked. */
uint32_t plum_abi_version(void);

/* Open a core (the session, the in-process text engine). config_json may be NULL; its fields:
 * {"store_root": path, "demo": bool}. wake may be NULL. ctx is passed to wake untouched and
 * must stay valid until plum_close returns. Returns 0 on failure: see plum_last_error. */
plum_core plum_open(const uint8_t *config_json, size_t len, plum_wake_fn wake, void *ctx);

/* Why the last plum_open on this thread failed (UTF-8 text). */
plum_buf plum_last_error(void);

/* Close a core: cancel its run, stop its threads. Waits for the core thread, except when
 * called from wake. Lent bytes stay valid until released. PLUM_OK or PLUM_ERR_CORE. */
int32_t plum_close(plum_core core);

/* Queue a control-protocol request, {"id"?, "method", "params"?} (copied). Returns the
 * request's number, which its reply carries as "id" (the caller's id comes back as "tag"),
 * or 0 when the core isn't open. Bad JSON gets an error reply, not a 0. */
uint64_t plum_request(plum_core core, const uint8_t *json, size_t len);

/* Everything since the last poll: {"replies": [{id, ok, result | error, tag?}],
 * "events": [engine event envelopes], "dropped": n}, with "asks" and "asksEnded" when the core
 * asked the shell something, and "media": [{key, handle, want, status, w?, h?, len?, format?,
 * reason?}] when values asked for (media.get, a pending plum_blob) arrived. Empty when there is
 * nothing. */
plum_buf plum_poll(plum_core core);

/* Free a buffer from plum_poll or plum_last_error. An empty buffer is ignored. */
void plum_free(plum_buf buf);

/* Lend a store blob's bytes by handle ("md5:raw:..."), without a copy, or what media.get made
 * by its "key" ("derive:rgba:<handle>", "derive:image:<handle>@<px>", ...: RGBA8, straight
 * alpha, w and h from media.get or the feed). Never waits for an engine: PLUM_PENDING while the
 * bytes are on their way, and plum_poll's "media" says when they are here. */
plum_bytes plum_blob(plum_core core, const uint8_t *handle, size_t len);

/* Give lent bytes back. PLUM_OK or PLUM_ERR_MISSING. Allowed after plum_close. */
int32_t plum_release(uint64_t token);

/* How many blobs are lent and not released. */
uint64_t plum_leases(void);

#ifdef __cplusplus
}
#endif

#endif /* PLUMCRAFT_H */
