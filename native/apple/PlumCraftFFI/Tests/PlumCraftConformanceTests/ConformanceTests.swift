// The conformance suite from Swift (docs/shell-interface.md 16.1), on a real core: the read-only
// half (N3's gate) through the kit and over the core's control port, the array engine's case on a
// core opened on it, the whole suite through the kit as well, and the runner shown not vacuous.

import Foundation
import PlumCraft
import PlumCraftConformance
import PlumCraftKit
import PlumCraftModels
import Testing

let repo: URL = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

/// The shared cases, read where they are.
let casesURL = repo.appendingPathComponent("crates/editor-core/fixtures/conformance/cases.json")

func scratch(_ name: String) -> String {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("plumcraft-test-\(ProcessInfo.processInfo.processIdentifier)")
        .appendingPathComponent("swift-\(name)")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.path
}

let testEnvironment: Void = {
    setenv("PLUMCRAFT_EDITOR_PREFS", scratch("prefs") + "/prefs.json", 1)
    // never the maintainer's store, whatever the environment names (N3 slice 4)
    setenv("COMMONKIT_STORE_ROOT", scratch("store"), 1)
    setenv("COMMONKIT_MARK_DIR", scratch("marks"), 1)
    setenv("COMMONKIT_TEST_RUN", scratch("run"), 1)
    setenv("PLUMCRAFT_CRAFT_ROOT", scratch("craft"), 1)
    setenv("COMMONKIT_HASH", "b3", 0)
    setenv("COMMONKIT_STORE_MIN_FREE_GB", "2", 0)
}()

/// The suite's engines folder (B71): every core here reads it (`dirs.engines`), so the drivers
/// have `manifests`, and none reads the person's own.
let suiteEngines = repo.appendingPathComponent("crates/editor-core/fixtures/conformance/engines").path

func openCore(_ name: String, _ extra: [String: JSONValue] = [:]) throws -> PlumCore {
    _ = testEnvironment
    var config: [String: JSONValue] = ["store_root": .string(scratch("store-\(name)")), "dirs": ["engines": .string(suiteEngines)]]
    for (k, v) in extra { config[k] = v }
    return try PlumCore(config: .object(config))
}

/// The engines folder of a driver with `copied_manifest`, as `conformance::copied_manifest_folder`
/// makes it (`root/engines`: the suite's probe.toml, copied.toml away from its project, and a
/// stand-in `uv` that fails as Python does there).
func copiedManifestFolder(_ root: String) throws -> String {
    let folder = root + "/engines"
    let fm = FileManager.default
    try fm.createDirectory(atPath: folder, withIntermediateDirectories: true)
    try? fm.removeItem(atPath: folder + "/probe.toml")
    try fm.copyItem(atPath: suiteEngines + "/probe.toml", toPath: folder + "/probe.toml")
    try "id = \"copied\"\nname = \"Copied Engine\"\nmode = \"process\"\ncommand = [\"./uv\", \"run\", \"--project\", \".\", \"python\", \"-m\", \"copied_engine\"]\ncwd = \".\"\n"
        .write(toFile: folder + "/copied.toml", atomically: true, encoding: .utf8)
    try "#!/bin/sh\necho 'No module named copied_engine' >&2\nexit 1\n".write(toFile: folder + "/uv", atomically: true, encoding: .utf8)
    try fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: folder + "/uv")
    return folder
}

/// The outcomes as one line each, printed for the report.
func log(_ driver: String, _ out: [Outcome]) {
    print(Conformance.summary(driver, out))
    for o in out { print("CASE \(driver) | \(o.passed ? "pass" : o.skipped != nil ? "skip" : "FAIL") | \(o.name) | \(o.steps) steps\(o.skipped.map { " | \($0)" } ?? "")") }
}

