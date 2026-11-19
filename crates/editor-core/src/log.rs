//! One log for the core and every shell (N1 slice 8, `docs/shell-interface.md` section 15,
//! `docs/research/24-native-shells.md` section 9): a native app has no stderr anyone sees, so
//! what the editor said there is also kept here, where a shell, a script and a person can read
//! it.
//!
//! - **Entries** `{seq, time, level, source, message, doc?}`: `seq` counts up from 1 for the
//!   process, `time` is seconds since the Unix epoch, `level` one of `error warn info debug
//!   trace`, `source` what said it (`core`, `ffi`, `editor`, `engine.<id>`, `shell`, …), `doc`
//!   the document it is about, when one is.
//! - **A ring** of the last [`CAPACITY`] entries, read with [`tail`] (`log.tail`); older ones
//!   are dropped and counted.
//! - **Stderr** stays a sink on desktops, so nothing a developer sees today disappears: an
//!   entry written with [`say`] prints its line exactly as the `eprintln!` it replaced did.
//! - **A file**, when the shell names a folder ([`set_file`]): `plumcraft.log`, at most
//!   [`FILE_MAX`] bytes, then moved to `plumcraft.log.1` (one kept) and started again.
//! - **A level** ([`set_level`]): what is below it isn't kept. From the C ABI's config (`log:
//!   {level}`), the egui editor's preferences (`log_level`) and `PLUMCRAFT_LOG`, which wins.
//! - **Hostile input is bounded:** a message is cut at [`MESSAGE_MAX`] characters, a source at
//!   [`SOURCE_MAX`] of `[a-z0-9._-]`; control characters other than a newline or a tab become
//!   `\u{fffd}`; a flood only moves the ring on.
//! - **No secrets:** the control channel's token is registered with [`redact`] and never enters
//!   the ring or the file (the stderr line the user reads is the one place it is printed, as
//!   before). File contents are never logged.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use serde::Serialize;
use serde_json::{Value, json};

/// How many entries the ring keeps.
pub const CAPACITY: usize = 10_000;
/// The longest message kept, in characters.
pub const MESSAGE_MAX: usize = 4_096;
/// The longest source kept, in characters.
pub const SOURCE_MAX: usize = 64;
/// The log file's size before it is moved aside.
pub const FILE_MAX: u64 = 4 << 20;
/// The file's name in the folder the shell gives.
pub const FILE_NAME: &str = "plumcraft.log";
/// The most entries one `log.tail` returns.
pub const TAIL_MAX: usize = 5_000;
/// The environment variable that sets the level (`error`, `warn`, `info`, `debug`, `trace`).
pub const ENV: &str = "PLUMCRAFT_LOG";

/// How serious an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "error" => Level::Error,
            "warn" | "warning" => Level::Warn,
            "info" => Level::Info,
            "debug" => Level::Debug,
            "trace" => Level::Trace,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }
}

/// One entry.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Entry {
    pub seq: u64,
    pub time: f64,
    pub level: Level,
    pub source: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<u64>,
}

struct File {
    path: PathBuf,
    size: u64,
}

struct Log {
    ring: VecDeque<Entry>,
    next: u64,
    dropped: u64,
    level: Level,
    stderr: bool,
    file: Option<File>,
    secrets: Vec<String>,
}

fn log() -> MutexGuard<'static, Log> {
    static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
    let m = LOG.get_or_init(|| {
        let level = std::env::var(ENV).ok().and_then(|v| Level::parse(&v)).unwrap_or(Level::Info);
        Mutex::new(Log { ring: VecDeque::new(), next: 1, dropped: 0, level, stderr: true, file: None, secrets: Vec::new() })
    });
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Keep entries at `level` and above. `PLUMCRAFT_LOG`, when set to a level, wins over this.
pub fn set_level(level: Level) {
    let env = std::env::var(ENV).ok().and_then(|v| Level::parse(&v));
    log().level = env.unwrap_or(level);
}

/// The level kept.
pub fn level() -> Level {
    log().level
}

