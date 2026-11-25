// The canvas's look: colours resolved from the theme (`theme.get`), fonts, and the face's
// constants in graph units. Everything here is immutable and safe on any thread, so faces are laid
// out and drawn off the main thread.

import CoreGraphics
import CoreText
import Foundation
import PlumCraftModels

/// The theme's tokens as Core Graphics colours. Scene and theme colours are premultiplied
/// (contract 8.3); a CGColor is not, so each is divided by its alpha here.
public struct CanvasColors: Sendable, Equatable {
    public let tokens: [String: RGBA]
    /// The theme's name (`paper`, `pewter`, `dusk`).
    public let name: String

    public init(tokens: [String: RGBA] = [:], name: String = "") {
        self.tokens = tokens
        self.name = name
    }

    public init(_ theme: ThemeReply?) {
        self.init(tokens: theme?.tokens ?? [:], name: theme?.name ?? "")
    }

    public static func cg(_ c: RGBA) -> CGColor {
        let a = CGFloat(c.a) / 255
        if a <= 0 { return CGColor(srgbRed: 0, green: 0, blue: 0, alpha: 0) }
        let un = { (v: UInt8) in min(CGFloat(v) / 255 / a, 1) }
        return CGColor(srgbRed: un(c.r), green: un(c.g), blue: un(c.b), alpha: a)
    }

    public func rgba(_ p: Paint) -> RGBA { p.resolve(tokens) }
    public func color(_ p: Paint) -> CGColor { Self.cg(rgba(p)) }
    /// A token's colour, or `fallback` (as bytes) when the theme lacks it or hasn't come.
    public func token(_ name: String, _ fallback: RGBA) -> CGColor { Self.cg(tokens[name] ?? fallback) }

    /// Whether the theme is a dark one, judged by its canvas colour.
    public var isDark: Bool {
        guard let c = tokens["canvas"] else { return false }
        return Int(c.r) + Int(c.g) + Int(c.b) < 3 * 110
    }

    // the tokens a face uses, with the Paper theme's values as fallbacks before theme.get answers
    var canvas: CGColor { token("canvas", RGBA(250, 248, 244, 255)) }
    var text: CGColor { token("text", RGBA(40, 40, 44, 255)) }
    var textDim: CGColor { token("text_dim", RGBA(110, 110, 116, 255)) }
    var textFaint: CGColor { token("text_faint", RGBA(150, 150, 156, 255)) }
    var headerText: CGColor { token("header_text", RGBA(30, 30, 34, 255)) }
    var field: CGColor { token("field", RGBA(255, 255, 255, 255)) }
    var fieldBorder: CGColor { token("field_border", RGBA(200, 198, 192, 255)) }
    var separator: CGColor { token("separator", RGBA(214, 210, 202, 255)) }
    var accent: CGColor { token("accent", RGBA(60, 110, 200, 255)) }
    var placeholder: CGColor { token("placeholder", RGBA(230, 228, 222, 255)) }
    var wire: CGColor { token("wire", RGBA(120, 120, 128, 255)) }
    var error: CGColor { token("error", RGBA(200, 50, 50, 255)) }
    var warning: CGColor { token("warning", RGBA(210, 150, 30, 255)) }
    var highlight: CGColor { token("highlight", RGBA(240, 180, 40, 255)) }
    var group: CGColor { token("group", RGBA(140, 90, 160, 255)) }
    var groupBody: CGColor { token("group_body", RGBA(240, 232, 244, 255)) }
    var pinDefault: CGColor { token("pin_default", RGBA(120, 120, 128, 255)) }
}

/// The fonts faces are drawn with: the system face, at the theme's sizes (contract 14.3: title
/// 14 semibold, port 13, caption 12.5, small 12, tag 12 monospaced), times `scale`.
///
/// CTFont is an immutable Core Foundation object and safe to share between threads, which is why
/// this class is `@unchecked Sendable`: nothing in it changes after `init`.
public final class FaceFonts: @unchecked Sendable {
    public let scale: CGFloat
    let title: CTFont
    let port: CTFont
    let caption: CTFont
    let small: CTFont
    let mono: CTFont
    let italic: CTFont

    public init(scale: CGFloat = 1) {
        self.scale = scale
        func ui(_ kind: CTFontUIFontType, _ size: CGFloat, _ fallback: String) -> CTFont {
            CTFontCreateUIFontForLanguage(kind, size * scale, nil) ?? CTFontCreateWithName(fallback as CFString, size * scale, nil)
        }
        title = ui(.emphasizedSystem, 14, "Helvetica-Bold")
        port = ui(.system, 13, "Helvetica")
        caption = ui(.system, 12.5, "Helvetica")
        small = ui(.system, 12, "Helvetica")
        mono = ui(.userFixedPitch, 12, "Menlo")
        italic = CTFontCreateCopyWithSymbolicTraits(port, 0, nil, .traitItalic, .traitItalic) ?? port
    }

    public static let standard = FaceFonts()
}

/// A run of text with a font and colour, as Core Text lays it out.
struct TextLine {
    let line: CTLine
    let width: CGFloat
    let ascent: CGFloat
    let descent: CGFloat

    init(_ s: String, _ font: CTFont, _ color: CGColor? = nil) {
        var attrs: [NSAttributedString.Key: Any] = [NSAttributedString.Key(kCTFontAttributeName as String): font]
        if let color { attrs[NSAttributedString.Key(kCTForegroundColorAttributeName as String)] = color }
        line = CTLineCreateWithAttributedString(NSAttributedString(string: s, attributes: attrs))
        var a: CGFloat = 0
        var d: CGFloat = 0
        width = CGFloat(CTLineGetTypographicBounds(line, &a, &d, nil))
        ascent = a
        descent = d
    }

    /// The same text cut to `max` wide with an ellipsis at the end.
    init(_ s: String, _ font: CTFont, _ color: CGColor?, max: CGFloat) {
        let full = TextLine(s, font, color)
        if full.width <= max || max <= 0 {
            self = full
            return
        }
        let dots = TextLine("…", font, color)
        if let cut = CTLineCreateTruncatedLine(full.line, Double(max), .end, dots.line) {
            line = cut
            var a: CGFloat = 0
            var d: CGFloat = 0
            width = CGFloat(CTLineGetTypographicBounds(cut, &a, &d, nil))
            ascent = a
            descent = d
        } else {
            self = full
        }
    }

    /// Width of `s` in `font`.
    static func width(_ s: String, _ font: CTFont) -> CGFloat { TextLine(s, font).width }
}
