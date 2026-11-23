// The shared shell pieces (N3 slice 5): the config the apps build, the core's start, the menu
// model's lookups, the words, watched values as text, and the app's lines reaching the core's log.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import Testing

/// Scratch for this process (swept by the Rust tests once it exited), and the environment every
/// test core runs in: never the maintainer's store, whatever the login shell says.
func scratch(_ name: String) -> String {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("plumcraft-test-\(ProcessInfo.processInfo.processIdentifier)")
        .appendingPathComponent("swift-shell-\(name)")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.path
}

let testEnvironment: Void = {
    setenv("PLUMCRAFT_EDITOR_PREFS", scratch("prefs") + "/prefs.json", 1)
    setenv("COMMONKIT_STORE_ROOT", scratch("store"), 1)
    setenv("COMMONKIT_MARK_DIR", scratch("marks"), 1)
    setenv("COMMONKIT_TEST_RUN", scratch("run"), 1)
    setenv("PLUMCRAFT_CRAFT_ROOT", scratch("craft"), 1)
    setenv("COMMONKIT_HASH", "b3", 0)
    setenv("COMMONKIT_STORE_MIN_FREE_GB", "2", 0)
}()

@Suite struct ShellConfigs {
    @Test func theConfigIsTheContractsShape() {
        var c = ShellConfig(appID: "com.swapwizard.plummer.pad", appName: "Plummer", version: "0.7.0", storeRoot: "/s")
        c.dirs = ["caches": "/c", "logs": "/l"]
        c.platform = ["os": "ios", "subprocess": false]
        c.engine = "text"
        c.also = ["array", "text"]
        let j = c.json
        #expect(j["store_root"].string == "/s")
        #expect(j["dirs"]["store"].string == "/s")
        #expect(j["dirs"]["caches"].string == "/c")
        #expect(j["platform"]["os"].string == "ios")
        #expect(j["engines"]["default"].string == "text")
        // the first engine isn't named again beside itself
        #expect(j["engines"]["also"].array?.compactMap(\.string) == ["array"])
        // engines picked by the graph unless the app says otherwise (decision 8C)
        #expect(j["engines"]["auto"].bool == true)
        c.pickEngines = false
        #expect(c.json["engines"]["auto"].bool == false)
        // no control port unless a token file is named
        #expect(j["control"] == .null)
        c.controlTokenFile = "/t"
        #expect(c.json["control"]["token_file"].string == "/t")
        #expect(c.json["control"]["port"].uint64 == 0)
    }

    @Test func theCoresOwnFirstDocumentIsClosed() async throws {
        _ = testEnvironment
        var c = ShellConfig(appID: "com.plumcraft.test", appName: "PlumCraft", version: "0", storeRoot: scratch("store-open"))
        c.platform = ["os": "ios", "subprocess": false, "craft_apps": false, "python": false]
        c.also = ["array"]
        let (core, ready) = try ShellCore.open(c)
        defer { core.close() }
        let port = await ready.value
        #expect(port == nil)
        let list = try await core.request("document.list")
        #expect(list["docs"].array?.isEmpty == true)
        // the app opens its own, on both engines
        let r = try await core.request("document.open")
        #expect(r["doc"].uint64 != nil)
    }

    /// Decision 8C at N3's exit: as the apps open a core (no `also`), a document starts only the
    /// engines its graph uses, and a node of another engine's kind added later starts that one.
    @Test func aDocumentStartsOnlyTheEnginesItsGraphUses() async throws {
        _ = testEnvironment
        var c = ShellConfig(appID: "com.plumcraft.test", appName: "PlumCraft", version: "0", storeRoot: scratch("store-pick"))
        c.platform = ["os": "ios", "subprocess": false, "craft_apps": false, "python": false]
        let (core, ready) = try ShellCore.open(c)
        defer { core.close() }
        _ = await ready.value
        let dir = scratch("pick")
        func file(_ name: String, _ kinds: [String]) throws -> String {
            var nodes: [String: JSONValue] = [:]
            for (i, k) in kinds.enumerated() { nodes["n\(i)"] = ["kind": .string(k), "pos": [.double(Double(i) * 300), 0]] }
            let path = dir + "/" + name
            try JSONValue.object(["version": 1, "nodes": .object(nodes), "wires": []]).serialized().write(to: URL(fileURLWithPath: path))
            return path
        }
        func engines(_ doc: UInt64) async throws -> [String] {
            let l = try await core.request("command.run", ["command": "engine.list", "params": [:]], doc: doc)
            return [l["current"]["engine"].string ?? ""] + (l["added"].array ?? []).compactMap { $0["engine"].string }
        }
        let text = try await core.request("document.open", ["path": .string(try file("text.json", ["text.literal", "text.upper"]))])
        let textDoc = try #require(text["doc"].uint64)
        #expect(try await engines(textDoc) == ["text"])
        let both = try await core.request("document.open", ["path": .string(try file("both.json", ["text.literal", "array.mandelbrot"]))])
        let bothDoc = try #require(both["doc"].uint64)
        #expect(try await engines(bothDoc) == ["text", "array"])
        // the text graph comes to use the array engine: it starts, and the add waits for it
        let m = try await core.request("command.run", ["command": "node.add", "params": ["kind": "array.mandelbrot"]], doc: textDoc)
        #expect(m["id"].string != nil)
        #expect(try await engines(textDoc) == ["text", "array"])
    }
}

