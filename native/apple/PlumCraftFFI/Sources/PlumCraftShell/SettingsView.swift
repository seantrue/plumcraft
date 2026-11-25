// Settings, natively, from the core's model (N4 slice 4): the same sections and items on both apps,
// each control setting its value through the command its item names (SettingsModel). On the Mac a
// Settings window with a tab per section (the app menu's Settings…, Command-comma); on the iPad an
// in-app sheet listing the sections, each a page. A grouped Form either way, as the system's own
// settings look. The theme, Text size, colours and families, theme files, the run's look, engine
// picking and the Cache Report are the egui editor's Settings window's, one for one.

import Foundation
import PlumCraftKit
import PlumCraftModels
import SwiftUI
#if canImport(AppKit)
import AppKit
#else
import UIKit
#endif

/// The SF Symbol for a section.
func sectionSymbol(_ id: String) -> String {
    switch id {
    case "appearance": "paintpalette"
    case "colours": "swatchpalette"
    case "families": "square.stack.3d.up"
    case "files": "doc.badge.gearshape"
    case "running": "play.circle"
    case "engines": "cpu"
    case "cache": "internaldrive"
    default: "gearshape"
    }
}

/// `#rrggbb` as a colour, and back.
enum Hex {
    static func color(_ s: String?) -> Color {
        guard let s, s.count == 7, s.hasPrefix("#"), let n = UInt32(s.dropFirst(), radix: 16) else { return .clear }
        return Color(.sRGB, red: Double((n >> 16) & 0xff) / 255, green: Double((n >> 8) & 0xff) / 255, blue: Double(n & 0xff) / 255, opacity: 1)
    }

    static func string(_ c: Color) -> String? {
        var r: CGFloat = 0, g: CGFloat = 0, b: CGFloat = 0
        #if canImport(AppKit)
        guard let ns = NSColor(c).usingColorSpace(.sRGB) else { return nil }
        r = ns.redComponent; g = ns.greenComponent; b = ns.blueComponent
        #else
        var a: CGFloat = 0
        guard UIColor(c).getRed(&r, green: &g, blue: &b, alpha: &a) else { return nil }
        #endif
        let byte = { (x: CGFloat) in Int((Swift.max(0, Swift.min(1, x)) * 255).rounded()) }
        return String(format: "#%02x%02x%02x", byte(r), byte(g), byte(b))
    }
}

/// The settings window's or sheet's content.
public struct SettingsView: SwiftUI.View {
    @Bindable var settings: SettingsModel
    var done: (() -> Void)?

    public init(settings: SettingsModel, done: (() -> Void)? = nil) {
        self.settings = settings
        self.done = done
    }

    public var body: some SwiftUI.View {
        #if os(macOS)
        TabView(selection: $settings.tab) {
            ForEach(settings.sections) { s in
                SectionForm(section: s, settings: settings)
                    .tabItem { Label(s.label, systemImage: sectionSymbol(s.id)) }
                    .tag(s.id)
            }
        }
        .frame(minWidth: 520, idealWidth: 560, minHeight: 420, idealHeight: 560)
        .overlay(alignment: .bottom) { errorLine }
        #else
        NavigationStack {
            List {
                ForEach(settings.sections) { s in
                    NavigationLink {
                        // Done on every page of the sheet, not only the first (N4 slice 6)
                        SectionForm(section: s, settings: settings).navigationTitle(s.label)
                            .toolbar {
                                ToolbarItem(placement: .confirmationAction) {
                                    Button("Done") { done?() }.accessibilityIdentifier("settings.done")
                                }
                            }
                    } label: {
                        Label(s.label, systemImage: sectionSymbol(s.id))
                    }
                    .accessibilityIdentifier("settings.section.\(s.id)")
                }
            }
            .navigationTitle("Settings")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { done?() }.accessibilityIdentifier("settings.done")
                }
            }
            .overlay(alignment: .bottom) { errorLine }
        }
        #endif
    }

    @ViewBuilder var errorLine: some SwiftUI.View {
        if let e = settings.error {
            Text(e).font(.footnote).foregroundStyle(.red).padding(8)
        }
    }
}

