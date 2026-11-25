// Note 26 slice 5 on the iPad and iPhone: a histogram pops out as the core's chart, painted from
// its display list (the maintainer's decision 1A), with the core's ticks and titles, and the
// chart's accessibility summary. In a file of its own, beside GestureTests' helpers.

import XCTest

extension GestureTests {
    @MainActor
    func testAHistogramIsTheCoresChart() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "plot-graph.json"
        try #"""
        {"version": 1, "nodes": {
          "m": {"kind": "array.mandelbrot", "pos": [0, 0], "values": {"width": 384, "height": 256}},
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
        wait("the histogram as the core's chart", timeout: 150) { _ in
            (self.popout("h")?["viewer"] as? String) == "array.plot" && (self.popout("h")?["chartStatus"] as? String) == "ready"
        }
        let chart = app.descendants(matching: .any)["viewer-chart"].firstMatch
        XCTAssertTrue(chart.waitForExistence(timeout: 10), "the chart drawn")
        XCTAssertTrue((chart.value as? String ?? "").contains("256 columns of 256 rows"), "its summary for VoiceOver: \(String(describing: chart.value))")
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        shot("views-s5-ui-histogram")
    }
}