@Suite(.serialized) @MainActor struct Suite16 {
    @Test func theReadHalfPassesThroughTheKit() async throws {
        let core = try openCore("conf-kit-read")
        defer { core.close() }
        let d = KitDriver(core: core, engine: "text")
        // the audio engine is compiled into the C ABI's library (2026-11-21): every core here has it
        d.also = ["manifests", "engine:audio"]
        let cases = try Conformance.cases(at: casesURL)
        let out = await Conformance.run(d, cases: cases, half: .read)
        log(d.name, out)
        #expect(out.allSatisfy { $0.error == nil }, "\(Conformance.summary(d.name, out))")
        #expect(out.count == cases.filter { $0["half"] == "read" }.count && out.count >= 12)
        let skipped = out.filter { $0.skipped != nil }.map(\.name)
        // the array engine's cases, and those that need a core that installs engines (this one is
        // given no platform that can: decision 5's pick-list case)
        let arrays = cases.filter { $0["half"] == "read" && ($0["needs"].array?.contains("engine:array") == true || $0["needs"].array?.contains("install") == true) }.compactMap { $0["name"].string }
        #expect(skipped == arrays && arrays.count >= 5, "only the array engine's cases are skipped: \(skipped)")
        // the document model kept up with every case's scene
        let full = try await core.request(Scene.self, "scene.get")
        #expect(await d.model.wait { d.model.gen == full.gen })
        #expect(d.model.faces.items == full.nodes)
    }

    @Test func theArrayCasePassesThroughTheKitOnTheArrayEngine() async throws {
        let core = try openCore("conf-kit-array", ["engine": "array"])
        defer { core.close() }
        let d = KitDriver(core: core, engine: "array")
        let cases = try Conformance.cases(at: casesURL).filter { $0["needs"].array?.contains("engine:array") == true }
        #expect(!cases.isEmpty)
        let out = await Conformance.run(d, cases: cases, half: .read)
        log(d.name, out)
        #expect(!out.isEmpty && out.allSatisfy(\.passed), "\(Conformance.summary(d.name, out))")
    }

