// The remaining viewers (N4 slice 5; note 24 §8's order, contract 13), shared by both apps: SwiftUI
// over the platform's own controls (a SwiftUI Table is an NSTableView on the Mac and a collection
// view on the iPad; disclosure groups are the system's outline; AVFoundation plays). Each draws
// what the core gives (`popout.get`, `viewer.data`) and sends what the person does as the core's
// commands (`viewer.table`, `viewer.tree`, `viewer.audio`, `viewer.grid`, `viewer.hex`,
// `view.viewer`, `run.item`, `viewer.sheet`, `value.copy`, `value.share`, `value.saveAs`), so a
// script sets and reads the same state. What stays here is motion only: scrolling, a column's
// width, the playing clock.
//
// Nothing large is read on the main thread: the core reads and pages on its media workers, and
// what is lent (a sound's samples, a waveform's peaks) is turned into buffers on a detached task.

@preconcurrency import AVFoundation
import Foundation
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import SwiftUI

/// What a viewer is shown with: the pop-out, the pictures the core lends, the value now, and
/// whether sound may play (never from an automated run).
@MainActor
@Observable
public final class ViewerBox {
    public let popout: PopoutModel
    @ObservationIgnored public let pictures: Pictures
    public var value: PopoutValue?
    /// The core's stamp for the pop-out when `value` was last given (`PopoutWindow.stamp`): a viewer
    /// whose page depends on more than the value (a chart's spec in the graph) keys on it.
    public var stamp: UInt64 = 0
    /// A scripted run (`--scratch-state` or `--status-file`): the audio device is never opened, the clock runs silent.
    public let muted: Bool
    /// What the viewer shows now, for scripts (`--status-file`).
    @ObservationIgnored public var describe: [String: JSONValue] = [:] { didSet { onDescribe?() } }
    /// Told when `describe` changes (the app writes its status file again).
    @ObservationIgnored public var onDescribe: (() -> Void)?
    /// Bumped by a person's action so a view reads its page again.
    public var reload = 0

    public init(popout: PopoutModel, pictures: Pictures, muted: Bool) {
        self.popout = popout
        self.pictures = pictures
        self.muted = muted
    }

    /// The viewers the apps draw in SwiftUI (picture, SVG, text and scalar are each app's own).
    public static let kinds: Set<String> = ["array.plot", "table.chart", "array.pictures", "array.grid", "audio", "table.grid", "yaml", "tree", "items", "hex"]

    /// Shown here: one of [`kinds`], or a viewer offered that no Apple viewer draws (the engine's
    /// picture stands in, contract 13.3), or a value whose kind has no viewer at all.
    public static func handles(_ v: PopoutValue) -> Bool {
        guard let id = v.viewer else { return false }
        if kinds.contains(id) { return true }
        return !["picture", "svg", "text.plain", "scalar"].contains(id)
    }

    func state(_ part: String) -> JSONValue { value?.state?[part] ?? .null }
}

/// The viewer for a pop-out's value, with the bar every viewer has: View As (the viewers offered),
/// and Copy, Share and Save As (the core's commands, the existing asks).
public struct ViewerView: SwiftUI.View {
    @Bindable var box: ViewerBox

    public init(box: ViewerBox) { self.box = box }

    public var body: some SwiftUI.View {
        VStack(spacing: 0) {
            if let v = box.value {
                ViewerBar(box: box, value: v)
                Divider()
                body(for: v).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                Text("Reading the value…").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }

    @ViewBuilder
    private func body(for v: PopoutValue) -> some SwiftUI.View {
        switch v.viewer ?? "" {
        case "array.plot", "table.chart": ChartViewer(box: box)
        case "table.grid": TableViewer(box: box)
        case "array.grid": GridViewer(box: box)
        case "tree": OutlineViewer(box: box)
        case "yaml": YamlViewer(box: box)
        case "hex": HexViewer(box: box)
        case "audio": AudioViewer(box: box)
        case "items": ItemsViewer(box: box)
        case "array.pictures": ArrayPicturesViewer(box: box)
        default: OpaqueViewer(box: box)
        }
    }
}

/// A list of lists' steppers inside the run shown (interface 7.6 rule 2, decision 6A): one a
/// level below the runs, "item j of m" for the innermost, each asking `run.item {index, path}`
/// with its own level changed. Nothing for a list that isn't nested.
public struct LevelSteppers: SwiftUI.View {
    let box: ViewerBox
    let value: PopoutValue

    /// One inner level: what it steps, where it is (`at` of `of`), and the path to each item.
    public struct Step: Hashable, Sendable {
        public var word: String
        public var at: Int64
        public var of: UInt64
        public var level: Int
        public var shown: [Int64]

        /// The path that shows item `j` of this level: the levels above kept, those below at their first.
        public func path(_ j: Int64) -> [Int64] { Array(shown.prefix(level)) + [j] }
    }

    /// The outer stepper's word: "run" for a list of lists, else "item".
    public nonisolated static func outerWord(_ value: PopoutValue) -> String {
        (value.levels ?? []).count > 1 ? "run" : "item"
    }

    public nonisolated static func steps(_ value: PopoutValue) -> [Step] {
        let levels = value.levels ?? [], path = value.path ?? []
        let deep = max(levels.count - 1, 0)
        return (0..<deep).map { k in
            Step(word: k + 1 == deep ? "item" : "run", at: k < path.count ? path[k] : 0, of: levels[k + 1], level: k, shown: path)
        }
    }

    public var body: some SwiftUI.View {
        ForEach(Self.steps(value), id: \.level) { s in
            Divider().frame(height: 14)
            Button { go(s, s.at - 1) } label: { Image(systemName: "chevron.left") }
                .disabled(s.at <= 0).accessibilityLabel("Previous \(s.word.capitalized) in Run")
            // its own width, so a crowded bar never squeezes it out
            Text("\(s.word) \(s.at + 1) of \(s.of)").font(.caption).monospacedDigit().fixedSize().layoutPriority(1)
                .accessibilityIdentifier("viewer-level-\(s.level)")
            Button { go(s, s.at + 1) } label: { Image(systemName: "chevron.right") }
                .disabled(s.at + 1 >= Int64(s.of)).accessibilityLabel("Next \(s.word.capitalized) in Run")
        }
    }

    private func go(_ s: Step, _ j: Int64) {
        guard j >= 0, UInt64(j) < s.of else { return }
        let p = box.popout
        let index = value.index ?? 0
        Task { await p.command("run.item", ["index": .int(index), "path": .array(s.path(j).map { .int($0) })]) }
    }
}

/// View As and the ways out; a list's stepper.
public struct ViewerBar: SwiftUI.View {
    let box: ViewerBox
    let value: PopoutValue

    public init(box: ViewerBox, value: PopoutValue) {
        self.box = box
        self.value = value
    }

    public var body: some SwiftUI.View {
        // B161: in a narrow window (an iPhone, a narrow iPad window) the bar takes two or three
        // rows instead of running off both edges: the widest arrangement that fits is used
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 10) {
                choosers
                itemStepper
                Spacer(minLength: 4)
                actions
            }
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 10) {
                    choosers
                    itemStepper
                }
                HStack(spacing: 10) {
                    Spacer(minLength: 4)
                    actions
                }
            }
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 10) { choosers }
                HStack(spacing: 10) { itemStepper }
                HStack(spacing: 10) {
                    Spacer(minLength: 4)
                    actions
                }
            }
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }

    /// View As and an array's View mode.
    @ViewBuilder private var choosers: some SwiftUI.View {
            if let offered = value.viewers, offered.count > 1 {
                Picker("View As", selection: Binding(get: { value.viewer ?? "" }, set: { id in
                    let p = box.popout
                    Task { await p.choose(viewer: id) }
                })) {
                    ForEach(offered, id: \.id) { o in Text(o.native == false ? "\(o.label) (picture)" : o.label).tag(o.id) }
                }
                .labelsHidden()
                .fixedSize()
                .accessibilityIdentifier("viewer-as")
            }
            // an array's view mode (B154 slice 1, floor F51): the core's `run.view`; the run that
            // follows (cached) brings the mode's pictures
            if value.modes.count > 1 {
                ViewModePicker(box: box, value: value)
            }
    }

    /// A list's stepper (unchanged, moved here by B161).
    @ViewBuilder private var itemStepper: some SwiftUI.View {
            if let n = value.items, n > 1 {
                let i = value.index ?? 0
                let names = Self.itemNames(value)
                Button { step(i - 1, n) } label: { Image(systemName: "chevron.left") }.disabled(i <= 0).accessibilityLabel("Previous Item")
                if names.isEmpty {
                    Text("\(LevelSteppers.outerWord(value)) \(i + 1) of \(n)").font(.caption).monospacedDigit()
                } else {
                    // a list of named pictures (SIGNALS's planes, say): each by its name, as the
                    // egui editor's list of items has them (`run.item`)
                    Menu {
                        ForEach(Array(names.enumerated()), id: \.offset) { k, name in
                            Button { step(Int64(k), n) } label: {
                                if Int64(k) == i { Label(name, systemImage: "checkmark") } else { Text(name) }
                            }
                        }
                    } label: {
                        Text(Self.itemTitle(names, i, n)).font(.caption).monospacedDigit()
                    }
                    .fixedSize()
                    .accessibilityIdentifier("viewer-items")
                }
                Button { step(i + 1, n) } label: { Image(systemName: "chevron.right") }.disabled(i + 1 >= n).accessibilityLabel("Next Item")
                // a list of lists (decision 6A): a stepper a level inside the run
                LevelSteppers(box: box, value: value)
            }
    }

    /// Drag out, Compare, Recipe, Copy, Share and Save As.
    @ViewBuilder private var actions: some SwiftUI.View {
            let p = box.popout
            // drag the value out as a file of its own (`value.export`, N4 slice 6)
            Image(systemName: "arrow.up.and.down.and.arrow.left.and.right")
                .foregroundStyle(.tint)
                .help("Drag the value out as a file")
                .accessibilityLabel("Drag Out")
                .accessibilityIdentifier("viewer-drag")
                .onDrag { DragOut.provider(p, name: value.title ?? p.window.title) }
            Menu {
                Button("Pin as A") { Task { await p.command("viewer.pin", ["slot": "a"]) } }
                Button("Pin as B") { Task { await p.command("viewer.pin", ["slot": "b"]) } }
            } label: {
                Label("Compare", systemImage: "rectangle.split.2x1")
            }
            .labelStyle(.iconOnly)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("Pin this value as A or B and compare")
            .accessibilityIdentifier("viewer-compare")
            // what made the value (decision 27, F56): the core's `viewer.recipe`, offered only
            // for a value that has one (`hasRecipe`: a lineage-tracking engine's)
            if p.value?.hasRecipe == true {
                RecipeButton(popout: p)
            }
            Button { Task { await p.takeOut("value.copy") } } label: { Label("Copy", systemImage: "doc.on.doc") }
                .labelStyle(.iconOnly).help("Copy the value").accessibilityIdentifier("viewer-copy")
            Button { Task { await p.takeOut("value.share") } } label: { Label("Share…", systemImage: "square.and.arrow.up") }
                .labelStyle(.iconOnly).help("Share the value").accessibilityIdentifier("viewer-share")
            Button { Task { await p.takeOut("value.saveAs") } } label: { Label("Save As…", systemImage: "square.and.arrow.down") }
                .labelStyle(.iconOnly).help("Save the value to a file").accessibilityIdentifier("viewer-save")
    }

    /// A list's items by name (`sheet`'s labels, in order), or none when the engine named none.
    public nonisolated static func itemNames(_ value: PopoutValue) -> [String] {
        let cells = value.sheet?.array ?? []
        let names = cells.map { $0["label"].string ?? "" }
        return names.contains(where: { !$0.isEmpty }) ? names.enumerated().map { $1.isEmpty ? "item \($0 + 1)" : $1 } : []
    }

    /// The stepper's words for item `i` of `n`: its name and where it is ("Lab · 1 of 11").
    public nonisolated static func itemTitle(_ names: [String], _ i: Int64, _ n: UInt64) -> String {
        let name = i >= 0 && Int(i) < names.count ? names[Int(i)] : "item \(i + 1)"
        return "\(name) · \(i + 1) of \(n)"
    }

    private func step(_ i: Int64, _ n: UInt64) {
        guard i >= 0, UInt64(i) < n else { return }
        let p = box.popout
        Task { await p.command("run.item", ["index": .int(i)]) }
    }
}

