// The person check's steps (person-check.sh, decisions after B70 to B73: "Add it to every shell's
// gate"). A scripted launch otherwise drives the core over its control port, where every ask is a
// script's and no sheet is shown (Asks.swift), so B73's forms were never seen. These steps act
// where a person acts: a menu item of the real menu bar is performed as a pick of it is (its
// action sent to its target, or down the window's responder chain as AppKit sends it), a sheet's
// fields are filled and its buttons clicked, a file is opened as the Finder or Open Recent opens
// it. What they see is reported from the app's own windows: the documents, the menu's items as
// AppKit validated them, and a sheet's controls with their frames and a picture of it.
//
// `--then inbox:DIR` runs the steps a script writes as DIR/<k>.step (k = 0, 1, …, one step a
// file) in order, writing DIR/<k>.done (JSON) after each; it stops after 15 minutes with no step.
// Besides Script's own steps:
//
//   use:K                    the K-th graph window (document order) is the one steps act on; its
//                            document becomes the menu bar's, as clicking into it does
//   docs                     the documents: title, file, edited, core document, window, sheet
//   menu:PATH                the items under PATH (`Engine`, `File/Open Recent`): title, enabled,
//                            tooltip, checked, as AppKit validated them just now
//   menu-pick:PATH           pick the item at PATH (`File/New`, `Engine/Install Engine…`; a
//                            trailing `…` is optional); refused, with why, when it is disabled
//   sheet[:OUT.png]          the window's sheet: its controls (kind, text, accessibility label,
//                            frame in the sheet) and, with OUT, a picture of it from the sheet's
//                            own drawing
//   sheet-set:LABEL=VALUE    type VALUE into the sheet's field labelled LABEL (a pop-up: choose the
//                            item with that title)
//   sheet-press:TITLE        click the sheet's button TITLE
//   panel-at:PATH            the next save or open panel is set, before it shows, to open at
//                            PATH's folder (a save panel with PATH's name); the scripted copy
//                            only. The person check reads the panel's own fields through
//                            accessibility and cancels: it never presses Save or Open (the
//                            maintainer, 2026-11-21, after a save landed in ~/Documents)
//   context-pick:NODE:PATH   pick PATH (`Run on/Person Check Text`) in NODE's context menu, the menu a
//                            secondary click on its face shows
//   open:PATH                open PATH as the Finder and Open Recent do
//   dock-reopen              what a click on the Dock icon does with no window showing

import AppKit
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

@MainActor
enum PersonSteps {
    /// Where the next panel opens (`panel-at:PATH`), taken once by the panel that uses it.
    static var panelAt: URL?

    static func takePanelAt() -> URL? {
        defer { panelAt = nil }
        return panelAt
    }

    /// The graph windows, in the order their documents were opened.
    static func graphWindows() -> [GraphWindowController] {
        NSDocumentController.shared.documents.compactMap { ($0.windowControllers.first as? GraphWindowController) }
    }

