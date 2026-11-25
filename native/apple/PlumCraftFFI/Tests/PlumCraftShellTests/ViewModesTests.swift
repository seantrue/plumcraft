// B154 slice 1: an array's view modes and channel histograms in the shared viewers. The kit reads
// `viewModes`, `viewMode` and each channel's histogram from `popout.get`; a View choice and the
// Log toggle are the core's `run.view` and `viewer.array`; a field of the wrong shape is skipped.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import Testing

@MainActor @Suite struct ViewModes {
    @Test func theModesAndHistogramsAreTheCoresAndAChoiceIsACommand() async throws {
        _ = testEnvironment
        var c = ShellConfig(appID: "com.plumcraft.test", appName: "PlumCraft", version: "0", storeRoot: scratch("view-modes"))
        c.platform = ["os": "ios", "subprocess": false, "craft_apps": false, "python": false]
        c.also = ["array"]
        let (core, ready) = try ShellCore.open(c)
        _ = await ready.value
        defer { core.close() }
        let r = try await core.request("document.open")
        let model = DocumentModel(core: core, doc: try #require(r["doc"].uint64))
        model.start()
        #expect(await model.wait(timeout: .seconds(30)) { model.applied > 0 })
        let m = try #require(try await model.run("node.add", ["kind": "array.mandelbrot", "pos": [0, 0]])["id"].string)
        _ = try await model.run("node.set", ["node": .string(m), "port": "width", "value": 40])
        _ = try await model.run("node.set", ["node": .string(m), "port": "height", "value": 30])
        _ = try await model.run("view.popout", ["node": .string(m), "port": "escape"])
        _ = try await model.run("run.start", [:])
        #expect(await model.wait(timeout: .seconds(60)) { model.windows.list.contains { $0.node == m } })
        let p = PopoutModel(model: model, window: try #require(model.windows.list.first { $0.node == m }))
        p.refresh()
        #expect(await model.wait(timeout: .seconds(60)) {
            if p.value?.modes.isEmpty ?? true { p.refresh() }
            return !(p.value?.stats?.array ?? []).isEmpty && !(p.value?.modes.isEmpty ?? true)
        })
        let v = try #require(p.value)
        #expect(v.modes == ["auto", "color", "stretch", "clip", "heat", "labels", "angle"])
        #expect(v.mode == "auto")
        let h = try #require(v.stats?.array?.first)["histogram"]
        #expect(ArrayStats.heights(h).count == 256)
        #expect(ArrayStats.heights(h).max() == 1)
        // the View choice is run.view; the cached run brings the heat pictures
        await p.command("run.view", PlumCraftShell.ViewModes.params("heat", shown: v.viewMode))
        #expect(await model.wait(timeout: .seconds(60)) {
            if p.value?.mode != "heat" { p.refresh() }
            return p.value?.mode == "heat"
        })
        // the Log toggle is viewer.array
        await p.command("viewer.array", ["log": true])
        #expect(await model.wait(timeout: .seconds(10)) { p.value?.state?["array"]["log"].bool == true })
        #expect(p.value?.stats?.array?.first?["histogram"]["log"].bool == true)
    }

    @Test func wordsParamsAndHostileFields() throws {
        #expect(PlumCraftShell.ViewModes.label("color") == "Colour")
        #expect(PlumCraftShell.ViewModes.label("x.new") == "x.new", "a mode the shell doesn't know keeps its id")
        #expect(PlumCraftShell.ViewModes.params("heat", shown: nil) == ["view": "heat"])
        #expect(PlumCraftShell.ViewModes.params("angle", shown: ["id": "angle", "params": ["turn": 360]])["params"]?["turn"].double == 360)
        #expect(PlumCraftShell.ViewModes.params("angle", shown: nil)["params"]?["turn"].double == 1)
        #expect(ArrayStats.line([["name": "L", "min": "0", "max": "100", "nan": 3], ["name": "a", "min": "-1", "max": "1", "nan": 0]]) == "L 0 to 100, 3 NaN · a -1 to 1")
        #expect(ArrayStats.heights(["heights": [0.5, 2, -1, "x"]]) == [0.5, 1, 0, 0], "clamped; junk is 0")
        // a value whose new fields are the wrong shape still reads, with no modes
        let json = #"{"id": 1, "node": "n1", "port": "p", "viewModes": {"auto": 1}, "viewMode": 7, "stats": "none"}"#
        let v = try JSONDecoder().decode(PopoutValue.self, from: Data(json.utf8))
        #expect(v.modes.isEmpty)
        #expect(v.mode == "auto")
        #expect(!ArrayStats.shows(v))
    }
}
