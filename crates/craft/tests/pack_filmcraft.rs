//! The FilmCraft pack (`packs/filmcraft.toml`): it parses, its demo uses only its nodes, and,
//! when `filmcraft-cli` is on PATH, its curated nodes all pass the catalogue check and a chain
//! (new project → demo footage → sequence from it → text → look → MP4 and GIF) exports a real
//! video, warm and after a cold reopen of the project from the CAS.

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Kind, Manifest, builtin, demo};
use plumcraft_proto::data::Form;
use plumcraft_proto::run::RunReply;
use serde_json::{Value, json};

const CURATED: &[&str] = &[
    "fc.new",
    "fc.sequence",
    "fc.settings",
    "fc.inspect",
    "fc.items",
    "fc.demo_footage",
    "fc.color_matte",
    "fc.bars_tone",
    "fc.place",
    "fc.text",
    "fc.shape",
    "fc.marker",
    "fc.effect",
    "fc.look",
    "fc.speed",
    "fc.export_mp4",
    "fc.export_mov",
    "fc.export_gif",
    "fc.export_wav",
    "fc.export_png",
];

fn filmcraft() -> Option<Manifest> {
    let m = builtin("filmcraft").unwrap().unwrap();
    if m.executable().is_none() {
        eprintln!("SKIP: filmcraft-cli is not on PATH; {}", m.hint);
        return None;
    }
    Some(m)
}

fn connect(m: &Manifest, name: &str) -> (Conn, plumcraft_craft::Report) {
    let opts = plumcraft_craft::Options { root: Some(craft_root(name)), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(m, &opts).unwrap();
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(60));
    (Box::new(plumcraft_host::InProc(h)), report)
}

fn failures(r: &RunReply) -> Vec<(String, String)> {
    r.nodes.iter().filter_map(|(k, n)| n.error.as_ref().map(|e| (k.clone(), format!("{}: {}", e.kind, e.message)))).collect()
}

#[test]
fn the_filmcraft_pack_parses_and_its_demo_uses_its_nodes() {
    let m = builtin("filmcraft").unwrap().unwrap();
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    assert!(m.roots.is_none(), "filmcraft-cli mcp has no automation roots");
    assert!(m.nodes.iter().any(|n| n.kind == Kind::New));
    assert_eq!(m.actions.export.as_ref().and_then(|a| a.command.as_deref()), Some("file.exportMedia"));
    assert_eq!(m.gui.as_ref().and_then(|g| g.env.as_deref()), Some("PLUMCRAFT_FILMCRAFT_APP"));
    let d: Value = serde_json::from_str(demo("filmcraft").unwrap()).unwrap();
    for (name, n) in d["nodes"].as_object().unwrap() {
        let kind = n["kind"].as_str().unwrap();
        assert!(ids.contains(&kind), "demo node {name} is {kind}");
    }
}

fn demo_graph(mp4: Value) -> Value {
    graph(
        json!({
            "new": {"kind": "fc.new", "values": {"name": "Demo"}},
            "footage": {"kind": "fc.demo_footage", "values": {"scene": "Aurora"}},
            "seq": {"kind": "fc.sequence", "values": {"name": "Aurora"}},
            "title": {"kind": "fc.text", "values": {"text": "PlumCraft", "seconds": 3.0}},
            "look": {"kind": "fc.look", "values": {"name": "Golden Dusk"}},
            "mp4": {"kind": "fc.export_mp4", "values": mp4},
            "gif": {"kind": "fc.export_gif", "values": {"scale": 0.1}}
        }),
        &[
            ("new", "doc", "footage", "doc"),
            ("footage", "doc", "seq", "doc"),
            ("footage", "item", "seq", "fromItem"),
            ("seq", "doc", "title", "doc"),
            ("title", "doc", "look", "doc"),
            ("look", "doc", "mp4", "doc"),
            ("look", "doc", "gif", "doc"),
        ],
    )
}

fn mp4_bytes(h: &Conn, r: &RunReply) -> Vec<u8> {
    let v = blob(r, "mp4", "video");
    assert_eq!(v.meta.media, "video/mp4");
    let bytes = h.resolve(&v).expect("the MP4 resolves").bytes.into_vec();
    assert!(bytes.len() > 10_000, "{} bytes", bytes.len());
    assert_eq!(&bytes[4..8], b"ftyp", "an ISO media file");
    bytes
}

