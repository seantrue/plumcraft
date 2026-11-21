// One open document as the views see it: the scene by nid with a revision per item, the theme,
// the menus, the asks pending and the watched value, kept in step with the core by its feed.
//
// Threading: DocumentModel is @MainActor (views read it there). It subscribes to the core's feed
// (batches arrive from the pump task, already parsed) and, when the scene changed, asks for
// `scene.get {since}`, which the core wrapper decodes off the main actor; only applying the
// decoded delta (a few dictionary updates) runs here. Nothing waits on the main actor for the
// core: every request is awaited in a task.
//
// Redrawing only what changed: each item lives in its own @Observable ItemModel, replaced only
// when its `rev` changes, so a SwiftUI view reading one face is invalidated only by that face. The
// tables themselves (`faces`, `wires`, …) change only when items come or go. A layer canvas, which
// isn't SwiftUI, takes `onChange` instead: the nids upserted and removed by each apply.

import Foundation
import Observation
import PlumCraft
import PlumCraftModels

/// One scene item, observable on its own.
@MainActor
@Observable
public final class ItemModel<T: SceneItem> {
    public private(set) var item: T
    public var nid: UInt64 { item.nid }
    public var rev: UInt64 { item.rev }

    init(_ item: T) { self.item = item }

    /// Replace the item when it changed; whether it did.
    @discardableResult
    func update(_ new: T) -> Bool {
        if new.rev == item.rev && new == item { return false }
        item = new
        return true
    }
}

/// The items of one kind: by nid, in drawing order, and by id (what removals name).
public struct ItemTable<T: SceneItem> {
    public private(set) var byNid: [UInt64: ItemModel<T>] = [:]
    /// Drawing order: the scene's (sorted by id; the last face is on top).
    public private(set) var order: [UInt64] = []
    public private(set) var nidOf: [String: UInt64] = [:]

    public var count: Int { order.count }
    public subscript(nid: UInt64) -> ItemModel<T>? { byNid[nid] }
    @MainActor public func item(id: String) -> T? { nidOf[id].flatMap { byNid[$0]?.item } }
    @MainActor public var items: [T] { order.compactMap { byNid[$0]?.item } }

    /// Replace everything with `items` (a full scene), keeping the models of items still there.
    @MainActor
    mutating func replaceAll(_ items: [T], _ change: inout Set<UInt64>, _ gone: inout Set<UInt64>) {
        var next: [UInt64: ItemModel<T>] = [:]
        next.reserveCapacity(items.count)
        var ids: [String: UInt64] = [:]
        ids.reserveCapacity(items.count)
        for it in items {
            if let m = byNid[it.nid] {
                if m.update(it) { change.insert(it.nid) }
                next[it.nid] = m
            } else {
                next[it.nid] = ItemModel(it)
                change.insert(it.nid)
            }
            ids[it.id] = it.nid
        }
        for nid in byNid.keys where next[nid] == nil { gone.insert(nid) }
        byNid = next
        nidOf = ids
        order = items.map(\.nid)
    }

    /// Apply a delta's upserts and removals; whether items came or went (the table itself
    /// changed, not only items in it).
    @MainActor
    mutating func apply(upsert: [T], remove: [String], _ change: inout Set<UInt64>, _ gone: inout Set<UInt64>) -> Bool {
        var structural = false
        for id in remove {
            guard let nid = nidOf.removeValue(forKey: id) else { continue }
            byNid[nid] = nil
            gone.insert(nid)
            structural = true
        }
        for it in upsert {
            if let m = byNid[it.nid] {
                if m.update(it) { change.insert(it.nid) }
                if nidOf[it.id] != it.nid {
                    nidOf[it.id] = it.nid
                    structural = true
                }
            } else {
                // an id that came back with a new nid replaces the old item
                if let old = nidOf[it.id], old != it.nid {
                    byNid[old] = nil
                    gone.insert(old)
                }
                byNid[it.nid] = ItemModel(it)
                nidOf[it.id] = it.nid
                change.insert(it.nid)
                structural = true
            }
        }
        if structural {
            // drawing order: by id, bytewise, as the core sorts
            order = byNid.values.map(\.item).sorted { $0.id.utf8.lexicographicallyPrecedes($1.id.utf8) }.map(\.nid)
        }
        return structural
    }
}

