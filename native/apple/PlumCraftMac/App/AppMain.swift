// Plummer (PlumCraft's native editor) for the Mac: a document-based AppKit app. One core per process (the Swift kit's
// PlumCore, in-process through the C ABI), one core document per window (decision 8), each window
// a GraphDocument showing its graph on the layer canvas.
//
// Arguments (besides AppKit's own `-Key value` defaults):
//   <path>…                       graphs to open, each in its window
//   --control-token-file F        open the core's control port (loopback, port 0) with its token in F
//   --status-file F               write the app's state as JSON to F (pid, port, windows, readiness)
//   --store-root D                the store's root (default: the app's caches)
//   --appearance light|dark       the app's appearance, whatever the system's
//   --zoom Z --center ID          after the first frame, centre node ID at zoom Z (screenshots)
//   --select ID,ID                after the first frame, select these nodes
//   --no-open-panel               don't open a new graph when launched with nothing to open
//   --engine text|array           the engine the core starts on (default text)
//   --engines-also array,…        engines started beside it in every document, whatever the graph
//                                 uses (default none: each document starts only the engines its
//                                 graph uses, decision 8C)
//   --engines-pick on|off         pick a document's engines by its graph (default on; off: --engine
//                                 and --engines-also in every document, for comparisons)
//   --then STEP;STEP              what the first window does once ready (Script.swift)
//   --dump-menus F                write the built menu bar, as text, to F whenever it changes
//   --hide-watched                accepted and ignored (N4 slice 3: no Watched column)
//   --scratch-state               don't restore or save windows (scripts: the Mac's saved state is
//                                 left alone; `-ApplePersistenceIgnoreState YES` would do it too,
//                                 but AppKit then offers the `YES` as a file to open). A scripted
//                                 launch: it refuses to start unless everything it would write
//                                 (the store, marks, caches, log, preferences, craft root, the
//                                 engines folder) is
//                                 inside --scratch-root or the system's temporary folder
//   --scratch-root D              a scratch folder outside the temporary folder that a scripted
//                                 launch may write in (default: none, only the temporary folder)
//   --bench OUT --nodes N --wires M [--phase-seconds S]   the scripted budget run (Bench.swift)
//   --control-bind lan|ADDR --control-token HEX [--control-port N]
//                                 debug builds only (N4 slice 4): the control port on the local
//                                 network with this pre-shared token; a release build refuses it
//                                 and keeps the port on loopback

import AppKit
import PlumCraft
import PlumCraftModels
import PlumCraftShell

struct Options {
    var paths: [String] = []
    var controlTokenFile: String?
    var statusFile: String?
    var storeRoot: String?
    var appearance: String?
    var zoom: Double?
    var center: String?
    var select: [String] = []
    var openPanel = true
    var engine = "text"
    var also: [String] = []
    var pick = true
    var then: [String] = []
    var dumpMenus: String?
    var scratchState = false
    var scratchRoot: String?
    var bench: BenchOptions?
    var controlBind: String?
    var controlToken: String?
    var controlPort: UInt16 = 0

