// The layer canvas, headless (layers under a plain CALayer, no window): faces laid out and drawn
// from the shared scene fixtures; on a real core, every face measured and reported, so the core's
// geometry is what is drawn; one node's change redraws one face; selection, removal, fit, net tags
// where the core hits them; the view's arithmetic (zoom about a point, the rubber band, the raster
// scale's steps).

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import Testing

let repo: URL = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

func scratch(_ name: String) -> String {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("plumcraft-test-\(ProcessInfo.processInfo.processIdentifier)")
        .appendingPathComponent("swift-\(name)")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.path
}

let testEnvironment: Void = {
    setenv("PLUMCRAFT_EDITOR_PREFS", scratch("prefs") + "/prefs.json", 1)
    // never the maintainer's store, whatever the environment names (N3 slice 4)
    setenv("COMMONKIT_STORE_ROOT", scratch("store"), 1)
    setenv("COMMONKIT_MARK_DIR", scratch("marks"), 1)
    setenv("COMMONKIT_TEST_RUN", scratch("run"), 1)
    setenv("PLUMCRAFT_CRAFT_ROOT", scratch("craft"), 1)
    setenv("COMMONKIT_HASH", "b3", 0)
    setenv("COMMONKIT_STORE_MIN_FREE_GB", "2", 0)
}()

func openCore(_ name: String) throws -> PlumCore {
    _ = testEnvironment
    return try PlumCore(config: ["store_root": .string(scratch("store-\(name)"))])
}

func fixtureScene() throws -> Scene {
    let data = try Data(contentsOf: repo.appendingPathComponent("crates/editor-core/fixtures/scene/full.json"))
    return try JSONValueDecoder().decode(Scene.self, from: try JSONValue.parse(data))
}

/// Wait (at most `limit`) for `cond`, checking every few milliseconds: never a fixed sleep.
@MainActor
func until(_ limit: Duration = .seconds(20), _ cond: () -> Bool) async -> Bool {
    let deadline = ContinuousClock.now + limit
    while !cond() {
        if ContinuousClock.now > deadline { return cond() }
        try? await Task.sleep(for: .milliseconds(5))
    }
    return true
}

/// A core, its model and a canvas on a plain layer, with the first scene drawn.
@MainActor
func canvasOnCore(_ name: String) async throws -> (PlumCore, DocumentModel, LayerCanvas) {
    let core = try openCore(name)
    let model = DocumentModel(core: core)
    let host = CALayer()
    let canvas = LayerCanvas(model: model, host: host, screenScale: 2)
    canvas.viewSize = CGSize(width: 1200, height: 800)
    model.start()
    #expect(await model.wait(timeout: .seconds(30)) { model.applied > 0 && model.engine.connected && model.theme != nil })
    return (core, model, canvas)
}

/// Until the model has `faces` faces and the canvas has laid out, reported and drawn them all.
@MainActor
func settled(_ model: DocumentModel, _ canvas: LayerCanvas, faces: Int, wires: Int = 0) async -> Bool {
    await until { model.faces.count == faces && canvas.faceCount == faces && model.wires.count == wires && canvas.allMeasured && canvas.idle && !canvas.settlePending }
}

@MainActor
func add(_ model: DocumentModel, _ kind: String, _ x: Double, _ y: Double) async throws -> String {
    try #require(try await model.run("node.add", ["kind": .string(kind), "pos": [.double(x), .double(y)]])["id"].string)
}