@Suite struct ShellLookups {
    func item(_ json: String) throws -> [MenuItem] {
        try JSONValueDecoder().decode([MenuItem].self, from: Data(json.utf8))
    }

    @Test func aCommandIsFoundWhereverItIs() throws {
        let menus = try item(#"""
        [{"id":"run","label":"Run","items":[{"id":"run.start","label":"Run","command":"run.start","enabled":true},
          {"id":"run.with","label":"Run With","command":"run.start","params":{"targets":["a"]},"enabled":true},
          {"id":"sub","label":"More","items":[{"id":"run.cancel","label":"Cancel Run","command":"run.cancel","enabled":false,"reason":"Nothing is running"}]}]}]
        """#)
        #expect(menus.item(command: "run.start")?.id == "run.start")
        #expect(menus.item(command: "run.cancel")?.reason == "Nothing is running")
        #expect(menus.item(command: "graph.tidy") == nil)
    }

    @Test func unbuiltCommandsSaySo() throws {
        let it = try item(#"[{"id":"e","label":"Edit Value…","command":"node.editor","enabled":true}]"#)[0]
        // built in N4 slice 6: nothing is left on the banner
        #expect(ShellCommands.notBuilt(it) == nil)
        #expect(ShellCommands.unbuilt.isEmpty)
        // built in N4 slice 5: the Messages window
        #expect(ShellCommands.unbuilt["view.messages"] == nil)
        // built in N4 slice 4: Settings and About
        #expect(ShellCommands.unbuilt["app.settings"] == nil)
        #expect(ShellCommands.unbuilt["app.about"] == nil)
        let run = try item(#"[{"id":"r","label":"Run","command":"run.start","enabled":true}]"#)[0]
        #expect(ShellCommands.notBuilt(run) == nil)
        // built in N4 slice 3: the parameters inspector and pop-outs
        #expect(ShellCommands.unbuilt["node.parameters"] == nil)
        #expect(ShellCommands.unbuilt["view.popout"] == nil)
        #expect(ToolbarWords.symbol("play") == "play.fill")
        #expect(ToolbarWords.symbol("nope") == "square.dashed")
    }

    @Test func theAnswersWords() {
        #expect(AskWords.buttonTitle("discard", kind: "unsavedChanges") == "Don't Save")
        #expect(AskWords.buttonTitle("discard", kind: nil) == "Discard")
        #expect(AskWords.buttonTitle("save", kind: nil) == "Save")
        #expect(AskWords.buttonTitle("later", kind: nil) == "Later")
        // the core's own words when it gives them (the saveShape prompt), else the platform's
        let shape: JSONValue = ["kind": "saveShape", "answers": ["png", "npy", "cancel"],
                                "labels": ["png": "Save as PNG anyway", "npy": "Save as NumPy (.npy)", "cancel": "Cancel"]]
        #expect(AskWords.buttonTitle("png", prompt: shape) == "Save as PNG anyway")
        #expect(AskWords.buttonTitle("npy", prompt: shape) == "Save as NumPy (.npy)")
        #expect(AskWords.buttonTitle("discard", prompt: ["kind": "unsavedChanges"]) == "Don't Save")
        #expect(AskWords.extensions(["kinds": [["label": "W", "extensions": ["json", "zip"]]]]) == ["json", "zip"])
        #expect(AskWords.extensions(["kinds": [["label": "Any", "extensions": ["*"]]]]) == [])
        #expect(AskWords.extensions(.null) == [])
        // Save Result…'s formats: a kind each, the name's extension picking one, renamed by another
        let save: JSONValue = ["kinds": [["label": "Quoted Text", "extensions": ["quoted"]], ["label": "Text", "extensions": ["txt"]], ["label": "Any", "extensions": ["*"]]]]
        let kinds = AskWords.kinds(save)
        #expect(kinds.map(\.label) == ["Quoted Text", "Text"])
        #expect(AskWords.kindIndex(kinds, name: "Result.quoted") == 0)
        #expect(AskWords.kindIndex(kinds, name: "plain.TXT") == 1)
        #expect(AskWords.kindIndex(kinds, name: "nothing") == 0)
        #expect(AskWords.renamed("Result.quoted", to: kinds[1]) == "Result.txt")
        #expect(AskWords.renamed("Result", to: kinds[0]) == "Result.quoted")
    }
}

@Suite struct WatchedText {
    func watched(_ json: String) throws -> Watched {
        try JSONValueDecoder().decode(Watched.self, from: Data(json.utf8))
    }

    @Test func scalarsAreFormatted() throws {
        #expect(ValueText.scalar(try watched(#"{"node":"a","port":"o","value":1234567}"#)) == NumberFormatter.localizedString(from: 1234567, number: .decimal))
        #expect(ValueText.scalar(try watched(#"{"node":"a","port":"o","value":true}"#)) == "true")
        #expect(ValueText.scalar(try watched(#"{"node":"a","port":"o","value":"x"}"#)) == "x")
        #expect(ValueText.scalar(try watched(#"{"node":"a","port":"o","summary":"7 items"}"#)) == "7 items")
        #expect(ValueText.scalar(try watched(#"{"node":"a","port":"o"}"#)) == "—")
    }

    @Test func anExcerptSaysHowMuchIsShown() throws {
        let w = try watched(#"{"node":"a","port":"o","excerpt":{"text":"hello","totalChars":12000},"data":{"handle":"b3:abc"}}"#)
        let e = ValueText.excerpt(w)
        #expect(e.shown == 5 && e.total == 12000 && e.text == "hello")
        #expect(ValueText.dataHandle(w) == "b3:abc")
        #expect(ValueText.summary(try watched(#"{"node":"a","port":"o","media":"text"}"#)) == "text, not computed yet")
    }
}

@Suite struct Logging {
    @MainActor @Test func theAppsLinesReachTheCoresLog() async throws {
        _ = testEnvironment
        let core = try PlumCore(config: ["store_root": .string(scratch("store-log"))])
        defer { core.close() }
        let log = UnifiedLog(core: core, subsystem: "com.plumcraft.test")
        log.start()
        defer { log.stop() }
        let mark = "slice 5 line \(UUID().uuidString)"
        log.say("info", mark)
        // the core's log has it under `shell`; wait on the condition, at most 10 s
        let deadline = ContinuousClock.now + .seconds(10)
        var found = false
        while !found && ContinuousClock.now < deadline {
            let t = try await core.request("log.tail", ["source": "shell", "limit": 500])
            found = (t["entries"].array ?? []).contains { $0["message"].string == mark }
            if !found { try await Task.sleep(for: .milliseconds(20)) }
        }
        #expect(found)
    }
}

/// A list of named pictures (an engine's `sheet` labels) is stepped by name in the viewer bar.
@Test func aListsItemsAreSteppedByName() throws {
    let named = try JSONValueDecoder().decode(PopoutValue.self, from: Data("""
        {"id": 1, "node": "n", "port": "p", "items": 3, "index": 1,
         "sheet": [{"index": 0, "label": "Lab"}, {"index": 1, "label": "l"}, {"index": 2, "label": ""}]}
        """.utf8))
    let names = ViewerBar.itemNames(named)
    #expect(names == ["Lab", "l", "item 3"])
    #expect(ViewerBar.itemTitle(names, 1, 3) == "l · 2 of 3")
    let unnamed = try JSONValueDecoder().decode(PopoutValue.self, from: Data("""
        {"id": 1, "node": "n", "port": "p", "items": 3, "index": 0, "sheet": [{"index": 0, "label": ""}]}
        """.utf8))
    #expect(ViewerBar.itemNames(unnamed).isEmpty, "no names: the plain stepper")
}
