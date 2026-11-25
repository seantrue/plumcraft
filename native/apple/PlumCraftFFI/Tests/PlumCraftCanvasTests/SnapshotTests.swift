// B168: the canvas's picture (`ui.screenshot`'s offscreen one) is the right way up. A bitmap's
// origin is at its bottom left; a layer whose y runs down is drawn through a flip. Before the fix
// the Mac's host, geometry-flipped, was drawn without one, so the whole picture came out upside
// down while the window on screen was right.

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import Testing

/// The picture's pixels as RGBA, rows from the top.
func pixels(_ img: CGImage) -> (w: Int, h: Int, rgba: [UInt8]) {
    let (w, h) = (img.width, img.height)
    var buf = [UInt8](repeating: 0, count: w * h * 4)
    buf.withUnsafeMutableBytes { p in
        if let ctx = CGContext(data: p.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                               space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) {
            ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
        }
    }
    return (w, h, buf)
}

@Suite(.serialized) @MainActor struct Snapshot {
    /// A layer with a red band at its top, by its own geometry: the band is at the picture's top,
    /// for a flipped layer (y down, the Mac canvas's host) and an unflipped one (y up).
    @Test func aLayersTopIsThePicturesTop() throws {
        for flipped in [true, false] {
            let layer = CALayer()
            layer.isGeometryFlipped = flipped
            layer.frame = CGRect(x: 0, y: 0, width: 40, height: 40)
            layer.backgroundColor = CGColor(red: 1, green: 1, blue: 1, alpha: 1)
            let band = CALayer()
            // the top 10 points: y 0 when y runs down, y 30 when it runs up
            band.frame = CGRect(x: 0, y: flipped ? 0 : 30, width: 40, height: 10)
            band.backgroundColor = CGColor(red: 1, green: 0, blue: 0, alpha: 1)
            layer.addSublayer(band)
            let img = try #require(LayerCanvas.snapshot(of: layer, size: CGSize(width: 40, height: 40), scale: 1))
            let p = pixels(img)
            let at = { (x: Int, y: Int) in Array(p.rgba[(y * p.w + x) * 4 ..< (y * p.w + x) * 4 + 3]) }
            let red = { (c: [UInt8]) in c[0] > 200 && c[1] < 90 && c[2] < 90 }
            #expect(red(at(20, 3)), "flipped \(flipped): the top row is red, \(at(20, 3))")
            #expect(!red(at(20, 36)), "flipped \(flipped): the bottom row is not, \(at(20, 36))")
        }
    }

    /// The canvas on a core: a face placed at the view's top left is at the picture's top left.
    @Test func aFaceAtTheTopIsAtThePicturesTop() async throws {
        let (core, model, canvas) = try await canvasOnCore("snapshot-way-up")
        defer { core.close() }
        _ = try await add(model, "text.literal", 0, 0)
        #expect(await settled(model, canvas, faces: 1))
        canvas.viewSize = CGSize(width: 600, height: 600)
        canvas.setViewport(Viewport(pan: CGPoint(x: 20, y: 20), zoom: 1))
        #expect(await until { canvas.idle && !canvas.settlePending })
        let img = try #require(canvas.snapshot(size: canvas.viewSize, scale: 1))
        let p = pixels(img)
        let bg = Array(p.rgba[((p.h - 1) * p.w + p.w - 1) * 4 ..< ((p.h - 1) * p.w + p.w - 1) * 4 + 3])
        func drawn(_ rows: Range<Int>) -> Int {
            var n = 0
            for y in rows { for x in stride(from: 0, to: p.w / 2, by: 2) where Array(p.rgba[(y * p.w + x) * 4 ..< (y * p.w + x) * 4 + 3]) != bg { n += 1 } }
            return n
        }
        let (top, bottom) = (drawn(0 ..< p.h / 2), drawn(p.h / 2 ..< p.h))
        #expect(top > 500 && bottom < top / 10, "the face drawn in the top half: top \(top), bottom \(bottom)")
    }
}
