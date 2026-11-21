// `--then STEP;STEP;…`: what a script asks the first graph to do once it is ready, so the
// simulator's screenshots (`simctl io screenshot`, taken from the Mac) show a run, the viewers and
// a context menu. Each step's state goes to `--status-file` (`step`), which the script waits on
// with a time limit. Nothing here waits on the main thread.
//
//   run                       Run (as the bar's button does)
//   watch:NODE:PORT           watch an output (run.watch)
//   select:ID,ID              select nodes
//   wait:SECONDS              wait
//   wait-file:PATH            wait (120 s at most) until PATH exists (a script says go)
//   waitrun                   wait (60 s at most) until nothing runs
//   fit                       fit everything
//   zoom:Z[:ID]               zoom Z, centred on node ID when named
//   theme:dark|light          the window's appearance
//   close                     close the graph (back to the document browser)
//   compact:on|off            the window's horizontal size class forced compact (what Split View and
//                             a narrow Stage Manager window give; the simulator can't be split by a
//                             script), so the parameters and a pop-out are sheets

import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import UIKit

@MainActor
struct Script {
    let graph: GraphViewController
    private static var claimed = false

    static func claim() -> Bool {
        if claimed { return false }
        claimed = true
        return true
    }

    func run(_ steps: [String]) async {
        for (k, step) in steps.enumerated() {
            StatusFile.shared.set("step", ["index": .int(Int64(k)), "name": .string(step), "done": false])
            await one(step)
            StatusFile.shared.set("step", ["index": .int(Int64(k)), "name": .string(step), "done": true])
        }
        StatusFile.shared.set("script", "done")
    }

    private func one(_ step: String) async {
        let parts = step.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
        guard let head = parts.first, let v = graph.canvasView else { return }
        let m = v.model
        switch head {
        case "run":
            graph.runGraph()
        case "watch" where parts.count >= 3:
            _ = try? await m.run("run.watch", ["node": .string(parts[1]), "port": .string(parts[2])])
        case "select" where parts.count >= 2:
            _ = try? await m.run("selection.set", ["nodes": .array(parts[1].split(separator: ",").map { .string(String($0)) })])
        case "wait" where parts.count >= 2:
            try? await Task.sleep(for: .seconds(Double(parts[1]) ?? 1))
        case "wait-file" where parts.count >= 2:
            let path = parts.dropFirst().joined(separator: ":")
            let deadline = ContinuousClock.now + .seconds(120)
            while !FileManager.default.fileExists(atPath: path) && ContinuousClock.now < deadline {
                try? await Task.sleep(for: .milliseconds(100))
            }
        case "waitrun":
            // the run starts, then nothing runs (at most 60 s)
            _ = await m.wait(timeout: .seconds(3)) { m.menuBar.item(command: "run.cancel")?.enabled == true }
            _ = await m.wait(timeout: .seconds(60)) { m.menuBar.item(command: "run.cancel")?.enabled != true }
            _ = await m.wait(timeout: .seconds(10)) { !m.anim.on }
        case "fit":
            v.fitView()
            try? await Task.sleep(for: .milliseconds(400))
        case "zoom" where parts.count >= 2:
            let z = CGFloat(Double(parts[1]) ?? 1)
            if parts.count >= 3, let nid = v.canvas.nid(of: parts[2]), let L = v.canvas.faceLayout(nid: nid), let f = m.faces[nid]?.item {
                let c = CGPoint(x: f.pos.x + L.size.width / 2, y: f.pos.y + L.size.height / 2)
                let size = v.canvas.viewSize
                v.canvas.setViewport(Viewport(pan: CGPoint(x: size.width / 2 - c.x * z, y: size.height / 2 - c.y * z), zoom: z))
            } else {
                v.canvas.zoom(to: z, animated: false)
            }
        case "theme" where parts.count >= 2:
            graph.view.window?.overrideUserInterfaceStyle = parts[1] == "dark" ? .dark : .light
            _ = await m.wait(timeout: .seconds(3)) { (m.theme?.name ?? "") == (parts[1] == "dark" ? "dusk" : "paper") }
            try? await Task.sleep(for: .milliseconds(500))
        case "close":
            graph.close {}
            try? await Task.sleep(for: .seconds(1.5))
        case "compact" where parts.count >= 2:
            if let w = graph.view.window {
                if parts[1] == "on" { w.traitOverrides.horizontalSizeClass = .compact } else { w.traitOverrides.remove(UITraitHorizontalSizeClass.self) }
            }
            try? await Task.sleep(for: .seconds(1.2))
        case "select-mode":
            if (parts.count < 2 || parts[1] == "on") != v.selectMode { graph.toggleSelectMode() }
            _ = await m.wait(timeout: .seconds(3)) { (parts.count < 2 || parts[1] == "on") == m.view.selectMode }
        default:
            // the editing gestures' steps, shared with the Mac (N4 slice 1)
            _ = await CanvasSteps(canvas: v.canvas, drag: v.drag, palette: v.palette, rows: v.rowEditor, field: v.fieldEditor).run(parts)
        }
    }
}
