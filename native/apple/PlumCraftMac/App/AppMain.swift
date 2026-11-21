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
//   --no-open-panel               don't show File › Open when launched with nothing to open
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
//                                 launch: it refuses to start unless COMMONKIT_STORE_ROOT and
//                                 COMMONKIT_MARK_DIR name scratch folders, never the Mac's own store
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

    static func open() {
        let fm = FileManager.default
        let id = AppID.bundle
        // a scripted launch (--scratch-state) keeps its caches and log beside the scratch marks it
        // must be given, not in the person's Library: since the core writes exports under
        // dirs.caches (B42, the N4 device gate) a scripted export would land there
        let scratch = options.scratchState
            ? ProcessInfo.processInfo.environment["COMMONKIT_MARK_DIR"].flatMap { $0.isEmpty ? nil : URL(fileURLWithPath: $0).deletingLastPathComponent() }
            : nil
        let caches = scratch.map { $0.appendingPathComponent("mac-caches").appendingPathComponent(id) }
            ?? (fm.urls(for: .cachesDirectory, in: .userDomainMask).first ?? fm.temporaryDirectory).appendingPathComponent(id)
        let store = options.storeRoot ?? caches.appendingPathComponent("store").path
        try? fm.createDirectory(atPath: store, withIntermediateDirectories: true)
        let logs = scratch.map { $0.appendingPathComponent("mac-logs").path }
            ?? (fm.urls(for: .libraryDirectory, in: .userDomainMask).first ?? fm.temporaryDirectory).appendingPathComponent("Logs/Plummer").path
        // the config (contract 4.1) and the start are the kit's, shared with the iPad app
        var config = ShellConfig(appID: id, appName: "Plummer", version: ShellConfig.bundleVersion, storeRoot: store)
        // the app's own preferences (the core keeps every setting there, N4 slice 4);
        // PLUMCRAFT_EDITOR_PREFS, which every script sets to scratch, wins in the core
        let prefs = (fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first ?? fm.temporaryDirectory)
            .appendingPathComponent("Plummer").path
        config.dirs = ["caches": caches.path, "logs": logs, "prefs": prefs]
        config.dark = NSApp.effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        config.moreContrast = NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast
        config.reduceMotion = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        config.reduceTransparency = NSWorkspace.shared.accessibilityDisplayShouldReduceTransparency
        config.platform = ["os": "macos", "subprocess": true, "craft_apps": true, "python": true, "multi_window": true,
                           "pointer": true, "keyboard": true, "hover": true]
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
/// never notes a document in the person's recent documents (the system keeps them outside the
/// app, in the shared file list, so a scratch graph stayed in File › Open Recent: B50), and keeps
/// none (a count of 0): AppKit then leaves the list alone, where with its own count the first
/// launch of each new build rewrote the list's properties (the code requirement it carries). Made
/// in `main` before AppKit's first use, so it is `NSDocumentController.shared`.
///
/// `recentDocumentURLs` is left as AppKit's: overriding it to an empty list made AppKit write the
/// empty list back over the person's (B50's first try).
final class PlummerDocumentController: NSDocumentController {
    /// Read from the arguments, not `CoreHost.options`: AppKit may ask before the delegate parses them.
    static let scratch = CommandLine.arguments.contains("--scratch-state")

    override func noteNewRecentDocumentURL(_ url: URL) {
        if Self.scratch { return }
        super.noteNewRecentDocumentURL(url)
    }

    override var maximumRecentDocumentCount: Int { Self.scratch ? 0 : super.maximumRecentDocumentCount }
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
        if CoreHost.options.scratchState, let why = Self.scratchRefusal() {
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

    /// Why a scripted launch (`--scratch-state`) must not start: the store or marks it would use
    /// aren't named, or are the Mac's own (COMMONKIT_STORE_ROOT wins over the config's root).
    static func scratchRefusal() -> String? {
        let env = ProcessInfo.processInfo.environment
        for k in ["COMMONKIT_STORE_ROOT", "COMMONKIT_MARK_DIR", "PLUMCRAFT_EDITOR_PREFS"] {
            guard let v = env[k], !v.isEmpty else { return "--scratch-state needs \(k) set to a scratch folder" }
            if v.hasPrefix("/Volumes/CommonkitStore") { return "--scratch-state refuses \(k)=\(v): that is the Mac's own store" }
            if k == "PLUMCRAFT_EDITOR_PREFS", v.contains("/Library/Application Support/") {
                return "--scratch-state refuses PLUMCRAFT_EDITOR_PREFS=\(v): that is a person's own preferences"
            }
        }
        if let r = CoreHost.options.storeRoot, r.hasPrefix("/Volumes/CommonkitStore") { return "--scratch-state refuses --store-root \(r)" }
        return nil
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
        if o.paths.isEmpty && o.openPanel && NSDocumentController.shared.documents.isEmpty {
            NSDocumentController.shared.openDocument(nil)
        }
        // a scripted launch (conformance, screenshots) stays in the background: it never takes
        // the keyboard from the person at the Mac (N4 slice 1)
        if !o.scratchState { NSApp.activate() }
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }

    /// A click on the Dock icon with no window showing (every graph closed, or only pop-outs
    /// left and closed): the graphs' windows come back, or, with none open, File › Open, so a
    /// running app is never left with nothing to click (N4 slice 4).
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if flag || CoreHost.options.scratchState { return true }
        let docs = NSDocumentController.shared.documents
        if docs.isEmpty {
            NSDocumentController.shared.openDocument(nil)
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
