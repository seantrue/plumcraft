// FR4: the Acknowledgements window's words, from the core's model.

import Foundation
import PlumCraftModels
@testable import PlumCraftShell
import Testing

@Suite struct AcknowledgementsWords {
    @Test func aLicenceIsTitledByItsIdAndFirstPackages() {
        let g: JSONValue = ["licence": "MIT", "packages": .array((1...6).map { ["name": .string("p\($0)")] })]
        #expect(groupTitle(g) == "MIT: p1, p2, p3, p4 and 2 more")
        #expect(groupTitle(["licence": "Zlib", "packages": [["name": "z"]]]) == "Zlib: z")
    }

    @Test func aSearchSaysHowManyOfHowMany() {
        #expect(packagesLine(["packages": 450, "shownPackages": 450]) == "450 packages")
        #expect(packagesLine(["packages": 450, "shownPackages": 16]) == "16 of 450 packages")
    }

    @Test func theWindowsDomainCarriesIt() throws {
        let w = try JSONDecoder().decode(Windows.self, from: Data(#"{"acknowledgements": true}"#.utf8))
        #expect(w.acknowledgements == true)
        #expect(RoleWord(rawValue: "acknowledgements") == .acknowledgements)
    }
}
