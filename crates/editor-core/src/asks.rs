//! Asks: what the core needs from the platform, asked for and answered later
//! (`docs/shell-interface.md` section 11).
//!
//! The core never calls a dialog, a prompt window, the clipboard or another app. It raises an
//! ask `{ask, kind, modal, by, params}`; the shell presents it with the platform's own UI and
//! answers later with [`Asks::answer`] (or [`Asks::cancel`]). Nothing blocks: an iOS document
//! picker that answers seconds later is the same as rfd answering at once.
//!
//! Every answer is hostile input: an unknown id, an ask already answered, cancelled, withdrawn or
//! timed out, or an answer of the wrong shape is refused with a reason ([`Refusal`]), never a
//! panic. A wrong shape leaves the ask open, so the shell (or script) can answer again.
//!
//! Life of an ask:
//!
//! - **Raised** by the core, with a continuation `T` (what to do with the answer: the invoke
//!   machine's [`crate::invoke::Cont`]). A newer ask of the same kind withdraws an open one of
//!   that kind for the modal kinds and `clipboard.get` (one dialog of a kind at a time, as today).
//! - **Answered** or **cancelled** once: the first answer wins.
//! - **Withdrawn** when its reason went (the document closed, the node it would set was removed).
//! - **Timed out**: only asks raised with a timeout (`clipboard.get`, whose answer may never come
//!   when the clipboard is empty). Asks a person answers have none.
//!
//! `by` says who started the chain the ask belongs to: a person (a menu, a shortcut, a button)
//! or a script (`ui.invoke`, `ask.answer`). The egui shell presents a person's file asks at once
//! with its native dialogs, and leaves a script's for the script to answer.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

/// An ask's id: unique within its queue, never reused.
pub type AskId = u64;

/// The longest path an answer may carry, in bytes.
pub const MAX_PATH: usize = 4096;
/// The most paths a `file.open` answer may carry.
pub const MAX_PATHS: usize = 10_000;
/// The longest text an answer may carry (a clipboard's text, a form field), in bytes.
pub const MAX_TEXT: usize = 64 << 20;
/// How many ended asks are remembered, so a late answer to one is told why, not "unknown".
const REMEMBERED: usize = 256;
/// The most asks open at once; the oldest is withdrawn past it (a script that never answers).
pub const MAX_OPEN: usize = 64;

/// What is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    FileOpen,
    FileSave,
    FolderPick,
    Prompt,
    Form,
    Text,
    ClipboardSet,
    ClipboardGet,
    Share,
    OpenExternal,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::FileOpen,
        Kind::FileSave,
        Kind::FolderPick,
        Kind::Prompt,
        Kind::Form,
        Kind::Text,
        Kind::ClipboardSet,
        Kind::ClipboardGet,
        Kind::Share,
        Kind::OpenExternal,
    ];

    /// Its name in the feed and on the control channel.
    pub fn name(self) -> &'static str {
        match self {
            Kind::FileOpen => "file.open",
            Kind::FileSave => "file.save",
            Kind::FolderPick => "folder.pick",
            Kind::Prompt => "prompt",
            Kind::Form => "form",
            Kind::Text => "text",
            Kind::ClipboardSet => "clipboard.set",
            Kind::ClipboardGet => "clipboard.get",
            Kind::Share => "share",
            Kind::OpenExternal => "open.external",
        }
    }

    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// One at a time per document; while one is open the document's keyboard shortcuts wait.
    pub fn modal(self) -> bool {
        matches!(self, Kind::FileOpen | Kind::FileSave | Kind::FolderPick | Kind::Prompt | Kind::Form | Kind::Text)
    }

    /// A file dialog of some sort.
    pub fn is_file(self) -> bool {
        matches!(self, Kind::FileOpen | Kind::FileSave | Kind::FolderPick)
    }
}

