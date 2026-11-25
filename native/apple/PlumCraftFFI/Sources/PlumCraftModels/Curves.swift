// A curves value, for drawing (docs/shell-interface.md 12.21): the Rust model's maths
// (plumcraft_proto::curves) ported to Swift, so the curve drawn is the curve applied. Pinned to the
// Rust by the shared vectors (crates/proto/fixtures/curves.json), compared by bit pattern in
// `swift test`. Every edit still goes to the core as a command (`curves.edit`, …): this only draws,
// hit-tests and shapes a gesture's points before they are sent.

import Foundation

/// One curve: its points sorted by x (each in 0..1), and whether it is on.
public struct CurvesCurve: Hashable, Sendable {
    public var points: [CurvesPoint]
    public var enabled: Bool

    public init(points: [CurvesPoint], enabled: Bool = true) {
        self.points = points
        self.enabled = enabled
    }

    public static let identity = CurvesCurve(points: [CurvesPoint(0, 0), CurvesPoint(1, 1)])

    /// Two points, (0, 0) and (1, 1).
    public var isIdentity: Bool { points == CurvesCurve.identity.points }
    /// Does it change values?
    public var applies: Bool { enabled && !points.isEmpty && !isIdentity }

    /// The curve at `x`, or `x` when it doesn't apply.
    public func at(_ x: Double) -> Double { applies ? Curves.evaluate(points, x) : x }
}

public struct CurvesPoint: Hashable, Sendable {
    public var x: Double
    public var y: Double
    public init(_ x: Double, _ y: Double) {
        self.x = x
        self.y = y
    }
}

public enum Curves {
    /// The channels' keys, in the order channels take them.
    public static let channelKeys = ["master", "red", "green", "blue"]
    /// The most points an edit leaves on a curve (the core refuses more).
    public static let editMaxPoints = 64
    /// The least x between neighbouring points an edit leaves.
    public static let minGap = 1e-3
    /// The presets the core's `curves.preset` takes, with what each does.
    public static let presets: [(name: String, help: String)] = [
        ("Light S", "A little more contrast"), ("Medium S", "More contrast"), ("Strong S", "Much more contrast"), ("Inverse S", "Less contrast"),
    ]

    /// Rust's `f64::clamp(0, 1)`: NaN stays NaN.
    static func unit(_ v: Double) -> Double { v < 0 ? 0 : (v > 1 ? 1 : v) }

    /// Each point's slope: the secant through its neighbours (the end segments' own at the ends;
    /// 0 where they share an x).
    static func slopes(_ xs: [Double], _ ys: [Double]) -> [Double] {
        let n = xs.count
        return (0..<n).map { i in
            let lo = max(i - 1, 0), hi = min(i + 1, n - 1)
            let dx = xs[hi] - xs[lo]
            return dx != 0 ? (ys[hi] - ys[lo]) / dx : 0
        }
    }

    /// The curve through `points` (sorted by x) at `x`: cubic Hermite segments with secant
    /// slopes, clamped to 0..1, flat beyond the end points; one point is its y everywhere, none
    /// leaves `x`. The same arithmetic in the same order as the Rust, so the two agree bit for bit.
    public static func evaluate(_ points: [CurvesPoint], _ x: Double) -> Double {
        if points.isEmpty { return x }
        if points.count == 1 { return points[0].y }
        let xs = points.map(\.x), ys = points.map(\.y)
        let m = slopes(xs, ys)
        let n = xs.count
        // the first index whose x isn't below `x` (Rust's partition_point)
        var found = n
        if !x.isNaN {
            var lo = 0, hi = n
            while lo < hi {
                let mid = (lo + hi) / 2
                if xs[mid] < x { lo = mid + 1 } else { hi = mid }
            }
            found = lo
        }
        let i = min(max(found - 1, 0), n - 2)
        let (x1, x2, y1, y2, m1, m2) = (xs[i], xs[i + 1], ys[i], ys[i + 1], m[i], m[i + 1])
        let h = x2 - x1
        let safe = h == 0 ? 1.0 : h
        let t = (x - x1) / safe
        let t2 = t * t
        let t3 = t2 * t
        let a = (2.0 * t3 - 3.0 * t2 + 1.0) * y1
        let b = (t3 - 2.0 * t2 + t) * h * m1
        let c = (-2.0 * t3 + 3.0 * t2) * y2
        let d = (t3 - t2) * h * m2
        var y = a + b + c + d
        if h == 0 { y = y2 }
        y = unit(y)
        if x <= xs[0] { y = ys[0] }
        if x >= xs[n - 1] { y = ys[n - 1] }
        return y
    }

