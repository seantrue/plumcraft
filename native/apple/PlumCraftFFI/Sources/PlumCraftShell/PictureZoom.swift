// B159: a pop-out picture's zoom and view, kept by the shell in the picture's own terms so that
// stepping through a list's items keeps them. The core keeps Fit and 1:1 (`viewer.zoom`) and
// sends every zoom asked of the pop-out (`popout.get`'s `zoomAsk`); a pinch, a scroll and a pan
// are the shell's (docs/shell-interface.md 13.2.1, "where the line falls").
//
// The rule: within one pop-out, a zoom and view, once made, stay through every item: the same
// scale (screen points per pixel of the value) and the same centre as a fraction of the picture,
// so items of one size show the same region at the same zoom, and an item of another size the
// same zoom about the same relative point. With nothing kept the picture fits (or shows 1:1 when
// the core says so). Fit or 1:1 asked again forgets what was kept.

import CoreGraphics
import Foundation
import PlumCraftModels

/// A picture's zoom and view, in its own terms.
public struct PictureZoom: Equatable, Sendable {
    /// Screen points per pixel of the value.
    public var scale: Double
    /// The point at the middle of the view, as a fraction of the picture's width and height.
    public var centre: CGPoint

    public init(scale: Double, centre: CGPoint) {
        self.scale = scale
        self.centre = centre
    }

    /// The smallest and largest scales a viewer takes.
    public static let minScale = 0.01
    public static let maxScale = 64.0

    /// The scale that fits `picture` (value pixels) in `content` (points), at most 8: nil while the
    /// view isn't laid out yet (a zero size), which is when a fit can't be known.
    public static func fit(content: CGSize, picture: CGSize) -> Double? {
        guard content.width > 0, content.height > 0, picture.width > 0, picture.height > 0 else { return nil }
        return min(min(content.width / picture.width, content.height / picture.height), 8)
    }

    /// What a zoom ask (`zoomAsk`) makes of the view: a kept zoom, or nil to forget it (fit and
    /// 1:1 are then the core's `fit`). `current` is the view shown now (its scale and centre),
    /// `backing` the screen's pixels per point, `picture` the value's size in pixels.
    public static func asked(_ ask: JSONValue, current: PictureZoom, backing: Double, picture: CGSize) -> PictureZoom? {
        var z = current
        switch ask["to"] {
        case .string("fit"), .string("1:1"): return nil
        case .string("in"): z.scale = current.scale * 2
        case .string("out"): z.scale = current.scale / 2
        case let n where n.double != nil: z.scale = (n.double ?? 1) / max(backing, 1)
        default: break
        }
        if let at = ask["at"].array, at.count == 2, let r = at[0].double, let c = at[1].double, picture.width > 0, picture.height > 0 {
            z.centre = CGPoint(x: min(max((c + 0.5) / picture.width, 0), 1), y: min(max((r + 0.5) / picture.height, 0), 1))
        }
        z.scale = min(max(z.scale, minScale), maxScale)
        return z
    }

    /// The view's origin (points, in a document laid out at `scale`) that puts `centre` in the
    /// middle of a `content`-sized view, held inside the document where it is larger than the view.
    public func origin(content: CGSize, picture: CGSize) -> CGPoint {
        let doc = CGSize(width: picture.width * scale, height: picture.height * scale)
        func one(_ c: CGFloat, _ d: CGFloat, _ v: CGFloat) -> CGFloat { d <= v ? (d - v) / 2 : min(max(c * d - v / 2, 0), d - v) }
        return CGPoint(x: one(centre.x, doc.width, content.width), y: one(centre.y, doc.height, content.height))
    }

    /// The view shown: `origin` (points in the scaled document, top-left) of a `content`-sized view.
    public static func shown(scale: Double, origin: CGPoint, content: CGSize, picture: CGSize) -> PictureZoom {
        let w = max(picture.width * scale, 1), h = max(picture.height * scale, 1)
        let c = CGPoint(x: min(max((origin.x + content.width / 2) / w, 0), 1), y: min(max((origin.y + content.height / 2) / h, 0), 1))
        return PictureZoom(scale: scale, centre: c)
    }

    /// For a status file: the scale (also as a percentage), the centre, and whether it was kept.
    public func facts(kept: Bool, fit: Double?) -> JSONValue {
        var o: [String: JSONValue] = ["scale": .double((scale * 1e4).rounded() / 1e4), "percent": .double((scale * 1000).rounded() / 10),
                                      "centre": [.double((centre.x * 1e4).rounded() / 1e4), .double((centre.y * 1e4).rounded() / 1e4)], "kept": .bool(kept)]
        if let fit { o["fit"] = .double((fit * 1e4).rounded() / 1e4) }
        return .object(o)
    }
}
