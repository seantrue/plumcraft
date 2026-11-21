// The scripted budget run (plan section 10): the kit's CanvasBench (PlumCraftShell, shared with
// the iPad app) in a window of its own. Writes JSON to `--bench OUT` and exits.

import AppKit
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

@MainActor
final class BenchWindow: NSObject {
    let options: BenchOptions
    let window: NSWindow
    let bench: CanvasBench

    init(options: BenchOptions) {
        self.options = options
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1440, height: 900), styleMask: [.titled, .closable, .resizable],
                          backing: .buffered, defer: false)
        window.title = "Plummer: benchmark"
        bench = CanvasBench(options: options, subsystem: AppID.bundle + ".bench")
        super.init()
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.activate()
        _ = ProcessInfo.processInfo.beginActivity(options: [.userInitiated, .latencyCritical], reason: "benchmark")
        bench.maxFPS = { [weak self] in self?.window.screen?.maximumFramesPerSecond ?? 60 }
        Task { await run() }
    }

    func run() async {
        guard let core = CoreHost.core else { return finish(["error": "no core"]) }
        let w = window
        let r = await bench.run(core: core, ready: CoreHost.ready) { model in
            let v = CanvasView(model: model, frame: w.contentLayoutRect)
            v.autoresizingMask = [.width, .height]
            w.contentView = v
            return v
        }
        // whether the run can be trusted: a bench behind another app's window, or in an app the
        // system didn't let come forward, is throttled (its frames and its threads' priority)
        var out = r
        out["app_active"] = .bool(NSApp.isActive)
        out["window_visible"] = .bool(w.occlusionState.contains(.visible))
        try? JSONValue.object(out).serialized().write(to: URL(fileURLWithPath: options.out))
        finish(out)
    }

    func finish(_ r: [String: JSONValue]) {
        CoreHost.core?.close()
        exit(r["error"] == nil ? 0 : 1)
    }
}
