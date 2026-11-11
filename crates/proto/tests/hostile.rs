//! Every top-level message parses hostile input without panicking (golden rule 1, spec 18):
//! truncations of real messages, wrong types in every position, deep nesting and huge numbers.

use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::display::{Display, WatchRequest};
use plumcraft_proto::event::{Envelope, LiveUpdate, StatusRecord};
use plumcraft_proto::run::{PeekReply, PlanReply, RunReply, RunRequest, RunSnapshot, SaveReply, SaveRequest};
use plumcraft_proto::session::{EngineState, Hello, HelloReply};
use plumcraft_proto::{Caps, DataRef, Handle, PortValue, ProtoError};

const SEEDS: &[&str] = &[
    r#"{"v": 2, "seq": 1043, "ts": 1.5, "kind": "node.status", "run_id": "r", "generation": 17, "body": {"node_id": "n7", "state": "preview", "thumb": {"kind": "rgba8", "ref": {"handle": "md5:nd1:9e107d9d372bb6826bd81d3542a419d6", "at": [{"kind": "path", "path": "/x"}]}, "w": 4, "h": 3}, "display": [{"media": "ndarray"}]}}"#,
    r#"{"v": 2, "run_id": "0b5e", "generation": 17, "ok": true, "order": ["n1"], "nodes": {"n1": {"state": "done", "ms": 1.0, "outputs": {"value": {"kind": "list", "items": [{"kind": "json", "value": 1}]}}}}, "watch": [{"media": "text", "renditions": [{"kind": "excerpt", "text": "hi"}]}]}"#,
    r#"{"v": 2, "nodes": [{"id": "a", "inputs": [{"name": "x", "type": "FLOAT", "options": {"k": [{"name": "y"}]}}]}], "wire_types": [{"id": "ARRAY", "view_params": {"angle": [{"name": "turn"}]}}]}"#,
    r#"{"v": 2, "nodes": [{"id": "w", "display": "WAV to Audio", "inputs": [{"name": "wav", "type": "WAV"}], "outputs": [{"name": "audio", "type": "AUDIO"}], "converts": {"from": "WAV", "to": "AUDIO", "input": "wav", "output": "audio", "lossless": true}, "engines": ["audio"]}, {"id": "b", "inputs": [{"name": "doc", "type": "D"}], "outputs": [{"name": "w", "type": "WAV"}], "converts": {"from": "D", "to": "WAV"}}]}"#,
    r#"{"v": 2, "op": "hello", "protocol": {"min": 2, "max": 2}, "caps": ["events.push"], "proof": {"path": "/p", "nonce": "n"}}"#,
    r#"{"v": 2, "op": "run", "run_id": "r", "generation": 1, "graph": {"kind": "inline", "graph": {}}, "watch": [{"node": "n", "want": [{"kind": "rgba8"}]}], "inputs": {"n.x": {"kind": "json", "value": 1}}}"#,
    r#"{"stream": "append", "node_id": "n", "port": "p", "offset": 5, "ref": {"handle": "md5:raw:d41d8cd98f00b204e9800998ecf8427e"}}"#,
    r#"{"command": "t.cmd", "nodes": {"a": {"kind": "t.a", "pos": [1, 2], "values": {"x": 1}, "title": "t"}}, "wires": [{"from": {"node": "a", "port": "p"}, "to": {"node": "a", "port": "q"}, "slice": ":-1"}], "inputs": {"i": [["a", "q"]]}, "settings": {"s": [["a", "x"]]}, "outputs": {"o": ["a", "p"]}, "group": {"promoted": [{"port": {"node": "a", "port": "x"}, "label": "l", "min": 0}], "checkpoints": ["a"], "outputs": [{"from": {"node": "a", "port": "p"}, "name": "n"}]}}"#,
    r#"{"labels": {"x": "Ex", "a": {"y": "Why", "x": 3}}, "v": 2, "nodes": [{"id": "a", "inputs": [{"name": "x", "type": "FLOAT", "options": {"k": [{"name": "y"}]}}]}]}"#,
    r#"{"format": "plumcraft.expansion", "version": 1, "display": "My group", "description": "d", "expansion": {"command": "t.cmd", "nodes": {"a": {"kind": "t.a", "pos": [1, 2], "values": {"x": 1}}, "b": {"kind": "t.b"}}, "wires": [{"from": {"node": "a", "port": "p"}, "to": {"node": "b", "port": "q"}}], "inputs": {"i": [["a", "q"]]}, "settings": {"s": [["a", "x"]]}, "outputs": {"o": ["b", "p"]}, "group": {"promoted": [{"port": {"node": "a", "port": "x"}, "label": "s"}], "checkpoints": ["a"]}}}"#,
];

