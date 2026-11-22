//! The shell conformance suite (N2 slice 4; `docs/shell-interface.md` section 16): scripted
//! cases, written once as data (`fixtures/conformance/cases.json`), run against any shell
//! through an abstract [`Driver`]: the C ABI headless (`crates/ffi/tests/conformance.rs`, through
//! the real exported functions), the egui editor (`crates/ui/tests/conformance.rs`, through its
//! control dispatcher), and any shell that serves the control port (a Swift or Kotlin shell's
//! core opens one; a driver speaking the control protocol's JSON lines runs the same cases).
//!
//! **A case** is `{name, about, half, needs?, steps: [step]}`. The runner starts each case on a
//! new graph (`graph.new {discard: true}`) and runs its steps in order; the first that fails
//! fails the case, with the step's number and what was seen.
//!
//! **Halves** (N3 slice 1; `docs/shell-interface.md` 16.1): `half` is `"read"` or `"edit"`. A
//! read-only case changes nothing of the graph through the shell under test: it may build the
//! graph it reads with `command.run` (its setup), and after that only selects, taps, hits,
//! locates, opens menus, the palette and windows, fits, runs and watches. Anything that moves,
//! wires, routes, adds from the palette, undoes or redoes is an editing case. [`run_half`] runs
//! one half; N3's gate is the read half on the Apple shells.
//!
//! **Needs:** `needs: [capability]` runs the case only on a driver that has every one
//! ([`Driver::has`]): `engine:text` and `engine:array` (an engine of that pack is connected, so
//! runs and watches work), `media` (it serves `media.get`), `reports` (the suite gives it its
//! measurements). A case a driver can't run is skipped, and the outcome says so.
//!
//! A step is one of:
//!
//! - `{call: method, params?, ok?: bool (default true), expect?: {pointer: want}, save?: {var:
//!   pointer}, until?: {pointer: want}, timeout?: seconds}`: one control request; `pointer` is a
//!   JSON pointer into the reply `{ok, result | error}`. With `until` the request is sent again
//!   (the driver settling between, [`Driver::pause`]) until the reply matches, or `timeout`
//!   (default 30) seconds pass: a condition waited on, never a fixed time; with `holds:
//!   {pointer: want}` (and `for?: seconds`, default 1, at most 10) the request is sent again,
//!   the driver settling between, for that long and every reply must match: a condition that
//!   must stay so (B43: a window the shell closed by itself a moment after it opened);
//! - `{gesture: params, …}`: `ui.gesture` with these params, as `call`;
//! - `{geometry: faces}`: report these faces' measurements (`geometry.set`'s `faces`); a driver
//!   whose shell measures its own faces ignores it ([`Driver::reports`]);
//! - `{hot: {at, slop?}, expect}`: the typed drag move (`plum_gesture_move`) where the driver has
//!   one, else skipped;
//! - `{check: name, …}`: a check the runner knows ([`check`]);
//! - `only: capability` (or a list) on any step runs it only on a driver that has it.
//!
//! **Values.** A string `"$v"` is the saved value `v` (any JSON); `$v` inside a longer string is
//! its text. An object `{"$locate": {node | wire | frame, part?, pin?}, "$plus"?: [dx, dy]}` is
//! the point `geometry.locate` gives, moved by `$plus`: so a case aims at what the shell
//! measured, whatever its sizes. A want is a value (numbers equal within 1e-3), or one of
//! `{"$len": n}`, `{"$exists": bool}`, `{"$contains": v}`, `{"$deep": {k: v}}` (some object in
//! it has these fields), `{"$not": v}`, `{"$one": [v, …]}`, `{"$find": {where: {k: v}, want:
//! {pointer: want}}}` (an item of a list with these fields, and what it must hold), `{"$min": n}`
//! (a number at least `n`).
//!
//! **Waiting.** After every call that changes something the runner calls [`Driver::settle`]:
//! a shell that measures what it draws waits there, on a condition (every face measured), never
//! on a fixed time.

use serde_json::{Map, Value, json};

/// The cases, as checked in.
pub const CASES: &str = include_str!("../fixtures/conformance/cases.json");

