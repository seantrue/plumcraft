// The two hosts. The canvas is the same; only input and the display link's owner differ.
import Foundation
import QuartzCore

func deviceInfo() -> [String: Any] {
    var sys = utsname()
    uname(&sys)
    let machine = withUnsafeBytes(of: &sys.machine) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
    var model = machine
    var size = 0
    if sysctlbyname("hw.model", nil, &size, nil, 0) == 0, size > 0 {
        var buf = [CChar](repeating: 0, count: size)
        if sysctlbyname("hw.model", &buf, &size, nil, 0) == 0 {
            model = String(decoding: buf.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }, as: UTF8.self)
        }
    }
    #if targetEnvironment(simulator)
    let sim = true
    #else
    let sim = false
    #endif
    return [
        "machine": machine, "model": model, "simulator": sim,
        "os": ProcessInfo.processInfo.operatingSystemVersionString,
        "cpus": ProcessInfo.processInfo.activeProcessorCount,
        "memory_gb": Double(ProcessInfo.processInfo.physicalMemory) / 1_073_741_824,
    ]
}

#if os(macOS)
import AppKit

/// AppKit host: a layer-hosting NSView. Two-finger scroll pans, pinch zooms, ⌘-scroll zooms,
/// drag on a node moves it, drag on empty canvas pans.
@MainActor
public final class CanvasHostView: NSView {
    public private(set) var canvas: CanvasController!
    var bench: BenchRunner?
    var link: CADisplayLink?
    var dragNode: Int?
    var dragOffset = CGPoint.zero
    var lastMouse = CGPoint.zero
    var activity: NSObjectProtocol?

    public init(frame: NSRect, options: BenchOptions?) {
        super.init(frame: frame)
        let root = CALayer()
        root.backgroundColor = Palette.canvas
        root.isGeometryFlipped = true  // y down, as the scene
        layer = root
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        let n = options?.nodes ?? 500
        let m = options?.wires ?? 800
        let scene = FakeScene.make(nodes: n, wires: m)
        canvas = CanvasController(host: root, scene: scene, wireMode: .perWire, screenScale: scale)
        canvas.viewSize = frame.size
        canvas.onLog = { print($0) }
        if let options {
            let hz = Double(NSScreen.main?.maximumFramesPerSecond ?? 60)
            let b = BenchRunner(canvas: canvas, options: options, refreshHz: hz, device: deviceInfo())
            // exit at once: NSApp.terminate left the process hanging (stopped) under xctrace
            b.onDone = { [weak self] _ in
                self?.link?.invalidate()
                fflush(stdout)
                exit(0)
            }
            bench = b
            activity = ProcessInfo.processInfo.beginActivity(options: [.userInitiated, .latencyCritical], reason: "benchmark")
        } else {
            let t = now()
            canvas.build(zoom: 1)
            print(String(format: "built %d nodes, %d wires in %.1f ms", scene.nodes.count, scene.wires.count, (now() - t) * 1000))
            canvas.fit()
            canvas.settle()
        }
    }

    required init?(coder: NSCoder) { nil }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        guard window != nil, link == nil else { return }
        let l = displayLink(target: self, selector: #selector(tick(_:)))
        l.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        l.add(to: .main, forMode: .common)
        link = l
    }

    public override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        canvas?.viewSize = newSize
    }

    @objc func tick(_ l: CADisplayLink) {
        bench?.tick(timestamp: l.timestamp, duration: l.duration)
    }

    public override func scrollWheel(with e: NSEvent) {
        if e.modifierFlags.contains(.command) {
            canvas.zoom(by: exp(e.scrollingDeltaY * 0.01), at: convert(e.locationInWindow, from: nil))
        } else {
            let k: CGFloat = e.hasPreciseScrollingDeltas ? 1 : 10
            canvas.pan(by: CGPoint(x: e.scrollingDeltaX * k, y: e.scrollingDeltaY * k))
        }
        canvas.gestureChanged()
    }

    public override func magnify(with e: NSEvent) {
        canvas.zoom(by: 1 + e.magnification, at: convert(e.locationInWindow, from: nil))
        canvas.gestureChanged()
    }

    public override func smartMagnify(with e: NSEvent) {
        canvas.fit()
        canvas.gestureChanged()
    }

    public override func mouseDown(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        lastMouse = p
        if let i = canvas.hitNode(atView: p) {
            dragNode = i
            let g = canvas.toGraph(p)
            let pos = canvas.scene.nodes[i].position
            dragOffset = CGPoint(x: g.x - pos.x, y: g.y - pos.y)
            canvas.beginDrag(i)
        }
    }

    public override func mouseDragged(with e: NSEvent) {
        let p = convert(e.locationInWindow, from: nil)
        if let i = dragNode {
            let g = canvas.toGraph(p)
            canvas.moveNode(i, to: CGPoint(x: g.x - dragOffset.x, y: g.y - dragOffset.y))
        } else {
            canvas.pan(by: CGPoint(x: p.x - lastMouse.x, y: p.y - lastMouse.y))
            canvas.gestureChanged()
        }
        lastMouse = p
    }

    public override func mouseUp(with e: NSEvent) {
        if dragNode != nil { canvas.endDrag() }
        dragNode = nil
    }

    public override func keyDown(with e: NSEvent) {
        switch e.charactersIgnoringModifiers {
        case "1": canvas.build(wireMode: .perWire); print("wires: perWire")
        case "2": canvas.build(wireMode: .batched); print("wires: batched")
        case "3": canvas.build(wireMode: .hybrid); print("wires: hybrid")
        case "f": canvas.fit(); canvas.gestureChanged()
        default: super.keyDown(with: e)
        }
    }

    /// Writes a PNG of the layer tree, rendered with CALayer.render(in:) at the backing scale.
    public func snapshot(to path: String) {
        guard let root = layer else { return }
        let s = canvas.screenScale
        let w = Int(bounds.width * s), h = Int(bounds.height * s)
        guard let ctx = CGContext(
            data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue)
        else { return }
        ctx.scaleBy(x: s, y: s)
        root.render(in: ctx)
        guard let img = ctx.makeImage() else { return }
        let rep = NSBitmapImageRep(cgImage: img)
        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
        print("snapshot: \(path)")
    }
}
#endif