/// Who started the chain an ask belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum By {
    /// A menu, a shortcut, a button: the shell presents the ask with its own UI.
    #[default]
    Person,
    /// A control client (`ui.invoke`, `ask.answer`): it answers with `ask.answer`.
    Script,
}

impl By {
    pub fn name(self) -> &'static str {
        match self {
            By::Person => "person",
            By::Script => "script",
        }
    }
}

/// One open ask.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub id: AskId,
    pub kind: Kind,
    pub params: Value,
    pub by: By,
    pub raised: Instant,
    /// When it times out unanswered (none for what a person answers).
    pub timeout: Option<Duration>,
}

impl Ask {
    /// As the feed and `ui.inspect`'s `asks` show it.
    pub fn to_json(&self) -> Value {
        json!({"ask": self.id, "kind": self.kind.name(), "modal": self.kind.modal(), "by": self.by.name(), "params": self.params})
    }

    /// `params[key]` as a string.
    pub fn param_str(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(Value::as_str)
    }
}

/// An answer, checked against its ask's kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    /// `file.open`: one or more paths (one unless the ask said `multiple`).
    Paths(Vec<PathBuf>),
    /// `file.save`, `folder.pick`.
    Path(PathBuf),
    /// `prompt`: one of the ask's `answers`.
    Choice(String),
    /// `form`: text by field key.
    Values(Map<String, Value>),
    /// `text`, `clipboard.get`.
    Text(String),
    /// `clipboard.set`, `share`, `open.external`: done, perhaps with a word for the status bar.
    Done(Option<String>),
    /// `clipboard.set`, `share`, `open.external`: it couldn't be done, and why.
    Failed(String),
}

/// How an ask ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    Answered,
    Cancelled,
    Withdrawn,
    TimedOut,
}

impl Ended {
    pub fn name(self) -> &'static str {
        match self {
            Ended::Answered => "answered",
            Ended::Cancelled => "cancelled",
            Ended::Withdrawn => "withdrawn",
            Ended::TimedOut => "timedOut",
        }
    }
}

/// Why an answer (or a cancel) was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No such ask was ever raised here (or it ended long ago).
    Unknown(AskId),
    /// It has ended already.
    Ended(AskId, Ended),
    /// The answer has the wrong shape for its kind; the ask stays open.
    Shape(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Unknown(id) => write!(f, "no ask {id} is open"),
            Refusal::Ended(id, how) => write!(f, "ask {id} was already {}", how.name()),
            Refusal::Shape(why) => write!(f, "{why}"),
        }
    }
}

/// The asks of one document, each with what to do once it is answered (`T`).
#[derive(Debug)]
pub struct Asks<T> {
    open: Vec<(Ask, T)>,
    next: AskId,
    ended: VecDeque<(AskId, Ended)>,
    /// Raised since the feed was last taken.
    raised_feed: Vec<Value>,
    /// Ended since the feed was last taken: `{ask, ended}`.
    ended_feed: Vec<Value>,
    /// Counts every change, so a shell can tell its presentation is stale.
    pub generation: u64,
}

impl<T> Default for Asks<T> {
    fn default() -> Self {
        Asks { open: Vec::new(), next: 1, ended: VecDeque::new(), raised_feed: Vec::new(), ended_feed: Vec::new(), generation: 0 }
    }
}

impl<T> Asks<T> {
    /// Raise an ask; its id, and the asks it withdrew (a newer of the same kind), with their
    /// continuations, which the caller drops.
    pub fn raise(&mut self, kind: Kind, params: Value, by: By, then: T) -> (AskId, Vec<(Ask, T)>) {
        self.raise_with(kind, params, by, None, then)
    }

