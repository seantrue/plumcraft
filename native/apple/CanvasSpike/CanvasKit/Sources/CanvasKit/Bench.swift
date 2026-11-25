// Scripted benchmark: drives pan, zoom, a node drag and run progress with no human, timing every
// display-link frame, then writes JSON and exits.
import CoreGraphics
import Darwin
import Foundation
import QuartzCore
import os

public struct BenchOptions: Sendable {
    public var out: String
    public var nodes = 500
    public var wires = 800
    public var phaseSeconds = 4.0
    public var modes: [WireMode] = WireMode.allCases

    /// Parses `--bench out.json [--nodes N] [--wires M] [--phase-seconds S] [--modes a,b]`; nil without `--bench`.
    public static func parse(_ args: [String]) -> BenchOptions? {
        func value(_ k: String) -> String? {
            guard let i = args.firstIndex(of: k), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        guard let out = value("--bench") else { return nil }
        var o = BenchOptions(out: out)
        if let v = value("--nodes").flatMap(Int.init) { o.nodes = v }
        if let v = value("--wires").flatMap(Int.init) { o.wires = v }
        if let v = value("--phase-seconds").flatMap(Double.init) { o.phaseSeconds = v }
        if let v = value("--modes") { o.modes = v.split(separator: ",").compactMap { WireMode(rawValue: String($0)) } }
        return o
    }
}

public func physFootprintMB() -> Double {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<integer_t>.size)
    let kr = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            task_info(task_self_trap(), task_flavor_t(TASK_VM_INFO), $0, &count)
        }
    }
    return kr == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
}

func stats(_ xs: [Double]) -> [String: Double] {
    guard !xs.isEmpty else { return [:] }
    let s = xs.sorted()
    func p(_ q: Double) -> Double { s[min(s.count - 1, Int((Double(s.count - 1) * q).rounded()))] }
    return ["mean": xs.reduce(0, +) / Double(xs.count), "p50": p(0.5), "p95": p(0.95), "p99": p(0.99), "max": s[s.count - 1]]
}

func r2(_ x: Double) -> Double { (x * 100).rounded() / 100 }

@MainActor
public final class BenchRunner {
    struct Phase {
        var name: String
        var seconds: Double
        var setup: @MainActor () -> Void = {}
        var step: @MainActor (Double, Int) -> Void = { _, _ in }  // t in 0..1, frame number
        var teardown: @MainActor () -> Void = {}
    }

    enum Step {
        case action(String, @MainActor () -> [String: Any])
        case phase(String, Phase)  // group key, phase
    }

    let canvas: CanvasController
    let options: BenchOptions
    let refreshHz: Double
    let device: [String: Any]
    var steps: [Step] = []
    var current = 0
    var phaseStart: Double?
    var lastTs: Double?
    var frame = 0
    var intervals: [Double] = []
    var work: [Double] = []
    var linkDurations: [Double] = []
    var results: [String: Any] = [:]
    var modeResults: [String: [String: Any]] = [:]
    public private(set) var done = false
    let signposter = OSSignposter(subsystem: "com.plumcraft.spike.canvas", category: .pointsOfInterest)
    var interval: OSSignpostIntervalState?
    let launched = now()
    var phaseStartRel = 0.0
    public var onDone: ((String) -> Void)?

    public init(canvas: CanvasController, options: BenchOptions, refreshHz: Double, device: [String: Any]) {
        self.canvas = canvas
        self.options = options
        self.refreshHz = refreshHz
        self.device = device
        plan()
    }

    func put(_ group: String, _ key: String, _ value: Any) {
        var g = modeResults[group] ?? [:]
        g[key] = value
        modeResults[group] = g
    }

