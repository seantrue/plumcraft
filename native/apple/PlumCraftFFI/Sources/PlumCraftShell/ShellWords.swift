// What the Apple apps on the kit say and look up the same way (N3 slice 5, moved out of the Mac
// app): the model's item for a command, the commands whose UI isn't built yet, the platform's
// words for the core's answers, and the text of a watched value.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels

public extension Array where Element == MenuItem {
    /// The model's item running `command` without fixed params, wherever it is in these menus.
    func item(command: String) -> MenuItem? {
        for it in self {
            if it.command == command && it.params == nil { return it }
            if let f = it.items.item(command: command) { return f }
        }
        return nil
    }
}

public enum ShellCommands {
    /// Commands whose effect is UI these apps don't have yet: choosing one says so, without
    /// running it (it would open state nothing shows). Command id to what is missing.
    public static let unbuilt: [String: String] = [:]
    // N4 slice 3 built the parameters inspector (node.parameters) and pop-outs (view.popout);
    // slice 4 Settings (app.settings) and About (app.about); slice 5 the Messages window
    // (view.messages); slice 6 the rest: value editors (node.editor), the Cache Report
    // (run.cacheReport), the graph's Parameters window (view.params), Compare (viewer.compare,
    // viewer.pin) and Engine › Attach… and Install Engine… (the core's form asks). Nothing is left:
    // a command whose UI a later stage hasn't built goes back in here with what is missing.

    /// What to say when a gesture ran commands whose UI isn't built here: the first such, or nil.
    public static func notBuilt(ran: [CommandRun]) -> String? {
        for c in ran where c.ok {
            if let what = unbuilt[c.command] { return "That opens \(what), which isn't available yet in this app." }
        }
        return nil
    }

    /// What to say when `item`'s command isn't built here; nil when it is.
    public static func notBuilt(_ item: MenuItem) -> String? {
        guard let c = item.command, let what = unbuilt[c] else { return nil }
        return "\(item.label.replacingOccurrences(of: "…", with: "")) isn't available yet in this app (\(what))."
    }
}

public enum AskWords {
    /// A form's pick field (`kind: "pick"`, decision 5): its choices as `(value, label, hint)`
    /// and whether any other text may be typed (`other`).
    public static func pick(_ field: JSONValue) -> (choices: [(value: String, label: String, hint: String?)], other: Bool)? {
        guard field["kind"].string == "pick" else { return nil }
        let choices = (field["choices"].array ?? []).compactMap { c -> (String, String, String?)? in
            guard let v = c["value"].string else { return nil }
            return (v, c["label"].string ?? v, c["hint"].string)
        }
        return (choices.map { (value: $0.0, label: $0.1, hint: $0.2) }, field["other"].bool ?? false)
    }

    /// What a pick field answers for the text shown: a choice's value when the text is its label
    /// (or its value), else the text as typed.
    public static func pickAnswer(_ field: JSONValue, shown: String) -> String {
        guard let p = pick(field) else { return shown }
        return p.choices.first { $0.label == shown || $0.value == shown }?.value ?? shown
    }

    /// The label a pick field shows for a value it is given (a choice's label, else the value).
    public static func pickShown(_ field: JSONValue, value: String) -> String {
        pick(field)?.choices.first { $0.value == value }?.label ?? value
    }

    /// A prompt's button words: the core's own for the answer (`labels`, by answer: "Save as PNG
    /// anyway"), else the Apple platforms' words for it ([`buttonTitle(_:kind:)`]).
    public static func buttonTitle(_ answer: String, prompt params: JSONValue) -> String {
        if let w = params["labels"][answer].string, !w.trimmingCharacters(in: .whitespaces).isEmpty { return w }
        return buttonTitle(answer, kind: params["kind"].string)
    }

    /// The Apple platforms' words for the core's answers to a `prompt` (Save, Don't Save, Cancel).
    public static func buttonTitle(_ answer: String, kind: String?) -> String {
        switch answer {
        case "save": "Save"
        case "discard": kind == "unsavedChanges" ? "Don't Save" : "Discard"
        case "cancel": "Cancel"
        case "trust": "Trust"
        default: answer.prefix(1).uppercased() + answer.dropFirst()
        }
    }

