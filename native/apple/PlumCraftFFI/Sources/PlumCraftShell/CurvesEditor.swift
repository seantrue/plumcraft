// The shell's value editors (docs/shell-interface.md 12.21): a native view by the kind a node's
// engine names (`NodeDef.editor.kind`), opened by `node.editor` like the core's value window; a
// kind with no native view gets that window (the value as JSON). The shell ships `curves`.
//
// The curves editor draws with the Swift port of the curves maths (PlumCraftModels' `Curves`,
// pinned to the Rust by the shared vectors) and edits only through core commands: a gesture is
// the view's until the finger or pointer lifts, then one `curves.edit` (one undo step).

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import SwiftUI

/// The editor kinds the shell draws natively.
public enum NativeEditors {
    public static let kinds: Set<String> = ["curves"]
}

/// A value editor's window: the native editor for the node's kind, else the core's value window.
public struct ValueEditorView: SwiftUI.View {
    @Bindable var report: ReportModel
    let pictures: Pictures?
    let done: (() -> Void)?
    @State private var kind: String?
    @State private var asked = false

    public init(report: ReportModel, pictures: Pictures?, done: (() -> Void)? = nil) {
        self.report = report
        self.pictures = pictures
        self.done = done
    }

    var node: String { String(report.key.dropFirst("editor:".count)) }

