// The windows drawn from a model the core makes (N4 slice 6; contract 12.5): the Cache Report
// (`run.cacheReport`), the graph's Parameters window (`view.params`), a value editor
// (`node.editor`), read with `report.get`, and Compare (`viewer.compare`, `compare.get`). The
// core keeps which are open (the scene's `view.windows`); each app opens its own window (Mac) or
// sheet (iPad, iPhone) while it says so, and every button and field here is a core command, so a
// script reads and does the same.

import Foundation
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import SwiftUI

/// One report window's model: `report.get {window, node?}`, read again when the core's
/// `view.windows.reports` stamp changes.
@MainActor
@Observable
public final class ReportModel {
    @ObservationIgnored public let model: DocumentModel
    /// `cache`, `params` or `editor:<node>`.
    public let key: String
    public var reply: JSONValue?
    public var error: String?
    /// What each field holds while a person types (by the edit's command and row).
    public var drafts: [String: String] = [:]
    /// Why a field's text was refused, by the same key.
    public var refused: [String: String] = [:]
    @ObservationIgnored private var stamp: UInt64?
    @ObservationIgnored private var busy = false
    @ObservationIgnored private var again = false

    public init(model: DocumentModel, key: String) {
        self.model = model
        self.key = key
    }

    public var title: String { reply?["title"].string ?? (key == "cache" ? "Cache Report" : key == "params" ? "Parameters" : "Value Editor") }

    var params: JSONValue {
        if key.hasPrefix("editor:") { return ["window": "editor", "node": .string(String(key.dropFirst("editor:".count)))] }
        return ["window": .string(key)]
    }

    /// The core's stamp: read again when it moved.
    public func follow(_ s: UInt64?) {
        if reply != nil, s == stamp { return }
        stamp = s
        reload()
    }

    public func reload() {
        if busy {
            again = true
            return
        }
        busy = true
        Task {
            do {
                let r = try await model.core.request("report.get", params, doc: model.requestDoc)
                if r != reply { reply = r }
                error = nil
            } catch {
                self.error = "\(error)"
            }
            busy = false
            if again {
                again = false
                reload()
            }
        }
    }

    /// A button's command (`{command, params}`).
    public func run(_ action: JSONValue) async {
        guard let c = action["command"].string else { return }
        do {
            _ = try await model.run(c, action["params"])
            error = nil
        } catch {
            self.error = "\(error)"
        }
        reload()
    }

    static func draftKey(_ edit: JSONValue, row: String) -> String { "\(edit["command"].string ?? "")|\(row)" }

    /// A field's text, sent as its command takes it (`{…params, key: text}`; read as JSON when the
    /// edit says `json`). True when taken.
    @discardableResult
    public func commit(_ edit: JSONValue, row: String, text: String) async -> Bool {
        guard let c = edit["command"].string, let k = edit["key"].string else { return false }
        let dk = Self.draftKey(edit, row: row)
        if text == (edit["value"].string ?? "") {
            drafts[dk] = nil
            return true
        }
        var p = edit["params"].object ?? [:]
        if edit["json"].bool == true {
            guard let data = text.data(using: .utf8), let v = try? JSONDecoder().decode(JSONValue.self, from: data) else {
                refused[dk] = "That isn't JSON."
                return false
            }
            p[k] = v
        } else {
            p[k] = .string(text)
        }
        do {
            _ = try await model.run(c, .object(p))
            refused[dk] = nil
            drafts[dk] = nil
            reload()
            return true
        } catch {
            refused[dk] = "\(error)"
            return false
        }
    }

    /// The person closed the window: its Close action in the core.
    public func close() async {
        if let a = reply?["actions"].array?.first(where: { $0["label"].string == "Close" }) {
            _ = try? await model.run(a["command"].string ?? "", a["params"])
        } else if key == "cache" {
            _ = try? await model.run("run.cacheReport", ["open": false])
        } else if key == "params" {
            _ = try? await model.run("view.params", ["open": false])
        } else if key.hasPrefix("editor:") {
            _ = try? await model.run("node.editor", ["node": .string(String(key.dropFirst("editor:".count))), "open": false])
        }
    }

