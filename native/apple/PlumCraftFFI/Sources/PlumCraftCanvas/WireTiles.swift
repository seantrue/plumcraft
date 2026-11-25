// Wires drawn into tile pictures (the wire-hitch work, N4's gate).
//
// The canvas drew a CAShapeLayer per wire with its frame tight around the wire (15.2). That is
// cheap while wires are short, as the N0 spike's were (each went forward one to three columns),
// but a wire across the graph has a layer as big as the graph, and the render server rasterises
// and blends a shape layer over its whole visible bounds every frame the view moves: at 500
// nodes and 800 wires that was 40 to 160 views' worth of blending a frame, and the iPad missed
// frames in every phase that moved the view (the faces alone missed none).
//
// So wires are drawn as faces are: into pictures, on workers, at a raster scale that steps in
// powers of two of the screen's, and again only when a view change settles or a wire changes.
// The graph is cut into square tiles of `tileSize` graph units; a tile's picture holds every
// wire that crosses it, in the scene's order, stroked as the shape layers stroked them (2.5
// units, round caps and joins, rings at bend points). Pan and zoom then move pictures only, as
// for faces. A wire being moved (a face drag) or picked up off its input leaves the tiles for a
// shape layer of its own, clipped to the view and its margin, and goes back once the tiles that
// draw it again are up (`returning`), so it never vanishes or shows twice for longer than a batch.
// The wire under the pointer is drawn wider over the tiles (`hoverWireLayer`).
//
// Platform-free, shared by the Mac and iPad canvases, as the rest of LayerCanvas.

import CoreGraphics
import Foundation
import os
import PlumCraftModels
import QuartzCore

/// A drawn wire: its path (graph units), its box widened by the stroke, its colour.
struct WireGeom: @unchecked Sendable {
    // CGPath and CGColor are immutable and thread-safe
    var path: CGPath
    var box: CGRect
    var color: CGColor
}

struct TileKey: Hashable, Sendable {
    var x: Int
    var y: Int
}

final class WireTile {
    let layer = CALayer()
    var drawnScale: CGFloat = 0
}

struct TileJob: @unchecked Sendable {
    var key: TileKey
    var rect: CGRect
    var scale: CGFloat
    var items: [(CGPath, CGColor)]
}

struct TileDone: @unchecked Sendable {
    var key: TileKey
    var scale: CGFloat
    var image: CGImage?
}

extension LayerCanvas {
    /// A tile's side in graph units: at the bench's zooms a view holds 4 to 100 of them, and a
    /// picture is 256 to 2048 pixels a side.
    static let tileSize: CGFloat = 512
    static let wireWidth: CGFloat = 2.5
    /// Texels drawn beyond each side of a tile (see `drawTile`).
    nonisolated static let tileMargin = 2
    static let hoverWidth: CGFloat = 4.5
    static let signposter = OSSignposter(subsystem: (Bundle.main.bundleIdentifier ?? "plumcraft") + ".canvas", category: .pointsOfInterest)

    var wireWidthNow: CGFloat { wireSwitches.contains(.thin) ? 2 : Self.wireWidth }
    var layersMode: Bool { wireSwitches.contains(.layers) }

    /// Whether a wire is drawn on a layer of its own rather than in the tiles.
    func isLive(_ nid: UInt64) -> Bool {
        layersMode || nid == pickedWire || movingWires.contains(nid) || liveUntilPathed.contains(nid) || nid == dragRoute?.nid
    }

    func tileKeys(_ r: CGRect) -> [TileKey] {
        guard !r.isNull, !r.isInfinite else { return [] }
        let T = Self.tileSize
        let x0 = Int((r.minX / T).rounded(.down)), x1 = Int((r.maxX / T).rounded(.down))
        let y0 = Int((r.minY / T).rounded(.down)), y1 = Int((r.maxY / T).rounded(.down))
        // a wire's box is a graph's at most; never more tiles than a hostile path could ask for
        guard x1 - x0 < 4096, y1 - y0 < 4096 else { return [] }
        var out: [TileKey] = []
        for y in y0...y1 { for x in x0...x1 { out.append(TileKey(x: x, y: y)) } }
        return out
    }

    func tileRect(_ k: TileKey) -> CGRect {
        CGRect(x: CGFloat(k.x) * Self.tileSize, y: CGFloat(k.y) * Self.tileSize, width: Self.tileSize, height: Self.tileSize)
    }