/// The suite's engines folder (B71): a driver with the `manifests` capability points its shell's
/// engines folder here (`Launcher::folder`, a C ABI config's `dirs.engines`, or
/// `$PLUMCRAFT_ENGINE_FOLDER` for an app), so the case about manifests finds its engine, `probe`.
/// Only read, never written.
pub fn suite_engines_folder() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join("conformance").join("engines")
}

/// A shell the suite drives.
pub trait Driver {
    /// For messages.
    fn name(&self) -> String;
    /// One control request (`method`, `params`); the reply `{ok, result | error}`.
    fn call(&mut self, method: &str, params: Value) -> Value;
    /// Does this shell take its measurements from the suite (a headless shell), rather than
    /// measure the faces it draws (the egui editor)?
    fn reports(&self) -> bool;
    /// Report `faces` (`geometry.set`'s `faces`, ids resolved) as this shell reports what it
    /// measured, and wait until the core has them. Called only when [`Driver::reports`].
    fn report_geometry(&mut self, faces: &Value) -> Result<(), String>;
    /// Wait until the shell has drawn and measured what the last request changed.
    fn settle(&mut self) {}
    /// The typed drag move at `at` within `slop`, as `ui.gesture`'s `update` (`plum_gesture_move`
    /// on the C ABI); none where the driver has no typed form.
    fn hot_move(&mut self, _at: [f32; 2], _slop: f32) -> Option<Value> {
        None
    }
    /// Does this driver have `capability` (`engine:<pack>`, `media`; `reports` is
    /// [`Driver::reports`])? None by default.
    fn has(&self, _capability: &str) -> bool {
        false
    }
    /// Let the shell get on (a run, a read) before a request is sent again (`until`): wait for
    /// the core's wake, draw a frame. The default is the driver's [`Driver::settle`].
    fn pause(&mut self) {
        self.settle();
    }
}

/// A half of the suite (section 16.1): what changes nothing of the graph, and what edits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    Read,
    Edit,
}

impl Half {
    pub fn word(self) -> &'static str {
        match self {
            Half::Read => "read",
            Half::Edit => "edit",
        }
    }
}

/// `reports` is [`Driver::reports`]; `rows` (N4 slice 2: rows' widgets and the header's parts
/// are measured, so a tap or a drag can aim at them) a driver the suite gives its measurements
/// to, or one whose shell reports them (the Apple shells; not the egui editor, whose own widgets
/// take the clicks on its rows).
fn has(d: &dyn Driver, capability: &str) -> bool {
    match capability {
        "reports" => d.reports(),
        "rows" => d.reports() || d.has("rows"),
        _ => d.has(capability),
    }
}

/// How one case went.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub name: String,
    /// Steps run.
    pub steps: usize,
    /// None: passed (or skipped).
    pub error: Option<String>,
    /// Not run, and why: the driver lacks what the case needs.
    pub skipped: Option<String>,
}

/// The cases (`fixtures/conformance/cases.json`).
pub fn cases() -> Result<Vec<Value>, String> {
    let v: Value = serde_json::from_str(CASES).map_err(|e| format!("cases.json: {e}"))?;
    v.get("cases").and_then(Value::as_array).cloned().ok_or_else(|| "cases.json: no \"cases\" list".into())
}

/// Run every case on `d`.
pub fn run_all(d: &mut dyn Driver) -> Result<Vec<Outcome>, String> {
    Ok(cases()?.iter().map(|c| run_case(d, c)).collect())
}

/// Run the cases of one half on `d` (N3's gate is the read half).
pub fn run_half(d: &mut dyn Driver, half: Half) -> Result<Vec<Outcome>, String> {
    Ok(cases()?.iter().filter(|c| c.get("half").and_then(Value::as_str) == Some(half.word())).map(|c| run_case(d, c)).collect())
}

