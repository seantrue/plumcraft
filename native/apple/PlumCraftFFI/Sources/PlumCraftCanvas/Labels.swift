// Small pictures with text: frames' tabs, open groups' bands, net tags, wire marks. Drawn once on
// a worker like faces, at a fixed scale (twice the screen's), so they stay sharp to zoom 2.

import CoreGraphics
import CoreText
import Foundation

public enum LabelRenderer {
    public struct Spec: Sendable, Equatable {
        public var text: String
        public var size: CGSize
        public var fill: CGColorBox?
        public var stroke: CGColorBox?
        public var ink: CGColorBox
        public var corner: CGFloat
        public var mono: Bool
        public var bold: Bool
        public var inset: CGFloat = 6
        public var centred: Bool = false
    }

    /// Draw `spec` at `scale` pixels per graph unit.
    public static func render(_ s: Spec, fonts: FaceFonts = .standard, scale: CGFloat) -> CGImage? {
        let pw = Int((s.size.width * scale).rounded(.up))
        let ph = Int((s.size.height * scale).rounded(.up))
        guard pw > 0, ph > 0, pw < 16384, ph < 16384, let space = CGColorSpace(name: CGColorSpace.sRGB),
              let ctx = CGContext(data: nil, width: pw, height: ph, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return nil }
        ctx.translateBy(x: 0, y: CGFloat(ph))
        ctx.scaleBy(x: scale, y: -scale)
        ctx.textMatrix = CGAffineTransform(scaleX: 1, y: -1)
        let rect = CGRect(origin: .zero, size: s.size).insetBy(dx: 0.75, dy: 0.75)
        let path = CGPath(roundedRect: rect, cornerWidth: min(s.corner, rect.height / 2), cornerHeight: min(s.corner, rect.height / 2), transform: nil)
        if let f = s.fill {
            ctx.addPath(path); ctx.setFillColor(f.color); ctx.fillPath()
        }
        if let k = s.stroke {
            ctx.addPath(path); ctx.setStrokeColor(k.color); ctx.setLineWidth(1.2); ctx.strokePath()
        }
        let font = s.mono ? fonts.mono : (s.bold ? fonts.title : fonts.small)
        let l = TextLine(s.text, font, s.ink.color, max: s.size.width - 2 * s.inset)
        let x = s.centred ? (s.size.width - l.width) / 2 : s.inset
        ctx.textPosition = CGPoint(x: x, y: s.size.height / 2 + (l.ascent - l.descent) / 2)
        CTLineDraw(l.line, ctx)
        return ctx.makeImage()
    }
}

/// A CGColor that can cross to a worker. CGColor is immutable and thread-safe; this box only
/// says so to the compiler, and compares by components.
public struct CGColorBox: @unchecked Sendable, Equatable {
    public let color: CGColor
    public init(_ c: CGColor) { color = c }
    public static func == (a: CGColorBox, b: CGColorBox) -> Bool { a.color == b.color }
}