    func dirty(_ box: CGRect?) {
        guard let box else { return }
        dirtyTiles.formUnion(tileKeys(box))
    }

    /// A wire's path and colour from the scene (none for a wire drawn through a net); the tiles it crosses, before and after, are drawn again.
    func pathWire(_ w: Wire) {
        counts.wiresPathed += 1
        guard w.kind.word != .net, w.path.count >= 4 else {
            removeWire(w.nid)
            return
        }
        // its bend points' dots are drawn on their own layer, only when wanted (B158)
        let path = Self.chain(w.path.map { CGPoint(x: $0.x, y: $0.y) })
        let b = path.boundingBoxOfPath
        if wireSwitches.contains(.shortOnly), b.width > 800 || b.height > 400 {
            removeWire(w.nid)
            return
        }
        let g = WireGeom(path: path, box: b.insetBy(dx: -(Self.hoverWidth / 2 + 1), dy: -(Self.hoverWidth / 2 + 1)), color: colors.color(w.paint))
        dirty(wireGeoms[w.nid]?.box)
        wireGeoms[w.nid] = g
        dirty(g.box)
        // a kept move's wires go back to the tiles once the scene has their new paths
        liveUntilPathed.remove(w.nid)
        if dragRoute?.nid == w.nid {
            // bent by a drag under way: its layer keeps the drag's path
            updateLive(w.nid, setPath: false)
        } else {
            updateLive(w.nid)
        }
        if hoverWire == w.nid { showHoverWire() }
    }

    func removeWire(_ nid: UInt64) {
        if let g = wireGeoms.removeValue(forKey: nid) { dirty(g.box) }
        live.removeValue(forKey: nid)?.removeFromSuperlayer()
        returning.remove(nid)
        liveUntilPathed.remove(nid)
        livePaths[nid] = nil
        if hoverWire == nid {
            hoverWire = nil
            showHoverWire()
        }
    }

    /// Put a wire on a layer of its own or take it back into the tiles, as `isLive` says; its
    /// tiles are drawn again when that changes. `setPath`: give the layer the scene's path (a
    /// move gives its own). The wire's layer, while it has one.
    @discardableResult
    func updateLive(_ nid: UInt64, setPath: Bool = true) -> CAShapeLayer? {
        guard let g = wireGeoms[nid] else {
            live.removeValue(forKey: nid)?.removeFromSuperlayer()
            returning.remove(nid)
            return nil
        }
        if isLive(nid) {
            let was = live[nid] != nil && !returning.contains(nid)
            returning.remove(nid)
            let l = live[nid] ?? {
                let s = CAShapeLayer()
                s.fillColor = nil
                s.lineCap = .round
                s.anchorPoint = .zero
                s.zPosition = 1
                wireRoot.addSublayer(s)
                live[nid] = s
                return s
            }()
            l.strokeColor = g.color
            l.lineWidth = hoverWire == nid && layersMode ? Self.hoverWidth : wireWidthNow
            l.lineJoin = wireSwitches.contains(.miter) ? .miter : .round
            l.opacity = nid == pickedWire ? 0.15 : 1
            if setPath || !was { setLivePath(l, nid, g.path) }
            // out of the tiles: they are drawn again without it
            if !was && !layersMode { dirty(g.box) }
            return l
        }
        if live[nid] != nil, !returning.contains(nid) {
            // back into the tiles; its layer goes once they are up
            returning.insert(nid)
            dirty(g.box)
        }
        return nil
    }

    /// A live wire's path on its layer: the frame tight around it, and clipped to the view with
    /// the cull margin (so a wire across the graph is never a graph-sized layer); unclipped in
    /// the old mode (`--wire-layers`).
    func setLivePath(_ l: CAShapeLayer, _ nid: UInt64, _ p: CGPath) {
        livePaths[nid] = p
        placeShape(l, p, clip: !layersMode)
    }

    func placeShape(_ l: CAShapeLayer, _ p: CGPath, clip: Bool) {
        var box = p.boundingBoxOfPath.insetBy(dx: -4, dy: -4)
        if clip {
            let vis = visibleRect
            box = box.intersection(vis.insetBy(dx: -vis.width * cullMargin, dy: -vis.height * cullMargin))
            l.masksToBounds = true
            if box.isNull || box.width <= 0 || box.height <= 0 {
                l.path = nil
                return
            }
        }
        var t = CGAffineTransform(translationX: -box.minX, y: -box.minY)
        l.position = box.origin
        l.bounds = CGRect(origin: .zero, size: box.size)
        l.path = p.copy(using: &t)
    }

