// The Mac's host for the layer canvas: an NSView whose layer is the canvas's, recognising the
// macOS column of plan 5.5. What is recognised goes to the core as `ui.gesture`; pan and zoom are
// the view's own transform and never cross (5.3).
//
//   Pan: two-finger scroll (with the system's momentum, and a rubber band past the content),
//        Space-drag, middle drag; a mouse wheel scrolls without the band.
//   Zoom: pinch and ⌘-scroll about the pointer; ⌘+ ⌘− ⌘0 (the View menu calls zoomIn, zoomOut,
//        actualSize); smart zoom (two-finger double tap) fits the node under the pointer, and a
//        second one goes back.
//   Select: click, Shift-click (adds), ⌘-click (takes away); a drag the core decides on (a
//        marquee on empty canvas, a move on a face, a wire from a pin) through ui.gesture's drag.
//   Hover: a ring on the pin and a wider wire under the pointer, from the typed hit test.

#if os(macOS)
import AppKit
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import QuartzCore

@MainActor
public final class CanvasView: NSView, NSMenuItemValidation {
    public func validateMenuItem(_ mi: NSMenuItem) -> Bool {
        validate?(mi) ?? true
    }

    public let canvas: LayerCanvas
    public var model: DocumentModel { canvas.model }
    let root = CALayer()

    var spaceHeld = false
    var panning = false
    var lastPoint = CGPoint.zero
    var downPoint: CGPoint?
    var downMods: [ModifierWord] = []
    var downHit: HotHit?
    var dragTask: Task<GestureReply?, Never>?
    var dragKind: DragWord?
    var dragging = false
    var smartReturn: Viewport?
    var endPanTask: Task<Void, Never>?
    var trackingArea: NSTrackingArea?
    /// Errors from the core that the user didn't cause directly (a gesture refused), for a log.
    public var onError: ((String) -> Void)?
    /// A secondary click (or Control-click) got the core's context menu for what it was on: the
    /// host shows it at the view point and dispatches its items (N3 slice 4).
    public var onContextMenu: ((CGPoint, [MenuItem]) -> Void)?
    /// A key the canvas recognises for the core (contract 12.3: Delete, Escape, F, the arrows) was
    /// pressed: the host runs `command` for the person (the menu model's own dispatch).
    public var onCanvasKey: ((String) -> Void)?
    var keyPress: UInt64 = 0

