// Two ways a Swift shell runs the suite (docs/shell-interface.md 16.1):
//
// - KitDriver: through the kit itself, as the app will: requests through PlumCore, the document
//   model following the feed, measurements reported typed (CanvasLink.report, plum_geometry_set),
//   the drag's move through the typed hot call (CanvasLink.dragMove, plum_gesture_move).
// - PortDriver: over the core's control port (JSON lines over TCP, `auth` first), as 16.1 says a
//   shell's test harness drives its own core, from any language.

import Foundation
import Network
import PlumCraft
import PlumCraftKit
import PlumCraftModels

/// The suite through the Swift kit, on a core whose engine is `engine`.
@MainActor
public final class KitDriver: ConformanceDriver {
    public let core: PlumCore
    public let model: DocumentModel
    public let canvas: CanvasLink
    public let engine: String

    public init(core: PlumCore, engine: String) {
        self.core = core
        self.engine = engine
        model = DocumentModel(core: core)
        canvas = CanvasLink(model: model)
        model.start()
    }

    public var name: String { "the Swift kit (\(engine) engine)" }

    public func call(_ method: String, _ params: JSONValue) async -> JSONValue {
        do {
            let r = try await core.request(method, params)
            return ["ok": true, "result": r]
        } catch let e as PlumError {
            var v: [String: JSONValue] = ["ok": false, "error": .string(e.description)]
            if case .refused(let code?) = e.code { v["code"] = .string(code) }
            return .object(v)
        } catch {
            return ["ok": false, "error": .string("\(error)")]
        }
    }

    public var reports: Bool { true }

    /// The faces by graph id, typed by nid (the scene's), through CanvasLink.report; then wait
    /// until the core says each is measured.
    public func reportGeometry(_ faces: JSONValue) async throws {
        let scene = try await core.request(Scene.self, "scene.get")
        var nids: [String: UInt64] = [:]
        for f in scene.nodes { nids[f.id] = f.nid }
        guard let list = faces.array else { throw ConformanceError("faces must be a list") }
        var typed: [FaceMeasure] = []
        for f in list {
            guard let id = f["node"].string else { throw ConformanceError("a face's node must be an id") }
            guard let nid = nids[id] else { throw ConformanceError("no face \(id) in the scene") }
            let floats: (JSONValue) -> [Float?] = { v in (v.array ?? []).map { $0.double.map(Float.init) } }
            let rows: [Rect?] = (f["rows"].array ?? []).map { r in
                guard let a = r.array, a.count == 4, let x0 = a[0].double, let y0 = a[1].double, let x1 = a[2].double, let y1 = a[3].double else { return nil }
                return Rect(x0, y0, x1, y1)
            }
            typed.append(FaceMeasure(face: nid, width: Float(f["size"][0].double ?? 0), height: Float(f["size"][1].double ?? 0),
                                     header: f["header"].double.map(Float.init),
                                     inputs: floats(f["pins"]["in"]), outputs: floats(f["pins"]["out"]), rows: rows))
        }
        try canvas.report(typed)
        // the header's parts after it, as the canvas sends them (the typed call has none)
        let parts: [(id: String, parts: [String: Rect])] = list.compactMap { f in
            guard let id = f["node"].string, let o = f["parts"].object else { return nil }
            var m: [String: Rect] = [:]
            for (k, v) in o {
                if let a = v.array, a.count == 4, let x0 = a[0].double, let y0 = a[1].double, let x1 = a[2].double, let y1 = a[3].double { m[k] = Rect(x0, y0, x1, y1) }
            }
            return (id: id, parts: m)
        }
        await CanvasLink.reportParts(core: core, doc: nil, faces: parts)
        // the core has them once a request after them sees them measured (they apply in order)
        let deadline = ContinuousClock.now + .seconds(20)
        for f in list {
            while true {
                let r = await call("geometry.locate", ["node": f["node"]])
                if r["result"]["measured"] == true { break }
                if ContinuousClock.now > deadline { throw ConformanceError("never measured: \(f["node"])") }
                await pause()
            }
        }
    }