@Suite struct Layout {
    @Test func everyFixtureFaceLaysOutAndDraws() throws {
        let scene = try fixtureScene()
        #expect(scene.nodes.count > 5)
        let colors = CanvasColors(tokens: ["canvas": RGBA(250, 248, 244, 255)])
        for f in scene.nodes {
            let L = FaceLayout.make(f)
            #expect(L.size.width >= FaceMetrics.minWidth && L.size.width <= FaceMetrics.maxWidth, "\(f.id) \(L.size)")
            #expect(L.inputPins.count == f.inputs.count && L.outputPins.count == f.outputs.count)
            #expect(L.widgets.count == f.inputs.count)
            for y in L.inputPins + L.outputPins { #expect(y >= 0 && y <= L.size.height, "\(f.id): a pin at \(y) outside 0…\(L.size.height)") }
            for w in L.widgets.compactMap({ $0 }) { #expect(CGRect(origin: .zero, size: L.size).contains(w), "\(f.id): widget \(w)") }
            if f.view.word == .collapsed {
                #expect(L.size.height == FaceMetrics.header && L.preview == nil && L.footer.isEmpty)
            }
            if let p = L.preview { #expect(p.maxY <= L.size.height) }
            let img = FaceRenderer.render(f, L, colors: colors, scale: 2)
            let i = try #require(img, "\(f.id) drew nothing")
            #expect(i.width == Int(((L.size.width + 2 * FaceMetrics.margin) * 2).rounded(.up)))
            // what is reported is what was laid out
            let m = L.measure(face: f.nid, rev: f.rev)
            #expect(m.width == Float(L.size.width) && m.inputs.count == f.inputs.count && m.rows.count == f.inputs.count)
        }
    }

    @Test func hostileTextIsCutNotGrown() throws {
        var f = try #require(try fixtureScene().nodes.first { !$0.inputs.isEmpty && $0.proxy == nil })
        f.header.title.text = String(repeating: "W", count: 5000)
        f.inputs[0].label = String(repeating: "label ", count: 2000)
        f.inputs[0].text = f.inputs[0].label
        let status = try JSONValueDecoder().decode(Status.self, from: ["text": "12345 ms", "paint": ["token": "text_dim"], "strong": false])
        f.header.status = Array(repeating: status, count: 50)
        let L = FaceLayout.make(f)
        #expect(L.size.width <= FaceMetrics.maxWidth && L.size.height < 10_000)
        #expect(FaceRenderer.render(f, L, colors: CanvasColors(), scale: 1) != nil)
        // an empty face: a title and nothing else
        f.inputs = []
        f.outputs = []
        f.footer = []
        f.preview = nil
        f.header.title.text = ""
        let E = FaceLayout.make(f)
        #expect(E.size.width >= FaceMetrics.minWidth && E.size.height >= FaceMetrics.header)
    }

    @Test func widgetsShowTheirValues() throws {
        let scene = try fixtureScene()
        let widgets = scene.nodes.flatMap { $0.inputs.compactMap(\.widget) }
        #expect(!widgets.isEmpty)
        for w in widgets {
            let t = FaceLayout.widgetText(w)
            #expect(!t.isEmpty || [.text, .multiline, .path, .multichoice, .color].contains(w.kind.word), "\(w.kind) \(w.value): empty")
        }
    }
}

@Suite @MainActor struct View {
    @Test func zoomKeepsThePointUnderThePointer() async throws {
        let (core, _, canvas) = try await canvasOnCore("view-zoom")
        defer { core.close() }
        let p = CGPoint(x: 300, y: 200)
        let before = canvas.viewport.toGraph(p)
        canvas.zoom(by: 1.7, at: p)
        let after = canvas.viewport.toGraph(p)
        #expect(abs(before.x - after.x) < 1e-9 && abs(before.y - after.y) < 1e-9)
        canvas.zoom(by: 1000, at: p)
        #expect(canvas.viewport.zoom == Viewport.maxZoom)
        canvas.zoom(by: 1e-6, at: p)
        #expect(canvas.viewport.zoom == Viewport.minZoom)
    }

    @Test func theRasterScaleStepsInPowersOfTwo() async throws {
        let (core, _, canvas) = try await canvasOnCore("view-raster")
        defer { core.close() }
        #expect(canvas.rasterScale(forZoom: 1) == 2)
        #expect(canvas.rasterScale(forZoom: 0.9) == 2)
        #expect(canvas.rasterScale(forZoom: 0.5) == 1)
        #expect(canvas.rasterScale(forZoom: 0.3) == 1)
        #expect(canvas.rasterScale(forZoom: 0.1) == 0.5)
        #expect(canvas.rasterScale(forZoom: 3) == 4)
    }

    @Test func pastTheContentTheRubberBandGivesLessAndSpringsBack() async throws {
        let (core, model, canvas) = try await canvasOnCore("view-band")
        defer { core.close() }
        _ = try await add(model, "text.literal", 0, 0)
        #expect(await settled(model, canvas, faces: 1))
        canvas.setViewport(Viewport(pan: CGPoint(x: 400, y: 300), zoom: 1))
        let start = canvas.viewport.pan
        // far past the content: the view moves much less than asked
        canvas.pan(by: CGPoint(x: 5000, y: 0), elastic: true)
        let moved = canvas.viewport.pan.x - start.x
        #expect(moved > 0 && moved < 1200, "moved \(moved) for 5000 asked")
        canvas.endPan()
        // and the end puts the content's middle back in reach
        #expect(await until { canvas.viewport.pan.x < start.x + moved })
        // without the band, a pan is what it asks
        let p0 = canvas.viewport.pan
        canvas.pan(by: CGPoint(x: 10, y: -7), elastic: false)
        #expect(canvas.viewport.pan == CGPoint(x: p0.x + 10, y: p0.y - 7))
    }
}

@Suite(.serialized) @MainActor struct OnACore {
    @Test func whatIsDrawnIsWhatTheCoreHits() async throws {
        let (core, model, canvas) = try await canvasOnCore("canvas-geometry")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.join", 400, 40)
        _ = try await model.run("wire.connect", ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "b"]])
        #expect(await settled(model, canvas, faces: 2, wires: 1))
        let link = CanvasLink(model: model)
        for id in [a, b] {
            let nid = try #require(canvas.nid(of: id))
            let L = try #require(canvas.faceLayout(nid: nid))
            let f = try #require(model.faces[nid]?.item)
            let loc = try await link.locate(["node": .string(id)])
            #expect(loc.measured == true)
            let r = try #require(loc.rect)
            #expect(abs(r.width - Double(L.size.width)) < 0.01 && abs(r.height - Double(L.size.height)) < 0.01, "\(id): core \(r), drawn \(L.size)")
            #expect(r.x0 == f.pos.x && r.y0 == f.pos.y)
            // each pin where the canvas drew it
            for (k, y) in L.outputPins.enumerated() {
                let p = try await link.locate(["node": .string(id), "pin": ["side": "out", "index": .int(Int64(k))]])
                #expect(abs(p.at.y - (f.pos.y + Double(y))) < 0.01 && abs(p.at.x - (f.pos.x + Double(L.size.width))) < 0.01)
            }
            // the face's layer is the face's rect and its margin, with a picture
            let layer = try #require(canvas.faceLayer(nid: nid))
            #expect(layer.contents != nil)
            #expect(abs(layer.bounds.width - (L.size.width + 2 * FaceMetrics.margin)) < 0.01)
        }
        // the wire's layer follows the core's path, which follows the reported pins
        let out = try await link.locate(["node": .string(a), "pin": ["side": "out", "index": 0]])
        // the scene's path, once it is recomputed from the measurements
        #expect(await until {
            guard let p = model.wires.items.first?.path.first else { return false }
            return abs(p.x - out.at.x) < 0.01 && abs(p.y - out.at.y) < 0.01
        })
        let w = try #require(model.wires.items.first)
        // the wire as drawn follows it: its box holds the path's start, and a tile draws it
        #expect(await until { canvas.wireBox(nid: w.nid)?.contains(CGPoint(x: w.path[0].x, y: w.path[0].y)) == true && canvas.idle })
        #expect(canvas.wireShown(nid: w.nid))
        // a typed hit on the drawn pin finds the pin
        let pa = try #require(canvas.pinPoint(face: a, output: true, pin: 0))
        let h = link.hit(Point(Double(pa.x), Double(pa.y)), slop: 4)
        #expect(h.kind == .pin && h.isOutput)
    }

    @Test func oneNodesChangeRedrawsOneFace() async throws {
        let (core, model, canvas) = try await canvasOnCore("canvas-one")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 400, 0)
        #expect(await settled(model, canvas, faces: 2))
        let na = try #require(canvas.nid(of: a))
        let nb = try #require(canvas.nid(of: b))
        let pictureB = canvas.faceLayer(nid: nb)?.contents as AnyObject?
        let pictureA = canvas.faceLayer(nid: na)?.contents as AnyObject?
        let before = canvas.counts
        var drawn: [Set<UInt64>] = []
        canvas.onDrawn = { drawn.append($0) }
        _ = try await model.run("node.set", ["node": .string(a), "port": "text", "value": "a longer value than before, to change the face"])
        #expect(await until { drawn.contains { $0.contains(na) } && canvas.idle })
        let after = canvas.counts
        #expect(after.facesLaidOut - before.facesLaidOut == 1, "\(before) → \(after)")
        #expect(after.facesDrawn - before.facesDrawn == 1)
        #expect(!drawn.contains { $0.contains(nb) }, "the other face was drawn again")
        #expect(canvas.faceLayer(nid: nb)?.contents as AnyObject? === pictureB)
        #expect(canvas.faceLayer(nid: na)?.contents as AnyObject? !== pictureA)
    }

    @Test func selectionIsASublayerAndRemovalTakesTheLayers() async throws {
        let (core, model, canvas) = try await canvasOnCore("canvas-select")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 400, 0)
        _ = try await model.run("wire.connect", ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "text"]])
        #expect(await settled(model, canvas, faces: 2, wires: 1))
        // the faces' icons arrive from the core after they are first drawn, and draw them again
        // (N4 slice 2): the count is read once it has held for a moment
        var drawnBefore = canvas.counts.facesDrawn
        for _ in 0..<40 {
            try await Task.sleep(for: .milliseconds(100))
            if canvas.counts.facesDrawn == drawnBefore && canvas.idle && !canvas.settlePending { break }
            drawnBefore = canvas.counts.facesDrawn
        }
        let link = CanvasLink(model: model)
        let at = try await link.locate(["node": .string(b), "part": "header"]).at
        try await link.tap(at, slop: 2)
        #expect(await until { model.selection.nodes == [b] })
        let nb = try #require(canvas.nid(of: b))
        let na = try #require(canvas.nid(of: a))
        func selected(_ nid: UInt64) -> Bool {
            (canvas.faceLayer(nid: nid)?.sublayers ?? []).contains { ($0 as? CAShapeLayer).map { !$0.isHidden && $0.lineWidth == 2.5 } ?? false }
        }
        #expect(await until { selected(nb) })
        #expect(!selected(na))
        #expect(canvas.counts.facesDrawn == drawnBefore, "selecting drew no face")
        _ = try await model.run("node.remove", ["nodes": [.string(b)]])
        #expect(await until { canvas.faceCount == 1 && canvas.wireLayerCount == 0 })
        #expect(canvas.faceLayer(nid: nb) == nil)
    }

    @Test func theCoresFitIsTakenOncePerRequest() async throws {
        let (core, model, canvas) = try await canvasOnCore("canvas-fit")
        defer { core.close() }
        _ = try await add(model, "text.literal", 2000, 1000)
        #expect(await settled(model, canvas, faces: 1))
        canvas.setViewport(Viewport(pan: .zero, zoom: 1))
        let r = try await model.run("canvas.fit")
        let rect = (r["rect"].array ?? []).compactMap { $0.double }
        #expect(rect.count == 4)
        // the view's centre is the rect's centre
        #expect(await until {
            let c = canvas.viewport.toGraph(CGPoint(x: 600, y: 400))
            return abs(c.x - (rect[0] + rect[2]) / 2) < 1 && abs(c.y - (rect[1] + rect[3]) / 2) < 1
        })
        // moved away, the same fit (no new seq) isn't taken again
        canvas.setViewport(Viewport(pan: .zero, zoom: 1))
        _ = try await model.run("node.set", ["node": "n1", "port": "text", "value": "x"])
        try? await Task.sleep(for: .milliseconds(100))
        #expect(canvas.viewport == Viewport(pan: .zero, zoom: 1))
    }

    @Test func netTagsAreWhereTheCoreHitsThem() async throws {
        let (core, model, canvas) = try await canvasOnCore("canvas-tags")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        let b = try await add(model, "text.upper", 600, 200)
        _ = try await model.run("wire.connect", ["from": ["node": .string(a), "port": "text"], "to": ["node": .string(b), "port": "text"]])
        _ = try await model.run("wire.label", ["to": ["node": .string(b), "port": "text"], "name": "loud"])
        #expect(await until { canvas.allMeasured && canvas.idle && model.nets.count == 1 && canvas.tagCount == 2 })
        let net = try #require(model.nets.items.first)
        let pin = try #require(canvas.pinPoint(face: net.driver.face, output: true, pin: net.driver.pin))
        // the driver's tag starts at its pin plus `at`, 17 high; its middle is a tag to the core
        let mid = Point(Double(pin.x) + net.driver.at.x + 12, Double(pin.y) + net.driver.at.y)
        let h = CanvasLink(model: model).hit(mid, slop: 0)
        #expect(h.kind == .tag, "\(h)")
    }
}

