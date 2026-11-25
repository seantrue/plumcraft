//! The status line, the core's (research note 28; the maintainer: "The information in the original
//! egui status line was timely, centralized, and includes more than I can easily see now"): what
//! the egui editor's bottom bar showed, as a model every shell draws and none composes. Segments,
//! left to right, each `{id, role, side, priority, text, detail?, tone, dot?, busy?, on?,
//! command?, tooltip?}`:
//!
//! - `engine` (role `engine`): the engine's short name, a dot (`ok`, `busy`, `down`), the long
//!   label as its tooltip, a manifest warning (tone `warn`), and the connected engine's own status
//!   text as its detail (an application's engine says it through its catalogue texts);
//! - `install` while an engine installs; `document`: its name, `•` when edited, the graph's size as
//!   detail; `selection` when something is; `hash`: the hash and where it came from;
//! - `run`: running (count, time, `busy`, Cancel as its command), or the last run's time with the
//!   engine's time and the cache as detail (the Cache Report its command), or "not run yet";
//! - `store` when the store refused writes or the disk is low (the Cache Report);
//! - `auto` (role `toggle`, `on`): Auto-run; `save` while a save is pending; `convert` when a
//!   refused wire offers a converter (its command places it);
//! - `message.0`, `message.1`, …: the messages, most serious first ([`crate::control::status_messages`]),
//!   each opening the Messages window. They stay until what they say changes: the last error
//!   until a command clears it, after its banner has gone.
//!
//! `short`, where one makes sense, is the segment's text for a narrow line (the engine's name, the
//! run's time or count, the hash's algorithm). A shell shows a segment whole or not at all: when
//! the line is too narrow it drops the detail lines, then the segments of priority 3 and 2, then
//! takes the short forms, then drops priority 1 from the end; the newest message keeps its place
//! and is cut only when it alone is wider than the line (the Swift kit's `StatusLine.fit`).
//!
//! `priority` 1 is what a narrow window keeps (the engine, the run, the newest message); 2 and 3
//! go first as the width runs out. `on`: View › Status Line (`view.statusLine`).

use std::time::Instant;

use plumcraft_session::commands::Tone;
use serde_json::{Value, json};

use crate::link::Link;
use plumcraft_proto::session::EngineState;

/// What the line is made from: the document's parts a shell's `View` lends.
pub struct Parts<'a> {
    pub session: &'a plumcraft_session::Session,
    pub link: Option<&'a Link>,
    pub state: &'a EngineState,
    pub last_error: Option<&'a str>,
    /// An engine install under way: its URL and what it is doing.
    pub installing: Option<(&'a str, Option<&'a str>)>,
    /// A refused wire's converter, offered: its params and label.
    pub convert_offer: Option<&'a (Value, String)>,
    /// Selected nodes and groups.
    pub selected: usize,
    /// The document's name as the title gives it.
    pub document: &'a str,
    /// An application's own segment (its text and tone), where a shell keeps one.
    pub app: Option<(&'a str, Tone)>,
}

fn tone_word(t: Tone) -> &'static str {
    match t {
        Tone::Info => "info",
        Tone::Warn => "warn",
        Tone::Error => "error",
    }
}

fn seg(id: &str, role: &str, side: &str, priority: u8, text: impl Into<String>) -> Value {
    json!({"id": id, "role": role, "side": side, "priority": priority, "text": text.into(), "tone": "info"})
}

/// The hash cell's words: `hash b3 · policy file` (the algorithm and where it came from).
pub fn hash_label(algo: &str, source: &str) -> String {
    let kind = if source.starts_with("policy file") {
        "policy file"
    } else if source.starts_with("environment") {
        "environment"
    } else {
        source
    };
    format!("hash {algo} · {kind}")
}

