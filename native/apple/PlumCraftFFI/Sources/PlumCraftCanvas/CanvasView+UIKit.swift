// The iPad's host for the layer canvas (N3 slice 5; editing in N4 slice 1): the canvas itself is
// the Mac's; only recognition differs (plan 5.5's iPadOS column). What it recognises:
//
//   pan        one finger on empty canvas (and on a wire, a tag or a frame's body), two fingers
//              anywhere, with momentum when the fingers lift and the rubber band past the
//              content; a trackpad's or mouse's scroll
//   zoom       pinch about the fingers (a trackpad's pinch too)
//   drag       one finger on a face moves it (the selection with it when it is selected), on a
//              pin draws a wire (a wired input's picks its end up), on a frame's tab or handle, a
//              group's band or a bend point moves or sizes it: `ui.gesture` drags, decided by the
//              core and drawn by the kit's CanvasDrag, with autoscroll at the edge; a second
//              finger, Escape or the system cancels it and the document stays as it was
//   marquee    a drag on empty canvas with the Pencil, with a pointer, or in Select mode (the
//              app's toolbar toggle, `selectMode`); one finger otherwise pans (N4 slice 1)
//   tap        `ui.gesture` tap with the keyboard's modifiers: the core selects, or watches a pin
//   double tap `ui.gesture` doubleTap: on empty canvas the core opens the palette there, on a
//              face it does what the core says ("open what it is on"); Fit is the toolbar's, F
//              and View › Fit (the maintainer, N3's exit, cards 2 and 3)
//   long press, secondary click   on a face, pin, wire, tag, frame or group, the core's context
//              menu (`ui.gesture` context, `menu.get`) as a UIContextMenuInteraction whose items
//              the app makes (`contextMenuItems`); on empty canvas, the palette there
//   two-finger tap Undo; three-finger tap Redo; two-finger double tap Tidy (the iPad spike's
//              rule, note 24 section 17); the app turns the system's three-finger editing
//              gestures off for the graph's view (`editingInteractionConfiguration = .none`)
//   hover      a pointer or Pencil hovering rings the pin or widens the wire under it
//
// A press is held back until it is clearly a tap, a drag or a long press: UIKit's recognisers
// decide (a pan begins past its slop, a tap fails on movement, the context menu waits 0.5 s).
// A mouse or trackpad (an indirect pointer) works as on the Mac: a drag on empty canvas is a
// marquee, its scroll pans, and its hit target is a pointer's.

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
    /// One finger, a Pencil or a pointer on what the core drags (or anywhere, for a marquee).
    let dragPan = UIPanGestureRecognizer()
    let twoTap = UITapGestureRecognizer()
    let twoDoubleTap = UITapGestureRecognizer()
    let threeTap = UITapGestureRecognizer()
    var lastPan = CGPoint.zero
    var momentum: CADisplayLink?
    var velocity = CGPoint.zero
    var lastFrame: CFTimeInterval = 0
    var contextAt: CGPoint?
    var contextTask: Task<[MenuItem], Never>?
    /// The type of the touch that began the press under way (a finger, a Pencil, a pointer).
    var pressType: UITouch.TouchType = .direct
    /// Select mode, the core's (`canvas.selectMode`, the scene's `view.select`; N4 slice 3): a
    /// one-finger drag on empty canvas is a marquee.
    public var selectMode: Bool { model.view.selectMode }
    /// The drag under way (the kit's, shared with the Mac).
    public let drag: CanvasDrag
    /// The node palette (the core's state); the app presents it (`presentPalette`).
    public let palette: PaletteModel
    /// The palette opened: the host presents it at the view point given (`byKey`: Tab opened it,
    /// so its field takes the keyboard).
    public var presentPalette: ((CGPoint, Bool) -> Void)?
    /// The palette closed in the core: the host dismisses what it presented.
    public var dismissPalette: (() -> Void)?
    var paletteByKey = false
    /// Undo, Redo and Tidy from the touch taps: the host runs the command for the person.
    public var onCommand: ((String) -> Void)?
    /// A gesture ran commands (the host says when one has no UI yet).
    public var onCommands: (([CommandRun]) -> Void)?
    let snapFeedback = UISelectionFeedbackGenerator()
    /// Editing a value on a face (N4 slice 2): the core's row and the control over it.
    public let rowEditor: RowEditor
    public private(set) var fieldEditor: FieldEditorHost!
    /// A name edited in place (N4 slice 3).
    public let nameEditor: NameEditor
    public private(set) var nameField: NameFieldHost!

    public init(model: DocumentModel, frame: CGRect = CGRect(x: 0, y: 0, width: 1024, height: 768)) {
        let host = CALayer()
        canvas = LayerCanvas(model: model, host: host, screenScale: UITraitCollection.current.displayScale)
        palette = PaletteModel(model: model)
        rowEditor = RowEditor(model: model)
        nameEditor = NameEditor(model: model)
        let c = canvas
        // a finger's target size (44 points across, plan 5.5); a pointer's while one is pressing
        drag = CanvasDrag(canvas: c) { CanvasLink.slop(points: 22, zoom: Double(c.viewport.zoom)) }
        super.init(frame: frame)
        fieldEditor = FieldEditorHost(canvasView: self, editor: rowEditor)
        nameField = NameFieldHost(canvasView: self, editor: nameEditor)
        canvas.followers = { [weak self] in
            self?.fieldEditor.follow()
            self?.nameField.follow()
        }
        drag.onError = { [weak self] e in self?.onError?(e) }
        drag.onBegan = { [weak self] k in self?.onGesture?("drag.begin.\(k?.rawValue ?? "none")") }
        drag.onEnded = { [weak self] k, r in
            self?.onGesture?(r == nil ? "drag.cancel" : "drag.end.\(k?.rawValue ?? "none")")
            if let r { self?.onCommands?(r.commands) }
        }
        drag.onSnap = { [weak self] in self?.snapFeedback.selectionChanged() }
        palette.fallbackAt = { [weak self] in
            guard let self else { return Point(0, 0) }
            return graph(CGPoint(x: bounds.midX, y: bounds.midY))
        }
        palette.onOpen = { [weak self] p in self?.showPalette(p) }
        palette.onClose = { [weak self] in
            self?.dismissPalette?()
            self?.drag.releaseWire()
        }
        model.onView = { [weak self] v in
            self?.palette.follow(v)
            self?.rowEditor.follow(v)
            self?.nameEditor.follow(v.name)
        }
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
        // a drag the core decides on: one finger, a Pencil or a pointer (gestureRecognizerShouldBegin)
        dragPan.maximumNumberOfTouches = 1
        dragPan.addTarget(self, action: #selector(onDrag(_:)))
        // the touch taps of the iPad spike: two fingers Undo, three Redo, two twice Tidy
        twoTap.numberOfTouchesRequired = 2
        twoTap.addTarget(self, action: #selector(onTwoTap(_:)))
        twoDoubleTap.numberOfTouchesRequired = 2
        twoDoubleTap.numberOfTapsRequired = 2
        twoDoubleTap.addTarget(self, action: #selector(onTwoDoubleTap(_:)))
        twoTap.require(toFail: twoDoubleTap)
        threeTap.numberOfTouchesRequired = 3
        threeTap.addTarget(self, action: #selector(onThreeTap(_:)))
        for t in [twoTap, twoDoubleTap, threeTap] { t.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)] }
        onePan.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]
        for g in [onePan, twoPan, scrollPan, pinch, tap, doubleTap, dragPan, twoTap, twoDoubleTap, threeTap] as [UIGestureRecognizer] {
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

    public func graph(_ v: CGPoint) -> Point {
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

    public func gestureRecognizer(_ g: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
        // a touch on a field editor or a face's menu button is theirs, not the canvas's
        if let v = touch.view, v !== self { return false }
        if g === dragPan || g === onePan { pressType = touch.type }
        return true
    }

    /// A touch on a choice's row or the more-button goes to the button over it with the system's
    /// menu (the core's choices, the node's menu); everything else is the canvas's.
    public override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        let v = super.hitTest(point, with: event)
        if let v, v !== self { return v }
        guard event?.type == .touches || event == nil, !drag.active else { return v }
        let h = hit(at: point)
        if h.ok, h.kind == .node, let routed = fieldEditor.routeTouch(at: point, hit: h) { return routed }
        fieldEditor.menuButton.isHidden = true
        return v
    }

    /// Whether the core drags what is at `h` (a face, a pin, a bend point, a frame's tab or
    /// handle, an open group's band); anything else a drag would marquee.
    static func dragsItem(_ h: HotHit) -> Bool {
        guard h.ok else { return false }
        switch h.kind {
        case .node, .pin, .bend, .group: return true
        case .frame: return h.part != 1
        default: return false
        }
    }

    /// A drag on empty canvas (a wire, a tag, a frame's body) is a marquee rather than a pan.
    var marqueeing: Bool { selectMode || pressType == .pencil || pressType == .indirectPointer }

    public override func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
        if g === onePan || g === dragPan {
            // where the press went down, not where the pan was recognised
            let p0 = downPoint(g as? UIPanGestureRecognizer)
            let item = Self.dragsItem(hit(at: p0, pointer: pressType == .indirectPointer))
            if g === onePan { return !item && !marqueeing }
            return item || marqueeing
        }
        return true
    }

    func downPoint(_ g: UIPanGestureRecognizer?) -> CGPoint {
        guard let g else { return .zero }
        let p = g.location(in: self)
        let t = g.translation(in: self)
        return CGPoint(x: p.x - t.x, y: p.y - t.y)
    }

    public override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        // a finger down stops the momentum; a second finger cancels a drag under way
        stopMomentum()
        if drag.active, (event?.allTouches?.count ?? 0) > 1 { cancelDrag() }
        super.touchesBegan(touches, with: event)
    }

    // MARK: Drags, decided by the core

    @objc func onDrag(_ g: UIPanGestureRecognizer) {
        let p = g.location(in: self)
        switch g.state {
        case .began:
            stopMomentum()
            canvas.hover(nil)
            snapFeedback.prepare()
            drag.slop = { [weak self] in self?.slop(pointer: self?.pressType == .indirectPointer) ?? 22 }
            drag.begin(at: downPoint(g), mods: Self.mods(g.modifierFlags))
            drag.move(to: p)
        case .changed:
            drag.move(to: p)
        case .ended:
            drag.end(at: p)
        default:
            drag.cancel()
        }
    }

    /// Escape, a second finger: the drag under way goes, and the document is as it was.
    public func cancelDrag() {
        guard drag.active else { return }
        drag.cancel()
        // the recogniser stops tracking this press
        dragPan.isEnabled = false
        dragPan.isEnabled = true
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
            do {
                let r = try await link.tap(at, mods: mods, slop: s)
                self?.onCommands?(r.commands)
            } catch { self?.onError?("\(error)") }
        }
    }

    /// The core's double tap: the palette on empty canvas, "open what it is on" on a face.
    @objc func onDoubleTap(_ g: UITapGestureRecognizer) {
        let at = graph(g.location(in: self))
        let s = slop(pointer: false)
        let mods = Self.mods(g.modifierFlags)
        let link = canvas.link
        onGesture?("doubleTap")
        Task { [weak self] in
            do {
                let r = try await link.doubleTap(at, mods: mods, slop: s)
                self?.onCommands?(r.commands)
            } catch { self?.onError?("\(error)") }
        }
    }

    @objc func onTwoTap(_ g: UITapGestureRecognizer) {
        onGesture?("undo")
        command("edit.undo")
    }

    @objc func onThreeTap(_ g: UITapGestureRecognizer) {
        onGesture?("redo")
        command("edit.redo")
    }

    @objc func onTwoDoubleTap(_ g: UITapGestureRecognizer) {
        onGesture?("tidy")
        command("graph.tidy")
    }

    /// Run `id` for the person: the host's (it shows refusals), else straight to the core.
    func command(_ id: String) {
        if let onCommand { return onCommand(id) }
        let model = self.model
        Task { [weak self] in
            do { _ = try await model.invoke(id, person: true) } catch { self?.onError?("\(error)") }
        }
    }

    /// Fit everything (the toolbar's Fit, View › Fit, the F key).
    public func fitView() {
        canvas.requestFit()
    }

    // MARK: The palette

    /// Open the core's palette at view point `p` (the view's centre when nil): Tab, Node ›
    /// Search…, the toolbar's Add.
    public func openPalette(at p: CGPoint? = nil, byKey: Bool = false) {
        let at = graph(p ?? CGPoint(x: bounds.midX, y: bounds.midY))
        paletteByKey = byKey
        let palette = self.palette
        Task { [weak self] in
            do { try await palette.open(at: at) } catch { self?.onError?("\(error)") }
        }
    }

    /// The core opened the palette: the host presents it at its point (the view's centre when it
    /// has none or the point is out of sight).
    func showPalette(_ p: Palette) {
        var v = CGPoint(x: bounds.midX, y: bounds.midY)
        if let at = p.at {
            let q = canvas.viewport.toView(CGPoint(x: at.x, y: at.y))
            if bounds.insetBy(dx: 8, dy: 8).contains(q) { v = q }
        }
        onGesture?(p.wire != nil ? "palette.wire" : "palette.open")
        presentPalette?(v, paletteByKey)
        paletteByKey = false
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
        guard let make = contextMenuItems, !drag.active else { return nil }
        stopMomentum()
        if !hit(at: location).ok || hit(at: location).kind == .none {
            // empty canvas: no menu, the palette there (the core's context on nothing)
            let g = graph(location)
            let s = slop(pointer: false)
            let link = canvas.link
            onGesture?("context.palette")
            Task { [weak self] in
                do { _ = try await link.context(g, slop: s) } catch { self?.onError?("\(error)") }
            }
            return nil
        }
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