/// A short report of `outcomes`, for a failing test's message.
pub fn summary(driver: &str, outcomes: &[Outcome]) -> String {
    let failed: Vec<String> = outcomes.iter().filter_map(|o| o.error.as_ref().map(|e| format!("  {}: {e}", o.name))).collect();
    let skipped: Vec<String> = outcomes.iter().filter_map(|o| o.skipped.as_ref().map(|why| format!("  skipped {}: {why}", o.name))).collect();
    let run = outcomes.len() - skipped.len();
    format!(
        "{driver}: {} of {run} cases passed, {} skipped\n{}{}",
        run - failed.len(),
        skipped.len(),
        failed.join("\n"),
        if skipped.is_empty() { String::new() } else { format!("\n{}", skipped.join("\n")) }
    )
}

/// Run one case on a new graph (or skip it, when `d` lacks what it needs).
pub fn run_case(d: &mut dyn Driver, case: &Value) -> Outcome {
    let name = case.get("name").and_then(Value::as_str).unwrap_or("(unnamed)").to_owned();
    let lacks: Vec<&str> = case.get("needs").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|c| !has(d, c)).collect();
    if !lacks.is_empty() {
        return Outcome { name, steps: 0, error: None, skipped: Some(format!("{} lacks {}", d.name(), lacks.join(", "))) };
    }
    let mut vars = Map::new();
    let fresh = d.call("command.run", json!({"command": "graph.new", "params": {"discard": true}}));
    if fresh["ok"] != true {
        return Outcome { name, steps: 0, error: Some(format!("graph.new: {fresh}")), skipped: None };
    }
    d.settle();
    let steps = case.get("steps").and_then(Value::as_array).cloned().unwrap_or_default();
    for (k, s) in steps.iter().enumerate() {
        if let Err(e) = step(d, &mut vars, s) {
            return Outcome { name, steps: k, error: Some(format!("step {}: {e}", k + 1)), skipped: None };
        }
    }
    // a drag left under way would carry into the next case, and so would an open palette
    let _ = d.call("ui.gesture", json!({"kind": "drag", "phase": "cancel"}));
    let _ = d.call("command.run", json!({"command": "canvas.palette", "params": {"open": false}}));
    Outcome { name, steps: steps.len(), error: None, skipped: None }
}

