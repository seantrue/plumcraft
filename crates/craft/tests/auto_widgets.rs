//! Auto widgets: a curated node exposes the rest of its command's parameters as `advanced`,
//! `optional` widgets typed from the craft's own description, except what it sends, hides or
//! can't own (plumbing, files, the selection's keys), and never on an export whose command
//! several formats share. Against a fake craft (a Python MCP server) whose command list uses
//! the three dialects: VectorCraft's, quoted keys, and SolidCraft's prose.

use plumcraft_craft::manifest::Manifest;
use plumcraft_craft::{Options, build};
use plumcraft_proto::catalogue::{NodeDef, Port};
use serde_json::{Value, json};

/// The fake craft: `initialize`, and `list_commands` answering `commands`.
const FAKE: &str = r#"
import json, sys
commands = json.loads(sys.argv[1])
for line in sys.stdin:
    try:
        msg = json.loads(line)
    except Exception:
        continue
    if "id" not in msg:
        continue
    if msg.get("method") == "initialize":
        result = {"protocolVersion": "2025-06-18", "serverInfo": {"name": "fake", "version": "1"}, "capabilities": {"tools": {}}}
    elif msg.get("method") == "tools/call" and msg["params"]["name"] == "list_commands":
        result = {"content": [{"type": "text", "text": json.dumps(commands)}]}
    else:
        result = {"content": [{"type": "text", "text": "{}"}], "isError": True}
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}) + "\n")
    sys.stdout.flush()
"#;

fn commands() -> Value {
    json!([
        {"id": "shape.make", "params": "{kind: rect|ellipse, width?: pt (100), sides=6, name?|title?: label, ids?: [id], points?: [[x,y]], path?: file, wait?: bool} → {id, made}"},
        {"id": "clip.place", "params": "{\"item\":id|\"items\":[id],\"track\":\"V1\"|id|\"new\"?,\"fps\":f64=23.976,\"mix\":\"Stereo|Mono\"?,\"secret\":str?}"},
        {"id": "solid.round", "params": "radius | diameter: expr; center?: [x,y,z], the origin by default; flip?: bool (combine?: join with the original) → volume: mm3, ok"},
        {"id": "file.export", "params": "{path, format?, quality?: 0..100}"},
        {"id": "frame.save", "params": "{path, format?: png|jpg, scale?: 1..8}"},
    ])
}

const MANIFEST: &str = r#"
id = "fake"
name = "Fake"
command = ["python3", "-c", "FAKE", "COMMANDS"]

[tools]
list = "list_commands"
run = { tool = "execute", id = "command", params = "params" }

[actions]
open = { tool = "open", path = "path" }
save = { tool = "save", path = "path" }
export = { command = "file.export", path = "path" }

[selection]
all = "select.all"
none = "select.none"
set = "select.set"

[doc]
wire = "fake.doc"
ext = "fake"
media = "application/x-fake"
app = "fake"

[auto_widgets]
hide = ["secret"]

[[node]]
id = "f.shape"
display = "Shape"
kind = "transform"
command = "shape.make"
select = "all"
[[node.widget]]
name = "kind"
type = "ENUM"
choices = ["rect", "ellipse"]
default = "rect"

[[node]]
id = "f.shape_hidden"
display = "Shape, hiding"
kind = "transform"
command = "shape.make"
hide = ["width", "sides"]

[[node]]
id = "f.shape_plain"
display = "Shape, no auto widgets"
kind = "transform"
command = "shape.make"
auto = false

[[node]]
id = "f.place"
display = "Place"
kind = "transform"
command = "clip.place"
[[node.widget]]
name = "item"
type = "STRING"
default = "a"

[[node]]
id = "f.round"
display = "Round"
kind = "transform"
command = "solid.round"
[[node.widget]]
name = "radius"
type = "STRING"
default = "5"
[[node.widget]]
name = "depth"
type = "STRING"
default = "1"

[[node]]
id = "f.png"
display = "PNG"
kind = "export"
format = "png"
ext = "png"

[[node]]
id = "f.svg"
display = "SVG"
kind = "export"
format = "svg"
ext = "svg"

[[node]]
id = "f.frame"
display = "Frame"
kind = "export"
command = "frame.save"
ext = "png"
"#;

fn python() -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("python3")).find(|c| c.is_file()))
}

fn built(manifest: &str) -> Option<(Vec<NodeDef>, plumcraft_craft::Report)> {
    if python().is_none() {
        eprintln!("skipped: no python3 on PATH for the fake craft");
        return None;
    }
    let mut m = Manifest::parse(manifest).unwrap();
    m.command = vec!["python3".into(), "-c".into(), FAKE.into(), commands().to_string()];
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let root = plumcraft_scratch::dir(&format!("craft-auto-widgets-{}", N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let (e, report, _) = build(&m, &Options { root: Some(root), store: Some(None), include_excluded: false }).unwrap();
    Some((e.schema().nodes, report))
}

fn node<'a>(nodes: &'a [NodeDef], id: &str) -> &'a NodeDef {
    nodes.iter().find(|n| n.id == id).unwrap_or_else(|| panic!("no node {id}"))
}