/// What one apply changed, by nid, for a canvas that redraws by item.
public struct SceneChange: Sendable {
    public var gen: UInt64
    public var full: Bool
    /// Items new or changed (any kind: nids are unique within a document).
    public var changed: Set<UInt64>
    /// Items gone.
    public var removed: Set<UInt64>
    public var selectionChanged: Bool
    public var viewChanged: Bool

    public init(gen: UInt64, full: Bool, changed: Set<UInt64>, removed: Set<UInt64>, selectionChanged: Bool, viewChanged: Bool) {
        self.gen = gen; self.full = full; self.changed = changed; self.removed = removed
        self.selectionChanged = selectionChanged; self.viewChanged = viewChanged
    }
}

/// The engine as the feed last said.
public struct EngineState: Sendable, Equatable {
    public var connected = false
    public var ready = false
    public var label = ""

    public init(connected: Bool = false, ready: Bool = false, label: String = "") {
        self.connected = connected; self.ready = ready; self.label = label
    }
}

@MainActor
@Observable
public final class DocumentModel {
    public let core: PlumCore
    /// The document's number for requests; nil: the first document open.
    public let requestDoc: UInt64?
    /// The document's number, once the core said it (the scene's `doc`).
    public private(set) var doc: UInt64?

    // the scene
    public private(set) var gen: UInt64 = 0
    public private(set) var faces = ItemTable<Face>()
    public private(set) var wires = ItemTable<Wire>()
    public private(set) var nets = ItemTable<Net>()
    public private(set) var frames = ItemTable<Frame>()
    public private(set) var groups = ItemTable<Group>()
    public private(set) var marks = ItemTable<Mark>()
    public private(set) var selection = Picked()
    public private(set) var focus: Focus?
    public private(set) var anim = Anim()
    public private(set) var view = View()

    // the rest
    public private(set) var theme: ThemeReply?
    public private(set) var menuBar: [MenuItem] = []
    public private(set) var menuGen: UInt64?
    /// The toolbar's buttons, from the core (`menu.get {context: {kind: "toolbar"}}`, N4 slice 3):
    /// label, icon, enabled, reason and check are the core's, read again whenever menus change.
    public private(set) var toolbar: [MenuItem] = []
    public private(set) var asks: [Ask] = []
    public private(set) var watched: Watched?
    public private(set) var engine = EngineState()
    /// Title, path, dirty state, undo and redo steps, as the feed last said (N3 slice 4).
    public private(set) var state = DocumentState()
    public private(set) var closing = false
    /// The last error a fetch met (a request refused, a reply that didn't decode); nil after a
    /// fetch that went well. Never thrown at the views.
    public private(set) var lastError: String?
    /// Media notes by key (`media.get`, a pending blob), the latest of each.
    public private(set) var media: [String: MediaNote] = [:]
    /// Scenes applied (full and delta), for tests and measurements.
    public private(set) var applied: UInt64 = 0