    /// For scripts (`--status-file`).
    public var describe: JSONValue {
        ["key": .string(key), "title": .string(title), "sections": .uint(UInt64(reply?["sections"].array?.count ?? 0)),
         "rows": .uint(UInt64((reply?["sections"].array ?? []).reduce(0) { $0 + ($1["rows"].array?.count ?? 0) })),
         "error": error.map(JSONValue.string) ?? .null]
    }
}

/// A report drawn in the platform's grouped list: sections of rows, each a label and value, its
/// fields and its buttons.
public struct ReportView: SwiftUI.View {
    @Bindable var report: ReportModel
    let done: (() -> Void)?

    public init(report: ReportModel, done: (() -> Void)? = nil) {
        self.report = report
        self.done = done
    }

    public var body: some SwiftUI.View {
        NavigationStack {
            content
                .navigationTitle(report.title)
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
                .toolbar {
                    if let done { ToolbarItem(placement: .confirmationAction) { Button("Done", action: done).accessibilityIdentifier("report-done") } }
                }
        }
        .accessibilityIdentifier("report-\(report.key)")
        .onAppear { report.reload() }
    }

    @ViewBuilder private var content: some SwiftUI.View {
        if let r = report.reply {
            if let empty = r["empty"].string {
                ContentUnavailableView(report.title, systemImage: report.key == "cache" ? "internaldrive" : "slider.horizontal.3", description: Text(empty))
            } else {
                List {
                    if let intro = r["intro"].string { Text(intro).font(.footnote).foregroundStyle(.secondary) }
                    if let e = report.error { Text(e).font(.footnote).foregroundStyle(.red) }
                    ForEach(Array((r["sections"].array ?? []).enumerated()), id: \.offset) { _, s in
                        let rows = s["rows"].array ?? []
                        Section {
                            ForEach(Array(rows.enumerated()), id: \.offset) { i, row in
                                ReportRow(report: report, row: row, id: row["id"].string ?? "\(s["title"].string ?? "")#\(i)")
                            }
                        } header: {
                            if let t = s["title"].string, !t.isEmpty { Text(t) }
                        }
                    }
                }
                #if os(macOS)
                .listStyle(.inset(alternatesRowBackgrounds: true))
                #else
                .listStyle(.insetGrouped)
                #endif
            }
        } else if let e = report.error {
            ContentUnavailableView(report.title, systemImage: "exclamationmark.triangle", description: Text(e))
        } else {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

struct ReportRow: SwiftUI.View {
    @Bindable var report: ReportModel
    let row: JSONValue
    let id: String

    var body: some SwiftUI.View {
        VStack(alignment: .leading, spacing: 6) {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline) {
                    Text(row["label"].string ?? "").fontWeight(.medium)
                    Spacer(minLength: 12)
                    valueText.multilineTextAlignment(.trailing)
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(row["label"].string ?? "").fontWeight(.medium)
                    valueText
                }
            }
            if let d = row["detail"].string, !d.isEmpty { Text(d).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
            ForEach(Array((row["edits"].array ?? []).enumerated()), id: \.offset) { _, e in
                ReportField(report: report, edit: e, row: id)
            }
            let actions = row["actions"].array ?? []
            if !actions.isEmpty {
                HStack(spacing: 14) {
                    ForEach(Array(actions.enumerated()), id: \.offset) { _, a in
                        Button(role: a["destructive"].bool == true ? .destructive : nil) {
                            Task { await report.run(a) }
                        } label: {
                            if let icon = a["icon"].string {
                                Label(a["label"].string ?? "", systemImage: icon).labelStyle(.iconOnly)
                            } else {
                                Text(a["label"].string ?? "")
                            }
                        }
                        .help(a["help"].string ?? a["label"].string ?? "")
                        .accessibilityIdentifier("report-action-\(id)-\(a["label"].string ?? "")")
                    }
                }
                .buttonStyle(.borderless)
                .font(.callout)
            }
        }
        .padding(.vertical, 2)
    }

    private var valueText: some SwiftUI.View {
        Text(row["value"].string ?? "")
            .foregroundStyle(row["warn"].bool == true ? Color.orange : Color.secondary)
            .monospacedDigit()
            .textSelection(.enabled)
    }
}

/// A field whose text a command takes: on Return (one line) or Apply (several).
struct ReportField: SwiftUI.View {
    @Bindable var report: ReportModel
    let edit: JSONValue
    let row: String

    var body: some SwiftUI.View {
        let key = ReportModel.draftKey(edit, row: row)
        let text = Binding<String>(get: { report.drafts[key] ?? edit["value"].string ?? "" }, set: { report.drafts[key] = $0 })
        VStack(alignment: .leading, spacing: 4) {
            if edit["multiline"].bool == true {
                Text(edit["label"].string ?? "").font(.caption).foregroundStyle(.secondary)
                TextEditor(text: text)
                    .font(.body.monospaced())
                    .frame(minHeight: 120)
                    .accessibilityIdentifier("report-field-\(row)-\(edit["key"].string ?? "")")
                HStack {
                    Button("Apply") { Task { await report.commit(edit, row: row, text: text.wrappedValue) } }
                        .disabled(report.drafts[key] == nil)
                        .accessibilityIdentifier("report-apply-\(row)")
                    Button("Revert") { report.drafts[key] = nil; report.refused[key] = nil }.disabled(report.drafts[key] == nil)
                }
                .buttonStyle(.borderless)
            } else {
                LabeledContent(edit["label"].string ?? "") {
                    TextField(edit["hint"].string ?? "", text: text)
                        .multilineTextAlignment(.trailing)
                        .onSubmit { Task { await report.commit(edit, row: row, text: text.wrappedValue) } }
                        .accessibilityIdentifier("report-field-\(row)-\(edit["key"].string ?? "")")
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        #endif
                }
                .font(.callout)
            }
            if let why = report.refused[key] { Text(why).font(.caption).foregroundStyle(.red) }
        }
    }
}

// MARK: - Compare (N4 slice 6)

/// Compare's state and sides (`compare.get`), read again when the core's stamp moves.
@MainActor
@Observable
public final class CompareModel {
    @ObservationIgnored public let model: DocumentModel
    @ObservationIgnored public let pictures: Pictures
    public var reply: JSONValue?
    public var error: String?
    @ObservationIgnored private var stamp: UInt64?

    public init(model: DocumentModel, pictures: Pictures) {
        self.model = model
        self.pictures = pictures
    }

    public func follow(_ s: UInt64?) {
        if reply != nil, s == stamp { return }
        stamp = s
        reload()
    }

    public func reload() {
        Task {
            do {
                reply = try await model.core.request("compare.get", [:], doc: model.requestDoc)
                error = nil
            } catch {
                self.error = "\(error)"
            }
        }
    }

    /// `viewer.compare {…}` or `viewer.unpin {…}`.
    public func run(_ command: String, _ params: JSONValue) async {
        do {
            _ = try await model.run(command, params)
            error = nil
        } catch {
            self.error = "\(error)"
        }
        reload()
    }

    public func close() async {
        _ = try? await model.run("viewer.compare", ["on": false])
    }

    public var describe: JSONValue {
        ["how": reply?["how"] ?? .null, "a": reply?["a"]["title"] ?? .null, "b": reply?["b"]["title"] ?? .null]
    }
}

/// A and B side by side, or one over the other with a divider (`viewer.compare {how, split}`).
/// The difference itself (the egui editor's third way) is computed from the arrays' own numbers;
/// the Apple apps don't offer it yet, and say so rather than draw it from the pictures.
public struct CompareView: SwiftUI.View {
    @Bindable var compare: CompareModel
    let done: (() -> Void)?
    @State private var split: Double = 0.5

    public init(compare: CompareModel, done: (() -> Void)? = nil) {
        self.compare = compare
        self.done = done
    }

    public var body: some SwiftUI.View {
        NavigationStack {
            content
                .navigationTitle("Compare")
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #endif
                .toolbar {
                    ToolbarItem(placement: .principal) {
                        Picker("Show", selection: Binding(get: { compare.reply?["how"].string == "split" ? "split" : "side" }, set: { how in
                            Task { await compare.run("viewer.compare", ["how": .string(how)]) }
                        })) {
                            Text("Side by Side").tag("side")
                            Text("Split").tag("split")
                        }
                        .pickerStyle(.segmented)
                        .fixedSize()
                        .accessibilityIdentifier("compare-how")
                    }
                    if let done { ToolbarItem(placement: .confirmationAction) { Button("Done", action: done).accessibilityIdentifier("compare-done") } }
                }
        }
        .accessibilityIdentifier("compare")
        .onAppear { compare.reload() }
        .onChange(of: compare.reply?["split"].double ?? 0.5) { _, s in split = s }
    }

    @ViewBuilder private var content: some SwiftUI.View {
        let r = compare.reply
        VStack(spacing: 8) {
            if let e = compare.error { Text(e).font(.footnote).foregroundStyle(.red) }
            if r?["how"].string == "split" {
                ZStack {
                    CompareSide(compare: compare, side: r?["b"] ?? .null, slot: "b")
                    CompareSide(compare: compare, side: r?["a"] ?? .null, slot: "a")
                        .mask(GeometryReader { g in Rectangle().frame(width: g.size.width * split).frame(maxWidth: .infinity, alignment: .leading) })
                }
                Slider(value: $split, in: 0...1) { Text("Divider") } onEditingChanged: { editing in
                    if !editing { Task { await compare.run("viewer.compare", ["split": .double(split)]) } }
                }
                .accessibilityIdentifier("compare-split")
                .padding(.horizontal)
            } else {
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 8) {
                        CompareSide(compare: compare, side: r?["a"] ?? .null, slot: "a").frame(minWidth: 280)
                        CompareSide(compare: compare, side: r?["b"] ?? .null, slot: "b").frame(minWidth: 280)
                    }
                    VStack(spacing: 8) {
                        CompareSide(compare: compare, side: r?["a"] ?? .null, slot: "a")
                        CompareSide(compare: compare, side: r?["b"] ?? .null, slot: "b")
                    }
                }
            }
            Text("The exact difference of A and B is the egui editor's for now.").font(.caption).foregroundStyle(.secondary)
        }
        .padding(8)
    }
}

