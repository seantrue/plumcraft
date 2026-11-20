// What a canvas tells the core and asks it, below its views: measured faces, hits, gestures, and
// the wire paths of faces being dragged. The canvas recognises; the core decides (plan 3, rule 2).
//
// Units: every point and distance here is in graph units. A canvas converts from its view with
// its own transform (`graph = (screen - pan) / zoom`) and passes `slop` as its target size over
// its zoom (`CanvasLink.slop`).
//
// Hot paths (synchronous, no JSON, never waiting for the core thread): `hit`, `wireTarget`,
// `dragMove`, `wirePaths`, `report(_ faces:)`. Everything else is an async request.

import Foundation
import PlumCraft
import PlumCraftModels

@MainActor
public final class CanvasLink {
    public let core: PlumCore
    /// The document's number for the hot calls (0: the first open).
    public let hotDoc: UInt64
    /// The document's number for requests (nil: the first open).
    public let requestDoc: UInt64?
    /// Reused by `wirePaths` across a drag's frames.
    private var pathBuffer: [Float] = []
    /// The faces moving in the drag under way (its begin's `drag.faces`).
    public private(set) var draggedFaces: [UInt64] = []
    /// The drag under way, as its begin replied.
    public private(set) var drag: DragBegun?

    public init(core: PlumCore, doc: UInt64? = nil) {
        self.core = core
        requestDoc = doc
        hotDoc = doc ?? 0
    }

    /// The model to tell when a gesture was answered, so it fetches the scene at once.
    public weak var model: DocumentModel?

    public convenience init(model: DocumentModel) {
        self.init(core: model.core, doc: model.requestDoc)
        self.model = model
    }

    /// A target size in screen points as graph units at `zoom` (12 points at zoom 2 is 6).
    public static func slop(points: Double, zoom: Double) -> Double {
        zoom > 0 ? points / zoom : points
    }

    // MARK: Geometry

    /// Report measured faces, typed (`plum_geometry_set`). A shell reports after a face's `rev`
    /// changes, and after the text size or theme changes; all at once when it can.
    public func report(_ faces: [FaceMeasure]) throws {
        try core.geometrySet(doc: hotDoc, faces: faces)
    }

    /// Report measured faces in `geometry.set`'s JSON form: what the typed call can't carry
    /// (`offset`, header and footer `parts`). Faces by graph id.
    @discardableResult
    public func report(json faces: [FaceReport]) async throws -> GeometrySetReply {
        let r = try await core.command("geometry.set", ["faces": .array(faces.map(\.json))], doc: requestDoc)
        return try JSONValueDecoder().decode(GeometrySetReply.self, from: r)
    }

    /// Where the core has a face, pin, wire or frame (`geometry.locate`).
    public func locate(_ what: JSONValue) async throws -> Located {
        try await core.request(Located.self, "geometry.locate", what, doc: requestDoc)
    }

    // MARK: Hits

    /// What is at `at` within `slop` (typed).
    public func hit(_ at: Point, slop: Double) -> HotHit {
        core.hitTest(doc: hotDoc, x: Float(at.x), y: Float(at.y), slop: Float(slop))
    }

    /// The same as JSON, with ids (`geometry.hit`).
    public func hitJSON(_ at: Point, slop: Double) async throws -> Hit {
        try await core.request(Hit.self, "geometry.hit", ["at": at.json, "slop": .double(slop)], doc: requestDoc)
    }

    /// Where a wire from a pin may land (typed).
    public func wireTarget(face: UInt64, output: Bool, pin: UInt32, at: Point, slop: Double) -> HotHit {
        core.wireTarget(doc: hotDoc, face: face, output: output, pin: pin, x: Float(at.x), y: Float(at.y), slop: Float(slop))
    }

    // MARK: Gestures

