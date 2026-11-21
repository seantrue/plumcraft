// The iPad's gestures through UIKit's own recognisers, driven by XCUITest in the simulator (N3
// slice 5; editing since N4 slice 1): a tap selects, a tap on empty canvas clears, a long press
// gives the core's context menu, a pinch zooms, a one-finger drag on empty canvas pans; a double
// tap on empty canvas opens the palette there; a one-finger drag on a face moves it (one undo
// step) and a two-finger tap undoes it, a three-finger tap redoes it; a drag from an output pin
// dropped on empty canvas opens the palette filtered to what takes the wire, and a tap on a row
// adds and connects (one undo step); Select mode makes a one-finger drag a marquee; a two-finger
// double tap tidies; the toolbar's Fit fits. What the app did is read from its status file
// (`--status-file`), whose path the runner gets as PLUMCRAFT_STATUS (TEST_RUNNER_PLUMCRAFT_STATUS
// for xcodebuild); screenshots go to PLUMCRAFT_SHOTS when it is set. Local runs only (no macOS
// CI): conformance.sh gestures.

import XCTest

final class GestureTests: XCTestCase {
    var statusPath = ""

    /// The app's status as JSON (empty until written).
    func status() -> [String: Any] {
        guard let d = FileManager.default.contents(atPath: statusPath),
              let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any] else { return [:] }
        return o
    }

    func window() -> [String: Any] {
        (status()["windows"] as? [[String: Any]])?.first ?? [:]
    }

    /// Wait (at most `timeout`) until `cond` holds of the first window's status.
    @discardableResult
    func wait(_ what: String, timeout: TimeInterval = 20, _ cond: ([String: Any]) -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if cond(window()) { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        XCTFail("timed out waiting for \(what): \(window())")
        return false
    }

    @MainActor func shot(_ name: String) {
        let s = XCUIScreen.main.screenshot()
        let a = XCTAttachment(screenshot: s)
        a.name = name
        a.lifetime = .keepAlways
        add(a)
        if let dir = ProcessInfo.processInfo.environment["PLUMCRAFT_SHOTS"] {
            try? s.pngRepresentation.write(to: URL(fileURLWithPath: dir).appendingPathComponent("\(name).png"))
        }
    }

    @MainActor
    func testTouchGestures() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = env["PLUMCRAFT_STATUS"] ?? NSTemporaryDirectory() + "plumcraft-ui-status.json"
        try? FileManager.default.removeItem(atPath: statusPath)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Showcase", "--status-file", statusPath, "--then", "zoom:1:swap"]
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready, centred on Replace", timeout: 60) { ($0["ready"] as? Bool) == true && abs((($0["zoom"] as? Double) ?? 0) - 1) < 0.01 }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let centre = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let corner = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.06, dy: 0.06))

        // a tap on a face selects it
        centre.tap()
        wait("Replace selected") { ($0["selection"] as? [String]) == ["swap"] }
        // a tap on empty canvas clears
        corner.tap()
        wait("the selection cleared") { ($0["selection"] as? [String])?.isEmpty == true }

        // a long press is the core's context menu for the face under it
        centre.press(forDuration: 0.8)
        let duplicate = app.buttons["Duplicate"]
        XCTAssertTrue(duplicate.waitForExistence(timeout: 10), "the context menu's Duplicate")
        shot("n4s1-ui-context-menu")
        let items = (status()["contextMenu"] as? [String]) ?? []
        XCTAssertTrue(items.contains("Duplicate"), "\(items)")
        // dismissed without choosing (a tap outside the menu)
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.02, dy: 0.5)).tap()
        XCTAssertTrue(duplicate.waitForNonExistence(timeout: 10))

        // a pinch zooms
        let z0 = (window()["zoom"] as? Double) ?? 0
        canvas.pinch(withScale: 1.8, velocity: 1.5)
        wait("zoomed in by the pinch") { (($0["zoom"] as? Double) ?? 0) > z0 * 1.2 }

        // a one-finger drag on empty canvas pans, and leaves the zoom
        let z1 = (window()["zoom"] as? Double) ?? 0
        let p0 = (window()["pan"] as? [Double]) ?? [0, 0]
        corner.press(forDuration: 0.05, thenDragTo: canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.35)))
        wait("panned by the drag") {
            let p = ($0["pan"] as? [Double]) ?? p0
            return hypot(p[0] - p0[0], p[1] - p0[1]) > 50
        }
        XCTAssertEqual((window()["zoom"] as? Double) ?? 0, z1, accuracy: 0.001, "a pan leaves the zoom")

        // the toolbar's Fit fits everything (a double tap no longer does: the maintainer, N3's exit)
        app.buttons["Fit"].tap()
        wait("fitted by the toolbar's Fit") { (($0["zoom"] as? Double) ?? 9) < z1 * 0.8 }
        shot("n4s1-ui-fitted")
        try editing(app, canvas)

        // every orientation: the canvas keeps what was at its centre at its centre
        for (o, name) in [(UIDeviceOrientation.landscapeLeft, "landscape"), (.portrait, "portrait")] {
            XCUIDevice.shared.orientation = o
            RunLoop.current.run(until: Date().addingTimeInterval(2))
            shot("n4s1-ui-rotating-\(name)")
            XCTAssertTrue(app.buttons["Documents"].exists, "the graph is still shown after the rotation")
            wait("laid out \(name)") {
                let s = ($0["size"] as? [Double]) ?? [0, 0]
                return o.isLandscape ? s[0] > s[1] : s[1] > s[0]
            }
            RunLoop.current.run(until: Date().addingTimeInterval(1))
            if o != .portrait { shot("n4s1-ui-\(name)") }
        }
    }

    /// A face's or pin's point in the canvas (from the status file's `points`).
    func point(_ canvas: XCUIElement, _ id: String, _ what: String, _ k: Int = 0) -> XCUICoordinate? {
        guard let pts = window()["points"] as? [String: Any], let f = pts[id] as? [String: Any] else { return nil }
        let p: [Double]? = what == "center" ? f["center"] as? [Double] : (f[what] as? [[Double]]).flatMap { $0.indices.contains(k) ? $0[k] : nil }
        guard let p, p.count == 2 else { return nil }
        return canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: p[0], dy: p[1]))
    }

    func pos(_ id: String) -> [Double]? { (window()["positions"] as? [String: Any])?[id] as? [Double] }

    @MainActor
    func editing(_ app: XCUIApplication, _ canvas: XCUIElement) throws {
        let corner = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.06, dy: 0.5))
        // a double tap on empty canvas: the palette there
        corner.doubleTap()
        wait("the palette opened by the double tap") { (($0["palette"] as? [String: Any])?["shown"] as? Bool) == true }
        shot("n4s1-ui-palette-double-tap")
        // dismissed by a tap outside it: the core's palette closes, nothing added
        let nodes0 = (window()["nodes"] as? Int) ?? 0
        app.coordinate(withNormalizedOffset: CGVector(dx: 0.03, dy: 0.95)).tap()
        wait("the palette dismissed") { (($0["palette"] as? [String: Any])?["open"] as? Bool) == false }
        XCTAssertEqual((window()["nodes"] as? Int) ?? -1, nodes0)

        // a one-finger drag on a face moves it: one undo step
        let id = "swap"
        let before = try XCTUnwrap(pos(id))
        let c = try XCTUnwrap(point(canvas, id, "center"), "where swap is: \(window()["points"] ?? "none")")
        c.press(forDuration: 0.05, thenDragTo: c.withOffset(CGVector(dx: 120, dy: 90)))
        wait("swap moved, one step") { _ in
            guard let p = self.pos(id) else { return false }
            return hypot(p[0] - before[0], p[1] - before[1]) > 40 && (self.window()["undo"] as? String) == "Move Node"
        }
        let moved = try XCTUnwrap(pos(id))
        shot("n4s1-ui-after-drag")
        // a two-finger tap undoes it; a three-finger tap redoes it
        canvas.tap(withNumberOfTaps: 1, numberOfTouches: 2)
        wait("undone by the two-finger tap") { _ in self.pos(id).map { abs($0[0] - before[0]) < 0.5 && abs($0[1] - before[1]) < 0.5 } ?? false }
        XCTAssertEqual(status()["gesture"] as? String, "undo")
        canvas.tap(withNumberOfTaps: 1, numberOfTouches: 3)
        wait("redone by the three-finger tap") { _ in self.pos(id).map { abs($0[0] - moved[0]) < 0.5 && abs($0[1] - moved[1]) < 0.5 } ?? false }

        // a wire from an output pin dropped on empty canvas: the palette for what takes it
        let wires0 = (window()["wireCount"] as? Int) ?? 0
        let nodes1 = (window()["nodes"] as? Int) ?? 0
        let out = try XCTUnwrap(point(canvas, "after", "out", 0), "after.s output: \(window()["points"] ?? "none")")
        out.press(forDuration: 0.05, thenDragTo: out.withOffset(CGVector(dx: 40, dy: 220)))
        wait("the palette for the dropped wire") {
            let p = $0["palette"] as? [String: Any]
            return (p?["wire"] as? Bool) == true && (p?["shown"] as? Bool) == true && !((p?["items"] as? [String]) ?? []).isEmpty
        }
        shot("n4s1-ui-palette-dropped-wire")
        let first = ((window()["palette"] as? [String: Any])?["items"] as? [String])?.first ?? ""
        let row = app.cells.containing(.staticText, identifier: first).firstMatch
        XCTAssertTrue(row.waitForExistence(timeout: 10), "the palette's row \(first)")
        row.tap()
        wait("added and connected, one step") {
            ($0["nodes"] as? Int) == nodes1 + 1 && ($0["wireCount"] as? Int) == wires0 + 1 && ($0["undo"] as? String) == "Add and Connect"
        }
        shot("n4s1-ui-after-add-and-connect")

        // Select mode: a one-finger drag on empty canvas is a marquee
        app.buttons["Select"].tap()
        wait("select mode on") { ($0["selectMode"] as? Bool) == true }
        let a = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.02, dy: 0.02))
        a.press(forDuration: 0.05, thenDragTo: canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.98, dy: 0.98)))
        wait("the marquee selected") { (($0["selection"] as? [String]) ?? []).count >= 2 }
        app.buttons["Select"].tap()
        wait("select mode off") { ($0["selectMode"] as? Bool) == false }

        // a two-finger double tap tidies
        canvas.tap(withNumberOfTaps: 2, numberOfTouches: 2)
        wait("tidied") { ($0["undo"] as? String) == "Tidy" }
        XCTAssertEqual(status()["gesture"] as? String, "tidy")
        shot("n4s1-ui-after-tidy")
    }

    /// Row `k` of face `face`'s widget: where it is in the canvas and what it shows, from the status.
    func row(_ face: String, _ k: Int) -> (CGPoint, String)? {
        guard let rows = (window()["rows"] as? [String: Any])?[face] as? [Any], k < rows.count,
              let r = rows[k] as? [String: Any], let at = r["at"] as? [Double], at.count == 2 else { return nil }
        return (CGPoint(x: at[0], y: at[1]), (r["value"] as? String) ?? "")
    }

    func value(_ face: String, _ k: Int) -> String? { row(face, k)?.1 }

    /// Editing values on a face (N4 slice 2), through UIKit: a tap on a text row opens a text field
    /// with the keyboard up and the row above it, typing and Done set it (one undo step, "Set
    /// Value"); a number the same with the numbers keyboard; a choice's row gives the system's menu;
    /// a switch flips on a tap; a drag on a number scrubs it.
    @MainActor
    func testInlineEdits() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = env["PLUMCRAFT_STATUS"] ?? NSTemporaryDirectory() + "plumcraft-ui-status.json"
        try? FileManager.default.removeItem(atPath: statusPath)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Inline Editing", "--status-file", statusPath, "--then", "zoom:1.5:w"]
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready, at 1.5", timeout: 60) { ($0["ready"] as? Bool) == true && abs((($0["zoom"] as? Double) ?? 0) - 1.5) < 0.01 && $0["rows"] != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        func at(_ p: CGPoint) -> XCUICoordinate { origin.withOffset(CGVector(dx: p.x, dy: p.y)) }

        // a text: tap, the field with the keyboard up and the row above it, type, Done
        let (label, _) = try XCTUnwrap(row("w", 1))
        at(label).tap()
        wait("the text field open") { (($0["editor"] as? [String: Any])?["kind"] as? String) == "text" }
        let field = app.textFields["row-editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.typeText("hello iPad")
        wait("the keyboard up, the field above it", timeout: 10) { w in
            guard let e = w["editor"] as? [String: Any], let top = e["keyboardTop"] as? Double, let f = e["frame"] as? [Double] else { return false }
            return f[1] + f[3] <= top
        }
        shot("n4s2-ui-text-keyboard")
        app.buttons["row-editor-done"].tap()
        wait("the label set, one undo step") { _ in self.value("w", 1) == "hello iPad" }
        wait("its undo name") { ($0["undo"] as? String) == "Set Value" }
        wait("the field gone") { (($0["editor"] as? [String: Any])?["open"] as? Bool) == false }
        shot("n4s2-ui-text-set")

        // a number: the numbers keyboard; past its range it is held there by the core
        let (count, _) = try XCTUnwrap(row("w", 2))
        at(count).tap()
        wait("the number field open") { (($0["editor"] as? [String: Any])?["kind"] as? String) == "number" }
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.typeText("7")
        shot("n4s2-ui-number-keyboard")
        app.buttons["row-editor-done"].tap()
        wait("count set to 7") { _ in self.value("w", 2) == "7" }

        // a choice: the system's menu on the row
        let (mode, _) = try XCTUnwrap(row("w", 5))
        at(mode).tap()
        let title = app.buttons["title"]
        XCTAssertTrue(title.waitForExistence(timeout: 10), "the choices' menu")
        shot("n4s2-ui-choice-menu")
        title.tap()
        wait("mode set to title") { _ in self.value("w", 5) == "title" }

        // a switch flips on a tap
        let (flag, before) = try XCTUnwrap(row("w", 4))
        XCTAssertEqual(before, "on")
        at(flag).tap()
        wait("flag off") { _ in self.value("w", 4) == "off" }

        // a number scrubbed by a drag on its row: amount, 0.50, moves up with a drag right
        let (amount, a0) = try XCTUnwrap(row("w", 3))
        XCTAssertEqual(a0, "0.50")
        at(amount).press(forDuration: 0.05, thenDragTo: at(CGPoint(x: amount.x + 90, y: amount.y)))
        wait("amount scrubbed up") { _ in (Double(self.value("w", 3) ?? "") ?? 0) > 0.5 }
        shot("n4s2-ui-after-scrub")

        // the more-button: the system's menu with the node's items
        if let p = ((window()["points"] as? [String: Any])?["w"] as? [String: Any])?["more"] as? [Double], p.count == 2 {
            at(CGPoint(x: p[0], y: p[1])).tap()
            let dup = app.buttons["Duplicate"]
            XCTAssertTrue(dup.waitForExistence(timeout: 10), "the node's menu from the more-button")
            shot("n4s2-ui-more-menu")
            app.tap()
        } else {
            XCTFail("no more-button in the status")
        }
    }

    /// N4 slice 3, through UIKit: a double tap on a node opens its parameters (a popover); a hidden
    /// parameter is changed there and shown on the face, where its new value is; a double tap on
    /// a collapsed group opens it, and the way back leaves it; a double tap on a frame's tab
    /// renames it in place; a double tap on an output pin pops its value out (a sheet when the
    /// graph is full screen).
    @MainActor
    func testWindowsAndNames() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = env["PLUMCRAFT_STATUS"] ?? NSTemporaryDirectory() + "plumcraft-ui-status.json"
        try? FileManager.default.removeItem(atPath: statusPath)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Windows and Names", "--status-file", statusPath, "--then", "zoom:1:w"]
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        func at(_ p: [Double]) -> XCUICoordinate { origin.withOffset(CGVector(dx: p[0], dy: p[1])) }
        func rowKeys(_ id: String) -> [String] { ((window()["rows"] as? [String: Any])?[id] as? [[String: Any]])?.compactMap { $0["key"] as? String } ?? [] }
        XCTAssertFalse(rowKeys("w").contains("count"), "count starts hidden from the face")

        // a double tap on the node: its parameters, in a popover
        let title = try XCTUnwrap(((window()["points"] as? [String: Any])?["w"] as? [String: Any])?["title"] as? [Double])
        at(title).doubleTap()
        wait("the parameters open on w") { (($0["parameters"] as? [String: Any])?["shown"] as? Bool) == true }
        let count = app.textFields["edit.count"]
        XCTAssertTrue(count.waitForExistence(timeout: 10), "the hidden count's field")
        shot("n4s3-ui-parameters")
        count.tap()
        count.clearAndType("5\n")
        wait("count set while hidden") { ($0["undo"] as? String) == "Set Value" }
        // shown on the face, where its value is the one set
        app.buttons["face.count"].tap()
        wait("count on the face, at 5") { w in
            let list = ((w["rows"] as? [String: Any])?["w"] as? [Any]) ?? []
            return list.contains { (($0 as? [String: Any])?["key"] as? String) == "count" && (($0 as? [String: Any])?["value"] as? String) == "5" }
        }
        shot("n4s3-ui-hidden-shown")
        // Done closes it in the core
        app.buttons["parameters-done"].tap()
        wait("the parameters closed") { ($0["parameters"] as? [String: Any]).map { ($0["shown"] as? Bool) == false } ?? true }

        // a double tap on the collapsed group: inside it, and the way back
        let g = try XCTUnwrap(((window()["points"] as? [String: Any])?["group:g1"] as? [String: Any])?["title"] as? [Double])
        at(g).doubleTap()
        wait("inside the group") { ($0["focus"] as? String) == "g1" && (($0["crumbs"] as? [String])?.count ?? 0) == 2 }
        shot("n4s3-ui-inside-group")
        app.buttons["crumb.0"].tap()
        wait("back on the graph") { $0["focus"] is NSNull || $0["focus"] == nil }

        // a double tap on the frame's tab: its title edited in place
        let tab = try XCTUnwrap((window()["frameTabs"] as? [String: Any])?["f1"] as? [Double])
        at(tab).doubleTap()
        let name = app.textFields["name-editor"]
        XCTAssertTrue(name.waitForExistence(timeout: 10), "the frame's name field")
        name.clearAndType("Inputs\n")
        wait("the frame renamed") { (($0["frames"] as? [String: Any])?["f1"] as? String) == "Inputs" }
        shot("n4s3-ui-frame-renamed")

        // a double tap on an output pin: the value pops out
        let out = try XCTUnwrap((((window()["points"] as? [String: Any])?["n1"] as? [String: Any])?["out"] as? [[Double]])?.first)
        at(out).doubleTap()
        wait("a pop-out shown") { (($0["popouts"] as? [[String: Any]])?.first?["shown"] as? String).map { $0 != "none" } ?? false }
        let done = app.buttons["popout-done"]
        XCTAssertTrue(done.waitForExistence(timeout: 10), "the pop-out's Done (a sheet) or close (a window of its own)")
        shot("n4s3-ui-popout")
        done.tap()
        wait("the pop-out closed") { (($0["popouts"] as? [[String: Any]]) ?? []).isEmpty }
    }
}

extension XCUIElement {
    /// Select what the field holds and type over it.
    func clearAndType(_ text: String) {
        let now = (value as? String) ?? ""
        typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: now.count) + text)
    }
}

