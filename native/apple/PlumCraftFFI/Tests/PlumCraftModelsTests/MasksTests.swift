// The mask brush's Swift port against the shared vectors (crates/proto/fixtures/masks.json): the
// file's bytes, the pixel hash and the unrounded mask's bits, exactly (decision 2A).

import Foundation
import PlumCraftModels
import Testing

@Suite struct MasksPort {
    /// The vectors' picture: pixel i is [(i % w) * 5 % 256, (i / w) * 7 % 256, i * 3 % 256, 255].
    func picture(_ w: Int, _ h: Int) -> [UInt8] {
        (0..<(w * h)).flatMap { i in [UInt8((i % w) * 5 % 256), UInt8((i / w) * 7 % 256), UInt8(i * 3 % 256), 255] }
    }

    func brush(_ s: JSONValue) -> MaskBrush {
        let d = MaskBrush()
        func num(_ k: String, _ def: Double, _ lo: Double, _ hi: Double) -> Double { s[k].double.map { min(max($0, lo), hi) } ?? def }
        return MaskBrush(tool: s["tool"].string.flatMap(MaskTool.init(rawValue:)) ?? d.tool, radius: num("radius", d.radius, 0.5, 400),
                         hardness: num("hardness", d.hardness, 0, 1), flow: Float(num("flow", Double(d.flow), 0, 1)),
                         density: Float(num("density", Double(d.density), 0, 1)), value: UInt8(num("value", Double(d.value), 0, 255).rounded()))
    }

    @Test func theSharedVectorsHoldBitForBit() throws {
        let v = try fixture("crates/proto/fixtures/masks.json")
        let (w, h) = (Int(v["w"].double ?? 0), Int(v["h"].double ?? 0))
        let cases = try #require(v["cases"].array)
        #expect(cases.count == 11)
        for c in cases {
            var canvas = MaskCanvas(w: w, h: h, picture: picture(w, h))
            for s in c["strokes"].array ?? [] {
                var st = Masks.begin(canvas, brush(s))
                for q in s["points"].array ?? [] {
                    Masks.strokeTo(&canvas, &st, (q[0].double ?? 0, q[1].double ?? 0))
                }
            }
            let name = c["name"].string ?? ""
            let hex = canvas.rgba.map { String(format: "%02x", $0) }.joined()
            #expect(hex == c["rgba"].string, "\(name): rgba")
            #expect(String(format: "%016llx", canvas.pixelHash) == c["pixel_hash"].string, "\(name): pixel hash")
            var bits: UInt64 = 0xcbf2_9ce4_8422_2325
            for m in canvas.mask { bits = (bits ^ UInt64(m.bitPattern)) &* 0x0100_0000_01b3 }
            #expect(String(format: "%016llx", bits) == c["mask_bits"].string, "\(name): mask bits")
        }
    }

    @Test func aStrokeSaysWhatItTouchedAndTheSizeRuleMatchesTheCores() {
        var c = MaskCanvas(w: 48, h: 32, picture: picture(48, 32))
        var s = Masks.begin(c, MaskBrush(radius: 4))
        Masks.strokeTo(&c, &s, (24, 16))
        let d = s.takeDirty()
        #expect(d.map { $0.0 <= 20 && $0.2 >= 28 } == true)
        #expect(s.takeDirty() == nil)
        #expect(s.points == [[24, 16]])
        #expect(Masks.canvasSize(h: 4000, w: 6000) == (1365, 2048))
        #expect(Masks.canvasSize(h: 1280, w: 960) == (1280, 960))
        let back = MaskCanvas(w: 48, h: 32, rgba: c.rgba)
        #expect(back?.pixelHash == c.pixelHash)
        #expect(Masks.alpha(.nan) == 0 && Masks.alpha(2) == 255)
    }
}
