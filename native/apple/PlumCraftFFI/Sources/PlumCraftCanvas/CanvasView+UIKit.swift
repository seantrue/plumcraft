// The iPad's host for the layer canvas (N3 slice 5): the canvas itself is the Mac's; only
// recognition differs (plan 5.5's iPadOS column). What it recognises in this slice, read and run:
//
//   pan        one finger on empty canvas, two fingers anywhere, with momentum when the fingers
//              lift and the rubber band past the content; a trackpad's or mouse's scroll
//   zoom       pinch about the fingers (a trackpad's pinch too)
//   tap        `ui.gesture` tap with the keyboard's modifiers: the core selects, or watches a pin
//   double tap on a face fits it, and a second goes back; on empty canvas fits everything (the
//              iPad spike's rule; decision 6 gives empty canvas's double tap to the palette, which
//              is N4's, so this is left for the maintainer, note 24 section 18)
//   long press, secondary click   the core's context menu (`ui.gesture` context, `menu.get`), as a
//              UIContextMenuInteraction whose items the app makes (`contextMenuItems`)
//   hover      a pointer or Pencil hovering rings the pin or widens the wire under it
//
// Editing gestures (a drag on a face moves it, a drag from a pin draws a wire, a marquee) are N4's:
// a one-finger pan that starts on a face or a pin does not begin, so N4 can give it to `ui.gesture`
// drags without taking anything away from panning.

#if os(iOS)
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import UIKit

@MainActor
public final class CanvasView: UIView, UIGestureRecognizerDelegate, UIContextMenuInteractionDelegate {
    public let canvas: LayerCanvas
    public var model: DocumentModel { canvas.model }
    public var onError: ((String) -> Void)?
    /// The core's context menu as the app's menu elements (it dispatches them); nil: no context
    /// menus.
    public var contextMenuItems: (([MenuItem]) -> [UIMenuElement])?
    /// Called with what a recognised gesture did, for scripts (`tap`, `doubleTap`, `fit`, …).
    public var onGesture: ((String) -> Void)?

    let onePan = UIPanGestureRecognizer()
    let twoPan = UIPanGestureRecognizer()
    let scrollPan = UIPanGestureRecognizer()
    let pinch = UIPinchGestureRecognizer()
    let tap = UITapGestureRecognizer()
    let doubleTap = UITapGestureRecognizer()
    var lastPan = CGPoint.zero
    var momentum: CADisplayLink?
    var velocity = CGPoint.zero
    var lastFrame: CFTimeInterval = 0
    var smartReturn: Viewport?
    var contextAt: CGPoint?
    var contextTask: Task<[MenuItem], Never>?

