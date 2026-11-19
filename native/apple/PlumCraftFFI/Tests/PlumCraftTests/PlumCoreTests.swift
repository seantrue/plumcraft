// The spike's sequence through the xcframework, as a Swift shell would drive it, then the
// measurements (each prints one `MEASURE {json}` line).

import Foundation
import XCTest
import PlumCraft

/// A store root under the scratch root the Rust tests sweep (`plumcraft-test-<pid>`).
func storeRoot(_ name: String) -> String {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("plumcraft-test-\(ProcessInfo.processInfo.processIdentifier)")
        .appendingPathComponent("swift-\(name)")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.path
}

func residentKB() -> UInt64 {
    var info = mach_task_basic_info()
    var count = mach_msg_type_number_t(MemoryLayout<mach_task_basic_info>.size / MemoryLayout<natural_t>.size)
    let kr = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(MACH_TASK_BASIC_INFO), $0, &count) }
    }
    return kr == KERN_SUCCESS ? info.resident_size / 1024 : 0
}

func now() -> UInt64 { DispatchTime.now().uptimeNanoseconds }

func summary(_ us: [Double]) -> [String: Any] {
    let s = us.sorted()
    let at = { (q: Double) in s[Int((Double(s.count - 1) * q).rounded())] }
    return ["n": s.count, "median_us": at(0.5), "p99_us": at(0.99), "min_us": s.first!, "max_us": s.last!]
}

func report(_ name: String, _ v: [String: Any]) {
    let d = try! JSONSerialization.data(withJSONObject: ["name": name, "from": "swift", "v": v], options: [.sortedKeys])
    print("MEASURE " + String(data: d, encoding: .utf8)!)
}

func str(_ v: Any, _ key: String) -> String { (v as! [String: Any])[key] as! String }

/// The graph Codable sees: what a typed shell would decode.
struct WireGraph: Decodable {
    struct Node: Decodable { let kind: String; let pos: [Double] }
    struct End: Decodable { let node: String; let port: String }
    struct Wire: Decodable { let from: End; let to: End }
    let nodes: [String: Node]
    let wires: [Wire]
}
struct WirePoll: Decodable {
    struct Reply: Decodable { let id: UInt64; let ok: Bool; let result: WireGraph? }
    let replies: [Reply]
}

@MainActor
final class PlumCoreTests: XCTestCase {
    func testOpenListBuildRunAndBorrow() async throws {
        let core = try PlumCore(config: ["store_root": storeRoot("flow")])
        defer { core.close() }
        let list = try await core.call("command.list") as! [[String: Any]]
        XCTAssertTrue(list.contains { $0["id"] as? String == "node.add" })

        let lit = str(try await core.command("node.add", ["kind": "text.literal", "pos": [0, 0]]), "id")
        _ = try await core.command("node.set", ["node": lit, "port": "text", "value": "The quick brown fox jumps over the lazy dog, then naps. 0123456\n"])
        let rep = str(try await core.command("node.add", ["kind": "text.repeat", "pos": [260, 0]]), "id")
        _ = try await core.command("node.set", ["node": rep, "port": "times", "value": 1_000_000])
        let up = str(try await core.command("node.add", ["kind": "text.upper", "pos": [260, 160]]), "id")
        _ = try await core.command("wire.connect", ["from": ["node": lit, "port": "text"], "to": ["node": rep, "port": "text"]])
        _ = try await core.command("wire.connect", ["from": ["node": lit, "port": "text"], "to": ["node": up, "port": "text"]])

        // request(_:) -> Data, the wrapper's typed-free form
        let g = try await core.request(JSONSerialization.data(withJSONObject: ["method": "graph.get"]))
        let graph = try JSONDecoder().decode(WireGraph.self, from: g)
        XCTAssertEqual(graph.nodes.count, 3)
        XCTAssertEqual(graph.wires.count, 2)

        _ = try await core.command("run.start", ["watch": [["node": rep, "port": "text"]]])
        let done = try await core.call("spike.waitRun", ["timeout_ms": 30_000]) as! [String: Any]
        let nodes = done["nodes"] as! [String: [String: Any]]
        XCTAssertEqual(nodes[rep]?["state"] as? String, "done")
        XCTAssertEqual(nodes[up]?["state"] as? String, "done")
        let kinds = Set(core.events.compactMap { $0["kind"] as? String })
        XCTAssertTrue(kinds.contains("node.status"), "\(kinds)")
        XCTAssertTrue(kinds.contains("run.reply"), "\(kinds)")
        XCTAssertGreaterThan(core.wakes, 0)

        let full = ((done["watch"] as! [[String: Any]])[0]["full"] as! [String: Any])
        let handle = full["handle"] as! String
        let leases = PlumCore.leases
        do {
            let blob = try core.blob(handle)
            XCTAssertEqual(blob.data.count, 64_000_000)
            XCTAssertTrue(blob.data.starts(with: Array("The quick brown fox".utf8)))
            blob.data.withUnsafeBytes { XCTAssertEqual(UInt(bitPattern: $0.baseAddress), blob.address, "Data reads the lent bytes in place") }
            XCTAssertEqual(PlumCore.leases, leases + 1)
        }
        // the Data is gone: released
        XCTAssertEqual(PlumCore.leases, leases)
    }

