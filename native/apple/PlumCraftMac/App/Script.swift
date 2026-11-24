// `--then STEP;STEP;…`: what a script asks the first window to do once it is ready, so its
// screenshots show menus, a run and sheets. Each step's state goes to `--status-file`
// (`step`, `menuWindows`, `sheet`), which the script waits on (with a time limit) before it takes
// a picture of the app's own windows by number. Menus close by themselves after `hold` seconds;
// a sheet a step opens is closed by a later step (`end-sheet`) or by the script answering its ask
// over the control port. Nothing here waits on the main thread.
//
//   run                       Run (as the toolbar does)
//   watch:NODE:PORT           watch an output (run.watch)
//   select:ID,ID              select nodes
//   wait:SECONDS              wait
//   wait-file:PATH            wait (120 s at most) until PATH exists (a script says go)
//   waitrun                   wait (60 s at most) until nothing runs
//   menu:TITLE[:HOLD]         open the bar's menu TITLE (File, Edit, …) for HOLD seconds (5)
//   context:NODE[:HOLD]       open NODE's context menu where its face is (`selected`: the first
//                             node selected)
//   saveas                    File › Save As… as the person would: the core asks, a save sheet shows
//   doc-saveas                NSDocument's own Save As… panel (the File menu's): its name and format
//                             in the status (`savePanel`, FR2); a later `end-sheet` cancels it
//   close                     File › Close: NSDocument's unsaved-changes sheet when edited
//   end-sheet                 close the window's sheet (Cancel)
//   theme:dark|light          the app's appearance
//   quit                      quit
//   inbox:DIR                 run the steps a script writes as DIR/<k>.step, one at a time
//                             (the person check, PersonSteps.swift)
//   press, moveto, release, release-cancel, palette, type, pick, dismiss: the editing gestures'
//                             steps (PlumCraftShell's CanvasSteps, shared with the iPad)

import AppKit
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

@MainActor
struct Script {
    let window: GraphWindowController
    private static var claimed = false
    /// The menu a step holds open, so a TERM can close it before quitting.
    static var tracking: NSMenu?

    /// Only the first window runs the steps.
    static func claim() -> Bool {
        if claimed { return false }
        claimed = true
        return true
    }

    func run(_ steps: [String]) async {
        for (k, step) in steps.enumerated() {
            Status.shared.set("step", ["index": .int(Int64(k)), "name": .string(step), "done": false])
            await one(step)
            Status.shared.set("step", ["index": .int(Int64(k)), "name": .string(step), "done": true])
        }
        Status.shared.set("script", "done")
    }