    public func settle() async {}

    public func hotMove(at: (Float, Float), slop: Float) -> JSONValue? {
        let d = canvas.dragMove(Point(Double(at.0), Double(at.1)), slop: Double(slop))
        if !d.ok { return ["status": .int(Int64(d.status))] }
        let kinds = ["none", "node", "pin", "wire", "bend", "tag", "frame", "group"]
        let t = d.target
        let tk: Int = switch t.kind {
        case .none: 0
        case .node: 1
        case .pin: 2
        case .wire: 3
        case .bend: 4
        case .tag: 5
        case .frame: 6
        case .group: 7
        case .unknown: -1
        }
        var v: [String: JSONValue] = [
            "status": .int(Int64(d.status)), "kind": .int(Int64(d.rawKind)), "serial": .uint(d.serial),
            "dx": .double(Double(d.dx)), "dy": .double(Double(d.dy)),
            "target": ["kind": .string(tk >= 0 ? kinds[tk] : "?"), "nid": .uint(t.node), "pin": .int(Int64(t.pin)),
                       "fits": .int(Int64(t.fits)), "at": [.double(Double(t.x)), .double(Double(t.y))]],
        ]
        if d.kind == .marquee {
            v["rect"] = [.double(Double(d.rect.0)), .double(Double(d.rect.1)), .double(Double(d.rect.2)), .double(Double(d.rect.3))]
        }
        if let value = d.scrubValue { v["value"] = .double(value) }
        return .object(v)
    }

    /// Its engine, and `media.get` (the C ABI serves it).
    public func has(_ capability: String) -> Bool {
        capability == "media" || capability == "engine:\(engine)"
    }

    /// The next feed batch the model sees, or 50 ms.
    public func pause() async {
        await model.nextChange(within: .milliseconds(50))
    }
}

/// The suite over a core's control port: a JSON line out, a JSON line back.
///
/// Two uses. A headless core (the kit's tests): the suite gives the measurements
/// (`reports: true`). A running app whose canvas measures its own faces (N3 slice 3): `reports:
/// false`, so the suite's `geometry` steps are skipped and `settle` waits until the app has
/// measured every face the scene has (`geometry.locate`'s `measured`); `doc` names the app's
/// document, and `engines` what the app's core has connected.
@MainActor
public final class PortDriver: ConformanceDriver {
    private let conn: LineConnection
    private var next: UInt64 = 0
    /// The core whose port it is (to wait on its feed between tries), if this process has it.
    private let model: DocumentModel?
    public let reports: Bool
    /// The document every request names (nil: the core's first).
    public let doc: UInt64?
    private let engines: Set<String>
    /// How long `settle` waits for the app to measure.
    public var settleLimit: Duration = .seconds(10)
    /// Settles that ran out of time (the app never measured every face), for the report.
    public private(set) var settleTimeouts = 0

    /// Where it connects, and the token it was given (a second connection checks the port asks
    /// for it: `authRefused`, N4 slice 4).
    public let host: String
    public let port: UInt16
    private let token: String
    /// The app's core keeps its preferences in a file (`settings.get {fromDisk}` says so).
    public let prefs: Bool

    /// Connect to `host:port` (loopback by default; a device's debug build on the local network,
    /// N4 slice 4) and authenticate with `token`.
    public init(host: String = "127.0.0.1", port: UInt16, token: String, following model: DocumentModel? = nil, reports: Bool = true,
                doc: UInt64? = nil, engines: Set<String> = ["text"], prefs: Bool = false) async throws {
        conn = try await LineConnection(host: host, port: port)
        self.host = host
        self.port = port
        self.token = token
        self.prefs = prefs
        self.model = model
        self.reports = reports
        self.doc = doc
        self.engines = engines
        let auth = try await conn.send(["id": "auth", "method": "auth", "params": ["token": .string(token)]])
        guard auth["ok"] == true else { throw ConformanceError("auth: \(auth)") }
    }

    public var name: String { reports ? "the core's control port" : "an app's control port, measuring its own faces" }

