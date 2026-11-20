// A graph file as an NSDocument: the system gives File › New, Open, Open Recent, Save, Save As,
// Revert, the title with its proxy icon and edited dot, the unsaved-changes sheet on close and the
// review on quit; the core holds the graph and writes the file itself.
//
// The edited state is the core's: the feed's `document` change carries `dirty`, and the document's
// change count follows it (`updateChangeCount(.changeDone)` when it becomes dirty,
// `.changeCleared` when it becomes clean), so the dot, the close sheet and Quit's review see what
// the core says. Saving goes to the core (`graph.save {path}` or `graph.saveAs {path}`), which
// writes the file atomically; NSDocument's own writing is never used. Autosave in place is off:
// desktop keeps explicit Save (plan 6, decision 7), and autosave would make the core write the
// file behind its own dirty state.

import AppKit
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

@objc(GraphDocument)
final class GraphDocument: NSDocument {
    /// The core's number for this document, once opened.
    var doc: UInt64?
    var model: DocumentModel?

    override class var autosavesInPlace: Bool { false }
    override class var autosavesDrafts: Bool { false }
    override class func canConcurrentlyReadDocuments(ofType typeName: String) -> Bool { false }

    /// A graph is a JSON object with `nodes`; anything else is refused here with the reason, as
    /// the Mac does for a file an app can't read.
    override nonisolated func read(from data: Data, ofType typeName: String) throws {
        let v = try? JSONSerialization.jsonObject(with: data)
        guard let o = v as? [String: Any], o["nodes"] != nil else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError,
                          userInfo: [NSLocalizedFailureReasonErrorKey: "It isn't a PlumCraft graph (a JSON object with \"nodes\")."])
        }
    }

    override func makeWindowControllers() {
        addWindowController(GraphWindowController(document: self))
    }

    /// The core's dirty state, as the feed said it.
    func follow(_ st: DocumentState) {
        if st.dirty && !isDocumentEdited {
            updateChangeCount(.changeDone)
        } else if !st.dirty && isDocumentEdited {
            updateChangeCount(.changeCleared)
        }
    }

    /// Save and Save As: the core writes the file (`graph.save` to the document's own file,
    /// `graph.saveAs` to another), then NSDocument records where and clears the edited dot.
    override func save(to url: URL, ofType typeName: String, for saveOperation: NSDocument.SaveOperationType,
                       completionHandler: @escaping (Error?) -> Void) {
        guard let model else {
            return completionHandler(NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError,
                                             userInfo: [NSLocalizedFailureReasonErrorKey: "The graph isn't open yet."]))
        }
        let same = saveOperation == .saveOperation && url.standardizedFileURL == fileURL?.standardizedFileURL
        let command = same ? "graph.save" : "graph.saveAs"
        let path = url.path
        nonisolated(unsafe) let done = completionHandler
        Task {
            do {
                _ = try await model.run(command, ["path": .string(path)])
                done(nil)
            } catch {
                done(NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError,
                             userInfo: [NSLocalizedFailureReasonErrorKey: "\(error)"]))
            }
        }
    }

    /// Revert to Saved: the core opens the file again, dropping what changed.
    override func revert(toContentsOf url: URL, ofType typeName: String) throws {
        guard let model else { return }
        Task { _ = try? await model.run("graph.open", ["path": .string(url.path), "discard": true]) }
    }

    override func close() {
        if let doc, let core = CoreHost.core {
            // the person has decided (NSDocument's sheet, or nothing unsaved)
            core.post("document.close", ["discard": true], doc: doc)
            Status.shared.removeWindow(doc)
        }
        model?.stop()
        super.close()
    }
}