    /// [`Asks::raise`], timing out after `timeout`.
    pub fn raise_with(&mut self, kind: Kind, params: Value, by: By, timeout: Option<Duration>, then: T) -> (AskId, Vec<(Ask, T)>) {
        let mut gone = Vec::new();
        if kind.modal() || kind == Kind::ClipboardGet {
            gone = self.withdraw(|a| a.kind == kind);
        }
        if self.open.len() >= MAX_OPEN {
            let (ask, t) = self.open.remove(0);
            self.end(ask.id, Ended::Withdrawn);
            gone.push((ask, t));
        }
        let id = self.next;
        self.next = self.next.saturating_add(1);
        let ask = Ask { id, kind, params, by, raised: Instant::now(), timeout };
        self.raised_feed.push(ask.to_json());
        self.open.push((ask, then));
        self.generation += 1;
        (id, gone)
    }

    /// Put an ask back as it was, with the same id (a form whose command refused: it stays
    /// open, saying why).
    pub fn reopen(&mut self, ask: Ask, then: T) {
        if let Some(i) = self.ended.iter().position(|(id, _)| *id == ask.id) {
            self.ended.remove(i);
        }
        self.ended_feed.retain(|v| v["ask"] != json!(ask.id));
        self.raised_feed.push(ask.to_json());
        self.open.push((ask, then));
        self.generation += 1;
    }

    fn end(&mut self, id: AskId, how: Ended) {
        if self.ended.len() >= REMEMBERED {
            self.ended.pop_front();
        }
        self.ended.push_back((id, how));
        self.ended_feed.push(json!({"ask": id, "ended": how.name()}));
        self.generation += 1;
    }

    fn refuse(&self, id: AskId) -> Refusal {
        match self.ended.iter().rev().find(|(i, _)| *i == id) {
            Some((_, how)) => Refusal::Ended(id, *how),
            None => Refusal::Unknown(id),
        }
    }

    /// Answer ask `id` with `answer` (the shape its kind takes). On success the ask has ended
    /// and its continuation is the caller's; a wrong shape leaves it open.
    pub fn answer(&mut self, id: AskId, answer: &Value) -> Result<(Ask, T, Answer), Refusal> {
        let Some(i) = self.open.iter().position(|(a, _)| a.id == id) else { return Err(self.refuse(id)) };
        let parsed = parse(&self.open[i].0, answer).map_err(Refusal::Shape)?;
        let (ask, then) = self.open.remove(i);
        self.end(id, Ended::Answered);
        Ok((ask, then, parsed))
    }

    /// Cancel ask `id` (the dialog dismissed, Esc, a script's `cancel: true`).
    pub fn cancel(&mut self, id: AskId) -> Result<(Ask, T), Refusal> {
        let Some(i) = self.open.iter().position(|(a, _)| a.id == id) else { return Err(self.refuse(id)) };
        let (ask, then) = self.open.remove(i);
        self.end(id, Ended::Cancelled);
        Ok((ask, then))
    }

    /// Withdraw every open ask `which` picks; they are returned with their continuations.
    pub fn withdraw(&mut self, which: impl Fn(&Ask) -> bool) -> Vec<(Ask, T)> {
        let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.open).into_iter().partition(|(a, _)| which(a));
        self.open = keep;
        for (a, _) in &gone {
            self.end(a.id, Ended::Withdrawn);
        }
        gone
    }

    /// Withdraw everything (the document closed).
    pub fn withdraw_all(&mut self) -> Vec<(Ask, T)> {
        self.withdraw(|_| true)
    }

    /// The asks whose time ran out by `now`, ended as timed out.
    pub fn expire(&mut self, now: Instant) -> Vec<(Ask, T)> {
        let late = |a: &Ask| a.timeout.is_some_and(|t| now.duration_since(a.raised) >= t);
        if !self.open.iter().any(|(a, _)| late(a)) {
            return Vec::new();
        }
        let (gone, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.open).into_iter().partition(|(a, _)| late(a));
        self.open = keep;
        for (a, _) in &gone {
            self.end(a.id, Ended::TimedOut);
        }
        gone
    }

    /// The open asks, oldest first.
    pub fn open(&self) -> impl Iterator<Item = &Ask> {
        self.open.iter().map(|(a, _)| a)
    }

    /// The open asks with their continuations.
    pub fn entries(&self) -> impl Iterator<Item = (&Ask, &T)> {
        self.open.iter().map(|(a, t)| (a, t))
    }

    pub fn get(&self, id: AskId) -> Option<&Ask> {
        self.open().find(|a| a.id == id)
    }

    /// The oldest open ask of `kind`.
    pub fn first(&self, kind: Kind) -> Option<&Ask> {
        self.open().find(|a| a.kind == kind)
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    pub fn len(&self) -> usize {
        self.open.len()
    }

    /// A modal ask is open.
    pub fn modal_open(&self) -> bool {
        self.open().any(|a| a.kind.modal())
    }

    /// The open asks as JSON, oldest first (`ui.inspect`'s `asks`).
    pub fn inspect(&self) -> Value {
        Value::Array(self.open().map(Ask::to_json).collect())
    }

    /// What was raised and what ended since the last call (the FFI's feed).
    pub fn take_feed(&mut self) -> (Vec<Value>, Vec<Value>) {
        (std::mem::take(&mut self.raised_feed), std::mem::take(&mut self.ended_feed))
    }
}