#[test]
fn the_filmcraft_demo_chain_exports_video() {
    let Some(m) = filmcraft() else { return };
    let (h, report) = connect(&m, "pack-filmcraft");
    eprintln!("FilmCraft {}: nodes {:?}; hatch {:?}; dropped {:?}", report.server_version, report.curated, report.hatch, report.dropped);
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(report.curated, CURATED);
    assert_eq!(report.hatch, ["fc.run"]);

    let short = json!({"scale": 0.25, "range": "custom", "startSeconds": 0.0, "endSeconds": 2.0});
    let r = go(&h, "fc-warm", demo_graph(short), &["footage", "look", "mp4", "gif"], "use");
    assert!(r.ok, "{:?}", failures(&r));
    assert_eq!(blob(&r, "look", "doc").meta.form, Form::Doc { app: "filmcraft".into() });
    assert_eq!(json_out(&r, "footage", "item"), json!(1));
    let warm = mp4_bytes(&h, &r);
    let res = json_out(&r, "mp4", "result");
    assert_eq!(res["result"]["frames"], 48, "2 s at 23.976 fps: {res}");
    let gif = h.resolve(&blob(&r, "gif", "image")).unwrap().bytes;
    assert_eq!(&gif[0..4], b"GIF8");

    // Cold: the one process (pool max 1) now holds another project, so the export reopens the
    // cached look project from the CAS (demo footage and text live in it) and renders the same length.
    let other = go(&h, "fc-other", chain(&[("n", "fc.new", json!({"name": "Other"})), ("i", "fc.items", json!({}))]), &["i"], "off");
    assert!(other.ok, "{:?}", failures(&other));
    let cold = go(&h, "fc-cold", demo_graph(json!({"scale": 0.2, "range": "custom", "startSeconds": 0.0, "endSeconds": 2.0})), &["look", "mp4"], "use");
    assert!(cold.ok, "{:?}", failures(&cold));
    assert_eq!(blob(&cold, "look", "doc").handle, blob(&r, "look", "doc").handle, "upstream came from the memo");
    let c = mp4_bytes(&h, &cold);
    assert_eq!(json_out(&cold, "mp4", "result")["result"]["frames"], 48);
    assert!(c.len() > warm.len() / 4, "the cold render is real video: {} vs {}", c.len(), warm.len());
}

