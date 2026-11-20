// Any JSON, as a Sendable value: what crosses the boundary before it is typed, what the
// conformance runner compares, and what a widget's value or a request's params are.

import Foundation

/// A JSON value. Whole numbers are kept apart from fractions, because the core refuses `0.0`
/// where it wants a count (`palette.items {limit: 0}` is a different request from `0.5`).
public enum JSONValue: Sendable, Hashable {
    case null
    case bool(Bool)
    case int(Int64)
    /// A whole number past Int64 (a 64-bit nid or hash the core sends unsigned).
    case uint(UInt64)
    case double(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])
}

/// Bad JSON, or a value that isn't the JSON asked for.
public struct JSONError: Error, CustomStringConvertible, Sendable {
    public let description: String
    public init(_ d: String) { description = d }
}

// MARK: - Literals

extension JSONValue: ExpressibleByNilLiteral, ExpressibleByBooleanLiteral, ExpressibleByIntegerLiteral,
    ExpressibleByFloatLiteral, ExpressibleByStringLiteral, ExpressibleByArrayLiteral, ExpressibleByDictionaryLiteral
{
    public init(nilLiteral: ()) { self = .null }
    public init(booleanLiteral value: Bool) { self = .bool(value) }
    public init(integerLiteral value: Int64) { self = .int(value) }
    public init(floatLiteral value: Double) { self = .double(value) }
    public init(stringLiteral value: String) { self = .string(value) }
    public init(arrayLiteral elements: JSONValue...) { self = .array(elements) }
    public init(dictionaryLiteral elements: (String, JSONValue)...) {
        var o: [String: JSONValue] = [:]
        for (k, v) in elements { o[k] = v }
        self = .object(o)
    }
}

// MARK: - Reading

extension JSONValue {
    public var isNull: Bool { if case .null = self { true } else { false } }
    public var bool: Bool? { if case .bool(let b) = self { b } else { nil } }
    public var string: String? { if case .string(let s) = self { s } else { nil } }
    public var array: [JSONValue]? { if case .array(let a) = self { a } else { nil } }
    public var object: [String: JSONValue]? { if case .object(let o) = self { o } else { nil } }

    /// Any number, as a Double.
    public var double: Double? {
        switch self {
        case .int(let i): Double(i)
        case .uint(let u): Double(u)
        case .double(let d): d
        default: nil
        }
    }

    /// A whole number that fits (a fraction is not one).
    public var int64: Int64? {
        switch self {
        case .int(let i): i
        case .uint(let u): Int64(exactly: u)
        case .double(let d): d.rounded() == d ? Int64(exactly: d) : nil
        default: nil
        }
    }

    public var uint64: UInt64? {
        switch self {
        case .int(let i): UInt64(exactly: i)
        case .uint(let u): u
        case .double(let d): d.rounded() == d ? UInt64(exactly: d) : nil
        default: nil
        }
    }

    public var isNumber: Bool { double != nil }

    /// A member of an object (`.null` when there is none, or this isn't an object).
    public subscript(key: String) -> JSONValue {
        object?[key] ?? .null
    }

    /// An item of an array (`.null` past its end, or when this isn't an array).
    public subscript(index: Int) -> JSONValue {
        guard let a = array, index >= 0, index < a.count else { return .null }
        return a[index]
    }

    /// What a JSON pointer (RFC 6901: `/a/0/b`, `~1` for `/`, `~0` for `~`) names; nil when
    /// nothing is there. The empty pointer is the value itself.
    public func pointer(_ p: String) -> JSONValue? {
        if p.isEmpty { return self }
        guard p.hasPrefix("/") else { return nil }
        var at = self
        for raw in p.dropFirst().split(separator: "/", omittingEmptySubsequences: false) {
            let token = raw.replacingOccurrences(of: "~1", with: "/").replacingOccurrences(of: "~0", with: "~")
            switch at {
            case .object(let o):
                guard let next = o[token] else { return nil }
                at = next
            case .array(let a):
                guard let i = Int(token), i >= 0, i < a.count else { return nil }
                at = a[i]
            default:
                return nil
            }
        }
        return at
    }
}

// MARK: - From and to bytes

extension JSONValue {
    /// Parse JSON bytes (any value at the top, as the core sends fragments too), straight to a
    /// JSONValue (JSONParser).
    public static func parse(_ data: Data) throws -> JSONValue {
        try JSONParser.parse(data)
    }

    /// Parse through JSONSerialization and bridge its objects: the reference the parser is
    /// tested against, and the slower path the measurements compare.
    public static func parseWithFoundation(_ data: Data) throws -> JSONValue {
        let any: Any
        do {
            any = try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        } catch {
            throw JSONError("not JSON: \(error.localizedDescription)")
        }
        return JSONValue(foundation: any)
    }

    public static func parse(_ text: String) throws -> JSONValue {
        try parse(Data(text.utf8))
    }

