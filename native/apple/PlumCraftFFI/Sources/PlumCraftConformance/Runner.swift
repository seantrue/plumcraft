// The shell conformance suite's runner, in Swift (docs/shell-interface.md 16.1): the cases are
// the shared data, crates/editor-core/fixtures/conformance/cases.json, read where they are (never
// copied); this is crates/editor-core/src/conformance.rs's step language, value substitution,
// `$locate`, matchers and checks, line for line, so a case means the same here as in Rust.

import Foundation
import PlumCraftModels

/// A shell the suite drives (conformance.rs's `Driver`).
@MainActor
public protocol ConformanceDriver: AnyObject {
    /// For messages.
    var name: String { get }
    /// One control request; the reply `{ok, result | error}` (never thrown).
    func call(_ method: String, _ params: JSONValue) async -> JSONValue
    /// Does this shell take its measurements from the suite (a headless shell), rather than
    /// measure the faces it draws?
    var reports: Bool { get }
    /// Report `faces` (`geometry.set`'s faces, ids resolved) and wait until the core has them.
    func reportGeometry(_ faces: JSONValue) async throws
    /// Wait until the shell drew and measured what the last request changed.
    func settle() async
    /// The typed drag move, as `plum_gesture_move` answers it (over a port its JSON form,
    /// `geometry.dragMove`, N5 slice 4); nil where the driver has none.
    func hotMove(at: (Float, Float), slop: Float) async -> JSONValue?
    /// `engine:<pack>`, `media`; `reports` for a driver that measures its own faces but reports
    /// the suite's over them (N5 slice 4; a headless driver's is `reports`).
    func has(_ capability: String) -> Bool
    /// Let the shell get on before a request is sent again (`until`).
    func pause() async
}

/// The halves of the suite (16.1).
public enum Half: String, Sendable {
    case read, edit
}

/// How one case went.
public struct Outcome: Sendable, Equatable {
    public var name: String
    public var steps: Int
    /// Nil: passed (or skipped).
    public var error: String?
    /// Not run, and why.
    public var skipped: String?
    /// Steps this driver didn't run (an `only` it lacks, a `hot` or `geometry` step it has no
    /// form for): N5 slice 4's gate counts them.
    public var stepsSkipped = 0

    public var passed: Bool { error == nil && skipped == nil }
}

public struct ConformanceError: Error, CustomStringConvertible, Sendable {
    public let description: String
    public init(_ d: String) { description = d }
}

/// The core's default hit distance in graph units (`gesture::DEFAULT_SLOP`).
let defaultSlop: Float = 12

@MainActor
public enum Conformance {
    /// The cases in `cases.json` at `url`.
    nonisolated public static func cases(at url: URL) throws -> [JSONValue] {
        let v = try JSONValue.parse(Data(contentsOf: url))
        guard let cases = v["cases"].array else { throw ConformanceError("\(url.path): no \"cases\" list") }
        return cases
    }

    /// Run every case of `half` on `d`.
    public static func run(_ d: ConformanceDriver, cases: [JSONValue], half: Half?) async -> [Outcome] {
        var out: [Outcome] = []
        for c in cases where half == nil || c["half"].string == half?.rawValue {
            out.append(await runCase(d, c))
        }
        return out
    }

    /// A short report: passed, failed with why, skipped with why.
    nonisolated public static func summary(_ driver: String, _ outcomes: [Outcome]) -> String {
        let failed = outcomes.compactMap { o in o.error.map { "  \(o.name): \($0)" } }
        let skipped = outcomes.compactMap { o in o.skipped.map { "  skipped \(o.name): \($0)" } }
        let run = outcomes.count - skipped.count
        var s = "\(driver): \(run - failed.count) of \(run) cases passed, \(skipped.count) skipped"
        // the steps (N5 slice 4): run of the cases' total, and those this driver skipped
        let total = outcomes.reduce(0) { $0 + $1.steps }, notRun = outcomes.reduce(0) { $0 + $1.stepsSkipped }
        s += "; steps \(total - notRun) run of \(total), \(notRun) skipped"
        if !failed.isEmpty { s += "\n" + failed.joined(separator: "\n") }
        if !skipped.isEmpty { s += "\n" + skipped.joined(separator: "\n") }
        return s
    }

