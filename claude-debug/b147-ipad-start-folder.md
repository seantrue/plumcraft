# B147: a new graph is made in the app's own folder (iPad, iPhone)

*2026-11-23, branch `fix/b147-ipad-start-folder` from `origin/feat/native-n0`, brought up to `2d0d106`. Uncommitted. No device.*

## Answer first

**Cause, as far as the simulator shows it.** I reproduced from a fresh install each time (the app uninstalled, so its container is new).

- The first launch opens Showcase from the app's own folder.
- The document browser it goes back to also opens in that folder, Plummer (Samples and New Graph).
- But the browser's **New Graph made the graph wherever the browser stood.** With the browser in On My iPad, reached from its sidebar, the dev branch's app made `Untitled.json` in On My iPad's own storage, outside the app's container (`…/Shared/AppGroup/…/File Provider Storage/Untitled.json`), and opened it there in place.
- A failure to make a graph reached only the log (`failedToImportDocumentAt` → `CoreHost.say`), never the person.
- On the simulator the save to that outside place worked. So the "failed save" itself is the device's and **was not reproduced**. Likely device causes:
  - a place the app may not write beside its file (On My iPad's top folder, another provider);
  - a start folder remembered from an earlier install's container, since a container's path changes between installs.
- It was not B137's "relative to /" fault: the paths seen are absolute.

**Fix,** as the maintainer suggested ("Files / Plummer seems a better starting place"):

- New Graph always makes the graph in the app's own folder, `Documents`, which the Files app shows as Plummer ("Untitled.json", then "Untitled 2.json" and on), wherever the browser stands. It opens it there, and the browser shows it.
- The browser shows the app's folder when the app starts.
- A graph that can't be made, and a save that fails, tell the person **why and where**: "Couldn't make a new graph — In Plummer (this app's folder in Files): …"; "Couldn't save Untitled in Plummer: …, Plummer keeps trying".
- A file ask's remembered start folder that no longer exists falls back to the app's folder.
- **The command rule:** New Graph is still the system document browser's own button, answered by the shell, as it was before. No core command was added or bypassed.

## The named test

`testANewGraphIsMadeAndSavedInTheAppsFolder` (`PlumCraftPad/UITests/FirstLaunchTests.swift`) is run from a fresh install by `claude-debug/n4-decided-2/fresh.sh`. It walks:

1. the first launch;
2. back to the browser;
3. On My iPad, from the sidebar;
4. New Graph;
5. a node added from the palette;
6. the saved file in the app's `Documents`, which holds the node.

On the dev branch's app it **failed**: the graph was made in On My iPad's storage. With the fix it **passes**. The pictures are in `claude-debug/n4-decided-2/shots-b147before/` and `shots-b147after/`.

## The Mac and Windows

These were read from the code, not run:

- **Mac:** the open and save panels start in the core's `startIn` without checking that it exists. A missing folder makes the panel fall back to its own default, and the person picks any place they can write; a save that fails there is the core's to say. That is not the same fault, but adding the existence check would be a small change; listed, not made.
- **Windows:** the path dialogs check that the start folder exists (`PathBrowse.StartFolder`). No like fault seen.

## Gate

The final round on the merged branch (`2d0d106`), `claude-debug/n4-decided-2/b147final.sh`. No Rust changed, so there is no Rust or Mac gate.

| Check | Result |
|---|---|
| xcframework, swift test | built; passed |
| iPad standard gate | 60 of 60; UI tests 11 passed, 5 skipped (the named test among them, since that run isn't a fresh install), 0 failed |
| Named test, iPad, fresh install | passed (25 s): New Graph from On My iPad made `…/Documents/Untitled.json`, and the saved file holds the node |
| Named test, iPhone 17 Pro, fresh install, one step larger | passed (25 s): from On My iPhone, the same |

The named test runs only from a fresh install: it skips unless `fresh.sh` says so (`PLUMCRAFT_FRESH_INSTALL=1`).

## Not checked

- A device, so the failed save itself.
- iCloud Drive and third-party providers; the simulator has neither.