const JUNK: &[&str] = &[
    "",
    "null",
    "true",
    "0",
    "-1",
    "1e400",
    "\"\"",
    "[]",
    "{}",
    "[null]",
    "{\"v\": -2}",
    "{\"v\": 18446744073709551616}",
    "{\"kind\": null}",
    "{\"kind\": 7}",
    "{\"body\": [], \"kind\": \"node.status\"}",
    "{\"nodes\": \"many\"}",
    "{\"state\": {\"state\": {}}}",
    "\u{feff}{}",
];

fn deep(n: usize) -> String {
    format!("{}{}", "[".repeat(n), "]".repeat(n))
}

fn all_parsers(text: &str) {
    let _ = serde_json::from_str::<Envelope>(text);
    let _ = serde_json::from_str::<StatusRecord>(text);
    let _ = serde_json::from_str::<LiveUpdate>(text);
    let _ = serde_json::from_str::<RunReply>(text);
    let _ = serde_json::from_str::<RunRequest>(text);
    let _ = serde_json::from_str::<RunSnapshot>(text);
    let _ = serde_json::from_str::<PeekReply>(text);
    let _ = serde_json::from_str::<PlanReply>(text);
    let _ = serde_json::from_str::<SaveRequest>(text);
    let _ = serde_json::from_str::<SaveReply>(text);
    if let Ok(mut c) = serde_json::from_str::<SchemaReply>(text) {
        // a catalogue's conversion marks are hostile too: searching them never panics
        let _ = c.convert_paths("D", "AUDIO");
        let _ = c.convert_path_of("D", &["b".to_owned(), "w".to_owned()]);
        // and labels from a hostile table land on a hostile catalogue without panicking
        if let Ok(t) = plumcraft_proto::labels::Labels::from_json(text) {
            t.apply(&mut c);
        }
    }
    let _ = plumcraft_proto::labels::Labels::from_json(text);
    let _ = plumcraft_proto::labels::Labels::from_toml(text);
    let _ = serde_json::from_str::<Hello>(text);
    let _ = serde_json::from_str::<HelloReply>(text);
    let _ = serde_json::from_str::<EngineState>(text);
    let _ = serde_json::from_str::<Display>(text);
    let _ = serde_json::from_str::<WatchRequest>(text);
    let _ = serde_json::from_str::<DataRef>(text);
    let _ = serde_json::from_str::<PortValue>(text);
    let _ = serde_json::from_str::<Handle>(text);
    let _ = serde_json::from_str::<Caps>(text);
    let _ = serde_json::from_str::<ProtoError>(text);
    let _ = text.parse::<Handle>();
    let _ = plumcraft_proto::slice::parse(text);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        let _ = plumcraft_proto::expansion::Expansion::parse(&v);
    }
    let _ = plumcraft_proto::expansion::StoredExpansion::parse(text.as_bytes());
}

#[test]
fn junk_never_panics() {
    for j in JUNK {
        all_parsers(j);
    }
    all_parsers(&deep(10_000));
    all_parsers(&format!("{{\"v\": 2, \"nodes\": [{}]}}", deep(200)));
}

