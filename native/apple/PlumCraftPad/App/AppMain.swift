// PlumCraft for the iPad: a document-based UIKit app. One core per process (the Swift kit's
// PlumCore, in-process through the C ABI, both built-in engines in it, each document starting the
// ones its graph uses), one core document per open
// graph, each scene a document browser that presents its graph on the layer canvas (decision 8:
// several scenes, Split View and Stage Manager).
//
// Launch arguments (scripts: `simctl launch`, `devicectl device process launch`):
//   --open PATH                 open this graph at launch (a path the app can read)
//   --open-sample NAME          open the bundled sample NAME (Documents/Samples/NAME.json)
//   --control-token-file F      open the core's control port (loopback, port 0) with its token in F
//   --status-file F             write the app's state as JSON to F (pid, port, scenes, readiness);
//                               a relative F is inside the container's tmp (a device's scripts
//                               copy it back with devicectl)
//   --open-empty                open a new empty graph in the container's tmp (the conformance
//                               suite on a device: never a sample, which autosave would write)
//   --store-root D              the store's root; refused unless inside the app's own container
//   --scratch-state             don't restore or save the scene's state, and don't open a sample on
//                               first launch (scripts)
//   --appearance light|dark     the app's appearance, whatever the system's
//   --engine text|array         the engine the core starts on (default text)
//   --engines-also array,…      engines started beside it in every document, whatever the graph uses
//                               (default none: each document starts only the engines its graph uses)
//   --engines-pick on|off       pick a document's engines by its graph (default on; off: --engine and
//                               --engines-also in every document, for comparisons)
//   --hide-watched              accepted and ignored (N4 slice 3: no Watched column)
//   --then STEP;STEP            what the first graph does once ready (Script.swift)
//   --bench OUT --nodes N --wires M [--phase-seconds S]   the budget run (Bench.swift); OUT is
//                               relative to Documents unless absolute
//   --control-bind lan|ADDR --control-token HEX [--control-port N]
//                               debug builds only (N4 slice 4): the control port on the local
//                               network with this pre-shared token, so the conformance suite runs
//                               on the device; a release build refuses it (loopback only)
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
    /// `--status-element` (debug builds): the status also in an accessibility element, which a UI
    /// test reads on a device, where its runner can't read the app's files (the N4 device gate).
    var statusElement = false
    var storeRoot: String?
    var scratchState = false
    var appearance: String?
    /// `--width-class compact`: the scripted launch's windows lay out as a narrow window does
    /// (Split View, Slide Over, a narrow Stage Manager window), on any iPad (N4 slice 6).
    var compactWidth = false
    var engine = "text"
    var also: [String] = []
    var pick = true
    var then: [String] = []
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
            switch argv[k] {
            case "--open": if let p = next() { o.open.append(p) }
            case "--open-sample": if let n = next() { o.sample = n }
            case "--control-token-file": o.controlTokenFile = next()
            case "--status-file": o.statusFile = next()
            case "--status-element": o.statusElement = true
            case "--store-root": o.storeRoot = next()
            case "--scratch-state": o.scratchState = true
            case "--appearance": o.appearance = next()
            case "--width-class": o.compactWidth = next() == "compact"
            case "--engine": o.engine = next() ?? "text"
            case "--engines-pick": o.pick = next() != "off"
            case "--engines-also":
                let v = next() ?? ""
                o.also = v == "none" ? [] : v.split(separator: ",").map(String.init)
            case "--hide-watched": break  // N4 slice 3: no Watched column to hide
            case "--open-empty":
                let u = FileManager.default.temporaryDirectory.appendingPathComponent("conformance-empty.json")
                try? Data(#"{"version": 1, "nodes": {}, "wires": []}"#.utf8).write(to: u, options: .atomic)
                o.open.append(u.path)
            case "--control-bind": o.controlBind = next()
            case "--control-token": o.controlToken = next()
            case "--control-port": o.controlPort = next().flatMap(UInt16.init) ?? 0
            case "--then": o.then = (next() ?? "").split(separator: ";").map(String.init)
            case "--bench": b = BenchOptions(out: next() ?? "bench.json")
            case "--nodes": b?.nodes = next().flatMap(Int.init) ?? 500
            case "--wires": b?.wires = next().flatMap(Int.init) ?? 800
            case "--phase-seconds": b?.phaseSeconds = next().flatMap(Double.init) ?? 4
            case "--hide-wires": b?.hideWires = true
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

#if DEBUG
/// The status as an accessibility element (`--status-element`, debug builds only): a UI test on
/// a device reads it with `XCUIApplication().descendants(matching: .any)["plumcraft-status"]`,
/// since the test runner's sandbox can't see the app's container there (in the simulator both
/// share the Mac's files and the tests read `--status-file`). It lives in a window of its own
/// above every sheet, a point square, untouchable and all but transparent, so a presented sheet
/// (which hides what is under it from accessibility) never hides it; one per process, in the
/// first scene. The value is the status file's text, whole.
@MainActor
final class StatusElement {
    static let shared = StatusElement()
    private var window: UIWindow?
    private let view = UIView(frame: CGRect(x: 0, y: 0, width: 2, height: 2))
    private var text = "{}"

    func start() {
        view.isAccessibilityElement = true
        view.accessibilityIdentifier = "plumcraft-status"
        view.accessibilityLabel = "status"
        view.backgroundColor = UIColor.black.withAlphaComponent(0.02)
        StatusFile.shared.mirror = { t in
            // the status is written on the main thread; anything else is moved there
            if Thread.isMainThread {
                MainActor.assumeIsolated { StatusElement.shared.set(t) }
            } else {
                DispatchQueue.main.async { MainActor.assumeIsolated { StatusElement.shared.set(t) } }
            }
        }
    }

    private func set(_ t: String) {
        text = t
        view.accessibilityValue = t
    }

    /// In the first scene, once it has a window.
    func install(in scene: UIWindowScene) {
        guard window == nil, StatusFile.shared.mirror != nil else { return }
        let w = UIWindow(windowScene: scene)
        w.windowLevel = .alert + 10
        w.isUserInteractionEnabled = false
        w.frame = CGRect(x: 0, y: 0, width: 2, height: 2)
        let vc = UIViewController()
        vc.view = view
        w.rootViewController = vc
        w.isHidden = false
        view.accessibilityValue = text
        window = w
    }
}
#endif

/// A background task's identifier, set once it is begun and read by its expiry handler.
final class BackgroundTask: @unchecked Sendable {
    // set on the main thread before the handler can run, read there by it
    var id = UIBackgroundTaskIdentifier.invalid
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

    /// Backgrounding: the core writes what autosave hasn't yet (`lifecycle.background`), within
    /// the time the system gives (N4 slice 4, contract 4.4).
    static func flushForBackground() {
        guard let core else { return }
        let box = BackgroundTask()
        box.id = UIApplication.shared.beginBackgroundTask(withName: "PlumCraft autosave") {
            // the system's time ran out: give it back (called on the main thread)
            MainActor.assumeIsolated { UIApplication.shared.endBackgroundTask(box.id) }
        }
        let id = box.id
        Task {
            let r = try? await core.request("lifecycle.background")
            say("info", "backgrounded: saved \(r?["saved"].array?.count ?? 0), failed \(r?["failed"].array?.count ?? 0)")
            StatusFile.shared.set("background", r ?? .null)
            UIApplication.shared.endBackgroundTask(id)
        }
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
        var config = ShellConfig(appID: Bundle.main.bundleIdentifier ?? "com.plumcraft.pad", appName: "PlumCraft", version: ShellConfig.bundleVersion, storeRoot: store)
        config.dirs = ["documents": d.documents.path, "caches": d.caches.path, "scratch": d.scratch.path, "prefs": d.prefs.path, "logs": d.logs.path]
        // contract 4.2: no programs, no craft apps, no Python; in-process engines only
        config.platform = ["os": "ios", "subprocess": false, "craft_apps": false, "python": false, "multi_window": true,
                           "pointer": true, "keyboard": true, "hover": true, "pencil": true, "max_picture_px": 2048]
        config.displayScale = Double(UITraitCollection.current.displayScale)
        config.frameMs = 1000.0 / 120
        config.engine = options.engine
        config.also = options.also
        config.pickEngines = options.pick
        config.controlTokenFile = options.controlTokenFile
        let t = UITraitCollection.current
        config.dark = t.userInterfaceStyle == .dark
        config.moreContrast = UIAccessibility.isDarkerSystemColorsEnabled
        config.reduceMotion = UIAccessibility.isReduceMotionEnabled
        config.reduceTransparency = UIAccessibility.isReduceTransparencyEnabled
        if let bind = options.controlBind, bind != "loopback" {
            #if DEBUG
            config.controlBind = bind
            config.controlToken = options.controlToken
            config.controlPort = options.controlPort
            #else
            // a release build never listens beyond loopback (decision 10)
            logger.error("--control-bind \(bind, privacy: .public) refused: a release build keeps the control port on loopback")
            StatusFile.shared.set("controlBindRefused", .string(bind))
            #endif
        }
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
            Task {
                if let info = try? await c.request("core.control") {
                    StatusFile.shared.set("control", info)
                    if let b = info["bind"].string { say("info", "control port on \(b):\(info["port"].uint64 ?? 0)") }
                }
            }
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
        StatusFile.shared.path = CoreHost.options.statusFile.map { f in
            f.hasPrefix("/") ? f : FileManager.default.temporaryDirectory.appendingPathComponent(f).path
        }
        StatusFile.shared.notePid()
        #if DEBUG
        if CoreHost.options.statusElement { StatusElement.shared.start() }
        #endif
        CoreHost.open()
        return true
    }

    func application(_ application: UIApplication, configurationForConnecting connectingSceneSession: UISceneSession,
                     options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        // a value popped out in a window of its own (N4 slice 3)
        let popout = options.userActivities.contains { $0.activityType == PadPopouts.activityType }
            || connectingSceneSession.stateRestorationActivity?.activityType == PadPopouts.activityType
            || connectingSceneSession.configuration.name == "Pop-Out"
        let c = UISceneConfiguration(name: popout ? "Pop-Out" : "Default", sessionRole: connectingSceneSession.role)
        c.delegateClass = popout ? PopoutSceneDelegate.self : SceneDelegate.self
        return c
    }

    func applicationWillTerminate(_ application: UIApplication) {
        CoreHost.log?.stop()
        CoreHost.core?.close()
        StatusFile.shared.set("quit", true)
    }

    func applicationDidReceiveMemoryWarning(_ application: UIApplication) {
        // the shell drops its own decoded pictures (the canvas keeps its face pictures), and the
        // core its media caches (contract 4.4's lifecycle.memory, N4 slice 4)
        for g in GraphViewController.all { g.canvasView?.canvas.pictures.drop() }
        _ = CoreHost.core?.post("lifecycle.memory", ["level": "low"])
        CoreHost.say("warn", "memory warning: decoded pictures and the core's media caches dropped")
    }

    // MARK: The menu bar (iPadOS 26's, and the keyboard shortcut overlay)

    override func buildMenu(with builder: any UIMenuBuilder) {
        super.buildMenu(with: builder)
        guard builder.system == .main else { return }
        PadMenus.build(builder)
    }
}