    /// Send a recognised gesture (`ui.gesture`): the core hit-tests it and runs what it means.
    @discardableResult
    public func gesture(_ g: Gesture) async throws -> GestureReply {
        let r = try await core.request(GestureReply.self, "ui.gesture", g.json, doc: requestDoc)
        if !r.commands.isEmpty { model?.afterOwnRequest() }
        if g.kind == .drag {
            switch g.phase {
            case .begin?:
                drag = r.drag
                draggedFaces = r.drag?.faces ?? []
            case .end?, .cancel?:
                drag = nil
                draggedFaces = []
                pathBuffer.removeAll(keepingCapacity: true)
            default:
                break
            }
        }
        return r
    }

    @discardableResult
    public func tap(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil) async throws -> GestureReply {
        try await gesture(.tap(at, mods: mods, slop: slop))
    }

    @discardableResult
    public func doubleTap(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil) async throws -> GestureReply {
        try await gesture(.doubleTap(at, mods: mods, slop: slop))
    }

    /// A secondary click or long press: the reply's `context` and `menu`.
    @discardableResult
    public func context(_ at: Point, slop: Double? = nil) async throws -> GestureReply {
        try await gesture(.context(at, slop: slop))
    }

    @discardableResult
    public func marquee(_ from: Point, _ to: Point, mods: [ModifierWord] = []) async throws -> GestureReply {
        try await gesture(.marquee(from, to, mods: mods))
    }

    /// Begin a drag: the core decides what it is (a move, a wire, a marquee…) and replies.
    @discardableResult
    public func dragBegin(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil, zoom: Double? = nil) async throws -> GestureReply {
        try await gesture(.dragBegin(at, mods: mods, slop: slop, zoom: zoom))
    }

    /// Report faces' header parts (`geometry.parts`): what the typed `plum_geometry_set` can't
    /// carry, after it (the same queue keeps them in order).
    public nonisolated static func reportParts(core: PlumCore, doc: UInt64?, faces: [(id: String, parts: [String: Rect])]) async {
        guard !faces.isEmpty else { return }
        let list: [JSONValue] = faces.map { f in
            var parts: [String: JSONValue] = [:]
            for (k, r) in f.parts { parts[k] = .array([.double(r.x0), .double(r.y0), .double(r.x1), .double(r.y1)]) }
            return ["node": .string(f.id), "parts": .object(parts)]
        }
        _ = try? await core.command("geometry.parts", ["faces": .array(list)], doc: doc)
    }

    /// Where the drag is, typed (`plum_gesture_move`): never JSON, never a request. Nothing
    /// changes until the end.
    public func dragMove(_ at: Point, slop: Double) -> HotDrag {
        core.gestureMove(doc: hotDoc, x: Float(at.x), y: Float(at.y), slop: Float(slop))
    }

    /// End the drag: the core runs what it means (one undo step at most).
    @discardableResult
    public func dragEnd(_ at: Point, slop: Double? = nil) async throws -> GestureReply {
        try await gesture(.dragEnd(at, slop: slop))
    }

    /// Cancel the drag: the document stays as it was.
    @discardableResult
    public func dragCancel() async throws -> GestureReply {
        try await gesture(.dragCancel)
    }

    /// The paths of the wires on the faces being dragged, moved by `(dx, dy)` (the drag's
    /// offset from `dragMove`). Reuses one buffer for the drag.
    public func wirePaths(dx: Float, dy: Float) -> WirePaths {
        core.wirePaths(doc: hotDoc, faces: draggedFaces, dx: dx, dy: dy, buffer: &pathBuffer)
    }

    /// The paths of the wires on `faces`, moved by `(dx, dy)`.
    public func wirePaths(faces: [UInt64], dx: Float, dy: Float) -> WirePaths {
        core.wirePaths(doc: hotDoc, faces: faces, dx: dx, dy: dy, buffer: &pathBuffer)
    }

    // MARK: Keys

    /// An arrow key nudges the selection (`selection.nudge`), naming the key press so its
    /// repeats join one undo step (12.6): pass the same `press` for a key's repeats.
    @discardableResult
    public func nudge(dx: Double, dy: Double, press: UInt64) async throws -> JSONValue {
        try await core.command("selection.nudge", ["by": [.double(dx), .double(dy)], "drag": .string("key:\(press)")], doc: requestDoc)
    }
}