    /// Called after each scene apply with what changed (a layer canvas redraws by it).
    @ObservationIgnored public var onChange: ((SceneChange) -> Void)?
    /// Called with every feed batch's engine events (envelopes), for a log or a status line.
    @ObservationIgnored public var onEvents: (([JSONValue]) -> Void)?
    /// Called when the document's state changes (title, dirty, undo and redo, last error).
    @ObservationIgnored public var onState: ((DocumentState) -> Void)?
    /// Called when the menu bar changed (`menu.get` answered with a new `gen`).
    @ObservationIgnored public var onMenus: (([MenuItem]) -> Void)?
    /// Called when asks for this document arrive or end (the pending list after the change).
    @ObservationIgnored public var onAsks: (([Ask]) -> Void)?
    /// Called when the watched value changed.
    @ObservationIgnored public var onWatched: ((Watched?) -> Void)?
    /// Called when the engine's state changed.
    @ObservationIgnored public var onEngine: ((EngineState) -> Void)?
    /// Called when the scene's `view` changed (a fit asked, the palette opened or closed), after
    /// `onChange`: the palette's UI follows it (N4 slice 1).
    @ObservationIgnored public var onView: ((View) -> Void)?
    /// Called when the document's windows changed (`view.windows`: the parameters inspector and
    /// the pop-outs; N4 slice 3), after `onView`.
    @ObservationIgnored public var onWindows: ((Windows) -> Void)?
    /// The core's look changed (the feed's `prefs`, N4 slice 4): settings shown read them again.
    @ObservationIgnored public var onPrefs: (() -> Void)?
    @ObservationIgnored private var prefsGen: UInt64?
    /// Called when the toolbar's buttons changed (the core's toolbar model, N4 slice 3).
    @ObservationIgnored public var onToolbar: (([MenuItem]) -> Void)?
    /// Called when a name editor opened, moved or closed (`view.name`, N4 slice 3).
    @ObservationIgnored public var onName: ((NameEdit?) -> Void)?

    @ObservationIgnored private var feedTask: Task<Void, Never>?
    @ObservationIgnored private var sceneBusy = false
    @ObservationIgnored private var sceneAgain = false
    @ObservationIgnored private var watchedBusy = false
    @ObservationIgnored private var watchedAgain = false
    @ObservationIgnored private var menusBusy = false
    @ObservationIgnored private var waiters: [UInt64: CheckedContinuation<Void, Never>] = [:]
    @ObservationIgnored private var nextWaiter: UInt64 = 0
    @ObservationIgnored private var watchedNids: Set<UInt64> = []

    /// A model of document `doc` (nil: the first open) on `core`. It does nothing until `start`.
    public init(core: PlumCore, doc: UInt64? = nil) {
        self.core = core
        requestDoc = doc
        self.doc = doc
    }

    /// Follow the feed, and fetch the scene, theme and menus (the first `scene.get` and
    /// `menu.get` also start their feed changes).
    public func start() {
        guard feedTask == nil else { return }
        let stream = core.feed()
        feedTask = Task { [weak self] in
            for await batch in stream {
                guard let self else { return }
                self.handle(batch)
            }
        }
        refreshScene()
        Task { await refreshTheme() }
        refreshMenus()
        refreshEngine()
    }

    /// Read the engine's state once (`ui.inspect`'s `engine`): the feed says when it changes, but
    /// an engine that connected before this model followed the feed said so before it listened
    /// (N3 slice 5 found the iPad's engines ready before its graph's model started).
    public func refreshEngine() {
        Task {
            guard let r = try? await core.request("ui.inspect", [:], doc: requestDoc), let connected = r["engine"]["connected"].bool else { return }
            let e = EngineState(connected: connected, ready: engine.ready || connected, label: r["engine"]["label"].string ?? engine.label)
            // the feed's word, if it came meanwhile, is newer
            if engine == EngineState() && e != engine {
                engine = e
                onEngine?(e)
            }
            pulse()
        }
    }

    /// Stop following the feed (the core stays open).
    public func stop() {
        feedTask?.cancel()
        feedTask = nil
        for (_, w) in waiters { w.resume() }
        waiters = [:]
    }

    // MARK: The feed

    private func mine(_ d: UInt64?) -> Bool {
        guard let d, let doc else { return true }
        return d == doc
    }