#[test]
fn truncations_never_panic() {
    for seed in SEEDS {
        let chars: Vec<char> = seed.chars().collect();
        for cut in 0..chars.len() {
            all_parsers(&chars[..cut].iter().collect::<String>());
        }
    }
}

/// Replace each scalar in a seed with values of every other JSON type.
#[test]
fn type_confusion_never_panics() {
    let swaps = ["null", "-1", "1.5e308", "\"x\"", "[]", "{}", "true", "18446744073709551615"];
    for seed in SEEDS {
        let v: serde_json::Value = serde_json::from_str(seed).unwrap();
        let mut paths = Vec::new();
        collect(&v, &mut Vec::new(), &mut paths);
        for path in &paths {
            for s in swaps {
                let mut w = v.clone();
                if let Some(slot) = pointer_mut(&mut w, path) {
                    *slot = serde_json::from_str(s).unwrap();
                }
                all_parsers(&w.to_string());
            }
        }
    }
}

fn collect(v: &serde_json::Value, at: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    out.push(at.clone());
    match v {
        serde_json::Value::Object(m) => {
            for (k, c) in m {
                at.push(k.clone());
                collect(c, at, out);
                at.pop();
            }
        }
        serde_json::Value::Array(a) => {
            for (i, c) in a.iter().enumerate() {
                at.push(i.to_string());
                collect(c, at, out);
                at.pop();
            }
        }
        _ => {}
    }
}

fn pointer_mut<'a>(v: &'a mut serde_json::Value, path: &[String]) -> Option<&'a mut serde_json::Value> {
    let p: String = path.iter().map(|s| format!("/{}", s.replace('~', "~0").replace('/', "~1"))).collect();
    v.pointer_mut(&p)
}

/// The engine manifest comes from a file on disk, not the wire, but it is hostile input all the
/// same: truncations, wrong types in every field, deep nesting and junk are errors, never panics.
#[test]
fn engine_manifests_never_panic() {
    use plumcraft_proto::manifest::EngineManifest;
    const SEED: &str = "id = \"weather\"\nname = \"Weather\"\nmode = \"process\"\ncommand = [\"uv\", \"run\"]\ncwd = \".\"\nmax_restarts = 5\ndemo = \"examples/demo.json\"\n[env]\nA = \"b\"\n[conformance]\nslow = \"weather.slow\"\n";
    let _ = EngineManifest::parse(SEED).expect("the seed is valid");
    for n in 0..=SEED.len() {
        if SEED.is_char_boundary(n) {
            let _ = EngineManifest::parse(&SEED[..n]);
        }
    }
    for key in ["id", "name", "mode", "command", "cwd", "env", "max_restarts", "url", "demo", "conformance", "restart", "hint", "token_file"] {
        for value in ["1", "-1", "1.5", "true", "[]", "{}", "\"\"", "[1, 2]", "[[1]]", "{a = 1}", "1979-05-27", "99999999999999999999", "nan", "inf"] {
            let _ = EngineManifest::parse(&format!("{key} = {value}\n"));
            let _ = EngineManifest::parse(&format!("id = \"a\"\ncommand = [\"x\"]\n{key} = {value}\n"));
        }
    }
    for n in [8, 128, 1024, 16_000] {
        let _ = EngineManifest::parse(&format!("id = \"a\"\nx = {}{}\n", "[".repeat(n), "]".repeat(n)));
        let _ = EngineManifest::parse(&format!("id = \"a\"\nx = {}1{}\n", "{a = ".repeat(n), "}".repeat(n)));
        let _ = EngineManifest::parse(&format!("id = \"a\"\n[{}]\n", vec!["a"; n].join(".")));
    }
    for junk in JUNK {
        let _ = EngineManifest::parse(junk);
    }
    let _ = EngineManifest::parse("id = \"\u{0}\"\ncommand = [\"\u{0}\"]\n");
    let _ = EngineManifest::parse("\u{feff}id = \"a\"\n");
    let _ = EngineManifest::parse("[[[[[");
    let _ = EngineManifest::parse("command = [\"unterminated");
}