    public func call(_ method: String, _ params: JSONValue) async -> JSONValue {
        next += 1
        var req: [String: JSONValue] = ["id": .uint(next), "method": .string(method), "params": params]
        if let doc { req["doc"] = .uint(doc) }
        do {
            return try await conn.send(.object(req))
        } catch {
            return ["ok": false, "error": .string("\(error)")]
        }
    }

    /// A running app measures its own faces; with `suiteGeometry` (the default since N5 slice 4)
    /// the suite's `geometry` steps still run: the suite's measurements are reported over the
    /// app's once the app has measured those faces, and held until the core has them (the app
    /// reports a face again only when it changes).
    public var suiteGeometry = true

    public func reportGeometry(_ faces: JSONValue) async throws {
        let list = faces.array ?? []
        if !reports {
            // the app's own report first, so the suite's comes after it
            let until = ContinuousClock.now + settleLimit
            for f in list {
                while await call("geometry.locate", ["node": f["node"]])["result"]["measured"] != true, ContinuousClock.now < until {
                    try? await Task.sleep(for: .milliseconds(10))
                }
            }
        }
        let r = await call("command.run", ["command": "geometry.set", "params": ["faces": faces]])
        if r["ok"] != true { throw ConformanceError("geometry.set: \(r)") }
        if reports { return }
        // held: each face's rect is the suite's size (one the app laid out again meanwhile is reported again)
        let deadline = ContinuousClock.now + settleLimit
        var round = 0
        while true {
            var all = true
            for f in list {
                let rect = await call("geometry.locate", ["node": f["node"]])["result"]["rect"].array ?? []
                let size = f["size"].array ?? []
                guard rect.count == 4, size.count == 2, let x0 = rect[0].double, let y0 = rect[1].double, let x1 = rect[2].double, let y1 = rect[3].double,
                      let w = size[0].double, let h = size[1].double, abs(x1 - x0 - w) <= 1e-3, abs(y1 - y0 - h) <= 1e-3
                else {
                    all = false
                    break
                }
            }
            if all { return }
            if ContinuousClock.now > deadline { throw ConformanceError("the app's own measurements kept replacing the suite's") }
            try? await Task.sleep(for: .milliseconds(20))
            round += 1
            if round % 10 == 0 {
                let again = await call("command.run", ["command": "geometry.set", "params": ["faces": faces]])
                if again["ok"] != true { throw ConformanceError("geometry.set: \(again)") }
            }
        }
    }

    /// A headless core: nothing to wait for. An app: until it has measured every face in the
    /// scene, waiting on that condition (never a fixed time), at most `settleLimit`.
    public func settle() async {
        if reports { return }
        let deadline = ContinuousClock.now + settleLimit
        while ContinuousClock.now < deadline {
            if await everyFaceMeasured() { return }
            try? await Task.sleep(for: .milliseconds(10))
        }
        settleTimeouts += 1
    }

    func everyFaceMeasured() async -> Bool {
        let scene = await call("scene.get", [:])
        guard scene["ok"] == true else { return false }
        for f in scene["result"]["nodes"].array ?? [] {
            let r = await call("geometry.locate", ["node": f["id"]])
            if r["result"]["measured"] != true { return false }
        }
        return true
    }

    /// The typed move's JSON form over the port (`geometry.dragMove`, N5 slice 4).
    public func hotMove(at: (Float, Float), slop: Float) async -> JSONValue? {
        let r = await call("geometry.dragMove", ["at": [.double(Double(at.0)), .double(Double(at.1))], "slop": .double(Double(slop))])
        return r["ok"] == true ? r["result"] : ["status": -1, "error": r["error"]]
    }

    /// The engines the core has; `media.get` (a C ABI core's port serves it since N5 slice 4);
    /// an app's canvas measures its rows' widgets and header parts (`rows`), and with
    /// `suiteGeometry` the suite's measurements are reported over its own (`reports`).
    public func has(_ capability: String) -> Bool {
        if capability == "rows" { return !reports }
        if capability == "reports" { return !reports && suiteGeometry }
        if capability == "media" { return true }
        if capability == "port" { return true }
        if capability == "prefs" { return prefs }
        return capability.hasPrefix("engine:") && engines.contains(String(capability.dropFirst(7)))
    }

