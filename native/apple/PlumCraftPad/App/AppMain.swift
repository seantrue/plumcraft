// Plummer (PlumCraft's native editor) for the iPad: a document-based UIKit app. One core per process (the Swift kit's
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
import PlumCraftCanvas
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
    /// `--window-size WxH` or `full` (scripts and the UI tests): the window asks for that size
    /// where the system lets a window be any size (iPadOS's Windowed Apps or Stage Manager),
    /// through the scene's size restrictions; iPadOS keeps a window's last size otherwise. Not
    /// honoured with Full Screen Apps (the window is the screen).
    var windowSize: String?
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
            case "--window-size": o.windowSize = next()
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
    var caches: URL { home.appendingPathComponent("Library/Caches/plummer", isDirectory: true) }
    var store: URL { home.appendingPathComponent("Library/Caches/commonkit/store", isDirectory: true) }
    var scratch: URL { home.appendingPathComponent("tmp/plummer", isDirectory: true) }
    var prefs: URL { home.appendingPathComponent("Library/Application Support/plummer", isDirectory: true) }
    var logs: URL { home.appendingPathComponent("Library/Logs/plummer", isDirectory: true) }

    /// Whether `path` is inside the container.
    func inside(_ path: String) -> Bool {
        let p = URL(fileURLWithPath: path).standardizedFileURL.path
        let h = home.standardizedFileURL.path
        return p == h || p.hasPrefix(h.hasSuffix("/") ? h : h + "/")
    }
}

/// B118: the on-screen keyboard (`keyboard`: shown, its frame in the screen) and what holds the
/// keyboard's focus (`firstResponder`: its type, and a text field's accessibility identifier),
/// written to the status whenever the keyboard shows, hides or moves.
/// The window as the system has it, for scripts and the UI tests (`scene` in the status): its size,
/// its size classes, the scene's activation state, the orientation, and the size a launch asked
/// for with `--window-size`. On an iPad with Windowed Apps or Stage Manager a window can be any
/// size and keeps the size it last had, so every log should say what it was (B119).
@MainActor
final class SceneReport {
    static let shared = SceneReport()
    private var asked: String?
    private var watched: Set<ObjectIdentifier> = []

    static func word(_ c: UIUserInterfaceSizeClass) -> String {
        switch c {
        case .compact: return "compact"
        case .regular: return "regular"
        default: return "unspecified"
        }
    }

    static func word(_ s: UIScene.ActivationState) -> String {
        switch s {
        case .foregroundActive: return "foregroundActive"
        case .foregroundInactive: return "foregroundInactive"
        case .background: return "background"
        default: return "unattached"
        }
    }

    /// The scene's window, its classes and state, as JSON.
    static func describe(_ ws: UIWindowScene) -> JSONValue {
        let t = ws.traitCollection
        let b = ws.coordinateSpace.bounds
        var o: [String: JSONValue] = [
            "size": .array([.double(Double(b.width)), .double(Double(b.height))]),
            "screen": .array([.double(Double(ws.screen.bounds.width)), .double(Double(ws.screen.bounds.height))]),
            "horizontal": .string(word(t.horizontalSizeClass)),
            "vertical": .string(word(t.verticalSizeClass)),
            "state": .string(word(ws.activationState)),
            "orientation": .int(Int64(ws.effectiveGeometry.interfaceOrientation.rawValue)),
            "fullScreen": .bool(abs(b.width - ws.screen.bounds.width) < 1 && abs(b.height - ws.screen.bounds.height) < 1),
        ]
        if let r = ws.sizeRestrictions {
            o["restricted"] = .array([.double(Double(r.minimumSize.width)), .double(Double(r.minimumSize.height)),
                                      .double(Double(r.maximumSize.width)), .double(Double(r.maximumSize.height))])
        }
        if let a = shared.asked { o["asked"] = .string(a) }
        return .object(o)
    }

