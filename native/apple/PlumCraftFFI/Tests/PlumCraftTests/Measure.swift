// The plan's section 10 budgets, measured through the kit (opt-in: PLUMCRAFT_MEASURE=1; run with
// `swift test -c release`, the core being a release build already). Each prints `MEASURE {json}`.
//
//   PLUMCRAFT_MEASURE=1 swift test -c release --filter Measure

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import Synchronization
import Testing

@MainActor
func graphOf(_ n: Int, _ name: String) async throws -> PlumCore {
    let core = try openCore(name)
    let r = try await core.request("spike.scene", ["nodes": .int(Int64(n)), "wires": .int(Int64(n * 8 / 5))])
    #expect(r["nodes"].int64 == Int64(n))
    return core
}

@Suite(.serialized, .enabled(if: measuring)) @MainActor struct Measure {
    /// A one-node delta: the core's change to the Swift model updated. End to end (node.set sent
    /// until the model applied the face's new value, through the feed, `scene.get {since}`, the
    /// decode off the main actor and the apply on it), and by part.
    @Test(arguments: [500, 2000]) func oneNodeDelta(n: Int) async throws {
        let core = try await graphOf(n, "m-delta-\(n)")
        defer { core.close() }
        let model = DocumentModel(core: core)
        let t0 = nowNs()
        model.start()
        #expect(await model.wait(timeout: .seconds(60)) { model.faces.count == n })
        let first = micros(since: t0)
        let lit = try #require(model.faces.nidOf["n1"].flatMap { model.faces[$0] })
        var applyAt: UInt64 = 0
        model.onChange = { c in if c.changed.contains(lit.nid) { applyAt = nowNs() } }
        var e2e: [Double] = []
        var viaKit: [Double] = []
        var appliedFaces: [Int] = []
        // twice: the command sent around the model (it learns of the change from the feed), and
        // through it (it fetches once the command is answered)
        for kit in [false, true] {
            for i in 0..<40 {
                let value = JSONValue.string("value \(kit) \(i)")
                applyAt = 0
                var changedCount = 0
                model.onChange = { c in if c.changed.contains(lit.nid) { applyAt = nowNs(); changedCount = c.changed.count } }
                let s = nowNs()
                if kit {
                    _ = try await model.run("node.set", ["node": "n1", "port": "text", "value": value])
                } else {
                    _ = try await core.command("node.set", ["node": "n1", "port": "text", "value": value])
                }
                #expect(await model.wait { lit.item.inputs.first?.widget?.value == value })
                if i >= 5 {
                    if kit { viaKit.append(Double(applyAt - s) / 1000) } else { e2e.append(Double(applyAt - s) / 1000) }
                    appliedFaces.append(changedCount)
                }
            }
        }
        // by part, with the model's own fetching stopped
        model.stop()
        var request: [Double] = [], decode: [Double] = [], apply: [Double] = []
        var bytes = 0
        for i in 0..<40 {
            _ = try await core.command("node.set", ["node": "n1", "port": "text", "value": .string("part \(i)")])
            let a = nowNs()
            let v = try await core.request("scene.get", ["since": .uint(model.gen)])
            let b = nowNs()
            let r = try JSONValueDecoder().decode(SceneReply.self, from: v)
            let c = nowNs()
            model.apply(r)
            let d = nowNs()
            if i >= 5 {
                request.append(Double(b - a) / 1000)
                decode.append(Double(c - b) / 1000)
                apply.append(Double(d - c) / 1000)
                bytes = v.serialized().count
            }
            guard case .delta(let dd) = r, dd.upsert.nodes.count == 1 else { Issue.record("not a one-node delta: \(r.gen)"); continue }
        }
        report("kit.delta.\(n)", ["nodes": .int(Int64(n)), "first_full_scene_us": .double(first),
                                  "end_to_end_via_feed": stats(e2e), "end_to_end_via_kit_run": stats(viaKit), "items_changed": .array(Array(Set(appliedFaces)).map { .int(Int64($0)) }),
                                  "delta_bytes": .int(Int64(bytes)),
                                  "request_to_parsed_reply": stats(request), "decode": stats(decode), "apply_on_main": stats(apply)])
    }

    /// The whole scene: its bytes, parse, decode, and the apply into a model (new, and again).
    @Test(arguments: [500, 2000]) func fullScene(n: Int) async throws {
        let core = try await graphOf(n, "m-full-\(n)")
        defer { core.close() }
        var request: [Double] = [], parse: [Double] = [], decode: [Double] = [], foundation: [Double] = [], viaFoundation: [Double] = []
        var applyNew: [Double] = [], applyAgain: [Double] = []
        var bytes = 0
        for i in 0..<12 {
            let a = nowNs()
            let v = try await core.request("scene.get")
            let b = nowNs()
            let data = v.serialized()
            let c = nowNs()
            let parsed = try JSONValue.parse(data)
            let d = nowNs()
            let r = try JSONValueDecoder().decode(SceneReply.self, from: parsed)
            let e = nowNs()
            _ = try JSONDecoder().decode(SceneReply.self, from: data)
            let f = nowNs()
            _ = try JSONValue.parseWithFoundation(data)
            let f2 = nowNs()
            let model = DocumentModel(core: core)
            let g = nowNs()
            model.apply(r)
            let h = nowNs()
            model.apply(r)
            let k = nowNs()
            #expect(model.faces.count == n)
            if i >= 2 {
                request.append(Double(b - a) / 1000)
                parse.append(Double(d - c) / 1000)
                decode.append(Double(e - d) / 1000)
                foundation.append(Double(f - e) / 1000)
                viaFoundation.append(Double(f2 - f) / 1000)
                applyNew.append(Double(h - g) / 1000)
                applyAgain.append(Double(k - h) / 1000)
                bytes = data.count
            }
        }
        report("kit.full.\(n)", ["nodes": .int(Int64(n)), "bytes": .int(Int64(bytes)),
                                 "request_to_parsed_reply": stats(request), "parse": stats(parse), "parse_through_jsonserialization": stats(viaFoundation),
                                 "decode_from_parsed": stats(decode), "decode_jsondecoder_from_bytes": stats(foundation),
                                 "apply_new_model_on_main": stats(applyNew), "apply_unchanged_on_main": stats(applyAgain)])
    }

    /// The hot calls' cost from Swift: hit test, wire target, a drag's move and its wire paths.
    @Test(arguments: [500, 2000]) func hotCalls(n: Int) async throws {
        let core = try await graphOf(n, "m-hot-\(n)")
        defer { core.close() }
        let model = DocumentModel(core: core)
        model.start()
        #expect(await model.wait(timeout: .seconds(60)) { model.faces.count == n })
        let canvas = CanvasLink(model: model)
        // every face measured, typed
        let faces = model.faces.items
        let measures = faces.map { f in
            FaceMeasure(face: f.nid, rev: f.rev, width: 250, height: 160, header: 28,
                        inputs: f.inputs.indices.map { Float(40 + 22 * $0) }, outputs: f.outputs.indices.map { Float(40 + 22 * $0) })
        }
        let g0 = nowNs()
        try canvas.report(measures)
        let geometryCall = micros(since: g0)
        let last = try #require(faces.last)
        // polled as the Rust measurement polls it (the hot call itself, not the feed): past the
        // core's estimate (220 wide) and inside the measured 250, only the new geometry hits
        var located = false
        while !located && nowNs() - g0 < 30_000_000_000 {
            located = canvas.hit(Point(last.pos.x + 240, last.pos.y + 100), slop: 1).node == last.nid
        }
        let geometryToSnapshot = micros(since: g0)
        #expect(located)
        var rng = SystemRandomNumberGenerator()
        let pts = (0..<4096).map { _ in Point(Double.random(in: -100...6500, using: &rng), Double.random(in: -100...Double(n / 25 * 160 + 100), using: &rng)) }
        func perCall(_ k: Int, _ body: (Int) -> Void) -> [Double] {
            var out: [Double] = []
            for round in 0..<20 {
                let t = nowNs()
                for i in 0..<k { body(i + round * k) }
                out.append(Double(nowNs() - t) / Double(k) / 1000)
            }
            return out
        }
        var sink: UInt64 = 0
        // anywhere (often empty canvas, which looks at every wire), and on faces as the Rust
        // measurement aims (crates/ffi/tests/measure_scene.rs: a face's column and row)
        let hit = perCall(2000) { i in sink &+= canvas.hit(pts[i % pts.count], slop: 6).item }
        let onFaces = perCall(2000) { k in
            sink &+= canvas.hit(Point(Double(k % 25) * 260 + 100, Double(k / 25 % (n / 25)) * 160 + 62), slop: 12).item
        }
        // 20 faces at a time through the graph, as Rust picks them (and, below, the 20 a drag of
        // the first 20 faces by id carries: 520 wires)
        let nids = faces.map(\.nid)
        var buf: [Float] = []
        var rotating = 0
        let pathsRotating = perCall(200) { k in
            let picked = (0..<20).map { nids[(k * 20 + $0) % nids.count] }
            var c = 0
            for p in core.wirePaths(faces: picked, dx: 3, dy: 4, buffer: &buf) { c += 1; sink &+= p.nid }
            rotating = max(rotating, c)
        }
        let lit = try #require(model.face(id: "n1"))
        let target = perCall(2000) { i in sink &+= canvas.wireTarget(face: lit.nid, output: true, pin: 0, at: pts[i % pts.count], slop: 6).node }
        // a drag of a selection of 20 faces
        let picked = Array(faces.prefix(20)).map(\.id)
        _ = try await model.run("selection.set", ["nodes": .array(picked.map(JSONValue.string))])
        let first = try #require(model.face(id: picked[0]))
        let begun = try await canvas.dragBegin(Point(first.pos.x + 100, first.pos.y + 10), slop: 2)
        #expect(begun.drag?.faces?.count == 20)
        let move = perCall(2000) { i in sink &+= canvas.dragMove(Point(first.pos.x + 100 + Double(i % 50), first.pos.y + 10), slop: 2).serial }
        var wires = 0
        let paths = perCall(200) { i in
            var c = 0
            for p in canvas.wirePaths(dx: Float(i % 50), dy: 0) { c += p.pointCount }
            wires = c
        }
        _ = try await canvas.dragCancel()
        report("kit.hot.\(n)", ["nodes": .int(Int64(n)), "geometry_set_call_us": .double(geometryCall),
                                "geometry_to_snapshot_us": .double(geometryToSnapshot),
                                "hit_test_anywhere": stats(hit), "hit_test_on_faces": stats(onFaces),
                                "wire_paths_20_faces_rotating": stats(pathsRotating), "most_wires_rotating": .int(Int64(rotating)), "wire_target": stats(target), "gesture_move": stats(move),
                                "wire_paths_first_20_faces_dragged": stats(paths), "points_in_dragged_paths": .int(Int64(wires)), "sink": .uint(sink)])
    }

    /// The model's memory: bytes the heap has in use (every malloc zone) before the scene's
    /// bytes are parsed, at the peak (bytes, parsed tree and decoded scene alive at once), and what
    /// the model keeps once they are gone. The core is idle meanwhile (its graph built and its
    /// scene published first), so its own allocations don't move the count.
    @Test func modelMemory() async throws {
        func heap() -> Int64 {
            var st = malloc_statistics_t()
            malloc_zone_statistics(nil, &st)
            return Int64(st.size_in_use)
        }
        for n in [500, 2000] {
            let core = try await graphOf(n, "m-mem-\(n)")
            let v = try await core.request("scene.get")
            let bytes = v.serialized()
            try await Task.sleep(for: .milliseconds(200))  // let the core go quiet
            let h0 = heap()
            var parsed: JSONValue? = try JSONValue.parse(bytes)
            let h1 = heap()
            var reply: SceneReply? = try JSONValueDecoder().decode(SceneReply.self, from: parsed ?? .null)
            let h2 = heap()
            let model = DocumentModel(core: core)
            if let reply { model.apply(reply) }
            let h3 = heap()
            parsed = nil
            reply = nil
            let h4 = heap()
            #expect(model.faces.count == n)
            report("kit.memory.\(n)", ["nodes": .int(Int64(n)), "scene_bytes": .int(Int64(bytes.count)),
                                       "parsed_tree_kb": .int((h1 - h0) / 1024), "decoded_scene_kb": .int((h2 - h1) / 1024),
                                       "peak_kb": .int((h3 - h0) / 1024), "model_keeps_kb": .int((h4 - h0) / 1024)])
            withExtendedLifetime(model) {}
            core.close()
        }
    }

    /// The N0 numbers again through the new wrapper: request latency and the 64 MiB borrow.
    @Test func latencyAndBlob() async throws {
        let core = try openCore("m-latency", ["demo": true])
        defer { core.close() }
        for (name, method, params) in [("latency.ping", "ping", JSONValue.object([:])),
                                       ("latency.command.node.move", "command.run", ["command": "node.move", "params": ["id": "shout", "pos": [10, 10]]])] {
            for _ in 0..<50 { _ = try await core.request(method, params) }
            var us: [Double] = []
            for _ in 0..<2000 {
                let t = nowNs()
                _ = try await core.request(method, params)
                us.append(micros(since: t))
            }
            report(name, stats(us))
        }
        let p = try await core.request("spike.publish", ["bytes": .int(64 << 20), "seed": 11])
        let handle = try #require(p["handle"].string)
        let t = nowNs()
        guard case .ready(let blob) = try core.blob(handle) else { Issue.record("pending"); return }
        let obtain = micros(since: t)
        var same = false
        blob.data.withUnsafeBytes { same = UInt(bitPattern: $0.baseAddress) == blob.address }
        #expect(same && blob.data.count == 64 << 20)
        report("blob.64MiB", ["obtain_us": .double(obtain), "zero_copy": .bool(same)])
    }
}

