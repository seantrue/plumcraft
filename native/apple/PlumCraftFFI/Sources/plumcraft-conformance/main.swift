// The conformance suite (docs/shell-interface.md 16.1) against a running shell's control port:
//
//   plumcraft-conformance --port N --token-file F --cases cases.json [--doc D] [--half read|edit|all]
//                         [--measured] [--engine text]... [--case NAME] [--needing engine:array]
//
// `--measured`: the shell measures its own faces (an app with a canvas), so the suite's geometry
// steps are skipped and each settle waits until every face is measured. Prints one CASE line per
// case and a summary; exits 0 when no case failed (skipped cases are reported, never passed).

import Foundation
import PlumCraftConformance
import PlumCraftModels

@MainActor
func run() async -> Int32 {
    var args = Array(CommandLine.arguments.dropFirst())
    func value(_ k: String) -> String? {
        guard let i = args.firstIndex(of: k), i + 1 < args.count else { return nil }
        let v = args[i + 1]
        args.removeSubrange(i...(i + 1))
        return v
    }
    var engines: Set<String> = []
    while let e = value("--engine") { engines.insert(e) }
    let measured = args.contains("--measured")
    guard let portText = value("--port"), let port = UInt16(portText), let tokenFile = value("--token-file"), let casesPath = value("--cases") else {
        FileHandle.standardError.write(Data("usage: plumcraft-conformance --port N --token-file F --cases cases.json [--doc D] [--half read|edit|all] [--measured] [--engine text] [--case NAME] [--needing CAPABILITY]\n".utf8))
        return 2
    }
    let doc = value("--doc").flatMap(UInt64.init)
    let halfWord = value("--half") ?? "read"
    let only = value("--case")
    let needing = value("--needing")
    let half: Half? = halfWord == "all" ? nil : Half(rawValue: halfWord)
    do {
        let token = try String(contentsOfFile: tokenFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        var cases = try Conformance.cases(at: URL(fileURLWithPath: casesPath))
        if let only { cases = cases.filter { $0["name"].string == only } }
        if let needing { cases = cases.filter { ($0["needs"].array ?? []).contains { $0.string == needing } } }
        let d = try await PortDriver(port: port, token: token, reports: !measured, doc: doc, engines: engines.isEmpty ? ["text"] : engines)
        defer { d.close() }
        let t0 = ContinuousClock.now
        let out = await Conformance.run(d, cases: cases, half: half)
        let took = ContinuousClock.now - t0
        for o in out {
            let word = o.passed ? "pass" : o.skipped != nil ? "skip" : "FAIL"
            print("CASE \(word) | \(o.name) | \(o.steps) steps\(o.skipped.map { " | \($0)" } ?? "")\(o.error.map { " | \($0)" } ?? "")")
        }
        print(Conformance.summary(d.name, out))
        print("settle timeouts: \(d.settleTimeouts); took \(took)")
        return out.contains { $0.error != nil } ? 1 : 0
    } catch {
        FileHandle.standardError.write(Data("plumcraft-conformance: \(error)\n".utf8))
        return 2
    }
}

let code = await run()
exit(code)
