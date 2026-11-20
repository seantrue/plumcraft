// The document model against a real core, headless: it follows the feed, applies deltas by nid,
// redraws only what changed, and holds the theme, menus, asks and watched value. And every live
// reply the kit decodes, decoded strictly: no field the Swift types don't know.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import Testing

/// Decode `v` strictly: the value, and the fields the types don't know.
func strict<T: Decodable>(_ type: T.Type, _ v: JSONValue) throws -> (T, [String]) {
    let r = KeyRecorder()
    let t = try JSONValueDecoder(recorder: r).decode(T.self, from: v)
    return (t, r.unknownFields(in: v))
}

@MainActor
func started(_ name: String, _ extra: [String: JSONValue] = [:]) async throws -> (PlumCore, DocumentModel) {
    let core = try openCore(name, extra)
    let model = DocumentModel(core: core)
    model.start()
    let ok = await model.wait(timeout: .seconds(30)) { model.applied > 0 && model.engine.connected && model.theme != nil && !model.menuBar.isEmpty }
    #expect(ok, "the model never had its first scene, engine, theme and menus: \(model.lastError ?? "")")
    return (core, model)
}

@MainActor
func add(_ model: DocumentModel, _ kind: String, _ x: Double, _ y: Double) async throws -> String {
    try #require(try await model.run("node.add", ["kind": .string(kind), "pos": [.double(x), .double(y)]])["id"].string)
}

