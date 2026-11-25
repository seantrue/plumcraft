//! Autosave (N4 slice 4; `docs/shell-interface.md` 4.4): a document opened with
//! `document.open {autosave: true}` (or switched with `document.autosave {on: true}`) is saved
//! to its own file by the core, with no Save command. The maintainer: "Build autosave in the core
//! and use it on the iPad; the Mac keeps explicit Save."
//!
//! - **When:** once a change has settled ([`QUIET`] with no other change), or at most
//!   [`LONGEST`] after the first unsaved change while changes keep coming; never while a run is
//!   in flight (it waits for the run to end), never for a graph with no file (`untitled`: the
//!   shell gives every document a file, the iPad's in Documents). Every change that makes the
//!   graph dirty counts ([`plumcraft_session::Session::changes`]): edits, moves, undo and redo,
//!   and the changes that aren't undo steps (Show on Face, `node.fields`, `node.view`).
//! - **How:** the graph is serialised on the document's thread (the core's, never the UI's) and
//!   written on a thread of its own, atomically (a temporary file in the same folder, synced,
//!   then renamed: `plumcraft_session::file_cmds::write_atomic`), so a failed or interrupted
//!   write leaves the file as it was. One save at a time per document.
//! - **Dirty** then means "not yet saved by autosave": a save of change `n` cleans the document
//!   only if nothing changed while it was written; otherwise the next save takes the rest.
//! - **Failure** (a read-only folder, a provider offline): the state is `failed` with the reason,
//!   the document stays dirty, and the save is tried again after a back-off (5 s, doubling to a
//!   minute) or as soon as the next change settles. Nothing modal: the shell shows the state.
//! - **Flush** ([`flush`]): `lifecycle.background`, closing the document and closing the core
//!   save at once, waiting for a save in flight, so no edit is lost on quit or backgrounding.
//!
//! The feed's `document` domain carries the state (`autosave: {on, state, error?, saves}`), and
//! `document.autosave {}` reads it with what is on disk.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::document::Document;

/// A change is saved once nothing else has changed for this long.
pub const QUIET: Duration = Duration::from_secs(1);
/// …or at most this long after the first unsaved change, however often changes come.
pub const LONGEST: Duration = Duration::from_secs(10);
/// After a failed save, the first wait before trying again; it doubles up to [`RETRY_MOST`].
pub const RETRY_FIRST: Duration = Duration::from_secs(5);
pub const RETRY_MOST: Duration = Duration::from_secs(60);
/// How often a document looks again while a save is being written or a run holds it.
const SOON: Duration = Duration::from_millis(100);
/// The largest file `document.autosave` reads back to compare.
const MOST_READ: u64 = 64 << 20;

/// Where autosave is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// Not on for this document.
    Off,
    /// On, and nothing is unsaved.
    Clean,
    /// A change waits to settle (or for a run to end).
    Pending,
    /// On, but the graph has no file to save to.
    Untitled,
    /// Being written.
    Saving,
    /// The last save succeeded and nothing has changed since.
    Saved,
    /// The last save failed; it is tried again.
    Failed,
}

impl Word {
    pub fn as_str(self) -> &'static str {
        match self {
            Word::Off => "off",
            Word::Clean => "clean",
            Word::Pending => "pending",
            Word::Untitled => "untitled",
            Word::Saving => "saving",
            Word::Saved => "saved",
            Word::Failed => "failed",
        }
    }
}

/// A save being written on its thread.
#[derive(Debug)]
struct InFlight {
    /// The change it is of.
    as_of: u64,
    path: PathBuf,
    rx: Receiver<Result<(), String>>,
}

/// A document's autosave state (kept in [`Document::autosave`]).
#[derive(Debug)]
pub struct Autosave {
    on: bool,
    pub quiet: Duration,
    pub longest: Duration,
    /// The session's change count last seen, and when it moved.
    seen: Option<u64>,
    last_change: Option<Instant>,
    first_unsaved: Option<Instant>,
    in_flight: Option<InFlight>,
    word: Word,
    error: Option<String>,
    retry_at: Option<Instant>,
    backoff: Duration,
    saves: u64,
    failures: u64,
}

impl Default for Autosave {
    fn default() -> Self {
        Autosave {
            on: false,
            quiet: QUIET,
            longest: LONGEST,
            seen: None,
            last_change: None,
            first_unsaved: None,
            in_flight: None,
            word: Word::Off,
            error: None,
            retry_at: None,
            backoff: RETRY_FIRST,
            saves: 0,
            failures: 0,
        }
    }
}

