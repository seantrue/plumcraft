//! Blobs in the commonkit store (plan 10.2, D1; spec 4.4, 5.4), through the `commonkit` crate.
//!
//! - **Layout:** commonkit's. A key `plum:v1:{form}:{md5}:c` names
//!   `{root}/{form}/{md5[:2]}/{md5}.c.npy`. Arrays go under form `plumarr` keyed by their `nd1`
//!   digest; anything else (text, JSON tables) under `plumraw` as a 1-d `|u1` array keyed by its
//!   `raw` digest, so a blob has one identity (spec 4.4). The `plum` scheme is registered with
//!   commonkit on first use.
//! - **Writing** is commonkit's `Store::publish`: write-once, a temp file then `rename`, the
//!   tree's policy (`.policy.json`, the environment) for budget, free space and size, and its
//!   reap turns. Every size is taken (`min_array_bytes` 0): a blob is stored, not only big ones.
//!   A decline is normal and is `None`.
//! - **Reading** is commonkit's `attach`, bounded, and a miss is `None`, never an error (spec
//!   4.6). Files are read, except an array over [`MAX_READ`], mapped.
//! - **Compression (deflate, zlib format):** a raw blob (`plumraw`) of at least
//!   [`COMPRESS_MIN`] bytes that deflate makes at least 10% smaller is stored compressed instead,
//!   under form `plumz` keyed by the same `raw` digest: its identity is still the MD5 of the
//!   uncompressed bytes, and reading inflates and checks it. A compressed blob gets a `commonkit`
//!   locator only (a `path` reader expects the bytes as they are); arrays (`plumarr`), which the
//!   UI may read in place, are never compressed. Deflate is pure Rust here (`flate2` on
//!   `miniz_oxide`) and the standard library's `zlib` in Python, so both SDKs read each other's.
//! - **Memo records** (spec 14.5) live under form `plummemo`, keyed by the plan key's digest,
//!   always compressed: [`Store::put_record`], [`Store::read_record`].
//! - **PlumCraft's own:** locators, checking bytes against the handle, the trusted roots for a
//!   `path` locator, inline values, and the same-host proof (nonce files under
//!   `{root}/.hello/`, mode 0600).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use base64::Engine as _;
use commonkit::segment::Segment;
use commonkit::{ArrayRef, AttachMode, Descr, KeyScheme, Mode, Order, Variants};
use plumcraft_proto::data::{BlobMeta, DataRef, Form, Locator};
use plumcraft_proto::handle::{Handle, Recipe};
use plumcraft_proto::run::TreeUsage;

/// commonkit's environment override for the store root.
pub const ROOT_ENV: &str = "COMMONKIT_STORE_ROOT";
/// PlumCraft's commonkit domain and decode contract.
pub const DOMAIN: &str = "plum";
pub const VERSION: &str = "v1";
/// Forms (global in the shared tree, so PlumCraft-specific names).
pub const FORM_ARRAY: &str = "plumarr";
pub const FORM_RAW: &str = "plumraw";
/// A raw blob stored compressed (deflate, zlib format), keyed by its uncompressed `raw` digest.
pub const FORM_Z: &str = "plumz";
/// A memo record (spec 14.5), compressed, keyed by its plan key's digest.
pub const FORM_MEMO: &str = "plummemo";
/// The smallest raw blob worth compressing.
pub const COMPRESS_MIN: usize = 64 << 10;
/// How much of a blob is compressed first to see whether the rest is worth it.
const SAMPLE: usize = 64 << 10;
/// The one variant: the content itself.
const VARIANT: &str = "c";
/// The largest blob read back (commonkit's `max_array_bytes` default).
pub const MAX_READ: u64 = 128 << 20;
/// The largest array published: a 12 MP RGB float32 picture is 144 MB, past commonkit's default.
pub const MAX_ARRAY: i64 = 2 << 30;

/// The store root: `$COMMONKIT_STORE_ROOT`, else commonkit's per-platform default.
pub fn default_root() -> PathBuf {
    commonkit::policy::resolve_root(&|k| std::env::var_os(k)).unwrap_or_else(|| PathBuf::from("/tmp/commonkit"))
}

/// The commonkit tree's policy at `root` (its `.policy.json` over the environment, else the
/// platform default). Working stores kept beside the tree, such as the craft CAS, keep files no
/// longer than its TTL, so nothing PlumCraft stores outlives what the tree would keep.
pub fn tree_policy(root: &Path) -> commonkit::Policy {
    commonkit::Policy::for_root(root, &commonkit::policy::process_env)
}

