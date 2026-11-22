// A node face laid out from the scene's rows, in graph units at zoom 1 (contract 8.11: the shell
// measures). Inputs in a column on the left with their pins on the left edge, outputs on the right
// with theirs on the right edge, as egui-snarl places them; then the preview and the footer.
//
// Pure and nonisolated: a face is laid out on a worker when its `rev` changes, never per frame.
// The renderer (FaceRenderer) draws from the same plan, so what is drawn, measured and reported
// to the core (and so hit-tested there) always agree.

import CoreGraphics
import CoreText
import Foundation
import PlumCraftModels

/// The face's constants, in graph units.
public enum FaceMetrics {
    public static let header: CGFloat = 28
    public static let row: CGFloat = 22
    public static let multilineRow: CGFloat = 56
    public static let pinRadius: CGFloat = 4.5
    public static let corner: CGFloat = 6
    public static let minWidth: CGFloat = 140
    public static let maxWidth: CGFloat = 440
    public static let titleMax: CGFloat = 230
    /// The header's status slot is kept at least this wide, so a face doesn't jump when a status
    /// comes (contract 8.3).
    public static let statusSlot: CGFloat = 88
    public static let footerLine: CGFloat = 15
    /// The node's icon and the more-button (N4 slice 2), square, in the header.
    public static let icon: CGFloat = 14
    public static let more: CGFloat = 18
    /// The face's picture reaches this far past its rect on every side, for the pins on its edges
    /// and its border.
    public static let margin: CGFloat = 8
}

/// One piece of text placed in the face (baseline origin, y down).
public struct PlacedText: Sendable, Equatable {
    public enum Style: Sendable, Equatable { case title, port, caption, small, mono, italic }
    public var text: String
    public var x: CGFloat
    public var baseline: CGFloat
    public var style: Style
    /// Cut to this width with an ellipsis (0: never).
    public var max: CGFloat
    /// Right-aligned at `x` rather than starting there.
    public var right: Bool = false
}

/// What a footer item shows.
public struct FooterPlan: Sendable, Equatable {
    public var rect: CGRect
    public var lines: [String]
    public var mono: Bool
    public var progress: Double?
}

/// A face laid out.
public struct FaceLayout: Sendable, Equatable {
    public var size: CGSize
    public var header: CGFloat
    public var collapsed: Bool
    /// Each pin's height from the top, in row order (the scene's `inputs` and `outputs`).
    public var inputPins: [CGFloat]
    public var outputPins: [CGFloat]
    public var inputRows: [CGRect]
    public var outputRows: [CGRect]
    /// Each input row's widget rect (face coordinates), none where the row has no widget.
    public var widgets: [CGRect?]
    /// Where the preview's picture goes.
    public var preview: CGRect?
    public var footer: [FooterPlan]
    /// Each footer item's rect in the scene's order (face coordinates; nil for one not drawn),
    /// reported as `parts.footer` so the core hit-tests a tap on a line (the Save Result… line's
    /// tap is the core's save, B122).
    public var footerParts: [CGRect?] = []
    // the header's pieces
    public var chevron: CGPoint?
    public var spot: CGRect
    public var title: PlacedText
    public var badges: [(text: String, rect: CGRect)]
    public var status: [PlacedText]
    public var engine: PlacedText?
    public var enter: CGRect?
    /// The node's icon and the more-button (⋮, the node's menu), N4 slice 2.
    public var icon: CGRect?
    public var more: CGRect?
    /// The title's drawn width (its text, cut at `title.max`): the title part is only that.
    public var titleWidth: CGFloat = 0

    public static func == (a: FaceLayout, b: FaceLayout) -> Bool {
        a.size == b.size && a.header == b.header && a.inputPins == b.inputPins && a.outputPins == b.outputPins && a.widgets == b.widgets
            && a.preview == b.preview && a.footer == b.footer && a.title == b.title && a.status == b.status
            && a.badges.map(\.text) == b.badges.map(\.text) && a.badges.map(\.rect) == b.badges.map(\.rect)
            && a.icon == b.icon && a.more == b.more
    }

    /// The header's and footer's parts the core hit-tests (`geometry.parts`): the title, the
    /// chevron, the icon, the more-button, the enter button, the preview, each footer item.
    public var parts: [String: Rect] {
        func r(_ c: CGRect) -> Rect { Rect(Double(c.minX), Double(c.minY), Double(c.maxX), Double(c.maxY)) }
        var out: [String: Rect] = [:]
        let tw = min(titleWidth, title.max > 0 ? title.max : FaceMetrics.titleMax)
        out["title"] = r(CGRect(x: title.x - 2, y: 4, width: tw + 4, height: header - 8))
        if let c = chevron { out["chevron"] = r(CGRect(x: c.x - 7, y: 4, width: 14, height: header - 8)) }
        if let i = icon { out["icon"] = r(i.insetBy(dx: -2, dy: -2)) }
        if let m = more { out["menu"] = r(m) }
        if let e = enter { out["enter"] = r(e.insetBy(dx: -2, dy: -2)) }
        if let p = preview { out["preview"] = r(p) }
        return out
    }