    static func has(_ d: ConformanceDriver, _ capability: String) -> Bool {
        switch capability {
        // the suite's measurements reported: by a headless driver, or over an app's own (N5 slice 4)
        case "reports": d.reports || d.has("reports")
        // rows' widgets and header parts measured (N4 slice 2): reported by the suite, or by the app
        case "rows": d.reports || d.has("rows")
        default: d.has(capability)
        }
    }

    /// Run one case on a new graph, or skip it when `d` lacks what it needs.
    public static func runCase(_ d: ConformanceDriver, _ c: JSONValue) async -> Outcome {
        let name = c["name"].string ?? "(unnamed)"
        let lacks = (c["needs"].array ?? []).compactMap(\.string).filter { !has(d, $0) }
        if !lacks.isEmpty {
            return Outcome(name: name, steps: 0, error: nil, skipped: "\(d.name) lacks \(lacks.joined(separator: ", "))")
        }
        var vars: [String: JSONValue] = [:]
        let fresh = await d.call("command.run", ["command": "graph.new", "params": ["discard": true]])
        if fresh["ok"] != true {
            return Outcome(name: name, steps: 0, error: "graph.new: \(fresh)", skipped: nil)
        }
        await d.settle()
        let steps = c["steps"].array ?? []
        var notRun = 0
        for (k, s) in steps.enumerated() {
            do {
                if try await !step(d, &vars, s) { notRun += 1 }
            } catch {
                return Outcome(name: name, steps: k, error: "step \(k + 1): \(error)", skipped: nil, stepsSkipped: notRun)
            }
        }
        // a drag left under way would carry into the next case, and so would an open palette
        _ = await d.call("ui.gesture", ["kind": "drag", "phase": "cancel"])
        _ = await d.call("command.run", ["command": "canvas.palette", "params": ["open": false]])
        return Outcome(name: name, steps: steps.count, error: nil, skipped: nil, stepsSkipped: notRun)
    }

    /// Run one step: false when this driver skipped it.
    @discardableResult
    static func step(_ d: ConformanceDriver, _ vars: inout [String: JSONValue], _ s: JSONValue) async throws -> Bool {
        let only: [String] = switch s["only"] {
        case .string(let c): [c]
        case .array(let a): a.compactMap(\.string)
        default: []
        }
        if only.contains(where: { !has(d, $0) }) { return false }
        if let faces = s.object?["geometry"] {
            guard has(d, "reports") else { return false }
            try await d.reportGeometry(subst(faces, vars))
            return true
        }
        if let h = s.object?["hot"] {
            let r = try await resolve(d, subst(h, vars))
            guard let at = r["at"].array, at.count == 2 else { throw ConformanceError("hot.at must be [x, y]") }
            guard let x = at[0].double, let y = at[1].double else { throw ConformanceError("hot.at must be numbers") }
            let slop = r["slop"].double.map { Float($0) } ?? defaultSlop
            guard let v = await d.hotMove(at: (Float(x), Float(y)), slop: slop) else { return false }
            try expect(["ok": true, "result": v], s.object?["expect"], vars)
            return true
        }
        if let name = s["check"].string {
            if name == "authRefused" {
                guard let p = d as? PortDriver else { throw ConformanceError("authRefused needs a driver over the control port") }
                try await p.authRefused()
                return true
            }
            try check(name, subst(s, vars))
            return true
        }
        let method: String
        let params: JSONValue
        switch (s["call"].string, s.object?["gesture"]) {
        case (let m?, nil):
            method = m
            params = s.object?["params"] ?? [:]
        case (nil, let g?):
            method = "ui.gesture"
            params = g
        default:
            throw ConformanceError("a step is a call, a gesture, geometry, hot or a check: \(s)")
        }
        let p = try await resolve(d, subst(params, vars))
        var reply = await d.call(method, p)
        if let until = s.object?["until"] {
            let limit = min(max(s["timeout"].double ?? 30, 0), 600)
            let deadline = ContinuousClock.now + .milliseconds(Int64(limit * 1000))
            while true {
                do {
                    try expect(reply, until, vars)
                    break
                } catch {
                    if ContinuousClock.now > deadline {
                        throw ConformanceError("\(method) \(p): not so in \(limit) s: \(error)")
                    }
                }
                await d.pause()
                reply = await d.call(method, p)
            }
        }
        // a condition that must stay so for a while (`holds`, `for`; B43)
        if let holds = s.object?["holds"] {
            let span = min(max(s["for"].double ?? 1, 0), 10)
            let start = ContinuousClock.now
            let end = start + .milliseconds(Int64(span * 1000))
            while true {
                do {
                    try expect(reply, holds, vars)
                } catch {
                    let t = ContinuousClock.now - start
                    throw ConformanceError("\(method) \(p): didn't stay so (\(t) in): \(error)")
                }
                if ContinuousClock.now >= end { break }
                await d.pause()
                reply = await d.call(method, p)
            }
        }
        let wantOk = s["ok"].bool ?? true
        if (reply["ok"] == true) != wantOk {
            throw ConformanceError("\(method) \(p): wanted ok \(wantOk), got \(short(reply))")
        }
        if ["command.run", "ui.gesture", "ui.invoke", "geometry.set"].contains(method) {
            await d.settle()
        }
        do {
            try expect(reply, s.object?["expect"], vars)
        } catch {
            throw ConformanceError("\(method) \(p): \(error)")
        }
        if let save = s["save"].object {
            for (v, ptr) in save {
                guard let ptr = ptr.string else { throw ConformanceError("save's values are JSON pointers") }
                guard let x = reply.pointer(substStr(ptr, vars)) else {
                    throw ConformanceError("\(method): nothing at \(ptr) to save as \(v): \(short(reply))")
                }
                vars[v] = x
            }
        }
        return true
    }

