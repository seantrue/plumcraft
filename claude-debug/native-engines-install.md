# B71: installing engines and the engines folder in every shell

*2026-11-21, branch `fix/b71-native-engines` from `feat/native-n0` at `8820a35`, uncommitted.*

## What the maintainer hit

On first using the Mac app, Plummer, by hand: "Install engine is greyed out, no engines available. WTF?" They had been told twice that the Mac build could install an engine. It could not.

## The cause, checked

Each of the controller's readings held:

- `engine.install` was enabled only when `session.engines.can_install` was true, and only the egui shell set it (`EditorApp::with_installer`).
- The C ABI's core set `can_attach` but never `can_install`, and a test asserted the refusal (`attach_asks_with_the_cores_form_and_install_is_refused_here`).
- The C ABI's core never called `load_manifests`, so the native apps offered no engine from `~/.config/plumcraft/engines`, and it had no Reloader, so Reload Engine List only said "this editor has no engine manifests to read".
- The Mac app declares `subprocess: true` and `python: true`, so nothing about its platform forbade either.

One correction to the reading: the installer code (`install::install_engine`, `Installing`) and the manifest reader were already in the editor core since N1 slice 2. What lived in the egui application was the wiring (start-up reading, the Reloader closure, the Installer closure, `can_install`) and the per-frame `poll_install`. The C ABI's core had none of it.

## What moved into the core

