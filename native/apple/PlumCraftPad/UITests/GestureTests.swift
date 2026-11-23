// The iPad's gestures through UIKit's own recognisers, driven by XCUITest in the simulator (N3
// slice 5; editing since N4 slice 1): a tap selects, a tap on empty canvas clears, a long press
// gives the core's context menu, a pinch zooms, a one-finger drag on empty canvas pans; a double
// tap on empty canvas opens the palette there; a one-finger drag on a face moves it (one undo
// step) and a two-finger tap undoes it, a three-finger tap redoes it; a drag from an output pin
// dropped on empty canvas opens the palette filtered to what takes the wire, and a tap on a row
// adds and connects (one undo step); Select mode makes a one-finger drag a marquee; a two-finger
// double tap tidies; the toolbar's Fit fits. What the app did is read from its status: in the
// simulator from its status file (`--status-file`), whose path the runner gets as
// PLUMCRAFT_STATUS (TEST_RUNNER_PLUMCRAFT_STATUS for xcodebuild), since runner and app share the
// Mac's files there; on a device, where the runner's sandbox can't see the app's container, from
// the debug build's status element (`--status-element`: the same JSON as an accessibility value,
// N4 device gate). PLUMCRAFT_STATUS_VIA=element or file chooses either anywhere. Screenshots go to
// PLUMCRAFT_SHOTS when it is set. Local runs only (no macOS CI): conformance.sh gestures, and
// device.sh ui on an iPad.

import ObjectiveC
import XCTest

/// B111 (the maintainer, 2026-11-22: "go to skipping the idle wait"): XCTest waits, before and
/// after every action, for the app to report its animations idle, 60 s each when it never does
/// (on a device, after a field typed into). These tests wait on the app's own state instead (the
/// status, an element's existence, each with its own limit), so XCTest's pre- and post-event
/// waits are skipped. XCTest has no public switch for it: its own `XCUIApplicationProcess`
/// answers `shouldSkipPreEventQuiescence` and `shouldSkipPostEventQuiescence` (it logs "Skipping
/// pre-event quiescence check" when they say yes), and both are made to say yes here, for every
/// app the tests drive. Not covered: the wait for SpringBoard before each action (the
/// interruption check), and launch's own wait for the app to come up. PLUMCRAFT_IDLE_WAIT=on
/// leaves XCTest's waits as they were.
enum IdleWait {
    static let skipped: Bool = {
        guard ProcessInfo.processInfo.environment["PLUMCRAFT_IDLE_WAIT"] != "on", let cls = NSClassFromString("XCUIApplicationProcess") else { return false }
        let yes: @convention(block) (AnyObject) -> Bool = { _ in true }
        // before each action, a short settle of a fixed length in its place (PLUMCRAFT_SETTLE_MS,
        // default 300): an action sent while a sheet, menu or keyboard is still moving is lost
        // (seen at 375 points and a larger text size), and a bounded pause costs far less than
        // XCTest's wait, which never ended on the device (B111)
        let settle = Double(ProcessInfo.processInfo.environment["PLUMCRAFT_SETTLE_MS"] ?? "") ?? 300
        let pre: @convention(block) (AnyObject) -> Bool = { _ in
            RunLoop.current.run(until: Date().addingTimeInterval(settle / 1000))
            return true
        }
        var done = 0
        for (name, block) in [("shouldSkipPreEventQuiescence", pre as Any), ("shouldSkipPostEventQuiescence", yes as Any)] {
            guard let m = class_getInstanceMethod(cls, NSSelectorFromString(name)) else {
                print("IDLE: XCTest has no \(name) here; its idle wait stays")
                continue
            }
            method_setImplementation(m, imp_implementationWithBlock(block))
            done += 1
        }
        print("IDLE: XCTest's idle waits skipped (\(done) of 2), \(Int(settle)) ms settle before each action")
        return done == 2
    }()
}

final class GestureTests: XCTestCase {
    override func setUp() {
        super.setUp()
        _ = IdleWait.skipped
    }

    /// B111: the app's layer animations as it reports them when asked (the debug build's
    /// `plumcraft-layers` element, beside the status element; read only where that is read).
    @MainActor
    func layers() -> String {
        guard viaElement, let app = launched else { return "(read on a device: the status element)" }
        let e = app.descendants(matching: .any).matching(identifier: "plumcraft-layers").firstMatch
        return e.exists ? ((e.value as? String) ?? "-") : "(no layers element)"
    }

    var statusPath = ""
    /// Read the status from the app's accessibility element, not a file (on a device).
    var viaElement = false
    weak var launched: XCUIApplication?

    /// The status file's path, and whether the status is read from the element instead.
    func statusFilePath(_ env: [String: String]) -> String {
        #if targetEnvironment(simulator)
        let onDevice = false
        #else
        let onDevice = true
        #endif
        switch env["PLUMCRAFT_STATUS_VIA"] {
        case "element"?: viaElement = true
        case "file"?: viaElement = false
        default: viaElement = onDevice && env["PLUMCRAFT_STATUS"] == nil
        }
        let p = env["PLUMCRAFT_STATUS"] ?? NSTemporaryDirectory() + "plumcraft-ui-status.json"
        if !viaElement { try? FileManager.default.removeItem(atPath: p) }
        return p
    }

    /// The launch arguments with `--status-file <path>` made `--status-element` when the status
    /// is read from the element.
    /// The window each test asks for (`--window-size`): the whole screen unless a test sets
    /// another, or PLUMCRAFT_WINDOW (`WxH` or `full`) sets one for every test. On an iPad with
    /// Windowed Apps or Stage Manager a window keeps the size it last had (B119), so each launch
    /// asks; with Full Screen Apps the window is the screen whatever is asked.
    var windowSize = "full"
    /// A test that needs one size whatever PLUMCRAFT_WINDOW says (the others work at any size).
    var windowNeeded: String?
    /// Whether this launch's window has been said yet (once, when the app first answers).
    var windowSaid = false

    @MainActor
    func adoptStatus(_ app: XCUIApplication) {
        launched = app
        windowSaid = false
        let size = windowNeeded ?? ProcessInfo.processInfo.environment["PLUMCRAFT_WINDOW"].flatMap { $0.isEmpty ? nil : $0 } ?? windowSize
        app.launchArguments += ["--window-size", size]
        // PLUMCRAFT_ORIENTATION=landscape: the device turned first, as the maintainer's iPad was
        switch ProcessInfo.processInfo.environment["PLUMCRAFT_ORIENTATION"] {
        case "landscape"?: XCUIDevice.shared.orientation = .landscapeLeft
        case "portrait"?: XCUIDevice.shared.orientation = .portrait
        default: break
        }
        guard viaElement, let k = app.launchArguments.firstIndex(of: "--status-file"), k + 1 < app.launchArguments.count else { return }
        app.launchArguments.replaceSubrange(k...(k + 1), with: ["--status-element"])
    }

    /// The app's status as JSON (empty until written).
    func status() -> [String: Any] {
        let data: Data?
        if viaElement {
            guard let app = launched else { return [:] }
            let e = app.descendants(matching: .any).matching(identifier: "plumcraft-status").firstMatch
            data = e.exists ? (e.value as? String).map { Data($0.utf8) } : nil
        } else {
            data = FileManager.default.contents(atPath: statusPath)
        }
        guard let d = data, let o = try? JSONSerialization.jsonObject(with: d) as? [String: Any] else { return [:] }
        return o
    }

    func window() -> [String: Any] {
        (status()["windows"] as? [[String: Any]])?.first ?? [:]
    }