@Suite @MainActor struct Model {
    @Test func itFollowsTheSceneByNidAndRedrawsOnlyWhatChanged() async throws {
        let (core, model) = try await started("model")
        defer { core.close() }
        var changes: [SceneChange] = []
        model.onChange = { changes.append($0) }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 500, 0)
        _ = try await model.run("wire.connect", ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "text"]])
        #expect(await model.wait { model.faces.count == 2 && model.wires.count == 1 })
        let fa = try #require(model.faces.nidOf[a].flatMap { model.faces[$0] })
        let fb = try #require(model.faces.nidOf[b].flatMap { model.faces[$0] })
        let (revA, revB) = (fa.rev, fb.rev)

        // one node's value changes: its face's model alone takes a new item
        changes = []
        _ = try await model.run("node.set", ["node": .string(a), "port": "text", "value": "hello"])
        #expect(await model.wait { fa.rev != revA })
        #expect(fa.item.inputs.first?.widget?.value == "hello")
        #expect(model.faces[fb.nid] === fb, "the other face keeps its model")
        #expect(fb.rev == revB, "and its revision")
        #expect(changes.contains { $0.changed.contains(fa.nid) })
        #expect(!changes.contains { $0.changed.contains(fb.nid) })

        // a tap selects: the selection comes with the delta
        let canvas = CanvasLink(model: model)
        let at = try await canvas.locate(["node": .string(b), "part": "header"]).at
        _ = try await canvas.tap(at, slop: 2)
        #expect(await model.wait { model.selection.nodes == [b] })

        // a node removed: its face and its wire go
        changes = []
        _ = try await model.run("node.remove", ["nodes": [.string(b)]])
        #expect(await model.wait { model.faces.count == 1 && model.wires.count == 0 })
        #expect(model.faces[fb.nid] == nil)
        #expect(changes.contains { $0.removed.contains(fb.nid) })

        // the model is what a full scene says, item for item
        let full = try await core.request(Scene.self, "scene.get")
        #expect(model.gen == full.gen)
        #expect(model.faces.items == full.nodes)
        #expect(model.wires.items == full.wires)
        #expect(model.selection == full.selection)
        #expect(model.lastError == nil)
    }

    @Test func manyFacesComeAndGoInOrder() async throws {
        let (core, model) = try await started("model-many")
        defer { core.close() }
        _ = try await core.request("spike.scene", ["nodes": 60, "wires": 80])
        #expect(await model.wait { model.faces.count == 60 && model.wires.count == 80 })
        let full = try await core.request(Scene.self, "scene.get")
        #expect(model.faces.items.map(\.id) == full.nodes.map(\.id), "drawing order is the scene's")
        _ = try await model.run("graph.new", ["discard": true])
        #expect(await model.wait { model.faces.count == 0 && model.wires.count == 0 })
        let a = try await add(model, "text.literal", 0, 0)
        #expect(await model.wait { model.face(id: a) != nil })
    }

    @Test func themeMenusAndAContextMenu() async throws {
        let (core, model) = try await started("model-menus")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        #expect(await model.wait { model.face(id: a) != nil })
        let face = try #require(model.face(id: a))
        // every token the face names is in the theme
        if case .token(let t) = face.tint.header { #expect(model.theme?.tokens[t] != nil, "token \(t)") }
        #expect(model.color(face.header.state.paint ?? .token("text")) != .clear || face.header.state.paint == nil)
        let titles = model.menuBar.map(\.label)
        #expect(titles.contains("File") && titles.contains("Edit"), "\(titles)")
        let canvas = CanvasLink(model: model)
        let at = try await canvas.locate(["node": .string(a), "part": "header"]).at
        let g = try await canvas.context(at, slop: 2)
        let context = try #require(g.context)
        let menu = try await model.contextMenu(context)
        func flat(_ items: [MenuItem]) -> [MenuItem] { items.flatMap { [$0] + flat($0.items) } }
        #expect(flat(menu).contains { $0.command == "node.remove" })
        // a menu item runs: View › Fit is a plain command
        if let fit = flat(model.menuBar).first(where: { $0.command == "canvas.fit" }) {
            _ = try await model.activate(fit)
            #expect(await model.wait { model.view.fit != nil })
        }
    }

    @Test func anAskArrivesAndEnds() async throws {
        let (core, model) = try await started("model-asks")
        defer { core.close() }
        _ = try await model.invoke("graph.open")
        #expect(await model.wait { !model.asks.isEmpty })
        let ask = try #require(model.asks.first)
        #expect(ask.kind == .known(.fileOpen), "\(ask.kind)")
        #expect(ask.modal)
        try await model.cancel(ask)
        #expect(await model.wait { model.asks.isEmpty })
    }

    @Test func watchedTextAndScalarArrive() async throws {
        let (core, model) = try await started("model-watched")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        _ = try await model.run("node.set", ["node": .string(a), "port": "text", "value": "hello"])
        _ = try await model.run("run.watch", ["node": .string(a), "port": "text"])
        #expect(await model.wait(timeout: .seconds(60)) { model.watched?.viewer == "text.plain" && model.watched?.excerpt != nil })
        #expect(model.watched?.excerpt?.text.hasPrefix("hello") == true, "\(String(describing: model.watched))")
        let v = try #require(try await model.run("node.add", ["kind": "vary.float", "pos": [0, 400]])["id"].string)
        _ = try await model.run("node.set", ["node": .string(v), "port": "values", "value": "2.5"])
        _ = try await model.run("run.watch", ["node": .string(v), "port": "values"])
        #expect(await model.wait(timeout: .seconds(60)) { model.watched?.viewer == "scalar" && model.watched?.value != nil })
        // the face of a run node says so
        #expect(await model.wait { model.face(id: v)?.run.state != nil })
    }

    @Test func aSecondDocumentHasItsOwnModel() async throws {
        let (core, first) = try await started("model-docs")
        defer { core.close() }
        let opened = try await core.request("document.open")
        let doc = try #require(opened["doc"].uint64)
        let second = DocumentModel(core: core, doc: doc)
        second.start()
        #expect(await second.wait { second.applied > 0 })
        #expect(second.doc == doc && first.doc != doc)
        _ = try await second.run("node.add", ["kind": "text.upper", "pos": [0, 0]])
        #expect(await second.wait { second.faces.count == 1 })
        #expect(first.faces.count == 0, "the first document's model saw nothing")
        // hot calls name the document
        let canvas = CanvasLink(model: second)
        #expect(canvas.hotDoc == doc)
        let f = try #require(second.faces.items.first)
        #expect(canvas.hit(Point(f.pos.x + 10, f.pos.y + 10), slop: 2).node == f.nid)
    }

    /// A picture from the array engine: watched, made by media.get, lent by plum_blob.
    @Test func aWatchedPictureIsLentWithoutACopy() async throws {
        let (core, model) = try await started("model-picture", ["engine": "array"])
        defer { core.close() }
        let m = try await add(model, "array.mandelbrot", 0, 0)
        let e = try await add(model, "array.encode_image", 500, 0)
        _ = try await model.run("node.set", ["node": .string(m), "port": "width", "value": 64])
        _ = try await model.run("node.set", ["node": .string(m), "port": "height", "value": 48])
        _ = try await model.run("wire.connect", ["from": ["node": .string(m), "port": "escape"], "to": ["node": .string(e), "port": "image"]])
        _ = try await model.run("run.watch", ["node": .string(e), "port": "image"])
        #expect(await model.wait(timeout: .seconds(120)) { model.watched?.viewer == "picture" && model.watched?.picture != nil })
        let pic = try #require(model.watched?.picture)
        let handle = try #require(pic.handle)
        let got = try await core.request("media.get", ["handle": .string(handle), "want": "rgba"])
        var key = got["key"].string ?? ""
        if got["status"] != "ready" {
            #expect(await model.wait(timeout: .seconds(60)) { model.media.values.contains { $0.handle == handle && $0.status == .known(.ready) } })
            key = model.media.values.first { $0.handle == handle }?.key ?? key
        }
        guard case .ready(let blob) = try core.blob(key) else { Issue.record("the picture was still pending"); return }
        #expect(blob.data.count == Int(pic.w) * Int(pic.h) * 4, "\(blob.data.count) bytes for \(pic.w)×\(pic.h)")
    }
}