/// `v` as an object with only `allowed` keys.
fn fields<'a>(v: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>, String> {
    let o = v.as_object().ok_or("an answer is a JSON object")?;
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!("unknown field {:?} in the answer (allowed: {})", k.chars().take(64).collect::<String>(), allowed.join(", ")));
    }
    Ok(o)
}

fn path_of(v: Option<&Value>, what: &str) -> Result<PathBuf, String> {
    let p = v.and_then(Value::as_str).ok_or_else(|| format!("{what} must be a path"))?;
    if p.trim().is_empty() || p.len() > MAX_PATH || p.contains('\0') {
        return Err(format!("{what} must be a path of 1 to {MAX_PATH} bytes"));
    }
    Ok(PathBuf::from(p))
}

fn text_of(v: Option<&Value>, what: &str) -> Result<String, String> {
    let t = v.and_then(Value::as_str).ok_or_else(|| format!("{what} must be text"))?;
    if t.len() > MAX_TEXT {
        return Err(format!("{what} is longer than {MAX_TEXT} bytes"));
    }
    Ok(t.to_owned())
}

/// The answer's notice or error (`clipboard.set`, `share`, `open.external`).
fn done_of(v: &Value) -> Result<Answer, String> {
    let o = fields(v, &["notice", "error"])?;
    match (o.get("notice"), o.get("error")) {
        (_, Some(e)) => Ok(Answer::Failed(text_of(Some(e), "error")?.chars().take(1024).collect())),
        (Some(n), None) => Ok(Answer::Done(Some(text_of(Some(n), "notice")?.chars().take(256).collect()))),
        (None, None) => Ok(Answer::Done(None)),
    }
}

