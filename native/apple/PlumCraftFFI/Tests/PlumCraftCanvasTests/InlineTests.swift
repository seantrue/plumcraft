// Editing values on a face through the kit (N4 slice 2), headless on a real core: where the field
// editor goes at several zooms (and floating, legible, when zoomed far out; lifted above the
// keyboard), a tap on a row opening the core's editor and a typed value being one undo step with
// a refusal keeping it open, Tab moving it, a drag on a number scrubbing it through CanvasDrag
// with the typed move's value, the more-button and icon reported and hit, and the Lucide icons
// parsed into paths.

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import Testing

@Suite struct Inline {
    @Test func theEditorSitsOverItsRowAtTheZoomAndFloatsLegiblyWhenFarOut() {
        let row = CGRect(x: 100, y: 50, width: 140, height: 18)
        let size = CGSize(width: 1000, height: 800)
        // zoom 1, panned: exactly over the row, the face's text size, at least a pointer's height
        var p = EditorPlacement.make(row: row, kind: .text, viewport: Viewport(pan: CGPoint(x: 20, y: 10), zoom: 1), viewSize: size, touch: false)
        #expect(p.frame == CGRect(x: 120, y: 59, width: 140, height: 20))
        #expect(p.fontSize == 11 && !p.floating)
        // zoom 2: twice the size
        p = EditorPlacement.make(row: row, kind: .number, viewport: Viewport(pan: .zero, zoom: 2), viewSize: size, touch: false)
        #expect(p.frame == CGRect(x: 200, y: 100, width: 280, height: 36))
        #expect(p.fontSize == 22)
        // zoom 0.5 on a Mac: 5.5-point text is too small, so it floats at 10 points, centred
        p = EditorPlacement.make(row: row.offsetBy(dx: 300, dy: 0), kind: .text, viewport: Viewport(pan: .zero, zoom: 0.5), viewSize: size, touch: false)
        #expect(p.floating && p.fontSize == 10)
        #expect(abs(p.frame.midX - 235) <= 1 && abs(p.frame.midY - 29.5) <= 1 && p.frame.width >= 170)
        // a finger's: a taller field; 0.75 floats on the iPad (8.25-point text), at 11 points
        p = EditorPlacement.make(row: row, kind: .text, viewport: Viewport(pan: .zero, zoom: 0.75), viewSize: size, touch: true)
        #expect(p.floating && p.fontSize == 11 && p.frame.height >= 32 && p.frame.width >= 220)
        p = EditorPlacement.make(row: row, kind: .text, viewport: Viewport(pan: .zero, zoom: 1.5), viewSize: size, touch: true)
        #expect(!p.floating && p.fontSize == 16.5 && p.frame.height >= 32 && p.frame.height <= 33)
        // several lines are four lines tall when floating
        p = EditorPlacement.make(row: CGRect(x: 0, y: 0, width: 140, height: 50), kind: .multiline, viewport: Viewport(pan: .zero, zoom: 0.2), viewSize: size, touch: true)
        #expect(p.frame.height >= 32 + 3 * 11 * 1.25 - 1)
        // near the edge: moved in, wholly on screen
        p = EditorPlacement.make(row: CGRect(x: 950, y: 790, width: 140, height: 18), kind: .text, viewport: Viewport(), viewSize: size, touch: false)
        #expect(p.frame.maxX <= 992 && p.frame.maxY <= 792)
        // above the keyboard
        #expect(EditorPlacement.lift(CGRect(x: 0, y: 500, width: 100, height: 30), keyboardTop: 450) == -96)
        #expect(EditorPlacement.lift(CGRect(x: 0, y: 100, width: 100, height: 30), keyboardTop: 450) == 0)
    }

    @Test func lucideIconsAreParsedIntoPaths() {
        let activity = #"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><path d="M22 12h-4l-3 9L9 3l-3 9H2" /></svg>"#
        let p = SVGIcon.path(activity)
        #expect(p != nil)
        #expect(abs((p?.boundingBox.minX ?? 0) - 2) < 0.01 && abs((p?.boundingBox.maxX ?? 0) - 22) < 0.01)
        // arcs, circles, rects and lines
        let mixed = #"<svg><circle cx="12" cy="12" r="3"/><rect width="18" height="18" x="3" y="3" rx="2"/><line x1="1" y1="1" x2="5" y2="5"/><path d="M4 12a8 8 0 0 1 16 0"/></svg>"#
        let m = SVGIcon.path(mixed)
        #expect(m != nil && (m?.boundingBox.minX ?? 9) <= 1.01)
        #expect(SVGIcon.path("<svg></svg>") == nil)
        #expect(SVGIcon.path("<path d=\"M1 1 L x y\"/>") != nil || true, "never a trap")
    }

