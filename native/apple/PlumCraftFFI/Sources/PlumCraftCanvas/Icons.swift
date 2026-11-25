// The node icons (N4 slice 2): the Lucide set the core embeds (`icon.get`, `plumcraft_editor_core::
// icons`), the same icons the egui editor draws, parsed once into Core Graphics paths in their
// 24-unit box and stroked into a face's header at its size. A pack's own icon is never replaced by
// an SF Symbol; a name the set lacks comes back as the core's fallback.
//
// The parser takes what Lucide uses: <path d>, <circle>, <ellipse>, <rect> (with rx), <line>,
// <polyline> and <polygon>, and path data's M L H V C S Q T A Z in both cases. Anything else is
// skipped; a malformed number ends that element, never a trap (the SVG is the core's, but every
// message from the other side is hostile input).

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftModels
import Synchronization

/// An icon's outline, in its 24 × 24 box. CGPath is immutable and thread-safe.
public struct IconPath: @unchecked Sendable {
    public let path: CGPath
}

/// The icons fetched so far, by the name a face asks for: any thread reads, the canvas fills.
public final class IconStore: Sendable {
    private let state = Mutex<[String: IconPath]>([:])
    private let asked = Mutex<Set<String>>([])

    public init() {}

    public func path(_ name: String) -> IconPath? { state.withLock { $0[name] } }
    public var all: [String: IconPath] { state.withLock { $0 } }

    /// The names of `names` not fetched nor being fetched, marked as being fetched.
    public func claim(_ names: Set<String>) -> [String] {
        asked.withLock { a in
            let new = names.subtracting(a)
            a.formUnion(new)
            return Array(new).sorted()
        }
    }

    /// Fetch `names` from the core (`icon.get`); the ones parsed are kept.
    public func fetch(_ names: [String], core: PlumCore, doc: UInt64?) async -> Int {
        guard !names.isEmpty, let r = try? await core.request("icon.get", ["names": .array(names.map { .string($0) })], doc: doc),
              let icons = r["icons"].object else { return 0 }
        var got: [String: IconPath] = [:]
        for (name, v) in icons {
            if let svg = v["svg"].string, let p = SVGIcon.path(svg) { got[name] = IconPath(path: p) }
        }
        state.withLock { s in s.merge(got) { _, n in n } }
        return got.count
    }

    /// Put an icon in by hand (tests).
    public func put(_ name: String, svg: String) {
        if let p = SVGIcon.path(svg) { state.withLock { $0[name] = IconPath(path: p) } }
    }
}

/// Lucide's SVG subset as one CGPath.
public enum SVGIcon {
    public static func path(_ svg: String) -> CGPath? {
        let p = CGMutablePath()
        var rest = Substring(svg)
        while let open = rest.firstIndex(of: "<") {
            rest = rest[rest.index(after: open)...]
            guard let close = rest.firstIndex(of: ">") else { break }
            let tag = rest[..<close]
            rest = rest[rest.index(after: close)...]
            let name = tag.prefix { $0.isLetter }
            let a = attributes(tag.dropFirst(name.count))
            let n = { (k: String) -> CGFloat? in a[k].flatMap { Double($0.trimmingCharacters(in: .whitespaces)) }.map { CGFloat($0) } }
            switch name {
            case "path":
                if let d = a["d"] { PathData.add(d, to: p) }
            case "circle":
                if let cx = n("cx"), let cy = n("cy"), let r = n("r"), r > 0 { p.addEllipse(in: CGRect(x: cx - r, y: cy - r, width: 2 * r, height: 2 * r)) }
            case "ellipse":
                if let cx = n("cx"), let cy = n("cy"), let rx = n("rx"), let ry = n("ry") { p.addEllipse(in: CGRect(x: cx - rx, y: cy - ry, width: 2 * rx, height: 2 * ry)) }
            case "rect":
                if let w = n("width"), let h = n("height"), w > 0, h > 0 {
                    let r = CGRect(x: n("x") ?? 0, y: n("y") ?? 0, width: w, height: h)
                    let rx = min(n("rx") ?? n("ry") ?? 0, w / 2, h / 2)
                    p.addPath(CGPath(roundedRect: r, cornerWidth: rx, cornerHeight: rx, transform: nil))
                }
            case "line":
                if let x1 = n("x1"), let y1 = n("y1"), let x2 = n("x2"), let y2 = n("y2") {
                    p.move(to: CGPoint(x: x1, y: y1))
                    p.addLine(to: CGPoint(x: x2, y: y2))
                }
            case "polyline", "polygon":
                let v = PathData.numbers(a["points"] ?? "")
                var k = 0
                while k + 1 < v.count {
                    let pt = CGPoint(x: v[k], y: v[k + 1])
                    if k == 0 { p.move(to: pt) } else { p.addLine(to: pt) }
                    k += 2
                }
                if name == "polygon", v.count >= 4 { p.closeSubpath() }
            default:
                break
            }
        }
        return p.isEmpty ? nil : p
    }