    public init(model: DocumentModel, frame: NSRect = NSRect(x: 0, y: 0, width: 1024, height: 768)) {
        root.isGeometryFlipped = true
        root.masksToBounds = true
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        canvas = LayerCanvas(model: model, host: root, screenScale: scale)
        super.init(frame: frame)
        layer = root
        wantsLayer = true
        layerContentsRedrawPolicy = .never
        canvas.viewSize = frame.size
        setAccessibilityElement(true)
        setAccessibilityRole(.layoutArea)
        setAccessibilityLabel("Canvas")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    // MARK: Following the system's appearance

    /// Paper in light mode, Dusk in dark (plan 5.5's "follow the system"): the core builds the
    /// scene in the theme named (`view.theme`), and the canvas redraws when `theme.get` answers.
    /// Not kept between runs; the egui editor's own theme setting is untouched.
    public var followsAppearance = true
    var themeAsked: String?

    public override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        followAppearance()
    }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        followAppearance()
    }

    public func followAppearance() {
        guard followsAppearance else { return }
        let dark = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let name = dark ? "dusk" : "paper"
        if name == themeAsked { return }
        themeAsked = name
        let model = self.model
        Task {
            _ = try? await model.run("view.theme", ["name": .string(name)])
            await model.refreshTheme()
        }
    }

    public override var isFlipped: Bool { true }
    public override var acceptsFirstResponder: Bool { true }
    public override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    public override func setFrameSize(_ newSize: NSSize) {
        super.setFrameSize(newSize)
        canvas.viewSize = newSize
    }

    public override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let t = trackingArea { removeTrackingArea(t) }
        let t = NSTrackingArea(rect: .zero, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect, .mouseEnteredAndExited], owner: self, userInfo: nil)
        addTrackingArea(t)
        trackingArea = t
    }

    func point(_ e: NSEvent) -> CGPoint { convert(e.locationInWindow, from: nil) }
    func graph(_ v: CGPoint) -> Point {
        let g = canvas.viewport.toGraph(v)
        return Point(Double(g.x), Double(g.y))
    }
    /// The hit distance: 10 points on screen, in graph units.
    var slop: Double { CanvasLink.slop(points: 10, zoom: Double(canvas.viewport.zoom)) }

    static func mods(_ f: NSEvent.ModifierFlags) -> [ModifierWord] {
        var m: [ModifierWord] = []
        if f.contains(.shift) { m.append(.shift) }
        if f.contains(.command) { m.append(.primary) }
        if f.contains(.option) { m.append(.alt) }
        if f.contains(.control) { m.append(.ctrl) }
        return m
    }

    // MARK: Pan and zoom

    public override func scrollWheel(with e: NSEvent) {
        let p = point(e)
        if e.modifierFlags.contains(.command) {
            let dy = e.hasPreciseScrollingDeltas ? e.scrollingDeltaY : e.scrollingDeltaY * 8
            canvas.zoom(by: exp(dy * 0.01), at: p)
            return
        }
        let precise = e.hasPreciseScrollingDeltas
        let k: CGFloat = precise ? 1 : 10
        if e.momentumPhase == .began { endPanTask?.cancel() }
        canvas.pan(by: CGPoint(x: e.scrollingDeltaX * k, y: e.scrollingDeltaY * k), elastic: precise)
        if e.momentumPhase == .ended || e.momentumPhase == .cancelled {
            canvas.endPan()
        } else if e.phase == .ended || e.phase == .cancelled {
            // momentum may follow; if it doesn't within a moment, spring back now
            endPanTask?.cancel()
            endPanTask = Task { @MainActor [weak self] in
                try? await Task.sleep(for: .milliseconds(80))
                if !Task.isCancelled { self?.canvas.endPan() }
            }
        }
    }

    public override func magnify(with e: NSEvent) {
        canvas.zoom(by: 1 + e.magnification, at: point(e))
    }

    public override func smartMagnify(with e: NSEvent) {
        if let back = smartReturn {
            smartReturn = nil
            canvas.setViewport(back, animated: true)
            return
        }
        let h = canvas.link.hit(graph(point(e)), slop: slop)
        smartReturn = canvas.viewport
        if h.ok, h.kind == .node || h.kind == .pin {
            canvas.fitFace(h.node)
        } else {
            canvas.requestFit()
        }
    }

    @objc public func zoomIn(_ sender: Any?) { canvas.zoom(to: canvas.viewport.zoom * 1.25) }
    @objc public func zoomOut(_ sender: Any?) { canvas.zoom(to: canvas.viewport.zoom / 1.25) }
    @objc public func actualSize(_ sender: Any?) { canvas.zoom(to: 1) }
    @objc public func fitView(_ sender: Any?) {
        smartReturn = nil
        canvas.requestFit()
    }

    // MARK: Keys

    public override func keyDown(with e: NSEvent) {
        if e.charactersIgnoringModifiers == " " {
            if !e.isARepeat {
                spaceHeld = true
                NSCursor.openHand.set()
            }
            return
        }
        if e.keyCode == 53, dragging {  // Escape cancels a drag
            cancelDrag()
            return
        }
        // the canvas's keys (12.3), when no modifier but Shift is held
        let plain = e.modifierFlags.intersection([.command, .control, .option]).isEmpty
        if plain {
            switch e.keyCode {
            case 51, 117:  // Delete, Forward Delete
                onCanvasKey?("edit.delete")
                return
            case 53:  // Escape: cancels a run in flight, else clears the selection
                onCanvasKey?("escape")
                return
            case 123, 124, 125, 126:  // arrows nudge; Shift ten times as far; a held key is one step
                if !e.isARepeat { keyPress &+= 1 }
                let k: Double = e.modifierFlags.contains(.shift) ? 10 : 1
                let (dx, dy): (Double, Double) = switch e.keyCode {
                case 123: (-k, 0)
                case 124: (k, 0)
                case 125: (0, k)
                default: (0, -k)
                }
                let link = canvas.link
                let press = keyPress
                Task { [weak self] in
                    do { _ = try await link.nudge(dx: dx, dy: dy, press: press) } catch { self?.onError?("\(error)") }
                }
                return
            default:
                if e.charactersIgnoringModifiers?.lowercased() == "f" {
                    fitView(nil)
                    return
                }
            }
        }
        super.keyDown(with: e)
    }

    public override func keyUp(with e: NSEvent) {
        if e.charactersIgnoringModifiers == " " {
            spaceHeld = false
            if !panning { NSCursor.arrow.set() }
            return
        }
        super.keyUp(with: e)
    }

    // MARK: The mouse

    public override func mouseMoved(with e: NSEvent) {
        if spaceHeld || panning { return }
        let h = canvas.link.hit(graph(point(e)), slop: slop)
        canvas.hover(h.ok ? h : nil)
        // a node's error, whole, as the tooltip (the face shows it cut to 180 characters)
        var tip: String?
        if h.ok, h.kind == .node || h.kind == .pin, let f = model.faces[h.node]?.item {
            for case .error(let text, let full, _) in f.footer { tip = full.isEmpty ? text : full }
        }
        if toolTip != tip { toolTip = tip }
    }

    // MARK: Context menus

    public override func rightMouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        context(at: point(e))
    }

    /// The core's menu for what is at `p`: a `context` gesture (it hit-tests and says what it is
    /// on), then `menu.get {context}`.
    func context(at p: CGPoint) {
        let g = graph(p)
        let s = slop
        let link = canvas.link
        let model = self.model
        Task { [weak self] in
            do {
                let r = try await link.context(g, slop: s)
                guard let ctx = r.context, ctx != .null else { return }
                let items = try await model.contextMenu(ctx)
                self?.onContextMenu?(p, items)
            } catch {
                self?.onError?("\(error)")
            }
        }
    }

    public override func mouseExited(with e: NSEvent) {
        canvas.hover(nil)
    }

    public override func mouseDown(with e: NSEvent) {
        window?.makeFirstResponder(self)
        let p = point(e)
        // Control-click is a secondary click on the Mac
        if e.modifierFlags.contains(.control) && !spaceHeld {
            downPoint = nil
            context(at: p)
            return
        }
        lastPoint = p
        if spaceHeld {
            panning = true
            NSCursor.closedHand.set()
            return
        }
        downPoint = p
        downMods = Self.mods(e.modifierFlags)
        let h = canvas.link.hit(graph(p), slop: slop)
        downHit = h.ok ? h : nil
        dragging = false
    }

    public override func mouseDragged(with e: NSEvent) {
        let p = point(e)
        defer { lastPoint = p }
        if panning {
            canvas.pan(by: CGPoint(x: p.x - lastPoint.x, y: p.y - lastPoint.y), elastic: false)
            return
        }
        guard let d = downPoint else { return }
        if !dragging {
            guard hypot(p.x - d.x, p.y - d.y) > 3 else { return }
            dragging = true
            beginDrag(at: graph(d))
        }
        moveDrag(to: p)
    }

    public override func mouseUp(with e: NSEvent) {
        let p = point(e)
        if panning {
            panning = false
            if spaceHeld { NSCursor.openHand.set() } else { NSCursor.arrow.set() }
            return
        }
        defer {
            downPoint = nil
            downHit = nil
        }
        if dragging {
            dragging = false
            endDrag(at: graph(p))
            return
        }
        guard downPoint != nil else { return }
        let g = graph(p)
        let mods = downMods
        let s = slop
        let double = e.clickCount == 2
        Task { [weak self] in
            guard let self else { return }
            do {
                if double {
                    try await canvas.link.doubleTap(g, mods: mods, slop: s)
                } else {
                    try await canvas.link.tap(g, mods: mods, slop: s)
                }
            } catch {
                onError?("\(error)")
            }
        }
    }

    public override func otherMouseDown(with e: NSEvent) {
        guard e.buttonNumber == 2 else { return super.otherMouseDown(with: e) }
        panning = true
        lastPoint = point(e)
        NSCursor.closedHand.set()
    }

    public override func otherMouseDragged(with e: NSEvent) {
        guard panning else { return super.otherMouseDragged(with: e) }
        let p = point(e)
        canvas.pan(by: CGPoint(x: p.x - lastPoint.x, y: p.y - lastPoint.y), elastic: false)
        lastPoint = p
    }

    public override func otherMouseUp(with e: NSEvent) {
        guard panning else { return super.otherMouseUp(with: e) }
        panning = false
        NSCursor.arrow.set()
    }

    // MARK: Drags, decided by the core

    func beginDrag(at g: Point) {
        let mods = downMods
        let s = slop
        dragKind = nil
        let link = canvas.link
        dragTask = Task { [weak self] in
            do {
                let r = try await link.dragBegin(g, mods: mods, slop: s)
                guard let self else { return r }
                dragKind = r.drag?.kind.word
                if dragKind == .move, let faces = r.drag?.faces { canvas.beginMove(faces) }
                if dragging, let p = latest { moveDrag(to: p) }
                return r
            } catch {
                self?.onError?("\(error)")
                return nil
            }
        }
    }

    var latest: CGPoint?

    /// Each pointer move while dragging: the typed move (`plum_gesture_move`), never JSON.
    func moveDrag(to p: CGPoint) {
        latest = p
        guard let kind = dragKind else { return }
        let g = graph(p)
        let d = canvas.link.dragMove(g, slop: slop)
        guard d.ok else { return }
        switch kind {
        case .move:
            canvas.move(dx: CGFloat(d.dx), dy: CGFloat(d.dy))
        case .marquee:
            canvas.showMarquee(CGRect(x: CGFloat(d.rect.0), y: CGFloat(d.rect.1), width: CGFloat(d.rect.2 - d.rect.0), height: CGFloat(d.rect.3 - d.rect.1)))
        case .wire:
            let from = downHit.map { CGPoint(x: CGFloat($0.x), y: CGFloat($0.y)) }
            let snap = d.target.kind == .pin ? CGPoint(x: CGFloat(d.target.x), y: CGFloat(d.target.y)) : CGPoint(x: g.x, y: g.y)
            canvas.showDragWire(from: from, to: snap, fits: d.target.kind == .pin ? d.target.fits : 0)
        default:
            break
        }
    }

    func endDrag(at g: Point) {
        let s = slop
        let begun = dragTask
        let link = canvas.link
        Task { [weak self] in
            _ = await begun?.value
            var moved = false
            do {
                let r = try await link.dragEnd(g, slop: s)
                moved = r.commands.contains { $0.ok && $0.command == "edit.move" }
            } catch {
                self?.onError?("\(error)")
            }
            self?.clearDrag(kept: moved)
        }
    }

    func cancelDrag() {
        dragging = false
        let begun = dragTask
        let link = canvas.link
        Task { [weak self] in
            _ = await begun?.value
            _ = try? await link.dragCancel()
            self?.clearDrag()
        }
    }

    func clearDrag(kept: Bool = false) {
        dragKind = nil
        latest = nil
        canvas.showMarquee(nil)
        canvas.showDragWire(from: nil, to: .zero, fits: 0)
        canvas.endMove(kept: kept)
    }

    // MARK: The standard actions (the Edit menu's, down the responder chain)

    /// How the host validates the Edit menu's standard items for the canvas: the model's
    /// enabled state and the core's step names (nil: enabled).
    public var validate: ((NSMenuItem) -> Bool?)?

    @objc public func undo(_ sender: Any?) { onCanvasKey?("edit.undo") }
    @objc public func redo(_ sender: Any?) { onCanvasKey?("edit.redo") }
    @objc public func cut(_ sender: Any?) { onCanvasKey?("edit.cut") }
    @objc public func copy(_ sender: Any?) { onCanvasKey?("edit.copy") }
    @objc public func paste(_ sender: Any?) { onCanvasKey?("edit.paste") }
    @objc public func delete(_ sender: Any?) { onCanvasKey?("edit.delete") }
    @objc public override func selectAll(_ sender: Any?) { onCanvasKey?("selection.all") }

    // MARK: Snapshot

    /// The view as the canvas draws it (the app's own picture, not the window server's).
    public func snapshotPNG() -> Data? {
        let s = window?.backingScaleFactor ?? canvas.screenScale
        guard let img = canvas.snapshot(size: bounds.size, scale: s) else { return nil }
        return NSBitmapImageRep(cgImage: img).representation(using: .png, properties: [:])
    }
}
#endif