    static func short(_ v: JSONValue) -> String {
        let t = v.text
        return t.count > 600 ? String(t.prefix(600)) + "…" : t
    }

    /// `$var` in a string: each replaced by its text (a string's own, else its JSON).
    static func substStr(_ s: String, _ vars: [String: JSONValue]) -> String {
        var out = ""
        var rest = Substring(s)
        while let k = rest.firstIndex(of: "$") {
            out += rest[..<k]
            let after = rest[rest.index(after: k)...]
            let end = after.firstIndex { !($0.isASCII && ($0.isLetter || $0.isNumber || $0 == "_")) } ?? after.endIndex
            let name = String(after[..<end])
            if !name.isEmpty, let v = vars[name] {
                if case .string(let str) = v { out += str } else { out += v.text }
            } else {
                out += "$" + name
            }
            rest = after[end...]
        }
        out += rest
        return out
    }

    /// `"$v"` whole is the saved value (any JSON); keys and longer strings take its text.
    static func subst(_ v: JSONValue, _ vars: [String: JSONValue]) -> JSONValue {
        switch v {
        case .string(let s):
            if s.hasPrefix("$"), let val = vars[String(s.dropFirst())] { return val }
            return .string(substStr(s, vars))
        case .array(let a):
            return .array(a.map { subst($0, vars) })
        case .object(let o):
            var m: [String: JSONValue] = [:]
            for (k, x) in o { m[substStr(k, vars)] = subst(x, vars) }
            return .object(m)
        default:
            return v
        }
    }

    /// `{"$locate": …, "$plus"?: [dx, dy]}` anywhere in `v`: the point `geometry.locate` gives.
    static func resolve(_ d: ConformanceDriver, _ v: JSONValue) async throws -> JSONValue {
        switch v {
        case .object(let o) where o["$locate"] != nil:
            let what = o["$locate"] ?? .null
            let r = await d.call("geometry.locate", what)
            if r["ok"] != true { throw ConformanceError("geometry.locate \(what): \(short(r))") }
            let at = r["result"]["at"]
            guard let x = at[0].double, let y = at[1].double else { throw ConformanceError("geometry.locate gave no point: \(r)") }
            let plus = o["$plus"]
            let dx = plus?[0].double ?? 0
            let dy = plus?[1].double ?? 0
            return [.double(x + dx), .double(y + dy)]
        case .object(let o):
            var m: [String: JSONValue] = [:]
            for (k, x) in o { m[k] = try await resolve(d, x) }
            return .object(m)
        case .array(let a):
            var out: [JSONValue] = []
            for x in a { out.append(try await resolve(d, x)) }
            return .array(out)
        default:
            return v
        }
    }

    static func expect(_ reply: JSONValue, _ want: JSONValue?, _ vars: [String: JSONValue]) throws {
        guard let want = want?.object else { return }
        for (ptr, w) in want {
            let p = substStr(ptr, vars)
            let wv = subst(w, vars)
            do {
                try matches(reply.pointer(p), wv)
            } catch {
                throw ConformanceError("at \(p): \(error); the reply: \(short(reply))")
            }
        }
    }

