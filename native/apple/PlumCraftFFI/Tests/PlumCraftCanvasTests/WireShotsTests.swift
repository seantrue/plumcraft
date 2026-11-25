// Screenshots of the wires before and after the wire-hitch fix, in one build: the canvas as it is
// (wires in tile pictures) and with `--wire-layers` (a shape layer per wire, as before), at 500
// nodes zoomed out and in, light and dark, and a small graph with a wire being dragged, the wire
// under the pointer, a backward wire's route, net tags and a selected net. Each pair is compared
// pixel by pixel. Runs only with WIRE_SHOTS=DIR (pictures and a summary go there).

import CoreGraphics
import Foundation
import ImageIO
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import Testing

@MainActor
func writePNG(_ image: CGImage, _ path: String) {
    // the right way up since B168 (it was turned over here before the snapshot was)
    let i = image
    guard let d = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, "public.png" as CFString, 1, nil) else { return }
    CGImageDestinationAddImage(d, i, nil)
    CGImageDestinationFinalize(d)
}

@Suite(.serialized) @MainActor struct WireShots {
    nonisolated static let dir = ProcessInfo.processInfo.environment["WIRE_SHOTS"]

    /// Tiles, then layers, at the canvas's present view; both written, and their difference noted.
    func pair(_ canvas: LayerCanvas, _ name: String, _ lines: inout [String], during: (() -> Void)? = nil) async throws {
        guard let dir = Self.dir else { return }
        var shots: [CGImage] = []
        for sw: Set<WireSwitch> in [[], [.layers]] {
            canvas.wireSwitches = sw
            canvas.settle()
            #expect(await until { canvas.idle && !canvas.settlePending })
            during?()
            #expect(await until { canvas.idle })
            let i = try #require(canvas.snapshot(size: canvas.viewSize, scale: 2))
            writePNG(i, "\(dir)/\(name)-\(sw.isEmpty ? "tiles" : "layers").png")
            shots.append(i)
        }
        canvas.wireSwitches = []
        let d = diff(shots[0], shots[1])
        let pct = d.drawn > 0 ? Double(d.differ) / Double(d.drawn) * 100 : 0
        lines.append("| \(name) | \(d.drawn) | \(d.differ) | \(String(format: "%.2f", pct)) |")
    }

    func appearance(_ model: DocumentModel, _ canvas: LayerCanvas, dark: Bool) async throws {
        _ = try await model.core.request("lifecycle.appearance", ["dark": .bool(dark), "contrast": "normal", "reduceMotion": false, "reduceTransparency": false])
        await model.refreshTheme()
        // the canvas has the theme's colours, and every face is drawn in them
        #expect(await until { canvas.colors.isDark == dark && canvas.idle })
    }

    func save(_ lines: [String], _ file: String) {
        guard let dir = Self.dir else { return }
        let head = ["| picture | pixels drawn | pixels differing (over 48 of 255 in a channel) | % of drawn |", "|---|---|---|---|"]
        try? (head + lines).joined(separator: "\n").appending("\n").write(toFile: "\(dir)/\(file)", atomically: true, encoding: .utf8)
    }

    @Test(.enabled(if: dir != nil)) func bigGraph() async throws {
        let (core, model, canvas) = try await canvasOnCore("shots-500")
        defer { core.close() }
        canvas.viewSize = CGSize(width: 834, height: 1210)
        canvas.host.isGeometryFlipped = true
        _ = try await core.request("spike.scene", ["nodes": 500, "wires": 800], doc: model.doc)
        #expect(await until(.seconds(60)) { model.faces.count == 500 && canvas.allMeasured && canvas.idle })
        var lines: [String] = []
        let b = try #require(canvas.contentBounds)
        for dark in [false, true] {
            try await appearance(model, canvas, dark: dark)
            #expect(await until { canvas.idle })
            let tone = dark ? "dark" : "light"
            canvas.fit(b.insetBy(dx: -32, dy: -32), lo: 0.01, hi: 1, animated: false)
            try await pair(canvas, "500-fit-\(tone)", &lines)
            canvas.setViewport(Viewport(pan: CGPoint(x: 417 - b.midX * 0.5, y: 605 - b.midY * 0.5), zoom: 0.5))
            try await pair(canvas, "500-zoom0.5-\(tone)", &lines)
            canvas.setViewport(Viewport(pan: CGPoint(x: 417 - b.midX * 1.5, y: 605 - b.midY * 1.5), zoom: 1.5))
            try await pair(canvas, "500-zoom1.5-\(tone)", &lines)
        }
        save(lines, "summary-500.md")
    }

    @Test(.enabled(if: dir != nil)) func wireFeatures() async throws {
        let (core, model, canvas) = try await canvasOnCore("shots-features")
        defer { core.close() }
        canvas.viewSize = CGSize(width: 1200, height: 800)
        canvas.host.isGeometryFlipped = true
        let a = try await add(model, "text.literal", 40, 60)
        let b = try await add(model, "text.upper", 700, 380)
        let c = try await add(model, "text.upper", 200, 520)
        let d = try await add(model, "text.lower", 760, 60)
        func wire(_ f: String, _ t: String) async throws {
            _ = try await model.run("wire.connect", ["from": ["node": .string(f), "port": "text"], "to": ["node": .string(t), "port": "text"]])
        }
        try await wire(a, b)
        // backward: from b, right, into c, left and below: the core routes it around
        try await wire(b, c)
        try await wire(a, d)
        // a net: the wire into d drawn as tags
        _ = try await model.run("wire.label", ["to": ["node": .string(d), "port": "text"], "name": "loud"])
        #expect(await settled(model, canvas, faces: 4, wires: 3))
        #expect(await until { model.nets.count == 1 && canvas.tagCount == 2 })
        canvas.setViewport(Viewport(pan: .zero, zoom: 1))
        var lines: [String] = []
        for dark in [false, true] {
            try await appearance(model, canvas, dark: dark)
            #expect(await until { canvas.idle })
            let tone = dark ? "dark" : "light"
            try await pair(canvas, "features-\(tone)", &lines)
            // the net selected (its tags filled)
            _ = try await model.run("net.select", ["name": "loud"])
            #expect(await until { model.nets.items.first?.selected == true })
            try await pair(canvas, "net-selected-\(tone)", &lines)
            _ = try await model.run("selection.clear")
            // the wire under the pointer, drawn wider (a point on the a-to-b wire)
            let w = try #require(model.wires.items.first { $0.toFace == b })
            let p = w.path.count >= 4 ? w.path[w.path.count / 2] : w.path[0]
            let h = canvas.link.hit(p, slop: 6)
            #expect(h.kind == .wire)
            try await pair(canvas, "hover-\(tone)", &lines) { canvas.hover(h) }
            canvas.hover(nil)
            // a face moved part way (its wires live), and a new wire being dragged from a pin
            let na = try #require(canvas.nid(of: a))
            try await pair(canvas, "drag-\(tone)", &lines) {
                canvas.endMove(kept: false)
                canvas.beginMove([na])
                canvas.move(dx: 60, dy: 140)
                let pin = canvas.pinPoint(face: c, output: true, pin: 0) ?? .zero
                canvas.showDragWire(from: pin, to: CGPoint(x: pin.x + 260, y: pin.y + 120), fits: 1)
            }
            canvas.showDragWire(from: nil, to: .zero, fits: 0)
            canvas.endMove(kept: false)
            #expect(await until { canvas.idle })
        }
        save(lines, "summary-features.md")
    }
}
