# PlumCraft on Windows (stage N5): the spike

The native Windows shell, C# and WinUI 3, on the same editor core as the Apple apps, through the C ABI in `crates/ffi` (`crates/ffi/include/plumcraft.h`, `PLUM_ABI_VERSION` 1). This folder holds stage N5's spike (2026-11-21). What it found is in `docs/research/24-native-shells.md` section 20 and the report `claude-debug/native-n5-spike.md`.

The maintainer, 2026-11-21: "Let's swap a windows implementation for the Kotlin implementation." The toolkit is "C#, WinUI 3". egui stays the Windows editor until this shell passes the conformance suite.

## What is here

| Path | What | Spike or kept |
|---|---|---|
| `src/PlumCraft.Core/Native.cs` | The C ABI as P/Invoke (`LibraryImport`), struct for struct. The only file with pointers | Kept: the kit's lowest layer |
| `src/PlumCraft.Core/PlumCore.cs` | A core: the wake as an `[UnmanagedCallersOnly]` function that only sets an event, one pump thread that polls and completes requests, the feed, borrowed blobs (`Lease`), the hot calls | Kept, to grow into the kit as the Swift `PlumCore` did |
| `src/PlumCraft.Core/ShellConfig.cs` | The core's config for Windows and the environment set before it opens (the store and every root under one folder) | Kept, to grow |
| `src/PlumCraft.Probe/` | A console program: N0's flow (open, list, build, run, node status, a 64 MB blob borrowed) and the boundary's timings | Spike; becomes the kit's tests |
| `src/PlumCraft.CanvasSpike/` | A WinUI 3 window that follows the core's scene by delta, lays faces out with DirectWrite (Win2D `CanvasTextLayout`), reports them (`plum_geometry_set`), and draws them three ways (Win2D immediate mode, Composition visuals, plain XAML); a scripted bench; input as `ui.gesture` | Spike. `SceneModel.cs` and `FaceLayout.cs` are first drafts of the kit's model and face layout; the Composition renderer is the one recommended |

Nothing here implements a user action: input goes to the core as `ui.gesture` (and a drag's moves as `plum_gesture_move`), and the core decides what it is. Pan and zoom are the view's, as on the Apple shells (contract 12.13).

## Building (on the Windows 11 ARM64 test machine)

1. The core as a DLL, release, no debug info:

   ```powershell
   git config core.symlinks true   # before checkout; the repository has 45 symlinks (digest.rs, the Inter font, licences)
   $env:CARGO_PROFILE_RELEASE_DEBUG = '0'
   cargo build -p plumcraft-ffi --release --target aarch64-pc-windows-msvc
   ```

   This gives `target\aarch64-pc-windows-msvc\release\plumcraft_ffi.dll` (about 26 MB) and a 168 MB static library nobody here uses.

2. The probe and the window, with the .NET 10 SDK (no Visual Studio workload, no MSIX tooling):

   ```powershell
   dotnet build src\PlumCraft.Probe -c Release -p:PlumFfiDir=<repo>\target\aarch64-pc-windows-msvc\release
   dotnet build src\PlumCraft.CanvasSpike -c Release -p:Platform=ARM64 -p:PlumFfiDir=<repo>\target\aarch64-pc-windows-msvc\release
   ```

   NuGet brings the Windows App SDK 2.6.5 and Win2D 1.4.0. The window is unpackaged and carries the Windows App SDK with it (`WindowsPackageType None`, `WindowsAppSDKSelfContained`): its folder (about 280 MB) runs as it is.

## Running

- `PlumCraft.Probe.exe <scratch folder> [out.json]`: prints a line a check and exits 0 when all pass.
- `PlumCraft.CanvasSpike.exe --root <folder> --mode win2d|composition|xaml --nodes 500 [--bench out.json --exit] [--control-port 0 --token-file F --status F] [--view zoom,x,y] [--input-log F]`.

A window started from an SSH session lands in session 0, where nobody sees it. The scripts in `claude-debug/native-n5/` (not tracked, except their Markdown) start it on the signed-in desktop through a one-shot Interactive scheduled task, screenshot that desktop the same way, inject mouse, touch and pen input there, and run the conformance suite from the Mac through an SSH-forwarded control port.

## Not built

The kit proper (observable models, menus from `menu.get`, the asks, viewers, settings, documents and windows), packaging (MSIX or an installer), a UI-test runner, and any CI: CI stays Linux only. Section 20 of note 24 has the slice plan.
