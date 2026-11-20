// PlumCraft for the iPad: a document-based UIKit app. One core per process (the Swift kit's
// PlumCore, in-process through the C ABI, both built-in engines in it), one core document per open
// graph, each scene a document browser that presents its graph on the layer canvas (decision 8:
// several scenes, Split View and Stage Manager).
//
// Launch arguments (scripts: `simctl launch`, `devicectl device process launch`):
//   --open PATH                 open this graph at launch (a path the app can read)
//   --open-sample NAME          open the bundled sample NAME (Documents/Samples/NAME.json)
//   --control-token-file F      open the core's control port (loopback, port 0) with its token in F
//   --status-file F             write the app's state as JSON to F (pid, port, scenes, readiness)
//   --store-root D              the store's root; refused unless inside the app's own container
//   --scratch-state             don't restore or save the scene's state, and don't open a sample on
//                               first launch (scripts)
//   --appearance light|dark     the app's appearance, whatever the system's
//   --engine text|array         the engine the core starts on (default text)
//   --engines-also array,…      engines started beside it in every document (default array; "none")
//   --hide-watched              start with the Watched panel hidden
//   --then STEP;STEP            what the first graph does once ready (Script.swift)
//   --bench OUT --nodes N --wires M [--phase-seconds S]   the budget run (Bench.swift); OUT is
//                               relative to Documents unless absolute
//
// The store, marks, craft root and preferences are always inside the app's container: the app
// sets COMMONKIT_STORE_ROOT and the rest itself before the core opens, so nothing a launcher passes
// (a simulator inherits SIMCTL_CHILD_ variables from the Mac) can point the core at another store.

import os
import PlumCraft
import PlumCraftModels
import PlumCraftShell
import UIKit

struct Options {
    var open: [String] = []
    /// A bundled sample to open by name (UI tests, which don't know the container's path).
    var sample: String?
    var controlTokenFile: String?
    var statusFile: String?
    var storeRoot: String?
    var scratchState = false
    var appearance: String?
    var engine = "text"
    var also = ["array"]
    var hideWatched = false
    var then: [String] = []
    var bench: BenchOptions?

    static func parse(_ argv: [String]) -> Options {
        var o = Options()
        var b: BenchOptions?
        var k = 1
        func next() -> String? {
            k += 1
            return k < argv.count ? argv[k] : nil
        }
        while k < argv.count {
            switch argv[k] {
            case "--open": if let p = next() { o.open.append(p) }
            case "--open-sample": if let n = next() { o.sample = n }
            case "--control-token-file": o.controlTokenFile = next()
            case "--status-file": o.statusFile = next()
            case "--store-root": o.storeRoot = next()
            case "--scratch-state": o.scratchState = true
            case "--appearance": o.appearance = next()
            case "--engine": o.engine = next() ?? "text"
            case "--engines-also":
                let v = next() ?? ""
                o.also = v == "none" ? [] : v.split(separator: ",").map(String.init)
            case "--hide-watched": o.hideWatched = true
            case "--then": o.then = (next() ?? "").split(separator: ";").map(String.init)
            case "--bench": b = BenchOptions(out: next() ?? "bench.json")
            case "--nodes": b?.nodes = next().flatMap(Int.init) ?? 500
            case "--wires": b?.wires = next().flatMap(Int.init) ?? 800
            case "--phase-seconds": b?.phaseSeconds = next().flatMap(Double.init) ?? 4
            default: break  // the system's own arguments (-NSDoubleLocalizedStrings YES, …)
            }
            k += 1
        }
        o.bench = b
        return o
    }
}

/// The app's folders, all inside its container (plan 6: the core sees only these roots).
struct Dirs {
    let home = URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
    var documents: URL { home.appendingPathComponent("Documents", isDirectory: true) }
    var caches: URL { home.appendingPathComponent("Library/Caches/plumcraft", isDirectory: true) }
    var store: URL { home.appendingPathComponent("Library/Caches/commonkit/store", isDirectory: true) }
    var scratch: URL { home.appendingPathComponent("tmp/plumcraft", isDirectory: true) }
    var prefs: URL { home.appendingPathComponent("Library/Application Support/plumcraft", isDirectory: true) }
    var logs: URL { home.appendingPathComponent("Library/Logs/plumcraft", isDirectory: true) }

    /// Whether `path` is inside the container.
    func inside(_ path: String) -> Bool {
        let p = URL(fileURLWithPath: path).standardizedFileURL.path
        let h = home.standardizedFileURL.path
        return p == h || p.hasPrefix(h.hasSuffix("/") ? h : h + "/")
    }
}

