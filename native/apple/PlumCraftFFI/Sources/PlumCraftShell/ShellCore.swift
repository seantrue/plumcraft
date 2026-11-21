// What every Apple app on the kit does to start (N3 slice 5, moved out of the Mac app so the iPad
// app shares it): the core's config (contract 4.1) built from what the app knows of its platform,
// the one core opened, and the core's own first, untitled document closed, since these apps open
// one core document per window or scene (decision 8).

import Foundation
import PlumCraft
import PlumCraftModels

/// The core's config, as an app on the kit fills it in (contract 4.1).
public struct ShellConfig: Sendable {
    public var appID: String
    public var appName: String
    public var version: String
    /// The store's root (also `dirs.store`).
    public var storeRoot: String
    /// `dirs.*` besides the store (`documents`, `caches`, `scratch`, `prefs`, `logs`).
    public var dirs: [String: String] = [:]
    /// `platform` (contract 4.2), as the app declares it.
    public var platform: [String: JSONValue] = [:]
    public var displayScale: Double = 2
    public var frameMs: Double = 8.3
    /// `engines.default` and `engines.also`.
    public var engine = "text"
    public var also: [String] = []
    /// `engines.auto`: each document starts only the engines its graph uses, and starts another
    /// when the graph comes to use it (decision 8C at N3's exit). False: `engine` and `also` in
    /// every document.
    public var pickEngines = true
    /// The control port's token file: the port opens (loopback, port 0) only when set.
    public var controlTokenFile: String?
    /// `log.level`, when the app wants other than the core's default.
    public var logLevel: String?
    /// `dirs.prefs` is in `dirs`; the system's appearance at start (`display.dark`, …).
    public var dark = false
    public var moreContrast = false
    public var reduceMotion = false
    public var reduceTransparency = false
    #if DEBUG
    /// A debug build only (N4 slice 4, decision 10): the control port on the local network
    /// (`lan` or an address), with a pre-shared token from the launch arguments. A release build
    /// has neither field, so it can never ask the core for more than loopback.
    public var controlBind: String?
    public var controlToken: String?
    /// The port it listens on there (0 picks one; a device's scripts want a known one).
    public var controlPort: UInt16 = 0
    #endif

    public init(appID: String, appName: String, version: String, storeRoot: String) {
        self.appID = appID; self.appName = appName; self.version = version; self.storeRoot = storeRoot
    }

    /// The JSON the core takes.
    public var json: JSONValue {
        var d: [String: JSONValue] = ["store": .string(storeRoot)]
        for (k, v) in dirs { d[k] = .string(v) }
        var c: [String: JSONValue] = [
            "shell": 1,
            "app": ["id": .string(appID), "name": .string(appName), "version": .string(version)],
            "store_root": .string(storeRoot),
            "dirs": .object(d),
            "platform": .object(platform),
            "display": ["scale": .double(displayScale), "frame_ms": .double(frameMs), "dark": .bool(dark),
                        "contrast": moreContrast ? "more" : "normal", "reduce_motion": .bool(reduceMotion),
                        "reduce_transparency": .bool(reduceTransparency)],
            "engines": ["default": .string(engine), "mode": "inproc", "also": .array(also.filter { $0 != engine }.map { .string($0) }),
                        "auto": .bool(pickEngines)],
        ]
        if let tf = controlTokenFile { c["control"] = ["port": 0, "token_file": .string(tf)] }
        #if DEBUG
        if let bind = controlBind, bind != "loopback", let token = controlToken {
            // no token file: the token is the one the launch gave, never printed
            c["control"] = ["port": .uint(UInt64(controlPort)), "bind": .string(bind), "token": .string(token), "network_debug": true]
        }
        #endif
        if let l = logLevel { c["log"] = ["level": .string(l)] }
        return .object(c)
    }
}

/// The process's one core, opened at launch.
public enum ShellCore {
    /// Open the core on `config`. The returned task finishes once the core's own first document
    /// is closed (windows open theirs after it), with the control port when one is open.
    public static func open(_ config: ShellConfig) throws -> (PlumCore, Task<UInt64?, Never>) {
        let c = try PlumCore(config: config.json)
        let ready = Task { () -> UInt64? in
            if let list = try? await c.request("document.list"), let docs = list["docs"].array {
                for d in docs {
                    if let id = d["doc"].uint64 {
                        _ = try? await c.request("document.close", ["discard": true], doc: id)
                    }
                }
            }
            return try? await c.request("core.control")["port"].uint64
        }
        return (c, ready)
    }
}

/// What an app writes to `--status-file` for the scripts that drive it: they wait on it with a
/// time limit, never on a process name. Written atomically on every change.
@MainActor
public final class StatusFile {
    public static let shared = StatusFile()
    /// Where it is written; nil: nowhere.
    public var path: String?
    /// Also handed, whole, to this (the iPad's debug status element, which a UI test on a device
    /// reads through accessibility: its runner can't see the app's files there).
    public var mirror: ((String) -> Void)?
    /// Whether anything reads it.
    public var active: Bool { path != nil || mirror != nil }
    private var state: [String: JSONValue] = ["pid": .int(Int64(ProcessInfo.processInfo.processIdentifier))]
    private var windows: [UInt64: JSONValue] = [:]

    public init(path: String? = nil) { self.path = path }

    public func set(_ key: String, _ v: JSONValue) {
        state[key] = v
        write()
    }

    public func window(_ doc: UInt64, _ v: JSONValue) {
        windows[doc] = v
        write()
    }

    public func removeWindow(_ doc: UInt64) {
        windows[doc] = nil
        write()
    }

    private func write() {
        guard active else { return }
        var s = state
        s["windows"] = .array(windows.keys.sorted().compactMap { windows[$0] })
        let data = JSONValue.object(s).serialized()
        if let path { try? data.write(to: URL(fileURLWithPath: path), options: .atomic) }
        if let mirror { mirror(String(decoding: data, as: UTF8.self)) }
    }

    /// Append this process's pid to `<path>.pids` (a tracer may start more than one copy), so a
    /// script can quit each by its PID.
    public func notePid() {
        guard let path else { return }
        let f = path + ".pids"
        if !FileManager.default.fileExists(atPath: f) { FileManager.default.createFile(atPath: f, contents: nil) }
        guard let h = FileHandle(forWritingAtPath: f) else { return }
        h.seekToEndOfFile()
        h.write(Data("\(ProcessInfo.processInfo.processIdentifier)\n".utf8))
        try? h.close()
    }
}
