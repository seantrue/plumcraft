// The FFI wrapper against a real core: N0's sequence (open, list, build, run, borrow), its
// hostile input, the feed's threading promises, and the hot calls.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import Testing

@Suite struct Core {
    @Test func openListBuildRunAndBorrow() async throws {
        let core = try openCore("flow")
        defer { core.close() }
        var events: [JSONValue] = []
        let feed = core.feed()
        let collector = Task { for await b in feed { events.append(contentsOf: b.events) }; return events }

        let list = try await core.request("command.list")
        #expect(list.array?.contains { $0["id"] == "node.add" } == true)
        let lit = try #require(try await core.command("node.add", ["kind": "text.literal", "pos": [0, 0]])["id"].string)
        _ = try await core.command("node.set", ["node": .string(lit), "port": "text", "value": "The quick brown fox jumps over the lazy dog, then naps. 0123456\n"])
        let rep = try #require(try await core.command("node.add", ["kind": "text.repeat", "pos": [260, 0]])["id"].string)
        _ = try await core.command("node.set", ["node": .string(rep), "port": "times", "value": 1_000_000])
        let up = try #require(try await core.command("node.add", ["kind": "text.upper", "pos": [260, 160]])["id"].string)
        _ = try await core.command("wire.connect", ["from": ["node": .string(lit), "port": "text"], "to": ["node": .string(rep), "port": "text"]])
        _ = try await core.command("wire.connect", ["from": ["node": .string(lit), "port": "text"], "to": ["node": .string(up), "port": "text"]])
        let graph = try await core.request("graph.get")
        #expect(graph["nodes"].object?.count == 3)
        #expect(graph["wires"].array?.count == 2)

        _ = try await core.command("run.start", ["watch": [["node": .string(rep), "port": "text"]]])
        let done = try await core.request("spike.waitRun", ["timeout_ms": 30_000])
        #expect(done["nodes"][rep]["state"] == "done")
        #expect(done["nodes"][up]["state"] == "done")
        let handle = try #require(done["watch"][0]["full"]["handle"].string)
        let leases = PlumCore.leases
        do {
            guard case .ready(let blob) = try core.blob(handle) else { Issue.record("a store blob was pending"); return }
            #expect(blob.data.count == 64_000_000)
            #expect(blob.data.starts(with: Array("The quick brown fox".utf8)))
            blob.data.withUnsafeBytes { #expect(UInt(bitPattern: $0.baseAddress) == blob.address, "Data reads the lent bytes in place") }
            #expect(PlumCore.leases == leases + 1)
        }
        #expect(PlumCore.leases == leases, "the Data is gone: released")
        core.close()
        let seen = await collector.value
        let kinds = Set(seen.compactMap { $0["kind"].string })
        #expect(kinds.contains("node.status"), "\(kinds)")
        #expect(core.feedCounts.wakes > 0)
    }

    @Test func hostileInputAndASecondClose() async throws {
        let core = try openCore("hostile")
        for bad in [Data(), Data([0xff, 0xfe]), Data("{nope".utf8), Data("{\"method\":\"no.such\"}".utf8), Data("[1,2]".utf8)] {
            await #expect(throws: PlumError.self) { _ = try await core.send(bad) }
        }
        do {
            _ = try await core.request("no.such.method")
            Issue.record("an unknown method answered")
        } catch let e as PlumError {
            // the core's words; its `code` (contract 6.1) isn't sent for this one yet
            guard case .refused = e.code else { Issue.record("\(e)"); return }
            #expect(e.description.contains("no.such.method"))
        }
        #expect(throws: PlumError.self) { _ = try core.blob("b3:raw:0000") }
        #expect(throws: PlumError.self) { _ = try core.blob("") }
        // hot calls on a document that doesn't exist: a status, never a crash
        #expect(core.hitTest(doc: 999, x: 0, y: 0, slop: 4).status == PlumStatus.errMissing)
        #expect(core.gestureMove(x: .nan, y: 0, slop: 4).status != PlumStatus.ok)
        var buf: [Float] = []
        #expect(Array(core.wirePaths(doc: 999, faces: [1, 2], dx: 0, dy: 0, buffer: &buf)).isEmpty)
        #expect(throws: PlumError.self) { try core.geometrySet(faces: [FaceMeasure(face: 1, width: 1, height: 1, inputs: Array(repeating: 1, count: 5000))]) }
        let id = core.id
        core.close()
        core.close()
        #expect(PlumCore.rawClose(id) == PlumStatus.errCore)
        await #expect(throws: PlumError.self) { _ = try await core.request("ping") }
        #expect(core.hitTest(x: 0, y: 0, slop: 1).status == PlumStatus.errCore)
        #expect(throws: PlumError.self) { _ = try openCore("bad-engine", ["engine": "vectorcraft"]) }
        #expect(PlumCore.abiVersion == 1)
    }

    /// A reply never waits for the main actor: with the main actor busy, requests from elsewhere
    /// are still answered, because the pump isn't on it.
    @Test func repliesDontWaitForTheMainActor() async throws {
        let core = try openCore("main-busy")
        defer { core.close() }
        _ = try await core.request("ping")
        let started = Task { @MainActor in
            let t0 = nowNs()
            while nowNs() - t0 < 400_000_000 {}  // the main actor stuck for 0.4 s
        }
        let t0 = nowNs()
        var answered = 0
        for _ in 0..<20 {
            _ = try await core.request("ping")
            answered += 1
        }
        let took = micros(since: t0)
        await started.value
        #expect(answered == 20)
        #expect(took < 300_000, "20 pings took \(took) µs with the main actor stuck")
    }

    /// Many requests in flight at once all come back, each to its own caller, and wakes
    /// coalesce (fewer wakes than replies).
    @Test func manyRequestsInFlightAndWakesCoalesce() async throws {
        let core = try openCore("many")
        defer { core.close() }
        _ = try await core.request("ping")
        let (w0, _, _) = core.feedCounts
        try await withThrowingTaskGroup(of: (Int, JSONValue).self) { g in
            for i in 0..<500 {
                g.addTask { (i, try await core.request("ping", ["n": .int(Int64(i))])) }
            }
            var seen = Set<Int>()
            for try await (i, _) in g { seen.insert(i) }
            #expect(seen.count == 500)
        }
        let (w1, _, _) = core.feedCounts
        #expect(w1 - w0 < 500, "wakes \(w1 - w0) for 500 replies")
    }

    @Test func aRequestToAClosedCoreFailsAndPendingOnesEnd() async throws {
        let core = try openCore("close-pending")
        _ = try await core.request("ping")
        let waiting = Task { try await core.request("spike.waitRun", ["timeout_ms": 60_000]) }
        core.close()
        do {
            _ = try await waiting.value
        } catch let e as PlumError {
            // either it ended with the core, or the core answered it before closing
            #expect(e.code == .closed || e.code == .refused(core: nil) || e.description.contains("run"), "\(e)")
        }
        var ended = false
        for await _ in core.feed() {}
        ended = true
        #expect(ended)
    }
}

@Suite @MainActor struct Hot {
    /// Two faces measured typed, then every hot call against what the core has.
    @Test func hitTargetPathsAndADrag() async throws {
        let core = try openCore("hot")
        defer { core.close() }
        let a = try #require(try await core.command("node.add", ["kind": "text.literal", "pos": [0, 0]])["id"].string)
        let b = try #require(try await core.command("node.add", ["kind": "text.upper", "pos": [500, 0]])["id"].string)
        _ = try await core.command("wire.connect", ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "text"]])
        let scene = try await core.request(Scene.self, "scene.get")
        let fa = try #require(scene.nodes.first { $0.id == a })
        let fb = try #require(scene.nodes.first { $0.id == b })
        let canvas = CanvasLink(core: core)
        try canvas.report([FaceMeasure(face: fa.nid, rev: fa.rev, width: 180, height: 100, header: 28, inputs: [], outputs: [60]),
                                 FaceMeasure(face: fb.nid, rev: fb.rev, width: 160, height: 90, header: 28, inputs: [44], outputs: [44])])
        // measured once a request after it says so
        var measured = false
        for _ in 0..<200 where !measured {
            measured = try await core.request(Located.self, "geometry.locate", ["node": .string(a)]).measured == true
        }
        #expect(measured)