/// The other nodes and the escape hatch in one chain: an empty sequence, a colour matte placed
/// on it, bars and tone placed after it, a shape, an effect, a speed change, a marker, the
/// settings, then MOV, WAV and the inspect.
#[test]
fn every_filmcraft_node_runs() {
    let Some(m) = filmcraft() else { return };
    let (h, _) = connect(&m, "pack-filmcraft-all");
    let g = graph(
        json!({
            "a": {"kind": "fc.new", "values": {}},
            "b": {"kind": "fc.sequence", "values": {"width": 320, "height": 180, "fps": 24.0}},
            "c": {"kind": "fc.color_matte", "values": {"color": "#a02050", "seconds": 1.0}},
            "d": {"kind": "fc.place", "values": {"track": "V1", "seconds": 0.0}},
            "e": {"kind": "fc.bars_tone", "values": {"seconds": 1.0}},
            "f": {"kind": "fc.place", "values": {"track": "V1", "seconds": 1.0}},
            "g": {"kind": "fc.shape", "values": {"shape": "ellipse", "size": [100, 100], "seconds": 1.0}},
            "h": {"kind": "fc.effect", "values": {"effect": "tint"}},
            "i": {"kind": "fc.speed", "values": {"speed": 200.0}},
            "j": {"kind": "fc.marker", "values": {"name": "Start"}},
            "k": {"kind": "fc.settings", "values": {"fps": 25.0}},
            "l": {"kind": "fc.run", "values": {"command": "timeline.setTrack", "params": {"track": "A1", "volumeDb": -6.0}}},
            "m": {"kind": "fc.export_mov", "values": {"proresProfile": "proxy"}},
            "n": {"kind": "fc.export_wav", "values": {}},
            "o": {"kind": "fc.inspect", "values": {}},
            "p": {"kind": "fc.items", "values": {}}
        }),
        &[
            ("a", "doc", "b", "doc"),
            ("b", "doc", "c", "doc"),
            ("c", "doc", "d", "doc"),
            ("c", "item", "d", "item"),
            ("d", "doc", "e", "doc"),
            ("e", "doc", "f", "doc"),
            ("e", "item", "f", "item"),
            ("f", "doc", "g", "doc"),
            ("g", "doc", "h", "doc"),
            ("h", "doc", "i", "doc"),
            ("i", "doc", "j", "doc"),
            ("j", "doc", "k", "doc"),
            ("k", "doc", "l", "doc"),
            ("l", "doc", "m", "doc"),
            ("l", "doc", "n", "doc"),
            ("l", "doc", "o", "doc"),
            ("l", "doc", "p", "doc"),
        ],
    );
    let r = go(&h, "fc-all", g, &["m", "n", "o", "p"], "off");
    assert!(r.ok, "{:?}", failures(&r));
    let mov = h.resolve(&blob(&r, "m", "video")).unwrap().bytes;
    assert!(mov.len() > 1000, "{} bytes", mov.len());
    assert!(mov.windows(4).take(64).any(|w| w == b"ftyp" || w == b"moov" || w == b"wide" || w == b"mdat"), "a QuickTime file: {:?}", &mov[..16]);
    let wav = h.resolve(&blob(&r, "n", "audio")).unwrap().bytes;
    assert_eq!((&wav[0..4], &wav[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    let s = json_out(&r, "o", "result");
    let text = s.to_string();
    assert!(text.contains("tint") && text.contains("Start"), "{}", &text[..text.len().min(2000)]);
    assert!(json_out(&r, "p", "result").to_string().contains("Bars"), "the bars and tone item is in the project");
}

/// Round 2: a PNG still through `file.exportFrame`, and `select = "ids:…"` by clip id
/// (`set_key = "clips"`): only the chosen clip gets the effect.
#[test]
fn a_png_frame_and_a_selection_by_clip_id() {
    let Some(m) = filmcraft() else { return };
    let (h, _) = connect(&m, "pack-filmcraft-png");
    let base = |effect: Value| {
        graph(
            json!({
                "a": {"kind": "fc.new", "values": {}},
                "b": {"kind": "fc.sequence", "values": {"width": 320, "height": 180, "fps": 24.0}},
                "c": {"kind": "fc.color_matte", "values": {"color": "#20a050", "seconds": 2.0}},
                "d": {"kind": "fc.place", "values": {}},
                "t": {"kind": "fc.text", "values": {"text": "Still", "seconds": 2.0}},
                "e": {"kind": "fc.effect", "values": effect},
                "i": {"kind": "fc.inspect", "values": {}},
                "p": {"kind": "fc.export_png", "values": {"time": 254016000000_u64}}
            }),
            &[
                ("a", "doc", "b", "doc"),
                ("b", "doc", "c", "doc"),
                ("c", "doc", "d", "doc"),
                ("c", "item", "d", "item"),
                ("d", "doc", "t", "doc"),
                ("t", "doc", "e", "doc"),
                ("e", "doc", "i", "doc"),
                ("e", "doc", "p", "doc"),
            ],
        )
    };
    let r = go(&h, "fc-png", base(json!({"effect": "tint", "select": "none"})), &["t", "p"], "use");
    // `select = none` leaves nothing selected: effects.apply refuses, a config error.
    let clip = json_out(&r, "t", "clip").as_u64().unwrap_or_else(|| panic!("{:?}", failures(&r)));
    assert!(r.nodes["e"].error.is_some(), "an effect on nothing is an error");
    let r = go(&h, "fc-png2", base(json!({"effect": "tint", "select": format!("ids:{clip}")})), &["e", "i", "p"], "use");
    assert!(r.ok, "{:?}", failures(&r));
    let png = blob(&r, "p", "image");
    assert!(matches!(png.meta.form, Form::Image { width: 320, height: 180 }), "{:?}", png.meta.form);
    let bytes = h.resolve(&png).unwrap().bytes;
    assert_eq!(&bytes[1..4], b"PNG");
    let applied = json_out(&r, "e", "result")["applied"].clone();
    assert_eq!(applied.as_array().map(Vec::len), Some(1), "only the chosen clip: {applied}");
    assert_eq!(applied[0]["clip"], json!(clip));
}

/// A WAV file's length in seconds, from its `fmt ` and `data` chunks.
fn wav_seconds(b: &[u8]) -> f64 {
    let (mut at, mut rate, mut align, mut data) = (12, 0u32, 0u16, 0u32);
    while at + 8 <= b.len() {
        let size = u32::from_le_bytes([b[at + 4], b[at + 5], b[at + 6], b[at + 7]]);
        match &b[at..at + 4] {
            b"fmt " => {
                rate = u32::from_le_bytes([b[at + 12], b[at + 13], b[at + 14], b[at + 15]]);
                align = u16::from_le_bytes([b[at + 20], b[at + 21]]);
            }
            b"data" => data = size,
            _ => {}
        }
        at += 8 + size as usize + (size as usize & 1);
    }
    assert!(rate > 0 && align > 0, "a WAV with a fmt chunk");
    f64::from(data) / f64::from(align) / f64::from(rate)
}

/// The export options: a one-second custom range on MOV, GIF and WAV (from a four-second
/// sequence), the MOV without audio, and the GIF at its own size and frame rate.
#[test]
fn a_custom_range_exports_one_second() {
    let Some(m) = filmcraft() else { return };
    let (h, _) = connect(&m, "pack-filmcraft-range");
    let one = |extra: Value| {
        let mut v = json!({"range": "custom", "startSeconds": 1.0, "endSeconds": 2.0});
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        v
    };
    let g = graph(
        json!({
            "a": {"kind": "fc.new", "values": {}},
            "b": {"kind": "fc.sequence", "values": {"width": 320, "height": 180, "fps": 24.0}},
            "c": {"kind": "fc.bars_tone", "values": {"seconds": 4.0}},
            "d": {"kind": "fc.place", "values": {"track": "V1", "seconds": 0.0}},
            "mov": {"kind": "fc.export_mov", "values": one(json!({"proresProfile": "proxy", "audio": false}))},
            "gif": {"kind": "fc.export_gif", "values": one(json!({"width": 80, "height": 45, "fps": 10.0}))},
            "wav": {"kind": "fc.export_wav", "values": one(json!({}))},
            "all": {"kind": "fc.export_wav", "values": {}}
        }),
        &[
            ("a", "doc", "b", "doc"),
            ("b", "doc", "c", "doc"),
            ("c", "doc", "d", "doc"),
            ("c", "item", "d", "item"),
            ("d", "doc", "mov", "doc"),
            ("d", "doc", "gif", "doc"),
            ("d", "doc", "wav", "doc"),
            ("d", "doc", "all", "doc"),
        ],
    );
    let r = go(&h, "fc-range", g, &["mov", "gif", "wav", "all"], "off");
    assert!(r.ok, "{:?}", failures(&r));
    let mov = json_out(&r, "mov", "result");
    assert_eq!(mov["result"]["frames"], 24, "1 s at 24 fps: {mov}");
    let gif = json_out(&r, "gif", "result");
    assert_eq!(gif["result"]["frames"], 10, "1 s at 10 fps: {gif}");
    let bytes = h.resolve(&blob(&r, "gif", "image")).unwrap().bytes;
    let size = (u16::from_le_bytes([bytes[6], bytes[7]]), u16::from_le_bytes([bytes[8], bytes[9]]));
    assert_eq!((&bytes[0..4], size), (&b"GIF8"[..], (80, 45)));
    let wav = wav_seconds(&h.resolve(&blob(&r, "wav", "audio")).unwrap().bytes);
    assert!((wav - 1.0).abs() < 0.01, "the range: {wav} s");
    let all = wav_seconds(&h.resolve(&blob(&r, "all", "audio")).unwrap().bytes);
    assert!((all - 4.0).abs() < 0.05, "the whole sequence: {all} s");
}