/// The viewer bar's Recipe (decision 27; floor F56): what made the value, as the core gives it
/// (`viewer.recipe {popout}`: `{recipe: {style?, params, settings, graph}}`, or null when the
/// engine sent none), shown in a popover (a sheet in compact width), with Copy
/// (`viewer.recipe {copy: true}`, the core's clipboard ask).
struct RecipeButton: SwiftUI.View {
    let popout: PopoutModel
    /// What the popover shows, given with its presentation (B128: a popover adapted to a sheet in
    /// compact width drew the text it had before the reply, "No recipe", when kept apart from it)
    struct Shown: Identifiable {
        let id = UUID()
        let text: String?
    }
    @State private var shown: Shown?

    var body: some SwiftUI.View {
        Button {
            let p = popout
            Task {
                let r = await p.command("viewer.recipe")
                let recipe = r?["recipe"]
                let text: String? = (recipe == nil || recipe?.isNull == true) ? nil : recipe.map { v in
                    let o = try? JSONSerialization.jsonObject(with: v.serialized(), options: [.fragmentsAllowed])
                    let d = o.flatMap { try? JSONSerialization.data(withJSONObject: $0, options: [.prettyPrinted, .sortedKeys, .fragmentsAllowed]) }
                    return d.map { String(decoding: $0, as: UTF8.self) } ?? v.text
                }
                shown = Shown(text: text)
                // what the popover shows, for scripts (its first lines)
                StatusFile.shared.set("recipe", ["popout": .uint(p.id), "shown": true, "has": .bool(text != nil),
                                                 "text": .string(String((text ?? "").prefix(400)))])
            }
        } label: { Label("Recipe", systemImage: "list.bullet.rectangle") }
            .labelStyle(.iconOnly)
            .help("What made this value")
            .accessibilityIdentifier("viewer-recipe")
            .popover(item: $shown) { s in
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("Recipe").font(.headline)
                        Spacer()
                        if s.text != nil {
                            Button("Copy") {
                                let p = popout
                                Task { await p.takeOut("viewer.recipe", ["copy": true]) }
                            }
                            .accessibilityIdentifier("recipe-copy")
                        }
                    }
                    if let text = s.text {
                        ScrollView {
                            Text(text).font(.caption.monospaced()).textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .accessibilityIdentifier("recipe-text")
                    } else {
                        Text("No recipe: the engine sent none for this value.").foregroundStyle(.secondary)
                            .accessibilityIdentifier("recipe-none")
                    }
                }
                .padding(14)
                .frame(minWidth: 320, idealWidth: 420, maxWidth: 560, minHeight: 120, idealHeight: 320, maxHeight: 560)
            }
    }
}

/// A value dragged out of its viewer: the file `value.export` writes, made when the drop asks.
@MainActor
enum DragOut {
    static func provider(_ p: PopoutModel, name: String) -> NSItemProvider {
        let provider = NSItemProvider()
        provider.suggestedName = name.replacingOccurrences(of: "/", with: "-").replacingOccurrences(of: ":", with: "-")
        provider.registerFileRepresentation(forTypeIdentifier: "public.data", fileOptions: [], visibility: .all) { done in
            let deliver = Deliver(done)
            Task { @MainActor in
                if let f = await p.exportFile() {
                    deliver.call(URL(fileURLWithPath: f), nil)
                } else {
                    deliver.call(nil, CocoaError(.fileWriteUnknown))
                }
            }
            return nil
        }
        return provider
    }

    /// The drop's completion, handed to the main actor once the file is written.
    final class Deliver: @unchecked Sendable {
        // the system's completion handler: called once, from any thread
        let done: (URL?, Bool, (any Error)?) -> Void
        init(_ done: @escaping (URL?, Bool, (any Error)?) -> Void) { self.done = done }
        func call(_ url: URL?, _ error: (any Error)?) { done(url, false, error) }
    }
}

