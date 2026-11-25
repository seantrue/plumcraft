// The layer canvas (plan 5.4, as N0 measured it in 15.2): a CALayer per node with its face drawn
// once into it, one CAShapeLayer per wire with its frame tight around the wire, frames, open
// groups, net tags and marks as their own layers, all under one transformed layer. Pan and zoom
// change that one transform, so they are the compositor's work; nothing crosses to the core per
// frame (5.3).
//
// It follows a DocumentModel: `onChange` names the nids that changed or went, and only those
// layers are touched. A face whose `rev` changed is laid out again and drawn again on a worker,
// and its measurements go to the core (`plum_geometry_set`, typed) from there, so text is never
// measured on the main thread or per frame. The core then recomputes wire paths from what was
// measured, and those wires arrive as changed too.
//
// Faces are drawn at a scale that steps in powers of two of the screen's, chosen from the zoom;
// when a pan or zoom settles, only the visible faces whose scale is wrong are drawn again (15.2).
// The selection and the run wash are sublayers, so selecting or a run's animation never redraws a
// face.
//
// Platform-free: QuartzCore, Core Graphics and Core Text only. The hosts (an NSView, a UIView)
// recognise input and call in here.

import CoreGraphics
import CoreText
import Foundation
import Observation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import QuartzCore
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

/// The canvas transform: `screen = graph · zoom + pan` (contract 6.3, `ui.viewport`).
public struct Viewport: Equatable, Sendable {
    public var pan: CGPoint
    public var zoom: CGFloat
    public init(pan: CGPoint = .zero, zoom: CGFloat = 1) { self.pan = pan; self.zoom = zoom }
    public static let minZoom: CGFloat = 0.1
    public static let maxZoom: CGFloat = 4
    public func toGraph(_ v: CGPoint) -> CGPoint { CGPoint(x: (v.x - pan.x) / zoom, y: (v.y - pan.y) / zoom) }
    public func toView(_ g: CGPoint) -> CGPoint { CGPoint(x: g.x * zoom + pan.x, y: g.y * zoom + pan.y) }
}

/// Switches that change how wires are drawn, for the bench alone (the wire-hitch work: each
/// isolates one suspect, so the device pass can bisect by switch). Never set by the apps.
public enum WireSwitch: String, Sendable, CaseIterable {
    /// Mitred joins, as the N0 spike drew them (the canvas joins round).
    case miter = "--wire-miter"
    /// 2-point wires, as the spike's (the canvas's are 2.5).
    case thin = "--wire-thin"
    /// Only wires whose box is under 800 by 400 graph units, as the spike's scene had (its wires
    /// went forward 1 to 3 columns; the core's bench scene wires each node to any earlier one).
    case shortOnly = "--wire-short-only"
    /// The canvas before the wire-hitch fix: a shape layer per wire, its frame tight around the
    /// wire (unclipped), instead of wires drawn into tile pictures. The device pass's before, in
    /// the same build.
    case layers = "--wire-layers"
}

/// What the wire layers give the compositor at one moment (`LayerCanvas.wireStats`).
public struct WireStats: Sendable, Equatable {
    public var layers = 0
    public var shown = 0
    public var inView = 0
    public var elements = 0
    /// Layers with a picture (drawn wires), as against shape layers.
    public var tiles = 0
    /// The shown layers' view-clipped area over the view's (1: one view's worth of blending).
    public var coverage = 0.0
    /// The largest one's, as a fraction of the view.
    public var largest = 0.0
    public init() {}
}

/// What the canvas did, for measurements and tests.
public struct CanvasCounts: Sendable, Equatable {
    public var facesLaidOut = 0
    public var facesDrawn = 0
    public var reports = 0
    public var wiresPathed = 0
    public var batches = 0
    public var tilesDrawn = 0
}

@MainActor
public final class LayerCanvas {
    public let model: DocumentModel
    public let link: CanvasLink
    /// The layer the host gives: the canvas's background; `world` hangs under it.
    public let host: CALayer
    public let screenScale: CGFloat
    public private(set) var colors = CanvasColors()
    public var fonts = FaceFonts.standard

    let world = CALayer()
    let frameRoot = CALayer()
    let groupRoot = CALayer()
    let wireRoot = CALayer()
    let nodeRoot = CALayer()
    let tagRoot = CALayer()
    let overlay = CALayer()

    public var viewSize: CGSize = CGSize(width: 1024, height: 768) {
        didSet { if viewSize != oldValue { scheduleSettle() } }
    }
    public private(set) var viewport = Viewport()
    /// The pan a gesture asks for before the rubber band is applied.
    var rawPan: CGPoint?

    final class FaceEntry {
        let nid: UInt64
        let layer = CALayer()
        var wash: CALayer?
        var select: CAShapeLayer?
        var preview: CALayer?
        var face: Face
        var layout: FaceLayout?
        var laidRev: UInt64?
        var drawnRev: UInt64?
        var drawnScale: CGFloat = 0
        var drawnTheme = -1
        var previewKey: String?
        init(nid: UInt64, face: Face) { self.nid = nid; self.face = face }
        var rect: CGRect {
            CGRect(x: face.pos.x, y: face.pos.y, width: layout?.size.width ?? 220, height: layout?.size.height ?? 160)
        }
    }

    var faces: [UInt64: FaceEntry] = [:]
    var faceById: [String: UInt64] = [:]
    /// Every drawn wire's path and colour (WireTiles.swift): the wires are drawn into tile
    /// pictures, and only a wire being moved or picked up has a shape layer of its own (`live`).
    var wireGeoms: [UInt64: WireGeom] = [:]
    var tiles: [TileKey: WireTile] = [:]
    var dirtyTiles: Set<TileKey> = []
    var tileWorking = false
    /// The shape layers of wires drawn on their own, by wire; those in `returning` go once the
    /// tiles that now draw them are up.
    var live: [UInt64: CAShapeLayer] = [:]
    var returning: Set<UInt64> = []
    /// Wires a kept move left live until the scene brings their new paths.
    var liveUntilPathed: Set<UInt64> = []
    /// The paths the live layers draw (graph units), to clip them again when the view moves.
    var livePaths: [UInt64: CGPath] = [:]
    /// The wire under the pointer, drawn wider over the tiles.
    let hoverWireLayer = CAShapeLayer()
    /// The bend points' rings shown now (B158): the hovered wire's, the wires of selected faces',
    /// the dragged wire's (`BendRings.swift`).
    let bendRingLayer = CALayer()
    /// The wire the pointer is on or on a bend point of (B158: its rings show).
    var ringHover: UInt64?
    /// A wire being bent by a drag (B158): its path and bend points as they would be, drawn in
    /// place of its own until the drag ends or is cancelled.
    var dragRoute: (nid: UInt64, path: [CGPoint], via: [CGPoint])?
    /// The bend points drawn now, by wire (B158).
    public internal(set) var bendDotsShown: [UInt64: [CGPoint]] = [:]
    var tags: [UInt64: [CALayer]] = [:]
    var frames: [UInt64: CALayer] = [:]
    var groups: [UInt64: CALayer] = [:]
    var marks: [UInt64: CALayer] = [:]
    var themeGen = 0

    // work on faces: what is waiting, whether a batch is out
    var needLayout: Set<UInt64> = []
    var needDraw: Set<UInt64> = []
    var working = false
    var settleTask: Task<Void, Never>?
    var lastFitSeq: UInt64?
    var lastRevealSeq: UInt64?
    /// The first full scene's faces were all measured (the time to first frame stops here).
    public private(set) var firstFrameAt: ContinuousClock.Instant?
    public internal(set) var counts = CanvasCounts()
    /// Called after each batch of faces is on its layers (measurements, budgets).
    public var onDrawn: ((Set<UInt64>) -> Void)?
    /// Called when the transform changes (the host shows the zoom, reports it).
    public var onViewport: ((Viewport) -> Void)?
    /// Called when the canvas wants the theme read again (an appearance change).
    var hoverWire: UInt64?
    let hoverRing = CAShapeLayer()
    let marquee = CAShapeLayer()
    let dragWire = CAShapeLayer()
    /// Where a dragged wire would land (N4 slice 1): a ring on the target pin, filled when it fits.
    let dragTarget = CAShapeLayer()
    /// A frame's, an open group's or a bend point's outline while it is dragged.
    let ghost = CAShapeLayer()
    /// A wire whose end is picked up off its input: hidden while the drag draws it.
    var pickedWire: UInt64?
    /// Faces moved by a drag under way, at their positions when it began.
    var moving: [UInt64: CGPoint] = [:]
    var movingWires: Set<UInt64> = []
    /// Pictures the core lends (the faces' previews; a Watched panel may share it).
    public let pictures: Pictures
    /// The node icons the faces name, fetched from the core once each (`icon.get`, N4 slice 2).
    public let icons = IconStore()
    /// A scrubbed number's value over its row while the drag goes on, and a note the core gave
    /// for an edit (held to its range, refused), shown for a moment by its row.
    let scrubLabel = CATextLayer()
    let noteLabel = CATextLayer()
    var noteTask: Task<Void, Never>?
    /// When the run animation's colours began to fade, for the generation shown (8.6): the core
    /// keeps `anim.on` while they show and drops it once faded; the shell fades them itself.
    var fadeBegin: CFTimeInterval?
    var fadeGen: UInt64?
    /// Culling (N3 slice 4): face, wire, tag and frame layers outside `culledFor` widened by
    /// `cullMargin` views are hidden, so the compositor skips them; their pictures and paths are
    /// kept, so a face coming into view shows at once. Recomputed when the view has moved a
    /// quarter of itself since the last time.
    public var culling = true
    /// The wires not drawn (the bench's `--hide-wires`, the N4 device gate: on the iPad's hitch
    /// trace it tells the wires' cost from the faces'). Never set by the apps.
    public var wiresHidden = false {
        didSet { wireRoot.isHidden = wiresHidden }
    }
    /// The bench's wire switches (`WireSwitch`); every wire is drawn again when they change.
    public var wireSwitches: Set<WireSwitch> = [] {
        didSet {
            guard wireSwitches != oldValue else { return }
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            for nid in wireGeoms.keys { if let w = model.wires[nid]?.item { pathWire(w) } }
            cull(force: true)
            CATransaction.commit()
            pumpTiles()
        }
    }
    public private(set) var culledFor: CGRect?
    public private(set) var hiddenCount = 0
    let cullMargin: CGFloat = 0.5