/// A drop of graph files on a window opens each in its own window.
final class DropView: NSView {
    override init(frame: NSRect) {
        super.init(frame: frame)
        registerForDraggedTypes([.fileURL])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    func urls(_ s: NSDraggingInfo) -> [URL] {
        (s.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? [])
            .filter { $0.pathExtension.lowercased() == "json" }
    }

    override func draggingEntered(_ s: NSDraggingInfo) -> NSDragOperation { urls(s).isEmpty ? [] : .copy }

    override func performDragOperation(_ s: NSDraggingInfo) -> Bool {
        let list = urls(s)
        for u in list {
            NSDocumentController.shared.openDocument(withContentsOf: u, display: true) { _, _, error in
                if let error { CoreHost.present(error) }
            }
        }
        return !list.isEmpty
    }
}

@MainActor
final class GraphWindowController: NSWindowController, NSWindowDelegate, NSToolbarDelegate, NSToolbarItemValidation {
    let container = DropView(frame: NSRect(x: 0, y: 0, width: 1000, height: 820))
    var canvasView: CanvasView?
    var asks: AskPresenter?
    var watched: WatchedPanel?
    let split = NSSplitViewController()
    var watchedItem: NSSplitViewItem?
    let placeholder = NSTextField(labelWithString: "Opening…")
    let bannerLabel = NSTextField(wrappingLabelWithString: "")
    var bannerTask: Task<Void, Never>?
    var shownZoom = -1
    var appliedStart = false
    let engineLabel = NSTextField(labelWithString: "Engines: starting…")
    let engineDot = NSView()
    static var all: [ObjectIdentifier: GraphWindowController] = [:]

    /// The window showing `model`.
    static func of(_ model: DocumentModel) -> GraphWindowController? {
        all.values.first { $0.canvasView?.model === model }
    }

    init(document: GraphDocument) {
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1320, height: 820),
                         styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
        w.tabbingMode = .automatic  // tabs when the user prefers them (System Settings), else a window each
        w.minSize = NSSize(width: 560, height: 360)
        super.init(window: w)
        Self.all[ObjectIdentifier(self)] = self
        // the canvas, and the Watched panel as an inspector on the right
        let canvasVC = NSViewController()
        canvasVC.view = container
        let canvasItem = NSSplitViewItem(viewController: canvasVC)
        canvasItem.minimumThickness = 320
        split.addSplitViewItem(canvasItem)
        w.contentViewController = split
        w.setContentSize(NSSize(width: 1320, height: 820))
        placeholder.textColor = .secondaryLabelColor
        for x in [placeholder, bannerLabel] {
            x.translatesAutoresizingMaskIntoConstraints = false
            container.addSubview(x)
        }
        bannerLabel.isHidden = true
        bannerLabel.wantsLayer = true
        bannerLabel.drawsBackground = true
        bannerLabel.backgroundColor = .windowBackgroundColor.withAlphaComponent(0.92)
        bannerLabel.layer?.cornerRadius = 6
        bannerLabel.alignment = .center
        NSLayoutConstraint.activate([
            placeholder.centerXAnchor.constraint(equalTo: container.centerXAnchor),
            placeholder.centerYAnchor.constraint(equalTo: container.centerYAnchor),
            bannerLabel.centerXAnchor.constraint(equalTo: container.centerXAnchor),
            bannerLabel.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -16),
            bannerLabel.widthAnchor.constraint(lessThanOrEqualTo: container.widthAnchor, constant: -40),
        ])
        let tb = NSToolbar(identifier: "graph")
        tb.delegate = self
        tb.displayMode = .iconOnly
        w.toolbar = tb
        w.toolbarStyle = .unified
        w.delegate = self
        shouldCascadeWindows = true
        w.center()
        open(document)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    var graphDocument: GraphDocument? { document as? GraphDocument }

    /// Open the file (or an untitled graph) in the core, then put the canvas and panel in.
    func open(_ document: GraphDocument) {
        guard let core = CoreHost.core else { return }
        let path = document.fileURL?.path
        let opened = ContinuousClock.now
        Task { [weak self] in
            _ = await CoreHost.ready?.value
            guard let self else { return }
            do {
                let r = try await core.request("document.open", path.map { ["path": .string($0)] } ?? [:])
                guard let doc = r["doc"].uint64 else { throw PlumError(.decode, "document.open gave no doc") }
                document.doc = doc
                let model = DocumentModel(core: core, doc: doc)
                document.model = model
                attach(model, doc: doc, opened: opened)
            } catch {
                placeholder.stringValue = "Couldn't open: \(error)"
            }
        }
    }

