// Photos as a source for Browse on the iPad and iPhone, launched (decisions 11A and 12A, the
// maintainer, 2026-11-23): the test engine's Read Picture File (`ports.picture`, a path that takes
// PNG and JPEG only) shows a Browse button; a tap on it asks "Photos or Files"; Files opens the
// document picker (cancelled); Photos opens the photo library's picker, a photo from the
// simulator's own library is picked, and the node's path names a copy in Picked Files that
// exists. Run with the test engine attached (PAD_PORTS=1); skipped without it.
//
// The photo library's picker runs in another process: its grid isn't among this app's elements,
// so a photo is tapped where the picker draws its first one, trying a few places until the app
// says the picker has answered.

import XCTest

extension GestureTests {
    @MainActor
    func testBrowseOffersPhotosForAPicture() throws {
        let env = ProcessInfo.processInfo.environment
        let engines = env["PLUMCRAFT_PORTS_ENGINES"] ?? ""
        try XCTSkipIf(engines.isEmpty, "no test engine attached (PAD_PORTS=1)")
        statusPath = statusFilePath(env)
        let graph = engines + "/../picture-graph.json"
        try #"{"version": 1, "engines": ["ports"], "nodes": {"p": {"kind": "ports.picture", "pos": [0, 0], "engine": "ports"}}, "wires": []}"#
            .write(toFile: graph, atomically: true, encoding: .utf8)
        let app = XCUIApplication()
        app.launchArguments = ["--scratch-state", "--open", graph, "--status-file", statusPath, "--then", "zoom:1.5:p"]
        app.launchEnvironment["PLUMCRAFT_ENGINE_FOLDER"] = engines
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        func browsePoint() -> [Double]? {
            guard let rows = (window()["rows"] as? [String: Any])?["p"] as? [Any], let r = rows.first as? [String: Any] else { return nil }
            return r["browse"] as? [Double]
        }
        wait("the graph, ready, with the picture path's Browse", timeout: 150) { _ in browsePoint() != nil }
        let canvas = app.otherElements["Canvas"]
        XCTAssertTrue(canvas.waitForExistence(timeout: 10))
        reveal(app, canvas, browsePoint)
        func picking() -> [String: Any]? { status()["picking"] as? [String: Any] }
        func tapBrowse() throws {
            let at = try XCTUnwrap(browsePoint())
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: at[0], dy: at[1])).tap()
        }
        func chooseSource(_ name: String) {
            wait("Photos or Files", timeout: 20) { _ in (picking()?["source"] as? String) == "choice" }
            let b = app.buttons[name].firstMatch
            XCTAssertTrue(b.waitForExistence(timeout: 10), "the choice's \(name)")
            shot("photos-ui-choice")
            b.tap()
        }
        let wf = app.windows.firstMatch.frame
        let origin = app.coordinate(withNormalizedOffset: .zero)

        // 1. Files: the document picker for the same ask, cancelled
        try tapBrowse()
        chooseSource("Files")
        wait("the document picker", timeout: 20) { _ in (picking()?["source"] as? String) == "files" }
        RunLoop.current.run(until: Date().addingTimeInterval(2))
        shot("photos-ui-files")
        // its ✕ at the sheet's top left on an iPad, at the top right on an iPhone (B133's test)
        let places = wf.width < 600 ? [CGPoint(x: wf.width - 97, y: 110), CGPoint(x: wf.width - 97, y: 96), CGPoint(x: wf.width - 90, y: 124)]
            : [CGPoint(x: 37, y: 84), CGPoint(x: 34, y: 96), CGPoint(x: 34, y: 112), CGPoint(x: 40, y: 72)]
        for p in places where picking() != nil {
            origin.withOffset(CGVector(dx: wf.minX + p.x, dy: wf.minY + p.y)).tap()
            let gone = Date().addingTimeInterval(4)
            while Date() < gone, picking() != nil { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
        }
        XCTAssertNil(picking(), "the document picker cancelled")
        XCTAssertEqual(value("p", 0), "", "nothing set when cancelled")

        // 2. Photos: the photo library's picker; its first photo picked
        try tapBrowse()
        chooseSource("Photos")
        wait("the photo library's picker", timeout: 20) { _ in (picking()?["source"] as? String) == "photos" }
        RunLoop.current.run(until: Date().addingTimeInterval(3))
        shot("photos-ui-library")
        // the first photo of the grid, where the iPad's picker draws it (a sheet with places at
        // its left and the grid at its right, under a "Private Access to Photos" note: the first
        // photo at about 0.44, 0.45 of an 834 by 1210 window), then the next ones
        let fractions: [(Double, Double)] = [(0.437, 0.449), (0.54, 0.449), (0.437, 0.52), (0.44, 0.40)]
        for (fx, fy) in fractions where picking() != nil {
            origin.withOffset(CGVector(dx: wf.minX + wf.width * fx, dy: wf.minY + wf.height * fy)).tap()
            let answered = Date().addingTimeInterval(4)
            while Date() < answered, picking() != nil { RunLoop.current.run(until: Date().addingTimeInterval(0.2)) }
            print("PHOTOS: tapped at \(fx), \(fy): \(picking() == nil ? "answered" : "still open")")
        }
        shot("photos-ui-after-pick")
        XCTAssertNil(picking(), "the photo picked (the picker answered)")
        // the node names a copy in Picked Files, and the file is there
        wait("the path set from the photo", timeout: 30) { _ in (self.value("p", 0) ?? "").contains("Picked Files") }
        let path = value("p", 0) ?? ""
        print("PHOTOS: the node's path \(path), picked \(status()["picked"] ?? "-")")
        XCTAssertTrue(path.contains("/Picked Files/"), "a copy in Picked Files: \(path)")
        XCTAssertTrue(["png", "jpg", "jpeg"].contains((path as NSString).pathExtension.lowercased()), "in a form the path takes: \(path)")
        // (the core keeps a chosen path relative to the workflow's folder when it can)
        let full = path.hasPrefix("/") ? path : (((graph as NSString).deletingLastPathComponent as NSString).appendingPathComponent(path) as NSString).standardizingPath
        XCTAssertTrue(FileManager.default.fileExists(atPath: full), "the copy exists: \(full)")
        shot("photos-ui-set")
    }
}
