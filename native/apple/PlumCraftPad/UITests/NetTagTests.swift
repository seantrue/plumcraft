// B146: a named wire's tags beside their pins, pictured. Windows and Names names Every widget's
// output "said"; its driver's tag ("said ×2") is beside that output pin and a receiving tag beside
// each input it feeds (Reverse's and Upper case's). The picture is close on both, at whatever
// text size the run sets (TEXT_SIZE).

import XCTest

extension GestureTests {
    @MainActor
    func testANamedWiresTagsBesideTheirPins() throws {
        let env = ProcessInfo.processInfo.environment
        statusPath = statusFilePath(env)
        let app = XCUIApplication()
        // Every widget's output and Reverse's input both in sight (the run turns the iPad to landscape)
        app.launchArguments = ["--scratch-state", "--open-sample", "Windows and Names", "--status-file", statusPath, "--then", "zoom:1.2:w"]
        adoptStatus(app)
        app.launch()
        defer { app.terminate() }
        wait("the graph, ready", timeout: 150) { ($0["ready"] as? Bool) == true && $0["points"] != nil }
        RunLoop.current.run(until: Date().addingTimeInterval(2))
        shot("b146-ui-tags")
    }
}
