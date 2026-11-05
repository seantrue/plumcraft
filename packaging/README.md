# Releases and install images

Scripts that build PlumCraft's install images. Each image holds the editor, the `plumcraft`
command line and the text, array, audio and craft engines. It is the way to get the audio engine
without a checkout, as `plumcraft-audio` isn't on crates.io.

| Script | Runs on | Makes |
|---|---|---|
| `macos.sh` | macOS | `PlumCraft-VERSION-macos-ARCH.dmg` |
| `linux.sh` | Linux | `plumcraft-VERSION-linux-ARCH.tar.gz`, `.deb` |
| `windows.sh` | Windows, under Git for Windows' bash | `PlumCraft-VERSION-windows-x86_64-setup.exe`, `plumcraft-VERSION-windows-x86_64.zip` |
| `release.sh` | macOS or Linux | The gate, this machine's images, a Linux and a Windows host's if named, `SHA256SUMS`, and a GitHub release if asked |

All of them write to `dist/`. Each script's header lists its options.

## macOS

`macos.sh` signs with the keychain's first Developer ID Application identity, or ad hoc without
one. To notarize, store credentials once and name the profile:

```sh
xcrun notarytool store-credentials plumcraft --apple-id YOU --team-id TEAM
NOTARY_PROFILE=plumcraft packaging/macos.sh
```

An image that isn't notarized makes Gatekeeper warn whoever downloads it.

## Linux

Install a Rust toolchain, `pkg-config` and ALSA's headers (Debian: `libasound2-dev`). The `.deb`
needs `dpkg-deb`, and is skipped with a warning without it.

## Windows

Install the MSVC Rust toolchain, uv and Inno Setup 6 (`winget install JRSoftware.InnoSetup`), and
clone with real links (`git clone -c core.symlinks=true …`, `docs/guide.md`). Then, in Git for
Windows' bash, `packaging/windows.sh`. Without Inno Setup it makes the zip only, with a warning.

The installer (`plumcraft.iss`) installs for the user by default, with no administrator needed,
or for everyone if chosen. It adds a Start menu entry, and optionally a desktop icon and the
folder on PATH for the `plumcraft` command line. The uninstaller removes all three. Neither the
installer nor the programs are signed yet, so SmartScreen warns whoever downloads them.

A release build of the editor is a windowed app, so it opens no console. Its output goes nowhere,
so `plumcraft-editor run FILE` prints nothing on Windows; use the `plumcraft` command line.

## Licences

Each image carries `THIRD-PARTY.txt`, made by `third_party.py`, with the licences of every crate in
the binaries and of what those crates bundle. Some crates keep their licence at their repository's
root, so `collect_licences.py` fetches those into `licences/`, which is tracked so a release build
stays offline. Run it when dependencies change:

```sh
python3 packaging/collect_licences.py
```

It fails if a crate has no licence text and isn't on its `NONE_PUBLISHED` list.

## Making a release

Bump `version` under `[workspace.package]` and merge it, then, from a clean `main`:

```sh
packaging/release.sh --linux-host HOST --windows-host HOST [--windows-jump JUMP] --upload
```

Without `--upload`, nothing leaves the machine. With it, the script pushes the tag `app-vVERSION`
and makes a draft GitHub release to read over. `--publish` skips the draft. `--linux-host` needs
a Rust toolchain and the Linux packages above on that host, and `--windows-host` what Windows
needs above, with OpenSSH. `--windows-jump` reaches a Windows VM through the machine that hosts
it: for yellowfin's, `--windows-host win11-dev --windows-jump yellowfin.lan`.

Crates and the Python SDK release separately. `vVERSION` tags are the SDK's.
