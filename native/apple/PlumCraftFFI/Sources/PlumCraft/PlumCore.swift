// The Swift face of plumcraft-ffi's C ABI (crates/ffi/include/plumcraft.h).
//
// Threading model (docs/research/24-native-shells.md 4.2, contract 5.2 and 5.4):
//
// - PlumCore is a Sendable class. Its only mutable state (the requests awaiting replies, the
//   feed's subscribers) sits behind one Mutex. Every method may be called from any thread or
//   actor: the C side allows it, and nothing here needs a particular one.
// - The core's wake is a plain C function (no isolation). It only yields to an AsyncStream whose
//   buffer holds one element, so the core thread never blocks on Swift and any number of wakes
//   between two polls coalesce into one.
// - One pump task (not on the main actor) waits on that stream and polls until the core has
//   nothing more. Each poll is parsed once (JSONSerialization); each reply resumes its request
//   with its result as a JSONValue; the rest of the poll goes to every subscriber as a
//   FeedBatch. So no JSON is parsed on the main actor unless the main actor asks for it.
// - `request` is nonisolated and async. The typed form decodes on the cooperative pool
//   (`@concurrent`), so a 5 MB scene never decodes on the main actor.
// - The hot calls (hitTest, wireTarget, wirePaths, geometrySet, gestureMove) are synchronous
//   and read the core's latest snapshot without waiting for the core thread (contract 9.4); a
//   canvas calls them on the main thread during a gesture.
// - Errors are thrown PlumErrors carrying the core's message; nothing here traps.

import Foundation
import PlumCraftFFI
import PlumCraftModels
import Synchronization

/// What went wrong, with the core's own words where it gave some.
public struct PlumError: Error, CustomStringConvertible, Sendable, Equatable {
    public enum Code: Sendable, Equatable {
        /// The core refused the request; `core` is its `code` (`bad_params`, `unknown_method`, …).
        case refused(core: String?)
        /// The core is closed (or closed while the request waited).
        case closed
        /// `plum_open` refused the config.
        case open
        /// A C call returned a status that isn't OK (PLUM_ERR_*).
        case status(Int32)
        /// A reply that isn't the shape asked for.
        case decode
    }

    public let code: Code
    public let description: String

    public init(_ code: Code, _ d: String) { self.code = code; description = d }
}

/// The C side's statuses, named.
public enum PlumStatus {
    public static let ok: Int32 = 0
    public static let errCore: Int32 = -1
    public static let errArg: Int32 = -2
    public static let errMissing: Int32 = -3
    public static let errPanic: Int32 = -4
    public static let errUnreadable: Int32 = -5
    public static let pending: Int32 = 1

    public static func word(_ s: Int32) -> String {
        switch s {
        case ok: "ok"
        case errCore: "no such core"
        case errArg: "a bad argument"
        case errMissing: "missing"
        case errPanic: "a panic caught at the boundary"
        case errUnreadable: "unreadable"
        case pending: "pending"
        default: "status \(s)"
        }
    }
}

/// The wake handed to the C side. It only yields: one buffered element, so wakes coalesce.
final class WakeBox: Sendable {
    let continuation: AsyncStream<Void>.Continuation
    init(_ c: AsyncStream<Void>.Continuation) { continuation = c }
}

/// Called on the core's thread with no payload. Plain C: no actor, no allocation, no lock.
private func plumWake(_ ctx: UnsafeMutableRawPointer?) {
    guard let ctx else { return }
    Unmanaged<WakeBox>.fromOpaque(ctx).takeUnretainedValue().continuation.yield()
}

/// A buffer from the C side as Data, without a copy; plum_free'd when the Data goes.
func takeBuffer(_ buf: plum_buf) -> Data? {
    guard let ptr = buf.ptr, buf.len > 0 else { return nil }
    let cap = buf.cap
    return Data(bytesNoCopy: ptr, count: buf.len, deallocator: .custom { p, n in
        plum_free(plum_buf(ptr: p.assumingMemoryBound(to: UInt8.self), len: n, cap: cap))
    })
}

/// Bytes the core lends without a copy. `data` reads them in place and gives them back
/// (`plum_release`) when the last copy of it goes; a handle names the same bytes forever, so it
/// is a texture cache's key.
public struct Blob: Sendable {
    public let data: Data
    /// The lent pointer as a number (for checks that nothing was copied).
    public let address: UInt
    public let token: UInt64
}

/// What `blob` found.
public enum BlobResult: Sendable {
    case ready(Blob)
    /// On its way: the feed's `media` says when (`FeedBatch.media`), then ask again.
    case pending
}

