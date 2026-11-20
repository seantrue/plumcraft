// A drag on the canvas, the same on every Apple host (N4 slice 1): the host recognises the press
// and its movement (a mouse drag on the Mac, a one-finger pan or a Pencil on the iPad) and feeds
// this driver view points; the core decides what the drag is (`ui.gesture` drag `begin`: a move of
// the faces under it and the selection with them, a new wire, a wire's end picked up, a marquee, a
// bend point, a frame, a frame's size, an open group) and what its end means (one undo step at
// most). Between the two ends nothing crosses as JSON: each move is the typed `plum_gesture_move`,
// and a move's wires come from `plum_wire_paths` (contract 8.13, 9.6).
//
// What the driver draws while dragging: the faces moving with their wires re-pathed every frame;
// a new or picked-up wire from its anchor to the pointer, snapped to the pin it would land on with
// that pin ringed (it fits, it fits through a Convert, or it doesn't); the marquee's rect; a
// frame's, a group's or a bend point's outline at its new place. Near the view's edge the canvas
// scrolls under the pointer (autoscroll), and the drag follows it. `cancel` (Escape, a second
// finger, the system taking the touch) leaves the document as it was.

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels

@MainActor
public final class CanvasDrag {
    public let canvas: LayerCanvas
    /// The hit distance now (the host's target size over the zoom).
    public var slop: () -> Double
    /// A gesture was refused or failed.
    public var onError: ((String) -> Void)?
    /// The drag's begin was answered: what the core made of it.
    public var onBegan: ((DragWord?) -> Void)?
    /// The drag ended (or was cancelled: nil reply), with what the core ran.
    public var onEnded: ((DragWord?, GestureReply?) -> Void)?
    /// The wire's target changed to a pin that takes it (the iPad gives a haptic).
    public var onSnap: (() -> Void)?
    /// How close to the view's edge (points) autoscroll starts, and its most points a frame.
    public var edge: CGFloat = 36
    public var maxScroll: CGFloat = 18

    public private(set) var active = false
    public private(set) var kind: DragWord?
    var begun: DragBegun?
    var beginTask: Task<GestureReply?, Never>?
    var latest: CGPoint?
    var anchor: CGPoint?
    var lastTarget: (UInt64, UInt32, UInt32)?
    var scroller: Task<Void, Never>?
    /// The original rect of a dragged frame or group, or a bend point (graph units).
    var ghostFrom: CGRect?
    var ghostRound = false
    /// A scrub's row (graph units) and its widget, for the value shown over it.
    var scrubRow: CGRect?
    var scrubWidget: Widget?
    /// A new wire dropped on empty canvas opened the palette: the wire stays drawn until the
    /// palette closes (`releaseWire`).
    public private(set) var holdingWire = false

    public init(canvas: LayerCanvas, slop: @escaping () -> Double) {
        self.canvas = canvas
        self.slop = slop
    }

    func graph(_ v: CGPoint) -> Point {
        let g = canvas.viewport.toGraph(v)
        return Point(Double(g.x), Double(g.y))
    }

    // MARK: The four phases

    /// The press began a drag at view point `at` (where it went down, not where it is now).
    public func begin(at v: CGPoint, mods: [ModifierWord]) {
        releaseWire()
        active = true
        kind = nil
        begun = nil
        anchor = nil
        lastTarget = nil
        ghostFrom = nil
        let g = graph(v)
        let s = slop()
        let z = Double(canvas.viewport.zoom)
        let link = canvas.link
        beginTask = Task { [weak self] in
            do {
                let r = try await link.dragBegin(g, mods: mods, slop: s, zoom: z)
                guard let self else { return r }
                started(r)
                return r
            } catch {
                self?.onError?("\(error)")
                return nil
            }
        }
    }