impl Autosave {
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Turn it on or off. Off forgets a failure; a save in flight still finishes.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
        if !on {
            self.word = Word::Off;
            self.error = None;
            self.retry_at = None;
            self.backoff = RETRY_FIRST;
        } else if self.word == Word::Off {
            self.word = Word::Clean;
        }
    }

    pub fn word(&self) -> Word {
        self.word
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// `{on, state, error?, saves, failures}`, as the feed's `document` domain carries it.
    pub fn status(&self) -> Value {
        let mut v = json!({"on": self.on, "state": self.word.as_str(), "saves": self.saves, "failures": self.failures});
        if let Some(e) = &self.error {
            v["error"] = json!(e);
        }
        v
    }

    /// The session's change count is `changes`: a new change restarts the quiet time, and a
    /// failed save is tried again once it settles.
    fn note(&mut self, changes: u64, now: Instant) {
        if self.seen == Some(changes) {
            return;
        }
        // the first sight counts as a change (a document dirty when autosave came on waits out
        // the quiet time like any other)
        self.seen = Some(changes);
        self.last_change = Some(now);
        self.first_unsaved.get_or_insert(now);
        self.retry_at = None;
    }

    /// Is a save due at `now`? None: not yet, and when to look again (none: nothing to wait for).
    fn due(&self, now: Instant) -> Result<(), Option<Duration>> {
        if let Some(at) = self.retry_at
            && at > now
        {
            return Err(Some(at - now));
        }
        let quiet = self.last_change.map_or(Duration::ZERO, |t| self.quiet.saturating_sub(now.saturating_duration_since(t)));
        let longest = self.first_unsaved.map_or(Duration::ZERO, |t| self.longest.saturating_sub(now.saturating_duration_since(t)));
        let wait = quiet.min(longest);
        if wait.is_zero() { Ok(()) } else { Err(Some(wait)) }
    }

    /// A save's outcome: the document is clean if nothing changed while it was written.
    fn finished(&mut self, outcome: Result<(), String>, path: PathBuf, as_of: u64, session: &mut plumcraft_session::Session, now: Instant) {
        match outcome {
            Ok(()) => {
                self.saves += 1;
                self.error = None;
                self.retry_at = None;
                self.backoff = RETRY_FIRST;
                if session.mark_autosaved(path, as_of) {
                    self.word = Word::Saved;
                    self.first_unsaved = None;
                } else {
                    self.word = Word::Pending;
                }
            }
            Err(e) => {
                self.failures += 1;
                crate::log::warn("autosave", &format!("couldn't save {}: {e}", path.display()));
                self.error = Some(e);
                self.word = Word::Failed;
                self.retry_at = Some(now + self.backoff);
                self.backoff = (self.backoff * 2).min(RETRY_MOST);
            }
        }
    }
}

/// The graph as the file holds it, and where it goes; none when there is nothing to save.
fn snapshot(d: &Document) -> Option<Result<(String, PathBuf, u64), String>> {
    let s = &d.session;
    if !s.is_dirty() {
        return None;
    }
    let path = s.path()?.to_path_buf();
    Some(s.graph().to_json().map(|text| (text, path, s.changes())).map_err(|e| e.to_string()))
}

