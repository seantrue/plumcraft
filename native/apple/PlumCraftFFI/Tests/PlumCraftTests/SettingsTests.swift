// N4 slice 4 through the kit, against a real core: the settings model (`settings.get`, an item set
// through its command, the feed's `prefs`), the theme following the system (`lifecycle.appearance`),
// a watch popping its value out (the maintainer: "No watcher popout at all"), and autosave's state
// in the document domain.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import Testing

@Suite @MainActor struct Settings {
    @Test func theSettingsAreTheCoresAndAnItemSetsThroughItsCommand() async throws {
        let (core, model) = try await started("settings")
        defer { core.close() }
        let s = SettingsModel(model: model)
        var told = 0
        model.onPrefs = { told += 1 }
        await s.refresh()
        #expect(s.sections.map(\.id) == ["appearance", "colours", "families", "files", "running", "engines", "cache"])
        #expect(s.chosen == "system")
        let theme = try #require(s.item("theme"))
        #expect(theme.options.map(\.label).first == "Match the System")
        // Dusk chosen: the model's theme follows, and the feed said so
        let dusk = try #require(theme.options.first { $0.value == "dusk" })
        await s.set(theme, dusk.value)
        #expect(s.chosen == "dusk")
        #expect(await model.wait { model.theme?.name == "dusk" && told > 0 })
        // a colour, then its own reset
        let accent = try #require(s.item("token.accent"))
        await s.set(accent, "#123456")
        #expect(s.item("token.accent")?.changed == true)
        #expect(s.item("token.accent")?.value == "#123456")
        if let a = s.item("token.accent") { await s.reset(a) }
        #expect(s.item("token.accent")?.changed == false)
        // back to the system's, which is dark now: Dusk again
        await s.set(theme, "system")
        _ = try await core.request("lifecycle.appearance", ["dark": true, "contrast": "more"])
        #expect(await model.wait { model.theme?.name == "dusk" })
        _ = try await core.request("lifecycle.appearance", ["dark": false, "contrast": "normal"])
        #expect(await model.wait { model.theme?.name == "paper" })
    }

    @Test func watchingAPortPopsItsValueOut() async throws {
        let (core, model) = try await started("watch-popout")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        // what a click on the output pin and the Watch item run
        _ = try await model.run("run.watch", ["node": .string(a), "port": "text"])
        #expect(await model.wait { model.windows.list.count == 1 })
        #expect(model.windows.list.first?.port == "text")
        _ = try await model.run("run.watch", ["node": .string(a), "port": "text"])
        #expect(await model.wait { model.windows.list.count == 1 })
    }

    @Test func anAutosavedDocumentSaysSoInItsState() async throws {
        let (core, model) = try await started("autosave")
        defer { core.close() }
        _ = try await core.request("document.autosave", ["on": true, "scratch": "kit-autosave.json"])
        _ = try await add(model, "text.literal", 0, 0)
        #expect(await model.wait(timeout: .seconds(30)) { model.state.autosave.on && model.state.autosave.state == "saved" && !model.state.dirty })
        _ = try await core.request("document.autosave", ["path": "/plumcraft-nowhere/kit.json"])
        _ = try await add(model, "text.literal", 300, 0)
        #expect(await model.wait(timeout: .seconds(30)) { model.state.autosave.state == "failed" && model.state.autosave.error != nil && model.state.dirty })
    }
}
