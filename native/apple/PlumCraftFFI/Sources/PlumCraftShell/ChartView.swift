// A chart the core lays out, painted here (note 26 slice 5; the maintainer's decision 1A: "the core
// lays charts out, each shell paints them"). The core reads the value and sends a display list
// (`viewer.data {part: "chart", width, height}`): the plot rectangle, marks in the chart's own
// points, the axes' ticks with their text and titles, grid lines and an accessibility outline. This
// view asks for it at its own size and draws it with SwiftUI's Canvas; it computes no scale, tick
// or number. The accessibility outline becomes the chart's label and value, and an
// `AXChartDescriptor` (Audio Graphs) from the core's ranges.

import Foundation
import PlumCraftKit
import PlumCraftModels
import SwiftUI

/// The display list's parts as numbers, read leniently (anything not a number is skipped).
public enum ChartList {
    public static func numbers(_ v: JSONValue) -> [Double] { (v.array ?? []).compactMap(\.double) }

    /// Points as (x, y) pairs from a flat list.
    public static func pairs(_ v: JSONValue) -> [CGPoint] {
        let n = numbers(v)
        return stride(from: 0, to: n.count - 1, by: 2).map { CGPoint(x: n[$0], y: n[$0 + 1]) }
    }

    /// Rects from a flat list of x, y, w, h.
    public static func rects(_ v: JSONValue) -> [CGRect] {
        let n = numbers(v)
        return stride(from: 0, to: n.count - 3, by: 4).map { CGRect(x: n[$0], y: n[$0 + 1], width: n[$0 + 2], height: n[$0 + 3]) }
    }

    /// A mark's colour (`#rrggbb` from the core's palette), else the accent.
    public static func colour(_ v: JSONValue) -> Color {
        guard let s = v.string, s.hasPrefix("#"), s.count == 7, let n = UInt32(s.dropFirst(), radix: 16) else { return .accentColor }
        return Color(red: Double((n >> 16) & 0xff) / 255, green: Double((n >> 8) & 0xff) / 255, blue: Double(n & 0xff) / 255)
    }

    /// The plot rectangle.
    public static func plot(_ list: JSONValue) -> CGRect {
        let p = numbers(list["plot"])
        return p.count == 4 ? CGRect(x: p[0], y: p[1], width: p[2], height: p[3]) : .zero
    }
}

/// The Plot viewer: the core's chart at this view's size.
struct ChartViewer: SwiftUI.View {
    let box: ViewerBox
    @State private var loader = PageLoader()
    @State private var size: CGSize = .zero