    func started(_ r: GestureReply) {
        guard active else { return }
        begun = r.drag
        kind = r.drag?.kind.word
        switch kind {
        case .move?:
            if let faces = r.drag?.faces { canvas.beginMove(faces) }
        case .wire?:
            anchor = canvas.anchorPoint(r.drag?.from)
            if let to = r.drag?.picked { canvas.pickUp(wire: canvas.wireNid(into: to)) }
        case .frame?, .resize?:
            if let h = r.hit, let f = frameRect(h) { ghostFrom = f }
        case .group?:
            if let h = r.hit, let f = groupRect(h) { ghostFrom = f }
        case .scrub?:
            if let row = r.drag?.row, let face = row["face"].string, let k = row["index"].int64, let nid = canvas.nid(of: face) {
                scrubRow = canvas.rowRect(face: nid, row: Int(k))
                scrubWidget = canvas.model.faces[nid]?.item.inputs[safe: Int(k)]?.widget
            }
        case .bend?:
            if let h = r.hit {
                ghostFrom = CGRect(x: h.at.x - 6, y: h.at.y - 6, width: 12, height: 12)
                ghostRound = true
            }
        default:
            break
        }
        onBegan?(kind)
        if let p = latest { update(p) }
    }

    func frameRect(_ h: Hit) -> CGRect? {
        guard let id = h.frame, let f = canvas.model.frames.item(id: id) else { return nil }
        return CGRect(x: f.pos.x, y: f.pos.y, width: f.size.x, height: f.size.y)
    }

    func groupRect(_ h: Hit) -> CGRect? {
        guard let id = h.group, let g = canvas.model.groups.item(id: id), let r = g.rect else { return nil }
        return CGRect(x: r.x0, y: r.y0, width: r.width, height: r.height)
    }

    /// The pointer or finger is at view point `v`.
    public func move(to v: CGPoint) {
        guard active else { return }
        latest = v
        update(v)
        autoscroll(v)
    }

    func update(_ v: CGPoint) {
        guard let kind else { return }
        let g = graph(v)
        let d = canvas.link.dragMove(g, slop: slop())
        guard d.ok else { return }
        switch kind {
        case .move:
            canvas.move(dx: CGFloat(d.dx), dy: CGFloat(d.dy))
        case .marquee:
            canvas.showMarquee(CGRect(x: CGFloat(d.rect.0), y: CGFloat(d.rect.1), width: CGFloat(d.rect.2 - d.rect.0), height: CGFloat(d.rect.3 - d.rect.1)))
        case .wire:
            let t = d.target
            let on = t.ok && t.kind == .pin
            let snap = on ? CGPoint(x: CGFloat(t.x), y: CGFloat(t.y)) : CGPoint(x: g.x, y: g.y)
            canvas.showDragWire(from: anchor, to: snap, fits: on ? t.fits : 0)
            canvas.showDragTarget(on ? t : nil)
            let key = on ? (t.node, t.pin, t.part) : nil
            if let key, t.fits != 0, lastTarget.map({ $0 != key }) ?? true { onSnap?() }
            lastTarget = key
        case .frame, .group, .bend:
            if let r = ghostFrom { canvas.showGhost(r.offsetBy(dx: CGFloat(d.dx), dy: CGFloat(d.dy)), round: ghostRound) }
        case .resize:
            if let r = ghostFrom {
                canvas.showGhost(CGRect(x: r.minX, y: r.minY, width: max(r.width + CGFloat(d.dx), 0), height: max(r.height + CGFloat(d.dy), 0)))
            }
        case .scrub:
            if let v = d.scrubValue { canvas.showScrub(Self.numberText(v, scrubWidget), over: scrubRow) }
        }
    }