    /// The footer's items as `parts.footer` (scene order; nil where one isn't drawn).
    public var footerRects: [Rect?] {
        footerParts.map { $0.map { Rect(Double($0.minX), Double($0.minY), Double($0.maxX), Double($0.maxY)) } }
    }

    /// What `plum_geometry_set` takes for this face.
    public func measure(face: UInt64, rev: UInt64) -> FaceMeasureValues {
        FaceMeasureValues(face: face, rev: rev, width: Float(size.width), height: Float(size.height), header: Float(header),
                          inputs: inputPins.map { Float($0) }, outputs: outputPins.map { Float($0) },
                          rows: widgets.map { $0.map { Rect(Double($0.minX), Double($0.minY), Double($0.maxX), Double($0.maxY)) } })
    }
}

/// The plain values of a measurement (PlumCraft's FaceMeasure, without depending on it here).
public struct FaceMeasureValues: Sendable, Equatable {
    public var face: UInt64
    public var rev: UInt64
    public var width: Float
    public var height: Float
    public var header: Float
    public var inputs: [Float]
    public var outputs: [Float]
    public var rows: [Rect?]
}

extension FaceLayout {
    /// Lay `face` out with `fonts`.
    public static func make(_ face: Face, fonts: FaceFonts = .standard) -> FaceLayout {
        let M = FaceMetrics.self
        let h = face.header
        let collapsed = face.view.word == .collapsed

        // the header, left to right: chevron, state spot, title, badges; right: status, engine, enter
        var x: CGFloat = 8
        let chevron = CGPoint(x: x + 4, y: M.header / 2)
        x += 14
        let spot = CGRect(x: x, y: M.header / 2 - 4, width: 8, height: 8)
        x += 14
        let icon = CGRect(x: x, y: (M.header - M.icon) / 2, width: M.icon, height: M.icon)
        x += M.icon + 6
        let titleW = min(TextLine.width(h.title.text, fonts.title), M.titleMax)
        let title = PlacedText(text: h.title.text, x: x, baseline: M.header / 2 + 5, style: .title, max: M.titleMax)
        x += titleW + 8
        var badges: [(String, CGRect)] = []
        for b in h.badges {
            let w = TextLine.width(b.text, fonts.small) + 10
            badges.append((b.text, CGRect(x: x, y: M.header / 2 - 8, width: w, height: 16)))
            x += w + 4
        }
        var statusW: CGFloat = 0
        let statusWidths = h.status.map { TextLine.width($0.text, fonts.small) }
        for w in statusWidths { statusW += w + 6 }
        let engineW = h.engine.map { TextLine.width($0.text, fonts.small) + 8 } ?? 0
        let enterW: CGFloat = h.enter ? 20 : 0
        let moreW: CGFloat = M.more + 2
        let headerW = x + max(statusW, M.statusSlot) + engineW + enterW + moreW + 8

        // rows
        var inW: CGFloat = 0
        var inHeights: [CGFloat] = []
        var widgetSizes: [CGSize?] = []
        var labelWidths: [CGFloat] = []
        for r in face.inputs {
            let label = Self.inputCaption(r)
            let font = r.source.word == .inherited ? fonts.italic : fonts.port
            var lw = TextLine.width(label, font)
            if let n = r.named { lw += TextLine.width(n.name + " →", fonts.small) + 6 }
            if let b = r.badge { lw += TextLine.width(b.text, fonts.small) + 14 }
            labelWidths.append(lw)
            var ws: CGSize?
            if r.source.word == .own, let w = r.widget { ws = Self.widgetSize(w, r, fonts) }
            widgetSizes.append(ws)
            let rowH = (ws?.height ?? 0) > 30 ? M.multilineRow : M.row
            inHeights.append(rowH)
            inW = max(inW, 14 + lw + (ws.map { $0.width + 8 } ?? 0) + 8)
        }
        var outW: CGFloat = 0
        for r in face.outputs {
            outW = max(outW, 8 + TextLine.width(r.label, fonts.port) + 14)
        }
        let rowsW = inW + outW + (inW > 0 && outW > 0 ? 12 : 0)

        // preview and footer widths
        let showPreview = face.view.word == .full || face.view.word == nil
        let pv = showPreview ? face.preview?.size : nil
        let previewW = pv.map { CGFloat($0.x) + 16 } ?? 0
        var width = max(M.minWidth, headerW, rowsW, previewW)
        width = min(width, M.maxWidth)

        // vertical
        let top = M.header + 4
        var inputRows: [CGRect] = []
        var inputPins: [CGFloat] = []
        var widgets: [CGRect?] = []
        var y = top
        for (k, r) in face.inputs.enumerated() {
            let rh = inHeights[k]
            let rect = CGRect(x: 0, y: y, width: inW, height: rh)
            inputRows.append(rect)
            inputPins.append(collapsed ? M.header / 2 : y + min(rh, M.row) / 2)
            if let ws = widgetSizes[k] {
                let wx = min(14 + labelWidths[k] + 8, max(width - ws.width - 8, 14))
                widgets.append(CGRect(x: wx, y: y + (min(rh, M.row) - min(ws.height, 18)) / 2, width: ws.width, height: ws.height))
            } else {
                widgets.append(nil)
            }
            _ = r
            y += rh
        }
        let inBottom = y
        var outputRows: [CGRect] = []
        var outputPins: [CGFloat] = []
        y = top
        for _ in face.outputs {
            outputRows.append(CGRect(x: width - outW, y: y, width: outW, height: M.row))
            outputPins.append(collapsed ? M.header / 2 : y + M.row / 2)
            y += M.row
        }
        var bottom = max(inBottom, y)
        if face.inputs.isEmpty && face.outputs.isEmpty { bottom = M.header }

        var preview: CGRect?
        if let pv {
            let w = CGFloat(pv.x)
            let hgt = CGFloat(pv.y)
            preview = CGRect(x: (width - w) / 2, y: bottom + 4, width: w, height: hgt)
            bottom += hgt + 8
        }

        var footer: [FooterPlan] = []
        var footerParts: [CGRect?] = []
        let textW = width - 16
        for item in face.footer {
            var lines: [String] = []
            var mono = false
            var progress: Double?
            switch item {
            case .fields(let text, _, _): lines = [text]
            case .convert(let ls): lines = ls.map(\.text)
            case .editor(let label, _): lines = [label]
            case .progress(let f, let stage):
                progress = f
                lines = stage.map { [$0] } ?? []
            case .error(let text, _, _): lines = Self.wrap(text, fonts.small, textW, 3)
            case .save(_, let sheet): lines = [sheet ? "Save Result…   Contact Sheet…" : "Save Result…"]
            case .excerpt(let text, _, _):
                mono = true
                lines = Self.wrap(text, fonts.mono, textW, 3)
            case .engineText(let text, _), .appText(let text, _): lines = Self.wrap(text, fonts.small, textW, 2)
            case .unknown:
                footerParts.append(nil)
                continue
            }
            let n = CGFloat(lines.count) + (progress != nil ? 0.6 : 0)
            let hgt = max(n, 1) * M.footerLine + 4
            footer.append(FooterPlan(rect: CGRect(x: 8, y: bottom + 2, width: textW, height: hgt), lines: lines, mono: mono, progress: progress))
            footerParts.append(CGRect(x: 8, y: bottom + 2, width: textW, height: hgt))
            bottom += hgt + 2
        }

        let height = collapsed ? M.header : bottom + 6

        // the header's right side: the more-button at the end
        var rx = width - 6
        let more = CGRect(x: rx - M.more, y: (M.header - M.more) / 2, width: M.more, height: M.more)
        rx -= moreW + 2
        var enter: CGRect?
        if h.enter {
            enter = CGRect(x: rx - 16, y: M.header / 2 - 8, width: 16, height: 16)
            rx -= enterW
        }
        var engine: PlacedText?
        if let e = h.engine {
            engine = PlacedText(text: e.text, x: rx, baseline: M.header / 2 + 4, style: .small, max: 0, right: true)
            rx -= engineW
        }
        var status: [PlacedText] = []
        for (k, s) in h.status.enumerated().reversed() {
            status.append(PlacedText(text: s.text, x: rx, baseline: M.header / 2 + 4, style: s.strong ? .caption : .small, max: 0, right: true))
            rx -= statusWidths[k] + 6
        }
        status.reverse()
        // a title that would run into the status is cut
        var t = title
        t.max = max(20, min(M.titleMax, rx - title.x - 8 - badges.reduce(0) { $0 + $1.1.width + 4 }))

        var made = FaceLayout(size: CGSize(width: width, height: height), header: M.header, collapsed: collapsed,
                          inputPins: inputPins, outputPins: outputPins, inputRows: inputRows, outputRows: outputRows,
                          widgets: collapsed ? widgets.map { _ in nil } : widgets, preview: collapsed ? nil : preview,
                          footer: collapsed ? [] : footer, chevron: chevron, spot: spot, title: t,
                          badges: badges.map { (text: $0.0, rect: $0.1) }, status: status, engine: engine, enter: enter,
                          icon: icon, more: more)
        made.titleWidth = titleW
        made.footerParts = collapsed ? [] : footerParts
        return made
    }

