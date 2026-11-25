// An array's view modes and channel histograms on the iPad and iPhone (B154 slice 1, floor F51;
// docs/shell-interface.md 13.2.2), driven by XCUITest in the simulator: the Viewers sample's
// Mandelbrot popped out by a double tap shows its histogram under the bar, the View menu's Heat
// is the core's `run.view` (the status's viewMode follows the core's), and Log is the core's
// `viewer.array`. In a file of its own, beside GestureTests' helpers.

import XCTest

extension GestureTests {
    /// The first pop-out's status entry.
    func firstPopout() -> [String: Any]? { (window()["popouts"] as? [[String: Any]])?.first }

    @MainActor
    func testViewModesAndHistograms() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open-sample", "Viewers", "--status-file", statusPath, "--then", "zoom:0.7:m"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready, on m", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil && abs((($0["zoom"] as? Double) ?? 0) - 0.7) < 0.01 }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        let origin = canvas.coordinate(withNormalizedOffset: .zero)
        let out = try XCTUnwrap(reachable(app, canvas, "m", "out", 0), "m's pin")
        let pin = origin.withOffset(CGVector(dx: out[0], dy: out[1]))
        pin.doubleTap()
        for _ in 0..<2 {
            let deadline = Date().addingTimeInterval(8)
            while Date() < deadline, firstPopout() == nil { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            if firstPopout() == nil { pin.doubleTap() }
        }
        wait("m popped out with its modes and histograms", timeout: 90) { _ in
            guard let p = self.firstPopout() else { return false }
            return (p["viewer"] as? String) == "array.pictures" && (p["viewMode"] as? String) == "auto" && ((p["histograms"] as? Int) ?? 0) > 0
        }
        XCTAssertTrue(app.otherElements["viewer-histograms"].waitForExistence(timeout: 10) || app.descendants(matching: .any)["viewer-histograms"].exists, "the histograms under the bar")
        let shownPicture = { (self.firstPopout()?["picture"] as? [String: Any]) ?? [:] }
        wait("the auto picture drawn", timeout: 60) { _ in (shownPicture()["loaded"] as? Bool) == true }
        let auto = try XCTUnwrap(shownPicture()["handle"] as? String, "the picture's handle")
        shot("b154-s1-ui-auto")
        // View › Heat: the core's run.view, and a cached run brings the heat picture
        let mode = app.buttons["viewer-mode"]
        XCTAssertTrue(mode.waitForExistence(timeout: 10), "the View menu beside View As")
        mode.tap()
        let heat = app.buttons["Heat"].exists ? app.buttons["Heat"] : app.descendants(matching: .any)["viewer-mode-heat"]
        XCTAssertTrue(heat.waitForExistence(timeout: 10), "Heat in the View menu")
        heat.tap()
        // the heat picture itself drawn (B154: the app once kept the auto picture under a heat mode)
        wait("the heat picture drawn", timeout: 90) { _ in
            (self.firstPopout()?["viewMode"] as? String) == "heat" && (shownPicture()["loaded"] as? Bool) == true && (shownPicture()["handle"] as? String).map { $0 != auto } == true
        }
        XCTAssertTrue(app.buttons["viewer-mode"].waitForExistence(timeout: 5), "the View menu says what is shown")
        shot("b154-s1-ui-heat")
        // Log: the core's viewer.array
        let log = app.buttons["viewer-log"].exists ? app.buttons["viewer-log"] : app.switches["viewer-log"]
        XCTAssertTrue(log.waitForExistence(timeout: 10), "the Log toggle")
        log.tap()
        wait("the histograms on a log scale", timeout: 30) { _ in (self.firstPopout()?["log"] as? Bool) == true }
        shot("b154-s1-ui-log")
        let done = app.buttons["popout-done"]
        XCTAssertTrue(done.waitForExistence(timeout: 10))
        done.tap()
        wait("the pop-out closed") { (($0["popouts"] as? [[String: Any]]) ?? []).isEmpty }
    }
}
