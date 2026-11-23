/*
 * plumcraft.h: the C ABI of plumcraft-ffi (docs/research/24-native-shells.md section 7).
 * Kept by hand; crates/ffi/tests/header.rs fails when its functions drift from the library's.
 *
 * Threads: every function may be called from any thread. A core runs on threads of its own;
 * plum_request only queues. Replies and engine events come out of plum_poll, after a wake.
 *
 * Strings and JSON are UTF-8, passed as (pointer, length), never NUL-terminated.
 * Buffers the library returns are freed with plum_free, once.
 *
 * An application with engines of its own links a library of its own built around this one: every
 * function here, plus an entry of the application's that registers its engines (Rust:
 * plumcraft_ffi::app::register), called before the first plum_open (docs/shell-interface.md 4.5).
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

/* Open a core and its first document (the in-process text engine by default). config_json may
 * be NULL; its fields: {"store_root": path, "demo": bool, ...}. wake may be NULL. ctx is passed to
 * wake untouched and must stay valid until plum_close returns. Returns at once: the engine
 * connects on a worker, requests meanwhile are held and answered once it has, and plum_poll's
 * "changed" says when ({doc, domain: "engine", ready, ...}). Returns 0 on a bad config: see
 * plum_last_error. */
plum_core plum_open(const uint8_t *config_json, size_t len, plum_wake_fn wake, void *ctx);

/* Why the last plum_open on this thread failed (UTF-8 text). */
plum_buf plum_last_error(void);

/* Close a core: close its documents (their runs cancelled and engines stopped on their own
 * workers, which nothing waits for), stop its threads. Waits for the core thread, except when
 * called from wake. Lent bytes stay valid until released. PLUM_OK or PLUM_ERR_CORE. */
int32_t plum_close(plum_core core);

/* Queue a control-protocol request, {"id"?, "method", "params"?, "doc"?} (copied; without "doc",
 * for the first document open; document.open, document.list and document.close manage them). Returns the
 * request's number, which its reply carries as "id" (the caller's id comes back as "tag"),
 * or 0 when the core isn't open. Bad JSON gets an error reply, not a 0. */
uint64_t plum_request(plum_core core, const uint8_t *json, size_t len);

/* Everything since the last poll: {"replies": [{id, ok, result | error, tag?}],
 * "events": [engine event envelopes], "dropped": n}, with "asks" and "asksEnded" when the core
 * asked the shell something, and "media": [{key, handle, want, status, w?, h?, len?, format?,
 * reason?}] when values asked for (media.get, a pending plum_blob) arrived, and "changed":
 * [{doc, domain, ...}] when a document's engine connected or the document closed. Each event and
 * each ask raised names its "doc". Empty when there is nothing. */
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

/* ---------- hot calls: typed, synchronous, no JSON (docs/shell-interface.md section 9) ----------
 * They read the document's latest snapshot (its published scene with the geometry the core
 * has) without waiting for the core thread, from any thread; they may be one publish behind a
 * request just sent. A document is named by its number (0: the first open). Coordinates and
 * distances are graph units (the shell divides its screen slop by its zoom). Faces, wires,
 * nets, frames and groups are named by the scene's nid: one number per item for the document's
 * life, never two items with the same number, whatever their kinds. The same queries are JSON
 * methods too: geometry.hit, geometry.wireTarget, geometry.wirePaths, the command geometry.set,
 * and ui.gesture's drag move for plum_gesture_move. */

/* What is at a point, or where a dragged wire may land.
 * kind: 0 none, 1 node (part: 1 body, 2 header, 3 title, 4 chevron, 5 icon, 6 menu, 7 enter,
 *       8 row (index), 9 preview, 10 footer item (index)), 2 pin (part: 1 input, 2 output;
 *       node, pin), 3 wire (wire), 4 bend point (wire, index), 5 net tag (part: 1 driver,
 *       item the net; 2 use, wire), 6 frame (part: 1 body, 2 tab, 3 handle; item), 7 open
 *       group (part 1 band; item).
 * status: PLUM_OK; PLUM_ERR_CORE; PLUM_ERR_MISSING (no such document, or no scene published
 *       for it yet: scene.get or plum_geometry_set first); PLUM_ERR_ARG; PLUM_ERR_PANIC. */