struct CompareSide: SwiftUI.View {
    @Bindable var compare: CompareModel
    let side: JSONValue
    let slot: String
    @State private var image: CGImage?

    var body: some SwiftUI.View {
        VStack(spacing: 4) {
            HStack {
                Text(slot.uppercased()).font(.headline)
                Text(side["title"].string ?? "Not pinned").font(.callout).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
                if !side.isNull {
                    Button("Unpin") { Task { await compare.run("viewer.unpin", ["slot": .string(slot)]) } }
                        .buttonStyle(.borderless)
                        .accessibilityIdentifier("compare-unpin-\(slot)")
                }
            }
            Group {
                if let image {
                    Image(decorative: image, scale: 1).resizable().interpolation(.medium).aspectRatio(contentMode: .fit)
                } else if side.isNull {
                    Text("Pin a value as \(slot.uppercased()) from its viewer's bar.").font(.callout).foregroundStyle(.secondary)
                } else if let s = side["summary"].string {
                    Text(s).font(.callout).foregroundStyle(.secondary).textSelection(.enabled)
                } else {
                    ProgressView()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 6))
        }
        .task(id: Self.handle(side) ?? "") {
            guard let (h, want, px) = Self.source(side) else {
                image = nil
                return
            }
            image = await compare.pictures.load(h, want: want, px: px)?.image
        }
    }

    static func handle(_ side: JSONValue) -> String? { source(side).map { "\($0.0)\($0.1)" } }

    /// The picture a side shows, as a pop-out's would (`PictureSource.of`): an array's first view,
    /// a picture file decoded, an SVG drawn, else the engine's rendition.
    static func source(_ side: JSONValue) -> (String, String, Int?)? {
        guard !side.isNull else { return nil }
        if side["viewer"].string == "svg", let h = side["handle"].string { return (h, "svg", 1024) }
        // a rendition's handle is its DataRef's (`ref.handle`)
        func handle(_ v: JSONValue) -> String? { v["handle"].string ?? v["ref"]["handle"].string }
        if let v = side["views"].array?.first, let h = handle(v) { return (h, "rgba", nil) }
        if side["media"].string == "image", let h = side["handle"].string { return (h, "image", 2048) }
        if let h = handle(side["picture"]) { return (h, "rgba", nil) }
        return nil
    }
}

/// What a report or Compare window is, by the core's key, for both apps' window managers.
@MainActor
public enum CoreWindowKeys {
    /// The keys open now: the reports', and `compare`.
    public static func of(_ w: Windows) -> [String] {
        var out = w.reportKeys
        if w.compare == true { out.append("compare") }
        return out
    }
}
