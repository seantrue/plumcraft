// The editing gestures through the kit (N4 slice 1), headless on a real core: a face dragged by
// CanvasDrag follows the pointer and is one undo step; Escape's cancel leaves the document as it
// was; a wire drawn from a pin to a pin connects, the target saying whether it fits; a wired
// input's end picked up and dropped on nothing disconnects; a new wire dropped on nothing opens
// the palette filtered to what takes it, and a pick from the PaletteModel adds and connects in
// one undo step; a marquee selects; autoscroll pans at the view's edge; the palette's model
// searches, moves its highlight, and dismisses leaving nothing.

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import Testing

@MainActor
func editor(_ name: String) async throws -> (PlumCore, DocumentModel, LayerCanvas, CanvasDrag, PaletteModel) {
    let (core, model, canvas) = try await canvasOnCore(name)
    let drag = CanvasDrag(canvas: canvas) { CanvasLink.slop(points: 10, zoom: Double(canvas.viewport.zoom)) }
    let palette = PaletteModel(model: model)
    model.onView = { palette.follow($0) }
    return (core, model, canvas, drag, palette)
}

@MainActor
func view(_ canvas: LayerCanvas, _ p: Point) -> CGPoint { canvas.viewport.toView(CGPoint(x: p.x, y: p.y)) }

@MainActor
func pos(_ model: DocumentModel, _ id: String) -> Point? { model.face(id: id)?.pos }

@Suite struct Editing {
    @MainActor @Test func aDraggedFaceFollowsThePointerAndIsOneUndoStep() async throws {
        let (core, model, canvas, drag, _) = try await editor("edit-move")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        #expect(await settled(model, canvas, faces: 1))
        canvas.setViewport(Viewport(pan: CGPoint(x: 200, y: 200), zoom: 1))
        let c = try await canvas.link.locate(["node": .string(a)])
        let p0 = view(canvas, c.at)
        let nid = try #require(canvas.nid(of: a))
        let layer = try #require(canvas.faceLayer(nid: nid))
        let x0 = layer.frame.minX
        drag.begin(at: p0, mods: [])
        #expect(await until { drag.kind == .move })
        for k in 1...10 { drag.move(to: CGPoint(x: p0.x + CGFloat(k) * 8, y: p0.y + CGFloat(k) * 3)) }
        // the face's layer moved with the pointer before anything was sent
        #expect(abs(layer.frame.minX - x0 - 80) < 0.5, "\(layer.frame.minX) from \(x0)")
        #expect(pos(model, a) == Point(0, 0))
        drag.end(at: CGPoint(x: p0.x + 80, y: p0.y + 30))
        #expect(await until { pos(model, a).map { abs($0.x - 80) < 0.5 && abs($0.y - 30) < 0.5 } ?? false })
        #expect(model.state.undo == "Move Node")
        _ = try await model.invoke("edit.undo")
        #expect(await until { pos(model, a) == Point(0, 0) })
        // a cancelled drag leaves the document as it was
        drag.begin(at: p0, mods: [])
        #expect(await until { drag.kind == .move })
        drag.move(to: CGPoint(x: p0.x + 120, y: p0.y))
        drag.cancel()
        #expect(await until { !drag.active && drag.kind == nil })
        try await Task.sleep(for: .milliseconds(200))
        #expect(pos(model, a) == Point(0, 0))
        #expect(model.state.undo != "Move Node")
    }

