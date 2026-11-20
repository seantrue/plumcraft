// The iPad's gestures through UIKit's own recognisers (N3 slice 5), driven by XCUITest in the
// simulator: a tap selects, a tap on empty canvas clears, a long press gives the core's context
// menu, a pinch zooms, a one-finger drag on empty canvas pans, a double tap on empty canvas fits.
// What the app did is read from its status file (`--status-file`), whose path the runner gets as
// PLUMCRAFT_STATUS (TEST_RUNNER_PLUMCRAFT_STATUS for xcodebuild); screenshots go to
// PLUMCRAFT_SHOTS when it is set. Local runs only (no macOS CI): conformance.sh gestures.

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
        shot("s5-ui-context-menu")
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

        // a double tap on empty canvas fits everything
        corner.doubleTap()
        wait("fitted by the double tap") { (($0["zoom"] as? Double) ?? 9) < z1 * 0.8 }
        XCTAssertEqual(status()["gesture"] as? String, "doubleTap.fit")
        shot("s5-ui-after-gestures")

        // every orientation: the canvas keeps what was at its centre at its centre
        for (o, name) in [(UIDeviceOrientation.landscapeLeft, "landscape"), (.portrait, "portrait")] {
            XCUIDevice.shared.orientation = o
            RunLoop.current.run(until: Date().addingTimeInterval(2))
            shot("s5-ui-rotating-\(name)")
            XCTAssertTrue(app.buttons["Documents"].exists, "the graph is still shown after the rotation")
            wait("laid out \(name)") {
                let s = ($0["size"] as? [Double]) ?? [0, 0]
                return o.isLandscape ? s[0] > s[1] : s[1] > s[0]
            }
            RunLoop.current.run(until: Date().addingTimeInterval(1))
            if o != .portrait { shot("s5-ui-\(name)") }
        }
    }
}
