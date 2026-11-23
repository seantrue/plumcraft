// The mask editor's model on a real core (docs/shell-interface.md 12.22): the canvas transform's
// maths; one finger's stroke is one `mask.stroke` (a new file, the value changed) and one undo
// takes it back; the stroke the view painted is the file the core wrote. Scratch folders only.

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import Testing

@Suite struct MaskTransformMaths {
    @Test func fitCentresTheWholeCanvas() {
        let t = MaskTransform.fit(w: 200, h: 100, in: CGSize(width: 424, height: 424), margin: 12)
        #expect(t.scale == 2)
        #expect(t.origin == CGPoint(x: 12, y: 112))
        #expect(t.view(CGPoint(x: 200, y: 100)) == CGPoint(x: 412, y: 312))
    }

    @Test func zoomKeepsThePointUnderTheFingers() {
        let t = MaskTransform(scale: 1.5, origin: CGPoint(x: 10, y: -20))
        let about = CGPoint(x: 300, y: 140)
        let before = t.canvas(about)
        let z = t.zoomed(by: 2.5, about: about)
        let after = z.canvas(about)
        #expect(abs(after.x - before.x) < 1e-9 && abs(after.y - before.y) < 1e-9)
        #expect(z.scale == 3.75)
        // held to its range
        #expect(t.zoomed(by: 1e9, about: .zero).scale == MaskTransform.maxScale)
        // a pan moves the canvas, not the scale; view and canvas are inverses
        let p = t.panned(by: CGSize(width: 5, height: -7))
        #expect(p.origin == CGPoint(x: 15, y: -27) && p.scale == t.scale)
        let q = CGPoint(x: 33.25, y: 71.5)
        #expect(t.canvas(t.view(q)) == q)
    }
}

@MainActor @Suite struct MaskEditor {
    func until(_ ok: () -> Bool) async -> Bool {
        for _ in 0..<1000 {
            if ok() { return true }
            try? await Task.sleep(for: .milliseconds(10))
        }
        return ok()
    }

    @Test func oneStrokeIsOneCommandOneFileAndOneUndo() async throws {
        _ = testEnvironment
        let folder = scratch("mask-edits")
        try FileManager.default.createDirectory(atPath: folder, withIntermediateDirectories: true)
        var c = ShellConfig(appID: "com.plumcraft.test", appName: "PlumCraft", version: "0", storeRoot: scratch("store-mask"))
        c.platform = ["os": "ios", "subprocess": false, "craft_apps": false, "python": false]
        c.also = ["array"]
        let (core, ready) = try ShellCore.open(c)
        defer { core.close() }
        _ = await ready.value
        let doc = try #require(try await core.request("document.open")["doc"].uint64)
        let model = DocumentModel(core: core, doc: doc)
        // the feed brings the pictures' media notes
        model.start()
        #expect(await model.wait(timeout: .seconds(30)) { model.applied > 0 })
        let src = try #require(try await model.run("node.add", ["kind": "array.mandelbrot"])["id"].string)
        let pm = try #require(try await model.run("node.add", ["kind": "array.paint_mask"])["id"].string)
        _ = try await model.run("wire.connect", ["from": ["node": .string(src), "port": "escape"], "to": ["node": .string(pm), "port": "picture"]])
        _ = try await model.run("mask.folder", ["folder": .string(folder)])
        _ = try await model.run("run.start")
        _ = try await core.request("ui.waitRun", ["timeout": 30], doc: doc)
        let report = ReportModel(model: model, key: "editor:\(pm)")
        let mask = MaskEditorProbe.model(report)
        mask.reload()
        #expect(await until { mask.loaded && mask.w > 0 })
        #expect((mask.w, mask.h) == (512, 512))
        #expect(mask.value.isEmpty)
        // one finger: down, moves, up
        mask.radius = 10
        mask.begin(CGPoint(x: 100, y: 100))
        mask.extend(CGPoint(x: 160, y: 120))
        mask.extend(CGPoint(x: 220, y: 200))
        #expect(mask.stroking && mask.sent == 0)
        mask.end()
        #expect(mask.sent == 1)
        #expect(await until { !mask.value.isEmpty && mask.file != nil })
        let first = mask.value
        #expect(first.hasPrefix(folder))
        let u = try await core.request("ui.inspect", doc: doc)
        #expect(u["undo"].string == "Mask Stroke")
        // the stroke the view painted is the file the core wrote, bit for bit
        let file = try #require(mask.file)
        let written = try #require(MaskEditorProbe.canvas(file, w: 512, h: 512))
        var mine = MaskCanvas(w: 512, h: 512, picture: MaskEditorProbe.picture(mask))
        var s = Masks.begin(mine, MaskBrush(radius: 10))
        for p in [(100.0, 100.0), (160.0, 120.0), (220.0, 200.0)] { Masks.strokeTo(&mine, &s, p) }
        #expect(written.pixelHash == mine.pixelHash)
        // the file read by the editor's own PNG reader, not the platform's decoder: exact
        let own = try #require(PNGRGBA.decode(try Data(contentsOf: URL(fileURLWithPath: file))))
        #expect(MaskCanvas(w: own.w, h: own.h, rgba: own.rgba)?.pixelHash == mine.pixelHash)
        #expect(PNGRGBA.decode(Data([137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0])) == nil)
        #expect(PNGRGBA.decode(Data(repeating: 0, count: 64)) == nil)
        var torn = try Data(contentsOf: URL(fileURLWithPath: file))
        torn.removeLast(torn.count / 2)
        _ = PNGRGBA.decode(torn)
        // one undo takes it back
        mask.undo()
        #expect(await until { mask.value.isEmpty })
        #expect(mask.sent == 1 && mask.undone == 1)
    }
}
