// Note 26 slice 5: the shell reads the core's chart display list as it is, leniently: points as
// pairs, rects as x y w h, the plot rectangle; anything not a number is skipped, an odd count's
// last value dropped, never a crash.

import CoreGraphics
import Foundation
import PlumCraftModels
import PlumCraftShell
import Testing

@Suite struct ChartListTests {
    @Test func theDisplayListIsReadAsTheCoreLaidItOut() throws {
        let json = #"{"plot": [40, 12, 300, 200], "marks": [{"kind": "points", "xy": [1, 2, 3, 4, 5]}, {"kind": "rects", "rects": [10, 20, 5, 30, "x", 1, 2, 3, 4]}]}"#
        let list = try JSONDecoder().decode(JSONValue.self, from: Data(json.utf8))
        #expect(ChartList.plot(list) == CGRect(x: 40, y: 12, width: 300, height: 200))
        #expect(ChartList.pairs(list["marks"][0]["xy"]) == [CGPoint(x: 1, y: 2), CGPoint(x: 3, y: 4)], "an odd count's last value dropped")
        #expect(ChartList.rects(list["marks"][1]["rects"]) == [CGRect(x: 10, y: 20, width: 5, height: 30), CGRect(x: 1, y: 2, width: 3, height: 4)], "junk skipped")
        #expect(ChartList.plot(["plot": [1, 2]]) == .zero)
        #expect(ChartList.pairs(.null).isEmpty)
    }
}