fn step(d: &mut dyn Driver, vars: &mut Map<String, Value>, s: &Value) -> Result<(), String> {
    let only: Vec<&str> = match s.get("only") {
        Some(Value::String(c)) => vec![c.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    if only.iter().any(|c| !has(d, c)) {
        return Ok(());
    }
    if let Some(faces) = s.get("geometry") {
        if d.reports() {
            let faces = subst(faces, vars);
            d.report_geometry(&faces)?;
        }
        return Ok(());
    }
    if let Some(h) = s.get("hot") {
        let h = resolve(d, &subst(h, vars))?;
        let at = h.get("at").and_then(Value::as_array).filter(|a| a.len() == 2).ok_or("hot.at must be [x, y]")?;
        let n = |v: &Value| v.as_f64().map(|f| f as f32).ok_or("hot.at must be numbers");
        let slop = h.get("slop").and_then(Value::as_f64).map_or(crate::gesture::DEFAULT_SLOP, |f| f as f32);
        if let Some(v) = d.hot_move([n(&at[0])?, n(&at[1])?], slop) {
            expect(&json!({"ok": true, "result": v}), s.get("expect"), vars)?;
        }
        return Ok(());
    }
    if let Some(name) = s.get("check").and_then(Value::as_str) {
        return check(name, &subst(s, vars));
    }
    let (method, params) = match (s.get("call").and_then(Value::as_str), s.get("gesture")) {
        (Some(m), None) => (m.to_owned(), s.get("params").cloned().unwrap_or(json!({}))),
        (None, Some(g)) => ("ui.gesture".to_owned(), g.clone()),
        _ => return Err(format!("a step is a call, a gesture, geometry, hot or a check: {s}")),
    };
    let params = resolve(d, &subst(&params, vars))?;
    let mut reply = d.call(&method, params.clone());
    if let Some(until) = s.get("until") {
        let limit = std::time::Duration::from_secs_f64(s.get("timeout").and_then(Value::as_f64).unwrap_or(30.0).clamp(0.0, 600.0));
        let t0 = std::time::Instant::now();
        while let Err(e) = expect(&reply, Some(until), vars) {
            if t0.elapsed() > limit {
                return Err(format!("{method} {params}: not so in {limit:?}: {e}"));
            }
            d.pause();
            reply = d.call(&method, params.clone());
        }
    }
    if let Some(holds) = s.get("holds") {
        let span = std::time::Duration::from_secs_f64(s.get("for").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.0, 10.0));
        let t0 = std::time::Instant::now();
        loop {
            expect(&reply, Some(holds), vars).map_err(|e| format!("{method} {params}: didn't stay so ({:.2} s in): {e}", t0.elapsed().as_secs_f64()))?;
            if t0.elapsed() >= span {
                break;
            }
            d.pause();
            reply = d.call(&method, params.clone());
        }
    }
    let want_ok = s.get("ok").and_then(Value::as_bool).unwrap_or(true);
    if (reply["ok"] == true) != want_ok {
        return Err(format!("{method} {params}: wanted ok {want_ok}, got {}", short(&reply)));
    }
    if matches!(method.as_str(), "command.run" | "ui.gesture" | "ui.invoke" | "geometry.set") {
        d.settle();
    }
    expect(&reply, s.get("expect"), vars).map_err(|e| format!("{method} {params}: {e}"))?;
    if let Some(save) = s.get("save").and_then(Value::as_object) {
        for (var, ptr) in save {
            let ptr = ptr.as_str().ok_or("save's values are JSON pointers")?;
            let v = reply.pointer(&subst_str(ptr, vars)).cloned().ok_or_else(|| format!("{method}: nothing at {ptr} to save as {var}: {}", short(&reply)))?;
            vars.insert(var.clone(), v);
        }
    }
    Ok(())
}

fn short(v: &Value) -> String {
    let t = v.to_string();
    if t.chars().count() > 600 { format!("{}…", t.chars().take(600).collect::<String>()) } else { t }
}

/// `$var` in a string: the whole string a var (its value, any JSON), or each `$var` in it
/// replaced by its text.
fn subst_str(s: &str, vars: &Map<String, Value>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(k) = rest.find('$') {
        out.push_str(&rest[..k]);
        let after = &rest[k + 1..];
        let len = after.char_indices().find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_')).map_or(after.len(), |(i, _)| i);
        let name = &after[..len];
        match vars.get(name) {
            Some(Value::String(v)) if !name.is_empty() => out.push_str(v),
            Some(v) if !name.is_empty() => out.push_str(&v.to_string()),
            _ => {
                out.push('$');
                out.push_str(name);
            }
        }
        rest = &after[len..];
    }
    out.push_str(rest);
    out
}

fn subst(v: &Value, vars: &Map<String, Value>) -> Value {
    match v {
        Value::String(s) => {
            if let Some(name) = s.strip_prefix('$')
                && let Some(val) = vars.get(name)
            {
                return val.clone();
            }
            Value::String(subst_str(s, vars))
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| subst(x, vars)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (subst_str(k, vars), subst(x, vars))).collect()),
        other => other.clone(),
    }
}

/// `{"$locate": …, "$plus"?: [dx, dy]}` anywhere in `v`: the point `geometry.locate` gives.
fn resolve(d: &mut dyn Driver, v: &Value) -> Result<Value, String> {
    match v {
        Value::Object(o) if o.contains_key("$locate") => {
            let r = d.call("geometry.locate", o["$locate"].clone());
            if r["ok"] != true {
                return Err(format!("geometry.locate {}: {}", o["$locate"], short(&r)));
            }
            let at = &r["result"]["at"];
            let (Some(x), Some(y)) = (at[0].as_f64(), at[1].as_f64()) else { return Err(format!("geometry.locate gave no point: {r}")) };
            let (dx, dy) = match o.get("$plus") {
                Some(p) => (p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0)),
                None => (0.0, 0.0),
            };
            Ok(json!([x + dx, y + dy]))
        }
        Value::Object(o) => {
            let mut m = Map::new();
            for (k, x) in o {
                m.insert(k.clone(), resolve(d, x)?);
            }
            Ok(Value::Object(m))
        }
        Value::Array(a) => a.iter().map(|x| resolve(d, x)).collect::<Result<Vec<_>, _>>().map(Value::Array),
        other => Ok(other.clone()),
    }
}

