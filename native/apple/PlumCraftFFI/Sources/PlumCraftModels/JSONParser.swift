// JSON bytes straight to a JSONValue, without Foundation's objects in between. Going through
// JSONSerialization and bridging every NSDictionary, NSString and NSNumber cost two to three times
// the decode itself on a whole scene (N3 slice 2's measurements), so the feed is parsed here.
//
// Hostile input: every error throws (never traps); nesting deeper than `maxDepth` is refused
// before the stack is; a string's bytes that aren't UTF-8 are replaced, as Swift's String does.

import Foundation

enum JSONParser {
    /// Deeper than this is refused (the core nests a dozen levels or so; a widget's value more
    /// only if an engine sends it). Bounded well inside a cooperative-pool thread's stack (512 KiB),
    /// where a feed is parsed, in a debug build too: 512 levels overflowed it.
    static let maxDepth = 128

    static func parse(_ data: Data) throws -> JSONValue {
        try data.withUnsafeBytes { raw in
            var p = Cursor(bytes: raw.bindMemory(to: UInt8.self))
            p.skipSpace()
            let v = try p.value(depth: 0)
            p.skipSpace()
            if p.at < p.bytes.count { throw p.fail("trailing characters") }
            return v
        }
    }

    struct Cursor {
        let bytes: UnsafeBufferPointer<UInt8>
        var at = 0

        init(bytes: UnsafeBufferPointer<UInt8>) { self.bytes = bytes }

        func fail(_ why: String) -> JSONError { JSONError("not JSON: \(why) at byte \(at)") }

        mutating func skipSpace() {
            while at < bytes.count {
                switch bytes[at] {
                case 0x20, 0x0A, 0x0D, 0x09: at += 1
                default: return
                }
            }
        }

        mutating func value(depth: Int) throws -> JSONValue {
            guard at < bytes.count else { throw fail("unexpected end") }
            switch bytes[at] {
            case UInt8(ascii: "{"): return try object(depth: depth + 1)
            case UInt8(ascii: "["): return try array(depth: depth + 1)
            case UInt8(ascii: "\""): return .string(try string())
            case UInt8(ascii: "t"): try word("true"); return .bool(true)
            case UInt8(ascii: "f"): try word("false"); return .bool(false)
            case UInt8(ascii: "n"): try word("null"); return .null
            default: return try number()
            }
        }

        mutating func word(_ w: String) throws {
            let u = Array(w.utf8)
            guard at + u.count <= bytes.count else { throw fail("unexpected end") }
            for (k, c) in u.enumerated() where bytes[at + k] != c { throw fail("not a value") }
            at += u.count
        }

        mutating func object(depth: Int) throws -> JSONValue {
            if depth > JSONParser.maxDepth { throw fail("nested too deep") }
            at += 1
            var o: [String: JSONValue] = [:]
            skipSpace()
            if at < bytes.count, bytes[at] == UInt8(ascii: "}") {
                at += 1
                return .object(o)
            }
            while true {
                skipSpace()
                guard at < bytes.count, bytes[at] == UInt8(ascii: "\"") else { throw fail("an object's key must be a string") }
                let k = try string()
                skipSpace()
                guard at < bytes.count, bytes[at] == UInt8(ascii: ":") else { throw fail("wanted ':'") }
                at += 1
                skipSpace()
                o[k] = try value(depth: depth)
                skipSpace()
                guard at < bytes.count else { throw fail("unexpected end") }
                if bytes[at] == UInt8(ascii: ",") {
                    at += 1
                } else if bytes[at] == UInt8(ascii: "}") {
                    at += 1
                    return .object(o)
                } else {
                    throw fail("wanted ',' or '}'")
                }
            }
        }

        mutating func array(depth: Int) throws -> JSONValue {
            if depth > JSONParser.maxDepth { throw fail("nested too deep") }
            at += 1
            var a: [JSONValue] = []
            skipSpace()
            if at < bytes.count, bytes[at] == UInt8(ascii: "]") {
                at += 1
                return .array(a)
            }
            while true {
                skipSpace()
                a.append(try value(depth: depth))
                skipSpace()
                guard at < bytes.count else { throw fail("unexpected end") }
                if bytes[at] == UInt8(ascii: ",") {
                    at += 1
                } else if bytes[at] == UInt8(ascii: "]") {
                    at += 1
                    return .array(a)
                } else {
                    throw fail("wanted ',' or ']'")
                }
            }
        }

