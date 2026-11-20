// The scripted budget run (plan section 10, read as N0 did in 15.2), on a real scene: the core
// builds `spike.scene {nodes, wires}` (text nodes, real faces, real wires), the canvas draws it,
// and a display link drives pan, zoom and a face drag with no hand on the trackpad. Each phase is
// an os_signpost interval named "phase", so Instruments' Animation Hitches bins its hitches by
// phase (bench.sh trace); the display link's own intervals are recorded too, though 15.2 found
// them the wrong clock. Then a one-node change is timed until its face is on its layer, and
// memory is read. Writes JSON to `--bench OUT` and exits.

import AppKit
import Darwin
import os
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore

struct BenchOptions {
    var out: String
    var nodes = 500
    var wires = 800
    var phaseSeconds = 4.0
}

func physFootprintMB() -> Double {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let kr = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) }
    }
    return kr == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
}

/// When this process started, from the kernel (for launch to first frame).
func processStart() -> Date? {
    var kp = kinfo_proc()
    var size = MemoryLayout<kinfo_proc>.size
    var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
    guard sysctl(&mib, 4, &kp, &size, nil, 0) == 0 else { return nil }
    let tv = kp.kp_proc.p_un.__p_starttime
    return Date(timeIntervalSince1970: Double(tv.tv_sec) + Double(tv.tv_usec) / 1e6)
}

func ms(_ d: Duration) -> Double { Double(d.components.seconds) * 1000 + Double(d.components.attoseconds) / 1e15 }

func stats(_ xs: [Double]) -> JSONValue {
    guard !xs.isEmpty else { return [:] }
    let s = xs.sorted()
    func p(_ q: Double) -> Double { s[min(s.count - 1, Int((Double(s.count - 1) * q).rounded()))] }
    let r = { (x: Double) in JSONValue.double((x * 100).rounded() / 100) }
    return ["n": .int(Int64(s.count)), "p50": r(p(0.5)), "p95": r(p(0.95)), "max": r(s[s.count - 1]), "min": r(s[0])]
}

@MainActor
final class BenchWindow: NSObject {
    let options: BenchOptions
    let window: NSWindow
    var view: CanvasView?
    var results: [String: JSONValue] = [:]
    let signposter = OSSignposter(subsystem: "com.plumcraft.mac.bench", category: .pointsOfInterest)
    var link: CADisplayLink?
    var tick: ((Double) -> Void)?
    var lastTs: Double?
    var intervals: [Double] = []
    var drawnWaiters: [(Set<UInt64>) -> Bool] = []

