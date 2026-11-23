// The curves value's Swift port against the shared vectors (crates/proto/fixtures/curves.json):
// every y the Rust gives, bit for bit; and the gestures' rules (neighbours, two points kept, hit).

import Foundation
import PlumCraftModels
import Testing

private func bits(_ s: String?) -> Double? { s.flatMap { UInt64($0, radix: 16) }.map(Double.init(bitPattern:)) }

@Suite struct CurvesPort {
    @Test func theSharedVectorsHoldBitForBit() throws {
        let v = try fixture("crates/proto/fixtures/curves.json")
        let cases = try #require(v["cases"].array)
        #expect(cases.count >= 10)
        var n = 0
        for c in cases {
            let pts = try (c["point_bits"].array ?? []).map { p -> CurvesPoint in
                let a = try #require(p.array)
                return CurvesPoint(try #require(bits(a[0].string)), try #require(bits(a[1].string)))
            }
            for s in c["samples"].array ?? [] {
                let a = try #require(s.array)
                let x = try #require(bits(a[2].string)), want = try #require(bits(a[3].string))
                let got = Curves.evaluate(pts, x)
                #expect(got.bitPattern == want.bitPattern, "\(c["name"].string ?? "") at \(x): \(got) not \(want)")
                n += 1
            }
        }
        #expect(n >= 400)
    }

    @Test func aValueParsesAsTheRustReadsIt() throws {
        let s = #"{"master":{"points":[{"x":1,"y":0.9},{"x":0,"y":0.1}],"enabled":true},"red":{"points":[[0,0],[1,1]],"enabled":false}}"#
        let set = Curves.parse(.string(s))
        #expect(set.count == 4)
        #expect(set[0].points == [CurvesPoint(0, 0.1), CurvesPoint(1, 0.9)])
        #expect(!set[1].enabled && set[2].isIdentity)
        let unread = Curves.parse(.string("not json")).filter { !$0.isIdentity }
        #expect(unread.isEmpty)
        let comfy = Curves.parse(.object(["points": .array([.array([.double(0), .double(0.2)]), .array([.double(1), .double(1)])])]))
        #expect(comfy[0].points.first == CurvesPoint(0, 0.2))
        // the lookup table: master then red
        let lut = Curves.lut8(set, 0)
        #expect(lut[0] == 26 && lut[255] == 230)
    }

    @Test func gesturesKeepNeighboursAndTwoPoints() {
        var c = CurvesCurve.identity
        let i = Curves.insert(&c, 0.5, 0.7)
        #expect(i == 1 && c.points.count == 3)
        Curves.move(&c, 1, 2.0, -1)
        #expect(c.points[1].x == 1 - Curves.minGap && c.points[1].y == 0)
        Curves.move(&c, 1, -3, 0.5)
        #expect(c.points[1].x == Curves.minGap)
        #expect(Curves.hit(c, 0.01, 0.5, reach: 0.05) == 1)
        #expect(Curves.hit(c, 0.5, 0.5, reach: 0.05) == nil)
        #expect(Curves.remove(&c, 1))
        #expect(!Curves.remove(&c, 0), "two points are kept")
        for k in 0..<70 { _ = Curves.insert(&c, Double(k) / 70, 0.5) }
        #expect(c.points.count == Curves.editMaxPoints)
    }
}