typedef struct {
    int32_t status;
    uint32_t kind;
    uint32_t part;
    int32_t index;   /* a row, footer item or bend point; -1 none */
    uint64_t item;   /* the nid of what was hit (face, wire, net, frame or group, by kind) */
    uint64_t node;   /* the face (kinds 1 and 2; a tag's face) */
    uint64_t wire;   /* the wire (kinds 3 and 4; a use's tag) */
    uint32_t pin;    /* the pin's index on its side (kind 2) */
    uint32_t fits;   /* plum_wire_target: 0 doesn't fit, 1 wire.connect takes it, 2 only through a Convert */
    float x, y;      /* the pin's point (kind 2: where a wire snaps), else the point asked */
} plum_hit;

/* One face's measurements for plum_geometry_set (graph units, at zoom 1). */
typedef struct {
    uint64_t face;          /* the face's nid */
    uint64_t rev;           /* the face's rev measured; 0: not said */
    float w, h;
    float header;           /* the header's height; NaN or below 0: not said */
    const float *inputs;    /* each input pin's height from the top, row order (NaN: not measured) */
    size_t n_inputs;
    const float *outputs;
    size_t n_outputs;
    const float *rows;      /* 4 floats an input row: its widget rect (x0, y0, x1, y1), NaN x0: none */
    size_t n_rows;          /* rows, not floats */
} plum_face;

/* What is at (x, y) within slop: a pin first, then a face and its part (the last drawn on top),
 * a bend point, a net tag, a frame's tab or handle, a wire, an open group's band, a frame. */
plum_hit plum_hit_test(plum_core core, uint64_t doc, float x, float y, float slop);
/* Where a wire dragged from pin `pin` of face `face` (side 1 an input, 2 an output) may land at
 * (x, y): the nearest pin on the other side within slop that fits, else the nearest that
 * doesn't (fits 0), else kind 0. PLUM_ERR_ARG for another side. */
plum_hit plum_wire_target(plum_core core, uint64_t doc, uint64_t face, uint32_t side, uint32_t pin, float x, float y, float slop);
/* The paths of every wire with an end on one of the n faces (nids), as if they were moved by
 * (dx, dy): per wire its nid's low and high 32 bits and its point count (each a uint32_t
 * bit-cast into a float: three floats), then the points as x, y (a cubic chain: the start, then
 * three points a segment).
 * Returns the floats needed; nothing is written when that is more than cap or out is NULL.
 * 0 for a core or document that isn't there. */
size_t plum_wire_paths(plum_core core, uint64_t doc, const uint64_t *faces, size_t n, float dx, float dy, float *out, size_t cap);
/* What the shell measured of n faces: geometry.set, typed. Copied before it returns, applied on
 * the core's thread; a nid the scene doesn't have is skipped. PLUM_OK, PLUM_ERR_CORE, or
 * PLUM_ERR_ARG (NULL faces with n > 0, more than 100000 faces, more than 4096 pins or rows a
 * face, a misaligned pointer). */
int32_t plum_geometry_set(plum_core core, uint64_t doc, const plum_face *faces, size_t n);

/* Where a drag under way is (ui.gesture's drag move, typed; N2 slice 4). The drag begins, ends
 * and is cancelled with ui.gesture {kind: "drag", phase: "begin" | "end" | "cancel", at}; the
 * core decides at begin what it is, and its reply gives serial, kind and, for a move, the faces
 * moving (nids, for plum_wire_paths). Nothing changes until the end.
 * kind: 1 move (faces), 2 wire, 3 marquee, 4 bend point, 5 frame, 6 frame's size, 7 open group,
 *       8 a number scrubbed (a drag on a number's row, N4 slice 2: its value now in rect).
 * status: PLUM_OK; PLUM_ERR_CORE; PLUM_ERR_MISSING (no such document, no drag under way, no
 *       scene published); PLUM_ERR_ARG (a point that isn't a number); PLUM_ERR_PANIC. */
typedef struct {
    int32_t status;
    uint32_t kind;
    uint64_t serial;   /* the drag's number, as its begin replied */
    float dx, dy;      /* the offset from where it began */
    float rect[4];     /* a marquee's rect (x0, y0, x1, y1); a scrub's value: a double whose low and high
                          32 bits are rect[0] and rect[1] bit-cast, rect[2] it as a float; zeros otherwise */
    plum_hit target;   /* a wire's: where it would land (kind 2: a pin, x and y its point), fits */
} plum_drag;

/* The drag under way in document doc with the pointer at (x, y), within slop (graph units). */
plum_drag plum_gesture_move(plum_core core, uint64_t doc, float x, float y, float slop);

#ifdef __cplusplus
}
#endif

#endif /* PLUMCRAFT_H */
