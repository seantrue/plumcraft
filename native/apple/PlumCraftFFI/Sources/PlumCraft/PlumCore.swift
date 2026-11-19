// A thin Swift face on plumcraft-ffi's C ABI (crates/ffi/include/plumcraft.h).
//
// - PlumCore is @MainActor: requests are async and resume on the main actor.
// - The C wake is a plain @convention(c) function at file scope (nonisolated): it only yields
//   to an AsyncStream. Nothing main-actor-isolated ever runs on the core's thread, which is the
//   Swift 6 trap the native virtimp.com app hit (a main-actor closure called on a foreign queue).
// - The same care for Data's deallocators: they are made in nonisolated functions, because they
//   run on whatever thread frees the Data.
// - Blob wraps lent bytes as Data(bytesNoCopy:) and releases them when the Data goes.

import Foundation
import PlumCraftFFI

public struct PlumError: Error, CustomStringConvertible, Sendable {
    public let description: String
    public init(_ d: String) { description = d }
}

/// The wake handed to the C side. It must only schedule: it yields to the core's stream.
final class WakeBox: Sendable {
    let continuation: AsyncStream<Void>.Continuation
    init(_ c: AsyncStream<Void>.Continuation) { continuation = c }
}

/// Called on the core's thread. Plain C, no isolation, no payload.
private func plumWake(_ ctx: UnsafeMutableRawPointer?) {
    guard let ctx else { return }
    Unmanaged<WakeBox>.fromOpaque(ctx).takeUnretainedValue().continuation.yield()
}

/// Bytes lent by the core, without a copy: `data` reads them in place until it is freed.
public struct Blob: Sendable {
    public let data: Data
    /// The lent pointer, as a number (for checks that nothing was copied).
    public let address: UInt
    public let token: UInt64
}

/// A buffer from plum_poll as Data, without a copy; freed with plum_free when the Data goes.
nonisolated func takeBuffer(_ buf: plum_buf) -> Data? {
    guard let ptr = buf.ptr, buf.len > 0 else { return nil }
    let cap = buf.cap
    return Data(bytesNoCopy: ptr, count: buf.len, deallocator: .custom { p, n in
        plum_free(plum_buf(ptr: p.assumingMemoryBound(to: UInt8.self), len: n, cap: cap))
    })
}

/// Lent bytes as Data, released when the Data goes.
nonisolated func lentData(_ b: plum_bytes) -> Blob {
    let token = b.token
    let ptr = UnsafeMutableRawPointer(mutating: b.ptr!)
    let data = Data(bytesNoCopy: ptr, count: b.len, deallocator: .custom { _, _ in _ = plum_release(token) })
    return Blob(data: data, address: UInt(bitPattern: ptr), token: token)
}

/// A parsed reply crossing from drain() to the awaiting request. JSONSerialization's objects are
/// handed over, not shared (drain keeps no reference), hence unchecked.
struct Reply: @unchecked Sendable { let value: Any }

@MainActor
public final class PlumCore {
    public let id: plum_core
    private let wakeBox: WakeBox
    private var pending: [UInt64: CheckedContinuation<Reply, Error>] = [:]
    private var pump: Task<Void, Never>?
    /// Every engine event envelope seen, parsed.
    public private(set) var events: [[String: Any]] = []
    /// Wakes the main actor saw (after the stream's own coalescing), polls that returned
    /// something, and events per such poll.
    public private(set) var wakes = 0
    public private(set) var polls = 0
    public private(set) var eventsPerPoll: [Int] = []
    /// Called with each poll's raw bytes before they are parsed (measurements).
    public var onPoll: ((Data) -> Void)?
    /// Called with each poll's events.
    public var onEvents: (([[String: Any]]) -> Void)?

    public init(config: [String: Any] = [:]) throws {
        let (stream, continuation) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        wakeBox = WakeBox(continuation)
        let json = try JSONSerialization.data(withJSONObject: config)
        let ctx = Unmanaged.passUnretained(wakeBox).toOpaque()
        id = json.withUnsafeBytes { raw in
            plum_open(raw.bindMemory(to: UInt8.self).baseAddress, raw.count, plumWake, ctx)
        }
        if id == 0 {
            let why = takeBuffer(plum_last_error()).flatMap { String(data: $0, encoding: .utf8) } ?? "?"
            throw PlumError("plum_open: \(why)")
        }
        pump = Task { @MainActor [weak self] in
            for await _ in stream {
                guard let self else { return }
                self.wakes += 1
                self.drain()
            }
        }
    }

    /// Close the core (the C side waits for its thread). Requests still pending fail.
    public func close() {
        _ = plum_close(id)
        pump?.cancel()
        wakeBox.continuation.finish()
        for (_, c) in pending { c.resume(throwing: PlumError("closed")) }
        pending = [:]
    }

    /// Queue a request without waiting: its number.
    @discardableResult
    public func send(_ json: Data) -> UInt64 {
        json.withUnsafeBytes { raw in plum_request(id, raw.bindMemory(to: UInt8.self).baseAddress, raw.count) }
    }

    /// A request, `{id?, method, params?}` as JSON; the reply's result as JSON.
    public func request(_ json: Data) async throws -> Data {
        let result = try await requestObject(json)
        return try JSONSerialization.data(withJSONObject: result, options: [.fragmentsAllowed])
    }

    /// A request; the reply's result, parsed (no second encoding).
    public func requestObject(_ json: Data) async throws -> Any {
        let rid = send(json)
        if rid == 0 { throw PlumError("the core is closed") }
        let reply = try await withCheckedThrowingContinuation { c in
            pending[rid] = c
            drain()  // the reply may already be there
        }
        return reply.value
    }

    public func call(_ method: String, _ params: [String: Any] = [:]) async throws -> Any {
        try await requestObject(JSONSerialization.data(withJSONObject: ["method": method, "params": params]))
    }

    /// `command.run`.
    public func command(_ id: String, _ params: [String: Any] = [:]) async throws -> Any {
        try await call("command.run", ["command": id, "params": params])
    }

    /// Poll until nothing is left: replies to their requests, events to `events`.
    public func drain() {
        while let data = takeBuffer(plum_poll(id)) {
            polls += 1
            onPoll?(data)
            guard let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
            for r in obj["replies"] as? [[String: Any]] ?? [] {
                guard let rid = (r["id"] as? NSNumber)?.uint64Value, let c = pending.removeValue(forKey: rid) else { continue }
                if (r["ok"] as? Bool) == true {
                    c.resume(returning: Reply(value: r["result"] ?? NSNull()))
                } else {
                    c.resume(throwing: PlumError(r["error"] as? String ?? "error"))
                }
            }
            let evs = obj["events"] as? [[String: Any]] ?? []
            if !evs.isEmpty {
                eventsPerPoll.append(evs.count)
                events.append(contentsOf: evs)
                onEvents?(evs)
            }
        }
    }

    /// Borrow a store blob's bytes by handle.
    public func blob(_ handle: String) throws -> Blob {
        var h = Array(handle.utf8)
        let b = h.withUnsafeMutableBufferPointer { plum_blob(id, $0.baseAddress, $0.count) }
        guard b.status == PLUM_OK, b.ptr != nil else { throw PlumError("plum_blob \(handle): status \(b.status)") }
        return lentData(b)
    }

    public static var leases: UInt64 { plum_leases() }
}
