// B161: the viewer bar fits a narrow window. On an iPhone (compact width) the View menu B154 added
// ran the bar off both edges: View As cut at the left, the actions at the right. Now the bar takes
// two or three rows when one is too wide (`ViewThatFits`); every control is inside the window.
// In a file of its own, beside GestureTests' helpers.

import XCTest

extension GestureTests {
    @MainActor
    func testTheViewerBarFitsANarrowWindow() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "viewer-bar-graph.json"
        // a list of named pictures: View As, View, the item stepper and every action at once
        try #"""
        {"version": 1, "nodes": {"v": {"kind": "vary.int", "pos": [-420, 0], "values": {"values": "320,321,322"}},
         "m": {"kind": "array.mandelbrot", "pos": [0, 0], "values": {"height": 200}}},
         "wires": [{"from": {"node": "v", "port": "values"}, "to": {"node": "m", "port": "width"}}]}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--then", "zoom:0.7:m"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready, on m", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        let out = try XCTUnwrap(reachable(app, canvas, "m", "out", 0), "m's pin")
        let pin = origin.withOffset(CGVector(dx: out[0], dy: out[1]))
        pin.doubleTap()
        for _ in 0..<2 {
            let deadline = Date().addingTimeInterval(8)
            while Date() < deadline, firstPopout() == nil { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            if firstPopout() == nil { pin.doubleTap() }
        }
        wait("the picture drawn", timeout: 150) { _ in ((self.firstPopout()?["picture"] as? [String: Any])?["loaded"] as? Bool) == true }
        let win = app.windows.firstMatch.frame
        for id in ["viewer-as", "viewer-mode", "viewer-compare", "viewer-copy", "viewer-share", "viewer-save"] {
            let e = app.descendants(matching: .any)[id].firstMatch
            XCTAssertTrue(e.waitForExistence(timeout: 10), "\(id) in the bar")
            XCTAssertGreaterThanOrEqual(e.frame.minX, win.minX - 0.5, "\(id) starts inside the window: \(e.frame) in \(win)")
            XCTAssertLessThanOrEqual(e.frame.maxX, win.maxX + 0.5, "\(id) ends inside the window: \(e.frame) in \(win)")
        }
        for label in ["Previous Item", "Next Item"] {
            let b = app.buttons[label]
            XCTAssertTrue(b.exists, label)
            XCTAssertTrue(b.frame.minX >= win.minX - 0.5 && b.frame.maxX <= win.maxX + 0.5, "\(label) inside the window: \(b.frame)")
        }
        shot("b161-ui-bar")
    }
}