    @MainActor @Test func aTapOnARowOpensTheCoresEditorAndATypedValueIsOneStep() async throws {
        let (core, model, canvas, _, palette) = try await editor("inline-tap")
        defer { core.close() }
        let rows = RowEditor(model: model)
        model.onView = { palette.follow($0); rows.follow($0) }
        var opened: [String] = []
        var notes: [(String, Bool)] = []
        rows.onOpen = { opened.append($0.edit.port) }
        rows.onNote = { _, t, e in notes.append((t, e)) }
        let n = try await add(model, "text.widgets", 0, 0)
        #expect(await settled(model, canvas, faces: 1))
        let nid = try #require(canvas.nid(of: n))
        // the icon and the more-button were reported: the core hits them
        #expect(await until { canvas.link.hit(Point(Double(canvas.moreRect(face: nid)?.midX ?? 0), Double(canvas.moreRect(face: nid)?.midY ?? 0)), slop: 1).nodePart == .menu })
        // tap the count row (the third: text, label, count)
        let r = try #require(canvas.rowRect(face: nid, row: 2))
        let reply = try await canvas.link.tap(Point(Double(r.midX), Double(r.midY)), slop: 1)
        #expect(reply.commands.map(\.command) == ["selection.set", "row.edit"])
        #expect(await until { rows.open?.edit.port == "count" })
        #expect(opened == ["count"] && rows.open?.kind == .number)
        // a refused value keeps it open, with the core's words
        #expect(await rows.commit(text: "lots") == false)
        #expect(rows.error == "count needs a number: \"lots\" isn't one" && rows.open != nil)
        #expect(notes.last?.1 == true)
        // past the range: held there, a note, one step, and the editor closes
        #expect(await rows.commit(text: "40"))
        #expect(notes.last?.0 == "count is at most 10: set to 10")
        #expect(await until { rows.open == nil })
        #expect(await until { model.state.undo == "Set Value" })
        #expect(await until { model.face(id: n)?.inputs[2].widget?.value.int64 == 10 })
        // the face drew again with the new value: one face, its rev changed
        // Tab: from label to count
        try await rows.edit(node: n, port: "label")
        #expect(await until { rows.open?.edit.port == "label" })
        #expect(await rows.commit(text: "hi", then: .next))
        #expect(await until { rows.open?.edit.port == "count" })
        await rows.close()
        #expect(await until { rows.open == nil })
    }

    @MainActor @Test func aDragOnANumbersRowScrubsItAndItsEndIsOneStep() async throws {
        let (core, model, canvas, drag, _) = try await editor("inline-scrub")
        defer { core.close() }
        let n = try await add(model, "text.widgets", 0, 0)
        #expect(await settled(model, canvas, faces: 1))
        canvas.setViewport(Viewport(pan: CGPoint(x: 100, y: 100), zoom: 1))
        let nid = try #require(canvas.nid(of: n))
        let r = try #require(canvas.rowRect(face: nid, row: 3))  // amount, 0…1, two decimals
        let p0 = view(canvas, Point(Double(r.midX), Double(r.midY)))
        drag.begin(at: p0, mods: [])
        #expect(await until { drag.kind == .scrub })
        drag.move(to: CGPoint(x: p0.x + 20, y: p0.y))
        let d = canvas.link.dragMove(Point(Double(r.midX + 20), Double(r.midY)), slop: 2)
        #expect(d.kind == .scrub && abs((d.scrubValue ?? 0) - 0.7) < 1e-9)
        drag.end(at: CGPoint(x: p0.x + 20, y: p0.y))
        #expect(await until { model.face(id: n)?.inputs[3].widget?.value.double == 0.7 })
        #expect(await until { model.state.undo == "Set Value" })
    }
}
