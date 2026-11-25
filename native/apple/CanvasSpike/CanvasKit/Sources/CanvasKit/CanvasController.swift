// The canvas (plan 5.4): a CALayer per node with its face drawn once into its contents, wires in
// CAShapeLayers, all under one transformed layer. Shared by the AppKit and UIKit hosts.
import CoreGraphics
import Foundation
import QuartzCore

public enum WireMode: String, CaseIterable, Sendable {
    /// One CAShapeLayer per wire, its frame tight around the wire.
    case perWire
    /// One CAShapeLayer per wire with a zero frame (the path spills outside the bounds).
    case perWireLoose
    /// One CAShapeLayer per wire colour; a drag re-paths every wire of the dragged wires' colours.
    case batched
    /// Batched while still; the dragged node's wires move to their own layers for the drag.
    case hybrid
}

func now() -> Double { CACurrentMediaTime() }

@MainActor
public final class CanvasController {
    public let host: CALayer
    let world = CALayer()
    let wireRoot = CALayer()
    let nodeRoot = CALayer()

    public private(set) var scene: SceneData
    public private(set) var wireMode: WireMode
    public let screenScale: CGFloat
    public var viewSize: CGSize = CGSize(width: 1024, height: 768)
    public private(set) var pan: CGPoint = .zero
    public private(set) var zoom: CGFloat = 1

    var nodeLayers: [CALayer] = []
    var washLayers: [CALayer] = []
    var progressLayers: [CALayer] = []
    var rasterScale: [CGFloat] = []
    var thumbs: [Int: CGImage] = [:]
    var wireLayers: [CAShapeLayer] = []  // perWire: index = wire
    var batchLayers: [Int: CAShapeLayer] = [:]  // batched/hybrid: type -> layer
    var liveLayers: [Int: CAShapeLayer] = [:]  // hybrid while dragging: wire -> layer
    var excluded: Set<Int> = []  // hybrid: wires out of the batches
    public private(set) var dragging: Int?
    var settleTask: Task<Void, Never>?
    public var onLog: ((String) -> Void)?

    public init(host: CALayer, scene: SceneData, wireMode: WireMode, screenScale: CGFloat) {
        self.host = host
        self.scene = scene
        self.wireMode = wireMode
        self.screenScale = screenScale
        world.anchorPoint = .zero
        world.position = .zero
        world.bounds = CGRect(x: 0, y: 0, width: 1, height: 1)
        for l in [wireRoot, nodeRoot] {
            l.anchorPoint = .zero
            l.position = .zero
            l.bounds = CGRect(x: 0, y: 0, width: 1, height: 1)
            world.addSublayer(l)
        }
        host.addSublayer(world)
    }

    // MARK: building

    /// Raster scale for a zoom: powers of two of the screen scale, so a small zoom change never re-rasters.
    public func rasterScale(forZoom z: CGFloat) -> CGFloat {
        let s = screenScale * pow(2, ceil(log2(max(z, 0.01))))
        return min(max(s, 0.5), screenScale * 2)
    }

    /// Rebuilds every layer. Returns timings in ms.
    @discardableResult
    public func build(scene: SceneData? = nil, wireMode: WireMode? = nil, zoom z: CGFloat? = nil, concurrent: Bool = true) -> [String: Double] {
        if let scene { self.scene = scene }
        if let wireMode { self.wireMode = wireMode }
        if let z { zoom = z }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        nodeRoot.sublayers = nil
        wireRoot.sublayers = nil
        nodeLayers = []; washLayers = []; progressLayers = []
        wireLayers = []; batchLayers = [:]; liveLayers = [:]; excluded = []
        dragging = nil

        var t = now()
        var timings: [String: Double] = [:]
        if thumbs.isEmpty || scene != nil {
            thumbs = [:]
            for node in self.scene.nodes {
                for row in node.rows { if case .preview(let seed) = row.kind { thumbs[node.id] = FaceRenderer.thumbnail(seed: seed) } }
            }
        }
        timings["thumbnails_ms"] = (now() - t) * 1000

        t = now()
        let s = rasterScale(forZoom: zoom)
        let all = Array(0..<self.scene.nodes.count)
        let images = renderFaces(all, scale: s, concurrent: concurrent)
        timings["faces_ms"] = (now() - t) * 1000

        t = now()
        rasterScale = [CGFloat](repeating: s, count: self.scene.nodes.count)
        for (i, node) in self.scene.nodes.enumerated() {
            let l = CALayer()
            l.anchorPoint = .zero
            l.bounds = CGRect(origin: .zero, size: node.size)
            l.position = node.position
            l.contents = images[i]
            l.contentsScale = s
            l.contentsGravity = .resize
            l.isOpaque = false
            let wash = CALayer()
            wash.frame = l.bounds
            wash.cornerRadius = 8
            wash.backgroundColor = Palette.state(node.state)
            wash.isHidden = Palette.state(node.state) == nil
            let prog = CALayer()
            prog.anchorPoint = .zero
            prog.position = CGPoint(x: 8, y: node.size.height - 5)
            prog.bounds = CGRect(x: 0, y: 0, width: 0, height: 3)
            prog.backgroundColor = Palette.rgb(0x42A5F5)
            prog.isHidden = true
            l.addSublayer(wash)
            l.addSublayer(prog)
            nodeRoot.addSublayer(l)
            nodeLayers.append(l); washLayers.append(wash); progressLayers.append(prog)
        }
        timings["node_layers_ms"] = (now() - t) * 1000

        t = now()
        buildWires()
        timings["wire_layers_ms"] = (now() - t) * 1000
        applyTransform()
        return timings
    }