/// The process's one core.
@MainActor
enum CoreHost {
    static var core: PlumCore?
    static var options = Options()
    static var openError: String?
    static var ready: Task<UInt64?, Never>?
    static var log: UnifiedLog?
    static let dirs = Dirs()
    static let logger = Logger(subsystem: "com.plumcraft.pad", category: "app")

    /// A line for the unified log and the core's (`log.write`).
    static func say(_ level: String, _ message: String) {
        if let log { log.say(level, message) } else { logger.notice("\(message, privacy: .public)") }
    }

    static func open() {
        let fm = FileManager.default
        let d = dirs
        var store = d.store.path
        if let r = options.storeRoot {
            guard d.inside(r) else {
                openError = "--store-root \(r) is outside the app's container; refused"
                return
            }
            store = r
        }
        for u in [d.documents, d.caches, d.scratch, d.prefs, d.logs] { try? fm.createDirectory(at: u, withIntermediateDirectories: true) }
        try? fm.createDirectory(atPath: store, withIntermediateDirectories: true)
        // the store, marks, craft root and preferences: here, whatever the environment said
        // (COMMONKIT_STORE_ROOT wins over the config's store_root)
        setenv("COMMONKIT_STORE_ROOT", store, 1)
        setenv("COMMONKIT_MARK_DIR", d.caches.appendingPathComponent("marks").path, 1)
        setenv("COMMONKIT_TEST_RUN", d.scratch.appendingPathComponent("run").path, 1)
        setenv("PLUMCRAFT_CRAFT_ROOT", d.caches.appendingPathComponent("craft").path, 1)
        setenv("PLUMCRAFT_EDITOR_PREFS", d.prefs.appendingPathComponent("prefs.json").path, 1)
        setenv("COMMONKIT_HASH", "b3", 0)
        setenv("COMMONKIT_STORE_MIN_FREE_GB", "2", 0)
        var config = ShellConfig(appID: Bundle.main.bundleIdentifier ?? "com.plumcraft.pad", appName: "PlumCraft", version: "0.7.0", storeRoot: store)
        config.dirs = ["documents": d.documents.path, "caches": d.caches.path, "scratch": d.scratch.path, "prefs": d.prefs.path, "logs": d.logs.path]
        // contract 4.2: no programs, no craft apps, no Python; in-process engines only
        config.platform = ["os": "ios", "subprocess": false, "craft_apps": false, "python": false, "multi_window": true,
                           "pointer": true, "keyboard": true, "hover": true, "pencil": true, "max_picture_px": 2048]
        config.displayScale = Double(UITraitCollection.current.displayScale)
        config.frameMs = 1000.0 / 120
        config.engine = options.engine
        config.also = options.also
        config.controlTokenFile = options.controlTokenFile
        do {
            let (c, r) = try ShellCore.open(config)
            core = c
            let l = UnifiedLog(core: c, subsystem: "com.plumcraft.pad")
            l.start()
            log = l
            ready = Task {
                let port = await r.value
                if let port { StatusFile.shared.set("port", .uint(port)) }
                return port
            }
            StatusFile.shared.set("store", .string(store))
            say("info", "PlumCraft started: store \(store), engines \(([options.engine] + options.also).joined(separator: ", "))")
        } catch {
            openError = "\(error)"
            logger.error("couldn't start the core: \(error, privacy: .public)")
        }
    }
}

@main
@MainActor
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, willFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
        CoreHost.options = Options.parse(CommandLine.arguments)
        StatusFile.shared.path = CoreHost.options.statusFile
        StatusFile.shared.notePid()
        CoreHost.open()
        return true
    }

    func application(_ application: UIApplication, configurationForConnecting connectingSceneSession: UISceneSession,
                     options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let c = UISceneConfiguration(name: "Default", sessionRole: connectingSceneSession.role)
        c.delegateClass = SceneDelegate.self
        return c
    }

    func applicationWillTerminate(_ application: UIApplication) {
        CoreHost.log?.stop()
        CoreHost.core?.close()
        StatusFile.shared.set("quit", true)
    }

    func applicationDidReceiveMemoryWarning(_ application: UIApplication) {
        // contract 4.4's lifecycle.memory isn't built in the core yet: the shell drops its own
        // decoded pictures (the canvas keeps its face pictures)
        for g in GraphViewController.all { g.canvasView?.canvas.pictures.drop() }
        CoreHost.say("warn", "memory warning: decoded pictures dropped")
    }

    // MARK: The menu bar (iPadOS 26's, and the keyboard shortcut overlay)

    override func buildMenu(with builder: any UIMenuBuilder) {
        super.buildMenu(with: builder)
        guard builder.system == .main else { return }
        PadMenus.build(builder)
    }
}
