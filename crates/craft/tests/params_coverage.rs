//! Parameter coverage of every built-in pack whose CLI is on PATH (`plumcraft_craft::coverage`):
//! per craft, how many commands no node runs, and per curated node command, the parameters no
//! widget, file socket or fixed value sends (hidden). The hidden sets are checked against
//! `tests/fixtures/params_coverage.json`: a node command whose hidden set grows (the craft added
//! a parameter, or the pack dropped one), that disappears, or whose description stops parsing
//! fails; fewer hidden parameters, a new node or a newly parsed description is printed. Rewrite
//! the baseline with `PLUMCRAFT_BLESS=1 cargo test -p plumcraft-craft --test params_coverage`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use plumcraft_craft::coverage::{Coverage, coverage};
use plumcraft_craft::manifest::{BUILTIN, builtin};
use plumcraft_craft::{Options, build};
use serde_json::Value;

/// craft → "<node> <command>" → hidden keys (`None`: the description can't be read).
type Baseline = BTreeMap<String, BTreeMap<String, Option<Vec<String>>>>;

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/params_coverage.json")
}

fn read_baseline() -> Baseline {
    let Ok(text) = std::fs::read_to_string(baseline_path()) else { return Baseline::new() };
    let v: Value = serde_json::from_str(&text).unwrap();
    serde_json::from_value(v).unwrap()
}

/// Sorted, one node command per line, so a bless diffs well.
fn write_baseline(b: &Baseline) {
    let mut out = String::from("{\n");
    for (ci, (craft, rows)) in b.iter().enumerate() {
        out.push_str(&format!("  {}: {{\n", serde_json::to_string(craft).unwrap()));
        for (ri, (key, hidden)) in rows.iter().enumerate() {
            let comma = if ri + 1 < rows.len() { "," } else { "" };
            out.push_str(&format!("    {}: {}{comma}\n", serde_json::to_string(key).unwrap(), serde_json::to_string(hidden).unwrap()));
        }
        out.push_str(if ci + 1 < b.len() { "  },\n" } else { "  }\n" });
    }
    out.push_str("}\n");
    std::fs::write(baseline_path(), out).unwrap();
}

fn rows(c: &Coverage) -> BTreeMap<String, Option<Vec<String>>> {
    c.nodes.iter().map(|n| (format!("{} {}", n.node, n.command), n.hidden.clone())).collect()
}

/// What changed for one craft: failures, and improvements to print.
fn compare(craft: &str, was: &BTreeMap<String, Option<Vec<String>>>, now: &BTreeMap<String, Option<Vec<String>>>) -> (Vec<String>, Vec<String>) {
    let (mut fail, mut note) = (Vec::new(), Vec::new());
    for (key, old) in was {
        match (old, now.get(key)) {
            (_, None) => fail.push(format!("{craft}: {key}: the node or its command is gone")),
            (Some(_), Some(None)) => fail.push(format!("{craft}: {key}: its description no longer parses")),
            (None, Some(Some(h))) => note.push(format!("{craft}: {key}: now parsed, hidden {h:?}")),
            (None, Some(None)) => {}
            (Some(o), Some(Some(n))) => {
                let grown: Vec<&String> = n.iter().filter(|k| !o.contains(k)).collect();
                let gone: Vec<&String> = o.iter().filter(|k| !n.contains(k)).collect();
                if !grown.is_empty() {
                    fail.push(format!("{craft}: {key}: newly hidden {grown:?}"));
                }
                if !gone.is_empty() {
                    note.push(format!("{craft}: {key}: no longer hidden {gone:?}"));
                }
            }
        }
    }
    for (key, h) in now.iter().filter(|(k, _)| !was.contains_key(*k)) {
        match h {
            Some(h) => note.push(format!("{craft}: {key}: new, hidden {h:?}")),
            None => note.push(format!("{craft}: {key}: new, unparsed")),
        }
    }
    (fail, note)
}

