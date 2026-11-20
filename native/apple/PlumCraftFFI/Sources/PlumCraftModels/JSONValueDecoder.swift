// A Decoder over a JSONValue: the feed is parsed once (JSONSerialization, the fast path N0
// measured) and each reply is typed from the parsed tree, with no second parse.
//
// It can also record every key a type asked for, so a test can find the fields a fixture has and
// the Swift types don't know (KeyRecorder, unknownFields): the check that the types haven't
// drifted from the contract. Unknown fields are never an error when decoding: a newer core may
// send more (docs/shell-interface.md 3.1, readers skip what they don't know).

import Foundation

/// Every key asked for while decoding, by where its object is (`/nodes/0/header`), and the
/// places decoded whole (a JSONValue: a widget's value, a command's params).
public final class KeyRecorder: @unchecked Sendable {
    // only touched by the decoder that owns it, on one thread at a time
    private(set) var asked: Set<String> = []
    private(set) var whole: Set<String> = []
    public init() {}

    func ask(_ path: String) { asked.insert(path) }
    func take(_ path: String) { whole.insert(path) }

    /// The fields in `value` that no type asked for, as paths (`/nodes/3/header/bogus`), sorted.
    public func unknownFields(in value: JSONValue) -> [String] {
        var out: [String] = []
        walk(value, "", &out)
        return out.sorted()
    }

    private func walk(_ v: JSONValue, _ path: String, _ out: inout [String]) {
        if whole.contains(path) { return }
        switch v {
        case .object(let o):
            for (k, x) in o {
                let p = path + "/" + KeyRecorder.escape(k)
                if !asked.contains(p) {
                    out.append(p)
                } else {
                    walk(x, p, &out)
                }
            }
        case .array(let a):
            for (i, x) in a.enumerated() { walk(x, path + "/\(i)", &out) }
        default:
            break
        }
    }

    static func escape(_ k: String) -> String {
        k.replacingOccurrences(of: "~", with: "~0").replacingOccurrences(of: "/", with: "~1")
    }
}

/// Decodes `Decodable` types from a JSONValue.
public struct JSONValueDecoder: Sendable {
    /// Record the keys asked for (tests); nil in use.
    public var recorder: KeyRecorder?

    public init(recorder: KeyRecorder? = nil) { self.recorder = recorder }

    public func decode<T: Decodable>(_ type: T.Type, from value: JSONValue) throws -> T {
        try T(from: Impl(value: value, path: [], recorder: recorder))
    }

