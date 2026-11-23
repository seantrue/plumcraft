// `--then` steps both apps share for the editing gestures (N4 slice 1), so a script's screenshots
// show a node mid-drag, a wire over a pin, a marquee and the palette, driven through the same
// CanvasDrag and PaletteModel the recognisers feed (not the recognisers themselves: the iPad's UI
// test drives those; a Mac UI test would take the keyboard from the person at the Mac).
//
//   press:TARGET        begin a drag where TARGET is (it stays pressed)
//   moveto:TARGET[:N]   move the press there in N steps (10), a frame apart
//   release             let go (the core runs what the drag means)
//   release-cancel      cancel it (Escape)
//   palette:TARGET      open the palette there (Tab, Node › Search…)
//   type:TEXT           the palette's query
//   pick:LABEL          choose the palette's row with that label or kind
//   dismiss             dismiss the palette
//   tap:TARGET          a tap there, as a click or a finger's (`ui.gesture` tap: a row opens its
//                       editor, a switch flips)
//   edit:ID.PORT        open the field editor on a row (`row.edit`, as Tab or a script does)
//   field:TEXT          what the open field holds now, as a person would type it
//   return, tab, backtab, escape   the field editor's keys (Return or Done commits)
//   choose:LABEL        pick a choice as the row's menu would (`row.set`)
//   color:#RRGGBB       a colour, as the colour picker closing commits it
//   zoom:Z              the canvas's zoom about the view's centre (no animation)
//
// TARGET is `node=ID` (a face's centre), `head=ID` (its header), `out=ID.K` and `in=ID.K` (its
// K-th output or input pin), `wire=ID` (a point on the wire), `at=X,Y` (graph units) or
// `rel=DX,DY` (view points from the press, or from the view's centre with no press), `row=ID.K`
// (the K-th input row's widget), `browse=ID.K` (a path row's Browse button, B133) or `more=ID` (its
// more-button).

import CoreGraphics
import Foundation
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels

@MainActor
public struct CanvasSteps {
    public let canvas: LayerCanvas
    public let drag: CanvasDrag
    public let palette: PaletteModel
    public let rows: RowEditor?
    public let field: FieldEditing?
    /// Where the press under way is (view points), across steps.
    static var pressed: CGPoint?

    public init(canvas: LayerCanvas, drag: CanvasDrag, palette: PaletteModel, rows: RowEditor? = nil, field: FieldEditing? = nil) {
        self.canvas = canvas
        self.drag = drag
        self.palette = palette
        self.rows = rows
        self.field = field
    }

    /// Run `parts` (a step split at ':') if it is one of these: whether it was.
    public func run(_ parts: [String]) async -> Bool {
        guard let head = parts.first else { return false }
        let arg = parts.count >= 2 ? parts[1] : ""
        switch head {
        case "press":
            guard let p = await point(arg) else { return true }
            Self.pressed = p
            drag.begin(at: p, mods: [])
            _ = await canvas.model.wait(timeout: .seconds(3)) { drag.kind != nil }
            drag.move(to: p)
        case "moveto":
            guard let to = await point(arg), let from = Self.pressed else { return true }
            let n = max(Int(parts.count >= 3 ? parts[2] : "") ?? 10, 1)
            for k in 1...n {
                let t = CGFloat(k) / CGFloat(n)
                let p = CGPoint(x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t)
                drag.move(to: p)
                try? await Task.sleep(for: .milliseconds(16))
            }
            Self.pressed = to
        case "release":
            if let p = Self.pressed { drag.end(at: p) }
            Self.pressed = nil
            try? await Task.sleep(for: .milliseconds(300))
        case "release-cancel":
            drag.cancel()
            Self.pressed = nil
            try? await Task.sleep(for: .milliseconds(300))
        case "palette":
            let p = await point(arg) ?? CGPoint(x: canvas.viewSize.width / 2, y: canvas.viewSize.height / 2)
            let g = canvas.viewport.toGraph(p)
            try? await palette.open(at: Point(Double(g.x), Double(g.y)))
            _ = await canvas.model.wait(timeout: .seconds(3)) { palette.isOpen }
        case "type":
            palette.search(parts.dropFirst().joined(separator: ":"))
            try? await Task.sleep(for: .milliseconds(400))
        case "pick":
            let label = parts.dropFirst().joined(separator: ":")
            _ = await canvas.model.wait(timeout: .seconds(3)) { palette.items.contains { $0.label == label || $0.kind == label } }
            if let it = palette.items.first(where: { $0.label == label || $0.kind == label }) { palette.choose(it) }
            _ = await canvas.model.wait(timeout: .seconds(40)) { !palette.isOpen }
        case "dismiss":
            palette.dismiss()
            try? await Task.sleep(for: .milliseconds(400))
        case "tap":
            guard let p = await point(arg) else { return true }
            let g = canvas.viewport.toGraph(p)
            _ = try? await canvas.link.tap(Point(Double(g.x), Double(g.y)), slop: CanvasLink.slop(points: 4, zoom: Double(canvas.viewport.zoom)))
            try? await Task.sleep(for: .milliseconds(400))
        case "edit":
            guard let dot = arg.lastIndex(of: ".") else { return true }
            try? await rows?.edit(node: String(arg[..<dot]), port: String(arg[arg.index(after: dot)...]))
            _ = await canvas.model.wait(timeout: .seconds(3)) { rows?.open != nil }
            try? await Task.sleep(for: .milliseconds(300))
        case "field":
            field?.type(parts.dropFirst().joined(separator: ":"))
            try? await Task.sleep(for: .milliseconds(200))
        case "return":
            field?.commitField()
            try? await Task.sleep(for: .milliseconds(500))
        case "tab", "backtab":
            field?.pressTab(back: head == "backtab")
            try? await Task.sleep(for: .milliseconds(500))
        case "escape":
            field?.pressEscape()
            try? await Task.sleep(for: .milliseconds(400))
        case "choose":
            let label = parts.dropFirst().joined(separator: ":")
            if let o = rows?.open, let c = o.widget.choices.first(where: { $0.label == label }) { await rows?.commit(value: c.value) }
            try? await Task.sleep(for: .milliseconds(500))
        case "color":
            let hex = arg.hasPrefix("#") ? String(arg.dropFirst()) : arg
            if hex.count == 6, let v = UInt32(hex, radix: 16) {
                field?.commitColor(CGColor(srgbRed: Double((v >> 16) & 255) / 255, green: Double((v >> 8) & 255) / 255, blue: Double(v & 255) / 255, alpha: 1))
            }
            try? await Task.sleep(for: .milliseconds(500))
        case "zoom":
            if let z = Double(arg) { canvas.zoom(to: CGFloat(z), animated: false) }
            try? await Task.sleep(for: .milliseconds(500))
        default:
            return false
        }
        return true
    }

