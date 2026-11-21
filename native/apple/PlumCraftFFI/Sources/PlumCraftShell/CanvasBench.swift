// The scripted budget run (plan section 10, read as N0 did in 15.2), on a real scene, shared by the
// Mac and iPad apps (N3 slice 5, moved out of the Mac app): the core builds
// `spike.scene {nodes, wires}` (text nodes, real faces, real wires), the canvas draws it, and a
// display link drives pan, zoom and a face drag with no hand on the screen. Each phase is an
// os_signpost interval named "phase", so Instruments' Animation Hitches bins its hitches by phase;
// the display link's own intervals are recorded too, though 15.2 found them the wrong clock. Then a
// one-node change is timed until its face is on its layer, and memory is read.

import Darwin
import Foundation
import os
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore
#if os(macOS)
import AppKit
#else
import UIKit
#endif

public struct BenchOptions: Sendable {
    public var out: String
    public var nodes = 500
    public var wires = 800
    public var phaseSeconds = 4.0
    /// Draw no wires (`--hide-wires`): the faces' share of a hitch trace alone.
    public var hideWires = false
    /// No culling (`--no-cull`, as `PLUMCRAFT_NO_CULL=1`): every layer shown.
    public var noCull = false
    /// The Mac only (`--offscreen`): no window; the canvas's layer tree is rendered each frame by
    /// a CARenderer into a Metal texture and timed (Core Animation's own renderer, the work the
    /// render server does for a window, measured in this process without Instruments). The
    /// phases then step frame by frame, 120 to a second of theirs, instead of following a display
    /// link.
    public var offscreen = false
    /// The view's size in points (`--bench-view=834x1210`, the iPad Pro 11-inch in portrait) and
    /// its scale (`--bench-scale=2`), for `--offscreen`.
    public var view = CGSize(width: 834, height: 1210)
    public var scale: CGFloat = 2

    public init(out: String) { self.out = out }

    /// Take one of the bench's own switches (shared by both apps' parsers, so a new switch needs
    /// no change to either app's startup): whether `a` was one. Each is one word, so neither
    /// app's parser takes a value for a file to open.
    public mutating func take(_ a: String) -> Bool {
        switch a {
        case "--hide-wires": hideWires = true
        case "--no-cull": noCull = true
        case "--offscreen": offscreen = true
        default:
            if a.hasPrefix("--bench-view=") {
                let wh = a.dropFirst("--bench-view=".count).split(separator: "x").compactMap { Double($0) }
                if wh.count == 2, wh[0] > 0, wh[1] > 0 { view = CGSize(width: wh[0], height: wh[1]) }
            } else if a.hasPrefix("--bench-scale="), let s = Double(a.dropFirst("--bench-scale=".count)), s > 0 {
                scale = CGFloat(s)
            } else if let w = WireSwitch(rawValue: a) {
                wireSwitches.insert(w)
            } else {
                return false
            }
        }
        return true
    }

    /// Every bench switch in `argv`.
    public mutating func take(all argv: [String]) {
        for a in argv { _ = take(a) }
    }

    /// The canvas's wire switches the run sets (`WireSwitch`, the device pass's bisection).
    public var wireSwitches: Set<WireSwitch> = []
}

/// This process's physical footprint, in MB.
public func physFootprintMB() -> Double {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let kr = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) }
    }
    return kr == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
}

/// When this process started, from the kernel (for launch to first frame).
public func processStart() -> Date? {
    var kp = kinfo_proc()
    var size = MemoryLayout<kinfo_proc>.size
    var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
    guard sysctl(&mib, 4, &kp, &size, nil, 0) == 0 else { return nil }
    let tv = kp.kp_proc.p_un.__p_starttime
    return Date(timeIntervalSince1970: Double(tv.tv_sec) + Double(tv.tv_usec) / 1e6)
}

public func ms(_ d: Duration) -> Double { Double(d.components.seconds) * 1000 + Double(d.components.attoseconds) / 1e15 }

public func stats(_ xs: [Double]) -> JSONValue {
    guard !xs.isEmpty else { return [:] }
    let s = xs.sorted()
    func p(_ q: Double) -> Double { s[min(s.count - 1, Int((Double(s.count - 1) * q).rounded()))] }
    let r = { (x: Double) in JSONValue.double((x * 100).rounded() / 100) }
    return ["n": .int(Int64(s.count)), "p50": r(p(0.5)), "p95": r(p(0.95)), "max": r(s[s.count - 1]), "min": r(s[0])]
}

@MainActor
public final class CanvasBench: NSObject {
    public let options: BenchOptions
    public private(set) var results: [String: JSONValue] = [:]
    let signposter: OSSignposter
    var view: CanvasView?
    var link: CADisplayLink?
    var tick: ((Double) -> Void)?
    var lastTs: Double?
    var intervals: [Double] = []
    var drawnWaiters: [(Set<UInt64>) -> Bool] = []
    /// The display's most frames a second (for "late").
    public var maxFPS: () -> Int = { 120 }