#if os(iOS)
import UIKit

/// UIKit host: one finger on empty canvas pans, on a node drags it; pinch zooms; double tap fits.
@MainActor
public final class CanvasHostView: UIView, UIGestureRecognizerDelegate {
    public private(set) var canvas: CanvasController!
    var bench: BenchRunner?
    var link: CADisplayLink?
    var dragNode: Int?
    var dragOffset = CGPoint.zero
    var lastPan = CGPoint.zero
    var options: BenchOptions?
    var started = false

    public init(frame: CGRect, options: BenchOptions?, screenScale: CGFloat, maxFPS: Int) {
        self.options = options
        super.init(frame: frame)
        layer.backgroundColor = Palette.canvas
        let n = options?.nodes ?? 500
        let m = options?.wires ?? 800
        let scene = FakeScene.make(nodes: n, wires: m)
        canvas = CanvasController(host: layer, scene: scene, wireMode: .perWire, screenScale: screenScale)
        canvas.viewSize = frame.size
        canvas.onLog = { print($0) }
        if let options {
            let b = BenchRunner(canvas: canvas, options: options, refreshHz: Double(maxFPS), device: deviceInfo())
            b.onDone = { [weak self] _ in
                self?.link?.invalidate()
                fflush(stdout)
                exit(0)
            }
            bench = b
        } else {
            canvas.build(zoom: 1)
            canvas.fit()
            canvas.settle()
        }
        let pan = UIPanGestureRecognizer(target: self, action: #selector(onPan(_:)))
        pan.maximumNumberOfTouches = 2
        pan.delegate = self
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(onPinch(_:)))
        pinch.delegate = self
        let dbl = UITapGestureRecognizer(target: self, action: #selector(onDoubleTap))
        dbl.numberOfTapsRequired = 2
        for g in [pan, pinch, dbl] as [UIGestureRecognizer] { addGestureRecognizer(g) }
        let l = CADisplayLink(target: self, selector: #selector(tick(_:)))
        l.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: Float(maxFPS), preferred: Float(maxFPS))
        l.add(to: .main, forMode: .common)
        link = l
    }

    required init?(coder: NSCoder) { nil }

    public override func layoutSubviews() {
        super.layoutSubviews()
        canvas.viewSize = bounds.size
    }

    @objc func tick(_ l: CADisplayLink) {
        bench?.tick(timestamp: l.timestamp, duration: l.duration)
    }

    public func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith o: UIGestureRecognizer) -> Bool { true }

    @objc func onPan(_ g: UIPanGestureRecognizer) {
        let p = g.location(in: self)
        switch g.state {
        case .began:
            lastPan = p
            if g.numberOfTouches == 1, let i = canvas.hitNode(atView: p) {
                dragNode = i
                let gp = canvas.toGraph(p)
                let pos = canvas.scene.nodes[i].position
                dragOffset = CGPoint(x: gp.x - pos.x, y: gp.y - pos.y)
                canvas.beginDrag(i)
            }
        case .changed:
            if let i = dragNode {
                let gp = canvas.toGraph(p)
                canvas.moveNode(i, to: CGPoint(x: gp.x - dragOffset.x, y: gp.y - dragOffset.y))
            } else {
                canvas.pan(by: CGPoint(x: p.x - lastPan.x, y: p.y - lastPan.y))
                canvas.gestureChanged()
            }
            lastPan = p
        default:
            if dragNode != nil { canvas.endDrag() }
            dragNode = nil
        }
    }

    @objc func onPinch(_ g: UIPinchGestureRecognizer) {
        guard g.state == .changed || g.state == .began else { return }
        canvas.zoom(by: g.scale, at: g.location(in: self))
        g.scale = 1
        canvas.gestureChanged()
    }

    @objc func onDoubleTap() {
        canvas.fit()
        canvas.gestureChanged()
    }
}
#endif
