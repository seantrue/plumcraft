// N4 slice 3: the kit's models of the core's windows and names, against a real core: the
// parameters inspector (every input, a hidden one set, shown and reset), a pop-out (its value, the
// viewer's state, closed), a name edited in place, and the toolbar from the core.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import Testing

@Suite @MainActor struct Windows {
    @Test func theParametersInspectorFollowsTheCoreAndSetsAHiddenInput() async throws {
        let (core, model) = try await started("params")
        defer { core.close() }
        let n = try await add(model, "text.widgets", 0, 0)
        let params = ParametersModel(model: model)
        model.onWindows = { params.follow($0) }
        await params.open(n)
        #expect(await model.wait { params.reply?.node == n })
        let r = try #require(params.reply)
        #expect(r.count == 9)
        #expect(r.hidden.isEmpty)
        #expect(r.sections.contains { $0.title == "Parameters" })
        // hide count from the face, then set it there
        let ok410 = await params.showOnFace(port: "count", false)
        #expect(ok410)
        #expect(await model.wait { params.reply?.hidden == ["count"] })
        let ok602 = await params.set(port: "count", text: "5")
        #expect(ok602)
        #expect(await model.wait { params.reply?.sections.flatMap(\.rows).first { $0.row.key == "count" }?.row.widget?.value == .int(5) })
        let undone = await model.wait { model.state.undo == "Set Value" }
        #expect(undone)
        // reset, and a refusal says why
        let ok464 = await params.reset(port: "count")
        #expect(ok464)
        #expect(await model.wait { params.reply?.sections.flatMap(\.rows).first { $0.row.key == "count" }?.reset == false })
        var refused: String?
        params.onNote = { _, t, e in if e { refused = t } }
        let lots = await params.set(port: "count", text: "lots")
        #expect(!lots)
        #expect(refused != nil)
        await params.close()
        #expect(await model.wait { params.node == nil })
    }

    @Test func aPopOutIsTheCoresWindowWithItsViewerState() async throws {
        let (core, model) = try await started("popout")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        var seen: [PopoutDiff] = []
        var known: [UInt64: PopoutWindow] = [:]
        model.onWindows = { w in
            seen.append(PopoutDiff.of(known, w.list))
            known = Dictionary(uniqueKeysWithValues: w.list.map { ($0.id, $0) })
        }
        _ = try await model.run("view.popout", ["node": .string(a), "port": "text"])
        #expect(await model.wait { model.windows.list.count == 1 })
        let w = try #require(model.windows.list.first)
        #expect(seen.last?.opened.map(\.id) == [w.id])
        let p = PopoutModel(model: model, window: w)
        p.refresh()
        #expect(await model.wait { p.value != nil })
        #expect(p.value?.text?.wrap == true)
        await p.text(mono: true)
        #expect(await model.wait { p.value?.text?.mono == true })
        // asked for again: brought forward
        _ = try await model.run("view.popout", ["node": .string(a), "port": "text"])
        #expect(await model.wait { (model.windows.list.first?.raised ?? 0) == 1 })
        #expect(seen.last?.raised.map(\.id) == [w.id])
        await p.close()
        #expect(await model.wait { model.windows.list.isEmpty })
        #expect(seen.last?.closed == [w.id])
    }

    @Test func aNameIsEditedInPlaceAndTheToolbarIsTheCores() async throws {
        let (core, model) = try await started("names")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let names = NameEditor(model: model)
        var opened: NameEdit?
        names.onOpen = { opened = $0 }
        model.onName = { names.follow($0) }
        _ = try await model.run("name.edit", ["node": .string(a)])
        #expect(await model.wait { opened != nil })
        #expect(opened?.label == "Node title")
        let tab = await names.commit("a\tb")
        #expect(!tab, "a control character is refused")
        #expect(names.error != nil)
        let words = await names.commit("words")
        #expect(words)
        #expect(await model.wait { names.open == nil })
        #expect(model.face(id: a)?.header.title.text == "words")
        // the toolbar: the core's buttons, Select mode's check following the core
        #expect(await model.wait { !model.toolbar.isEmpty })
        #expect(model.toolbar.contains { $0.command == "canvas.fit" && $0.icon != nil })
        _ = try await model.run("canvas.selectMode", ["on": true])
        #expect(await model.wait { model.toolbar.first { $0.command == "canvas.selectMode" }?.checked == true })
        #expect(model.view.selectMode)
    }
}
