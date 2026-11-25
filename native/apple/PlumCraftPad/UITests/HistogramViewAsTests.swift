// FR6 slice 3 on the iPad, the maintainer's instance as a person does it: a 3-D array's port (a
// stack of two planes) pops out, View As is tapped and Histogram picked, and every one of its
// values is binned, drawn as the core's chart, no node added. In a file of its own, beside
// GestureTests' helpers.

import XCTest

extension GestureTests {
    @MainActor
    func testA3DArrayShowsAHistogramThroughViewAs() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "histogram-view-as-graph.json"
        try #"""
        {"version": 1, "nodes": {
          "n": {"kind": "array.simplex_noise", "pos": [0, 0], "values": {"seed": 1, "width": 384, "height": 256, "cells": 6.0}},
          "m": {"kind": "array.mandelbrot", "pos": [0, 320], "values": {"width": 384, "height": 256}},
          "s": {"kind": "array.stack", "pos": [440, 0], "values": {"space": "none"}}},
         "wires": [{"from": {"node": "n", "port": "noise"}, "to": {"node": "s", "port": "c1"}},
                   {"from": {"node": "m", "port": "escape"}, "to": {"node": "s", "port": "c2"}}]}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--then", "zoom:0.6:s"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let nodes = (window()["points"] as? [String: Any])?.keys.sorted()
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        try popOut(app, canvas, "s")
        wait("the stack's pictures", timeout: 150) { _ in (self.popout("s")?["viewer"] as? String) == "array.pictures" }
        // View As, as a person picks it: the menu, then its Histogram item
        let viewAs = app.descendants(matching: .any)["viewer-as"].firstMatch
        XCTAssertTrue(viewAs.waitForExistence(timeout: 10), "View As in the bar")
        viewAs.tap()
        let item = app.buttons["Histogram"].firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: 10), "Histogram in View As")
        item.tap()
        wait("the histogram of every value, drawn", timeout: 150) { _ in
            (self.popout("s")?["viewer"] as? String) == "view.histogram" && (self.popout("s")?["chartStatus"] as? String) == "ready"
        }
        let chart = app.descendants(matching: .any)["viewer-chart"].firstMatch
        XCTAssertTrue(chart.waitForExistence(timeout: 10), "the chart drawn")
        let line = app.descendants(matching: .any)["chart-derived"].firstMatch
        XCTAssertTrue(line.waitForExistence(timeout: 10), "what was binned, said")
        XCTAssertTrue(line.label.contains("256 bins of all 196608 values"), "every value of the (256, 384, 2) array: \(line.label)")
        XCTAssertEqual((window()["points"] as? [String: Any])?.keys.sorted(), nodes, "no node added")
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        shot("fr6-s3-ui-histogram")
    }
}