/// The `plum` scheme, registered with commonkit once per process. `None` only if commonkit
/// refuses it (another `plum` scheme registered first), and then nothing is published.
fn scheme() -> Option<&'static Arc<KeyScheme>> {
    static SCHEME: OnceLock<Option<Arc<KeyScheme>>> = OnceLock::new();
    SCHEME
        .get_or_init(|| {
            let variants = Variants::closed([VARIANT]).ok()?;
            let scheme =
                KeyScheme::new(DOMAIN, VERSION, [FORM_ARRAY, FORM_RAW, FORM_Z, FORM_MEMO], variants, &[(FORM_RAW, "|u1"), (FORM_Z, "|u1"), (FORM_MEMO, "|u1")])
                    .ok()?;
            commonkit::keys::register(scheme).ok()
        })
        .as_ref()
}

/// A commonkit key for a handle.
pub fn key_for(handle: &Handle) -> String {
    let form = if handle.recipe() == Recipe::Nd1 { FORM_ARRAY } else { FORM_RAW };
    format!("{DOMAIN}:{VERSION}:{form}:{}:{VARIANT}", handle.digest())
}

/// The key a raw handle's compressed copy is stored under.
pub fn z_key_for(handle: &Handle) -> String {
    format!("{DOMAIN}:{VERSION}:{FORM_Z}:{}:{VARIANT}", handle.digest())
}

/// The key of a plan key's memo record (`memo:md5:<hex>`); `None` for anything else.
pub fn memo_key_for(plan_key: &str) -> Option<String> {
    let hex = plan_key.strip_prefix("memo:md5:").filter(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))?;
    Some(format!("{DOMAIN}:{VERSION}:{FORM_MEMO}:{hex}:{VARIANT}"))
}

/// The form of a PlumCraft key (`plum:v1:<form>:…`).
fn form_of(key: &str) -> Option<&str> {
    key.split(':').nth(2)
}

/// `bytes` compressed (deflate, zlib format, level 1), or `None` when it isn't worth it: under
/// [`COMPRESS_MIN`], over [`MAX_READ`] (a reader inflates at most that), or less than 10%
/// smaller, judged on the first [`SAMPLE`] bytes before the whole is compressed.
pub fn compress(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < COMPRESS_MIN || bytes.len() as u64 > MAX_READ {
        return None;
    }
    let worth = |raw: usize, z: usize| z * 10 <= raw * 9;
    let sample = bytes.get(..SAMPLE.min(bytes.len()))?;
    if !worth(sample.len(), deflate(sample)?.len()) {
        return None;
    }
    let z = deflate(bytes)?;
    worth(bytes.len(), z.len()).then_some(z)
}

fn deflate(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::with_capacity(bytes.len() / 4), flate2::Compression::fast());
    e.write_all(bytes).ok()?;
    e.finish().ok()
}

/// Inflate a zlib stream, at most [`MAX_READ`] bytes out; anything longer or malformed is `None`.
pub fn inflate(z: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(z).take(MAX_READ + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= MAX_READ).then_some(out)
}

/// Where a `.npy` file's payload starts (version 1.0: 10 bytes plus the header length; 2.0 and
/// 3.0: 12 plus a 4-byte length).
fn npy_offset(path: &Path) -> Option<u64> {
    let mut head = [0u8; 12];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).ok()?;
    match head.get(6)? {
        1 => Some(10 + u64::from(u16::from_le_bytes([*head.get(8)?, *head.get(9)?]))),
        2 | 3 => Some(12 + u64::from(u32::from_le_bytes([*head.get(8)?, *head.get(9)?, *head.get(10)?, *head.get(11)?]))),
        _ => None,
    }
}

/// Mark a segment used now (commonkit's LRU is the file's mtime). Best effort.
fn touch(path: &Path) {
    let _ = std::fs::File::options().append(true).open(path).and_then(|f| f.set_modified(std::time::SystemTime::now()));
}

/// The relative path a PlumCraft key names; `None` for any other key (never a path outside
/// the root).
pub fn relpath(key: &str) -> Option<PathBuf> {
    scheme()?.parse(key).ok().map(|k| k.relpath_buf())
}

/// A blob's bytes: read into memory, or for an array over [`MAX_READ`], mapped from its segment,
/// so a 12-megapixel stack costs what is looked at (spec 13.2: the UI maps what it can).
#[derive(Clone)]
pub enum Bytes {
    Owned(Vec<u8>),
    Mapped(Arc<Segment>),
}

impl std::ops::Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        match self {
            Bytes::Owned(v) => v,
            Bytes::Mapped(s) => s.bytes(),
        }
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl Bytes {
    /// As a vector: without a copy when it is one.
    pub fn into_vec(self) -> Vec<u8> {
        match self {
            Bytes::Owned(v) => v,
            Bytes::Mapped(s) => s.bytes().to_vec(),
        }
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Bytes {
        Bytes::Owned(v)
    }
}

impl std::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let how = if matches!(self, Bytes::Mapped(_)) { "mapped" } else { "owned" };
        write!(f, "{} bytes, {how}", self.len())
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Bytes) -> bool {
        **self == **other
    }
}

impl Eq for Bytes {}