/// A stored expansion is a file anyone can drop in the folder: bytes changed anywhere, and a graph
/// shaped to hurt, are errors, never panics or hangs.
#[test]
fn stored_expansions_never_panic() {
    use plumcraft_proto::expansion::{MAX_STORED_BYTES, StoredExpansion};
    let seed = SEEDS.last().unwrap().as_bytes();
    assert!(StoredExpansion::parse(seed).is_ok(), "the seed is valid");
    for at in 0..seed.len() {
        for b in [0, b'"', b'{', b'}', b'\\', 0xff] {
            let mut bad = seed.to_vec();
            bad[at] = b;
            let _ = StoredExpansion::parse(&bad);
        }
        let _ = StoredExpansion::parse(&seed[..at]);
    }
    let _ = StoredExpansion::parse(&vec![b'['; MAX_STORED_BYTES]);
    assert!(StoredExpansion::parse(&vec![b'['; MAX_STORED_BYTES + 1]).is_err());
    // a chain as long as the limits allow, closed into one cycle, in a file: refused, quickly
    let nodes: serde_json::Map<String, serde_json::Value> = (0..4096).map(|i| (format!("n{i}"), serde_json::json!({"kind": "t.a"}))).collect();
    let wires: Vec<_> = (0..4096)
        .map(|i| serde_json::json!({"from": {"node": format!("n{i}"), "port": "o"}, "to": {"node": format!("n{}", (i + 1) % 4096), "port": "i"}}))
        .collect();
    let e = serde_json::json!({"command": "t.ring", "nodes": nodes, "wires": wires, "outputs": {"o": ["n0", "o"]}});
    let file = StoredExpansion::file("Ring", "", e).to_string();
    assert!(StoredExpansion::parse(file.as_bytes()).unwrap_err().contains("cycle"));
}

/// An STL file is bytes from anywhere (#38): changed anywhere, cut anywhere, or claiming more
/// triangles than it holds, it reads as an error or as the triangles there are, never a panic.
#[test]
fn stl_files_never_panic() {
    use plumcraft_proto::mesh::{MAX_TRIANGLES, read_stl, tetrahedron, write_ascii_stl, write_binary_stl};
    let binary = write_binary_stl("solid trick", &tetrahedron());
    let ascii = write_ascii_stl("t", &tetrahedron()).into_bytes();
    for seed in [&binary, &ascii] {
        assert!(read_stl(seed).is_ok(), "the seed is valid");
        for at in 0..seed.len() {
            let _ = read_stl(&seed[..at]);
            for b in [0, 0xff, b' ', b'\n', b'e', b'-', b'9'] {
                let mut bad = seed.to_vec();
                bad[at] = b;
                if let Ok(m) = read_stl(&bad) {
                    assert!(m.triangles.iter().flatten().flatten().all(|x| x.is_finite()));
                }
            }
        }
    }
    // a header claiming 4 billion triangles reserves nothing for them
    let mut huge = binary.clone();
    huge[80..84].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(read_stl(&huge).unwrap().triangles.len(), 4);
    // more triangles than the cap, in either form
    let many = 84 + 50 * (MAX_TRIANGLES + 1);
    assert!(read_stl(&vec![0u8; many]).is_err());
    let facets = "facet outer loop vertex 0 0 0 vertex 1 0 0 vertex 0 1 0 endloop endfacet\n".repeat(MAX_TRIANGLES / 64);
    let _ = read_stl(format!("solid x\n{facets}").as_bytes());
    for junk in JUNK {
        let _ = read_stl(junk.as_bytes());
    }
    let _ = read_stl(b"solid\nfacet vertex vertex vertex vertex 1 2 3 endfacet endfacet");
    let _ = read_stl("solid ☃\nfacet\u{0}vertex 1e40 -0 nan endfacet".as_bytes());
}
