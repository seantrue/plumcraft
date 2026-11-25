// A deterministic fake scene: what the core would publish (plan 5.2), in graph coordinates.
import CoreGraphics
import Foundation

/// SplitMix64: small, fast, deterministic on every platform.
public struct SplitMix64: Sendable {
    var state: UInt64
    public init(seed: UInt64) { state = seed }
    public mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }
    public mutating func unit() -> Double { Double(next() >> 11) / Double(1 << 53) }
    public mutating func int(_ r: ClosedRange<Int>) -> Int {
        r.lowerBound + Int(next() % UInt64(r.upperBound - r.lowerBound + 1))
    }
    public mutating func chance(_ p: Double) -> Bool { unit() < p }
}

public enum RunState: Int, CaseIterable, Sendable {
    case notRun, waiting, running, done, cached, failed, skipped, stale
}

public enum RowKind: Sendable, Equatable {
    case input(type: Int)
    case output(type: Int)
    case widget(value: String)
    case preview(seed: UInt64)
}

public struct Row: Sendable {
    public var kind: RowKind
    public var caption: String
}

public struct NodeSpec: Sendable {
    public var id: Int
    public var title: String
    public var family: Int
    public var column: Int
    public var position: CGPoint  // top-left, graph coordinates (y down)
    public var rows: [Row]
    public var state: RunState
    // Measured by the shell (plan 5.2 "who measures"): size and pin anchors, local, y down.
    public var size: CGSize = .zero
    public var rowFrames: [CGRect] = []
    public var pins: [Int: CGPoint] = [:]  // row index -> local pin centre
}

public struct WireSpec: Sendable {
    public var from: Int
    public var fromRow: Int
    public var to: Int
    public var toRow: Int
    public var type: Int
}

public struct SceneData: Sendable {
    public var nodes: [NodeSpec]
    public var wires: [WireSpec]
    public var bounds: CGRect
    public var wiresByNode: [[Int]]

    public func pinPoint(node: Int, row: Int) -> CGPoint {
        let n = nodes[node]
        let p = n.pins[row] ?? .zero
        return CGPoint(x: n.position.x + p.x, y: n.position.y + p.y)
    }
}

public enum FakeScene {
    static let titles = [
        "Load Image", "Resize", "Blur", "Sharpen", "Levels", "Curves", "Mask", "Blend",
        "Threshold", "Segment", "Edge Detect", "Palette", "Quantize", "Stroke", "Brush",
        "Composite", "Noise", "Warp", "Crop", "Save Image", "Table to Rows", "Rows to Table",
        "Convert", "Histogram", "Upscale", "Denoise", "Text", "Json Get", "Merge", "Split",
    ]
    static let inputCaptions = ["image", "mask", "amount", "radius", "palette", "rows", "seed", "scale", "text", "table"]
    static let outputCaptions = ["image", "mask", "result", "rows", "stats", "array", "json"]
    static let widgetCaptions = ["strength", "radius", "steps", "gamma", "threshold", "mode"]

    /// Builds a layered data-flow graph: columns left to right, wires flowing forward 1-3 columns.
    public static func make(nodes n: Int, wires m: Int, seed: UInt64 = 0x5EED_CAFE) -> SceneData {
        var rng = SplitMix64(seed: seed)
        let cols = max(3, Int((Double(n).squareRoot() * 1.1).rounded()))
        var nodes: [NodeSpec] = []
        nodes.reserveCapacity(n)
        var colY = [CGFloat](repeating: 0, count: cols)
        for i in 0..<n {
            let col = i % cols
            let total = rng.int(2...6)
            var outs = rng.int(1...min(2, total))
            var ins = total - outs
            if col == 0 { ins = min(ins, 1); outs = total - ins }
            if col == cols - 1 { outs = 1; ins = total - 1 }
            var rows: [Row] = []
            for _ in 0..<outs {
                let t = rng.int(0...7)
                rows.append(Row(kind: .output(type: t), caption: outputCaptions[rng.int(0...outputCaptions.count - 1)]))
            }
            for _ in 0..<ins {
                let t = rng.int(0...7)
                rows.append(Row(kind: .input(type: t), caption: inputCaptions[rng.int(0...inputCaptions.count - 1)]))
            }
            if rng.chance(0.4) {
                let v = String(format: "%.2f", rng.unit() * 4)
                rows.append(Row(kind: .widget(value: v), caption: widgetCaptions[rng.int(0...widgetCaptions.count - 1)]))
            }
            if rng.chance(0.2) {
                rows.append(Row(kind: .preview(seed: rng.next()), caption: "preview"))
            }
            let states: [RunState] = [.done, .cached, .failed, .running, .waiting, .stale]
            let state: RunState = rng.chance(0.3) ? states[rng.int(0...states.count - 1)] : .notRun
            var node = NodeSpec(
                id: i, title: "\(titles[rng.int(0...titles.count - 1)]) \(i)", family: rng.int(0...7),
                column: col, position: .zero, rows: rows, state: state)
            FaceLayout.measure(&node)
            let jitter = CGFloat(rng.int(-20...20))
            node.position = CGPoint(x: CGFloat(col) * 300 + jitter, y: colY[col])
            colY[col] += node.size.height + 40
            nodes.append(node)
        }

        // index nodes by column, with their output and input rows
        var byCol = [[Int]](repeating: [], count: cols)
        for node in nodes { byCol[node.column].append(node.id) }
        func rowsOf(_ i: Int, outputs: Bool) -> [Int] {
            nodes[i].rows.indices.filter { r in
                switch nodes[i].rows[r].kind {
                case .output: return outputs
                case .input: return !outputs
                default: return false
                }
            }
        }
        var wires: [WireSpec] = []
        var attempts = 0
        while wires.count < m && attempts < m * 50 {
            attempts += 1
            let src = rng.int(0...n - 1)
            let col = nodes[src].column
            guard col < cols - 1 else { continue }
            let dcol = min(cols - 1, col + rng.int(1...3))
            let cands = byCol[dcol]
            guard !cands.isEmpty else { continue }
            let dst = cands[rng.int(0...cands.count - 1)]
            let outs = rowsOf(src, outputs: true)
            let ins = rowsOf(dst, outputs: false)
            guard !outs.isEmpty, !ins.isEmpty else { continue }
            let o = outs[rng.int(0...outs.count - 1)]
            let ii = ins[rng.int(0...ins.count - 1)]
            var type = 0
            if case .output(let t) = nodes[src].rows[o].kind { type = t }
            wires.append(WireSpec(from: src, fromRow: o, to: dst, toRow: ii, type: type))
        }
        var wiresByNode = [[Int]](repeating: [], count: n)
        for (w, wire) in wires.enumerated() {
            wiresByNode[wire.from].append(w)
            if wire.to != wire.from { wiresByNode[wire.to].append(w) }
        }
        var bounds = CGRect.null
        for node in nodes { bounds = bounds.union(CGRect(origin: node.position, size: node.size)) }
        return SceneData(nodes: nodes, wires: wires, bounds: bounds, wiresByNode: wiresByNode)
    }
}