    /// Run the steps DIR/<k>.step in order (Script's own steps too), each acting on the window
    /// `use:` chose last (the first window at the start).
    static func inbox(_ dir: String, first: GraphWindowController) async {
        var current = first
        var k = 0
        while true {
            let step = URL(fileURLWithPath: dir).appendingPathComponent("\(k).step")
            let deadline = ContinuousClock.now + .seconds(900)
            while !FileManager.default.fileExists(atPath: step.path) {
                if ContinuousClock.now > deadline { return }
                try? await Task.sleep(for: .milliseconds(50))
            }
            let text = ((try? String(contentsOf: step, encoding: .utf8)) ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            var out: [String: JSONValue] = ["index": .int(Int64(k)), "step": .string(text)]
            if text.hasPrefix("use:") {
                let n = Int(text.dropFirst(4)) ?? 0
                let all = graphWindows()
                if n >= 0, n < all.count {
                    current = all[n]
                    // what a click into the window does to the bar (windowDidBecomeKey)
                    if let m = current.canvasView?.model { MenuBar.shared.show(m) }
                    out["ok"] = true
                } else {
                    out["ok"] = false
                    out["error"] = .string("there are \(all.count) graph windows")
                }
            } else if let r = await run(text, window: current) {
                for (key, v) in r { out[key] = v }
            } else {
                await Script(window: current).one(text)
                out["ok"] = true
            }
            let done = URL(fileURLWithPath: dir).appendingPathComponent("\(k).done")
            try? JSONValue.object(out).serialized().write(to: done, options: .atomic)
            k += 1
        }
    }

    /// One of the steps above, or nil when `step` is none of them.
    static func run(_ step: String, window w: GraphWindowController) async -> [String: JSONValue]? {
        let (head, arg) = split(step)
        switch head {
        case "docs":
            return ["ok": true, "docs": .array(docs())]
        case "menu":
            guard let m = menu(at: arg) else { return ["ok": false, "error": .string("no menu \(arg)")] }
            // Open Recent is AppKit's: its delegate fills it as opening it does (the app's own
            // menus are the menu bar's, rebuilt for the window in use: never asked here, or the
            // bar follows another window)
            if arg.hasSuffix("Open Recent") { m.delegate?.menuNeedsUpdate?(m) }
            m.update()
            return ["ok": true, "items": .array(m.items.filter { !$0.isHidden }.map { describe($0, w) }),
                    "recent": .array(NSDocumentController.shared.recentDocumentURLs.map { .string($0.path) })]
        case "menu-pick":
            return await pick(arg, window: w)
        case "sheet":
            guard let s = w.window?.attachedSheet else { return ["ok": false, "error": "no sheet on the window"] }
            var r: [String: JSONValue] = ["ok": true, "sheet": sheet(s)]
            if !arg.isEmpty { r["picture"] = picture(s, to: arg) }
            return r
        case "sheet-set":
            guard let s = w.window?.attachedSheet, let eq = arg.firstIndex(of: "=") else { return ["ok": false, "error": "no sheet, or not LABEL=VALUE"] }
            return set(in: s, label: String(arg[..<eq]), value: String(arg[arg.index(after: eq)...]))
        case "sheet-press":
            guard let s = w.window?.attachedSheet else { return ["ok": false, "error": "no sheet on the window"] }
            guard let b = controls(s.contentView).compactMap({ $0 as? NSButton }).first(where: { $0.title == arg && $0.isEnabled }) else {
                return ["ok": false, "error": .string("no enabled button \(arg) on the sheet")]
            }
            await onRunLoop { b.performClick(nil) }
            return ["ok": true]
        case "panel-cancel":
            // the save or open panel cancelled from inside the app, when accessibility can't
            // press its Cancel (the screen locked): never left open, never Save or Open
            let open = (w.window?.attachedSheet as? NSSavePanel) ?? NSApp.windows.compactMap({ $0 as? NSOpenPanel }).first(where: \.isVisible)
            guard let p = open else { return ["ok": false, "error": "no panel"] }
            await onRunLoop {
                if let sheetOf = p.sheetParent { sheetOf.endSheet(p, returnCode: .cancel) } else { p.cancel(nil) }
            }
            return ["ok": true]
        case "panel-at":
            // the next save or open panel opens at PATH's folder (a save panel with PATH's name),
            // set before it is shown (GraphDocument.prepareSavePanel, Asks): the scripted copy only
            guard ScratchGuard.bundleRefusal(Bundle.main.bundleIdentifier) == nil else {
                return ["ok": false, "error": "panel-at: only the scripted copy points its panels"]
            }
            panelAt = URL(fileURLWithPath: arg)
            return ["ok": true]
        case "open":
            let url = URL(fileURLWithPath: arg).standardizedFileURL
            let r: [String: JSONValue] = await withCheckedContinuation { c in
                RunLoop.main.perform(inModes: [.common]) {
                    MainActor.assumeIsolated {
                        NSDocumentController.shared.openDocument(withContentsOf: url, display: true) { d, already, error in
                            c.resume(returning: ["ok": .bool(error == nil && d != nil), "already": .bool(already),
                                                 "error": error.map { .string($0.localizedDescription) } ?? .null])
                        }
                    }
                }
            }
            return r
        case "context-pick":
            // NODE:PATH: the node's context menu as the app builds it for a secondary click, and
            // the item at PATH (`Run on/Person Check Text`) picked in it
            let (node, path) = split(arg)
            guard let m = w.canvasView?.model, let items = try? await m.contextMenu(["kind": "node", "node": .string(node)]) else {
                return ["ok": false, "error": .string("no context menu for \(node)")]
            }
            let top = ContextMenu.make(items, model: m)
            var menu: NSMenu = top
            let parts = path.split(separator: "/").map(String.init)
            for (k, p) in parts.enumerated() {
                menu.update()
                guard let i = menu.items.firstIndex(where: { matches($0.title, p) }) else {
                    return ["ok": false, "error": .string("no item \(p)"), "items": .array(menu.items.map { .string($0.title) })]
                }
                if k == parts.count - 1 {
                    guard menu.items[i].isEnabled else { return ["ok": false, "error": .string("\(p) is disabled")] }
                    let m = menu
                    await onRunLoop { withExtendedLifetime(top) { m.performActionForItem(at: i) } }
                    return ["ok": true]
                }
                guard let s = menu.items[i].submenu else { return ["ok": false, "error": .string("\(p) has no submenu")] }
                menu = s
            }
            return ["ok": false, "error": "empty path"]
        case "core-drag":
            // KEY|ID|FX,FY|TX,TY: in the core window KEY (`editor:n`), a mouse drag from fraction
            // (FX, FY) of the element ID's frame (`curves-plot`) to (TX, TY), as a person's: down,
            // a few drags, up, sent to the window (S4a: the curves editor's drag on the Mac)
            let parts = arg.split(separator: "|").map(String.init)
            func pair(_ s: String) -> (CGFloat, CGFloat)? {
                let v = s.split(separator: ",").compactMap { Double($0) }
                return v.count == 2 ? (CGFloat(v[0]), CGFloat(v[1])) : nil
            }
            guard parts.count == 4, let from = pair(parts[2]), let to = pair(parts[3]) else {
                return ["ok": false, "error": "core-drag: KEY|ID|FX,FY|TX,TY"]
            }
            guard let win = NSApp.windows.first(where: { $0.identifier?.rawValue == "core-\(parts[0])" }), let content = win.contentView else {
                return ["ok": false, "error": .string("no window \(parts[0])")]
            }
            guard let screen = element(parts[1], in: content, depth: 0)?.accessibilityFrame?(), screen.width > 0 else {
                return ["ok": false, "error": .string("no element \(parts[1]) in \(parts[0])")]
            }
            // the element's frame in the window (accessibility frames are the screen's, y up)
            let r = win.convertFromScreen(screen)
            func at(_ f: (CGFloat, CGFloat)) -> NSPoint { NSPoint(x: r.minX + f.0 * r.width, y: r.maxY - f.1 * r.height) }
            let a = at(from), b = at(to)
            await onRunLoop {
                // key within the app (never activating it): a click into a window that isn't key
                // only makes it key, as a person's first click does
                win.makeKey()
                func send(_ type: NSEvent.EventType, _ p: NSPoint) {
                    if let e = NSEvent.mouseEvent(with: type, location: p, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                                                  windowNumber: win.windowNumber, context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseUp ? 0 : 1) {
                        // to the view under the point, as AppKit routes a click there (the window's own
                        // routing drops a click into a window that isn't on screen: a locked screen)
                        let at = content.convert(p, from: nil)
                        let v = content.hitTest(at) ?? content
                        switch type {
                        case .leftMouseDown: v.mouseDown(with: e)
                        case .leftMouseDragged: v.mouseDragged(with: e)
                        default: v.mouseUp(with: e)
                        }
                    }
                }
                send(.leftMouseDown, a)
                for k in 1...8 {
                    let t = CGFloat(k) / 8
                    send(.leftMouseDragged, NSPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t))
                }
                send(.leftMouseUp, b)
            }
            return ["ok": true, "frame": [.double(Double(r.minX)), .double(Double(r.minY)), .double(Double(r.width)), .double(Double(r.height))]]
        case "core-undo":
            // Edit › Undo as AppKit routes it in the core window KEY (`editor:n`), as when it is
            // key: its first responder up the responder chain, then the window and its delegate.
            // A scripted launch is never active, so no window is key and an action sent to nil
            // goes nowhere; this walks the same chain from that window.
            guard let win = NSApp.windows.first(where: { $0.identifier?.rawValue == "core-\(arg)" }) else {
                return ["ok": false, "error": .string("no window \(arg)")]
            }
            let sel = #selector(UndoActions.undo(_:))
            var sent = false
            await onRunLoop {
                sent = win.firstResponder?.tryToPerform(sel, with: nil) ?? false
                if !sent { sent = win.tryToPerform(sel, with: nil) }
            }
            return ["ok": .bool(sent), "window": .string(arg)]
        case "dock-reopen":
            let visible = NSApp.windows.contains { $0.isVisible && $0.windowController is GraphWindowController }
            (NSApp.delegate as? AppDelegate)?.reopen(hasVisibleWindows: visible)
            return ["ok": true, "hadVisible": .bool(visible)]
        default:
            return nil
        }
    }