    static func parse(_ argv: [String]) -> Options {
        var o = Options()
        var b: BenchOptions?
        var k = 1
        func next() -> String? {
            k += 1
            return k < argv.count ? argv[k] : nil
        }
        while k < argv.count {
            let a = argv[k]
            switch a {
            case "--control-token-file": o.controlTokenFile = next()
            case "--status-file": o.statusFile = next()
            case "--store-root": o.storeRoot = next()
            case "--appearance": o.appearance = next()
            case "--zoom": o.zoom = next().flatMap(Double.init)
            case "--center": o.center = next()
            case "--select": o.select = (next() ?? "").split(separator: ",").map(String.init)
            case "--no-open-panel": o.openPanel = false
            case "--scratch-state": o.scratchState = true
            case "--scratch-root": o.scratchRoot = next()
            case "--engines-pick": o.pick = next() != "off"
            case "--engines-also":
                let v = next() ?? ""
                o.also = v == "none" ? [] : v.split(separator: ",").map(String.init)
            case "--then": o.then = (next() ?? "").split(separator: ";").map(String.init)
            case "--dump-menus": o.dumpMenus = next()
            case "--control-bind": o.controlBind = next()
            case "--control-token": o.controlToken = next()
            case "--control-port": o.controlPort = next().flatMap(UInt16.init) ?? 0
            case "--hide-watched": break  // N4 slice 3: no Watched column to hide
            case "--engine": o.engine = next() ?? "text"
            case "--bench": b = BenchOptions(out: next() ?? "bench.json")
            case "--nodes": b?.nodes = next().flatMap(Int.init) ?? 500
            case "--wires": b?.wires = next().flatMap(Int.init) ?? 800
            case "--phase-seconds": b?.phaseSeconds = next().flatMap(Double.init) ?? 4
            default:
                if a.hasPrefix("-") {
                    // AppKit's `-Key value` defaults (-ApplePersistenceIgnoreState YES, …)
                    if !a.hasPrefix("--") { k += 1 }
                } else {
                    o.paths.append(a)
                }
            }
            k += 1
        }
        o.bench = b
        return o
    }
}

/// The app's bundle id, as the bundle carries it (set in one place, Config/BundleId.xcconfig);
/// the ids derived from it (log subsystems) follow it.
enum AppID {
    static let bundle = Bundle.main.bundleIdentifier ?? "com.swapwizard.plummer.mac"
}

/// The process's one core, opened at launch and closed at quit.
@MainActor
enum CoreHost {
    static var core: PlumCore?
    static var options = Options()
    static var openError: String?

    /// An error for the person: a sheet-less alert normally; a scripted launch (`--scratch-state`)
    /// logs it instead, so no dialog stalls a run with nobody at the Mac.
    static func present(_ error: Error) {
        if options.scratchState {
            FileHandle.standardError.write(Data("Plummer: \(error.localizedDescription)\n".utf8))
        } else {
            NSApp.presentError(error)
        }
    }
    /// Done once the core's own first document is closed: windows open theirs after it.
    static var ready: Task<UInt64?, Never>?

    /// Where the app writes: its caches, the store's root, its log and its preferences' folder.
    struct Layout {
        var scratch: URL?
        var caches: String
        var store: String
        var logs: String
        var prefs: String
    }

    /// The layout for these options. A scripted launch (--scratch-state) keeps its caches, log and
    /// preferences beside the scratch marks it must be given, not in the person's Library: since
    /// the core writes exports under dirs.caches (B42, the N4 device gate) a scripted export would
    /// land there. With no marks named, it falls back to the Library, which the guard refuses.
    static func layout(_ options: Options, _ env: [String: String]) -> Layout {
        let fm = FileManager.default
        let id = AppID.bundle
        let scratch = options.scratchState
            ? env["COMMONKIT_MARK_DIR"].flatMap { $0.isEmpty ? nil : URL(fileURLWithPath: $0).deletingLastPathComponent() }
            : nil
        let caches = scratch.map { $0.appendingPathComponent("mac-caches").appendingPathComponent(id) }
            ?? (fm.urls(for: .cachesDirectory, in: .userDomainMask).first ?? fm.temporaryDirectory).appendingPathComponent(id)
        let logs = scratch.map { $0.appendingPathComponent("mac-logs").path }
            ?? (fm.urls(for: .libraryDirectory, in: .userDomainMask).first ?? fm.temporaryDirectory).appendingPathComponent("Logs/Plummer").path
        // the app's own preferences (the core keeps every setting there, N4 slice 4);
        // PLUMCRAFT_EDITOR_PREFS, which every script sets to scratch, wins in the core
        let prefs = scratch.map { $0.appendingPathComponent("mac-prefs").path }
            ?? (fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first ?? fm.temporaryDirectory)
            .appendingPathComponent("Plummer").path
        return Layout(scratch: scratch, caches: caches.path, store: options.storeRoot ?? caches.appendingPathComponent("store").path,
                      logs: logs, prefs: prefs)
    }