/// A short message in a viewer's place.
struct ViewerMessage: SwiftUI.View {
    let text: String
    var body: some SwiftUI.View {
        Text(text).foregroundStyle(.secondary).multilineTextAlignment(.center).padding().frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// A page bar: "rows 1–500 of 1,000,000", previous and next.
struct PageBar: SwiftUI.View {
    let what: String
    let start: UInt64
    let shown: Int
    let total: UInt64
    let page: UInt64
    let pages: UInt64
    let go: (UInt64) -> Void

    var body: some SwiftUI.View {
        HStack {
            Text(total == 0 ? "no \(what)" : "\(what) \(start + 1)–\(start + UInt64(shown)) of \(total)").font(.caption).monospacedDigit().foregroundStyle(.secondary)
            Spacer()
            if pages > 1 {
                Button { go(page - 1) } label: { Image(systemName: "chevron.left") }.disabled(page == 0).accessibilityLabel("Previous Page")
                Text("page \(page + 1) of \(pages)").font(.caption).monospacedDigit()
                Button { go(page + 1) } label: { Image(systemName: "chevron.right") }.disabled(page + 1 >= pages).accessibilityLabel("Next Page")
            }
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, 12)
        .padding(.vertical, 5)
    }
}

// MARK: - Table (table.grid) and grid (array.grid)

struct PageRow: Identifiable, Hashable {
    let id: Int
    let cells: [String]
    /// Each cell's place from its range's low to high end, 0 to 1 (colour by value), or nil.
    var tints: [Double?] = []
    /// The row is in its value's selection (note 26 slice 8).
    var selected = false
    /// Its row in the value (the page's `index`), what a selection names.
    var index: UInt64 = 0

    /// A page's rows, with their tints when the page has them (`viewer.data`'s `tints`).
    static func of(_ page: JSONValue) -> [PageRow] {
        let start = page["start"].uint64 ?? 0
        let tints = page["tints"].array ?? []
        let selected = page["selected"].array ?? []
        let index = page["index"].array ?? []
        return (page["cells"].array ?? []).enumerated().map { k, row in
            PageRow(id: Int(start) + k, cells: (row.array ?? []).map { $0.string ?? "" },
                    tints: k < tints.count ? (tints[k].array ?? []).map { $0.double.map { min(max($0, 0), 1) } } : [],
                    selected: k < selected.count && selected[k].bool == true,
                    index: k < index.count ? index[k].uint64 ?? UInt64(Int(start) + k) : UInt64(Int(start) + k))
        }
    }

    func tint(_ c: Int) -> Double? { c < tints.count ? tints[c] : nil }
}

/// A cell's background for colour by value: the accent, deeper for a larger value (the egui
/// editor's heat runs from the panel's colour to the chart colour).
struct CellTint: ViewModifier {
    let t: Double?
    func body(content: Content) -> some SwiftUI.View {
        if let t {
            content.background(Color.accentColor.opacity(0.08 + 0.5 * t))
        } else {
            content
        }
    }
}

/// Colour by value, and for a grid its number format: the core's state (`viewer.table` or
/// `viewer.grid` `{colored, format}`, note 26 slice 2), shown above the cells.
struct CellOptions: SwiftUI.View {
    let box: ViewerBox
    /// `viewer.table` or `viewer.grid`.
    let command: String
    let state: JSONValue
    var body: some SwiftUI.View {
        HStack(spacing: 12) {
            let on = state["colored"].bool ?? false
            Toggle("Colour by Value", isOn: Binding(get: { on }, set: { v in
                let p = box.popout
                Task { await p.command(command, ["colored": .bool(v)]) }
            }))
            .toggleStyle(.button)
            .controlSize(.small)
            .font(.caption)
            .help("Tint each number from its range's lowest to its highest (\(command) colored)")
            .accessibilityIdentifier("viewer-colour")
            if command == "viewer.grid" {
                let shown = state["format"].string
                Menu {
                    Button { set(.null) } label: { if shown == nil { Label("Default", systemImage: "checkmark") } else { Text("Default") } }
                    ForEach((state["formats"].array ?? []).compactMap(\.string), id: \.self) { f in
                        Button { set(.string(f)) } label: { if shown == f { Label(f, systemImage: "checkmark") } else { Text(f) } }
                    }
                } label: {
                    Text("Format: \(shown ?? "Default")").font(.caption)
                }
                .fixedSize()
                .buttonStyle(.borderless)
                .help("How the numbers are written (viewer.grid format)")
                .accessibilityIdentifier("viewer-format")
            }
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 4)
    }

    private func set(_ f: JSONValue) {
        let p = box.popout
        Task { await p.command("viewer.grid", ["format": f]) }
    }
}

struct PageColumn: Identifiable, Hashable {
    let id: Int
    let name: String
    let numeric: Bool
}

/// A sort a SwiftUI table asks for; the rows come sorted from the core, so it compares nothing.
struct CoreSort: SortComparator, Hashable {
    var column: Int
    var order: SortOrder = .forward
    func compare(_ a: PageRow, _ b: PageRow) -> ComparisonResult { .orderedSame }
}

/// Fetches a page whenever the value or the viewer's state changes.
@MainActor
@Observable
final class PageLoader {
    var page: JSONValue?
    var error: String?
    var loading = false
    private var asked: JSONValue?

    func load(_ box: ViewerBox, _ part: String, key: JSONValue, extra: [String: JSONValue] = [:]) {
        if key == asked && page != nil { return }
        asked = key
        loading = true
        let p = box.popout
        Task {
            let r = await p.data(part, extra)
            guard key == asked else { return }
            loading = false
            if r["status"].string == "ready" {
                page = r
                error = nil
            } else {
                error = r["error"].string ?? "the value couldn't be read"
            }
            box.describe[part + "Status"] = r["status"]
            if r["status"].string == "ready" {
                // what a script reads back: the rows and the first of them, the members open
                var d: [String: JSONValue] = ["status": "ready"]
                for k in ["rows", "cols", "page", "pages", "sort", "desc", "plane", "bytes", "totalChars", "kind", "selected", "selectedRows"] where r[k] != .null && r[k].array == nil { d[k] = r[k] }
                if let first = r["cells"][0].array { d["first"] = .array(Array(first.prefix(6))) }
                if let n = r["nodes"].object { d["open"] = .uint(UInt64(n.count)) }
                box.describe[part] = .object(d)
            }
        }
    }
}

struct TableViewer: SwiftUI.View {
    let box: ViewerBox
    @State private var loader = PageLoader()
    @State private var follow = SelectionFollow()

    var body: some SwiftUI.View {
        let st = box.state("table")
        let key: JSONValue = ["stamp": .uint(box.stamp), "st": st, "r": .int(Int64(box.reload))]
        VStack(spacing: 0) {
            if let e = loader.error {
                ViewerMessage(text: "This table can't be shown: \(e)")
            } else if let page = loader.page {
                let cols = (page["columns"].array ?? []).enumerated().map { PageColumn(id: $0.offset, name: $0.element["name"].string ?? "c\($0.offset + 1)", numeric: $0.element["numeric"].bool ?? false) }
                let start = page["start"].uint64 ?? 0
                let rows = PageRow.of(page)
                CellOptions(box: box, command: "viewer.table", state: st)
                if let n = page["selectedRows"].uint64 {
                    // the value's selection (note 26 slice 8, decision 7A): its rows marked, or only they
                    HStack(spacing: 10) {
                        Text("\(n) of \(page["allRows"].uint64 ?? 0) rows selected").font(.caption.monospacedDigit()).foregroundStyle(.orange)
                        SelectedOnlyButton(box: box, on: st["selectedOnly"].bool == true)
                        Button("Clear Selection") {
                            let p = box.popout
                            Task { await p.command("viewer.select", ["clear": true]) }
                        }
                        .buttonStyle(.borderless).font(.caption)
                        Spacer()
                    }
                    .padding(.horizontal, 12).padding(.vertical, 2)
                }
                let sort = Binding<[CoreSort]>(
                    get: { st["sort"].uint64.map { [CoreSort(column: Int($0), order: st["desc"].bool == true ? .reverse : .forward)] } ?? [] },
                    set: { new in
                        let p = box.popout
                        if let s = new.first {
                            Task { await p.command("viewer.table", ["sort": .uint(UInt64(s.column)), "desc": .bool(s.order == .reverse)]) }
                        } else {
                            Task { await p.command("viewer.table", ["sort": .null]) }
                        }
                    })
                #if os(macOS)
                ScrollViewReader { proxy in
                Table(rows, selection: Binding(get: { Set(rows.filter(\.selected).map(\.id)) }, set: { ids in
                    // a person's rows chosen in the table are the value's selection (decision 7A)
                    let p = box.popout
                    let picked = rows.filter { ids.contains($0.id) }.map { JSONValue.uint($0.index) }
                    Task { await p.command("viewer.select", picked.isEmpty ? ["clear": true] : ["rows": .array(picked)]) }
                }), sortOrder: sort) {
                    TableColumnForEach(cols) { c in
                        TableColumn(c.name, sortUsing: CoreSort(column: c.id)) { (r: PageRow) in
                            Text(c.id < r.cells.count ? r.cells[c.id] : "")
                                .font(c.numeric ? .body.monospacedDigit() : .body)
                                .frame(maxWidth: .infinity, alignment: c.numeric ? .trailing : .leading)
                                .modifier(CellTint(t: r.tint(c.id)))
                                .textSelection(.enabled)
                        }
                    }
                }
                .accessibilityIdentifier("viewer-table")
                .onScrollPhaseChange { _, phase in if phase == .interacting { follow.personScrolled = true } }
                .onChange(of: [page["selectionSeq"], page["selectedOnly"]]) { _, _ in follow.go(page, box: box) { proxy.scrollTo($0, anchor: .top) } }
                .onAppear { follow.go(page, box: box) { proxy.scrollTo($0, anchor: .top) } }
                }
                #else
                // a SwiftUI Table on iOS shows one column in compact width and starts its columns
                // far from the edge in a sheet (N4 slice 6): a grid of the page's own widths, and a
                // list of rows when the width is compact
                PageGrid(cols: cols, rows: rows, sort: sort.wrappedValue.first, onSort: { sort.wrappedValue = $0.map { [$0] } ?? [] }, id: "viewer-table", page: page, box: box)
                #endif
                Divider()
                PageBar(what: "rows", start: start, shown: rows.count, total: page["rows"].uint64 ?? 0, page: page["page"].uint64 ?? 0, pages: page["pages"].uint64 ?? 1) { n in
                    let p = box.popout
                    Task { await p.command("viewer.table", ["page": .uint(n)]) }
                }
            } else {
                ProgressView("Reading the table…").frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .onAppear { loader.load(box, "table", key: key) }
        .onChange(of: key) { _, k in loader.load(box, "table", key: k) }
    }
}

#if os(iOS)
/// A page of rows on iOS: a grid with a header row (a tap on a header sorts through the core, as a
/// table's header click does on the Mac) in regular width; a list of rows, each a card of its
/// columns, in compact width (an iPhone, a narrow Split View or Stage Manager window).
struct PageGrid: SwiftUI.View {
    let cols: [PageColumn]
    let rows: [PageRow]
    let sort: CoreSort?
    let onSort: ((CoreSort?) -> Void)?
    let id: String
    var rowNumbers = false
    /// The table's page and viewer, when the grid shows a value's table: it goes to a selection
    /// that arrives (`firstSelected`, `selectionSeq`) unless the person scrolled it.
    var page: JSONValue = .null
    var box: ViewerBox?
    @State private var follow = SelectionFollow()
    @Environment(\.horizontalSizeClass) private var size
    /// Widths a person dragged, by column (note 26 slice 3): a column's width is the shell's
    /// (docs/shell-interface.md 13.2.1), kept while the viewer shows this table.
    @State private var dragged: [Int: CGFloat] = [:]
    /// A drag on a column's edge in progress: the column and how far.
    @GestureState private var dragging: (column: Int, by: CGFloat)? = nil

    var body: some SwiftUI.View {
        if size == .compact { list } else { grid }
    }

    private func cell(_ r: PageRow, _ c: PageColumn) -> String { c.id < r.cells.count ? r.cells[c.id] : "" }

    /// Each column as wide as its header and the page's cells need, within reason, or as wide
    /// as a person dragged it (at least 48 points).
    private var widths: [CGFloat] {
        cols.map { c in
            // the header's sort chevron has room too
            let longest = rows.prefix(200).reduce(c.name.count + 3) { max($0, cell($1, c).count) }
            var w = dragged[c.id] ?? min(max(CGFloat(longest) * 10 + 28, 64), 360)
            if let d = dragging, d.column == c.id { w += d.by }
            return max(w, 48)
        }
    }

    /// The handle at a header's trailing edge: a drag sets the column's width.
    private func edge(_ c: PageColumn, _ width: CGFloat) -> some SwiftUI.View {
        Rectangle()
            .fill(Color.secondary.opacity(0.25))
            .frame(width: 2, height: 18)
            .padding(.horizontal, 5)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 2)
                    .updating($dragging) { g, st, _ in st = (c.id, g.translation.width) }
                    .onEnded { g in dragged[c.id] = max(width + g.translation.width, 48) }
            )
            .accessibilityLabel("Width of \(c.name)")
            .accessibilityValue("\(Int(width)) points")
            .accessibilityAdjustableAction { dir in
                dragged[c.id] = max(width + (dir == .increment ? 24 : -24), 48)
            }
            .accessibilityIdentifier("\(id)-edge-\(c.id)")
    }

    private func tapped(_ c: PageColumn) {
        guard let onSort else { return }
        if let s = sort, s.column == c.id {
            onSort(CoreSort(column: c.id, order: s.order == .forward ? .reverse : .forward))
        } else {
            onSort(CoreSort(column: c.id, order: .forward))
        }
    }

    private var grid: some SwiftUI.View {
        let w = widths
        // from the leading edge (a two-way scroll view centres content narrower than itself)
        return GeometryReader { g in ScrollViewReader { proxy in ScrollView([.horizontal, .vertical]) {
            LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                Section {
                    ForEach(rows) { r in
                        HStack(spacing: 0) {
                            if rowNumbers { Text("\(r.id)").font(.caption.monospacedDigit()).foregroundStyle(.secondary).frame(width: 56, alignment: .leading) }
                            ForEach(Array(cols.enumerated()), id: \.offset) { i, c in
                                Text(cell(r, c))
                                    .font(c.numeric ? .body.monospacedDigit() : .body)
                                    .lineLimit(1)
                                    .frame(width: w[i] - 16, alignment: c.numeric ? .trailing : .leading)
                                    .padding(.horizontal, 8)
                                    .modifier(CellTint(t: r.tint(c.id)))
                                    .textSelection(.enabled)
                            }
                        }
                        .padding(.vertical, 7)
                        // a selected row marked whole, under its cells (note 26 slice 8)
                        .background(r.selected ? Color.orange.opacity(0.22) : r.id % 2 == 1 ? Color.secondary.opacity(0.06) : Color.clear)
                        .id(r.id)
                    }
                } header: {
                    HStack(spacing: 0) {
                        if rowNumbers { Text("row").font(.caption).foregroundStyle(.secondary).frame(width: 56, alignment: .leading) }
                        ForEach(Array(cols.enumerated()), id: \.offset) { i, c in
                            Button { tapped(c) } label: {
                                HStack(spacing: 4) {
                                    if c.numeric { Spacer(minLength: 0) }
                                    Text(c.name).fontWeight(.semibold).lineLimit(1)
                                    if let s = sort, s.column == c.id { Image(systemName: s.order == .forward ? "chevron.up" : "chevron.down").font(.caption) }
                                    if !c.numeric { Spacer(minLength: 0) }
                                }
                                .frame(width: w[i] - 16)
                                .padding(.horizontal, 8)
                            }
                            .buttonStyle(.borderless)
                            .disabled(onSort == nil)
                            .accessibilityIdentifier("\(id)-header-\(c.id)")
                            .overlay(alignment: .trailing) { edge(c, w[i]) }
                        }
                    }
                    .padding(.vertical, 8)
                    .background(.background)
                    .overlay(alignment: .bottom) { Divider() }
                }
            }
            .padding(.leading, 8)
            .frame(minWidth: g.size.width, minHeight: g.size.height, alignment: .topLeading)
        }
        .onScrollPhaseChange { _, phase in if phase == .interacting { follow.personScrolled = true } }
        .onChange(of: [page["selectionSeq"], page["selectedOnly"]]) { _, _ in if let box { follow.go(page, box: box) { proxy.scrollTo($0, anchor: .top) } } }
        .onAppear { if let box { follow.go(page, box: box) { proxy.scrollTo($0, anchor: .top) } } }
        } }
        .accessibilityIdentifier(id)
    }

    private var list: some SwiftUI.View {
        List {
            if let onSort, !cols.isEmpty {
                Picker("Sort by", selection: Binding<Int>(get: { sort.map { $0.column } ?? -1 }, set: { c in
                    onSort(c < 0 ? nil : CoreSort(column: c, order: sort?.order ?? .forward))
                })) {
                    Text("Unsorted").tag(-1)
                    ForEach(cols) { c in Text(c.name).tag(c.id) }
                }
                .accessibilityIdentifier("\(id)-sort")
                if let s = sort {
                    Toggle("Descending", isOn: Binding(get: { s.order == .reverse }, set: { onSort(CoreSort(column: s.column, order: $0 ? .reverse : .forward)) }))
                }
            }
            ForEach(rows) { r in
                VStack(alignment: .leading, spacing: 3) {
                    if rowNumbers { Text("row \(r.id)").font(.caption).foregroundStyle(.secondary) }
                    ForEach(cols) { c in
                        HStack(alignment: .firstTextBaseline) {
                            Text(c.name).font(.callout).foregroundStyle(.secondary)
                            Spacer(minLength: 12)
                            Text(cell(r, c)).font(c.numeric ? .body.monospacedDigit() : .body).multilineTextAlignment(.trailing)
                                .modifier(CellTint(t: r.tint(c.id))).textSelection(.enabled)
                        }
                    }
                }
                .padding(.vertical, 2)
                .listRowBackground(r.selected ? Color.orange.opacity(0.22) : nil)
            }
        }
        .listStyle(.plain)
        .accessibilityIdentifier(id)
    }
}
#endif

struct GridViewer: SwiftUI.View {
    let box: ViewerBox
    @State private var loader = PageLoader()