/// Check `v` against what `ask`'s kind takes.
fn parse(ask: &Ask, v: &Value) -> Result<Answer, String> {
    match ask.kind {
        Kind::FileOpen => {
            let o = fields(v, &["paths", "grants"])?;
            let list = o.get("paths").and_then(Value::as_array).ok_or("paths must be a list of paths (cancel instead to choose nothing)")?;
            if list.is_empty() || list.len() > MAX_PATHS {
                return Err(format!("paths must name 1 to {MAX_PATHS} files (cancel instead to choose nothing)"));
            }
            if list.len() > 1 && ask.params.get("multiple").and_then(Value::as_bool) != Some(true) {
                return Err("this ask takes one path".into());
            }
            Ok(Answer::Paths(list.iter().map(|p| path_of(Some(p), "each of paths")).collect::<Result<_, _>>()?))
        }
        Kind::FileSave | Kind::FolderPick => {
            let o = fields(v, &["path", "grant", "commit"])?;
            if o.get("commit").and_then(Value::as_bool) == Some(true) {
                return Err("commit (a save streamed to its destination) isn't supported yet: answer with a path to write".into());
            }
            Ok(Answer::Path(path_of(o.get("path"), "path")?))
        }
        Kind::Prompt => {
            let o = fields(v, &["answer"])?;
            let a = o.get("answer").and_then(Value::as_str).ok_or("answer must be one of the prompt's answers")?;
            let answers: Vec<&str> = ask.params.get("answers").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
            if !answers.contains(&a) {
                return Err(format!("answer must be one of {}", answers.join(", ")));
            }
            Ok(Answer::Choice(a.to_owned()))
        }
        Kind::Form => {
            let o = fields(v, &["values"])?;
            let values = o.get("values").and_then(Value::as_object).ok_or("values must be an object of text by field key")?;
            let keys: Vec<&str> =
                ask.params.get("fields").and_then(Value::as_array).into_iter().flatten().filter_map(|f| f.get("key").and_then(Value::as_str)).collect();
            for (k, v) in values {
                if !keys.contains(&k.as_str()) {
                    return Err(format!("the form has no field {:?}", k.chars().take(64).collect::<String>()));
                }
                text_of(Some(v), k)?;
            }
            Ok(Answer::Values(values.clone()))
        }
        Kind::Text => {
            let o = fields(v, &["text"])?;
            Ok(Answer::Text(text_of(o.get("text"), "text")?))
        }
        Kind::ClipboardGet => {
            let o = fields(v, &["text"])?;
            Ok(Answer::Text(text_of(o.get("text"), "text")?))
        }
        Kind::ClipboardSet | Kind::Share | Kind::OpenExternal => done_of(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save_ask(q: &mut Asks<&'static str>) -> AskId {
        q.raise(Kind::FileSave, json!({"purpose": "workflow"}), By::Person, "save").0
    }

    #[test]
    fn an_ask_is_answered_once_and_late_or_unknown_answers_are_refused() {
        let mut q: Asks<&str> = Asks::default();
        let id = save_ask(&mut q);
        assert_eq!(q.inspect()[0]["kind"], "file.save");
        assert_eq!(q.inspect()[0]["modal"], true);
        let (ask, then, answer) = q.answer(id, &json!({"path": "/tmp/a.json"})).unwrap();
        assert_eq!((ask.id, then, answer), (id, "save", Answer::Path("/tmp/a.json".into())));
        assert_eq!(q.answer(id, &json!({"path": "/tmp/b.json"})).unwrap_err(), Refusal::Ended(id, Ended::Answered));
        assert_eq!(q.cancel(id).unwrap_err(), Refusal::Ended(id, Ended::Answered));
        assert_eq!(q.answer(999, &json!({})).unwrap_err(), Refusal::Unknown(999));
        let id = save_ask(&mut q);
        q.cancel(id).unwrap();
        assert_eq!(q.answer(id, &json!({"path": "/x"})).unwrap_err().to_string(), format!("ask {id} was already cancelled"));
    }

    #[test]
    fn hostile_answers_are_refused_and_leave_the_ask_open() {
        let mut q: Asks<()> = Asks::default();
        let save = q.raise(Kind::FileSave, json!({}), By::Script, ()).0;
        let open = q.raise(Kind::FileOpen, json!({"multiple": false}), By::Script, ()).0;
        let prompt = q.raise(Kind::Prompt, json!({"answers": ["save", "discard", "cancel"]}), By::Script, ()).0;
        let form = q.raise(Kind::Form, json!({"fields": [{"key": "name"}]}), By::Script, ()).0;
        let bad: Vec<(AskId, Value)> = vec![
            (save, json!(null)),
            (save, json!([1])),
            (save, json!("x")),
            (save, json!({"path": 3})),
            (save, json!({"path": ""})),
            (save, json!({"path": "a\0b"})),
            (save, json!({"path": "x".repeat(MAX_PATH + 1)})),
            (save, json!({"path": "/a", "evil": true})),
            (save, json!({"path": "/a", "commit": true})),
            (open, json!({"paths": []})),
            (open, json!({"paths": "/a"})),
            (open, json!({"paths": ["/a", "/b"]})),
            (open, json!({"paths": [1]})),
            (prompt, json!({"answer": "trust"})),
            (prompt, json!({"answer": 1})),
            (form, json!({"values": {"nope": "x"}})),
            (form, json!({"values": {"name": 1}})),
            (form, json!({"values": []})),
        ];
        for (id, v) in bad {
            let r = q.answer(id, &v);
            assert!(matches!(r, Err(Refusal::Shape(_))), "{v}: {r:?}");
        }
        assert_eq!(q.len(), 4, "every ask is still open");
        assert!(q.answer(open, &json!({"paths": ["/a"]})).is_ok());
        assert!(q.answer(prompt, &json!({"answer": "discard"})).is_ok());
        assert!(q.answer(form, &json!({"values": {"name": "Ada"}})).is_ok());
    }

    #[test]
    fn a_newer_modal_ask_of_a_kind_withdraws_the_older_and_ends_are_fed() {
        let mut q: Asks<u8> = Asks::default();
        let a = q.raise(Kind::Prompt, json!({"answers": ["cancel"]}), By::Person, 1).0;
        let c = q.raise(Kind::ClipboardSet, json!({}), By::Person, 3).0;
        let (b, gone) = q.raise(Kind::Prompt, json!({"answers": ["cancel"]}), By::Person, 2);
        assert_eq!(gone.into_iter().map(|(a, t)| (a.id, t)).collect::<Vec<_>>(), [(a, 1)]);
        assert_eq!(q.answer(a, &json!({"answer": "cancel"})).unwrap_err(), Refusal::Ended(a, Ended::Withdrawn));
        assert_eq!(q.open().map(|x| x.id).collect::<Vec<_>>(), [c, b], "a non-modal ask stays");
        let (raised, ended) = q.take_feed();
        assert_eq!(raised.len(), 3);
        assert_eq!(ended, [json!({"ask": a, "ended": "withdrawn"})]);
        assert_eq!(q.take_feed(), (vec![], vec![]));
        assert_eq!(q.withdraw_all().len(), 2);
        assert!(q.is_empty());
    }

    #[test]
    fn an_ask_with_a_timeout_expires_and_others_never_do() {
        let mut q: Asks<()> = Asks::default();
        let get = q.raise_with(Kind::ClipboardGet, json!({}), By::Person, Some(Duration::from_millis(1)), ()).0;
        let keep = save_ask_unit(&mut q);
        let later = Instant::now() + Duration::from_secs(3600);
        let gone = q.expire(later);
        assert_eq!(gone.iter().map(|(a, _)| a.id).collect::<Vec<_>>(), [get]);
        assert_eq!(q.answer(get, &json!({"text": "x"})).unwrap_err(), Refusal::Ended(get, Ended::TimedOut));
        assert!(q.get(keep).is_some());
    }

    fn save_ask_unit(q: &mut Asks<()>) -> AskId {
        q.raise(Kind::FileSave, json!({}), By::Person, ()).0
    }

    #[test]
    fn a_reopened_ask_keeps_its_id_and_the_queue_is_bounded() {
        let mut q: Asks<()> = Asks::default();
        let f = q.raise(Kind::Form, json!({"fields": []}), By::Person, ()).0;
        let (ask, (), _) = q.answer(f, &json!({"values": {}})).unwrap();
        q.reopen(ask, ());
        assert!(q.get(f).is_some());
        assert!(q.answer(f, &json!({"values": {}})).is_ok());
        for _ in 0..MAX_OPEN + 5 {
            q.raise(Kind::ClipboardSet, json!({}), By::Script, ());
        }
        assert_eq!(q.len(), MAX_OPEN);
        assert!(Kind::ALL.iter().all(|k| Kind::from_name(k.name()) == Some(*k)));
    }
}