    var body: some SwiftUI.View {
        GeometryReader { g in
            let want = CGSize(width: (g.size.width).rounded(), height: (g.size.height).rounded())
            // `box.stamp`, observed, moves with the core's stamp: a run, or a chart chosen anywhere (an
            // undo included) draws again, though the value itself is equal
            let key: JSONValue = ["stamp": .uint(box.stamp), "viewer": box.value?.viewer.map(JSONValue.string) ?? .null, "w": .double(want.width), "h": .double(want.height), "r": .int(Int64(box.reload))]
            Group {
                if let e = loader.error {
                    ViewerMessage(text: "This chart can't be drawn: \(e)")
                } else if let list = loader.page {
                    VStack(spacing: 4) {
                        if list["columns"].array != nil { ChartChoices(box: box, list: list) }
                        ChartCanvas(list: list)
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel(list["a11y"]["title"].string ?? "Chart")
                        .accessibilityValue(list["a11y"]["summary"].string ?? "")
                        .accessibilityChartDescriptor(ChartDescription(list: list))
                        .accessibilityIdentifier("viewer-chart")
                    }
                } else {
                    ProgressView("Laying out the chart…").frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .onAppear { load(key, want) }
            .onChange(of: key) { _, k in load(k, want) }
        }
        .padding(8)
    }

    private func load(_ key: JSONValue, _ want: CGSize) {
        guard want.width > 1, want.height > 1 else { return }
        // a table's choices take a row above the chart
        let h = box.value?.viewer == "table.chart" ? max(want.height - 34, 120) : want.height
        loader.load(box, "chart", key: key, extra: ["width": .double(want.width), "height": .double(h)])
        box.describe["chartSize"] = [.double(want.width), .double(h)]
    }
}

/// The display list drawn: grid, marks, axes and their text, in the list's own points.
public struct ChartCanvas: SwiftUI.View {
    let list: JSONValue
    public init(list: JSONValue) { self.list = list }

    public var body: some SwiftUI.View {
        Canvas { cx, _ in
            let plot = ChartList.plot(list)
            let font = list["font"].double ?? 11
            // grid
            var grid = Path()
            for x in ChartList.numbers(list["grid"]["x"]) {
                grid.move(to: CGPoint(x: x, y: plot.minY))
                grid.addLine(to: CGPoint(x: x, y: plot.maxY))
            }
            for y in ChartList.numbers(list["grid"]["y"]) {
                grid.move(to: CGPoint(x: plot.minX, y: y))
                grid.addLine(to: CGPoint(x: plot.maxX, y: y))
            }
            cx.stroke(grid, with: .color(.secondary.opacity(0.18)), lineWidth: 0.5)
            // marks
            for m in list["marks"].array ?? [] {
                switch m["kind"].string {
                case "points":
                    let r = m["r"].double ?? 2
                    var p = Path()
                    for c in ChartList.pairs(m["xy"]) { p.addEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r)) }
                    cx.fill(p, with: .color(ChartList.colour(m["color"])))
                case "segments":
                    let pts = ChartList.pairs(m["xy"])
                    var p = Path()
                    for i in stride(from: 0, to: pts.count - 1, by: 2) {
                        p.move(to: pts[i])
                        p.addLine(to: pts[i + 1])
                    }
                    cx.stroke(p, with: .color(ChartList.colour(m["color"])), lineWidth: m["width"].double ?? 1)
                case "rects":
                    var p = Path()
                    for r in ChartList.rects(m["rects"]) { p.addRect(r) }
                    cx.fill(p, with: .color(ChartList.colour(m["color"]).opacity(0.85)))
                case "line":
                    let pts = ChartList.pairs(m["xy"])
                    guard let first = pts.first else { break }
                    var p = Path()
                    p.move(to: first)
                    for q in pts.dropFirst() { p.addLine(to: q) }
                    cx.stroke(p, with: .color(ChartList.colour(m["color"])), lineWidth: m["width"].double ?? 1.5)
                case "area":
                    // a stacked band: its top left to right, then its base back
                    let pts = ChartList.pairs(m["xy"])
                    guard let first = pts.first else { break }
                    var p = Path()
                    p.move(to: first)
                    for q in pts.dropFirst() { p.addLine(to: q) }
                    p.closeSubpath()
                    cx.fill(p, with: .color(ChartList.colour(m["color"]).opacity(0.8)))
                case "labels":
                    for l in m["labels"].array ?? [] {
                        guard let x = l["x"].double, let y = l["y"].double else { continue }
                        cx.draw(Text(l["text"].string ?? "").font(.system(size: font).weight(.medium)).foregroundStyle(.white), at: CGPoint(x: x, y: y), anchor: .topLeading)
                        if let d = l["detail"].string {
                            cx.draw(Text(d).font(.system(size: font).monospacedDigit()).foregroundStyle(.white.opacity(0.8)), at: CGPoint(x: x, y: y + font + 2), anchor: .topLeading)
                        }
                    }
                default:
                    break
                }
            }
            // the legend: a swatch and a name a series
            for l in list["legend"].array ?? [] {
                guard let x = l["x"].double, let y = l["y"].double else { continue }
                cx.fill(Path(roundedRect: CGRect(x: x, y: y, width: 10, height: 10), cornerRadius: 2), with: .color(ChartList.colour(l["color"])))
                cx.draw(Text(l["text"].string ?? "").font(.system(size: font)).foregroundStyle(.primary), at: CGPoint(x: x + 15, y: y + 5), anchor: .leading)
            }
            // axes (a treemap has none)
            guard !(list["axes"].array ?? []).isEmpty else { return }
            var axes = Path()
            axes.move(to: CGPoint(x: plot.minX, y: plot.minY))
            axes.addLine(to: CGPoint(x: plot.minX, y: plot.maxY))
            axes.addLine(to: CGPoint(x: plot.maxX, y: plot.maxY))
            cx.stroke(axes, with: .color(.secondary.opacity(0.6)), lineWidth: 1)
            for a in list["axes"].array ?? [] {
                let bottom = a["side"].string == "bottom"
                for t in a["ticks"].array ?? [] {
                    guard let at = t["at"].double else { continue }
                    let text = Text(t["text"].string ?? "").font(.system(size: font).monospacedDigit()).foregroundStyle(.secondary)
                    if bottom {
                        cx.draw(text, at: CGPoint(x: at, y: plot.maxY + 4), anchor: .top)
                    } else {
                        cx.draw(text, at: CGPoint(x: plot.minX - 5, y: at), anchor: .trailing)
                    }
                }
                let title = Text(a["title"].string ?? "").font(.system(size: font).weight(.medium)).foregroundStyle(.primary)
                if bottom {
                    cx.draw(title, at: CGPoint(x: plot.midX, y: plot.maxY + font + 10), anchor: .top)
                } else {
                    var c = cx
                    c.translateBy(x: 4, y: plot.midY)
                    c.rotate(by: .degrees(-90))
                    c.draw(title, at: .zero, anchor: .top)
                }
            }
        }
    }
}