    func attach(_ model: DocumentModel, doc: UInt64, opened: ContinuousClock.Instant) {
        let v = CanvasView(model: model, frame: container.bounds)
        v.autoresizingMask = [.width, .height]
        v.onError = { NSLog("PlumCraft: %@", $0) }
        placeholder.removeFromSuperview()
        container.addSubview(v, positioned: .below, relativeTo: bannerLabel)
        canvasView = v
        window?.makeFirstResponder(v)
        v.canvas.onViewport = { [weak self] vp in self?.showZoom(vp.zoom) }
        v.canvas.onDrawn = { [weak self] _ in self?.drawn(doc: doc, opened: opened) }
        v.onContextMenu = { [weak self] p, items in self?.popContext(items, at: p) }
        v.onCanvasKey = { [weak self] id in self?.canvasKey(id) }
        v.validate = { mi in MenuBar.shared.validateStandard(mi) }
        v.onCommands = { [weak self] ran in self?.ranByGesture(ran) }
        v.palettePopover.onShown = { [weak self] _ in self?.reportNow() }
        // the panel
        let panel = WatchedPanel(model: model, pictures: v.canvas.pictures)
        panel.onShown = { [weak self] in self?.report(doc: doc, ready: nil, opened: opened) }
        let item = NSSplitViewItem(inspectorWithViewController: panel)
        item.minimumThickness = 260
        item.maximumThickness = 640
        item.canCollapse = true
        split.addSplitViewItem(item)
        if CoreHost.options.hideWatched {
            // collapsed once it is in the split view
            DispatchQueue.main.async { item.isCollapsed = true }
        }
        watchedItem = item
        watched = panel
        // the asks
        let a = AskPresenter(model: model)
        a.window = window
        a.canvas = v
        a.say = { [weak self] s in self?.banner(s) }
        a.onSheet = { [weak self] ask in
            Status.shared.set("sheet", ["ask": .uint(ask.ask), "kind": .string(ask.kind.raw), "window": .int(Int64(self?.window?.windowNumber ?? 0))])
        }
        asks = a
        model.onAsks = { [weak a] list in a?.update(list) }
        model.onWatched = { [weak panel] w in panel?.show(w) }
        model.onState = { [weak self] st in self?.followState(st) }
        model.onMenus = { [weak self] _ in
            guard let self, let m = canvasView?.model else { return }
            MenuBar.shared.modelChanged(m)
            window?.toolbar?.validateVisibleItems()
        }
        model.onEngine = { [weak self] e in self?.showEngine(e) }
        model.start()
        report(doc: doc, ready: false, opened: opened)
        // the bar follows the key window, else the first document opened (a launch in the
        // background has no key window yet)
        if window?.isKeyWindow == true || MenuBar.shared.source == nil { MenuBar.shared.show(model) }
    }

    // MARK: The document's state

    func followState(_ st: DocumentState) {
        graphDocument?.follow(st)
        if let m = canvasView?.model { MenuBar.shared.modelChanged(m) }
        if let e = st.lastError, !e.isEmpty, e != lastErrorShown {
            lastErrorShown = e
            banner(e)
        }
        showEngine(canvasView?.model.engine ?? EngineState())
        reportNow()
    }

    var lastErrorShown: String?

    func windowDidBecomeKey(_ notification: Notification) {
        if let m = canvasView?.model { MenuBar.shared.show(m) }
    }

    func windowWillClose(_ notification: Notification) {
        Self.all[ObjectIdentifier(self)] = nil
        if MenuBar.shared.source === canvasView?.model { MenuBar.shared.show(nil) }
    }

    // MARK: Banner: what the app says without a dialog