    func renderFaces(_ idx: [Int], scale: CGFloat, concurrent: Bool) -> [CGImage?] {
        let nodes = scene.nodes
        let th = thumbs
        let out = UnsafeMutablePointer<CGImage?>.allocate(capacity: idx.count)
        out.initialize(repeating: nil, count: idx.count)
        defer { out.deinitialize(count: idx.count); out.deallocate() }
        if concurrent {
            nonisolated(unsafe) let o = out
            DispatchQueue.concurrentPerform(iterations: idx.count) { k in
                let i = idx[k]
                o[k] = FaceRenderer.render(nodes[i], thumbnail: th[i], scale: scale)
            }
        } else {
            for (k, i) in idx.enumerated() { out[k] = FaceRenderer.render(nodes[i], thumbnail: th[i], scale: scale) }
        }
        return Array(UnsafeBufferPointer(start: out, count: idx.count))
    }

    func shapeLayer(type: Int) -> CAShapeLayer {
        let s = CAShapeLayer()
        s.strokeColor = Palette.types[type % 8]
        s.fillColor = nil
        s.lineWidth = 2
        s.lineCap = .round
        s.anchorPoint = .zero
        s.position = .zero
        return s
    }

    func wirePath(_ w: Int, into path: CGMutablePath) {
        let wire = scene.wires[w]
        let a = scene.pinPoint(node: wire.from, row: wire.fromRow)
        let b = scene.pinPoint(node: wire.to, row: wire.toRow)
        let dx = max(40, abs(b.x - a.x) * 0.5)
        path.move(to: a)
        path.addCurve(to: b, control1: CGPoint(x: a.x + dx, y: a.y), control2: CGPoint(x: b.x - dx, y: b.y))
    }

    func wirePath(_ w: Int) -> CGPath {
        let p = CGMutablePath()
        wirePath(w, into: p)
        return p
    }

    /// Gives a single-wire layer its path with the layer's frame tight around it (so it can be culled).
    func setWire(_ l: CAShapeLayer, _ w: Int) {
        let p = wirePath(w)
        if wireMode == .perWireLoose {
            l.path = p
            return
        }
        let box = p.boundingBoxOfPath.insetBy(dx: -2, dy: -2)
        var t = CGAffineTransform(translationX: -box.minX, y: -box.minY)
        l.frame = box
        l.path = p.copy(using: &t)
    }

    func buildWires() {
        switch wireMode {
        case .perWire, .perWireLoose:
            for (w, wire) in scene.wires.enumerated() {
                let s = shapeLayer(type: wire.type)
                setWire(s, w)
                wireRoot.addSublayer(s)
                wireLayers.append(s)
            }
        case .batched, .hybrid:
            for t in 0..<8 {
                let s = shapeLayer(type: t)
                wireRoot.addSublayer(s)
                batchLayers[t] = s
            }
            rebatch(types: Set(0..<8))
        }
    }

    func rebatch(types: Set<Int>) {
        var paths: [Int: CGMutablePath] = [:]
        for t in types { paths[t] = CGMutablePath() }
        for (w, wire) in scene.wires.enumerated() where types.contains(wire.type) && !excluded.contains(w) {
            if let p = paths[wire.type] { wirePath(w, into: p) }
        }
        for (t, p) in paths { batchLayers[t]?.path = p }
    }

    // MARK: viewport

    public func setViewport(pan p: CGPoint, zoom z: CGFloat) {
        pan = p
        zoom = min(max(z, 0.1), 4)
        applyTransform()
    }

