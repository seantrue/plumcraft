// The iPad's menu bar and its keyboard shortcuts (N3 slice 5), from the core's menu model
// (`menu.get`) of the front graph, as the Mac's is: the bar is rebuilt (`UIMenuSystem` rebuild)
// only when the model's structure changes, and each command's title, state and enabled are read
// from the latest model when UIKit validates it (`GraphViewController.validate`). A disabled item
// shows the core's reason as its subtitle. Items dispatch their command by id
// (`ui.invoke {command, by: "person"}`, or `command.run` with fixed params).
//
// What iPadOS places itself: Cut, Copy, Paste, Delete and Select All use the standard edit actions
// down the responder chain, so a text field that is editing takes them and the canvas sends the
// core's commands otherwise. A model shortcut is a key command only when it has a modifier besides
// Shift (a plain key would take typing from a text field); F, Delete and Escape are the canvas's
// own key commands (`GraphViewController.keyCommands`). The menus show in iPadOS's menu bar and in
// the shortcut overlay (hold Command).
//
// The same items make context menus (`elements(_:model:)`), as UIActions.

import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import UIKit

/// The model whose menus the bar shows (the front graph's), and the bar's items by place.
@MainActor
final class MenuState {
    static let shared = MenuState()
    private(set) weak var source: DocumentModel?
    private(set) var items: [String: MenuItem] = [:]
    private var shape = ""
    /// The items the menus drew with the warning mark (decision 17), for the status: the menu
    /// bar's as the system built it, and the More menu's as it was shown.
    private(set) var marked: Set<String> = []

    func noteMarked(_ label: String) {
        guard marked.insert(label).inserted else { return }
        StatusFile.shared.set("menuMarked", .array(marked.sorted().map(JSONValue.string)))
    }

    /// Show `model`'s menus (nil: the bar with no graph).
    func show(_ model: DocumentModel?) {
        source = model
        refresh(force: true)
    }

    /// The model changed: rebuild when its structure did, else revalidate.
    func modelChanged(_ model: DocumentModel) {
        guard model === source else { return }
        refresh(force: false)
    }

    private func refresh(force: Bool) {
        var flat: [String: MenuItem] = [:]
        var sig = ""
        func walk(_ list: [MenuItem], _ path: String) {
            for (k, it) in list.enumerated() {
                let key = path.isEmpty ? "\(k)" : "\(path)/\(k)"
                flat[key] = it
                sig += "\(key)|\(it.id)|\(it.label)|\(it.command ?? "")|\(it.separator)|\(it.isSubmenu)|\(it.shortcut.map { "\($0.mods)\($0.key)" } ?? "")|\(it.warning ?? "")\n"
                walk(it.items, key)
            }
        }
        walk(source?.menuBar ?? [], "")
        items = flat
        if force || sig != shape {
            shape = sig
            UIMenuSystem.main.setNeedsRebuild()
        } else {
            UIMenuSystem.main.setNeedsRevalidate()
        }
    }

    /// The model's item at `key` (a command's property list).
    func item(_ key: String) -> MenuItem? { items[key] }

    /// The title an item shows: Undo and Redo carry the core's step names.
    func title(_ it: MenuItem) -> String {
        guard let st = source?.state else { return it.label }
        switch it.role?.word {
        case .undo?: return st.undo.map { "Undo \($0)" } ?? "Undo"
        case .redo?: return st.redo.map { "Redo \($0)" } ?? "Redo"
        default: return it.label
        }
    }
}

@MainActor
enum PadMenus {
    /// Selectors the bar's model items use; the graph's view controller answers them.
    static let run = #selector(GraphViewController.runMenuItem(_:))

