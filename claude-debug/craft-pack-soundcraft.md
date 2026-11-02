# Craft pack: SoundCraft

Worker B, 2026-10-28. Status: **done** (for self-contained sessions; see the limits below).

## What the pack covers

`crates/craft/packs/soundcraft.toml` grows the M2.6 genericity pack into a real one: build a session from MIDI on built-in instrument tracks, edit the MIDI and the timeline, mix it through built-in plug-ins, and bounce it to WAV, FLAC or AIFF. `sc.new` and `sc.inspect` are kept.

- Server: `soundcraft-cli mcp` (0.14.0), confined by `--automation-read-root`/`--automation-write-root`.
- Tools: `list_commands`; `execute {command, params}`; `open_session`/`save_session {path, replace}`; `session.close`; export through the `bounce_mix` tool (it takes `format`, `path`, `bit_depth`, `normalize` and returns `peak_db`, `lufs`, `seconds`).
- Doc: wire `soundcraft.session`, ext `scraft`, stem `session`.
- GUI: `soundcraft <file>` (on PATH in `~/bin`), override `PLUMCRAFT_SOUNDCRAFT_APP`.
- Demo: `soundcraft-demo.json`: New → Instrument track (subtractive synth) → MIDI notes → Transpose +5 → Reverb → Bounce WAV (normalised), with Inspect on the side.

## Nodes (21 curated + `sc.run`; round 2 added the MIDI and MusicXML exports)

| Node | Kind | Command |
| --- | --- | --- |
| sc.new | new | session.new (template fixed to blank) |
| sc.inspect | query | session.inspect |
| sc.instrument_track | transform | track.new {kind: instrument} (subtractive_synth or drum_synth) |
| sc.notes | transform | event.retrospective_record {track, notes} |
| sc.transpose | transform | event.transpose |
| sc.quantize | transform | event.quantize |
| sc.velocity | transform | event.change_velocity |
| sc.tempo | transform | event.tempo |
| sc.meter | transform | event.meter |
| sc.marker | transform | markers.add |
| sc.volume | transform | mix.volume |
| sc.pan | transform | mix.pan |
| sc.insert | transform | mix.insert (any built-in effect plug-in, params JSON) |
| sc.reverb | transform | mix.insert plate_reverb or room_reverb |
| sc.delay | transform | mix.insert mod_delay |
| sc.compressor | transform | mix.insert compressor |
| sc.export_wav | export | bounce_mix wav (wire `WAV`, no viewer) |
| sc.export_flac | export | bounce_mix flac (wire `FLAC`, no viewer) |
| sc.export_aiff | export | bounce_mix aiff (wire `AIFF`, no viewer) |
| sc.export_midi | export | own command file.export_midi (wire `MIDI`, no viewer) |
| sc.export_musicxml | export | own command file.export_sibelius (wire `MUSICXML`, no viewer) |
| sc.run | hatch | allow event., mix., markers., automation., midi., track., clip., edit., session.inspect, clip.inspect; deny list in the pack |

## Tests

`cargo test -p plumcraft-craft --test pack_soundcraft`: 3 pass (the pack parses and the demo uses its nodes; the demo chain bounces a non-silent WAV warm, then again after a cold reopen of the memoised session from the CAS, with the same peak; every other node and `sc.run` in one chain, bounced to FLAC and AIFF, with inspect confirming volume, inserts, tempo and marker). The existing `manifest`, `conformance` (incl. `soundcraft_runs_from_its_manifest`) and `client` suites pass.

Edits to existing tests, because the pack grew (allowed by the brief):

- `tests/conformance.rs` `soundcraft_runs_from_its_manifest`: `assert_eq!(report.curated, ["sc.new", "sc.inspect"])` became "the first two curated nodes are sc.new and sc.inspect".
- `tests/manifest.rs` `the_vectorcraft_pack_names_its_app_and_a_demo`: `soundcraft gui.is_none()` became "SoundCraft's gui label is SoundCraft"; `demo("soundcraft").is_none()` became `demo("nosuchcraft").is_none()`.

`tests/packs_all.rs` currently fails on **wordcraft** (`design.styleSet` contains "sign"), not on this pack.

## Assumptions