        let pin = core.hitTest(x: 180, y: 60, slop: 4)
        #expect(pin.ok && pin.kind == .pin && pin.isOutput && pin.node == fa.nid && pin.pin == 0, "\(pin)")
        let head = core.hitTest(x: 520, y: 10, slop: 2)
        #expect(head.kind == .node && head.nodePart == .header && head.node == fb.nid, "\(head)")
        #expect(core.hitTest(x: -3000, y: -3000, slop: 4).kind == .none)
        let target = core.wireTarget(face: fa.nid, output: true, pin: 0, x: 502, y: 44, slop: 8)
        #expect(target.kind == .pin && target.node == fb.nid && target.fits == 1, "\(target)")

        // wire paths: the wire's own path at no offset, moved by the offset when asked
        let wire = try #require(try await core.request(Scene.self, "scene.get").wires.first)
        var buf: [Float] = []
        let still = Array(core.wirePaths(faces: [fb.nid], dx: 0, dy: 0, buffer: &buf))
        #expect(still.count == 1 && still.first?.nid == wire.nid)
        if let p = still.first {
            #expect(p.pointCount == wire.path.count)
            for (k, q) in wire.path.enumerated() {
                #expect(abs(Double(p.points[p.points.startIndex + 2 * k]) - q.x) < 0.01)
            }
        }
        let moved = Array(core.wirePaths(faces: [fb.nid], dx: 10, dy: 20, buffer: &buf))
        if let p = moved.first, let last = wire.path.last {
            #expect(abs(Double(p.points[p.points.endIndex - 2]) - (last.x + 10)) < 0.01)
            #expect(abs(Double(p.points[p.points.endIndex - 1]) - (last.y + 20)) < 0.01)
        }