/// Whether [`say`] prints its line to stderr too (on desktops, as before; a phone's shell turns
/// it off and reads the ring, or the unified log, instead).
pub fn set_stderr(on: bool) {
    log().stderr = on;
}

/// Never keep `secret` (a token): every entry has it replaced. Short strings are ignored, so a
/// two-letter word can't blank the log.
pub fn redact(secret: &str) {
    let secret = secret.trim();
    if secret.len() < 8 {
        return;
    }
    let mut l = log();
    if !l.secrets.iter().any(|s| s == secret) {
        l.secrets.push(secret.to_owned());
    }
}

/// Also write to `dir/plumcraft.log` (made if missing), moved to `plumcraft.log.1` past
/// [`FILE_MAX`]. `None` stops writing a file.
pub fn set_file(dir: Option<&Path>) -> Result<(), String> {
    let file = match dir {
        None => None,
        Some(d) => {
            std::fs::create_dir_all(d).map_err(|e| format!("log folder {}: {e}", d.display()))?;
            let path = d.join(FILE_NAME);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("log file {}: {e}", path.display()))?;
            Some(File { path, size })
        }
    };
    log().file = file;
    Ok(())
}

/// The log file being written, if any.
pub fn file() -> Option<PathBuf> {
    log().file.as_ref().map(|f| f.path.clone())
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// `text` bounded and printable: at most `max` characters, control characters but a newline and
/// a tab replaced.
fn clean(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).map(|c| if c.is_control() && c != '\n' && c != '\t' { '\u{fffd}' } else { c }).collect();
    if text.chars().nth(max).is_some() {
        out.push('…');
    }
    out
}

fn clean_source(source: &str) -> String {
    let s: String =
        source.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).take(SOURCE_MAX).collect::<String>().to_ascii_lowercase();
    if s.is_empty() { "core".to_owned() } else { s }
}

fn write_entry(level: Level, source: &str, message: &str, doc: Option<u64>, stderr_line: Option<&str>) -> Option<u64> {
    let mut l = log();
    if l.stderr
        && let Some(line) = stderr_line
    {
        eprintln!("{line}");
    }
    if level > l.level {
        return None;
    }
    let mut message = clean(message, MESSAGE_MAX);
    for s in &l.secrets {
        if message.contains(s.as_str()) {
            message = message.replace(s.as_str(), "‹redacted›");
        }
    }
    let seq = l.next;
    l.next += 1;
    let entry = Entry { seq, time: now(), level, source: clean_source(source), message, doc };
    if let Some(f) = &mut l.file {
        let line = format!("{} {} {} {}\n", entry.time, entry.level.name(), entry.source, entry.message.replace('\n', "\n    "));
        if f.size + line.len() as u64 > FILE_MAX {
            let old = f.path.with_extension("log.1");
            let _ = std::fs::rename(&f.path, &old);
            f.size = 0;
        }
        if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(&f.path)
            && out.write_all(line.as_bytes()).is_ok()
        {
            f.size += line.len() as u64;
        }
    }
    l.ring.push_back(entry);
    while l.ring.len() > CAPACITY {
        l.ring.pop_front();
        l.dropped += 1;
    }
    Some(seq)
}

/// Keep `message` at `level` from `source` (about document `doc`, if any). Its seq, or `None`
/// when the level isn't kept.
pub fn write(level: Level, source: &str, message: &str, doc: Option<u64>) -> Option<u64> {
    write_entry(level, source, message, doc, None)
}

/// What an `eprintln!` said: `line` to stderr as it was printed (on desktops), and to the log.
pub fn say(level: Level, source: &str, line: &str) -> Option<u64> {
    write_entry(level, source, line, None, Some(line))
}

pub fn error(source: &str, message: &str) -> Option<u64> {
    write(Level::Error, source, message, None)
}

pub fn warn(source: &str, message: &str) -> Option<u64> {
    write(Level::Warn, source, message, None)
}

pub fn info(source: &str, message: &str) -> Option<u64> {
    write(Level::Info, source, message, None)
}

pub fn debug(source: &str, message: &str) -> Option<u64> {
    write(Level::Debug, source, message, None)
}