    /// `k="v"` pairs of a tag.
    static func attributes(_ s: Substring) -> [String: String] {
        var out: [String: String] = [:]
        var rest = s
        while let eq = rest.firstIndex(of: "=") {
            let key = rest[..<eq].trimmingCharacters(in: .whitespacesAndNewlines)
            rest = rest[rest.index(after: eq)...]
            guard let q = rest.first, q == "\"" || q == "'" else { break }
            rest = rest.dropFirst()
            guard let end = rest.firstIndex(of: q) else { break }
            out[key] = String(rest[..<end])
            rest = rest[rest.index(after: end)...]
        }
        return out
    }
}

/// SVG path data (`d`) into a CGPath.
enum PathData {
    /// The numbers in `s` (commas or spaces between, a sign or a second point starting the next).
    static func numbers(_ s: String) -> [CGFloat] {
        var out: [CGFloat] = []
        var cur = ""
        func flush() {
            if let v = Double(cur) { out.append(CGFloat(v)) }
            cur = ""
        }
        for c in s {
            if c == "-" || c == "+" {
                if !cur.isEmpty && !(cur.last == "e" || cur.last == "E") { flush() }
                cur.append(c)
            } else if c == "." {
                if cur.contains(".") && !cur.contains("e") { flush() }
                cur.append(c)
            } else if c.isNumber || c == "e" || c == "E" {
                cur.append(c)
            } else {
                flush()
            }
        }
        flush()
        return out
    }

    static func add(_ d: String, to p: CGMutablePath) {
        // split into commands and their numbers
        var cmds: [(Character, [CGFloat])] = []
        var at = d.startIndex
        while at < d.endIndex {
            let c = d[at]
            if c.isLetter && c != "e" && c != "E" {
                var end = d.index(after: at)
                while end < d.endIndex, !(d[end].isLetter && d[end] != "e" && d[end] != "E") { end = d.index(after: end) }
                cmds.append((c, numbers(String(d[d.index(after: at)..<end]))))
                at = end
            } else {
                at = d.index(after: at)
            }
        }
        var cur = CGPoint.zero
        var start = CGPoint.zero
        var lastC: CGPoint?  // the last cubic's second control point (S)
        var lastQ: CGPoint?  // the last quadratic's control point (T)
        for (c, v) in cmds {
            let rel = c.isLowercase
            func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint { rel ? CGPoint(x: cur.x + x, y: cur.y + y) : CGPoint(x: x, y: y) }
            var k = 0
            func take(_ n: Int) -> [CGFloat]? {
                guard k + n <= v.count else { return nil }
                defer { k += n }
                return Array(v[k..<k + n])
            }
            switch c.uppercased().first ?? " " {
            case "M":
                var first = true
                while let a = take(2) {
                    let q = pt(a[0], a[1])
                    if first { p.move(to: q); start = q; first = false } else { p.addLine(to: q) }
                    cur = q
                }
                lastC = nil; lastQ = nil
            case "L":
                while let a = take(2) { cur = pt(a[0], a[1]); p.addLine(to: cur) }
                lastC = nil; lastQ = nil
            case "H":
                while let a = take(1) { cur = CGPoint(x: rel ? cur.x + a[0] : a[0], y: cur.y); p.addLine(to: cur) }
                lastC = nil; lastQ = nil
            case "V":
                while let a = take(1) { cur = CGPoint(x: cur.x, y: rel ? cur.y + a[0] : a[0]); p.addLine(to: cur) }
                lastC = nil; lastQ = nil
            case "C":
                while let a = take(6) {
                    let c1 = pt(a[0], a[1]), c2 = pt(a[2], a[3]), e = pt(a[4], a[5])
                    p.addCurve(to: e, control1: c1, control2: c2)
                    lastC = c2; cur = e
                }
                lastQ = nil
            case "S":
                while let a = take(4) {
                    let c1 = lastC.map { CGPoint(x: 2 * cur.x - $0.x, y: 2 * cur.y - $0.y) } ?? cur
                    let c2 = pt(a[0], a[1]), e = pt(a[2], a[3])
                    p.addCurve(to: e, control1: c1, control2: c2)
                    lastC = c2; cur = e
                }
                lastQ = nil
            case "Q":
                while let a = take(4) {
                    let q = pt(a[0], a[1]), e = pt(a[2], a[3])
                    p.addQuadCurve(to: e, control: q)
                    lastQ = q; cur = e
                }
                lastC = nil
            case "T":
                while let a = take(2) {
                    let q = lastQ.map { CGPoint(x: 2 * cur.x - $0.x, y: 2 * cur.y - $0.y) } ?? cur
                    let e = pt(a[0], a[1])
                    p.addQuadCurve(to: e, control: q)
                    lastQ = q; cur = e
                }
                lastC = nil
            case "A":
                while let a = take(7) {
                    let e = pt(a[5], a[6])
                    arc(p, from: cur, to: e, rx: a[0], ry: a[1], rotation: a[2], large: a[3] != 0, sweep: a[4] != 0)
                    cur = e
                }
                lastC = nil; lastQ = nil
            case "Z":
                p.closeSubpath()
                cur = start
                lastC = nil; lastQ = nil
            default:
                break
            }
        }
    }