    /// Channel `k` (0, 1, 2; a fourth and beyond take the master only) at `x`.
    public static func apply(_ set: [CurvesCurve], _ k: Int, _ x: Double) -> Double {
        let y = set.first?.at(x) ?? x
        return (k < 3 && k + 1 < set.count) ? set[k + 1].at(y) : y
    }

    /// An 8-bit lookup table for channel `k`: entry i is the curve at i/255, rounded to 8 bits.
    public static func lut8(_ set: [CurvesCurve], _ k: Int) -> [UInt8] {
        (0..<256).map { i in
            let y = Float(apply(set, k, Double(i) / 255.0))
            return UInt8(min(max(y, 0), 1) * 255 + 0.5)
        }
    }

    /// The four curves of a value: the JSON object or that object as a string; a missing curve is
    /// the identity; ComfyUI's `{"points": …}` is the master curve. Unreadable: all identity.
    public static func parse(_ value: JSONValue) -> [CurvesCurve] {
        var v = value
        if let s = value.string {
            v = (s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : try? JSONValue.parse(Data(s.utf8))) ?? .null
        }
        var set = Array(repeating: CurvesCurve.identity, count: 4)
        guard let o = v.object else { return set }
        if o["points"] != nil && o["master"] == nil {
            set[0] = curve(.object(["points": o["points"] ?? .null, "enabled": true]))
            return set
        }
        for (k, key) in channelKeys.enumerated() {
            if let c = o[key], c.object != nil { set[k] = curve(c) }
        }
        return set
    }

    static func curve(_ c: JSONValue) -> CurvesCurve {
        let pts: [CurvesPoint] = (c["points"].array ?? []).prefix(4096).compactMap { p in
            if let a = p.array, a.count >= 2, let x = a[0].double, let y = a[1].double { return CurvesPoint(x, y) }
            if p.object != nil { return CurvesPoint(p["x"].double ?? 0, p["y"].double ?? 0) }
            return nil
        }
        // a stable sort by x, as the Rust's
        let sorted = pts.enumerated().sorted { $0.element.x != $1.element.x ? $0.element.x < $1.element.x : $0.offset < $1.offset }.map(\.element)
        return CurvesCurve(points: sorted, enabled: c["enabled"].bool ?? true)
    }

    // MARK: Gestures (the view's; the result goes to the core as one `curves.edit`)

    /// Add a point at (x, y) (each held to 0..1) after the points at or before its x: its index,
    /// or nil when the curve has `editMaxPoints`.
    public static func insert(_ c: inout CurvesCurve, _ x: Double, _ y: Double) -> Int? {
        guard c.points.count < editMaxPoints, x.isFinite, y.isFinite else { return nil }
        let p = CurvesPoint(unit(x), unit(y))
        let i = c.points.firstIndex { $0.x > p.x } ?? c.points.count
        c.points.insert(p, at: i)
        return i
    }

    /// Move point `i` to (x, y): x kept between its neighbours, `minGap` apart.
    public static func move(_ c: inout CurvesCurve, _ i: Int, _ x: Double, _ y: Double) {
        let n = c.points.count
        guard i >= 0, i < n, x.isFinite, y.isFinite else { return }
        let lo = i > 0 ? c.points[i - 1].x + minGap : 0
        let hi = i + 1 < n ? c.points[i + 1].x - minGap : 1
        let nx = lo <= hi ? min(max(x, lo), hi) : c.points[i].x
        c.points[i] = CurvesPoint(unit(nx), unit(y))
    }

    /// Remove point `i`, unless only two are left.
    @discardableResult
    public static func remove(_ c: inout CurvesCurve, _ i: Int) -> Bool {
        guard c.points.count > 2, i >= 0, i < c.points.count else { return false }
        c.points.remove(at: i)
        return true
    }

    /// The nearest point to (x, y) within `reach` (the plot's 0..1 units).
    public static func hit(_ c: CurvesCurve, _ x: Double, _ y: Double, reach: Double) -> Int? {
        var best: (Int, Double)?
        for (i, p) in c.points.enumerated() {
            let d = ((p.x - x) * (p.x - x) + (p.y - y) * (p.y - y)).squareRoot()
            if d <= reach, best.map({ d < $0.1 }) ?? true { best = (i, d) }
        }
        return best?.0
    }

    /// The histogram a tab shows: its channel's (1, 2, 3), or the channels' mean for All (0).
    public static func tabHistogram(_ histograms: [[Double]], _ tab: Int) -> [Double] {
        if tab == 0 {
            guard !histograms.isEmpty else { return [] }
            let n = histograms.map(\.count).max() ?? 0
            return (0..<n).map { i in histograms.reduce(0) { $0 + ($1.count > i ? $1[i] : 0) } / Double(histograms.count) }
        }
        return tab - 1 < histograms.count ? histograms[tab - 1] : []
    }
}
