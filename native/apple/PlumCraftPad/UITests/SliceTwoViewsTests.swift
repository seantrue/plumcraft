// Note 26 slice 2 on the iPad and iPhone, driven by XCUITest in the simulator: an array's two
// channel pictures share one zoom (asked of the core as a script asks, `viewer.zoom`, since
// XCUITest's pinch doesn't reach a scroll view in a sheet), a histogram is a table first and its
// Colour by Value is the core's `viewer.table {colored}`, and an array's grid takes a number
// format and colour (`viewer.grid`). In a file of its own, beside GestureTests' helpers.

import XCTest

extension GestureTests {
    func popout(_ node: String) -> [String: Any]? {
        ((window()["popouts"] as? [[String: Any]]) ?? []).first { ($0["node"] as? String) == node }
    }

    /// Pop a node's first output out by a double tap on its pin.
    @MainActor
    func popOut(_ app: XCUIApplication, _ canvas: XCUIElement, _ node: String) throws {
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        let out = try XCTUnwrap(reachable(app, canvas, node, "out", 0), "\(node)'s pin")
        let pin = origin.withOffset(CGVector(dx: out[0], dy: out[1]))
        pin.doubleTap()
        for _ in 0..<2 {
            let deadline = Date().addingTimeInterval(8)
            while Date() < deadline, popout(node) == nil { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            if popout(node) == nil { pin.doubleTap() }
        }
    }

    @MainActor
    func testSliceTwoViews() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "slice-two-graph.json"
        try #"""
        {"version": 1, "nodes": {
          "n": {"kind": "array.simplex_noise", "pos": [0, 0], "values": {"seed": 1, "width": 384, "height": 256, "cells": 6.0}},
          "m": {"kind": "array.mandelbrot", "pos": [0, 320], "values": {"width": 384, "height": 256}},
          "s": {"kind": "array.stack", "pos": [440, 0], "values": {"space": "none"}},
          "h": {"kind": "array.histogram", "pos": [440, 320]}},
         "wires": [{"from": {"node": "n", "port": "noise"}, "to": {"node": "s", "port": "c1"}},
                   {"from": {"node": "m", "port": "escape"}, "to": {"node": "s", "port": "c2"}},
                   {"from": {"node": "m", "port": "escape"}, "to": {"node": "h", "port": "array"}}]}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let tokenFile = NSTemporaryDirectory() + "slice-two-token-\(UUID().uuidString)"
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--control-token-file", tokenFile, "--then", "zoom:0.5:s"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let port = try XCTUnwrap(status()["port"] as? Int, "the control port")
        let token = try String(contentsOfFile: tokenFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        let control = try XCTUnwrap(ControlPort(port: port, token: token))

        // two channel pictures, one zoom
        try popOut(app, canvas, "s")
        wait("two pictures drawn", timeout: 150) { _ in ((self.popout("s")?["pictures"] as? Int) ?? 0) >= 2 && self.popout("s")?["zoom"] != nil }
        let sid = try XCTUnwrap(popout("s")?["id"] as? Int)
        let px = Int((UIScreen.main.scale * 2).rounded())
        XCTAssertNotNil(control.call("command.run", ["command": "viewer.zoom", "params": ["popout": sid, "to": px, "at": [64, 96]]], doc: window()["doc"]))
        wait("both at 200%", timeout: 30) { _ in ((self.popout("s")?["zoom"] as? [String: Any])?["percent"] as? Double) == 200 }
        XCTAssertTrue(app.descendants(matching: .any)["pictures-tile-0"].exists && app.descendants(matching: .any)["pictures-tile-1"].exists, "two tiles")
        shot("views-s2-ui-pictures-200")
        app.buttons["popout-done"].tap()
        wait("closed") { (($0["popouts"] as? [[String: Any]]) ?? []).isEmpty }

        // the histogram: a table first; Colour by Value
        try popOut(app, canvas, "h")
        wait("the histogram as a table", timeout: 150) { _ in (self.popout("h")?["viewer"] as? String) == "table.grid" }
        let colour = app.descendants(matching: .any)["viewer-colour"].firstMatch
        XCTAssertTrue(colour.waitForExistence(timeout: 20), "Colour by Value")
        colour.tap()
        let hid = try XCTUnwrap(popout("h")?["id"] as? Int)
        var colored = false
        for _ in 0..<50 where !colored {
            colored = ((control.call("popout.get", ["id": hid], doc: window()["doc"]) as? [String: Any])?["state"] as? [String: Any]).flatMap { ($0["table"] as? [String: Any])?["colored"] as? Bool } == true
            if !colored { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        }
        XCTAssertTrue(colored, "the tap is the core's viewer.table {colored}")
        RunLoop.current.run(until: Date().addingTimeInterval(1.5))
        shot("views-s2-ui-table-colour")
        app.buttons["popout-done"].tap()
    }
}
