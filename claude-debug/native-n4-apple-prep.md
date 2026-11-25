# N4 Apple prep: the notarized Mac build and the TestFlight build, ready but not sent

*2026-11-21, branch `native/n4-apple-prep` from `feat/native-n0` at `57adab6`, uncommitted. Nothing was sent to Apple: no notarization, no upload, no `-allowProvisioningUpdates`, no certificate, identifier or profile made. Every signature made here was made without Apple's timestamp server.*

The maintainer's decisions: "Prepare everything, then ask again before each upload" and, on export compliance, "Check the binaries and report before setting it".

## Summary

Both apps can now be built for Apple with one command each, `native/apple/release.sh mac|ios|both`. The Mac app runs under the hardened runtime with no entitlements at all, signed with Developer ID, and passes the conformance suite (52 of 52) that way. It now carries its engine programs, and starts them as processes from inside its bundle. Both apps have a privacy manifest built from what the binaries actually call, a placeholder icon, and a version scheme with no per-build edits. The encryption question is answered with facts below; the flag is not set.

What only Apple or the maintainer can do: choose the store bundle ids, make an App Store provisioning profile and the App Store Connect records, answer the encryption question, replace or approve the icon, notarize the Mac DMG, and upload the iOS build. The checklist at the end lists them in order.

## What changed

| File | Change |
|---|---|
| `native/apple/Config/Mac.xcconfig` (new) | The Mac app's signing: ad hoc by default, hardened runtime on for every build, the empty entitlements file, no App Sandbox, no injected `get-task-allow` in Release; team from `PLUMCRAFT_TEAM_ID`; an optional untracked `Signing.local.xcconfig` |
| `native/apple/Config/Pad.xcconfig` (new) | The iOS app's signing defaults (ad hoc in the simulator), the team from the environment, the optional local file |
| `native/apple/Config/Version.xcconfig` (new) | `MARKETING_VERSION` and `CURRENT_PROJECT_VERSION` for local builds only |
| `native/apple/Config/Signing.local.xcconfig.example`, `.gitignore` (new) | How to sign Xcode builds without committing a team id |
| `native/apple/Config/ExportOptions-ios-app-store.plist`, `ExportOptions-ios-development.plist` (new) | The App Store export (destination `export`, never `upload`) and `--local`'s development export |
| `native/apple/PlumCraftMac/PlumCraft.entitlements` (new) | Empty, with the evidence in its comment |
| `native/apple/PlumCraftMac/project.yml` | Versions from the build settings, the xcconfig, the app icon, a Bundle engines phase (only when `PLUMCRAFT_ENGINES_DIR` is set), user-script sandboxing off for it; the signing lines moved to the xcconfig |
| `native/apple/PlumCraftPad/project.yml` | Versions from the build settings, the xcconfig, the app icon; the signing lines moved to the xcconfig |
| `native/apple/PlumCraftMac/App/PrivacyInfo.xcprivacy`, `native/apple/PlumCraftPad/App/PrivacyInfo.xcprivacy` (new) | The privacy manifests (the same file) |
| `native/apple/PlumCraftMac/App/Assets.xcassets`, `native/apple/PlumCraftPad/App/Assets.xcassets` (new) | AppIcon sets with the PLACEHOLDER icon |
| `native/apple/Icons/icon.swift`, `make-icons.sh` (new) | The icon generator: the placeholder, or one 1024 px picture |
| `native/apple/release.sh` (new) | The release builds for Apple; prints the upload commands, never runs them |
| `native/apple/PlumCraftFFI/Sources/PlumCraftShell/ShellCore.swift`, both `App/AppMain.swift` | `ShellConfig.bundleVersion`: the apps tell the core the bundle's version instead of a written-in "0.7.0" |
| `native/apple/PlumCraftMac/conformance.sh` | `APP=…` runs the suite against an app already built and signed |
| `native/apple/build-xcframework.sh` | Reads the libraries from `CARGO_TARGET_DIR` when set (B48) |
| `docs/research/24-native-shells.md`, `docs/STATUS.md`, `docs/BUGS.md` | Section 19's Apple prep paragraph and exit table; the status entry; B47 and B48 |

