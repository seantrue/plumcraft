// The mask brush, for drawing a stroke as it is painted (docs/shell-interface.md 12.22): the Rust
// model's maths (plumcraft_proto::masks) ported to Swift, the same arithmetic in the same order,
// so the stroke a finger lays is the stroke the core keeps (the maintainer, 2026-11-23, decision
// 2A). Pinned by the shared vectors (crates/proto/fixtures/masks.json) in `swift test`: the file's
// bytes, the pixel hash and the unrounded mask's bits. The stroke itself still goes to the core as
// one `mask.stroke` when it lifts; the core's file is what the node reads.

import Foundation

public enum MaskTool: String, Sendable, CaseIterable {
    case mask, erase, paint
}

public struct MaskBrush: Hashable, Sendable {
    public var tool: MaskTool
    public var radius: Double
    public var hardness: Double
    public var flow: Float
    public var density: Float
    public var value: UInt8

    public init(tool: MaskTool = .mask, radius: Double = 24, hardness: Double = 0.5, flow: Float = 0.6, density: Float = 1, value: UInt8 = 0) {
        self.tool = tool
        self.radius = radius
        self.hardness = hardness
        self.flow = flow
        self.density = density
        self.value = value
    }

    public static let maxRadius = 400.0
    public static let minRadius = 0.5
}

/// An edit: the picture as painted and the mask, `w × h`, row by row.
public struct MaskCanvas: Sendable {
    public var w: Int
    public var h: Int
    public var rgb: [UInt8]
    public var mask: [Float]

    /// A clean canvas on `picture` (RGBA bytes), starting from `base` (one value a pixel) if given.
    public init(w: Int, h: Int, picture: [UInt8], base: [Float]? = nil) {
        self.w = max(w, 0)
        self.h = max(h, 0)
        let n = self.w * self.h
        rgb = [UInt8](repeating: 0, count: n * 3)
        for i in 0..<n where i * 4 + 2 < picture.count {
            rgb[i * 3] = picture[i * 4]
            rgb[i * 3 + 1] = picture[i * 4 + 1]
            rgb[i * 3 + 2] = picture[i * 4 + 2]
        }
        mask = [Float](repeating: 0, count: n)
        if let base {
            for i in 0..<min(n, base.count) { mask[i] = base[i].isFinite ? min(max(base[i], 0), 1) : 0 }
        }
    }

    /// An edit read back from its file's RGBA bytes (alpha the mask).
    public init?(w: Int, h: Int, rgba: [UInt8]) {
        guard w > 0, h > 0, rgba.count == w * h * 4 else { return nil }
        self.w = w
        self.h = h
        rgb = []
        rgb.reserveCapacity(w * h * 3)
        mask = []
        mask.reserveCapacity(w * h)
        for i in 0..<(w * h) {
            rgb.append(contentsOf: rgba[(i * 4)..<(i * 4 + 3)])
            mask.append(Float(rgba[i * 4 + 3]) / 255)
        }
    }

    /// The file's bytes: RGBA, alpha the mask rounded to 8 bits.
    public var rgba: [UInt8] {
        var out = [UInt8]()
        out.reserveCapacity(w * h * 4)
        for i in 0..<(w * h) {
            out.append(contentsOf: [rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2], Masks.alpha(mask[i])])
        }
        return out
    }

    /// The hash an edit file's name carries.
    public var pixelHash: UInt64 {
        let prime: UInt64 = 0x0100_0000_01b3
        var hv = ((0xcbf2_9ce4_8422_2325 as UInt64) ^ UInt64(w)) &* prime ^ UInt64(h)
        for i in 0..<(w * h) {
            let word = UInt32(rgb[i * 3]) | UInt32(rgb[i * 3 + 1]) << 8 | UInt32(rgb[i * 3 + 2]) << 16 | UInt32(Masks.alpha(mask[i])) << 24
            hv = (hv ^ UInt64(word)) &* prime
        }
        return hv ^ (hv >> 32)
    }
}