    /// From what JSONSerialization makes. Anything else is null (it makes nothing else).
    public init(foundation any: Any) {
        switch any {
        case let s as String:
            self = .string(s)
        case let n as NSNumber:
            self = JSONValue(number: n)
        case let a as [Any]:
            self = .array(a.map(JSONValue.init(foundation:)))
        case let o as [String: Any]:
            var out: [String: JSONValue] = [:]
            out.reserveCapacity(o.count)
            for (k, v) in o { out[k] = JSONValue(foundation: v) }
            self = .object(out)
        default:
            self = .null
        }
    }

    private init(number n: NSNumber) {
        if CFGetTypeID(n) == CFBooleanGetTypeID() {
            self = .bool(n.boolValue)
            return
        }
        if CFNumberIsFloatType(n) {
            self = .double(n.doubleValue)
            return
        }
        // a whole number: Int64 when it fits, else unsigned (JSONSerialization keeps a u64
        // past Int64 as an unsigned NSNumber, "Q")
        if n.objCType.pointee == UInt8(ascii: "Q") {
            let u = n.uint64Value
            self = Int64(exactly: u).map(JSONValue.int) ?? .uint(u)
        } else {
            self = .int(n.int64Value)
        }
    }

    /// Compact JSON text. Non-finite numbers, which JSON has none of, are written as null.
    public func serialized() -> Data {
        var out = ""
        out.reserveCapacity(64)
        write(into: &out)
        return Data(out.utf8)
    }

    /// Compact JSON text, as a String.
    public var text: String {
        var out = ""
        write(into: &out)
        return out
    }

    private func write(into out: inout String) {
        switch self {
        case .null: out += "null"
        case .bool(let b): out += b ? "true" : "false"
        case .int(let i): out += String(i)
        case .uint(let u): out += String(u)
        case .double(let d):
            if !d.isFinite {
                out += "null"
            } else if d.rounded() == d, abs(d) < 1e15 {
                // a whole Double: written as a fraction so it reads back as one
                out += String(format: "%.1f", d)
            } else {
                out += "\(d)"
            }
        case .string(let s): JSONValue.quote(s, into: &out)
        case .array(let a):
            out += "["
            for (i, x) in a.enumerated() {
                if i > 0 { out += "," }
                x.write(into: &out)
            }
            out += "]"
        case .object(let o):
            out += "{"
            // sorted, so the same value always writes the same text
            for (i, k) in o.keys.sorted().enumerated() {
                if i > 0 { out += "," }
                JSONValue.quote(k, into: &out)
                out += ":"
                o[k, default: .null].write(into: &out)
            }
            out += "}"
        }
    }

    private static func quote(_ s: String, into out: inout String) {
        out += "\""
        for u in s.unicodeScalars {
            switch u {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case "\n": out += "\\n"
            case "\r": out += "\\r"
            case "\t": out += "\\t"
            case _ where u.value < 0x20:
                out += String(format: "\\u%04x", u.value)
            default:
                out.unicodeScalars.append(u)
            }
        }
        out += "\""
    }
}

extension JSONValue: CustomStringConvertible {
    public var description: String { text }
}

// MARK: - Codable

extension JSONValue: Codable {
    public init(from decoder: Decoder) throws {
        if let d = decoder as? JSONValueDecoder.Impl {
            d.tookWhole()
            self = d.value
            return
        }
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            self = .null
        } else if let b = try? c.decode(Bool.self) {
            self = .bool(b)
        } else if let i = try? c.decode(Int64.self) {
            self = .int(i)
        } else if let u = try? c.decode(UInt64.self) {
            self = .uint(u)
        } else if let d = try? c.decode(Double.self) {
            self = .double(d)
        } else if let s = try? c.decode(String.self) {
            self = .string(s)
        } else if let a = try? c.decode([JSONValue].self) {
            self = .array(a)
        } else {
            self = .object(try c.decode([String: JSONValue].self))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil()
        case .bool(let b): try c.encode(b)
        case .int(let i): try c.encode(i)
        case .uint(let u): try c.encode(u)
        case .double(let d): try c.encode(d)
        case .string(let s): try c.encode(s)
        case .array(let a): try c.encode(a)
        case .object(let o): try c.encode(o)
        }
    }
}

// MARK: - Comparing as the conformance suite does

extension JSONValue {
    /// Equal as the suite compares (`conformance::same`): numbers within 1e-3, whatever their
    /// kind; arrays item by item; objects with the same keys and like values.
    public func same(_ other: JSONValue) -> Bool {
        if let a = double, let b = other.double { return abs(a - b) <= 1.0e-3 }
        switch (self, other) {
        case (.array(let x), .array(let y)):
            return x.count == y.count && zip(x, y).allSatisfy { $0.same($1) }
        case (.object(let x), .object(let y)):
            return x.count == y.count && x.allSatisfy { k, v in y[k].map { v.same($0) } ?? false }
        default:
            return self == other
        }
    }
}