        mutating func string() throws -> String {
            at += 1
            let start = at
            // the common case: no escapes, the bytes as they are
            while at < bytes.count {
                let c = bytes[at]
                if c == UInt8(ascii: "\"") {
                    let s = String(decoding: UnsafeBufferPointer(rebasing: bytes[start..<at]), as: UTF8.self)
                    at += 1
                    return s
                }
                if c == UInt8(ascii: "\\") { break }
                if c < 0x20 { throw fail("a control character in a string") }
                at += 1
            }
            var out = [UInt8](UnsafeBufferPointer(rebasing: bytes[start..<at]))
            while at < bytes.count {
                let c = bytes[at]
                if c == UInt8(ascii: "\"") {
                    at += 1
                    return String(decoding: out, as: UTF8.self)
                }
                if c < 0x20 { throw fail("a control character in a string") }
                if c != UInt8(ascii: "\\") {
                    out.append(c)
                    at += 1
                    continue
                }
                at += 1
                guard at < bytes.count else { throw fail("unexpected end") }
                let e = bytes[at]
                at += 1
                switch e {
                case UInt8(ascii: "\""): out.append(0x22)
                case UInt8(ascii: "\\"): out.append(0x5C)
                case UInt8(ascii: "/"): out.append(0x2F)
                case UInt8(ascii: "b"): out.append(0x08)
                case UInt8(ascii: "f"): out.append(0x0C)
                case UInt8(ascii: "n"): out.append(0x0A)
                case UInt8(ascii: "r"): out.append(0x0D)
                case UInt8(ascii: "t"): out.append(0x09)
                case UInt8(ascii: "u"):
                    var scalar = try hex4()
                    if (0xD800..<0xDC00).contains(scalar) {
                        // a high surrogate wants its low one
                        if at + 1 < bytes.count, bytes[at] == UInt8(ascii: "\\"), bytes[at + 1] == UInt8(ascii: "u") {
                            at += 2
                            let low = try hex4()
                            if (0xDC00..<0xE000).contains(low) {
                                scalar = 0x10000 + ((scalar - 0xD800) << 10) + (low - 0xDC00)
                            } else {
                                scalar = 0xFFFD
                            }
                        } else {
                            scalar = 0xFFFD
                        }
                    } else if (0xDC00..<0xE000).contains(scalar) {
                        scalar = 0xFFFD
                    }
                    let u = Unicode.Scalar(scalar) ?? "\u{FFFD}"
                    out.append(contentsOf: String(Character(u)).utf8)
                default:
                    throw fail("a bad escape")
                }
            }
            throw fail("an unterminated string")
        }

        mutating func hex4() throws -> UInt32 {
            guard at + 4 <= bytes.count else { throw fail("unexpected end") }
            var v: UInt32 = 0
            for _ in 0..<4 {
                let c = bytes[at]
                let d: UInt32
                switch c {
                case UInt8(ascii: "0")...UInt8(ascii: "9"): d = UInt32(c - UInt8(ascii: "0"))
                case UInt8(ascii: "a")...UInt8(ascii: "f"): d = UInt32(c - UInt8(ascii: "a") + 10)
                case UInt8(ascii: "A")...UInt8(ascii: "F"): d = UInt32(c - UInt8(ascii: "A") + 10)
                default: throw fail("a bad \\u escape")
                }
                v = v << 4 | d
                at += 1
            }
            return v
        }

        mutating func number() throws -> JSONValue {
            let start = at
            var negative = false
            if bytes[at] == UInt8(ascii: "-") {
                negative = true
                at += 1
            }
            let digits = at
            var whole: UInt64 = 0
            var overflow = false
            while at < bytes.count, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(bytes[at]) {
                let (m, o1) = whole.multipliedReportingOverflow(by: 10)
                let (s, o2) = m.addingReportingOverflow(UInt64(bytes[at] - UInt8(ascii: "0")))
                overflow = overflow || o1 || o2
                whole = s
                at += 1
            }
            if at == digits { throw fail("not a value") }
            if bytes[digits] == UInt8(ascii: "0") && at - digits > 1 { throw fail("a number with a leading zero") }
            var fraction = false
            if at < bytes.count, bytes[at] == UInt8(ascii: ".") {
                fraction = true
                at += 1
                let f = at
                while at < bytes.count, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(bytes[at]) { at += 1 }
                if at == f { throw fail("a fraction without digits") }
            }
            if at < bytes.count, bytes[at] == UInt8(ascii: "e") || bytes[at] == UInt8(ascii: "E") {
                fraction = true
                at += 1
                if at < bytes.count, bytes[at] == UInt8(ascii: "+") || bytes[at] == UInt8(ascii: "-") { at += 1 }
                let e = at
                while at < bytes.count, (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(bytes[at]) { at += 1 }
                if at == e { throw fail("an exponent without digits") }
            }
            if !fraction && !overflow {
                if negative {
                    if whole <= UInt64(Int64.max) { return .int(-Int64(whole)) }
                    if whole == UInt64(Int64.max) + 1 { return .int(Int64.min) }
                } else {
                    return whole <= UInt64(Int64.max) ? .int(Int64(whole)) : .uint(whole)
                }
            }
            let text = String(decoding: UnsafeBufferPointer(rebasing: bytes[start..<at]), as: UTF8.self)
            guard let d = Double(text) else { throw fail("a number out of range") }
            return .double(d)
        }
    }
}