    /// The caption an input row shows: its label, or its text for a wired, socket, inherited or
    /// read-only row.
    static func inputCaption(_ r: InRow) -> String {
        switch r.source.word {
        case .own?: return r.label
        default: return r.text ?? r.label
        }
    }

    /// A widget's size at rest.
    static func widgetSize(_ w: Widget, _ r: InRow, _ fonts: FaceFonts) -> CGSize {
        switch w.kind.word {
        case .number?: return CGSize(width: max(56, TextLine.width(widgetText(w), fonts.mono) + 14), height: 18)
        case .toggle?:
            #if os(iOS)
            // a switch on the iPad, a checkbox on the Mac (each platform's convention)
            return CGSize(width: 26, height: 15)
            #else
            return CGSize(width: 14, height: 14)
            #endif
        case .choice?: return CGSize(width: 120, height: 18)
        case .multichoice?, .text?, .path?: return CGSize(width: 140, height: 18)
        case .multiline?: return CGSize(width: 140, height: 50)
        case .color?: return CGSize(width: 28, height: 16)
        case .editor?: return CGSize(width: TextLine.width(r.label, fonts.small) + 16, height: 18)
        case nil: return CGSize(width: 100, height: 18)
        }
    }

    /// What a widget shows as text at rest.
    public static func widgetText(_ w: Widget) -> String {
        if w.blank || w.value.isNull { return "blank" }
        switch w.kind.word {
        case .number?:
            // the core's text (N4 slice 4): an INT whole, a FLOAT at its decimals
            if let t = w.text { return t }
            guard let d = w.value.double else { return w.value.text }
            if w.int { return String(Int64(d.rounded())) }
            let places = max(0, min(w.decimals ?? 3, 8))
            return String(format: "%.\(places)f", d)
        case .choice?:
            return w.choices.first { $0.value.same(w.value) }?.label ?? (w.value.string ?? w.value.text)
        case .multichoice?:
            if let a = w.value.array { return a.map { $0.string ?? $0.text }.joined(separator: ", ") }
            return w.value.string ?? w.value.text
        case .toggle?:
            return w.value.bool == true ? "on" : "off"
        default:
            return w.value.string ?? w.value.text
        }
    }

