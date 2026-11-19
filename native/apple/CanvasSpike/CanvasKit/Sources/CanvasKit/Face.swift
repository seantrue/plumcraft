// Node faces: layout (the shell measures, plan 5.2) and drawing with Core Graphics and Core Text.
// Everything here is nonisolated and pure, so faces can be rasterised on any thread.
import CoreGraphics
import CoreText
import Foundation

public enum FaceLayout {
    public static let width: CGFloat = 220
    static let header: CGFloat = 30
    static let portRow: CGFloat = 22
    static let widgetRow: CGFloat = 26
    static let previewRow: CGFloat = 126
    static let pad: CGFloat = 8

    public static func measure(_ node: inout NodeSpec) {
        var y = header + 4
        var frames: [CGRect] = []
        var pins: [Int: CGPoint] = [:]
        for (i, row) in node.rows.enumerated() {
            let h: CGFloat
            switch row.kind {
            case .input: h = portRow; pins[i] = CGPoint(x: 7, y: y + h / 2)
            case .output: h = portRow; pins[i] = CGPoint(x: width - 7, y: y + h / 2)
            case .widget: h = widgetRow
            case .preview: h = previewRow
            }
            frames.append(CGRect(x: 0, y: y, width: width, height: h))
            y += h
        }
        node.rowFrames = frames
        node.pins = pins
        node.size = CGSize(width: width, height: y + pad)
    }
}

enum Palette {
    static func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
        CGColor(
            srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255, alpha: a)
    }
    static let typeHex: [UInt32] = [0x64B5F6, 0x81C784, 0xFFB74D, 0xE57373, 0xBA68C8, 0x4DD0E1, 0xF06292, 0xDCE775]
    static let familyHex: [UInt32] = [0x3D5A80, 0x5C4D7D, 0x2E6F57, 0x7A4E2D, 0x6B2D5C, 0x2D5F6B, 0x5F6B2D, 0x4A4A6A]
    static let types: [CGColor] = typeHex.map { rgb($0) }
    static let families: [CGColor] = familyHex.map { rgb($0) }
    static let body = rgb(0x2A2D33)
    static let border = rgb(0x4A4F58)
    static let text = rgb(0xE8EAED)
    static let dim = rgb(0xA0A6B0)
    static let field = rgb(0x1E2025)
    static let canvas = rgb(0x17181B)

    static func state(_ s: RunState) -> CGColor? {
        switch s {
        case .notRun: return nil
        case .waiting: return rgb(0x9E9E9E, 0.22)
        case .running: return rgb(0x42A5F5, 0.28)
        case .done: return rgb(0x66BB6A, 0.22)
        case .cached: return rgb(0x26A69A, 0.18)
        case .failed: return rgb(0xEF5350, 0.30)
        case .skipped: return rgb(0x757575, 0.30)
        case .stale: return rgb(0xFFCA28, 0.20)
        }
    }
}

public enum FaceRenderer {
    nonisolated(unsafe) static let titleFont = CTFontCreateUIFontForLanguage(.emphasizedSystem, 12.5, nil)
        ?? CTFontCreateWithName("Helvetica-Bold" as CFString, 12.5, nil)
    nonisolated(unsafe) static let rowFont = CTFontCreateUIFontForLanguage(.system, 11, nil)
        ?? CTFontCreateWithName("Helvetica" as CFString, 11, nil)
    nonisolated(unsafe) static let monoFont = CTFontCreateUIFontForLanguage(.userFixedPitch, 11, nil)
        ?? CTFontCreateWithName("Menlo" as CFString, 11, nil)

    static func line(_ s: String, _ font: CTFont, _ color: CGColor) -> CTLine {
        let attrs: [NSAttributedString.Key: Any] = [
            NSAttributedString.Key(kCTFontAttributeName as String): font,
            NSAttributedString.Key(kCTForegroundColorAttributeName as String): color,
        ]
        return CTLineCreateWithAttributedString(NSAttributedString(string: s, attributes: attrs))
    }

    /// Draws `s` with its baseline at `y` (CG, y up); right-aligned to x when `right`.
    static func draw(_ s: String, _ font: CTFont, _ color: CGColor, x: CGFloat, y: CGFloat, right: Bool = false, in ctx: CGContext) {
        let l = line(s, font, color)
        var px = x
        if right { px -= CGFloat(CTLineGetTypographicBounds(l, nil, nil, nil)) }
        ctx.textPosition = CGPoint(x: px, y: y)
        CTLineDraw(l, ctx)
    }

