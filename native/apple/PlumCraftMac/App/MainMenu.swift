// The real menu bar (N3 slice 4), built from the core's menu model (`menu.get`) of the front
// document and kept current by the feed: an item's title, enabled state, check mark and tooltip
// (the reason it is disabled) are read from the model each time AppKit validates it, and the bar
// is rebuilt only when the model's structure changes. Items dispatch their command by id
// (`ui.invoke {command, by: "person"}`, or `command.run` with fixed params), never a shell-side
// interpretation.
//
// What the Mac places itself (the model's roles): About, Settings…, Services, Hide, Quit in the
// application menu; New, Open…, Open Recent, Close, Save, Save As… as NSDocument's; Undo, Redo,
// Cut, Copy, Paste, Delete, Select All as the standard actions down the responder chain, so a text
// field that is editing takes them and the canvas sends the core's commands otherwise; the Window
// and Help menus as the system's.

import AppKit
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

/// A menu item's place in the model, for validation (looked up again each time).
final class ModelRef: NSObject {
    let key: String
    init(_ key: String) { self.key = key }
}

@MainActor
final class MenuBar: NSObject, NSMenuItemValidation {
    static let shared = MenuBar()
    let appName = "PlumCraft"
    /// The model's items by their place in the bar ("2/5"), as last fetched.
    private var items: [String: MenuItem] = [:]
    private var shape = ""
    /// The document whose menus the bar shows.
    private(set) weak var source: DocumentModel?
    /// Called with the text of the built bar each time it is built or changes (`--dump-menus`).
    var onDump: ((String) -> Void)?

    // MARK: Building

    /// Show `model`'s menus (nil: the bar with no document).
    func show(_ model: DocumentModel?) {
        source = model
        rebuild(model?.menuBar ?? [], force: true)
    }

    /// The model changed: rebuild when its structure did, else validation picks up the rest.
    func modelChanged(_ model: DocumentModel) {
        guard model === source else { return }
        rebuild(model.menuBar, force: false)
    }

    private func rebuild(_ menus: [MenuItem], force: Bool) {
        var flat: [String: MenuItem] = [:]
        var sig = ""
        func walk(_ list: [MenuItem], _ path: String) {
            for (k, it) in list.enumerated() {
                let key = path.isEmpty ? "\(k)" : "\(path)/\(k)"
                flat[key] = it
                sig += "\(key)|\(it.id)|\(it.label)|\(it.command ?? "")|\(it.separator)|\(it.isSubmenu)|\(it.shortcut.map { "\($0.mods)\($0.key)" } ?? "")\n"
                walk(it.items, key)
            }
        }
        walk(menus, "")
        items = flat
        if !force && sig == shape {
            dump()
            return
        }
        shape = sig
        NSApp.mainMenu = make(menus)
        dump()
    }

