// B158, headless on a real core: a wire follows a drag on its bend point (the core's path as it
// would be, `plum_drag_wire`, drawn in place of the wire's own), keeps it after the end until the
// scene brings the new one, and goes back at once on a cancel; the bend points' dots show only
// while the wire is hovered, has an end on a selected face, or is being bent. With WIRE_SHOTS=DIR
// the canvas is pictured unselected, hovered, selected and mid-drag.

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import Testing

@Suite(.serialized) @MainActor struct BendDrag {
    func shot(_ canvas: LayerCanvas, _ name: String) async {
        guard let dir = WireShots.dir else { return }
        // the tiles' pictures drawn first, as the wire pictures do
        canvas.settle()
        _ = await until { canvas.idle && !canvas.settlePending }
        if let i = canvas.snapshot(size: canvas.viewSize, scale: 2) { writePNG(i, "\(dir)/b158-\(name).png") }
    }

    @Test func aWireFollowsItsBendPointsDragAndItsDotsShowOnlyWhenWanted() async throws {
        let (core, model, canvas, drag, _) = try await editor("edit-bend-drag")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 400, 0)
        _ = try await model.invoke("wire.connect", params: ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "text"]])
        _ = try await model.invoke("wire.route", params: ["to": ["node": .string(b), "port": "text"], "via": [[230, 150]]])
        #expect(await settled(model, canvas, faces: 2, wires: 1))
        canvas.viewSize = CGSize(width: 800, height: 500)
        canvas.setViewport(Viewport(pan: CGPoint(x: 100, y: 120), zoom: 1))
        let nid = try #require(model.wires.order.first)
        let via = { model.wires[nid]?.item.via.map { [$0.x, $0.y] } ?? [] }
        #expect(await until { via() == [[230, 150]] && canvas.wireBox(nid: nid) != nil })

        // quiet: no dots on a wire that isn't hovered, selected or bent
        #expect(canvas.bendDotsShown.isEmpty, "\(canvas.bendDotsShown)")
        await shot(canvas, "1-unselected")
        // hovered (the wire, or the bend point itself): its dots
        canvas.hover(canvas.link.hit(Point(230, 150), slop: 4))
        #expect(canvas.bendDotsShown[nid] == [CGPoint(x: 230, y: 150)])
        await shot(canvas, "2-hovered")
        canvas.hover(nil)
        #expect(canvas.bendDotsShown.isEmpty)
        // an end on a selected face: its dots
        _ = try await model.invoke("selection.set", params: ["nodes": [.string(a)]])
        #expect(await until { canvas.bendDotsShown[nid] != nil })
        await shot(canvas, "3-selected")
        _ = try await model.invoke("selection.clear")
        #expect(await until { canvas.bendDotsShown.isEmpty })
        // still grabbed where it isn't drawn: the hit radius is the core's
        #expect(canvas.link.hit(Point(233, 153), slop: 0).kind == .bend)

        // the drag: the wire drawn where it would land, live, nothing changed yet
        let before = try #require(canvas.wireBox(nid: nid))
        drag.begin(at: view(canvas, Point(230, 150)), mods: [])
        #expect(await until { drag.kind == .bend })
        drag.move(to: view(canvas, Point(250, 170)))
        drag.move(to: view(canvas, Point(270, 230)))
        let bent = try #require(canvas.bentWireShown)
        #expect(bent.nid == nid)
        let box = try #require(bent.path?.boundingBoxOfPath)
        #expect(box.maxY >= 229 && box.maxY > before.maxY, "bent \(box) from \(before)")
        #expect(canvas.bendDotsShown[nid] == [CGPoint(x: 270, y: 230)], "the dot moves with it")
        #expect(canvas.liveWireLayer(nid: nid) != nil, "drawn on its own layer, out of the tiles")
        #expect(via() == [[230, 150]], "nothing changed until the end")
        await shot(canvas, "4-mid-drag")
        drag.end(at: view(canvas, Point(270, 230)))
        #expect(await until { via() == [[270, 230]] })
        #expect(await until { canvas.bentWireShown == nil && canvas.liveWireLayer(nid: nid) == nil })
        let after = try #require(canvas.wireBox(nid: nid))
        #expect(abs(after.maxY - before.maxY - 80) < 2, "the scene's path is where the drag left it: \(after) from \(before)")
        #expect(canvas.bendDotsShown.isEmpty, "quiet again")
        await shot(canvas, "5-after")
        #expect(model.state.undo != nil)

        // a cancel: the wire's own back at once, the document as it was
        drag.begin(at: view(canvas, Point(270, 230)), mods: [])
        #expect(await until { drag.kind == .bend })
        drag.move(to: view(canvas, Point(320, 300)))
        #expect(canvas.bentWireShown?.nid == nid)
        drag.cancel()
        #expect(await until { !drag.active && drag.kind == nil && canvas.bentWireShown == nil })
        #expect(await until { canvas.liveWireLayer(nid: nid) == nil })
        try await Task.sleep(for: .milliseconds(200))
        #expect(via() == [[270, 230]])
        #expect(canvas.bendDotsShown.isEmpty)
    }

    @Test func anAutomaticRoutesCornerDragDrawsTheRouteLive() async throws {
        let (core, model, canvas, drag, _) = try await editor("edit-route-drag")
        defer { core.close() }
        // the source to the right: an automatic route round the faces
        let a = try await add(model, "text.literal", 500, 0)
        let b = try await add(model, "text.upper", 0, 0)
        _ = try await model.invoke("wire.connect", params: ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "text"]])
        #expect(await settled(model, canvas, faces: 2, wires: 1))
        canvas.setViewport(Viewport(pan: CGPoint(x: 100, y: 120), zoom: 1))
        let nid = try #require(model.wires.order.first)
        let c = try await canvas.link.locate(["wire": .string("\(b).text"), "corner": 2])
        let before = try #require(canvas.wireBox(nid: nid))
        drag.begin(at: view(canvas, c.at), mods: [])
        #expect(await until { drag.kind == .route })
        drag.move(to: view(canvas, Point(c.at.x, c.at.y + 60)))
        let bent = try #require(canvas.bentWireShown)
        let box = try #require(bent.path?.boundingBoxOfPath)
        #expect(box.maxY > before.maxY + 50, "the route drawn lower, live: \(box) from \(before)")
        #expect((canvas.bendDotsShown[nid]?.count ?? 0) == 4)
        await shot(canvas, "6-route-mid-drag")
        drag.end(at: view(canvas, Point(c.at.x, c.at.y + 60)))
        #expect(await until { (model.wires[nid]?.item.via.count ?? 0) == 4 })
        #expect(await until { canvas.bentWireShown == nil && canvas.liveWireLayer(nid: nid) == nil })
    }
}