    func handle(_ batch: FeedBatch) {
        for c in batch.changed where mine(c.doc) {
            switch c.domain.word {
            case .scene?: refreshScene()
            case .menus?: if c.gen != menuGen { refreshMenus() }
            case .engine?:
                let e = EngineState(connected: c.connected ?? engine.connected, ready: c.ready ?? engine.ready, label: c.label ?? engine.label)
                if e != engine {
                    engine = e
                    onEngine?(e)
                }
            case .document?:
                if c.closing == true { closing = true }
                if let st = DocumentState(c), st.gen >= state.gen, st != state {
                    state = st
                    onState?(st)
                }
            case .prefs?:
                // a setting, the theme or the system's appearance changed (N4 slice 4): the
                // tokens again, and whoever shows the settings
                if c.gen != prefsGen {
                    prefsGen = c.gen
                    Task { await refreshTheme() }
                    onPrefs?()
                }
            case nil:
                break  // a domain this kit doesn't know: skipped
            }
        }
        let before = asks
        for a in batch.asks where mine(a.doc) && !asks.contains(where: { $0.ask == a.ask }) {
            asks.append(a)
        }
        if !batch.asksEnded.isEmpty {
            let ended = Set(batch.asksEnded.map(\.ask))
            asks.removeAll { ended.contains($0.ask) }
        }
        if asks != before { onAsks?(asks) }
        for m in batch.media { media[m.key] = m }
        if !batch.events.isEmpty { onEvents?(batch.events) }
        pulse()
    }

    // MARK: The scene

    /// Ask for what changed since the scene this model has (all of it the first time). Calls
    /// made while one is under way coalesce into one more after it.
    public func refreshScene() {
        if sceneBusy {
            sceneAgain = true
            return
        }
        sceneBusy = true
        let since = applied == 0 ? nil : gen
        let params: JSONValue = since.map { ["since": .uint($0)] } ?? [:]
        Task {
            do {
                let r = try await core.request(SceneReply.self, "scene.get", params, doc: requestDoc)
                apply(r)
                lastError = nil
            } catch {
                lastError = "\(error)"
            }
            sceneBusy = false
            if sceneAgain {
                sceneAgain = false
                refreshScene()
            }
            pulse()
        }
    }

    /// Apply a scene reply (public for tests and for a shell that fetched one itself).
    public func apply(_ reply: SceneReply) {
        var changed = Set<UInt64>()
        var removed = Set<UInt64>()
        let before = (selection, view)
        switch reply {
        case .full(let s):
            doc = doc ?? s.doc
            faces.replaceAll(s.nodes, &changed, &removed)
            wires.replaceAll(s.wires, &changed, &removed)
            nets.replaceAll(s.nets, &changed, &removed)
            frames.replaceAll(s.frames, &changed, &removed)
            groups.replaceAll(s.groups, &changed, &removed)
            marks.replaceAll(s.marks, &changed, &removed)
            setIfChanged(s.selection, s.focus, s.anim, s.view)
            gen = s.gen
        case .delta(let d):
            doc = doc ?? d.doc
            delta(\.faces, d.upsert.nodes, d.remove.nodes, &changed, &removed)
            delta(\.wires, d.upsert.wires, d.remove.wires, &changed, &removed)
            delta(\.nets, d.upsert.nets, d.remove.nets, &changed, &removed)
            delta(\.frames, d.upsert.frames, d.remove.frames, &changed, &removed)
            delta(\.groups, d.upsert.groups, d.remove.groups, &changed, &removed)
            delta(\.marks, d.upsert.marks, d.remove.marks, &changed, &removed)
            setIfChanged(d.selection, d.focus, d.anim, d.view)
            gen = d.gen
        }
        applied += 1
        noteWatched(changed: changed, removed: removed)
        let full: Bool = if case .full = reply { true } else { false }
        onChange?(SceneChange(gen: gen, full: full, changed: changed, removed: removed,
                              selectionChanged: before.0 != selection, viewChanged: before.1 != view))
        if before.1 != view {
            onView?(view)
            if before.1.windows != view.windows { onWindows?(view.windows ?? Windows()) }
            if before.1.name != view.name { onName?(view.name) }
            // Select mode is a toolbar button's check
            if before.1.select != view.select { refreshToolbar() }
        }
    }

    /// The windows the core has open for this document.
    public var windows: Windows { view.windows ?? Windows() }