fn expect(reply: &Value, want: Option<&Value>, vars: &Map<String, Value>) -> Result<(), String> {
    let Some(want) = want.and_then(Value::as_object) else { return Ok(()) };
    for (ptr, w) in want {
        let ptr = subst_str(ptr, vars);
        let w = subst(w, vars);
        let got = reply.pointer(&ptr);
        matches(got, &w).map_err(|e| format!("at {ptr}: {e}; the reply: {}", short(reply)))?;
    }
    Ok(())
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1.0e-3,
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w))),
        _ => a == b,
    }
}

fn deep_has(v: &Value, fields: &Map<String, Value>) -> bool {
    match v {
        Value::Object(o) => fields.iter().all(|(k, w)| o.get(k).is_some_and(|x| same(x, w))) || o.values().any(|x| deep_has(x, fields)),
        Value::Array(a) => a.iter().any(|x| deep_has(x, fields)),
        _ => false,
    }
}

fn matches(got: Option<&Value>, want: &Value) -> Result<(), String> {
    if let Value::Object(o) = want
        && o.len() == 1
        && let Some((k, arg)) = o.iter().next()
        && k.starts_with('$')
    {
        let ok = match k.as_str() {
            "$exists" => got.is_some() == arg.as_bool().unwrap_or(true),
            "$len" => got.and_then(|g| g.as_array().map(Vec::len).or_else(|| g.as_object().map(Map::len))).map(|n| n as u64) == arg.as_u64(),
            "$contains" => got.and_then(Value::as_array).is_some_and(|a| a.iter().any(|x| same(x, arg))),
            "$deep" => arg.as_object().is_some_and(|f| got.is_some_and(|g| deep_has(g, f))),
            "$not" => got.is_none_or(|g| !same(g, arg)),
            "$one" => arg.as_array().is_some_and(|a| got.is_some_and(|g| a.iter().any(|x| same(g, x)))),
            "$min" => arg.as_f64().is_some_and(|n| got.and_then(Value::as_f64).is_some_and(|g| g >= n)),
            "$find" => {
                let fields = arg.get("where").and_then(Value::as_object).ok_or("$find needs where: {field: value}")?;
                let found = got.and_then(Value::as_array).and_then(|a| a.iter().find(|x| fields.iter().all(|(k, w)| x.get(k).is_some_and(|v| same(v, w)))));
                let Some(item) = found else {
                    return Err(format!("no item with {} in {}", Value::Object(fields.clone()), got.map_or_else(|| "nothing".into(), short)));
                };
                for (ptr, w) in arg.get("want").and_then(Value::as_object).into_iter().flatten() {
                    matches(item.pointer(ptr), w).map_err(|e| format!("the item with {}: at {ptr}: {e}", Value::Object(fields.clone())))?;
                }
                true
            }
            other => return Err(format!("unknown matcher {other}")),
        };
        return if ok { Ok(()) } else { Err(format!("wanted {k} {arg}, got {}", got.map_or_else(|| "nothing".into(), short))) };
    }
    match got {
        Some(g) if same(g, want) => Ok(()),
        Some(g) => Err(format!("wanted {want}, got {}", short(g))),
        None => Err(format!("wanted {want}, got nothing")),
    }
}

