// A graph's file name on the Apple shells (FR2, docs/shell-interface.md 12.23): `.plum` for new
// graphs, `.json` still opened and saved back as it is. The rule is the core's
// (`plumcraft_session::graph_file`, `graph.fileTypes`): `ShellCore.open` reads it once, and these
// are its values until then. The type identifier is the one each app's Info.plist declares, and an
// application built on the shell declares too.

import Foundation
import PlumCraft
import PlumCraftModels
import Synchronization

public enum GraphFiles {
    /// The uniform type of a `.plum` graph, conforming to `public.json`. Plummer exports it; an
    /// application built on the shell imports it (or exports it, when it ships alone) in its own
    /// Info.plist and lists it in its document types.
    public static let typeIdentifier = "com.swapwizard.plummer.graph"

    /// What the core says a graph's file is named.
    public struct Rule: Sendable, Equatable {
        /// The extension a new graph gets, without its dot.
        public var defaultExtension: String
        /// Every extension a graph's file has, the default first.
        public var extensions: [String]
        /// A new graph's stem before it is named ("untitled").
        public var untitled: String

        /// The core's rule as it stands (`graph.fileTypes`), before the core has said it.
        public static let core = Rule(defaultExtension: "plum", extensions: ["plum", "json"], untitled: "untitled")

        /// `graph.fileTypes`'s reply; nil when it isn't one.
        public init?(reply v: JSONValue) {
            guard let d = v["default"].string, !d.isEmpty,
                  let exts = v["extensions"].array?.compactMap(\.string), exts.contains(d) else { return nil }
            self.init(defaultExtension: d, extensions: exts, untitled: v["untitled"].string ?? "untitled")
        }

        public init(defaultExtension: String, extensions: [String], untitled: String) {
            self.defaultExtension = defaultExtension; self.extensions = extensions; self.untitled = untitled
        }
    }

    private static let rule = Mutex(Rule.core)

    /// The rule in force.
    public static var current: Rule { rule.withLock { $0 } }

    /// Take the core's rule (`ShellCore.open`, once); a reply that isn't one leaves the rule as it was.
    public static func adopt(_ reply: JSONValue) {
        if let r = Rule(reply: reply) { rule.withLock { $0 = r } }
    }

    /// Ask `core` for its rule and take it.
    public static func load(from core: PlumCore, doc: UInt64? = nil) async {
        if let r = try? await core.command("graph.fileTypes", [:], doc: doc) { adopt(r) }
    }

    /// Is `url` a graph's file by its name (`.plum` or `.json`, any case)?
    public static func isGraph(_ url: URL) -> Bool {
        let ext = url.pathExtension.lowercased()
        return current.extensions.contains { $0.lowercased() == ext }
    }

    /// The `k`th new graph's file name, title-cased as the platform's own new documents are:
    /// "Untitled.plum", then "Untitled 2.plum" and on.
    public static func newName(_ k: Int = 1) -> String {
        let r = current
        let stem = r.untitled.prefix(1).uppercased() + r.untitled.dropFirst()
        return k <= 1 ? "\(stem).\(r.defaultExtension)" : "\(stem) \(k).\(r.defaultExtension)"
    }

    /// Where a graph opened from `url` is saved: `url` itself when it is a graph's file (a `.json`
    /// graph stays `.json`); else (a picture opened as its recipe, through an application's
    /// opener) a new graph's file of the same name beside it, never over it.
    public static func savePath(for url: URL) -> URL {
        isGraph(url) ? url : url.deletingPathExtension().appendingPathExtension(current.defaultExtension)
    }
}