    /// The core's next feed batch when this process has the core, else 20 ms.
    public func pause() async {
        if let model {
            await model.nextChange(within: .milliseconds(50))
        } else {
            try? await Task.sleep(for: .milliseconds(20))
        }
    }

    public func close() { conn.close() }

    /// The port asks for its token (N4 slice 4): a second connection with a wrong token is
    /// refused, and so is a request sent before authenticating.
    public func authRefused() async throws {
        let wrong = String(token.reversed().map { $0 == "0" ? "1" : "0" })
        let a = try await LineConnection(host: host, port: port)
        defer { a.close() }
        let r = try await a.send(["id": 1, "method": "auth", "params": ["token": .string(wrong)]])
        guard r["ok"] == false, r["error"].string == "authentication required" else { throw ConformanceError("a wrong token was taken: \(r)") }
        let b = try await LineConnection(host: host, port: port)
        defer { b.close() }
        let g = try await b.send(["id": 2, "method": "graph.get", "params": [:]])
        guard g["ok"] == false, g["error"].string == "authentication required" else { throw ConformanceError("a request before auth was served: \(g)") }
    }
}

/// JSON lines over a loopback TCP connection (Network.framework).
final class LineConnection: Sendable {
    private let conn: NWConnection
    private let queue = DispatchQueue(label: "plumcraft-control-client")
    private let buffer = Buffer()

    final class Buffer: @unchecked Sendable {
        // touched only on `queue`'s callbacks and the awaiting task in turn (one request at a time)
        var data = Data()
    }

    init(host: String = "127.0.0.1", port: UInt16) async throws {
        guard let p = NWEndpoint.Port(rawValue: port) else { throw ConformanceError("bad port \(port)") }
        conn = NWConnection(host: NWEndpoint.Host(host), port: p, using: .tcp)
        let c = conn
        let q = queue
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Void, Error>) in
            let once = Once()
            c.stateUpdateHandler = { state in
                switch state {
                case .ready: if once.take() { k.resume() }
                case .failed(let e): if once.take() { k.resume(throwing: e) }
                case .cancelled: if once.take() { k.resume(throwing: ConformanceError("cancelled")) }
                default: break
                }
            }
            c.start(queue: q)
        }
    }

    func close() { conn.cancel() }

    /// Send one request and read one line back.
    func send(_ v: JSONValue) async throws -> JSONValue {
        var line = v.serialized()
        line.append(0x0A)
        let c = conn
        try await withCheckedThrowingContinuation { (k: CheckedContinuation<Void, Error>) in
            c.send(content: line, completion: .contentProcessed { e in
                if let e { k.resume(throwing: e) } else { k.resume() }
            })
        }
        while true {
            if let nl = buffer.data.firstIndex(of: 0x0A) {
                let text = buffer.data[buffer.data.startIndex..<nl]
                buffer.data.removeSubrange(buffer.data.startIndex...nl)
                do {
                    return try JSONValue.parse(Data(text))
                } catch {
                    return ["ok": false, "error": .string("not JSON: \(error)")]
                }
            }
            let chunk: Data = try await withCheckedThrowingContinuation { (k: CheckedContinuation<Data, Error>) in
                c.receive(minimumIncompleteLength: 1, maximumLength: 1 << 20) { data, _, done, e in
                    if let e {
                        k.resume(throwing: e)
                    } else if let data, !data.isEmpty {
                        k.resume(returning: data)
                    } else if done {
                        k.resume(throwing: ConformanceError("the port closed"))
                    } else {
                        k.resume(returning: Data())
                    }
                }
            }
            buffer.data.append(chunk)
        }
    }
}

/// True the first time only, from any thread.
final class Once: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false
    func take() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if done { return false }
        done = true
        return true
    }
}