    func applyTransform() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        world.setAffineTransform(CGAffineTransform(a: zoom, b: 0, c: 0, d: zoom, tx: pan.x, ty: pan.y))
        CATransaction.commit()
    }

    /// Zooms by `factor` keeping the view point `at` fixed.
    public func zoom(by factor: CGFloat, at v: CGPoint) {
        let g = toGraph(v)
        let z = min(max(zoom * factor, 0.1), 4)
        setViewport(pan: CGPoint(x: v.x - g.x * z, y: v.y - g.y * z), zoom: z)
    }

    public func pan(by d: CGPoint) { setViewport(pan: CGPoint(x: pan.x + d.x, y: pan.y + d.y), zoom: zoom) }

    public func toGraph(_ v: CGPoint) -> CGPoint { CGPoint(x: (v.x - pan.x) / zoom, y: (v.y - pan.y) / zoom) }

    public func center(on g: CGPoint, zoom z: CGFloat) {
        setViewport(pan: CGPoint(x: viewSize.width / 2 - g.x * z, y: viewSize.height / 2 - g.y * z), zoom: z)
    }

    public func fit() {
        let b = scene.bounds.insetBy(dx: -40, dy: -40)
        let z = min(viewSize.width / b.width, viewSize.height / b.height)
        setViewport(pan: CGPoint(x: -b.minX * z + (viewSize.width - b.width * z) / 2, y: -b.minY * z + (viewSize.height - b.height * z) / 2), zoom: z)
    }

    public var visibleRect: CGRect {
        CGRect(x: -pan.x / zoom, y: -pan.y / zoom, width: viewSize.width / zoom, height: viewSize.height / zoom)
    }

    public func visibleNodes() -> [Int] {
        let r = visibleRect
        return scene.nodes.indices.filter { CGRect(origin: scene.nodes[$0].position, size: scene.nodes[$0].size).intersects(r) }
    }

    // MARK: hit testing and dragging

    public func hitNode(atView v: CGPoint) -> Int? {
        let g = toGraph(v)
        for i in scene.nodes.indices.reversed() where CGRect(origin: scene.nodes[i].position, size: scene.nodes[i].size).contains(g) {
            return i
        }
        return nil
    }

    public func beginDrag(_ i: Int) {
        dragging = i
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        nodeLayers[i].removeFromSuperlayer()
        nodeRoot.addSublayer(nodeLayers[i])  // to the front
        if wireMode == .hybrid {
            let attached = scene.wiresByNode[i]
            excluded = Set(attached)
            rebatch(types: Set(attached.map { scene.wires[$0].type }))
            for w in attached {
                let s = shapeLayer(type: scene.wires[w].type)
                setWire(s, w)
                wireRoot.addSublayer(s)
                liveLayers[w] = s
            }
        }
    }

    public func moveNode(_ i: Int, to p: CGPoint) {
        scene.nodes[i].position = p
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        nodeLayers[i].position = p
        let attached = scene.wiresByNode[i]
        switch wireMode {
        case .perWire, .perWireLoose:
            for w in attached { setWire(wireLayers[w], w) }
        case .batched:
            rebatch(types: Set(attached.map { scene.wires[$0].type }))
        case .hybrid:
            for w in attached { if let l = liveLayers[w] { setWire(l, w) } }
        }
    }

    public func endDrag() {
        guard let i = dragging else { return }
        dragging = nil
        if wireMode == .hybrid {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            for (_, l) in liveLayers { l.removeFromSuperlayer() }
            liveLayers = [:]
            let types = Set(scene.wiresByNode[i].map { scene.wires[$0].type })
            excluded = []
            rebatch(types: types)
            CATransaction.commit()
        }
    }

    // MARK: run state

    public func setRunState(_ i: Int, _ s: RunState, fraction: CGFloat?) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        defer { CATransaction.commit() }
        let c = Palette.state(s)
        washLayers[i].backgroundColor = c
        washLayers[i].isHidden = c == nil
        if let fraction {
            progressLayers[i].isHidden = false
            progressLayers[i].bounds = CGRect(x: 0, y: 0, width: (scene.nodes[i].size.width - 16) * fraction, height: 3)
        } else {
            progressLayers[i].isHidden = true
        }
    }

    // MARK: re-rasterising

    /// Redraws the given faces at `scale`. Returns ms spent rendering and assigning (not committing).
    @discardableResult
    public func reraster(_ idx: [Int], scale: CGFloat, concurrent: Bool = true) -> Double {
        let t = now()
        let images = renderFaces(idx, scale: scale, concurrent: concurrent)
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for (k, i) in idx.enumerated() {
            nodeLayers[i].contents = images[k]
            nodeLayers[i].contentsScale = scale
            rasterScale[i] = scale
        }
        CATransaction.commit()
        return (now() - t) * 1000
    }

    /// Visible faces whose raster scale doesn't match the zoom are redrawn.
    @discardableResult
    public func settle() -> (count: Int, ms: Double) {
        let s = rasterScale(forZoom: zoom)
        let idx = visibleNodes().filter { rasterScale[$0] != s }
        guard !idx.isEmpty else { return (0, 0) }
        let ms = reraster(idx, scale: s)
        onLog?(String(format: "settle: %d faces at %.2fx in %.1f ms", idx.count, s, ms))
        return (idx.count, ms)
    }

    /// Interactive: settle 200 ms after the last gesture change.
    public func gestureChanged() {
        settleTask?.cancel()
        settleTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .milliseconds(200))
            if Task.isCancelled { return }
            self?.settle()
        }
    }

    public func faceBytes() -> Int {
        scene.nodes.indices.reduce(0) { $0 + FaceRenderer.bytes(scene.nodes[$1], scale: rasterScale[$1]) }
    }
}