/// The runner's own checks:
/// - `tokensResolved {scene, theme}`: every colour in the scene is a token alone or a colour
///   alone, and `theme.get` has every token it names;
/// - `nidsUnique {scene}`: no two items of a full scene share a nid, whatever their kinds.
pub fn check(name: &str, s: &Value) -> Result<(), String> {
    match name {
        "tokensResolved" => {
            let tokens = s["theme"]["tokens"].as_object().ok_or("theme.tokens is not a map")?;
            let mut named = std::collections::BTreeSet::new();
            let mut bad = Vec::new();
            paints(&s["scene"], &mut named, &mut bad);
            if !bad.is_empty() {
                return Err(format!("paints with both a token and a colour, or neither: {:?}", &bad[..bad.len().min(5)]));
            }
            if named.is_empty() {
                return Err("the scene names no token".into());
            }
            let missing: Vec<&String> = named.iter().filter(|t| !tokens.contains_key(t.as_str())).collect();
            if missing.is_empty() { Ok(()) } else { Err(format!("theme.get lacks {missing:?}")) }
        }
        "nidsUnique" => {
            let mut seen = std::collections::BTreeMap::new();
            for kind in ["nodes", "wires", "nets", "frames", "groups", "marks"] {
                for item in s["scene"][kind].as_array().into_iter().flatten() {
                    let nid = item["nid"].as_u64().ok_or_else(|| format!("{kind} item without a nid: {}", short(item)))?;
                    if let Some(other) = seen.insert(nid, (kind, item["id"].clone())) {
                        return Err(format!("nid {nid} is both {other:?} and {kind} {}", item["id"]));
                    }
                }
            }
            if seen.len() < 2 { Err("too few items to check".into()) } else { Ok(()) }
        }
        other => Err(format!("unknown check {other}")),
    }
}

