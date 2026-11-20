// One open graph on the iPad (N3 slice 5): the layer canvas, a navigation bar with the run's
// controls and the engines' state, the Watched panel (an inspector column in regular width, a
// sheet with detents in compact width), the asks, context menus and the canvas's keys.
//
// Saving: explicit, as the core has it today. Plan 6 and decision 7 give iOS autosave with no Save
// command; the contract puts it in the core (`document.open {autosave: true}`, 4.4), which isn't
// built, and a shell-side autosave would write the file behind the core's dirty state. So the
// core's Save (File › Save, Command-S, the title's menu) saves, the title says "Edited" while the
// core's document is dirty, and closing an edited graph asks Save, Don't Save or Cancel. That is a
// decision for the maintainer (note 24 section 18).

import os
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import UIKit

@MainActor
final class GraphViewController: UIViewController, UIAdaptivePresentationControllerDelegate {
    static var all: [GraphViewController] { live.allObjects }
    private static let live = NSHashTable<GraphViewController>.weakObjects()

    let document: GraphDocument
    /// The browser the graph came from, which gets the window back when it closes.
    weak var browser: BrowserViewController?
    let scoped: Bool
    var restore: Viewport?
    var then: [String]
    var doc: UInt64?
    var model: DocumentModel?
    var canvasView: CanvasView?
    var asks: AskPresenter?
    var watched: WatchedPanel?
    let canvasHost = UIView()
    let panelHost = UIView()
    var panelWidth: NSLayoutConstraint?
    var panelShown = true
    let placeholder = UILabel()
    let banner = UILabel()
    var bannerTask: Task<Void, Never>?
    let engineDot = UIView()
    let engineLabel = UILabel()
    var runItem: UIBarButtonItem?
    var runSelectedItem: UIBarButtonItem?
    var cancelItem: UIBarButtonItem?
    var watchedItem: UIBarButtonItem?
    var selectItem: UIBarButtonItem?
    var paletteView: PaletteController?
    var opened = ContinuousClock.now
    var readyShown = false
    var appliedStart = false
    var lastErrorShown: String?
    var keyPress: UInt64 = 0
    var lastArrow: ContinuousClock.Instant?
    var closing = false