/// One section as a grouped form, its items under their headings.
struct SectionForm: SwiftUI.View {
    let section: SettingsSection
    @Bindable var settings: SettingsModel

    var body: some SwiftUI.View {
        Form {
            if let note = section.note {
                Section { Text(note).font(.callout).foregroundStyle(.secondary) }
            }
            ForEach(Array(section.groups.enumerated()), id: \.offset) { _, g in
                Section(g.0 ?? "") {
                    ForEach(g.1) { it in ItemRow(item: it, settings: settings) }
                }
            }
        }
        .formStyle(.grouped)
    }
}

/// One setting's native control.
struct ItemRow: SwiftUI.View {
    let item: SettingsItem
    @Bindable var settings: SettingsModel
    /// A slider being dragged: the value shown follows it; it is set when the drag ends.
    @State private var dragged: Double?

    var body: some SwiftUI.View {
        control
            .disabled(!item.enabled)
            .accessibilityIdentifier("setting.\(item.id)")
    }

    @ViewBuilder var control: some SwiftUI.View {
        switch item.kind {
        case "choice":
            Picker(item.label, selection: Binding<Int>(
                get: { item.options.firstIndex { $0.value.same(item.value) } ?? 0 },
                set: { i in
                    guard item.options.indices.contains(i) else { return }
                    let v = item.options[i].value
                    Task { await settings.set(item, v) }
                }
            )) {
                ForEach(Array(item.options.enumerated()), id: \.offset) { i, o in Text(o.label).tag(i) }
            }
            if let n = item.note { Text(n).font(.footnote).foregroundStyle(.secondary) }
        case "number":
            let lo = item.min ?? 0, hi = Swift.max(item.max ?? 1, lo + 0.001)
            let now = dragged ?? item.value.double ?? lo
            LabeledContent(item.label) {
                HStack {
                    Slider(value: Binding(get: { dragged ?? item.value.double ?? lo }, set: { dragged = $0 }), in: lo...hi, step: item.step ?? 0.5) { editing in
                        if !editing, let v = dragged {
                            Task {
                                await settings.set(item, .double(v))
                                dragged = nil
                            }
                        }
                    }
                    .frame(minWidth: 140)
                    Text(String(format: "%.1f", now) + (item.unit.map { " \($0)" } ?? ""))
                        .monospacedDigit()
                        .frame(minWidth: 56, alignment: .trailing)
                }
            }
            if let d = item.defaultValue.double, let v = item.value.double, abs(d - v) > 1e-9 {
                Button("Default (\(String(format: "%.0f", d)) \(item.unit ?? ""))") { Task { await settings.set(item, .null) } }
            }
        case "toggle":
            Toggle(item.label, isOn: Binding(get: { item.value.bool ?? false }, set: { on in Task { await settings.set(item, .bool(on)) } }))
            if let n = item.note { Text(n).font(.footnote).foregroundStyle(.secondary) }
        case "color":
            HStack {
                ColorPicker(item.label, selection: Binding(
                    get: { Hex.color(item.value.string) },
                    set: { c in
                        guard let h = Hex.string(c), h != item.value.string else { return }
                        Task { await settings.set(item, .string(h)) }
                    }
                ), supportsOpacity: false)
                if let c = item.contrast {
                    Text(c).font(.caption).monospacedDigit()
                        .foregroundStyle(item.contrastLevel == "fail" ? Color.red : item.contrastLevel == "low" ? Color.orange : Color.secondary)
                }
                if item.changed {
                    Button("Reset") { Task { await settings.reset(item) } }.buttonStyle(.borderless)
                }
            }
        case "action":
            Button(item.label) { Task { await settings.act(item) } }
        default:
            LabeledContent(item.label, value: item.value.string ?? item.value.text)
        }
    }
}