fn port<'a>(n: &'a NodeDef, name: &str) -> Option<&'a Port> {
    n.inputs.iter().find(|p| p.name == name)
}

fn names(n: &NodeDef) -> Vec<&str> {
    n.inputs.iter().map(|p| p.name.as_str()).collect()
}

#[test]
fn a_curated_node_exposes_the_rest_of_its_command() {
    let Some((nodes, report)) = built(MANIFEST) else { return };
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    let shape = node(&nodes, "f.shape");
    // what the node lists stays as it was; the rest are advanced, optional and unset
    assert_eq!(port(shape, "kind").unwrap().default, json!("rect"));
    let width = port(shape, "width").unwrap();
    assert!(width.advanced && width.optional && width.default.is_null(), "{width:?}");
    assert_eq!(width.ty, "FLOAT");
    assert_eq!(port(shape, "sides").unwrap().ty, "INT");
    assert_eq!(port(shape, "title").unwrap().ty, "STRING");
    assert_eq!(port(shape, "points").unwrap().ty, "JSON", "arrays are JSON widgets");
    // plumbing, files and the reply are never widgets; `ids` is the select port's own socket
    for k in ["path", "wait", "made"] {
        assert!(port(shape, k).is_none(), "{k}: {:?}", names(shape));
    }
    assert_eq!(shape.inputs.iter().filter(|p| p.name == "ids").count(), 1);
    assert!(!port(shape, "ids").unwrap().advanced, "the select port's socket, not an auto widget");
}

#[test]
fn hide_and_auto_false_leave_parameters_out() {
    let Some((nodes, _)) = built(MANIFEST) else { return };
    let hidden = node(&nodes, "f.shape_hidden");
    assert!(port(hidden, "width").is_none() && port(hidden, "sides").is_none(), "{:?}", names(hidden));
    assert!(port(hidden, "title").is_some());
    let plain = node(&nodes, "f.shape_plain");
    assert_eq!(names(plain), ["doc"], "auto = false: only what the pack lists");
    // the manifest-wide hide
    assert!(port(node(&nodes, "f.place"), "secret").is_none());
}

#[test]
fn quoted_and_prose_dialects_are_typed() {
    let Some((nodes, report)) = built(MANIFEST) else { return };
    let place = node(&nodes, "f.place");
    assert!(port(place, "items").is_some(), "{:?}", names(place));
    let track = port(place, "track").unwrap();
    assert_eq!((track.ty.as_str(), track.choices.as_ref()), ("STRING", None), "\"V1\"|id|\"new\" is an open set");
    let mix = port(place, "mix").unwrap();
    assert_eq!(mix.ty, "ENUM");
    assert_eq!(mix.choices.as_deref().unwrap(), [json!("Stereo"), json!("Mono")]);
    assert_eq!(port(place, "fps").unwrap().ty, "FLOAT");
    let round = node(&nodes, "f.round");
    for k in ["diameter", "center", "flip", "combine"] {
        assert!(port(round, k).is_some(), "{k}: {:?}", names(round));
    }
    assert!(port(round, "volume").is_none() && port(round, "ok").is_none(), "the reply after → is not a widget");
    assert_eq!(port(round, "center").unwrap().ty, "JSON");
    assert_eq!(port(round, "flip").unwrap().ty, "BOOL");
    // `depth` isn't in the prose, which the grammar reads only in part: a warning, not a drop
    assert!(report.curated.contains(&"f.round".to_owned()), "{:?}", report.dropped);
    assert!(report.warnings.iter().any(|(id, why)| id == "f.round" && why.contains("depth")), "{:?}", report.warnings);
}

#[test]
fn exports_on_a_shared_command_get_none() {
    let Some((nodes, _)) = built(MANIFEST) else { return };
    for id in ["f.png", "f.svg"] {
        let n = node(&nodes, id);
        assert!(port(n, "quality").is_none(), "{id}: {:?}", names(n));
    }
    // its own command: the options, never `format` (the node's own) or the path
    let frame = node(&nodes, "f.frame");
    assert!(port(frame, "scale").is_some(), "{:?}", names(frame));
    assert!(port(frame, "format").is_none() && port(frame, "path").is_none());
}

#[test]
fn a_braced_description_missing_a_key_still_drops_the_node() {
    let m = MANIFEST.replace("name = \"kind\"\ntype = \"ENUM\"", "name = \"colour\"\ntype = \"ENUM\"");
    let Some((_, report)) = built(&m) else { return };
    assert!(report.dropped.iter().any(|(id, why)| id == "f.shape" && why.contains("colour")), "{:?}", report.dropped);
}