    func banner(_ s: String) {
        bannerLabel.stringValue = "  \(s)  "
        bannerLabel.isHidden = false
        Status.shared.set("banner", .string(s))
        bannerTask?.cancel()
        bannerTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            if !Task.isCancelled { self?.bannerLabel.isHidden = true }
        }
    }

    // MARK: Context menus

    func popContext(_ items: [MenuItem], at p: CGPoint) {
        guard let v = canvasView, !items.isEmpty else { return }
        let menu = ContextMenu.make(items, model: v.model)
        contextMenu = menu
        Status.shared.set("contextMenu", .array(items.map { .string($0.label) }))
        menu.popUp(positioning: nil, at: p, in: v)
    }

    var contextMenu: ContextMenu?

    /// A key the canvas recognised, or a standard action it took: the core's command.
    func canvasKey(_ id: String) {
        guard let m = canvasView?.model else { return }
        var command = id
        if id == "escape" {
            command = (modelItem("run.cancel")?.enabled ?? false) ? "run.cancel" : "selection.clear"
        }
        Task {
            do {
                _ = try await m.invoke(command, person: true)
            } catch {
                banner("\(error)")
            }
        }
    }

    // MARK: Toolbar: Run, Run Selected, Cancel, the engines, the Watched panel

    static let runID = NSToolbarItem.Identifier("run")
    static let runSelectedID = NSToolbarItem.Identifier("runSelected")
    static let cancelID = NSToolbarItem.Identifier("cancel")
    static let engineID = NSToolbarItem.Identifier("engine")
    static let watchedID = NSToolbarItem.Identifier("watched")
    static let fitID = NSToolbarItem.Identifier("fit")
    static let addID = NSToolbarItem.Identifier("add")

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [Self.addID, Self.fitID, .space, Self.runID, Self.runSelectedID, Self.cancelID, .flexibleSpace, Self.engineID, .space, Self.watchedID]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item = NSToolbarItem(itemIdentifier: id)
        switch id {
        case Self.runID:
            item.label = "Run"
            item.toolTip = "Run the graph (⌘Return)"
            item.image = NSImage(systemSymbolName: "play.fill", accessibilityDescription: "Run")
            item.action = #selector(runGraph(_:))
        case Self.runSelectedID:
            item.label = "Run Selected"
            item.toolTip = "Run what the selected nodes need"
            item.image = NSImage(systemSymbolName: "play.square", accessibilityDescription: "Run Selected")
            item.action = #selector(runSelected(_:))
        case Self.cancelID:
            item.label = "Cancel"
            item.toolTip = "Cancel the run (⌘.)"
            item.image = NSImage(systemSymbolName: "stop.fill", accessibilityDescription: "Cancel Run")
            item.action = #selector(cancelRun(_:))
        case Self.engineID:
            let box = NSStackView(views: [engineDot, engineLabel])
            box.orientation = .horizontal
            box.spacing = 6
            engineDot.wantsLayer = true
            engineDot.layer?.cornerRadius = 4
            engineDot.layer?.backgroundColor = NSColor.systemOrange.cgColor
            engineDot.widthAnchor.constraint(equalToConstant: 8).isActive = true
            engineDot.heightAnchor.constraint(equalToConstant: 8).isActive = true
            engineLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            engineLabel.textColor = .secondaryLabelColor
            item.view = box
            item.label = "Engines"
            item.visibilityPriority = .low
        case Self.fitID:
            item.label = "Fit"
            item.toolTip = "Fit the view to all nodes (F)"
            item.image = NSImage(systemSymbolName: "arrow.up.left.and.down.right.magnifyingglass", accessibilityDescription: "Fit")
            item.action = #selector(fitView(_:))
        case Self.addID:
            item.label = "Add Node"
            item.toolTip = "Add a node from the palette (Tab, or double-click the canvas)"
            item.image = NSImage(systemSymbolName: "plus", accessibilityDescription: "Add Node")
            item.action = #selector(addNode(_:))
        case Self.watchedID:
            item.label = "Watched"
            item.toolTip = "Show or hide the Watched panel (⌥⌘I)"
            item.image = NSImage(systemSymbolName: "sidebar.right", accessibilityDescription: "Watched panel")
            item.action = #selector(toggleWatched(_:))
        default:
            return nil
        }
        item.target = self
        return item
    }

    /// The model's item for `command`, wherever it is in the bar (the kit's lookup).
    func modelItem(_ command: String) -> MenuItem? {
        canvasView?.model.menuBar.item(command: command)
    }

    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        switch item.itemIdentifier {
        case Self.runID:
            let it = modelItem("run.start")
            item.toolTip = it?.enabled == false ? (it?.reason ?? "Can't run now") : "Run the graph (⌘Return)"
            return it?.enabled ?? false
        case Self.runSelectedID:
            // the core's command and its reason (N4 slice 1)
            let it = modelItem("run.selected")
            item.toolTip = it?.enabled == false ? (it?.reason ?? "Can't run now") : "Run the selected nodes and what they need"
            return it?.enabled ?? false
        case Self.fitID, Self.addID:
            return canvasView != nil
        case Self.cancelID:
            let it = modelItem("run.cancel")
            item.toolTip = it?.enabled == false ? (it?.reason ?? "Nothing is running") : "Cancel the run (⌘.)"
            return it?.enabled ?? false
        case Self.watchedID:
            return canvasView != nil
        default:
            return true
        }
    }

    @objc func runGraph(_ sender: Any?) {
        guard let m = canvasView?.model, let it = modelItem("run.start") else { return }
        MenuBar.shared.dispatch(it, model: m)
    }

    /// Run Selected: the core's `run.selected` (N4 slice 1: everything is a command).
    @objc func runSelected(_ sender: Any?) {
        guard let m = canvasView?.model, let it = modelItem("run.selected") else { return }
        MenuBar.shared.dispatch(it, model: m)
    }

    /// Fit: the core's `canvas.fit` (the View menu's item, the F key).
    @objc func fitView(_ sender: Any?) {
        canvasView?.fitView(sender)
    }

    /// Add Node: the palette at the view's centre.
    @objc func addNode(_ sender: Any?) {
        guard let v = canvasView else { return }
        v.openPalette(at: CGPoint(x: v.bounds.midX, y: v.bounds.midY))
    }

    /// What a gesture ran: a refusal (a wire that doesn't fit) or a command with no UI yet is said.
    func ranByGesture(_ ran: [CommandRun]) {
        if let bad = ran.first(where: { !$0.ok }), let e = bad.error { return banner(e) }
        if let s = ShellCommands.notBuilt(ran: ran) { banner(s) }
    }

    @objc func cancelRun(_ sender: Any?) {
        guard let m = canvasView?.model, let it = modelItem("run.cancel") else { return }
        MenuBar.shared.dispatch(it, model: m)
    }

    @objc func toggleWatched(_ sender: Any?) {
        guard let item = watchedItem else { return }
        item.animator().isCollapsed.toggle()
    }

    func showEngine(_ e: EngineState) {
        let err = canvasView?.model.state.lastError
        let (text, color): (String, NSColor) =
            if !e.connected && e.ready { ("Engine down", .systemRed) }
            else if !e.connected { ("Engines starting…", .systemOrange) }
            else { ("Engines ready", .systemGreen) }
        engineLabel.stringValue = text
        engineDot.layer?.backgroundColor = color.cgColor
        engineLabel.toolTip = [e.label, (!e.connected && e.ready) ? err : nil].compactMap { $0 }.joined(separator: "\n")
        Status.shared.set("engine", ["connected": .bool(e.connected), "ready": .bool(e.ready), "label": .string(e.label), "shown": .string(text)])
        window?.toolbar?.validateVisibleItems()
    }

    // MARK: Title and status

    func showZoom(_ z: CGFloat) {
        let pct = Int((z * 100).rounded())
        if pct != shownZoom {
            shownZoom = pct
            window?.subtitle = "\(pct)%"
        }
    }

    var lastReport: (UInt64, ContinuousClock.Instant)?

    func drawn(doc: UInt64, opened: ContinuousClock.Instant) {
        guard let v = canvasView else { return }
        let ready = v.canvas.allMeasured && v.canvas.idle && v.model.applied > 0
        report(doc: doc, ready: ready, opened: opened)
        window?.toolbar?.validateVisibleItems()
        // the start options after the core's own fit (graph.open owes one once every face is measured)
        if ready && !appliedStart && (v.model.view.fit != nil || v.model.faces.count == 0) {
            appliedStart = true
            applyStartOptions(v)
        }
    }

    /// `--zoom`, `--center`, `--select`, then `--then`'s steps, once the first frame is there.
    func applyStartOptions(_ v: CanvasView) {
        let o = CoreHost.options
        if !o.select.isEmpty {
            Task { _ = try? await v.model.run("selection.set", ["nodes": .array(o.select.map { .string($0) })]) }
        }
        if let z = o.zoom {
            if let id = o.center, let nid = v.canvas.nid(of: id), let L = v.canvas.faceLayout(nid: nid), let f = v.model.faces[nid]?.item {
                let c = CGPoint(x: f.pos.x + L.size.width / 2, y: f.pos.y + L.size.height / 2)
                let size = v.canvas.viewSize
                v.canvas.setViewport(Viewport(pan: CGPoint(x: size.width / 2 - c.x * z, y: size.height / 2 - c.y * z), zoom: z))
            } else {
                v.canvas.zoom(to: z, animated: false)
            }
        }
        if !o.then.isEmpty && Script.claim() {
            Task { await Script(window: self).run(o.then) }
        }
    }

    var readyShown = false

    func report(doc: UInt64, ready: Bool?, opened: ContinuousClock.Instant) {
        if let ready { readyShown = ready }
        reportNow()
        if lastReport == nil { lastReport = (doc, opened) }
        _ = doc
    }

    func reportNow() {
        guard let w = window, let doc = graphDocument?.doc else { return }
        var s: [String: JSONValue] = ["doc": .uint(doc), "window": .int(Int64(w.windowNumber)), "ready": .bool(readyShown),
                                      "path": .string(document?.fileURL?.path ?? ""), "edited": .bool(document?.isDocumentEdited ?? false),
                                      "title": .string(w.title)]
        if let v = canvasView {
            s["faces"] = .int(Int64(v.canvas.faceCount))
            s["wires"] = .int(Int64(v.canvas.wireLayerCount))
            s["hidden"] = .int(Int64(v.canvas.hiddenCount))
            s["zoom"] = .double(Double(v.canvas.viewport.zoom))
            if let (_, opened) = lastReport, let f = v.canvas.firstFrameAt {
                let d = f - opened
                s["firstFrameMs"] = .double(Double(d.components.attoseconds) / 1e15 + Double(d.components.seconds) * 1000)
            }
            s["selection"] = .array(v.model.selection.nodes.map { .string($0) })
            let st = v.model.state
            s["undo"] = st.undo.map(JSONValue.string) ?? .null
            s["dirty"] = .bool(st.dirty)
            s["washes"] = .int(Int64(v.model.faces.items.filter { $0.shown != nil }.count))
            s["anim"] = .bool(v.model.anim.on)
            s["redo"] = st.redo.map(JSONValue.string) ?? .null
            s["nodes"] = .int(Int64(v.model.faces.count))
            s["wireCount"] = .int(Int64(v.model.wires.count))
            s["dragging"] = .bool(v.drag.active)
            let pal = v.palette
            s["palette"] = pal.isOpen
                ? ["open": true, "wire": .bool(pal.forWire), "items": .array(pal.items.prefix(40).map { .string($0.label) }),
                   "shown": .bool(v.palettePopover.isShown), "pending": pal.pendingText.map(JSONValue.string) ?? .null]
                : ["open": false, "shown": .bool(v.palettePopover.isShown)]
        }
        if let p = watched { s["watched"] = .object(p.describe) }
        Status.shared.window(doc, .object(s))
    }

    func windowDidChangeBackingProperties(_ notification: Notification) {
        canvasView?.canvas.settle()
    }
}

