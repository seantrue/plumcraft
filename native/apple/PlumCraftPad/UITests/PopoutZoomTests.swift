// B159, a list's items in a pop-out keep the picture's zoom and view (the maintainer: "scrolling
// through items in result does not maintain zoom and view of image"), driven by XCUITest in the
// simulator: a Vary Int makes three Mandelbrots (two of one size but a pixel, so their pictures differ; one half as wide), a double tap pops
// them out, a pinch zooms in, Next Item steps: the next picture shows at the same zoom about the
// same relative centre (the status's `zoom`, in the picture's own terms), and so does the narrower
// one. In a file of its own, beside GestureTests' helpers.

import XCTest

/// The app's control port, as a script reaches it (loopback, which the simulator shares): one
/// request at a time, JSON lines, after `auth` with the token the app wrote.
final class ControlPort {
    private let input: InputStream
    private let output: OutputStream
    private var n = 0

    init?(port: Int, token: String) {
        var i: InputStream?, o: OutputStream?
        Stream.getStreamsToHost(withName: "127.0.0.1", port: port, inputStream: &i, outputStream: &o)
        guard let i, let o else { return nil }
        input = i
        output = o
        i.open()
        o.open()
        guard call("auth", ["token": token]) != nil else { return nil }
    }

    deinit {
        input.close()
        output.close()
    }

    /// The reply's `result`, or nil when refused or unanswered.
    @discardableResult
    func call(_ method: String, _ params: [String: Any], doc: Any? = nil) -> Any? {
        n += 1
        var req: [String: Any] = ["id": n, "method": method, "params": params]
        if let doc { req["doc"] = doc }
        guard var line = try? JSONSerialization.data(withJSONObject: req) else { return nil }
        line.append(0x0a)
        let sent = line.withUnsafeBytes { output.write($0.bindMemory(to: UInt8.self).baseAddress!, maxLength: line.count) }
        guard sent == line.count else { return nil }
        var got = Data()
        var buf = [UInt8](repeating: 0, count: 65536)
        let deadline = Date().addingTimeInterval(30)
        while Date() < deadline, !got.contains(0x0a) {
            if input.hasBytesAvailable {
                let k = input.read(&buf, maxLength: buf.count)
                if k <= 0 { break }
                got.append(contentsOf: buf[0..<k])
            } else {
                RunLoop.current.run(until: Date().addingTimeInterval(0.02))
            }
        }
        guard let end = got.firstIndex(of: 0x0a), let r = try? JSONSerialization.jsonObject(with: got[..<end]) as? [String: Any],
              (r["ok"] as? Bool) == true else { return nil }
        return r["result"]
    }
}

extension GestureTests {
    func popoutZoom() -> [String: Any]? { (firstPopout()?["zoom"] as? [String: Any]) }