    @Test func theReadHalfPassesOverTheControlPort() async throws {
        let tokenFile = scratch("conf-port") + "/token"
        let core = try openCore("conf-port", ["control": ["port": 0, "token_file": .string(tokenFile)]])
        defer { core.close() }
        let port = try #require(try await core.request("core.control")["port"].uint64)
        let token = try String(contentsOfFile: tokenFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        let model = DocumentModel(core: core)
        model.start()
        let d = try await PortDriver(port: UInt16(port), token: token, following: model)
        defer { d.close() }
        // the audio engine is compiled into the C ABI's library (2026-11-21): every core here has it
        d.also = ["manifests", "engine:audio"]
        let cases = try Conformance.cases(at: casesURL)
        let out = await Conformance.run(d, cases: cases, half: .read)
        log(d.name, out)
        #expect(out.allSatisfy { $0.error == nil }, "\(Conformance.summary(d.name, out))")
        let skipped = out.filter { $0.skipped != nil }.map(\.name)
        // the array engine's cases, and those that need a core that installs engines (this one is
        // given no platform that can: decision 5's pick-list case)
        let arrays = cases.filter { $0["half"] == "read" && ($0["needs"].array?.contains("engine:array") == true || $0["needs"].array?.contains("install") == true) }.compactMap { $0["name"].string }
        #expect(skipped == arrays, "\(skipped)")
    }

    /// Not N3's gate, but the kit's typed calls (geometry, drag moves) behind every case.
    @Test func theWholeSuitePassesThroughTheKit() async throws {
        let core = try openCore("conf-kit-all")
        defer { core.close() }
        let d = KitDriver(core: core, engine: "text")
        // the audio engine is compiled into the C ABI's library (2026-11-21): every core here has it
        d.also = ["manifests", "engine:audio"]
        let cases = try Conformance.cases(at: casesURL)
        let out = await Conformance.run(d, cases: cases, half: nil)
        log(d.name + ", every case", out)
        #expect(out.allSatisfy { $0.error == nil }, "\(Conformance.summary(d.name, out))")
        #expect(out.count == cases.count)
    }

    /// Decision 16: the copied-manifest steps (`only: copied_manifest`), which need `$includes`
    /// and a matcher as a `$deep` field, run through the kit on a core whose engines folder is the
    /// one `conformance::copied_manifest_folder` makes (made here the same way, in scratch); and
    /// they fail on the suite's own folder, so they are not vacuous.
    @Test func theCopiedManifestStepsPassThroughTheKit() async throws {
        let folder = try copiedManifestFolder(scratch("conf-copied"))
        let cases = try Conformance.cases(at: casesURL).filter { $0["name"].string?.hasPrefix("an engine a manifest") == true }
        #expect(cases.count == 1)
        // a platform that starts programs (the copied engine is a process), as the Mac app is
        let core = try openCore("conf-copied", ["dirs": ["engines": .string(folder)], "platform": ["os": "macos", "subprocess": true]])
        defer { core.close() }
        let d = KitDriver(core: core, engine: "text")
        d.also = ["manifests", "copied_manifest"]
        let out = await Conformance.run(d, cases: cases, half: nil)
        log(d.name + ", a copied manifest", out)
        #expect(out.count == 1 && out.allSatisfy(\.passed) && (out.first?.steps ?? 0) >= 9, "\(out)")
        let plain = try openCore("conf-copied-plain", ["platform": ["os": "macos", "subprocess": true]])
        defer { plain.close() }
        let p = KitDriver(core: plain, engine: "text")
        p.also = ["manifests", "copied_manifest"]
        let failed = await Conformance.run(p, cases: cases, half: nil)
        #expect(failed.first?.error?.hasPrefix("step 5:") == true, "\(failed)")
    }

    /// The matchers a copied-manifest step uses: `$includes` holds for text only, and a `$deep`
    /// field may be a matcher.
    @Test func includesAndMatcherFieldsInDeep() async throws {
        let core = try openCore("conf-matchers")
        defer { core.close() }
        let d = KitDriver(core: core, engine: "text")
        let steps: [(JSONValue, Bool)] = [
            (["call": "ui.inspect", "expect": ["/result/title": ["$includes": "Untitled"]]], true),
            (["call": "ui.inspect", "expect": ["/result/title": ["$includes": "nowhere in it"]]], false),
            (["call": "ui.inspect", "expect": ["/result/graph/wires": ["$includes": "0"]]], false),
            (["call": "menu.get", "expect": ["/result/menus": ["$deep": ["label": ["$includes": "Engine"], "submenu": true]]]], true),
            (["call": "menu.get", "expect": ["/result/menus": ["$deep": ["label": ["$includes": "Nonesuch"]]]]], false),
        ]
        for (step, holds) in steps {
            let o = await Conformance.runCase(d, ["name": "m", "about": "-", "half": "read", "steps": [step]])
            #expect(o.passed == holds, "\(step): \(o.error ?? "passed")")
        }
    }

    /// The runner is not vacuous: a wrong want, a wrong ok, a missing value each fail their case.
    @Test func aCaseThatIsWrongFails() async throws {
        let core = try openCore("conf-wrong")
        defer { core.close() }
        let d = KitDriver(core: core, engine: "text")
        let add: JSONValue = ["call": "command.run", "params": ["command": "node.add", "params": ["kind": "text.literal", "pos": [0, 0]]], "save": ["a": "/result/id"]]
        let bad: [(JSONValue, String)] = [
            (["call": "scene.get", "expect": ["/result/selection/nodes": ["$a"]]], "wanted"),
            (["gesture": ["kind": "tap", "at": [0, 0]], "ok": false], "wanted ok false"),
            (["gesture": ["kind": "bogus", "at": [0, 0]]], "wanted ok true"),
            (["call": "graph.get", "expect": ["/result/nodes/$a/nothing": 1]], "got nothing"),
            (["gesture": ["kind": "tap", "at": ["$locate": ["node": "n404"]]]], "geometry.locate"),
            (["call": "scene.get", "save": ["x": "/nope"]], "nothing at"),
            (["call": "scene.get", "expect": ["/result/nodes": ["$len": 7]]], "$len"),
            (["check": "nidsUnique", "scene": ["nodes": []]], "too few"),
        ]
        for (step, says) in bad {
            let c: JSONValue = ["name": "wrong", "about": "-", "half": "read", "steps": [add, step]]
            let o = await Conformance.runCase(d, c)
            let e = o.error ?? ""
            #expect(e.hasPrefix("step 2:") && e.contains(says), "\(step): \(e)")
        }
        // and a case needing what the driver lacks is skipped, never passed
        let o = await Conformance.runCase(d, ["name": "needs", "about": "-", "half": "read", "needs": ["engine:nonesuch"], "steps": [add]])
        #expect(o.skipped != nil && o.error == nil && !o.passed)
    }
}
