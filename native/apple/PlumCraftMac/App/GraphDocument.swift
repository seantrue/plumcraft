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
import UniformTypeIdentifiers

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
                          userInfo: [NSLocalizedFailureReasonErrorKey: "It isn't a Plummer graph (a JSON object with \"nodes\")."])
        }
    }

    override func makeWindowControllers() {
        addWindowController(GraphWindowController(document: self))
    }

    /// The core's dirty state, as the feed said it.
    func follow(_ st: DocumentState) {
        // the core's file for this document, when the core moved it (Open into an untouched
        // window, a script's graph.saveAs): the title, proxy icon and Save follow it
        if let p = st.path, !p.isEmpty, fileURL?.standardizedFileURL.path != URL(fileURLWithPath: p).standardizedFileURL.path {
            let url = URL(fileURLWithPath: p)
            fileURL = url
            fileType = Self.type(of: url)
            fileModificationDate = (try? FileManager.default.attributesOfItem(atPath: p)[.modificationDate]) as? Date
            NSDocumentController.shared.noteNewRecentDocumentURL(url)
            for wc in windowControllers { wc.synchronizeWindowTitleWithDocumentName() }
        }
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

    /// The document type of a graph's file at `url` (the Info.plist's, FR2): `.json` is the
    /// alternate `public.json`; anything else is a graph of Plummer's own type.
    static func type(of url: URL) -> String {
        url.pathExtension.lowercased() == "json" ? "public.json" : GraphFiles.typeIdentifier
    }

    /// The save panel, before it is shown: the folder and name the person check asked for
    /// (`panel-at:PATH`, the scripted copy only), so the panel (out of process) opens there.
    /// Setting them once it shows reached the app's own object but not the panel a person sees
    /// (the first run of the scripted copy saved into the panel's default folder).
    override func prepareSavePanel(_ panel: NSSavePanel) -> Bool {
        offerGraphType(panel)
        if let u = PersonSteps.takePanelAt() {
            panel.directoryURL = u.deletingLastPathComponent()
            panel.nameFieldStringValue = u.lastPathComponent
        }
        return super.prepareSavePanel(panel)
    }

    /// FR2 (docs/shell-interface.md 12.23): Save As of a `.json` graph offers `.plum` first. The
    /// panel's own format choice (`showsContentTypes`, which NSDocument turns on with more than
    /// one type it can write) is set to the graph's own type, as a person's choice sets it, and
    /// the name follows; `.json` stays on it. Save of the `.json` graph itself never shows a panel
    /// and keeps its name. What the panel offers is in the status (`savePanel`).
    private func offerGraphType(_ panel: NSSavePanel) {
        let graph = GraphFiles.typeIdentifier
        if fileType == "public.json", let t = panel.allowedContentTypes.first(where: { $0.identifier == graph }) {
            panel.currentContentType = t
            let stem = (panel.nameFieldStringValue as NSString).deletingPathExtension
            panel.nameFieldStringValue = "\(stem).\(GraphFiles.current.defaultExtension)"
        }
        Status.shared.set("savePanel", ["name": .string(panel.nameFieldStringValue), "type": .string(panel.currentContentType?.identifier ?? ""),
                                        "showsTypes": .bool(panel.showsContentTypes),
                                        "formats": .array(panel.allowedContentTypes.map { .string($0.identifier) })])
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
            .filter { GraphFiles.isGraph($0) }
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
    /// The pop-out windows and the parameters inspector (N4 slice 3), following the core's.
    var popouts: PopoutWindows?
    var parameters: ParametersPopover?
    var messagesWindow: MessagesWindow?
    var acknowledgementsWindow: AcknowledgementsWindow?
    /// The Cache Report, Parameters, value editors and Compare (N4 slice 6).
    var coreWindows: CoreWindows?
    /// The way back out of an entered group: Graph › outer › inner (the scene's focus path).
    let crumbs = NSPathControl()
    /// The path control's material, following the appearance as the toolbar's does.
    let crumbsBox = NSVisualEffectView()
    /// The toolbar's buttons as the core last gave them, by command.
    var toolItems: [String: MenuItem] = [:]
    let placeholder = NSTextField(labelWithString: "Opening…")
    let bannerLabel = NSTextField(wrappingLabelWithString: "")
    /// The status line (research note 28), the core's model; View › Status Line hides it.
    let statusBar = StatusBar(frame: .zero)
    var statusBarHeight: NSLayoutConstraint?
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
        // the canvas fills the window; a watched value opens in a window of its own (N4 slice 3:
        // the Watched column is gone)
        let canvasVC = NSViewController()
        canvasVC.view = container
        w.contentViewController = canvasVC
        w.setContentSize(NSSize(width: 1320, height: 820))
        placeholder.textColor = .secondaryLabelColor
        crumbs.pathStyle = .standard
        crumbs.backgroundColor = .clear
        crumbsBox.material = .headerView
        crumbsBox.blendingMode = .withinWindow
        crumbsBox.state = .active
        crumbsBox.wantsLayer = true
        crumbsBox.layer?.cornerRadius = 8
        crumbsBox.isHidden = true
        crumbs.translatesAutoresizingMaskIntoConstraints = false
        crumbsBox.addSubview(crumbs)
        crumbs.target = self
        crumbs.action = #selector(crumbClicked(_:))
        crumbs.setAccessibilityLabel("Groups entered")
        for x in [placeholder, bannerLabel, crumbsBox, statusBar] as [NSView] {
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
            // above the status line, never over it
            bannerLabel.bottomAnchor.constraint(equalTo: statusBar.topAnchor, constant: -12),
            statusBar.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            statusBar.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            statusBar.bottomAnchor.constraint(equalTo: container.bottomAnchor),
            bannerLabel.widthAnchor.constraint(lessThanOrEqualTo: container.widthAnchor, constant: -40),
            crumbs.topAnchor.constraint(equalTo: crumbsBox.topAnchor, constant: 3),
            crumbs.bottomAnchor.constraint(equalTo: crumbsBox.bottomAnchor, constant: -3),
            crumbs.leadingAnchor.constraint(equalTo: crumbsBox.leadingAnchor, constant: 6),
            crumbs.trailingAnchor.constraint(equalTo: crumbsBox.trailingAnchor, constant: -8),
            crumbsBox.topAnchor.constraint(equalTo: container.safeAreaLayoutGuide.topAnchor, constant: 8),
            crumbsBox.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 12),
            crumbsBox.widthAnchor.constraint(lessThanOrEqualTo: container.widthAnchor, constant: -24),
        ])
        let h = statusBar.heightAnchor.constraint(equalToConstant: 0)
        h.isActive = true
        statusBarHeight = h
        statusBar.isHidden = true
        statusBar.onCommand = { [weak self] c in self?.statusCommand(c) }
        statusBar.onFit = { [weak self] in if let self { Status.shared.set("statusBar", self.statusBar.report) } }
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
                // a document the core made (New or Open in a window, DocumentWindows) is shown as
                // it is; any other is opened in the core now
                let doc: UInt64
                if let made = document.doc {
                    doc = made
                } else {
                    let r = try await core.request("document.open", path.map { ["path": .string($0)] } ?? [:])
                    guard let d = r["doc"].uint64 else { throw PlumError(.decode, "document.open gave no doc") }
                    doc = d
                }
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
        // the canvas fills the window above the status line
        v.translatesAutoresizingMaskIntoConstraints = false
        v.onError = { NSLog("Plummer: %@", $0) }
        placeholder.removeFromSuperview()
        // under everything that floats over it: the banner, the crumbs, the status line
        container.addSubview(v, positioned: .below, relativeTo: bannerLabel)
        NSLayoutConstraint.activate([
            v.topAnchor.constraint(equalTo: container.topAnchor),
            v.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            v.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            v.bottomAnchor.constraint(equalTo: statusBar.topAnchor),
        ])
        canvasView = v
        window?.makeFirstResponder(v)
        v.canvas.onViewport = { [weak self] vp in
            self?.showZoom(vp.zoom)
            self?.parameters?.follow()
        }
        v.canvas.onDrawn = { [weak self] _ in
            self?.drawn(doc: doc, opened: opened)
            self?.parameters?.params.sceneChanged()
            self?.showCrumbs()
        }
        v.onContextMenu = { [weak self] p, items in self?.popContext(items, at: p) }
        v.onCanvasKey = { [weak self] id in self?.canvasKey(id) }
        v.validate = { mi in MenuBar.shared.validateStandard(mi) }
        v.onCommands = { [weak self] ran in self?.ranByGesture(ran) }
        v.palettePopover.onShown = { [weak self] _ in self?.reportNow() }
        // the core's windows: pop-outs and the parameters inspector
        let pops = PopoutWindows(model: model, pictures: v.canvas.pictures)
        pops.documentWindow = window
        pops.onChange = { [weak self] in self?.reportNow() }
        popouts = pops
        let params = ParametersPopover(model: model, canvasView: v)
        parameters = params
        let msgs = MessagesWindow(model: model)
        messagesWindow = msgs
        let acks = AcknowledgementsWindow(model: model)
        acks.onChange = { [weak self] in self?.reportNow() }
        acknowledgementsWindow = acks
        let cw = CoreWindows(model: model, pictures: v.canvas.pictures)
        coreWindows = cw
        model.onWindows = { [weak pops, weak params, weak model, weak self, weak msgs, weak acks, weak cw] w in
            pops?.follow(w)
            params?.params.follow(w)
            msgs?.follow(w.messages ?? false, over: self?.window)
            let wasOpen = acks?.window != nil
            acks?.follow(w.acknowledgements ?? false, over: self?.window)
            if wasOpen != (acks?.window != nil) { self?.reportNow() }
            let had = !(cw?.windows.isEmpty ?? true)
            cw?.follow(w, over: self?.window)
            if had || !CoreWindowKeys.of(w).isEmpty { self?.reportNow() }
            if let model { SettingsWindow.shared.follow(w.settings, model: model, over: self?.window) }
        }
        model.onToolbar = { [weak self] items in self?.followToolbar(items) }
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
        // a banner for each error the core says, the same one again too (B167): by its count
        if let e = st.lastError, !e.isEmpty, st.errorSeq != lastErrorShown {
            lastErrorShown = st.errorSeq
            banner(e)
        }
        showEngine(canvasView?.model.engine ?? EngineState())
        showStatusLine(st.statusLine)
        reportNow()
    }

    /// The status line as the core has it now: shown or hidden by View › Status Line.
    func showStatusLine(_ l: StatusLine) {
        let on = l.on && canvasView != nil
        statusBar.show(l)
        if statusBar.isHidden == on {
            statusBar.isHidden = !on
            statusBarHeight?.constant = on ? StatusBar.height : 0
        }
        Status.shared.set("statusBar", statusBar.report)
    }

    /// A status-line segment clicked: its command, as the person's.
    func statusCommand(_ c: StatusCommand) {
        guard let m = canvasView?.model else { return }
        Task {
            do {
                _ = try await m.invoke(c.id, params: c.params, person: true)
            } catch {
                banner("\(error)")
            }
        }
    }

    var lastErrorShown: UInt64 = 0

    func windowDidBecomeKey(_ notification: Notification) {
        if let m = canvasView?.model { MenuBar.shared.show(m) }
    }

    func windowWillClose(_ notification: Notification) {
        Self.all[ObjectIdentifier(self)] = nil
        popouts?.closeAll()
        coreWindows?.closeAll()
        if MenuBar.shared.source === canvasView?.model { MenuBar.shared.show(nil) }
    }

    // MARK: Banner: what the app says without a dialog

    func banner(_ s: String) {
        bannerLabel.stringValue = "  \(s)  "
        bannerLabel.isHidden = false
        Status.shared.set("banner", .string(s))
        // and every one said since launch (the last 30), so a check reads what was said, not
        // only the last (B87's step raced a later banner)
        Self.said = Array((Self.said + [s]).suffix(30))
        Status.shared.set("banners", .array(Self.said.map(JSONValue.string)))
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

    // MARK: Toolbar: the core's buttons (N4 slice 3), then the engines

    static let engineID = NSToolbarItem.Identifier("engine")
    /// The core's buttons the Mac shows: Select mode is a touch shell's (a drag on empty canvas is
    /// already a marquee here), so it is left out of this toolbar but stays in View.
    static let notOnMac: Set<String> = ["canvas.selectMode"]

    /// The identifiers, in the core's order, a separator a space.
    func coreIdentifiers() -> [NSToolbarItem.Identifier] {
        var out: [NSToolbarItem.Identifier] = []
        for it in canvasView?.model.toolbar ?? [] {
            if it.separator {
                if let last = out.last, last != .space { out.append(.space) }
            } else if let c = it.command, !Self.notOnMac.contains(c) {
                out.append(NSToolbarItem.Identifier(c))
            }
        }
        while out.last == .space { out.removeLast() }
        return out
    }

    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        coreIdentifiers() + [.flexibleSpace, Self.engineID]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    /// The core's toolbar changed: new buttons rebuild it; new states only revalidate it.
    func followToolbar(_ items: [MenuItem]) {
        let before = Set(toolItems.keys)
        toolItems = Dictionary(ToolbarWords.buttons(items).map { ($0.command ?? $0.id, $0) }, uniquingKeysWith: { a, _ in a })
        guard let w = window else { return }
        if Set(toolItems.keys) != before || w.toolbar?.items.count ?? 0 <= 1 {
            let tb = NSToolbar(identifier: "graph")
            tb.delegate = self
            tb.displayMode = .iconOnly
            w.toolbar = tb
        }
        for item in w.toolbar?.items ?? [] { style(item) }
        w.toolbar?.validateVisibleItems()
    }

    func style(_ item: NSToolbarItem) {
        guard let it = toolItems[item.itemIdentifier.rawValue] else { return }
        item.label = it.label
        item.paletteLabel = it.label
        item.toolTip = ToolbarWords.help(it)
        item.image = NSImage(systemSymbolName: ToolbarWords.symbol(it.icon), accessibilityDescription: it.label)
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item = NSToolbarItem(itemIdentifier: id)
        if id == Self.engineID {
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
            return item
        }
        guard toolItems[id.rawValue] != nil else { return nil }
        style(item)
        item.target = self
        item.action = #selector(toolbarButton(_:))
        return item
    }

    /// The model's item for `command`, wherever it is in the bar (the kit's lookup).
    func modelItem(_ command: String) -> MenuItem? {
        canvasView?.model.menuBar.item(command: command)
    }

    /// Enabled as the core says, with its reason as the tooltip.
    func validateToolbarItem(_ item: NSToolbarItem) -> Bool {
        guard let it = toolItems[item.itemIdentifier.rawValue] else { return item.itemIdentifier == Self.engineID }
        item.toolTip = ToolbarWords.help(it)
        return it.enabled && canvasView != nil
    }

    /// A toolbar button: the core's item, dispatched as a menu's (the palette at the view's
    /// centre for Add Node).
    @objc func toolbarButton(_ sender: NSToolbarItem) {
        guard let m = canvasView?.model, let it = toolItems[sender.itemIdentifier.rawValue] else { return }
        if it.command == "canvas.palette", let v = canvasView {
            v.openPalette(at: CGPoint(x: v.bounds.midX, y: v.bounds.midY))
            return
        }
        MenuBar.shared.dispatch(it, model: m)
    }

    // MARK: The way back out of a group

    func showCrumbs() {
        guard let m = canvasView?.model else { return }
        guard let f = m.focus else {
            if !crumbsBox.isHidden { crumbsBox.isHidden = true }
            return
        }
        var items: [NSPathControlItem] = []
        let root = NSPathControlItem()
        root.title = "Graph"
        root.image = NSImage(systemSymbolName: "square.grid.3x3", accessibilityDescription: "Graph")
        items.append(root)
        for c in f.path {
            let it = NSPathControlItem()
            it.title = c.name
            it.image = NSImage(systemSymbolName: "square.on.square", accessibilityDescription: "Group")
            items.append(it)
        }
        let titles = items.map(\.title)
        if crumbs.pathItems.map(\.title) != titles { crumbs.pathItems = items }
        crumbsBox.isHidden = false
    }

    /// A crumb: back out to the graph, or to that group (`canvas.focus`).
    @objc func crumbClicked(_ sender: NSPathControl) {
        guard let m = canvasView?.model, let f = m.focus, let clicked = sender.clickedPathItem,
              let k = sender.pathItems.firstIndex(where: { $0 === clicked }) else { return }
        let target: JSONValue = k == 0 ? .null : (k - 1 < f.path.count ? .string(f.path[k - 1].id) : .null)
        Task { _ = try? await m.run("canvas.focus", ["group": target]) }
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

    /// What the banners said, every window's, oldest first.
    static var said: [String] = []

    /// Whether this window has read the engine manifests' problems from the core yet.
    var manifestProblemsRead = false

    /// B87: the engine manifests the core couldn't use are said in the banner once the window
    /// has its model, read from the engine's state (`ui.inspect`'s `engine.manifestProblems`),
    /// which keeps them. The core also puts them in its status line, but only until a command
    /// clears it; a window that came up while the machine was busy could miss that (the person
    /// check saw it once, the banner never set).
    func sayManifestProblems() {
        guard !manifestProblemsRead, let m = canvasView?.model else { return }
        manifestProblemsRead = true
        Task { [weak self] in
            guard let s = try? await m.inspect() else { return }
            let probs = (s["engine"]["manifestProblems"].array ?? []).compactMap(\.string)
            guard let first = probs.first, let self else { return }
            let said = probs.count == 1 ? "engine manifests: \(first)" : "engine manifests: \(probs.count) problems; the first: \(first)"
            // once a window (manifestProblemsRead), whatever the status line said meanwhile
            self.banner(said)
        }
    }

    func showEngine(_ e: EngineState) {
        sayManifestProblems()
        let err = canvasView?.model.state.lastError
        let (text, color): (String, NSColor) =
            if !e.connected && e.ready { ("Engine down", .systemRed) }
            else if !e.connected { ("Engines starting…", .systemOrange) }
            else { ("Engines ready", .systemGreen) }
        // an engine install under way (B71): said here while it runs, what it is doing on hover
        // (the core's words; Engine › Cancel Install stops it)
        let installing = canvasView?.model.state.installing
        // decision 17: an engine in use whose manifest warned is marked here as in its menu: the
        // warning symbol before the words, the warning first in the tooltip
        if installing == nil, let w = e.warning {
            let s = NSMutableAttributedString()
            if let img = NSImage(systemSymbolName: "exclamationmark.triangle", accessibilityDescription: w) {
                let a = NSTextAttachment()
                a.image = img
                s.append(NSAttributedString(attachment: a))
                s.append(NSAttributedString(string: " "))
            }
            s.append(NSAttributedString(string: text))
            s.addAttributes([.font: engineLabel.font ?? NSFont.systemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.systemOrange],
                            range: NSRange(location: 0, length: s.length))
            engineLabel.attributedStringValue = s
            engineLabel.setAccessibilityHelp(w)
        } else {
            engineLabel.stringValue = installing == nil ? text : "Installing an engine…"
            engineLabel.textColor = .labelColor
            engineLabel.setAccessibilityHelp(nil)
        }
        engineDot.layer?.backgroundColor = (installing == nil ? color : .systemOrange).cgColor
        engineLabel.toolTip = [e.warning, installing?.status ?? installing?.repo, e.label, (!e.connected && e.ready) ? err : nil].compactMap { $0 }.joined(separator: "\n")
        Status.shared.set("engine", ["connected": .bool(e.connected), "ready": .bool(e.ready), "label": .string(e.label), "shown": .string(engineLabel.stringValue),
                                     "installing": installing.map { .string($0.status ?? $0.repo) } ?? .null,
                                     "warning": e.warning.map(JSONValue.string) ?? .null, "tooltip": .string(engineLabel.toolTip ?? "")])
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
        if let p = popouts { s["popouts"] = p.describe }
        if let p = parameters { s["parameters"] = p.describe }
        if let m = messagesWindow { s["messages"] = m.describe }
        if let a = acknowledgementsWindow { s["acknowledgements"] = a.describe }
        if let c = coreWindows { s["coreWindows"] = c.describe }
        if let n = canvasView?.nameField.field { s["nameField"] = ["text": .string(n.stringValue), "window": .int(Int64(n.window?.windowNumber ?? 0))] }
        s["toolbar"] = .array(coreIdentifiers().map { id in
            let it = toolItems[id.rawValue]
            return ["id": .string(id.rawValue), "label": .string(it?.label ?? ""), "enabled": .bool(it?.enabled ?? false), "checked": it?.checked.map(JSONValue.bool) ?? .null]
        })
        s["crumbs"] = .array(crumbsBox.isHidden ? [] : crumbs.pathItems.map { .string($0.title) })
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
            if it.isSection {
                // B80: a section's items go inline, under its heading
                if !it.label.isEmpty { into.addItem(NSMenuItem.sectionHeader(title: it.label)) }
                fill(into, it.items)
                continue
            }
            let mi = NSMenuItem(title: it.label, action: it.command == nil || it.isSubmenu ? nil : #selector(choose(_:)), keyEquivalent: "")
            mi.target = self
            mi.tag = entries.count
            entries.append(it)
            mi.state = it.checked == true ? .on : .off
            mi.toolTip = it.enabled ? (it.warning ?? it.hover) : (it.reason ?? it.hover)
            MenuBar.markWarning(it, mi)
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
