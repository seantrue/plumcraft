// Note 26 slice 2b on the iPad and iPhone, driven by XCUITest in the simulator: two Mandelbrots
// pinned as A and B (through the control port, as a script pins them), Compare's Difference
// tapped in its sheet: the core computes |A − B| and lends its picture, which the sheet draws
// with the core's words. In a file of its own, beside GestureTests' helpers.

import XCTest

extension GestureTests {
    func compareFacts() -> [String: Any] { (coreWindows()["compare"] as? [String: Any]) ?? [:] }

    @MainActor
    func testCompareDifference() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "compare-difference-graph.json"
        try #"""
        {"version": 1, "nodes": {
          "a": {"kind": "array.mandelbrot", "pos": [0, 0], "values": {"width": 384, "height": 256}},
          "b": {"kind": "array.mandelbrot", "pos": [0, 320], "values": {"width": 384, "height": 256, "max_iterations": 24}}},
         "wires": []}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let tokenFile = NSTemporaryDirectory() + "compare-difference-token-\(UUID().uuidString)"
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--control-token-file", tokenFile, "--then", "zoom:0.5:a"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let port = try XCTUnwrap(status()["port"] as? Int, "the control port")
        let token = try String(contentsOfFile: tokenFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        let control = try XCTUnwrap(ControlPort(port: port, token: token))
        let doc = window()["doc"]
        func run(_ c: String, _ p: [String: Any]) -> [String: Any]? { control.call("command.run", ["command": c, "params": p], doc: doc) as? [String: Any] }
        let pa = try XCTUnwrap(run("view.popout", ["node": "a", "port": "escape"])?["id"] as? Int)
        let pb = try XCTUnwrap(run("view.popout", ["node": "b", "port": "escape"])?["id"] as? Int)
        _ = run("run.start", [:])
        _ = control.call("ui.waitRun", ["timeout": 120], doc: doc)
        XCTAssertNotNil(run("viewer.pin", ["slot": "a", "popout": pa]))
        XCTAssertNotNil(run("viewer.pin", ["slot": "b", "popout": pb]))
        XCTAssertNotNil(run("viewer.compare", ["on": true, "how": "side"]))
        wait("the Compare sheet", timeout: 30) { _ in (self.coreWindows()["shown"] as? String) == "compare" }
        let diff = app.buttons["Difference"]
        XCTAssertTrue(diff.waitForExistence(timeout: 15), "Difference in Compare's choice")
        diff.tap()
        wait("the core's difference drawn", timeout: 90) { _ in
            let d = self.compareFacts()["difference"] as? [String: Any]
            return (d?["status"] as? String) == "ready" && ((d?["pictures"] as? Int) ?? 0) == 1
        }
        let d = compareFacts()["difference"] as? [String: Any]
        XCTAssertEqual(d?["identical"] as? Bool, false)
        XCTAssertTrue((d?["text"] as? String ?? "").contains("elements differ"), "\(String(describing: d))")
        XCTAssertTrue(app.descendants(matching: .any)["compare-difference-text"].waitForExistence(timeout: 10))
        RunLoop.current.run(until: Date().addingTimeInterval(1.5))
        shot("views-s2b-ui-difference")
    }
}
