// One rule for where controls go when the window is short and wide (B135, the maintainer: "Fix
// b135 with that", a column of readable width at one side with the canvas still visible beside
// it, the way the iPad's inspector works). Every presentation of controls asks here, not per
// screen.
//
//   side column   the window's height is compact (an iPhone turned to landscape) and it is wider
//                 than tall: the controls in a column at the trailing edge, the canvas live beside
//   popover       regular width (an iPad): from what it is about, as before
//   sheet         compact width with regular height (an iPhone upright, a narrow iPad window)

import UIKit

enum ReadableColumn {
    /// The column's width: readable, never the whole width: 45% of the window, at least 320 and
    /// at most 420 points (an iPad's inspector popover is 420).
    static func width(in size: CGSize) -> CGFloat {
        min(420, max(320, (size.width * 0.45).rounded()))
    }

    /// A side column rather than a sheet: a short window wider than it is tall.
    static func side(_ traits: UITraitCollection, size: CGSize) -> Bool {
        traits.verticalSizeClass == .compact && size.width > size.height
    }
}
