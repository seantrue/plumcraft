# 03. The data plane: CAS handles, local mmap, remote upload and download, checkpoint reuse

Date: 2026-10-27. Branch `generic-base-research`. Builds on `docs/research/flocraft_effort_20261017.md` (working name FloCraft; prefixes below use `flo` as a placeholder until the name study settles).

## 1. Decisions in one table

| Question | Decision |
|---|---|
| What is a handle | An algorithm-tagged content digest plus a digest recipe: `md5:nd1:<32 hex>`, `b3:raw:<64 hex>`. Always shipped with typed metadata (`BlobMeta`) and zero or more locators |
| Which hash | MD5 is the default on one machine and for alexandria/commonkit interop. BLAKE3 is required wherever bytes cross a trust boundary (a shared remote CAS, more than one client). Both are first-class in v1; the engine declares which it accepts |
| What is hashed | Two recipes. `nd1`: the research software's existing array digest (dtype string, shape repr, C-order bytes), specified exactly so Rust reproduces it. `raw`: the bytes of a file, which is what alexandria's item md5 is |
| Single system | The engine publishes to the commonkit store; the UI mmaps by path; arrays never travel in messages. The in-process engine skips the filesystem and hands `Arc` buffers through the same `BlobStore` trait |
| Remote | `has` / `want` negotiation by handle, upload only what is missing, results fetched lazily by handle, previews and tiles as derived, content-addressed blobs so a remote run stays interactive |
| Checkpoint reuse | A memo record maps a memo key to output handles. Memo key = hash of node kind, node version, engine code salt, canonical params and input identities. Two lookups: a plan key (Merkle over upstream keys, known before running) and a content key (over input handles, known after) |
| Non-array data | One typed blob model with media types: arrays, text, JSON, audio, SVG/vector, opaque bytes. Each kind has a preview derivative the UI can draw |
| Retention | Four tiers: executor RAM, commonkit (shm, TTL), durable local CAS (pins, budget), remote CAS. Pins and leases are GC roots; a miss is always a slower path, never an error |

## 2. What exists today

| Piece | Where | What it does | Consequence for the design |
|---|---|---|---|
| commonkit `SharedArrayStore` | `../commonkit/src/commonkit/store.py` | Named `.npy` segments under one root; atomic `os.replace` publish; mmap attach (read-only enforced by numpy); four admission gates (min 256 KiB, max 128 MiB, free-space floor, byte budget); LRU-by-mtime + TTL reaper that only unlinks `*.npy` regular files; unlink-while-mapped is safe | The single-system data plane already exists and is right. Missing: pins, non-`.npy` blobs, a durable tier |
| commonkit keys | `../commonkit/src/commonkit/keys.py` | `{domain}:{version}:{form}:{md5}:{variant}` → `{root}/{form}/{md5[:2]}/{md5}.{variant}.npy`; tokens `[a-z0-9]+`; digest slot is exactly 32 lowercase hex; per-form dtype gate | The 32-hex slot fits MD5 or a 128-bit truncated BLAKE3, so a new scheme needs no commonkit change |
| commonkit defaults | `shared.py` | `MAX_GB` 2 on macOS (8 elsewhere), `TTL_S` 900, `MAX_ARRAY_BYTES` 128 MiB | A 15-minute TTL is right for scratch and wrong for checkpoints a designer expects tomorrow |
| `research-shm` | `crates/shm/src/lib.rs`, `npy.rs`, `map.rs` | Strict key parse; mmap; `.npy` v1/v2/v3 header parse; torn file → `Ok(None)`; oversize file → error; zero-copy `f32`/`f64` views | Keep as the L0 reader. Generalise from "parse a commonkit key" to "resolve a locator" |
| Python publish | `python/src/research_engine/publish.py` | `md5(dtype.str + repr(shape) + bytes)`; scheme `impc:v1` forms `plumarr`, `impcprev`, variant `c`; falls back to `~/Library/Caches/research/arrays/<form>/<d[:2]>/<d>.npy` when commonkit declines | This digest becomes recipe `nd1`. The fallback becomes the durable tier |
| Executor memo | `python/src/research_engine/executor.py:670` | `sha1(json.dumps([kind, version, plain, identity], sort_keys, default=_jsonable))`; identity = upstream memo + port (+ slice): a Merkle chain; RAM LRU of 1.5 GB | Right shape. Two weaknesses: `default=repr` is Python-specific (a Rust engine cannot reproduce it), and the chain never reuses a result across two graphs that produce the same input bytes |
| The research software checkpoints | `../research/src/research/core/checkpoint_store.py` | Scheme `research:v1`; digest slot = picture md5; variant = sha1 of (method, args, salt); salt = settings digest + `code_version()` (sha1 over package, yapocis, kernels); the research software's native engine keeps its own salt (`rust:research-0.1.0+<code digest>`, `docs/shared-renderer-plan.md`) | Engine build salt already exists in both engines; the base adopts it as `engine.salt` |
| alexandria CAS | `../alexandria/alexandria/storage/cas.py`, `materialize.py` | `<url_base><md5[:2]>/<md5>.<ext>`, md5 of file bytes is the item pk; any UPath backend (`file://`, `rclone://a remote server/...`, `s3://`); idempotent `store`; `verify()` rehashes; derivatives under `derivatives/<key>`; negative-lookup memo (remote misses cost about 285 ms); remote reads materialise into a digest-keyed local cache, so hits are unconditional | The remote store is this model: a `url_base`, sharded digests, idempotent put, derivatives, a miss memo. There is no has/want upload negotiation in alexandria (it hashes after upload), so that part is new |