#[test]
fn compare_flags_growth_and_loss_and_prints_improvement() {
    let row = |k: &str, h: Option<&[&str]>| (k.to_owned(), h.map(|h| h.iter().map(|s| (*s).to_owned()).collect()));
    let was = BTreeMap::from([row("a x", Some(&["p", "q"])), row("b y", Some(&[])), row("c z", None), row("d w", Some(&["r"]))]);
    let now = BTreeMap::from([row("a x", Some(&["q", "s"])), row("c z", Some(&["t"])), row("d w", None), row("e v", Some(&[]))]);
    let (fail, note) = compare("k", &was, &now);
    assert_eq!(fail, ["k: a x: newly hidden [\"s\"]", "k: b y: the node or its command is gone", "k: d w: its description no longer parses"], "{fail:?}");
    assert_eq!(note, ["k: a x: no longer hidden [\"p\"]", "k: c z: now parsed, hidden [\"t\"]", "k: e v: new, hidden []"], "{note:?}");
}

#[test]
fn hidden_params_match_the_baseline() {
    let bless = std::env::var_os("PLUMCRAFT_BLESS").is_some_and(|v| !v.is_empty() && v != "0");
    let scratch = plumcraft_scratch::dir("params-coverage");
    let t0 = std::time::Instant::now();
    // One build per craft, all at once.
    let results: Vec<(String, Result<Coverage, String>)> = std::thread::scope(|s| {
        let handles: Vec<_> = BUILTIN
            .iter()
            .map(|id| {
                let root = scratch.join(id);
                s.spawn(move || {
                    let m = builtin(id).unwrap().unwrap();
                    if m.executable().is_none() {
                        return ((*id).to_owned(), Err(format!("{} is not on PATH; {}", m.program().first().cloned().unwrap_or_default(), m.hint)));
                    }
                    let opts = Options { root: Some(root), store: Some(None), include_excluded: true };
                    let r = build(&m, &opts).map(|(_, report, _)| {
                        let c = coverage(&m, &report);
                        for (node, why) in &report.dropped {
                            eprintln!("{id}: {node} fails the catalogue check: {why}");
                        }
                        c
                    });
                    ((*id).to_owned(), r)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let baseline = read_baseline();
    let mut next = baseline.clone();
    let (mut fails, mut notes) = (Vec::new(), Vec::new());
    for (craft, r) in &results {
        let c = match r {
            Ok(c) => c,
            Err(e) if e.contains("not on PATH") => {
                eprintln!("SKIP: {craft}: {e}");
                continue;
            }
            Err(e) => panic!("{craft}: {e}"),
        };
        eprintln!(
            "{craft}: commands {} | run by a node {} | not run {} (with params {}, unparsed {}) | node commands {} | with hidden params {} | hidden params {} | unparsed {}",
            c.commands,
            c.run.len(),
            c.not_run.len(),
            c.not_run_with_params,
            c.not_run_unparsed,
            c.nodes.len(),
            c.with_hidden(),
            c.hidden(),
            c.unparsed()
        );
        for (node, cmd) in &c.missing {
            eprintln!("{craft}: {node}: {cmd} isn't in the catalogue");
        }
        let now = rows(c);
        match baseline.get(craft) {
            Some(was) => {
                let (f, n) = compare(craft, was, &now);
                fails.extend(f);
                notes.extend(n);
            }
            None => notes.push(format!("{craft}: not in the baseline ({} node commands)", now.len())),
        }
        next.insert(craft.clone(), now);
    }
    eprintln!("params coverage: {} packs in {:.1} s", results.len(), t0.elapsed().as_secs_f64());
    for n in &notes {
        eprintln!("improved: {n}");
    }
    if bless {
        write_baseline(&next);
        eprintln!("blessed {}", baseline_path().display());
        return;
    }
    if !notes.is_empty() {
        eprintln!("(PLUMCRAFT_BLESS=1 records these in {})", baseline_path().display());
    }
    assert!(fails.is_empty(), "parameter coverage got worse (PLUMCRAFT_BLESS=1 to accept):\n{}", fails.join("\n"));
}