/// A context menu from the core's model: items dispatch by id as the bar's do.
@MainActor
final class ContextMenu: NSMenu, NSMenuItemValidation {
    var model: DocumentModel?
    var entries: [MenuItem] = []

    static func make(_ items: [MenuItem], model: DocumentModel) -> ContextMenu {
        let m = ContextMenu(title: "")
        m.model = model
        m.autoenablesItems = true
        m.fill(m, items)
        while m.items.last?.isSeparatorItem == true { m.removeItem(at: m.items.count - 1) }
        return m
    }

    private func fill(_ into: NSMenu, _ items: [MenuItem]) {
        for it in items {
            if it.separator {
                if !(into.items.last?.isSeparatorItem ?? true) { into.addItem(.separator()) }
                continue
            }
            let mi = NSMenuItem(title: it.label, action: it.command == nil || it.isSubmenu ? nil : #selector(choose(_:)), keyEquivalent: "")
            mi.target = self
            mi.tag = entries.count
            entries.append(it)
            mi.state = it.checked == true ? .on : .off
            mi.toolTip = it.enabled ? it.hover : (it.reason ?? it.hover)
            if it.isSubmenu {
                let sub = NSMenu(title: it.label)
                fill(sub, it.items)
                mi.submenu = sub
            }
            into.addItem(mi)
        }
    }

    func validateMenuItem(_ mi: NSMenuItem) -> Bool {
        guard mi.tag >= 0, mi.tag < entries.count else { return false }
        return entries[mi.tag].enabled
    }

    @objc func choose(_ sender: NSMenuItem) {
        guard sender.tag >= 0, sender.tag < entries.count, let model else { return }
        MenuBar.shared.dispatch(entries[sender.tag], model: model)
    }
}