    @MainActor @Test func wiresConnectPickUpAndDropIntoTheFilteredPalette() async throws {
        let (core, model, canvas, drag, palette) = try await editor("edit-wire")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 400, 0)
        #expect(await settled(model, canvas, faces: 2))
        canvas.setViewport(Viewport(pan: CGPoint(x: 100, y: 200), zoom: 1))
        let out = try await canvas.link.locate(["node": .string(a), "pin": ["side": "out", "index": 0]])
        let inp = try await canvas.link.locate(["node": .string(b), "pin": ["side": "in", "index": 0]])
        // a new wire to a pin that takes it: the target fits, the drop connects
        drag.begin(at: view(canvas, out.at), mods: [])
        #expect(await until { drag.kind == .wire })
        drag.move(to: view(canvas, inp.at))
        let t = canvas.link.dragMove(inp.at, slop: 10).target
        #expect(t.kind == .pin && t.fits == 1)
        drag.end(at: view(canvas, inp.at))
        #expect(await until { model.wires.count == 1 })
        #expect(model.state.undo != nil)
        // the wired input's end picked up and dropped on nothing: the wire goes
        drag.begin(at: view(canvas, inp.at), mods: [])
        #expect(await until { drag.kind == .wire })
        let away = view(canvas, Point(inp.at.x + 60, inp.at.y + 300))
        drag.move(to: away)
        drag.end(at: away)
        #expect(await until { model.wires.count == 0 })
        // a new wire dropped on nothing: the palette there, only what takes it
        drag.begin(at: view(canvas, out.at), mods: [])
        #expect(await until { drag.kind == .wire })
        let drop = view(canvas, Point(out.at.x + 150, out.at.y + 260))
        drag.move(to: drop)
        drag.end(at: drop)
        #expect(await until { palette.isOpen && palette.forWire && !palette.items.isEmpty })
        #expect(drag.holdingWire, "the wire stays drawn while the palette is open")
        #expect(palette.items.allSatisfy { $0.port != nil })
        let undoBefore = model.state.undo
        palette.search("upper")
        #expect(await until { palette.items.contains { $0.kind == "text.upper" } && palette.items.allSatisfy { $0.kind.contains("upper") || $0.label.lowercased().contains("upper") } })
        palette.chooseSelected()
        #expect(await until { model.faces.count == 3 && model.wires.count == 1 && !palette.isOpen })
        #expect(model.state.undo == "Add and Connect", "one step: \(String(describing: model.state.undo)), before \(String(describing: undoBefore))")
        _ = try await model.invoke("edit.undo")
        #expect(await until { model.faces.count == 2 && model.wires.count == 0 })
    }

    @MainActor @Test func aMarqueeSelectsAndThePaletteDismissesLeavingNothing() async throws {
        let (core, model, canvas, drag, palette) = try await editor("edit-marquee")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 400, 0)
        #expect(await settled(model, canvas, faces: 2))
        canvas.setViewport(Viewport(pan: CGPoint(x: 100, y: 200), zoom: 1))
        drag.begin(at: view(canvas, Point(-40, -60)), mods: [])
        #expect(await until { drag.kind == .marquee })
        drag.move(to: view(canvas, Point(700, 400)))
        drag.end(at: view(canvas, Point(700, 400)))
        #expect(await until { Set(model.selection.nodes) == [a, b] })
        // the palette: open, search, move, dismiss
        let undo = model.state.undo
        try await palette.open(at: Point(200, 300))
        #expect(await until { palette.isOpen && !palette.items.isEmpty })
        #expect(palette.sections.count >= 1)
        palette.move(1)
        #expect(palette.selected == 1 || palette.items.count == 1)
        palette.dismiss()
        #expect(await until { model.view.palette == nil })
        #expect(model.state.undo == undo && model.faces.count == 2)
    }

    @MainActor @Test func atTheEdgeTheCanvasScrollsUnderTheDrag() async throws {
        let (core, model, canvas, drag, _) = try await editor("edit-autoscroll")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        #expect(await settled(model, canvas, faces: 1))
        canvas.setViewport(Viewport(pan: CGPoint(x: 200, y: 200), zoom: 1))
        let c = try await canvas.link.locate(["node": .string(a)])
        drag.begin(at: view(canvas, c.at), mods: [])
        #expect(await until { drag.kind == .move })
        let pan0 = canvas.viewport.pan
        drag.move(to: CGPoint(x: canvas.viewSize.width - 4, y: 300))
        #expect(await until { canvas.viewport.pan.x < pan0.x - 40 })
        #expect(drag.scrolling)
        drag.move(to: CGPoint(x: canvas.viewSize.width / 2, y: 300))
        #expect(await until { !drag.scrolling })
        drag.end(at: CGPoint(x: canvas.viewSize.width / 2, y: 300))
        #expect(await until { (pos(model, a)?.x ?? 0) > 100 })
    }
}