/// One look at a document's autosave, from its turn ([`crate::document::pump`]): take a save
/// that finished, and start one that is due. When to look again, if anything waits.
pub fn tick(d: &mut Document, now: Instant) -> Option<Duration> {
    if let Some(f) = &d.autosave.in_flight {
        let outcome = match f.rx.try_recv() {
            Ok(o) => o,
            Err(TryRecvError::Empty) => return Some(SOON),
            Err(TryRecvError::Disconnected) => Err("the save stopped without an answer".into()),
        };
        if let Some(f) = d.autosave.in_flight.take() {
            d.autosave.finished(outcome, f.path, f.as_of, &mut d.session, now);
        }
    }
    let a = &mut d.autosave;
    if !a.on {
        a.seen = Some(d.session.changes());
        return None;
    }
    a.note(d.session.changes(), now);
    if !d.session.is_dirty() {
        a.first_unsaved = None;
        if matches!(a.word, Word::Pending | Word::Untitled | Word::Failed) {
            a.word = Word::Clean;
            a.error = None;
        }
        return None;
    }
    if d.session.path().is_none() {
        a.word = Word::Untitled;
        return None;
    }
    if a.word != Word::Failed {
        a.word = Word::Pending;
    }
    // never while a run is in flight: the change waits for it to end
    if d.session.run.running {
        return Some(SOON);
    }
    if let Err(wait) = a.due(now) {
        return wait;
    }
    let (text, path, as_of) = match snapshot(d)? {
        Ok(s) => s,
        Err(e) => {
            let path = d.session.path().map(std::path::Path::to_path_buf).unwrap_or_default();
            let as_of = d.session.changes();
            d.autosave.finished(Err(e), path, as_of, &mut d.session, now);
            return d.autosave.retry_at.map(|t| t.saturating_duration_since(now));
        }
    };
    let (tx, rx) = channel();
    let wake = d.waker().cloned();
    let to = path.clone();
    let spawned = std::thread::Builder::new().name("plumcraft-autosave".into()).spawn(move || {
        let r = plumcraft_session::file_cmds::write_atomic(&to, &text).map_err(|e| e.to_string());
        let _ = tx.send(r);
        if let Some(w) = wake {
            w.wake();
        }
    });
    match spawned {
        Ok(_) => {
            d.autosave.in_flight = Some(InFlight { as_of, path, rx });
            d.autosave.word = Word::Saving;
            Some(SOON)
        }
        Err(e) => {
            d.autosave.finished(Err(e.to_string()), path, as_of, &mut d.session, now);
            d.autosave.retry_at.map(|t| t.saturating_duration_since(now))
        }
    }
}

/// Save now, whatever the quiet time and a run in flight (backgrounding, closing, quitting),
/// waiting at most `wait` for a save already being written. Ok(true): nothing is unsaved;
/// Ok(false): autosave is off, or the graph has no file; Err: the save failed (the document stays
/// dirty and the state says why).
pub fn flush(d: &mut Document, wait: Duration) -> Result<bool, String> {
    if let Some(f) = d.autosave.in_flight.take() {
        let outcome = match f.rx.recv_timeout(wait) {
            Ok(o) => o,
            Err(RecvTimeoutError::Timeout) => {
                // still being written: keep it, and say so
                d.autosave.in_flight = Some(f);
                return Err("a save is still being written".into());
            }
            Err(RecvTimeoutError::Disconnected) => Err("the save stopped without an answer".into()),
        };
        d.autosave.finished(outcome, f.path, f.as_of, &mut d.session, Instant::now());
    }
    if !d.autosave.on {
        return Ok(false);
    }
    let Some(snap) = snapshot(d) else { return Ok(d.session.path().is_some() || !d.session.is_dirty()) };
    let now = Instant::now();
    let (text, path, as_of) = snap?;
    let r = plumcraft_session::file_cmds::write_atomic(&path, &text).map_err(|e| e.to_string());
    let failed = r.as_ref().err().cloned();
    d.autosave.finished(r, path, as_of, &mut d.session, now);
    match failed {
        Some(e) => Err(e),
        None => Ok(!d.session.is_dirty()),
    }
}

/// `document.autosave {on?, path?, scratch?}` (the control channel's, N4 slice 4): turn autosave
/// on or off for this document, give it a file without writing it yet (`path`, or `scratch`: a
/// file of that name in the core's scratch folder), and read its state with what is on disk:
/// `{on, state, error?, saves, failures, path, dirty, disk?: {exists, matches, nodes}}`.
pub fn control(d: &mut Document, p: &Value) -> Result<Value, String> {
    crate::control::check_fields(p, &["on", "path", "scratch"])?;
    let on = match p.get("on") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err("on must be true or false".into()),
    };
    let path = match (p.get("path"), p.get("scratch")) {
        (Some(_), Some(_)) => return Err("give path or scratch, not both".into()),
        (Some(Value::String(s)), None) if !s.trim().is_empty() && s.len() <= 4096 && !s.contains('\0') => Some(PathBuf::from(s)),
        (None, Some(Value::String(n))) if !n.is_empty() && n.len() <= 128 && !n.contains(['/', '\\', '\0']) && !n.starts_with('.') => {
            // the shell's scratch folder when it gave one (B42: never a folder it didn't give)
            Some(match &d.scratch {
                Some(dir) => {
                    let dir = dir.join("autosave");
                    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                    dir.join(n)
                }
                None => plumcraft_scratch::dir("autosave").join(n),
            })
        }
        (None | Some(Value::Null), None | Some(Value::Null)) => None,
        _ => return Err("path must be a file's path, scratch a file name".into()),
    };
    if let Some(path) = path {
        let _ = std::fs::remove_file(&path).ok().filter(|_| p.get("scratch").is_some());
        d.session.relocate(path);
    }
    if let Some(on) = on {
        d.autosave.set_on(on);
    }
    Ok(describe(d))
}