    static func build(_ builder: any UIMenuBuilder) {
        builder.remove(menu: .format)
        let state = MenuState.shared
        var used: Set<String> = []
        let menus = state.source?.menuBar ?? []
        var after: UIMenu.Identifier = .view
        for (k, top) in menus.enumerated() where top.isSubmenu {
            let children = elements(top.items, path: "\(k)", used: &used)
            switch top.role?.word {
            case .fileMenu?:
                builder.replaceChildren(ofMenu: .file) { _ in children }
            case .editMenu?:
                builder.replaceChildren(ofMenu: .edit) { _ in children }
            case .viewMenu?:
                builder.replaceChildren(ofMenu: .view) { _ in children + [viewExtras(&used)] }
            case .helpMenu?:
                // iPadOS's Help keeps its search, with the model's About first (B151: which build
                // this is, where a person looks for it); its key is its place in the model
                if let j = top.items.firstIndex(where: { $0.role?.word == .about }) {
                    let it = top.items[j]
                    let about = UICommand(title: "\(it.label) \(AppID.name)", action: run, propertyList: "\(k)/\(j)")
                    // Acknowledgements beside it (FR4)
                    var beside: [UIMenuElement] = [about]
                    if let a = top.items.firstIndex(where: { $0.role?.word == .acknowledgements }) {
                        beside.append(UICommand(title: top.items[a].label, action: run, propertyList: "\(k)/\(a)"))
                    }
                    builder.insertChild(UIMenu(options: .displayInline, children: beside), atStartOfMenu: .help)
                }
            default:
                let id = UIMenu.Identifier(AppID.bundle + ".menu.\(top.id.isEmpty ? "\(k)" : top.id)")
                let m = UIMenu(title: top.label, identifier: id, children: children)
                builder.insertSibling(m, afterMenu: after)
                after = id
            }
        }
        if menus.isEmpty {
            builder.replaceChildren(ofMenu: .view) { old in old + [viewExtras(&used)] }
        }
    }

