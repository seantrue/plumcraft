// N4 slice 5: the viewers' kit against a real core: a large table is read and sorted on the core's
// media workers while the main actor goes on (a ticker on it keeps running), paged by the core's
// state; a JSON outline opens a member; a malformed value is refused with a reason.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import Testing

@Suite @MainActor struct Viewers {
    /// A pop-out of `node`'s `port`, its value read.
    func popout(_ model: DocumentModel, _ node: String, _ port: String) async throws -> PopoutModel {
        _ = try await model.run("view.popout", ["node": .string(node), "port": .string(port)])
        _ = try await model.run("run.start", [:])
        #expect(await model.wait(timeout: .seconds(60)) { model.windows.list.contains { $0.node == node } })
        let w = try #require(model.windows.list.first { $0.node == node })
        let p = PopoutModel(model: model, window: w)
        p.refresh()
        #expect(await model.wait(timeout: .seconds(60)) {
            if p.value?.summary == nil { p.refresh() }
            return p.value?.summary != nil
        })
        return p
    }

    @Test func aLargeTableIsPagedAndSortedOffTheMainThread() async throws {
        let (core, model) = try await started("viewers-table")
        defer { core.close() }
        let head = try await add(model, "text.literal", 0, 0)
        let row = try await add(model, "text.literal", 0, 200)
        let rep = try await add(model, "text.repeat", 300, 200)
        let join = try await add(model, "text.join", 600, 100)
        let tab = try await add(model, "text.text_to_table", 900, 100)
        _ = try await model.run("node.set", ["node": .string(head), "port": "text", "value": "k,v\n"])
        _ = try await model.run("node.set", ["node": .string(row), "port": "text", "value": "7,0.5\n"])
        _ = try await model.run("node.set", ["node": .string(rep), "port": "times", "value": 50_000])
        _ = try await model.run("node.set", ["node": .string(join), "port": "sep", "value": ""])
        for (a, ap, b, bp) in [(row, "text", rep, "text"), (head, "text", join, "a"), (rep, "text", join, "b"), (join, "text", tab, "text")] {
            _ = try await model.run("wire.connect", ["from": ["node": .string(a), "port": .string(ap)], "to": ["node": .string(b), "port": .string(bp)]])
        }
        let p = try await popout(model, tab, "table")
        // the main actor keeps turning while the core reads, parses and sorts
        var ticks = 0
        let ticker = Task { @MainActor in
            while !Task.isCancelled {
                ticks += 1
                try? await Task.sleep(for: .milliseconds(5))
            }
        }
        let page = await p.data("table")
        #expect(page["status"].string == "ready", "\(page)")
        #expect(page["rows"].uint64 == 50_000)
        #expect(page["cells"].array?.count == 500)
        #expect(page["pages"].uint64 == 100)
        await p.command("viewer.table", ["sort": 1, "desc": true, "page": 3])
        let sorted = await p.data("table")
        #expect(sorted["page"].uint64 == 3)
        #expect(sorted["cells"][0][1].string == "0.5")
        ticker.cancel()
        #expect(ticks > 0, "the main actor ran while the table was read")
        #expect(await model.wait { p.value?.state?["table"]["page"].uint64 == 3 })
    }

    @Test func anOutlineOpensAMemberAndAMalformedValueIsRefused() async throws {
        let (core, model) = try await started("viewers-json")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let j = try await add(model, "text.text_to_json", 300, 0)
        _ = try await model.run("node.set", ["node": .string(a), "port": "text", "value": "{\"a\": [1, {\"b\": 2}]}"])
        _ = try await model.run("wire.connect", ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(j), "port": "text"]])
        let p = try await popout(model, j, "json")
        #expect(p.value?.viewers?.map(\.id).contains("tree") == true)
        await p.command("viewer.tree", ["expand": "/a"])
        let t = await p.data("tree")
        #expect(t["nodes"]["/a"]["children"][1]["text"].string == "{1 key}")
        let bad = await p.data("table")
        #expect(bad["status"].string == "failed")
        #expect(bad["error"].string != nil)
    }
}
