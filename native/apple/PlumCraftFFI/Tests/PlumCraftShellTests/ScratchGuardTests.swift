// The scripted launch's rule (ScratchGuard): every place it writes inside the scratch root or the
// system's temporary folder, compared resolved; unset places refused where their fallback is a
// person's own.

import Foundation
import PlumCraftShell
import Testing

@Suite struct ScratchGuardTests {
    /// A fresh folder of this test's own under the process's scratch.
    func folder(_ name: String) -> String {
        let p = scratch("guard-\(name)-\(UUID().uuidString.prefix(8))")
        return p
    }

    @Test func insideTheScratchRootPasses() {
        let root = folder("in")
        let l: [ScratchGuard.Location] = [.init("COMMONKIT_STORE_ROOT", root + "/store"), .init("the log folder", root + "/mac-logs/x")]
        #expect(ScratchGuard.refusal(l, inside: [root]) == nil)
        #expect(ScratchGuard.refusal([.init("--store-root", root)], inside: [root]) == nil, "the root itself")
    }

    @Test func outsideIsRefusedByName() {
        let root = folder("a"), other = folder("b")
        let why = ScratchGuard.refusal([.init("COMMONKIT_MARK_DIR", other + "/marks")], inside: [root])
        #expect(why?.contains("COMMONKIT_MARK_DIR=") == true)
        #expect(why?.contains("outside the scratch root") == true)
        // a sibling whose name only starts like the root's is outside it
        #expect(ScratchGuard.refusal([.init("x", root + "b/store")], inside: [root]) != nil)
        // `..` out of the root is outside it
        #expect(ScratchGuard.refusal([.init("x", root + "/store/../../elsewhere")], inside: [root]) != nil)
    }

    @Test func symlinksAreResolved() throws {
        let root = folder("link"), away = folder("away")
        let fm = FileManager.default
        // a link inside the root that leads out of it: refused
        try fm.createSymbolicLink(atPath: root + "/out", withDestinationPath: away)
        #expect(ScratchGuard.refusal([.init("COMMONKIT_STORE_ROOT", root + "/out/store")], inside: [root]) != nil)
        // a root named through a link: what is under its target passes
        try fm.createSymbolicLink(atPath: away + "/to-root", withDestinationPath: root)
        #expect(ScratchGuard.refusal([.init("x", root + "/store")], inside: [away + "/to-root"]) == nil)
        #expect(ScratchGuard.refusal([.init("x", away + "/to-root/store")], inside: [root]) == nil)
    }

    @Test func privateAliasesAndTrailingSlashesAreOnePlace() {
        let name = "plumcraft-guard-\(UUID().uuidString)"
        #expect(ScratchGuard.resolved("/tmp/\(name)/a/") == ScratchGuard.resolved("/private/tmp/\(name)/a"))
        #expect(ScratchGuard.resolved("/var/\(name)") == ScratchGuard.resolved("/private/var/\(name)"))
        #expect(!ScratchGuard.resolved("/tmp/\(name)/a/").hasSuffix("/"))
        #expect(ScratchGuard.refusal([.init("x", "/private/tmp/\(name)/store")], inside: ["/tmp/\(name)/"]) == nil)
    }

    @Test func unsetRefusesWhereItFallsBackToAPersonsOwn() {
        let root = folder("unset")
        let why = ScratchGuard.refusal([.init("PLUMCRAFT_CRAFT_ROOT", nil, ifUnset: "the craft apps' stores would go to ~/Library/Caches")], inside: [root])
        #expect(why == "--scratch-state refuses: PLUMCRAFT_CRAFT_ROOT is unset, so the craft apps' stores would go to ~/Library/Caches")
        #expect(ScratchGuard.refusal([.init("PLUMCRAFT_CRAFT_ROOT", "", ifUnset: "it falls back")], inside: [root]) != nil, "empty is unset")
        #expect(ScratchGuard.refusal([.init("COMMONKIT_TEST_RUN", nil)], inside: [root]) == nil, "optional: nothing written")
    }

    @Test func aRealStoreShapedPathIsRefused() {
        // a store on a volume of its own, as a login shell names a person's store: no name is
        // special, it is simply outside
        let why = ScratchGuard.refusal([.init("COMMONKIT_STORE_ROOT", "/Volumes/SomeStore/commonkit/store")],
                                       inside: ScratchGuard.temporaryFolders())
        #expect(why?.hasPrefix("--scratch-state refuses COMMONKIT_STORE_ROOT=/Volumes/SomeStore/commonkit/store") == true)
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        #expect(ScratchGuard.refusal([.init("the caches folder", home + "/Library/Caches/x")], inside: ScratchGuard.temporaryFolders()) != nil)
    }

    @Test func aScriptedLaunchRunsOnlyAsAScriptedCopy() {
        // B84: the person's ids are refused, with the copy to use named
        let why = ScratchGuard.bundleRefusal("com.swapwizard.plummer.mac")
        #expect(why?.contains("com.swapwizard.plummer.mac.scripted") == true)
        #expect(why?.contains("scripted-app.sh") == true)
        #expect(ScratchGuard.bundleRefusal("com.swapwizard.plummer.mac.scripted") == nil)
        #expect(ScratchGuard.bundleRefusal("com.swapwizard.plummer.mac.scriptedx") != nil, "only the suffix itself")
        #expect(ScratchGuard.bundleRefusal(".scripted") != nil, "the suffix alone is no id")
        #expect(ScratchGuard.bundleRefusal(nil) != nil)
        #expect(ScratchGuard.bundleRefusal("") != nil)
    }

    @Test func theTemporaryFolderIsAllowedAndRootIsNot() {
        let t = ScratchGuard.temporaryFolders([:])
        #expect(ScratchGuard.refusal([.init("x", NSTemporaryDirectory() + "plumcraft-guard/store")], inside: t) == nil)
        #expect(ScratchGuard.refusal([.init("x", "/tmp/plumcraft-guard/store")], inside: t) == nil)
        // an allowed folder that resolves to `/` allows nothing
        #expect(ScratchGuard.refusal([.init("x", "/Users/me/store")], inside: ["/", "//"]) != nil)
        // a relative TMPDIR is not a folder the system gave
        #expect(!ScratchGuard.temporaryFolders(["TMPDIR": "relative/tmp"]).contains("relative/tmp"))
    }
}