    /// Let go at view point `v`: the core runs what the drag means.
    public func end(at v: CGPoint) {
        guard active else { return }
        active = false
        stopScroll()
        let g = graph(v)
        let s = slop()
        let begin = beginTask
        let link = canvas.link
        let k = kind
        Task { [weak self] in
            _ = await begin?.value
            var reply: GestureReply?
            do {
                reply = try await link.dragEnd(g, slop: s)
            } catch {
                self?.onError?("\(error)")
            }
            guard let self else { return }
            let moved = reply?.commands.contains { $0.ok && $0.command == "edit.move" } ?? false
            // a new wire dropped on nothing: the palette opens for it, and the wire stays drawn
            // from its pin to the drop until a kind is picked or the palette is dismissed
            let palette = reply?.commands.contains { $0.ok && $0.command == "canvas.palette" } ?? false
            // a scrub held to its range, or refused: the core's words by the row
            if let row = scrubRow, let set = reply?.commands.first(where: { $0.command == "row.set" }) {
                if let e = set.error { canvas.flashNote(e, under: row, error: true) } else if let n = set.result?["note"].string { canvas.flashNote(n, under: row, error: false) }
            }
            clear(kept: moved, holdWire: k == .wire && palette)
            onEnded?(k, reply)
        }
    }

    /// Escape, a second finger, the system: the document stays as it was.
    public func cancel() {
        guard active else { return }
        active = false
        stopScroll()
        let begin = beginTask
        let link = canvas.link
        let k = kind
        Task { [weak self] in
            _ = await begin?.value
            _ = try? await link.dragCancel()
            self?.clear()
            self?.onEnded?(k, nil)
        }
    }

    func clear(kept: Bool = false, holdWire: Bool = false) {
        kind = nil
        latest = nil
        begun = nil
        ghostFrom = nil
        ghostRound = false
        scrubRow = nil
        scrubWidget = nil
        canvas.showScrub(nil, over: nil)
        canvas.showMarquee(nil)
        canvas.showGhost(nil)
        canvas.showDragTarget(nil)
        canvas.pickUp(wire: nil)
        if holdWire {
            holdingWire = true
        } else {
            canvas.showDragWire(from: nil, to: .zero, fits: 0)
        }
        canvas.endMove(kept: kept)
    }

    /// The palette a dropped wire opened has closed: stop drawing the wire.
    public func releaseWire() {
        guard holdingWire else { return }
        holdingWire = false
        canvas.showDragWire(from: nil, to: .zero, fits: 0)
    }

    /// A number as its row shows it: whole for an INT, else the widget's decimals.
    public static func numberText(_ v: Double, _ w: Widget?) -> String {
        if w?.int == true { return String(Int64(v.rounded())) }
        let places = max(0, min(w?.decimals ?? 3, 8))
        return String(format: "%.\(places)f", v)
    }

    // MARK: Autoscroll

    /// How far into the edge band `v` is, as a pan a frame (zero: not in it).
    func scrollStep(_ v: CGPoint) -> CGPoint {
        let size = canvas.viewSize
        guard size.width > edge * 3, size.height > edge * 3 else { return .zero }
        func axis(_ p: CGFloat, _ len: CGFloat) -> CGFloat {
            if p < edge { return maxScroll * min((edge - p) / edge, 1) }
            if p > len - edge { return -maxScroll * min((p - (len - edge)) / edge, 1) }
            return 0
        }
        return CGPoint(x: axis(v.x, size.width), y: axis(v.y, size.height))
    }

    func autoscroll(_ v: CGPoint) {
        // a scrub moves a value, not the view
        if kind == .scrub { return stopScroll() }
        let step = scrollStep(v)
        if step == .zero {
            stopScroll()
            return
        }
        guard scroller == nil else { return }
        scroller = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self, active, let p = latest else { break }
                let st = scrollStep(p)
                if st == .zero { break }
                canvas.pan(by: st, elastic: false)
                update(p)
                try? await Task.sleep(for: .milliseconds(16))
            }
            self?.scroller = nil
        }
    }

    func stopScroll() {
        scroller?.cancel()
        scroller = nil
    }

    /// Whether autoscroll is under way (tests).
    public var scrolling: Bool { scroller != nil }
}

extension Array {
    /// The element at `i`, or nil past either end.
    subscript(safe i: Int) -> Element? { i >= 0 && i < count ? self[i] : nil }
}
