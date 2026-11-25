// The Swift types against the contract's shared data: the scene fixtures decode with no field
// left unknown, every property shell1.json names is known to the type it maps to, unknown
// words decode to `unknown`, and hostile input throws and never traps.

import Foundation
import PlumCraftModels
import Testing

/// The repository's root, from this file's place in it.
let repo: URL = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

func fixture(_ path: String) throws -> JSONValue {
    try JSONValue.parse(Data(contentsOf: repo.appendingPathComponent(path)))
}

/// Decode `v` as `T`, recording the keys asked for; the fields `T` doesn't know.
func unknown<T: Decodable>(_ type: T.Type, _ v: JSONValue) throws -> (T, [String]) {
    let r = KeyRecorder()
    let t = try JSONValueDecoder(recorder: r).decode(T.self, from: v)
    return (t, r.unknownFields(in: v))
}

@Suite struct Fixtures {
    @Test func theFullSceneDecodesWithNoFieldUnknown() throws {
        let v = try fixture("crates/editor-core/fixtures/scene/full.json")
        let (scene, missed) = try unknown(Scene.self, v)
        #expect(missed == [], "fields the Swift types don't know: \(missed)")
        #expect(scene.full)
        #expect(scene.nodes.count == v["nodes"].array?.count)
        #expect(scene.wires.count == v["wires"].array?.count)
        // every word the fixture uses is one the kit knows (it covers every row source, widget
        // kind, footer kind and run state)
        for f in scene.nodes {
            if let s = f.run.state { #expect(s.word != nil, "run state \(s)") }
            #expect(f.view.word != nil)
            #expect(f.header.state.kind.word != nil)
            for r in f.inputs {
                #expect(r.source.word != nil, "row source \(r.source)")
                #expect(r.pin.shape.word != nil)
                if let w = r.widget { #expect(w.kind.word != nil, "widget \(w.kind)") }
            }
            for item in f.footer { if case .unknown(let k) = item { Issue.record("footer kind \(k)") } }
            if case .unknown(let k)? = f.preview { Issue.record("preview kind \(k)") }
        }
        for w in scene.wires { #expect(w.kind.word != nil) }
        let footers = Set(scene.nodes.flatMap(\.footer).map { "\($0)".split(separator: "(").first.map(String.init) ?? "" })
        #expect(footers.count >= 9, "\(footers)")
        let sources = Set(scene.nodes.flatMap(\.inputs).compactMap(\.source.word))
        #expect(sources == Set(RowSourceWord.allCases))
        let states = Set(scene.nodes.compactMap(\.run.state?.word))
        #expect(states == Set(RunWord.allCases))
    }

    @Test func theDeltaDecodesWithNoFieldUnknown() throws {
        let v = try fixture("crates/editor-core/fixtures/scene/delta.json")
        let (delta, missed) = try unknown(Delta.self, v)
        #expect(missed == [], "\(missed)")
        #expect(!delta.full)
        #expect(delta.upsert.nodes.count == 1)
        #expect(delta.since < delta.gen)
        // and through the reply type, which tells them apart by `full`
        let r = try JSONValueDecoder().decode(SceneReply.self, from: v)
        guard case .delta(let d) = r else { Issue.record("a delta decoded as \(r)"); return }
        #expect(d == delta)
        let f = try JSONValueDecoder().decode(SceneReply.self, from: fixture("crates/editor-core/fixtures/scene/full.json"))
        guard case .full = f else { Issue.record("a full scene decoded as a delta"); return }
    }

    @Test func theCasesFileParses() throws {
        let v = try fixture("crates/editor-core/fixtures/conformance/cases.json")
        let cases = v["cases"].array ?? []
        #expect(cases.count >= 20)
        #expect(cases.filter { $0["half"] == "read" }.count >= 10)
    }
}

// MARK: - Against the schema

/// A sample of every property a schema type has (each variant of a oneOf by `variant`), so a
/// Swift type is checked against fields no fixture happens to fill.
struct Sampler {
    let defs: [String: JSONValue]
    var variant: Int

    func sample(_ s: JSONValue, depth: Int = 0) -> JSONValue {
        if depth > 60 { return .null }
        if let ref = s["$ref"].string {
            let name = String(ref.split(separator: "/").last ?? "")
            return sample(defs[name] ?? .null, depth: depth + 1)
        }
        if let c = s.object?["const"] { return c }
        if let e = s["enum"].array, !e.isEmpty { return e[variant % e.count] }
        for key in ["oneOf", "anyOf"] {
            if let alts = s[key].array {
                let real = alts.filter { $0["type"] != "null" }
                if real.isEmpty { return .null }
                return sample(real[variant % real.count], depth: depth + 1)
            }
        }
        if let all = s["allOf"].array, let first = all.first { return sample(first, depth: depth + 1) }
        var type = s["type"]
        if case .array(let ts) = type { type = ts.first { $0 != "null" } ?? .null }
        let word = type.string ?? (s.object?["properties"] != nil ? "object" : nil)
        switch word {
        case "object"?:
            var o: [String: JSONValue] = [:]
            for (k, p) in s["properties"].object ?? [:] { o[k] = sample(p, depth: depth + 1) }
            if o.isEmpty, let extra = s.object?["additionalProperties"], extra.object != nil {
                o["x"] = sample(extra, depth: depth + 1)
            }
            return .object(o)
        case "array"?:
            let n = s["minItems"].int64.map(Int.init) ?? 1
            let items = s.object?["items"] ?? .null
            if let tuple = items.array { return .array(tuple.map { sample($0, depth: depth + 1) }) }
            return .array((0..<max(n, 1)).map { _ in sample(items, depth: depth + 1) })
        case "string"?: return "x"
        case "integer"?: return 1
        case "number"?: return 1.5
        case "boolean"?: return false
        case "null"?: return .null
        // any JSON (a `Value` in Rust): the schema says nothing to sample, so null
        default: return .null
        }
    }
}

@Suite struct Schema {
    let defs: [String: JSONValue]

    init() throws {
        defs = try fixture("crates/editor-core/schema/shell1.json")["$defs"].object ?? [:]
    }

    func check<T: Decodable>(_ type: T.Type, _ name: String) throws {
        let def = try #require(defs[name], "shell1.json has no \(name)")
        for variant in 0..<12 {
            let v = Sampler(defs: defs, variant: variant).sample(def)
            let (_, missed) = try unknown(T.self, v)
            #expect(missed == [], "\(name) (variant \(variant)): fields \(T.self) doesn't know: \(missed)")
        }
    }

    @Test func everyMessageTheShellReadsIsKnown() throws {
        try check(Scene.self, "Scene")
        try check(Delta.self, "Delta")
        try check(Hit.self, "Hit")
        try check(Located.self, "LocateReply")
        try check(GestureReply.self, "GestureReply")
        try check(ThemeReply.self, "ThemeReply")
        try check(GeometrySetReply.self, "GeometrySetReply")
        try check(DragUpdate.self, "DragUpdate")
    }

    /// What the shell sends uses only fields the schema has.
    @Test func whatTheShellSendsIsInTheSchema() throws {
        func props(_ name: String) -> Set<String> { Set(defs[name]?["properties"].object?.keys.map { $0 } ?? []) }
        let gestures: [Gesture] = [.tap(Point(1, 2), mods: [.shift], slop: 4), .marquee(Point(0, 0), Point(5, 5)), .dragBegin(Point(1, 1)), .dragCancel]
        for g in gestures {
            let keys = Set(g.json.object?.keys.map { $0 } ?? [])
            #expect(keys.isSubset(of: props("Gesture")), "\(keys.subtracting(props("Gesture")))")
            let kinds = defs["GestureKind"]?["oneOf"].array?.flatMap { $0["enum"].array ?? [$0["const"]] }.compactMap(\.string) ?? []
            #expect(kinds.contains(g.kind.rawValue), "\(g.kind) not in \(kinds)")
        }
        let r = FaceReport(node: "n1", rev: 3, size: Point(10, 20), header: 28, inputs: [4, nil], outputs: [5], rows: [Rect(0, 0, 1, 1), nil],
                           parts: ["title": Rect(1, 1, 2, 2)], footer: [Rect(0, 9, 9, 10)], offset: Point(-7, -7))
        let keys = Set(r.json.object?.keys.map { $0 } ?? [])
        #expect(keys.isSubset(of: props("FaceReport")), "\(keys.subtracting(props("FaceReport")))")
        let parts = Set(r.json["parts"].object?.keys.map { $0 } ?? [])
        #expect(parts.isSubset(of: props("PartsReport")), "\(parts.subtracting(props("PartsReport")))")
    }
}

// MARK: - Unknown words and hostile input

@Suite struct Hostile {
    func face() throws -> JSONValue {
        try fixture("crates/editor-core/fixtures/scene/full.json")["nodes"][3]
    }

    func set(_ v: JSONValue, _ path: [String], _ x: JSONValue) -> JSONValue {
        guard let k = path.first else { return x }
        if case .object(var o) = v {
            o[k] = set(o[k] ?? .null, Array(path.dropFirst()), x)
            return .object(o)
        }
        if case .array(var a) = v, let i = Int(k), i < a.count {
            a[i] = set(a[i], Array(path.dropFirst()), x)
            return .array(a)
        }
        return v
    }

    @Test func unknownWordsDecodeAsUnknown() throws {
        var f = try face()
        f = set(f, ["run", "state"], "teleporting")
        f = set(f, ["view"], "holographic")
        f = set(f, ["shown"], "dancing")
        f = set(f, ["footer"], [["kind": "hologram", "text": "?"]])
        f = set(f, ["preview"], ["kind": "volume", "voxels": 9])
        f = set(f, ["futureField"], ["anything": [1, 2]])
        let d = try JSONValueDecoder().decode(Face.self, from: f)
        #expect(d.run.state == .unknown("teleporting"))
        #expect(d.view == .unknown("holographic"))
        #expect(d.shown == .unknown("dancing"))
        #expect(d.footer == [.unknown("hologram")])
        #expect(d.preview == .unknown("volume"))
        let w = try JSONValueDecoder().decode(Changed.self, from: ["doc": 1, "domain": "weather", "gen": 3])
        #expect(w.domain == .unknown("weather"))
        let a = try JSONValueDecoder().decode(Ask.self, from: ["ask": 4, "kind": "telepathy", "modal": true, "params": [:]])
        #expect(a.kind == .unknown("telepathy"))
        let p = try JSONValueDecoder().decode(Paint.self, from: ["token": "accent", "color": [1, 2, 3, 4]])
        #expect(p == .token("accent"))
        #expect(try JSONValueDecoder().decode(Paint.self, from: [:]) == Paint.none)
    }

    /// Every leaf of a face replaced, one at a time, by each of a few wrong values: decoding
    /// throws or succeeds, and never traps.
    @Test func wrongValuesThrowAndNeverTrap() throws {
        let f = try face()
        var paths: [[String]] = []
        func leaves(_ v: JSONValue, _ p: [String]) {
            switch v {
            case .object(let o): for (k, x) in o { leaves(x, p + [k]) }
            case .array(let a): for (i, x) in a.enumerated() { leaves(x, p + [String(i)]) }
            default: paths.append(p)
            }
            if !p.isEmpty { paths.append(p) }
        }
        leaves(f, [])
        let wrong: [JSONValue] = [.null, "x", -1, 1e300, .uint(UInt64.max), [], [:], [1, 2, 3, 4, 5], true]
        var threw = 0
        var took = 0
        for p in paths {
            for w in wrong {
                do {
                    _ = try JSONValueDecoder().decode(Face.self, from: set(f, p, w))
                    took += 1
                } catch {
                    threw += 1
                }
            }
        }
        #expect(threw > 0 && took > 0, "\(threw) threw, \(took) decoded")
        // the poll itself: a broken item costs only itself
        let poll: JSONValue = ["replies": [["id": 1, "ok": true, "result": 5]], "events": [],
                               "asks": [["ask": 1, "kind": "prompt", "modal": true, "params": [:]], ["nonsense": true], 7],
                               "changed": [["doc": 1, "domain": "scene", "gen": 2], "oops"], "dropped": "many"]
        let b = try JSONValueDecoder().decode(FeedBatch.self, from: poll)
        #expect(b.asks.count == 1 && b.changed.count == 1 && b.replies.count == 1 && b.dropped == 0)
    }
}

// MARK: - JSON values

@Suite struct Values {
    @Test func numbersKeepTheirKind() throws {
        let v = try JSONValue.parse(#"{"a": 0, "b": 0.5, "c": 18446744073709551615, "d": -3, "e": 1e300, "f": true, "g": null, "h": "é\n\"", "i": 2.0}"#)
        #expect(v["a"] == .int(0))
        #expect(v["b"] == .double(0.5))
        #expect(v["c"] == .uint(UInt64.max))
        #expect(v["d"] == .int(-3))
        #expect(v["e"].double == 1e300)
        #expect(v["f"] == true)
        #expect(v["g"].isNull)
        #expect(v["h"] == "é\n\"")
        #expect(v["i"].double == 2.0)
        let back = try JSONValue.parse(v.serialized())
        #expect(back.same(v))
        #expect(back["a"] == .int(0) && back["c"] == .uint(UInt64.max))
        #expect(JSONValue.int(5).same(.double(5.0004)))
        #expect(!JSONValue.int(5).same(.double(5.01)))
    }

    @Test func pointers() throws {
        let v: JSONValue = ["a": [["b/c": 1, "d~e": 2]], "n": .null]
        #expect(v.pointer("/a/0/b~1c") == 1)
        #expect(v.pointer("/a/0/d~0e") == 2)
        #expect(v.pointer("/a/1") == nil)
        #expect(v.pointer("/n") == .null)
        #expect(v.pointer("/nope") == nil)
        #expect(v.pointer("") == v)
        #expect(v.pointer("a") == nil)
    }

    /// The drift check isn't vacuous: a field a type doesn't have is reported, inside arrays and
    /// optionals too, and a JSONValue field takes everything under it.
    @Test func theRecorderFindsFieldsATypeLacks() throws {
        var v = try fixture("crates/editor-core/fixtures/scene/full.json")
        guard case .object(var o) = v, case .array(var nodes) = o["nodes"] ?? .null, case .object(var n0) = nodes[0],
              case .object(var hdr) = n0["header"] ?? .null else { Issue.record("fixture shape"); return }
        hdr["sparkle"] = true
        n0["header"] = .object(hdr)
        nodes[0] = .object(n0)
        o["nodes"] = .array(nodes)
        o["future"] = 1
        v = .object(o)
        let (_, missed) = try unknown(Scene.self, v)
        #expect(missed == ["/future", "/nodes/0/header/sparkle"])
        let mark: JSONValue = ["id": "a.b", "nid": 1, "rev": 1, "label": "m", "tone": "warn", "paint": ["token": "warning"], "tooltip": .null, "at": .null, "glow": 3]
        #expect(try unknown(Mark.self, mark).1 == ["/glow"])
    }

    /// The byte parser gives what JSONSerialization gives, on the shared data.
    @Test func theParserAgreesWithFoundation() throws {
        for path in ["crates/editor-core/fixtures/scene/full.json", "crates/editor-core/fixtures/scene/delta.json",
                     "crates/editor-core/fixtures/conformance/cases.json", "crates/editor-core/schema/shell1.json"] {
            let data = try Data(contentsOf: repo.appendingPathComponent(path))
            #expect(try JSONValue.parse(data) == JSONValue.parseWithFoundation(data), "\(path)")
        }
        let tricky = #"{"s": "a\"b\\c\/d\b\f\n\r\t\u00e9\ud83d\ude00 x", "lone": "\ud800!", "n": [0, -0, -1, 9223372036854775807, -9223372036854775808, 18446744073709551615, 18446744073709551616, 1.5e-3, 1E2, -2.5]}"#
        let v = try JSONValue.parse(tricky)
        #expect(v["s"] == "a\"b\\c/d\u{8}\u{c}\n\r\té😀 x")
        #expect(v["lone"] == "\u{FFFD}!")
        #expect(v["n"] == [0, 0, -1, .int(Int64.max), .int(Int64.min), .uint(UInt64.max), .double(18446744073709551616), .double(0.0015), .double(100), .double(-2.5)])
        #expect(try JSONValue.parse(" 5 ") == 5)
        #expect(try JSONValue.parse("\"x\"") == "x")
    }

    @Test func deepNestingIsRefusedNotAStackOverflow() throws {
        let deep = String(repeating: "[", count: 100_000) + String(repeating: "]", count: 100_000)
        #expect(throws: JSONError.self) { try JSONValue.parse(deep) }
        let edge = String(repeating: "[", count: 129) + String(repeating: "]", count: 129)
        #expect(throws: JSONError.self) { try JSONValue.parse(edge) }
        // as deep as it takes: and every walk over it (write, compare, decode, record) stays on
        // the stack of the thread it is on
        let ok = String(repeating: "[{\"a\":", count: 64) + "1" + String(repeating: "}]", count: 64)
        guard let v = try? JSONValue.parse(ok) else { Issue.record("128 levels refused"); return }
        #expect(try JSONValue.parse(v.serialized()) == v)
        #expect(v.same(v))
        let r = KeyRecorder()
        #expect(try JSONValueDecoder(recorder: r).decode(JSONValue.self, from: v) == v)
        #expect(r.unknownFields(in: v).isEmpty)
    }

    @Test func badJSONThrows() {
        for bad in ["", "{", "[1,", "nul", "\u{0}", "01", "1.", "1e", "-", "{\"a\" 1}", "{1: 2}", "[1 2]", "\"\\q\"", "\"\\u12\"", "\"a\nb\"", "[1]x", "tru", "\"open"] {
            #expect(throws: JSONError.self) { try JSONValue.parse(bad) }
        }
    }
}

/// The feed's `document` change (N3 slice 4): the state a window shows, and what isn't one.
@Suite struct DocumentChanges {
    @Test func aDocumentChangeCarriesTheStateAWindowShows() throws {
        let v = try JSONValue.parse(#"{"doc": 2, "domain": "document", "gen": 3, "title": "a.json • — PlumCraft", "document": "a.json", "path": "/x/a.json", "dirty": true, "undo": "Add Node", "redo": null, "lastError": null}"#)
        let c = try JSONValueDecoder().decode(Changed.self, from: v)
        let st = try #require(DocumentState(c))
        #expect(st.gen == 3 && st.dirty && st.undo == "Add Node" && st.redo == nil && st.path == "/x/a.json" && st.name == "a.json")
        // a closing change has no state; another domain isn't one
        let closing = try JSONValueDecoder().decode(Changed.self, from: try JSONValue.parse(#"{"doc": 2, "domain": "document", "closing": true}"#))
        #expect(DocumentState(closing) == nil)
        let engine = try JSONValueDecoder().decode(Changed.self, from: try JSONValue.parse(#"{"doc": 2, "domain": "engine", "gen": 1, "connected": true}"#))
        #expect(DocumentState(engine) == nil)
    }

    /// The status line (research note 28): the core's segments, which a shell draws; a narrow
    /// bar keeps priority 1; junk is left out, never fatal.
    @Test func theStatusLineComesWithTheDocument() throws {
        let v = try JSONValue.parse(#"{"doc": 2, "domain": "document", "gen": 4, "statusLine": {"on": true, "segments": [{"id": "engine", "role": "engine", "side": "left", "priority": 1, "text": "Text 0.7", "tone": "info", "dot": "ok", "tooltip": "long"}, {"id": "hash", "role": "hash", "priority": 3, "text": "hash b3 · default", "tone": "info"}, {"id": "run", "role": "run", "priority": 1, "text": "running: 1 of 3 nodes", "busy": true, "detail": "0.4 s", "command": {"id": "run.cancel", "params": {}, "label": "Cancel"}}, {"id": "message.0", "role": "message", "side": "right", "priority": 1, "text": "boom", "tone": "error", "dot": "sideways"}, {"role": "no id"}, 7]}}"#)
        let st = try #require(DocumentState(try JSONValueDecoder().decode(Changed.self, from: v)))
        let line = st.statusLine
        #expect(line.on && line.segments.map(\.id) == ["engine", "hash", "run", "message.0"])
        #expect(line.shown(upTo: 1).map(\.id) == ["engine", "run", "message.0"])
        #expect(line.segments[2].command?.id == "run.cancel" && line.segments[2].busy == true && line.segments[2].detail == "0.4 s")
        #expect(line.segments[3].tone?.word == .error && line.segments[3].dot?.word == nil, "an unknown dot is kept unknown")
        // off, and a line that isn't one
        let off = try JSONValueDecoder().decode(Changed.self, from: try JSONValue.parse(#"{"doc": 2, "domain": "document", "gen": 5, "statusLine": {"on": false, "segments": []}}"#))
        #expect(DocumentState(off)?.statusLine.on == false)
        let junk = try JSONValueDecoder().decode(Changed.self, from: try JSONValue.parse(#"{"doc": 2, "domain": "document", "gen": 6, "statusLine": "nope"}"#))
        #expect(DocumentState(junk)?.statusLine.segments.isEmpty == true)
    }
}