    /// The wire under the pointer, wider, over the tiles (its own layer in the old mode).
    func showHoverWire() {
        guard let nid = hoverWire, let g = wireGeoms[nid] else {
            hoverWireLayer.isHidden = true
            if layersMode { for (n, l) in live { l.lineWidth = n == hoverWire ? Self.hoverWidth : wireWidthNow } }
            return
        }
        if layersMode {
            for (n, l) in live { l.lineWidth = n == nid ? Self.hoverWidth : wireWidthNow }
            hoverWireLayer.isHidden = true
            return
        }
        hoverWireLayer.strokeColor = g.color
        hoverWireLayer.lineWidth = Self.hoverWidth
        hoverWireLayer.opacity = nid == pickedWire ? 0.15 : 1
        placeShape(hoverWireLayer, livePaths[nid] ?? g.path, clip: true)
        hoverWireLayer.isHidden = false
    }

    /// Culling's part for wires: tiles outside `keep` hidden; live layers clipped to the view
    /// again (or, in the old mode, hidden as the per-wire layers were). The number hidden.
    func cullWires(keep: CGRect, all: Bool) -> Int {
        var hidden = 0
        for (k, t) in tiles {
            let h = !all && !tileRect(k).intersects(keep)
            if t.layer.isHidden != h { t.layer.isHidden = h }
            if h { hidden += 1 }
        }
        for (nid, l) in live {
            if layersMode {
                let h = !all && !l.frame.intersects(keep) && !movingWires.contains(nid)
                if l.isHidden != h { l.isHidden = h }
                if h { hidden += 1 }
            } else if let p = livePaths[nid] {
                placeShape(l, p, clip: true)
            }
        }
        if hoverWire != nil { showHoverWire() }
        return hidden
    }

    /// Send the tiles waiting to a worker (one batch at a time): the visible at the zoom's raster
    /// scale, those within the cull margin at no more than 1, the rest at 0.5.
    func pumpTiles() {
        if tileWorking || dirtyTiles.isEmpty { return }
        let want = tileScale
        let vis = visibleRect
        let keep = vis.insetBy(dx: -vis.width * cullMargin, dy: -vis.height * cullMargin)
        var order: [UInt64: Int] = [:]
        for (k, nid) in model.wires.order.enumerated() { order[nid] = k }
        let drawn = wireGeoms.filter { !isLive($0.key) }.sorted { (order[$0.key] ?? .max, $0.key) < (order[$1.key] ?? .max, $1.key) }
        var jobs: [TileJob] = []
        for key in dirtyTiles {
            let r = tileRect(key)
            let scale = r.intersects(vis) ? want : (r.intersects(keep) ? min(want, 1) : min(want, 0.5))
            let items = drawn.filter { $0.value.box.intersects(r) }.map { ($0.value.path, $0.value.color) }
            jobs.append(TileJob(key: key, rect: r, scale: scale, items: items))
        }
        dirtyTiles = []
        tileWorking = true
        let back = returning
        let width = wireWidthNow
        let join: CGLineJoin = wireSwitches.contains(.miter) ? .miter : .round
        let iv = Self.signposter.beginInterval("wires.tiles", "\(jobs.count)")
        Task.detached(priority: .userInitiated) { [weak self] in
            let done = LayerCanvas.drawTiles(jobs, width: width, join: join)
            await self?.finishTiles(done, returned: back, interval: iv)
        }
    }

    nonisolated static func drawTiles(_ jobs: [TileJob], width: CGFloat, join: CGLineJoin) -> [TileDone] {
        let out = UnsafeMutableBufferPointer<TileDone?>.allocate(capacity: jobs.count)
        out.initialize(repeating: nil)
        defer {
            _ = out.deinitialize()
            out.deallocate()
        }
        nonisolated(unsafe) let o = out
        DispatchQueue.concurrentPerform(iterations: jobs.count) { k in
            let j = jobs[k]
            o[k] = TileDone(key: j.key, scale: j.scale, image: drawTile(j, width: width, join: join))
        }
        return out.compactMap { $0 }
    }

