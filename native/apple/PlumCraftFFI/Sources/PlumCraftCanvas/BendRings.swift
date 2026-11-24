// A wire's bend points, and a wire bent by a drag (B158).
//
// The dots at a wire's bend points show only while they are wanted: the wire under the pointer
// (or one of its bend points under it), a wire with an end on a selected face, and the wire a
// drag is bending. They are drawn smaller than the core's hit radius (5 graph units, plus the
// slop), so a bend point is grabbed as easily as before. A drag on a bend point, or on a
// right-angled route's corner or leg, draws the wire as the core says it would be
// (`plum_drag_wire`), in place of its own, until the drag ends or is cancelled.

import Foundation
import PlumCraftModels
import QuartzCore

extension LayerCanvas {
    /// A bend point's drawn radius (graph units). The core's hit radius stays 5.
    static let bendDot: CGFloat = 3

    /// Show the wire a drag is bending, as the core says it would be (`b`), in place of its own;
    /// nil puts the wire's own back. `kept`: the drag's end changed the wire, so its layer keeps
    /// the bent path until the scene brings the new one (at most a second), with no jump back.
    public func showBentWire(_ b: (nid: UInt64, path: [CGPoint], via: [CGPoint])?, kept: Bool = false) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        let was = dragRoute?.nid
        dragRoute = b
        if let b, wireGeoms[b.nid] != nil, b.path.count >= 4 {
            if let l = updateLive(b.nid, setPath: false) {
                setLivePath(l, b.nid, Self.chain(b.path))
            }
            if hoverWire == b.nid { showHoverWire() }
        } else if let was, b == nil {
            if kept {
                liveUntilPathed.insert(was)
                Task { @MainActor [weak self] in
                    try? await Task.sleep(for: .seconds(1))
                    guard let self, self.liveUntilPathed.contains(was) else { return }
                    if let w = self.model.wires[was]?.item { self.pathWire(w) } else { self.liveUntilPathed.remove(was) }
                    self.pumpTiles()
                }
            } else if let g = wireGeoms[was] {
                if let l = updateLive(was) { setLivePath(l, was, g.path) }
                if hoverWire == was { showHoverWire() }
            }
        }
        showBendRings()
        pumpTiles()
    }

    /// The wires whose bend points show now: being bent, hovered, an end on a selected face.
    func ringWires() -> [UInt64] {
        var out: [UInt64] = []
        if let d = dragRoute?.nid { out.append(d) }
        if let h = ringHover, !out.contains(h) { out.append(h) }
        let sel = model.selection
        if !sel.nodes.isEmpty || !sel.groups.isEmpty {
            let nodes = Set(sel.nodes)
            let groups = Set(sel.groups)
            func on(_ face: String) -> Bool {
                if nodes.contains(face) { return true }
                guard let nid = faceById[face], let p = faces[nid]?.face.proxy else { return false }
                return groups.contains(p.group)
            }
            for nid in model.wires.order where !out.contains(nid) {
                guard let w = model.wires[nid]?.item, !w.via.isEmpty else { continue }
                if on(w.fromFace) || on(w.toFace) { out.append(nid) }
            }
        }
        return out
    }

    /// Draw the dots `ringWires` names: one in each wire's colour at each of its bend points.
    func showBendRings() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        var keep: [CALayer] = []
        var shown: [UInt64: [CGPoint]] = [:]
        for nid in ringWires() {
            let via: [CGPoint]
            if let d = dragRoute, d.nid == nid {
                via = d.via
            } else if let w = model.wires[nid]?.item, w.kind.word != .net {
                via = w.via.map { CGPoint(x: $0.x, y: $0.y) }
            } else {
                continue
            }
            guard !via.isEmpty, let g = wireGeoms[nid] else { continue }
            let p = CGMutablePath()
            let r = Self.bendDot
            for v in via { p.addEllipse(in: CGRect(x: v.x - r, y: v.y - r, width: 2 * r, height: 2 * r)) }
            let l = CAShapeLayer()
            l.anchorPoint = .zero
            l.path = p
            l.fillColor = g.color
            l.strokeColor = colors.canvas
            l.lineWidth = 1
            keep.append(l)
            shown[nid] = via
        }
        bendRingLayer.sublayers = keep
        bendDotsShown = shown
    }

    /// The wire a drag is bending, as drawn now (for tests): its nid and the path on its layer.
    public var bentWireShown: (nid: UInt64, path: CGPath?)? {
        guard let d = dragRoute else { return nil }
        return (d.nid, livePaths[d.nid])
    }
}