/// Where a one-node delta's time goes (a probe: PLUMCRAFT_MEASURE=1).
@Suite(.serialized, .enabled(if: measuring)) @MainActor struct DeltaTimeline {
    @Test(arguments: [500, 2000]) func timeline(n: Int) async throws {
        let core = try await graphOf(n, "m-timeline-\(n)")
        defer { core.close() }
        let model = DocumentModel(core: core)
        model.start()
        #expect(await model.wait(timeout: .seconds(60)) { model.faces.count == n })
        let lit = try #require(model.faces.nidOf["n1"].flatMap { model.faces[$0] })
        final class Times: Sendable { let changed = Mutex<[UInt64]>([]) }
        let times = Times()
        let feed = core.feed()
        let watcher = Task.detached {
            for await b in feed where b.changed.contains(where: { $0.domain == .known(.scene) }) {
                times.changed.withLock { $0.append(nowNs()) }
            }
        }
        var applyAt: UInt64 = 0
        model.onChange = { c in if c.changed.contains(lit.nid) { applyAt = nowNs() } }
        var rows: [String] = []
        for i in 0..<12 {
            let value = JSONValue.string("t \(i)")
            times.changed.withLock { $0 = [] }
            let s = nowNs()
            _ = try await model.run("node.set", ["node": "n1", "port": "text", "value": value])
            let r = nowNs()
            #expect(await model.wait { lit.item.inputs.first?.widget?.value == value })
            let ch = times.changed.withLock { $0 }
            rows.append("n \(n) reply \(Double(r - s) / 1000) changed \(ch.map { Double($0 - s) / 1000 }) applied \(Double(applyAt - s) / 1000)")
        }
        watcher.cancel()
        for r in rows { print("TIMELINE \(r)") }
    }
}