    /// A ~128 px generated picture standing in for a preview.
    public static func thumbnail(seed: UInt64, px: Int = 128) -> CGImage? {
        var rng = SplitMix64(seed: seed)
        guard let ctx = CGContext(
            data: nil, width: px, height: px, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue)
        else { return nil }
        let c0 = Palette.rgb(UInt32(truncatingIfNeeded: rng.next()) & 0xFFFFFF)
        let c1 = Palette.rgb(UInt32(truncatingIfNeeded: rng.next()) & 0xFFFFFF)
        if let g = CGGradient(colorsSpace: nil, colors: [c0, c1] as CFArray, locations: [0, 1]) {
            ctx.drawLinearGradient(g, start: .zero, end: CGPoint(x: px, y: px), options: [])
        }
        for _ in 0..<6 {
            ctx.setFillColor(Palette.rgb(UInt32(truncatingIfNeeded: rng.next()) & 0xFFFFFF, 0.6))
            let r = CGFloat(rng.int(8...40))
            ctx.fillEllipse(in: CGRect(x: CGFloat(rng.int(0...px)) - r, y: CGFloat(rng.int(0...px)) - r, width: 2 * r, height: 2 * r))
        }
        return ctx.makeImage()
    }

    /// Rasterises a node's face at `scale` pixels per graph point. Thread-safe.
    public static func render(_ node: NodeSpec, thumbnail: CGImage?, scale: CGFloat) -> CGImage? {
        let w = Int((node.size.width * scale).rounded(.up))
        let h = Int((node.size.height * scale).rounded(.up))
        guard w > 0, h > 0, let ctx = CGContext(
            data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue)
        else { return nil }
        ctx.scaleBy(x: scale, y: scale)
        let H = node.size.height
        let W = node.size.width
        // y-down rect to CG (y up)
        func cg(_ r: CGRect) -> CGRect { CGRect(x: r.minX, y: H - r.maxY, width: r.width, height: r.height) }

        let body = CGPath(roundedRect: CGRect(x: 0.5, y: 0.5, width: W - 1, height: H - 1), cornerWidth: 8, cornerHeight: 8, transform: nil)
        ctx.addPath(body)
        ctx.setFillColor(Palette.body)
        ctx.fillPath()
        // header
        ctx.saveGState()
        ctx.addPath(body)
        ctx.clip()
        ctx.setFillColor(Palette.families[node.family % 8])
        ctx.fill(cg(CGRect(x: 0, y: 0, width: W, height: FaceLayout.header)))
        ctx.restoreGState()
        ctx.addPath(body)
        ctx.setStrokeColor(Palette.border)
        ctx.setLineWidth(1)
        ctx.strokePath()
        draw(node.title, titleFont, Palette.text, x: 10, y: H - 20, in: ctx)
        // engine mark
        ctx.setFillColor(Palette.types[(node.family + 3) % 8])
        ctx.fillEllipse(in: cg(CGRect(x: W - 18, y: 11, width: 8, height: 8)))

        for (i, row) in node.rows.enumerated() {
            let f = node.rowFrames[i]
            let base = H - f.maxY + 7
            switch row.kind {
            case .input(let t):
                ctx.setFillColor(Palette.types[t])
                ctx.fillEllipse(in: cg(CGRect(x: 2, y: f.midY - 5, width: 10, height: 10)))
                draw(row.caption, rowFont, Palette.dim, x: 18, y: base, in: ctx)
            case .output(let t):
                ctx.setFillColor(Palette.types[t])
                ctx.fillEllipse(in: cg(CGRect(x: W - 12, y: f.midY - 5, width: 10, height: 10)))
                draw(row.caption, rowFont, Palette.dim, x: W - 18, y: base, right: true, in: ctx)
            case .widget(let v):
                let fr = CGRect(x: 10, y: f.minY + 3, width: W - 20, height: f.height - 6)
                ctx.addPath(CGPath(roundedRect: cg(fr), cornerWidth: 4, cornerHeight: 4, transform: nil))
                ctx.setFillColor(Palette.field)
                ctx.fillPath()
                draw(row.caption, rowFont, Palette.dim, x: 18, y: base + 1, in: ctx)
                draw(v, monoFont, Palette.text, x: W - 18, y: base + 1, right: true, in: ctx)
            case .preview:
                let fr = CGRect(x: 10, y: f.minY + 4, width: W - 20, height: f.height - 8)
                if let thumbnail {
                    ctx.saveGState()
                    ctx.addPath(CGPath(roundedRect: cg(fr), cornerWidth: 4, cornerHeight: 4, transform: nil))
                    ctx.clip()
                    ctx.interpolationQuality = .medium
                    ctx.draw(thumbnail, in: cg(fr))
                    ctx.restoreGState()
                }
            }
        }
        return ctx.makeImage()
    }

    /// Bytes a face bitmap takes at `scale` (4 bytes a pixel), for the memory estimate.
    public static func bytes(_ node: NodeSpec, scale: CGFloat) -> Int {
        Int((node.size.width * scale).rounded(.up)) * Int((node.size.height * scale).rounded(.up)) * 4
    }
}