    static func deepHas(_ v: JSONValue, _ fields: [String: JSONValue]) -> Bool {
        switch v {
        case .object(let o):
            fields.allSatisfy { k, w in o[k].map { $0.same(w) } ?? false } || o.values.contains { deepHas($0, fields) }
        case .array(let a):
            a.contains { deepHas($0, fields) }
        default:
            false
        }
    }

    static func matches(_ got: JSONValue?, _ want: JSONValue) throws {
        if case .object(let o) = want, o.count == 1, let (k, arg) = o.first, k.hasPrefix("$") {
            let ok: Bool
            switch k {
            case "$exists": ok = (got != nil) == (arg.bool ?? true)
            case "$len":
                let n = got.flatMap { $0.array?.count ?? $0.object?.count }
                ok = n.map(UInt64.init) == arg.uint64
            case "$contains": ok = got?.array?.contains { $0.same(arg) } ?? false
            case "$deep": ok = arg.object.map { f in got.map { deepHas($0, f) } ?? false } ?? false
            case "$not": ok = got.map { !$0.same(arg) } ?? true
            case "$one": ok = arg.array.map { a in got.map { g in a.contains { g.same($0) } } ?? false } ?? false
            case "$min": ok = arg.double.map { n in (got?.double).map { $0 >= n } ?? false } ?? false
            case "$find":
                guard let fields = arg["where"].object else { throw ConformanceError("$find needs where: {field: value}") }
                let found = got?.array?.first { x in fields.allSatisfy { k, w in x.object?[k].map { $0.same(w) } ?? false } }
                guard let item = found else {
                    throw ConformanceError("no item with \(JSONValue.object(fields)) in \(got.map(short) ?? "nothing")")
                }
                for (ptr, w) in arg["want"].object ?? [:] {
                    do {
                        try matches(item.pointer(ptr), w)
                    } catch {
                        throw ConformanceError("the item with \(JSONValue.object(fields)): at \(ptr): \(error)")
                    }
                }
                ok = true
            default:
                throw ConformanceError("unknown matcher \(k)")
            }
            if !ok { throw ConformanceError("wanted \(k) \(arg), got \(got.map(short) ?? "nothing")") }
            return
        }
        guard let g = got else { throw ConformanceError("wanted \(want), got nothing") }
        if !g.same(want) { throw ConformanceError("wanted \(want), got \(short(g))") }
    }

    /// The runner's own checks: `tokensResolved {scene, theme}` and `nidsUnique {scene}`.
    public static func check(_ name: String, _ s: JSONValue) throws {
        switch name {
        case "tokensResolved":
            guard let tokens = s["theme"]["tokens"].object else { throw ConformanceError("theme.tokens is not a map") }
            var named = Set<String>()
            var bad: [String] = []
            paints(s["scene"], &named, &bad)
            if !bad.isEmpty { throw ConformanceError("paints with both a token and a colour, or neither: \(bad.prefix(5))") }
            if named.isEmpty { throw ConformanceError("the scene names no token") }
            let missing = named.filter { tokens[$0] == nil }.sorted()
            if !missing.isEmpty { throw ConformanceError("theme.get lacks \(missing)") }
        case "nidsUnique":
            var seen: [UInt64: (String, JSONValue)] = [:]
            for kind in ["nodes", "wires", "nets", "frames", "groups", "marks"] {
                for item in s["scene"][kind].array ?? [] {
                    guard let nid = item["nid"].uint64 else { throw ConformanceError("\(kind) item without a nid: \(short(item))") }
                    if let other = seen[nid] { throw ConformanceError("nid \(nid) is both \(other.0) \(other.1) and \(kind) \(item["id"])") }
                    seen[nid] = (kind, item["id"])
                }
            }
            if seen.count < 2 { throw ConformanceError("too few items to check") }
        default:
            throw ConformanceError("unknown check \(name)")
        }
    }

    static func paints(_ v: JSONValue, _ named: inout Set<String>, _ bad: inout [String]) {
        switch v {
        case .object(let o):
            let token = o["token"]?.string
            let color = o["color"].flatMap { $0.array?.count == 4 ? $0 : nil }
            let paintish = (token != nil || color != nil) && o.keys.allSatisfy { $0 == "token" || $0 == "color" }
            if paintish {
                switch (token, color) {
                case (let t?, nil): named.insert(t)
                case (nil, _?): break
                default: bad.append(v.text)
                }
            } else {
                for x in o.values { paints(x, &named, &bad) }
            }
        case .array(let a):
            for x in a { paints(x, &named, &bad) }
        default:
            break
        }
    }
}
