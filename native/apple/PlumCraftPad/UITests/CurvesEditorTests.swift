// The curves editor on the iPad (S4a, docs/shell-interface.md 12.21), driven by XCUITest in the
// simulator: Apply Curve's curves row opens the shell's native curves editor (not the core's
// value window), on the node and on a collapsed group's promoted row; a drag on the plot adds a
// point and moves it, the lift is one `curves.edit` (the status's undo says Edit Curve), and one
// two-finger tap on the canvas undoes it. In a file of its own, beside GestureTests' helpers.

import XCTest

extension GestureTests {
    /// The open editor's native state (`coreWindows.report.native`), when the shown window is it.
    func curvesShown(_ key: String) -> [String: Any]? {
        let c = coreWindows()
        guard (c["shown"] as? String) == key, let r = c["report"] as? [String: Any] else { return nil }
        return r["native"] as? [String: Any]
    }

    func curvesPoints(_ key: String) -> Int? { (curvesShown(key)?["points"] as? [Any])?.count }

    @MainActor
    func testCurvesEditor() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "curves-editor-graph.json"
        // one Apply Curve on its own; another in a collapsed group with its curves promoted
        try #"""
        {"version": 1, "nodes": {"c": {"kind": "array.apply_curve", "pos": [0, 0]}, "d": {"kind": "array.apply_curve", "pos": [0, 500]}},
         "wires": [], "groups": {"g1": {"name": "Tone", "nodes": ["d"], "collapsed": true, "pos": [520, 0],
         "promoted": [{"port": {"node": "d", "port": "curves"}, "label": "curves"}]}}}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--then", "zoom:1:c"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        func rowAt(_ face: String) -> [Double]? {
            guard let rows = (window()["rows"] as? [String: Any])?[face] as? [Any] else { return nil }
            return rows.compactMap { $0 as? [String: Any] }.first { ($0["key"] as? String) == "curves" }?["at"] as? [Double]
        }
        wait("the graph, ready, with both curves rows", timeout: 150) { _ in rowAt("c") != nil && rowAt("group:g1") != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))

        for (face, node, name) in [("c", "c", "node"), ("group:g1", "d", "group")] {
            let key = "editor:\(node)"
            reveal(app, canvas) { rowAt(face) }
            let at = try XCTUnwrap(rowAt(face))
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at[0], dy: at[1])).tap()
            wait("the curves editor from the \(name)'s row") { _ in self.curvesPoints(key) == 2 }
            let plot = app.otherElements["curves-plot"]
            XCTAssertTrue(plot.waitForExistence(timeout: 10), "the native editor, not the value window")
            shot("s4a-ui-curves-\(name)-open")
            // press on empty space at the middle, drag up and left: a point added and moved
            let from = plot.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
            let to = plot.coordinate(withNormalizedOffset: CGVector(dx: 0.4, dy: 0.3))
            from.press(forDuration: 0.05, thenDragTo: to)
            wait("one curves.edit, three points") { _ in self.curvesPoints(key) == 3 && (self.curvesShown(key)?["sent"] as? Int) == 1 }
            wait("the edit, one undo step") { ($0["undo"] as? String) == "Edit Curve" }
            if let p = (curvesShown(key)?["points"] as? [[Double]])?[1] {
                XCTAssertEqual(p[0], 0.4, accuracy: 0.05)
                XCTAssertEqual(p[1], 0.7, accuracy: 0.05)
            }
            shot("s4a-ui-curves-\(name)-dragged")
            app.buttons["report-done"].tap()
            wait("the editor closed") { _ in self.coreWindows()["shown"] == nil || self.coreWindows()["shown"] is NSNull }
            // one undo takes the whole gesture back
            canvas.tap(withNumberOfTaps: 1, numberOfTouches: 2)
            wait("undone") { ($0["undo"] as? String) != "Edit Curve" }
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at[0], dy: at[1])).tap()
            wait("the editor again, the curve back to two points") { _ in self.curvesPoints(key) == 2 }
            app.buttons["report-done"].tap()
            wait("the editor closed again") { _ in self.coreWindows()["shown"] == nil || self.coreWindows()["shown"] is NSNull }
        }
    }
}