    var body: some SwiftUI.View {
        let st = box.state("grid")
        let key: JSONValue = ["stamp": .uint(box.stamp), "st": st]
        VStack(spacing: 0) {
            if let e = loader.error {
                ViewerMessage(text: "These numbers can't be shown: \(e)")
            } else if let page = loader.page {
                let c0 = page["firstCol"].uint64 ?? 0
                let cells = page["cells"].array ?? []
                let width = cells.first?.array?.count ?? 0
                let cols = (0..<width).map { PageColumn(id: $0, name: "\(c0 + UInt64($0))", numeric: true) }
                let start = page["start"].uint64 ?? 0
                let rows = PageRow.of(page)
                HStack(spacing: 12) {
                    Text("\(page["dtype"].string ?? "") · \((page["shape"].array ?? []).compactMap(\.uint64).map(String.init).joined(separator: "×"))").font(.caption).foregroundStyle(.secondary)
                    if let planes = page["planes"].uint64, planes > 1 {
                        let plane = page["plane"].uint64 ?? 0
                        Stepper("\(page["planeName"].string ?? "plane") \(plane + 1) of \(planes)", onIncrement: {
                            let p = box.popout
                            Task { await p.command("viewer.grid", ["plane": .uint(min(plane + 1, planes - 1))]) }
                        }, onDecrement: {
                            let p = box.popout
                            Task { await p.command("viewer.grid", ["plane": .uint(plane == 0 ? 0 : plane - 1)]) }
                        }).font(.caption).fixedSize()
                    }
                    Spacer()
                    // which columns are shown is the core's window (decision 26): its page says
                    // where it is and whether it can step; the buttons step it (viewer.grid cols)
                    let prev = page["prevCols"].bool ?? false, next = page["nextCols"].bool ?? false
                    if let all = page["cols"].uint64, prev || next {
                        let end = page["endCol"].uint64 ?? (c0 + UInt64(width))
                        Button {
                            let p = box.popout
                            Task { await p.command("viewer.grid", ["cols": "prev"]) }
                        } label: { Image(systemName: "chevron.left") }.disabled(!prev).accessibilityLabel("Previous columns")
                        Text("columns \(c0)–\(end > 0 ? end - 1 : 0) of \(all)").font(.caption).monospacedDigit()
                        Button {
                            let p = box.popout
                            Task { await p.command("viewer.grid", ["cols": "next"]) }
                        } label: { Image(systemName: "chevron.right") }.disabled(!next).accessibilityLabel("Next columns")
                    }
                }
                .buttonStyle(.borderless)
                .padding(.horizontal, 12)
                .padding(.vertical, 5)
                CellOptions(box: box, command: "viewer.grid", state: st)
                if let stats = box.value?.stats?.array, !stats.isEmpty {
                    Text(ArrayStats.line(stats))
                        .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 12).frame(maxWidth: .infinity, alignment: .leading)
                }
                #if os(macOS)
                Table(rows) {
                    TableColumn("row") { (r: PageRow) in Text("\(r.id)").font(.caption.monospacedDigit()).foregroundStyle(.secondary) }.width(min: 36, ideal: 48)
                    TableColumnForEach(cols) { c in
                        TableColumn(c.name) { (r: PageRow) in
                            Text(c.id < r.cells.count ? r.cells[c.id] : "").font(.body.monospacedDigit()).frame(maxWidth: .infinity, alignment: .trailing)
                                .modifier(CellTint(t: r.tint(c.id)))
                        }
                        .width(min: 56, ideal: 84)
                    }
                }
                .accessibilityIdentifier("viewer-grid")
                #else
                PageGrid(cols: cols, rows: rows, sort: nil, onSort: nil, id: "viewer-grid", rowNumbers: true)
                #endif
                Divider()
                PageBar(what: "rows", start: start, shown: rows.count, total: page["rows"].uint64 ?? 0, page: page["page"].uint64 ?? 0, pages: page["pages"].uint64 ?? 1) { n in
                    let p = box.popout
                    Task { await p.command("viewer.grid", ["page": .uint(n)]) }
                }
            } else {
                ProgressView("Reading the numbers…").frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .onAppear { loader.load(box, "grid", key: key) }
        .onChange(of: key) { _, k in loader.load(box, "grid", key: k) }
    }
}

// MARK: - JSON outline (tree) and YAML

struct OutlineViewer: SwiftUI.View {
    let box: ViewerBox
    @State private var loader = PageLoader()

