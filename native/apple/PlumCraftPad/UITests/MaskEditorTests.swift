// The mask editor on the iPad (S4b, docs/shell-interface.md 12.22), driven by XCUITest in the
// simulator: Paint Mask's edit row opens the shell's mask editor, on the node and on a collapsed
// group's promoted row; its Run button runs the graph for a picture; one finger paints one stroke
// (one `mask.stroke`: the value names a new file), two fingers pinch without painting, and a
// two-finger tap on the canvas undoes the stroke. In a file of its own, beside GestureTests'
// helpers. Pictures to PLUMCRAFT_SHOTS.

import XCTest

extension GestureTests {
    func maskShown(_ key: String) -> [String: Any]? {
        let c = coreWindows()
        guard (c["shown"] as? String) == key, let r = c["report"] as? [String: Any] else { return nil }
        return r["native"] as? [String: Any]
    }

    @MainActor
    func testMaskEditor() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "mask-editor-graph.json"
        try? FileManager.default.removeItem(atPath: NSTemporaryDirectory() + "mask-editor-graph.edits")
        // a Mandelbrot to paint on; one Paint Mask on its own, another in a collapsed group with
        // its edit promoted
        try #"""
        {"version": 1, "nodes": {"m": {"kind": "array.mandelbrot", "pos": [0, 0]}, "c": {"kind": "array.paint_mask", "pos": [460, 0]},
          "d": {"kind": "array.paint_mask", "pos": [460, 520]}},
         "wires": [{"from": {"node": "m", "port": "escape"}, "to": {"node": "c", "port": "picture"}},
                   {"from": {"node": "m", "port": "escape"}, "to": {"node": "d", "port": "picture"}}],
         "groups": {"g1": {"name": "Touch Up", "nodes": ["d"], "collapsed": true, "pos": [460, 420],
          "promoted": [{"port": {"node": "d", "port": "edit"}, "label": "edit"}]}}}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--then", "zoom:0.8:c"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        func rowAt(_ face: String) -> [Double]? {
            guard let rows = (window()["rows"] as? [String: Any])?[face] as? [Any] else { return nil }
            return rows.compactMap { $0 as? [String: Any] }.first { ($0["key"] as? String) == "edit" }?["at"] as? [Double]
        }
        wait("the graph, ready, with both edit rows", timeout: 150) { _ in rowAt("c") != nil && rowAt("group:g1") != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))

        for (face, node, name) in [("c", "c", "node"), ("group:g1", "d", "group")] {
            let key = "editor:\(node)"
            reveal(app, canvas) { rowAt(face) }
            let at = try XCTUnwrap(rowAt(face))
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at[0], dy: at[1])).tap()
            wait("the mask editor from the \(name)'s row") { _ in (self.maskShown(key)?["loaded"] as? Bool) == true }
            // no picture before a run: the editor's Run
            if (maskShown(key)?["picture"] as? Bool) != true {
                let run = app.buttons["mask-run"]
                XCTAssertTrue(run.waitForExistence(timeout: 10))
                run.tap()
            }
            wait("the picture to paint on", timeout: 60) { _ in (self.maskShown(key)?["picture"] as? Bool) == true }
            let paint = app.descendants(matching: .any).matching(identifier: "mask-canvas").firstMatch
            XCTAssertTrue(paint.waitForExistence(timeout: 10), "the native editor's canvas")
            let undo0 = window()["undo"] as? String
            // two fingers: a pinch zooms and paints nothing
            paint.pinch(withScale: 1.6, velocity: 2)
            RunLoop.current.run(until: Date().addingTimeInterval(1))
            XCTAssertEqual(maskShown(key)?["sent"] as? Int, 0, "two fingers don't paint")
            XCTAssertEqual(window()["undo"] as? String, undo0)
            // one finger: a stroke, one command, a new file
            paint.coordinate(withNormalizedOffset: CGVector(dx: 0.35, dy: 0.4)).press(forDuration: 0.05, thenDragTo: paint.coordinate(withNormalizedOffset: CGVector(dx: 0.65, dy: 0.6)))
            wait("one mask.stroke, the value a file", timeout: 30) { _ in
                (self.maskShown(key)?["sent"] as? Int) == 1 && ((self.maskShown(key)?["value"] as? String)?.hasSuffix(".png") ?? false)
            }
            XCTAssertFalse(app.otherElements["Choose a Folder for Mask Files"].exists, "beside the graph: no folder asked")
            wait("the stroke, one undo step") { ($0["undo"] as? String) == "Mask Stroke" }
            XCTAssertGreaterThan(maskShown(key)?["masked"] as? Int ?? 0, 0)
            shot("s4b-ui-mask-\(name)-stroked")
            // a two-finger tap undoes it
            paint.tap(withNumberOfTaps: 1, numberOfTouches: 2)
            wait("undone: no edit") { _ in (self.maskShown(key)?["value"] as? String) == "" }
            app.buttons["report-done"].tap()
            wait("the editor closed") { _ in self.coreWindows()["shown"] == nil || self.coreWindows()["shown"] is NSNull }
        }
    }

    /// S4b-3 (decision 4A): a graph in a folder the app can't write has nowhere to put mask files,
    /// so a person's first stroke brings up the system's folder picker (`folder.pick`, purpose
    /// `edits`); nothing is set until a folder is chosen.
    @MainActor
    func testMaskAsksForAFolder() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let dir = NSTemporaryDirectory() + "mask-read-only"
        let fm = FileManager.default
        if fm.fileExists(atPath: dir) { try? fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir); try? fm.removeItem(atPath: dir) }
        try fm.createDirectory(atPath: dir, withIntermediateDirectories: true)
        let graph = dir + "/locked.json"
        try #"""
        {"version": 1, "nodes": {"m": {"kind": "array.mandelbrot", "pos": [0, 0]}, "c": {"kind": "array.paint_mask", "pos": [460, 0]}},
         "wires": [{"from": {"node": "m", "port": "escape"}, "to": {"node": "c", "port": "picture"}}]}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        try fm.setAttributes([.posixPermissions: 0o555], ofItemAtPath: dir)
        defer { try? fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir) }
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--then", "zoom:0.8:c"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        func rowAt() -> [Double]? {
            guard let rows = (window()["rows"] as? [String: Any])?["c"] as? [Any] else { return nil }
            return rows.compactMap { $0 as? [String: Any] }.first { ($0["key"] as? String) == "edit" }?["at"] as? [Double]
        }
        wait("the graph, ready", timeout: 150) { _ in rowAt() != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        reveal(app, canvas) { rowAt() }
        let at = try XCTUnwrap(rowAt())
        canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at[0], dy: at[1])).tap()
        wait("the mask editor") { _ in (self.maskShown("editor:c")?["loaded"] as? Bool) == true }
        if (maskShown("editor:c")?["picture"] as? Bool) != true {
            let run = app.buttons["mask-run"]
            XCTAssertTrue(run.waitForExistence(timeout: 10))
            run.tap()
        }
        wait("the picture to paint on", timeout: 60) { _ in (self.maskShown("editor:c")?["picture"] as? Bool) == true }
        let paint = app.descendants(matching: .any).matching(identifier: "mask-canvas").firstMatch
        XCTAssertTrue(paint.waitForExistence(timeout: 10))
        paint.coordinate(withNormalizedOffset: CGVector(dx: 0.4, dy: 0.4)).press(forDuration: 0.05, thenDragTo: paint.coordinate(withNormalizedOffset: CGVector(dx: 0.6, dy: 0.5)))
        // the system's folder picker, for the mask files
        let deadline = Date().addingTimeInterval(30)
        var picking: [String: Any]?
        while Date() < deadline {
            picking = status()["picking"] as? [String: Any]
            if picking != nil { break }
            RunLoop.current.run(until: Date().addingTimeInterval(0.3))
        }
        XCTAssertEqual(picking?["purpose"] as? String, "edits", "the folder picker for mask files: \(String(describing: picking))")
        XCTAssertEqual(picking?["kind"] as? String, "folder.pick")
        RunLoop.current.run(until: Date().addingTimeInterval(2))
        shot("s4b-ui-mask-folder-ask")
        XCTAssertEqual(maskShown("editor:c")?["value"] as? String ?? "", "", "nothing set until a folder is chosen")
    }
}