    static func open() {
        let fm = FileManager.default
        let id = AppID.bundle
        let l = layout(options, ProcessInfo.processInfo.environment)
        let store = l.store
        try? fm.createDirectory(atPath: store, withIntermediateDirectories: true)
        // the config (contract 4.1) and the start are the kit's, shared with the iPad app
        var config = ShellConfig(appID: id, appName: "Plummer", version: ShellConfig.bundleVersion, storeRoot: store)
        config.dirs = ["caches": l.caches, "logs": l.logs, "prefs": l.prefs]
        config.dark = NSApp.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        config.moreContrast = NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast
        config.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        config.reduceTransparency = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
        // document_windows (decision 20, "Mac adopts it"): New and Open in a window are the
        // core's commands, and the feed's `window` domain says which windows to make
        // (DocumentWindows); NSDocument keeps the title bar, Save, Revert, close and Quit
        config.platform = ["os": "macos", "subprocess": true, "craft_apps": true, "python": true, "multi_window": true,
                           "pointer": true, "keyboard": true, "hover": true, "document_windows": true]
        config.displayScale = Double(NSScreen.main?.backingScaleFactor ?? 2)
        config.frameMs = 8.3
        config.engine = options.engine
        config.also = options.also
        config.pickEngines = options.pick
        config.controlTokenFile = options.controlTokenFile
        if let bind = options.controlBind, bind != "loopback" {
            #if DEBUG
            config.controlBind = bind
            config.controlToken = options.controlToken
            config.controlPort = options.controlPort
            #else
            // a release build never listens beyond loopback (decision 10)
            FileHandle.standardError.write(Data("Plummer: --control-bind \(bind) refused: a release build keeps the control port on loopback\n".utf8))
            Status.shared.set("controlBindRefused", .string(bind))
            #endif
        }
        do {
            let (c, r) = try ShellCore.open(config)
            core = c
            DocumentWindows.follow(c)
            ready = Task {
                let port = await r.value
                if let port { Status.shared.set("port", .uint(port)) }
                return port
            }
        } catch {
            openError = "\(error)"
        }
    }
}

/// The app's document controller: AppKit's, except that a scripted launch (`--scratch-state`)
/// under the person's own bundle id would never note a document in the recent documents (the
/// system keeps them outside the app, in the shared file list, so a scratch graph stayed in
/// File › Open Recent: B50) and keep none (a count of 0: with its own count the first launch of
/// each new build rewrote the list's properties, B81). Such a launch now refuses to start (B84):
/// a scripted launch runs as the scripted copy, whose recent documents are its own list (the
/// scripts remove it when they end), so there File › Open Recent works as a person's does and
/// the person's list is never touched. Made in `main` before AppKit's first use, so it is
/// `NSDocumentController.shared`.
///
/// `recentDocumentURLs` is left as AppKit's: overriding it to an empty list made AppKit write the
/// empty list back over the person's (B50's first try).
final class PlummerDocumentController: NSDocumentController {
    /// Read from the arguments, not `CoreHost.options`: AppKit may ask before the delegate parses
    /// them. True only for a scripted launch that is not the scripted copy (which then refuses).
    nonisolated static let scratch = CommandLine.arguments.contains("--scratch-state")
        && ScratchGuard.bundleRefusal(Bundle.main.bundleIdentifier) != nil

    override func noteNewRecentDocumentURL(_ url: URL) {
        if Self.scratch { return }
        super.noteNewRecentDocumentURL(url)
    }

    /// AppKit asks this from a queue of its own once a recent document is noted
    /// (`_NSRecentItemsMenuController`, an NSOperation), so it is nonisolated: as a main-actor
    /// getter it stopped the app in Swift's isolation check the first time the scripted copy
    /// noted a document (the person check after B84). AppKit's own count is its default (10) or
    /// the `NSRecentDocumentsLimit` default, read here without the main actor.
    nonisolated override var maximumRecentDocumentCount: Int {
        if Self.scratch { return 0 }
        return (UserDefaults.standard.object(forKey: "NSRecentDocumentsLimit") as? Int) ?? 10
    }
}