    /// One tile's picture: its wires stroked in order, y down, in graph units; none when no wire
    /// crosses it.
    nonisolated static func drawTile(_ j: TileJob, width: CGFloat, join: CGLineJoin) -> CGImage? {
        guard !j.items.isEmpty else { return nil }
        // the tile and a margin of tileMargin texels each side, drawn as well: the layer shows
        // the tile alone (contentsRect), and a texel sampled at its edge then blends with the
        // wire beyond it, not with nothing, so no seam shows where tiles meet
        let inner = Int((j.rect.width * j.scale).rounded(.up))
        let px = inner + 2 * tileMargin
        guard inner > 0, px < 16384, let space = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: px, height: px, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return nil }
        ctx.translateBy(x: 0, y: CGFloat(px))
        ctx.scaleBy(x: j.scale, y: -j.scale)
        let m = CGFloat(tileMargin) / j.scale
        ctx.translateBy(x: -j.rect.minX + m, y: -j.rect.minY + m)
        ctx.setLineWidth(width)
        ctx.setLineCap(.round)
        ctx.setLineJoin(join)
        for (p, c) in j.items {
            ctx.addPath(p)
            ctx.setStrokeColor(c)
            ctx.strokePath()
        }
        return ctx.makeImage()
    }

    func finishTiles(_ done: [TileDone], returned: Set<UInt64>, interval: OSSignpostIntervalState) {
        Self.signposter.endInterval("wires.tiles", interval)
        tileWorking = false
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for d in done {
            guard let img = d.image else {
                tiles.removeValue(forKey: d.key)?.layer.removeFromSuperlayer()
                continue
            }
            let t = tiles[d.key] ?? {
                let t = WireTile()
                t.layer.anchorPoint = .zero
                t.layer.contentsGravity = .resize
                t.layer.isOpaque = false
                let r = tileRect(d.key)
                t.layer.position = r.origin
                t.layer.bounds = CGRect(origin: .zero, size: r.size)
                wireRoot.insertSublayer(t.layer, at: 0)
                tiles[d.key] = t
                return t
            }()
            t.layer.contents = img
            t.layer.contentsScale = d.scale
            let f = CGFloat(Self.tileMargin) / CGFloat(img.width)
            t.layer.contentsRect = CGRect(x: f, y: f, width: 1 - 2 * f, height: 1 - 2 * f)
            t.drawnScale = d.scale
            counts.tilesDrawn += 1
        }
        // the wires that went back into the tiles are drawn there now: their own layers go
        for nid in returned where returning.contains(nid) && !isLive(nid) {
            live.removeValue(forKey: nid)?.removeFromSuperlayer()
            livePaths[nid] = nil
            returning.remove(nid)
        }
        cull(force: true)
        CATransaction.commit()
        pumpTiles()
        if idle { onDrawn?([]) }
    }

    /// After a view change settles: the visible tiles not at the zoom's raster scale.
    func settleTiles() {
        let want = tileScale
        let vis = visibleRect
        let keep = vis.insetBy(dx: -vis.width * cullMargin, dy: -vis.height * cullMargin)
        for (k, t) in tiles {
            let r = tileRect(k)
            if r.intersects(vis) {
                if abs(t.drawnScale - want) > want * 0.01 { dirtyTiles.insert(k) }
            } else if !r.intersects(keep) && t.drawnScale > min(want, 0.5) {
                // far from the view: back to the lowest scale, so a zoom in doesn't leave big
                // pictures behind it
                dirtyTiles.insert(k)
            }
        }
        pumpTiles()
    }

    /// The scale visible tiles are drawn at: the screen's pixels a graph unit at this zoom, so a
    /// settled view shows wires stroked at its own resolution, as the shape layers were (a power
    /// of two above it, as faces are drawn, shrinks a 2.5-unit stroke by up to half and draws it
    /// heavier). From a quarter to twice the screen's scale, as faces (a tile at most 2,048 pixels a side).
    var tileScale: CGFloat {
        let s = (screenScale * viewport.zoom * 64).rounded() / 64
        return min(max(s, 0.25), screenScale * 2)
    }

    /// Bytes of the wire tiles' pictures as drawn.
    public var wirePictureBytes: Int {
        tiles.values.reduce(0) { a, t in
            let px = Int((Self.tileSize * t.drawnScale).rounded(.up)) + 2 * Self.tileMargin
            return a + px * px * 4
        }
    }
}