## 3. The model

Rust sketch for `crates/schema` (L0, serde only). Every field is parsed strictly; nothing here panics.

```rust
/// Content digest algorithm. Closed set: an unknown tag is a parse error.
pub enum Algo { Md5, Blake3 }          // wire tags "md5", "b3"

/// How the digested bytes were formed from the value.
pub enum Recipe {
    Raw,   // "raw": the bytes of the blob as stored (files, text, JSON, SVG, encoded audio)
    Nd1,   // "nd1": dtype.str + shape repr + C-order bytes (the research software's publish.digest)
}

/// The identity of an immutable value. Display/parse form: "md5:nd1:3f2a…".
pub struct Handle { pub algo: Algo, pub recipe: Recipe, digest: [u8; 32], len: u8 }

/// What the bytes are. Media type plus kind-specific facts the UI needs without opening them.
pub struct BlobMeta {
    pub media: String,                 // "application/x-npy", "text/plain; charset=utf-8", "audio/x-pcm", ...
    pub size: u64,                     // bytes of the stored payload
    pub kind: BlobKind,
    pub aliases: Vec<Handle>,          // the same bytes under the other algorithm (md5 <-> b3), when known
}

pub enum BlobKind {
    Array { dtype: String, shape: Vec<u64>, space: Option<String>, names: Vec<String>, table: bool },
    Text  { lines: u64 },
    Json,
    Audio { sample_rate: u32, channels: u16, frames: u64, dtype: String },   // stored as an (frames, channels) array
    Vector { width: f64, height: f64 },                                      // SVG or another vector format
    Image { width: u32, height: u32 },                                       // an encoded file (PNG, JPEG): a source
    Bytes,
}

/// Where the bytes can be read. Several may be offered; the reader picks the cheapest it can use.
pub enum Locator {
    Commonkit { key: String },                         // resolve under the reader's own store root
    Path { path: PathBuf, offset: u64 },               // same host only; offset for a payload inside a .npy
    Memory { slot: u64 },                              // in-process engine only: an Arc<Bytes> registry slot
    Http { url: String, expires: Option<f64> },        // remote CAS or a signed URL
}

/// What travels in every message instead of data.
pub struct BlobRef { pub handle: Handle, pub meta: BlobMeta, pub at: Vec<Locator> }
```

Rules:

