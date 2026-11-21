// The scripted budget run (plan section 10): the kit's CanvasBench (PlumCraftShell, shared with
// the iPad app) in a window of its own, or with `--offscreen` in none (the canvas rendered by
// Core Animation into a texture and timed). Writes JSON to `--bench OUT` and exits.

import AppKit
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

@MainActor
final class BenchWindow: NSObject {
    let options: BenchOptions
    let window: NSWindow?
    let bench: CanvasBench

    init(options: BenchOptions) {
        var o = options
        // the bench's own switches (--hide-wires, --offscreen, the wire switches), read here so
        // the app's startup needn't know them
        o.take(all: CommandLine.arguments)
        self.options = o
        bench = CanvasBench(options: o, subsystem: AppID.bundle + ".bench")
        if o.offscreen {
            window = nil
        } else {
            let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1440, height: 900), styleMask: [.titled, .closable, .resizable],
                             backing: .buffered, defer: false)
            w.title = "Plummer: benchmark"
            window = w
        }
        super.init()
        if let w = window {
            w.center()
            // a scripted launch never takes the keyboard (N4 slice 1): shown, not activated
            if CoreHost.options.scratchState {
                w.orderFrontRegardless()
            } else {
                w.makeKeyAndOrderFront(nil)
                NSApp.activate()
            }
        }
        _ = ProcessInfo.processInfo.beginActivity(options: [.userInitiated, .latencyCritical], reason: "benchmark")
        bench.maxFPS = { [weak self] in self?.window?.screen?.maximumFramesPerSecond ?? 120 }
        Task { await run() }
    }

    func run() async {
        guard let core = CoreHost.core else { return finish(["error": "no core"]) }
        let w = window
        let view = CGRect(origin: .zero, size: options.view)
        let r = await bench.run(core: core, ready: CoreHost.ready) { model in
            guard let w else { return CanvasView(model: model, frame: view) }
            let v = CanvasView(model: model, frame: w.contentLayoutRect)
            v.autoresizingMask = [.width, .height]
            w.contentView = v
            return v
        }
        // whether the run can be trusted: a bench behind another app's window, or in an app the
        // system didn't let come forward, is throttled (its frames and its threads' priority)
        var out = r
        out["app_active"] = .bool(NSApp.isActive)
        out["window_visible"] = .bool(w?.occlusionState.contains(.visible) ?? false)
        try? JSONValue.object(out).serialized().write(to: URL(fileURLWithPath: options.out))
        finish(out)
    }

    func finish(_ r: [String: JSONValue]) {
        CoreHost.core?.close()
        exit(r["error"] == nil ? 0 : 1)
    }
}