    /// Parse and decode in one go.
    public func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
        try decode(type, from: JSONValue.parse(data))
    }

    struct Key: CodingKey {
        let stringValue: String
        let intValue: Int?
        init(stringValue: String) { self.stringValue = stringValue; intValue = nil }
        init(intValue: Int) { stringValue = String(intValue); self.intValue = intValue }
    }

    final class Impl: Decoder {
        let value: JSONValue
        let codingPath: [CodingKey]
        let recorder: KeyRecorder?
        var userInfo: [CodingUserInfoKey: Any] { [:] }

        init(value: JSONValue, path: [CodingKey], recorder: KeyRecorder?) {
            self.value = value
            codingPath = path
            self.recorder = recorder
        }

        var pointer: String { Impl.pointer(codingPath) }

        static func pointer(_ path: [CodingKey]) -> String {
            path.map { "/" + KeyRecorder.escape($0.stringValue) }.joined()
        }

        /// A JSONValue taken whole: nothing under it is unknown.
        func tookWhole() { recorder?.take(pointer) }

        func container<K: CodingKey>(keyedBy type: K.Type) throws -> KeyedDecodingContainer<K> {
            guard case .object(let o) = value else {
                throw DecodingError.typeMismatch([String: Any].self, .init(codingPath: codingPath, debugDescription: "wanted an object, got \(value.kindWord)"))
            }
            return KeyedDecodingContainer(Keyed<K>(object: o, decoder: self))
        }

        func unkeyedContainer() throws -> UnkeyedDecodingContainer {
            guard case .array(let a) = value else {
                throw DecodingError.typeMismatch([Any].self, .init(codingPath: codingPath, debugDescription: "wanted an array, got \(value.kindWord)"))
            }
            return Unkeyed(items: a, decoder: self)
        }

        func singleValueContainer() throws -> SingleValueDecodingContainer { Single(decoder: self) }

        func child(_ v: JSONValue, _ key: CodingKey) -> Impl {
            Impl(value: v, path: codingPath + [key], recorder: recorder)
        }
    }

    struct Keyed<K: CodingKey>: KeyedDecodingContainerProtocol {
        let object: [String: JSONValue]
        let decoder: Impl
        var codingPath: [CodingKey] { decoder.codingPath }

        var allKeys: [K] {
            // asking for every key (a dictionary) knows every key
            if let r = decoder.recorder { for k in object.keys { r.ask(decoder.pointer + "/" + KeyRecorder.escape(k)) } }
            return object.keys.compactMap { K(stringValue: $0) }
        }

        private func note(_ key: K) {
            decoder.recorder?.ask(decoder.pointer + "/" + KeyRecorder.escape(key.stringValue))
        }

        func contains(_ key: K) -> Bool {
            note(key)
            return object[key.stringValue] != nil
        }

        private func get(_ key: K) throws -> JSONValue {
            note(key)
            guard let v = object[key.stringValue] else {
                throw DecodingError.keyNotFound(key, .init(codingPath: codingPath, debugDescription: "no \(key.stringValue)"))
            }
            return v
        }

        func decodeNil(forKey key: K) throws -> Bool {
            note(key)
            return object[key.stringValue].map(\.isNull) ?? true
        }

        func decode(_ type: Bool.Type, forKey key: K) throws -> Bool { try Prim.bool(get(key), codingPath + [key]) }
        func decode(_ type: String.Type, forKey key: K) throws -> String { try Prim.string(get(key), codingPath + [key]) }
        func decode(_ type: Double.Type, forKey key: K) throws -> Double { try Prim.double(get(key), codingPath + [key]) }
        func decode(_ type: Float.Type, forKey key: K) throws -> Float { Float(try Prim.double(get(key), codingPath + [key])) }
        func decode(_ type: Int.Type, forKey key: K) throws -> Int { try Prim.int(get(key), codingPath + [key]) }
        func decode(_ type: Int8.Type, forKey key: K) throws -> Int8 { try Prim.int(get(key), codingPath + [key]) }
        func decode(_ type: Int16.Type, forKey key: K) throws -> Int16 { try Prim.int(get(key), codingPath + [key]) }
        func decode(_ type: Int32.Type, forKey key: K) throws -> Int32 { try Prim.int(get(key), codingPath + [key]) }
        func decode(_ type: Int64.Type, forKey key: K) throws -> Int64 { try Prim.int(get(key), codingPath + [key]) }
        func decode(_ type: UInt.Type, forKey key: K) throws -> UInt { try Prim.uint(get(key), codingPath + [key]) }
        func decode(_ type: UInt8.Type, forKey key: K) throws -> UInt8 { try Prim.uint(get(key), codingPath + [key]) }
        func decode(_ type: UInt16.Type, forKey key: K) throws -> UInt16 { try Prim.uint(get(key), codingPath + [key]) }
        func decode(_ type: UInt32.Type, forKey key: K) throws -> UInt32 { try Prim.uint(get(key), codingPath + [key]) }
        func decode(_ type: UInt64.Type, forKey key: K) throws -> UInt64 { try Prim.uint(get(key), codingPath + [key]) }

        func decode<T: Decodable>(_ type: T.Type, forKey key: K) throws -> T {
            try T(from: decoder.child(get(key), key))
        }

        func nestedContainer<NK: CodingKey>(keyedBy type: NK.Type, forKey key: K) throws -> KeyedDecodingContainer<NK> {
            try decoder.child(get(key), key).container(keyedBy: type)
        }

        func nestedUnkeyedContainer(forKey key: K) throws -> UnkeyedDecodingContainer {
            try decoder.child(get(key), key).unkeyedContainer()
        }

        func superDecoder() throws -> Decoder { decoder }

        func superDecoder(forKey key: K) throws -> Decoder {
            decoder.child(object[key.stringValue] ?? .null, key)
        }
    }

    struct Unkeyed: UnkeyedDecodingContainer {
        let items: [JSONValue]
        let decoder: Impl
        var codingPath: [CodingKey] { decoder.codingPath }
        var count: Int? { items.count }
        var isAtEnd: Bool { currentIndex >= items.count }
        private(set) var currentIndex = 0

        init(items: [JSONValue], decoder: Impl) {
            self.items = items
            self.decoder = decoder
        }

        private var key: Key { Key(intValue: currentIndex) }

        private mutating func next() throws -> JSONValue {
            guard currentIndex < items.count else {
                throw DecodingError.valueNotFound(JSONValue.self, .init(codingPath: codingPath + [key], debugDescription: "past the array's end"))
            }
            defer { currentIndex += 1 }
            return items[currentIndex]
        }

        mutating func decodeNil() throws -> Bool {
            guard currentIndex < items.count else { return true }
            if items[currentIndex].isNull {
                currentIndex += 1
                return true
            }
            return false
        }

        mutating func decode(_ type: Bool.Type) throws -> Bool { let p = codingPath + [key]; return try Prim.bool(next(), p) }
        mutating func decode(_ type: String.Type) throws -> String { let p = codingPath + [key]; return try Prim.string(next(), p) }
        mutating func decode(_ type: Double.Type) throws -> Double { let p = codingPath + [key]; return try Prim.double(next(), p) }
        mutating func decode(_ type: Float.Type) throws -> Float { let p = codingPath + [key]; return Float(try Prim.double(next(), p)) }
        mutating func decode(_ type: Int.Type) throws -> Int { let p = codingPath + [key]; return try Prim.int(next(), p) }
        mutating func decode(_ type: Int32.Type) throws -> Int32 { let p = codingPath + [key]; return try Prim.int(next(), p) }
        mutating func decode(_ type: Int64.Type) throws -> Int64 { let p = codingPath + [key]; return try Prim.int(next(), p) }
        mutating func decode(_ type: UInt8.Type) throws -> UInt8 { let p = codingPath + [key]; return try Prim.uint(next(), p) }
        mutating func decode(_ type: UInt32.Type) throws -> UInt32 { let p = codingPath + [key]; return try Prim.uint(next(), p) }
        mutating func decode(_ type: UInt64.Type) throws -> UInt64 { let p = codingPath + [key]; return try Prim.uint(next(), p) }

        mutating func decode<T: Decodable>(_ type: T.Type) throws -> T {
            let k = key
            let v = try next()
            return try T(from: decoder.child(v, k))
        }

        mutating func nestedContainer<NK: CodingKey>(keyedBy type: NK.Type) throws -> KeyedDecodingContainer<NK> {
            let k = key
            return try decoder.child(next(), k).container(keyedBy: type)
        }

        mutating func nestedUnkeyedContainer() throws -> UnkeyedDecodingContainer {
            let k = key
            return try decoder.child(next(), k).unkeyedContainer()
        }

        mutating func superDecoder() throws -> Decoder {
            let k = key
            return decoder.child(try next(), k)
        }
    }

    struct Single: SingleValueDecodingContainer {
        let decoder: Impl
        var codingPath: [CodingKey] { decoder.codingPath }
        func decodeNil() -> Bool { decoder.value.isNull }
        func decode(_ type: Bool.Type) throws -> Bool { try Prim.bool(decoder.value, codingPath) }
        func decode(_ type: String.Type) throws -> String { try Prim.string(decoder.value, codingPath) }
        func decode(_ type: Double.Type) throws -> Double { try Prim.double(decoder.value, codingPath) }
        func decode(_ type: Float.Type) throws -> Float { Float(try Prim.double(decoder.value, codingPath)) }
        func decode(_ type: Int.Type) throws -> Int { try Prim.int(decoder.value, codingPath) }
        func decode(_ type: Int8.Type) throws -> Int8 { try Prim.int(decoder.value, codingPath) }
        func decode(_ type: Int16.Type) throws -> Int16 { try Prim.int(decoder.value, codingPath) }
        func decode(_ type: Int32.Type) throws -> Int32 { try Prim.int(decoder.value, codingPath) }
        func decode(_ type: Int64.Type) throws -> Int64 { try Prim.int(decoder.value, codingPath) }
        func decode(_ type: UInt.Type) throws -> UInt { try Prim.uint(decoder.value, codingPath) }
        func decode(_ type: UInt8.Type) throws -> UInt8 { try Prim.uint(decoder.value, codingPath) }
        func decode(_ type: UInt16.Type) throws -> UInt16 { try Prim.uint(decoder.value, codingPath) }
        func decode(_ type: UInt32.Type) throws -> UInt32 { try Prim.uint(decoder.value, codingPath) }
        func decode(_ type: UInt64.Type) throws -> UInt64 { try Prim.uint(decoder.value, codingPath) }
        func decode<T: Decodable>(_ type: T.Type) throws -> T { try T(from: decoder) }
    }

    /// Scalars, strictly typed but for numbers: any whole number fits an integer type it fits
    /// in, any number a Double.
    enum Prim {
        static func mismatch<T>(_ t: T.Type, _ v: JSONValue, _ path: [CodingKey]) -> DecodingError {
            DecodingError.typeMismatch(t, .init(codingPath: path, debugDescription: "wanted \(t), got \(v.kindWord)"))
        }

        static func bool(_ v: JSONValue, _ p: [CodingKey]) throws -> Bool {
            guard let b = v.bool else { throw mismatch(Bool.self, v, p) }
            return b
        }

        static func string(_ v: JSONValue, _ p: [CodingKey]) throws -> String {
            guard let s = v.string else { throw mismatch(String.self, v, p) }
            return s
        }

        static func double(_ v: JSONValue, _ p: [CodingKey]) throws -> Double {
            guard let d = v.double else { throw mismatch(Double.self, v, p) }
            return d
        }

        static func int<T: FixedWidthInteger & SignedInteger>(_ v: JSONValue, _ p: [CodingKey]) throws -> T {
            guard let i = v.int64, let t = T(exactly: i) else { throw mismatch(T.self, v, p) }
            return t
        }

        static func uint<T: FixedWidthInteger & UnsignedInteger>(_ v: JSONValue, _ p: [CodingKey]) throws -> T {
            guard let u = v.uint64, let t = T(exactly: u) else { throw mismatch(T.self, v, p) }
            return t
        }
    }
}

extension JSONValue {
    /// What kind of JSON it is, for messages.
    public var kindWord: String {
        switch self {
        case .null: "null"
        case .bool: "a boolean"
        case .int, .uint, .double: "a number"
        case .string: "a string"
        case .array: "an array"
        case .object: "an object"
        }
    }
}