/// The chart for VoiceOver and Audio Graphs, from the core's outline and marks.
struct ChartDescription: AXChartDescriptorRepresentable {
    let list: JSONValue

    func makeChartDescriptor() -> AXChartDescriptor {
        let a = list["a11y"]
        let num = { (s: JSONValue) in Double(s.string ?? "") ?? 0 }
        let x = AXNumericDataAxisDescriptor(title: a["x"]["label"].string ?? "x", range: num(a["x"]["min"])...max(num(a["x"]["max"]), num(a["x"]["min"])), gridlinePositions: []) { "\($0)" }
        let y = AXNumericDataAxisDescriptor(title: a["y"]["label"].string ?? "y", range: num(a["y"]["min"])...max(num(a["y"]["max"]), num(a["y"]["min"])), gridlinePositions: []) { "\($0)" }
        return AXChartDescriptor(title: a["title"].string, summary: a["summary"].string, xAxis: x, yAxis: y, additionalAxes: [], series: [])
    }
}

/// A table chart's choices, each a core command (`viewer.chart`, kept in the graph's meta): the
/// kind, and which columns are x, y, colour, and a treemap's groups and size. The columns and the
/// spec drawn are the core's, from the chart's reply.
struct ChartChoices: SwiftUI.View {
    let box: ViewerBox
    let list: JSONValue

    private static let kinds: [(String, String, Bool)] = [
        ("Column", "column", false), ("Stacked Column", "column", true), ("Line", "line", false),
        ("Stacked Line", "line", true), ("Histogram", "histogram", false), ("Treemap", "treemap", false),
    ]

    var body: some SwiftUI.View {
        let spec = list["spec"]
        let mark = spec["mark"].string ?? "column"
        let stack = spec["stack"].bool ?? false
        let cols: [(String, Bool)] = (list["columns"].array ?? []).compactMap { c in
            guard let n = c["name"].string else { return nil }
            return (n, c["numeric"].bool ?? false)
        }
        let numbers = cols.filter(\.1).map(\.0)
        let all = cols.map(\.0)
        HStack(spacing: 10) {
            Menu {
                ForEach(Self.kinds, id: \.0) { k in
                    Button { send(["mark": .string(k.1), "stack": .bool(k.2)]) } label: {
                        if k.1 == mark && k.2 == stack { Label(k.0, systemImage: "checkmark") } else { Text(k.0) }
                    }
                }
            } label: { Text(Self.kinds.first { $0.1 == mark && $0.2 == stack }?.0 ?? mark.capitalized).font(.caption) }
                .fixedSize().accessibilityIdentifier("chart-kind").help("The kind of chart (viewer.chart mark, stack)")
            if mark == "treemap" {
                pick("Groups", spec["path"].array?.first?.string, all, nil) { send(["path": $0.map { .array([.string($0)]) } ?? .null]) }
                pick("Size", spec["size"].string, numbers, "Rows") { send(["size": $0.map(JSONValue.string) ?? .null]) }
            } else {
                pick("X", spec["x"].string, mark == "histogram" || mark == "line" ? numbers : all, mark == "line" ? "Row" : nil) { send(["x": $0.map(JSONValue.string) ?? .null]) }
                if mark != "histogram" {
                    pick("Y", spec["y"].array?.first?.string, numbers, mark == "column" ? "Count" : nil) { send(["y": $0.map { .array([.string($0)]) } ?? .null]) }
                }
                pick("Colour", spec["color"].string, all, "None") { send(["color": $0.map(JSONValue.string) ?? .null]) }
            }
            Spacer()
        }
        .padding(.horizontal, 4)
        .frame(height: 26)
    }

    /// A menu of columns for one role; `none`: the choice of no column, named so.
    private func pick(_ role: String, _ shown: String?, _ names: [String], _ none: String?, _ set: @escaping (String?) -> Void) -> some SwiftUI.View {
        Menu {
            if let none { Button { set(nil) } label: { if shown == nil { Label(none, systemImage: "checkmark") } else { Text(none) } } }
            ForEach(names, id: \.self) { n in
                Button { set(n) } label: { if shown == n { Label(n, systemImage: "checkmark") } else { Text(n) } }
            }
        } label: { Text("\(role): \(shown ?? none ?? "–")").font(.caption) }
            .fixedSize()
            .accessibilityIdentifier("chart-\(role.lowercased())")
            .help("The column for \(role.lowercased()) (viewer.chart)")
    }

    /// One choice, sent with the rest of the spec drawn (the core's own pick when none was made, so a
    /// first choice changes only what it names).
    private func send(_ params: [String: JSONValue]) {
        let p = box.popout
        let b = box
        var all = list["spec"].object ?? [:]
        for (k, v) in params { all[k] = v }
        Task { @MainActor in
            _ = await p.command("viewer.chart", all)
            b.reload += 1
        }
    }
}