/// Geometry to the hot calls' snapshot with and without a model following the core (a probe).
@Suite(.serialized, .enabled(if: measuring)) @MainActor struct GeometryProbe {
    @Test(arguments: [false, true]) func geometryToSnapshot(following: Bool) async throws {
        let n = 500
        let core = try await graphOf(n, "m-geo-\(following)")
        defer { core.close() }
        let model = DocumentModel(core: core)
        let scene: Scene
        if following {
            model.start()
            #expect(await model.wait(timeout: .seconds(60)) { model.faces.count == n && !model.menuBar.isEmpty })
            scene = try await core.request(Scene.self, "scene.get")
        } else {
            scene = try await core.request(Scene.self, "scene.get")
        }
        let canvas = CanvasLink(core: core)
        var out: [Double] = []
        for round in 0..<6 {
            let w = Float(240 + 4 * round)
            let measures = scene.nodes.map { FaceMeasure(face: $0.nid, width: w, height: 160, header: 28, outputs: [40]) }
            let last = try #require(scene.nodes.last)
            // let the core go quiet first, as a zoom settling would find it
            try await Task.sleep(for: .milliseconds(120))
            let g0 = nowNs()
            try canvas.report(measures)
            var hit = false
            while !hit && nowNs() - g0 < 10_000_000_000 {
                hit = canvas.hit(Point(last.pos.x + Double(w) - 2, last.pos.y + 100), slop: 0.5).node == last.nid
                    && canvas.hit(Point(last.pos.x + Double(w) + 1, last.pos.y + 100), slop: 0.5).node != last.nid
            }
            out.append(micros(since: g0))
        }
        report("kit.geometry.\(following ? "following" : "alone")", ["nodes": .int(Int64(n)), "to_snapshot": stats(out)])
    }
}