/// A core (`plum_open`) and its documents.
public final class PlumCore: Sendable {
    /// The C side's number for it.
    public let id: UInt64
    private let wakeBox: WakeBox
    private let shared: Shared

    /// The state the pump task shares with the core (a class: a Mutex can't be copied).
    final class Shared: Sendable {
        let state = Mutex(State())
    }

    struct State {
        var pending: [UInt64: CheckedContinuation<JSONValue, Error>] = [:]
        var subscribers: [UInt64: AsyncStream<FeedBatch>.Continuation] = [:]
        var nextSubscriber: UInt64 = 1
        var closed = false
        var polls: UInt64 = 0
        var wakes: UInt64 = 0
        var events: UInt64 = 0
    }

    /// The ABI version of the library linked (`PLUM_ABI_VERSION`).
    public static var abiVersion: UInt32 { plum_abi_version() }

    /// Blobs lent and not yet given back, process-wide.
    public static var leases: UInt64 { plum_leases() }

    /// Open a core and its first document (`docs/shell-interface.md` 4.1's config). Returns at
    /// once; the engine connects on a worker and the feed says when (`Changed` domain `engine`).
    public init(config: JSONValue = [:]) throws {
        let (stream, continuation) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let box = WakeBox(continuation)
        let json = config.serialized()
        let ctx = Unmanaged.passUnretained(box).toOpaque()
        let core = json.withUnsafeBytes { raw in
            plum_open(raw.bindMemory(to: UInt8.self).baseAddress, raw.count, plumWake, ctx)
        }
        if core == 0 {
            let why = takeBuffer(plum_last_error()).flatMap { String(data: $0, encoding: .utf8) } ?? "no reason given"
            continuation.finish()
            throw PlumError(.open, "plum_open: \(why)")
        }
        id = core
        wakeBox = box
        let shared = Shared()
        self.shared = shared
        let pumpState = PumpTarget(core: core, shared: shared)
        // not the main actor: a poll is parsed where nothing waits on it
        Task.detached(name: "plumcraft-feed") {
            for await _ in stream {
                pumpState.drain()
            }
            pumpState.finish()
        }
    }

    deinit {
        close()
    }

    /// Close the core: its documents, runs and threads (the C side waits for its thread).
    /// Requests still waiting fail with `.closed`; subscribers' streams end. A second close does
    /// nothing.
    public func close() {
        let already = shared.state.withLock { s in
            let was = s.closed
            s.closed = true
            return was
        }
        if already { return }
        _ = plum_close(id)
        wakeBox.continuation.finish()
        let (pending, subs) = shared.state.withLock { s in
            let p = s.pending
            let q = s.subscribers
            s.pending = [:]
            s.subscribers = [:]
            return (p, q)
        }
        for (_, c) in pending { c.resume(throwing: PlumError(.closed, "the core closed")) }
        for (_, c) in subs { c.finish() }
    }

    public var isClosed: Bool { shared.state.withLock { $0.closed } }

    // MARK: Requests

    /// Send a control request and wait for its reply's result. `doc` nil: the first document.
    /// Throws the core's error (`PlumError.refused`) when the reply isn't ok.
    public func request(_ method: String, _ params: JSONValue = [:], doc: UInt64? = nil) async throws -> JSONValue {
        var req: [String: JSONValue] = ["method": .string(method), "params": params]
        if let doc { req["doc"] = .uint(doc) }
        return try await send(JSONValue.object(req).serialized())
    }

    /// A request, and its result decoded as `T`, off the caller's actor.
    @concurrent
    public func request<T: Decodable & Sendable>(_ type: T.Type, _ method: String, _ params: JSONValue = [:], doc: UInt64? = nil) async throws -> T {
        let v = try await request(method, params, doc: doc)
        do {
            return try JSONValueDecoder().decode(T.self, from: v)
        } catch {
            throw PlumError(.decode, "\(method): the reply isn't a \(T.self): \(error)")
        }
    }

    /// `command.run {command, params}`.
    public func command(_ command: String, _ params: JSONValue = [:], doc: UInt64? = nil) async throws -> JSONValue {
        try await request("command.run", ["command": .string(command), "params": params], doc: doc)
    }

    /// Queue a request already in JSON (`{id?, method, params?, doc?}`) and wait for its result.
    public func send(_ json: Data) async throws -> JSONValue {
        try await withCheckedThrowingContinuation { (c: CheckedContinuation<JSONValue, Error>) in
            // Held across plum_request so the pump can't route the reply before it is awaited.
            let refused: Bool = shared.state.withLock { s in
                if s.closed { return true }
                let rid = json.withUnsafeBytes { raw in
                    plum_request(id, raw.bindMemory(to: UInt8.self).baseAddress, raw.count)
                }
                if rid == 0 { return true }
                s.pending[rid] = c
                return false
            }
            if refused { c.resume(throwing: PlumError(.closed, "the core is closed")) }
        }
    }