    var body: some SwiftUI.View {
        let st = box.state("tree")
        let key: JSONValue = ["stamp": .uint(box.stamp), "st": st]
        VStack(spacing: 0) {
            if let e = loader.error {
                ViewerMessage(text: "This value can't be outlined: \(e)")
            } else if let page = loader.page {
                HStack {
                    Text(page["root"]["text"].string ?? "").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                    Button("Collapse All") {
                        let p = box.popout
                        Task { await p.command("viewer.tree", ["collapseAll": true]) }
                    }
                    .buttonStyle(.borderless)
                    .accessibilityIdentifier("outline-collapse-all")
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 5)
                List {
                    OutlineMembers(box: box, nodes: page["nodes"], at: "")
                }
                .accessibilityIdentifier("viewer-outline")
            } else {
                ProgressView("Reading the value…").frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .onAppear { loader.load(box, "tree", key: key) }
        .onChange(of: key) { _, k in loader.load(box, "tree", key: k) }
    }
}

/// The children of the member at `at`, each open as the core says.
struct OutlineMembers: SwiftUI.View {
    let box: ViewerBox
    let nodes: JSONValue
    let at: String

    var body: some SwiftUI.View {
        let node = nodes[at]
        ForEach(Array((node["children"].array ?? []).enumerated()), id: \.offset) { _, c in
            let pointer = c["pointer"].string ?? ""
            let container = (c["kind"].string == "object" || c["kind"].string == "array") && (c["count"].uint64 ?? 0) > 0
            if container {
                DisclosureGroup(isExpanded: Binding(get: { nodes[pointer] != .null }, set: { open in
                    let p = box.popout
                    Task { await p.command("viewer.tree", [open ? "expand" : "collapse": .string(pointer)]) }
                })) {
                    if nodes[pointer] != .null { OutlineMembers(box: box, nodes: nodes, at: pointer) }
                } label: {
                    MemberRow(box: box, member: c)
                }
            } else {
                MemberRow(box: box, member: c)
            }
        }
        if let more = node["more"].uint64, more > 0 {
            Text("\(more) more not listed").font(.caption).foregroundStyle(.secondary)
        }
    }
}

struct MemberRow: SwiftUI.View {
    let box: ViewerBox
    let member: JSONValue

    var body: some SwiftUI.View {
        HStack(alignment: .firstTextBaseline) {
            Text(member["key"].string ?? "").fontWeight(.medium)
            Text(member["text"].string ?? "").foregroundStyle(member["kind"].string == "string" ? Color.primary : Color.secondary).lineLimit(3).textSelection(.enabled)
        }
        .contextMenu {
            let p = box.popout
            let pointer = member["pointer"].string ?? ""
            Button("Copy Value") { Task { await p.takeOut("value.copy", ["pointer": .string(pointer)]) } }
        }
    }
}

struct YamlViewer: SwiftUI.View {
    let box: ViewerBox
    @State private var loader = PageLoader()

    var body: some SwiftUI.View {
        // `box.stamp`, observed: the pop-out model itself isn't
        let key: JSONValue = ["stamp": .uint(box.stamp)]
        Group {
            if let e = loader.error {
                ViewerMessage(text: "This value can't be shown as YAML: \(e)")
            } else if let page = loader.page {
                ScrollView([.vertical, .horizontal]) {
                    Text(page["text"].string ?? "").font(.system(.body, design: .monospaced)).textSelection(.enabled).padding(12)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .accessibilityIdentifier("viewer-yaml")
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .onAppear { loader.load(box, "yaml", key: key) }
        .onChange(of: key) { _, k in loader.load(box, "yaml", key: k) }
    }
}

// MARK: - Bytes (hex) and values with no viewer of their own

struct HexViewer: SwiftUI.View {
    let box: ViewerBox
    @State private var loader = PageLoader()

    var body: some SwiftUI.View {
        let key: JSONValue = ["stamp": .uint(box.stamp), "st": box.state("hex")]
        VStack(spacing: 0) {
            if let e = loader.error {
                ViewerMessage(text: "These bytes can't be read: \(e)")
            } else if let page = loader.page {
                ScrollView([.vertical, .horizontal]) {
                    Text((page["lines"].array ?? []).compactMap(\.string).joined(separator: "\n"))
                        .font(.system(.footnote, design: .monospaced)).textSelection(.enabled).padding(12).frame(maxWidth: .infinity, alignment: .leading)
                }
                .accessibilityIdentifier("viewer-hex")
                Divider()
                let start = page["start"].uint64 ?? 0
                let total = page["bytes"].uint64 ?? 0
                PageBar(what: "bytes", start: start, shown: Int(min(page["pageBytes"].uint64 ?? 4096, total - min(start, total))), total: total, page: page["page"].uint64 ?? 0, pages: page["pages"].uint64 ?? 1) { n in
                    let p = box.popout
                    Task { await p.command("viewer.hex", ["page": .uint(n)]) }
                }
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .onAppear { loader.load(box, "hex", key: key) }
        .onChange(of: key) { _, k in loader.load(box, "hex", key: k) }
    }
}

/// A value no Apple viewer draws: what it is (its type, its media type, its size), the summary, and
/// the engine's picture of it if it sent one. Its bytes are never guessed at.
struct OpaqueViewer: SwiftUI.View {
    let box: ViewerBox

    var body: some SwiftUI.View {
        let v = box.value
        VStack(alignment: .leading, spacing: 8) {
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 4) {
                if let t = v?.type { GridRow { Text("Type").foregroundStyle(.secondary); Text(t).textSelection(.enabled) } }
                if let m = v?.mediaType, !m.isEmpty { GridRow { Text("Media").foregroundStyle(.secondary); Text(m) } }
                if let b = v?.bytes { GridRow { Text("Size").foregroundStyle(.secondary); Text(ByteCountFormatter.string(fromByteCount: Int64(b), countStyle: .file)) } }
                if let s = v?.summary { GridRow { Text("Summary").foregroundStyle(.secondary); Text(s).textSelection(.enabled) } }
                if let id = v?.viewer { GridRow { Text("Viewer").foregroundStyle(.secondary); Text("\(id): not drawn by this app; the engine's picture stands in") } }
            }
            .font(.callout)
            if let pic = v?.picture, let h = pic.handle {
                LentPicture(box: box, handle: h, want: "rgba", px: nil)
            } else {
                Text("The engine sent no picture of it.").foregroundStyle(.secondary)
                Spacer()
            }
        }
        .padding(12)
        .onAppear { box.describe["opaque"] = ["type": v?.type.map(JSONValue.string) ?? .null, "bytes": v?.bytes.map(JSONValue.uint) ?? .null] }
    }
}

/// A picture the core lends (decoded on a worker), fitted.
struct LentPicture: SwiftUI.View {
    let box: ViewerBox
    let handle: String
    let want: String
    let px: Int?
    @State private var image: CGImage?
    @State private var failed: String?

    var body: some SwiftUI.View {
        Group {
            if let image {
                Image(decorative: image, scale: 1).resizable().interpolation(.medium).aspectRatio(contentMode: .fit)
            } else if let failed {
                Text(failed).font(.caption).foregroundStyle(.secondary)
            } else {
                ProgressView()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task(id: "\(want)\(handle)\(px ?? 0)") {
            if let p = await box.pictures.load(handle, want: want, px: px) {
                image = p.image
            } else {
                failed = box.pictures.failures[handle] ?? "couldn't load"
            }
        }
    }
}

// MARK: - An array's view modes and channel histograms (B154 slice 1)

/// The view modes as words, and what each does (the egui editor's tips).
public enum ViewModes {
    public static func label(_ id: String) -> String {
        switch id {
        case "auto": "Auto"
        case "color": "Colour"
        case "stretch": "Stretch"
        case "clip": "Clip"
        case "heat": "Heat"
        case "labels": "Labels"
        case "angle": "Angle"
        default: id
        }
    }

    public static func help(_ id: String) -> String {
        switch id {
        case "auto": "Chosen for the array: labels for integers, colour for three channels in a colour space, else clip or stretch"
        case "color": "One picture: grey, grey and alpha, RGB (from its colour space) or RGBA"
        case "stretch": "Each channel from its own min to max"
        case "clip": "Each channel's values 0 to 1 as they are"
        case "heat": "Each channel stretched, in the Turbo colour map"
        case "labels": "A colour per integer value (negatives black)"
        case "angle": "Each value as a hue: one turn round the colour wheel"
        default: ""
        }
    }

    /// `run.view`'s params for `id`: `angle` keeps the turn shown (1 when none).
    public static func params(_ id: String, shown: JSONValue?) -> [String: JSONValue] {
        var p: [String: JSONValue] = ["view": .string(id)]
        if id == "angle" { p["params"] = ["turn": .double(shown?["params"]["turn"].double ?? 1)] }
        return p
    }
}

/// View: the array's view modes, the one shown checked; a choice is the core's `run.view`.
struct ViewModePicker: SwiftUI.View {
    let box: ViewerBox
    let value: PopoutValue

    var body: some SwiftUI.View {
        let shown = value.mode
        Menu {
            ForEach(value.modes, id: \.self) { id in
                Button {
                    guard id != shown else { return }
                    let p = box.popout
                    let params = ViewModes.params(id, shown: value.viewMode)
                    Task { await p.command("run.view", params) }
                } label: {
                    if id == shown { Label(ViewModes.label(id), systemImage: "checkmark") } else { Text(ViewModes.label(id)) }
                }
                .help(ViewModes.help(id))
                .accessibilityIdentifier("viewer-mode-\(id)")
            }
        } label: {
            Text("View: \(ViewModes.label(shown))").font(.caption)
        }
        .menuIndicator(.visible)
        .fixedSize()
        .help("How the array is drawn (run.view)")
        .accessibilityLabel("View Mode")
        .accessibilityValue(ViewModes.label(shown))
        .accessibilityIdentifier("viewer-mode")
    }
}

/// An array's channels: each one's range and NaN count, and its histogram as small columns (the
/// core's heights, already on the scale `viewer.array {log}` sets), with Log Scale beside them.
public struct ArrayStats: SwiftUI.View {
    let box: ViewerBox
    let value: PopoutValue

    public init(box: ViewerBox, value: PopoutValue) {
        self.box = box
        self.value = value
    }

    /// What a pop-out's status says of an array's mode and histograms, for scripts and UI tests:
    /// `viewMode` and `viewModes` (when offered), `histograms` (channels with one), `log`, `nan`.
    public static func facts(_ v: PopoutValue) -> [String: JSONValue] {
        var f: [String: JSONValue] = [:]
        if !v.modes.isEmpty {
            f["viewMode"] = .string(v.mode)
            f["viewModes"] = .uint(UInt64(v.modes.count))
        }
        let stats = v.stats?.array ?? []
        if !stats.isEmpty {
            f["histograms"] = .uint(UInt64(stats.filter { !heights($0["histogram"]).isEmpty }.count))
            f["log"] = .bool(v.state?["array"]["log"].bool ?? false)
            f["nan"] = .array(stats.map { .uint($0["nan"].uint64 ?? 0) })
        }
        return f
    }

    /// Whether `v` has channel statistics to show.
    public static func shows(_ v: PopoutValue) -> Bool { !(v.stats?.array ?? []).isEmpty }

    /// "L 0 to 100, 3 NaN · a −1 to 1": each channel's range, and its NaN count when it has any.
    public nonisolated static func line(_ stats: [JSONValue]) -> String {
        stats.map(channelText).joined(separator: " · ")
    }

    public nonisolated static func channelText(_ c: JSONValue) -> String {
        let nan = c["nan"].uint64 ?? 0
        return "\(c["name"].string ?? "") \(c["min"].string ?? "?") to \(c["max"].string ?? "?")" + (nan > 0 ? ", \(nan) NaN" : "")
    }

    /// A histogram's bars, 0 to 1, as the core gives them (anything else dropped to 0).
    public nonisolated static func heights(_ h: JSONValue) -> [Double] {
        (h["heights"].array ?? []).map { min(max($0.double ?? 0, 0), 1) }
    }

    public var body: some SwiftUI.View {
        let stats = value.stats?.array ?? []
        let histograms = stats.filter { !Self.heights($0["histogram"]).isEmpty }
        let log = value.state?["array"]["log"].bool ?? false
        VStack(alignment: .leading, spacing: 4) {
            if histograms.isEmpty {
                Text(Self.line(stats)).font(.caption).foregroundStyle(.secondary)
            } else {
                HStack(alignment: .top, spacing: 10) {
                    Toggle("Log", isOn: Binding(get: { log }, set: { on in
                        let p = box.popout
                        Task { await p.command("viewer.array", ["log": .bool(on)]) }
                    }))
                    .toggleStyle(.button)
                    .controlSize(.small)
                    .font(.caption)
                    .help("Histogram counts on a log scale (viewer.array)")
                    .accessibilityIdentifier("viewer-log")
                    ScrollView(.horizontal) {
                        HStack(alignment: .top, spacing: 12) {
                            ForEach(Array(stats.enumerated()), id: \.offset) { _, c in
                                ChannelHistogram(channel: c)
                            }
                        }
                    }
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("viewer-histograms")
    }
}

/// One channel: its range and NaN count, and its histogram's columns between the range's ends.
struct ChannelHistogram: SwiftUI.View {
    let channel: JSONValue

    var body: some SwiftUI.View {
        let h = channel["histogram"]
        let bars = ArrayStats.heights(h)
        VStack(alignment: .leading, spacing: 2) {
            Text(ArrayStats.channelText(channel)).font(.caption2.monospacedDigit()).foregroundStyle(.secondary).lineLimit(1)
            if !bars.isEmpty {
                Canvas { cx, size in
                    let w = size.width / CGFloat(bars.count)
                    var path = Path()
                    for (i, b) in bars.enumerated() where b > 0 {
                        let hgt = CGFloat(b) * size.height
                        path.addRect(CGRect(x: CGFloat(i) * w, y: size.height - hgt, width: max(w, 0.5), height: hgt))
                    }
                    cx.fill(path, with: .color(.accentColor.opacity(0.8)))
                }
                .frame(width: 176, height: 36)
                .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 2))
                HStack {
                    Text(h["lo"].string ?? "").font(.caption2.monospacedDigit()).foregroundStyle(.tertiary)
                    Spacer()
                    Text(h["hi"].string ?? "").font(.caption2.monospacedDigit()).foregroundStyle(.tertiary)
                }
                .frame(width: 176)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Histogram of \(channel["name"].string ?? "a channel")")
        .accessibilityValue(Self.spoken(channel))
        .accessibilityIdentifier("viewer-histogram-\(channel["name"].string ?? "")")
    }

    /// What VoiceOver says: the range, the bins, the largest count, the NaN count, the scale.
    static func spoken(_ c: JSONValue) -> String {
        let h = c["histogram"]
        var parts = ["\(c["min"].string ?? "?") to \(c["max"].string ?? "?")"]
        if let n = h["bins"].uint64 { parts.append("\(n) bins from \(h["lo"].string ?? "?") to \(h["hi"].string ?? "?")") }
        if let t = h["top"].uint64 { parts.append("largest count \(t)") }
        if let nan = c["nan"].uint64, nan > 0 { parts.append("\(nan) NaN") }
        if h["log"].bool == true { parts.append("log scale") }
        return parts.joined(separator: ", ")
    }
}

// MARK: - An array's pictures, a list's items

struct ArrayPicturesViewer: SwiftUI.View {
    let box: ViewerBox

    var body: some SwiftUI.View {
        let v = box.value
        let views = v?.views ?? []
        VStack(alignment: .leading, spacing: 6) {
            if let v, ArrayStats.shows(v) {
                ArrayStats(box: box, value: v)
            }
            if views.isEmpty, let pic = v?.picture, let h = pic.handle {
                LentPicture(box: box, handle: h, want: "rgba", px: nil)
            } else if views.isEmpty {
                ViewerMessage(text: "This array has no picture: see its values.")
            } else if let v {
                SharedZoomPictures(box: box, value: v, views: views)
            }
        }
        .padding(12)
        .onAppear { box.describe["pictures"] = .uint(UInt64(views.count)) }
    }
}

/// An array's several pictures (one per channel), sharing one zoom and view and one probe, as the
/// egui editor's array view does (note 26 slice 2): a pinch or a drag on any picture zooms and
/// pans them all, Fit forgets it, and a tap reads the value's own numbers there and pins the
/// probe (`viewer.probe`). The zoom and view are the shell's, kept in the picture's own terms
/// through the items of a list (`PictureZoom`, as B159 keeps a single picture's).
struct SharedZoomPictures: SwiftUI.View {
    let box: ViewerBox
    let value: PopoutValue
    let views: [PopoutValue.ViewPicture]
    /// The zoom kept (nil: each picture fits its tile).
    @State private var kept: PictureZoom?
    /// A pinch or a drag in progress, applied on top of what is kept.
    @GestureState private var pinch: CGFloat = 1
    @GestureState private var drag: CGSize = .zero
    @State private var reading = ""
    /// The zoom ask (`zoomAsk.seq`) last taken (B159): a script's `viewer.zoom` on the pop-out.
    @State private var askSeen: UInt64 = 0
    /// The tiles' size, the same for all.
    @State private var tile = CGSize(width: 1, height: 1)

    /// The value's size in pixels (the pictures may be smaller renditions of it).
    private var full: CGSize {
        if let f = PictureSource.fullSize(value) { return CGSize(width: f.w, height: f.h) }
        if let p = views.first { return CGSize(width: Int(p.w), height: Int(p.h)) }
        return CGSize(width: 1, height: 1)
    }

    private var fit: Double { PictureZoom.fit(content: tile, picture: full) ?? 1 }

    /// What is shown now: the kept zoom (or fit, about the middle) with the gesture in progress.
    private var shown: PictureZoom {
        var z = kept ?? PictureZoom(scale: fit, centre: CGPoint(x: 0.5, y: 0.5))
        z.scale = min(max(z.scale * Double(pinch), PictureZoom.minScale), PictureZoom.maxScale)
        z.centre.x = min(max(z.centre.x - drag.width / (full.width * z.scale), 0), 1)
        z.centre.y = min(max(z.centre.y - drag.height / (full.height * z.scale), 0), 1)
        return z
    }

    var body: some SwiftUI.View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Button("Fit") { kept = nil }.disabled(kept == nil).accessibilityIdentifier("pictures-fit")
                Button("1:1") { kept = PictureZoom(scale: 1, centre: shown.centre) }.accessibilityIdentifier("pictures-one")
                Text("\(Int((shown.scale * 100).rounded()))%").font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    .accessibilityIdentifier("pictures-zoom")
                Spacer()
            }
            .buttonStyle(.borderless)
            ScrollView {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 10)], spacing: 10) {
                    ForEach(Array(views.enumerated()), id: \.offset) { k, p in
                        VStack(spacing: 3) {
                            if let h = p.handle {
                                ZoomTile(box: box, handle: h, full: full, zoom: shown) { px in probe(px) }
                                    .frame(minHeight: 160)
                                    .background(GeometryReader { g in Color.clear.onAppear { tile = g.size }.onChange(of: g.size) { _, s in tile = s } })
                                    .accessibilityIdentifier("pictures-tile-\(k)")
                            }
                            Text(p.caption ?? "").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                .gesture(
                    MagnifyGesture().updating($pinch) { g, st, _ in st = g.magnification }
                        .onEnded { g in keep { $0.scale *= Double(g.magnification) } }
                        .simultaneously(with: DragGesture(minimumDistance: 6).updating($drag) { g, st, _ in st = g.translation }
                            .onEnded { g in keep { z in
                                z.centre.x -= g.translation.width / (full.width * z.scale)
                                z.centre.y -= g.translation.height / (full.height * z.scale)
                            } })
                )
            }
            if !reading.isEmpty {
                Text(reading).font(.caption.monospacedDigit()).lineLimit(1).accessibilityIdentifier("pictures-reading")
            }
        }
        .onAppear {
            take(value.zoomAsk)
            describe()
        }
        .onChange(of: kept) { _, _ in describe() }
        .onChange(of: tile) { _, _ in describe() }
        .onChange(of: value.zoomAsk) { _, a in take(a) }
    }

    /// A zoom asked of the pop-out, taken once (`zoomAsk`, B159): its scale and centre kept, or
    /// Fit and 1:1 forgetting what was kept.
    private func take(_ a: JSONValue?) {
        guard let a, let seq = a["seq"].uint64, seq > askSeen else { return }
        askSeen = seq
        #if os(macOS)
        let backing = Double(NSScreen.main?.backingScaleFactor ?? 2)
        #else
        let backing = Double(UIScreen.main.scale)
        #endif
        if a["to"].string == "1:1" {
            kept = PictureZoom(scale: 1, centre: shown.centre)
        } else {
            kept = PictureZoom.asked(a, current: kept ?? shown, backing: backing, picture: full)
        }
    }

    /// A gesture ended: what it made is kept.
    private func keep(_ change: (inout PictureZoom) -> Void) {
        var z = kept ?? PictureZoom(scale: fit, centre: CGPoint(x: 0.5, y: 0.5))
        change(&z)
        z.scale = min(max(z.scale, PictureZoom.minScale), PictureZoom.maxScale)
        z.centre = CGPoint(x: min(max(z.centre.x, 0), 1), y: min(max(z.centre.y, 0), 1))
        kept = z
    }

    private func describe() {
        box.describe["zoom"] = shown.facts(kept: kept != nil, fit: fit)
    }

    /// A tap at a value pixel: its numbers read by the core, and the probe pinned there.
    private func probe(_ px: (row: UInt64, col: UInt64)) {
        let p = box.popout
        Task {
            let r = await p.data("probe", ["row": .uint(px.row), "col": .uint(px.col)], timeout: .seconds(5))
            reading = "row \(px.row), col \(px.col)  " + (r["text"].string ?? r["unavailable"].string ?? r["error"].string ?? "")
            _ = try? await p.model.run("viewer.probe", ["popout": .uint(p.id), "row": .uint(px.row), "col": .uint(px.col)])
        }
    }
}

/// One picture of a value drawn at a shared zoom: `zoom.scale` points a value pixel, `zoom.centre`
/// in the middle of the tile; a tap gives the value pixel under it.
struct ZoomTile: SwiftUI.View {
    let box: ViewerBox
    let handle: String
    let full: CGSize
    let zoom: PictureZoom
    let tapped: ((row: UInt64, col: UInt64)) -> Void
    @State private var image: CGImage?

    var body: some SwiftUI.View {
        GeometryReader { g in
            let w = full.width * zoom.scale, h = full.height * zoom.scale
            let x = g.size.width / 2 - zoom.centre.x * w, y = g.size.height / 2 - zoom.centre.y * h
            ZStack(alignment: .topLeading) {
                if let image {
                    Image(decorative: image, scale: 1).resizable().interpolation(zoom.scale >= 2 ? .none : .medium)
                        .frame(width: w, height: h)
                        .offset(x: x, y: y)
                } else {
                    ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(width: g.size.width, height: g.size.height, alignment: .topLeading)
            .clipped()
            .contentShape(Rectangle())
            .onTapGesture { at in
                let c = (at.x - x) / w, r = (at.y - y) / h
                guard c >= 0, c < 1, r >= 0, r < 1 else { return }
                tapped((UInt64(r * full.height), UInt64(c * full.width)))
            }
        }
        .task(id: handle) {
            if let p = await box.pictures.load(handle, want: "rgba", px: nil) { image = p.image }
        }
    }
}

struct ItemsViewer: SwiftUI.View {
    let box: ViewerBox

    var body: some SwiftUI.View {
        let cells = box.value?.sheet?.array ?? []
        ScrollView {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 150), spacing: 10)], spacing: 12) {
                ForEach(Array(cells.enumerated()), id: \.offset) { i, c in
                    Button {
                        let p = box.popout
                        Task { await p.command("viewer.sheet", ["on": false, "index": .int(Int64(i))]) }
                    } label: {
                        VStack(spacing: 3) {
                            if let h = c["thumb"]["ref"]["handle"].string {
                                LentPicture(box: box, handle: h, want: "rgba", px: nil).frame(height: 120)
                            }
                            Text(c["label"].string ?? "item \(i + 1)").font(.caption).lineLimit(2)
                        }
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(12)
        }
        .onAppear { box.describe["items"] = .uint(UInt64(cells.count)) }
    }
}

// MARK: - Audio

/// Plays a lent sound through AVAudioEngine, a few seconds of buffers ahead (an hour of sound is
/// never copied whole), or, muted, runs the clock alone without opening the audio device.
@MainActor
final class SoundPlayer {
    private let data: Data
    let channels: Int
    let frames: Int
    let rate: Double
    private let muted: Bool
    private var engine: AVAudioEngine?
    private var node: AVAudioPlayerNode?
    private var format: AVAudioFormat?
    private var next = 0
    private var startFrame = 0
    private var startedAt: Date?
    private var generation = 0
    var level: Float = 1 { didSet { node?.volume = level } }
    var looped = false
    var onEnd: (() -> Void)?
    static let chunk = 1 << 16

    init(data: Data, channels: Int, frames: Int, rate: Double, muted: Bool) {
        self.data = data
        self.channels = max(1, channels)
        self.frames = frames
        self.rate = rate
        self.muted = muted
    }

    var playing: Bool { startedAt != nil }

    /// Where it is, in seconds.
    var position: Double {
        guard let t = startedAt else { return Double(startFrame) / rate }
        let p = Double(startFrame) / rate + Date().timeIntervalSince(t)
        let total = Double(frames) / rate
        return looped && total > 0 ? p.truncatingRemainder(dividingBy: total) : min(p, total)
    }

    func play(from seconds: Double) {
        stop()
        generation += 1
        startFrame = max(0, min(frames, Int(seconds * rate)))
        next = startFrame
        startedAt = Date()
        if muted { return }
        guard let fmt = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: rate, channels: AVAudioChannelCount(channels), interleaved: false) else { return }
        let e = AVAudioEngine()
        let n = AVAudioPlayerNode()
        e.attach(n)
        e.connect(n, to: e.mainMixerNode, format: fmt)
        n.volume = level
        format = fmt
        engine = e
        node = n
        do {
            try e.start()
        } catch {
            stop()
            return
        }
        for _ in 0..<3 { schedule(generation) }
        n.play()
    }

    private func schedule(_ gen: Int) {
        guard gen == generation, let n = node, let fmt = format else { return }
        if next >= frames {
            if looped { next = 0 } else { return }
        }
        let count = min(Self.chunk, frames - next)
        guard let buf = AVAudioPCMBuffer(pcmFormat: fmt, frameCapacity: AVAudioFrameCount(count)), let out = buf.floatChannelData else { return }
        buf.frameLength = AVAudioFrameCount(count)
        let ch = channels
        let from = next
        data.withUnsafeBytes { raw in
            let f = raw.bindMemory(to: Float32.self)
            for i in 0..<count {
                for c in 0..<ch {
                    let k = (from + i) * ch + c
                    let s = k < f.count ? f[k] : 0
                    out[c][i] = s.isFinite ? max(-1, min(1, s)) : 0
                }
            }
        }
        next += count
        let last = next >= frames && !looped
        n.scheduleBuffer(buf, completionCallbackType: .dataPlayedBack) { [weak self] _ in
            Task { @MainActor in
                guard let self, gen == self.generation else { return }
                if last {
                    self.ended()
                } else {
                    self.schedule(gen)
                }
            }
        }
    }

    /// Muted: the clock reaching the end does what the sound's end would.
    func tick() {
        guard muted, playing, !looped, position >= Double(frames) / rate else { return }
        ended()
    }

    private func ended() {
        stop()
        startFrame = 0
        onEnd?()
    }

    func stop() {
        if let t = startedAt { startFrame = min(frames, startFrame + Int(Date().timeIntervalSince(t) * rate)) }
        startedAt = nil
        generation += 1
        node?.stop()
        engine?.stop()
        node = nil
        engine = nil
    }
}

@MainActor
@Observable
final class AudioModel {
    var peaks: [[Float]] = []
    var channels = 1
    var spectrogram: CGImage?
    var error: String?
    var spectroError: String?
    var player: SoundPlayer?
    var position: Double = 0
    var seq: UInt64 = 0
    var handle: String?

    func load(_ box: ViewerBox) {
        guard let h = box.value?.handle, h != handle else { return }
        handle = h
        peaks = []
        spectrogram = nil
        player?.stop()
        player = nil
        let pics = box.pictures
        Task {
            // the waveform, from every sample, on the core's workers
            if let (note, data) = await pics.lend(h, want: "waveform", px: 1600) {
                let ch = Int(note.channels ?? 1), bins = Int(note.bins ?? 0)
                let lanes = await Task.detached { () -> [[Float]] in
                    data.withUnsafeBytes { raw in
                        let f = raw.bindMemory(to: Float32.self)
                        return (0..<ch).map { c in (0..<(bins * 2)).map { i in let k = c * bins * 2 + i; return k < f.count ? f[k] : 0 } }
                    }
                }.value
                channels = ch
                peaks = lanes
            } else {
                error = pics.failures[h] ?? "the waveform couldn't be made"
            }
        }
        Task {
            // the spectrogram, on the core's workers too (never this thread: B36 is the egui editor's)
            if let p = await pics.load(h, want: "spectrogram") { spectrogram = p.image } else { spectroError = pics.failures[h] }
        }
        Task {
            let form = box.value?.form
            let rate = form?["sample_rate"].double ?? 0
            if let (note, data) = await pics.lend(h, want: "bytes") {
                let ch = Int(note.shape?.last ?? 1), frames = Int(note.shape?.first ?? 0)
                guard rate > 0, frames > 0 else {
                    error = "this sound has no samples to play"
                    return
                }
                let p = SoundPlayer(data: data, channels: ch, frames: frames, rate: rate, muted: box.muted)
                let m = box.popout
                p.onEnd = { Task { await m.command("viewer.audio", ["play": false, "at": 0]) } }
                player = p
                follow(box)
            } else {
                error = pics.failures[h] ?? "the samples couldn't be read"
            }
        }
    }

    /// The core's state: playing or not, from where, the level and loop.
    func follow(_ box: ViewerBox) {
        guard let p = player else { return }
        let st = box.state("audio")
        p.level = Float(st["level"].double ?? 1)
        p.looped = st["loop"].bool ?? false
        let playing = st["playing"].bool ?? false
        let s = st["seq"].uint64 ?? 0
        if playing && (!p.playing || s != seq) {
            p.play(from: st["at"].double ?? 0)
        } else if !playing && p.playing {
            p.stop()
        }
        seq = s
        position = p.position
        box.describe["audio"] = ["playing": .bool(p.playing), "muted": .bool(box.muted), "at": .double(p.position), "seconds": .double(Double(p.frames) / p.rate),
                                 "peaks": .uint(UInt64(peaks.first?.count ?? 0) / 2), "spectrogram": .bool(spectrogram != nil)]
    }
}

struct AudioViewer: SwiftUI.View {
    let box: ViewerBox
    @State private var audio = AudioModel()
    @State private var level: Double = 1

    var body: some SwiftUI.View {
        let st = box.state("audio")
        let seconds = (box.value?.form?["frames"].double ?? 0) / max(1, box.value?.form?["sample_rate"].double ?? 1)
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                let playing = st["playing"].bool ?? false
                Button {
                    let p = box.popout
                    let at = audio.player?.position ?? (st["at"].double ?? 0)
                    Task { await p.command("viewer.audio", playing ? ["play": false, "at": .double(at)] : ["play": true]) }
                } label: {
                    Label(playing ? "Pause" : "Play", systemImage: playing ? "pause.fill" : "play.fill")
                }
                .keyboardShortcut(.space, modifiers: [])
                .accessibilityIdentifier("audio-play")
                Toggle("Loop", isOn: Binding(get: { st["loop"].bool ?? false }, set: { on in
                    let p = box.popout
                    Task { await p.command("viewer.audio", ["loop": .bool(on)]) }
                }))
                .fixedSize()
                TimelineView(.periodic(from: .now, by: 0.1)) { _ in
                    Text("\(Self.clock(audio.player?.position ?? (st["at"].double ?? 0))) / \(Self.clock(seconds))").font(.callout.monospacedDigit()).fixedSize()
                        .onAppear { audio.player?.tick() }
                }
                Spacer()
                Image(systemName: "speaker.wave.2").foregroundStyle(.secondary)
                Slider(value: $level, in: 0...1) { editing in
                    guard !editing else { return }
                    let p = box.popout
                    Task { await p.command("viewer.audio", ["level": .double(level)]) }
                }
                .frame(width: 120)
                .accessibilityIdentifier("audio-level")
                if box.muted { Text("muted (scripted run)").font(.caption).foregroundStyle(.secondary) }
            }
            .buttonStyle(.bordered)
            Waveform(audio: audio, seconds: seconds) { at in
                let p = box.popout
                Task { await p.command("viewer.audio", ["at": .double(at)]) }
            }
            .frame(minHeight: 110)
            .accessibilityIdentifier("audio-waveform")
            Text("Spectrogram").font(.caption).foregroundStyle(.secondary)
            Group {
                if let s = audio.spectrogram {
                    Image(decorative: s, scale: 1).resizable().interpolation(.medium)
                } else if let e = audio.spectroError {
                    Text(e).font(.caption).foregroundStyle(.secondary)
                } else {
                    ProgressView()
                }
            }
            .frame(maxWidth: .infinity, minHeight: 120, maxHeight: .infinity)
            .accessibilityIdentifier("audio-spectrogram")
            if let e = audio.error { Text(e).font(.caption).foregroundStyle(.red) }
        }
        .padding(12)
        .onAppear {
            level = st["level"].double ?? 1
            audio.load(box)
        }
        .onChange(of: box.value) { _, _ in
            audio.load(box)
            audio.follow(box)
        }
        .onDisappear { audio.player?.stop() }
    }

    static func clock(_ s: Double) -> String {
        let t = max(0, s.isFinite ? s : 0)
        return String(format: "%d:%04.1f", Int(t) / 60, t.truncatingRemainder(dividingBy: 60))
    }
}

/// The waveform, a lane a channel (at most eight), with the playhead; a tap or a drag seeks.
struct Waveform: SwiftUI.View {
    let audio: AudioModel
    let seconds: Double
    let seek: (Double) -> Void

    var body: some SwiftUI.View {
        GeometryReader { geo in
            TimelineView(.periodic(from: .now, by: 1.0 / 30)) { tl in
                // the playhead moves with the clock: the canvas is drawn again each tick
                let now = tl.date
                Canvas { ctx, size in
                    _ = now
                    let lanes = Array(audio.peaks.prefix(8))
                    guard !lanes.isEmpty else { return }
                    let h = size.height / CGFloat(lanes.count)
                    for (li, lane) in lanes.enumerated() {
                        let bins = lane.count / 2
                        guard bins > 0 else { continue }
                        let mid = h * (CGFloat(li) + 0.5)
                        var path = Path()
                        for b in 0..<bins {
                            let x = CGFloat(b) / CGFloat(bins) * size.width
                            let lo = CGFloat(max(-1, min(1, lane[b * 2]))), hi = CGFloat(max(-1, min(1, lane[b * 2 + 1])))
                            path.move(to: CGPoint(x: x, y: mid - hi * h / 2))
                            path.addLine(to: CGPoint(x: x, y: mid - lo * h / 2 + 0.5))
                        }
                        ctx.stroke(path, with: .color(.accentColor), lineWidth: max(1, size.width / CGFloat(bins)))
                    }
                    if seconds > 0, let p = audio.player {
                        let x = CGFloat(p.position / seconds) * size.width
                        ctx.fill(Path(CGRect(x: x - 0.75, y: 0, width: 1.5, height: size.height)), with: .color(.primary))
                    }
                }
            }
            .background(.quaternary.opacity(0.4), in: RoundedRectangle(cornerRadius: 6))
            .overlay {
                if audio.peaks.isEmpty { ProgressView() }
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onEnded { g in
                let w = geo.size.width
                guard seconds > 0, w > 0 else { return }
                seek(max(0, min(seconds, Double(g.location.x / w) * seconds)))
            })
        }
    }
}

// MARK: - The picture viewers' shared parts (each app draws the picture in its own zoomable view)

/// Where a pop-out's picture comes from, and what its pixels are of.
public struct PictureSource: Sendable, Equatable {
    public let handle: String
    /// `rgba` (the engine's rendition), `image` (the picture file decoded), `svg` (drawn).
    public let want: String
    public let px: Int?
    /// The size of the value itself (a picture file's, an array's), when larger than what is shown.
    public let full: (w: Int, h: Int)?

    public init(handle: String, want: String, px: Int?, full: (w: Int, h: Int)?) {
        self.handle = handle
        self.want = want
        self.px = px
        self.full = full
    }

    public static func == (a: PictureSource, b: PictureSource) -> Bool {
        a.handle == b.handle && a.want == b.want && a.px == b.px && a.full?.w == b.full?.w && a.full?.h == b.full?.h
    }

    /// The picture a value shows first: an SVG drawn at `px`, an array's one view or its
    /// rendition, a picture's rendition.
    public static func of(_ v: PopoutValue, svgPx: Int = 2048) -> PictureSource? {
        let full = fullSize(v)
        if v.viewer == "svg", let h = v.handle { return PictureSource(handle: h, want: "svg", px: svgPx, full: nil) }
        if v.viewer == "array.pictures", let p = v.views?.first, let h = p.handle { return PictureSource(handle: h, want: "rgba", px: nil, full: full) }
        // a picture file: decoded from its own bytes (up to 2048 a side; zooming in decodes more)
        if v.media == "image", let h = v.handle, let f = full { return PictureSource(handle: h, want: "image", px: min(2048, max(f.w, f.h)), full: full) }
        if let h = v.picture?.handle { return PictureSource(handle: h, want: "rgba", px: nil, full: full) }
        return nil
    }

    /// More detail than the picture shown has, for a zoom past it: a picture file decoded larger
    /// (at most 4096 a side), or an SVG drawn larger. Nil when there is none to be had here (an
    /// array asks the core, `viewer.zoom {to: "detail"}`).
    public func sharper(than shown: Int, v: PopoutValue) -> PictureSource? {
        if want == "svg" { return (px ?? 0) < 4096 ? PictureSource(handle: handle, want: "svg", px: 4096, full: nil) : nil }
        guard v.media == "image", let h = v.handle, let f = full, max(f.w, f.h) > shown else { return nil }
        return PictureSource(handle: h, want: "image", px: min(4096, max(f.w, f.h)), full: full)
    }

    /// The value's own size: a picture file's width and height, an array's (H, W, …).
    public static func fullSize(_ v: PopoutValue) -> (w: Int, h: Int)? {
        guard let f = v.form else { return nil }
        if let w = f["width"].uint64, let h = f["height"].uint64, w > 0, h > 0 { return (Int(w), Int(h)) }
        if let s = f["shape"].array, s.count >= 2, let h = s[0].uint64, let w = s[1].uint64, w > 0, h > 0 { return (Int(w), Int(h)) }
        return nil
    }

    /// The value's pixel (row, column) under a point of the shown picture (`x`, `y` in its pixels,
    /// top-left origin), by its fraction of the picture, as the core's probe maps it.
    public static func pixel(x: Double, y: Double, shown: (w: Int, h: Int), full: (w: Int, h: Int)?) -> (row: UInt64, col: UInt64)? {
        let f = full ?? shown
        guard shown.w > 0, shown.h > 0, x >= 0, y >= 0, x < Double(shown.w), y < Double(shown.h) else { return nil }
        let col = min(f.w - 1, Int(x / Double(shown.w) * Double(f.w)))
        let row = min(f.h - 1, Int(y / Double(shown.h) * Double(f.h)))
        return (UInt64(max(0, row)), UInt64(max(0, col)))
    }

    /// A picture's one channel as grey, or its colour without alpha (`viewer.picture`'s channel),
    /// made off the main thread. `rgba` gives it back as it is.
    public nonisolated static func isolate(_ image: CGImage, channel: String) -> CGImage? {
        if channel == "rgba" { return image }
        let w = image.width, h = image.height
        guard w > 0, h > 0, let space = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue), let base = ctx.data
        else { return nil }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        let px = base.bindMemory(to: UInt8.self, capacity: w * h * 4)
        for i in 0..<(w * h) {
            let r = px[i * 4], g = px[i * 4 + 1], b = px[i * 4 + 2], a = px[i * 4 + 3]
            // premultiplied: straight values back for a channel
            let un = { (c: UInt8) -> UInt8 in a == 0 ? 0 : UInt8(min(255, Int(c) * 255 / Int(a))) }
            let v: UInt8
            switch channel {
            case "r": v = un(r)
            case "g": v = un(g)
            case "b": v = un(b)
            case "a": v = a
            default:
                // rgb: opaque colour
                px[i * 4] = un(r); px[i * 4 + 1] = un(g); px[i * 4 + 2] = un(b); px[i * 4 + 3] = 255
                continue
            }
            px[i * 4] = v; px[i * 4 + 1] = v; px[i * 4 + 2] = v; px[i * 4 + 3] = 255
        }
        return ctx.makeImage()
    }

    /// The channels a viewer offers: `(id, label)`.
    public static let channels: [(String, String)] = [("rgba", "All Channels"), ("rgb", "Colour"), ("r", "Red"), ("g", "Green"), ("b", "Blue"), ("a", "Alpha")]
}

/// A pop-out picture's state between its controls (the source, its size shown, the picture as
/// decoded, whether detail was asked for).
@MainActor
public final class PicState {
    nonisolated(unsafe) public static var key: UInt8 = 0
    public var src: PictureSource
    public var shown: (w: Int, h: Int)
    public var original: CGImage?
    public var sharpened = false
    public var lastAsk = Date.distantPast
    public var set: ((CGImage) -> Void)?
    public init(src: PictureSource, shown: (Int, Int)) {
        self.src = src
        self.shown = shown
    }
}


// MARK: - The Messages window (N4 slice 5)

/// The status bar's messages in full (`messages.get`), read again while the window is open.
@MainActor
@Observable
public final class MessagesModel {
    @ObservationIgnored public let model: DocumentModel
    public var messages: [JSONValue] = []
    @ObservationIgnored private var task: Task<Void, Never>?

    public init(model: DocumentModel) { self.model = model }

    /// Read them now and every second until `stop`.
    public func start() {
        guard task == nil else { return }
        task = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                if let r = try? await self.model.core.request("messages.get", [:], doc: self.model.requestDoc) {
                    let list = r["messages"].array ?? []
                    if list != self.messages { self.messages = list }
                }
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    public func stop() {
        task?.cancel()
        task = nil
    }

    /// The person closed it: the core's window closes too.
    public func close() async {
        _ = try? await model.run("view.messages", ["open": false])
    }
}

public struct MessagesView: SwiftUI.View {
    @Bindable var messages: MessagesModel
    let done: (() -> Void)?

    public init(messages: MessagesModel, done: (() -> Void)? = nil) {
        self.messages = messages
        self.done = done
    }

    public var body: some SwiftUI.View {
        NavigationStack {
            Group {
                if messages.messages.isEmpty {
                    ContentUnavailableView("No Messages", systemImage: "checkmark.circle", description: Text("Errors, a save's outcome and what went wrong show here."))
                } else {
                    List(Array(messages.messages.enumerated()), id: \.offset) { _, m in
                        let tone = m["tone"].string ?? "info"
                        Label {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(m["text"].string ?? "").textSelection(.enabled)
                                if let d = m["detail"].string, !d.isEmpty { Text(d).font(.caption).foregroundStyle(.secondary).textSelection(.enabled) }
                            }
                        } icon: {
                            Image(systemName: tone == "error" ? "xmark.octagon.fill" : tone == "warn" ? "exclamationmark.triangle.fill" : "info.circle")
                                .foregroundStyle(tone == "error" ? Color.red : tone == "warn" ? Color.orange : Color.secondary)
                        }
                    }
                    .accessibilityIdentifier("messages-list")
                }
            }
            .navigationTitle("Messages")
            .toolbar {
                if let done { ToolbarItem(placement: .confirmationAction) { Button("Done", action: done).accessibilityIdentifier("messages-done") } }
            }
        }
        .onAppear { messages.start() }
        .onDisappear { messages.stop() }
    }
}

/// How a table follows its value's selection (note 26 slice 8): a selection that arrives, from this
/// window or another, is shown (the page holding its first row asked of the core, then that row
/// scrolled to the top) unless the person has scrolled this table themselves.
@MainActor
final class SelectionFollow {
    var personScrolled = false
    private var seen: String?

    func go(_ page: JSONValue, box: ViewerBox, scroll: (Int) -> Void) {
        // a new selection, or Selected Only turned on or off: the rows move, the selection is shown again
        guard let n = page["selectionSeq"].uint64 else { return }
        let seq = "\(n)|\(page["selectedOnly"].bool == true)"
        guard seq != seen else { return }
        let first = page["firstSelected"]
        guard let at = first["at"].uint64, !personScrolled else {
            seen = seq
            return
        }
        if let want = first["page"].uint64, want != (page["page"].uint64 ?? 0) {
            // another page holds it: ask for that one; this runs again when it arrives
            let p = box.popout
            Task { await p.command("viewer.table", ["page": .uint(want)]) }
            return
        }
        seen = seq
        let start = page["start"].uint64 ?? 0
        scroll(Int(start + (at - start)))
        box.describe["followed"] = .uint(at)
    }
}

/// Show only the selected rows, or all: on and off plainly different (a filled check, a prominent
/// button, when on).
struct SelectedOnlyButton: SwiftUI.View {
    let box: ViewerBox
    let on: Bool

    var body: some SwiftUI.View {
        // its own colours, not the control's tint (an inactive window greys a prominent button)
        Button { set(!on) } label: {
            Label("Selected Only", systemImage: on ? "checkmark.circle.fill" : "circle")
                .foregroundStyle(on ? Color.white : Color.primary)
                .padding(.horizontal, 8).padding(.vertical, 3)
                .background(Capsule().fill(on ? Color.orange : Color.secondary.opacity(0.12)))
                .overlay(Capsule().strokeBorder(on ? Color.orange : Color.secondary.opacity(0.4), lineWidth: 1))
        }
        .buttonStyle(.plain)
        .controlSize(.small)
        .font(.caption)
        .accessibilityIdentifier("table-selected-only")
        .accessibilityValue(on ? "on" : "off")
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    private func set(_ v: Bool) {
        let p = box.popout
        Task { await p.command("viewer.table", ["selectedOnly": .bool(v)]) }
    }
}
