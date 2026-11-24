// The typed hot calls (contract section 9; plumcraft.h's "hot calls"): synchronous, no JSON,
// reading the document's latest snapshot without waiting for the core thread. A canvas calls them
// during a gesture, on its own thread; they never block on an engine or a request.

import Foundation
import PlumCraftFFI
import PlumCraftModels

/// What `plum_hit_test` or `plum_wire_target` found.
public struct HotHit: Sendable, Equatable {
    public enum Kind: Sendable, Equatable {
        case none, node, pin, wire, bend, tag, frame, group
        case unknown(UInt32)

        init(_ k: UInt32) {
            switch k {
            case 0: self = .none
            case 1: self = .node
            case 2: self = .pin
            case 3: self = .wire
            case 4: self = .bend
            case 5: self = .tag
            case 6: self = .frame
            case 7: self = .group
            default: self = .unknown(k)
            }
        }
    }

    /// A face's part (kind node), by plumcraft.h's numbers.
    public enum NodePart: UInt32, Sendable {
        case body = 1, header, title, chevron, icon, menu, enter, row, preview, footer
    }

    public let status: Int32
    public let kind: Kind
    /// By kind: a face's part (NodePart), a pin's side (1 input, 2 output), a wire's (1 a
    /// right-angled route's corner, 2 its leg between corners, B155; 0 anywhere else on it), a
    /// tag's (1 driver, 2 use), a frame's (1 body, 2 tab, 3 handle), a group's (1 band).
    public let part: UInt32
    /// A row, footer item or bend point; -1 none.
    public let index: Int32
    /// The nid of what was hit (face, wire, net, frame or group, by kind).
    public let item: UInt64
    /// The face (a node or pin; a tag's face).
    public let node: UInt64
    public let wire: UInt64
    /// A pin's index on its side.
    public let pin: UInt32
    /// `plum_wire_target`: 0 doesn't fit, 1 `wire.connect` takes it, 2 only through a Convert.
    public let fits: UInt32
    /// A pin's point (where a dragged wire snaps); else the point asked.
    public let x: Float
    public let y: Float

    init(_ h: plum_hit) {
        status = h.status
        kind = Kind(h.kind)
        part = h.part
        index = h.index
        item = h.item
        node = h.node
        wire = h.wire
        pin = h.pin
        fits = h.fits
        x = h.x
        y = h.y
    }

    public var ok: Bool { status == PlumStatus.ok }
    public var nodePart: NodePart? { kind == .node ? NodePart(rawValue: part) : nil }
    /// A pin's side: true for an output.
    public var isOutput: Bool { kind == .pin && part == 2 }
}

/// Where a drag under way is (`plum_gesture_move`).
public struct HotDrag: Sendable, Equatable {
    public enum Kind: Sendable, Equatable {
        case move, wire, marquee, bend, frame, resize, group, scrub
        case unknown(UInt32)

        init(_ k: UInt32) {
            switch k {
            case 8: self = .scrub
            case 1: self = .move
            case 2: self = .wire
            case 3: self = .marquee
            case 4: self = .bend
            case 5: self = .frame
            case 6: self = .resize
            case 7: self = .group
            default: self = .unknown(k)
            }
        }
    }

    public let status: Int32
    public let kind: Kind
    public let rawKind: UInt32
    public let serial: UInt64
    public let dx: Float
    public let dy: Float
    /// A marquee's rect (x0, y0, x1, y1); zeros otherwise.
    public let rect: (Float, Float, Float, Float)
    /// A wire's: where it would land, and `fits`.
    public let target: HotHit

    init(_ d: plum_drag) {
        status = d.status
        kind = Kind(d.kind)
        rawKind = d.kind
        serial = d.serial
        dx = d.dx
        dy = d.dy
        rect = d.rect
        target = HotHit(d.target)
    }

    public var ok: Bool { status == PlumStatus.ok }

    /// A scrub's value now: a double in `rect.0` (low 32 bits) and `rect.1` (high), bit-cast.
    public var scrubValue: Double? {
        guard kind == .scrub else { return nil }
        return Double(bitPattern: UInt64(rect.0.bitPattern) | (UInt64(rect.1.bitPattern) << 32))
    }

    public static func == (a: HotDrag, b: HotDrag) -> Bool {
        a.status == b.status && a.kind == b.kind && a.serial == b.serial && a.dx == b.dx && a.dy == b.dy
            && a.rect == b.rect && a.target == b.target
    }
}

/// One wire's path as `plum_wire_paths` gives it: a cubic chain (the start, then three points a
/// segment), as interleaved x, y.
public struct WirePath: Sendable {
    public let nid: UInt64
    /// x0, y0, x1, y1, …: `points.count / 2` points.
    public let points: ArraySlice<Float>
    public var pointCount: Int { points.count / 2 }
}

/// The paths `plum_wire_paths` wrote, read in place from the floats.
public struct WirePaths: Sequence, Sendable {
    public let floats: [Float]

    public struct Iterator: IteratorProtocol {
        let floats: [Float]
        var at = 0

        public mutating func next() -> WirePath? {
            guard at + 3 <= floats.count else { return nil }
            let lo = UInt64(floats[at].bitPattern)
            let hi = UInt64(floats[at + 1].bitPattern)
            let n = Int(floats[at + 2].bitPattern)
            let start = at + 3
            let end = start + 2 * n
            // a count past the end would be the C side's bug: stop rather than trap
            guard end <= floats.count else { return nil }
            at = end
            return WirePath(nid: lo | (hi << 32), points: floats[start..<end])
        }
    }

    public func makeIterator() -> Iterator { Iterator(floats: floats) }
}

