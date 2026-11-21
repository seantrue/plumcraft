// The rule a scripted launch (`--scratch-state`) starts by (maintainer, 2026-11-21: "refuse anything
// outside the scratch root"): every place the app would write is inside the scratch root the
// launch was given or the system's temporary folder. It names no volume or store: a location
// is refused for where it resolves, not for what it is called.

import Foundation

public enum ScratchGuard {
    /// One place the app would write, by the name the person set it under (an environment
    /// variable, an argument, or the folder's role), and its path. A nil or empty path is unset:
    /// refused with `ifUnset` (what it would fall back to, and why that refuses), or allowed when
    /// `ifUnset` is nil (nothing is written there then).
    public struct Location: Sendable, Equatable {
        public var name: String
        public var path: String?
        public var ifUnset: String?

        public init(_ name: String, _ path: String?, ifUnset: String? = nil) {
            self.name = name
            self.path = path
            self.ifUnset = ifUnset
        }
    }

    /// Why the launch must not start, or nil when every location is inside one of `allowed`.
    /// Both sides are compared resolved (`resolved(_:)`); an allowed folder that resolves to `/`
    /// allows nothing.
    public static func refusal(_ locations: [Location], inside allowed: [String]) -> String? {
        var bases: [String] = []
        for b in allowed where !b.isEmpty {
            let r = resolved(b)
            if r != "/" && !bases.contains(r) { bases.append(r) }
        }
        for l in locations {
            guard let p = l.path, !p.isEmpty else {
                if let why = l.ifUnset { return "--scratch-state refuses: \(l.name) is unset, so \(why)" }
                continue
            }
            let r = resolved(p)
            if !bases.contains(where: { isInside(r, $0) }) {
                let shown = r == p ? p : "\(p) (\(r))"
                return "--scratch-state refuses \(l.name)=\(shown): it is outside the scratch root and the system's "
                    + "temporary folder (\(bases.joined(separator: ", "))), and a scripted launch writes nowhere else"
            }
        }
        return nil
    }

    /// The system's temporary folders: `TMPDIR` when it is set, Foundation's temporary directory,
    /// the per-user one the system gives, and `/tmp`.
    public static func temporaryFolders(_ env: [String: String] = ProcessInfo.processInfo.environment) -> [String] {
        var out: [String] = []
        if let t = env["TMPDIR"], t.hasPrefix("/") { out.append(t) }
        out.append(NSTemporaryDirectory())
        var buf = [CChar](repeating: 0, count: Int(PATH_MAX))
        if confstr(_CS_DARWIN_USER_TEMP_DIR, &buf, buf.count) > 0 {
            out.append(String(decoding: buf.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }, as: UTF8.self))
        }
        out.append("/tmp")
        return out
    }

    /// `path` as the file system will reach it: absolute (against the current folder), `.` and
    /// `..` taken out, symlinks resolved as far as the path exists (the rest, not made yet, is
    /// appended as it is), so `/var` and `/tmp` come out as `/private/var` and `/private/tmp`,
    /// and with no trailing slash.
    public static func resolved(_ path: String) -> String {
        let p = URL(fileURLWithPath: path).standardizedFileURL.path
        var head = p
        var tail: [String] = []
        while !head.isEmpty {
            if let r = realpath(head, nil) {
                var out = String(cString: r)
                free(r)
                for c in tail.reversed() { out = out.hasSuffix("/") ? out + c : out + "/" + c }
                return out
            }
            if head == "/" { break }
            tail.append((head as NSString).lastPathComponent)
            head = (head as NSString).deletingLastPathComponent
        }
        return p
    }

    /// Whether resolved `path` is `base` or under it (`/t/ab` is not under `/t/a`).
    public static func isInside(_ path: String, _ base: String) -> Bool {
        path == base || path.hasPrefix(base.hasSuffix("/") ? base : base + "/")
    }
}
