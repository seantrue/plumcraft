// Documents the iPadOS way (N3 slice 5): each scene's root is the system's document browser
// (Files: recents, iCloud Drive, On My iPad, other providers, tags, search), which presents a graph
// full screen; closing the graph goes back to it. Graphs open in place where the file's provider
// allows (LSSupportsOpeningDocumentsInPlace): the URL's security-scoped access is held for the
// graph's life and given up when it closes. A UIDocument reads the file (coordinated, downloading
// it first when it is in the cloud) and checks it is a graph; the core then opens the same path.
//
// State restoration: a scene says which graph it shows (a bookmark, so access comes back after a
// relaunch) and the view (pan and zoom) in its restoration activity; reconnecting reopens both.
//
// Samples: the first launch copies the bundled graphs into Documents/Samples and opens Showcase,
// so the app shows something without a file; closing it shows the browser.

import PlumCraftCanvas
import PlumCraftModels
import PlumCraftShell
import UIKit
import UniformTypeIdentifiers

/// A graph file. The core holds the graph and writes the file itself (`graph.save`); this only
/// reads it once to check it is a graph and to get it from its provider, so it never saves.
final class GraphDocument: UIDocument {
    override func load(fromContents contents: Any, ofType typeName: String?) throws {
        // a file of another kind the app declares (an application's picture opened as its
        // recipe, docs/shell-interface.md 4.5): the core reads it through the application's
        // opener, or says why it can't
        guard GraphFiles.isGraph(fileURL) else { return }
        let v = (contents as? Data).flatMap { try? JSONSerialization.jsonObject(with: $0) }
        guard let o = v as? [String: Any], o["nodes"] != nil else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError,
                          userInfo: [NSLocalizedDescriptionKey: "It isn't a Plummer graph (a JSON object with \"nodes\")."])
        }
    }

    override func contents(forType typeName: String) throws -> Any {
        throw NSError(domain: NSCocoaErrorDomain, code: NSFileWriteUnknownError,
                      userInfo: [NSLocalizedDescriptionKey: "Plummer's core writes the graph's file itself."])
    }

    override var hasUnsavedChanges: Bool { false }
}

@MainActor
enum Samples {
    static var folder: URL { CoreHost.dirs.documents.appendingPathComponent("Samples", isDirectory: true) }
    static let firstKey = "PlumCraftPadOpenedASample"

    /// Copy the bundled samples into Documents/Samples (those not already there).
    static func install() {
        let fm = FileManager.default
        guard let src = Bundle.main.url(forResource: "Samples", withExtension: nil),
              let list = try? fm.contentsOfDirectory(at: src, includingPropertiesForKeys: nil) else { return }
        try? fm.createDirectory(at: folder, withIntermediateDirectories: true)
        for u in list where GraphFiles.isGraph(u) {
            let to = folder.appendingPathComponent(u.lastPathComponent)
            if !fm.fileExists(atPath: to.path) { try? fm.copyItem(at: u, to: to) }
        }
    }

    /// A fresh copy of bundled sample `name` in scratch, for a scripted launch (`--scratch-state`
    /// with `--open-sample`): every document autosaves, so a UI test opening the copy in Documents
    /// edited it for the next run (the device gate: the tests failed on a second run in the same
    /// container, and on a device the container stays) and changed the person's samples.
    static func scratchCopy(_ name: String) -> URL? {
        let fm = FileManager.default
        guard let src = Bundle.main.url(forResource: "Samples", withExtension: nil)?.appendingPathComponent(name + ".json"),
              fm.fileExists(atPath: src.path) else { return nil }
        let dir = CoreHost.dirs.scratch.appendingPathComponent("samples", isDirectory: true)
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        let to = dir.appendingPathComponent(name + ".json")
        try? fm.removeItem(at: to)
        return (try? fm.copyItem(at: src, to: to)) != nil ? to : nil
    }

    /// The sample to open on the first launch, once.
    static func firstLaunch() -> URL? {
        if UserDefaults.standard.bool(forKey: firstKey) { return nil }
        UserDefaults.standard.set(true, forKey: firstKey)
        let u = folder.appendingPathComponent("Showcase.json")
        return FileManager.default.fileExists(atPath: u.path) ? u : nil
    }

    /// A new graph in the app's own folder (B147), from `template`: "Untitled.plum", or
    /// "Untitled 2.plum" and on when that is taken (FR2: the core's extension). Nil when it can't
    /// be written.
    static func newGraph(from template: URL) -> URL? {
        let fm = FileManager.default
        let dir = CoreHost.dirs.documents
        guard (try? fm.createDirectory(at: dir, withIntermediateDirectories: true)) != nil else { return nil }
        for k in 1...999 {
            let u = dir.appendingPathComponent(GraphFiles.newName(k))
            if fm.fileExists(atPath: u.path) { continue }
            return (try? fm.moveItem(at: template, to: u)) != nil ? u : nil
        }
        return nil
    }