- Tracks are named, not selected: every node that acts on a track takes `track`/`tracks` (default `Synth`, the instrument track's default name), so a chain doesn't depend on the selection a cold open restores.
- MIDI ticks are 960 a beat (inferred from clip lengths in the probe; the description of `sc.notes` says so).
- Bounces are byte-stable (same session → same WAV), and a session saves byte-stable (checked with `cmp` in the probe), so the export nodes are pure.
- Third-party plug-ins (`clap:`, `vst3:`, `au:`) aren't offered by `sc.insert`; `sc.run` can still name one through `mix.insert`. A headless engine without a rescan only knows the built-ins.

## Left out, and why

- **Audio files (the main limit).** A session with recorded, imported or rendered audio saves it beside the `.scraft` in `Audio Files/` (`audio_files_written` in the save reply); the adapter adopts only the `.scraft` and removes the temp folder, so the audio is lost on the next cold open. Hence no import-audio node, no AudioSuite (`audiosuite.process`, including `signal_generator`), no freeze/commit/track bounce/consolidate/clip render, and `sc.new`'s `template` is fixed to `blank` (the demo template writes 5 audio files). **Adapter change needed:** a bundle document kind: `doc.bundle = true` (or `doc.kind = "folder"`), where save goes to `<tmp>/<stem>/<stem>.<ext>`, the whole folder is adopted into the CAS as one value (a deterministic tar, or a directory handle hashed over sorted relative paths and bytes), and `ensure` unpacks it before `open`. Alternatively a SoundCraft change (not ours to make now, D15): a `save_session {embed_audio: true}` single-file form.
- **Open from an audio file.** `kind = "open"` always runs `actions.open` (`open_session`), so a WAV can't be opened as a new session. **Adapter change:** let an open node name its own command/tool (`NodeSpec.command`/`tool` for `kind = "open"`, with the path key), e.g. `session.new` then `file.import_audio {path}`; it also needs the bundle kind above to be useful.
- **SVG score / MIDI / MusicXML exports.** `file.print_score` (SVG), `file.export_midi`, `file.export_sibelius` are commands with a path, but the adapter has one `actions.export` per pack (here `bounce_mix`). **Adapter change:** per-node export action (`NodeSpec.action = {command|tool, path}` for `kind = "export"`, falling back to `actions.export`). `print_score` also trips the "print" word in `packs_all.rs`' deny list although it only writes an SVG; it would need an exception.
- `bounce_mix`'s `start`/`end` aren't widgets: `end: "0:04.000"` was ignored in a probe (the whole session bounced). Not investigated further.
- No PNG/SVG/PDF export (only through `print_score`, above), so no thumbnails and no in-editor viewer for the bounces.

## Probe

`claude-debug/soundcraft_probe.py` (generic MCP stdio probe), scripts `soundcraft_s*.json`, outputs `soundcraft_tools.json`, `soundcraft_commands.json`/`.txt`, `soundcraft_plugins.json`, scratch audio in `claude-debug/sc_work/`.

## Round 2 (2026-10-28, later)

- Added `sc.export_midi` (`file.export_midi`, a Standard MIDI File) and `sc.export_musicxml` (`file.export_sibelius`, MusicXML 4.0) through the per-node export `command`. Both are byte-stable (`cmp`), so pure. Tested in `every_soundcraft_node_runs` (`MThd`/`MTrk`; `<score-partwise>` with notes). The parse test now accepts an export with its own command and no `format`.
- **Score SVG left out on purpose:** the engraved page is `file.print_score`, and `packs_all.rs` denies "print" at a word boundary. Putting it in a node's `action` would get past the test (it only checks `node.command`), which is against the rule, so it isn't done. If the maintainer agrees it only writes an SVG, the fix is an explicit exception in `packs_all.rs`, then `sc.export_score_svg` with `command = "file.print_score"`.
- The stale-assertion edits from round 1 are still in place:
  - `tests/conformance.rs` `soundcraft_runs_from_its_manifest`: `assert_eq!(report.curated, ["sc.new", "sc.inspect"])` became "the first two curated nodes are sc.new and sc.inspect".
  - `tests/manifest.rs` `the_vectorcraft_pack_names_its_app_and_a_demo`: `gui.is_none()` became "the gui label is SoundCraft", and `demo("soundcraft").is_none()` became `demo("nosuchcraft").is_none()`.

  Nothing else in either file changed. The genericity checks still hold and pass. In `manifest.rs`, the SoundCraft run tool is `execute`, with `command` as the id key. In `conformance.rs`, SoundCraft lists over 100 commands including `session.new`, runs `sc.new` → `sc.inspect` end to end (sample_rate 44100), and the session saves byte-stable.
- Reran: `pack_soundcraft` 3/3, `packs_all` 3/3, `conformance` 18/18, `manifest` 4/4, `client` 13/13; clippy clean.
