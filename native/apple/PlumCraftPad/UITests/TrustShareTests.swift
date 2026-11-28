// B185: a graph from elsewhere waits for the Review prompt before it runs, with what it touches
// in the prompt's own words; trusted, it runs. B184: Share… for the graph and a value's Share in
// its pop-out bring up the system's share sheet with the file, cancelled (nothing is sent: no
// target is ever picked). In a file of its own, beside GestureTests' helpers; pictures go to
// PLUMCRAFT_SHOTS. Local runs only (no macOS CI): conformance.sh gestures with ONLY=… .

import XCTest

extension GestureTests {
    /// A graph somebody else wrote, in the test's temporary folder: a literal into upper case,
    /// its text new each run so no approval kept in the app's container covers it.
    func strangerGraph(_ name: String) throws -> String {
        let path = NSTemporaryDirectory() + name
        let mark = UUID().uuidString
        try """
        {"version": 1, "nodes": {"a": {"kind": "text.literal", "pos": [0, 0], "values": {"text": "from elsewhere \(mark)"}},
         "b": {"kind": "text.upper", "pos": [320, 0]}},
         "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "b", "port": "text"}}]}
        """.write(toFile: path, atomically: true, encoding: .utf8)
        return path
    }

    @MainActor
    func testAGraphFromElsewhereWaitsForTheReviewPrompt() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = try strangerGraph("b185-stranger.plum")
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let alert = app.alerts["Review before running"]
        // a run asked for: held, and the prompt comes up
        if !alert.waitForExistence(timeout: 5) { toolbar(app, "Run") }
        XCTAssertTrue(alert.waitForExistence(timeout: 20), "the Review prompt")
        let words = alert.staticTexts.allElementsBoundByIndex.map(\.label).joined(separator: "\n")
        XCTAssertTrue(words.contains("came from elsewhere"), "the prompt's sentence: \(words)")
        XCTAssertTrue(words.contains("Engines: Text"), "what it lists, in the core's words: \(words)")
        XCTAssertTrue(alert.buttons["Trust"].exists && alert.buttons["Cancel"].exists)
        // nothing ran while it waits: the status line says no run has finished
        func line() -> String { status()["statusBar"].map { "\($0)" } ?? "" }
        XCTAssertFalse(line().contains("last run"), "nothing ran before Trust: \(line())")
        shot("b185-ipad-review-prompt")
        alert.buttons["Trust"].tap()
        XCTAssertTrue(wait("the prompt gone", timeout: 10) { _ in !alert.exists })
        toolbar(app, "Run")
        let ran = Date().addingTimeInterval(60)
        while Date() < ran, !line().contains("last run") { RunLoop.current.run(until: Date().addingTimeInterval(0.3)) }
        XCTAssertFalse(alert.exists, "not asked again once trusted")
        shot("b185-ipad-trusted-and-run")
        XCTAssertTrue(line().contains("last run"), "trusted, it runs: \(line())")
    }

    /// The share sheet up, its file named, then closed without picking anything.
    @MainActor
    func shareSheetShown(_ app: XCUIApplication, _ file: String, _ picture: String) {
        let sheet = app.otherElements["ActivityListView"].firstMatch
        let up = Date().addingTimeInterval(20)
        while Date() < up, !sheet.exists, ((status()["sharing"] as? [String: Any])?["file"] as? String) == nil {
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        let said = (status()["sharing"] as? [String: Any])?["file"] as? String
        XCTAssertEqual(said, file, "the share sheet has the file")
        XCTAssertTrue(sheet.waitForExistence(timeout: 10), "the system's share sheet")
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        shot(picture)
        // cancelled: the sheet's own Close, else a tap outside its popover (never a target)
        let close = app.buttons["Close"].firstMatch
        if close.exists { close.tap() } else { app.coordinate(withNormalizedOffset: CGVector(dx: 0.05, dy: 0.5)).tap() }
        let gone = Date().addingTimeInterval(10)
        while Date() < gone, sheet.exists { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        XCTAssertFalse(sheet.exists, "the share sheet closed")
    }

    @MainActor
    func testShareSendsTheGraphAndAValueToTheShareSheet() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = try strangerGraph("b184-share.plum")
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let alert = app.alerts["Review before running"]
        // the graph: File › Share… (the bar's Share)
        toolbar(app, "tool.graph.share")
        shareSheetShown(app, "b184-share.plum", "b184-ipad-share-graph")
        // a value: run (trusting this file from elsewhere), pop upper case's output out, Share
        toolbar(app, "Run")
        if alert.waitForExistence(timeout: 10) { alert.buttons["Trust"].tap(); toolbar(app, "Run") }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        let out = try XCTUnwrap(reachable(app, canvas, "b", "out", 0), "b's pin")
        let pin = origin.withOffset(CGVector(dx: out[0], dy: out[1]))
        pin.doubleTap()
        for _ in 0..<2 {
            let deadline = Date().addingTimeInterval(8)
            while Date() < deadline, firstPopout() == nil { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            if firstPopout() == nil { pin.doubleTap() }
        }
        XCTAssertNotNil(firstPopout(), "the value popped out")
        let share = app.descendants(matching: .any)["viewer-share"].firstMatch
        XCTAssertTrue(share.waitForExistence(timeout: 20), "the pop-out's Share")
        let ready = Date().addingTimeInterval(30)
        while Date() < ready, !share.isEnabled { RunLoop.current.run(until: Date().addingTimeInterval(0.3)) }
        share.tap()
        let sheet = app.otherElements["ActivityListView"].firstMatch
        let up = Date().addingTimeInterval(20)
        while Date() < up, ((status()["sharing"] as? [String: Any])?["file"] as? String) == nil, !sheet.exists {
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        let file = ((status()["sharing"] as? [String: Any])?["file"] as? String) ?? ""
        XCTAssertTrue(file.hasSuffix(".txt"), "a text value is shared as text: \(file)")
        shareSheetShown(app, file, "b184-ipad-share-value")
    }
}