    /// An empty graph to start a new one from (the browser's +).
    static func template() -> URL? {
        let u = CoreHost.dirs.scratch.appendingPathComponent(GraphFiles.newName())
        let ok = (try? Data(#"{"version": 1, "nodes": {}, "wires": []}"#.utf8).write(to: u, options: .atomic)) != nil
        return ok ? u : nil
    }
}

@MainActor
final class BrowserViewController: UIDocumentBrowserViewController, @preconcurrency UIDocumentBrowserViewControllerDelegate {
    /// The graph shown over the browser.
    weak var graph: GraphViewController?

    init() {
        // the document types the app declares (its Info.plist): graphs, and whatever an
        // application built on this shell opens beside them
        super.init(forOpening: nil)
        delegate = self
        allowsDocumentCreation = true
        allowsPickingMultipleItems = false
        localizedCreateDocumentActionTitle = "New Graph"
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    func documentBrowser(_ controller: UIDocumentBrowserViewController, didPickDocumentsAt urls: [URL]) {
        if let u = urls.first { open(u) }
    }

    func documentBrowser(_ controller: UIDocumentBrowserViewController, didRequestDocumentCreationWithHandler handler: @escaping (URL?, UIDocumentBrowserViewController.ImportMode) -> Void) {
        // B147 (the maintainer, 2026-11-23: "Files / Plummer seems a better starting place"): a new
        // graph is made in the app's own folder, the one the Files app shows as Plummer, wherever
        // the browser stands (a place outside it, such as On My iPad's top folder or a provider's,
        // may not let the app write beside the file); the browser then shows it there
        handler(nil, .none)
        guard let t = Samples.template(), let made = Samples.newGraph(from: t) else {
            return creationFailed(CoreHost.dirs.documents, "the empty graph couldn't be written")
        }
        revealDocument(at: made, importIfNeeded: false) { _, _ in }
        open(made)
    }

    /// The browser opens in the app's own folder (B147), once, as the app starts.
    private var revealed = false
    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        guard !revealed else { return }
        revealed = true
        let fm = FileManager.default
        try? fm.createDirectory(at: CoreHost.dirs.documents, withIntermediateDirectories: true)
        // a document inside it, so the browser goes to the folder that holds it
        let inside = (try? fm.contentsOfDirectory(at: CoreHost.dirs.documents, includingPropertiesForKeys: nil))?.sorted { $0.path < $1.path }.first
        if let u = inside { revealDocument(at: u, importIfNeeded: false) { _, _ in } }
    }

    /// A new graph couldn't be made: the person is told why and where (B147: never quietly).
    func creationFailed(_ dir: URL, _ why: String) {
        let place = dir.standardizedFileURL.path == CoreHost.dirs.documents.standardizedFileURL.path ? "\(AppID.name) (this app's folder in Files)" : dir.lastPathComponent
        CoreHost.say("error", "couldn't create a graph in \(dir.path): \(why)")
        StatusFile.shared.set("createError", .string("\(place): \(why)"))
        if CoreHost.options.scratchState { return }
        let a = UIAlertController(title: "Couldn't make a new graph", message: "In \(place): \(why).", preferredStyle: .alert)
        a.addAction(UIAlertAction(title: "OK", style: .default))
        present(a, animated: true)
    }

    func documentBrowser(_ controller: UIDocumentBrowserViewController, didImportDocumentAt source: URL, toDestinationURL destination: URL) {
        open(destination)
    }

    func documentBrowser(_ controller: UIDocumentBrowserViewController, failedToImportDocumentAt documentURL: URL, error: (any Error)?) {
        creationFailed(documentURL.deletingLastPathComponent(), error?.localizedDescription ?? "the system didn't say why")
    }

    /// Open `url` over the browser (closing a graph already shown), with `view` to restore once
    /// its first frame is there.
    func open(_ url: URL, view: Viewport? = nil, then: [String] = []) {
        if let g = graph {
            g.close { [weak self] in self?.open(url, view: view, then: then) }
            return
        }
        let scoped = url.startAccessingSecurityScopedResource()
        nonisolated(unsafe) let doc = GraphDocument(fileURL: url)
        // UIDocument calls back on the main queue
        doc.open { [weak self] ok in
            MainActor.assumeIsolated {
                guard let self else { return }
                guard ok else {
                    if scoped { url.stopAccessingSecurityScopedResource() }
                    self.failed(url, "It couldn't be read, or it isn't a Plummer graph.")
                    return
                }
                let g = GraphViewController(document: doc, scoped: scoped, restore: view, then: then)
                let nav = UINavigationController(rootViewController: g)
                g.browser = self
                self.graph = g
                // the graph takes the window's place, and gives it back when it closes: presented
                // over the browser full screen, it was dismissed by a rotation (N3 slice 5)
                self.show(root: nav, in: self.view.window)
            }
        }
    }

    /// Put `vc` in the window's place (the graph, or the browser again), cross-dissolving.
    func show(root vc: UIViewController, in window: UIWindow?) {
        guard let w = window else { return }
        let animate = !CoreHost.options.scratchState
        w.rootViewController = vc
        if animate { UIView.transition(with: w, duration: 0.25, options: .transitionCrossDissolve, animations: nil) }
    }

    /// Say why `url` didn't open: an alert for a person; the log for a scripted launch.
    func failed(_ url: URL, _ why: String) {
        CoreHost.say("error", "couldn't open \(url.lastPathComponent): \(why)")
        StatusFile.shared.set("openError", .string("\(url.lastPathComponent): \(why)"))
        if CoreHost.options.scratchState { return }
        let a = UIAlertController(title: "Couldn't open \(url.lastPathComponent)", message: why, preferredStyle: .alert)
        a.addAction(UIAlertAction(title: "OK", style: .default))
        present(a, animated: true)
    }
}

@MainActor
final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?
    var browser: BrowserViewController?
    var bench: BenchViewController?
    static var firstScene = true
    static let activityType = AppID.bundle + ".graph"

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions) {
        guard let ws = scene as? UIWindowScene else { return }
        let w = UIWindow(windowScene: ws)
        window = w
        let o = CoreHost.options
        switch o.appearance {
        case "dark"?: w.overrideUserInterfaceStyle = .dark
        case "light"?: w.overrideUserInterfaceStyle = .light
        default: break
        }
        if o.compactWidth { w.traitOverrides.horizontalSizeClass = .compact }
        if let spec = o.windowSize { SceneReport.ask(ws, spec) }
        SceneReport.shared.watch(ws)
        let first = Self.firstScene
        Self.firstScene = false
        StatusFile.shared.set("scenes", .int(Int64(UIApplication.shared.connectedScenes.count)))
        if o.scratchState && !first {
            // a scripted launch has one window: scenes the system kept from earlier runs go (they
            // came forward over the script's graph on a rotation, N3 slice 5)
            UIApplication.shared.requestSceneSessionDestruction(session, options: nil)
            return
        }
        if let b = o.bench, first {
            let v = BenchViewController(options: b)
            bench = v
            w.rootViewController = v
            w.makeKeyAndVisible()
            return
        }
        Samples.install()
        let b = BrowserViewController()
        browser = b
        w.rootViewController = b
        w.makeKeyAndVisible()
        #if DEBUG
        StatusElement.shared.install(in: ws)
        #endif
        if let why = CoreHost.openError {
            let a = UIAlertController(title: "\(AppID.name) couldn't start its core", message: why, preferredStyle: .alert)
            b.present(a, animated: false)
            return
        }
        // what to open: a script's path, a file Files handed over, the scene's restored graph, or
        // on the first launch a sample
        if first, let p = o.open.first {
            b.open(URL(fileURLWithPath: p), then: o.then)
        } else if first, let n = o.sample {
            let fresh = o.scratchState ? Samples.scratchCopy(n) : nil
            b.open(fresh ?? Samples.folder.appendingPathComponent(n + ".json"), then: o.then)
        } else if let ctx = connectionOptions.urlContexts.first {
            b.open(ctx.url)
        } else if !o.scratchState, let act = session.stateRestorationActivity, let (url, view) = Self.restored(act) {
            b.open(url, view: view)
        } else if first, !o.scratchState, let u = Samples.firstLaunch() {
            b.open(u, then: o.then)
        }
    }