    /// Read the toolbar's buttons again.
    public func refreshToolbar() {
        Task {
            guard let m = try? await core.request(ContextMenuReply.self, "menu.get", ["context": ["kind": "toolbar"]], doc: requestDoc) else { return }
            if m.items != toolbar {
                toolbar = m.items
                onToolbar?(m.items)
            }
            pulse()
        }
    }

    /// `parameters.get {node}` (N4 slice 3): every input of the node for the inspector.
    public func parameters(node: String) async throws -> ParametersReply {
        try await core.request(ParametersReply.self, "parameters.get", ["node": .string(node)], doc: requestDoc)
    }

    /// `popout.get {id}` (N4 slice 3): a pop-out's value and its viewer's state.
    public func popout(id: UInt64) async throws -> PopoutValue {
        try await core.request(PopoutValue.self, "popout.get", ["id": .uint(id)], doc: requestDoc)
    }

    /// A delta on one table, written back only when items came or went: a table observed by a
    /// view isn't invalidated by an item changing inside it (that item's model is).
    private func delta<T: SceneItem>(_ kp: ReferenceWritableKeyPath<DocumentModel, ItemTable<T>>, _ upsert: [T], _ remove: [String],
                                     _ changed: inout Set<UInt64>, _ removed: inout Set<UInt64>) {
        if upsert.isEmpty && remove.isEmpty { return }
        var t = self[keyPath: kp]
        if t.apply(upsert: upsert, remove: remove, &changed, &removed) { self[keyPath: kp] = t }
    }

    private func setIfChanged(_ sel: Picked, _ foc: Focus?, _ an: Anim, _ v: View) {
        if sel != selection { selection = sel }
        if foc != focus { focus = foc }
        if an != anim { anim = an }
        if v != view { view = v }
    }

    /// The face of a graph node (or a collapsed group's `group:<id>`).
    public func face(id: String) -> Face? { faces.item(id: id) }

    // MARK: Watched

    /// A watched face changed, or which face is watched did: read the watched value again.
    private func noteWatched(changed: Set<UInt64>, removed: Set<UInt64>) {
        var now = watchedNids.subtracting(removed)
        var touched = false
        for nid in changed {
            guard let f = faces[nid]?.item else { continue }
            if f.watched {
                if now.insert(nid).inserted { touched = true }
                touched = true
            } else if now.remove(nid) != nil {
                touched = true
            }
        }
        if now != watchedNids { touched = true }
        watchedNids = now
        if touched { refreshWatched() }
    }

    /// Read the watched value (`ui.inspect`'s `watched`). Coalesced like the scene.
    public func refreshWatched() {
        if watchedBusy {
            watchedAgain = true
            return
        }
        watchedBusy = true
        Task {
            do {
                let i = try await core.request(Inspect.self, "ui.inspect", [:], doc: requestDoc)
                if i.watched != watched {
                    watched = i.watched
                    onWatched?(watched)
                }
            } catch {
                lastError = "\(error)"
            }
            watchedBusy = false
            if watchedAgain {
                watchedAgain = false
                refreshWatched()
            }
            pulse()
        }
    }

    // MARK: Theme and menus

    /// Read the theme's tokens (`theme.get`).
    public func refreshTheme() async {
        do {
            theme = try await core.request(ThemeReply.self, "theme.get", [:], doc: requestDoc)
        } catch {
            lastError = "\(error)"
        }
        pulse()
    }

    /// A paint's colour in the current theme (clear before the theme arrives).
    public func color(_ p: Paint) -> RGBA { p.resolve(theme?.tokens ?? [:]) }

    /// Read the menu bar again (`menu.get`; after it the feed says when menus change).
    public func refreshMenus() {
        if menusBusy { return }
        menusBusy = true
        Task {
            do {
                let m = try await core.request(MenuReply.self, "menu.get", [:], doc: requestDoc)
                let changed = m.gen != menuGen || m.menus != menuBar
                menuBar = m.menus
                menuGen = m.gen
                if changed { onMenus?(m.menus) }
                if changed || toolbar.isEmpty { refreshToolbar() }
            } catch {
                lastError = "\(error)"
            }
            menusBusy = false
            pulse()
        }
    }