- **Arrays never travel in messages,** on any transport, however small. Scalars, settings and curves stay JSON as today. A `BlobRef` is the only way a value with bytes is named.
- `BlobRef` replaces today's `{key, path}` pairs in `NodeReport.outputs`, `thumb`, `display.previews`, `display.array` and the status record's `outputs`. The existing pair maps one-to-one: `key` → `Locator::Commonkit`, `path` → `Locator::Path`.
- The handle is identity; locators are hints. Two refs with one handle are the same value whatever their locators say, which is what lets the UI cache, compare and pin by handle.
- A handle's text form is `<algo>:<recipe>:<hex>`, lowercase, exact length (32 hex for MD5, 64 for BLAKE3). It is short enough for status records, workflow files and logs.

## 4. MD5 or BLAKE3

The argument, honestly put:

- **For MD5:** commonkit's key slot, the research software's checkpoint store, the research software's `publish.digest`, alexandria's item pks and its whole rclone CAS on a remote server are MD5. A source the maintainer already has in alexandria is addressable with no rehash. One machine and one user cannot attack themselves, and accidental MD5 collisions are not a practical concern.
- **Against MD5:** chosen-prefix collisions are cheap. On a CAS that several clients write, a client can upload a crafted blob whose MD5 equals another user's future input and poison their results. Server-side rehashing does not help: the collision is real. It is also not fast: one core manages very roughly 0.6–0.8 GB/s, so a 24-megapixel float32 RGB intermediate (about 290 MB) costs several hundred milliseconds to name, and the research software names every published array. (Measure on the development machine before relying on these figures; `claude-debug/hash_bench_*` would be the place.)
- **For BLAKE3:** collision-resistant, and multi-threaded and SIMD-fast (several GB/s on Apple Silicon), so it is cheaper than MD5 for large arrays. Pure-Rust crate and a Python wheel.
- **SHA-256:** collision-resistant and universal, but slower than both on large inputs without hardware help. No reason to prefer it here except an external standard requiring it.

So: **algorithm-tagged handles, both algorithms in v1.**