| Part | Before | Now |
|---|---|---|
| Reading the manifests at start | `apps/plumcraft-editor` (`load_manifests(&manifests, manifest_folder())`) | `Launcher::load_manifests`, called by the egui app and by the C ABI's core thread |
| Reload Engine List | a closure in the egui app | `Launcher::reloader` |
| The engines folder | `~/.config/plumcraft/engines` only | `Launcher::folder` (a config's `dirs.engines`), else `$PLUMCRAFT_ENGINE_FOLDER`, else `~/.config/plumcraft/engines` |
| Whether to offer installing | the egui shell said yes; the C ABI never did | `install::refusal(caps)`: `subprocess` and `python`, with a reason a person reads |
| Giving a document all of it | `with_reloader`, `with_installer` in the egui app | `Document::offer_installs(&launcher)` |
| The installer | `install_engine(url, env)` | `install_engine(url, &Setup, &Progress)`: uv found for a Finder launch, `register --folder`, a process group per step, the search PATH, a 30-minute step limit, cancellation, progress |
| Following an install | the egui frame loop (`poll_install`) | `control::step`, every shell's turn |
| Other documents of a core | (no reload) | `Document::follow_manifests`: what one document read again, every document offers |
| Cancelling | none | `engine.install.cancel`, Engine › Cancel Install |

The egui shell keeps its Install window and the status bar spinner (drawing), and takes the core's wiring through `EditorApp::with_launcher`. `with_installer` and `with_reloader` stay for tests' stand-ins.

## What a person can do in Plummer now

1. Engine › Install Engine… is enabled. It raises the core's form (Repository: owner/repo or a GitHub URL; Install).
2. While it runs, the banner says it has begun, and the engine cell reads "Installing an engine…", what uv is doing on hover. Engine › Cancel Install stops it.
3. When it ends, the banner says "Installed and registered … The Engine menu offers it now." and the engine is in the Engine menu.
4. Engines already in `~/.config/plumcraft/engines` (installed with the egui editor) are offered from the start; Engine › Reload Engine List reads the folder again.
5. Picking the engine starts it as a process, and its nodes are in the palette.

What a person still cannot do: pick one of the public engines from a list in the form (the egui window offers three; the core's form has one field), or see the progress anywhere but the engine cell's hover and the banner (no progress bar).

## The contract's changes

| Where | What |
|---|---|
| `engine.install.cancel` | New command, Engine › Cancel Install: `{} -> {cancelling: url}` |
| `engine.list` | Adds `installRefused`, `installStatus` (`canInstall`, `installing` were there) |
| `ui.inspect`'s `engine` | Adds `installStatus`, `canInstall`, `installRefused` |
| `changed`, domain `document` | Adds `installing: {repo, status}` or null |
| C ABI config | `dirs.engines`, `engines.manifests` (at most 64 files), `engines.uv` |
| Environment | `PLUMCRAFT_ENGINE_FOLDER` names another engines folder (wins over `dirs.engines`) |
| `engine.install` | Disabled with the platform's reason where `subprocess` or `python` is false; it now runs `register --folder` |
| Messages | `lastError` at an install's start and end: "Installing …", "Installed and registered …", "The install of … was cancelled", "Installing … failed: …" |
| Conformance | Two cases; a new driver capability, `manifests`; the Swift runner's `--has` |
| Swift models | `Changed.installing`, `DocumentState.installing`, `InstallState` |

The shell schema (`crates/editor-core/schema/shell1.json`) was regenerated and did not change: these are command results and feed fields the schema doesn't type.

## The hardened runtime and where uv comes from

- **The test:** the Release Plummer from this branch, copied to scratch, signed ad hoc with the hardened runtime and no entitlements (`codesign --sign - --options runtime`; `flags=0x10002(adhoc,runtime)`, no entitlements), started with a bare environment as the Finder starts an app: `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, no `TERM`, `HOME` the person's.
- **Everything it wrote was scratch:** the engines folder (`PLUMCRAFT_ENGINE_FOLDER`), uv's tool folder, bin folder, cache and Python folders (`UV_TOOL_DIR`, `UV_TOOL_BIN_DIR`, `UV_CACHE_DIR`, `UV_PYTHON_INSTALL_DIR`, `UV_PYTHON_BIN_DIR`), the store, preferences, marks and craft root, and `--scratch-state`.
- **uv:** not on that PATH. The core found it as manifests' programs are found: the app has no `TERM`, so it asks the login shell for its PATH once, then looks in the usual folders, one of which, `~/.local/bin`, holds uv (where uv's own installer puts it); which of the two found it was not recorded. Without uv anywhere the install fails at once: "Installing an engine needs uv (https://docs.astral.sh/uv), which wasn't found: not on PATH, the login shell's PATH, ~/bin, ~/.local/bin, ~/.cargo/bin, /opt/homebrew/bin or /usr/local/bin. Install it, or set PLUMCRAFT_UV to it". Install Engine… stays enabled then (the reason is given when it is tried); see the decisions.
- **The engine process:** uv chose a Homebrew Python 3.14 for the tool. The engine runs as its own process of that Python, not loaded into the app, so the app's library validation does not reach it, and nothing needed an entitlement.

## The real install, once

`seantrue/plumcraft-duckdb-engine` (public; its README asks only for uv), through the running app's control port (`engine.install`, then `ui.inspect` once a second):

| Time | What the core said |
|---|---|
| 0.6 s | `engine.list`: `canInstall` true; `engine.install` replied `{installing: …}` |
| 0.7 s | "Installing seantrue/plumcraft-duckdb-engine: uv downloads and builds it, which can take a few minutes (Engine › Cancel Install stops it)" |
| 3.7 s | "… uv is downloading and building it (Updating https://github.com/seantrue/plumcraft (34f6aea…))" |
| 5.7 s | "… (Built plumcraft-engine @ git+https://github.com/seantrue/plumcraft@34f6aea…#subdirectory=python)" |
| 6.7 s | "Installing seantrue/plumcraft-duckdb-engine: registering duckdb-engine" |
| 7.7 s | "Installed and registered duckdb-engine from https://github.com/seantrue/plumcraft-duckdb-engine. The Engine menu offers it now." |
| 8.2 s | `engine.use duckdb`: added as a process; "engine: Text + DuckDB 0.7.0 · 2 engines · connected · 54 nodes" |
| 8.2 s | a SQL Query node, `SELECT 6 * 7 AS answer, 'plum' AS fruit`, watched `value`: done in 25 ms, the value 42 |

The manifest `register` wrote (`duckdb.toml` in the scratch folder) runs the scratch tool's Python with `-m plumcraft_duckdb`, and its demo is the wheel's. The app's child was that Python. The app quit on TERM, by its PID, with status 0.

Failures on the way: none in the install. A first try of the form check sent `ui.invoke {by}`, which the port refuses ("unknown field"); without it the core raised the form (Install Engine, one field, `repo`) and it was cancelled. The app shows no sheet for a form a script raised (`by: script`), so the sheet a person's menu pick shows was not seen here.

## The iPad and the Windows app

- **The iPad** declares neither `subprocess` nor `python`. Install Engine… is listed, disabled by the core with "This device can't install engines: it can't start programs", and invoking it raises nothing. An engine already running elsewhere that a manifest names (an attach) is still offered. No Swift change was needed there. Tested through the C ABI with the iPad's platform; the iPad app itself was not run.
- **The Windows app** declares `subprocess: true, python: false` (`native/windows/src/PlumCraft.Core/ShellConfig.cs`), so its Install Engine… is disabled with "This device can't install engines: it has no Python", and it reads the same folder (`%USERPROFILE%\.config\plumcraft\engines`). To install, it must:
  1. declare `python: true` where uv is found;
  2. show the core's `form` ask for `engine.install` (as for `engine.attach`);
  3. show `lastError` (the start and end messages);
  4. optionally, show the `document` domain's `installing: {repo, status}` in its engine cell.
  The core runs `<uv bin>\duckdb-engine.exe register` (the `.exe` is found by `with_extensions`). Not run on Windows.

## Names

The folder stays `~/.config/plumcraft/engines` for every editor, so engines the maintainer installed with the egui editor (`duckdb.toml`, `opencv.toml`) appear in Plummer without reinstalling, and the reverse. What is keyed on the app's name, and so not shared: the preferences (`Application Support/Plummer/prefs.json` for the Mac app; `plumcraft-editor`'s for egui), with their kept settings and the egui editor's trusted workflows (`trust_path_for(program)`), the Mac app's defaults domain, saved state, caches and logs. Shared besides the engines folder: the control token's default file (`~/.config/plumcraft/control.token`) and uv's own tool folders. Not run: Plummer reading the person's real folder (the rules forbade touching it); the code path is the same as the scratch one with no folder named.

## Tests

| Suite | Result |
|---|---|
| `cargo nextest run --all-features --no-fail-fast` (scratch env) | 1,577 of 1,578: `plumcraft-craft::params_coverage hidden_params_match_the_baseline`, the known local failure; 8 tests more than the baseline 1,570 |
| New core tests (`install`) | a stand-in uv: install and register into the folder given, uv's failure, cancel within 5 s, a step past its limit, a uv named that isn't there, the platform's refusal, uv's output read |
| New C ABI tests (`engines_install.rs`) | a desktop platform's manifests offered at start, the core's form, an install, a second document offered it, a cancelled install; the iPad's platform refused with the reason; bad config fields refused |
| Replaced C ABI test | `attach_asks_with_the_cores_form_and_install_says_why_this_platform_cannot` |
| egui (`kittest_install`, conformance) | pass, through the core's install loop |
| `cargo clippy --all-targets --all-features -D warnings`, `cargo fmt --check` | clean |
| `scripts/check-hash-gates.sh`, `scripts/check-core-toolkit-free.sh` | ok |
| Python (`uv run pytest -q`, `ruff check`) | 224 passed; clean |
| xcframework (this worktree, every platform) | built |
| `swift test` (`native/apple/PlumCraftFFI`) | 81 tests in 27 suites, all passed; the two new cases pass on the kit and its port |
| `native/apple/PlumCraftMac/conformance.sh all` | 55 of 55, 0 skipped; the person's Library unchanged |
| Mac Release, iOS Release device SDK (not signed, not installed) | both built, no warnings |

## Before and after

| What | Result |
|---|---|
| `~/.config/plumcraft/engines` (names, sizes, times, hashes) | unchanged |
| The real store root (top level) | unchanged |
| `~/Library` under both app names (Application Support, Caches, Logs) and uv's real tool and bin folders | unchanged |
| `library-check.sh` for the Mac app's id, before the work and after the last launch | unchanged |

## Defects caught on the way

Both found by reading, before any run failed:

- The first draft of the C ABI test waited for a `changed` entry that the poll inside a reply's wait could already have consumed (and thrown away); the test client now keeps every `changed` it polls.
- The registering step ran `<bin>/<name>`, which on Windows is `<name>.exe`; it now finds the file with the platform's extensions (not run on Windows).

## Unverified

- A Developer ID, notarized Plummer (only an ad hoc hardened build was run).
- A Mac with no Python at all (uv would download one into its own folder), and a Mac with no uv (the message is unit-tested with a uv named that isn't there, not with the search failing).
- The sheet a person's menu pick shows in Plummer (only the form raised by a script, which the app doesn't present).
- The iPad app and the iPad conformance script (changed to pass the suite's engines folder and `--has manifests`, not run); the Windows app.
- Plummer reading the person's real engines folder.

## Decisions for the maintainer

1. With no uv found, keep Install Engine… enabled and fail at once with where it looked (as built), or disable it with that reason (checked when the menus are built).
2. Should the core's Install form offer the three public engines as choices, as the egui window does (a new field kind, a pick list), or stay one line?
3. Should `--scratch-state` launches of the Mac app refuse to start without `PLUMCRAFT_ENGINE_FOLDER`, as they refuse without a scratch store (the agent editing `AppMain.swift`'s scratch-state guard could add it)?
4. Should the Windows app declare `python: true` when it finds uv, so it installs as the Mac app now does?
