# B151: About says which build is running

Branch `fix/b151-build-id` off the dev branch at `22f5884`.

## The reply

`app.about` (the command the version was already in) now also replies `build`:

```json
{"line": "Built 2026-11-24 22:07 UTC · shell 22f5884+changes",
 "shell": {"commit": "22f5884", "dirty": true, "date": "2026-11-24T22:05:00Z"},
 "app": null,
 "ui": {"commit": "22f5884", "dirty": true, "date": "2026-11-24T22:07:48Z"}}
```

- `shell` is this library's stamp.
- `app` is an application library's own stamp, when it set one.
- `ui` is the native program's own stamp, from the new config field `app.built`.
- `line` is what every About shows: `Built <date> UTC · shell <commit>[+changes] [· app <commit>[+changes]] [· ui <commit>[+changes]]`. `ui` is added only when the native program was built from another commit or state of changes than the library.

It is documented in `docs/shell-interface.md` 4.1 (with `app.built`), in its `app.about` sentence, and in `docs/control-protocol.md`'s About line.

## Choices

**One source.** The facts come from a new crate, `plumcraft-build-stamp`. Its build script runs git once at build time, and the core composes the line. All shells show `line` as given.

**Rebuild rules.** The build script runs again when any of these change:

- git's `HEAD`, the branch's ref (or its folder when the branch is packed), `packed-refs`, or the index;
- anything under `crates/`, `Cargo.toml` or `Cargo.lock`.

Git is run with `--no-optional-locks`. A plain `git status` rewrote the index on every build, which made every build stamp again; I found this with `rebuild.sh` and fixed it.

What gets rebuilt:

- With nothing changed: everything is Fresh, checked twice in a row.
- After an edit under `crates/`: only the stamp crate, `plumcraft-ffi` (and egui's app) are rebuilt.

Limits:

- An edit that git counts but which is outside those paths (docs, Swift) does not re-mark the library as dirty. The native program's own stamp covers Swift edits.
- Another tool refreshing the index (an IDE, a person's `git status`) makes the library stamp again. This is cheap.
- With no git, or outside a repository, everything reads "unknown" and the build still succeeds.
- A crate built from inside some other git repository would report that repository's commit.
- `SOURCE_DATE_EPOCH` is honoured.
- Nothing personal is stored: only the hash, a yes/no and a UTC time.

**Dates.** The line shows the newest of the stamps' dates. That is normally the app's, because the Xcode "Stamp build" phase runs on every build and is ordered after the Info.plist. Each stamp's own date is kept in the reply. So any two builds made at different minutes, from different commits, or one with changes and one without, read differently.

**The application.** A private application's crate must do two things:

1. Have a build script that calls `plumcraft_build_stamp::capture::emit(&manifest_dir, &["src", "Cargo.toml", "Cargo.lock"])` (or whatever paths it wants watched).
2. In its entry, before the first `plum_open`, call `plumcraft_ffi::app::set_build(plumcraft_build_stamp::stamp!())`.

It needs `plumcraft-build-stamp` both as a build-dependency and as a normal dependency, at the same pinned PlumCraft commit. `set_build` is a separate call rather than a new field on `App`, so the existing `App { packs, openers }` literals keep compiling.

## Changes by file

| File | Change | Lines |
|---|---|---|
| `crates/build-stamp/` (new: `build.rs`, `src/lib.rs`, `src/capture.rs`, `Cargo.toml`) | Capture, `Stamp`, `stamp!`, `line`, `about`; 6 unit tests | 355 |
| `crates/ffi/src/app.rs`, `core.rs`, `Cargo.toml` | `set_build`/`build`; `app.built` parsed (hostile: anything malformed reads unknown); `about.build` composed | +24 |
| `crates/session` | `About.build`; the `app.about` reply and its help text | +5 |
| egui: `crates/ui/src/about_view.rs`, `apps/plumcraft-editor` | The line, selectable, under the version (shell stamp only) | +12 |
| `crates/ffi/tests/build_stamp.rs` (new) | The reply's shape through the C ABI, the app stamp, hostile `app.built` | 48 |
| `docs/shell-interface.md`, `docs/control-protocol.md` | The reply, `app.built`, the rebuild rules and limits | +11 |
| `native/apple/stamp-build.sh` (new) | Writes `PlumBuildCommit`/`Dirty`/`Date` into the built Info.plist before signing | 31 |
| `PlumCraftMac/project.yml`, `PlumCraftPad/project.yml` | The "Stamp build" phase (input: the Info.plist); iPad script sandboxing off | +23 |
| `PlumCraftShell/ShellCore.swift` | `ShellConfig.built` (from the main bundle), `BuildStamp.json` and `line` | +30 |
| Mac `Settings.swift` | The line first in the About panel's credits (selectable) | +2 |
| iPad `Settings.swift` | The line (text selection on) and a Copy Build button; in the status file too | +15 |
| iPad `Script.swift` | An `about` launch step | +5 |
| iPad `Menus.swift` | About Plummer first in the hardware Help menu | +9 |
| `BuildStampTests.swift` (new) | The Info.plist stamp into the config, and the line from a real core | 50 |

## Tests and checks

| Check | Result |
|---|---|
| fmt, clippy `-D warnings`, schema, hash, core toolkit-free, private strings | pass |
| nextest, all features | 1706 of 1707. `params_coverage` fails on this machine's installed packs, as on B158 |
| Stamp crate | 6 of 6 |
| FFI `build_stamp` | pass |
| pytest | 249 passed |
| swift test (kit) | all pass, BuildStamps 2 of 2 |
| Mac conformance, scripted copy, all (no person check) | 75 of 75 |
| iPad conformance, simulator created and deleted here | 63 of 63 (12 skipped as before) |

## Launched and read back

| What | Read back | Picture |
|---|---|---|
| Mac scripted copy, `--then about` | Info.plist stamp `22f5884`, dirty yes, `22:07:48Z`; `app.about.build.line` "Built 2026-11-24 22:07 UTC · shell 22f5884+changes" | `launch-shots/mac-about-panel.png` (the panel by its window number); `mac-about.png` is `ui.screenshot`, which pictures the document window only |
| iPad simulator (created, deleted) | Status `about.build` and the reply give the same line; Info.plist stamped | `launch-shots/pad-about-screen.png` (`simctl io screenshot`) |
| iPhone 17 Pro simulator (created, deleted) | The same | `launch-shots/phone-about-screen.png` |

The control port has no way to open About. `command.run app.about` only replies: the Apple shells open About from their own menus, not from the core's `views.about`. So About was opened by the scripted launch step `--then about`, which the Mac already had and the iPad now has. If you want About to open from the port itself, the shells would need to follow a `view.windows.about`.

## Help menu

- **Mac:** About stays in the app menu (Plummer › About Plummer), where macOS puts it. Help keeps its search.
- **iPad with a keyboard:** the menu bar's Help now starts with "About Plummer". Before this, About was not in the iPad's menu bar at all.
- **iPhone:** About is under More › Help.

No new window.

## Defects in my own work

- The first stamp script phase had no input. In one simulator build it ran before the Info.plist was made, so the build had no stamp and the line fell back to the library's date. It is fixed by ordering the phase after the Info.plist, and the rebuild was checked stamped.
- The first build script made every build stamp again (the index rewrite). Found and fixed.
- The first Swift edit did not compile (`.object` was missing).
- On the Mac panel the line wraps after "shell".

## Not checked

- No person check, so nobody has copied the text by hand.
- The iPad Help menu's About item was compiled but not opened.
- No private application's stamp was tried; a fake one is set in the FFI test.
- Windows: not built.

## For Windows

The About window must:

- read `app.about`'s `build.line` and show it under the version as selectable text, with a Copy button;
- stamp its own build (commit, dirty, UTC date) at build time, for example from an MSBuild target into assembly metadata, and pass it as the config's `app.built: {commit, dirty, date}`.

It must not run git at run time.
