# Moving the Windows shell's work to the x64 Windows test VM (2026-11-24)

The ARM64 Windows test VM on the Mac became unusable (its disk's drive hung). The work moved to an x64 Windows 11 Pro guest on the Linux test host. This note is what the move needed, for the next time.

## The machine

- **The guest:** Windows 11 Pro, x86_64 (AMD64), 8 vCPU, 8 GiB, no GPU. Direct3D renders through WARP (software), so no timing taken there is evidence of smoothness.
- **Reached from the Mac** by two ssh hops: the Mac, then the Linux host, then the guest. The guest's key lives only on the Linux host. The guest's shell is Windows PowerShell.
- **Already there:** rustc with `x86_64-pc-windows-msvc`, VS 2022 Build Tools (MSVC and a Windows SDK), git, and internet access. Nobody is signed in at the desktop unless someone logs in over RDP or the console.
- **Not there:** `dotnet`.

## What was set up (per user, nothing machine-wide)

- **The .NET SDK 10.0.404**, by Microsoft's `dotnet-install.ps1`, into a tools folder of the user's, which was added to the user's PATH. `DOTNET_CLI_TELEMETRY_OPTOUT=1` is set for the user.
- **A checkout of its own**, a fresh clone of the public repository, beside (never in) the maintainer's other checkouts. Uncommitted work goes to it as a tarball of what differs from the base commit, over the two hops.
- **NuGet packages** are restored into the user's profile.
- **Symlinks.** The repository has 45 symbolic links (each crate's `digest.rs`, its licence files, the shared fonts). Git for Windows checks them out as text files holding their targets unless `core.symlinks` is true. Then `include!("digest.rs")` fails with "expected item, found `..`". In the clone: `git config core.symlinks true`, delete those 45 files, and check them out again. Creating symlinks needs Developer Mode or an administrator's session; the ssh session here is an administrator's, so it worked without changing any setting.

- **A per-user .NET and apphosts.** A program's own `.exe` (an apphost, such as the C# runner's `plumcraft-conformance.exe`) looks for the runtime only machine-wide or in `DOTNET_ROOT`. With the SDK in a user folder it fails at once with exit `0x80008083`. So the scripts start it as `dotnet <its .dll>`. Setting `DOTNET_ROOT` for the user would also do. The app itself is self-contained, so it is unaffected.
- **The desktop session** must be connected, not only signed in. A disconnected RDP session (`query user` says `Disc`) still lets the app start, but nothing is drawn and input goes nowhere, so the person check and screenshots need the session connected.

## Scripts (scratch, under `claude-debug/b154-win/`, untracked)

- **`w.sh`** runs PowerShell on the guest through the hop. It sends the text as `-EncodedCommand`, so it needs no quoting, and reloads PATH from the registry first. It also copies files both ways. The hop host and the guest's alias come from `WIN_HOP` and `WIN_HOST`.
- **`w-sync.sh BASE`** brings the guest's checkout to the worktree's content.
- **`setup.ps1`** makes the clone and installs the SDK. It can be run again safely.
- **`w-build.ps1`** builds the core and the engine programs (`--target x86_64-pc-windows-msvc`), then the app (`-p:Platform=x64`).
- **`w-test.ps1`** runs `PlumCraft.Kit.Tests` with every root under the scratch folder.
- **`w-conform.ps1`** runs the C# runner against the app. The app is started in the SSH session (session 0) unless `-Interactive`.

## What the build assumed of ARM64

- **`native/windows/Directory.Build.props`** found the core DLL only under `target\aarch64-pc-windows-msvc`. It now picks cargo's target for the machine's architecture (`PlumRustTarget`: `aarch64-pc-windows-msvc` on Arm64, else `x86_64-pc-windows-msvc`; `-p:PlumRustTarget=` for another).
- **The app project** already chose `win-x64` and the x64 core for `-p:Platform=x64`.
- **`PlumCraft.Probe`** (a spike, built by no test) still names `win-arm64`. It is left as it is.
- **The old scripts** (`claude-debug/native-n5/r4-*`) hard-wire the ARM64 VM's address, user folder and checkout. The `w-*` scripts take them from the environment or parameters instead.

## What a full person check needs there

GUI programs started over ssh land in session 0, where their windows are invisible. A check that drives the app as a person does needs a signed-in desktop session on the guest; then a one-shot Interactive scheduled task starts the app there. That needs someone to sign in once, over RDP or the console. The account's password is the maintainer's to use.