    /// A colour widget's colour: `#rrggbb`, `r,g,b` in 0…1, or `[r, g, b]` in 0…1.
    public static func widgetColor(_ w: Widget) -> CGColor? {
        func cg(_ r: Double, _ g: Double, _ b: Double) -> CGColor { CGColor(srgbRed: r, green: g, blue: b, alpha: 1) }
        if let s = w.value.string {
            if s.hasPrefix("#"), s.count == 7, let v = UInt32(s.dropFirst(), radix: 16) {
                return cg(Double((v >> 16) & 0xFF) / 255, Double((v >> 8) & 0xFF) / 255, Double(v & 0xFF) / 255)
            }
            let parts = s.split(separator: ",").compactMap { Double($0.trimmingCharacters(in: .whitespaces)) }
            if parts.count >= 3 { return cg(parts[0], parts[1], parts[2]) }
        }
        if let a = w.value.array, a.count >= 3, let r = a[0].double, let g = a[1].double, let b = a[2].double { return cg(r, g, b) }
        return nil
    }

    /// `text` broken into at most `lines` lines `width` wide, the last cut with an ellipsis.
    static func wrap(_ text: String, _ font: CTFont, _ width: CGFloat, _ lines: Int) -> [String] {
        let flat = text.split(whereSeparator: \.isNewline).joined(separator: " ")
        let s = NSAttributedString(string: flat, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font])
        let ts = CTTypesetterCreateWithAttributedString(s)
        let ns = flat as NSString
        var out: [String] = []
        var at = 0
        while at < ns.length && out.count < lines {
            let n = CTTypesetterSuggestLineBreak(ts, at, Double(max(width, 10)))
            if n <= 0 { break }
            out.append(ns.substring(with: NSRange(location: at, length: n)).trimmingCharacters(in: .whitespaces))
            at += n
        }
        if at < ns.length, !out.isEmpty {
            out[out.count - 1] += "…"
        }
        return out
    }
}
