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
//        marquee on empty canvas, a wire, a tag or a frame's body; a move on a face; a wire from
//        a pin, or a wired input's end picked up; a frame, a group, a bend point) through
//        ui.gesture's drag, drawn by the kit's CanvasDrag (N4 slice 1), with autoscroll at the
//        view's edge; Escape cancels it.
//   Add: a double click on empty canvas, a secondary click there, Tab, or Node › Search… opens
//        the core's palette at that point (N4 slice 1), shown as a popover (PalettePopover).
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
    /// The open hand is up over a right-angled route's corner or leg (B155).
    var routeCursor = false
    var panning = false
    var lastPoint = CGPoint.zero
    var downPoint: CGPoint?
    var downMods: [ModifierWord] = []
    var dragging = false
    /// The drag under way (the kit's, shared with the iPad).
    public let drag: CanvasDrag
    /// The node palette (the core's state) and its popover.
    public let palette: PaletteModel
    public let palettePopover: PalettePopover
    /// Editing a value on a face (N4 slice 2): the core's row and the control over it.
    public let rowEditor: RowEditor
    public private(set) var fieldEditor: FieldEditorHost!
    /// A name edited in place (N4 slice 3): the core's name and the field over it.
    public let nameEditor: NameEditor
    public private(set) var nameField: NameFieldHost!
    /// A gesture ran commands (the host says when one has no UI yet).
    public var onCommands: (([CommandRun]) -> Void)?
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
        palette = PaletteModel(model: model)
        let c = canvas
        drag = CanvasDrag(canvas: c) { CanvasLink.slop(points: 10, zoom: Double(c.viewport.zoom)) }
        palettePopover = PalettePopover(palette: palette)
        rowEditor = RowEditor(model: model)
        nameEditor = NameEditor(model: model)
        super.init(frame: frame)
        fieldEditor = FieldEditorHost(canvasView: self, editor: rowEditor)
        nameField = NameFieldHost(canvasView: self, editor: nameEditor)
        canvas.followers = { [weak self] in
            self?.fieldEditor.follow()
            self?.nameField.follow()
        }
        drag.onError = { [weak self] e in self?.onError?(e) }
        drag.onEnded = { [weak self] _, r in if let r { self?.onCommands?(r.commands) } }
        palette.fallbackAt = { [weak self] in
            guard let self else { return Point(0, 0) }
            return graph(CGPoint(x: bounds.midX, y: bounds.midY))
        }
        palette.onOpen = { [weak self] p in self?.showPalette(p) }
        palette.onClose = { [weak self] in
            self?.palettePopover.close()
            self?.drag.releaseWire()
        }
        model.onView = { [weak self] v in
            self?.palette.follow(v)
            self?.rowEditor.follow(v)
            self?.nameEditor.follow(v.name)
        }
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

    /// The system's appearance, told to the core (`lifecycle.appearance`, N4 slice 4): a theme
    /// that matches the system shows Paper in light mode and Dusk in dark, and Increase Contrast
    /// and Reduce Transparency change its tokens; the canvas redraws when `theme.get` answers
    /// (the feed's `prefs`). The theme itself is the core's setting (Settings › Theme).
    public var followsAppearance = true
    var appearanceTold: JSONValue?
    var accessibilityObserver: NSObjectProtocol?

    public override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        followAppearance()
    }

    public override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if accessibilityObserver == nil {
            accessibilityObserver = NSWorkspace.shared.notificationCenter.addObserver(
                forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification, object: nil, queue: .main
            ) { [weak self] _ in MainActor.assumeIsolated { self?.followAppearance() } }
        }
        followAppearance()
    }

    /// What the system's appearance is now, as `lifecycle.appearance` takes it.
    public var appearanceNow: JSONValue {
        let dark = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        let ws = NSWorkspace.shared
        return ["dark": .bool(dark), "contrast": ws.accessibilityDisplayShouldIncreaseContrast ? "more" : "normal",
                "reduceMotion": .bool(ws.accessibilityDisplayShouldReduceMotion),
                "reduceTransparency": .bool(ws.accessibilityDisplayShouldReduceTransparency)]
    }

    public func followAppearance() {
        guard followsAppearance, window != nil else { return }
        let now = appearanceNow
        if let told = appearanceTold, told.same(now) { return }
        appearanceTold = now
        let model = self.model
        Task {
            _ = try? await model.core.request("lifecycle.appearance", now)
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
            dragging = false
            downPoint = nil
            drag.cancel()
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
            case 48 where !e.modifierFlags.contains(.shift):  // Tab: the palette at the pointer
                openPalette(at: pointerInView())
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
        // a right-angled route's corner or leg can be dragged (B155): the hand says so
        let onRoute = h.ok && h.kind == .wire && (h.part == 1 || h.part == 2)
        if onRoute != routeCursor {
            routeCursor = onRoute
            (onRoute ? NSCursor.openHand : NSCursor.arrow).set()
        }
        // a node's error, whole, as the tooltip (the face shows it cut to 180 characters)
        var tip: String?
        if h.ok, h.kind == .node || h.kind == .pin, let f = model.faces[h.node]?.item {
            for case .error(let text, let full, _) in f.footer { tip = full.isEmpty ? text : full }
            // else an input's own words, as Windows shows them on its pin (B164: what a wire into
            // it takes from a list joined from several runs is said there)
            if tip == nil {
                let row: Int? = h.kind == .pin && h.part == 1 ? Int(h.pin)
                    : h.kind == .node && h.part == HotHit.NodePart.row.rawValue && h.index >= 0 ? Int(h.index) : nil
                if let row, row < f.inputs.count, let words = f.inputs[row].hover, !words.isEmpty { tip = words }
            }
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
        dragging = false
        // a press on a row: if the core opens an editor for it, it is a person's (a pop-up's menu
        // may open at once)
        let h = canvas.link.hit(graph(p), slop: slop)
        if h.ok, h.nodePart == .row { fieldEditor.noteClick() }
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
            canvas.hover(nil)
            drag.begin(at: d, mods: downMods)
        }
        drag.move(to: p)
    }

    public override func mouseUp(with e: NSEvent) {
        let p = point(e)
        if panning {
            panning = false
            if spaceHeld { NSCursor.openHand.set() } else { NSCursor.arrow.set() }
            return
        }
        defer { downPoint = nil }
        if dragging {
            dragging = false
            drag.end(at: p)
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
                let r = if double {
                    try await canvas.link.doubleTap(g, mods: mods, slop: s)
                } else {
                    try await canvas.link.tap(g, mods: mods, slop: s)
                }
                onCommands?(r.commands)
                // the more-button: the node's menu, at the button (N4 slice 2)
                if let ctx = r.context, ctx != .null, !double {
                    let items = try await model.contextMenu(ctx)
                    onContextMenu?(p, items)
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

    // MARK: The palette

    /// Where the pointer is in the view (the view's centre when it is outside).
    func pointerInView() -> CGPoint {
        guard let w = window else { return CGPoint(x: bounds.midX, y: bounds.midY) }
        let p = convert(w.mouseLocationOutsideOfEventStream, from: nil)
        return bounds.contains(p) ? p : CGPoint(x: bounds.midX, y: bounds.midY)
    }

    /// Open the core's palette at view point `p` (Tab, Node › Search…, the toolbar).
    public func openPalette(at p: CGPoint? = nil) {
        let at = graph(p ?? pointerInView())
        let palette = self.palette
        Task { [weak self] in
            do { try await palette.open(at: at) } catch { self?.onError?("\(error)") }
        }
    }

    /// The core opened the palette: the popover at its point (the view's centre when it has none
    /// or the point is out of sight).
    func showPalette(_ p: Palette) {
        var v = CGPoint(x: bounds.midX, y: bounds.midY)
        if let at = p.at {
            let q = canvas.viewport.toView(CGPoint(x: at.x, y: at.y))
            if bounds.insetBy(dx: 8, dy: 8).contains(q) { v = q }
        }
        palettePopover.show(at: v, in: self)
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
