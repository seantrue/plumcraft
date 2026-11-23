// Where a row's field editor goes on screen (N4 slice 2), the same rule on both platforms: over
// the row's widget at the canvas's zoom, so the native control sits where the value was drawn and
// the text in it is the size the face's is. When the canvas is zoomed out so far that the field's
// text would be too small to read (or to hit with a finger), the editor is drawn at the smallest
// legible size instead, centred on the row and floating over the face (with a shadow), so editing
// never moves the view; the row stays visible under it. A field near the view's edge is moved in
// so it is wholly on screen.
//
// Pure: the hosts call it on every viewport change while an editor is open; tests call it with
// fixed numbers.

import CoreGraphics
import PlumCraftModels

public struct EditorPlacement: Equatable, Sendable {
    /// The control's frame in view points.
    public var frame: CGRect
    /// Its text size in points.
    public var fontSize: CGFloat
    /// Drawn bigger than the row (zoomed far out): the host gives it a shadow.
    public var floating: Bool

    /// The face's own text size in graph units (FaceFonts' port and mono sizes).
    public static let faceFont: CGFloat = 11
    /// The smallest legible text, points: a pointer's, and a finger's (iPad).
    public static func minFont(touch: Bool) -> CGFloat { touch ? 11 : 10 }
    /// The smallest field height, points (a finger's target on the iPad).
    public static func minHeight(touch: Bool) -> CGFloat { touch ? 32 : 20 }
    /// A text field's smallest width when floating, points.
    public static func minWidth(touch: Bool) -> CGFloat { touch ? 220 : 170 }

    public static func make(row: CGRect, kind: WidgetWord?, viewport v: Viewport, viewSize: CGSize, touch: Bool) -> EditorPlacement {
        let z = max(v.zoom, 0.01)
        let onScreen = CGRect(x: row.minX * z + v.pan.x, y: row.minY * z + v.pan.y, width: row.width * z, height: row.height * z)
        var font = faceFont * z
        let minFont = minFont(touch: touch)
        let floating = font < minFont
        var frame = onScreen
        let lines: CGFloat = kind == .multiline ? 4 : 1
        if floating {
            font = minFont
            let w = max(onScreen.width, kind == .toggle || kind == .color ? 44 : minWidth(touch: touch))
            let h = max(onScreen.height, minHeight(touch: touch) + (lines - 1) * font * 1.25)
            frame = CGRect(x: onScreen.midX - w / 2, y: onScreen.midY - h / 2, width: w, height: h)
        } else {
            font = min(font, 40)
            let h = max(onScreen.height, minHeight(touch: touch))
            frame = CGRect(x: onScreen.minX, y: onScreen.midY - h / 2, width: onScreen.width, height: h)
            if kind == .multiline { frame.size.height = max(onScreen.height, h) }
            // a path's field leaves its Browse button (B133: the square at the right end, the
            // row's height) uncovered, so a tap on it while the text is edited still browses
            if kind == .path { frame.size.width = max(onScreen.width - min(onScreen.height, onScreen.width / 2), 20) }
        }
        // wholly in the view (8 points in), when it fits
        let bounds = CGRect(origin: .zero, size: viewSize).insetBy(dx: 8, dy: 8)
        if bounds.width > frame.width {
            frame.origin.x = min(max(frame.minX, bounds.minX), bounds.maxX - frame.width)
        }
        if bounds.height > frame.height {
            frame.origin.y = min(max(frame.minY, bounds.minY), bounds.maxY - frame.height)
        }
        return EditorPlacement(frame: frame.integral, fontSize: (font * 2).rounded() / 2, floating: floating)
    }

    /// The pan that brings `frame` (view points) above `keyboardTop` (the on-screen keyboard's
    /// top, view points) with a margin: zero when it is above already.
    public static func lift(_ frame: CGRect, keyboardTop: CGFloat, margin: CGFloat = 16) -> CGFloat {
        let over = frame.maxY + margin - keyboardTop
        return over > 0 ? -over : 0
    }
}

/// What a script or a test does to the field editor open over a row, the same on both platforms
/// (each `FieldEditorHost` is one): what a person would type, the keys, a colour picked.
@MainActor
public protocol FieldEditing: AnyObject {
    /// The field holds `text` now.
    func type(_ text: String)
    /// Return (the Mac) or Done (the iPad): commit and close.
    func commitField()
    /// Tab or Shift-Tab: commit and go on.
    func pressTab(back: Bool)
    /// Escape: close, setting nothing.
    func pressEscape()
    /// The colour picker closed on `color`.
    func commitColor(_ color: CGColor)
}