/// The newest entry's seq (0: none yet).
pub fn last_seq() -> u64 {
    log().next - 1
}

/// The worst level among entries after `after`, for a shell's feed (`{seq, worst}`).
pub fn worst_after(after: u64) -> Option<Level> {
    log().ring.iter().filter(|e| e.seq > after).map(|e| e.level).min()
}

/// What [`tail`] returns.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tail {
    pub entries: Vec<Entry>,
    /// The seq to ask after next time.
    pub next: u64,
    /// Entries dropped from the ring since the process started.
    pub dropped: u64,
}

/// Entries after `after` (all kept, without), at `level` or worse, from sources starting with
/// `source`, the oldest first, at most `limit`.
pub fn tail(after: Option<u64>, level: Option<Level>, source: Option<&str>, limit: usize) -> Tail {
    let l = log();
    let after = after.unwrap_or(0);
    let entries: Vec<Entry> = l
        .ring
        .iter()
        .filter(|e| e.seq > after && level.is_none_or(|lv| e.level <= lv) && source.is_none_or(|s| e.source.starts_with(s)))
        .take(limit.clamp(1, TAIL_MAX))
        .cloned()
        .collect();
    let next = entries.last().map_or(after, |e| e.seq);
    Tail { entries, next, dropped: l.dropped }
}

/// `log.tail {after?, since?, level?, source?, limit?}` → `{entries, next, dropped}`.
pub fn tail_request(params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["after", "since", "level", "source", "limit"])?;
    let num = |k: &str| -> Result<Option<u64>, String> {
        match params.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{k} must be a whole number")),
        }
    };
    let after = num("after")?.or(num("since")?);
    let level = match params.get("level") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(Level::parse(s).ok_or("level must be error, warn, info, debug or trace")?),
        Some(_) => return Err("level must be error, warn, info, debug or trace".into()),
    };
    let source = match params.get("source") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.len() <= SOURCE_MAX => Some(s.as_str()),
        Some(_) => return Err(format!("source must be a source's name (at most {SOURCE_MAX} characters)")),
    };
    let limit = match num("limit")? {
        None => 500,
        Some(n) if (1..=TAIL_MAX as u64).contains(&n) => usize::try_from(n).unwrap_or(TAIL_MAX),
        Some(_) => return Err(format!("limit must be 1 to {TAIL_MAX}")),
    };
    serde_json::to_value(tail(after, level, source, limit)).map_err(|e| e.to_string())
}