/// A stroke in progress: its dabs so far, its coverage, the canvas as it began, and the area its
/// dabs have touched (for a redraw of only that).
public struct MaskStroke: Sendable {
    public let brush: MaskBrush
    var prev: (Double, Double)?
    var dabs: [(Double, Double)] = []
    var pressures: [Float] = []
    var coverage: [Float]
    let base: MaskCanvas
    /// The pixels touched since `takeDirty()` (x0, y0, x1, y1), inclusive.
    public var dirty: (Int, Int, Int, Int)?
    /// The points given, for the command sent on lift.
    public fileprivate(set) var points: [[Double]] = []

    public mutating func takeDirty() -> (Int, Int, Int, Int)? {
        defer { dirty = nil }
        return dirty
    }
}

public enum Masks {
    static let stationaryPx = 1e-6

    /// Rust's `f32::clamp(0, 1)`.
    static func unit(_ v: Float) -> Float { v < 0 ? 0 : (v > 1 ? 1 : v) }

    /// Rust's `(m.clamp(0, 1) * 255 + 0.5) as u8`: saturating, NaN to 0.
    public static func alpha(_ m: Float) -> UInt8 {
        let v = unit(m) * 255 + 0.5
        return v.isNaN ? 0 : UInt8(min(max(v, 0), 255).rounded(.towardZero))
    }

    static func dist(_ a: (Double, Double), _ b: (Double, Double)) -> Double {
        let dx = a.0 - b.0, dy = a.1 - b.1
        return (dx * dx + dy * dy).squareRoot()
    }

    static func smooth(_ e0: Float, _ e1: Float, _ x: Float) -> Float {
        let t = unit((x - e0) / (e1 - e0))
        return t * t * (3.0 - 2.0 * t)
    }

    public static func dabSpacing(_ r: Double) -> Double { max(r / 4.0, 0.5) }

    static func pushDabs(_ dabs: inout [(Double, Double)], _ prev: (Double, Double)?, _ q: (Double, Double), _ r: Double) -> Bool {
        guard q.0.isFinite, q.1.isFinite else { return false }
        if let prev {
            let d = dist(prev, q)
            if d.isNaN || d <= stationaryPx { return false }
            let nf = (d / dabSpacing(r)).rounded(.up)
            let n = nf.isFinite ? Int(min(max(nf, 0), 1_000_000)) : 0
            if n > 1 {
                for k in 1..<n {
                    let t = Double(k) / Double(n)
                    dabs.append((prev.0 + (q.0 - prev.0) * t, prev.1 + (q.1 - prev.1) * t))
                }
            }
        }
        dabs.append(q)
        return true
    }

    static func dabAlpha(_ dd: Double, _ r: Double, _ hard: Double) -> Float {
        if dd > r { return 0 }
        if dd <= hard { return 1 }
        return 1.0 - smooth(Float(hard), Float(r), Float(dd))
    }

    static func accumulate(_ v: Float, _ a: Float, _ flow: Float, _ density: Float) -> Float {
        min(v + a * flow * (1.0 - v), density)
    }

    static func pressureRadius(_ r: Double, _ p: Float) -> Double {
        let p = p.isNaN ? 1.0 : Double(unit(p))
        return min(max(r * (0.1 + 0.9 * p), 0.5), max(r, 0.5))
    }

    static func pressureFlow(_ flow: Float, _ p: Float) -> Float {
        flow * (p.isNaN ? 1 : unit(p))
    }

    /// Rust's `f64::clamp(lo, hi)` for a finite value.
    static func clamp(_ v: Double, _ lo: Double, _ hi: Double) -> Double { v < lo ? lo : (v > hi ? hi : v) }

    public static func begin(_ canvas: MaskCanvas, _ brush: MaskBrush) -> MaskStroke {
        MaskStroke(brush: brush, coverage: [Float](repeating: 0, count: canvas.mask.count), base: canvas)
    }

