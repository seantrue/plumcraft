// Wires drawn into tile pictures (the wire-hitch work): no wire is a graph-sized layer, a moved or
// picked-up wire leaves the tiles for a layer of its own and comes back, and the tiles draw what
// the per-wire shape layers drew.

import CoreGraphics
import Foundation
import ImageIO
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import Testing

/// Pixels of two snapshots that differ by more than `tol` in a channel, and the pixels that
/// differ from the background in either (the drawn ones).
func diff(_ a: CGImage, _ b: CGImage, tol: Int = 48) -> (differ: Int, drawn: Int) {
    func bytes(_ i: CGImage) -> [UInt8] {
        var d = [UInt8](repeating: 0, count: i.width * i.height * 4)
        let ctx = CGContext(data: &d, width: i.width, height: i.height, bitsPerComponent: 8, bytesPerRow: i.width * 4,
                            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        ctx?.draw(i, in: CGRect(x: 0, y: 0, width: i.width, height: i.height))
        return d
    }
    let x = bytes(a), y = bytes(b)
    let bg = Array(x[0..<4])
    var differ = 0, drawn = 0
    for k in stride(from: 0, to: min(x.count, y.count), by: 4) {
        var d = false, n = false
        for c in 0..<3 {
            if abs(Int(x[k + c]) - Int(y[k + c])) > tol { d = true }
            if abs(Int(x[k + c]) - Int(bg[c])) > 8 || abs(Int(y[k + c]) - Int(bg[c])) > 8 { n = true }
        }
        if d { differ += 1 }
        if n { drawn += 1 }
    }
    return (differ, drawn)
}

@Suite(.serialized) @MainActor struct WireTiles {
    /// Two faces far apart and a wire across, the bench scene's kind.
    func longWire(_ name: String) async throws -> (PlumCore, DocumentModel, LayerCanvas, UInt64) {
        let (core, model, canvas) = try await canvasOnCore(name)
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 2600, 900)
        let c = try await add(model, "text.upper", 300, 500)
        _ = try await model.run("wire.connect", ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "text"]])
        _ = try await model.run("wire.connect", ["from": ["node": .string(b), "port": "text"], "to": ["node": .string(c), "port": "text"]])
        #expect(await settled(model, canvas, faces: 3, wires: 2))
        let w = try #require(model.wires.items.first { $0.toFace == b })
        return (core, model, canvas, w.nid)
    }

    @Test func noWireIsALayerBiggerThanATile() async throws {
        let (core, model, canvas, nid) = try await longWire("tiles-size")
        defer { core.close() }
        canvas.setViewport(Viewport(pan: .zero, zoom: 0.4))
        #expect(await until { canvas.idle && !canvas.settlePending })
        #expect(canvas.wireShown(nid: nid))
        #expect(canvas.liveWireLayer(nid: nid) == nil)
        let s = canvas.wireStats()
        #expect(s.tiles > 0 && s.tiles == s.inView, "only pictures: \(s)")
        // a tile is 512 units; at 0.4 a 1200 by 800 view holds about 4.7 by 3.1 of them
        #expect(s.largest < 0.1, "\(s)")
        #expect(model.wires.count == canvas.wireLayerCount)
    }

    @Test func aMovedFacesWiresLeaveTheTilesAndComeBack() async throws {
        let (core, model, canvas, nid) = try await longWire("tiles-move")
        defer { core.close() }
        canvas.setViewport(Viewport(pan: .zero, zoom: 1))
        #expect(await until { canvas.idle && !canvas.settlePending })
        let from = try #require(model.wires[nid]?.item.fromFace)
        let a = try #require(canvas.nid(of: from))
        canvas.beginMove([a])
        let l = try #require(canvas.liveWireLayer(nid: nid))
        // clipped to the view and its margin: never the wire's 2,600 units
        #expect(l.masksToBounds && l.bounds.width <= 1200 / 1 * 2.4 + 1, "\(l.frame)")
        canvas.move(dx: 40, dy: 30)
        #expect(await until { canvas.idle })
        #expect(canvas.liveWireLayer(nid: nid) != nil, "still its own while the drag goes on")
        canvas.endMove(kept: false)
        // back in the tiles, its own layer gone once they are up
        #expect(await until { canvas.idle && canvas.liveWireLayer(nid: nid) == nil })
        #expect(canvas.wireShown(nid: nid))
        #expect(canvas.wireStats().tiles == canvas.wireStats().inView)
    }

    @Test func aPickedUpWireIsFaintOnItsOwnThenBack() async throws {
        let (core, _, canvas, nid) = try await longWire("tiles-pick")
        defer { core.close() }
        canvas.pickUp(wire: nid)
        let l = try #require(canvas.liveWireLayer(nid: nid))
        #expect(l.opacity < 0.2)
        canvas.pickUp(wire: nil)
        #expect(await until { canvas.idle && canvas.liveWireLayer(nid: nid) == nil })
        #expect(canvas.wireShown(nid: nid))
    }

    @Test func tilesDrawWhatTheShapeLayersDrew() async throws {
        let (core, _, canvas, _) = try await longWire("tiles-pixels")
        defer { core.close() }
        let size = canvas.viewSize
        // y down, as both apps' hosts have it (an NSView's flipped layer; a UIView's own), so the
        // faces' and tiles' pictures show the right way up
        canvas.host.isGeometryFlipped = true
        for z: CGFloat in [0.35, 1, 2] {
            canvas.setViewport(Viewport(pan: CGPoint(x: 40, y: 40), zoom: z))
            canvas.wireSwitches = []
            canvas.settle()
            #expect(await until { canvas.idle && !canvas.settlePending })
            let tiles = try #require(canvas.snapshot(size: size, scale: 2))
            canvas.wireSwitches = [.layers]
            #expect(await until { canvas.idle && !canvas.settlePending })
            let layers = try #require(canvas.snapshot(size: size, scale: 2))
            canvas.wireSwitches = []
            // WIRE_TILES_DUMP=DIR: both pictures, to look at
            if let dir = ProcessInfo.processInfo.environment["WIRE_TILES_DUMP"] {
                for (n, i) in [("tiles", tiles), ("layers", layers)] {
                    let u = URL(fileURLWithPath: dir).appendingPathComponent("\(n)-\(z).png") as CFURL
                    if let dst = CGImageDestinationCreateWithURL(u, "public.png" as CFString, 1, nil) {
                        CGImageDestinationAddImage(dst, i, nil)
                        CGImageDestinationFinalize(dst)
                    }
                }
            }
            let d = diff(tiles, layers)
            #expect(d.drawn > 1000)
            // antialiasing may differ at an edge; nothing more
            #expect(Double(d.differ) < Double(d.drawn) * 0.01, "zoom \(z): \(d.differ) of \(d.drawn) drawn pixels differ")
        }
    }
}
