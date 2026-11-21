// The settings as the core models them (N4 slice 4; contract 14.4): `settings.get` gives sections
// of items, each naming the command its value goes through, so a shell renders them natively and
// changes nothing itself (golden rule 4: "No exceptions to the rule"). The feed's `prefs` domain
// says when to read them again.

import Foundation
import Observation
import PlumCraft
import PlumCraftModels

/// One setting, as `settings.get` gives it.
public struct SettingsItem: Identifiable, Hashable, Sendable {
    public struct Option: Hashable, Sendable {
        public var value: JSONValue
        public var label: String
    }

    public var id: String
    public var label: String
    /// `choice`, `number`, `toggle`, `color`, `action`, `note`.
    public var kind: String
    public var value: JSONValue
    public var command: String?
    public var param: String?
    public var params: [String: JSONValue]
    public var options: [Option]
    public var min: Double?
    public var max: Double?
    public var step: Double?
    public var unit: String?
    public var defaultValue: JSONValue
    public var changed: Bool
    public var enabled: Bool
    public var invoke: Bool
    public var group: String?
    public var note: String?
    public var contrast: String?
    /// `fail`, `low`, `ok`.
    public var contrastLevel: String?
    public var swatch: String?
    public var reset: (command: String, params: JSONValue)? {
        guard let c = resetCommand else { return nil }
        return (c, resetParams)
    }
    var resetCommand: String?
    var resetParams: JSONValue = .null

    init(_ v: JSONValue) {
        id = v["id"].string ?? ""
        label = v["label"].string ?? ""
        kind = v["kind"].string ?? "note"
        value = v["value"]
        command = v["command"].string
        param = v["param"].string
        params = v["params"].object ?? [:]
        options = (v["options"].array ?? []).map { Option(value: $0["value"], label: $0["label"].string ?? $0["value"].text) }
        min = v["min"].double
        max = v["max"].double
        step = v["step"].double
        unit = v["unit"].string
        defaultValue = v["default"]
        changed = v["changed"].bool ?? false
        enabled = v["enabled"].bool ?? true
        invoke = v["invoke"].bool ?? false
        group = v["group"].string
        note = v["note"].string
        contrast = v["contrast"]["text"].string
        contrastLevel = v["contrast"]["level"].string
        swatch = v["swatch"].string
        resetCommand = v["reset"]["command"].string
        resetParams = v["reset"]["params"]
    }

    public static func == (a: SettingsItem, b: SettingsItem) -> Bool { a.id == b.id && a.value.same(b.value) && a.enabled == b.enabled && a.changed == b.changed && a.label == b.label }
    public func hash(into h: inout Hasher) { h.combine(id) }
}

/// A section of settings.
public struct SettingsSection: Identifiable, Hashable, Sendable {
    public var id: String
    public var label: String
    public var note: String?
    public var items: [SettingsItem]

    /// The items in runs of the same `group` (a heading within the section).
    public var groups: [(String?, [SettingsItem])] {
        var out: [(String?, [SettingsItem])] = []
        for it in items {
            if let last = out.last, last.0 == it.group {
                out[out.count - 1].1.append(it)
            } else {
                out.append((it.group, [it]))
            }
        }
        return out
    }

    public static func == (a: SettingsSection, b: SettingsSection) -> Bool { a.id == b.id && a.items == b.items && a.label == b.label }
    public func hash(into h: inout Hasher) { h.combine(id) }
}

/// The settings of the core, read through a document (every document shares them), set through
/// the commands the items name.
@MainActor
@Observable
public final class SettingsModel {
    public let model: DocumentModel
    public private(set) var sections: [SettingsSection] = []
    /// The theme chosen (`system` or a theme's name) and the one shown.
    public private(set) var chosen = "system"
    public private(set) var shown = "paper"
    public private(set) var error: String?
    /// The section shown (the Mac's tab, the iPad's page), so a script can show one.
    public var tab = "appearance"
    /// The settings were read again.
    @ObservationIgnored public var onChange: (() -> Void)?

    public init(model: DocumentModel) {
        self.model = model
    }

    /// Read them (`settings.get`).
    public func refresh() async {
        do {
            let v = try await model.core.request("settings.get", [:], doc: model.requestDoc)
            apply(v)
            error = nil
        } catch {
            self.error = "\(error)"
        }
        onChange?()
    }

    func apply(_ v: JSONValue) {
        chosen = v["chosen"].string ?? chosen
        shown = v["shown"].string ?? shown
        sections = (v["sections"].array ?? []).map { s in
            SettingsSection(id: s["id"].string ?? "", label: s["label"].string ?? "", note: s["note"].string, items: (s["items"].array ?? []).map(SettingsItem.init))
        }
    }

    public func section(_ id: String) -> SettingsSection? { sections.first { $0.id == id } }
    public func item(_ id: String) -> SettingsItem? { sections.lazy.flatMap(\.items).first { $0.id == id } }

    /// Set an item to `value` through its command (`command.run {command, params + {param: value}}`).
    public func set(_ item: SettingsItem, _ value: JSONValue) async {
        guard let c = item.command else { return }
        var p = item.params
        if let k = item.param { p[k] = value }
        await run(c, .object(p))
    }

    /// An item's own reset (a colour back to the theme's).
    public func reset(_ item: SettingsItem) async {
        guard let r = item.reset else { return }
        await run(r.command, r.params)
    }

    /// An action: as its menu item would run it (`ui.invoke` for the person, asking for a file),
    /// or the command with its params.
    public func act(_ item: SettingsItem) async {
        guard let c = item.command else { return }
        if item.invoke {
            do { _ = try await model.invoke(c, person: true) } catch { self.error = "\(error)" }
        } else {
            await run(c, .object(item.params))
        }
        await refresh()
    }

    private func run(_ c: String, _ p: JSONValue) async {
        do {
            _ = try await model.run(c, p)
            error = nil
        } catch {
            self.error = "\(error)"
        }
        await refresh()
    }
}