/// Pictures (N3 slice 4): rgba8 with straight alpha becomes a premultiplied CGImage of its size;
/// bytes too few for the size are refused, never read past.
@Suite struct PictureBytes {
    @Test func rgba8BecomesAnImageOfItsSize() throws {
        let px: [UInt8] = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 255, 255, 255, 255, 1, 2, 3, 4, 5, 6, 7, 8]
        let p = try #require(Pictures.make(Data(px), width: 3, height: 2))
        #expect(p.width == 3 && p.height == 2 && p.image.width == 3 && p.image.height == 2)
        #expect(Pictures.make(Data(px.prefix(20)), width: 3, height: 2) == nil)
        #expect(Pictures.make(Data(), width: 0, height: 0) == nil)
        #expect(Pictures.handle(of: ["handle": "md5:raw:x"]) == "md5:raw:x")
        #expect(Pictures.handle(of: "b3:raw:y") == "b3:raw:y")
    }
}

/// Culling and the run's wash on a real core (N3 slice 4).
@Suite struct CullAndRun {
    @MainActor @Test func farFacesAndWiresAreHiddenAndComeBackWhenSeen() async throws {
        let (core, model, canvas) = try await canvasOnCore("cull")
        defer { core.close() }
        let near = try await add(model, "text.literal", 0, 0)
        let far = try await add(model, "text.upper", 20_000, 0)
        _ = try await model.run("wire.connect", ["from": ["node": .string(near), "port": "text"], "to": ["node": .string(far), "port": "text"]])
        #expect(await settled(model, canvas, faces: 2, wires: 1))
        canvas.setViewport(Viewport(pan: .zero, zoom: 1))
        let farNid = try #require(canvas.nid(of: far))
        let nearNid = try #require(canvas.nid(of: near))
        #expect(canvas.faceLayer(nid: farNid)?.isHidden == true, "20,000 units off is hidden")
        #expect(canvas.faceLayer(nid: nearNid)?.isHidden == false)
        // the wire reaches into view: kept
        #expect(await until { canvas.idle && canvas.wireShown(nid: model.wires.order[0]) })
        // pan to it: shown at once, its picture kept
        let picture = canvas.faceLayer(nid: farNid)?.contents
        canvas.setViewport(Viewport(pan: CGPoint(x: -20_000 + 100, y: 100), zoom: 1))
        #expect(canvas.faceLayer(nid: farNid)?.isHidden == false)
        #expect(canvas.faceLayer(nid: nearNid)?.isHidden == true)
        #expect(picture != nil && canvas.faceLayer(nid: farNid)?.contents as AnyObject === picture as AnyObject)
        // culling off: everything shown
        canvas.culling = false
        canvas.setViewport(Viewport(pan: CGPoint(x: -20_000 + 101, y: 100), zoom: 1))
        canvas.cull(force: true)
        #expect(canvas.hiddenCount == 0)
    }

    @MainActor @Test func aRunWashesItsFacesThenTheyFadeAndGo() async throws {
        let (core, model, canvas) = try await canvasOnCore("wash")
        defer { core.close() }
        let a = try await add(model, "text.literal", 0, 0)
        #expect(await settled(model, canvas, faces: 1))
        _ = try await model.run("run.start")
        let nid = try #require(canvas.nid(of: a))
        // while the run shows (and holds), the face has a wash; once faded the core drops it
        #expect(await until(.seconds(20)) { model.anim.on && canvas.washed(nid: nid) })
        #expect(await until(.seconds(20)) { !model.anim.on && !canvas.washed(nid: nid) })
    }
}
