// The curves editor's model on a real core (docs/shell-interface.md 12.21): a press adds a point,
// a drag moves it, the lift sends one `curves.edit`, and one undo takes the gesture back; a press
// on a point takes it; a drag well outside removes it; a kind with no native editor is the core's.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import Testing

@MainActor @Suite struct CurvesEditor {
    func open(_ name: String) async throws -> (PlumCore, DocumentModel) {
        _ = testEnvironment
        var c = ShellConfig(appID: "com.plumcraft.test", appName: "PlumCraft", version: "0", storeRoot: scratch(name))
        c.platform = ["os": "ios", "subprocess": false, "craft_apps": false, "python": false]
        c.also = ["array"]
        let (core, ready) = try ShellCore.open(c)
        _ = await ready.value
        let r = try await core.request("document.open")
        let model = DocumentModel(core: core, doc: try #require(r["doc"].uint64))
        return (core, model)
    }

    func until(_ ok: () -> Bool) async -> Bool {
        for _ in 0..<600 {
            if ok() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return ok()
    }

    func master(_ model: DocumentModel, _ node: String) async throws -> CurvesCurve {
        let r = try await model.run("node.editorData", ["node": .string(node)])
        return Curves.parse(r["value"])[0]
    }

    @Test func aGestureIsOneEditAndOneUndo() async throws {
        let (core, model) = try await open("store-curves-gesture")
        defer { core.close() }
        let node = try #require(try await model.run("node.add", ["kind": "array.apply_curve"])["id"].string)
        let d = try await model.run("node.editorData", ["node": .string(node)])
        #expect(d["kind"] == "curves" && d["port"] == "curves")
        let report = ReportModel(model: model, key: "editor:\(node)")
        let curves = CurvesModel(report: report)
        curves.reload()
        #expect(await until { curves.loaded })
        #expect(curves.tabs == ["All", "Red", "Green", "Blue"])
        // a press on empty space adds a point; the drag is the view's until it lifts
        #expect(curves.press(0.5, 0.5, reach: 0.05) == 1)
        curves.drag(0.4, 0.7, outside: false)
        curves.drag(0.45, 0.75, outside: false)
        #expect(try await master(model, node).isIdentity, "nothing is sent while dragging")
        curves.lift()
        #expect(curves.sent == 1)
        var after = CurvesCurve.identity
        for _ in 0..<300 {
            after = try await master(model, node)
            if !after.isIdentity { break }
            try? await Task.sleep(for: .milliseconds(10))
        }
        #expect(after.points == [CurvesPoint(0, 0), CurvesPoint(0.45, 0.75), CurvesPoint(1, 1)])
        // a press on that point takes it (no new point); a drag well outside removes it
        #expect(curves.press(0.46, 0.74, reach: 0.05) == 1)
        curves.drag(0.5, 2, outside: true)
        curves.lift()
        #expect(curves.curve.points.count == 2)
        #expect(curves.sent == 2)
        _ = try await model.run("edit.undo")
        _ = try await model.run("edit.undo")
        #expect(try await master(model, node).isIdentity, "two gestures, two undo steps")
        // the end points stay: two are kept
        #expect(curves.press(0, 0, reach: 0.05) == 0)
        curves.drag(0, 0, outside: true)
        curves.lift()
        #expect(curves.curve.points.count == 2)
    }

    @Test func aKindWithNoNativeEditorIsTheCoresWindow() {
        #expect(NativeEditors.kinds.contains("curves"))
        #expect(!NativeEditors.kinds.contains("ports.curve"))
    }
}