    /// The extensions a file ask's `kinds` allow (empty: any).
    public static func extensions(_ params: JSONValue) -> [String] {
        let exts = (params["kinds"].array ?? []).flatMap { $0["extensions"].array ?? [] }.compactMap(\.string)
        return exts.contains("*") ? [] : exts
    }

    /// A file ask's kinds, each a label and its extensions (Save Result…'s formats: "Quoted Text",
    /// ["quoted"]), without the "any file" kind.
    public static func kinds(_ params: JSONValue) -> [(label: String, extensions: [String])] {
        (params["kinds"].array ?? []).compactMap { k in
            let exts = (k["extensions"].array ?? []).compactMap(\.string).filter { $0 != "*" }
            guard !exts.isEmpty else { return nil }
            return (k["label"].string ?? exts[0].uppercased(), exts)
        }
    }

    /// Which kind a file name picks: the one its extension is among, else the first.
    public static func kindIndex(_ kinds: [(label: String, extensions: [String])], name: String) -> Int {
        let ext = (name as NSString).pathExtension.lowercased()
        return kinds.firstIndex { $0.extensions.map { $0.lowercased() }.contains(ext) } ?? 0
    }

    /// `name` with its extension made the kind's first (a name without one gets it).
    public static func renamed(_ name: String, to kind: (label: String, extensions: [String])) -> String {
        guard let ext = kind.extensions.first else { return name }
        let stem = (name as NSString).deletingPathExtension
        return (stem.isEmpty ? name : stem) + "." + ext
    }
}

/// A watched value as text, the same on every Apple viewer.
public enum ValueText {
    /// A scalar, formatted: grouping, a whole number without decimals, up to six otherwise.
    public static func scalar(_ w: Watched) -> String {
        let v = w.value ?? .null
        switch v {
        case .bool(let b): return b ? "true" : "false"
        case .string(let s): return s
        case .null: return w.summary ?? "—"
        default:
            guard let d = v.double else { return v.text }
            let f = NumberFormatter()
            f.numberStyle = .decimal
            f.maximumFractionDigits = if case .double = v { 6 } else { 0 }
            f.usesGroupingSeparator = true
            return f.string(from: NSNumber(value: d)) ?? v.text
        }
    }

    /// What the panel's header says under the title: the summary, else what isn't there yet.
    public static func summary(_ w: Watched) -> String {
        w.summary ?? (w.media.map { "\($0), not computed yet" } ?? "not computed yet")
    }

    /// The excerpt's text, its length and the value's whole length (characters).
    public static func excerpt(_ w: Watched) -> (text: String, shown: Int, total: Int) {
        let ex = w.excerpt
        let text = ex?.text ?? ""
        let total = Int(ex?.totalChars ?? UInt64(text.count))
        return (text, text.count, max(total, text.count))
    }

    /// The handle of the full value's bytes, when the core named one.
    public static func dataHandle(_ w: Watched) -> String? {
        w.data.flatMap { $0["handle"].string ?? $0.string }
    }

    /// The value's own bytes as UTF-8 (`media.get {want: "bytes"}`, then `plum_blob`), read and
    /// decoded off the main thread. Nil when the core can't give them within 30 s.
    @MainActor
    public static func whole(_ model: DocumentModel, handle: String) async -> String? {
        let core = model.core
        guard let note = try? await core.request(MediaNote.self, "media.get", ["handle": .string(handle), "want": "bytes"], doc: model.requestDoc) else { return nil }
        var latest = note
        if latest.status.word == .pending {
            let key = note.key
            _ = await model.wait(timeout: .seconds(30)) { model.media[key].map { $0.status.word != .pending } ?? false }
            guard let m = model.media[key] else { return nil }
            latest = m
        }
        guard latest.status.word == .ready else { return nil }
        let key = latest.key
        return await Task.detached {
            guard case .ready(let b)? = try? core.blob(key) else { return nil }
            return String(decoding: b.data, as: UTF8.self)
        }.value
    }
}