    /// An SVG elliptical arc (endpoint form, SVG 1.1 appendix F.6) as cubics.
    static func arc(_ p: CGMutablePath, from a: CGPoint, to b: CGPoint, rx: CGFloat, ry: CGFloat, rotation: CGFloat, large: Bool, sweep: Bool) {
        var rx = abs(rx), ry = abs(ry)
        if rx == 0 || ry == 0 || a == b {
            p.addLine(to: b)
            return
        }
        let phi = rotation * .pi / 180
        let (cs, sn) = (cos(phi), sin(phi))
        let dx = (a.x - b.x) / 2, dy = (a.y - b.y) / 2
        let x1 = cs * dx + sn * dy, y1 = -sn * dx + cs * dy
        let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry)
        if lam > 1 { rx *= sqrt(lam); ry *= sqrt(lam) }
        let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1
        let den = rx * rx * y1 * y1 + ry * ry * x1 * x1
        var co = den > 0 ? sqrt(max(num / den, 0)) : 0
        if large == sweep { co = -co }
        let cxp = co * rx * y1 / ry, cyp = -co * ry * x1 / rx
        let cx = cs * cxp - sn * cyp + (a.x + b.x) / 2
        let cy = sn * cxp + cs * cyp + (a.y + b.y) / 2
        func angle(_ ux: CGFloat, _ uy: CGFloat, _ vx: CGFloat, _ vy: CGFloat) -> CGFloat {
            let d = hypot(ux, uy) * hypot(vx, vy)
            guard d > 0 else { return 0 }
            let c = max(-1, min(1, (ux * vx + uy * vy) / d))
            return (ux * vy - uy * vx < 0 ? -1 : 1) * acos(c)
        }
        let t1 = angle(1, 0, (x1 - cxp) / rx, (y1 - cyp) / ry)
        var dt = angle((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry)
        if !sweep && dt > 0 { dt -= 2 * .pi } else if sweep && dt < 0 { dt += 2 * .pi }
        let n = max(1, Int(ceil(abs(dt) / (.pi / 2))))
        let step = dt / CGFloat(n)
        let k = 4.0 / 3.0 * tan(step / 4)
        func at(_ t: CGFloat) -> (CGPoint, CGPoint) {
            let (c, s) = (cos(t), sin(t))
            let pt = CGPoint(x: cx + rx * c * cs - ry * s * sn, y: cy + rx * c * sn + ry * s * cs)
            let d = CGPoint(x: -rx * s * cs - ry * c * sn, y: -rx * s * sn + ry * c * cs)
            return (pt, d)
        }
        var t = t1
        for i in 0..<n {
            let (p0, d0) = at(t)
            let (p3, d3) = at(t + step)
            let end = i == n - 1 ? b : p3
            p.addCurve(to: end, control1: CGPoint(x: p0.x + k * d0.x, y: p0.y + k * d0.y), control2: CGPoint(x: p3.x - k * d3.x, y: p3.y - k * d3.y))
            t += step
        }
    }
}