    /// A context menu (`menu.get {context}`; a gesture's reply carries the `context`).
    public func contextMenu(_ context: JSONValue) async throws -> [MenuItem] {
        try await core.request(ContextMenuReply.self, "menu.get", ["context": context], doc: requestDoc).items
    }

    /// Activate a menu item: `command.run` with its params, else `ui.invoke` (which may ask).
    /// `person`: the person at the shell chose it (a menu, a toolbar button), so what it asks is
    /// the shell's to show (`ui.invoke {by: "person"}`); otherwise a script's.
    @discardableResult
    public func activate(_ item: MenuItem, person: Bool = false) async throws -> JSONValue {
        guard let command = item.command else { throw PlumError(.refused(core: nil), "\(item.id) is the shell's to do") }
        if let params = item.params { return try await run(command, params) }
        return try await invoke(command, person: person)
    }

    // MARK: Asks

    /// Answer an ask (`ask.answer {ask, answer}`); it leaves `asks` when the feed says it ended.
    public func answer(_ ask: Ask, _ answer: JSONValue) async throws {
        _ = try await core.request("ask.answer", ["ask": .uint(ask.ask), "answer": answer], doc: requestDoc)
    }

    /// Cancel an ask.
    public func cancel(_ ask: Ask) async throws {
        _ = try await core.request("ask.answer", ["ask": .uint(ask.ask), "cancel": true], doc: requestDoc)
    }

    // MARK: Commands

    /// `command.run`. Once it is answered the model fetches what changed at once rather than
    /// wait for the feed's look (read your writes, contract 7.3: the reply comes after its
    /// changes).
    @discardableResult
    public func run(_ command: String, _ params: JSONValue = [:]) async throws -> JSONValue {
        let r = try await core.command(command, params, doc: requestDoc)
        afterOwnRequest()
        return r
    }

    /// `ui.invoke {command}`: the command as a menu would run it (it may raise asks).
    @discardableResult
    public func invoke(_ command: String, person: Bool = false) async throws -> JSONValue {
        var p: [String: JSONValue] = ["command": .string(command)]
        if person { p["by"] = "person" }
        let r = try await core.request("ui.invoke", .object(p), doc: requestDoc)
        afterOwnRequest()
        return r
    }

    /// A request this model's shell sent was answered: fetch the scene now, once it has one.
    public func afterOwnRequest() {
        if applied > 0 && feedTask != nil { refreshScene() }
    }

    // MARK: Waiting (tests, and a shell that must have something before it goes on)

    /// Wake whoever waits on this model (`wait`): a model built on it changed something they may
    /// be waiting for (the parameters' rows, a pop-out's value).
    public func poke() { pulse() }

    private func pulse() {
        let w = waiters
        waiters = [:]
        for (_, c) in w { c.resume() }
    }

    /// Wait until `condition` holds, re-checking after each feed batch and each fetch this model
    /// makes, for at most `timeout`. Whether it held. Never a fixed sleep: the clock only bounds
    /// the wait.
    public func wait(timeout: Duration = .seconds(10), until condition: () -> Bool) async -> Bool {
        let deadline = ContinuousClock.now + timeout
        while !condition() {
            let left = deadline - ContinuousClock.now
            if left <= .zero { return condition() }
            await nextPulse(within: left)
        }
        return true
    }

    /// Wait for the next feed batch or fetch this model handles, or `within`, whichever first.
    public func nextChange(within: Duration) async {
        await nextPulse(within: within)
    }

    private func nextPulse(within left: Duration) async {
        let key = nextWaiter
        nextWaiter += 1
        let timer = Task { [weak self] in
            try? await Task.sleep(for: left)
            self?.expire(key)
        }
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            waiters[key] = c
        }
        timer.cancel()
    }

    private func expire(_ key: UInt64) {
        waiters.removeValue(forKey: key)?.resume()
    }
}