@Suite @MainActor struct LiveReplies {
    /// Every reply a shell decodes, from a real core, has no field the Swift types don't know.
    @Test func noFieldIsUnknown() async throws {
        let (core, model) = try await started("strict")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 500, 0)
        _ = try await model.run("wire.connect", ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "text"]])
        _ = try await model.run("frame.add", ["pos": [-600, 600], "size": [300, 200]])
        _ = try await model.run("group.create", ["nodes": [.string(b)], "collapsed": true])
        var problems: [String] = []
        func check<T: Decodable>(_ t: T.Type, _ what: String, _ v: JSONValue) {
            do {
                let (_, missed) = try strict(T.self, v)
                if !missed.isEmpty { problems.append("\(what): \(missed)") }
            } catch {
                problems.append("\(what): \(error)")
            }
        }
        let full = try await core.request("scene.get")
        check(SceneReply.self, "scene.get", full)
        _ = try await model.run("node.set", ["node": .string(a), "port": "text", "value": "changed"])
        check(SceneReply.self, "scene.get {since}", try await core.request("scene.get", ["since": full["gen"]]))
        check(ThemeReply.self, "theme.get", try await core.request("theme.get"))
        check(MenuReply.self, "menu.get", try await core.request("menu.get"))
        let canvas = CanvasLink(model: model)
        let at = try await core.request("geometry.locate", ["node": .string(a), "part": "header"])
        check(Located.self, "geometry.locate", at)
        let p = try JSONValueDecoder().decode(Located.self, from: at).at
        check(Hit.self, "geometry.hit", try await core.request("geometry.hit", ["at": p.json, "slop": 2]))
        for g: Gesture in [.tap(p, slop: 2), .doubleTap(Point(-3000, -3000)), .context(p, slop: 2), .marquee(Point(-50, -50), Point(40, 40))] {
            check(GestureReply.self, "ui.gesture \(g.kind)", try await core.request("ui.gesture", g.json))
        }
        let ctx = try await core.request("ui.gesture", Gesture.context(p, slop: 2).json)
        check(ContextMenuReply.self, "menu.get {context}", try await core.request("menu.get", ["context": ctx["context"]]))
        let pin = try await core.request(Located.self, "geometry.locate", ["node": .string(a), "pin": ["side": "out", "index": 0]]).at
        check(GestureReply.self, "drag begin", try await core.request("ui.gesture", Gesture.dragBegin(pin, slop: 4).json))
        check(GestureReply.self, "drag move", try await core.request("ui.gesture", Gesture.dragMove(Point(300, 300), slop: 4).json))
        check(GestureReply.self, "drag end", try await core.request("ui.gesture", Gesture.dragEnd(Point(300, 300), slop: 4).json))
        // the wire dropped on nothing opened the palette for it
        check(PaletteItems.self, "palette.items", try await core.request("palette.items", ["limit": 20]))
        check(SceneReply.self, "scene.get with the palette open", try await core.request("scene.get"))
        _ = try await canvas.gesture(.dragCancel)
        let r = try await core.command("geometry.set", ["faces": [FaceReport(node: a, size: Point(100, 80), outputs: [40]).json]])
        check(GeometrySetReply.self, "geometry.set", r)
        // a run, then the watched value
        _ = try await model.run("run.watch", ["node": .string(a), "port": "text"])
        _ = try await core.request("ui.waitRun", ["timeout": 60])
        #expect(await model.wait(timeout: .seconds(60)) { model.watched?.excerpt != nil })
        let inspect = try await core.request("ui.inspect")
        check(Watched.self, "ui.inspect's watched", inspect["watched"])
        check(SceneReply.self, "scene.get after a run", try await core.request("scene.get"))
        #expect(problems == [], "\(problems.joined(separator: "\n"))")
    }
}