    func plan() {
        let S = options.phaseSeconds
        let c = canvas
        var scene = FakeScene.make(nodes: options.nodes, wires: options.wires)
        let sceneBuild: Double = {
            let t = now()
            scene = FakeScene.make(nodes: options.nodes, wires: options.wires)
            return (now() - t) * 1000
        }()
        results["scene_generate_ms"] = r2(sceneBuild)
        results["nodes"] = scene.nodes.count
        results["wires"] = scene.wires.count
        results["preview_nodes"] = scene.nodes.filter { $0.rows.contains { if case .preview = $0.kind { return true } else { return false } } }.count
        let b = scene.bounds
        results["graph_bounds"] = [b.minX, b.minY, b.width, b.height].map { Double($0) }
        let mid = CGPoint(x: b.midX, y: b.midY)
        // the drag node: attached wire count nearest 8, nearest the middle on ties
        let dragNode = scene.nodes.indices.min { a, bb in
            let da = abs(scene.wiresByNode[a].count - 8), db = abs(scene.wiresByNode[bb].count - 8)
            if da != db { return da < db }
            func dist(_ i: Int) -> CGFloat { hypot(scene.nodes[i].position.x - mid.x, scene.nodes[i].position.y - mid.y) }
            return dist(a) < dist(bb)
        } ?? 0
        results["drag_node"] = dragNode
        results["drag_node_wires"] = scene.wiresByNode[dragNode].count
        let dragHome = scene.nodes[dragNode].position
        let runSet = Array((0..<scene.nodes.count).sorted {
            hypot(scene.nodes[$0].position.x - mid.x, scene.nodes[$0].position.y - mid.y)
                < hypot(scene.nodes[$1].position.x - mid.x, scene.nodes[$1].position.y - mid.y)
        }.prefix(50))
        let initialZoom: CGFloat = 0.5

        for (mi, mode) in options.modes.enumerated() {
            let key = mode.rawValue
            steps.append(.action(key) {
                let m0 = physFootprintMB()
                let t = now()
                var timings = c.build(scene: scene, wireMode: mode, zoom: initialZoom)
                c.center(on: mid, zoom: initialZoom)
                timings["build_total_ms"] = (now() - t) * 1000
                let f = now()
                CATransaction.flush()
                timings["first_commit_ms"] = (now() - f) * 1000
                return [
                    "build": timings.mapValues(r2), "memory_mb_before_build": r2(m0),
                    "memory_mb_after_build": r2(physFootprintMB()),
                    "face_bitmap_mb": r2(Double(c.faceBytes()) / 1_048_576),
                    "raster_scale": Double(c.rasterScale(forZoom: initialZoom)),
                ]
            })
            steps.append(.phase(key, Phase(name: "warmup", seconds: 1)))
            steps.append(.phase(key, Phase(name: "idle", seconds: 1.5)))
            steps.append(.phase(key, Phase(name: "pan", seconds: S, step: { t, _ in
                let a = 2 * Double.pi * t
                let g = CGPoint(x: b.minX + b.width * (0.5 - 0.4 * cos(a)), y: b.midY + b.height * 0.3 * sin(a))
                c.center(on: g, zoom: initialZoom)
            })))
            steps.append(.phase(key, Phase(name: "pan_fit", seconds: S, setup: { c.fit() }, step: { t, _ in
                c.fit()
                let a = 2 * Double.pi * t
                c.pan(by: CGPoint(x: 120 * sin(a), y: 80 * sin(2 * a)))
            })))
            steps.append(.phase(key, Phase(name: "zoom", seconds: S * 1.5, step: { t, _ in
                // log-space 1 -> 0.25 -> 2 -> 1, smoothstep within each third
                let legs: [(Double, Double)] = [(1, 0.25), (0.25, 2), (2, 1)]
                let k = min(2, Int(t * 3))
                let u = t * 3 - Double(k)
                let sm = u * u * (3 - 2 * u)
                let z = exp(log(legs[k].0) + (log(legs[k].1) - log(legs[k].0)) * sm)
                c.center(on: mid, zoom: z)
            })))
            steps.append(.phase(key, Phase(
                name: "drag", seconds: S,
                setup: {
                    c.center(on: CGPoint(x: dragHome.x + 110, y: dragHome.y + 80), zoom: 1)
                    c.beginDrag(dragNode)
                },
                step: { t, _ in
                    let a = 2 * Double.pi * t * (S / 2)  // one turn every 2 s
                    c.moveNode(dragNode, to: CGPoint(x: dragHome.x + 140 * sin(a), y: dragHome.y + 90 * (1 - cos(a))))
                },
                teardown: {
                    c.moveNode(dragNode, to: dragHome)
                    c.endDrag()
                })))
            steps.append(.phase(key, Phase(
                name: "run_progress", seconds: S,
                setup: { c.center(on: mid, zoom: initialZoom) },
                step: { t, f in
                    let states: [RunState] = [.waiting, .running, .done, .cached, .failed, .stale]
                    for (k, i) in runSet.enumerated() {
                        let s = states[(f / 6 + k) % states.count]
                        c.setRunState(i, s, fraction: s == .running ? CGFloat((t * 4 + Double(k) * 0.02).truncatingRemainder(dividingBy: 1)) : nil)
                    }
                },
                teardown: { for i in runSet { c.setRunState(i, scene.nodes[i].state, fraction: nil) } })))
            if mi == 0 { planReraster(mid: mid) }
            steps.append(.action(key) { ["memory_mb_end": r2(physFootprintMB())] })
        }
    }