    init(options: BenchOptions) {
        self.options = options
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1440, height: 900), styleMask: [.titled, .closable, .resizable],
                          backing: .buffered, defer: false)
        window.title = "PlumCraft: benchmark"
        super.init()
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        _ = ProcessInfo.processInfo.beginActivity(options: [.userInitiated, .latencyCritical], reason: "benchmark")
        Task { await run() }
    }

    func put(_ k: String, _ v: JSONValue) { results[k] = v }

    /// Wait for `cond`, checked after every batch of faces the canvas puts up, for at most `limit`.
    func waitDrawn(_ limit: Duration = .seconds(30), _ cond: @escaping (Set<UInt64>) -> Bool) async -> Bool {
        let deadline = ContinuousClock.now + limit
        var met = false
        drawnWaiters.append { s in
            if cond(s) { met = true; return true }
            return false
        }
        while !met && ContinuousClock.now < deadline {
            try? await Task.sleep(for: .milliseconds(1))
        }
        return met
    }

    func run() async {
        guard let core = CoreHost.core else { return finish("no core") }
        let launched = processStart()
        let t0 = ContinuousClock.now
        let wall0 = Date()
        do {
            await CoreHost.ready?.value
            let r = try await core.request("document.open")
            guard let doc = r["doc"].uint64 else { return finish("document.open gave no doc") }
            _ = try await core.request("spike.scene", ["nodes": .int(Int64(options.nodes)), "wires": .int(Int64(options.wires))], doc: doc)
            let built = ContinuousClock.now
            let model = DocumentModel(core: core, doc: doc)
            let v = CanvasView(model: model, frame: window.contentLayoutRect)
            // PLUMCRAFT_NO_CULL=1: the canvas as slice 3 had it, for a comparison
            v.canvas.culling = ProcessInfo.processInfo.environment["PLUMCRAFT_NO_CULL"] != "1"
            v.autoresizingMask = [.width, .height]
            window.contentView = v
            view = v
            v.canvas.onDrawn = { [weak self] s in self?.drawn(s) }
            model.start()
            let m0 = physFootprintMB()
            // the scene is at zoom 1 until fitted: wait for every face measured and the visible drawn
            _ = await waitDrawn(.seconds(60)) { _ in v.canvas.firstFrameAt != nil && v.canvas.allMeasured }
            let first = v.canvas.firstFrameAt ?? .now
            put("nodes", .int(Int64(model.faces.count)))
            put("wires", .int(Int64(model.wires.count)))
            put("scene_build_ms", .double(ms(built - t0)))
            put("open_to_first_frame_ms", .double(ms(first - t0)))
            put("open_to_all_measured_ms", .double(ms(ContinuousClock.now - t0)))
            if let launched { put("launch_to_first_frame_ms", .double((wall0.timeIntervalSince(launched)) * 1000 + ms(first - t0))) }
            put("memory_mb_before_scene", .double(m0))
            put("counts_after_open", ["laid_out": .int(Int64(v.canvas.counts.facesLaidOut)), "drawn": .int(Int64(v.canvas.counts.facesDrawn)),
                                      "batches": .int(Int64(v.canvas.counts.batches))])
            try await phases(v)
            try await oneNode(v, core: core, doc: doc)
            put("memory_mb_end", .double(physFootprintMB()))
            put("face_pictures_mb", .double(Double(v.canvas.pictureBytes) / 1_048_576))
            put("screen_scale", .double(Double(window.backingScaleFactor)))
            put("max_fps", .int(Int64(window.screen?.maximumFramesPerSecond ?? 60)))
            finish(nil)
        } catch {
            finish("\(error)")
        }
    }

    func drawn(_ s: Set<UInt64>) {
        drawnWaiters.removeAll { $0(s) }
    }

    // MARK: Phases, driven by the display link

    func phase(_ name: String, seconds: Double, setup: () -> Void = {}, step: @escaping (Double) -> Void) async {
        guard let v = view else { return }
        setup()
        intervals = []
        lastTs = nil
        let label = "\(options.nodes).\(name)"
        let iv = signposter.beginInterval("phase", "\(label)")
        let start = CACurrentMediaTime()
        var done = false
        tick = { ts in
            let t = (ts - start) / seconds
            if t >= 1 { done = true; return }
            step(max(t, 0))
        }
        if link == nil {
            let l = v.displayLink(target: self, selector: #selector(onTick(_:)))
            l.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
            l.add(to: .main, forMode: .common)
            link = l
        }
        while !done { try? await Task.sleep(for: .milliseconds(5)) }
        signposter.endInterval("phase", iv)
        tick = nil
        let expected = 1.0 / Double(window.screen?.maximumFramesPerSecond ?? 120) * 1000
        let late = intervals.filter { $0 > expected * 1.5 }.count
        put("phase_" + name, ["frames": .int(Int64(intervals.count)), "interval_ms": stats(intervals), "late_frames": .int(Int64(late)),
                              "visible_faces_redrawn": .int(Int64(v.canvas.settle())),
                              "hidden_layers": .int(Int64(v.canvas.hiddenCount)), "culling": .bool(v.canvas.culling)])
        // let the settle land before the next phase
        _ = await waitDrawn(.seconds(2)) { _ in v.canvas.idle }
    }

    @objc func onTick(_ l: CADisplayLink) {
        if let last = lastTs { intervals.append((l.timestamp - last) * 1000) }
        lastTs = l.timestamp
        tick?(l.targetTimestamp)
    }

    func phases(_ v: CanvasView) async throws {
        let c = v.canvas
        guard let b = c.contentBounds else { return }
        let S = options.phaseSeconds
        let mid = CGPoint(x: b.midX, y: b.midY)
        let size = c.viewSize
        func center(_ g: CGPoint, _ z: CGFloat) {
            c.setViewport(Viewport(pan: CGPoint(x: size.width / 2 - g.x * z, y: size.height / 2 - g.y * z), zoom: z))
        }
        center(mid, 0.5)
        _ = await waitDrawn(.seconds(5)) { _ in c.idle }
        c.settle()
        _ = await waitDrawn(.seconds(5)) { _ in c.idle }
        await phase("idle", seconds: 1.5) { _ in }
        await phase("pan", seconds: S) { t in
            let a = 2 * Double.pi * t
            center(CGPoint(x: b.minX + b.width * (0.5 - 0.4 * cos(a)), y: b.midY + b.height * 0.3 * sin(a)), 0.5)
        }
        await phase("pan_fit", seconds: S, setup: { c.fit(b.insetBy(dx: -32, dy: -32), lo: 0.01, hi: 1, animated: false) }) { t in
            let a = 2 * Double.pi * t
            let z = min(size.width / (b.width + 64), size.height / (b.height + 64))
            center(CGPoint(x: mid.x + 120 / z * sin(a) * 0.2, y: mid.y + 80 / z * sin(2 * a) * 0.2), z)
        }
        await phase("zoom", seconds: S * 1.5) { t in
            let legs: [(Double, Double)] = [(1, 0.25), (0.25, 2), (2, 1)]
            let k = min(2, Int(t * 3))
            let u = t * 3 - Double(k)
            let sm = u * u * (3 - 2 * u)
            center(mid, CGFloat(exp(log(legs[k].0) + (log(legs[k].1) - log(legs[k].0)) * sm)))
        }
        // a face with the most wires near the middle, dragged in circles (layers and wire paths
        // from plum_wire_paths each frame; nothing sent to the core)
        let model = v.model
        var best: (UInt64, Int, Double)?
        var perFace: [String: Int] = [:]
        for w in model.wires.items { perFace[w.fromFace, default: 0] += 1; perFace[w.toFace, default: 0] += 1 }
        for f in model.faces.items {
            let n = perFace[f.id] ?? 0
            let d = hypot(f.pos.x - Double(mid.x), f.pos.y - Double(mid.y))
            if best == nil || abs(n - 8) < abs((best?.1 ?? 0) - 8) || (abs(n - 8) == abs((best?.1 ?? 0) - 8) && d < (best?.2 ?? 0)) { best = (f.nid, n, d) }
        }
        if let (nid, n, _) = best, let f = model.faces[nid]?.item {
            put("drag_face_wires", .int(Int64(n)))
            await phase("drag", seconds: S, setup: {
                center(CGPoint(x: f.pos.x + 110, y: f.pos.y + 80), 1)
                c.beginMove([nid])
            }) { t in
                let a = 2 * Double.pi * t * (S / 2)
                c.move(dx: 140 * sin(a), dy: 90 * (1 - cos(a)))
            }
            c.endMove()
        }
        center(mid, 0.5)
    }

    // MARK: A one-node change

    /// `node.set` on a face, timed until its new face is on its layer and committed: through the
    /// kit's `run` (which fetches the scene when answered), and through the feed alone.
    func oneNode(_ v: CanvasView, core: PlumCore, doc: UInt64) async throws {
        let model = v.model
        guard let lit = model.faces.items.first(where: { $0.kind == "text.literal" }) else { return }
        let c = v.canvas
        // in view, at zoom 1
        let size = c.viewSize
        c.setViewport(Viewport(pan: CGPoint(x: size.width / 2 - lit.pos.x - 100, y: size.height / 2 - lit.pos.y - 60), zoom: 1))
        _ = await waitDrawn(.seconds(5)) { _ in c.idle }
        for (name, viaKit) in [("one_node_change_kit_ms", true), ("one_node_change_feed_ms", false)] {
            var times: [Double] = []
            for i in 0..<20 {
                let before = model.faces[lit.nid]?.rev ?? 0
                let t = ContinuousClock.now
                let params: JSONValue = ["node": .string(lit.id), "port": "text", "value": .string("value \(name.count) \(i) \(UUID().uuidString.prefix(6))")]
                if viaKit {
                    _ = try await model.run("node.set", params)
                } else {
                    _ = try await core.command("node.set", params, doc: doc)
                }
                let ok = await waitDrawn(.seconds(5)) { s in s.contains(lit.nid) && (model.faces[lit.nid]?.rev ?? 0) > before }
                CATransaction.flush()
                if ok { times.append(ms(ContinuousClock.now - t)) }
            }
            put(name, stats(times))
        }
    }

    func finish(_ error: String?) {
        if let error { put("error", .string(error)) }
        link?.invalidate()
        try? JSONValue.object(results).serialized().write(to: URL(fileURLWithPath: options.out))
        print("BENCH " + JSONValue.object(results).text)
        CoreHost.core?.close()
        exit(error == nil ? 0 : 1)
    }
}
