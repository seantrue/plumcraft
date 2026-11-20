// PlumCraft for the Mac: a document-based AppKit app. One core per process (the Swift kit's
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
//   --scratch-state               don't restore or save windows (scripts: the Mac's saved state is
//                                 left alone; `-ApplePersistenceIgnoreState YES` would do it too,
//                                 but AppKit then offers the `YES` as a file to open)
//   --bench OUT --nodes N --wires M [--phase-seconds S]   the scripted budget run (Bench.swift)

import AppKit
import PlumCraft
import PlumCraftModels

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
            case "--scratch-state": break
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

/// The process's one core, opened at launch and closed at quit.
@MainActor
enum CoreHost {
    static var core: PlumCore?
    static var options = Options()
    static var openError: String?
    /// Done once the core's own first document is closed: windows open theirs after it.
    static var ready: Task<Void, Never>?

    static func open() {
        let fm = FileManager.default
        let caches = (fm.urls(for: .cachesDirectory, in: .userDomainMask).first ?? fm.temporaryDirectory)
            .appendingPathComponent(Bundle.main.bundleIdentifier ?? "com.plumcraft.mac")
        let store = options.storeRoot ?? caches.appendingPathComponent("store").path
        try? fm.createDirectory(atPath: store, withIntermediateDirectories: true)
        let logs = (fm.urls(for: .libraryDirectory, in: .userDomainMask).first ?? fm.temporaryDirectory)
            .appendingPathComponent("Logs/PlumCraft").path
        var config: [String: JSONValue] = [
            "shell": 1,
            "app": ["id": .string(Bundle.main.bundleIdentifier ?? "com.plumcraft.mac"), "name": "PlumCraft", "version": "0.7.0"],
            "store_root": .string(store),
            "dirs": ["caches": .string(caches.path), "store": .string(store), "logs": .string(logs)],
            "platform": ["os": "macos", "subprocess": true, "craft_apps": true, "python": true, "multi_window": true,
                         "pointer": true, "keyboard": true, "hover": true],
            "display": ["scale": .double(Double(NSScreen.main?.backingScaleFactor ?? 2)), "frame_ms": 8.3],
            "engines": ["default": .string(options.engine), "mode": "inproc"],
        ]
        if let tf = options.controlTokenFile {
            config["control"] = ["port": 0, "token_file": .string(tf)]
        }
        do {
            let c = try PlumCore(config: .object(config))
            core = c
            // the core opens a first, untitled document of its own; this app opens one per window,
            // so that one is closed
            ready = Task {
                if let list = try? await c.request("document.list"), let docs = list["docs"].array {
                    for d in docs {
                        if let id = d["doc"].uint64 {
                            _ = try? await c.request("document.close", ["discard": true], doc: id)
                        }
                    }
                }
                if let port = try? await c.request("core.control")["port"].uint64 {
                    Status.shared.set("port", .uint(port))
                }
            }
        } catch {
            openError = "\(error)"
        }
    }
}

/// What the app writes to `--status-file` for the scripts that drive it (they wait on it with a
/// time limit, never on a process name).
@MainActor
final class Status {
    static let shared = Status()
    private var state: [String: JSONValue] = ["pid": .int(Int64(ProcessInfo.processInfo.processIdentifier))]
    private var windows: [UInt64: JSONValue] = [:]

    func set(_ key: String, _ v: JSONValue) {
        state[key] = v
        write()
    }

    func window(_ doc: UInt64, _ v: JSONValue) {
        windows[doc] = v
        write()
    }

    func removeWindow(_ doc: UInt64) {
        windows[doc] = nil
        write()
    }

    private func write() {
        guard let path = CoreHost.options.statusFile else { return }
        var s = state
        s["windows"] = .array(windows.keys.sorted().compactMap { windows[$0] })
        try? JSONValue.object(s).serialized().write(to: URL(fileURLWithPath: path), options: .atomic)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    var sigterm: DispatchSourceSignal?
    var bench: BenchWindow?

    func applicationWillFinishLaunching(_ note: Notification) {
        CoreHost.options = Options.parse(CommandLine.arguments)
        switch CoreHost.options.appearance {
        case "dark"?: NSApp.appearance = NSAppearance(named: .darkAqua)
        case "light"?: NSApp.appearance = NSAppearance(named: .aqua)
        default: break
        }
        NSApp.mainMenu = MainMenu.make()
        CoreHost.open()
        // a TERM (a script's quit) quits as ⌘Q does
        signal(SIGTERM, SIG_IGN)
        let s = DispatchSource.makeSignalSource(signal: SIGTERM, queue: .main)
        s.setEventHandler { NSApp.terminate(nil) }
        s.resume()
        sigterm = s
    }

    func applicationDidFinishLaunching(_ note: Notification) {
        if let why = CoreHost.openError {
            let a = NSAlert()
            a.messageText = "PlumCraft couldn't start its core"
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
                if let error { NSApp.presentError(error) }
            }
        }
        if o.paths.isEmpty && o.openPanel && NSDocumentController.shared.documents.isEmpty {
            NSDocumentController.shared.openDocument(nil)
        }
        NSApp.activate()
    }

    func applicationShouldOpenUntitledFile(_ sender: NSApplication) -> Bool { false }
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
        if CommandLine.arguments.contains("--scratch-state") {
            var args = UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)
            args["ApplePersistenceIgnoreState"] = true
            args["NSQuitAlwaysKeepsWindows"] = false
            UserDefaults.standard.setVolatileDomain(args, forName: UserDefaults.argumentDomain)
        }
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}