    /// Stroke to `q` (canvas pixels) at full pressure; repaints what its new dabs touch.
    public static func strokeTo(_ c: inout MaskCanvas, _ s: inout MaskStroke, _ q: (Double, Double)) {
        let p: Float = 1
        s.points.append([q.0, q.1])
        let before = s.dabs.count
        let r = max(s.brush.radius, 0.5)
        guard pushDabs(&s.dabs, s.prev, q, r) else { return }
        let from = s.pressures.last ?? p
        let added = s.dabs.count - before
        for k in stride(from: 1, through: added, by: 1) {
            s.pressures.append(from + (p - from) * Float(k) / Float(added))
        }
        s.prev = q
        for i in before..<s.dabs.count {
            let rr = pressureRadius(r, s.pressures[i])
            let hard = rr * clamp(s.brush.hardness, 0, 1)
            let flow = pressureFlow(unit(s.brush.flow), s.pressures[i])
            dab(&c, &s, s.dabs[i], rr, hard, flow)
        }
    }

    static func dab(_ c: inout MaskCanvas, _ s: inout MaskStroke, _ ctr: (Double, Double), _ r: Double, _ hard: Double, _ flow: Float) {
        let (w, h) = (Double(c.w), Double(c.h))
        if ctr.0 + r < 0 || ctr.1 + r < 0 || ctr.0 - r > w || ctr.1 - r > h { return }
        // Rust's `as usize` saturates: a negative or NaN bound is 0
        func idx(_ v: Double) -> Int { v.isNaN || v <= 0 ? 0 : (v >= Double(Int.max) ? Int.max : Int(v)) }
        let x0 = idx(max((ctr.0 - r).rounded(.down), 0)), y0 = idx(max((ctr.1 - r).rounded(.down), 0))
        let x1 = idx(max(min((ctr.0 + r).rounded(.up), w - 1), 0)), y1 = idx(max(min((ctr.1 + r).rounded(.up), h - 1), 0))
        guard x0 <= x1, y0 <= y1 else { return }
        let density = unit(s.brush.density)
        for y in y0...y1 {
            for x in x0...x1 {
                let i = y * c.w + x
                guard i < s.coverage.count, i < s.base.mask.count else { continue }
                let a = dabAlpha(dist((Double(x) + 0.5, Double(y) + 0.5), ctr), r, hard)
                if a <= 0 { continue }
                let v = accumulate(s.coverage[i], a, flow, density)
                s.coverage[i] = v
                let bm = s.base.mask[i]
                switch s.brush.tool {
                case .mask: c.mask[i] = bm + v * (1.0 - bm)
                case .erase: c.mask[i] = bm * (1.0 - v)
                case .paint:
                    let g = Float(s.brush.value)
                    for k in 0..<3 {
                        let o = Float(s.base.rgb[i * 3 + k])
                        c.rgb[i * 3 + k] = UInt8(min(max((o * (1.0 - v) + g * v).rounded(.toNearestOrAwayFromZero), 0), 255))
                    }
                    c.mask[i] = bm + v * (1.0 - bm)
                }
            }
        }
        if let d = s.dirty {
            s.dirty = (min(d.0, x0), min(d.1, y0), max(d.2, x1), max(d.3, y1))
        } else {
            s.dirty = (x0, y0, x1, y1)
        }
    }

    /// The mask's size for a picture of `h × w` (the core's `canvas_size`).
    public static func canvasSize(h: Int, w: Int) -> (h: Int, w: Int) {
        let scale = max(Double(max(h, w)) / 2048.0, 1.0)
        return (max(Int((Double(h) / scale).rounded(.toNearestOrAwayFromZero)), 1), max(Int((Double(w) / scale).rounded(.toNearestOrAwayFromZero)), 1))
    }
}