/// The state with what is on disk.
pub fn describe(d: &Document) -> Value {
    let mut v = d.autosave.status();
    let s = &d.session;
    v["path"] = json!(s.path().map(|p| p.display().to_string()));
    v["dirty"] = json!(s.is_dirty());
    if let Some(path) = s.path() {
        let read = std::fs::metadata(path).ok().filter(|m| m.len() <= MOST_READ).and_then(|_| std::fs::read_to_string(path).ok());
        v["disk"] = match read {
            None => json!({"exists": path.exists(), "matches": false}),
            // the file holds exactly the graph as it is now, written as a save writes it
            Some(text) => match plumcraft_graph::Graph::from_json(&text) {
                Ok(g) => json!({"exists": true, "matches": s.graph().to_json().is_ok_and(|now| now == text), "nodes": g.nodes.len(), "frames": g.frames.len()}),
                Err(_) => json!({"exists": true, "matches": false}),
            },
        };
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_session::Session;

    fn doc_at(path: &std::path::Path) -> Document {
        let mut d = Document::new(Session::default());
        d.session.relocate(path.to_path_buf());
        d.autosave.set_on(true);
        d
    }

    fn change(d: &mut Document, n: u32) {
        d.session
            .edit("Edit", |g, _| {
                g.meta.insert("n".into(), json!(n));
                Ok(())
            })
            .unwrap();
    }

    /// Tick until the save in flight is taken (the write is on a thread of its own), at most a
    /// generous minute: no wall-clock bound is asserted, only that it ends.
    fn settle(d: &mut Document, now: Instant) {
        let until = Instant::now() + Duration::from_secs(60);
        while d.autosave.in_flight.is_some() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
            tick(d, now);
        }
    }

    fn on_disk(path: &std::path::Path) -> Option<u64> {
        let g = plumcraft_graph::Graph::from_json(&std::fs::read_to_string(path).ok()?).ok()?;
        g.meta.get("n").and_then(Value::as_u64)
    }

    #[test]
    fn a_change_is_saved_once_it_settles_and_not_before() {
        let dir = plumcraft_scratch::dir("autosave-settles");
        let path = dir.join("g.json");
        let _ = std::fs::remove_file(&path);
        let mut d = doc_at(&path);
        let t0 = Instant::now();
        tick(&mut d, t0);
        change(&mut d, 1);
        // the clock is the test's: a tick half a quiet time later saves nothing
        assert!(tick(&mut d, t0 + QUIET / 2).is_some());
        assert_eq!(d.autosave.word(), Word::Pending);
        assert!(!path.exists());
        // another change restarts the quiet time
        change(&mut d, 2);
        tick(&mut d, t0 + QUIET * 3 / 4);
        assert!(tick(&mut d, t0 + QUIET * 3 / 2).is_some(), "not yet settled since the second change");
        assert!(!path.exists());
        tick(&mut d, t0 + QUIET * 2);
        assert_eq!(d.autosave.word(), Word::Saving);
        settle(&mut d, t0 + QUIET * 2);
        assert_eq!(d.autosave.word(), Word::Saved);
        assert!(!d.session.is_dirty());
        assert_eq!(on_disk(&path), Some(2), "the file holds the last change");
        assert_eq!(describe(&d)["disk"]["matches"], json!(true));
    }

    #[test]
    fn changes_that_keep_coming_are_saved_at_the_longest_wait() {
        let dir = plumcraft_scratch::dir("autosave-longest");
        let path = dir.join("g.json");
        let _ = std::fs::remove_file(&path);
        let mut d = doc_at(&path);
        let t0 = Instant::now();
        tick(&mut d, t0);
        let mut t = t0;
        let mut n = 0;
        while t < t0 + LONGEST {
            n += 1;
            change(&mut d, n);
            tick(&mut d, t);
            assert!(d.autosave.in_flight.is_none() || t >= t0 + LONGEST);
            t += QUIET / 2;
        }
        tick(&mut d, t0 + LONGEST + QUIET / 4);
        settle(&mut d, t0 + LONGEST + QUIET / 4);
        assert!(on_disk(&path).is_some(), "saved within the longest wait though changes kept coming");
    }

    #[test]
    fn a_save_of_an_older_change_leaves_the_document_dirty_for_the_next() {
        let dir = plumcraft_scratch::dir("autosave-older");
        let path = dir.join("g.json");
        let _ = std::fs::remove_file(&path);
        let mut d = doc_at(&path);
        let t0 = Instant::now();
        tick(&mut d, t0);
        change(&mut d, 1);
        tick(&mut d, t0);
        tick(&mut d, t0 + QUIET * 2);
        assert_eq!(d.autosave.word(), Word::Saving);
        // a change while the save is written
        change(&mut d, 2);
        settle(&mut d, t0 + QUIET * 2);
        assert!(d.session.is_dirty(), "change 2 isn't on disk yet");
        assert_eq!(on_disk(&path), Some(1));
        tick(&mut d, t0 + QUIET * 4);
        settle(&mut d, t0 + QUIET * 4);
        assert_eq!(on_disk(&path), Some(2));
        assert!(!d.session.is_dirty());
    }

    #[test]
    fn nothing_is_saved_while_a_run_is_in_flight_and_flush_saves_at_once() {
        let dir = plumcraft_scratch::dir("autosave-run");
        let path = dir.join("g.json");
        let _ = std::fs::remove_file(&path);
        let mut d = doc_at(&path);
        let t0 = Instant::now();
        tick(&mut d, t0);
        change(&mut d, 7);
        d.session.run.running = true;
        tick(&mut d, t0 + QUIET * 5);
        assert!(d.autosave.in_flight.is_none() && !path.exists(), "held while the run goes on");
        // backgrounding or quitting: saved now, run or no run
        assert_eq!(flush(&mut d, Duration::from_secs(30)), Ok(true));
        assert_eq!(on_disk(&path), Some(7));
        assert!(!d.session.is_dirty());
    }

    #[test]
    fn a_failure_is_reported_kept_dirty_and_tried_again_later() {
        let mut d = doc_at(std::path::Path::new("/plumcraft-nowhere/sub/g.json"));
        let t0 = Instant::now();
        tick(&mut d, t0);
        change(&mut d, 1);
        tick(&mut d, t0);
        tick(&mut d, t0 + QUIET * 2);
        settle(&mut d, t0 + QUIET * 2);
        assert_eq!(d.autosave.word(), Word::Failed);
        assert!(d.autosave.error().is_some_and(|e| !e.is_empty()));
        assert!(d.session.is_dirty());
        assert_eq!(d.autosave.status()["failures"], json!(1));
        // not tried again within the back-off
        tick(&mut d, t0 + QUIET * 2 + RETRY_FIRST / 2);
        assert!(d.autosave.in_flight.is_none());
        tick(&mut d, t0 + QUIET * 3 + RETRY_FIRST);
        settle(&mut d, t0 + QUIET * 3 + RETRY_FIRST);
        assert_eq!(d.autosave.status()["failures"], json!(2), "tried again after the back-off");
        assert!(flush(&mut d, Duration::from_secs(30)).is_err());
        // what is at the file's place is left as it was by a failed write (the write is atomic:
        // a temporary file beside it, then a rename), and no temporary file stays
        let dir = plumcraft_scratch::dir("autosave-atomic");
        let blocked = dir.join("kept.json");
        let _ = std::fs::create_dir_all(&blocked);
        std::fs::write(blocked.join("inner"), "kept").unwrap();
        let mut d = doc_at(&blocked);
        change(&mut d, 3);
        assert!(flush(&mut d, Duration::from_secs(30)).is_err());
        assert_eq!(std::fs::read_to_string(blocked.join("inner")).unwrap(), "kept");
        let left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".tmp-")).collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn untitled_and_off_save_nothing_and_the_control_method_says_so() {
        let mut d = Document::new(Session::default());
        assert_eq!(control(&mut d, &json!({})).unwrap()["state"], json!("off"));
        let v = control(&mut d, &json!({"on": true})).unwrap();
        assert_eq!(v["state"], json!("clean"));
        change(&mut d, 1);
        tick(&mut d, Instant::now());
        tick(&mut d, Instant::now() + LONGEST * 2);
        assert_eq!(d.autosave.word(), Word::Untitled);
        let v = control(&mut d, &json!({"scratch": "autosave-control.json"})).unwrap();
        assert_eq!(v["dirty"], json!(true));
        assert_eq!(v["disk"]["exists"], json!(false));
        for bad in [json!({"on": "yes"}), json!({"scratch": "../x"}), json!({"path": 3}), json!({"path": "/a", "scratch": "b"}), json!({"other": 1})] {
            assert!(control(&mut d, &bad).is_err(), "{bad}");
        }
        assert_eq!(control(&mut d, &json!({"on": false})).unwrap()["state"], json!("off"));
    }
}
