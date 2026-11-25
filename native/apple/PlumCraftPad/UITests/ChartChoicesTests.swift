// Note 26 slice 7 on the iPad and iPhone: a table's chart, its kind chosen from the chart's own
// menu (each choice the core's `viewer.chart`, kept in the graph), and drawn again when the choice
// changes elsewhere: an undo through the control port leaves the value as it was and changes only
// the spec in the graph, which the slice 7 Mac launch found no Apple view noticed. In a file of its
// own, beside GestureTests' helpers.

import XCTest

extension GestureTests {
    @MainActor
    func testATablesChartIsChosenFromItsMenuAndFollowsAnUndo() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "chart-choices-graph.json"
        try #"""
        {"version": 1, "nodes": {
          "a": {"kind": "text.literal", "pos": [0, 0], "values": {"text": "region,product,amount\nnorth,apples,10.5\nsouth,apples,20.25\nnorth,pears,5.5\neast,plums,7.75\nsouth,pears,3.5\nnorth,plums,15.25\n"}},
          "t": {"kind": "text.text_to_table", "pos": [440, 0]}},
         "wires": [{"from": {"node": "a", "port": "text"}, "to": {"node": "t", "port": "text"}}]}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        let tokenFile = NSTemporaryDirectory() + "chart-choices-token-\(UUID().uuidString)"
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--control-token-file", tokenFile, "--then", "zoom:0.6:t"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        try popOut(app, canvas, "t")
        wait("the table popped out", timeout: 150) { _ in (self.popout("t")?["viewer"] as? String) == "table.grid" }
        let port = try XCTUnwrap(status()["port"] as? Int, "the control port")
        let token = try String(contentsOfFile: tokenFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        let control = try XCTUnwrap(ControlPort(port: port, token: token), "the control port answers")
        let pid = try XCTUnwrap(popout("t")?["id"] as? Int, "the pop-out's id")
        let doc = window()["doc"]
        XCTAssertNotNil(control.call("command.run", ["command": "view.viewer", "params": ["popout": pid, "viewer": "table.chart"]], doc: doc))
        let drawn = { ((self.popout("t")?["chart"] as? [String: Any])?["kind"] as? String) ?? "" }
        wait("the chart the core picked: amount by region", timeout: 60) { _ in drawn() == "column" }
        // a person's choice: the chart's kind menu, Stacked Column
        let kind = app.buttons["chart-kind"].firstMatch
        XCTAssertTrue(kind.waitForExistence(timeout: 10), "the kind menu")
        kind.tap()
        let stacked = app.buttons["Stacked Column"].firstMatch
        XCTAssertTrue(stacked.waitForExistence(timeout: 5), "Stacked Column in the menu")
        stacked.tap()
        wait("the stacked column chart drawn", timeout: 60) { _ in drawn() == "stacked column" }
        shot("views-s7-ui-stacked-column")
        // an undo through the control port: the value is as it was, only the graph's spec moved
        XCTAssertNotNil(control.call("command.run", ["command": "edit.undo"], doc: doc))
        wait("the chart drawn again after the undo", timeout: 60) { _ in drawn() == "column" }
        shot("views-s7-ui-after-undo")
        // note 26 slice 8: a drag across the plot brushes its x; the value's rows there are selected,
        // the core's selection (`viewer.select`); Clear Selection takes it away
        XCTAssertNotNil(control.call("command.run", ["command": "viewer.chart", "params": ["popout": pid, "mark": "histogram", "x": "amount", "bins": 6]], doc: doc))
        wait("the histogram drawn", timeout: 60) { _ in drawn() == "histogram" }
        let chart = app.descendants(matching: .any)["viewer-chart"].firstMatch
        XCTAssertTrue(chart.waitForExistence(timeout: 10))
        let selected = { ((self.popout("t")?["chart"] as? [String: Any])?["selected"] as? Int) ?? -1 }
        chart.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.5)).press(forDuration: 0.1, thenDragTo: chart.coordinate(withNormalizedOffset: CGVector(dx: 0.45, dy: 0.5)))
        wait("rows selected by the brush", timeout: 30) { _ in selected() > 0 }
        shot("views-s8-ui-brushed")
        let clear = app.buttons["chart-clear-selection"].firstMatch
        XCTAssertTrue(clear.waitForExistence(timeout: 10), "Clear Selection")
        clear.tap()
        wait("the selection cleared", timeout: 30) { _ in selected() == -1 }
    }
}