No Rust changed. No CI changed.

## 1. The hardened runtime on the Mac app

It is on for every Mac build (`ENABLE_HARDENED_RUNTIME = YES` in `Config/Mac.xcconfig`), with an empty entitlements file. Every build and test on this Mac now runs the way a notarized build will.

### What the app was tested doing under it

| Test | Build | Result |
|---|---|---|
| Conformance suite, `conformance.sh all` | Release, ad hoc, hardened | 52 of 52, 0 skipped |
| Conformance suite, `APP=… conformance.sh all` | The exported Developer ID build, universal, engines inside, hardened | 52 of 52, 0 skipped |
| Engines as processes and a graph run (`claude-debug/native-n4/s8-engines.py`) | The exported Developer ID build | `engine.switch` text to `process`, `engine.add` audio and array as processes: all connected (169 nodes); "Windows and Names" ran, 6 of 6 nodes done; the three children ran from `Contents/MacOS` with `flags=0x10000(runtime)`, and offered `data.shm` |
| Audio playback path (`s8-probe`) | A probe signed as the app is (Developer ID, runtime, the empty entitlements) | AVAudioEngine started on the default output and a silent buffer played back at volume 0. The app itself never opens the device in a scripted run (its viewers are muted then), so this is the app's code path in a probe, not the app |
| A child program built separately (`s8-probe`) | The probe | `posix_spawn` of the cargo-built `plumcraft-text-engine` (linker-signed, another signer): ran and exited |
| Shared memory (`s8-probe`) | The probe | A file mapped `MAP_SHARED` read/write: allowed. commonkit maps store files read-only through memmap2; nothing calls `shm_open` |
| Loopback control port | The app (every suite run) and the probe | Listened and served |
| File open and save panels | Not reached | A scripted launch uses `--no-open-panel` and saves through the core; the panels are AppKit's own and need no entitlement outside the sandbox. Not exercised |

### The entitlement table

Each exception the app might have needed, what it permits, and the evidence that the app does not need it.

| Entitlement | What it allows | Needed? | Evidence |
|---|---|---|---|
| `com.apple.security.cs.allow-jit` | `MAP_JIT` memory | No | The probe's `MAP_JIT` mapping is refused under the runtime ("Invalid argument") and allowed without it; the app imports no `pthread_jit_write_protect_np`, and its suite and engine runs pass without it |
| `com.apple.security.cs.allow-unsigned-executable-memory` | Writable and executable memory | No | Writable-executable memory is refused even without the runtime on Apple silicon; nothing in the app needs it (all tests above pass) |
| `com.apple.security.cs.disable-executable-page-protection` | Changing code pages | No | As above |
| `com.apple.security.cs.disable-library-validation` | Loading a library of another signer | No | The probe's `dlopen` of an ad hoc library fails under the runtime ("different Team IDs") and works without it. `otool -L` of the app and of each engine lists only `/System` and `/usr/lib` libraries. The app imports `dlopen`, but neither the Rust library nor the Swift sources call it (most likely the compiler runtime's OS-version check, which opens CoreFoundation, a system library) |
| `com.apple.security.cs.allow-dyld-environment-variables` | `DYLD_*` variables | No | The runtime removes them from the process's environment (the probe); nothing uses them |
| `com.apple.security.device.audio-input` | The microphone | No | The app records nothing; playback needs no entitlement (the probe) |
| `com.apple.security.automation.apple-events` | Sending Apple Events | No | Craft apps are started as programs and spoken to over stdio (MCP), not by Apple Events |
| `com.apple.security.get-task-allow` | A debugger attaching | Must be absent | Release sets `CODE_SIGN_INJECT_BASE_ENTITLEMENTS = NO`; `release.sh` refuses an app or engine that carries it; the exported app's entitlements are an empty dictionary |