    func scene(_ scene: UIScene, openURLContexts contexts: Set<UIOpenURLContext>) {
        guard let ctx = contexts.first, let b = browser else { return }
        if ctx.url.isFileURL { b.open(ctx.url) }
    }

    func sceneDidDisconnect(_ scene: UIScene) {
        StatusFile.shared.set("sceneDisconnected", .string(scene.session.persistentIdentifier))
    }

    /// The window's size changed (a free-size window resized, a rotation): said in the status.
    @available(iOS 26.0, *)
    func windowScene(_ windowScene: UIWindowScene, didUpdateEffectiveGeometry previousEffectiveGeometry: UIWindowScene.Geometry) {
        SceneReport.shared.note(windowScene)
    }

    func sceneDidBecomeActive(_ scene: UIScene) {
        MenuState.shared.show(browser?.graph?.model)
    }

    func sceneDidEnterBackground(_ scene: UIScene) {
        CoreHost.say("info", "scene in the background")
        // what autosave hasn't written yet is written now, within the time the system allows
        // (contract 4.4's lifecycle.background, N4 slice 4): a field being edited commits first
        browser?.graph?.view.endEditing(true)
        CoreHost.flushForBackground()
    }

    // MARK: State restoration

    func stateRestorationActivity(for scene: UIScene) -> NSUserActivity? {
        if CoreHost.options.scratchState { return nil }
        return browser?.graph?.restorationActivity()
    }

    /// The graph and view a restoration activity names.
    static func restored(_ a: NSUserActivity) -> (URL, Viewport?)? {
        guard a.activityType == activityType, let data = a.userInfo?["bookmark"] as? Data else { return nil }
        var stale = false
        guard let url = try? URL(resolvingBookmarkData: data, options: [], relativeTo: nil, bookmarkDataIsStale: &stale) else { return nil }
        var view: Viewport?
        if let z = a.userInfo?["zoom"] as? Double, let x = a.userInfo?["panX"] as? Double, let y = a.userInfo?["panY"] as? Double {
            view = Viewport(pan: CGPoint(x: x, y: y), zoom: z)
        }
        return (url, view)
    }
}