/// What the app writes to `--status-file` for the scripts that drive it (the kit's, shared with the
/// iPad app).
typealias Status = StatusFile

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    var sigterm: DispatchSourceSignal?
    var bench: BenchWindow?

    func applicationWillFinishLaunching(_ note: Notification) {
        CoreHost.options = Options.parse(CommandLine.arguments)
        // every copy started for a status file says its pid in `<status>.pids` (a tracer may
        // start more than one), so a script can quit each by its PID
        Status.shared.path = CoreHost.options.statusFile
        Status.shared.notePid()
        switch CoreHost.options.appearance {
        case "dark"?: NSApp.appearance = NSAppearance(named: .darkAqua)
        case "light"?: NSApp.appearance = NSAppearance(named: .aqua)
        default: break
        }
        // B84: a scripted launch runs only as the scripted copy (its own bundle id), so the
        // panels, recent documents and saved state it leaves are the copy's, never the person's
        if CoreHost.options.scratchState, let why = ScratchGuard.bundleRefusal(Bundle.main.bundleIdentifier)
            ?? Self.scratchRefusal(CoreHost.options, ProcessInfo.processInfo.environment) {
            // loud, and no dialog: a scripted launch that would reach the Mac's own store stops here
            FileHandle.standardError.write(Data("Plummer: \(why)\n".utf8))
            exit(3)
        }
        if let f = CoreHost.options.dumpMenus {
            MenuBar.shared.onDump = { text in try? text.write(toFile: f, atomically: true, encoding: .utf8) }
        }
        MenuBar.shared.show(nil)
        CoreHost.open()
        // a TERM (a script's quit) quits as ⌘Q does
        signal(SIGTERM, SIG_IGN)
        let s = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        s.setEventHandler {
            // a scripted launch's TERM quits without asking about unsaved changes (it would wait
            // for a person who isn't there); otherwise TERM is ⌘Q, which asks
            if CoreHost.options.scratchState {
                for d in NSDocumentController.shared.documents { d.updateChangeCount(.changeCleared) }
            }
            // a menu a script opened is tracking: close it first, then quit as ⌘Q does
            if let m = Script.tracking {
                m.cancelTracking()
                DispatchQueue.main.async { NSApp.terminate(nil) }
            } else {
                NSApp.terminate(nil)
            }
        }
        s.resume()
        sigterm = s
    }

    /// Why a scripted launch (`--scratch-state`) must not start: a place it would write is unset
    /// where the fallback is the person's own, or is outside the scratch root it was given
    /// (`--scratch-root`) and the system's temporary folder (the kit's ScratchGuard). It names no
    /// store or volume (maintainer, 2026-11-21). Written files named on the command line
    /// (`--status-file`, `--dump-menus`, `--bench`, the token file) are the script's own outputs
    /// and are not checked.
    static func scratchRefusal(_ options: Options, _ env: [String: String]) -> String? {
        let l = CoreHost.layout(options, env)
        let locations: [ScratchGuard.Location] = [
            .init("COMMONKIT_STORE_ROOT", env["COMMONKIT_STORE_ROOT"],
                  ifUnset: "the store isn't the script's: a scripted launch must name it there, since it wins over --store-root"),
            .init("COMMONKIT_MARK_DIR", env["COMMONKIT_MARK_DIR"],
                  ifUnset: "the caches, log and preferences would go to the person's Library (~/Library/Caches, ~/Library/Logs, ~/Library/Application Support)"),
            // unset, the core keeps prefs.json in the preferences folder, checked below
            .init("PLUMCRAFT_EDITOR_PREFS", env["PLUMCRAFT_EDITOR_PREFS"]),
            .init("PLUMCRAFT_CRAFT_ROOT", env["PLUMCRAFT_CRAFT_ROOT"],
                  ifUnset: "the craft apps' stores would go to ~/Library/Caches/plumcraft/craft"),
            .init("COMMONKIT_TEST_RUN", env["COMMONKIT_TEST_RUN"]),
            // decision 7 ("Refuse without a scratch engines folder"): the engines folder the core
            // reads manifests from and Install Engine… registers into (B71) is the person's
            // ~/.config/plumcraft/engines unless named; the app sets no dirs.engines of its own
            .init("PLUMCRAFT_ENGINE_FOLDER", env["PLUMCRAFT_ENGINE_FOLDER"],
                  ifUnset: "the engines folder would be the person's ~/.config/plumcraft/engines (an install would register there)"),
            .init(options.storeRoot == nil ? "the store root (the caches' store)" : "--store-root", l.store),
            .init("the caches folder", l.caches),
            .init("the log folder", l.logs),
            .init("the preferences folder", l.prefs),
        ]
        var allowed = ScratchGuard.temporaryFolders(env)
        if let r = options.scratchRoot { allowed.insert(r, at: 0) }
        return ScratchGuard.refusal(locations, inside: allowed)
    }

    func applicationDidFinishLaunching(_ note: Notification) {
        if let why = CoreHost.openError {
            if CoreHost.options.scratchState {
                FileHandle.standardError.write(Data("Plummer couldn't start its core: \(why)\n".utf8))
                exit(4)
            }
            let a = NSAlert()
            a.messageText = "Plummer couldn't start its core"
            a.informativeText = why
            a.runModal()
            NSApp.terminate(nil)
            return
        }
        let o = CoreHost.options
        if let b = o.bench {
            bench = BenchWindow(options: b)
            return
        }
        for p in o.paths {
            let url = URL(fileURLWithPath: p).standardizedFileURL
            NSDocumentController.shared.openDocument(withContentsOf: url, display: true) { _, _, error in
                if let error { CoreHost.present(error) }
            }
        }
        // launched with nothing to open: a new, untitled graph, as Mac document apps do. File › Open
        // is one key away; the Open panel shown here before had no way to make a new graph and
        // blocked the menus while it was up (B70)
        if o.paths.isEmpty && o.openPanel && NSDocumentController.shared.documents.isEmpty {
            NSDocumentController.shared.newDocument(nil)
        }
        // a scripted launch (conformance, screenshots) stays in the background: it never takes
        // the keyboard from the person at the Mac (N4 slice 1)
        if !o.scratchState { NSApp.activate() }
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }

    /// A click on the Dock icon with no window showing (every graph closed, or only pop-outs
    /// left and closed): the graphs' windows come back, or, with none open, a new graph, so a
    /// running app is never left with nothing to click (N4 slice 4).
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if CoreHost.options.scratchState { return true }
        return reopen(hasVisibleWindows: flag)
    }

    /// What a Dock click does (the person check calls it on a scripted launch, whose own Dock
    /// clicks are left to AppKit).
    @discardableResult
    func reopen(hasVisibleWindows flag: Bool) -> Bool {
        if flag { return true }
        let docs = NSDocumentController.shared.documents
        if docs.isEmpty {
            NSDocumentController.shared.newDocument(nil)
        } else {
            for d in docs { d.showWindows() }
        }
        return false
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
    func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool { true }

    func applicationWillTerminate(_ note: Notification) {
        CoreHost.core?.close()
        Status.shared.set("quit", true)
    }
}

@main
enum Main {
    @MainActor static func main() {
        setvbuf(stdout, nil, _IOLBF, 0)
        // the app reads its own arguments; AppKit must not open the words of `--engine text` and
        // the like as documents (it did, with a modal error, N3 slice 4)
        var argDomain = UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)
        argDomain["NSTreatUnknownArgumentsAsOpen"] = "NO"
        UserDefaults.standard.setVolatileDomain(argDomain, forName: UserDefaults.argumentDomain)
        if CommandLine.arguments.contains("--scratch-state") {
            // the person's saved state is neither restored nor written: AppKit keeps a scripted
            // launch's in the system's temporary folder (`<id>.savedState` there, which it leaves
            // empty at quit; TMPDIR doesn't move it), never in ~/Library/Saved Application State
            var args = UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)
            args["ApplePersistenceIgnoreState"] = true
            args["NSQuitAlwaysKeepsWindows"] = false
            UserDefaults.standard.setVolatileDomain(args, forName: UserDefaults.argumentDomain)
        }
        // the first document controller made is the shared one (B50: a scripted launch's recents)
        _ = PlummerDocumentController()
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}