/// The line (module docs), at `now`.
pub fn model(p: &Parts<'_>, now: Instant) -> Value {
    let s = p.session;
    let mut out: Vec<Value> = Vec::new();
    // the engine
    let dot = if p.state.is_connected() {
        "ok"
    } else if matches!(p.state, EngineState::Connecting { .. } | EngineState::LoadingSchema) {
        "busy"
    } else {
        "down"
    };
    let mut e = seg("engine", "engine", "left", 1, crate::switchboard::engine_short_label(s, p.link, p.state));
    e["dot"] = json!(dot);
    // the short form: the engine's name, its state being the dot's
    let name = &s.catalog().engine.name;
    if !name.is_empty() {
        e["short"] = json!(name);
    }
    let long = crate::switchboard::engine_label(s, p.link, p.state);
    match crate::switchboard::engine_warning(s) {
        Some(w) => {
            e["tone"] = json!("warn");
            e["tooltip"] = json!(format!("{w}\n\n{long}"));
        }
        None => e["tooltip"] = json!(long),
    }
    if let Some((text, tone)) = crate::control::engine_status(s, p.state) {
        e["detail"] = json!(text);
        e["detailTone"] = json!(tone_word(tone));
    }
    out.push(e);
    if let Some((url, status)) = p.installing {
        let mut i = seg("install", "progress", "left", 2, format!("installing {}…", url.trim_start_matches("https://github.com/")));
        i["busy"] = json!(true);
        if let Some(st) = status {
            i["tooltip"] = json!(st);
        }
        out.push(i);
    }
    // the document
    let edited = if s.is_dirty() { " •" } else { "" };
    let g = s.graph();
    let mut d = seg("document", "document", "left", 2, format!("{}{edited}", p.document));
    d["detail"] = json!(format!("{} nodes, {} wires", g.nodes.len(), g.wires.len()));
    d["tooltip"] = json!(match s.path() {
        Some(path) => format!("{}{}", path.display(), if edited.is_empty() { "" } else { " (unsaved changes)" }),
        None => "not saved yet".into(),
    });
    out.push(d);
    if p.selected > 0 {
        let mut sel = seg("selection", "selection", "left", 3, format!("{} selected", p.selected));
        sel["tooltip"] = json!("The nodes and groups selected");
        out.push(sel);
    }
    let source = plumcraft_proto::handle::Algo::source();
    let algo = plumcraft_proto::handle::Algo::current();
    let mut h = seg("hash", "hash", "left", 3, hash_label(algo.as_str(), &source));
    h["short"] = json!(algo.as_str());
    h["tooltip"] = json!(format!("Every handle this editor makes and reads is {} ({source}). Engines must use the same.", algo.as_str()));
    out.push(h);
    // the run
    let run = &s.run;
    if run.running {
        let (done, total) = run.progress();
        let secs = run.elapsed(now).map_or(0.0, |d| d.as_secs_f64());
        let mut r = seg("run", "run", "left", 1, format!("running: {done} of {total} nodes"));
        r["short"] = json!(format!("{done}/{total}"));
        r["busy"] = json!(true);
        r["detail"] = json!(format!("{secs:.1} s"));
        r["command"] = json!({"id": "run.cancel", "params": {}, "label": "Cancel"});
        r["tooltip"] = json!("Stop this run (Esc): the engine stops between nodes or inside a long one, and the rest of its results are ignored");
        out.push(r);
    } else if let Some(wall) = run.last_wall {
        let took = crate::run_text::format_ms(wall.as_secs_f64() * 1e3);
        let mut r = seg("run", "run", "left", 1, format!("last run {took}"));
        r["short"] = json!(took);
        let engine = run.last_reply.as_ref().map(|r| r.elapsed_ms).filter(|ms| *ms > 0.0);
        let mut detail: Vec<String> = Vec::new();
        if let Some(ms) = engine {
            detail.push(format!("engine {}", crate::run_text::format_ms(ms)));
        }
        if let Some(c) = &run.cache.last {
            let (hits, total) = (c.hits(), c.hits() + c.computed);
            detail.push(if c.saved_ms > 0.0 {
                format!("cached {hits} of {total} · saved {}", crate::run_text::format_ms(c.saved_ms))
            } else {
                format!("cached {hits} of {total}")
            });
            r["command"] = json!({"id": "run.cacheReport", "params": {"open": true}, "label": "Cache Report"});
            r["tooltip"] = json!(format!(
                "{} from memory, {} from the store, {} by content; {} computed\nThis session: {} runs\nClick for the Cache Report",
                c.from_memory, c.from_store, c.from_content, c.computed, run.cache.runs
            ));
        }
        if !detail.is_empty() {
            r["detail"] = json!(detail.join(" · "));
        }
        out.push(r);
        if let Some(c) = &run.cache.last {
            let declined = c.declined();
            let warn = if declined > 0 {
                Some(format!("store refused {declined} {}", if declined == 1 { "write" } else { "writes" }))
            } else {
                c.tree.as_ref().filter(|t| t.low_disk()).map(|t| format!("disk low: {} free", bytes(t.free_bytes.unwrap_or(0))))
            };
            if let Some(w) = warn {
                let mut st = seg("store", "warning", "left", 2, w);
                st["tone"] = json!("warn");
                st["command"] = json!({"id": "run.cacheReport", "params": {"open": true}, "label": "Cache Report"});
                st["tooltip"] = json!("Below the disk's floor the store refuses writes, and values stay in memory only. Click for the Cache Report");
                out.push(st);
            }
        }
    } else {
        let mut r = seg("run", "run", "left", 1, "not run yet");
        r["short"] = json!("not run");
        out.push(r);
    }
    let mut a = seg("auto", "toggle", "left", 3, "Auto-run");
    a["short"] = json!("Auto");
    a["on"] = json!(run.auto);
    a["command"] = json!({"id": "run.auto", "params": {"on": !run.auto}});
    a["tooltip"] = json!("Run 150 ms after every edit");
    out.push(a);
    if let Some(ask) = &s.save.pending {
        let mut sv = seg("save", "progress", "left", 2, format!("saving {}…", crate::control::short_path(&ask.path)));
        sv["busy"] = json!(true);
        sv["tooltip"] = json!(ask.path);
        out.push(sv);
    }
    if let Some((params, label)) = p.convert_offer {
        let mut c = seg("convert", "action", "left", 2, format!("Insert {label}"));
        c["command"] = json!({"id": "wire.convert", "params": params, "label": format!("Insert {label}")});
        c["tooltip"] = json!(format!("wire.convert: place {label} between them, wired, as one undo step"));
        out.push(c);
    }
    // the messages, most serious first
    let mut msgs = crate::control::status_messages(s, p.last_error, p.app);
    msgs.sort_by_key(|m| match m.tone {
        Tone::Error => 0,
        Tone::Warn => 1,
        Tone::Info => 2,
    });
    for (k, m) in msgs.iter().enumerate().take(16) {
        let mut v = seg(&format!("message.{k}"), "message", "right", if k == 0 { 1 } else { 2 }, m.text.clone());
        v["tone"] = json!(tone_word(m.tone));
        v["command"] = json!({"id": "view.messages", "params": {"open": true}, "label": "Messages"});
        v["tooltip"] = json!(match &m.detail {
            Some(d) => format!("{}\n\n{d}\n\nClick to see every message in full", m.text),
            None => format!("{}\n\nClick to see every message in full", m.text),
        });
        out.push(v);
    }
    json!({"on": !s.views.status_line_off, "segments": out})
}