        // a drag of face b: begin (JSON), moves (typed), the paths, end (one edit.move)
        let begun = try await canvas.dragBegin(Point(520, 50), slop: 2)
        #expect(begun.drag?.kind == .known(.move))
        #expect(canvas.draggedFaces == [fb.nid])
        let mv = canvas.dragMove(Point(560, 80), slop: 2)
        #expect(mv.ok && mv.kind == .move && mv.dx == 40 && mv.dy == 30, "\(mv)")
        let paths = Array(canvas.wirePaths(dx: mv.dx, dy: mv.dy))
        #expect(paths.count == 1)
        let end = try await canvas.dragEnd(Point(560, 80), slop: 2)
        #expect(end.commands.first?.command == "edit.move")
        #expect(canvas.drag == nil)
        let after = try await core.request(Scene.self, "scene.get").nodes.first { $0.id == b }
        #expect(after?.pos == Point(540, 30))
        #expect(core.gestureMove(x: 0, y: 0, slop: 1).status == PlumStatus.errMissing, "no drag under way")
    }

    /// `geometry.set`'s JSON form carries what the typed call can't: parts and an offset.
    @Test func theJSONFormCarriesPartsAndOffset() async throws {
        let core = try openCore("hot-json")
        defer { core.close() }
        let a = try #require(try await core.command("node.add", ["kind": "text.upper", "pos": [0, 0]])["id"].string)
        let canvas = CanvasLink(core: core)
        let r = try await canvas.report(json: [FaceReport(node: a, size: Point(200, 120), header: 30, inputs: [44], outputs: [44],
                                                          parts: ["title": Rect(20, 4, 120, 24), "menu": Rect(180, 4, 196, 24)], offset: Point(0, 0))])
        #expect(r.applied == 1 && r.unknown?.isEmpty != false)
        let title = try await canvas.hitJSON(Point(60, 14), slop: 1)
        #expect(title.kind == .known(.node) && title.part == "title" && title.node == a, "\(title)")
        let menu = canvas.hit(Point(188, 14), slop: 1)
        #expect(menu.nodePart == .menu, "\(menu)")
        await #expect(throws: PlumError.self) { _ = try await canvas.report(json: [FaceReport(node: a, size: Point(1, 1), parts: ["sparkle": Rect(0, 0, 1, 1)])]) }
    }
}