    /// Queue a request and don't wait (its reply is dropped). Its number, or 0 when closed.
    @discardableResult
    public func post(_ method: String, _ params: JSONValue = [:], doc: UInt64? = nil) -> UInt64 {
        var req: [String: JSONValue] = ["method": .string(method), "params": params]
        if let doc { req["doc"] = .uint(doc) }
        let json = JSONValue.object(req).serialized()
        return json.withUnsafeBytes { raw in plum_request(id, raw.bindMemory(to: UInt8.self).baseAddress, raw.count) }
    }

    // MARK: The feed

    /// Everything the core says that isn't a reply: changed domains, asks, media, engine
    /// events, log marks. Each subscriber gets every batch from when it subscribed, in order,
    /// unbuffered-dropping nothing (asks must not be lost). The stream ends when the core closes.
    public func feed() -> AsyncStream<FeedBatch> {
        let (stream, c) = AsyncStream<FeedBatch>.makeStream(bufferingPolicy: .unbounded)
        let key: UInt64? = shared.state.withLock { s in
            if s.closed { return nil }
            let k = s.nextSubscriber
            s.nextSubscriber += 1
            s.subscribers[k] = c
            return k
        }
        guard let key else {
            c.finish()
            return stream
        }
        c.onTermination = { [weak self] _ in
            _ = self?.shared.state.withLock { $0.subscribers.removeValue(forKey: key) }
        }
        return stream
    }

    /// How the feed has gone: wakes the pump saw (after coalescing), polls that gave something,
    /// engine events.
    public var feedCounts: (wakes: UInt64, polls: UInt64, events: UInt64) {
        shared.state.withLock { ($0.wakes, $0.polls, $0.events) }
    }

    // MARK: Bytes

    /// Borrow bytes by key without a copy: a store handle (`md5:raw:…`), or what `media.get`
    /// made (`derive:rgba:<handle>`). Never waits for an engine: `.pending` while they are on
    /// their way. Throws for a key that names nothing or bytes that won't decode.
    public func blob(_ key: String) throws -> BlobResult {
        var k = Array(key.utf8)
        let b = k.withUnsafeMutableBufferPointer { plum_blob(id, $0.baseAddress, $0.count) }
        if b.status == PlumStatus.pending { return .pending }
        guard b.status == PlumStatus.ok, let ptr = b.ptr else {
            throw PlumError(.status(b.status), "plum_blob \(key): \(PlumStatus.word(b.status))")
        }
        let token = b.token
        let raw = UnsafeMutableRawPointer(mutating: ptr)
        // the deallocator runs on whichever thread frees the Data: it touches nothing but the C side
        let data = Data(bytesNoCopy: raw, count: b.len, deallocator: .custom { _, _ in _ = plum_release(token) })
        return .ready(Blob(data: data, address: UInt(bitPattern: raw), token: token))
    }
}

/// What the pump task holds: the core's number and its state, not the PlumCore (so the core can
/// go away while the pump waits).
private struct PumpTarget: Sendable {
    let core: UInt64
    let shared: PlumCore.Shared

    /// Poll until the core has nothing more.
    func drain() {
        shared.state.withLock { $0.wakes += 1 }
        while let data = takeBuffer(plum_poll(core)) {
            let v = (try? JSONValue.parse(data)) ?? .null
            route(v)
        }
    }

    func route(_ v: JSONValue) {
        // replies by hand: one that won't decode still resumes its request (with an error)
        var resumes: [(CheckedContinuation<JSONValue, Error>, Result<JSONValue, Error>)] = []
        let replies = v["replies"].array ?? []
        let batch = (try? JSONValueDecoder().decode(FeedBatch.self, from: v)) ?? FeedBatch()
        let subs: [AsyncStream<FeedBatch>.Continuation] = shared.state.withLock { s in
            s.polls += 1
            s.events += UInt64(batch.events.count)
            for r in replies {
                guard let rid = r["id"].uint64, let c = s.pending.removeValue(forKey: rid) else { continue }
                if r["ok"].bool == true {
                    resumes.append((c, .success(r["result"])))
                } else {
                    let why = r["error"].string ?? "the core refused it"
                    resumes.append((c, .failure(PlumError(.refused(core: r["code"].string), why))))
                }
            }
            return batch.isQuiet ? [] : Array(s.subscribers.values)
        }
        for (c, r) in resumes { c.resume(with: r) }
        for c in subs { c.yield(batch) }
    }

    /// The wake stream ended (the core closed).
    func finish() {}
}
