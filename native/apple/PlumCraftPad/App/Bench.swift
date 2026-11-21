// The budget run on the iPad (plan section 10): the kit's CanvasBench (PlumCraftShell, shared with
// the Mac app) full screen. Writes JSON to `--bench OUT` (in Documents unless absolute), says
// "BENCH {…}" on stdout and in the unified log, and exits.

import os
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import UIKit

@MainActor
final class BenchViewController: UIViewController {
    let options: BenchOptions
    var bench: CanvasBench?

    init(options: BenchOptions) {
        var o = options
        if !o.out.hasPrefix("/") { o.out = CoreHost.dirs.documents.appendingPathComponent(o.out).path }
        // an earlier run's results go first, so a script that reads after a launch the system
        // refused (a locked iPad) finds nothing rather than an old run's numbers (the N4 device gate)
        let fm = FileManager.default
        let docs = CoreHost.dirs.documents
        for f in (try? fm.contentsOfDirectory(atPath: docs.path)) ?? [] where f.hasPrefix("bench-") && f.hasSuffix(".json") {
            try? fm.removeItem(at: docs.appendingPathComponent(f))
        }
        self.options = o
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    override var prefersStatusBarHidden: Bool { true }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        guard bench == nil else { return }
        UIApplication.shared.isIdleTimerDisabled = true
        let b = CanvasBench(options: options, subsystem: "com.plumcraft.pad.bench")
        bench = b
        b.maxFPS = { [weak self] in self?.view.window?.windowScene?.screen.maximumFramesPerSecond ?? 60 }
        Task { await run(b) }
    }

    func run(_ b: CanvasBench) async {
        guard let core = CoreHost.core, let host = viewIfLoaded else { return finish(["error": .string(CoreHost.openError ?? "no core")]) }
        let r = await b.run(core: core, ready: CoreHost.ready) { model in
            let v = CanvasView(model: model, frame: host.bounds)
            v.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            host.addSubview(v)
            return v
        }
        finish(r)
    }

    func finish(_ r: [String: JSONValue]) {
        CoreHost.say("info", "BENCH " + JSONValue.object(r).text)
        StatusFile.shared.set("bench", .object(r))
        CoreHost.core?.close()
        exit(r["error"] == nil ? 0 : 1)
    }
}