    /// The accessibility element with identifier `id` under `node` (the app's own tree, read in
    /// process: no accessibility permission needed).
    static func element(_ id: String, in node: Any, depth: Int) -> AnyObject? {
        guard depth < 40 else { return nil }
        let o = node as AnyObject
        if let ident = o.accessibilityIdentifier?(), ident == id { return o }
        for c in o.accessibilityChildren?() ?? [] {
            if let hit = element(id, in: c, depth: depth + 1) { return hit }
        }
        return nil
    }

    /// Run `f` from the main run loop itself, as an event's handling runs, never inside this
    /// task's job: AppKit work that turns the run loop (a menu's tracking, a panel) inside a
    /// task's job ran other main-actor code with the task's executor still current, and the app
    /// crashed in the runtime's isolation check (two runs of the first person check). A person's
    /// pick never runs inside a task.
    static func onRunLoop(_ f: @escaping @MainActor () -> Void) async {
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            RunLoop.main.perform(inModes: [.common]) {
                MainActor.assumeIsolated { f() }
                c.resume()
            }
        }
    }

    static func split(_ s: String) -> (String, String) {
        guard let c = s.firstIndex(of: ":") else { return (s, "") }
        return (String(s[..<c]), String(s[s.index(after: c)...]))
    }

    // MARK: Documents

    static func docs() -> [JSONValue] {
        NSDocumentController.shared.documents.map { d in
            let w = d.windowControllers.first?.window
            return ["title": .string(d.displayName), "path": .string(d.fileURL?.path ?? ""), "edited": .bool(d.isDocumentEdited),
                    "doc": (d as? GraphDocument)?.doc.map(JSONValue.uint) ?? .null, "window": .int(Int64(w?.windowNumber ?? 0)),
                    "visible": .bool(w?.isVisible ?? false), "windowTitle": .string(w?.title ?? ""),
                    "sheet": w?.attachedSheet.map { .string(String(describing: type(of: $0))) } ?? .null,
                    "ready": .bool((d.windowControllers.first as? GraphWindowController)?.readyShown ?? false)]
        }
    }

    // MARK: Menus

    /// A title as a path part matches it: exactly, or with the `…` left off.
    static func matches(_ title: String, _ part: String) -> Bool {
        title == part || title == part + "…" || title.replacingOccurrences(of: "…", with: "") == part.replacingOccurrences(of: "…", with: "")
    }

    static func item(at path: String) -> (NSMenu, Int)? {
        guard var m = NSApp.mainMenu else { return nil }
        let parts = path.split(separator: "/").map(String.init)
        for (k, p) in parts.enumerated() {
            m.update()
            guard let i = m.items.firstIndex(where: { matches($0.title, p) }) else { return nil }
            if k == parts.count - 1 { return (m, i) }
            guard let sub = m.items[i].submenu else { return nil }
            m = sub
        }
        return nil
    }

    static func menu(at path: String) -> NSMenu? {
        guard let (m, i) = item(at: path) else { return nil }
        return m.items[i].submenu
    }

    static func describe(_ mi: NSMenuItem, _ w: GraphWindowController) -> JSONValue {
        if mi.isSeparatorItem { return ["separator": true] }
        if mi.isSectionHeader { return ["heading": .string(mi.title)] }
        return ["title": .string(mi.title), "enabled": .bool(enabled(mi, w)), "tooltip": mi.toolTip.map(JSONValue.string) ?? .null,
                "checked": .bool(mi.state == .on), "submenu": .bool(mi.submenu != nil),
                // the item's image as drawn (a warned engine's symbol carries its warning)
                "image": mi.image.map { .string($0.accessibilityDescription ?? "image") } ?? .null]
    }

    /// Where a pick of `mi` goes: its target, AppKit's answer, or the chain from `w`.
    static func target(_ mi: NSMenuItem, _ w: GraphWindowController) -> AnyObject? {
        guard let action = mi.action else { return nil }
        return mi.target ?? NSApp.target(forAction: action, to: nil, from: mi) as AnyObject? ?? chainTarget(action, w)
    }

    /// Whether `mi` is enabled as a person sees it with `w` key and main: the target's own
    /// validation (a launch in the background has no key window, so AppKit's own pass finds no
    /// responder for Save, Close and the like and greys them).
    static func enabled(_ mi: NSMenuItem, _ w: GraphWindowController) -> Bool {
        if mi.submenu != nil { return mi.isEnabled }
        guard let t = target(mi, w) else { return false }
        if let v = t as? NSMenuItemValidation { return v.validateMenuItem(mi) }
        if let v = t as? NSUserInterfaceValidations { return v.validateUserInterfaceItem(mi) }
        return true
    }

    /// A pick of the item at `path`, as a click on it sends it: validated first (a disabled item
    /// isn't sent), then its action to its target, or down the chain AppKit uses for the main
    /// window (`w`: a launch in the background has no key or main window, so the chain is walked
    /// from `w` as it would be from the window a person clicked in).
    static func pick(_ path: String, window w: GraphWindowController) async -> [String: JSONValue] {
        guard let (m, i) = item(at: path) else { return ["ok": false, "error": .string("no menu item \(path)")] }
        m.update()
        let mi = m.items[i]
        var r: [String: JSONValue] = ["item": describe(mi, w)]
        guard let action = mi.action else { r["ok"] = false; r["error"] = "the item has no action"; return r }
        guard let target = target(mi, w) else { r["ok"] = false; r["error"] = .string("nothing answers \(action)"); return r }
        guard enabled(mi, w) else { r["ok"] = false; r["error"] = .string("disabled: \(mi.toolTip ?? "no reason given")"); return r }
        await onRunLoop { NSApp.sendAction(action, to: target, from: mi) }
        r["ok"] = true
        r["target"] = .string(String(describing: type(of: target)))
        return r
    }

    /// AppKit's chain for a window that is key and main: its first responder up the responder
    /// chain, the window, its delegate, its document, then the application, its delegate and the
    /// document controller.
    static func chainTarget(_ action: Selector, _ w: GraphWindowController) -> AnyObject? {
        var r: NSResponder? = w.window?.firstResponder
        while let x = r {
            if x.responds(to: action) { return x }
            r = x.nextResponder
        }
        let rest: [AnyObject?] = [w.window, w.window?.delegate, w.document as AnyObject?, NSApp, NSApp.delegate, NSDocumentController.shared]
        return rest.compactMap { $0 }.first { $0.responds(to: action) }
    }

    // MARK: Sheets

    /// Every control in `v`, depth first.
    static func controls(_ v: NSView?) -> [NSView] {
        guard let v else { return [] }
        return [v] + v.subviews.flatMap { controls($0) }
    }

    /// The sheet's controls as a person sees them: kind, text, the label accessibility gives,
    /// the frame in the sheet's content (y down, points), and whether its text is cut off.
    static func sheet(_ s: NSWindow) -> JSONValue {
        guard let content = s.contentView else { return [:] }
        let h = content.bounds.height
        var out: [JSONValue] = []
        for v in controls(content) where !v.isHiddenOrHasHiddenAncestor {
            let kind: String
            var text = ""
            switch v {
            case let p as NSPopUpButton: kind = "popup"; text = p.titleOfSelectedItem ?? ""
            case let c as NSComboBox: kind = "combo"; text = c.stringValue
            case let b as NSButton: kind = "button"; text = b.title
            case let t as NSSecureTextField: kind = "secret"; text = t.stringValue.isEmpty ? "" : "•"
            case let t as NSTextField: kind = t.isEditable ? "field" : "label"; text = t.stringValue
            default: continue
            }
            let f = v.convert(v.bounds, to: content)
            var e: [String: JSONValue] = ["kind": .string(kind), "text": .string(text),
                                          "frame": [.double(f.minX), .double(h - f.maxY), .double(f.width), .double(f.height)]]
            if let l = v.accessibilityLabel(), !l.isEmpty { e["axLabel"] = .string(l) }
            if let c = v as? NSControl, kind == "label", text.hasSuffix(":") {
                // a label is cut off when its own text needs more room than it has
                let need = c.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: 10_000, height: f.height)) ?? .zero
                e["clipped"] = .bool(need.width > f.width + 0.5 || f.minX < -0.5 || h - f.maxY < -0.5)
            }
            if let p = v as? NSPopUpButton { e["choices"] = .array(p.itemTitles.map(JSONValue.string)) }
            if let c = v as? NSComboBox { e["choices"] = .array((0..<c.numberOfItems).compactMap { c.itemObjectValue(at: $0) as? String }.map(JSONValue.string)) }
            out.append(.object(e))
        }
        return ["size": [.double(content.bounds.width), .double(h)], "controls": .array(out),
                "class": .string(String(describing: type(of: s)))]
    }

    /// A picture of the sheet, from its own drawing (the window server's picture needs a display
    /// that is awake, which a scripted run can't count on).
    static func picture(_ s: NSWindow, to path: String) -> JSONValue {
        guard let content = s.contentView?.superview ?? s.contentView,
              let rep = content.bitmapImageRepForCachingDisplay(in: content.bounds) else { return "no picture" }
        content.cacheDisplay(in: content.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else { return "no picture" }
        do {
            try png.write(to: URL(fileURLWithPath: path), options: .atomic)
            return .string(path)
        } catch {
            return .string("\(error)")
        }
    }

    static func set(in s: NSWindow, label: String, value: String) -> [String: JSONValue] {
        for v in controls(s.contentView) {
            guard v.accessibilityLabel() == label else { continue }
            if let p = v as? NSPopUpButton {
                guard p.itemTitles.contains(value) else { return ["ok": false, "error": .string("\(label) has no choice \(value)")] }
                p.selectItem(withTitle: value)
                if let a = p.action { NSApp.sendAction(a, to: p.target, from: p) }
                return ["ok": true]
            }
            if let f = v as? NSTextField, f.isEditable {
                f.stringValue = value
                if let a = f.action { NSApp.sendAction(a, to: f.target, from: f) }
                return ["ok": true]
            }
        }
        return ["ok": false, "error": .string("no field labelled \(label) on the sheet")]
    }
}