    /// `--window-size WxH` or `full`: the scene's size restrictions pinned to that size, which a
    /// system with free-size windows honours by resizing the window (tried on the simulator).
    static func ask(_ ws: UIWindowScene, _ spec: String) {
        shared.asked = spec
        guard let r = ws.sizeRestrictions else { return }
        let size: CGSize
        if spec == "full" {
            size = ws.screen.bounds.size
        } else {
            let p = spec.lowercased().split(separator: "x").compactMap { Double($0) }
            guard p.count == 2, p[0] > 0, p[1] > 0 else { return }
            size = CGSize(width: p[0], height: p[1])
        }
        r.minimumSize = size
        r.maximumSize = size
    }

    /// Report the scene now and whenever its size, classes or state change.
    func watch(_ ws: UIWindowScene) {
        let id = ObjectIdentifier(ws)
        guard !watched.contains(id) else { return }
        watched.insert(id)
        ws.registerForTraitChanges([UITraitHorizontalSizeClass.self, UITraitVerticalSizeClass.self]) { (s: UIWindowScene, _: UITraitCollection) in
            SceneReport.shared.note(s)
        }
        for name in [UIScene.didActivateNotification, UIScene.willDeactivateNotification, UIScene.didEnterBackgroundNotification, UIScene.willEnterForegroundNotification] {
            NotificationCenter.default.addObserver(forName: name, object: ws, queue: .main) { [weak ws] _ in
                MainActor.assumeIsolated { if let ws { SceneReport.shared.note(ws) } }
            }
        }
        note(ws)
    }

    func note(_ ws: UIWindowScene) {
        StatusFile.shared.set("scene", Self.describe(ws))
    }
}

@MainActor
final class KeyboardWatch {
    static let shared = KeyboardWatch()
    private var frame: CGRect?

    func start() {
        let nc = NotificationCenter.default
        for name in [UIResponder.keyboardDidShowNotification, UIResponder.keyboardDidHideNotification, UIResponder.keyboardDidChangeFrameNotification] {
            nc.addObserver(forName: name, object: nil, queue: .main) { n in
                let end = n.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect
                let hidden = n.name == UIResponder.keyboardDidHideNotification
                MainActor.assumeIsolated { KeyboardWatch.shared.note(hidden ? nil : end) }
            }
        }
        note(nil)
    }

    func note(_ f: CGRect?) {
        frame = f
        let screen = UIApplication.shared.connectedScenes.compactMap { ($0 as? UIWindowScene)?.screen.bounds }.first ?? .zero
        // a frame wholly off the bottom of the screen is a keyboard put away
        let shown = f.map { $0.height > 0 && $0.minY < screen.maxY - 1 } ?? false
        var k: [String: JSONValue] = ["shown": .bool(shown)]
        if let f, shown { k["frame"] = .array([.double(f.minX), .double(f.minY), .double(f.width), .double(f.height)]) }
        StatusFile.shared.set("keyboard", .object(k))
        StatusFile.shared.set("firstResponder", Self.firstResponder())
    }

