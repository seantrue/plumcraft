# Releases and install images

Scripts that build PlumCraft's install images. Each image holds the editor, the `plumcraft`
command line and the text, array, audio and craft engines. It is the way to get the audio engine
without a checkout, as `plumcraft-audio` isn't on crates.io.

| Script | Runs on | Makes |
|---|---|---|
| `macos.sh` | macOS | `PlumCraft-VERSION-macos-ARCH.dmg` |
| `linux.sh` | Linux | `plumcraft-VERSION-linux-ARCH.tar.gz`, `.deb` |
| `release.sh` | either | The gate, this machine's images, `SHA256SUMS`, and a GitHub release if asked |

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

## Making a release

Bump `version` under `[workspace.package]` and merge it, then, from a clean `main`:

```sh
packaging/release.sh --linux-host HOST --upload
```

Without `--upload`, nothing leaves the machine. With it, the script pushes the tag `app-vVERSION`
and makes a draft GitHub release to read over. `--publish` skips the draft. `--linux-host` needs
a Rust toolchain and the Linux packages above on that host.

Crates and the Python SDK release separately. `vVERSION` tags are the SDK's.
