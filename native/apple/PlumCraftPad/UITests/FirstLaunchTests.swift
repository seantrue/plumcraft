// B147 (the maintainer, 2026-11-23: "iPad started In unwritable folder. Files / Plummer seems a
// better starting place"; what showed it: "failed save"): a person's first launch from a fresh
// install (run by claude-debug's fresh.sh, which uninstalls the app first so its container is
// new). The sample it opens, the document browser it goes back to (pictured: where it starts),
// then New Graph pressed from On My iPad's top folder, outside the app's container: the graph is
// made in the app's own folder (Documents, which the Files app shows as Plummer), a node is added,
// and the saved file there holds it. Pictures at each step.
//
// FR2 (the maintainer: the default graph file extension .json -> .plum, .json still accepted): the
// new graph is "Untitled.plum"; a graph opened from a .json file is saved back to its .json.

import XCTest

extension GestureTests {
    @MainActor
    func testANewGraphIsMadeAndSavedInTheAppsFolder() throws {
        let env = ProcessInfo.processInfo.environment
        // only from a fresh install (a first launch is the test): fresh.sh uninstalls the app and
        // says so; in a run over an installed app this is skipped
        try XCTSkipUnless(env["PLUMCRAFT_FRESH_INSTALL"] == "1", "not a fresh install (claude-debug/n4-decided-2/fresh.sh)")
        statusPath = statusFilePath(env)
        let app = XCUIApplication()
        // as a person launches it: no sample named, no scratch state; only the status file
        app.launchArguments = ["--status-file", statusPath]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the first launch's sample", timeout: 150) { ($0["ready"] as? Bool) == true }
        shot("b147-1-first-launch")
        // back to the browser, as a person closes the sample
        let back = app.buttons["BackButton"].exists ? app.buttons["BackButton"] : app.navigationBars.buttons.firstMatch
        XCTAssertTrue(back.waitForExistence(timeout: 10), "the graph's back button")
        back.tap()
        let create = app.buttons["New Graph"].firstMatch
        XCTAssertTrue(create.waitForExistence(timeout: 15), "the browser's New Graph")
        RunLoop.current.run(until: Date().addingTimeInterval(2))
        shot("b147-2-browser")
        // where a person might stand: Browse, then up out of the app's folder to On My iPad's top
        if app.buttons["Browse"].firstMatch.exists {
            app.buttons["Browse"].firstMatch.tap()
            RunLoop.current.run(until: Date().addingTimeInterval(2))
            // the sidebar's places: On My iPad, the folder that holds the app's own (Plummer)
            if app.buttons["Toggle sidebar"].firstMatch.exists { app.buttons["Toggle sidebar"].firstMatch.tap() }
            RunLoop.current.run(until: Date().addingTimeInterval(2))
            let mine = NSPredicate(format: "label BEGINSWITH 'On My'")
            let place = app.cells.matching(mine).firstMatch.exists ? app.cells.matching(mine).firstMatch
                : app.buttons.matching(mine).firstMatch.exists ? app.buttons.matching(mine).firstMatch : app.staticTexts.matching(mine).firstMatch
            print("B147: the sidebar's place \(place.exists ? place.label : "not found")")
            if place.exists { place.tap() }
            RunLoop.current.run(until: Date().addingTimeInterval(2))
            // the sidebar put away again, so New Graph is in reach
            if app.buttons["Toggle sidebar"].firstMatch.exists, !create.isHittable { app.buttons["Toggle sidebar"].firstMatch.tap() }
            RunLoop.current.run(until: Date().addingTimeInterval(1))
            shot("b147-3-on-my-ipad")
        }
        create.tap()
        // the new graph, open, in the app's own folder
        var path = ""
        let opened = Date().addingTimeInterval(30)
        while Date() < opened {
            path = (window()["path"] as? String) ?? ""
            if window()["ready"] != nil, path.hasSuffix(".plum") { break }
            RunLoop.current.run(until: Date().addingTimeInterval(0.5))
        }
        shot("b147-4-new-graph")
        print("B147: the new graph at \(path); createError \(status()["createError"] ?? "-")")
        XCTAssertTrue(path.contains("/Documents/Untitled"), "made in the app's Documents (Plummer in Files): \(path)")
        XCTAssertTrue(path.hasSuffix(".plum"), "FR2: a new graph is a .plum: \(path)")
        // make something: a node from the palette (a double tap on empty canvas)
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).doubleTap()
        let field = app.textFields["PaletteSearch"]
        XCTAssertTrue(field.waitForExistence(timeout: 10), "the palette")
        field.tap()
        field.typeText("Upper")
        let row = app.cells.firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10), "a kind in the palette")
        row.tap()
        // saved: the file in the app's folder holds the node (autosave, a moment after the edit)
        var nodes = 0
        let saved = Date().addingTimeInterval(30)
        while Date() < saved, nodes == 0 {
            if let d = FileManager.default.contents(atPath: path), let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any] {
                nodes = (o["nodes"] as? [String: Any])?.count ?? 0
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.5))
        }
        shot("b147-5-saved")
        XCTAssertGreaterThan(nodes, 0, "the saved file in the app's folder holds the new node: \(path)")
        XCTAssertNil(status()["createError"], "no failure to make the graph")
    }

    /// FR2: an existing `.json` graph opens (by its content), and an edit is saved back to its own
    /// `.json`, with no `.plum` written beside it.
    @MainActor
    func testAJsonGraphOpensAndSavesBackToItsJson() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        // a graph the app can read: beside the status file, a folder of the Mac's
        let dir = (statusPath as NSString).deletingLastPathComponent + "/fr2-json"
        try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true)
        let graph = dir + "/kept.json"
        try? FileManager.default.removeItem(atPath: dir + "/kept.plum")
        try #"{"version": 1, "nodes": {"a": {"kind": "text.literal", "pos": [0, 0], "values": {"text": "kept"}}}, "wires": []}"#
            .write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the .json graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true }
        let path = (window()["path"] as? String) ?? ""
        print("FR2: opened \(path)")
        XCTAssertTrue(path.hasSuffix("/kept.json"), "the core holds the .json: \(path)")
        shot("fr2-1-json-open")
        // an edit: a node from the palette (a double tap on empty canvas)
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        if let p = emptyCanvas(app, canvas) {
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: p.x, dy: p.y)).doubleTap()
        } else {
            canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.7, dy: 0.7)).doubleTap()
        }
        let field = app.textFields["PaletteSearch"]
        XCTAssertTrue(field.waitForExistence(timeout: 10), "the palette")
        field.tap()
        field.typeText("Upper")
        let row = app.cells.firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10), "a kind in the palette")
        row.tap()
        // saved back to the .json (autosave, a moment after the edit)
        var nodes = 0
        let saved = Date().addingTimeInterval(30)
        while Date() < saved, nodes < 2 {
            if let d = FileManager.default.contents(atPath: graph), let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any] {
                nodes = (o["nodes"] as? [String: Any])?.count ?? 0
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.5))
        }
        shot("fr2-2-json-saved")
        XCTAssertEqual(nodes, 2, "the edit is in the .json itself: \(graph)")
        XCTAssertFalse(FileManager.default.fileExists(atPath: dir + "/kept.plum"), "no .plum beside it")
        XCTAssertTrue(((window()["path"] as? String) ?? "").hasSuffix("/kept.json"), "still the .json")
    }
}