/// Every paint (`{token?, color?}` where a colour is) in `v`.
fn paints(v: &Value, named: &mut std::collections::BTreeSet<String>, bad: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            let token = o.get("token").and_then(Value::as_str);
            let color = o.get("color").filter(|c| c.as_array().is_some_and(|a| a.len() == 4));
            let paintish = (token.is_some() || color.is_some()) && o.keys().all(|k| k == "token" || k == "color");
            if paintish {
                match (token, color) {
                    (Some(t), None) => {
                        named.insert(t.to_owned());
                    }
                    (None, Some(_)) => {}
                    _ => bad.push(v.to_string()),
                }
            } else {
                o.values().for_each(|x| paints(x, named, bad));
            }
        }
        Value::Array(a) => a.iter().for_each(|x| paints(x, named, bad)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cases_parse_and_every_step_is_known() {
        let cases = cases().unwrap();
        assert!(cases.len() >= 15, "{}", cases.len());
        let mut names = std::collections::BTreeSet::new();
        for c in &cases {
            let name = c["name"].as_str().unwrap();
            assert!(names.insert(name), "two cases called {name}");
            assert!(c["about"].as_str().is_some_and(|a| !a.is_empty()), "{name} says what it checks");
            assert!(matches!(c["half"].as_str(), Some("read" | "edit")), "{name} names its half");
            for s in c["steps"].as_array().unwrap() {
                let kinds = ["call", "gesture", "geometry", "hot", "check"].iter().filter(|k| s.get(**k).is_some()).count();
                assert_eq!(kinds, 1, "{name}: {s}");
            }
        }
    }

    #[test]
    fn values_and_wants() {
        let mut vars = Map::new();
        vars.insert("a".into(), json!("n1"));
        vars.insert("g".into(), json!({"x": 1}));
        assert_eq!(subst(&json!(["$a", "$a.text", "$g", "$$", "$nope", {"$a": "$a"}]), &vars), json!(["n1", "n1.text", {"x": 1}, "$$", "$nope", {"n1": "n1"}]));
        assert!(matches(Some(&json!([1, 2])), &json!({"$len": 2})).is_ok());
        assert!(matches(Some(&json!(1.0004)), &json!(1)).is_ok());
        assert!(matches(Some(&json!(1.01)), &json!(1)).is_err());
        assert!(matches(None, &json!({"$exists": false})).is_ok());
        assert!(matches(Some(&json!({"a": [{"command": "x", "y": 1}]})), &json!({"$deep": {"command": "x"}})).is_ok());
        assert!(matches(Some(&json!("b")), &json!({"$one": ["a", "b"]})).is_ok());
        assert!(matches(Some(&json!(1)), &json!({"$bogus": 1})).is_err());
    }

    /// The core's own document as a shell: the dispatcher every shell's control port uses, with
    /// no engine and no canvas; it reports the suite's geometry as a headless shell would.
    struct DocDriver(crate::document::Document);

    impl Driver for DocDriver {
        fn name(&self) -> String {
            "the core's document".into()
        }
        fn call(&mut self, method: &str, params: Value) -> Value {
            crate::control::now(&mut self.0, method, &params)
        }
        fn reports(&self) -> bool {
            true
        }
        /// Waiting on a condition (`until`): the document's turn, as a shell's frame takes it
        /// (an autosave's quiet time ends there), then a moment.
        fn pause(&mut self) {
            crate::document::turn(&mut self.0);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        fn report_geometry(&mut self, faces: &Value) -> Result<(), String> {
            let r = self.call("command.run", json!({"command": "geometry.set", "params": {"faces": faces}}));
            if r["ok"] == true { Ok(()) } else { Err(format!("geometry.set: {r}")) }
        }
        /// Its engines folder is the suite's ([`suite_engines`]).
        fn has(&self, capability: &str) -> bool {
            capability == "manifests"
        }
    }

    /// The suite's engines folder (`fixtures/conformance/engines`), as a launcher's: read on this
    /// thread, offered by `d` with installing as a desktop has it.
    fn suite_engines(d: &mut crate::document::Document) {
        let launcher = crate::engines::Launcher { folder: Some(super::suite_engines_folder()), ..Default::default() };
        assert_eq!(launcher.load_manifests(), Vec::<String>::new());
        d.session.engines.choices = launcher.choices();
        d.offer_installs(&launcher);
    }

    /// The runner is not vacuous: a wrong want, a wrong ok, a missing value each fail their case.
    #[test]
    fn a_case_that_is_wrong_fails() {
        let session = plumcraft_session::Session::new(plumcraft_text::engine().schema());
        let mut d = DocDriver(crate::document::Document::new(session));
        let add =
            json!({"call": "command.run", "params": {"command": "node.add", "params": {"kind": "text.literal", "pos": [0, 0]}}, "save": {"a": "/result/id"}});
        for (bad, says) in [
            (json!({"call": "scene.get", "expect": {"/result/selection/nodes": ["$a"]}}), "wanted"),
            (json!({"gesture": {"kind": "tap", "at": [0, 0]}, "ok": false}), "wanted ok false"),
            (json!({"gesture": {"kind": "bogus", "at": [0, 0]}}), "wanted ok true"),
            (json!({"call": "graph.get", "expect": {"/result/nodes/$a/nothing": 1}}), "got nothing"),
            (json!({"gesture": {"kind": "tap", "at": {"$locate": {"node": "n404"}}}}), "geometry.locate"),
            (json!({"call": "scene.get", "save": {"x": "/nope"}}), "nothing at"),
            (json!({"call": "scene.get", "holds": {"/result/view/windows/params": true}, "for": 0.1}), "didn't stay so"),
        ] {
            let case = json!({"name": "wrong", "about": "-", "steps": [add.clone(), bad.clone()]});
            let o = run_case(&mut d, &case);
            let e = o.error.unwrap_or_else(|| panic!("{bad} passed"));
            assert!(e.starts_with("step 2:") && e.contains(says), "{bad}: {e}");
        }
    }

    #[test]
    fn the_suite_passes_on_the_cores_own_document() {
        let session = plumcraft_session::Session::new(plumcraft_text::engine().schema());
        let mut d = DocDriver(crate::document::Document::new(session));
        // a look of its own, in memory, as the C ABI's core gives its documents (N4 slice 4)
        d.0.look =
            Some(std::sync::Arc::new(std::sync::Mutex::new(crate::settings::Look::new(crate::prefs::Store::default(), crate::settings::System::default()))));
        suite_engines(&mut d.0);
        let out = run_all(&mut d).unwrap();
        assert!(out.iter().any(|o| o.name.starts_with("an engine a manifest") && o.skipped.is_none()), "the manifest case ran");
        assert!(out.iter().all(|o| o.error.is_none()), "{}", summary(&d.name(), &out));
        let steps: usize = out.iter().map(|o| o.steps).sum();
        assert!(out.len() >= 15 && steps >= 150, "{} cases, {steps} steps", out.len());
        eprintln!("{} ({steps} steps)", summary(&d.name(), &out));
    }
}