    /// TARGET as a view point.
    func point(_ t: String) async -> CGPoint? {
        let kv = t.split(separator: "=", maxSplits: 1).map(String.init)
        guard kv.count == 2 else { return nil }
        let (k, v) = (kv[0], kv[1])
        func view(_ g: Point) -> CGPoint { canvas.viewport.toView(CGPoint(x: g.x, y: g.y)) }
        func nums(_ s: String) -> (Double, Double)? {
            let a = s.split(separator: ",").compactMap { Double($0) }
            return a.count == 2 ? (a[0], a[1]) : nil
        }
        switch k {
        case "node", "head":
            let r = try? await canvas.link.locate(["node": .string(v), "part": .string(k == "head" ? "header" : "center")])
            return r.map { view($0.at) }
        case "save":
            // the footer's Save Result… line, where the canvas measured it
            let r = try? await canvas.link.locate(["node": .string(v), "part": "save"])
            return r.map { view($0.at) }
        case "out", "in":
            guard let dot = v.lastIndex(of: "."), let i = Int(v[v.index(after: dot)...]) else { return nil }
            let id = String(v[..<dot])
            let r = try? await canvas.link.locate(["node": .string(id), "pin": ["side": .string(k), "index": .int(Int64(i))]])
            return r.map { view($0.at) }
        case "wire":
            let r = try? await canvas.link.locate(["wire": .string(v)])
            return r.map { view($0.at) }
        case "at":
            return nums(v).map { view(Point($0.0, $0.1)) }
        case "row":
            guard let dot = v.lastIndex(of: "."), let i = Int(v[v.index(after: dot)...]), let nid = canvas.nid(of: String(v[..<dot])),
                  let r = canvas.rowRect(face: nid, row: i) else { return nil }
            return view(Point(Double(r.midX), Double(r.midY)))
        case "browse":
            // a path row's Browse button (B133), where the core has it (geometry.locate)
            guard let dot = v.lastIndex(of: "."), let i = Int(v[v.index(after: dot)...]) else { return nil }
            let r = try? await canvas.link.locate(["node": .string(String(v[..<dot])), "row": .int(Int64(i)), "part": "browse"])
            return r.map { view($0.at) }
        case "more":
            guard let nid = canvas.nid(of: v), let r = canvas.moreRect(face: nid) else { return nil }
            return view(Point(Double(r.midX), Double(r.midY)))
        case "rel":
            guard let (dx, dy) = nums(v) else { return nil }
            let o = Self.pressed ?? CGPoint(x: canvas.viewSize.width / 2, y: canvas.viewSize.height / 2)
            return CGPoint(x: o.x + dx, y: o.y + dy)
        default:
            return nil
        }
    }
}