/// `log.write {level, message}`: a shell's own entry, under source `shell`, so one log holds
/// both sides. → `{seq}` (`null` when the level isn't kept).
pub fn write_request(params: &Value, doc: Option<u64>) -> Result<Value, String> {
    crate::control::check_fields(params, &["level", "message"])?;
    let level = params.get("level").and_then(Value::as_str).and_then(Level::parse).ok_or("level must be error, warn, info, debug or trace")?;
    let message = params.get("message").and_then(Value::as_str).ok_or("message must be text")?;
    Ok(json!({"seq": write(level, "shell", message, doc)}))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The log is the process's: each test looks only at what it wrote after it started, and they
    // take turns (one floods the ring, one writes a file).
    fn serial() -> MutexGuard<'static, ()> {
        static TURN: Mutex<()> = Mutex::new(());
        TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn entries_are_kept_in_order_and_read_after_a_seq() {
        let _turn = serial();
        let start = last_seq();
        let a = write(Level::Warn, "core", "first", Some(3)).unwrap();
        let b = write(Level::Error, "engine.text", "second", None).unwrap();
        assert!(b > a && a > start);
        let t = tail(Some(start), None, None, 100);
        let mine: Vec<&Entry> = t.entries.iter().filter(|e| e.seq == a || e.seq == b).collect();
        assert_eq!(mine.len(), 2);
        assert_eq!((mine[0].message.as_str(), mine[0].doc, mine[0].level), ("first", Some(3), Level::Warn));
        assert_eq!(mine[1].source, "engine.text");
        // only errors, only the engine's
        assert!(tail(Some(start), Some(Level::Error), None, 100).entries.iter().all(|e| e.level == Level::Error));
        assert!(tail(Some(start), None, Some("engine."), 100).entries.iter().all(|e| e.source.starts_with("engine.")));
        assert!(tail(Some(b), None, None, 100).entries.iter().all(|e| e.seq > b));
    }

    #[test]
    fn hostile_entries_are_bounded_and_printable() {
        let _turn = serial();
        let huge = "x".repeat(MESSAGE_MAX * 10);
        let seq = write(Level::Error, "a b\u{0}C/../", &format!("bell\u{7} esc\u{1b}[31m {huge}"), None).unwrap();
        let e = tail(Some(seq - 1), None, None, 1).entries.into_iter().find(|e| e.seq == seq).unwrap();
        assert!(e.message.chars().count() <= MESSAGE_MAX + 1);
        assert!(!e.message.chars().any(|c| c.is_control()), "{:?}", &e.message[..40]);
        assert_eq!(e.source, "abc..");
        // a flood moves the ring on and counts what it dropped
        let before = tail(None, None, None, 1).dropped;
        for i in 0..(CAPACITY + 50) {
            write(Level::Error, "flood", &format!("{i}"), None);
        }
        let t = tail(None, None, None, TAIL_MAX);
        assert!(t.dropped >= before + 50);
        assert_eq!(t.entries.len(), TAIL_MAX);
        assert!(log().ring.len() <= CAPACITY);
    }

    #[test]
    fn a_secret_never_enters_the_log() {
        let _turn = serial();
        let token = "7f3c9d1e2b4a5f6071829304a5b6c7d8";
        redact(token);
        let seq = write(Level::Info, "core", &format!("control token: {token} and again {token}"), None).unwrap();
        let e = tail(Some(seq - 1), None, None, 10).entries.into_iter().find(|e| e.seq == seq).unwrap();
        assert!(!e.message.contains(token));
        assert!(e.message.contains("‹redacted›"));
    }

    #[test]
    fn the_file_is_bounded_with_one_kept_before_it() {
        let _turn = serial();
        let dir = plumcraft_scratch::dir("log-file");
        set_file(Some(&dir)).unwrap();
        assert_eq!(file(), Some(dir.join(FILE_NAME)));
        let line = "y".repeat(MESSAGE_MAX);
        let n = usize::try_from(FILE_MAX).unwrap() / MESSAGE_MAX + 20;
        for _ in 0..n {
            write(Level::Error, "file", &line, None);
        }
        set_file(None).unwrap();
        let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        assert!(size(&dir.join(FILE_NAME)) <= FILE_MAX);
        assert!(size(&dir.join("plumcraft.log.1")) > FILE_MAX / 2, "the full one moved aside");
        assert!(!dir.join("plumcraft.log.2").exists(), "one kept");
        assert!(file().is_none());
        // a folder it can't make is an error, not a panic
        assert!(set_file(Some(&dir.join(FILE_NAME).join("under-a-file"))).is_err());
    }

    #[test]
    fn requests_check_their_params() {
        let _turn = serial();
        assert!(tail_request(&json!({"limit": 0})).is_err());
        assert!(tail_request(&json!({"limit": 9_999_999})).is_err());
        assert!(tail_request(&json!({"level": "loud"})).is_err());
        assert!(tail_request(&json!({"after": -1})).is_err());
        assert!(tail_request(&json!({"nope": 1})).is_err());
        assert!(tail_request(&json!("x")).is_err());
        assert!(tail_request(&json!({"source": "s".repeat(500)})).is_err());
        assert!(write_request(&json!({"level": "warn"}), None).is_err());
        assert!(write_request(&json!({"level": "nope", "message": "m"}), None).is_err());
        let seq = write_request(&json!({"level": "warn", "message": "from the shell"}), Some(1)).unwrap()["seq"].as_u64().unwrap();
        let t = tail_request(&json!({"since": seq - 1, "source": "shell"})).unwrap();
        assert_eq!(t["entries"][0]["message"], "from the shell");
        assert_eq!(t["entries"][0]["doc"], 1);
        assert_eq!(t["next"], seq);
    }
}