    private func make(_ menus: [MenuItem]) -> NSMenu {
        let bar = NSMenu()
        let app = submenu(bar, appName)
        let about = app.addItem(withTitle: "About \(appName)", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        about.target = NSApp
        app.addItem(.separator())
        // Settings… from the model's preferences item, at its shortcut (⌘,)
        if let (key, it) = find(role: .preferences) {
            app.addItem(item(it, key: key))
            app.addItem(.separator())
        }
        let services = NSMenu(title: "Services")
        app.addItem(withTitle: "Services", action: nil, keyEquivalent: "").submenu = services
        NSApp.servicesMenu = services
        app.addItem(.separator())
        app.addItem(withTitle: "Hide \(appName)", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        app.addItem(withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h").keyEquivalentModifierMask = [.command, .option]
        app.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Quit \(appName)", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")

        var sawFile = false, sawEdit = false, sawView = false, help: NSMenu?
        for (k, top) in menus.enumerated() where top.isSubmenu {
            let m = submenu(bar, top.label)
            fill(m, top.items, path: "\(k)")
            switch top.role?.word {
            case .fileMenu?:
                sawFile = true
                fileExtras(m)
            case .editMenu?: sawEdit = true
            case .viewMenu?:
                sawView = true
                viewExtras(m)
            case .helpMenu?: help = m
            default: break
            }
        }
        if !sawFile {
            // no document yet: what NSDocumentController offers
            let m = NSMenu(title: "File")
            bar.insertItem(withTitle: "File", action: nil, keyEquivalent: "", at: 1).submenu = m
            m.addItem(withTitle: "New", action: #selector(NSDocumentController.newDocument(_:)), keyEquivalent: "n")
            m.addItem(withTitle: "Open…", action: #selector(NSDocumentController.openDocument(_:)), keyEquivalent: "o")
            fileExtras(m)
        }
        if !sawEdit {
            let m = NSMenu(title: "Edit")
            bar.insertItem(withTitle: "Edit", action: nil, keyEquivalent: "", at: 2).submenu = m
            for (t, a, k) in [("Undo", #selector(UndoActions.undo(_:)), "z"), ("Redo", #selector(UndoActions.redo(_:)), "Z")] {
                m.addItem(withTitle: t, action: a, keyEquivalent: k)
            }
            m.addItem(.separator())
            m.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
            m.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
            m.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
            m.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        }
        if !sawView {
            let m = NSMenu(title: "View")
            bar.insertItem(withTitle: "View", action: nil, keyEquivalent: "", at: 3).submenu = m
            viewExtras(m)
        }
        let window = NSMenu(title: "Window")
        let at = help.flatMap { h in bar.items.firstIndex { $0.submenu === h } } ?? bar.items.count
        bar.insertItem(withTitle: "Window", action: nil, keyEquivalent: "", at: at).submenu = window
        window.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        window.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        window.addItem(.separator())
        window.addItem(withTitle: "Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)), keyEquivalent: "")
        NSApp.windowsMenu = window
        let h = help ?? submenu(bar, "Help")
        NSApp.helpMenu = h
        return bar
    }

    /// File's items NSDocument gives that the model has no command for: Open Recent, Close,
    /// Revert.
    private func fileExtras(_ m: NSMenu) {
        let recent = NSMenu(title: "Open Recent")
        // the document controller fills this menu itself: it finds it by its Clear Menu item
        recent.addItem(withTitle: "Clear Menu", action: #selector(NSDocumentController.clearRecentDocuments(_:)), keyEquivalent: "")
        let r = NSMenuItem(title: "Open Recent", action: nil, keyEquivalent: "")
        r.submenu = recent
        let openAt = m.items.firstIndex { $0.action == #selector(NSDocumentController.openDocument(_:)) }.map { $0 + 1 } ?? m.items.count
        m.insertItem(r, at: openAt)
        let close = NSMenuItem(title: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        m.insertItem(close, at: openAt + 1)
        let saveAt = m.items.firstIndex { $0.action == #selector(NSDocument.saveAs(_:)) }.map { $0 + 1 }
        if let saveAt {
            m.insertItem(NSMenuItem(title: "Revert to Saved", action: #selector(NSDocument.revertToSaved(_:)), keyEquivalent: ""), at: saveAt)
        }
    }

    /// The view's own zoom, which the core never hears (plan 5.3), and full screen.
    private func viewExtras(_ m: NSMenu) {
        if !m.items.isEmpty { m.addItem(.separator()) }
        m.addItem(withTitle: "Zoom In", action: #selector(CanvasView.zoomIn(_:)), keyEquivalent: "+")
        m.addItem(withTitle: "Zoom Out", action: #selector(CanvasView.zoomOut(_:)), keyEquivalent: "-")
        m.addItem(withTitle: "Actual Size", action: #selector(CanvasView.actualSize(_:)), keyEquivalent: "0")
        // Enter Full Screen is the system's: AppKit adds it to the View menu itself
    }

    private func fill(_ m: NSMenu, _ list: [MenuItem], path: String) {
        m.autoenablesItems = true
        for (k, it) in list.enumerated() {
            let key = "\(path)/\(k)"
            if it.separator {
                if !(m.items.last?.isSeparatorItem ?? true) { m.addItem(.separator()) }
                continue
            }
            // the application menu has Settings…; the bar's own place for it is dropped
            if it.role?.word == .preferences || it.role?.word == .quit || it.role?.word == .about { continue }
            if it.isSubmenu {
                let sub = NSMenu(title: it.label)
                let mi = NSMenuItem(title: it.label, action: nil, keyEquivalent: "")
                mi.representedObject = ModelRef(key)
                mi.submenu = sub
                m.addItem(mi)
                fill(sub, it.items, path: key)
                continue
            }
            m.addItem(item(it, key: key))
        }
        while m.items.last?.isSeparatorItem == true { m.removeItem(at: m.items.count - 1) }
    }

    /// One model item as an NSMenuItem: a role the Mac places is its standard action; anything
    /// else dispatches its command through `run(_:)`.
    private func item(_ it: MenuItem, key: String) -> NSMenuItem {
        let action: Selector = switch it.role?.word {
        case .new?: #selector(NSDocumentController.newDocument(_:))
        case .open?: #selector(NSDocumentController.openDocument(_:))
        case .save?: #selector(NSDocument.save(_:))
        case .saveAs?: #selector(NSDocument.saveAs(_:))
        case .undo?: #selector(UndoActions.undo(_:))
        case .redo?: #selector(UndoActions.redo(_:))
        case .cut?: #selector(NSText.cut(_:))
        case .copy?: #selector(NSText.copy(_:))
        case .paste?: #selector(NSText.paste(_:))
        case .delete?: #selector(NSText.delete(_:))
        case .selectAll?: #selector(NSText.selectAll(_:))
        case .fullScreen?: #selector(NSWindow.toggleFullScreen(_:))
        default: #selector(run(_:))
        }
        let label = it.label
        let mi = NSMenuItem(title: label, action: action, keyEquivalent: "")
        if action == #selector(run(_:)) { mi.target = self }
        mi.representedObject = ModelRef(key)
        if let sc = it.shortcut, let (k, mods) = Self.keyEquivalent(sc) {
            mi.keyEquivalent = k
            mi.keyEquivalentModifierMask = mods
        }
        apply(it, to: mi)
        return mi
    }

    /// The Mac's key equivalent for a model shortcut. Keys with no modifier (F for Fit,
    /// Backspace for Delete, Escape) are not bound in the menu: the canvas recognises them, so a
    /// text field keeps its typing (contract 12.3).
    static func keyEquivalent(_ s: Shortcut) -> (String, NSEvent.ModifierFlags)? {
        var mods: NSEvent.ModifierFlags = []
        for m in s.mods {
            switch m.word {
            case .primary?: mods.insert(.command)
            case .shift?: mods.insert(.shift)
            case .alt?: mods.insert(.option)
            case .ctrl?: mods.insert(.control)
            case nil: return nil
            }
        }
        if mods.isEmpty || mods == [.shift] { return nil }
        let k: String? = switch s.key {
        case "Enter": "\r"
        case "Period": "."
        case "Comma": ","
        case "Escape": "\u{1b}"
        case "Tab": "\t"
        case "Space": " "
        case "Backspace": "\u{8}"
        case "Delete": "\u{7f}"
        case "Plus": "+"
        case "Minus": "-"
        case "ArrowUp": String(UnicodeScalar(UInt16(NSUpArrowFunctionKey)).map(Character.init) ?? " ")
        case "ArrowDown": String(UnicodeScalar(UInt16(NSDownArrowFunctionKey)).map(Character.init) ?? " ")
        case "ArrowLeft": String(UnicodeScalar(UInt16(NSLeftArrowFunctionKey)).map(Character.init) ?? " ")
        case "ArrowRight": String(UnicodeScalar(UInt16(NSRightArrowFunctionKey)).map(Character.init) ?? " ")
        default: s.key.count == 1 ? s.key.lowercased() : nil
        }
        guard let k else { return nil }
        return (k, mods)
    }

    /// The model item's state on an NSMenuItem: title, check, tooltip.
    private func apply(_ it: MenuItem, to mi: NSMenuItem) {
        var title = it.label
        // Undo and Redo carry the core's step names, as every Mac app's Edit menu does
        if let st = source?.state {
            if it.role?.word == .undo { title = st.undo.map { "Undo \($0)" } ?? "Undo" }
            if it.role?.word == .redo { title = st.redo.map { "Redo \($0)" } ?? "Redo" }
        }
        if mi.title != title { mi.title = title }
        let state: NSControl.StateValue = it.checked == true ? .on : .off
        if mi.state != state { mi.state = state }
        let tip = it.enabled ? it.hover : (it.reason ?? it.hover)
        if mi.toolTip != tip { mi.toolTip = tip }
    }

    func modelItem(_ mi: NSMenuItem) -> MenuItem? {
        (mi.representedObject as? ModelRef).flatMap { items[$0.key] }
    }

    private func find(role: RoleWord) -> (String, MenuItem)? {
        items.first { $0.value.role?.word == role }.map { ($0.key, $0.value) }
    }

    // MARK: Validation and dispatch

    func validateMenuItem(_ mi: NSMenuItem) -> Bool {
        guard let it = modelItem(mi) else { return false }
        apply(it, to: mi)
        return it.enabled && source != nil
    }

    /// For items that go down the responder chain (Undo, Cut, …): the model's state for the
    /// item, which the responder that handles it asks for.
    func validateStandard(_ mi: NSMenuItem) -> Bool? {
        guard let it = modelItem(mi) else { return nil }
        apply(it, to: mi)
        return it.enabled
    }

    @objc func run(_ sender: NSMenuItem) {
        guard let it = modelItem(sender), let model = source else { return }
        dispatch(it, model: model)
    }

    /// Run a model item for the person at the Mac (the menu bar, a context menu, the toolbar).
    func dispatch(_ it: MenuItem, model: DocumentModel) {
        guard let command = it.command else { return }
        let window = GraphWindowController.of(model)
        // what has no UI here yet says so instead of running (the kit's list, shared with the iPad)
        if let say = ShellCommands.notBuilt(it) {
            window?.banner(say)
            return
        }
        if command == "app.quit" {
            NSApp.terminate(nil)
            return
        }
        if command == "canvas.palette", it.params == nil, let v = window?.canvasView {
            // Node › Search…: the palette at the pointer (the view's centre when it is elsewhere)
            v.openPalette()
            return
        }
        if command == "graph.new" {
            NSDocumentController.shared.newDocument(nil)
            return
        }
        if command == "graph.open" {
            NSDocumentController.shared.openDocument(nil)
            return
        }
        Task {
            do {
                _ = try await model.activate(it, person: true)
            } catch {
                window?.banner("\(it.label): \(error)")
            }
        }
    }

    // MARK: Dumping (scripts compare it with the egui editor's menus)

    func dump() {
        guard let onDump, let bar = NSApp.mainMenu else { return }
        var out = ""
        func walk(_ m: NSMenu, _ depth: Int) {
            // hidden items (AppKit's alternates for other keyboards) aren't shown, so aren't dumped
            for mi in m.items where !mi.isHidden {
                if mi.isSeparatorItem {
                    out += String(repeating: "  ", count: depth) + "----\n"
                    continue
                }
                // a model item as the model has it (what a key window's validation shows; a launch
                // in the background has no key window, so standard actions find no responder)
                let it = modelItem(mi)
                if let it { apply(it, to: mi) }
                let enabled = it.map { $0.enabled && source != nil } ?? mi.isEnabled
                var line = String(repeating: "  ", count: depth) + mi.title
                if !mi.keyEquivalent.isEmpty { line += " [" + Self.describe(mi.keyEquivalentModifierMask, mi.keyEquivalent) + "]" }
                if !enabled && mi.submenu == nil { line += " (disabled)" }
                if mi.state == .on { line += " (checked)" }
                if let t = mi.toolTip, !enabled { line += " — \(t)" }
                out += line + "\n"
                if let s = mi.submenu { walk(s, depth + 1) }
            }
        }
        walk(bar, 0)
        onDump(out)
    }

    static func describe(_ mods: NSEvent.ModifierFlags, _ key: String) -> String {
        var s = ""
        if mods.contains(.control) { s += "Ctrl+" }
        if mods.contains(.option) { s += "Opt+" }
        if mods.contains(.shift) || (key.count == 1 && key != key.lowercased()) { s += "Shift+" }
        if mods.contains(.command) { s += "Cmd+" }
        let k: String = switch key {
        case "\r": "Return"
        case "\u{1b}": "Escape"
        case "\u{8}": "Backspace"
        case "\u{7f}": "Delete"
        case " ": "Space"
        default: key.uppercased()
        }
        return s + k
    }

    private func submenu(_ bar: NSMenu, _ title: String) -> NSMenu {
        let m = NSMenu(title: title)
        bar.addItem(withTitle: title, action: nil, keyEquivalent: "").submenu = m
        return m
    }
}

/// Undo and Redo as actions down the responder chain: a text field editing takes them (its
/// window's undo manager); the canvas answers them with the core's `edit.undo` and `edit.redo`.
@MainActor @objc protocol UndoActions {
    func undo(_ sender: Any?)
    func redo(_ sender: Any?)
}