    public var body: some SwiftUI.View {
        Group {
            if let kind, NativeEditors.kinds.contains(kind) {
                CurvesEditorView(model: report.curves ?? CurvesModel(report: report), pictures: pictures, done: done)
            } else if asked {
                ReportView(report: report, done: done)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task(id: report.key) {
            let r = try? await report.model.run("node.editorData", ["node": .string(node)])
            let k = r?["kind"].string
            if k == "curves", report.curves == nil { report.curves = CurvesModel(report: report) }
            kind = k
            asked = true
        }
    }
}

/// A curves editor's state: the value as the core holds it (or as a drag leaves it), what the
/// node's last run sent the editor, and the tab shown.
@MainActor @Observable
public final class CurvesModel {
    @ObservationIgnored public let report: ReportModel
    public var node: String { String(report.key.dropFirst("editor:".count)) }
    /// The four curves (master, red, green, blue).
    public var curveSet: [CurvesCurve] = Array(repeating: .identity, count: 4)
    /// 0 is All (the master curve); 1 to 3 a channel's own.
    public var tab = 0
    public var names: [String] = []
    public var histograms: [[Double]] = []
    public var live = false
    public var picture: String?
    public var error: String?
    public var wired = false
    public var loaded = false
    /// A drag under way: the point dragged, and whether it is outside enough to go.
    public var dragging: Int?
    public var removing = false
    /// The commands sent (for scripts).
    public var sent = 0
    @ObservationIgnored private var busy = false
    @ObservationIgnored private var again = false
    @ObservationIgnored private var before: CurvesCurve?

    public init(report: ReportModel) {
        self.report = report
    }

    /// The tabs: All, then the channels the data names (red, green and blue without data).
    public var tabs: [String] {
        let ch = names.isEmpty ? ["Red", "Green", "Blue"] : (names.count == 1 ? [] : names.prefix(3).map { $0.capitalized })
        return ["All"] + ch
    }

    public var curve: CurvesCurve { curveSet[min(tab, 3)] }

    func edited(_ k: Int) -> Bool { k < curveSet.count && (!curveSet[k].isIdentity || !curveSet[k].enabled) }

    public func reload() {
        if busy {
            again = true
            return
        }
        busy = true
        Task {
            if let r = try? await report.model.run("node.editorData", ["node": .string(node)]) {
                // a drag in hand keeps its points: the core's come after it lifts
                if dragging == nil { curveSet = Curves.parse(r["value"]) }
                wired = r["wired"].bool ?? false
                let d = r["data"]
                names = d["names"].array?.compactMap(\.string) ?? []
                histograms = (d["histograms"].array ?? []).map { ($0.array ?? []).map { $0.double ?? 0 } }
                live = d["live"].bool ?? false
                picture = Pictures.handle(of: d["picture"]["ref"])
                error = d["error"].string
                if tab >= tabs.count { tab = 0 }
                loaded = true
            }
            busy = false
            if again {
                again = false
                reload()
            }
        }
    }

    // MARK: Edits (each one command, one undo step)

    func send(_ command: String, _ extra: [String: JSONValue] = [:]) {
        var p: [String: JSONValue] = ["node": .string(node), "channel": .int(Int64(min(tab, 3)))]
        for (k, v) in extra { p[k] = v }
        sent += 1
        Task {
            do {
                _ = try await report.model.run(command, .object(p))
                error = nil
            } catch {
                self.error = "\(error)"
            }
            reload()
        }
    }

    /// Send the tab's curve as it is now (a gesture's end).
    func commit() {
        let c = curve
        send("curves.edit", ["points": .array(c.points.map { .array([.double($0.x), .double($0.y)]) }), "enabled": .bool(c.enabled)])
    }

    public func setEnabled(_ on: Bool) {
        curveSet[tab].enabled = on
        commit()
    }

    public func reset() { send("curves.reset") }
    public func preset(_ name: String) { send("curves.preset", ["preset": .string(name)]) }
    public func equalize() { send("curves.equalize") }

    // MARK: Gestures (unit coordinates, y up)

    /// A press at (x, y): the point it takes (one added on empty space), or nil.
    public func press(_ x: Double, _ y: Double, reach: Double) -> Int? {
        before = curve
        if let i = Curves.hit(curve, x, y, reach: reach) {
            dragging = i
        } else if let i = Curves.insert(&curveSet[tab], x, y) {
            dragging = i
        }
        removing = false
        return dragging
    }

    /// The finger or pointer at (x, y); `outside` when it is well off the plot.
    public func drag(_ x: Double, _ y: Double, outside: Bool) {
        guard let i = dragging else { return }
        removing = outside && curve.points.count > 2
        if !removing { Curves.move(&curveSet[tab], i, x, y) }
    }

    /// Lift: one `curves.edit` when the curve changed.
    public func lift() {
        guard let i = dragging else { return }
        if removing { Curves.remove(&curveSet[tab], i) }
        dragging = nil
        removing = false
        if curve != before { commit() }
        before = nil
    }

    public func remove(at i: Int) {
        if Curves.remove(&curveSet[tab], i) { commit() }
    }

    /// Move point `i`'s y by `dy` (accessibility's adjust): one `curves.edit`.
    public func nudge(_ i: Int, dy: Double) {
        guard i < curve.points.count else { return }
        let p = curve.points[i]
        Curves.move(&curveSet[tab], i, p.x, p.y + dy)
        commit()
    }

    /// For scripts (`--status-file`).
    public var describe: JSONValue {
        ["kind": "curves", "node": .string(node), "tab": .int(Int64(tab)), "tabs": .array(tabs.map { .string($0) }),
         "points": .array(curve.points.map { .array([.double($0.x), .double($0.y)]) }), "enabled": .bool(curve.enabled),
         "histogram": .bool(!Curves.tabHistogram(histograms, tab).isEmpty), "picture": .bool(picture != nil), "live": .bool(live),
         "dragging": .bool(dragging != nil), "sent": .uint(UInt64(sent)), "error": error.map(JSONValue.string) ?? .null]
    }
}

/// The curves editor: tabs, On, Reset, Presets and Equalize; the plot; the preview.
public struct CurvesEditorView: SwiftUI.View {
    @Bindable var model: CurvesModel
    let pictures: Pictures?
    let done: (() -> Void)?

    public var body: some SwiftUI.View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    header
                    CurvesPlot(model: model)
                        .aspectRatio(1, contentMode: .fit)
                        .frame(maxWidth: 420)
                        .frame(maxWidth: .infinity)
                    if model.wired {
                        Text("A wire feeds this value: edit it where it comes from.").font(.footnote).foregroundStyle(.secondary)
                    }
                    if let e = model.error { Text(e).font(.footnote).foregroundStyle(.red).accessibilityIdentifier("curves-error") }
                    if model.live, let handle = model.picture, let pictures {
                        CurvesPreview(handle: handle, set: model.curveSet, single: model.names.count == 1, pictures: pictures)
                            .frame(maxWidth: 420, maxHeight: 320)
                            .frame(maxWidth: .infinity)
                    } else if model.loaded, model.histograms.isEmpty {
                        Text("Run the graph to see the values' histogram and a preview.").font(.footnote).foregroundStyle(.secondary)
                    }
                }
                .padding()
            }
            .navigationTitle("Curves")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                if let done { ToolbarItem(placement: .confirmationAction) { Button("Done", action: done).accessibilityIdentifier("report-done") } }
            }
        }
        .accessibilityIdentifier("curves-editor")
        .onAppear { model.reload() }
        .onChange(of: model.report.followed) { model.reload() }
    }

    private var header: some SwiftUI.View {
        VStack(alignment: .leading, spacing: 8) {
            Picker("Channel", selection: $model.tab) {
                ForEach(Array(model.tabs.enumerated()), id: \.offset) { k, name in
                    Text(model.edited(k) ? "\(name) •" : name).tag(k)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier("curves-tabs")
            HStack(spacing: 12) {
                Toggle("On", isOn: Binding(get: { model.curve.enabled }, set: { model.setEnabled($0) }))
                    .fixedSize()
                    .accessibilityIdentifier("curves-on")
                Spacer()
                Menu("Presets") {
                    ForEach(Curves.presets, id: \.name) { p in
                        Button(p.name) { model.preset(p.name) }.help(p.help)
                    }
                }
                .fixedSize()
                .accessibilityIdentifier("curves-presets")
                Button("Equalize") { model.equalize() }
                    .disabled(model.histograms.isEmpty)
                    .help("Points at the quantiles of the channel's values")
                    .accessibilityIdentifier("curves-equalize")
                Button("Reset") { model.reset() }
                    .disabled(model.curve.isIdentity)
                    .accessibilityIdentifier("curves-reset")
            }
            .disabled(model.wired)
        }
    }
}

/// The plot: the tab's histogram behind, the other curves as ghosts, the tab's curve and its
/// points. Press adds, drag moves between neighbours, drag well outside or double tap removes.
struct CurvesPlot: SwiftUI.View {
    @Bindable var model: CurvesModel
    #if os(iOS)
    /// A finger's reach, in points.
    static let grab: Double = 22
    #else
    static let grab: Double = 10
    #endif
    /// How far past the plot a drag removes its point, in points.
    static let removeBeyond: Double = 24
    /// The plot's margin inside its square, so the end points show whole.
    static let inset: CGFloat = 10

    var body: some SwiftUI.View {
        GeometryReader { geo in
            let size = geo.size
            ZStack {
                Canvas { ctx, size in draw(&ctx, size) }
                ForEach(Array(model.curve.points.enumerated()), id: \.offset) { i, p in
                    Color.clear
                        .frame(width: Self.grab * 2, height: Self.grab * 2)
                        .contentShape(Rectangle())
                        .position(view(p, size))
                        .allowsHitTesting(false)
                        .accessibilityElement()
                        .accessibilityLabel("Point \(i + 1)")
                        .accessibilityValue(String(format: "input %.2f, output %.2f", p.x, p.y))
                        .accessibilityIdentifier("curves-point-\(i)")
                        .accessibilityAdjustableAction { dir in
                            model.nudge(i, dy: dir == .increment ? 0.01 : -0.01)
                        }
                        .accessibilityAction(named: "Remove") { model.remove(at: i) }
                }
            }
            .contentShape(Rectangle())
            .gesture(drag(size))
            .simultaneousGesture(SpatialTapGesture(count: 2).onEnded { e in
                let (x, y) = unit(e.location, size)
                if let i = Curves.hit(model.curve, x, y, reach: Self.grab / max(area(size).width, 1)) { model.remove(at: i) }
            })
        }
        .background(Color.gray.opacity(0.08))
        .clipShape(RoundedRectangle(cornerRadius: 6))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("curves-plot")
        .disabled(model.wired)
    }

    /// The plot's own square: the view less the margin.
    func area(_ size: CGSize) -> CGRect { CGRect(origin: .zero, size: size).insetBy(dx: Self.inset, dy: Self.inset) }

    /// A view point in unit coordinates (y up).
    func unit(_ p: CGPoint, _ size: CGSize) -> (Double, Double) {
        let a = area(size)
        return (Double((p.x - a.minX) / max(a.width, 1)), Double(1 - (p.y - a.minY) / max(a.height, 1)))
    }

    /// A unit point in the view.
    func view(_ p: CurvesPoint, _ size: CGSize) -> CGPoint {
        let a = area(size)
        return CGPoint(x: a.minX + p.x * a.width, y: a.minY + (1 - p.y) * a.height)
    }

    func drag(_ size: CGSize) -> some SwiftUI.Gesture {
        DragGesture(minimumDistance: 0)
            .onChanged { v in
                let (x, y) = unit(v.location, size)
                if model.dragging == nil {
                    let (sx, sy) = unit(v.startLocation, size)
                    guard model.press(sx, sy, reach: Self.grab / max(area(size).width, 1)) != nil else { return }
                }
                let l = v.location
                let outside = l.x < -Self.removeBeyond || l.y < -Self.removeBeyond || l.x > size.width + Self.removeBeyond || l.y > size.height + Self.removeBeyond
                model.drag(x, y, outside: outside)
            }
            .onEnded { _ in model.lift() }
    }

    func draw(_ ctx: inout GraphicsContext, _ size: CGSize) {
        let a = area(size)
        ctx.translateBy(x: a.minX, y: a.minY)
        let w = a.width, h = a.height
        // the grid, quarters
        var grid = Path()
        for q in 1..<4 {
            let f = CGFloat(q) / 4
            grid.move(to: CGPoint(x: f * w, y: 0)); grid.addLine(to: CGPoint(x: f * w, y: h))
            grid.move(to: CGPoint(x: 0, y: f * h)); grid.addLine(to: CGPoint(x: w, y: f * h))
        }
        ctx.stroke(grid, with: .color(.gray.opacity(0.25)), lineWidth: 0.5)
        // the histogram behind
        let hist = Curves.tabHistogram(model.histograms, model.tab)
        if let top = hist.max(), top > 0 {
            var p = Path()
            p.move(to: CGPoint(x: 0, y: h))
            for (i, c) in hist.enumerated() {
                let x0 = CGFloat(i) / CGFloat(hist.count) * w, x1 = CGFloat(i + 1) / CGFloat(hist.count) * w
                let y = h - CGFloat((c / top).squareRoot()) * h * 0.9
                p.addLine(to: CGPoint(x: x0, y: y)); p.addLine(to: CGPoint(x: x1, y: y))
            }
            p.addLine(to: CGPoint(x: w, y: h))
            p.closeSubpath()
            ctx.fill(p, with: .color(.gray.opacity(0.3)))
        }
        // the diagonal, the other curves as ghosts, then the tab's curve
        var diag = Path()
        diag.move(to: CGPoint(x: 0, y: h)); diag.addLine(to: CGPoint(x: w, y: 0))
        ctx.stroke(diag, with: .color(.gray.opacity(0.35)), style: StrokeStyle(lineWidth: 0.5, dash: [4, 4]))
        let colours: [Color] = [.primary, .red, .green, .blue]
        for k in 0..<min(model.tabs.count, 4) where k != model.tab && model.edited(k) {
            ctx.stroke(path(model.curveSet[k], a.size), with: .color(colours[k].opacity(0.3)), lineWidth: 1)
        }
        let c = model.curve
        ctx.stroke(path(c, a.size), with: .color(c.enabled ? colours[min(model.tab, 3)] : .gray), lineWidth: 2)
        for (i, p) in c.points.enumerated() {
            let r: CGFloat = model.dragging == i ? 7 : 5
            let at = CGPoint(x: p.x * w, y: (1 - p.y) * h)
            let dot = Path(ellipseIn: CGRect(x: at.x - r, y: at.y - r, width: 2 * r, height: 2 * r))
            if model.dragging == i && model.removing {
                ctx.stroke(dot, with: .color(.red), lineWidth: 1.5)
            } else {
                ctx.fill(dot, with: .color(.white))
                ctx.stroke(dot, with: .color(.accentColor), lineWidth: 1.5)
            }
        }
    }

    /// A curve through every column (drawn as it applies: the points as they are).
    func path(_ c: CurvesCurve, _ size: CGSize) -> Path {
        var p = Path()
        let n = max(Int(size.width), 2)
        for i in 0...n {
            let x = Double(i) / Double(n)
            let y = c.enabled ? Curves.evaluate(c.points, x) : x
            let at = CGPoint(x: x * size.width, y: (1 - y) * size.height)
            if i == 0 { p.move(to: at) } else { p.addLine(to: at) }
        }
        return p
    }
}

/// The run's picture through the curves' 8-bit lookup tables (one channel through channel 0's,
/// three through each's): what the curves do, before a run.
struct CurvesPreview: SwiftUI.View {
    let handle: String
    let set: [CurvesCurve]
    /// One channel, shown grey: every colour through channel 0's table.
    let single: Bool
    let pictures: Pictures
    @State private var source: LoadedPicture?
    @State private var shown: CGImage?

    var body: some SwiftUI.View {
        Group {
            if let shown {
                Image(decorative: shown, scale: 1).resizable().interpolation(.medium).aspectRatio(contentMode: .fit)
            } else {
                ProgressView()
            }
        }
        .accessibilityIdentifier("curves-preview")
        .task(id: handle) {
            source = await pictures.load(handle)
            await remake()
        }
        .task(id: set) { await remake() }
    }

    func remake() async {
        guard let src = source?.image else { return }
        let luts = (0..<3).map { Curves.lut8(set, single ? 0 : $0) }
        let made = await Task.detached(priority: .userInitiated) { CurvesPreview.apply(src, luts) }.value
        if !Task.isCancelled, let made { shown = made }
    }

    /// `image` with each colour channel through its table (alpha kept).
    nonisolated static func apply(_ image: CGImage, _ luts: [[UInt8]]) -> CGImage? {
        let w = image.width, h = image.height
        guard w > 0, h > 0, luts.count == 3, let space = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        guard let data = ctx.data else { return nil }
        let px = data.bindMemory(to: UInt8.self, capacity: w * h * 4)
        let (r, g, b) = (luts[0], luts[1], luts[2])
        for i in stride(from: 0, to: w * h * 4, by: 4) {
            px[i] = r[Int(px[i])]
            px[i + 1] = g[Int(px[i + 1])]
            px[i + 2] = b[Int(px[i + 2])]
        }
        return ctx.makeImage()
    }
}
