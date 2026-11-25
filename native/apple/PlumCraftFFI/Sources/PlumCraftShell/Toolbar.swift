// The toolbar from the core (N4 slice 3; golden rule 4, the maintainer: "No exceptions to the
// rule"): every button's command, label, enabled state, reason and check mark come from the core's
// toolbar model (`menu.get {context: {kind: "toolbar"}}`); a platform shows each with its own
// symbol for the core's icon (a Lucide name), the native look rather than the core's line art.

import Foundation
import PlumCraftModels

public enum ToolbarWords {
    /// The SF Symbol for a core icon name; a generic one for a name this shell doesn't know.
    public static func symbol(_ icon: String?) -> String {
        switch icon {
        case "plus"?: "plus"
        case "scan"?: "arrow.up.left.and.down.right.magnifyingglass"
        case "play"?: "play.fill"
        case "square-play"?: "play.square"
        case "square"?: "stop.fill"
        case "square-dashed-mouse-pointer"?: "rectangle.dashed"
        case "share"?: "square.and.arrow.up"
        default: "square.dashed"
        }
    }

    /// A button's tooltip or accessibility hint: the reason it is disabled, else its help.
    public static func help(_ it: MenuItem) -> String {
        it.enabled ? (it.hover ?? it.label) : (it.reason ?? it.hover ?? it.label)
    }

    /// The buttons, without separators, each with its stable identifier (its command).
    public static func buttons(_ items: [MenuItem]) -> [MenuItem] {
        items.filter { !$0.separator && $0.command != nil }
    }
}