    public init(model: DocumentModel, host: CALayer, screenScale: CGFloat) {
        self.model = model
        self.link = CanvasLink(model: model)
        self.host = host
        self.screenScale = max(screenScale, 1)
        self.pictures = Pictures(model: model)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        world.anchorPoint = .zero
        world.position = .zero
        world.bounds = CGRect(x: 0, y: 0, width: 1, height: 1)
        for l in [frameRoot, groupRoot, wireRoot, nodeRoot, tagRoot, overlay] {
            l.anchorPoint = .zero
            l.position = .zero
            l.bounds = CGRect(x: 0, y: 0, width: 1, height: 1)
            world.addSublayer(l)
        }
        for t in [scrubLabel, noteLabel] {
            t.anchorPoint = .zero
            t.isHidden = true
            t.contentsScale = self.screenScale * 2
            t.cornerRadius = 4
            t.alignmentMode = .center
            t.truncationMode = .end
            overlay.addSublayer(t)
        }
        for s in [hoverRing, marquee, dragWire, dragTarget, ghost] {
            s.anchorPoint = .zero
            s.position = .zero
            s.fillColor = nil
            s.isHidden = true
            overlay.addSublayer(s)
        }
        hoverWireLayer.anchorPoint = .zero
        hoverWireLayer.fillColor = nil
        hoverWireLayer.lineCap = .round
        hoverWireLayer.lineJoin = .round
        hoverWireLayer.isHidden = true
        hoverWireLayer.zPosition = 2
        wireRoot.addSublayer(hoverWireLayer)
        bendRingLayer.anchorPoint = .zero
        bendRingLayer.zPosition = 3
        wireRoot.addSublayer(bendRingLayer)
        host.addSublayer(world)
        CATransaction.commit()
        model.onChange = { [weak self] c in self?.apply(c) }
        observeTheme()
        if model.applied > 0 { rebuildAll() }
    }

    // MARK: - Following the model