    public init(model: DocumentModel, frame: CGRect = CGRect(x: 0, y: 0, width: 1024, height: 768)) {
        let host = CALayer()
        canvas = LayerCanvas(model: model, host: host, screenScale: UITraitCollection.current.displayScale)
        super.init(frame: frame)
        layer.addSublayer(host)
        host.frame = bounds
        host.masksToBounds = true
        canvas.viewSize = frame.size
        isMultipleTouchEnabled = true
        // one finger on empty canvas pans (gestureRecognizerShouldBegin); two fingers anywhere
        onePan.maximumNumberOfTouches = 1
        onePan.addTarget(self, action: #selector(onPan(_:)))
        twoPan.minimumNumberOfTouches = 2
        twoPan.maximumNumberOfTouches = 2
        twoPan.addTarget(self, action: #selector(onPan(_:)))
        // a trackpad's two-finger scroll and a mouse wheel: scroll events only, no touches
        scrollPan.allowedScrollTypesMask = .all
        scrollPan.allowedTouchTypes = []
        scrollPan.addTarget(self, action: #selector(onScroll(_:)))
        pinch.addTarget(self, action: #selector(onPinch(_:)))
        tap.addTarget(self, action: #selector(onTap(_:)))
        doubleTap.numberOfTapsRequired = 2
        doubleTap.addTarget(self, action: #selector(onDoubleTap(_:)))
        tap.require(toFail: doubleTap)
        for g in [onePan, twoPan, scrollPan, pinch, tap, doubleTap] as [UIGestureRecognizer] {
            g.delegate = self
            addGestureRecognizer(g)
        }
        let hover = UIHoverGestureRecognizer(target: self, action: #selector(onHover(_:)))
        addGestureRecognizer(hover)
        addInteraction(UIContextMenuInteraction(delegate: self))
        isAccessibilityElement = true
        accessibilityLabel = "Canvas"
        accessibilityTraits = .allowsDirectInteraction
        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (v: CanvasView, _: UITraitCollection) in v.followAppearance() }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    public override func layoutSubviews() {
        super.layoutSubviews()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        canvas.host.frame = bounds
        CATransaction.commit()
        // a resize (a rotation, Split View, Stage Manager) keeps what was at the view's centre at
        // its centre
        let old = canvas.viewSize
        canvas.viewSize = bounds.size
        if old != bounds.size, old.width > 0, old.height > 0, bounds.width > 0 {
            let v = canvas.viewport
            canvas.setViewport(Viewport(pan: CGPoint(x: v.pan.x + (bounds.width - old.width) / 2, y: v.pan.y + (bounds.height - old.height) / 2), zoom: v.zoom))
        }
    }

    public override func didMoveToWindow() {
        super.didMoveToWindow()
        followAppearance()
        if window != nil { canvas.settle() }
    }

    // MARK: Following the system's appearance

    /// Paper in light, Dusk in dark (plan 5.5's "follow the system"), as on the Mac.
    public var followsAppearance = true
    var themeAsked: String?

    public func followAppearance() {
        guard followsAppearance else { return }
        let name = traitCollection.userInterfaceStyle == .dark ? "dusk" : "paper"
        if name == themeAsked { return }
        themeAsked = name
        let model = self.model
        Task {
            _ = try? await model.run("view.theme", ["name": .string(name)])
            await model.refreshTheme()
        }
    }

    // MARK: Geometry

    func graph(_ v: CGPoint) -> Point {
        let g = canvas.viewport.toGraph(v)
        return Point(Double(g.x), Double(g.y))
    }

    /// 44 points on screen across, in graph units (plan 5.5's touch target); a pointer's is 10.
    func slop(pointer: Bool) -> Double { CanvasLink.slop(points: pointer ? 10 : 22, zoom: Double(canvas.viewport.zoom)) }

    static func mods(_ f: UIKeyModifierFlags) -> [ModifierWord] {
        var m: [ModifierWord] = []
        if f.contains(.shift) { m.append(.shift) }
        if f.contains(.command) { m.append(.primary) }
        if f.contains(.alternate) { m.append(.alt) }
        if f.contains(.control) { m.append(.ctrl) }
        return m
    }

    /// What is under `p` (view points), at a touch's or a pointer's size.
    public func hit(at p: CGPoint, pointer: Bool = false) -> HotHit {
        canvas.link.hit(graph(p), slop: slop(pointer: pointer))
    }

    // MARK: Recognisers

    public func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith o: UIGestureRecognizer) -> Bool {
        // pinch and the two-finger pan together; taps wait for nothing but each other
        let pair: Set<ObjectIdentifier> = [ObjectIdentifier(pinch), ObjectIdentifier(twoPan), ObjectIdentifier(scrollPan)]
        return pair.contains(ObjectIdentifier(g)) && pair.contains(ObjectIdentifier(o))
    }

    public override func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
        if g === onePan {
            // one finger pans only on empty canvas (and over a wire, frame or tag): a face or a
            // pin is N4's drag
            let h = hit(at: g.location(in: self))
            if h.ok, h.kind == .node || h.kind == .pin || h.kind == .group { return false }
        }
        return true
    }

    public override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        // a finger down stops the momentum
        stopMomentum()
        super.touchesBegan(touches, with: event)
    }

    @objc func onPan(_ g: UIPanGestureRecognizer) {
        let p = g.location(in: self)
        switch g.state {
        case .began:
            stopMomentum()
            lastPan = p
        case .changed:
            canvas.pan(by: CGPoint(x: p.x - lastPan.x, y: p.y - lastPan.y), elastic: true)
            lastPan = p
        case .ended:
            startMomentum(g.velocity(in: self))
        default:
            canvas.endPan()
        }
    }

    @objc func onScroll(_ g: UIPanGestureRecognizer) {
        switch g.state {
        case .began:
            stopMomentum()
            g.setTranslation(.zero, in: self)
        case .changed:
            let t = g.translation(in: self)
            canvas.pan(by: t, elastic: true)
            g.setTranslation(.zero, in: self)
        case .ended:
            // a trackpad's lift carries on as a finger's does
            startMomentum(g.velocity(in: self))
        default:
            canvas.endPan()
        }
    }

    @objc func onPinch(_ g: UIPinchGestureRecognizer) {
        switch g.state {
        case .began, .changed:
            stopMomentum()
            canvas.zoom(by: g.scale, at: g.location(in: self))
            g.scale = 1
        default:
            canvas.endPan()
        }
    }

    // MARK: Momentum, as a scroll view's (it slows by its normal deceleration rate)

    func startMomentum(_ v: CGPoint) {
        guard hypot(v.x, v.y) > 60 else { return canvas.endPan() }
        velocity = v
        lastFrame = CACurrentMediaTime()
        let l = CADisplayLink(target: self, selector: #selector(onMomentum(_:)))
        l.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        l.add(to: .main, forMode: .common)
        momentum = l
    }

    func stopMomentum() {
        guard let m = momentum else { return }
        m.invalidate()
        momentum = nil
    }

    @objc func onMomentum(_ l: CADisplayLink) {
        let now = l.targetTimestamp
        let dt = max(now - lastFrame, 0)
        lastFrame = now
        // UIScrollView.DecelerationRate.normal is 0.998 a millisecond
        let k = pow(0.998, dt * 1000)
        velocity = CGPoint(x: velocity.x * k, y: velocity.y * k)
        canvas.pan(by: CGPoint(x: velocity.x * dt, y: velocity.y * dt), elastic: true)
        if hypot(velocity.x, velocity.y) < 20 {
            stopMomentum()
            canvas.endPan()
        }
    }

    // MARK: Taps

    @objc func onTap(_ g: UITapGestureRecognizer) {
        let p = g.location(in: self)
        let at = graph(p)
        // a touch's target size: a click with a pointer is precise enough at it too
        let s = slop(pointer: false)
        let mods = Self.mods(g.modifierFlags)
        let link = canvas.link
        onGesture?("tap")
        Task { [weak self] in
            do { try await link.tap(at, mods: mods, slop: s) } catch { self?.onError?("\(error)") }
        }
    }

    @objc func onDoubleTap(_ g: UITapGestureRecognizer) {
        if let back = smartReturn {
            smartReturn = nil
            canvas.setViewport(back, animated: true)
            onGesture?("doubleTap.back")
            return
        }
        let h = hit(at: g.location(in: self))
        smartReturn = canvas.viewport
        if h.ok, h.kind == .node || h.kind == .pin {
            canvas.fitFace(h.node)
            onGesture?("doubleTap.fitFace")
        } else {
            canvas.requestFit()
            onGesture?("doubleTap.fit")
        }
    }

    /// Fit everything (View › Fit, the F key).
    public func fitView() {
        smartReturn = nil
        canvas.requestFit()
    }

    // MARK: Hover (a pointer or a hovering Pencil)

    @objc func onHover(_ g: UIHoverGestureRecognizer) {
        switch g.state {
        case .began, .changed:
            let h = hit(at: g.location(in: self), pointer: true)
            canvas.hover(h.ok ? h : nil)
        default:
            canvas.hover(nil)
        }
    }

    // MARK: Context menus (long press, secondary click)

    /// The core's menu for what is at `p`: a `context` gesture (it hit-tests and says what it is
    /// on), then `menu.get {context}`. Empty when there is nothing.
    public func contextItems(at p: CGPoint) async -> [MenuItem] {
        let g = graph(p)
        let s = slop(pointer: false)
        let link = canvas.link
        let model = self.model
        do {
            let r = try await link.context(g, slop: s)
            guard let ctx = r.context, ctx != .null else { return [] }
            return try await model.contextMenu(ctx)
        } catch {
            onError?("\(error)")
            return []
        }
    }

    public func contextMenuInteraction(_ interaction: UIContextMenuInteraction, configurationForMenuAtLocation location: CGPoint) -> UIContextMenuConfiguration? {
        guard let make = contextMenuItems else { return nil }
        stopMomentum()
        contextAt = location
        // asked at once; the menu's items come when the core answers
        let task = Task { await self.contextItems(at: location) }
        contextTask = task
        onGesture?("context")
        return UIContextMenuConfiguration(identifier: nil, previewProvider: nil) { _ in
            let deferred = UIDeferredMenuElement.uncached { done in
                Task { @MainActor in
                    let items = await task.value
                    let elements = make(items)
                    done(elements.isEmpty ? [UIAction(title: "Nothing here", attributes: .disabled) { _ in }] : elements)
                }
            }
            return UIMenu(children: [deferred])
        }
    }

    /// The menu lifts the face under the press (or a small spot on empty canvas), not the canvas.
    public func contextMenuInteraction(_ interaction: UIContextMenuInteraction, configuration: UIContextMenuConfiguration,
                                       highlightPreviewForItemWithIdentifier identifier: any NSCopying) -> UITargetedPreview? {
        guard let p = contextAt else { return nil }
        let h = hit(at: p)
        var rect = CGRect(x: p.x - 8, y: p.y - 8, width: 16, height: 16)
        if h.ok, h.kind == .node || h.kind == .pin, let L = canvas.faceLayout(nid: h.node), let f = model.faces[h.node]?.item {
            let v = canvas.viewport
            rect = CGRect(x: f.pos.x * v.zoom + v.pan.x, y: f.pos.y * v.zoom + v.pan.y, width: L.size.width * v.zoom, height: L.size.height * v.zoom)
        }
        let params = UIPreviewParameters()
        params.backgroundColor = .clear
        params.visiblePath = UIBezierPath(roundedRect: CGRect(origin: .zero, size: rect.size), cornerRadius: 6 * canvas.viewport.zoom)
        let spot = resizableSnapshotView(from: rect, afterScreenUpdates: false, withCapInsets: .zero) ?? UIView(frame: rect)
        spot.frame = rect
        return UITargetedPreview(view: spot, parameters: params, target: UIPreviewTarget(container: self, center: CGPoint(x: rect.midX, y: rect.midY)))
    }

    // MARK: Snapshot

    /// The view as the canvas draws it (the app's own picture).
    public func snapshotPNG() -> Data? {
        let s = window?.screen.scale ?? canvas.screenScale
        guard let img = canvas.snapshot(size: bounds.size, scale: s) else { return nil }
        return UIImage(cgImage: img).pngData()
    }
}

#endif