    init(document: GraphDocument, scoped: Bool, restore: Viewport?, then: [String]) {
        self.document = document
        self.scoped = scoped
        self.restore = restore
        self.then = then
        super.init(nibName: nil, bundle: nil)
        Self.live.add(self)
        panelShown = !CoreHost.options.hideWatched
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    var name: String { document.fileURL.deletingPathExtension().lastPathComponent }

    // MARK: The view

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        title = name
        for x in [canvasHost, panelHost] {
            x.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(x)
        }
        canvasHost.clipsToBounds = true
        panelHost.clipsToBounds = true
        let sep = UIView()
        sep.backgroundColor = .separator
        sep.translatesAutoresizingMaskIntoConstraints = false
        panelHost.addSubview(sep)
        let w = panelHost.widthAnchor.constraint(equalToConstant: 0)
        panelWidth = w
        // the canvas under the bar's safe area, edge to edge below and at the sides
        NSLayoutConstraint.activate([
            canvasHost.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            canvasHost.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            canvasHost.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            canvasHost.trailingAnchor.constraint(equalTo: panelHost.leadingAnchor),
            panelHost.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            panelHost.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            panelHost.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            w,
            sep.leadingAnchor.constraint(equalTo: panelHost.leadingAnchor),
            sep.topAnchor.constraint(equalTo: panelHost.topAnchor),
            sep.bottomAnchor.constraint(equalTo: panelHost.bottomAnchor),
            sep.widthAnchor.constraint(equalToConstant: 1 / max(traitCollection.displayScale, 1)),
        ])
        placeholder.text = "Opening…"
        placeholder.textColor = .secondaryLabel
        placeholder.font = .preferredFont(forTextStyle: .body)
        placeholder.adjustsFontForContentSizeCategory = true
        banner.numberOfLines = 0
        banner.textAlignment = .center
        banner.font = .preferredFont(forTextStyle: .callout)
        banner.adjustsFontForContentSizeCategory = true
        banner.backgroundColor = .secondarySystemBackground.withAlphaComponent(0.94)
        banner.layer.cornerRadius = 10
        banner.layer.masksToBounds = true
        banner.isHidden = true
        for x in [placeholder, banner] {
            x.translatesAutoresizingMaskIntoConstraints = false
            canvasHost.addSubview(x)
        }
        NSLayoutConstraint.activate([
            placeholder.centerXAnchor.constraint(equalTo: canvasHost.centerXAnchor),
            placeholder.centerYAnchor.constraint(equalTo: canvasHost.centerYAnchor),
            banner.centerXAnchor.constraint(equalTo: canvasHost.centerXAnchor),
            banner.bottomAnchor.constraint(equalTo: canvasHost.safeAreaLayoutGuide.bottomAnchor, constant: -16),
            banner.widthAnchor.constraint(lessThanOrEqualTo: canvasHost.widthAnchor, constant: -40),
        ])
        makeBar()
        registerForTraitChanges([UITraitHorizontalSizeClass.self]) { (g: GraphViewController, _: UITraitCollection) in g.placePanel(animated: false) }
        openInCore()
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        becomeFirstResponder()
        MenuState.shared.show(model)
        StatusFile.shared.set("graphShown", true)
    }

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        StatusFile.shared.set("graphShown", false)
    }

    override var canBecomeFirstResponder: Bool { true }

    /// The system's three-finger editing gestures (a tap's shortcut bar, swipes for undo and redo,
    /// pinches for copy and paste) are off on the graph: three fingers tapped are Redo here, and
    /// two Undo (the iPad spike's rule, note 24 section 17). ⌘Z and the Edit menu still undo.
    override var editingInteractionConfiguration: UIEditingInteractionConfiguration { .none }

    /// Run, Run Selected, Cancel; the engines' state; the Watched panel; and the document's menu
    /// on its title (Save, Share).
    func makeBar() {
        let done = UIBarButtonItem(title: "Documents", image: UIImage(systemName: "chevron.backward"),
                                   primaryAction: UIAction { [weak self] _ in self?.close {} })
        done.accessibilityLabel = "Documents"
        let fit = UIBarButtonItem(title: "Fit", image: UIImage(systemName: "arrow.up.left.and.down.right.magnifyingglass"),
                                  primaryAction: UIAction { [weak self] _ in self?.canvasView?.fitView() })
        fit.accessibilityLabel = "Fit"
        // Add Node (plan 5.5's + button): the palette at the view's centre
        let add = UIBarButtonItem(title: "Add Node", image: UIImage(systemName: "plus"),
                                  primaryAction: UIAction { [weak self] _ in self?.canvasView?.openPalette() })
        add.accessibilityLabel = "Add Node"
        // Select mode: a one-finger drag on empty canvas draws a marquee instead of panning
        let select = UIBarButtonItem(title: "Select", image: UIImage(systemName: "rectangle.dashed"),
                                     primaryAction: UIAction { [weak self] _ in self?.toggleSelectMode() })
        select.accessibilityLabel = "Select"
        selectItem = select
        navigationItem.leftBarButtonItems = [done, fit, add, select]
        let run = UIBarButtonItem(title: "Run", image: UIImage(systemName: "play.fill"), primaryAction: UIAction { [weak self] _ in self?.runGraph() })
        let sel = UIBarButtonItem(title: "Run Selected", image: UIImage(systemName: "play.square"), primaryAction: UIAction { [weak self] _ in self?.runSelected() })
        let cancel = UIBarButtonItem(title: "Cancel Run", image: UIImage(systemName: "stop.fill"), primaryAction: UIAction { [weak self] _ in self?.cancelRun() })
        let panel = UIBarButtonItem(title: "Watched", image: UIImage(systemName: "sidebar.right"), primaryAction: UIAction { [weak self] _ in self?.toggleWatched(nil) })
        for (b, t) in [(run, "Run"), (sel, "Run Selected"), (cancel, "Cancel Run"), (panel, "Watched")] { b.accessibilityLabel = t }
        // the engines: a dot and a word, the label and any error as its menu
        engineDot.backgroundColor = .systemOrange
        engineDot.layer.cornerRadius = 4
        engineDot.translatesAutoresizingMaskIntoConstraints = false
        engineDot.widthAnchor.constraint(equalToConstant: 8).isActive = true
        engineDot.heightAnchor.constraint(equalToConstant: 8).isActive = true
        engineLabel.font = .preferredFont(forTextStyle: .footnote)
        engineLabel.adjustsFontForContentSizeCategory = true
        engineLabel.textColor = .secondaryLabel
        engineLabel.text = "Engines starting…"
        let box = UIStackView(arrangedSubviews: [engineDot, engineLabel])
        box.spacing = 6
        box.alignment = .center
        box.isAccessibilityElement = true
        box.accessibilityLabel = "Engines starting"
        let engine = UIBarButtonItem(customView: box)
        navigationItem.rightBarButtonItems = [panel, engine, cancel, sel, run]
        runItem = run
        runSelectedItem = sel
        cancelItem = cancel
        watchedItem = panel
        navigationItem.style = .editor
        navigationItem.titleMenuProvider = { [weak self] suggested in
            guard let self else { return UIMenu(children: suggested) }
            let save = UIAction(title: "Save", image: UIImage(systemName: "square.and.arrow.down"),
                                attributes: self.model?.state.dirty == true ? [] : .disabled) { [weak self] _ in self?.save() }
            let share = UIAction(title: "Share…", image: UIImage(systemName: "square.and.arrow.up")) { [weak self] _ in self?.shareFile() }
            return UIMenu(children: [save, share] + suggested)
        }
        validateBar()
    }

    // MARK: Opening in the core

    func openInCore() {
        guard let core = CoreHost.core else {
            placeholder.text = CoreHost.openError.map { "The core didn't start: \($0)" } ?? "The core didn't start."
            return
        }
        let path = document.fileURL.path
        opened = ContinuousClock.now
        Task { [weak self] in
            _ = await CoreHost.ready?.value
            guard let self else { return }
            do {
                let r = try await core.request("document.open", ["path": .string(path)])
                guard let doc = r["doc"].uint64 else { throw PlumError(.decode, "document.open gave no doc") }
                self.doc = doc
                attach(DocumentModel(core: core, doc: doc), doc: doc)
            } catch {
                placeholder.text = "Couldn't open: \(error)"
                CoreHost.say("error", "couldn't open \(path): \(error)")
            }
        }
    }

    func attach(_ model: DocumentModel, doc: UInt64) {
        self.model = model
        let v = CanvasView(model: model, frame: canvasHost.bounds)
        v.translatesAutoresizingMaskIntoConstraints = false
        placeholder.removeFromSuperview()
        canvasHost.insertSubview(v, belowSubview: banner)
        NSLayoutConstraint.activate([
            v.topAnchor.constraint(equalTo: canvasHost.topAnchor),
            v.leadingAnchor.constraint(equalTo: canvasHost.leadingAnchor),
            v.trailingAnchor.constraint(equalTo: canvasHost.trailingAnchor),
            v.bottomAnchor.constraint(equalTo: canvasHost.bottomAnchor),
        ])
        canvasView = v
        v.onError = { CoreHost.say("warn", $0) }
        v.canvas.onViewport = { [weak self] vp in
            self?.showZoom(vp.zoom)
            self?.viewportChanged()
        }
        v.canvas.onDrawn = { [weak self] _ in self?.drawn() }
        v.onGesture = { g in StatusFile.shared.set("gesture", .string(g)) }
        v.onCommand = { [weak self] id in self?.invoke(id) }
        v.onCommands = { [weak self] ran in self?.ranByGesture(ran) }
        let pc = PaletteController(palette: v.palette)
        pc.onShown = { [weak self] _ in self?.reportNow() }
        paletteView = pc
        v.presentPalette = { [weak self] p, focus in
            guard let self, let v = canvasView, let pc = paletteView else { return }
            // a sheet the graph shows (the Watched panel's in compact width) gives way first
            if let other = presentedViewController, other !== pc {
                other.dismiss(animated: false) { pc.present(from: self, source: v, at: p, focus: focus) }
            } else {
                pc.present(from: self, source: v, at: p, focus: focus)
            }
            reportNow()
        }
        v.dismissPalette = { [weak self] in
            self?.paletteView?.close()
            self?.reportNow()
        }
        v.contextMenuItems = { [weak self, weak model] items in
            guard let self, let model else { return [] }
            StatusFile.shared.set("contextMenu", .array(items.map { .string($0.label) }))
            return PadMenus.actions(items, model: model) { [weak self] it in self?.dispatch(it) }
        }
        // the panel
        let panel = WatchedPanel(model: model, pictures: v.canvas.pictures)
        panel.onShown = { [weak self] in self?.reportNow() }
        watched = panel
        placePanel(animated: false)
        // the asks
        let a = AskPresenter(model: model)
        a.host = self
        a.canvas = v
        a.say = { [weak self] s in self?.say(s) }
        a.onSheet = { ask in StatusFile.shared.set("sheet", ["ask": .uint(ask.ask), "kind": .string(ask.kind.raw)]) }
        a.saveFolder = { [weak self] in self?.saveFolder() ?? CoreHost.dirs.documents }
        asks = a
        model.onAsks = { [weak a] list in a?.update(list) }
        model.onWatched = { [weak panel, weak self] w in
            panel?.show(w)
            self?.reportNow()
        }
        model.onState = { [weak self] st in self?.followState(st) }
        model.onMenus = { [weak self, weak model] _ in
            if let model { MenuState.shared.modelChanged(model) }
            self?.validateBar()
        }
        model.onEngine = { [weak self] e in self?.showEngine(e) }
        model.start()
        MenuState.shared.show(model)
        reportNow()
    }

    // MARK: The Watched panel: a column in regular width, a sheet in compact

    var compact: Bool { traitCollection.horizontalSizeClass == .compact }

    func placePanel(animated: Bool) {
        guard let panel = watched else { return }
        if compact {
            // out of the column; a sheet when shown
            if panel.parent === self {
                panel.willMove(toParent: nil)
                panel.view.removeFromSuperview()
                panel.removeFromParent()
            }
            panelWidth?.constant = 0
            if panelShown && panel.presentingViewController == nil && view.window != nil { presentPanelSheet(panel) }
        } else {
            if panel.presentingViewController != nil {
                // out of its sheet first: a view controller still presented can't be a child
                // (adding it at once threw, N3 slice 5)
                panel.dismiss(animated: false) { [weak self] in self?.placePanel(animated: animated) }
                return
            }
            if panel.parent == nil {
                addChild(panel)
                panel.view.translatesAutoresizingMaskIntoConstraints = false
                panelHost.addSubview(panel.view)
                NSLayoutConstraint.activate([
                    panel.view.topAnchor.constraint(equalTo: panelHost.topAnchor),
                    panel.view.bottomAnchor.constraint(equalTo: panelHost.bottomAnchor),
                    panel.view.leadingAnchor.constraint(equalTo: panelHost.leadingAnchor, constant: 1),
                    panel.view.trailingAnchor.constraint(equalTo: panelHost.trailingAnchor),
                ])
                panel.didMove(toParent: self)
            }
            panelWidth?.constant = panelShown ? min(360, max(280, view.bounds.width * 0.3)) : 0
        }
        if animated {
            UIView.animate(withDuration: 0.25) { self.view.layoutIfNeeded() }
        }
        watchedItem?.isSelected = panelShown
        if let m = model { showEngine(m.engine) }
        reportNow()
    }

    func presentPanelSheet(_ panel: WatchedPanel) {
        panel.modalPresentationStyle = .pageSheet
        if let s = panel.sheetPresentationController {
            s.detents = [.medium(), .large()]
            s.largestUndimmedDetentIdentifier = .medium
            s.prefersGrabberVisible = true
            s.prefersScrollingExpandsWhenScrolledToEdge = false
        }
        panel.presentationController?.delegate = self
        present(panel, animated: true)
    }

    /// The person swiped the sheet down: the panel is hidden.
    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        panelShown = false
        watchedItem?.isSelected = false
        reportNow()
    }

    @objc func toggleWatched(_ sender: Any?) {
        panelShown.toggle()
        if compact, let panel = watched, !panelShown, panel.presentingViewController != nil {
            panel.dismiss(animated: true)
        }
        placePanel(animated: true)
    }

    var laidOutSize = CGSize.zero

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        if view.bounds.size != laidOutSize {
            laidOutSize = view.bounds.size
            reportNow()
        }
    }

    override func viewWillTransition(to size: CGSize, with coordinator: any UIViewControllerTransitionCoordinator) {
        super.viewWillTransition(to: size, with: coordinator)
        coordinator.animate(alongsideTransition: nil) { [weak self] _ in self?.placePanel(animated: false) }
    }

    // MARK: Running

    func runGraph() {
        guard let m = model, let it = m.menuBar.item(command: "run.start") else { return }
        dispatch(it)
        _ = m
    }

    /// Run Selected: the core's `run.selected` (N4 slice 1: everything is a command), enabled and
    /// explained by the core's menu item.
    func runSelected() {
        guard let m = model, let it = m.menuBar.item(command: "run.selected") else { return }
        dispatch(it)
    }

    /// Select mode on or off (the toolbar's toggle).
    func toggleSelectMode() {
        guard let v = canvasView else { return }
        v.selectMode.toggle()
        selectItem?.isSelected = v.selectMode
        StatusFile.shared.set("selectMode", .bool(v.selectMode))
        reportNow()
    }

    /// What a gesture ran: a refusal (a wire that doesn't fit) or a command with no UI yet is said.
    func ranByGesture(_ ran: [CommandRun]) {
        if let bad = ran.first(where: { !$0.ok }), let e = bad.error { return say(e) }
        if let s = ShellCommands.notBuilt(ran: ran) { say(s) }
    }

    func cancelRun() {
        guard let m = model, let it = m.menuBar.item(command: "run.cancel") else { return }
        dispatch(it)
    }

    /// Run a model item for the person at the iPad (the menu bar, a context menu, the bar).
    func dispatch(_ it: MenuItem) {
        guard let command = it.command, let model else { return }
        if let s = ShellCommands.notBuilt(it) { return say(s) }
        switch command {
        case "canvas.palette":
            // Node › Search…: the palette at the view's centre
            canvasView?.openPalette()
            return
        case "graph.open", "graph.new":
            // the document browser opens and creates graphs here
            close {}
            return
        case "app.quit":
            return
        default:
            break
        }
        Task {
            do {
                _ = try await model.activate(it, person: true)
            } catch {
                say("\(it.label): \(error)")
            }
        }
    }

    func validateBar() {
        let m = model
        let run = m?.menuBar.item(command: "run.start")
        let cancel = m?.menuBar.item(command: "run.cancel")
        runItem?.isEnabled = run?.enabled ?? false
        runSelectedItem?.isEnabled = m?.menuBar.item(command: "run.selected")?.enabled ?? false
        cancelItem?.isEnabled = cancel?.enabled ?? false
        watchedItem?.isEnabled = m != nil
    }

    func showEngine(_ e: EngineState) {
        let err = model?.state.lastError
        let (text, color): (String, UIColor) =
            if !e.connected && e.ready { ("Engine down", .systemRed) }
            else if !e.connected { ("Engines starting…", .systemOrange) }
            else { ("Engines ready", .systemGreen) }
        engineLabel.text = compact ? nil : text
        engineDot.backgroundColor = color
        engineLabel.superview?.accessibilityLabel = [text, e.label, (!e.connected && e.ready) ? err : nil].compactMap { $0 }.joined(separator: ". ")
        StatusFile.shared.set("engine", ["connected": .bool(e.connected), "ready": .bool(e.ready), "label": .string(e.label), "shown": .string(text)])
        validateBar()
    }

    // MARK: The document's state, saving, closing

    func followState(_ st: DocumentState) {
        if let m = model { MenuState.shared.modelChanged(m) }
        if let e = st.lastError, !e.isEmpty, e != lastErrorShown {
            lastErrorShown = e
            say(e)
        }
        showTitle()
        showEngine(model?.engine ?? EngineState())
        reportNow()
    }

    func showTitle() {
        let edited = model?.state.dirty == true
        title = name
        let zoom = canvasView.map { "\(Int(($0.canvas.viewport.zoom * 100).rounded()))%" } ?? ""
        if #available(iOS 26.0, *) {
            navigationItem.subtitle = [edited ? "Edited" : nil, zoom.isEmpty ? nil : zoom].compactMap { $0 }.joined(separator: " · ")
        } else {
            title = edited ? "\(name) — Edited" : name
        }
    }

    var shownZoom = -1
    var viewportReport: Task<Void, Never>?

    /// The view moved: say so to scripts at most every 100 ms (UI tests read it).
    func viewportChanged() {
        guard viewportReport == nil, StatusFile.shared.path != nil else { return }
        viewportReport = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(100))
            self?.viewportReport = nil
            self?.reportNow()
        }
    }

    func showZoom(_ z: CGFloat) {
        let pct = Int((z * 100).rounded())
        if pct != shownZoom {
            shownZoom = pct
            showTitle()
        }
    }

    /// The core's Save (it writes the file atomically; its dirty state clears).
    func save() {
        guard let m = model, let it = m.menuBar.item(command: "graph.save") else { return }
        dispatch(it)
    }

    /// Where a save into a new file goes: the graph's own folder when it is writable here.
    func saveFolder() -> URL {
        let dir = document.fileURL.deletingLastPathComponent()
        return FileManager.default.isWritableFile(atPath: dir.path) ? dir : CoreHost.dirs.documents
    }

    /// The graph's file to the share sheet.
    func shareFile() {
        let sheet = UIActivityViewController(activityItems: [document.fileURL], applicationActivities: nil)
        sheet.popoverPresentationController?.sourceItem = navigationItem.titleView ?? watchedItem
        present(sheet, animated: true)
    }

    /// Close the graph and go back to the browser: an edited graph asks first.
    func close(_ then: @escaping () -> Void) {
        guard !closing else { return }
        if model?.state.dirty == true && !CoreHost.options.scratchState {
            let a = UIAlertController(title: "Save changes to \(name)?", message: "Your changes will be lost if you don't save them.", preferredStyle: .alert)
            a.addAction(UIAlertAction(title: "Save", style: .default) { [weak self] _ in
                guard let self, let m = self.model else { return }
                Task {
                    _ = try? await m.run("graph.save", ["path": .string(self.document.fileURL.path)])
                    self.finishClose(then)
                }
            })
            a.addAction(UIAlertAction(title: "Don't Save", style: .destructive) { [weak self] _ in self?.finishClose(then) })
            a.addAction(UIAlertAction(title: "Cancel", style: .cancel))
            present(a, animated: true)
            return
        }
        finishClose(then)
    }

    private func finishClose(_ then: @escaping () -> Void) {
        closing = true
        if let doc, let core = CoreHost.core {
            core.post("document.close", ["discard": true], doc: doc)
            StatusFile.shared.removeWindow(doc)
        }
        model?.stop()
        asks?.stop()
        nonisolated(unsafe) let d = document
        let scoped = scoped
        d.close { _ in
            if scoped { d.fileURL.stopAccessingSecurityScopedResource() }
        }
        MenuState.shared.show(nil)
        let b = browser
        b?.graph = nil
        // any sheet of the graph's first, then the browser takes the window back
        let w = view.window
        let back = {
            if let b { b.show(root: b, in: w) }
            then()
        }
        if presentedViewController != nil { dismiss(animated: false, completion: back) } else { back() }
    }

    /// The scene's restoration: this graph (a bookmark) and the view.
    func restorationActivity() -> NSUserActivity? {
        guard let data = try? document.fileURL.bookmarkData() else { return nil }
        let a = NSUserActivity(activityType: SceneDelegate.activityType)
        var info: [String: Any] = ["bookmark": data]
        if let v = canvasView?.canvas.viewport {
            info["zoom"] = Double(v.zoom)
            info["panX"] = Double(v.pan.x)
            info["panY"] = Double(v.pan.y)
        }
        a.addUserInfoEntries(from: info)
        return a
    }

    // MARK: Banner: what the app says without a dialog

    func say(_ s: String) {
        banner.text = "  \(s)  "
        banner.isHidden = false
        StatusFile.shared.set("banner", .string(s))
        CoreHost.say("info", s)
        bannerTask?.cancel()
        bannerTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(5))
            if !Task.isCancelled { self?.banner.isHidden = true }
        }
    }

    // MARK: Drawn and ready

    func drawn() {
        guard let v = canvasView else { return }
        let ready = v.canvas.allMeasured && v.canvas.idle && v.model.applied > 0
        if ready != readyShown {
            readyShown = ready
        }
        reportNow()
        validateBar()
        if ready && !appliedStart && (v.model.view.fit != nil || v.model.faces.count == 0) {
            appliedStart = true
            if let r = restore {
                // after the core's own fit, which a just-opened graph owes
                DispatchQueue.main.async { v.canvas.setViewport(r) }
            }
            if !then.isEmpty && Script.claim() {
                let steps = then
                Task { await Script(graph: self).run(steps) }
            }
        }
    }

    func reportNow() {
        guard let doc else { return }
        var s: [String: JSONValue] = ["doc": .uint(doc), "ready": .bool(readyShown), "path": .string(document.fileURL.path), "title": .string(name),
                                      "compact": .bool(compact), "panel": .bool(panelShown),
                                      "size": [.double(Double(view.bounds.width)), .double(Double(view.bounds.height))]]
        if let v = canvasView {
            s["faces"] = .int(Int64(v.canvas.faceCount))
            s["wires"] = .int(Int64(v.canvas.wireLayerCount))
            s["hidden"] = .int(Int64(v.canvas.hiddenCount))
            s["zoom"] = .double(Double(v.canvas.viewport.zoom))
            s["pan"] = [.double(Double(v.canvas.viewport.pan.x)), .double(Double(v.canvas.viewport.pan.y))]
            if let f = v.canvas.firstFrameAt { s["firstFrameMs"] = .double(ms(f - opened)) }
            s["selection"] = .array(v.model.selection.nodes.map { .string($0) })
            let st = v.model.state
            s["undo"] = st.undo.map(JSONValue.string) ?? .null
            s["dirty"] = .bool(st.dirty)
            s["washes"] = .int(Int64(v.model.faces.items.filter { $0.shown != nil }.count))
            s["anim"] = .bool(v.model.anim.on)
            s["redo"] = st.redo.map(JSONValue.string) ?? .null
            s["selectMode"] = .bool(v.selectMode)
            s["dragging"] = .bool(v.drag.active)
            s["nodes"] = .int(Int64(v.model.faces.count))
            s["wireCount"] = .int(Int64(v.model.wires.count))
            s["positions"] = .object(Dictionary(uniqueKeysWithValues: v.model.faces.items.prefix(64).map {
                ($0.id, JSONValue.array([.double($0.pos.x), .double($0.pos.y)]))
            }))
            // where faces and their pins are in the canvas view (UI tests aim with them)
            var points: [String: JSONValue] = [:]
            let vp = v.canvas.viewport
            func pt(_ g: CGPoint) -> JSONValue { [.double(Double(vp.toView(g).x)), .double(Double(vp.toView(g).y))] }
            for f in v.model.faces.items.prefix(64) {
                guard let L = v.canvas.faceLayout(nid: f.nid) else { continue }
                let c = CGPoint(x: f.pos.x + L.size.width / 2, y: f.pos.y + L.size.height / 2)
                let outs = (0..<L.outputPins.count).compactMap { v.canvas.pinPoint(face: f.id, output: true, pin: $0) }.map(pt)
                let ins = (0..<L.inputPins.count).compactMap { v.canvas.pinPoint(face: f.id, output: false, pin: $0) }.map(pt)
                points[f.id] = ["center": pt(c), "out": .array(outs), "in": .array(ins)]
            }
            s["points"] = .object(points)
            let pal = v.palette
            s["palette"] = pal.isOpen
                ? ["open": true, "wire": .bool(pal.forWire), "items": .array(pal.items.prefix(40).map { .string($0.label) }),
                   "shown": .bool(paletteView?.presentingViewController != nil), "pending": pal.pendingText.map(JSONValue.string) ?? .null]
                : ["open": false, "shown": .bool(paletteView?.presentingViewController != nil)]
        }
        if let p = watched { s["watched"] = .object(p.describe) }
        StatusFile.shared.window(doc, .object(s))
    }

    // MARK: Keys: the canvas's own (plain keys) and the menu's commands

    override var keyCommands: [UIKeyCommand]? {
        var out: [UIKeyCommand] = []
        func k(_ input: String, _ mods: UIKeyModifierFlags = [], _ title: String, _ sel: Selector) {
            let c = UIKeyCommand(title: title, action: sel, input: input, modifierFlags: mods)
            c.wantsPriorityOverSystemBehavior = true
            out.append(c)
        }
        k("f", [], "Fit", #selector(fitKey(_:)))
        k("\t", [], "Add Node", #selector(paletteKey(_:)))
        k(UIKeyCommand.inputDelete, [], "Delete", #selector(deleteKey(_:)))
        k("\u{8}", [], "Delete", #selector(deleteKey(_:)))
        k(UIKeyCommand.inputEscape, [], "Cancel or Clear Selection", #selector(escapeKey(_:)))
        for (input, title) in [(UIKeyCommand.inputLeftArrow, "Nudge Left"), (UIKeyCommand.inputRightArrow, "Nudge Right"),
                               (UIKeyCommand.inputUpArrow, "Nudge Up"), (UIKeyCommand.inputDownArrow, "Nudge Down")] {
            k(input, [], title, #selector(arrowKey(_:)))
            k(input, .shift, title + " Further", #selector(arrowKey(_:)))
        }
        return out
    }

    @objc func fitKey(_ c: UIKeyCommand) { canvasView?.fitView() }
    @objc func paletteKey(_ c: UIKeyCommand) { canvasView?.openPalette(byKey: true) }
    @objc func deleteKey(_ c: UIKeyCommand) { invoke("edit.delete") }

    @objc func escapeKey(_ c: UIKeyCommand) {
        if let v = canvasView, v.drag.active { return v.cancelDrag() }
        let running = model?.menuBar.item(command: "run.cancel")?.enabled ?? false
        invoke(running ? "run.cancel" : "selection.clear")
    }

    @objc func arrowKey(_ c: UIKeyCommand) {
        guard let v = canvasView else { return }
        // a held key repeats with the same press, so its repeats join one undo step
        // (UIKit doesn't mark a key command's repeats: a press that comes within half a second of
        // the last is taken as the same press)
        let now = ContinuousClock.now
        if let last = lastArrow, now - last < .milliseconds(500) {} else { keyPress &+= 1 }
        lastArrow = now
        let k: Double = c.modifierFlags.contains(.shift) ? 10 : 1
        let (dx, dy): (Double, Double) = switch c.input {
        case UIKeyCommand.inputLeftArrow: (-k, 0)
        case UIKeyCommand.inputRightArrow: (k, 0)
        case UIKeyCommand.inputDownArrow: (0, k)
        default: (0, -k)
        }
        let link = v.canvas.link
        let press = keyPress
        Task { [weak self] in
            do { _ = try await link.nudge(dx: dx, dy: dy, press: press) } catch { self?.say("\(error)") }
        }
    }

    func invoke(_ command: String) {
        guard let m = model else { return }
        Task {
            do { _ = try await m.invoke(command, person: true) } catch { say("\(error)") }
        }
    }

    @objc func zoomIn(_ sender: Any?) { canvasView.map { $0.canvas.zoom(to: $0.canvas.viewport.zoom * 1.25) } }
    @objc func zoomOut(_ sender: Any?) { canvasView.map { $0.canvas.zoom(to: $0.canvas.viewport.zoom / 1.25) } }
    @objc func actualSize(_ sender: Any?) { canvasView?.canvas.zoom(to: 1) }

    /// A menu bar item from the model (its place is the command's property list).
    @objc func runMenuItem(_ sender: Any?) {
        guard let c = sender as? UICommand, let key = c.propertyList as? String, let it = MenuState.shared.item(key) else { return }
        dispatch(it)
    }

    // the standard edit actions, when no text field takes them: the core's commands
    override func cut(_ sender: Any?) { invoke("edit.cut") }
    override func copy(_ sender: Any?) { invoke("edit.copy") }
    override func paste(_ sender: Any?) { invoke("edit.paste") }
    override func delete(_ sender: Any?) { invoke("edit.delete") }
    override func selectAll(_ sender: Any?) { invoke("selection.all") }

    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        switch action {
        case PadMenus.run, #selector(zoomIn(_:)), #selector(zoomOut(_:)), #selector(actualSize(_:)), #selector(toggleWatched(_:)):
            return model != nil
        case #selector(cut(_:)), #selector(copy(_:)), #selector(paste(_:)), #selector(delete(_:)), #selector(selectAll(_:)):
            return role(for: action)?.enabled ?? false
        default:
            return super.canPerformAction(action, withSender: sender)
        }
    }

    /// The model's item for a standard edit action.
    func role(for action: Selector) -> MenuItem? {
        let command: String? = switch action {
        case #selector(cut(_:)): "edit.cut"
        case #selector(copy(_:)): "edit.copy"
        case #selector(paste(_:)): "edit.paste"
        case #selector(delete(_:)): "edit.delete"
        case #selector(selectAll(_:)): "selection.all"
        default: nil
        }
        return command.flatMap { model?.menuBar.item(command: $0) }
    }

    /// The bar's state for a model item: its title (Undo's step name), check, enabled, and the
    /// core's reason as the subtitle of a disabled item.
    override func validate(_ command: UICommand) {
        super.validate(command)
        guard command.action == PadMenus.run, let key = command.propertyList as? String, let it = MenuState.shared.item(key) else { return }
        command.title = MenuState.shared.title(it)
        command.state = it.checked == true ? .on : .off
        command.attributes = it.enabled ? [] : .disabled
        command.subtitle = it.enabled ? nil : it.reason
        command.discoverabilityTitle = it.hover
    }
}
