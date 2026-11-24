// B151: which build this is. The app's own stamp, read from its Info.plist's PlumBuild* keys (the
// Stamp build script phase), goes to the core as `app.built`; About shows the core's line, which
// says the newest build time, the shell's commit, and the app's own when it differs.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import Testing

@Suite struct BuildStamps {
    @Test func theInfoPlistsStampIsTheConfigs() {
        let info: [String: Any] = ["PlumBuildCommit": "22f5884", "PlumBuildDirty": "yes", "PlumBuildDate": "2026-11-24T21:51:07Z"]
        let j = BuildStamp.json(from: info)
        #expect(j?["commit"].string == "22f5884")
        #expect(j?["dirty"].bool == true)
        #expect(j?["date"].string == "2026-11-24T21:51:07Z")
        #expect(BuildStamp.json(from: ["PlumBuildCommit": "22f5884", "PlumBuildDirty": "no", "PlumBuildDate": "x"])?["dirty"].bool == false)
        #expect(BuildStamp.json(from: ["PlumBuildCommit": "unknown", "PlumBuildDirty": "unknown", "PlumBuildDate": "unknown"])?["dirty"] == .null)
        // an unstamped build says nothing
        #expect(BuildStamp.json(from: [:]) == nil)
        var c = ShellConfig(appID: "com.plumcraft.test", appName: "PlumCraft", version: "0", storeRoot: "/s")
        c.built = j
        #expect(c.json["app"]["built"]["commit"].string == "22f5884")
        c.built = nil
        #expect(c.json["app"]["built"] == .null)
    }

    @Test func aboutShowsTheCoresLine() async throws {
        _ = testEnvironment
        var c = ShellConfig(appID: "com.plumcraft.test", appName: "PlumCraft", version: "0", storeRoot: scratch("store-build"))
        c.platform = ["os": "ios", "subprocess": false, "craft_apps": false, "python": false]
        // built later than any library could have been, from a commit no library has
        c.built = ["commit": "abcdef1", "dirty": .bool(true), "date": "2099-01-02T03:04:05Z"]
        let (core, ready) = try ShellCore.open(c)
        defer { core.close() }
        _ = await ready.value
        let r = try await core.request("document.open")
        let a = try await core.command("app.about", [:], doc: r["doc"].uint64)
        let line = try #require(BuildStamp.line(about: a))
        #expect(line.hasPrefix("Built 2099-01-02 03:04 UTC · shell "), "\(line)")
        #expect(line.hasSuffix(" · ui abcdef1+changes"), "\(line)")
        #expect(a["build"]["ui"]["commit"].string == "abcdef1")
        let shell = try #require(a["build"]["shell"]["commit"].string)
        #expect(shell == "unknown" || (shell.count >= 4 && shell.allSatisfy(\.isHexDigit)), "\(shell)")
        // a reply with no build: nothing shown
        #expect(BuildStamp.line(about: ["version": "1"]) == nil)
    }
}