    func one(_ step: String) async {
        let parts = step.split(separator: ":", omittingEmptySubsequences: false).map(String.init)
        guard let head = parts.first, let v = window.canvasView else { return }
        let m = v.model
        switch head {
        case "inbox" where parts.count >= 2:
            // the person check's steps, written by the script one at a time (PersonSteps.swift)
            await PersonSteps.inbox(parts[1...].joined(separator: ":"), first: window)
        case "run":
            window.runGraph(nil)
        case "watch" where parts.count >= 3:
            _ = try? await m.run("run.watch", ["node": .string(parts[1]), "port": .string(parts[2])])
        case "select" where parts.count >= 2:
            _ = try? await m.run("selection.set", ["nodes": .array(parts[1].split(separator: ",").map { .string(String($0)) })])
        case "wait" where parts.count >= 2:
            try? await Task.sleep(for: .seconds(Double(parts[1]) ?? 1))
        case "wait-file" where parts.count >= 2:
            let path = parts[1...].joined(separator: ":")
            let deadline = ContinuousClock.now + .seconds(120)
            while !FileManager.default.fileExists(atPath: path) && ContinuousClock.now < deadline {
                try? await Task.sleep(for: .milliseconds(100))
            }
        case "waitrun":
            _ = try? await m.core.request("ui.waitRun", ["timeout": 60], doc: m.requestDoc)
        case "menu" where parts.count >= 2:
            guard let bar = NSApp.mainMenu, let item = bar.items.first(where: { $0.title == parts[1] }), let menu = item.submenu,
                  let w = window.window, let content = w.contentView else { return }
            hold(menu, seconds: parts.count >= 3 ? Double(parts[2]) ?? 5 : 5) {
                menu.popUp(positioning: nil, at: NSPoint(x: 80, y: content.bounds.height - 8), in: content)
            }
        case "context" where parts.count >= 2:
            let id = parts[1] == "selected" ? (m.selection.nodes.first ?? "") : parts[1]
            guard let nid = v.canvas.nid(of: id), let L = v.canvas.faceLayout(nid: nid), let f = m.faces[nid]?.item else { return }
            let g = CGPoint(x: f.pos.x + L.size.width / 2, y: f.pos.y + 14)
            let p = v.canvas.viewport.toView(g)
            let r = try? await v.canvas.link.context(Point(Double(g.x), Double(g.y)))
            guard let ctx = r?.context, ctx != .null, let items = try? await m.contextMenu(ctx) else { return }
            let menu = ContextMenu.make(items, model: m)
            Status.shared.set("contextMenu", .array(items.map { .string($0.label) }))
            hold(menu, seconds: parts.count >= 3 ? Double(parts[2]) ?? 5 : 5) {
                menu.popUp(positioning: nil, at: p, in: v)
            }
        case "saveas":
            _ = try? await m.invoke("graph.saveAs", person: true)
        case "doc-saveas":
            (window.document as? NSDocument)?.saveAs(nil)
        case "close":
            window.window?.performClose(nil)
        case "end-sheet":
            if let w = window.window, let s = w.attachedSheet { w.endSheet(s, returnCode: .cancel) }
            Status.shared.set("sheet", .null)
        case "about":
            // the About panel, ordered as a scripted launch orders windows (N4 slice 4)
            AboutPanel.show(model: window.canvasView?.model, over: window.window)
        case "theme" where parts.count >= 2:
            NSApp.appearance = NSAppearance(named: parts[1] == "dark" ? .darkAqua : .aqua)
        case "quit":
            // a scripted launch quits without asking about unsaved changes, as its TERM does (N4
            // slice 1: an edited graph's review alert waited for nobody and blocked the TERM)
            if CoreHost.options.scratchState {
                for d in NSDocumentController.shared.documents { d.updateChangeCount(.changeCleared) }
            }
            NSApp.terminate(nil)
        case "windows":
            // every window of this app on screen, for a script's pictures by number (a popover is
            // its own window)
            Status.shared.set("allWindows", .array(Self.allWindows()))
        case "shot-ready":
            // a script's picture can be taken now (it waits on the step's `done`)
            try? await Task.sleep(for: .milliseconds(500))
        default:
            // the editing gestures' steps, shared with the iPad (N4 slice 1)
            if await !CanvasSteps(canvas: v.canvas, drag: v.drag, palette: v.palette, rows: v.rowEditor, field: v.fieldEditor).run(parts) {
                NSLog("Plummer: --then: unknown step %@", step)
            }
        }
    }

    /// Open `menu` (which tracks until it closes) and close it after `seconds`, with the
    /// menu's windows written to the status file meanwhile. Main-actor tasks run while a menu
    /// tracks (the main queue is served in the tracking run loop mode).
    private func hold(_ menu: NSMenu, seconds: Double, open: () -> Void) {
        // a menu tracks in its own run loop mode, where neither tasks nor the main queue run:
        // run-loop timers in the common modes do
        let note = Timer(timeInterval: 0.6, repeats: false) { _ in
            MainActor.assumeIsolated { Status.shared.set("menuWindows", .array(Self.menuWindows().map { .int(Int64($0)) })) }
        }
        nonisolated(unsafe) let held = menu
        let close = Timer(timeInterval: max(seconds, 0.8), repeats: false) { _ in
            MainActor.assumeIsolated { held.cancelTracking() }
        }
        RunLoop.main.add(note, forMode: .common)
        RunLoop.main.add(close, forMode: .common)
        Self.tracking = menu
        open()
        Self.tracking = nil
        note.invalidate()
        close.invalidate()
        Status.shared.set("menuWindows", .array([]))
    }

    /// This app's windows on screen, front first: number, layer and bounds.
    static func allWindows() -> [JSONValue] {
        let pid = ProcessInfo.processInfo.processIdentifier
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else { return [] }
        return list.compactMap { w in
            guard (w[kCGWindowOwnerPID as String] as? Int32) == pid, let n = w[kCGWindowNumber as String] as? Int else { return nil }
            let b = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
            return ["n": .int(Int64(n)), "layer": .int(Int64(w[kCGWindowLayer as String] as? Int ?? 0)),
                    "bounds": [.double(b["X"] ?? 0), .double(b["Y"] ?? 0), .double(b["Width"] ?? 0), .double(b["Height"] ?? 0)]]
        }
    }

    /// This app's windows above the normal level (open menus), front first.
    static func menuWindows() -> [Int] {
        let pid = ProcessInfo.processInfo.processIdentifier
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else { return [] }
        return list.compactMap { w in
            guard (w[kCGWindowOwnerPID as String] as? Int32) == pid, let layer = w[kCGWindowLayer as String] as? Int, layer > 0,
                  let n = w[kCGWindowNumber as String] as? Int else { return nil }
            return n
        }
    }
}