/// Bytes read back from a blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    /// The `.npy` dtype and shape, for arrays; `|u1` and `(n,)` for raw bytes.
    pub dtype: String,
    pub shape: Vec<u64>,
    pub bytes: Bytes,
}

/// One store root: a commonkit store, shared by clones.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
    ck: Arc<commonkit::Store>,
    /// The same tree for an array the first declines for its size: the same budget and reaper.
    large: Arc<commonkit::Store>,
}

impl Store {
    /// A store at `root`, with the tree's policy. Never fails: an unusable root gives a store
    /// that misses and declines (commonkit's rule).
    pub fn new(root: impl Into<PathBuf>) -> Store {
        let root = root.into();
        let ck = commonkit::shared_store_at(&root, Mode::ReadWrite, AttachMode::Read, 0);
        let p = tree_policy(&root);
        let options = commonkit::Options {
            mode: Mode::ReadWrite,
            attach: AttachMode::Read,
            max_bytes: p.max_bytes,
            min_free_bytes: p.min_free_bytes,
            ttl_s: p.ttl_s,
            min_array_bytes: 0,
            max_array_bytes: MAX_ARRAY,
            reap_interval_s: p.reap_interval_s,
            fsync: p.fsync,
            verify_republish: p.verify_republish,
        };
        let large = commonkit::Store::new(&root, options);
        Store { root, ck: Arc::new(ck), large: Arc::new(large) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Publish raw bytes (`md5:raw:`), wrapped as a 1-d `|u1` array. Returns the locators to
    /// reach it, or `None` if the store can't take it (a decline is normal, as commonkit's).
    ///
    /// Bytes already here (plain or compressed) are only touched. Otherwise they are stored
    /// compressed when [`compress`] says it is worth it (a `commonkit` locator only), else as
    /// they are.
    pub fn publish_raw(&self, bytes: &[u8]) -> Option<(Handle, Vec<Locator>)> {
        let handle = Handle::md5_raw(bytes);
        if let Some(at) = self.locate(&handle) {
            return Some((handle, at));
        }
        if let Some(z) = compress(bytes) {
            let key = z_key_for(&handle);
            if self.put(&key, "|u1", &[z.len() as u64], &z) {
                return Some((handle, vec![Locator::Commonkit { key }]));
            }
        }
        let locators = self.publish(&handle, "|u1", &[bytes.len() as u64], bytes)?;
        Some((handle, locators))
    }

    /// The locators of a value already in this store, touching it (commonkit's LRU); `None`
    /// when it isn't here. Cheaper than publishing again: nothing is read or hashed.
    pub fn locate(&self, handle: &Handle) -> Option<Vec<Locator>> {
        scheme()?;
        let key = key_for(handle);
        if let Some(p) = self.ck.path_for(&key).filter(|p| p.is_file()) {
            touch(&p);
            let offset = npy_offset(&p)?;
            return Some(vec![Locator::Commonkit { key }, Locator::Path { path: p.display().to_string(), offset }]);
        }
        if handle.recipe() == Recipe::Raw {
            let key = z_key_for(handle);
            let p = self.ck.path_for(&key).filter(|p| p.is_file())?;
            touch(&p);
            return Some(vec![Locator::Commonkit { key }]);
        }
        None
    }

    /// commonkit's counters since the last call, by contract name, each taken and reset.
    pub fn drain_stats(&self) -> std::collections::BTreeMap<String, u64> {
        self.ck.drain_stats().iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    /// The tree: its size from the reaper's last walk when that is known, its budget, and the
    /// disk's free space with the floor the store refuses writes under.
    pub fn usage(&self) -> TreeUsage {
        let used = self.ck.usage();
        let policy = tree_policy(self.root());
        let bytes = |v: i64| u64::try_from(v).unwrap_or(0);
        TreeUsage {
            bytes: used.map(|u| u.bytes),
            files: used.and_then(|u| u.files),
            max_bytes: bytes(policy.max_bytes),
            free_bytes: fs4::available_space(self.root()).ok(),
            min_free_bytes: bytes(policy.min_free_bytes),
        }
    }

    /// Store a memo record (spec 14.5) for `plan_key`, compressed, replacing one already there
    /// (a record is rewritten when its node is recomputed). `false` when the store declines it.
    pub fn put_record(&self, plan_key: &str, record: &[u8]) -> bool {
        let (Some(key), Some(z)) = (memo_key_for(plan_key), deflate(record)) else { return false };
        if let Some(p) = self.ck.path_for(&key).filter(|p| p.is_file()) {
            let _ = std::fs::remove_file(p);
        }
        self.put(&key, "|u1", &[z.len() as u64], &z)
    }

    /// The memo record for `plan_key`, inflated; `None` when there is none (or it is torn).
    /// The record is untrusted input: its reader parses it defensively.
    pub fn read_record(&self, plan_key: &str) -> Option<Vec<u8>> {
        self.read_key(&memo_key_for(plan_key)?).map(|b| b.bytes.to_vec())
    }

    /// Write one segment through commonkit; `false` when it declines or fails.
    fn put(&self, key: &str, dtype: &str, shape: &[u64], bytes: &[u8]) -> bool {
        let (Some(_), Ok(descr)) = (scheme(), Descr::parse(dtype)) else { return false };
        ArrayRef::new(&descr, shape, Order::C, bytes).is_ok_and(|a| self.ck.publish(key, a) == Ok(true))
    }

    /// Publish an array (`md5:nd1:`) given its little-endian dtype, shape and C-order bytes.
    pub fn publish_array(&self, dtype: &str, shape: &[u64], bytes: &[u8]) -> Option<(Handle, Vec<Locator>)> {
        let handle = Handle::md5_nd1(dtype, shape, bytes).ok()?;
        let locators = self.publish(&handle, dtype, shape, bytes)?;
        Some((handle, locators))
    }

    /// Publish through commonkit: written, or already there (a republish only touches it).
    /// A decline, a failure or a key collision (a different array under the same digest) is
    /// `None`; the caller keeps the bytes it holds.
    fn publish(&self, handle: &Handle, dtype: &str, shape: &[u64], bytes: &[u8]) -> Option<Vec<Locator>> {
        scheme()?;
        let key = key_for(handle);
        let descr = Descr::parse(dtype).ok()?;
        let array = ArrayRef::new(&descr, shape, Order::C, bytes).ok()?;
        let ck = if bytes.len() as u64 > MAX_READ { &self.large } else { &self.ck };
        if ck.publish(&key, array) != Ok(true) {
            return None;
        }
        let path = self.ck.path_for(&key)?;
        let offset = commonkit::npy::encode_header(&descr, shape, Order::C).ok()?.len() as u64;
        Some(vec![Locator::Commonkit { key }, Locator::Path { path: path.display().to_string(), offset }])
    }

    /// The segment file a handle is stored in under this root, whether or not it exists.
    pub fn path_for(&self, handle: &Handle) -> Option<PathBuf> {
        relpath(&key_for(handle))?;
        self.ck.path_for(&key_for(handle))
    }

    /// The segment file that holds a handle under this root, as it is or compressed; `None`
    /// when neither is there.
    pub fn stored_path(&self, handle: &Handle) -> Option<PathBuf> {
        let plain = self.path_for(handle).filter(|p| p.is_file());
        plain.or_else(|| (handle.recipe() == Recipe::Raw).then(|| self.ck.path_for(&z_key_for(handle))).flatten().filter(|p| p.is_file()))
    }

    /// Whether this root holds a handle, as it is or compressed.
    pub fn has(&self, handle: &Handle) -> bool {
        self.path_for(handle).is_some_and(|p| p.is_file())
            || (handle.recipe() == Recipe::Raw && self.ck.path_for(&z_key_for(handle)).is_some_and(|p| p.is_file()))
    }

    /// Read a blob by handle under this root (inflating a compressed one), checked against the
    /// handle; a miss is `None`.
    pub fn read_handle(&self, handle: &Handle) -> Option<Blob> {
        let plain = self.read_key(&key_for(handle)).filter(|b| matches_handle(Some(handle), b));
        plain.or_else(|| (handle.recipe() == Recipe::Raw).then(|| self.read_key(&z_key_for(handle))).flatten().filter(|b| matches_handle(Some(handle), b)))
    }

    /// Store bytes that claim to be `handle`, verifying first (see [`verifies`]). `false` when
    /// they don't match (nothing stored; spec 13.1) or when the store declines them (its
    /// budget, free-space floor or size limit): check [`verifies`] to tell the two apart.
    pub fn put_verified(&self, handle: &Handle, dtype: &str, shape: &[u64], bytes: &[u8]) -> bool {
        if !verifies(handle, dtype, shape, bytes) {
            return false;
        }
        if handle.recipe() == Recipe::Raw { self.publish_raw(bytes).is_some() } else { self.publish(handle, dtype, shape, bytes).is_some() }
    }

    /// Read a blob by commonkit key under this root, inflating a compressed one (`plumz`,
    /// `plummemo`) to the raw bytes. A missing or torn segment, a stream that doesn't inflate,
    /// or a key that isn't PlumCraft's, is `None`. A hit touches the segment (commonkit's LRU).
    pub fn read_key(&self, key: &str) -> Option<Blob> {
        relpath(key)?;
        let size = std::fs::metadata(self.ck.path_for(key)?).ok()?.len();
        if size > MAX_READ + 4096 {
            return None;
        }
        let b = blob_of(&self.ck.attach(key)?)?;
        if matches!(form_of(key), Some(FORM_Z | FORM_MEMO)) {
            let bytes = inflate(&b.bytes)?;
            return Some(Blob { dtype: "|u1".into(), shape: vec![bytes.len() as u64], bytes: bytes.into() });
        }
        Some(b)
    }

    /// Map a segment by key, as [`Store::read_key`] finds it; never a compressed one.
    fn map_key(&self, key: &str) -> Option<Blob> {
        relpath(key)?;
        if matches!(form_of(key), Some(FORM_Z | FORM_MEMO)) {
            return None;
        }
        mapped(self.ck.path_for(key)?)
    }

    /// Whether `path` is a real file under this root or one of `trusted`, with no symlink escape.
    fn trusted_path(&self, path: &str, trusted: &[String]) -> Option<PathBuf> {
        let p = Path::new(path).canonicalize().ok()?;
        let roots = std::iter::once(self.root.clone()).chain(trusted.iter().map(PathBuf::from));
        roots.filter_map(|r| r.canonicalize().ok()).any(|r| p.starts_with(r)).then_some(p)
    }

    /// Read a `path` locator, honoured only under this root or one of `trusted` (the engine's
    /// declared store roots), with no symlink escape.
    pub fn read_path(&self, path: &str, trusted: &[String]) -> Option<Blob> {
        let p = self.trusted_path(path, trusted)?;
        if std::fs::metadata(&p).ok()?.len() > MAX_READ + 4096 {
            return None;
        }
        blob_of(&Segment::open(&p, AttachMode::Read)?)
    }

    /// Resolve a DataRef to its bytes: the first locator this reader may use, cheapest first.
    /// `commonkit` and `path` only when `same_host` (the session's proof passed). The bytes
    /// are checked against the handle; a mismatch is a miss. An array over [`MAX_READ`] is
    /// mapped, not read: its header is checked against the DataRef's shape and size, and hashing
    /// it would read it all, which is what mapping is for.
    pub fn resolve(&self, r: &DataRef, same_host: bool) -> Option<Blob> {
        self.resolve_in(r, same_host, &[])
    }

    /// [`Store::resolve`], also trusting `path` locators under `trusted` roots.
    pub fn resolve_in(&self, r: &DataRef, same_host: bool, trusted: &[String]) -> Option<Blob> {
        let mut order: Vec<&Locator> = r.at.iter().collect();
        order.sort_by_key(|l| match l {
            Locator::Mem { .. } => 0,
            Locator::Commonkit { .. } => 1,
            Locator::Path { .. } => 2,
            Locator::Url { .. } => 4,
            Locator::Inline { .. } => 3,
        });
        for l in order {
            let blob = match l {
                Locator::Commonkit { key } if same_host => self.read_key(key).or_else(|| self.map_key(key).filter(|b| maps_to(r, b))),
                Locator::Path { path, .. } if same_host => {
                    self.read_path(path, trusted).or_else(|| mapped(self.trusted_path(path, trusted)?).filter(|b| maps_to(r, b)))
                }
                Locator::Inline { b64 } => inline_blob(r, b64),
                _ => None,
            };
            if let Some(b) = blob.filter(|b| matches!(b.bytes, Bytes::Mapped(_)) || matches_handle(r.handle.as_ref(), b)) {
                return Some(b);
            }
        }
        None
    }
}

/// A DataRef's inline bytes (spec 4.4 `inline`), checked against its handle; `None` when it has
/// no inline locator or the bytes don't match. Needs no store.
pub fn read_inline(r: &DataRef) -> Option<Blob> {
    r.at.iter().filter_map(|l| if let Locator::Inline { b64 } = l { inline_blob(r, b64) } else { None }).find(|b| matches_handle(r.handle.as_ref(), b))
}

fn inline_blob(r: &DataRef, b64: &str) -> Option<Blob> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    match &r.meta.form {
        Form::Array { dtype, shape, .. } => Some(Blob { dtype: dtype.clone(), shape: shape.clone(), bytes: bytes.into() }),
        Form::Audio { dtype, channels, frames, .. } => Some(Blob { dtype: dtype.clone(), shape: vec![*frames, u64::from(*channels)], bytes: bytes.into() }),
        _ => Some(Blob { dtype: "|u1".into(), shape: vec![bytes.len() as u64], bytes: bytes.into() }),
    }
}

/// An array's file, mapped. Same rule as [`blob_of`]: a simple dtype, in C order.
fn mapped(path: PathBuf) -> Option<Blob> {
    let seg = Segment::open(&path, AttachMode::Mmap)?;
    let simple = seg.descr().as_simple()?;
    if seg.fortran_order() && seg.shape().len() > 1 {
        return None;
    }
    Some(Blob { dtype: simple.to_string(), shape: seg.shape().to_vec(), bytes: Bytes::Mapped(Arc::new(seg)) })
}

/// Whether a mapped file has the shape `r` says, with bytes for it. (The file's dtype stands: a
/// DataRef may spell one its own way, `float32` for `<f4`.)
fn maps_to(r: &DataRef, b: &Blob) -> bool {
    matches!(&r.meta.form, Form::Array { shape, .. } if *shape == b.shape) && r.meta.size == b.bytes.len() as u64
}

/// Whether bytes are `handle`: an `md5:raw:` handle must be the MD5 of the bytes, an
/// `md5:nd1:` handle the digest of `dtype`, `shape` and the bytes.
pub fn verifies(handle: &Handle, dtype: &str, shape: &[u64], bytes: &[u8]) -> bool {
    if handle.recipe() == Recipe::Raw { &Handle::md5_raw(bytes) == handle } else { Handle::md5_nd1(dtype, shape, bytes).is_ok_and(|h| &h == handle) }
}

fn matches_handle(h: Option<&Handle>, b: &Blob) -> bool {
    match h {
        None => true,
        Some(h) if h.recipe() == Recipe::Nd1 => Handle::md5_nd1(&b.dtype, &b.shape, &b.bytes).is_ok_and(|x| &x == h),
        Some(h) => &Handle::md5_raw(&b.bytes) == h,
    }
}

/// A segment's bytes as a blob: a simple dtype in C order only (what PlumCraft writes);
/// anything else is a miss.
fn blob_of(seg: &Segment) -> Option<Blob> {
    let simple = seg.descr().as_simple()?;
    if seg.fortran_order() && seg.shape().len() > 1 {
        return None;
    }
    Some(Blob { dtype: simple.to_string(), shape: seg.shape().to_vec(), bytes: seg.bytes().to_vec().into() })
}

/// An inline DataRef for small bytes (at most the session's `max_inline`).
pub fn inline_ref(handle: Handle, meta: BlobMeta, bytes: &[u8]) -> DataRef {
    DataRef { handle: Some(handle), meta, at: vec![Locator::Inline { b64: base64::engine::general_purpose::STANDARD.encode(bytes) }] }
}

/// The same-host proof (spec 5.4): write a nonce file under `{root}/.hello/` (mode 0600) and
/// return its path and nonce. The reader proves it shares this filesystem by reading it.
/// How long a nonce file may live (spec 5.4); older ones are swept when a new one is written.
pub const NONCE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// Move `tmp` to `dest`, a content-addressed path (its name holds the digest of what it holds).
/// When the rename fails but `dest` is there, another writer of the same bytes got there first:
/// that is success, and `tmp` goes. Windows refuses to replace a file another rename or reader
/// holds ("Access is denied"), where Unix replaces it; two threads staging one document met that.
/// On any other failure `tmp` goes too and the error is returned.
pub fn rename_addressed(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::rename(tmp, dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(tmp);
            if dest.is_file() { Ok(()) } else { Err(e) }
        }
    }
}

pub fn write_nonce(root: &Path, who: &str) -> std::io::Result<(String, String)> {
    let dir = root.join(".hello");
    std::fs::create_dir_all(&dir)?;
    sweep_nonces(&dir, NONCE_TTL);
    let nonce = plumcraft_proto::random::hex(32)?;
    let path = dir.join(format!("{who}-{}-{}.nonce", std::process::id(), &nonce[..8]));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(&path)?;
    f.write_all(nonce.as_bytes())?;
    Ok((path.display().to_string(), nonce))
}

/// Remove nonce files in `dir` older than `ttl` (spec 5.4: they are deleted after 60 s), so a
/// side that never got to delete its own (a crash, an unanswered hello) leaves nothing behind
/// for long. Only `*.nonce` regular files are touched; failures are ignored.
pub fn sweep_nonces(dir: &Path, ttl: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let now = std::time::SystemTime::now();
    for e in entries.flatten() {
        let path = e.path();
        let is_nonce = path.extension().is_some_and(|x| x == "nonce");
        let old = e
            .metadata()
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|m| m.modified().ok())
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > ttl);
        if is_nonce && old {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Read the other side's nonce file and compare. Any failure is `false`, never an error.
pub fn check_nonce(path: &str, nonce: &str) -> bool {
    let mut buf = String::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(128).read_to_string(&mut buf));
    read.is_ok() && !nonce.is_empty() && buf.trim() == nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let d = plumcraft_scratch::root().join(format!("store-{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn an_array_past_the_read_limit_publishes_and_reads_back_mapped() {
        let dir = plumcraft_scratch::dir("store-large");
        let s = Store::new(&dir);
        let n = (MAX_READ as usize / 4) + 1024;
        let bytes: Vec<u8> = (0..n).flat_map(|i| ((i % 251) as f32).to_le_bytes()).collect();
        let shape = [n as u64];
        let (handle, at) = s.publish_array("<f4", &shape, &bytes).expect("published");
        assert_eq!(handle, Handle::md5_nd1("<f4", &shape, &bytes).unwrap());
        let meta = BlobMeta {
            size: bytes.len() as u64,
            form: Form::Array { dtype: "<f4".into(), shape: shape.to_vec(), space: None, names: Vec::new(), table: false },
            ..Default::default()
        };
        let got = s.resolve(&DataRef { handle: Some(handle.clone()), meta, at }, true).expect("it reads back");
        assert_eq!(&got.bytes[..], &bytes[..]);
        assert!(s.path_for(&handle).is_some_and(|p| p.is_file()), "in the tree, where the reaper sees it");
    }

    #[test]
    fn publish_read_resolve() {
        let s = Store::new(temp_root("prr"));
        let text = "the corgi ".repeat(10_000);
        let (h, at) = s.publish_raw(text.as_bytes()).unwrap();
        assert_eq!(h.recipe(), Recipe::Raw);
        let r = DataRef { handle: Some(h.clone()), meta: BlobMeta::default(), at };
        assert_eq!(&*s.resolve(&r, true).unwrap().bytes, text.as_bytes());
        assert!(s.resolve(&r, false).is_none(), "no proof, no commonkit or path");
        // Republishing is a no-op; a deleted segment is a miss.
        assert!(s.publish_raw(text.as_bytes()).is_some());
        let Locator::Commonkit { key } = &r.at[0] else { panic!() };
        std::fs::remove_file(s.ck.path_for(key).unwrap()).unwrap();
        assert!(s.resolve(&r, true).is_none());
    }

    /// Bytes that look random: deflate can't shrink them.
    fn noise(n: usize) -> Vec<u8> {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn large_compressible_raw_blobs_are_stored_compressed_and_read_back_whole() {
        let s = Store::new(temp_root("z"));
        let json = format!("[{}]", (0..20_000).map(|i| format!("{{\"x\": {i}, \"name\": \"node\"}}")).collect::<Vec<_>>().join(","));
        let (h, at) = s.publish_raw(json.as_bytes()).unwrap();
        assert_eq!(h, Handle::md5_raw(json.as_bytes()), "the identity is the uncompressed bytes'");
        assert_eq!(at, vec![Locator::Commonkit { key: z_key_for(&h) }], "compressed: no path locator");
        let stored = std::fs::metadata(s.ck.path_for(&z_key_for(&h)).unwrap()).unwrap().len();
        assert!(stored * 5 < json.len() as u64, "{stored} bytes stored for {}", json.len());
        assert!(!s.path_for(&h).unwrap().exists(), "not also stored plain");
        assert!(s.has(&h));
        assert_eq!(&*s.read_handle(&h).unwrap().bytes, json.as_bytes());
        let r = DataRef { handle: Some(h.clone()), meta: BlobMeta::default(), at: at.clone() };
        assert_eq!(&*s.resolve(&r, true).unwrap().bytes, json.as_bytes());
        // Publishing again finds it: the same locators, nothing rewritten.
        assert_eq!(s.publish_raw(json.as_bytes()).unwrap().1, at);
        assert_eq!(s.locate(&h), Some(at));

        // Incompressible, or small: stored as is, with a path locator.
        let n = noise(200_000);
        let (hn, at) = s.publish_raw(&n).unwrap();
        assert!(matches!(at.as_slice(), [Locator::Commonkit { .. }, Locator::Path { .. }]), "{at:?}");
        assert!(!s.ck.path_for(&z_key_for(&hn)).unwrap().exists());
        let Locator::Path { path, offset } = &at[1] else { panic!() };
        assert_eq!(std::fs::read(path).unwrap().get(*offset as usize..).unwrap(), n.as_slice(), "the path locator's offset is the payload");
        let small = "x".repeat(1000);
        assert_eq!(s.publish_raw(small.as_bytes()).unwrap().1.len(), 2, "under COMPRESS_MIN: as is");
        assert!(compress(&n).is_none() && compress(small.as_bytes()).is_none());

        // An upload is compressed the same way.
        let csv = "a,b,c\n1,2,3\n".repeat(10_000);
        let hc = Handle::md5_raw(csv.as_bytes());
        assert!(s.put_verified(&hc, "|u1", &[csv.len() as u64], csv.as_bytes()));
        assert!(s.ck.path_for(&z_key_for(&hc)).unwrap().is_file());

        // A damaged compressed segment is a miss, never wrong bytes or a panic.
        let z = s.read_key(&z_key_for(&h)).map(|_| ()).and_then(|()| s.ck.attach(&z_key_for(&h))).unwrap().bytes().to_vec();
        std::fs::remove_file(s.ck.path_for(&z_key_for(&h)).unwrap()).unwrap();
        assert!(s.put(&z_key_for(&h), "|u1", &[z.len() as u64 / 2], &z[..z.len() / 2]), "a valid segment holding half the stream");
        assert!(s.read_handle(&h).is_none());
        assert!(inflate(b"not a zlib stream").is_none());
    }

    #[test]
    fn memo_records_round_trip_and_bad_plan_keys_are_refused() {
        let s = Store::new(temp_root("memo"));
        let key = "memo:md5:0123456789abcdef0123456789abcdef";
        assert!(s.read_record(key).is_none());
        assert!(s.put_record(key, br#"{"v": 1}"#));
        assert_eq!(s.read_record(key).unwrap(), br#"{"v": 1}"#);
        assert!(s.put_record(key, br#"{"v": 2}"#), "rewritten when the node is recomputed");
        assert_eq!(s.read_record(key).unwrap(), br#"{"v": 2}"#);
        for bad in ["memo:md5:../../etc/passwd", "memo:md5:0123456789ABCDEF0123456789ABCDEF", "md5:raw:0123456789abcdef0123456789abcdef", ""] {
            assert!(memo_key_for(bad).is_none() && !s.put_record(bad, b"{}") && s.read_record(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn arrays_key_by_nd1_and_check_the_handle() {
        let s = Store::new(temp_root("arr"));
        let bytes: Vec<u8> = [1u32, 2, 3].iter().flat_map(|x| x.to_le_bytes()).collect();
        let (h, at) = s.publish_array("<u4", &[3], &bytes).unwrap();
        assert_eq!(h, Handle::md5_nd1("<u4", &[3], &bytes).unwrap());
        assert!(key_for(&h).starts_with("plum:v1:plumarr:"));
        let r = DataRef { handle: Some(h), meta: BlobMeta::default(), at: at.clone() };
        let b = s.resolve(&r, true).unwrap();
        assert_eq!((b.dtype.as_str(), b.shape.as_slice(), &*b.bytes), ("<u4", &[3u64][..], bytes.as_slice()));
        // Bytes that don't match the handle are a miss.
        let wrong = DataRef { handle: Some(Handle::md5_raw(b"other")), meta: BlobMeta::default(), at };
        assert!(s.resolve(&wrong, true).is_none());
    }

    /// The tree's policy applies: over its size limit a publish is declined, not an error, and
    /// `put_verified` is `false` for bytes that `verifies` accepts.
    #[test]
    fn a_declined_publish_is_none_and_not_a_mismatch() {
        let root = temp_root("policy");
        std::fs::write(root.join(".policy.json"), r#"{"MAX_ARRAY_BYTES": 1000}"#).unwrap();
        let s = Store::new(&root);
        assert!(s.publish_raw(&[7u8; 500]).is_some(), "under the limit");
        let big = vec![7u8; 5000];
        assert!(s.publish_raw(&big).is_none(), "over the limit: declined");
        let h = Handle::md5_raw(&big);
        assert!(verifies(&h, "|u1", &[5000], &big));
        assert!(!s.put_verified(&h, "|u1", &[5000], &big));
        assert!(!s.has(&h));
    }

    /// A segment written here is a commonkit segment: the crate's own store reads it back.
    #[test]
    fn commonkit_reads_what_plumcraft_writes() {
        let root = temp_root("interop");
        let s = Store::new(&root);
        let (h, _) = s.publish_array("<f4", &[2, 2], &[0u8; 16]).unwrap();
        let ck = commonkit::shared_store_at(&root, Mode::Read, AttachMode::Read, 0);
        let seg = ck.attach(&key_for(&h)).unwrap();
        assert_eq!((seg.shape(), seg.bytes().len()), (&[2u64, 2][..], 16));
    }

    #[test]
    fn keys_and_paths_are_strict() {
        assert!(relpath("plum:v1:plumraw:0123456789abcdef0123456789abcdef:c").is_some());
        for bad in [
            "impc:v1:impcarr:0123456789abcdef0123456789abcdef:c",
            "plum:v1:plumraw:../../etc/passwd:c",
            "plum:v1:plumraw:0123:c",
            "plum:v1:x:0123456789abcdef0123456789abcdef:c",
        ] {
            assert!(relpath(bad).is_none(), "{bad}");
        }
        let s = Store::new(temp_root("esc"));
        assert!(s.read_path("/etc/hosts", &[]).is_none(), "outside the root");
    }

    #[test]
    fn nonce_proof() {
        let root = temp_root("nonce");
        let (path, nonce) = write_nonce(&root, "e").unwrap();
        assert!(check_nonce(&path, &nonce));
        assert!(!check_nonce(&path, "wrong"));
        assert!(!check_nonce("/nonexistent/x.nonce", &nonce));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    /// Spec 5.4: nonce files don't accumulate; a stale one is swept when a new one is written.
    #[test]
    fn stale_nonces_are_swept() {
        let root = temp_root("sweep");
        let (old, _) = write_nonce(&root, "e").unwrap();
        let other = root.join(".hello").join("keep.txt");
        std::fs::write(&other, "not a nonce").unwrap();
        sweep_nonces(&root.join(".hello"), std::time::Duration::ZERO);
        assert!(!Path::new(&old).exists(), "an expired nonce is removed");
        assert!(other.exists(), "only nonce files are touched");
        let (fresh, _) = write_nonce(&root, "e").unwrap();
        assert!(Path::new(&fresh).exists(), "a fresh nonce survives the sweep");
    }
}