    func observeTheme() {
        withObservationTracking {
            _ = model.theme
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in
                self?.themeChanged()
                self?.observeTheme()
            }
        }
        themeChanged()
    }

    func themeChanged() {
        let c = CanvasColors(model.theme)
        guard c != colors else { return }
        colors = c
        themeGen += 1
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        host.backgroundColor = colors.canvas
        for nid in wireGeoms.keys { if let w = model.wires[nid]?.item { pathWire(w) } }
        for nid in frames.keys { if let f = model.frames[nid]?.item { placeFrame(f) } }
        for nid in groups.keys { if let g = model.groups[nid]?.item { placeGroup(g) } }
        for nid in marks.keys { if let m = model.marks[nid]?.item { placeMark(m) } }
        placeTags()
        updateSelection()
        CATransaction.commit()
        // every face is drawn again in the new colours (their sizes don't change)
        needDraw.formUnion(faces.keys)
        pump()
        pumpTiles()
    }

    /// Everything the model has, as if it had all just changed.
    func rebuildAll() {
        var all = Set<UInt64>()
        all.formUnion(model.faces.order)
        all.formUnion(model.wires.order)
        all.formUnion(model.nets.order)
        all.formUnion(model.frames.order)
        all.formUnion(model.groups.order)
        all.formUnion(model.marks.order)
        apply(SceneChange(gen: model.gen, full: true, changed: all, removed: [], selectionChanged: true, viewChanged: true))
    }

    /// One apply of the model: layers by nid.
    public func apply(_ c: SceneChange) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        for nid in c.removed { remove(nid) }
        var netsTouched = false
        var orderTouched = false
        for nid in c.changed {
            if let f = model.faces[nid]?.item {
                if let e = faces[nid] {
                    let moved = e.face.pos != f.pos
                    e.face = f
                    if moved { held[nid] = nil }
                    if moved { placeFace(e) }
                    if e.laidRev != f.rev { needLayout.insert(nid) }
                } else {
                    let e = FaceEntry(nid: nid, face: f)
                    e.layer.anchorPoint = .zero
                    e.layer.contentsGravity = .resize
                    e.layer.isOpaque = false
                    faces[nid] = e
                    nodeRoot.addSublayer(e.layer)
                    orderTouched = true
                    needLayout.insert(nid)
                }
                faceById[f.id] = nid
                netsTouched = true
            } else if let w = model.wires[nid]?.item {
                pathWire(w)
            } else if model.nets[nid] != nil {
                netsTouched = true
            } else if let fr = model.frames[nid]?.item {
                placeFrame(fr)
            } else if let g = model.groups[nid]?.item {
                placeGroup(g)
                orderTouched = true
            } else if let m = model.marks[nid]?.item {
                placeMark(m)
            }
        }
        if orderTouched { reorder() }
        fetchIcons(c.changed)
        if netsTouched || !c.removed.isEmpty { placeTags() }
        if c.selectionChanged || c.full { updateSelection() }
        showBendRings()
        updateWashes(c.changed)
        if c.viewChanged || c.full { honourFit() }
        cull(force: true)
        pump()
        pumpTiles()
        // nothing to draw (no face changed, or an empty graph): what is shown is already current
        if idle { onDrawn?([]) }
    }

    /// Fetch the icons the changed faces name that the store hasn't; draw those faces again when
    /// they come.
    func fetchIcons(_ changed: Set<UInt64>) {
        var want = Set<String>()
        for nid in changed { if let e = faces[nid], icons.path(e.face.header.icon) == nil { want.insert(e.face.header.icon) } }
        let names = icons.claim(want)
        if names.isEmpty { return }
        let core = link.core
        let doc = link.requestDoc
        let store = icons
        iconFetches += 1
        Task { [weak self] in
            _ = await store.fetch(names, core: core, doc: doc)
            guard let self else { return }
            iconFetches -= 1
            let set = Set(names)
            for e in faces.values where set.contains(e.face.header.icon) {
                e.drawnRev = nil
                needDraw.insert(e.nid)
            }
            pump()
        }
    }

    func remove(_ nid: UInt64) {
        if let e = faces.removeValue(forKey: nid) {
            e.layer.removeFromSuperlayer()
            faceById = faceById.filter { $0.value != nid }
            needLayout.remove(nid)
            needDraw.remove(nid)
        }
        removeWire(nid)
        for l in tags.removeValue(forKey: nid) ?? [] { l.removeFromSuperlayer() }
        frames.removeValue(forKey: nid)?.removeFromSuperlayer()
        groups.removeValue(forKey: nid)?.removeFromSuperlayer()
        marks.removeValue(forKey: nid)?.removeFromSuperlayer()
    }

    /// Faces in the scene's drawing order (the last on top); open groups outer first.
    func reorder() {
        var k: UInt32 = 0
        for nid in model.faces.order {
            if let e = faces[nid] {
                e.layer.zPosition = CGFloat(k)
                k += 1
            }
        }
        for (nid, l) in groups {
            l.zPosition = CGFloat(model.groups[nid]?.item.depth ?? 0)
        }
    }

    // MARK: - Faces: layout, measuring, drawing (on workers)

    /// The raster scale for a zoom: powers of two of the screen's, so a small zoom change never
    /// redraws (15.2); from half the screen's to twice it.
    public func rasterScale(forZoom z: CGFloat) -> CGFloat {
        let s = screenScale * pow(2, ceil(log2(max(z, 0.01))))
        return min(max(s, 0.5), screenScale * 2)
    }

    /// The visible part of the graph, with a margin of a fifth of the view each side.
    public var visibleRect: CGRect {
        let v = viewport
        let r = CGRect(x: -v.pan.x / v.zoom, y: -v.pan.y / v.zoom, width: viewSize.width / v.zoom, height: viewSize.height / v.zoom)
        return r.insetBy(dx: -r.width * 0.2, dy: -r.height * 0.2)
    }

    struct Job: Sendable {
        var nid: UInt64
        var face: Face
        var layout: FaceLayout?
        var relayout: Bool
        var scale: CGFloat?
        var icon: IconPath?
    }

    struct Done: @unchecked Sendable {
        // CGImage is immutable and thread-safe
        var nid: UInt64
        var rev: UInt64
        var layout: FaceLayout
        var image: CGImage?
        var scale: CGFloat?
    }

    /// Send the faces waiting to a worker: lay out those whose `rev` changed and report them, draw
    /// those that need it, then put the pictures on their layers. One batch at a time; what comes
    /// while one is out waits for the next.
    func pump() {
        if working || (needLayout.isEmpty && needDraw.isEmpty) { return }
        let visible = visibleRect
        let want = rasterScale(forZoom: viewport.zoom)
        var jobs: [Job] = []
        let ids = needLayout.union(needDraw)
        needLayout = []
        needDraw = []
        for nid in ids {
            guard let e = faces[nid] else { continue }
            let relayout = e.laidRev != e.face.rev || e.layout == nil
            // a visible face at the zoom's scale; one out of sight at the lowest, once
            let seen = e.rect.intersects(visible)
            let scale: CGFloat? = seen ? want : (e.drawnRev == e.face.rev && e.drawnTheme == themeGen ? nil : min(want, 1))
            if !relayout && scale == nil { continue }
            jobs.append(Job(nid: nid, face: e.face, layout: relayout ? nil : e.layout, relayout: relayout, scale: scale, icon: icons.path(e.face.header.icon)))
        }
        if jobs.isEmpty { return }
        working = true
        counts.batches += 1
        let colors = colors
        let fonts = fonts
        let core = link.core
        let doc = link.hotDoc
        let rdoc = link.requestDoc
        Task.detached(priority: .userInitiated) { [weak self] in
            let done = LayerCanvas.work(jobs, colors: colors, fonts: fonts)
            // measurements go to the core from here: plum_geometry_set copies and returns
            let measured: [FaceMeasure] = zip(jobs, done).compactMap { j, d in
                guard j.relayout else { return nil }
                let m = d.layout.measure(face: d.nid, rev: d.rev)
                return FaceMeasure(face: m.face, rev: m.rev, width: m.width, height: m.height, header: m.header,
                                   inputs: m.inputs.map { Optional($0) }, outputs: m.outputs.map { Optional($0) }, rows: m.rows)
            }
            var reported = false
            if !measured.isEmpty {
                reported = (try? core.geometrySet(doc: doc, faces: measured)) != nil
                // the header's parts (the more-button, the title, the icon) after it, which the
                // typed call can't carry (N4 slice 2)
                let parts = zip(jobs, done).compactMap { j, d in j.relayout ? (id: j.face.id, parts: d.layout.parts, footer: d.layout.footerRects) : nil }
                await CanvasLink.reportParts(core: core, doc: rdoc, faces: parts)
            }
            await self?.finish(done, laid: jobs.filter(\.relayout).count, reported: reported ? 1 : 0)
        }
    }

    nonisolated static func work(_ jobs: [Job], colors: CanvasColors, fonts: FaceFonts) -> [Done] {
        let out = UnsafeMutableBufferPointer<Done?>.allocate(capacity: jobs.count)
        out.initialize(repeating: nil)
        defer {
            _ = out.deinitialize()
            out.deallocate()
        }
        nonisolated(unsafe) let o = out
        DispatchQueue.concurrentPerform(iterations: jobs.count) { k in
            let j = jobs[k]
            let layout = j.layout ?? FaceLayout.make(j.face, fonts: fonts)
            let image = j.scale.flatMap { FaceRenderer.render(j.face, layout, colors: colors, fonts: fonts, scale: $0, icon: j.icon) }
            o[k] = Done(nid: j.nid, rev: j.face.rev, layout: layout, image: image, scale: j.scale)
        }
        return out.compactMap { $0 }
    }

    func finish(_ done: [Done], laid: Int, reported: Int) {
        working = false
        counts.facesLaidOut += laid
        counts.reports += reported
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        var touched = Set<UInt64>()
        for d in done {
            guard let e = faces[d.nid] else { continue }
            if e.face.rev != d.rev {
                // it changed again while drawn: do it again
                needLayout.insert(d.nid)
                continue
            }
            e.layout = d.layout
            e.laidRev = d.rev
            if let img = d.image, let s = d.scale {
                e.layer.contents = img
                e.layer.contentsScale = s
                e.drawnScale = s
                e.drawnRev = d.rev
                e.drawnTheme = themeGen
                counts.facesDrawn += 1
            }
            placeFace(e)
            touched.insert(d.nid)
        }
        placeTags()
        updateSelection()
        updateWashes(touched)
        for nid in touched { if let e = faces[nid] { updatePreview(e) } }
        cull(force: true)
        CATransaction.commit()
        let vis = visibleRect
        if firstFrameAt == nil, !faces.isEmpty, faces.values.allSatisfy({ $0.layout != nil && ($0.drawnRev != nil || !$0.rect.intersects(vis)) }) {
            firstFrameAt = .now
        }
        onDrawn?(touched)
        pump()
    }

    /// The face's layer where its scene position and layout say.
    func placeFace(_ e: FaceEntry) {
        let m = FaceMetrics.margin
        let size = e.layout?.size ?? CGSize(width: 220, height: 160)
        var p = CGPoint(x: e.face.pos.x, y: e.face.pos.y)
        if let start = moving[e.nid], let off = moveOffset {
            p = CGPoint(x: start.x + off.x, y: start.y + off.y)
        } else if let h = held[e.nid] {
            p = h
        }
        e.layer.bounds = CGRect(x: 0, y: 0, width: size.width + 2 * m, height: size.height + 2 * m)
        e.layer.position = CGPoint(x: p.x - m, y: p.y - m)
        if let pv = e.preview, let r = e.layout?.preview {
            pv.frame = r.offsetBy(dx: m, dy: m)
        }
        if let w = e.wash, let L = e.layout {
            w.frame = CGRect(x: m, y: m + L.header, width: size.width, height: max(size.height - L.header, 0))
        }
        if let s = e.select {
            s.frame = CGRect(x: 0, y: 0, width: size.width + 2 * m, height: size.height + 2 * m)
            s.path = CGPath(roundedRect: CGRect(x: m - 3.5, y: m - 3.5, width: size.width + 7, height: size.height + 7),
                            cornerWidth: FaceMetrics.corner + 3, cornerHeight: FaceMetrics.corner + 3, transform: nil)
        }
    }

    // MARK: - Selection and run washes

    func updateSelection() {
        let sel = model.selection
        let nodes = Set(sel.nodes)
        let groupsSel = Set(sel.groups)
        for e in faces.values {
            let on = nodes.contains(e.face.id) || (e.face.proxy.map { groupsSel.contains($0.group) } ?? false)
            if on {
                let s = e.select ?? {
                    let s = CAShapeLayer()
                    s.fillColor = nil
                    s.lineWidth = 2.5
                    s.anchorPoint = .zero
                    e.layer.addSublayer(s)
                    e.select = s
                    return s
                }()
                s.strokeColor = colors.accent
                s.isHidden = false
                placeFace(e)
            } else {
                e.select?.isHidden = true
            }
        }
    }

    /// The run's fill while it shows (contract 8.6): a sublayer over the body in the state's
    /// colour, at the table's amount; the running pulse is a Core Animation animation, so the
    /// compositor runs it and nothing is drawn per frame.
    func updateWashes(_ changed: Set<UInt64>) {
        let on = model.anim.on
        for nid in changed {
            guard let e = faces[nid], e.layout != nil else { continue }
            let shown = on ? e.face.shown?.word : nil
            guard let shown else {
                e.wash?.removeFromSuperlayer()
                e.wash = nil
                continue
            }
            let (token, amount): (String, Float) = switch shown {
            case .waiting: ("field", 0.55)
            case .running: ("state_running", 0.42)
            case .done: ("state_done", 0.40)
            case .cached: ("state_cached", 0.24)
            case .failed: ("state_error", 0.45)
            case .skipped: ("state_skipped", 0.60)
            }
            let w = e.wash ?? {
                let w = CALayer()
                w.anchorPoint = .zero
                w.cornerRadius = FaceMetrics.corner
                w.maskedCorners = [.layerMinXMaxYCorner, .layerMaxXMaxYCorner]
                e.layer.insertSublayer(w, at: 0)
                e.wash = w
                return w
            }()
            w.backgroundColor = colors.token(token, RGBA(128, 128, 128, 255))
            w.opacity = amount
            w.removeAnimation(forKey: "pulse")
            w.removeAnimation(forKey: "fade")
            if let b = fadeBegin, fadeGen == model.anim.generation, shown != .running, shown != .waiting {
                fade(w, from: amount, begin: b)
            }
            // Reduce Motion: a running face holds its wash rather than pulsing (N4 slice 4)
            if shown == .running, !Self.reduceMotion {
                let a = CABasicAnimation(keyPath: "opacity")
                a.fromValue = 0.28
                a.toValue = 0.56
                a.duration = Double(model.anim.pulseMs) / 2000
                a.autoreverses = true
                a.repeatCount = .infinity
                w.add(a, forKey: "pulse")
            }
            placeFace(e)
        }
        noteRunFinished()
        // what pulses without end, for the status (Activity)
        Activity.shared.count("pulse", faces.values.reduce(0) { $0 + ($1.wash?.animation(forKey: "pulse") != nil ? 1 : 0) })
    }

    /// The run (or its replay) is over once no face shows waiting or running: hold the colours
    /// for `holdMs`, then fade them over `fadeMs` (8.6), once per generation. The core drops
    /// `anim.on` at about the same time, and the washes go then.
    func noteRunFinished() {
        let a = model.anim
        guard a.on else {
            fadeBegin = nil
            fadeGen = nil
            return
        }
        if fadeGen != a.generation { fadeBegin = nil }
        let busy = faces.values.contains { e in
            guard let w = e.face.shown?.word else { return false }
            return w == .waiting || w == .running
        }
        if busy {
            fadeBegin = nil
            fadeGen = nil
            return
        }
        guard fadeBegin == nil else { return }
        let begin = CACurrentMediaTime() + Double(a.holdMs) / 1000
        fadeBegin = begin
        fadeGen = a.generation
        for e in faces.values {
            guard let w = e.wash else { continue }
            fade(w, from: w.opacity, begin: begin)
        }
    }

    func fade(_ w: CALayer, from: Float, begin: CFTimeInterval) {
        let f = CABasicAnimation(keyPath: "opacity")
        f.fromValue = from
        f.toValue = 0
        f.beginTime = begin
        f.duration = Double(model.anim.fadeMs) / 1000
        f.fillMode = .forwards
        f.isRemovedOnCompletion = false
        w.add(f, forKey: "fade")
        Activity.shared.timed("fade", begin - CACurrentMediaTime() + f.duration)
    }

    // MARK: - Previews

    /// A face's rgba8 preview: a sublayer whose contents come from the media service, made on a
    /// worker (`Pictures`), once per handle. Faded while the scene says so.
    func updatePreview(_ e: FaceEntry) {
        // a sound's waveform (N4 slice 5): the engine's peaks, drawn once on a worker
        if case .peaks(let pk)? = e.face.preview, let r = e.layout?.preview, let handle = Pictures.handle(of: pk.handle) {
            updateWaveform(e, pk, r, handle)
            return
        }
        guard case .rgba8(let p)? = e.face.preview, let r = e.layout?.preview, let handle = Pictures.handle(of: p.handle) else {
            e.preview?.removeFromSuperlayer()
            e.preview = nil
            e.previewKey = nil
            return
        }
        let pv = e.preview ?? {
            let l = CALayer()
            l.anchorPoint = .zero
            l.contentsGravity = .resizeAspect
            l.cornerRadius = 3
            l.masksToBounds = true
            e.layer.addSublayer(l)
            e.preview = l
            return l
        }()
        pv.frame = r.offsetBy(dx: FaceMetrics.margin, dy: FaceMetrics.margin)
        pv.opacity = p.faded ? 0.45 : 1
        listTag(pv, p)
        if e.previewKey == handle { return }
        e.previewKey = handle
        if let got = pictures.cached(handle) {
            pv.contents = got.image
            return
        }
        pv.contents = nil
        let nid = e.nid
        Task { [weak self] in
            guard let self, let got = await pictures.load(handle) else { return }
            // still the face's picture?
            guard let e = faces[nid], e.previewKey == handle, let l = e.preview else { return }
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            l.contents = got.image
            CATransaction.commit()
        }
    }

    /// The words on a face's picture when its value is a list (scene `items` above 1): the
    /// picture's own name and how many there are ("Lab · 11 pictures", or "11 items" unnamed), so
    /// a list of named pictures reads as one on the face; the pop-out steps through them by name.
    /// None for a single picture.
    public nonisolated static func listTagText(_ p: PlumCraftModels.Preview.Picture) -> String? {
        guard p.items > 1 else { return nil }
        let name = (p.caption ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        // a list of lists: each level's count, outermost first ("3 × 2 items"; contract 8.7)
        let count = (p.levels ?? []).count > 1 ? (p.levels ?? []).map(String.init).joined(separator: " × ") : "\(p.items)"
        return name.isEmpty ? "\(count) items" : "\(name) · \(count) pictures"
    }

    private func listTag(_ pv: CALayer, _ p: PlumCraftModels.Preview.Picture) {
        let tag = pv.sublayers?.first { $0.name == "list-tag" } as? CATextLayer
        guard let text = Self.listTagText(p) else {
            tag?.removeFromSuperlayer()
            return
        }
        let t = tag ?? {
            let l = CATextLayer()
            l.name = "list-tag"
            l.anchorPoint = .zero
            l.fontSize = 10
            l.cornerRadius = 3
            l.alignmentMode = .left
            l.truncationMode = .end
            l.contentsScale = screenScale * 2
            pv.addSublayer(l)
            return l
        }()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        t.string = " \(text) "
        t.foregroundColor = CGColor(gray: 1, alpha: 1)
        t.backgroundColor = CGColor(gray: 0, alpha: 0.55)
        let w = min(pv.bounds.width - 4, CGFloat(text.count) * 6.4 + 12)
        t.frame = CGRect(x: 2, y: pv.bounds.height - 16, width: max(0, w), height: 14)
        CATransaction.commit()
    }

    /// A face's waveform thumbnail: the peaks rendition (min and max a column, a lane a channel)
    /// lent by the core, drawn into a picture on a worker in the theme's waveform colours.
    private func updateWaveform(_ e: FaceEntry, _ pk: PlumCraftModels.Preview.Peaks, _ r: CGRect, _ handle: String) {
        let pv = e.preview ?? {
            let l = CALayer()
            l.anchorPoint = .zero
            l.contentsGravity = .resize
            l.cornerRadius = 3
            l.masksToBounds = true
            e.layer.addSublayer(l)
            e.preview = l
            return l
        }()
        pv.frame = r.offsetBy(dx: FaceMetrics.margin, dy: FaceMetrics.margin)
        pv.opacity = pk.faded ? 0.45 : 1
        let key = "peaks|\(handle)|\(colors.name)"
        if e.previewKey == key { return }
        e.previewKey = key
        if let got = pictures.cached(key) {
            pv.contents = got.image
            return
        }
        pv.contents = nil
        let nid = e.nid
        let (fg, bg) = (colors.token("waveform", RGBA(47, 163, 154, 255)), colors.token("waveform_bg", RGBA(236, 240, 238, 255)))
        let scale = 2
        let (w, h) = (max(1, Int(r.width) * scale), max(1, Int(r.height) * scale))
        Task { [weak self] in
            guard let self, let (note, data) = await pictures.lend(handle, want: "peaks") else { return }
            let ch = max(1, Int(note.channels ?? 1)), bins = Int(note.bins ?? 0)
            let made = await Task.detached(priority: .utility) { () -> LoadedPicture? in
                LayerCanvas.drawWaveform(data, channels: ch, bins: bins, w: w, h: h, fg: fg, bg: bg)
            }.value
            guard let made, let e = faces[nid], e.previewKey == key, let l = e.preview else { return }
            pictures.keep(key, made)
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            l.contents = made.image
            CATransaction.commit()
        }
    }

    /// Peaks (`<f4` min and max, channel-major) as a picture: a lane a channel, at most four.
    nonisolated static func drawWaveform(_ data: Data, channels: Int, bins: Int, w: Int, h: Int, fg: CGColor, bg: CGColor) -> LoadedPicture? {
        guard bins > 0, let space = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        ctx.setFillColor(bg)
        ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
        ctx.setStrokeColor(fg)
        ctx.setLineWidth(max(1, CGFloat(w) / CGFloat(bins)))
        let lanes = min(channels, 4)
        let lh = CGFloat(h) / CGFloat(lanes)
        data.withUnsafeBytes { raw in
            let f = raw.bindMemory(to: Float32.self)
            for c in 0..<lanes {
                let mid = lh * (CGFloat(c) + 0.5)
                for b in 0..<bins {
                    let k = (c * bins + b) * 2
                    guard k + 1 < f.count else { break }
                    let lo = CGFloat(max(-1, min(1, f[k].isFinite ? f[k] : 0))), hi = CGFloat(max(-1, min(1, f[k + 1].isFinite ? f[k + 1] : 0)))
                    let x = (CGFloat(b) + 0.5) / CGFloat(bins) * CGFloat(w)
                    ctx.move(to: CGPoint(x: x, y: mid + lo * lh / 2))
                    ctx.addLine(to: CGPoint(x: x, y: mid + hi * lh / 2 + 0.5))
                }
            }
        }
        ctx.strokePath()
        guard let img = ctx.makeImage() else { return nil }
        return LoadedPicture(image: img, width: w, height: h)
    }

    // MARK: - Culling

    /// Hide what is far outside the view (N3 slice 4, 15.2's "large graphs need culling"); the
    /// compositor then skips it. `force`: the scene changed, so look again however little the view
    /// moved. Every layer stays, with its picture or path: showing it again is one flag.
    public func cull(force: Bool) {
        let vis = visibleRect
        if !force, let last = culledFor, abs(last.midX - vis.midX) < vis.width * 0.25 / 1.4, abs(last.midY - vis.midY) < vis.height * 0.25 / 1.4,
           abs(last.width - vis.width) < vis.width * 0.25 {
            return
        }
        culledFor = vis
        let keep = vis.insetBy(dx: -vis.width * cullMargin, dy: -vis.height * cullMargin)
        var hidden = 0
        let all = !culling
        for e in faces.values {
            let h = !all && !e.rect.insetBy(dx: -FaceMetrics.margin, dy: -FaceMetrics.margin).intersects(keep) && moving[e.nid] == nil
            if e.layer.isHidden != h { e.layer.isHidden = h }
            if h { hidden += 1 }
        }
        hidden += cullWires(keep: keep, all: all)
        for ls in tags.values {
            for l in ls {
                let h = !all && !l.frame.intersects(keep)
                if l.isHidden != h { l.isHidden = h }
            }
        }
        hiddenCount = hidden
    }

    // MARK: - Wires

    /// A cubic chain (start, then three points a segment) as a path (bend points: `BendRings`).
    static func chain(_ pts: [CGPoint]) -> CGMutablePath {
        let p = CGMutablePath()
        guard let first = pts.first else { return p }
        p.move(to: first)
        var k = 1
        while k + 2 < pts.count {
            p.addCurve(to: pts[k + 2], control1: pts[k], control2: pts[k + 1])
            k += 3
        }
        return p
    }

    // MARK: - Frames, groups, tags, marks

    func labelLayer(_ spec: LabelRenderer.Spec) -> CALayer {
        let l = CALayer()
        l.anchorPoint = .zero
        l.bounds = CGRect(origin: .zero, size: spec.size)
        l.contentsGravity = .resize
        let s = screenScale * 2
        l.contents = LabelRenderer.render(spec, fonts: fonts, scale: s)
        l.contentsScale = s
        return l
    }

    func placeFrame(_ f: Frame) {
        frames[f.nid]?.removeFromSuperlayer()
        let col = colors.color(f.paint)
        let l = CALayer()
        l.anchorPoint = .zero
        l.position = CGPoint(x: f.pos.x, y: f.pos.y)
        l.bounds = CGRect(x: 0, y: 0, width: f.size.x, height: f.size.y)
        l.backgroundColor = col.copy(alpha: 0.08)
        l.borderColor = col.copy(alpha: 0.7)
        l.borderWidth = 1.5
        l.cornerRadius = 6
        let tabW = min(max(f.size.x, 22), 180)
        let tab = labelLayer(.init(text: "☰  " + f.title, size: CGSize(width: tabW, height: 22), fill: CGColorBox(col.copy(alpha: 0.85) ?? col),
                                   stroke: nil, ink: CGColorBox(colors.headerText), corner: 5, mono: false, bold: false))
        tab.position = CGPoint(x: 0, y: -22)
        l.addSublayer(tab)
        // the corner handle
        let h = CAShapeLayer()
        let p = CGMutablePath()
        for k in 1...3 {
            let d = CGFloat(k) * 4
            p.move(to: CGPoint(x: f.size.x - d - 2, y: f.size.y - 2)); p.addLine(to: CGPoint(x: f.size.x - 2, y: f.size.y - d - 2))
        }
        h.path = p
        h.strokeColor = col.copy(alpha: 0.8)
        h.lineWidth = 1.2
        l.addSublayer(h)
        frameRoot.addSublayer(l)
        frames[f.nid] = l
    }

    /// The widest an open group's band text is drawn (graph units): its chevron and name start
    /// the band, and a picture this wide stays well inside the label picture's limit at any scale
    /// the canvas uses (B156).
    static let bandText: CGFloat = 1024

    /// For tests: an open group's band as drawn, by the group's nid: the band's width and whether
    /// its chevron and name drew (B156).
    public func groupBand(_ nid: UInt64) -> (width: CGFloat, text: Bool)? {
        guard let band = groups[nid]?.sublayers?.first(where: { $0.name == "band" }) else { return nil }
        let text = band.sublayers?.first(where: { $0.name == "band-text" })
        return (band.bounds.width, text?.contents != nil)
    }

    func placeGroup(_ g: Group) {
        groups[g.nid]?.removeFromSuperlayer()
        groups[g.nid] = nil
        guard !g.collapsed, let r = g.rect else { return }
        let col = colors.group
        let l = CALayer()
        l.anchorPoint = .zero
        l.position = CGPoint(x: r.x0, y: r.y0)
        l.bounds = CGRect(x: 0, y: 0, width: r.width, height: r.height)
        l.backgroundColor = colors.groupBody.copy(alpha: 0.35)
        l.borderColor = col.copy(alpha: 0.8)
        l.borderWidth = 1.5
        l.cornerRadius = 8
        // the band: its colour a plain layer the group's width, its chevron and name a picture
        // no wider than `Self.bandText` (B156: drawn whole, a group wider than 4096 units, the
        // whole of a large graph, was past the label picture's limit and drew no band at all)
        let band = CALayer()
        band.anchorPoint = .zero
        band.position = .zero
        band.bounds = CGRect(x: 0, y: 0, width: r.width, height: 28)
        band.backgroundColor = col.copy(alpha: 0.85) ?? col
        band.cornerRadius = 8
        band.name = "band"
        let text = labelLayer(.init(text: "▾  " + g.name, size: CGSize(width: min(r.width, Self.bandText), height: 28), fill: nil,
                                    stroke: nil, ink: CGColorBox(colors.headerText), corner: 8, mono: false, bold: true, inset: 10))
        text.position = .zero
        text.name = "band-text"
        band.addSublayer(text)
        l.addSublayer(band)
        l.zPosition = CGFloat(g.depth)
        groupRoot.addSublayer(l)
        groups[g.nid] = l
    }

    func placeMark(_ m: Mark) {
        marks[m.nid]?.removeFromSuperlayer()
        marks[m.nid] = nil
        guard let at = m.at else { return }
        let tone: CGColor = switch m.tone.word {
        case .warn?: colors.warning
        case .error?: colors.error
        default: colors.accent
        }
        let w = TextLine.width(m.label, fonts.small) + 14
        let l = labelLayer(.init(text: m.label, size: CGSize(width: w, height: 18), fill: CGColorBox(colors.field), stroke: CGColorBox(tone),
                                 ink: CGColorBox(colors.text), corner: 9, mono: false, bold: false, inset: 7))
        l.position = CGPoint(x: at.x - w / 2, y: at.y - 9)
        tagRoot.addSublayer(l)
        marks[m.nid] = l
    }

    /// Net tags at their pins (8.8), where the core puts them for its hit test (`geometry.rs`'s
    /// estimate: 6.6 a character plus 17 wide, 17 high).
    func placeTags() {
        for (_, ls) in tags { for l in ls { l.removeFromSuperlayer() } }
        tags = [:]
        for n in model.nets.items {
            var ls: [CALayer] = []
            let col = colors.color(n.paint)
            let w = 6.6 * CGFloat(min(n.key.name.count, 64)) + 17
            let spec = LabelRenderer.Spec(text: n.key.name, size: CGSize(width: w, height: 17), fill: CGColorBox(n.selected ? (col.copy(alpha: 0.45) ?? col) : colors.field),
                                          stroke: CGColorBox(col), ink: CGColorBox(colors.text), corner: 8.5, mono: false, bold: false, inset: 4, centred: true)
            if let p = pinPoint(face: n.driver.face, output: true, pin: n.driver.pin) {
                // the driver's tag says how many inputs the net feeds ("picture ×3", N4 slice 3),
                // as wide as the core's hit test takes it
                let text = n.driver.text ?? n.key.name
                var ds = spec
                ds.text = text
                ds.size.width = 6.6 * CGFloat(min(text.count, 64)) + 17
                let l = labelLayer(ds)
                l.position = CGPoint(x: p.x + n.driver.at.x, y: p.y + n.driver.at.y - 8.5)
                tagRoot.addSublayer(l)
                ls.append(l)
            }
            for u in n.uses {
                guard let p = pinPoint(face: u.face, output: false, pin: u.pin) else { continue }
                let l = labelLayer(spec)
                l.position = CGPoint(x: p.x + u.at.x - w, y: p.y + u.at.y - 8.5)
                tagRoot.addSublayer(l)
                ls.append(l)
            }
            tags[n.nid] = ls
        }
    }

    /// A pin's point in graph units, from the face's layout.
    public func pinPoint(face id: String, output: Bool, pin: Int) -> CGPoint? {
        guard let nid = faceById[id], let e = faces[nid], let L = e.layout else { return nil }
        let ys = output ? L.outputPins : L.inputPins
        guard pin >= 0, pin < ys.count else { return nil }
        return CGPoint(x: e.face.pos.x + (output ? L.size.width : 0), y: e.face.pos.y + ys[pin])
    }

    // MARK: - The view

    func applyTransform(animated: Bool = false) {
        CATransaction.begin()
        if animated {
            Activity.shared.timed("viewport", 0.28)
            CATransaction.setAnimationDuration(0.28)
            CATransaction.setAnimationTimingFunction(CAMediaTimingFunction(name: .easeOut))
        } else {
            CATransaction.setDisableActions(true)
        }
        let v = viewport
        world.setAffineTransform(CGAffineTransform(a: v.zoom, b: 0, c: 0, d: v.zoom, tx: v.pan.x, ty: v.pan.y))
        cull(force: false)
        CATransaction.commit()
        onViewport?(v)
        followers?()
    }

    /// The host's own followers of the transform (a field editor over a row), beside the app's
    /// `onViewport`.
    var followers: (() -> Void)?

    public func setViewport(_ v: Viewport, animated: Bool = false) {
        viewport = Viewport(pan: v.pan, zoom: min(max(v.zoom, Viewport.minZoom), Viewport.maxZoom))
        applyTransform(animated: animated)
        scheduleSettle()
    }

    /// Zoom by `factor`, keeping the view point `at` where it is.
    public func zoom(by factor: CGFloat, at v: CGPoint, animated: Bool = false) {
        let g = viewport.toGraph(v)
        let z = min(max(viewport.zoom * factor, Viewport.minZoom), Viewport.maxZoom)
        setViewport(Viewport(pan: CGPoint(x: v.x - g.x * z, y: v.y - g.y * z), zoom: z), animated: animated)
    }

    /// Zoom to `z` about the view's centre.
    public func zoom(to z: CGFloat, animated: Bool = true) {
        zoom(by: z / viewport.zoom, at: CGPoint(x: viewSize.width / 2, y: viewSize.height / 2), animated: animated)
    }

    /// Fit `rect` (graph units) into the view, zoom held to `lo…hi`.
    public func fit(_ r: CGRect, lo: CGFloat = 0.2, hi: CGFloat = 1, animated: Bool = true) {
        guard r.width > 0, r.height > 0, viewSize.width > 0, viewSize.height > 0 else { return }
        let z = min(max(min(viewSize.width / r.width, viewSize.height / r.height), lo), hi)
        let pan = CGPoint(x: viewSize.width / 2 - r.midX * z, y: viewSize.height / 2 - r.midY * z)
        rawPan = nil
        setViewport(Viewport(pan: pan, zoom: z), animated: animated)
    }

    /// The core's fit, once per `seq` (8.9); and its reveal (N4 slice 3), centring the face at
    /// the zoom the view has (at least 0.6), with the selection's ring for a moment.
    func honourFit() {
        if let r = model.view.reveal, r.seq != lastRevealSeq {
            let first = lastRevealSeq == nil && firstFrameAt == nil
            lastRevealSeq = r.seq
            if !first {
                let z = max(viewport.zoom, 0.6)
                let pan = CGPoint(x: viewSize.width / 2 - (r.rect.x0 + r.rect.width / 2) * z, y: viewSize.height / 2 - (r.rect.y0 + r.rect.height / 2) * z)
                rawPan = nil
                setViewport(Viewport(pan: pan, zoom: z), animated: true)
                if let nid = faceById[r.node] ?? faceById["group:" + r.node], let l = faces[nid]?.layer {
                    let a = CABasicAnimation(keyPath: "opacity")
                    a.fromValue = 0.35
                    a.toValue = 1
                    a.duration = 0.25
                    a.repeatCount = 2
                    l.add(a, forKey: "reveal")
                    Activity.shared.timed("reveal", 0.5)
                }
            }
        }
        guard let f = model.view.fit, f.seq != lastFitSeq else { return }
        lastFitSeq = f.seq
        fit(CGRect(x: f.rect.x0, y: f.rect.y0, width: f.rect.width, height: f.rect.height), animated: firstFrameAt != nil)
    }

    /// View › Fit: the core works out the rect (`canvas.fit`); its reply and the scene carry it.
    public func requestFit() {
        Task {
            if let r = try? await model.run("canvas.fit"), let rect = r["rect"].array, rect.count == 4,
               let x0 = rect[0].double, let y0 = rect[1].double, let x1 = rect[2].double, let y1 = rect[3].double {
                lastFitSeq = r["seq"].uint64 ?? lastFitSeq
                fit(CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0))
            }
        }
    }

    /// Fit one face (smart zoom): a little room around it, up to zoom 2.
    public func fitFace(_ nid: UInt64) {
        guard let e = faces[nid] else { return }
        fit(e.rect.insetBy(dx: -40, dy: -40), lo: Viewport.minZoom, hi: 2)
    }

    /// Every face's and frame's rect together.
    public var contentBounds: CGRect? {
        var r: CGRect?
        for e in faces.values { r = r.map { $0.union(e.rect) } ?? e.rect }
        for f in model.frames.items {
            let fr = CGRect(x: f.pos.x, y: f.pos.y - 22, width: f.size.x, height: f.size.y + 22)
            r = r.map { $0.union(fr) } ?? fr
        }
        return r
    }

    // MARK: Panning with a rubber band

    /// Pan by `d` view points. While `elastic`, past the content the pan meets a rubber band
    /// (as a scroll view's: it gives less the further it goes) and `endPan` springs it back.
    public func pan(by d: CGPoint, elastic: Bool) {
        let raw = rawPan ?? viewport.pan
        let next = CGPoint(x: raw.x + d.x, y: raw.y + d.y)
        if !elastic {
            rawPan = nil
            setViewport(Viewport(pan: next, zoom: viewport.zoom))
            return
        }
        rawPan = next
        let c = clamped(next)
        let band = { (o: CGFloat, dim: CGFloat) -> CGFloat in
            let s: CGFloat = o < 0 ? -1 : 1
            return s * (1 - 1 / (abs(o) * 0.55 / max(dim, 1) + 1)) * dim
        }
        let shown = CGPoint(x: c.x + band(next.x - c.x, viewSize.width), y: c.y + band(next.y - c.y, viewSize.height))
        viewport.pan = shown
        applyTransform()
        scheduleSettle()
    }

    /// The gesture ended: spring back inside the content.
    public func endPan() {
        rawPan = nil
        let c = clamped(viewport.pan)
        if c != viewport.pan {
            setViewport(Viewport(pan: c, zoom: viewport.zoom), animated: true)
        }
    }

    /// `pan` held so the view's centre stays over the content.
    func clamped(_ pan: CGPoint) -> CGPoint {
        guard let b = contentBounds else { return pan }
        let z = viewport.zoom
        // the view's centre in graph units is (W/2 - pan) / z; keep it within b
        let cx = (viewSize.width / 2 - pan.x) / z
        let cy = (viewSize.height / 2 - pan.y) / z
        let kx = min(max(cx, b.minX), b.maxX)
        let ky = min(max(cy, b.minY), b.maxY)
        return CGPoint(x: viewSize.width / 2 - kx * z, y: viewSize.height / 2 - ky * z)
    }

    // MARK: Settling

    /// 200 ms after the last change of the view: draw the visible faces whose scale is wrong.
    func scheduleSettle() {
        settleTask?.cancel()
        settleTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(200))
            if Task.isCancelled { return }
            self?.settleTask = nil
            self?.settle()
        }
    }

    /// Draw again the visible faces not at the zoom's scale (and the theme's colours).
    @discardableResult
    public func settle() -> Int {
        let want = rasterScale(forZoom: viewport.zoom)
        let vis = visibleRect
        var n = 0
        for e in faces.values where e.layout != nil && e.rect.intersects(vis) && (e.drawnScale != want || e.drawnTheme != themeGen || e.drawnRev != e.face.rev) {
            needDraw.insert(e.nid)
            n += 1
        }
        pump()
        settleTiles()
        return n
    }

    /// Nothing waits to be laid out or drawn (faces or wire tiles), no batch is out, and no icon
    /// is being fetched.
    public var idle: Bool { !working && needLayout.isEmpty && needDraw.isEmpty && iconFetches == 0 && !tileWorking && dirtyTiles.isEmpty }
    var iconFetches = 0
    /// A redraw at the settled zoom is still to come (tests wait for it).
    public var settlePending: Bool { settleTask != nil }

    /// Every face the model has is laid out at its current `rev` (and so reported).
    public var allMeasured: Bool {
        model.faces.order.allSatisfy { nid in faces[nid].map { $0.laidRev == $0.face.rev } ?? false }
    }

    // MARK: - Hover, marquee, a drag's preview

    /// Show what the pointer is over: a ring on a pin, a wire drawn wider.
    public func hover(_ h: HotHit?) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        if hoverWire != nil, h?.kind != .wire || h?.wire != hoverWire {
            hoverWire = nil
        }
        if let h, h.kind == .wire, wireGeoms[h.wire] != nil {
            hoverWire = h.wire
        }
        showHoverWire()
        // a wire's bend points show while the pointer is on it or on one of them (B158)
        let ring = h.flatMap { ($0.kind == .wire || $0.kind == .bend) && wireGeoms[$0.wire] != nil ? $0.wire : nil }
        if ring != ringHover {
            ringHover = ring
            showBendRings()
        }
        if let h, h.kind == .pin {
            let r: CGFloat = 8
            hoverRing.path = CGPath(ellipseIn: CGRect(x: CGFloat(h.x) - r, y: CGFloat(h.y) - r, width: 2 * r, height: 2 * r), transform: nil)
            hoverRing.strokeColor = colors.accent
            hoverRing.lineWidth = 2
            hoverRing.isHidden = false
        } else {
            hoverRing.isHidden = true
        }
    }

    /// The marquee's rect (graph units), or none to hide it.
    public func showMarquee(_ r: CGRect?) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        guard let r else {
            marquee.isHidden = true
            return
        }
        marquee.path = CGPath(rect: r.standardized, transform: nil)
        marquee.strokeColor = colors.accent
        marquee.fillColor = colors.accent.copy(alpha: 0.08)
        marquee.lineWidth = 1 / viewport.zoom
        marquee.isHidden = false
    }

    /// A new wire being dragged, from `a` to `b` (graph units), coloured by whether it fits.
    public func showDragWire(from a: CGPoint?, to b: CGPoint, fits: UInt32) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        guard let a else {
            dragWire.isHidden = true
            return
        }
        let dx = max(abs(b.x - a.x) * 0.5, 30)
        let p = CGMutablePath()
        p.move(to: a)
        p.addCurve(to: b, control1: CGPoint(x: a.x + dx, y: a.y), control2: CGPoint(x: b.x - dx, y: b.y))
        dragWire.path = p
        dragWire.strokeColor = fits == 0 ? colors.textDim : (fits == 1 ? colors.accent : colors.warning)
        dragWire.lineWidth = 2.5
        dragWire.lineDashPattern = fits == 2 ? [6, 4] : nil
        dragWire.isHidden = false
    }

    /// Where a dragged wire would land: `h` a pin (kind pin) with `fits` (1 takes it, 2 through a
    /// Convert, 0 doesn't), or nil to hide it. A pin that fits is ringed and filled in the accent
    /// (a Convert's in the warning colour, dashed); one that doesn't is ringed in the error colour
    /// with a cross, so a drop there says no before it is let go.
    public func showDragTarget(_ h: HotHit?) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        guard let h, h.ok, h.kind == .pin else {
            dragTarget.isHidden = true
            return
        }
        let r: CGFloat = 9
        let c = CGPoint(x: CGFloat(h.x), y: CGFloat(h.y))
        let p = CGMutablePath()
        p.addEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r))
        switch h.fits {
        case 1:
            dragTarget.strokeColor = colors.accent
            dragTarget.fillColor = colors.accent.copy(alpha: 0.35)
            dragTarget.lineDashPattern = nil
        case 2:
            dragTarget.strokeColor = colors.warning
            dragTarget.fillColor = colors.warning.copy(alpha: 0.25)
            dragTarget.lineDashPattern = [3, 2]
        default:
            let k = r * 0.55
            p.move(to: CGPoint(x: c.x - k, y: c.y - k))
            p.addLine(to: CGPoint(x: c.x + k, y: c.y + k))
            p.move(to: CGPoint(x: c.x + k, y: c.y - k))
            p.addLine(to: CGPoint(x: c.x - k, y: c.y + k))
            dragTarget.strokeColor = colors.error
            dragTarget.fillColor = nil
            dragTarget.lineDashPattern = nil
        }
        dragTarget.path = p
        dragTarget.lineWidth = 2.5
        dragTarget.isHidden = false
    }

    /// A dragged frame's, group's or bend point's outline (graph units), or nil to hide it.
    public func showGhost(_ r: CGRect?, round: Bool = false) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        guard let r else {
            ghost.isHidden = true
            return
        }
        ghost.path = round ? CGPath(ellipseIn: r, transform: nil) : CGPath(roundedRect: r, cornerWidth: 6, cornerHeight: 6, transform: nil)
        ghost.strokeColor = colors.accent
        ghost.fillColor = colors.accent.copy(alpha: 0.06)
        ghost.lineWidth = 1.5 / max(viewport.zoom, 0.05)
        ghost.lineDashPattern = [5, 3]
        ghost.isHidden = false
    }

    /// Hide the wire whose end is picked up (nil: show it again).
    public func pickUp(wire nid: UInt64?) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        let old = pickedWire
        pickedWire = nid
        // the picked wire is drawn on its own, faint, and taken out of the tiles
        if let old, old != nid { updateLive(old) }
        if let nid { updateLive(nid) }
        pumpTiles()
    }

    /// The point of the pin a drag's wire is anchored at (`drag.from` of a wire's begin).
    public func anchorPoint(_ from: JSONValue?) -> CGPoint? {
        guard let f = from, let node = f["node"].string, let pin = f["pin"].int64 else { return nil }
        return pinPoint(face: node, output: f["side"].string == "out", pin: Int(pin))
    }

    /// The wire into `to` (a picked-up wire's).
    public func wireNid(into to: PortRef) -> UInt64? {
        model.wires.items.first { $0.to == to }?.nid
    }

    var moveOffset: CGPoint?

    /// A move drag began: these faces follow the pointer until it ends.
    public func beginMove(_ nids: [UInt64]) {
        moving = [:]
        for n in nids { if let e = faces[n] { moving[n] = CGPoint(x: e.face.pos.x, y: e.face.pos.y) } }
        moveOffset = .zero
        for n in nids { faces[n]?.layer.zPosition = 1_000_000 }
        // the moved faces' wires leave the tiles now, drawn as they are on layers of their own,
        // so the tiles without them are up before the pointer has gone far
        let ids = Set(moving.keys.compactMap { faces[$0]?.face.id })
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for w in model.wires.items where ids.contains(w.fromFace) || ids.contains(w.toFace) {
            guard wireGeoms[w.nid] != nil else { continue }
            movingWires.insert(w.nid)
            updateLive(w.nid)
        }
        CATransaction.commit()
        pumpTiles()
    }

    /// The drag is at `(dx, dy)` from its start: move the faces' layers, and re-path their wires
    /// from `plum_wire_paths` (one typed call, no JSON).
    public func move(dx: CGFloat, dy: CGFloat) {
        guard !moving.isEmpty else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        moveOffset = CGPoint(x: dx, y: dy)
        let iv = Self.signposter.beginInterval("wires.move")
        defer { Self.signposter.endInterval("wires.move", iv) }
        for n in moving.keys { if let e = faces[n] { placeFace(e) } }
        var taken = false
        for p in link.wirePaths(faces: Array(moving.keys), dx: Float(dx), dy: Float(dy)) {
            guard wireGeoms[p.nid] != nil, p.pointCount >= 4 else { continue }
            if !movingWires.contains(p.nid) {
                movingWires.insert(p.nid)
                taken = true
            }
            let l = updateLive(p.nid, setPath: false)
            var pts: [CGPoint] = []
            pts.reserveCapacity(p.pointCount)
            var k = p.points.startIndex
            while k + 1 < p.points.endIndex {
                pts.append(CGPoint(x: CGFloat(p.points[k]), y: CGFloat(p.points[k + 1])))
                k += 2
            }
            if let l { setLivePath(l, p.nid, Self.chain(pts)) }
        }
        if taken { pumpTiles() }
    }

    /// Positions a finished move holds until the scene brings the faces' new ones.
    var held: [UInt64: CGPoint] = [:]

    /// The drag ended (`kept`: it moved things, and the scene will say where) or was cancelled:
    /// the scene's positions take over again. Kept faces stay where they were dropped until their
    /// new positions arrive (at most a second), so they don't jump back for a frame.
    public func endMove(kept: Bool = false) {
        let was = moving
        let off = moveOffset ?? .zero
        moving = [:]
        moveOffset = nil
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        if kept && (off.x != 0 || off.y != 0) {
            for (n, start) in was { held[n] = CGPoint(x: start.x + off.x, y: start.y + off.y) }
            let ids = Array(was.keys)
            let wiresMoved = movingWires
            // the moved wires stay on their own layers, where they were dropped, until the scene
            // brings their new paths (pathWire takes them back into the tiles)
            liveUntilPathed.formUnion(wiresMoved)
            movingWires = []
            Task { @MainActor [weak self] in
                try? await Task.sleep(for: .seconds(1))
                guard let self else { return }
                var any = false
                for n in ids where self.held.removeValue(forKey: n) != nil {
                    any = true
                    if let e = self.faces[n] { self.placeFace(e) }
                }
                CATransaction.begin()
                CATransaction.setDisableActions(true)
                for w in wiresMoved where any || self.liveUntilPathed.contains(w) {
                    if let item = self.model.wires[w]?.item { self.pathWire(item) } else { self.liveUntilPathed.remove(w) }
                }
                CATransaction.commit()
                self.pumpTiles()
            }
        } else {
            for n in was.keys { if let e = faces[n] { placeFace(e) } }
            let wiresMoved = movingWires
            movingWires = []
            for w in wiresMoved {
                if let item = model.wires[w]?.item { pathWire(item) } else { updateLive(w) }
            }
        }
        reorder()
        CATransaction.commit()
        pumpTiles()
    }

    // MARK: - Rows (N4 slice 2: editing values on a face)

    /// Row `row`'s widget rect on face `nid`, in graph units (where the face is shown).
    public func rowRect(face nid: UInt64, row: Int) -> CGRect? {
        guard let e = faces[nid], let L = e.layout, row >= 0, row < L.widgets.count, let w = L.widgets[row] else { return nil }
        var p = CGPoint(x: e.face.pos.x, y: e.face.pos.y)
        if let h = held[nid] { p = h }
        return w.offsetBy(dx: p.x, dy: p.y)
    }

    /// The more-button's rect on face `nid`, in graph units.
    public func moreRect(face nid: UInt64) -> CGRect? {
        guard let e = faces[nid], let m = e.layout?.more else { return nil }
        return m.offsetBy(dx: e.face.pos.x, dy: e.face.pos.y)
    }

    func styleLabel(_ t: CATextLayer, _ text: String, over r: CGRect, ink: CGColor, fill: CGColor, size: CGFloat) {
        let w = max(r.width, TextLine.width(text, fonts.mono) * size / 11 + 12)
        t.string = NSAttributedString(string: text, attributes: [
            NSAttributedString.Key(kCTFontAttributeName as String): CTFontCreateWithName("Menlo" as CFString, size, nil),
            NSAttributedString.Key(kCTForegroundColorAttributeName as String): ink,
        ])
        t.backgroundColor = fill
        t.borderColor = colors.accent
        t.borderWidth = 1
        t.frame = CGRect(x: r.midX - w / 2, y: r.minY, width: w, height: r.height)
        t.isHidden = false
    }

    /// A scrub's value over its row (graph units), or nil to hide it: a label, so the face isn't
    /// drawn again every frame.
    public func showScrub(_ text: String?, over r: CGRect?) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        guard let text, let r else {
            scrubLabel.isHidden = true
            return
        }
        styleLabel(scrubLabel, text, over: r, ink: colors.text, fill: colors.field, size: max(r.height - 6, 8))
    }

    /// The core's note on an edit (a value held to its range, an edit refused) under row `r`
    /// for a few seconds.
    public func flashNote(_ text: String, under r: CGRect, error: Bool) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        let below = CGRect(x: r.minX, y: r.maxY + 3, width: r.width, height: 16)
        styleLabel(noteLabel, text, over: below, ink: error ? colors.error : colors.text, fill: colors.field, size: 10)
        noteLabel.borderColor = error ? colors.error : colors.accent
        CATransaction.commit()
        noteTask?.cancel()
        noteTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(3))
            if !Task.isCancelled { self?.noteLabel.isHidden = true }
        }
    }

    /// The note's text while it shows (tests).
    public var noteShown: String? {
        noteLabel.isHidden ? nil : (noteLabel.string as? NSAttributedString)?.string
    }

    // MARK: - For tests, measurements and screenshots

    public func faceLayout(nid: UInt64) -> FaceLayout? { faces[nid]?.layout }
    public func faceLayer(nid: UInt64) -> CALayer? { faces[nid]?.layer }
    /// Whether the face shows the run's wash now.
    public func washed(nid: UInt64) -> Bool { faces[nid]?.wash != nil }
    /// A wire's box in graph units, as drawn (its path's, widened by its stroke).
    public func wireBox(nid: UInt64) -> CGRect? { wireGeoms[nid]?.box }
    /// Whether a wire is on screen: on a shown layer of its own, or in a shown tile with a picture.
    public func wireShown(nid: UInt64) -> Bool {
        if let l = live[nid], !returning.contains(nid) { return !l.isHidden }
        guard let g = wireGeoms[nid] else { return false }
        return tileKeys(g.box).contains { k in tiles[k].map { !$0.layer.isHidden && $0.layer.contents != nil } ?? false }
    }
    /// A wire drawn on a layer of its own (moved, picked up), if it is.
    public func liveWireLayer(nid: UInt64) -> CAShapeLayer? { returning.contains(nid) ? nil : live[nid] }
    public var faceCount: Int { faces.count }
    /// The wires drawn (in tiles or on layers of their own).
    public var wireLayerCount: Int { wireGeoms.count }

    /// What the wires give the compositor now (the wire-hitch work): layers, those shown, those
    /// reaching into the view, their paths' elements, and the view-clipped area of the shown
    /// wire layers over the view's (the render server rasterises and blends a shape layer over
    /// its bounds, so this is the overdraw the wires cost a frame).
    public func wireStats() -> WireStats {
        var s = WireStats()
        let view = CGRect(origin: .zero, size: viewSize)
        let area = max(view.width * view.height, 1)
        let t = CGAffineTransform(a: viewport.zoom, b: 0, c: 0, d: viewport.zoom, tx: viewport.pan.x, ty: viewport.pan.y)
        func count(_ l: CALayer) {
            s.layers += 1
            guard !l.isHidden, !(l.superlayer?.isHidden ?? false) else { return }
            if let sl = l as? CAShapeLayer, sl.path == nil { return }
            s.shown += 1
            let r = l.frame.applying(t).intersection(view)
            guard !r.isNull, r.width > 0, r.height > 0 else { return }
            s.inView += 1
            let f = r.width * r.height / area
            s.coverage += Double(f)
            s.largest = max(s.largest, Double(f))
            if let sl = l as? CAShapeLayer, let p = sl.path { p.applyWithBlock { _ in s.elements += 1 } }
            if l.contents != nil { s.tiles += 1 }
        }
        for l in wireRoot.sublayers ?? [] { count(l) }
        return s
    }
    public var tagCount: Int { tags.values.reduce(0) { $0 + $1.count } }
    public var drawnBytes: Int {
        faces.values.reduce(0) { $0 + ($1.layout.map { FaceRenderer.bytes($0.size, scale: 1) } ?? 0) }
    }
    /// Bytes of the faces' pictures as drawn.
    public var pictureBytes: Int {
        faces.values.reduce(0) { a, e in a + (e.drawnScale > 0 ? (e.layout.map { FaceRenderer.bytes($0.size, scale: e.drawnScale) } ?? 0) : 0) }
    }

    /// The face of a graph node by its id.
    public func nid(of id: String) -> UInt64? { faceById[id] }

    /// The layer tree drawn into a picture, `size` points at `scale` (an app's own snapshot).
    public func snapshot(size: CGSize, scale: CGFloat) -> CGImage? {
        let w = Int(size.width * scale)
        let h = Int(size.height * scale)
        guard w > 0, h > 0, let space = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return nil }
        if host.isGeometryFlipped {
            ctx.scaleBy(x: scale, y: scale)
        } else {
            ctx.translateBy(x: 0, y: CGFloat(h))
            ctx.scaleBy(x: scale, y: -scale)
        }
        host.render(in: ctx)
        return ctx.makeImage()
    }
}

extension LayerCanvas {
    /// The system's Reduce Motion.
    static var reduceMotion: Bool {
        #if canImport(AppKit)
        NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        #else
        UIAccessibility.isReduceMotionEnabled
        #endif
    }
}