    /// Wait (at most `timeout`) until `cond` holds of the first window's status.
    @MainActor
    @discardableResult
    func wait(_ what: String, timeout: TimeInterval = 20, _ cond: ([String: Any]) -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            let w = window()
            if !windowSaid, w["ready"] != nil {
                windowSaid = true
                sizeWindow()
                // every log says the window each test had: size, classes, the scene's state
                print("WINDOW \(name): \(sceneLine(status()))")
            }
            if cond(w) { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        // what the app was animating then (Activity: empty when idle), for B111's device re-run
        let s = status()
        // and the keyboard with what had its focus (B118, B111)
        XCTFail("timed out waiting for \(what): window \(sceneLine(s)); animating \(s["animating"] ?? "(not reported)"), keyboard \(s["keyboard"] ?? "(not reported)"), first responder \(s["firstResponder"] ?? "(not reported)"), layers \(layers()): \(window())")
        return false
    }

    /// The window made the size this launch asked for. "full" the app gets by itself (the scene's
    /// minimum size is the screen). A smaller size iPadOS gives only to a window a person resizes:
    /// the app pins its scene's size to the one asked, and a drag of the window's corner toward
    /// the screen's top left stops there (tried on the simulator with Windowed Apps: 375 by 655,
    /// B119). With Full Screen Apps no window is resized, and the line says so.
    @MainActor
    func sizeWindow() {
        guard let app = launched, let c = status()["scene"] as? [String: Any], let asked = c["asked"] as? String else { return }
        if asked == "full" {
            // the system's own window controls, as a person zooms a window to the whole screen:
            // asking for it alone isn't enough (the scene's minimum size grows it only in steps)
            guard (c["fullScreen"] as? Bool) == false else { return }
            let sb = XCUIApplication(bundleIdentifier: "com.apple.springboard")
            let controls = sb.buttons.matching(NSPredicate(format: "identifier BEGINSWITH 'window-controls:'")).firstMatch
            guard controls.waitForExistence(timeout: 3) else { print("WINDOW \(name): no window controls to zoom with"); return }
            controls.tap()
            let zoom = sb.buttons["Zoom-button"]
            if zoom.waitForExistence(timeout: 3) { zoom.tap() }
            let until = Date().addingTimeInterval(5)
            while Date() < until, ((status()["scene"] as? [String: Any])?["fullScreen"] as? Bool) != true { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            if ((status()["scene"] as? [String: Any])?["fullScreen"] as? Bool) != true { print("WINDOW \(name): could not be made the whole screen") }
            return
        }
        let want = asked.lowercased().split(separator: "x").compactMap { Double($0) }
        func fits() -> Bool {
            guard want.count == 2, let s = (status()["scene"] as? [String: Any])?["size"] as? [Double], s.count == 2 else { return false }
            return abs(s[0] - want[0]) < 1 && abs(s[1] - want[1]) < 1
        }
        if fits() { return }
        let f = app.windows.firstMatch.frame
        guard f.width > 0 else { return }
        let origin = app.coordinate(withNormalizedOffset: .zero)
        origin.withOffset(CGVector(dx: f.maxX - 3, dy: f.maxY - 3))
            .press(forDuration: 0.6, thenDragTo: origin.withOffset(CGVector(dx: f.minX + 20, dy: f.minY + 20)))
        let until = Date().addingTimeInterval(5)
        while Date() < until, !fits() { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        if !fits() { print("WINDOW \(name): could not be made \(asked) (Full Screen Apps, or the system's own limits)") }
    }

    /// The scene as the app reports it, on one line: size, size classes, activation state,
    /// orientation, full screen or not, the size asked for.
    func sceneLine(_ s: [String: Any]) -> String {
        guard let c = s["scene"] as? [String: Any] else { return "(not reported)" }
        let size = (c["size"] as? [Double]).map { $0.map { String(format: "%.1f", $0) }.joined(separator: "x") } ?? "?"
        return "size \(size), h \(c["horizontal"] ?? "?"), v \(c["vertical"] ?? "?"), \(c["state"] ?? "?"), orientation \(c["orientation"] ?? "?"), fullScreen \(c["fullScreen"] ?? "?"), asked \(c["asked"] ?? "-")"
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
        statusPath = statusFilePath(env)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Showcase", "--status-file", statusPath, "--then", "zoom:1:swap"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready, centred on Replace", timeout: 60) { ($0["ready"] as? Bool) == true && abs((($0["zoom"] as? Double) ?? 0) - 1) < 0.01 }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let centre = canvas.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        // empty canvas a finger reaches, at any window size (B119)
        let empty = try XCTUnwrap(emptyCanvas(app, canvas), "empty canvas")
        let corner = canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: empty.x, dy: empty.y))
        let v = visibleCanvas(app, canvas)
        // the canvas's top left corner, below the bar: clear of a menu or a sheet from below
        let outside = canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: v.minX + 12, dy: v.minY + 12))

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
        outside.tap()
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
        toolbar(app, "Fit")
        wait("fitted by the toolbar's Fit") { (($0["zoom"] as? Double) ?? 9) < z1 * 0.8 }
        shot("n4s1-ui-fitted")
        try editing(app, canvas)

        // every orientation: the canvas keeps what was at its centre at its centre. A window
        // that is the whole screen turns with it; a free-size window (Windowed Apps, Stage
        // Manager) keeps its own shape. A zoomed window may go back to the size it had before the
        // zoom on a turn (seen on the simulator: whole screen, turned, 375 by 655 again), which is
        // how the device's window came to be small (B121): whichever, the graph is still shown
        func whole() -> Bool { ((status()["scene"] as? [String: Any])?["fullScreen"] as? Bool) ?? true }
        for (o, name) in [(UIDeviceOrientation.landscapeLeft, "landscape"), (.portrait, "portrait")] {
            XCUIDevice.shared.orientation = o
            RunLoop.current.run(until: Date().addingTimeInterval(2))
            shot("n4s1-ui-rotating-\(name)")
            XCTAssertTrue(app.buttons["Documents"].exists, "the graph is still shown after the rotation")
            wait("laid out \(name)") {
                let s = ($0["size"] as? [Double]) ?? [0, 0]
                guard whole() else { return s[0] > 0 && s[1] > 0 }
                return o.isLandscape ? s[0] > s[1] : s[1] > s[0]
            }
            // a turn can change a free-size window's size, which iPadOS then keeps (B121)
            print("WINDOW after turning \(name): \(sceneLine(status()))")
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
        let empty = try XCTUnwrap(emptyCanvas(app, canvas), "empty canvas")
        let corner = canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: empty.x, dy: empty.y))
        // a double tap on empty canvas: the palette there
        corner.doubleTap()
        wait("the palette opened by the double tap") { (($0["palette"] as? [String: Any])?["shown"] as? Bool) == true }
        shot("n4s1-ui-palette-double-tap")
        // dismissed by a tap outside it: the core's palette closes, nothing added
        let nodes0 = (window()["nodes"] as? Int) ?? 0
        // (above the palette, which a narrow window shows as a sheet from below)
        let v = visibleCanvas(app, canvas)
        canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: v.minX + 12, dy: v.minY + 12)).tap()
        wait("the palette dismissed") { (($0["palette"] as? [String: Any])?["open"] as? Bool) == false }
        XCTAssertEqual((window()["nodes"] as? Int) ?? -1, nodes0)

        // a one-finger drag on a face moves it: one undo step
        let id = "swap"
        let before = try XCTUnwrap(pos(id))
        _ = reachable(app, canvas, id, "center")
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
        _ = reachable(app, canvas, "after", "out", 0)
        let out = try XCTUnwrap(point(canvas, "after", "out", 0), "after.s output: \(window()["points"] ?? "none")")
        // dropped on empty canvas a finger reaches, well away from the pin (at any window size)
        let pin = try XCTUnwrap(((window()["points"] as? [String: Any])?["after"] as? [String: Any]).flatMap { ($0["out"] as? [[Double]])?.first })
        let drop = emptyPoints(app, canvas).filter { hypot($0.x - pin[0], $0.y - pin[1]) > 120 }
            .min { hypot($0.x - pin[0] - 40, $0.y - pin[1] - 220) < hypot($1.x - pin[0] - 40, $1.y - pin[1] - 220) }
            ?? CGPoint(x: pin[0] + 40, y: pin[1] + 220)
        print("WINDOW wire: from \(pin) to \(drop), the canvas reachable in \(visibleCanvas(app, canvas))")
        func paletteShown() -> Bool {
            let p = window()["palette"] as? [String: Any]
            return (p?["wire"] as? Bool) == true && (p?["shown"] as? Bool) == true
        }
        // once more if the drag was lost (with no idle wait it can start while the redo settles)
        for _ in 0..<2 where !paletteShown() {
            out.press(forDuration: 0.05, thenDragTo: canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: drop.x, dy: drop.y)))
            let shown = Date().addingTimeInterval(6)
            while Date() < shown, !paletteShown() { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        }
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
        toolbar(app, "Select")
        wait("select mode on") { ($0["selectMode"] as? Bool) == true }
        // across what a finger reaches of the canvas (below the bar, inside the window)
        let reach = visibleCanvas(app, canvas).insetBy(dx: 8, dy: 8)
        let o = canvas.coordinate(withNormalizedOffset: .zero)
        let a = o.withOffset(CGVector(dx: reach.minX, dy: reach.minY))
        a.press(forDuration: 0.05, thenDragTo: o.withOffset(CGVector(dx: reach.maxX, dy: reach.maxY)))
        wait("the marquee selected") { (($0["selection"] as? [String]) ?? []).count >= 2 }
        toolbar(app, "Select")
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

    /// Row `k` of a face, panned to first where a finger can reach it (B119).
    @MainActor
    func rowReachable(_ app: XCUIApplication, _ canvas: XCUIElement, _ face: String, _ k: Int) -> (CGPoint, String)? {
        reveal(app, canvas) { self.row(face, k).map { [Double($0.0.x), Double($0.0.y)] } }
        return row(face, k)
    }

    /// Editing values on a face (N4 slice 2), through UIKit: a tap on a text row opens a text field
    /// with the keyboard up and the row above it, typing and Done set it (one undo step, "Set
    /// Value"); a number the same with the numbers keyboard; a choice's row gives the system's menu;
    /// a switch flips on a tap; a drag on a number scrubs it.
    @MainActor
    func testInlineEdits() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Inline Editing", "--status-file", statusPath, "--then", "zoom:1.5:w"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready, at 1.5", timeout: 60) { ($0["ready"] as? Bool) == true && abs((($0["zoom"] as? Double) ?? 0) - 1.5) < 0.01 && $0["rows"] != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        func at(_ p: CGPoint) -> XCUICoordinate { origin.withOffset(CGVector(dx: p.x, dy: p.y)) }

        // a text: tap, the field with the keyboard up and the row above it, type, Done
        let (label, _) = try XCTUnwrap(rowReachable(app, canvas, "w", 1))
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
        let (count, _) = try XCTUnwrap(rowReachable(app, canvas, "w", 2))
        at(count).tap()
        wait("the number field open") { (($0["editor"] as? [String: Any])?["kind"] as? String) == "number" }
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.typeText("7")
        shot("n4s2-ui-number-keyboard")
        // B111: where the device's endless idle wait began; what is animating then, for its log
        print("B111: layers with the number typed: \(layers())")
        app.buttons["row-editor-done"].tap()
        wait("count set to 7") { _ in self.value("w", 2) == "7" }
        // B111: on the device the app never reported idle from here on. Whatever it animates
        // must end: the status's `animating` empties within a few seconds of the number's Done
        let calm = Date().addingTimeInterval(8)
        var animating: [Any] = ["(not read)"]
        while Date() < calm {
            animating = (status()["animating"] as? [Any]) ?? ["(not reported)"]
            if animating.isEmpty { break }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        XCTAssertTrue(animating.isEmpty, "B111: still animating 8 s after the number's Done: \(animating); keyboard \(status()["keyboard"] ?? "-"), first responder \(status()["firstResponder"] ?? "-")")
        // said either way, for the device run's log
        print("B111: animating 8 s after the number's Done: \(animating); layers \(layers())")

        // a choice: the system's menu on the row
        let (mode, _) = try XCTUnwrap(rowReachable(app, canvas, "w", 5))
        at(mode).tap()
        let title = app.buttons["title"]
        XCTAssertTrue(title.waitForExistence(timeout: 10), "the choices' menu")
        shot("n4s2-ui-choice-menu")
        title.tap()
        wait("mode set to title") { _ in self.value("w", 5) == "title" }

        // a switch flips on a tap
        let (flag, before) = try XCTUnwrap(rowReachable(app, canvas, "w", 4))
        XCTAssertEqual(before, "on")
        at(flag).tap()
        wait("flag off") { _ in self.value("w", 4) == "off" }

        // a number scrubbed by a drag on its row: amount, 0.50, moves up with a drag right
        let (amount, a0) = try XCTUnwrap(rowReachable(app, canvas, "w", 3))
        XCTAssertEqual(a0, "0.50")
        at(amount).press(forDuration: 0.05, thenDragTo: at(CGPoint(x: amount.x + 90, y: amount.y)))
        wait("amount scrubbed up") { _ in (Double(self.value("w", 3) ?? "") ?? 0) > 0.5 }
        shot("n4s2-ui-after-scrub")

        // the more-button: the system's menu with the node's items
        // panned into reach first: in a narrow window at a larger text size it was off the window
        if let p = reachable(app, canvas, "w", "more"), p.count == 2 {
            at(CGPoint(x: p[0], y: p[1])).tap()
            let dup = app.buttons["Duplicate"]
            if !dup.waitForExistence(timeout: 10) {
                shot("n4s2-ui-more-menu-missing")
                let seen = (app.buttons.allElementsBoundByIndex + app.menuItems.allElementsBoundByIndex).map(\.label).filter { !$0.isEmpty }
                XCTFail("the node's menu from the more-button at \(p): on screen \(seen.prefix(40)); face-menu \(app.buttons["face-menu"].exists ? "\(app.buttons["face-menu"].frame)" : "none")")
            }
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
        statusPath = statusFilePath(env)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Windows and Names", "--status-file", statusPath, "--then", "zoom:1:w"]
        adoptStatus(app)
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
        // each point made reachable first: in a small window it may be off the window (B119)
        let title = try XCTUnwrap(reachable(app, canvas, "w", "title"))
        at(title).doubleTap()
        wait("the parameters open on w") { (($0["parameters"] as? [String: Any])?["shown"] as? Bool) == true }
        let count = app.textFields["edit.count"]
        XCTAssertTrue(count.waitForExistence(timeout: 10), "the hidden count's field")
        shot("n4s3-ui-parameters")
        count.tap()
        count.clearAndType("5")
        // B60: what the popover looks like while the keyboard is up (a device's software
        // keyboard; the simulator's when no hardware keyboard is connected)
        shot("n4s3-ui-count-typing")
        count.typeText("\n")
        wait("count set while hidden") { ($0["undo"] as? String) == "Set Value" }
        shot("n4s3-ui-count-set")
        // shown on the face, where its value is the one set. B60: on the iPad this tap found no
        // `face.count` at once after the value was set (the simulator always did): wait for it as
        // every other step waits, look for it in the list both ways if it isn't there, and say
        // what the app showed when it can't be found
        let face = app.buttons["face.count"]
        if !face.waitForExistence(timeout: 10) || !face.isHittable {
            let list = app.tables.firstMatch
            for swipe in [{ list.swipeDown() }, { list.swipeUp() }, { list.swipeUp() }] where !(face.exists && face.isHittable) {
                swipe()
                _ = face.waitForExistence(timeout: 2)
            }
        }
        if !(face.exists && face.isHittable) {
            shot("b60-face-count-missing")
            let tree = XCTAttachment(string: app.debugDescription)
            tree.name = "b60-element-tree"
            tree.lifetime = .keepAlways
            add(tree)
        }
        face.tap()
        wait("count on the face, at 5") { w in
            let list = ((w["rows"] as? [String: Any])?["w"] as? [Any]) ?? []
            return list.contains { (($0 as? [String: Any])?["key"] as? String) == "count" && (($0 as? [String: Any])?["value"] as? String) == "5" }
        }
        shot("n4s3-ui-hidden-shown")
        // Done closes it in the core
        app.buttons["parameters-done"].tap()
        wait("the parameters closed") { ($0["parameters"] as? [String: Any]).map { ($0["shown"] as? Bool) == false } ?? true }

        // a double tap on the collapsed group: inside it, and the way back
        let g = try XCTUnwrap(reachable(app, canvas, "group:g1", "title"))
        at(g).doubleTap()
        wait("inside the group") { ($0["focus"] as? String) == "g1" && (($0["crumbs"] as? [String])?.count ?? 0) == 2 }
        shot("n4s3-ui-inside-group")
        app.buttons["crumb.0"].tap()
        wait("back on the graph") { $0["focus"] is NSNull || $0["focus"] == nil }

        // a double tap on the frame's tab: its title edited in place
        reveal(app, canvas) { (self.window()["frameTabs"] as? [String: Any])?["f1"] as? [Double] }
        let tab = try XCTUnwrap((window()["frameTabs"] as? [String: Any])?["f1"] as? [Double])
        at(tab).doubleTap()
        let name = app.textFields["name-editor"]
        XCTAssertTrue(name.waitForExistence(timeout: 10), "the frame's name field")
        name.clearAndType("Inputs\n")
        wait("the frame renamed") { (($0["frames"] as? [String: Any])?["f1"] as? String) == "Inputs" }
        shot("n4s3-ui-frame-renamed")

        // a double tap on an output pin: the value pops out
        let out = try XCTUnwrap(reachable(app, canvas, "n1", "out", 0))
        at(out).doubleTap()
        wait("a pop-out shown") { (($0["popouts"] as? [[String: Any]])?.first?["shown"] as? String).map { $0 != "none" } ?? false }
        let done = app.buttons["popout-done"]
        XCTAssertTrue(done.waitForExistence(timeout: 10), "the pop-out's Done (a sheet) or close (a window of its own)")
        shot("n4s3-ui-popout")
        done.tap()
        wait("the pop-out closed") { (($0["popouts"] as? [[String: Any]]) ?? []).isEmpty }
        // N4 slice 5 (the maintainer: "No way to close popout or get back to the main canvas"): the
        // graph is in front and takes a tap again: a tap on a face selects it
        let n5 = try XCTUnwrap(reachable(app, canvas, "n5", "title"))
        at(n5).tap()
        wait("the graph takes a tap after the pop-out closed") { ($0["selection"] as? [String])?.contains("n5") == true }
    }

    /// N4 slice 5: every viewer by the path a person's finger takes: a double tap on each value's
    /// output pin pops it out (a sheet over the graph, the graph in sight above it), the viewer
    /// draws it, Done closes it at once, and the graph takes a tap again.
    /// Works at any window size: each viewer's node is opened centred (`--then zoom:0.7:<node>`),
    /// so its pin is in reach in a 375-point window as on the whole screen (B121).
    @MainActor
    func testEveryViewerByATap() throws {
        let env = ProcessInfo.processInfo.environment
        // (node, the viewer it opens in, what the viewer says once it has drawn it)
        let kinds: [(String, String, String)] = [
            ("tab", "table.grid", "table"), ("js", "yaml", "yaml"), ("img", "picture", "picture"),
            ("snd", "audio", "audio"), ("m", "array.pictures", "picture|pictures"), ("svg", "svg", "picture"),
        ]
        for (node, viewer, drawn) in kinds {
            statusPath = statusFilePath(env)
            let app = XCUIApplication()
            app.launchArguments = ["--scratch-state", "--open-sample", "Viewers", "--status-file", statusPath, "--then", "zoom:0.7:\(node)"]
            adoptStatus(app)
            app.launch()
            defer { app.terminate() }
            wait("the graph, ready, on \(node)", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil && abs((($0["zoom"] as? Double) ?? 0) - 0.7) < 0.01 }
            let canvas = app.otherElements["Canvas"]
            XCTAssertTrue(canvas.waitForExistence(timeout: 10))
            let origin = canvas.coordinate(withNormalizedOffset: .zero)
            func at(_ p: [Double]) -> XCUICoordinate { origin.withOffset(CGVector(dx: p[0], dy: p[1])) }
            // centred on the node; panned to only if its pin is still out of reach
            let out = try XCTUnwrap(reachable(app, canvas, node, "out", 0), node)
            at(out).doubleTap()
            // a double tap the system took as two taps (the sheet just gone) is tried once more
            // (twice more at most: with no idle wait a double tap can land while the canvas settles)
            for _ in 0..<2 {
                let deadline = Date().addingTimeInterval(8)
                while Date() < deadline, ((window()["popouts"] as? [[String: Any]]) ?? []).isEmpty { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
                if ((window()["popouts"] as? [[String: Any]]) ?? []).isEmpty { at(out).doubleTap() }
            }
            let popped = wait("\(node) popped out in \(viewer)", timeout: 30) { w in
                guard let p = (w["popouts"] as? [[String: Any]])?.first else { return false }
                return (p["shown"] as? String) == "sheet" && (p["viewer"] as? String) == viewer && drawn.split(separator: "|").contains { p[String($0)] != nil }
            }
            if !popped {
                let pops = (try? JSONSerialization.data(withJSONObject: window()["popouts"] ?? [], options: [.fragmentsAllowed])).map { String(decoding: $0, as: UTF8.self) } ?? "?"
                XCTFail("\(node): the pop-outs were \(pops.prefix(800))")
            }
            let done = app.buttons["popout-done"]
            XCTAssertTrue(done.waitForExistence(timeout: 10), "\(node): the sheet's Done")
            shot("n4s5-ui-\(viewer)")
            done.tap()
            wait("\(node)'s pop-out closed") { (($0["popouts"] as? [[String: Any]]) ?? []).isEmpty }
            let title = try XCTUnwrap(reachable(app, canvas, node, "title"))
            at(title).tap()
            wait("the graph takes a tap after \(node)'s pop-out") { ($0["selection"] as? [String]) == [node] }
        }
    }
}

// N4 slice 6: the windows the core draws as a model, and Compare, each reached by a person's taps:
// the compact bar's More menu (an iPhone, or a narrow iPad window: `--width-class compact`) and its
// core menus, Settings › Cache's Open Cache Report, Engine › Attach…'s form, and a value's Compare
// menu in its sheet's bar.
extension GestureTests {
    func coreWindows() -> [String: Any] { window()["coreWindows"] as? [String: Any] ?? [:] }

    /// A menu item by its label, wherever the system put it (a button or a menu item).
    // MARK: Any window size (B119): what a person does when the window is small

    /// The part of the canvas a finger can reach, in the canvas's own points: inside the window,
    /// below the bar.
    @MainActor
    func visibleCanvas(_ app: XCUIApplication, _ canvas: XCUIElement) -> CGRect {
        let c = canvas.frame
        var v = c.intersection(app.windows.firstMatch.frame)
        let bar = app.navigationBars.firstMatch
        if bar.exists, bar.frame.maxY > v.minY {
            v = CGRect(x: v.minX, y: bar.frame.maxY, width: v.width, height: max(0, v.maxY - bar.frame.maxY))
        }
        return v.offsetBy(dx: -c.minX, dy: -c.minY)
    }

    /// Where the faces are, roughly, in the canvas (from the status's points), for finding empty canvas.
    func faceBoxes() -> [CGRect] {
        guard let pts = window()["points"] as? [String: Any] else { return [] }
        return pts.values.compactMap { v -> CGRect? in
            guard let f = v as? [String: Any], let c = f["center"] as? [Double], c.count == 2 else { return nil }
            // the app's own box when it says it (the whole face, previews included)
            if let b = f["box"] as? [[Double]], b.count == 2, b[0].count == 2, b[1].count == 2 {
                return CGRect(x: b[0][0] - 16, y: b[0][1] - 16, width: b[1][0] - b[0][0] + 32, height: b[1][1] - b[0][1] + 32)
            }
            var xs = [c[0]], ys = [c[1]]
            for k in ["title", "more"] { if let p = f[k] as? [Double], p.count == 2 { xs.append(p[0]); ys.append(p[1]) } }
            for k in ["in", "out"] { for p in (f[k] as? [[Double]]) ?? [] where p.count == 2 { xs.append(p[0]); ys.append(p[1]) } }
            let top = ys.min() ?? c[1]
            let bottom = max(ys.max() ?? c[1], 2 * c[1] - top)
            let left = min(xs.min() ?? c[0], 2 * c[0] - (xs.max() ?? c[0]))
            let right = max(xs.max() ?? c[0], 2 * c[0] - (xs.min() ?? c[0]))
            return CGRect(x: left - 24, y: top - 28, width: right - left + 48, height: bottom - top + 56)
        }
    }

    /// Points of empty canvas a finger can reach.
    @MainActor
    func emptyPoints(_ app: XCUIApplication, _ canvas: XCUIElement) -> [CGPoint] {
        let v = visibleCanvas(app, canvas).insetBy(dx: 16, dy: 16)
        let boxes = faceBoxes()
        var out: [CGPoint] = []
        for j in 0..<9 {
            for i in 0..<7 {
                let p = CGPoint(x: v.minX + v.width * (CGFloat(i) + 0.5) / 7, y: v.minY + v.height * (CGFloat(j) + 0.5) / 9)
                if !boxes.contains(where: { $0.contains(p) }) { out.append(p) }
            }
        }
        return out
    }

    /// A point of empty canvas a finger can reach (nil: none found).
    @MainActor
    func emptyCanvas(_ app: XCUIApplication, _ canvas: XCUIElement) -> CGPoint? { emptyPoints(app, canvas).first }

    /// Bring a point of the canvas (a pin, a title) where a finger can reach it, as a person
    /// would: a one-finger drag on empty canvas pans. `point` reads it afresh after each pan.
    @MainActor
    func reveal(_ app: XCUIApplication, _ canvas: XCUIElement, _ point: () -> [Double]?) {
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        for _ in 0..<6 {
            guard let p = point(), p.count == 2 else { return }
            let v = visibleCanvas(app, canvas)
            // well inside: a drag that starts at the point (a wire from a pin) needs room too
            if v.insetBy(dx: min(80, v.width / 4), dy: min(80, v.height / 4)).contains(CGPoint(x: p[0], y: p[1])) { return }
            // the drag stays inside what a finger reaches: from the empty point that lets it go
            // furthest toward bringing the point to the middle
            func move(_ s: CGPoint) -> CGVector {
                CGVector(dx: max(v.minX + 8 - s.x, min(v.maxX - 8 - s.x, v.midX - p[0])),
                         dy: max(v.minY + 8 - s.y, min(v.maxY - 8 - s.y, v.midY - p[1])))
            }
            guard let s = emptyPoints(app, canvas).max(by: { hypot(move($0).dx, move($0).dy) < hypot(move($1).dx, move($1).dy) }) else {
                print("WINDOW reveal: no empty canvas in \(v) to pan from, for \(p)")
                return
            }
            let dx = move(s).dx, dy = move(s).dy
            let undo0 = window()["undo"] as? String
            defer {
                // a drag that caught a face would move it, not pan: say so rather than go on astray
                let undo1 = window()["undo"] as? String
                XCTAssertTrue(undo1 == undo0 || undo1 != "Move Node", "reveal: the drag from \(s) moved a face instead of panning")
            }
            // slow, and held at the end, so the pan has no momentum to carry it past
            origin.withOffset(CGVector(dx: s.x, dy: s.y)).press(forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: s.x + dx, dy: s.y + dy)),
                                                               withVelocity: 400, thenHoldForDuration: 0.3)
            // then until the pan stops changing
            var last = (window()["pan"] as? [Double]) ?? [0, 0]
            let until = Date().addingTimeInterval(4)
            while Date() < until {
                RunLoop.current.run(until: Date().addingTimeInterval(0.3))
                let q = (window()["pan"] as? [Double]) ?? last
                if hypot(q[0] - last[0], q[1] - last[1]) < 0.5 { break }
                last = q
            }
        }
    }

    /// A point of a face (`center`, `title`, or pin `k` of `in`/`out`), made reachable first.
    @MainActor
    func reachable(_ app: XCUIApplication, _ canvas: XCUIElement, _ id: String, _ what: String, _ k: Int = 0) -> [Double]? {
        func p() -> [Double]? {
            guard let f = (window()["points"] as? [String: Any])?[id] as? [String: Any] else { return nil }
            if what == "center" || what == "title" || what == "more" { return f[what] as? [Double] }
            return (f[what] as? [[Double]]).flatMap { $0.indices.contains(k) ? $0[k] : nil }
        }
        reveal(app, canvas, p)
        return p()
    }

    /// A toolbar button, from the bar when it is there, else from the bar's More menu (a narrow
    /// window keeps only some of the core's toolbar on the bar).
    @MainActor
    func toolbar(_ app: XCUIApplication, _ label: String) {
        // the bar's button by its words, the one a finger can reach (another element may share them)
        func onBar() -> XCUIElement? { app.buttons.matching(NSPredicate(format: "identifier == %@ OR label == %@", label, label)).allElementsBoundByIndex.first { $0.isHittable } }
        let more = app.buttons["tool.more"]
        // either, as the bar settles (no idle wait holds the test until it has, B111)
        let until = Date().addingTimeInterval(10)
        while Date() < until, onBar() == nil, !more.exists { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        if let b = onBar() { b.tap(); return }
        XCTAssertTrue(more.exists, "\(label): neither on the bar nor a More menu")
        let it = fromMore(app, more, label)
        XCTAssertTrue(it.waitForExistence(timeout: 5), "\(label) in the More menu")
        it.tap()
    }

    /// B118: the on-screen keyboard is up only while a field is edited. After a launch with
    /// nothing edited there is none (the app's status says so, and so does what accessibility
    /// sees), and after a field's Done it goes away again. An open menu brings it (the system
    /// does that for every app's menus when the software keyboard is on: the stock Files app's
    /// too); closing the menu puts it away. Launched as the warned-engine test is (no `--then`,
    /// the compact bar), where it was seen.
    @MainActor
    func testNoKeyboardWithNothingEdited() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Inline Editing", "--status-file", statusPath, "--width-class", "compact"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["rows"] != nil }
        func keyboardShown() -> Bool { ((status()["keyboard"] as? [String: Any])?["shown"] as? Bool) ?? false }
        func noKeyboard(_ when: String) {
            // a keyboard that is going (after Done) goes within its animation: waited for, since
            // XCTest's idle wait no longer holds the test until it has (B111)
            let going = Date().addingTimeInterval(5)
            while Date() < going, keyboardShown() || app.keyboards.count > 0 { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            // a keyboard that is coming shows within a moment of what brings it
            let settle = Date().addingTimeInterval(3)
            while Date() < settle, !keyboardShown(), app.keyboards.count == 0 { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            let s = status()
            XCTAssertFalse(keyboardShown(), "B118: the keyboard is up \(when): keyboard \(s["keyboard"] ?? "-"), first responder \(s["firstResponder"] ?? "-")")
            XCTAssertEqual(app.keyboards.count, 0, "B118: accessibility sees a keyboard \(when): first responder \(s["firstResponder"] ?? "-")")
        }
        shot("n4k-ui-launched")
        noKeyboard("after the launch, nothing edited")

        // a menu opened and closed leaves none
        let more = app.buttons["tool.more"]
        XCTAssertTrue(more.waitForExistence(timeout: 10), "the bar's More menu")
        XCTAssertTrue(fromMore(app, more, "Settings").exists, "the More menu open")
        app.otherElements["Canvas"].coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: 0.3)).tap()
        let closed = Date().addingTimeInterval(10)
        while Date() < closed, item(app, "Settings").exists { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        noKeyboard("after a menu was opened and closed")

        // a text field: the keyboard comes with it, and goes with its Done
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        let (label, _) = try XCTUnwrap(rowReachable(app, canvas, "w", 1))
        origin.withOffset(CGVector(dx: label.x, dy: label.y)).tap()
        wait("the text field open") { (($0["editor"] as? [String: Any])?["kind"] as? String) == "text" }
        let field = app.textFields["row-editor"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        let up = Date().addingTimeInterval(10)
        while Date() < up, !keyboardShown() { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        XCTAssertTrue(keyboardShown(), "the keyboard up while the field is edited: \(status()["keyboard"] ?? "-")")
        field.typeText("no keyboard")
        app.buttons["row-editor-done"].tap()
        wait("the field gone") { (($0["editor"] as? [String: Any])?["open"] as? Bool) == false }
        shot("n4k-ui-after-done")
        noKeyboard("after the field's Done")
    }

    /// An item of the bar's More menu: More tapped, and again (up to three times) when the menu
    /// didn't open, since a tap while a sheet is still going away is lost (no idle wait holds
    /// the test until it has, B111).
    @MainActor
    func fromMore(_ app: XCUIApplication, _ more: XCUIElement, _ label: String) -> XCUIElement {
        for _ in 0..<3 {
            more.tap()
            let it = item(app, label)
            if it.exists { return it }
        }
        return item(app, label)
    }

    func item(_ app: XCUIApplication, _ label: String) -> XCUIElement {
        let b = app.buttons[label]
        let m = app.menuItems[label]
        // a menu just tapped open is still opening (no idle wait holds the test, B111): up to 5 s
        let until = Date().addingTimeInterval(5)
        while Date() < until, !b.exists, !m.exists { RunLoop.current.run(until: Date().addingTimeInterval(0.1)) }
        return b.exists ? b.firstMatch : m.firstMatch
    }

    /// Decision 17 on the iPad, launched: an engine whose manifest warned (a copy of a project's
    /// manifest away from its project, in the app's engines folder) is marked in the Engine menu
    /// reached from the compact bar's More menu: the item is there (disabled, since the iPad
    /// starts no programs), drawn with the warning symbol (the app says which items it marked)
    /// and its warning as its subtitle.
    @MainActor
    func testAWarnedEngineIsMarkedInTheMenu() throws {
        #if targetEnvironment(simulator)
        let shared = true
        #else
        let shared = false
        #endif
        // on a device the runner's files are in its own sandbox, where the app can't read them
        try XCTSkipUnless(shared, "needs a folder both the runner and the app can read: the simulator only")
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        // a folder the app can read: beside the status file the script names (a folder of the
        // Mac's the simulator reaches), not this runner's own container
        let base = env["PLUMCRAFT_STATUS"].map { ($0 as NSString).deletingLastPathComponent + "/" } ?? NSTemporaryDirectory()
        let engines = base + "plumcraft-ui-engines-\(UUID().uuidString.prefix(8))"
        try FileManager.default.createDirectory(atPath: engines, withIntermediateDirectories: true)
        try "id = \"copied\"\nname = \"Copied Engine\"\nmode = \"process\"\ncommand = [\"./uv\", \"run\", \"--project\", \".\", \"python\", \"-m\", \"copied_engine\"]\ncwd = \".\"\n"
            .write(toFile: engines + "/copied.toml", atomically: true, encoding: .utf8)
        // the program it names must be there for the manifest to load (never started on the iPad)
        try "#!/bin/sh\nexit 1\n".write(toFile: engines + "/uv", atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: engines + "/uv")
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Viewers", "--status-file", statusPath, "--width-class", "compact"]
        app.launchEnvironment["PLUMCRAFT_ENGINE_FOLDER"] = engines
        // the whole screen whatever PLUMCRAFT_WINDOW says: in a 375-point window the Engine menu
        // is short, the system's keyboard covers its foot (B118) and a finger's scroll of it is
        // taken as a press, which the test can't tell from the person's (B121)
        windowNeeded = "full"
        defer { windowNeeded = nil }
        adoptStatus(app)
        // and upright, where the menu has room above the keyboard
        XCUIDevice.shared.orientation = .portrait
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true }
        let more = app.buttons["tool.more"]
        XCTAssertTrue(more.waitForExistence(timeout: 10), "the compact bar's More menu")
        fromMore(app, more, "Engine").tap()
        // the item's label carries its subtitle; a disabled item may be a button or a menu item
        let named = NSPredicate(format: "label BEGINSWITH %@", "Copied Engine")
        let copied = app.descendants(matching: .any).matching(named).firstMatch
        // the engines run past the menu's foot: it is scrolled as a finger scrolls it
        var menuX: CGFloat?
        var menuBottom: CGFloat?
        for _ in 0..<10 where !copied.waitForExistence(timeout: 1) {
            // on an item the finger can reach (with the software keyboard on, the system brings
            // it up with any open menu, ours or a stock app's, and it covers the menu's foot: B118)
            // the menu's own items, the first still in sight once it has scrolled
            // (an item's label carries its subtitle: "CADCraft, CADCraft: this device has…")
            let named = NSPredicate(format: "label == 'Reconnect' OR label CONTAINS 'this device has no'")
            // (a button or a menu item: at a larger text size an effect view carries the label too)
            let items = app.buttons.matching(named).allElementsBoundByIndex + app.menuItems.matching(named).allElementsBoundByIndex
            // (a swipe on the app would close the menu)
            // (on screen and above the keyboard: at a larger text size the lowest "hittable" item
            // can be under it, and a swipe there finds an empty visible frame)
            let kb = app.keyboards.firstMatch
            let top = kb.exists ? kb.frame.minY : .greatestFiniteMagnitude
            let screen = app.windows.firstMatch.frame
            // the menu's column, from an item in reach or the last one seen (at a larger text size
            // every named item can have scrolled past while the warned one is still below)
            if let anchor = items.last(where: { $0.exists && $0.isHittable && $0.frame.maxY < top && screen.intersects($0.frame) }) {
                menuX = anchor.frame.midX
                menuBottom = min(anchor.frame.midY, top - 8)
            }
            guard let x = menuX, let y = menuBottom else { continue }
            // the menu's own list, swiped as a whole where UIKit offers it (an item's press and drag
            // doesn't scroll a menu at a larger text size)
            let at = CGPoint(x: x, y: min(y, top - 8) - 20)
            let lists = (app.collectionViews.allElementsBoundByIndex + app.scrollViews.allElementsBoundByIndex)
                .filter { $0.exists && $0.frame.contains(at) && $0.frame.width < screen.width * 0.9 }
            if let list = lists.min(by: { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }) {
                list.swipeUp(velocity: .slow)
                continue
            }
            // a finger's swipe up the menu by the screen's points (an element's own swipe refuses
            // one whose visible frame the menu has clipped)
            let from = CGPoint(x: x, y: min(y, top - 8))
            let origin = app.coordinate(withNormalizedOffset: .zero)
            origin.withOffset(CGVector(dx: from.x, dy: from.y))
                .press(forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: from.x, dy: max(screen.minY + 40, from.y - 220))), withVelocity: .fast, thenHoldForDuration: 0)
        }
        shot("n4h-ui-warned-engine")
        guard copied.waitForExistence(timeout: 10) else {
            let names = app.descendants(matching: .any).allElementsBoundByIndex.map(\.label).filter { !$0.isEmpty }
            XCTFail("the warned engine in the Engine menu: not there; on screen \(names); status engine \(status()["engine"] ?? "-"), menuMarked \(status()["menuMarked"] ?? "-")")
            return
        }
        let label = copied.label + " " + ((copied.value as? String) ?? "")
        let marked = (status()["menuMarked"] as? [String]) ?? []
        XCTAssertTrue(marked.contains("Copied Engine"), "drawn with the warning symbol: menuMarked \(marked)")
        XCTAssertTrue(label.contains("may be a copy") || copied.debugDescription.contains("may be a copy"),
                      "its warning as its subtitle: \(copied.debugDescription.prefix(400))")
    }

    /// The test engine on the iPad (decisions 2A, 3A, 7A), a process on the Mac serving HTTP that
    /// the app attaches by URL (PLUMCRAFT_PORTS_ENGINES, its manifest's folder: conformance.sh
    /// gestures with PAD_PORTS=1; skipped without it). Strip, a 4 by 1000 array, saved from its
    /// footer as a .png: the engine asks back and the core's prompt shows its own labelled buttons;
    /// "Save as NumPy (.npy)" writes a .npy that the export picker offers (cancelled). Traced's value
    /// pops out with a Recipe button that shows "traced"; Strip's has none.
    @MainActor
    func testTheSavesQuestionAndARecipe() throws {
        let env = ProcessInfo.processInfo.environment
        let engines = env["PLUMCRAFT_PORTS_ENGINES"] ?? ""
        try XCTSkipIf(engines.isEmpty, "no test engine attached (conformance.sh gestures with PAD_PORTS=1)")
        statusPath = statusFilePath(env)
        // a graph the app can read: beside the engines folder, a folder of the Mac's
        let graph = engines + "/../ports-graph.json"
        try #"{"version": 1, "engines": ["ports"], "nodes": {"t": {"kind": "ports.traced", "pos": [0, 0], "engine": "ports", "values": {"seed": "corgi"}}, "s": {"kind": "ports.strip", "pos": [0, 260], "engine": "ports"}}, "wires": []}"#
            .write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        // the compact layout: a value pops out as a sheet (one at a time), as on an iPhone
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--width-class", "compact"]
        app.launchEnvironment["PLUMCRAFT_ENGINE_FOLDER"] = engines
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        // the engine attached over HTTP before the run (the graph pins its nodes to it)
        wait("the test engine connected", timeout: 60) { _ in
            ((self.status()["engine"] as? [String: Any])?["label"] as? String).map { !$0.isEmpty } ?? false
        }
        toolbar(app, "Run")
        func savePoint() -> [Double]? { ((window()["points"] as? [String: Any])?["s"] as? [String: Any])?["save"] as? [Double] }
        wait("Strip run, its face offering Save Result…", timeout: 90) { _ in savePoint() != nil }
        reveal(app, canvas, savePoint)
        let at = try XCTUnwrap(savePoint())
        canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at[0], dy: at[1])).tap()
        let save = app.alerts.buttons["Save"]
        XCTAssertTrue(save.waitForExistence(timeout: 10), "the Save Result alert")
        let name = app.alerts.textFields["save-name"]
        XCTAssertTrue(name.exists, "its name field")
        name.tap()
        name.clearAndType("strip.png")
        save.tap()
        // the engine's question, asked by the core with its own words for the buttons
        let npy = app.alerts.buttons["Save as NumPy (.npy)"]
        XCTAssertTrue(npy.waitForExistence(timeout: 30), "the save's question: \(app.alerts.firstMatch.debugDescription.prefix(600))")
        XCTAssertTrue(app.alerts.buttons["Save as PNG anyway"].exists, "its Save as PNG anyway")
        XCTAssertTrue(app.alerts.buttons["Cancel"].exists, "its Cancel")
        XCTAssertFalse(app.alerts.buttons["Npy"].exists || app.alerts.buttons["Png"].exists, "no words made from the answers' names")
        shot("n4p-ui-save-shape")
        npy.tap()
        // the .npy written, offered to the export picker (cancelled: nothing put where a person keeps files)
        let exporting = Date().addingTimeInterval(30)
        var names: [String] = []
        while Date() < exporting, names.isEmpty {
            names = (status()["exporting"] as? [String]) ?? []
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        XCTAssertEqual(names, ["strip.npy"], "the export picker with the .npy: \(status()["exporting"] ?? "-"), banners \(status()["banners"] ?? "-")")
        // the picker has more than one Cancel (its bar's, its sheet's): the first that a finger reaches
        // In a narrow window the picker is a whole-window sheet with no Cancel (a Save and a
        // chevron): it is pulled down from its top, as a finger closes a sheet. Save is never pressed.
        let cancel = app.buttons.matching(NSPredicate(format: "identifier == 'Cancel' OR label == 'Cancel' OR label == 'Close'")).firstMatch
        let shown = Date().addingTimeInterval(10)
        while Date() < shown, !cancel.exists, !app.buttons["Save"].exists { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        shot("n4p-ui-npy-export")
        // (the first match only: the picker's elements come and go while it settles, and an index
        // taken from a list can be gone by the time it is touched)
        let cancels = app.buttons.matching(NSPredicate(format: "identifier == 'Cancel' OR label == 'Cancel' OR label == 'Close'"))
        let second = cancels.element(boundBy: 1)
        if cancel.exists, cancel.isHittable {
            cancel.tap()
        } else if second.exists, second.isHittable {
            second.tap()
        } else {
            print("PICKER buttons: \(app.buttons.allElementsBoundByIndex.map { "\($0.identifier)|\($0.label)" }.prefix(30))")
            app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.02))
                .press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.95)), withVelocity: .fast, thenHoldForDuration: 0)
        }
        let closed = Date().addingTimeInterval(10)
        while Date() < closed, status()["exporting"] is [Any] { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        XCTAssertFalse(status()["exporting"] is [Any], "the picker cancelled")
        shot("n4p-ui-after-export")

        // Traced's value popped out (a double tap on its output pin): Recipe, pressed, says what made it
        func popout(_ node: String) -> [String: Any]? { (window()["popouts"] as? [[String: Any]])?.first { ($0["node"] as? String) == node && ($0["shown"] as? String) != "none" } }
        let out = try XCTUnwrap(reachable(app, canvas, "t", "out", 0), "Traced's output: \(window()["points"] ?? "none")")
        canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: out[0], dy: out[1])).doubleTap()
        wait("Traced's value popped out, with a recipe", timeout: 60) { _ in (popout("t")?["hasRecipe"] as? Bool) == true }
        let recipe = app.buttons["viewer-recipe"]
        XCTAssertTrue(recipe.waitForExistence(timeout: 20), "the Recipe button on Traced's value")
        recipe.tap()
        let said = Date().addingTimeInterval(15)
        var text = ""
        while Date() < said, !text.contains("traced") {
            text = ((status()["recipe"] as? [String: Any])?["text"] as? String) ?? ""
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        XCTAssertTrue(text.contains("traced"), "the recipe the app had: \(status()["recipe"] ?? "-")")
        // and what the screen shows (B128: the sheet once drew "No recipe" over a recipe it had)
        let drawn = app.staticTexts.matching(NSPredicate(format: "label CONTAINS 'traced'")).firstMatch
        XCTAssertTrue(drawn.waitForExistence(timeout: 10), "the recipe's text on screen: \(app.staticTexts.allElementsBoundByIndex.map(\.label).prefix(20))")
        XCTAssertFalse(app.staticTexts["No recipe: the engine sent none for this value."].exists, "no \"No recipe\" over a value that has one")
        shot("n4p-ui-recipe")
        // the recipe's sheet, then the value's, swiped away as a finger does
        // (from the sheet's top edge down, as a finger pulls a sheet away)
        func pullDown() {
            app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.07))
                .press(forDuration: 0.05, thenDragTo: app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.95)), withVelocity: .fast, thenHoldForDuration: 0)
        }
        pullDown()
        let away = Date().addingTimeInterval(5)
        while Date() < away, drawn.exists { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        XCTAssertFalse(drawn.exists, "the recipe's sheet pulled away")
        if popout("t") != nil { pullDown() }
        wait("Traced's sheet closed", timeout: 15) { _ in popout("t") == nil }
        // and none for Strip's value (no lineage)
        let s = try XCTUnwrap(reachable(app, canvas, "s", "out", 0), "Strip's output")
        canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: s[0], dy: s[1])).doubleTap()
        wait("Strip's value popped out", timeout: 60) { _ in popout("s") != nil }
        XCTAssertEqual(popout("s")?["hasRecipe"] as? Bool, false, "Strip's value has no recipe")
        let gone = Date().addingTimeInterval(3)
        while Date() < gone { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        // one value's sheet at a time: Traced's gave way to Strip's, so no Recipe button is on screen
        if popout("t") == nil {
            XCTAssertFalse(app.buttons["viewer-recipe"].exists, "no Recipe button on Strip's value")
        }
        shot("n4p-ui-no-recipe")
    }

    /// B133 on the iPad and iPhone, with the test engine (PLUMCRAFT_PORTS_ENGINES; skipped
    /// without it): Read Text File's path row draws a Browse button at its right end; a tap on it
    /// is the core's row.browse, and the document picker opens for a copy (purpose `path`), then
    /// is cancelled (nothing picked: the simulator's picker has no file the test placed). A tap on
    /// it while the path's text is being edited closes the field (and its keyboard) and opens the
    /// picker the same way.
    @MainActor
    func testBrowseForAPath() throws {
        let env = ProcessInfo.processInfo.environment
        let engines = env["PLUMCRAFT_PORTS_ENGINES"] ?? ""
        try XCTSkipIf(engines.isEmpty, "no test engine attached (PAD_PORTS=1)")
        statusPath = statusFilePath(env)
        let graph = engines + "/../browse-graph.json"
        try #"{"version": 1, "engines": ["ports"], "nodes": {"r": {"kind": "ports.read", "pos": [0, 0], "engine": "ports"}}, "wires": []}"#
            .write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--then", "zoom:1.5:r"]
        app.launchEnvironment["PLUMCRAFT_ENGINE_FOLDER"] = engines
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        func browsePoint() -> [Double]? {
            guard let rows = (window()["rows"] as? [String: Any])?["r"] as? [Any], let r = rows.first as? [String: Any] else { return nil }
            return r["browse"] as? [Double]
        }
        wait("the graph, ready, with the path row's Browse", timeout: 150) { _ in browsePoint() != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        reveal(app, canvas, browsePoint)
        shot("b133-ui-path-row")
        func picking() -> [String: Any]? { status()["picking"] as? [String: Any] }
        func tapBrowse() throws {
            let at = try XCTUnwrap(browsePoint())
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at[0], dy: at[1])).tap()
        }
        // the system's picker on screen (it can take seconds to come, the first time)
        func pickerShown() -> Bool {
            let any = NSPredicate(format: "label IN {'Cancel', 'Open', 'Done', 'Browse', 'Recents'} OR identifier == 'Cancel'")
            let until = Date().addingTimeInterval(20)
            while Date() < until {
                if app.buttons.matching(any).firstMatch.exists || app.tabBars.firstMatch.exists {
                    // drawn a moment after its elements exist (a view of another process's)
                    RunLoop.current.run(until: Date().addingTimeInterval(2))
                    return true
                }
                RunLoop.current.run(until: Date().addingTimeInterval(0.3))
            }
            return false
        }
        // the picker is another process's view: its close button (an ✕ at the sheet's top left)
        // isn't among this app's elements, so it is tapped where it is drawn, trying the few
        // places a sheet puts it on an iPad and an iPhone until the app says the picker closed
        func cancelPicker() {
            let origin = app.coordinate(withNormalizedOffset: .zero)
            let w = app.windows.firstMatch.frame.width
            // an iPad's sheet has the ✕ at its top left; an iPhone's beside "…" at the top right
            let places = w < 600 ? [CGPoint(x: w - 97, y: 110), CGPoint(x: w - 97, y: 96), CGPoint(x: w - 90, y: 124)]
                : [CGPoint(x: 37, y: 84), CGPoint(x: 34, y: 96), CGPoint(x: 34, y: 112), CGPoint(x: 40, y: 72)]
            for p in places where picking() != nil {
                origin.withOffset(CGVector(dx: p.x, dy: p.y)).tap()
                let gone = Date().addingTimeInterval(4)
                while Date() < gone, picking() != nil { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            }
        }
        // 1. the Browse button: the picker, for a copy, the row's file kinds
        try tapBrowse()
        wait("the picker for the path", timeout: 20) { _ in (picking()?["purpose"] as? String) == "path" }
        XCTAssertEqual(picking()?["copy"] as? Bool, true, "a path's file is the picker's copy")
        XCTAssertTrue(pickerShown(), "the document picker on screen: \(app.buttons.allElementsBoundByIndex.map(\.label).prefix(20))")
        shot("b133-ui-picker")
        cancelPicker()
        XCTAssertNil(picking(), "the picker cancelled")
        XCTAssertEqual(value("r", 0), "", "nothing set when cancelled")
        // 2. while the path's text is edited: the field and its keyboard go, the picker comes
        let (row, _) = try XCTUnwrap(rowReachable(app, canvas, "r", 0))
        canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: row.x - 30, dy: row.y)).tap()
        wait("the path's field open") { (($0["editor"] as? [String: Any])?["open"] as? Bool) == true }
        app.textFields["row-editor"].typeText("half typed")
        shot("b133-ui-editing")
        try tapBrowse()
        wait("the picker, from Browse while editing", timeout: 20) { _ in (picking()?["purpose"] as? String) == "path" }
        XCTAssertNotEqual((window()["editor"] as? [String: Any])?["open"] as? Bool, true, "the field closed")
        XCTAssertTrue(pickerShown(), "the document picker on screen, from editing")
        shot("b133-ui-picker-from-editing")
        cancelPicker()
        XCTAssertEqual(value("r", 0), "", "nothing set: the half-typed text went with the field, and the picker was cancelled")
    }

    @MainActor
    func testCompactWindowsByATap() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Viewers", "--status-file", statusPath, "--width-class", "compact", "--then", "zoom:0.7:tab"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        XCTAssertEqual(window()["compact"] as? Bool, true, "the compact layout")
        let more = app.buttons["tool.more"]
        XCTAssertTrue(more.waitForExistence(timeout: 10), "the compact bar's More menu")
        shot("n4s6-ui-compact-canvas")

        // View › Parameters…, from the More menu
        fromMore(app, more, "View").tap()
        let params = item(app, "Parameters…")
        XCTAssertTrue(params.waitForExistence(timeout: 5))
        params.tap()
        wait("the Parameters sheet") { _ in (self.coreWindows()["shown"] as? String) == "params" }
        XCTAssertTrue(app.buttons["report-done"].waitForExistence(timeout: 10))
        shot("n4s6-ui-params")
        app.buttons["report-done"].tap()
        wait("the Parameters sheet closed") { _ in self.coreWindows()["shown"] is NSNull || self.coreWindows()["shown"] == nil }

        // Settings › Cache › Open Cache Report: the report over Settings
        fromMore(app, more, "Settings").tap()
        let cache = app.buttons["settings.section.cache"]
        XCTAssertTrue(cache.waitForExistence(timeout: 10), "Settings' Cache section")
        cache.tap()
        let open = app.buttons["Open Cache Report"]
        XCTAssertTrue(open.waitForExistence(timeout: 10), "Settings' Open Cache Report")
        open.tap()
        wait("the Cache Report sheet") { _ in (self.coreWindows()["shown"] as? String) == "cache" }
        shot("n4s6-ui-cache-report")
        app.buttons["report-done"].tap()
        wait("the Cache Report closed") { _ in self.coreWindows()["shown"] == nil || self.coreWindows()["shown"] is NSNull }
        let settingsDone = app.buttons["settings.done"]
        XCTAssertTrue(settingsDone.waitForExistence(timeout: 10))
        settingsDone.tap()

        // Engine › Attach…: the core's form, cancelled
        fromMore(app, more, "Engine").tap()
        let attach = item(app, "Attach…")
        XCTAssertTrue(attach.waitForExistence(timeout: 5))
        attach.tap()
        let alert = app.alerts.firstMatch
        XCTAssertTrue(alert.waitForExistence(timeout: 10), "the Attach form")
        XCTAssertTrue(alert.staticTexts["Attach to an Engine"].exists)
        shot("n4s6-ui-attach")
        alert.buttons["Cancel"].tap()
        // the alert gone before the canvas is touched (a gesture while it goes is lost, B111)
        let gone = Date().addingTimeInterval(10)
        while Date() < gone, app.alerts.count > 0 { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }

        // a value's sheet: Compare › Pin as A opens Compare
        let canvas = app.otherElements["Canvas"]
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        let out = try XCTUnwrap((((window()["points"] as? [String: Any])?["tab"] as? [String: Any])?["out"] as? [[Double]])?.first)
        func sheetShown() -> Bool { ((window()["popouts"] as? [[String: Any]])?.first?["shown"] as? String) == "sheet" }
        // and once more if the first double tap was lost
        for _ in 0..<2 where !sheetShown() {
            origin.withOffset(CGVector(dx: out[0], dy: out[1])).doubleTap()
            let shown = Date().addingTimeInterval(15)
            while Date() < shown, !sheetShown() { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        }
        wait("the table's sheet", timeout: 30) { _ in sheetShown() }
        let compare = app.buttons["viewer-compare"]
        XCTAssertTrue(compare.waitForExistence(timeout: 20))
        shot("n4s6-ui-table-compact")
        compare.tap()
        item(app, "Pin as A").tap()
        wait("Compare over the value") { _ in (self.coreWindows()["shown"] as? String) == "compare" }
        shot("n4s6-ui-compare")
        app.buttons["compare-done"].tap()
        wait("Compare closed") { _ in self.coreWindows()["shown"] == nil || self.coreWindows()["shown"] is NSNull }
    }
}

extension XCUIElement {
    /// Select what the field holds and type over it.
    func clearAndType(_ text: String) {
        let now = (value as? String) ?? ""
        typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: now.count) + text)
    }
}

