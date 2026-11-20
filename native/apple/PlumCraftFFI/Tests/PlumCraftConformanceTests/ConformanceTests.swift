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
    setenv("COMMONKIT_HASH", "b3", 0)
    setenv("COMMONKIT_STORE_MIN_FREE_GB", "2", 0)
}()

func openCore(_ name: String, _ extra: [String: JSONValue] = [:]) throws -> PlumCore {
    _ = testEnvironment
    var config: [String: JSONValue] = ["store_root": .string(scratch("store-\(name)"))]
    for (k, v) in extra { config[k] = v }
    return try PlumCore(config: .object(config))
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
        let cases = try Conformance.cases(at: casesURL)
        let out = await Conformance.run(d, cases: cases, half: .read)
        log(d.name, out)
        #expect(out.allSatisfy { $0.error == nil }, "\(Conformance.summary(d.name, out))")
        #expect(out.count == cases.filter { $0["half"] == "read" }.count && out.count >= 12)
        let skipped = out.filter { $0.skipped != nil }.map(\.name)
        #expect(skipped == ["a watched picture arrives as picture"], "only the array engine's case is skipped: \(skipped)")
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
        let cases = try Conformance.cases(at: casesURL)
        let out = await Conformance.run(d, cases: cases, half: .read)
        log(d.name, out)
        #expect(out.allSatisfy { $0.error == nil }, "\(Conformance.summary(d.name, out))")
        let skipped = out.filter { $0.skipped != nil }.map(\.name)
        #expect(skipped == ["a watched picture arrives as picture"], "\(skipped)")
    }

    /// Not N3's gate, but the kit's typed calls (geometry, drag moves) behind every case.
    @Test func theWholeSuitePassesThroughTheKit() async throws {
        let core = try openCore("conf-kit-all")
        defer { core.close() }
        let d = KitDriver(core: core, engine: "text")
        let cases = try Conformance.cases(at: casesURL)
        let out = await Conformance.run(d, cases: cases, half: nil)
        log(d.name + ", every case", out)
        #expect(out.allSatisfy { $0.error == nil }, "\(Conformance.summary(d.name, out))")
        #expect(out.count == cases.count)
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