    /// The view's own zoom, which the core never hears (plan 5.3).
    static func viewExtras(_ used: inout Set<String>) -> UIMenu {
        var out: [UIMenuElement] = []
        for (t, sel, input, mods) in [("Zoom In", #selector(GraphViewController.zoomIn(_:)), "+", UIKeyModifierFlags.command),
                                      ("Zoom Out", #selector(GraphViewController.zoomOut(_:)), "-", .command),
                                      ("Actual Size", #selector(GraphViewController.actualSize(_:)), "0", .command)] {
            let sig = "\(input)|\(mods.rawValue)"
            if used.insert(sig).inserted {
                out.append(UIKeyCommand(title: t, action: sel, input: input, modifierFlags: mods))
            } else {
                out.append(UICommand(title: t, action: sel))
            }
        }
        return UIMenu(title: "", options: .displayInline, children: out)
    }

    /// Model items as the bar's commands (each names its place in the model as its property list).
    static func elements(_ list: [MenuItem], path: String, used: inout Set<String>) -> [UIMenuElement] {
        var out: [UIMenuElement] = []
        var group: [UIMenuElement] = []
        func flush() {
            if !group.isEmpty { out.append(UIMenu(title: "", options: .displayInline, children: group)) }
            group = []
        }
        for (k, it) in list.enumerated() {
            let key = "\(path)/\(k)"
            if it.separator {
                flush()
                continue
            }
            // the system has no Settings, About or Quit place here yet
            if it.role?.word == .preferences || it.role?.word == .quit || it.role?.word == .about || it.role?.word == .acknowledgements { continue }
            if it.isSection {
                // B80: inline, under its heading (it was an untitled submenu holding every engine)
                flush()
                group.append(UIMenu(title: it.label, options: .displayInline, children: elements(it.items, path: key, used: &used)))
                flush()
                continue
            }
            // an engine whose manifest warned (decision 17): the symbol, the warning as subtitle
            let mark = it.warning.map { _ in UIImage(systemName: "exclamationmark.triangle") } ?? nil
            if mark != nil { MenuState.shared.noteMarked(it.label) }
            if it.isSubmenu {
                let sub = UIMenu(title: it.label, image: mark, children: elements(it.items, path: key, used: &used))
                if let w = it.warning { sub.subtitle = w }
                group.append(sub)
                continue
            }
            let action: Selector = switch it.role?.word {
            case .cut?: #selector(UIResponderStandardEditActions.cut(_:))
            case .copy?: #selector(UIResponderStandardEditActions.copy(_:))
            case .paste?: #selector(UIResponderStandardEditActions.paste(_:))
            case .delete?: #selector(UIResponderStandardEditActions.delete(_:))
            case .selectAll?: #selector(UIResponderStandardEditActions.selectAll(_:))
            default: run
            }
            // the standard actions carry no property list (the system's own edit menu has them)
            let plist: Any? = action == run ? key : nil
            if let sc = it.shortcut, let (input, mods) = keyEquivalent(sc), used.insert("\(input)|\(mods.rawValue)").inserted {
                let c = UIKeyCommand(title: it.label, image: mark, action: action, input: input, modifierFlags: mods, propertyList: plist)
                if let w = it.warning { c.discoverabilityTitle = w }
                group.append(c)
            } else {
                let c = UICommand(title: it.label, image: mark, action: action, propertyList: plist)
                if let w = it.warning { c.discoverabilityTitle = w }
                group.append(c)
            }
        }
        flush()
        return out
    }

    /// A model shortcut as a key command's input and modifiers; nil for a plain key (the canvas
    /// recognises those itself, so a text field keeps its typing, contract 12.3).
    static func keyEquivalent(_ s: Shortcut) -> (String, UIKeyModifierFlags)? {
        var mods: UIKeyModifierFlags = []
        for m in s.mods {
            switch m.word {
            case .primary?: mods.insert(.command)
            case .shift?: mods.insert(.shift)
            case .alt?: mods.insert(.alternate)
            case .ctrl?: mods.insert(.control)
            case nil: return nil
            }
        }
        if mods.isEmpty || mods == [.shift] { return nil }
        let k: String? = switch s.key {
        case "Enter": "\r"
        case "Period": "."
        case "Comma": ","
        case "Escape": UIKeyCommand.inputEscape
        case "Tab": "\t"
        case "Space": " "
        case "Backspace": "\u{8}"
        case "Delete": UIKeyCommand.inputDelete
        case "Plus": "+"
        case "Minus": "-"
        case "ArrowUp": UIKeyCommand.inputUpArrow
        case "ArrowDown": UIKeyCommand.inputDownArrow
        case "ArrowLeft": UIKeyCommand.inputLeftArrow
        case "ArrowRight": UIKeyCommand.inputRightArrow
        default: s.key.count == 1 ? s.key.lowercased() : nil
        }
        guard let k else { return nil }
        return (k, mods)
    }

    // MARK: Context menus: the core's items as actions

    static func actions(_ list: [MenuItem], model: DocumentModel, dispatch: @escaping (MenuItem) -> Void) -> [UIMenuElement] {
        var out: [UIMenuElement] = []
        var group: [UIMenuElement] = []
        func flush() {
            if !group.isEmpty { out.append(UIMenu(title: "", options: .displayInline, children: group)) }
            group = []
        }
        for it in list {
            if it.separator {
                flush()
                continue
            }
            if it.isSection {
                flush()
                out.append(UIMenu(title: it.label, options: .displayInline, children: actions(it.items, model: model, dispatch: dispatch)))
                continue
            }
            if it.isSubmenu {
                group.append(UIMenu(title: it.label, children: actions(it.items, model: model, dispatch: dispatch)))
                continue
            }
            let a = UIAction(title: it.label, attributes: it.enabled ? [] : .disabled, state: it.checked == true ? .on : .off) { _ in dispatch(it) }
            if !it.enabled, let why = it.reason { a.subtitle = why }
            // an engine whose manifest warned (shell-interface 12.1, decision 17): the symbol as
            // the action's image, the warning as its subtitle (after the reason when the item is
            // disabled, as a process engine is on the iPad)
            if let w = it.warning {
                a.image = UIImage(systemName: "exclamationmark.triangle")
                MenuState.shared.noteMarked(it.label)
                a.subtitle = [it.enabled ? nil : it.reason, w].compactMap { $0 }.joined(separator: " · ")
            }
            group.append(a)
        }
        flush()
        return out
    }
}