    @MainActor
    func testItemsKeepTheZoomAndView() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let graph = NSTemporaryDirectory() + "popout-zoom-graph.json"
        try #"""
        {"version": 1, "nodes": {"v": {"kind": "vary.int", "pos": [-420, 0], "values": {"values": "1600,1599,800"}},
         "m": {"kind": "array.mandelbrot", "pos": [0, 0], "values": {"height": 1000}}},
         "wires": [{"from": {"node": "v", "port": "values"}, "to": {"node": "m", "port": "width"}}]}
        """#.write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        let tokenFile = NSTemporaryDirectory() + "popout-zoom-token-\(UUID().uuidString)"
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--control-token-file", tokenFile, "--then", "zoom:0.7:m"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready, on m", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
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
        let shownPicture = { (self.firstPopout()?["picture"] as? [String: Any]) ?? [:] }
        wait("item 1 drawn, fitted", timeout: 150) { _ in (shownPicture()["loaded"] as? Bool) == true && (self.popoutZoom()?["kept"] as? Bool) == false }
        let first = try XCTUnwrap(shownPicture()["handle"] as? String)
        shot("b159-ui-item1-fit")
        // the sheet raised to its large height, so the whole picture is on screen to pinch
        let title = app.staticTexts["Mandelbrot · escape"]
        if title.waitForExistence(timeout: 5) {
            title.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).press(forDuration: 0.1, thenDragTo: app.windows.firstMatch.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.02)))
        }
        let pic = app.scrollViews["popout-picture-scroll"]
        XCTAssertTrue(pic.waitForExistence(timeout: 10), "the picture")
        wait("the picture on screen", timeout: 10) { _ in pic.frame.maxY <= app.windows.firstMatch.frame.maxY + 1 }
        RunLoop.current.run(until: Date().addingTimeInterval(0.8))
        // a zoom in, 200% about a point off the middle (far enough in that the narrower item can
        // put it in the middle too; nearer an edge the view is held inside the picture): asked of the core as a script asks it
        // (`viewer.zoom`; XCUITest's pinch never reached the picture's scroll view in a sheet),
        // which the shell then keeps as it keeps a pinch
        let port = try XCTUnwrap(status()["port"] as? Int, "the control port")
        let token = try String(contentsOfFile: tokenFile, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        let control = try XCTUnwrap(ControlPort(port: port, token: token), "the control port answers")
        let pid = try XCTUnwrap(firstPopout()?["id"] as? Int, "the pop-out's id")
        let scale2x = Int((UIScreen.main.scale * 2).rounded())  // 200%: two points a value pixel, in screen pixels (3 a point on an iPhone)
        XCTAssertNotNil(control.call("command.run", ["command": "viewer.zoom", "params": ["popout": pid, "to": scale2x, "at": [400, 700]]], doc: window()["doc"]),
                        "viewer.zoom refused")
        wait("the pinch kept", timeout: 20) { _ in (self.popoutZoom()?["kept"] as? Bool) == true }
        let zoomed = try XCTUnwrap(popoutZoom())
        let scale = try XCTUnwrap(zoomed["scale"] as? Double)
        let centre = try XCTUnwrap(zoomed["centre"] as? [Double])
        XCTAssertEqual(scale, 2, accuracy: 0.01, "200%")
        XCTAssertEqual(centre[0], 700.5 / 1600, accuracy: 0.01, "about column 700")
        XCTAssertEqual(centre[1], 400.5 / 1000, accuracy: 0.01, "about row 400")
        shot("b159-ui-item1-pinched")
        func same(_ z: [String: Any]?, _ what: String) {
            let s = z?["scale"] as? Double ?? 0, c = z?["centre"] as? [Double] ?? []
            XCTAssertEqual((z?["kept"] as? Bool), true, "\(what): the zoom kept")
            XCTAssertEqual(s, scale, accuracy: scale * 0.01, "\(what): the same scale")
            if c.count == 2 {
                XCTAssertEqual(c[0], centre[0], accuracy: 0.02, "\(what): the same centre, across")
                XCTAssertEqual(c[1], centre[1], accuracy: 0.02, "\(what): the same centre, down")
            } else {
                XCTFail("\(what): no centre in \(String(describing: z))")
            }
        }
        // Next Item: the second Mandelbrot, the same size
        let next = app.buttons["Next Item"]
        XCTAssertTrue(next.waitForExistence(timeout: 10))
        next.tap()
        wait("item 2 drawn", timeout: 90) { _ in (shownPicture()["loaded"] as? Bool) == true && (shownPicture()["handle"] as? String) != first }
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        same(popoutZoom(), "item 2")
        shot("b159-ui-item2")
        let second = shownPicture()["handle"] as? String
        // and the third, half as wide: the same zoom about the same relative point
        next.tap()
        wait("item 3 drawn", timeout: 90) { _ in (shownPicture()["loaded"] as? Bool) == true && (shownPicture()["handle"] as? String) != second }
        RunLoop.current.run(until: Date().addingTimeInterval(1))
        same(popoutZoom(), "item 3")
        shot("b159-ui-item3")
        // Fit asked again forgets the kept zoom
        let fit = app.segmentedControls["popout-zoom"].buttons["Fit"]
        if fit.exists {
            fit.tap()
            wait("fitted again", timeout: 20) { _ in (self.popoutZoom()?["kept"] as? Bool) == false }
        }
        let done = app.buttons["popout-done"]
        if done.waitForExistence(timeout: 5) { done.tap() }
    }
}