### Signatures inside the bundle

The exported Mac app holds `PlumCraft` and four engine programs, all universal (x86_64 and arm64), each signed with Developer ID and the runtime flag, the app last. `codesign --verify --deep --strict` passes on the app and the DMG. There are no dylibs or frameworks in the bundle (the core is a static library).

`spctl -a -vv` rejects both, as expected without notarization: "rejected, source=Unnotarized Developer ID, origin=Developer ID Application: … (team)". Notarization also requires Apple's secure timestamp on every signature; the `--local` signatures here have none on purpose (asking for one contacts Apple's timestamp server). Without `--local`, `release.sh` signs with `--timestamp`.

Why `release.sh` signs the Mac app by hand rather than with `xcodebuild -exportArchive`: the export does not promise to re-sign programs in `Contents/MacOS` with the timestamp, and a nested program signed only in the archive (no timestamp) would fail notarization. Signing nested code first and then the app, as `packaging/macos.sh` already does for the egui editor, is deterministic. The developer-id export was therefore not run.

## 2. The App Sandbox on the Mac

The current design supports **Developer ID distribution with notarization**, outside the Mac App Store. Notarization does not need the sandbox.

The Mac App Store, and TestFlight for the Mac (which is the App Store's channel), both need the App Sandbox. Turning it on would break:

- **Engines and craft apps started from outside the bundle.** Engine manifests in `~/.config/plumcraft/engines`, the craft apps under `PLUMCRAFT_CRAFT_ROOT`, ffmpeg from Homebrew, and Python engines through `uv` are all outside the container. A sandboxed app can start only helpers it carries, which inherit its sandbox.
- **Install Engine….** It downloads and installs code, which App Store review does not allow (guideline 2.5.2).
- **The store on an external volume.** A sandboxed app reaches a folder outside its container only through a folder the person picks in an open panel (a security-scoped bookmark), not through `COMMONKIT_STORE_ROOT`.
- **Shared memory with engines.** Bundled engines that inherit the sandbox can share files in the container; engines outside the bundle cannot. POSIX shared memory would need an app group prefix.
- **The network.** The loopback control port and HTTP engines need `network.server` and `network.client`, which are allowed but must be declared.

A Mac App Store build is possible as the iPad's model: engines in process (or bundled helpers), the store in the container, no outside programs. That is a different product; the sandbox was not turned on.

## 3. Privacy manifests

`PrivacyInfo.xcprivacy` is in both apps' `App` folders and lands in both bundles: `PlumCraft.app/Contents/Resources/PrivacyInfo.xcprivacy` on the Mac and `PlumCraftPad.app/PrivacyInfo.xcprivacy` on iOS (checked in the archives). It declares no tracking, no tracking domains, and no collected data.

That last is true as far as the code shows. The apps talk only to: engines they start or are pointed at (loopback or addresses the person enters, plain HTTP through `ureq`); craft apps over stdio; their own control port (loopback, or in debug builds a pre-shared-token port on the local network); and `uv`, a separate program, when the person installs an engine on the Mac. Neither app has `URLSession`, analytics, crash reporting or advertising code. A UDP socket is bound only to ask the routing table for the device's own address (debug network port); nothing is sent.

### Required-reason APIs

Found by scanning the imported symbols of the built Mac app, its four engines, the iOS app and the core library (`claude-debug/native-n4/s8-symbols.sh`), and by searching the Rust and Swift sources.

| Category | Found | Where | Reasons declared |
|---|---|---|---|
| User defaults | `NSUserDefaults` | The Mac app (the launch arguments' domain, pop-out windows); the iPad app (a first-launch flag) | CA92.1: the app's own settings |
| File timestamps | `stat`, `fstat`, `fstatat`, `lstat` | The core and every engine (Rust's file metadata): the store, caches and autosave; the open document's modification time, to notice a change on disk | C617.1: files in the app's container; 3B52.1: files the person opened |
| Disk space | `statvfs`, `fstatvfs`, `fstatfs` | commonkit and the store: the store stops writing under its free-space floor; the Cache Report shows free space | E174.1: deciding whether to write; 85F4.1: shown to the person |
| System boot time | Not found | No `mach_absolute_time` or `systemUptime`; timing uses `clock_gettime` (Rust) and `CACurrentMediaTime`, which are not on the list | Not declared |
| Active keyboards | Not found | No `activeInputModes` | Not declared |

`sysctl` and `sysctlbyname` are imported (Rust's CPU-feature and core-count detection); they are not on Apple's list.

## 4. Export compliance (not legal advice)

**These are the facts found in the binaries and the dependency tree, and the usual reading of each. They are not legal advice. The maintainer answers the question.**

**Answered 2026-11-21:** card 1A, "Answer No: exempt (reading a document only)". `ITSAppUsesNonExemptEncryption: false` is now in both `project.yml` files' `info: properties:` (branch `native/decisions-1121`), and lands in both built Info.plists.

| What | Found | Pulled in by | Used for |
|---|---|---|---|
| TLS (rustls, ring, openssl, native-tls, Security's TLS) | None | `ureq` is built with `default-features = false`, so without TLS | Engines over plain HTTP. An `https://` engine address can't be attached in this build |
| OS encryption (CommonCrypto, Security, CryptoKit) | `CCRandomGenerateBytes` only | `getrandom` (uuid, zip) | Random numbers (control-port tokens, ids). The Swift package links Security but no Security symbol is imported |
| AES, RC4, MD5 (decryption) | `hayro_syntax::crypto` (AES-CBC, RC4, the PDF key derivation) | `hayro` 0.8, the array engine's PDF reader (`plumcraft-array`), in the core library on both platforms and in the Mac's array engine | Opening encrypted PDF documents (the standard PDF security handler) so they can be shown; nothing is encrypted for sending or storing |
| Hashing | BLAKE3, SHA-256 (`sha2`), MD5 (`md-5`) | commonkit's `digest` feature | Content addresses in the store. Hashing is not encryption |
| ChaCha, RSA, ECDSA, Ed25519, X25519, HMAC | None | | |

The usual reading, case by case:

- **An app that uses no encryption, or only the operating system's** (for example HTTPS through `URLSession`): exempt. Answer "No" (`ITSAppUsesNonExemptEncryption` false), no documentation.
- **Hashing and random numbers:** not encryption for this question.
- **Bundled, non-standard or own encryption used to protect data:** not exempt; App Store Connect asks for documentation (and, in some cases, an annual self-classification report).
- **Here, the one piece of bundled cryptography is `hayro`'s PDF decryption.** Encryption whose only role is to support a function that is not information security (here, reading a document) is commonly treated as exempt, the way PDF viewers and image libraries are. That reading would make the answer "No"; a cautious reading would answer "Yes" and claim the exemption with documentation. The maintainer, or the maintainer's adviser, decides.

The one-line change for each answer, in both `project.yml` files' `info: properties:`:

- **No (exempt):** `ITSAppUsesNonExemptEncryption: false`
- **Yes:** `ITSAppUsesNonExemptEncryption: true`, then, once App Store Connect has accepted the documentation and given a code, `ITSEncryptionExportComplianceCode: <code>`

Without either, App Store Connect asks the question for each build before it can go to testers. The flag is read only by App Store Connect; the Developer ID Mac build is not asked.

## 5. Icons

**Answered 2026-11-21:** card 2B, "Use the egui editor's plums, adapted". The Mac's set is `assets/app-icon/plumcraft.png` as it is; the iOS master is `native/apple/Icons/plums-ios-1024.png`, the largest wholly opaque centred square of that tile (710 px) scaled to 1024 px with no alpha (`full-bleed.swift`), and `make-plum-icons.sh` makes both sets. What follows describes the placeholder as it was.

Both apps now have an asset catalog with an AppIcon set: ten sizes from 16 to 1024 px on the Mac, one 1024 px opaque picture on iOS (Xcode makes the rest; it builds `AppIcon60x60@2x.png` and `AppIcon76x76@2x~ipad.png` and the `CFBundleIcons` keys). Archives of both build with them.

**The icon is a placeholder, which the maintainer must replace or approve:** a plain plum tile with two white node faces and a yellow wire, drawn by `native/apple/Icons/icon.swift` (Core Graphics, no other tool). To use the real icon, run `native/apple/Icons/make-icons.sh MASTER.png` with one 1024 px square picture, or `MAC_MASTER=… IOS_MASTER=…` for one each. The Mac's picture is used as given (draw the rounded tile and its clear surround into it); the iOS picture is put over plum with its transparency removed, since the App Store refuses an iOS icon with alpha.

The egui editor already has an icon, `assets/app-icon/plumcraft.png` (the cartoon plums, 1024 px, with a transparent surround). It would work for the Mac as is; for iOS it needs a full-bleed version, since the surround would show as plum.

## 6. Versions and build numbers

| Number | Source | How |
|---|---|---|
| Marketing version (`CFBundleShortVersionString`) | `Cargo.toml`, `[workspace.package] version` | `release.sh` reads it and passes `MARKETING_VERSION` |
| Build number (`CFBundleVersion`) | `release.sh --build N` | Default: the UTC time as `YYYYMMDDHHMM` (for example 202611211900), which only goes up, as App Store Connect requires; the same for both apps |

`Config/Version.xcconfig` (0.7.0, 1) only labels builds made in Xcode or by the test scripts; `release.sh` says when it lags `Cargo.toml`. Both apps now tell the core the bundle's version (`ShellConfig.bundleVersion`), so the About text follows the build. Checked: the Mac app and the iOS archive read back 0.7.0 and 202611211900.

## 7. Signing and export

`native/apple/release.sh` (the header lists every option):

- **`mac`:** builds the four engines for both Macs, archives the app universal with Developer ID (the Bundle engines phase puts the engines in), copies the app out, signs the engines and then the app (hardened, the empty entitlements, Apple's timestamp), checks each program for the runtime flag, both architectures and no `get-task-allow`, makes a DMG with an Applications link, signs it, and prints the notarization commands.
- **`ios`:** archives the iPad and iPhone app with automatic signing from the profiles already on the Mac (no `-allowProvisioningUpdates`), exports it for the App Store with `PLUMCRAFT_IOS_PROFILE` (manual signing, Apple Distribution, destination `export`), and prints the upload command.
- **`--local`:** the same without the network: no timestamps, and a development export of the iOS archive.

Team ids and identities come from `PLUMCRAFT_TEAM_ID` (and optional identity names); the identity is chosen by its certificate's team, not its display name. Nothing team-specific is in the repository.

### What was built and checked here

| Artifact | Built | Checked |
|---|---|---|
| Mac Release, ad hoc (`native/apple/PlumCraftMac/build`) | Yes | `flags=0x10002(adhoc,runtime)`, empty entitlements, icon, manifest; suite 52 of 52 |
| Mac Developer ID archive and app (`release.sh mac --local`) | Yes, universal | Five programs signed by the team's Developer ID with the runtime flag, no timestamp; `codesign --verify --deep --strict` passes; `spctl` "Unnotarized Developer ID"; suite 52 of 52; engines as processes |
| Mac DMG | Yes (about 85 MB; the app 152 MB) | Signed, verifies; `spctl -t open` "Unnotarized Developer ID" |
| iOS Release device build, unsigned | Yes | Clean build |
| iOS archive (`release.sh ios --local`) | Yes, signed Apple Development | 0.7.0 (202611211900), icon, manifest, no `NSLocalNetworkUsageDescription`; entitlements only the profile's (application id, team, `get-task-allow`, which every development signature has; an App Store export replaces it) |
| iOS development export (`.ipa`) | Yes | Exports with the profiles on this Mac |
| iOS App Store export | No | There is no App Store profile for the iOS bundle id on this Mac |

### Release builds and the debug network port

- **iOS:** the Release Info.plist has no `NSLocalNetworkUsageDescription` (it is a Debug-only key in `project.yml`); the Swift code that binds beyond loopback and the status element are inside `#if DEBUG`. The Release binary still contains the core's config messages about `network_debug` (the core's parser, never given it) and the word `--status-element` (the argument is parsed, and mutes audio, but nothing is created).
- **Mac:** a Release build refuses `--control-bind` and keeps the port on loopback (the code is `#if DEBUG`); its Info.plist has no local-network text.
- No entitlements file is added to the iOS app; its signed entitlements come from the profile only.

## Checklist for the maintainer

In order. "Needs Apple" means the step talks to Apple or happens on Apple's web pages.

| Step | What | Where |
|---|---|---|
| 1 | (Answered 2026-11-21: the apps are Plummer, `com.swapwizard.plummer.mac` and `com.swapwizard.plummer.pad`, set in `Config/BundleId.xcconfig`) Choose the store bundle ids: the iPad and iPhone app is `com.plumcraft.pad` today, the Mac app `com.plumcraft.mac` (the egui editor's DMG is `com.swapwizard.plumcraft`, also named PlumCraft.app) | Local (decision) |
| 2 | If the ids change, set `PRODUCT_BUNDLE_IDENTIFIER` in both `project.yml` | Local |
| 3 | (Answered: the egui editor's plums, `Icons/make-plum-icons.sh`) Replace or approve the placeholder icon: `native/apple/Icons/make-icons.sh MASTER.png` | Local |
| 4 | (Answered: No, exempt; the line is in) Answer the encryption question (section 4) and add the one line to both `project.yml` | Local (decision) |
| 5 | Register the App ID for the iOS bundle id | Needs Apple |
| 6 | Make an App Store provisioning profile for it with the Apple Distribution certificate (already in the keychain), download it, and set `PLUMCRAFT_IOS_PROFILE` to its name | Needs Apple |
| 7 | Create the App Store Connect app record (name, primary language, bundle id, SKU) | Needs Apple |
| 8 | Fill in the App Privacy answers: no data collected, no tracking (matching the manifests) | Needs Apple |
| 9 | Fill in TestFlight's test information: description, feedback email, privacy policy URL, and for external testers a review contact and "What to Test" | Needs Apple |
| 10 | Make an App Store Connect API key (team key) and keep the `.p8` in `~/.appstoreconnect/private_keys/` | Needs Apple |
| 11 | Store notarization credentials once: `xcrun notarytool store-credentials plumcraft --apple-id YOU --team-id TEAM` (or with the API key) | Needs Apple |
| 12 | Build: `PLUMCRAFT_TEAM_ID=TEAM PLUMCRAFT_IOS_PROFILE='NAME' native/apple/build-xcframework.sh && native/apple/release.sh both` (from a clean tree; signs with Apple's timestamp) | Local (the timestamps contact Apple's timestamp server) |
| 13 | Notarize and staple the Mac DMG (the commands `release.sh` prints): `xcrun notarytool submit dist/apple/PlumCraft-V-B-macos.dmg --keychain-profile plumcraft --wait`, `xcrun stapler staple …dmg`, `xcrun stapler validate …dmg`, `spctl -a -t open --context context:primary-signature -vv …dmg` | Needs Apple (ask first) |
| 14 | Upload the iOS build: `xcrun altool --upload-app -f dist/apple/PlumCraft-V-B-ios/PlumCraft.ipa -t ios --apiKey KEY_ID --apiIssuer ISSUER_ID` (or an upload export, which `release.sh` prints) | Needs Apple (ask first) |
| 15 | In App Store Connect: wait for processing, answer export compliance if not set, add the build to a group; external testers need Beta App Review | Needs Apple |
| 16 | Decide whether to also staple the app inside the DMG (notarize the app as a zip first) so a copied app opens offline the first time | Local (decision) |

## Decisions for the maintainer

1. **The store bundle ids** (iOS `com.plumcraft.pad`, Mac `com.plumcraft.mac`), and the name clash with the egui editor's `PlumCraft.app` (`com.swapwizard.plumcraft`) in `/Applications`.
2. **Export compliance:** "No" on the reading that `hayro`'s PDF decryption supports reading documents only, or "Yes" with documentation.
3. **The icon:** replace the placeholder, use the egui editor's plums (a full-bleed version for iOS), or approve the placeholder for testing.
4. **The Mac's channel:** Developer ID and notarization only (as built), or also a sandboxed Mac App Store edition on the iPad's model.
5. **Engines in the Mac app:** keep all four (text, array, audio, craft; the app is 152 MB universal), or fewer.
6. **Universal Mac build** (as built, both Macs) or Apple silicon only.
7. **Build numbers:** the UTC time (as built) or a counter.

## Gate

| Check | Result |
|---|---|
| Rust | Not changed; fmt, clippy and nextest not run |
| `swift test` (`native/apple/PlumCraftFFI`) | 81 tests in five runs, all passed, no warnings |
| Conformance, Mac Release (hardened, ad hoc) | 52 of 52 |
| Conformance, the exported Developer ID app (hardened, universal, engines inside) | 52 of 52 |
| Mac Release build | Succeeded; no compiler warnings (Xcode's own notice "Metadata extraction skipped, no AppIntents.framework dependency found", as before) |
| iOS Release device build (unsigned, not installed) | Succeeded; no compiler warnings (the same AppIntents notice) |
| Shell scripts | `shellcheck` clean on `release.sh`, `make-icons.sh` and `build-xcframework.sh` (`conformance.sh` has its earlier note about its trap handler) |

## The maintainer's store, caches, logs and preferences

Every scripted launch used `claude-debug/native-n4/s8-env.sh` (slice 6's scratch environment, refusing an unset store or one under `/Volumes/<store>`) and `--scratch-state`, never activated, quit by the PID it was given. Before and after (`s8-snapshot.sh`): the store's top level, `~/Library/Caches/com.plumcraft.mac`, `~/Library/Logs/PlumCraft` and `~/Library/Preferences/com.plumcraft.mac.plist` had the same modification times and entries, and no file under them was newer than the marker made before the first launch. No `PlumCraft` process other than the ones started here was signalled. Nothing in the main checkout, its `build-device` folders or `claude-debug/native-n4/s7-*` was touched; no simulator or device was used.

## Defects

- **B47 (fixed):** a Mac app built for distribution carried no engine programs.
- **B48 (fixed):** `build-xcframework.sh` ignored `CARGO_TARGET_DIR`.
- **Caught in my own work:** `release.sh`'s checks piped `codesign` into `grep -q`, which under `pipefail` failed the check falsely when the match closed the pipe; it now captures first. The identity check matched the team id in the certificate's display name, which for Apple Development certificates is not the team; it now reads the certificate's team. The first draft of the privacy manifest declared system boot time, which the binaries don't use; removed.

## Scratch

In `claude-debug/native-n4/` (untracked): `s8-env.sh`, `s8-run.sh`, `s8-snapshot.sh`, `s8-before.txt`, `s8-after.txt`, `s8-marker`, `s8-symbols.sh`, `s8-symbols-*.txt`, `s8-tree-ffi-*.txt`, `s8-probe.swift`, `s8-probe.sh`, `s8-probe.log`, `s8-probe-build/`, `s8-signtest/`, `s8-engines.py`, `s8-engines.log`, `s8-build-mac.sh`, `s8-final.sh` and the logs `s8-*.log`, `s8-final.out`, `s8-release-snapshot.sh`. The builds are in `dist/apple/` (untracked).
