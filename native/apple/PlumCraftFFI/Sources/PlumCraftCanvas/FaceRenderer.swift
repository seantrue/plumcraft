// Drawing a face once into a bitmap with Core Graphics and Core Text, from its scene item and its
// layout. Nonisolated and pure: faces are drawn on workers, concurrently, and the picture becomes
// the face layer's contents (plan 5.4). The picture reaches FaceMetrics.margin past the face's
// rect on every side, for the pins on its edges and its border.
//
// Not drawn here, so a change to them never redraws a face: the selection, the run wash (both
// sublayers) and the preview's picture (a sublayer whose contents arrive from the media service).

import CoreGraphics
import CoreText
import Foundation
import PlumCraftModels

public enum FaceRenderer {
    /// Bytes a face's picture takes at `scale` (4 a pixel).
    public static func bytes(_ size: CGSize, scale: CGFloat) -> Int {
        let m = FaceMetrics.margin * 2
        return Int(((size.width + m) * scale).rounded(.up)) * Int(((size.height + m) * scale).rounded(.up)) * 4
    }

    /// Draw `face` laid out as `layout` at `scale` pixels per graph unit.
    public static func render(_ face: Face, _ layout: FaceLayout, colors: CanvasColors, fonts: FaceFonts = .standard, scale: CGFloat) -> CGImage? {
        let m = FaceMetrics.margin
        let W = layout.size.width
        let H = layout.size.height
        let pw = Int(((W + 2 * m) * scale).rounded(.up))
        let ph = Int(((H + 2 * m) * scale).rounded(.up))
        guard pw > 0, ph > 0, pw < 16384, ph < 16384, let space = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: pw, height: ph, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return nil }
        // y down, in graph units, with the face's top left at (0, 0)
        ctx.translateBy(x: 0, y: CGFloat(ph))
        ctx.scaleBy(x: scale, y: -scale)
        ctx.translateBy(x: m, y: m)
        ctx.textMatrix = CGAffineTransform(scaleX: 1, y: -1)
        ctx.setShouldSmoothFonts(false)
        draw(face, layout, colors: colors, fonts: fonts, in: ctx)
        return ctx.makeImage()
    }

    static func text(_ s: String, _ font: CTFont, _ color: CGColor, x: CGFloat, baseline: CGFloat, max: CGFloat = 0, right: Bool = false, in ctx: CGContext) {
        let l = max > 0 ? TextLine(s, font, color, max: max) : TextLine(s, font, color)
        ctx.textPosition = CGPoint(x: right ? x - l.width : x, y: baseline)
        CTLineDraw(l.line, ctx)
    }

    static func font(_ s: PlacedText.Style, _ f: FaceFonts) -> CTFont {
        switch s {
        case .title: f.title
        case .port: f.port
        case .caption: f.caption
        case .small: f.small
        case .mono: f.mono
        case .italic: f.italic
        }
    }

    static func draw(_ face: Face, _ L: FaceLayout, colors C: CanvasColors, fonts F: FaceFonts, in ctx: CGContext) {
        let W = L.size.width
        let H = L.size.height
        let r = FaceMetrics.corner
        let rect = CGRect(x: 0, y: 0, width: W, height: H)
        let body = CGPath(roundedRect: rect, cornerWidth: r, cornerHeight: r, transform: nil)

        // a soft shadow, then the body and the header
        ctx.saveGState()
        ctx.setShadow(offset: CGSize(width: 0, height: 1.5), blur: 4, color: CGColor(srgbRed: 0, green: 0, blue: 0, alpha: C.isDark ? 0.45 : 0.18))
        ctx.addPath(body)
        ctx.setFillColor(C.color(face.tint.body))
        ctx.fillPath()
        ctx.restoreGState()
        ctx.saveGState()
        ctx.addPath(body)
        ctx.clip()
        ctx.setFillColor(C.color(face.tint.header))
        ctx.fill(CGRect(x: 0, y: 0, width: W, height: L.header))
        if !L.collapsed {
            ctx.setFillColor(C.separator.copy(alpha: 0.6) ?? C.separator)
            ctx.fill(CGRect(x: 0, y: L.header - 0.5, width: W, height: 1))
        }
        ctx.restoreGState()

        // the border: the core's (highlight, proxy, stale, run state, watched), 1.5 wider when watched
        if let b = face.border {
            let w = CGFloat(b.width) + (face.watched ? 1.5 : 0)
            ctx.addPath(CGPath(roundedRect: rect.insetBy(dx: -w / 2 + 0.5, dy: -w / 2 + 0.5), cornerWidth: r + w / 2, cornerHeight: r + w / 2, transform: nil))
            ctx.setStrokeColor(C.color(b.paint))
            ctx.setLineWidth(w)
            ctx.strokePath()
        } else {
            ctx.addPath(body)
            ctx.setStrokeColor(C.fieldBorder)
            ctx.setLineWidth(1)
            ctx.strokePath()
        }

        header(face, L, C, F, ctx)
        if !L.collapsed {
            rows(face, L, C, F, ctx)
            if let p = L.preview {
                ctx.addPath(CGPath(roundedRect: p, cornerWidth: 3, cornerHeight: 3, transform: nil))
                ctx.setFillColor(C.placeholder)
                ctx.fillPath()
            }
            footer(face, L, C, F, ctx)
        }
        pins(face, L, C, ctx)
    }

    static func header(_ face: Face, _ L: FaceLayout, _ C: CanvasColors, _ F: FaceFonts, _ ctx: CGContext) {
        let h = face.header
        let ink = C.headerText
        if let c = L.chevron {
            // ▾ open, ▸ closed
            ctx.beginPath()
            if h.open {
                ctx.move(to: CGPoint(x: c.x - 4, y: c.y - 2)); ctx.addLine(to: CGPoint(x: c.x + 4, y: c.y - 2)); ctx.addLine(to: CGPoint(x: c.x, y: c.y + 3))
            } else {
                ctx.move(to: CGPoint(x: c.x - 2, y: c.y - 4)); ctx.addLine(to: CGPoint(x: c.x + 3, y: c.y)); ctx.addLine(to: CGPoint(x: c.x - 2, y: c.y + 4))
            }
            ctx.closePath()
            ctx.setFillColor(ink.copy(alpha: 0.55) ?? ink)
            ctx.fillPath()
        }
        switch h.state.kind.word {
        case .dot?:
            ctx.setFillColor(h.state.paint.map(C.color) ?? C.textDim)
            ctx.fillEllipse(in: L.spot)
        case .spinner?:
            ctx.setStrokeColor(h.state.paint.map(C.color) ?? C.accent)
            ctx.setLineWidth(1.6)
            ctx.addArc(center: CGPoint(x: L.spot.midX, y: L.spot.midY), radius: 4, startAngle: 0, endAngle: .pi * 1.5, clockwise: false)
            ctx.strokePath()
        default:
            break
        }
        let t = L.title
        text(t.text, F.title, ink, x: t.x, baseline: t.baseline, max: t.max, in: ctx)
        for (k, b) in L.badges.enumerated() where k < h.badges.count {
            let col = C.color(h.badges[k].paint)
            ctx.addPath(CGPath(roundedRect: b.rect, cornerWidth: 8, cornerHeight: 8, transform: nil))
            ctx.setStrokeColor(col)
            ctx.setLineWidth(1)
            ctx.strokePath()
            text(b.text, F.small, col, x: b.rect.minX + 5, baseline: b.rect.midY + 4, in: ctx)
        }
        for (k, s) in L.status.enumerated() where k < h.status.count {
            text(s.text, font(s.style, F), C.color(h.status[k].paint), x: s.x, baseline: s.baseline, right: true, in: ctx)
        }
        if let e = L.engine {
            text(e.text, F.small, C.textDim, x: e.x, baseline: e.baseline, right: true, in: ctx)
        }
        if let e = L.enter {
            // a pencil: enter the group
            ctx.setStrokeColor(ink.copy(alpha: 0.7) ?? ink)
            ctx.setLineWidth(1.4)
            ctx.move(to: CGPoint(x: e.minX + 3, y: e.maxY - 3)); ctx.addLine(to: CGPoint(x: e.maxX - 3, y: e.minY + 3))
            ctx.strokePath()
            ctx.move(to: CGPoint(x: e.minX + 3, y: e.maxY - 3)); ctx.addLine(to: CGPoint(x: e.minX + 6, y: e.maxY - 3))
            ctx.strokePath()
        }
    }

    static func rows(_ face: Face, _ L: FaceLayout, _ C: CanvasColors, _ F: FaceFonts, _ ctx: CGContext) {
        for (k, r) in face.inputs.enumerated() where k < L.inputRows.count {
            let row = L.inputRows[k]
            let base = row.minY + min(row.height, FaceMetrics.row) / 2 + 4.5
            var x: CGFloat = 14
            if let n = r.named {
                let s = n.name + " →"
                text(s, F.small, C.color(n.paint), x: x, baseline: base, in: ctx)
                x += TextLine.width(s, F.small) + 6
            }
            let caption = FaceLayout.inputCaption(r)
            let inherited = r.source.word == .inherited
            let col = r.dim ? C.textDim : C.text
            let lim = (L.widgets[k]?.minX ?? L.size.width - 8) - x - 6
            text(caption, inherited ? F.italic : F.port, col, x: x, baseline: base, max: max(lim, 20), in: ctx)
            if let b = r.badge {
                let bx = min(x + TextLine.width(caption, F.port) + 6, L.size.width - 40)
                let br = CGRect(x: bx, y: row.minY + 3, width: TextLine.width(b.text, F.small) + 8, height: 16)
                ctx.addPath(CGPath(roundedRect: br, cornerWidth: 4, cornerHeight: 4, transform: nil))
                ctx.setStrokeColor(C.color(b.paint)); ctx.setLineWidth(1); ctx.strokePath()
                text(b.text, F.small, C.color(b.paint), x: br.minX + 4, baseline: br.midY + 4, in: ctx)
            }
            if let wr = L.widgets[k], let w = r.widget { widget(w, r, wr, C, F, ctx) }
        }
        for (k, r) in face.outputs.enumerated() where k < L.outputRows.count {
            let row = L.outputRows[k]
            text(r.label, F.port, C.text, x: L.size.width - 14, baseline: row.minY + row.height / 2 + 4.5, max: row.width - 20, right: true, in: ctx)
        }
    }

    static func widget(_ w: Widget, _ r: InRow, _ rect: CGRect, _ C: CanvasColors, _ F: FaceFonts, _ ctx: CGContext) {
        let box = CGPath(roundedRect: rect, cornerWidth: 3, cornerHeight: 3, transform: nil)
        let value = FaceLayout.widgetText(w)
        let ink = w.blank ? C.textFaint : C.text
        switch w.kind.word {
        case .toggle?:
            ctx.addPath(box); ctx.setFillColor(w.value.bool == true ? C.accent : C.field); ctx.fillPath()
            ctx.addPath(box); ctx.setStrokeColor(C.fieldBorder); ctx.setLineWidth(1); ctx.strokePath()
            if w.value.bool == true {
                ctx.setStrokeColor(CGColor(srgbRed: 1, green: 1, blue: 1, alpha: 1)); ctx.setLineWidth(1.8)
                ctx.move(to: CGPoint(x: rect.minX + 3, y: rect.midY)); ctx.addLine(to: CGPoint(x: rect.minX + 6, y: rect.maxY - 3.5))
                ctx.addLine(to: CGPoint(x: rect.maxX - 3, y: rect.minY + 3.5)); ctx.strokePath()
            }
        case .color?:
            ctx.addPath(box); ctx.setFillColor(FaceLayout.widgetColor(w) ?? C.field); ctx.fillPath()
            ctx.addPath(box); ctx.setStrokeColor(C.fieldBorder); ctx.setLineWidth(1); ctx.strokePath()
        case .editor?:
            ctx.addPath(box); ctx.setFillColor(C.placeholder); ctx.fillPath()
            ctx.addPath(box); ctx.setStrokeColor(C.fieldBorder); ctx.setLineWidth(1); ctx.strokePath()
            text(r.label, F.small, C.text, x: rect.midX - TextLine.width(r.label, F.small) / 2, baseline: rect.midY + 4, in: ctx)
        default:
            ctx.addPath(box); ctx.setFillColor(C.field); ctx.fillPath()
            ctx.addPath(box); ctx.setStrokeColor(C.fieldBorder); ctx.setLineWidth(1); ctx.strokePath()
            switch w.kind.word {
            case .number?:
                text(value, F.mono, ink, x: rect.maxX - 6, baseline: rect.midY + 4, max: rect.width - 8, right: true, in: ctx)
            case .choice?:
                text(value, F.small, ink, x: rect.minX + 6, baseline: rect.midY + 4, max: rect.width - 20, in: ctx)
                let c = CGPoint(x: rect.maxX - 8, y: rect.midY)
                ctx.move(to: CGPoint(x: c.x - 3, y: c.y - 1.5)); ctx.addLine(to: CGPoint(x: c.x, y: c.y + 1.5)); ctx.addLine(to: CGPoint(x: c.x + 3, y: c.y - 1.5))
                ctx.setStrokeColor(C.textDim); ctx.setLineWidth(1.2); ctx.strokePath()
            case .multiline?:
                for (i, line) in FaceLayout.wrap(value, F.mono, rect.width - 10, 3).enumerated() {
                    text(line, F.mono, ink, x: rect.minX + 5, baseline: rect.minY + 13 + CGFloat(i) * 14, in: ctx)
                }
            case .path?:
                text(value, F.small, ink, x: rect.minX + 5, baseline: rect.midY + 4, max: rect.width - 22, in: ctx)
                text("…", F.small, C.textDim, x: rect.maxX - 5, baseline: rect.midY + 4, right: true, in: ctx)
            default:
                text(value, F.small, ink, x: rect.minX + 5, baseline: rect.midY + 4, max: rect.width - 10, in: ctx)
            }
        }
    }

    static func footer(_ face: Face, _ L: FaceLayout, _ C: CanvasColors, _ F: FaceFonts, _ ctx: CGContext) {
        let items = face.footer.filter { if case .unknown = $0 { false } else { true } }
        for (k, plan) in L.footer.enumerated() where k < items.count {
            var y = plan.rect.minY
            var ink = C.textDim
            switch items[k] {
            case .error(_, _, let paint): ink = C.color(paint)
            case .excerpt(_, let faded, _): ink = faded ? C.textFaint : C.text
            case .convert(let lines):
                for (i, l) in lines.enumerated() {
                    text(l.text, F.small, C.color(l.paint), x: plan.rect.minX, baseline: y + 11 + CGFloat(i) * FaceMetrics.footerLine, max: plan.rect.width, in: ctx)
                }
                continue
            case .fields, .save, .editor: ink = C.accent
            default: break
            }
            if let p = plan.progress {
                let bar = CGRect(x: plan.rect.minX, y: y + 4, width: plan.rect.width, height: 5)
                ctx.addPath(CGPath(roundedRect: bar, cornerWidth: 2.5, cornerHeight: 2.5, transform: nil))
                ctx.setFillColor(C.field); ctx.fillPath()
                let fill = CGRect(x: bar.minX, y: bar.minY, width: bar.width * CGFloat(min(max(p, 0), 1)), height: bar.height)
                ctx.addPath(CGPath(roundedRect: fill, cornerWidth: 2.5, cornerHeight: 2.5, transform: nil))
                ctx.setFillColor(C.token("state_running", RGBA(66, 165, 245, 255))); ctx.fillPath()
                y += FaceMetrics.footerLine * 0.6
            }
            for (i, line) in plan.lines.enumerated() {
                text(line, plan.mono ? F.mono : F.small, ink, x: plan.rect.minX, baseline: y + 11 + CGFloat(i) * FaceMetrics.footerLine, max: plan.rect.width, in: ctx)
            }
        }
    }

    static func pins(_ face: Face, _ L: FaceLayout, _ C: CanvasColors, _ ctx: CGContext) {
        func pin(_ p: Pin, at c: CGPoint) {
            guard p.wireable else { return }
            let r = FaceMetrics.pinRadius
            let box = CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r)
            ctx.setFillColor(C.color(p.fill))
            ctx.setStrokeColor(C.canvas)
            ctx.setLineWidth(1.2)
            if p.shape.word == .square {
                ctx.addRect(box.insetBy(dx: 0.5, dy: 0.5))
            } else {
                ctx.addEllipse(in: box)
            }
            ctx.drawPath(using: .fillStroke)
        }
        for (k, r) in face.inputs.enumerated() where k < L.inputPins.count {
            pin(r.pin, at: CGPoint(x: 0, y: L.inputPins[k]))
        }
        for (k, r) in face.outputs.enumerated() where k < L.outputPins.count {
            pin(r.pin, at: CGPoint(x: L.size.width, y: L.outputPins[k]))
        }
    }
}
