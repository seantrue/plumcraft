// The small types the contract's messages share: open enums (a word a newer core may add to),
// points and rects in graph units, colours, ports.

import Foundation

/// A word from the core that a newer core may add to: one this kit knows (`known`), or one it
/// doesn't (`unknown`, kept as sent). Decoding never fails on a new word (the hostile-input
/// rule: skip what you don't know), and it encodes back as itself.
///
///     switch face.run.state { case .known(.done)?: ...; case .unknown(let w)?: ...; ... }
public enum Open<Known>: Codable, Hashable, Sendable, CustomStringConvertible
where Known: RawRepresentable & Hashable & Sendable, Known.RawValue == String {
    case known(Known)
    case unknown(String)

    public init(raw: String) {
        if let k = Known(rawValue: raw) { self = .known(k) } else { self = .unknown(raw) }
    }

    public init(_ k: Known) { self = .known(k) }

    public var raw: String {
        switch self {
        case .known(let k): k.rawValue
        case .unknown(let s): s
        }
    }

    /// The known word, or nil for one this kit doesn't know.
    public var word: Known? { if case .known(let k) = self { k } else { nil } }

    public init(from decoder: Decoder) throws {
        self.init(raw: try decoder.singleValueContainer().decode(String.self))
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        try c.encode(raw)
    }

    public var description: String { raw }

    public static func == (a: Open, b: Known) -> Bool { a.word == b }
}

/// A point in graph units, `[x, y]` on the wire.
public struct Point: Codable, Hashable, Sendable {
    public var x: Double
    public var y: Double
    public init(_ x: Double, _ y: Double) { self.x = x; self.y = y }

    public init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        x = try c.decode(Double.self)
        y = try c.decode(Double.self)
        guard c.isAtEnd else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "a point is [x, y]")) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.unkeyedContainer()
        try c.encode(x)
        try c.encode(y)
    }

    public var json: JSONValue { [.double(x), .double(y)] }
}

/// A size in graph units, `[w, h]` on the wire.
public typealias Size = Point

/// A rect in graph units, `[x0, y0, x1, y1]` on the wire.
public struct Rect: Codable, Hashable, Sendable {
    public var x0: Double, y0: Double, x1: Double, y1: Double
    public init(_ x0: Double, _ y0: Double, _ x1: Double, _ y1: Double) {
        self.x0 = x0; self.y0 = y0; self.x1 = x1; self.y1 = y1
    }

    public var width: Double { x1 - x0 }
    public var height: Double { y1 - y0 }

    public init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        x0 = try c.decode(Double.self)
        y0 = try c.decode(Double.self)
        x1 = try c.decode(Double.self)
        y1 = try c.decode(Double.self)
        guard c.isAtEnd else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "a rect is [x0, y0, x1, y1]")) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.unkeyedContainer()
        for v in [x0, y0, x1, y1] { try c.encode(v) }
    }

    public var json: JSONValue { [.double(x0), .double(y0), .double(x1), .double(y1)] }
}

/// A colour, `[r, g, b, a]` bytes. Scene colours are premultiplied (8.3); theme tokens too.
public struct RGBA: Codable, Hashable, Sendable {
    public var r: UInt8, g: UInt8, b: UInt8, a: UInt8
    public init(_ r: UInt8, _ g: UInt8, _ b: UInt8, _ a: UInt8) { self.r = r; self.g = g; self.b = b; self.a = a }

    public init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        r = try c.decode(UInt8.self)
        g = try c.decode(UInt8.self)
        b = try c.decode(UInt8.self)
        a = try c.decode(UInt8.self)
        guard c.isAtEnd else { throw DecodingError.dataCorrupted(.init(codingPath: decoder.codingPath, debugDescription: "a colour is [r, g, b, a]")) }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.unkeyedContainer()
        for v in [r, g, b, a] { try c.encode(v) }
    }

    public static let clear = RGBA(0, 0, 0, 0)
}

/// A colour in the scene: a theme token by name (resolved with `theme.get`), or a colour of its
/// own (computed: a family's tint, a wire type's). Exactly one (8, question 13). Anything else
/// is `none`, drawn clear.
public enum Paint: Codable, Hashable, Sendable {
    case token(String)
    case color(RGBA)
    case none

    enum CodingKeys: String, CodingKey { case token, color }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let token = try c.decodeIfPresent(String.self, forKey: .token)
        let color = try c.decodeIfPresent(RGBA.self, forKey: .color)
        switch (token, color) {
        case (let t?, nil): self = .token(t)
        case (nil, let k?): self = .color(k)
        case (let t?, _?): self = .token(t)  // both: hostile; the token wins, as the core resolves it
        case (nil, nil): self = .none
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .token(let t): try c.encode(t, forKey: .token)
        case .color(let k): try c.encode(k, forKey: .color)
        case .none: break
        }
    }

    /// The colour, with `tokens` (a theme's) for a token; clear for a token the theme lacks.
    public func resolve(_ tokens: [String: RGBA]) -> RGBA {
        switch self {
        case .token(let t): tokens[t] ?? .clear
        case .color(let k): k
        case .none: .clear
        }
    }
}

/// A port of a graph node: `{node, port}`.
public struct PortRef: Codable, Hashable, Sendable {
    public var node: String
    public var port: String
    public init(node: String, port: String) { self.node = node; self.port = port }
    public var json: JSONValue { ["node": .string(node), "port": .string(port)] }
}

/// A net's key: its name, and the group it is local to (none: the top level).
public struct NetKey: Codable, Hashable, Sendable {
    public var name: String
    public var group: String?
    public init(name: String, group: String? = nil) { self.name = name; self.group = group }
}

/// A modifier held during a gesture or named by a shortcut. `primary` is ⌘ on Apple platforms.
public enum ModifierWord: String, Sendable, CaseIterable {
    case primary, ctrl, shift, alt
}

public typealias Modifier = Open<ModifierWord>