    /// Re-raster costs after a zoom settles: visible faces at the new scale, and the eager "all" case.
    func planReraster(mid: CGPoint) {
        let c = canvas
        steps.append(.action("reraster") {
            var out: [String: Any] = [:]
            @MainActor func measure(_ name: String, zoom: CGFloat?, all: Bool, scale: CGFloat? = nil, concurrent: Bool) {
                if let zoom { c.center(on: mid, zoom: zoom) } else { c.fit() }
                let idx = all ? Array(c.scene.nodes.indices) : c.visibleNodes()
                let s = scale ?? c.rasterScale(forZoom: c.zoom)
                let ms = c.reraster(idx, scale: s, concurrent: concurrent)
                let f = now()
                CATransaction.flush()
                out[name] = [
                    "zoom": r2(Double(c.zoom)), "faces": idx.count, "scale": Double(s), "concurrent": concurrent,
                    "render_ms": r2(ms), "commit_ms": r2((now() - f) * 1000),
                    "per_face_ms": r2(ms / Double(max(1, idx.count))),
                    "face_bitmap_mb_total": r2(Double(c.faceBytes()) / 1_048_576), "memory_mb": r2(physFootprintMB()),
                ]
            }
            measure("visible_zoom1_serial", zoom: 1, all: false, concurrent: false)
            measure("visible_zoom1_concurrent", zoom: 1, all: false, scale: c.screenScale, concurrent: true)
            measure("visible_zoom2_concurrent", zoom: 2, all: false, concurrent: true)
            measure("fit_all_concurrent", zoom: nil, all: true, concurrent: true)
            measure("all_at_screen_scale_serial", zoom: 1, all: true, scale: c.screenScale, concurrent: false)
            measure("all_at_screen_scale_concurrent", zoom: 1, all: true, scale: c.screenScale, concurrent: true)
            // back to the build's scale so later phases start where the other modes do
            measure("restore", zoom: 0.5, all: true, scale: c.rasterScale(forZoom: 0.5), concurrent: true)
            out["cpu_count"] = ProcessInfo.processInfo.activeProcessorCount
            return out
        })
    }

    /// Called by the host's display link every frame.
    public func tick(timestamp: Double, duration: Double) {
        guard !done else { return }
        while current < steps.count, case .action(let group, let run) = steps[current] {
            let r = run()
            if group == "reraster" { results["reraster"] = r } else { for (k, v) in r { put(group, k, v) } }
            current += 1
            lastTs = nil
            return  // let the frame after an action be the first of the next phase
        }
        guard current < steps.count, case .phase(let group, let phase) = steps[current] else {
            finish()
            return
        }
        if phaseStart == nil {
            phaseStart = timestamp
            frame = 0
            intervals = []; work = []; linkDurations = []
            phaseStartRel = now() - launched
            phase.setup()
            interval = signposter.beginInterval("phase", "\(group).\(phase.name)")
        }
        let t = (timestamp - (phaseStart ?? timestamp)) / phase.seconds
        if t >= 1 {
            if let interval { signposter.endInterval("phase", interval) }
            interval = nil
            phase.teardown()
            CATransaction.flush()
            put(group, phase.name, summarize())
            phaseStart = nil
            lastTs = nil
            current += 1
            return
        }
        let w0 = now()
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        phase.step(t, frame)
        CATransaction.commit()
        CATransaction.flush()
        work.append((now() - w0) * 1000)
        if let lastTs { intervals.append((timestamp - lastTs) * 1000) }
        linkDurations.append(duration * 1000)
        lastTs = timestamp
        frame += 1
    }

    func summarize() -> [String: Any] {
        let nominal = 1000 / refreshHz
        var dropped = 0
        var hitches = 0
        for d in intervals {
            let n = Int((d / nominal).rounded())
            if n > 1 { dropped += n - 1 }
            if d > nominal * 1.5 { hitches += 1 }
        }
        let total = intervals.reduce(0, +)
        return [
            "frames": intervals.count + 1,
            "fps": r2(total > 0 ? Double(intervals.count) * 1000 / total : 0),
            "interval_ms": stats(intervals).mapValues(r2),
            "dropped_frames": dropped,
            "hitch_frames": hitches,
            "work_ms": stats(work).mapValues(r2),
            "link_duration_ms_p50": r2(stats(linkDurations)["p50"] ?? 0),
            "start_s_since_runner": r2(phaseStartRel),
        ]
    }

    func finish() {
        done = true
        results["device"] = device
        results["refresh_hz_max"] = refreshHz
        results["phase_seconds"] = options.phaseSeconds
        results["modes"] = modeResults
        results["memory_mb_final"] = r2(physFootprintMB())
        let data = (try? JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys])) ?? Data()
        let path = BenchRunner.resolve(options.out)
        do {
            try data.write(to: URL(fileURLWithPath: path))
        } catch {
            print("bench: could not write \(path): \(error)")
        }
        let line = String(data: (try? JSONSerialization.data(withJSONObject: results, options: [.sortedKeys])) ?? Data(), encoding: .utf8) ?? ""
        print("BENCH_JSON \(line)")
        print("bench: wrote \(path)")
        fflush(stdout)
        onDone?(path)
    }

    /// A relative path goes to the app's Documents directory on iOS, the current directory elsewhere.
    public static func resolve(_ p: String) -> String {
        if p.hasPrefix("/") { return p }
        #if os(iOS)
        let docs = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        return docs.appendingPathComponent(p).path
        #else
        return FileManager.default.currentDirectoryPath + "/" + p
        #endif
    }
}