fn bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{b} B") } else { format!("{v:.1} {}", UNITS.get(unit).copied().unwrap_or("TB")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_session::Session;

    fn parts<'a>(s: &'a Session, state: &'a EngineState, last_error: Option<&'a str>, selected: usize) -> Parts<'a> {
        Parts { session: s, link: None, state, last_error, installing: None, convert_offer: None, selected, document: "Untitled", app: None }
    }

    fn ids(v: &Value) -> Vec<String> {
        v["segments"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_owned()).collect()
    }

    fn find<'a>(v: &'a Value, id: &str) -> &'a Value {
        v["segments"].as_array().unwrap().iter().find(|s| s["id"] == id).unwrap()
    }

    /// The egui bar's cells, in its order, as segments every shell draws.
    #[test]
    fn the_line_has_the_egui_bars_cells_in_order() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        s.execute("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap();
        let state = EngineState::Off;
        let v = model(&parts(&s, &state, None, 0), Instant::now());
        assert_eq!(v["on"], true, "on by default");
        assert_eq!(ids(&v), ["engine", "document", "hash", "run", "auto"]);
        assert_eq!(find(&v, "engine")["dot"], "down");
        assert_eq!(find(&v, "document")["text"], "Untitled •", "edited");
        assert_eq!(find(&v, "document")["detail"], "1 nodes, 0 wires");
        assert_eq!(find(&v, "run")["text"], "not run yet");
        assert_eq!(find(&v, "run")["short"], "not run");
        assert_eq!(find(&v, "auto")["short"], "Auto");
        assert_eq!(find(&v, "auto")["command"]["id"], "run.auto");
        assert!(find(&v, "hash")["text"].as_str().unwrap().starts_with("hash "));
    }

    /// The last error is the newest message, after the banner: it stays until a command clears it,
    /// opens the Messages window, and a narrow window keeps it (priority 1).
    #[test]
    fn the_last_message_stays_and_a_selection_shows() {
        let s = Session::new(plumcraft_text::engine().schema());
        let state = EngineState::Off;
        let v = model(&parts(&s, &state, Some("select nodes to copy first"), 2), Instant::now());
        let m = find(&v, "message.0");
        assert_eq!(
            (m["text"].as_str(), m["tone"].as_str(), m["priority"].as_u64(), m["side"].as_str()),
            (Some("select nodes to copy first"), Some("error"), Some(1), Some("right"))
        );
        assert_eq!(m["command"]["id"], "view.messages");
        assert_eq!(find(&v, "selection")["text"], "2 selected");
    }

    /// Hidden: the model says so, and a shell draws nothing; its segments are still there for a
    /// script.
    #[test]
    fn the_option_off_says_so() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        s.execute("view.statusLine", &json!({"on": false})).unwrap();
        let state = EngineState::Off;
        let v = model(&parts(&s, &state, None, 0), Instant::now());
        assert_eq!(v["on"], false);
        s.execute("view.statusLine", &json!({})).unwrap();
        assert_eq!(model(&parts(&s, &state, None, 0), Instant::now())["on"], true, "a toggle");
    }

    #[test]
    fn the_hash_cells_words() {
        assert_eq!(hash_label("b3", "policy file /x/y"), "hash b3 · policy file");
        assert_eq!(hash_label("md5", "environment COMMONKIT_HASH"), "hash md5 · environment");
        assert_eq!(hash_label("b3", "default"), "hash b3 · default");
    }
}
