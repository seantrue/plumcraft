// The real menu bar (N3 slice 4), built from the core's menu model (`menu.get`) of the front
// document and kept current by the feed: an item's title, enabled state, check mark and tooltip
// (the reason it is disabled) are read from the model each time AppKit validates it, and the bar
// is rebuilt only when the model's structure changes. Items dispatch their command by id
// (`ui.invoke {command, by: "person"}`, or `command.run` with fixed params), never a shell-side
// interpretation.
//
// What the Mac places itself (the model's roles): About, Settings…, Services, Hide, Quit in the
// application menu; New, Open…, Close, Save, Save As… as NSDocument's; Open Recent is the
// core's (B166: the model's File › Open Recent, the list the core keeps); Undo, Redo,
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
    let appName = "Plummer"
    /// The model's items by their place in the bar ("2/5"), as last fetched.
    private var items: [String: MenuItem] = [:]
    private var shape = ""
    /// The document whose menus the bar shows.
    private(set) weak var source: DocumentModel?
    /// Called with the text of the built bar each time it is built or changes (`--dump-menus`).
    var onDump: ((String) -> Void)?
    /// File › Open Recent's items as the last document's model had them (B166), for the bar with
    /// no document, which has no model to ask.
    private(set) var lastRecent: [MenuItem] = []

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
        if let recent = flat.values.first(where: { $0.id == Self.openRecentID }) { lastRecent = recent.items }
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
        // the standard panel with the core's versions (app.about, N4 slice 4), from the model's
        // About item (its label and enabled state, B169), with or without a document
        let about: NSMenuItem
        if let (key, it) = find(role: .about) {
            about = item(it, key: key)
            app.addItem(about)
        } else {
            about = app.addItem(withTitle: "About \(appName)", action: nil, keyEquivalent: "")
        }
        about.action = #selector(about(_:))
        about.target = self
        // Acknowledgements beside it (FR4): the model's item, run like any
        if let (key, it) = find(role: .acknowledgements) {
            app.addItem(item(it, key: key))
        }
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

    /// The model's id of File › Open Recent (the core's `menu::OPEN_RECENT`).
    static let openRecentID = "menu:File/Open Recent"

    /// File's items NSDocument gives that the model has no command for: Close, Revert; and with
    /// no document, Open Recent from the last model's list.
    ///
    /// B166: Open Recent was AppKit's, a submenu holding only Clear Menu for the document
    /// controller to fill. But the bar is built anew whenever the front document or the model's
    /// shape changes, and AppKit fills only the recent-documents menu it adopted, never the new
    /// one: the menu stayed empty while `recentDocumentURLs` had the graphs. Now the list and its
    /// items are the core's (`graph.openRecent`, `graph.clearRecent`), in the model like any menu.
    private func fileExtras(_ m: NSMenu) {
        // after Open… (the core's, a model item with the open role; NSDocumentController's when
        // no document is open)
        var openAt = m.items.firstIndex { $0.action == #selector(NSDocumentController.openDocument(_:)) || modelItem($0)?.role?.word == .open }
            .map { $0 + 1 } ?? m.items.count
        if !m.items.contains(where: { modelItem($0)?.id == Self.openRecentID }) {
            m.insertItem(recentWithoutDocument(), at: openAt)
            openAt += 1
        } else {
            openAt = (m.items.firstIndex { modelItem($0)?.id == Self.openRecentID } ?? openAt - 1) + 1
        }
        let close = NSMenuItem(title: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        m.insertItem(close, at: openAt)
        let saveAt = m.items.firstIndex { $0.action == #selector(NSDocument.saveAs(_:)) }.map { $0 + 1 }
        if let saveAt {
            m.insertItem(NSMenuItem(title: "Revert to Saved", action: #selector(NSDocument.revertToSaved(_:)), keyEquivalent: ""), at: saveAt)
        }
    }

    /// File › Open Recent with no document (every window closed): the last model's items, each
    /// opening its graph as Open… does (a document of its own); Clear Menu needs a document,
    /// whose model runs the core's command, so it waits for one.
    private func recentWithoutDocument() -> NSMenuItem {
        let sub = NSMenu(title: "Open Recent")
        sub.autoenablesItems = true
        for it in lastRecent {
            if it.separator {
                sub.addItem(.separator())
                continue
            }
            guard it.command == "graph.openRecent", let path = it.params?["path"].string else { continue }
            let mi = NSMenuItem(title: it.label, action: #selector(openRecentFile(_:)), keyEquivalent: "")
            mi.target = self
            mi.representedObject = path
            mi.toolTip = it.enabled ? it.hover : it.reason
            sub.addItem(mi)
        }
        let clear = sub.addItem(withTitle: "Clear Menu", action: nil, keyEquivalent: "")
        clear.toolTip = "open a graph to clear the list"
        let r = NSMenuItem(title: "Open Recent", action: nil, keyEquivalent: "")
        r.submenu = sub
        return r
    }

    /// A recent graph picked with no document open: opened as Open… opens a file (the core opens
    /// it in a document of its own, which notes it as the newest).
    @objc func openRecentFile(_ sender: NSMenuItem) {
        guard let path = sender.representedObject as? String else { return }
        NSDocumentController.shared.openDocument(withContentsOf: URL(fileURLWithPath: path), display: true) { _, _, error in
            if let error { CoreHost.present(error) }
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
            if it.role?.word == .preferences || it.role?.word == .quit || it.role?.word == .about || it.role?.word == .acknowledgements { continue }
            if it.isSection {
                // B80: inline, under its heading (it was an untitled submenu holding every engine)
                if !(m.items.last?.isSeparatorItem ?? true) { m.addItem(.separator()) }
                if !it.label.isEmpty { m.addItem(NSMenuItem.sectionHeader(title: it.label)) }
                fillSection(m, it.items, path: key)
                continue
            }
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

    /// A section's items, appended where the section is (their model keys under the section's).
    private func fillSection(_ m: NSMenu, _ list: [MenuItem], path: String) {
        let sub = NSMenu()
        fill(sub, list, path: path)
        for mi in sub.items {
            sub.removeItem(mi)
            m.addItem(mi)
        }
    }

    /// One model item as an NSMenuItem: a role the Mac places is its standard action; anything
    /// else dispatches its command through `run(_:)`.
    private func item(_ it: MenuItem, key: String) -> NSMenuItem {
        let action: Selector = switch it.role?.word {
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
        // the application menu's About names the application, as every Mac app's does
        if it.role?.word == .about { title = "\(it.label) \(appName)" }
        if mi.title != title { mi.title = title }
        let state: NSControl.StateValue = it.checked == true ? .on : .off
        if mi.state != state { mi.state = state }
        let tip = it.enabled ? (it.warning ?? it.hover) : (it.reason ?? it.hover)
        if mi.toolTip != tip { mi.toolTip = tip }
        Self.markWarning(it, mi)
    }

    /// An item whose engine's manifest warned (shell-interface 12.1, decision 17): the
    /// `exclamationmark.triangle` symbol as its image, the warning as its tooltip.
    static func markWarning(_ it: MenuItem, _ mi: NSMenuItem) {
        if let w = it.warning {
            if mi.image == nil || mi.image?.accessibilityDescription != w {
                mi.image = NSImage(systemSymbolName: "exclamationmark.triangle", accessibilityDescription: w)
            }
        } else if mi.image != nil {
            mi.image = nil
        }
    }

    func modelItem(_ mi: NSMenuItem) -> MenuItem? {
        (mi.representedObject as? ModelRef).flatMap { items[$0.key] }
    }

    private func find(role: RoleWord) -> (String, MenuItem)? {
        items.first { $0.value.role?.word == role }.map { ($0.key, $0.value) }
    }

    // MARK: Validation and dispatch

    func validateMenuItem(_ mi: NSMenuItem) -> Bool {
        if mi.action == #selector(openRecentFile(_:)) {
            return (mi.representedObject as? String).map { FileManager.default.fileExists(atPath: $0) } ?? false
        }
        // About needs no document (B169): the model's item says whether it is enabled, and
        // without one the panel shows what the core last said, or the bundle's own version
        if mi.action == #selector(about(_:)) {
            guard let it = modelItem(mi) else { return true }
            apply(it, to: mi)
            return it.enabled
        }
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

    @objc func about(_ sender: Any?) {
        AboutPanel.show(model: source, over: source.flatMap { GraphWindowController.of($0)?.window })
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
        // Settings… (⌘,) runs the core's app.settings like any item: the window follows the
        // core's view.windows.settings (N4 slice 4)
        if command == "app.about" {
            AboutPanel.show(model: model, over: window?.window)
            return
        }
        if command == "canvas.palette", it.params == nil, let v = window?.canvasView {
            // Node › Search…: the palette at the pointer (the view's centre when it is elsewhere)
            v.openPalette()
            return
        }
        // New and Open are the core's (decision 20, platform.document_windows): it makes the
        // document and the feed's window domain brings its window (DocumentWindows)
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
                let enabled = mi.target === self ? validateMenuItem(mi) : it.map { $0.enabled && source != nil } ?? mi.isEnabled
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