    public init(options: BenchOptions, subsystem: String) {
        self.options = options
        signposter = OSSignposter(subsystem: subsystem, category: .pointsOfInterest)
        super.init()
    }

    public func put(_ k: String, _ v: JSONValue) { results[k] = v }

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

    /// The whole run: open a document, build the scene, put the canvas `make` gives into the
    /// window, phases, the one-node change, memory. Its results (also written to `options.out`).
    public func run(core: PlumCore, ready: Task<UInt64?, Never>?, make: (DocumentModel) -> CanvasView) async -> [String: JSONValue] {
        let launched = processStart()
        let t0 = ContinuousClock.now
        let wall0 = Date()
        do {
            _ = await ready?.value
            let r = try await core.request("document.open")
            guard let doc = r["doc"].uint64 else { return finish("document.open gave no doc") }
            _ = try await core.request("spike.scene", ["nodes": .int(Int64(options.nodes)), "wires": .int(Int64(options.wires))], doc: doc)
            let built = ContinuousClock.now
            let model = DocumentModel(core: core, doc: doc)
            let v = make(model)
            // PLUMCRAFT_NO_CULL=1 or --no-cull: the canvas as slice 3 had it, for a comparison
            v.canvas.culling = ProcessInfo.processInfo.environment["PLUMCRAFT_NO_CULL"] != "1" && !options.noCull
            v.canvas.wiresHidden = options.hideWires
            v.canvas.wireSwitches = options.wireSwitches
            put("wires_hidden", .bool(options.hideWires))
            put("wire_switches", .array(options.wireSwitches.map(\.rawValue).sorted().map { .string($0) }))
            put("offscreen", .bool(options.offscreen))
            #if os(macOS)
            if options.offscreen {
                offscreen = OffscreenRenderer(layer: v.canvas.host, size: options.view, scale: options.scale)
                if offscreen == nil { return finish("--offscreen: no Metal device") }
            }
            #endif
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
            put("wire_pictures_mb", .double(Double(v.canvas.wirePictureBytes) / 1_048_576))
            put("wire_tiles_drawn", .int(Int64(v.canvas.counts.tilesDrawn)))
            put("screen_scale", .double(Double(v.canvas.screenScale)))
            // the view the phases ran in (the N4 device gate: two device runs a day apart differed
            // by how much of the graph was on screen and at what raster scale, not by the code):
            // compare runs only with the same view
            let vs = v.canvas.viewSize
            put("view_points", .array([.double(Double(vs.width)), .double(Double(vs.height))]))
            put("orientation", .string(vs.width >= vs.height ? "landscape" : "portrait"))
            put("max_fps", .int(Int64(maxFPS())))
            return finish(nil)
        } catch {
            return finish("\(error)")
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
        var wireSamples: [WireStats] = []
        #if os(macOS)
        if let r = offscreen {
            await offscreenPhase(name, seconds: seconds, renderer: r, step: step, samples: &wireSamples)
            signposter.endInterval("phase", iv)
            putWires(name, wireSamples)
            put("phase_" + name, ["visible_faces_redrawn": .int(Int64(v.canvas.settle())), "hidden_layers": .int(Int64(v.canvas.hiddenCount)),
                                  "culling": .bool(v.canvas.culling), "zoom_end": .double(Double(v.canvas.viewport.zoom)),
                                  "raster_scale_end": .double(Double(v.canvas.rasterScale(forZoom: v.canvas.viewport.zoom))),
                                  "face_pictures_mb": .double(Double(v.canvas.pictureBytes) / 1_048_576),
                                  "wire_pictures_mb": .double(Double(v.canvas.wirePictureBytes) / 1_048_576),
                                  "memory_mb": .double(physFootprintMB())])
            _ = await waitDrawn(.seconds(2)) { _ in v.canvas.idle }
            return
        }
        #endif
        let start = CACurrentMediaTime()
        var done = false
        var k = 0
        tick = { [weak v] ts in
            k += 1
            if k % 15 == 1, let v { wireSamples.append(v.canvas.wireStats()) }
            let t = (ts - start) / seconds
            if t >= 1 { done = true; return }
            step(max(t, 0))
        }
        if link == nil {
            #if os(macOS)
            let l = v.displayLink(target: self, selector: #selector(onTick(_:)))
            #else
            let l = CADisplayLink(target: self, selector: #selector(onTick(_:)))
            #endif
            l.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
            l.add(to: .main, forMode: .common)
            link = l
        }
        while !done { try? await Task.sleep(for: .milliseconds(5)) }
        signposter.endInterval("phase", iv)
        tick = nil
        let expected = 1.0 / Double(maxFPS()) * 1000
        let late = intervals.filter { $0 > expected * 1.5 }.count
        put("phase_" + name, ["frames": .int(Int64(intervals.count)), "interval_ms": stats(intervals), "late_frames": .int(Int64(late)),
                              "visible_faces_redrawn": .int(Int64(v.canvas.settle())),
                              "hidden_layers": .int(Int64(v.canvas.hiddenCount)), "culling": .bool(v.canvas.culling),
                              "zoom_end": .double(Double(v.canvas.viewport.zoom)),
                              "raster_scale_end": .double(Double(v.canvas.rasterScale(forZoom: v.canvas.viewport.zoom))),
                              "face_pictures_mb": .double(Double(v.canvas.pictureBytes) / 1_048_576),
                              "wire_pictures_mb": .double(Double(v.canvas.wirePictureBytes) / 1_048_576)])
        putWires(name, wireSamples)
        // let the settle land before the next phase
        _ = await waitDrawn(.seconds(2)) { _ in v.canvas.idle }
    }

    /// The wires' load over a phase (`wireStats` sampled every 15th frame): means, and the most.
    func putWires(_ name: String, _ xs: [WireStats]) {
        guard !xs.isEmpty else { return }
        let n = Double(xs.count)
        let r = { (x: Double) in JSONValue.double((x * 100).rounded() / 100) }
        func mean(_ f: (WireStats) -> Double) -> JSONValue { r(xs.map(f).reduce(0, +) / n) }
        put("wires_" + name, ["samples": .int(Int64(xs.count)), "layers": mean { Double($0.layers) }, "shown": mean { Double($0.shown) },
                              "in_view": mean { Double($0.inView) }, "path_elements": mean { Double($0.elements) },
                              "pictures_in_view": mean { Double($0.tiles) }, "coverage_views": mean(\.coverage),
                              "coverage_views_max": r(xs.map(\.coverage).max() ?? 0), "largest_view_fraction": r(xs.map(\.largest).max() ?? 0)])
    }

    #if os(macOS)
    var offscreen: OffscreenRenderer?

    /// A phase stepped frame by frame (120 to a second of its time), each frame committed and
    /// rendered by Core Animation into a texture, timed to the GPU's end. A frame whose render
    /// takes longer than a 120 Hz frame (8.33 ms) is one a render server would deliver late
    /// ("over_budget"); the excess summed, a second, is the hitch proxy ("late_ms_per_s").
    func offscreenPhase(_ name: String, seconds: Double, renderer r: OffscreenRenderer, step: (Double) -> Void,
                        samples: inout [WireStats]) async {
        guard let v = view else { return }
        let frames = max(1, Int(seconds * 120))
        var cpu: [Double] = [], gpu: [Double] = [], total: [Double] = [], commit: [Double] = []
        // a frame before the phase, so a first render's caches aren't counted in it
        _ = r.frame()
        let tiles0 = v.canvas.counts.tilesDrawn
        for k in 0..<frames {
            step(Double(k) / Double(frames))
            let c0 = CACurrentMediaTime()
            CATransaction.flush()
            commit.append((CACurrentMediaTime() - c0) * 1000)
            let f = r.frame()
            cpu.append(f.cpu)
            gpu.append(f.gpu)
            total.append(f.total)
            if k % 15 == 0 { samples.append(v.canvas.wireStats()) }
            // the main actor's other work (a cull, a batch of faces) runs between frames, as it
            // would between a display link's ticks
            await Task.yield()
        }
        let budget = 1000.0 / 120
        let late = total.filter { $0 > budget }.count
        let excess = total.reduce(0) { $0 + max(0, $1 - budget) }
        put("offscreen_" + name, ["frames": .int(Int64(frames)), "render_cpu_ms": stats(cpu), "render_gpu_ms": stats(gpu),
                                  "render_total_ms": stats(total), "commit_ms": stats(commit), "over_budget": .int(Int64(late)),
                                  "late_ms_per_s": .double((excess / seconds * 10).rounded() / 10),
                                  // the frames over budget, by index (when in the phase they came)
                                  "late_frames_at": .array(total.indices.filter { total[$0] > budget }.prefix(20).map { .int(Int64($0)) }),
                                  "tiles_drawn": .int(Int64(v.canvas.counts.tilesDrawn - tiles0)),
                                  "view_points": .array([.double(Double(r.size.width)), .double(Double(r.size.height))]),
                                  "scale": .double(Double(r.scale))])
    }
    #endif

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

    func finish(_ error: String?) -> [String: JSONValue] {
        if let error { put("error", .string(error)) }
        link?.invalidate()
        link = nil
        try? JSONValue.object(results).serialized().write(to: URL(fileURLWithPath: options.out))
        print("BENCH " + JSONValue.object(results).text)
        return results
    }
}