/// One face's measurements for `plum_geometry_set` (graph units, at zoom 1). What the typed
/// call has no field for (`offset`, header parts) goes in `geometry.set`'s JSON form.
public struct FaceMeasure: Sendable, Hashable {
    public var face: UInt64
    /// The face's `rev` measured; 0: not said.
    public var rev: UInt64
    public var width: Float
    public var height: Float
    /// The header's height; nil: not said.
    public var header: Float?
    /// Each input pin's height from the top, in row order (nil: not measured).
    public var inputs: [Float?]
    public var outputs: [Float?]
    /// Each input row's widget rect in face coordinates (nil: none).
    public var rows: [Rect?]

    public init(face: UInt64, rev: UInt64 = 0, width: Float, height: Float, header: Float? = nil, inputs: [Float?] = [], outputs: [Float?] = [], rows: [Rect?] = []) {
        self.face = face; self.rev = rev; self.width = width; self.height = height; self.header = header
        self.inputs = inputs; self.outputs = outputs; self.rows = rows
    }
}

extension PlumCore {
    /// What is at `(x, y)` in graph units within `slop` (the shell's target size over its zoom).
    public func hitTest(doc: UInt64 = 0, x: Float, y: Float, slop: Float) -> HotHit {
        HotHit(plum_hit_test(id, doc, x, y, slop))
    }

    /// Where a wire dragged from pin `pin` of face `face` (`output`: from an output) may land at
    /// `(x, y)`: the nearest pin on the other side within slop that fits, else the nearest that
    /// doesn't (`fits` 0), else kind none.
    public func wireTarget(doc: UInt64 = 0, face: UInt64, output: Bool, pin: UInt32, x: Float, y: Float, slop: Float) -> HotHit {
        HotHit(plum_wire_target(id, doc, face, output ? 2 : 1, pin, x, y, slop))
    }

    /// The paths of every wire with an end on one of `faces` (nids), as if they were moved by
    /// `(dx, dy)`. `buffer` is reused between calls (a canvas keeps one per drag), so a frame's
    /// call allocates nothing once it is big enough.
    public func wirePaths(doc: UInt64 = 0, faces: [UInt64], dx: Float, dy: Float, buffer: inout [Float]) -> WirePaths {
        if faces.isEmpty { return WirePaths(floats: []) }
        // the snapshot may grow between two calls: ask again with room, a few times at most
        for _ in 0..<4 {
            let cap = buffer.count
            let need: Int = faces.withUnsafeBufferPointer { f in
                buffer.withUnsafeMutableBufferPointer { out in
                    plum_wire_paths(id, doc, f.baseAddress, f.count, dx, dy, out.baseAddress, cap)
                }
            }
            if need == 0 { return WirePaths(floats: []) }
            if need <= cap { return WirePaths(floats: Array(buffer[0..<need])) }
            buffer = [Float](repeating: 0, count: need + need / 4 + 64)
        }
        return WirePaths(floats: [])
    }

    /// Report what the shell measured of `faces` (`geometry.set`, typed). Copied before it
    /// returns and applied on the core's thread; a nid the scene doesn't have is skipped.
    public func geometrySet(doc: UInt64 = 0, faces: [FaceMeasure]) throws {
        // every pin and row of every face in two flat buffers, so their pointers stay put
        var pins: [Float] = []
        var rows: [Float] = []
        var spans: [(inputs: Int, outputs: Int, rows: Int)] = []
        spans.reserveCapacity(faces.count)
        for f in faces {
            spans.append((pins.count, pins.count + f.inputs.count, rows.count))
            pins.append(contentsOf: f.inputs.map { $0 ?? .nan })
            pins.append(contentsOf: f.outputs.map { $0 ?? .nan })
            for r in f.rows {
                if let r {
                    rows.append(contentsOf: [Float(r.x0), Float(r.y0), Float(r.x1), Float(r.y1)])
                } else {
                    rows.append(contentsOf: [.nan, .nan, .nan, .nan])
                }
            }
        }
        let status: Int32 = pins.withUnsafeBufferPointer { p in
            rows.withUnsafeBufferPointer { r in
                var typed: [plum_face] = []
                typed.reserveCapacity(faces.count)
                for (f, s) in zip(faces, spans) {
                    let ins = f.inputs.isEmpty ? nil : p.baseAddress.map { $0 + s.inputs }
                    let outs = f.outputs.isEmpty ? nil : p.baseAddress.map { $0 + s.outputs }
                    let rs = f.rows.isEmpty ? nil : r.baseAddress.map { $0 + s.rows }
                    typed.append(plum_face(face: f.face, rev: f.rev, w: f.width, h: f.height, header: f.header ?? .nan,
                                           inputs: ins, n_inputs: f.inputs.count, outputs: outs, n_outputs: f.outputs.count,
                                           rows: rs, n_rows: f.rows.count))
                }
                return typed.withUnsafeBufferPointer { plum_geometry_set(id, doc, $0.baseAddress, $0.count) }
            }
        }
        if status != PlumStatus.ok {
            throw PlumError(.status(status), "plum_geometry_set: \(PlumStatus.word(status))")
        }
    }

    /// The drag under way with the pointer at `(x, y)` (`ui.gesture`'s move, typed). Nothing
    /// changes; the drag's begin and end are `ui.gesture` requests.
    public func gestureMove(doc: UInt64 = 0, x: Float, y: Float, slop: Float) -> HotDrag {
        HotDrag(plum_gesture_move(id, doc, x, y, slop))
    }

    /// `plum_close` on a number, for tests of a core closed twice: PLUM_ERR_CORE, never a crash.
    public static func rawClose(_ core: UInt64) -> Int32 { plum_close(core) }
}
