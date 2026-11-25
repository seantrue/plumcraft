# plumcraft-engine

PlumCraft's engine SDK in Rust: write nodes as `Node`s, collect them in an `Engine`, and host it in process (`host::Host`), as a process on a socket (`serve`) or over HTTP (`http`). The executor runs workflow JSON and emits every status record the interface requires; values cross as `Data` (JSON, arrays, audio, blobs, lists). The interface is `docs/interface.md` in the PlumCraft repository; the Python SDK (`plumcraft-engine` on PyPI) is its twin.

## Save Result

An engine gets Save Result with no code: a node that outputs a core type offers the SDK's formats for it (`save::defaults`), and the editor's Save Result… (a result node's menu and footer, the Watched panel) writes the file through the engine (`output.save`). TEXT saves as `.txt`; TABLE as `.csv` or `.json`; JSON as `.json`; IMAGE as `.png` or `.jpg` (with a quality); ARRAY as `.png` (its pixels when 8-bit, else the viewer's `auto` view), `.jpg` or `.npy`; SVG as `.svg`; AUDIO as `.wav` (32-bit float or 16-bit) or `.npy`. The first format a node offers is what a bare file name gets. A format of your own is a `save::Format`, its options ports the editor asks for with a form:

```rust
use plumcraft_engine::save::{Format, option};
let mark = Port { choices: Some(vec![json!("!"), json!("?")]), ..option("mark", "Mark", "ENUM", json!("!")) };
let shout = Format::new("shout", "Shouted Text", &["shout"], &[Media::Text], "text/plain", |v, cx| {
    Ok(format!("{}{}", v.as_str().unwrap_or_default().to_uppercase(), cx.options.get("mark").and_then(Value::as_str).unwrap_or("!")).into_bytes())
});
let engine = engine.with_save_format(shout.with_option(mark)); // and a node's own list: def.save_formats = Some(vec!["shout".into(), "txt".into()])
```

`with_save_format` adds a format (one with a default's id replaces it); `NodeDef::save_formats` gives a node its own list, in order (`Some(vec![])` offers none); `without_save_format("jpeg")` and `without_default_save_formats()` take formats away; the engine lists `output.save` while it has any. A save runs the graph for the node (a memo hit after a run), writes to the path the editor names (or returns the file as a blob, which the editor writes, when the engine is remote), saves a list as numbered files (`<stem>-00.<ext>`, …) or, for Contact Sheet…, as one picture of every item (PNG and JPEG), writes the workflow's name and graph hash into PNG, WAV and SVG files (informational; no recipe is ever embedded), and reports a failure as data: `config` for a format or option the output doesn't take, the node's own error when computing it fails (`docs/interface.md` section 15). An engine with a core of its own (`Engine::with_core`) answers `EngineCore::save` itself.
