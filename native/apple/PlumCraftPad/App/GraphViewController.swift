// One open graph on the iPad (N3 slice 5): the layer canvas, a navigation bar with the core's
// toolbar (N4 slice 3: every button's label, enabled state and check from the core) and the
// engines' state, the core's windows (the parameters inspector, pop-outs; the Watched column is
// gone), the way back out of a group, the asks, context menus and the canvas's keys.
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
    /// The core's windows (N4 slice 3).
    var popouts: PadPopouts?
    var parameters: PadParameters?
    let canvasHost = UIView()
    /// The way back out of an entered group: Graph › outer › inner.
    let crumbs = UIStackView()
    let crumbsBox = UIVisualEffectView(effect: UIBlurEffect(style: .systemMaterial))
    let placeholder = UILabel()
    let banner = UILabel()
    var bannerTask: Task<Void, Never>?
    let engineDot = UIView()
    let engineLabel = UILabel()
    /// The toolbar's buttons as the core last gave them, and the bar's items by command.
    var toolItems: [MenuItem] = []
    var barItems: [String: UIBarButtonItem] = [:]
    var engineItem: UIBarButtonItem?
    var doneItem: UIBarButtonItem?
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
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    var name: String { document.fileURL.deletingPathExtension().lastPathComponent }

    // MARK: The view

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        title = name
        canvasHost.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(canvasHost)
        canvasHost.clipsToBounds = true
        // the canvas under the bar's safe area, edge to edge below and at the sides
        NSLayoutConstraint.activate([
            canvasHost.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            canvasHost.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            canvasHost.bottomAnchor.constraint(equalTo: view.bottomAnchor),
            canvasHost.trailingAnchor.constraint(equalTo: view.trailingAnchor),
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
        crumbs.axis = .horizontal
        crumbs.spacing = 2
        crumbs.alignment = .center
        crumbs.translatesAutoresizingMaskIntoConstraints = false
        crumbsBox.contentView.addSubview(crumbs)
        crumbsBox.layer.cornerRadius = 18
        crumbsBox.clipsToBounds = true
        crumbsBox.isHidden = true
        crumbsBox.accessibilityIdentifier = "crumbs"
        for x in [placeholder, banner, crumbsBox] {
            x.translatesAutoresizingMaskIntoConstraints = false
            canvasHost.addSubview(x)
        }
        NSLayoutConstraint.activate([
            crumbs.topAnchor.constraint(equalTo: crumbsBox.contentView.topAnchor, constant: 4),
            crumbs.bottomAnchor.constraint(equalTo: crumbsBox.contentView.bottomAnchor, constant: -4),
            crumbs.leadingAnchor.constraint(equalTo: crumbsBox.contentView.leadingAnchor, constant: 8),
            crumbs.trailingAnchor.constraint(equalTo: crumbsBox.contentView.trailingAnchor, constant: -8),
            crumbsBox.topAnchor.constraint(equalTo: canvasHost.safeAreaLayoutGuide.topAnchor, constant: 10),
            crumbsBox.leadingAnchor.constraint(equalTo: canvasHost.safeAreaLayoutGuide.leadingAnchor, constant: 12),
            crumbsBox.trailingAnchor.constraint(lessThanOrEqualTo: canvasHost.trailingAnchor, constant: -12),
        ])
        NSLayoutConstraint.activate([
            placeholder.centerXAnchor.constraint(equalTo: canvasHost.centerXAnchor),
            placeholder.centerYAnchor.constraint(equalTo: canvasHost.centerYAnchor),
            banner.centerXAnchor.constraint(equalTo: canvasHost.centerXAnchor),
            banner.bottomAnchor.constraint(equalTo: canvasHost.safeAreaLayoutGuide.bottomAnchor, constant: -16),
            banner.widthAnchor.constraint(lessThanOrEqualTo: canvasHost.widthAnchor, constant: -40),
        ])
        makeBar()
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

    /// Documents (back to the browser), the core's toolbar, the engines' state, and the document's
    /// menu on its title (the core's Save and Share…).
    func makeBar() {
        let done = UIBarButtonItem(title: "Documents", image: UIImage(systemName: "chevron.backward"),
                                   primaryAction: UIAction { [weak self] _ in self?.close {} })
        done.accessibilityLabel = "Documents"
        doneItem = done
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
        engineItem = UIBarButtonItem(customView: box)
        navigationItem.style = .editor
        navigationItem.titleMenuProvider = { [weak self] suggested in
            guard let self else { return UIMenu(children: suggested) }
            // the core's Save and Share… (graph.share raises the share ask, N4 slice 3)
            let own = ["graph.save", "graph.share"].compactMap { c in self.model?.menuBar.item(command: c) }.map { it in
                UIAction(title: it.label, image: UIImage(systemName: it.command == "graph.share" ? "square.and.arrow.up" : "square.and.arrow.down"),
                         attributes: it.enabled ? [] : .disabled) { [weak self] _ in self?.dispatch(it) }
            }
            return UIMenu(children: own + suggested)
        }
        layoutBar()
    }

    /// The bar from the core's toolbar: Add, Fit and Select on the left after Documents; Run, Run
    /// Selected, Cancel and Share on the right before the engines.
    func layoutBar() {
        var left: [UIBarButtonItem] = doneItem.map { [$0] } ?? []
        var right: [UIBarButtonItem] = engineItem.map { [$0] } ?? []
        var made: [String: UIBarButtonItem] = [:]
        for it in ToolbarWords.buttons(toolItems) {
            guard let c = it.command else { continue }
            let b = UIBarButtonItem(title: it.label, image: UIImage(systemName: ToolbarWords.symbol(it.icon)),
                                    primaryAction: UIAction { [weak self] _ in self?.toolbarButton(c) })
            b.accessibilityLabel = it.label
            b.accessibilityIdentifier = "tool.\(c)"
            made[c] = b
            if ["canvas.palette", "canvas.fit", "canvas.selectMode"].contains(c) { left.append(b) } else { right.insert(b, at: 0) }
        }
        // the right side reads from the trailing edge: Share, Cancel, Run Selected, Run, engines
        let order = ["graph.share", "run.cancel", "run.selected", "run.start"]
        right.sort { a, b in
            let ka = made.first { $0.value === a }.map(\.key).flatMap { order.firstIndex(of: $0) } ?? 99
            let kb = made.first { $0.value === b }.map(\.key).flatMap { order.firstIndex(of: $0) } ?? 99
            return ka < kb
        }
        barItems = made
        navigationItem.leftBarButtonItems = left
        navigationItem.rightBarButtonItems = right
        validateBar()
    }

    /// The core's toolbar changed: new buttons lay the bar out again; states only revalidate.
    func followToolbar(_ items: [MenuItem]) {
        let before = Set(ToolbarWords.buttons(toolItems).compactMap(\.command))
        toolItems = items
        if Set(ToolbarWords.buttons(items).compactMap(\.command)) != before { layoutBar() } else { validateBar() }
        reportNow()
    }

    /// A toolbar button: the core's item, dispatched as a menu's.
    func toolbarButton(_ command: String) {
        guard let it = toolItems.first(where: { $0.command == command }) else { return }
        if command == "canvas.palette" {
            canvasView?.openPalette()
            return
        }
        dispatch(it)
    }

    /// Present `vc` from the graph, whatever the graph shows now giving way first (one
    /// presentation at a time: the palette, the parameters, a pop-out's sheet).
    func presentReplacing(_ vc: UIViewController) {
        if let other = presentedViewController, other !== vc {
            other.dismiss(animated: false) { [weak self] in self?.present(vc, animated: true) }
        } else if presentedViewController == nil {
            present(vc, animated: true)
        }
    }

    // MARK: The way back out of a group

    func showCrumbs() {
        guard let m = model, let f = m.focus else {
            if !crumbsBox.isHidden { crumbsBox.isHidden = true }
            return
        }
        let names = ["Graph"] + f.path.map(\.name)
        let now = crumbs.arrangedSubviews.compactMap { ($0 as? UIButton)?.configuration?.title }
        if now == names && !crumbsBox.isHidden { return }
        for v in crumbs.arrangedSubviews { v.removeFromSuperview() }
        for (k, n) in names.enumerated() {
            if k > 0 {
                let sep = UIImageView(image: UIImage(systemName: "chevron.right"))
                sep.tintColor = .tertiaryLabel
                crumbs.addArrangedSubview(sep)
            }
            var c = UIButton.Configuration.plain()
            c.title = n
            c.image = k == 0 ? UIImage(systemName: "chevron.backward") : nil
            c.imagePadding = 4
            c.buttonSize = .small
            let target: JSONValue = k == 0 ? .null : .string(f.path[k - 1].id)
            let last = k == names.count - 1
            let b = UIButton(configuration: c, primaryAction: UIAction { [weak m] _ in
                guard let m, !last else { return }
                Task { _ = try? await m.run("canvas.focus", ["group": target]) }
            })
            b.accessibilityIdentifier = "crumb.\(k)"
            b.isEnabled = !last
            crumbs.addArrangedSubview(b)
        }
        crumbsBox.isHidden = false
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
            self?.parameters?.follow()
        }
        v.canvas.onDrawn = { [weak self] _ in
            self?.drawn()
            self?.parameters?.params.sceneChanged()
            self?.showCrumbs()
        }
        v.nameField.onShown = { [weak self] in self?.reportNow() }
        v.onGesture = { g in StatusFile.shared.set("gesture", .string(g)) }
        v.onCommand = { [weak self] id in self?.invoke(id) }
        v.onCommands = { [weak self] ran in self?.ranByGesture(ran) }
        let pc = PaletteController(palette: v.palette)
        pc.onShown = { [weak self] _ in self?.reportNow() }
        paletteView = pc
        v.presentPalette = { [weak self] p, focus in
            guard let self, let v = canvasView, let pc = paletteView else { return }
            // a sheet or popover the graph shows (a pop-out's, the parameters) gives way first
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
        // the field editor's colour picker, as a popover from its row (N4 slice 2)
        v.fieldEditor.present = { [weak self] vc, rect in
            guard let self, let v = canvasView else { return }
            vc.modalPresentationStyle = .popover
            vc.popoverPresentationController?.sourceView = v
            vc.popoverPresentationController?.sourceRect = rect
            present(vc, animated: true)
        }
        v.fieldEditor.onShown = { [weak self] in self?.reportNow() }
        v.contextMenuItems = { [weak self, weak model] items in
            guard let self, let model else { return [] }
            StatusFile.shared.set("contextMenu", .array(items.map { .string($0.label) }))
            return PadMenus.actions(items, model: model) { [weak self] it in self?.dispatch(it) }
        }
        // the core's windows: pop-outs and the parameters inspector
        let pops = PadPopouts(model: model, pictures: v.canvas.pictures)
        pops.host = self
        pops.onChange = { [weak self] in self?.reportNow() }
        popouts = pops
        let params = PadParameters(model: model)
        params.host = self
        parameters = params
        model.onWindows = { [weak pops, weak params] w in
            pops?.follow(w)
            params?.params.follow(w)
        }
        model.onToolbar = { [weak self] items in self?.followToolbar(items) }
        // the asks
        let a = AskPresenter(model: model)
        a.host = self
        a.canvas = v
        a.say = { [weak self] s in self?.say(s) }
        a.onSheet = { ask in StatusFile.shared.set("sheet", ["ask": .uint(ask.ask), "kind": .string(ask.kind.raw)]) }
        a.saveFolder = { [weak self] in self?.saveFolder() ?? CoreHost.dirs.documents }
        asks = a
        model.onAsks = { [weak a] list in a?.update(list) }
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

    var compact: Bool { traitCollection.horizontalSizeClass == .compact }

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
        coordinator.animate(alongsideTransition: nil) { [weak self] _ in
            self?.parameters?.follow()
            self?.reportNow()
        }
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

    /// Select mode on or off: the core's `canvas.selectMode` (N4 slice 3; the toolbar's toggle
    /// shows the core's check).
    func toggleSelectMode() {
        guard let it = toolItems.first(where: { $0.command == "canvas.selectMode" }) else { return }
        dispatch(it)
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

    /// Each button as the core has it: enabled with its reason as the hint, checked when on.
    func validateBar() {
        for it in toolItems {
            guard let c = it.command, let b = barItems[c] else { continue }
            b.isEnabled = it.enabled && model != nil
            b.isSelected = it.checked == true
            b.accessibilityHint = ToolbarWords.help(it)
            b.title = it.label
        }
        StatusFile.shared.set("selectMode", .bool(model?.view.selectMode ?? false))
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
        popouts?.closeAll()
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
                                      "compact": .bool(compact),
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
                let more = v.canvas.moreRect(face: f.nid).map { pt(CGPoint(x: $0.midX, y: $0.midY)) } ?? .null
                points[f.id] = ["center": pt(c), "out": .array(outs), "in": .array(ins), "more": more,
                                "title": pt(CGPoint(x: f.pos.x + L.size.width * 0.4, y: f.pos.y + 14))]
            }
            s["points"] = .object(points)
            // each face's rows' widgets (view points) and values, for UI tests of inline editing
            var rows: [String: JSONValue] = [:]
            for f in v.model.faces.items.prefix(32) {
                var list: [JSONValue] = []
                for (k, r) in f.inputs.enumerated() {
                    guard let rr = v.canvas.rowRect(face: f.nid, row: k) else { list.append(.null); continue }
                    list.append(["key": .string(r.key), "at": pt(CGPoint(x: rr.midX, y: rr.midY)),
                                 "value": r.widget.map { .string(FaceLayout.widgetText($0)) } ?? .null])
                }
                rows[f.id] = .array(list)
            }
            s["rows"] = .object(rows)
            let fe = v.fieldEditor
            if let o = v.rowEditor.open, let c = fe?.control {
                s["editor"] = ["open": true, "port": .string(o.edit.port), "kind": .string(o.widget.kind.raw),
                               "frame": [.double(Double(c.frame.minX)), .double(Double(c.frame.minY)), .double(Double(c.frame.width)), .double(Double(c.frame.height))],
                               "keyboardTop": fe?.keyboardTopInView.map { .double(Double($0)) } ?? .null,
                               "error": v.rowEditor.error.map(JSONValue.string) ?? .null]
            } else {
                s["editor"] = ["open": false, "error": v.rowEditor.error.map(JSONValue.string) ?? .null]
            }
            let pal = v.palette
            s["palette"] = pal.isOpen
                ? ["open": true, "wire": .bool(pal.forWire), "items": .array(pal.items.prefix(40).map { .string($0.label) }),
                   "shown": .bool(paletteView?.presentingViewController != nil), "pending": pal.pendingText.map(JSONValue.string) ?? .null]
                : ["open": false, "shown": .bool(paletteView?.presentingViewController != nil)]
        }
        if let p = popouts { s["popouts"] = p.describe }
        if let p = parameters { s["parameters"] = p.describe }
        if let n = canvasView?.nameField.field {
            s["nameField"] = ["text": .string(n.text ?? ""), "frame": [.double(Double(n.frame.minX)), .double(Double(n.frame.minY)), .double(Double(n.frame.width)), .double(Double(n.frame.height))]]
        }
        s["toolbar"] = .array(ToolbarWords.buttons(toolItems).map { ["id": .string($0.command ?? ""), "label": .string($0.label), "enabled": .bool($0.enabled), "checked": $0.checked.map(JSONValue.bool) ?? .null] })
        s["crumbs"] = .array(crumbsBox.isHidden ? [] : crumbs.arrangedSubviews.compactMap { ($0 as? UIButton)?.configuration?.title }.map { .string($0) })
        if let m = model {
            // where named things are, in view points (UI tests aim with them)
            let vp = canvasView?.canvas.viewport ?? Viewport()
            s["frameTabs"] = .object(Dictionary(uniqueKeysWithValues: m.frames.items.map { f in
                (f.id, JSONValue.array([.double(Double(vp.toView(CGPoint(x: f.pos.x + 40, y: f.pos.y - 11)).x)), .double(Double(vp.toView(CGPoint(x: f.pos.x + 40, y: f.pos.y - 11)).y))]))
            }))
            s["frames"] = .object(Dictionary(uniqueKeysWithValues: m.frames.items.map { ($0.id, JSONValue.string($0.title)) }))
            s["focus"] = m.focus.map { .string($0.group) } ?? .null
        }
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
        case PadMenus.run, #selector(zoomIn(_:)), #selector(zoomOut(_:)), #selector(actualSize(_:)):
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