1. Single system: MD5 by default, so commonkit keys, the research software checkpoints and alexandria items line up with no conversion. An engine may choose BLAKE3 for large arrays to cut hashing time; it then publishes under a BLAKE3 commonkit scheme whose 32-hex digest slot holds the first 128 bits (a BLAKE3 truncation keeps its collision resistance at 64-bit security, which is ample for one store), with the full 256-bit digest in the handle.
2. Remote: the server requires `b3` for anything a client uploads into a shared namespace. It keeps an alias index `md5 → b3` populated only from bytes it hashed itself, so a client can still ask "do you have alexandria item `md5:raw:…`?" and get back a `b3` handle. A single-tenant remote (the maintainer's a remote server) may accept MD5 by configuration.
3. The engine declares `data_plane.algos: ["md5", "b3"]` in its Schema reply and the UI uses the first it also supports.

## 5. Digest recipes, exactly

Both engines must produce the same handle for the same value, so the recipes are byte-level specifications, with a shared test vector file (`crates/schema/fixtures/handles.json`, checked by Rust and pytest).

**`nd1`** (identical to `publish.digest` today, so existing `impc` keys stay valid):

1. `dtype.str` as ASCII: byte order char + kind + itemsize, for example `<f4`, `|u1`, `<i2`. Big-endian arrays are byte-swapped to little-endian before hashing, so `>f4` never appears (a new constraint: today's Python publishes whatever it gets; the engine adapter normalises first).
2. Python's tuple repr of the shape as ASCII: `()` for 0-d, `(5,)` for 1-d, `(812, 808, 3)` otherwise (`", "` separator, trailing comma only for one element).
3. The C-order element bytes.

**`raw`:** the bytes as stored, nothing else. This is alexandria's item digest. Text is hashed as its UTF-8 bytes; JSON as the bytes written (writers use RFC 8785 canonical JSON when the value is meant to dedupe).

A `.npy` file's own bytes are deliberately **not** the array identity: numpy pads the header differently across versions (the shm tests note numpy 2.x pads to 128 bytes), so the same array would get two names.

## 6. The typed blob model

Every kind is stored as something mmap-able, and every kind has a preview the UI can draw without understanding the full payload. This is what generalises node visualisation beyond images (the viewer study owns the widgets; this note owns what they read).

| Kind | Stored as | Recipe | Thumb (node, about 160 px) | Inspect preview | Full access |
|---|---|---|---|---|---|
| Array (image, field, table) | `.npy` | `nd1` | rgba8 `.npy`, as today | rgba8 ≤ 640 px plus histograms, channel ranges | mmap; tiles and regions remotely |
| Image (encoded source file) | the file | `raw` | rgba8 `.npy` | rgba8 ≤ 640 px | decoded on demand into the `pixels` commonkit domain (`pixels:v3:pil:<md5>:o1`), which alexandria's workers may already hold |
| Text | UTF-8 bytes | `raw` | first lines as a `Text` summary in the record (plain JSON, it is not an array) | first 64 KiB, line count, encoding verdict | byte or line ranges |
| JSON | canonical JSON bytes | `raw` | top-level keys and sizes | pretty-printed, depth- and length-limited | ranges; a JSON-pointer query endpoint remotely |
| Audio | `(frames, channels)` int16 or float32 `.npy`, with `sample_rate` in meta | `nd1` | peaks: `(buckets, channels, 2)` min/max `.npy`, 512 buckets | peaks at 4096 buckets plus a spectrogram as rgba8 | mmap; frame ranges remotely; a playback buffer by range |
| Vector / SVG | the SVG bytes | `raw` | an rgba8 rasterisation made by the engine | rasterisation at ≤ 640 px plus the element count | the SVG itself, rendered in the UI with `resvg` (pure Rust) under the limits in section 11 |
| Bytes | the bytes | `raw` | size and a sniffed media type | hex dump of the first 4 KiB | ranges |

**Non-`.npy` blobs in commonkit.** commonkit stores, gates and reaps only `.npy`. Two ways in:

- **Now, with no change:** store bytes as a 1-d `|u1` array. A 128-byte header buys mmap, admission, atomic publish and the reaper for free; `Locator::Path { offset }` lets a tool read the payload in place.
- **Later, fixed at source (golden rule 7):** propose a commonkit `raw` segment kind (`{digest}.{variant}.bin` plus a tiny header sidecar, reaped like `.npy`) so ffmpeg, a browser or an artcraft app can open the file as itself. Until then, `materialize` copies the payload out once into the durable tier under `<shard>/<digest>.<ext>`, alexandria's layout.

Derivatives (thumbs, previews, peaks, tiles, rasterisations) are themselves blobs with their own handles. Their identity is a **derivation key**: `hash(source handle, derivative spec, deriver version)`, as alexandria names `derivatives/<key>`. That makes a remote preview cacheable forever by URL and lets two clients share it.

## 7. Single system: zero-copy resolution

```rust
/// One trait for every engine kind. Implemented by the UI's resolver (L0/L1, no egui).
pub trait BlobStore: Send + Sync {
    fn view(&self, r: &BlobRef) -> Result<Option<BlobView>, DataError>;   // Ok(None) = miss: show stale, ask to re-run
    fn has(&self, hs: &[Handle]) -> Result<Vec<bool>, DataError>;
    fn put(&self, meta: BlobMeta, bytes: BlobSource) -> Result<BlobRef, DataError>;  // UI-side sources (load, paint)
    fn pin(&self, h: &Handle, why: PinReason) -> Result<(), DataError>;
}

/// Bytes the UI can borrow without copying: an mmap'd .npy payload or an Arc from the in-process engine.
pub enum BlobView { Mapped(NpyView), Shared(Arc<Bytes>, BlobMeta) }
```

Resolution order for a `BlobRef`, cheapest first:

1. `Memory` (in-process engine only): the engine's output buffer, shared by `Arc`. Nothing is written to disk until something outside the process needs it (a pipekit worker, a saved workflow, a pin). This is how the compiled-in engine stays at least as fast as today.
2. `Commonkit { key }` resolved under **the reader's** store root, then `Path`. Both are `mmap` through `research-shm`; `as_f32` stays zero-copy.
3. The durable local CAS (section 10) by handle.
4. `Http`: fetch, verify, publish into the local store, then mmap (section 8). After the first fetch, step 2 or 3 hits.

**Same host or not.** Today the research software assumes the engine shares its filesystem. The base proves it at connect time instead: the hello carries `host_id` (the platform UUID) and the store root, and the UI writes a nonce file into the store that the engine must read back. Passing means `Commonkit`/`Path` locators are honoured; failing means the session runs in remote mode even on a LAN, so a Docker or VM engine on the same Mac just works.

## 8. Remote: upload sources, download results

The remote data plane is a small HTTP API beside whatever control transport the engine uses (AMQP, HTTP, gRPC; other notes choose). It is HTTP because ranges, ETags, immutable caching, resumable uploads and proxies come for free, and because a pipekit engine on a remote server can serve it from the same CAS root it already writes. Every path is mirrored as a gRPC/Connect method so a gRPC transport needs no second design.

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/cas/has` | Body `{"handles": [...]}` (≤ 10,000). Reply `{"have": [...], "missing": [...], "aliases": {md5: b3}, "lease_s": 600}`. Every "have" holds a lease (section 10) so GC cannot remove it between the answer and the run |
| `PUT` | `/cas/blob/{algo}/{recipe}/{hex}` | Upload one blob. Headers: `Content-Type`, `Content-Length` (required), `Flo-Meta` (JSON `BlobMeta`). Streaming: the server hashes as it writes to a temp file and renames only on a match; mismatch → `422`. Idempotent: an existing blob → `200` without reading the body (`Expect: 100-continue`) |
| `PATCH` | `/cas/upload/{id}` | Resumable chunks for large sources (tus-style offsets), finalised against the declared handle |
| `GET` | `/cas/blob/{algo}/{recipe}/{hex}` | Download. `Range` supported; `ETag` = handle; `Cache-Control: immutable` |
| `GET` | `/cas/derived/{handle}?spec=...` | A derivative: `thumb:160`, `preview:640:auto`, `tile:z/x/y`, `region:x,y,w,h` (raw values, for the pixel probe and curves), `peaks:4096`, `frames:a-b`, `lines:a-b`, `raster:640`. Replies with the derivative's own handle in `Flo-Handle`, so a second request is a plain blob GET |
| `POST` | `/memo/lookup` | Body `{"keys": [...]}`. Reply: memo records (section 9) whose outputs the server still has |

**A remote run, step by step:**

1. The UI hashes its sources (files the user loaded, painted masks, edits) and asks `has`. It uploads only the missing ones, in parallel, largest last so small inputs unblock planning sooner.
2. The run request names sources by handle. The engine never sees a client path.
3. The engine plans. Nodes whose plan key hits the memo are reported `cached` before anything runs (as `Plan` does today).
4. As nodes finish, status records carry `BlobRef`s with `Http` locators for the output, its thumb and (for inspected ports) its preview. The engine makes thumbs and previews eagerly for exactly the nodes it reports today; nothing else is fetched unless asked.
5. The UI's fetcher downloads by priority: thumbs of visible nodes, the inspected node's preview, then nothing. Full resolution is fetched only when the user zooms past preview resolution (tiles), probes a pixel (a `region`), plays audio (`frames`) or saves (the whole blob, into the durable tier). Results are therefore **downloaded lazily by handle**.
6. Anything fetched is verified against its handle and published into the local commonkit store, so the rest of the UI takes the same mmap path it takes locally.

**Keeping a remote run interactive (data in flight):**

- Previews on the wire are encoded: lossless PNG or QOI for previews and tiles, optionally WebP/JPEG for thumbs (`preview_codec` negotiated). Locally they stay rgba8 `.npy`, as today.
- `progress` records may carry a progressive preview handle for long nodes (the research software's commands stage, Perona–Malik iterations). The server coalesces per node, latest wins, at most every 250 ms remotely (100 ms locally, as today).
- The client advertises `preview_budget_kbps`; the server drops superseded previews rather than queueing them, so a slow link shows the newest state, never a backlog.
- Status records stay small (handles only), so subscribing a watcher to `run.#` remains cheap, exactly as `docs/protocol.md` promises for AMQP.

## 9. Checkpoint and memo keys

A **memo record** separates "what was computed" from "where the bytes are":

```json
{"memo": "b3:…", "kind": "vi.cp.pencil", "version": "3", "salt": "py:research+1a2b…",
 "outputs": {"value": {"handle": "md5:nd1:…", "meta": {…}}, "settings": {"json": {…}}},
 "ui": {…}, "counts": {"value": [4]}, "ms": 412.0, "created": 1792170000.1, "scope": "global"}
```

**Memo key** = BLAKE3 of the RFC 8785 canonical JSON of:

```json
["flo-memo-v1", kind, node_version, engine_salt, device_class_or_null, params, inputs]
```

- `engine_salt` is the engine's declared code digest (Python: the research software's `code_version()` over the package, yapocis and kernels; Rust: `rust:research-0.1.0+<build.rs digest>`). A build whose code differs never reads another's results.
- `device_class` is present only when the node declares `device_sensitive` (OpenCL kernels whose bits differ by GPU). An engine that attests bit-exactness across devices omits it, and only then are results shared across machines.
- `params` are the resolved non-wired values. Canonical JSON replaces `json.dumps(default=repr)`: floats in shortest round-trip form, which Rust (`ryu`) and Python (`repr`) both produce; non-JSON values are refused, never `repr`'d. Provenance is excluded, as rule 8 of `docs/engine-contract.md` requires.
- `inputs` maps each port to an identity (below), including any wire slice.

**Two identities per input, two lookups:**

| Key | Input identity | Known | Use |
|---|---|---|---|
| Plan key | upstream plan key + port (+ slice); leaves use the source's content handle | before running | `cached` status at plan time; remote skip of whole subgraphs; the executor's existing Merkle chain, made portable |
| Content key | the input's content handle | after the upstream node ran | reuse when different graphs (or a Python and a Rust engine) produced identical bytes upstream |

The executor tries the plan key, then the content key. A hit on either writes the other as an alias, so the next plan hits early. Leaves are keyed by content, never by path, so `test.jpg` loaded from two folders, or uploaded from two machines, is one source.

**Memo scopes,** declared per node in the Schema: `none` (time, network, unseeded random, an artcraft MCP call whose app state is not captured), `session`, `machine`, `global`. The default is `machine`; `global` needs the engine's cross-device attestation. An MCP-backed engine marks a call `global` only when its inputs fully determine its result (for example a soundcraft render from a document handle).

**Exactness for free:** the exactness gate becomes handle equality. A plugin exported from a group and the group itself must produce the same output handles; Python and the research software's native engine must too.

## 10. Tiers, eviction, pinning, GC

| Tier | Where | Holds | Eviction |
|---|---|---|---|
| T0 executor RAM | engine process | recent `NodeResult`s by memo key (today 1.5 GB LRU) | LRU by bytes |
| T1 commonkit | `~/Library/Caches/commonkit/store`, `/dev/shm/commonkit` | everything published for the UI, in flight | commonkit's reaper: TTL 900 s, budget, free-space floor. Unchanged |
| T2 durable local CAS | `~/Library/Caches/<app>/cas/<shard>/<digest>.<ext>` plus `memo/<shard>/<memo>.json` | pinned values, memo records, materialised raw blobs, downloads the user saved | budgeted LRU (default 20 GB), never touching pins or leases |
| T3 remote CAS | a `url_base` as alexandria's (`file://`, `rclone://a remote server/…`, `s3://`) | uploaded sources, results, derivatives, memo records | mark-and-sweep from roots, after a grace period |

**Pins and roots.**

- Pinned: sources referenced by an open or recently saved workflow, outputs the user marked "keep" (frozen checkpoints), edits and masks (they are user work, not recomputable), and every blob a delivered plugin's tests reference.
- A pin is a row in T2's index (`pins.json`, atomic replace) naming the handle and why. Pinning copies the blob out of T1 into T2 if needed, because the T1 reaper may unlink it at any time.
- Leases are time-bound pins (default 10 minutes, renewed while a run is live). `has` grants them, uploads hold them until finalised, and a running job holds them on its inputs. They close the race between "the server said it has it" and "GC deleted it".
- GC on T2 and T3: roots = pins ∪ live leases ∪ memo records younger than the retention window; sweep unreferenced blobs older than the grace period; memo records whose outputs are gone are dropped, not trusted.

**Proposed at source:** commonkit gets an optional `keep` list (or a `pinned/` subtree the reaper skips, under its own budget), so a pinned segment can stay mmap-able in T1 instead of being copied to T2. Until then T2 is the pin tier, and the resolver in section 7 already checks it.

**The invariant from commonkit, kept everywhere:** a missing blob is never an error, only a slower path. A reaped preview greys out and marks the node stale; a memo hit whose blob is gone is a miss; a remote 404 is a recompute.

## 11. Integrity and hostile input (golden rule 1)

Engine replies, status records, memo records, workflow files, `.npy` headers, downloaded bytes and every remote response are hostile.

| Input | Defence |
|---|---|
| Handle text | Closed algo and recipe sets; exact hex length; lowercase only; parsed before use. Paths are composed only from a parsed handle or commonkit key (as `Key::parse` already does), never from a string in a message |
| `Locator::Path` | Honoured only after the same-host check, and only under the store root, the durable CAS root or the engine's declared work root (canonicalised, no symlink escape). Anything else is ignored and the handle is resolved another way |
| `.npy` header | As `research-shm` today: magic, version, header ≤ 64 KiB, dtype whitelist, shape product by `checked_mul`, file length exactly equal to header + data. Torn → miss |
| Size | Every `BlobMeta.size` is checked against the bytes; per-blob caps (commonkit's 128 MiB in T1, configurable in T2/T3, default 4 GiB) and per-session totals; `Content-Length` required on upload |
| Downloads | Hash while streaming into a temp file; rename into the store only on a match, so an unverified byte is never mmap'd. Mismatch → discard, mark the remote suspect, recompute |
| Uploads (server) | Same streaming verification; temp files outside the CAS; quotas per client; `has` batch cap; no path in any request; BLAKE3 required in shared namespaces (section 4) |
| Memo records from elsewhere | Outputs must exist and their meta must agree with the stored blob's header; salt and version must match the running engine exactly; optional audit mode recomputes a sample (say 1%) and compares handles |
| Encoded images and previews | Decode with dimension limits checked before allocation (`image` crate `Limits`); refuse more than 16,384 px a side or 256 Mpx |
| SVG | `resvg`/`usvg` with external resources disabled, no `<image href>` to files or URLs, element and depth limits, raster size cap |
| Text and JSON | UTF-8 validated (lossy display, never trusted for paths); JSON depth ≤ 64 and size caps before parse |
| Audio | Sample rate 1–768,000, channels 1–64, frames by `checked_mul` against size |
| Local store | Written only by atomic rename; scrub on demand (`data.verify`, as alexandria's `verify()`) rehashes T2 and reports corruption as a state, never a panic |

## 12. The four engine kinds on one data plane

| Engine | Locators it emits | Notes |
|---|---|---|
| Compiled-in Rust | `Memory`, plus `Commonkit` when it publishes (pins, other processes) | No serialisation, no hashing until a handle is needed. A derivation-addressed handle (`drv`, from memo key + port) may stand in until the content hash is computed lazily, so in-flight viewing is never waiting on MD5 |
| Separate-process Rust | `Commonkit`, `Path` | Same host proven at connect; the research software's native engine already writes the commonkit store |
| Separate-process HTTP | `Http` always; `Commonkit`/`Path` too when the same-host check passes | The data plane API of section 8 is its native form |
| Python / pipekit (AMQP) | `Commonkit`, `Path`; `Http` when the deployment runs the CAS API (a remote server) | The existing the research software engine: `publish.py` becomes `blob.publish()` returning a `BlobRef`; `impc` keys remain valid because `nd1` is today's digest |

## 13. Nothing the research software does today is lost

| Today | On the new data plane |
|---|---|
| `thumb` on every computed node, sent as a `preview` record the moment it finishes | Same records, same timing; the thumb is a `BlobRef`; remote adds encoding and coalescing |
| Inspect at viewer size, histograms, channel ranges, sheet of up to 64 items | Same `display` payload with `BlobRef`s; the sheet's 256 px items are derivatives |
| Full-resolution mmap for zoom, pixel probe and the curves live preview | `BlobView::Mapped` locally, unchanged zero copy; remotely `tile` and `region` derivatives, then the full blob on demand |
| commonkit decline → fallback `.npy` path | The T2 durable tier, under the same `Path` locator |
| Reaped segment → preview greys out, node stale | Identical; plus pins so it need not happen to what the user cares about |
| Executor memo and commonkit-cached `Plan` states | Plan key = today's Merkle chain made portable; content key adds reuse; `cached` records as today |
| The research software checkpoint store (`research` scheme, salted) | Untouched; it lives in the same commonkit store. The base's memo sits above it |
| Status tracking (`queued`, `started`, `progress`, `preview`, `done`, `cached`, `error`, `cancelled`) | Unchanged states; records carry handles only, so any transport (callback, poll, AMQP topic, SSE, gRPC stream) can carry them |

## 14. Proposed changes at source

| Repo | Change | Why |
|---|---|---|
| commonkit | `keep` list or `pinned/` subtree skipped by the reaper | Pins in T1 without copying |
| commonkit | A `raw` segment kind (`.bin` plus header sidecar) | Non-array blobs readable by external tools in place |
| commonkit | Optional BLAKE3 scheme helper (128-bit truncation in the digest slot) | Faster naming of large arrays |
| The research software | `checkpoint_store.settings_digest` and `variant` on canonical JSON instead of `repr` | So the research software's native engine and Python can share checkpoints (today each has its own salt) |
| The research software Python | `_memo` on canonical JSON; refuse non-JSON params instead of `repr`; `nd1` normalises byte order | Portable memo keys across engines |

## 15. Open questions for the maintainer

1. Should a remote engine on a remote server write results into alexandria's existing rclone CAS (one `url_base`, shared derivatives) or into its own root? Sharing gives free reuse of decoded pixels and thumbs; separating keeps alexandria's GC simple.
2. Is a 10-minute lease and 20 GB T2 default right for the development machine, given unified memory and the 2 GB commonkit budget on macOS?
3. Is cross-device sharing of the research software results wanted? It needs an attestation that OpenCL output is bit-identical on the M4 and on a remote server; until then results are `machine` scope.
4. Upload policy for big sources: hash the whole file first (one extra read) or stream-hash during upload and accept wasted bandwidth when the server turns out to have it? The design above hashes first.

## 16. Effort

| Work | Days |
|---|---|
| `Handle`, `BlobMeta`, `BlobRef`, `Locator` in `schema`, with shared test vectors (Rust and pytest) | 1.5 |
| `BlobStore` trait; resolver over Memory, commonkit, T2; same-host proof | 2 |
| Python `blob.publish` and memo record writer; canonical-JSON memo keys | 1.5 |
| T2 durable CAS with pins, leases and budgeted LRU; scrub | 2 |
| Typed kinds: text, JSON, audio peaks, SVG raster; their previews | 2.5 |
| Remote CAS service (has, put, get with ranges, derived, memo lookup) and client fetcher with priorities and verification | 4 |
| Hostile-input tests and a `panic_hunt` pass over the data-plane commands | 1.5 |
| **Total** | **about 15 days**, overlapping the 30–40 day extraction where the schema and bridge work coincide |