    /// The responder that has the keyboard's focus now: its type, and its identifier if any.
    static func firstResponder() -> JSONValue {
        FirstResponder.found = nil
        UIApplication.shared.sendAction(#selector(UIResponder.plumcraftNoteFirstResponder(_:)), to: nil, from: nil, for: nil)
        guard let r = FirstResponder.found else { return .null }
        var o: [String: JSONValue] = ["type": .string(String(describing: type(of: r)))]
        if let v = r as? UIView, let id = v.accessibilityIdentifier { o["id"] = .string(id) }
        return .object(o)
    }
}

@MainActor
enum FirstResponder {
    static weak var found: UIResponder?
}

extension UIResponder {
    /// Sent to nil, it reaches the first responder, which notes itself (`KeyboardWatch`).
    @objc func plumcraftNoteFirstResponder(_ sender: Any?) {
        MainActor.assumeIsolated { FirstResponder.found = self }
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

/// B111: every layer animation running in the app's windows and the display links the app knows
/// of (Activity's: the pan's momentum), as JSON, worked out when accessibility reads the value (a
/// UI test asks; nothing is walked otherwise). `{"layers": [{layer, owner, key, repeat}], "links":
/// [...], "windows": n}`. Display links the app didn't make (UIKit's own) can't be listed.
final class LayersElement: UIView {
    override init(frame: CGRect) {
        super.init(frame: frame)
        isAccessibilityElement = true
        accessibilityIdentifier = "plumcraft-layers"
        accessibilityLabel = "layers"
    }

    required init?(coder: NSCoder) { nil }

    override var accessibilityValue: String? {
        get { Self.report() }
        set {}
    }

    static func report() -> String {
        var found: [JSONValue] = []
        var windows = 0
        func walk(_ l: CALayer, depth: Int) {
            guard depth < 60, found.count < 60 else { return }
            for key in l.animationKeys() ?? [] {
                let a = l.animation(forKey: key)
                let owner = l.delegate.map { String(describing: type(of: $0)) } ?? "-"
                found.append(["layer": .string(String(describing: type(of: l))), "owner": .string(owner), "key": .string(key),
                              "repeat": .double(Double(a?.repeatCount ?? 0)), "duration": .double(a?.duration ?? 0)])
            }
            for s in l.sublayers ?? [] { walk(s, depth: depth + 1) }
        }
        for case let scene as UIWindowScene in UIApplication.shared.connectedScenes {
            for w in scene.windows {
                windows += 1
                walk(w.layer, depth: 0)
            }
        }
        let links = Activity.shared.names.filter { $0.contains("momentum") || $0.contains("link") }
        let o = JSONValue.object(["layers": .array(found), "links": .array(links.map(JSONValue.string)), "windows": .uint(UInt64(windows)),
                                  "animating": .array(Activity.shared.names.map(JSONValue.string))])
        return String(decoding: o.serialized(), as: UTF8.self)
    }
}

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
        // B111: beside it, what is animating as Core Animation sees it, worked out only when read
        // (where the status is read from accessibility: a device). Siblings in a plain container,
        // since an element's own subviews are hidden from accessibility
        let box = UIView(frame: CGRect(x: 0, y: 0, width: 2, height: 2))
        box.addSubview(view)
        box.addSubview(LayersElement(frame: CGRect(x: 1, y: 1, width: 1, height: 1)))
        vc.view = box
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

/// The app's bundle id, as the bundle carries it (set in one place, Config/BundleId.xcconfig);
/// the ids derived from it (activity types, menu identifiers, log subsystems) follow it.
enum AppID {
    static let bundle = Bundle.main.bundleIdentifier ?? "com.swapwizard.plummer.pad"
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
    static let logger = Logger(subsystem: AppID.bundle, category: "app")

    /// A line for the unified log and the core's (`log.write`).
    static func say(_ level: String, _ message: String) {
        if let log { log.say(level, message) } else { logger.notice("\(message, privacy: .public)") }
    }

    /// Backgrounding: the core writes what autosave hasn't yet (`lifecycle.background`), within
    /// the time the system gives (N4 slice 4, contract 4.4).
    static func flushForBackground() {
        guard let core else { return }
        let box = BackgroundTask()
        box.id = UIApplication.shared.beginBackgroundTask(withName: "Plummer autosave") {
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
        var config = ShellConfig(appID: AppID.bundle, appName: "Plummer", version: ShellConfig.bundleVersion, storeRoot: store)
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
            let l = UnifiedLog(core: c, subsystem: AppID.bundle)
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
            say("info", "Plummer started: store \(store), engines \(([options.engine] + options.also).joined(separator: ", "))")
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
        // what is animating now, for scripts and the UI tests (empty when idle; Activity)
        StatusFile.shared.set("animating", .array([]))
        Activity.shared.onChange = { names in StatusFile.shared.set("animating", .array(names.map(JSONValue.string))) }
        // B118: the on-screen keyboard and what holds the keyboard's focus, for scripts and the
        // UI tests (a keyboard up with nothing being edited covers half the canvas)
        KeyboardWatch.shared.start()
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
