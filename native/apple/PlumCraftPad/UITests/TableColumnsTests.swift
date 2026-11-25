// Note 26 slice 3, tables as tables on the iPad: a column's width is a drag on its header's edge
// (the shell's state, docs/shell-interface.md 13.2.1), as the egui editor's table and the Mac's
// `Table` allow. In a file of its own, beside GestureTests' helpers.

import XCTest

extension GestureTests {
    @MainActor
    func testATableColumnIsWidenedByADrag() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "table-columns-graph.json"
        try #"""
        {"version": 1, "nodes": {
          "m": {"kind": "array.mandelbrot", "pos": [0, 0], "values": {"width": 64, "height": 48}},
          "h": {"kind": "array.histogram", "pos": [440, 0]}},
         "wires": [{"from": {"node": "m", "port": "escape"}, "to": {"node": "h", "port": "array"}}]}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--then", "zoom:0.6:h"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        try popOut(app, canvas, "h")
        wait("the histogram as a table", timeout: 150) { _ in (self.popout("h")?["viewer"] as? String) == "table.grid" }
        let header = app.buttons["viewer-table-header-1"]
        XCTAssertTrue(header.waitForExistence(timeout: 20), "the count column's header")
        let before = header.frame.width
        let edge = app.descendants(matching: .any)["viewer-table-edge-0"].firstMatch
        XCTAssertTrue(edge.waitForExistence(timeout: 10), "the centre column's edge")
        let centre = app.buttons["viewer-table-header-0"]
        let w0 = centre.frame.width
        edge.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.1, thenDragTo: edge.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).withOffset(CGVector(dx: 140, dy: 0)))
        let deadline = Date().addingTimeInterval(5)
        while Date() < deadline, centre.frame.width < w0 + 100 { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        XCTAssertGreaterThan(centre.frame.width, w0 + 100, "the centre column widened by the drag: \(w0) to \(centre.frame.width)")
        XCTAssertEqual(header.frame.width, before, accuracy: 1, "the other column kept its width")
        shot("views-s3-ui-column-widened")
    }
}