    func testRequestLatency() async throws {
        let core = try PlumCore(config: ["store_root": storeRoot("latency"), "demo": true])
        defer { core.close() }
        let ping = try JSONSerialization.data(withJSONObject: ["method": "ping"])
        let move = try JSONSerialization.data(withJSONObject: ["method": "command.run", "params": ["command": "node.move", "params": ["id": "shout", "pos": [10, 10]]]])
        for (name, req) in [("latency.ping", ping), ("latency.command.node.move", move)] {
            for _ in 0..<50 { _ = try await core.requestObject(req) }
            var us: [Double] = []
            for _ in 0..<2000 {
                let t0 = now()
                _ = try await core.requestObject(req)
                us.append(Double(now() - t0) / 1000)
            }
            report(name, summary(us))
        }
    }

    func testGraphGetJSONCost() async throws {
        let core = try PlumCore(config: ["store_root": storeRoot("scene")])
        defer { core.close() }
        let get = try JSONSerialization.data(withJSONObject: ["method": "graph.get"])
        for (n, w) in [(500, 800), (2000, 3200)] {
            _ = try await core.call("spike.scene", ["nodes": n, "wires": w])
            var raw = Data()
            core.onPoll = { raw = $0 }
            var e2e: [Double] = [], ser: [Double] = [], codable: [Double] = [], reencode: [Double] = []
            for i in 0..<30 {
                let t0 = now()
                let result = try await core.requestObject(get)  // poll + JSONSerialization of the whole poll
                let t1 = now()
                _ = try JSONSerialization.data(withJSONObject: result)  // what request(_:) -> Data adds
                let t2 = now()
                let typed = try JSONDecoder().decode(WirePoll.self, from: raw)
                let t3 = now()
                let t4 = now()
                _ = try JSONSerialization.jsonObject(with: raw)
                let t5 = now()
                XCTAssertEqual(typed.replies.first?.result?.nodes.count, n)
                if i >= 5 {
                    e2e.append(Double(t1 - t0) / 1000)
                    reencode.append(Double(t2 - t1) / 1000)
                    codable.append(Double(t3 - t2) / 1000)
                    ser.append(Double(t5 - t4) / 1000)
                }
            }
            core.onPoll = nil
            report("scene.\(n)", ["nodes": n, "wires": w, "bytes": raw.count,
                                  "end_to_end_jsonserialization": summary(e2e),
                                  "decode_jsonserialization": summary(ser),
                                  "decode_codable": summary(codable),
                                  "reencode_result_to_data": summary(reencode)])
        }
    }

    func testBlob64MB() async throws {
        let core = try PlumCore(config: ["store_root": storeRoot("blob")])
        defer { core.close() }
        let p = try await core.call("spike.publish", ["bytes": 64 << 20, "seed": 11]) as! [String: Any]
        let handle = p["handle"] as! String
        let rss0 = residentKB()
        let t0 = now()
        let blob = try core.blob(handle)
        let obtain = Double(now() - t0) / 1000
        let rss1 = residentKB()
        var same = false
        let t1 = now()
        var sum: UInt64 = 0
        blob.data.withUnsafeBytes { raw in
            same = UInt(bitPattern: raw.baseAddress) == blob.address
            var i = 0
            while i < raw.count { sum &+= UInt64(raw[i]); i += 16 << 10 }
        }
        let touch = Double(now() - t1) / 1000
        let rss2 = residentKB()
        let t2 = now()
        let again = try core.blob(handle)
        let second = Double(now() - t2) / 1000
        XCTAssertTrue(same)
        XCTAssertEqual(again.address, blob.address, "a second borrow shares the bytes")
        XCTAssertEqual(blob.data.count, 64 << 20)
        report("blob.64MiB.mapped", ["len": blob.data.count, "obtain_us": obtain, "second_borrow_us": second, "touch_us": touch,
                                     "data_reads_lent_pointer": same, "rss_kb": [rss0, rss1, rss2], "sum": sum])
    }

    func testWakesAndEventsDuringARun() async throws {
        for n in [100, 500] {
            let core = try PlumCore(config: ["store_root": storeRoot("run-\(n)")])
            _ = try await core.call("spike.scene", ["nodes": n, "wires": n - 1])
            _ = try await core.command("node.set", ["node": "n1", "port": "text", "value": "a corgi"])
            let (w0, p0, e0) = (core.wakes, core.polls, core.eventsPerPoll.count)
            let t0 = now()
            _ = try await core.command("run.start")
            _ = try await core.call("spike.waitRun", ["timeout_ms": 120_000])
            let secs = Double(now() - t0) / 1e9
            let per = Array(core.eventsPerPoll[e0...])
            let events = per.reduce(0, +)
            let stats = try await core.call("spike.stats") as! [String: Any]
            report("run.\(n)", ["nodes": n, "seconds": secs, "events": events, "wakes": core.wakes - w0, "polls": core.polls - p0,
                                "wakes_per_s": Double(core.wakes - w0) / secs,
                                "events_per_poll": ["mean": Double(events) / Double(max(per.count, 1)), "max": per.max() ?? 0], "core": stats])
            XCTAssertGreaterThan(events, 2 * n - 10)
            core.close()
        }
    }

    func testHostileAndClose() async throws {
        let core = try PlumCore(config: ["store_root": storeRoot("hostile")])
        for bad in [Data(), Data([0xff, 0xfe]), Data("{nope".utf8), Data("{\"method\":\"no.such\"}".utf8)] {
            do { _ = try await core.requestObject(bad); XCTFail("answered ok") } catch {}
        }
        XCTAssertThrowsError(try core.blob("b3:raw:0000"))
        let id = core.id
        core.close()
        XCTAssertEqual(plumCloseAgain(id), -1)
        XCTAssertThrowsError(try PlumCore(config: ["engine": "vectorcraft"]))
    }
}
