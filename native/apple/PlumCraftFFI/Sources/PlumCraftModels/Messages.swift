// The contract's other messages a shell reads and sends: theme, menus, asks, watched values,
// the palette, hits and wire targets, gestures, geometry, and the feed itself.

import Foundation

// MARK: - Theme (14.1, as built: theme.get)

/// `theme.get`: every token a scene may name, resolved, in the theme the scenes are built in.
public struct ThemeReply: Codable, Hashable, Sendable {
    public var shell: UInt32
    public var name: String
    public var tokens: [String: RGBA]
    /// The theme chosen (`system` or a theme's name), the system's appearance as the core has it,
    /// and the look's generation (N4 slice 4; a core with a look of its own).
    public var chosen: String?
    public var system: SystemAppearance?
    public var gen: UInt64?
}

/// What the core has of the system's appearance (`lifecycle.appearance`, N4 slice 4).
public struct SystemAppearance: Codable, Hashable, Sendable {
    public var dark: Bool
    public var contrast: String
    public var reduceMotion: Bool
    public var reduceTransparency: Bool
}

// MARK: - Menus (12.1)

public enum RoleWord: String, Sendable, CaseIterable {
    case about, acknowledgements, preferences, quit, new, open, save, saveAs, undo, redo, cut, copy, paste, delete, selectAll
    case fileMenu, editMenu, viewMenu, helpMenu, windowMenu, fullScreen
}

public enum LookWord: String, Sendable, CaseIterable {
    case plain, check, radio, choice, selected, note, text, dim, row, section
}

public typealias Role = Open<RoleWord>
public typealias Look = Open<LookWord>

/// A keyboard shortcut: a key by its name and the modifiers (`primary` is ⌘ here).
public struct Shortcut: Codable, Hashable, Sendable {
    public var key: String
    public var mods: [Modifier]

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        key = try c.decode(String.self, forKey: .key)
        mods = try c.decodeIfPresent([Modifier].self, forKey: .mods) ?? []
    }
}

/// A menu item, a submenu (`items`, or `submenu` when empty) or a separator. Activating one is
/// `ui.invoke {command}`, or `command.run {command, params}` when it has `params`; with no
/// `command` the shell does it itself (its `id` says what).
public struct MenuItem: Codable, Hashable, Sendable, Identifiable {
    public var id: String
    public var label: String
    public var command: String?
    public var params: JSONValue?
    public var enabled: Bool
    public var reason: String?
    public var checked: Bool?
    public var shortcut: Shortcut?
    public var role: Role?
    public var hover: String?
    public var items: [MenuItem]
    public var submenu: Bool
    public var separator: Bool
    public var look: Look
    public var color: String?
    public var strong: Bool
    /// A toolbar button's icon (N4 slice 3): a Lucide name from the core's set.
    public var icon: String?
    /// Something wrong with what the item stands for that doesn't disable it (shell-interface
    /// 12.1, 2026-11-21): an engine whose manifest warned. The Mac draws the
    /// `exclamationmark.triangle` symbol as the item's image and this as its tooltip; the iPad
    /// the symbol as the action's image and this as its subtitle.
    public var warning: String?

    /// A section (`look: section`, never a submenu): its items belong inline in the menu it is
    /// in, under its label when it has one (the Engine menu's engines, B80).
    public var isSection: Bool { look.word == .section && !submenu }
    public var isSubmenu: Bool { submenu || (!items.isEmpty && !isSection) }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decodeIfPresent(String.self, forKey: .id) ?? ""
        label = try c.decodeIfPresent(String.self, forKey: .label) ?? ""
        command = try c.decodeIfPresent(String.self, forKey: .command)
        params = try c.decodeIfPresent(JSONValue.self, forKey: .params)
        enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? false
        reason = try c.decodeIfPresent(String.self, forKey: .reason)
        checked = try c.decodeIfPresent(Bool.self, forKey: .checked)
        shortcut = try c.decodeIfPresent(Shortcut.self, forKey: .shortcut)
        role = try c.decodeIfPresent(Role.self, forKey: .role)
        hover = try c.decodeIfPresent(String.self, forKey: .hover)
        items = try c.decodeIfPresent([MenuItem].self, forKey: .items) ?? []
        submenu = try c.decodeIfPresent(Bool.self, forKey: .submenu) ?? false
        separator = try c.decodeIfPresent(Bool.self, forKey: .separator) ?? false
        look = try c.decodeIfPresent(Look.self, forKey: .look) ?? .known(.plain)
        color = try c.decodeIfPresent(String.self, forKey: .color)
        strong = try c.decodeIfPresent(Bool.self, forKey: .strong) ?? false
        icon = try c.decodeIfPresent(String.self, forKey: .icon)
        warning = try c.decodeIfPresent(String.self, forKey: .warning)
    }
}

/// `menu.get`: the menu bar (`gen` rises only when the menus change).
public struct MenuReply: Codable, Hashable, Sendable {
    public var gen: UInt64?
    public var menus: [MenuItem]
}

/// `menu.get {context}`: a context menu, and the kind of thing it is for (`node`, `wire`, …).
public struct ContextMenuReply: Codable, Hashable, Sendable {
    public var context: String
    public var items: [MenuItem]
}

// MARK: - Asks (11)

public enum AskWord: String, Sendable, CaseIterable {
    case fileOpen = "file.open", fileSave = "file.save", folderPick = "folder.pick", prompt, form, text
    case clipboardSet = "clipboard.set", clipboardGet = "clipboard.get", share, openExternal = "open.external", screenshot
}

public typealias AskKind = Open<AskWord>

/// Something the core asks the shell (a file, a prompt, a form, the clipboard…), answered with
/// `ask.answer {ask, answer}` or `{ask, cancel: true}`. `params` are the kind's (11.2).
public struct Ask: Codable, Hashable, Sendable, Identifiable {
    public var ask: UInt64
    public var kind: AskKind
    public var modal: Bool
    /// Who started the chain it belongs to: `person` or `script`.
    public var by: String?
    public var doc: UInt64?
    public var params: JSONValue

    public var id: UInt64 { ask }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        ask = try c.decode(UInt64.self, forKey: .ask)
        kind = try c.decode(AskKind.self, forKey: .kind)
        modal = try c.decodeIfPresent(Bool.self, forKey: .modal) ?? false
        by = try c.decodeIfPresent(String.self, forKey: .by)
        doc = try c.decodeIfPresent(UInt64.self, forKey: .doc)
        params = try c.decodeIfPresent(JSONValue.self, forKey: .params) ?? .null
    }
}

/// An ask that ended: `answered`, `cancelled` or `withdrawn`.
public struct AskEnded: Codable, Hashable, Sendable {
    public var ask: UInt64
    public var ended: String
    public var doc: UInt64?
}

// MARK: - The watched value (ui.inspect's `watched`, 13.1 as built)

public struct WatchedPicture: Codable, Hashable, Sendable {
    /// The rgba8 rendition's reference: `handle` is what `plum_blob` and `media.get` take.
    public var ref: JSONValue
    public var w: UInt32
    public var h: UInt32

    /// The handle to borrow, when the reference names one.
    public var handle: String? { ref["handle"].string }
}

public struct WatchedExcerpt: Codable, Hashable, Sendable {
    public var text: String
    public var offset: UInt64?
    public var totalChars: UInt64?
    public var lines: UInt64?
}

/// What the Watched panel shows: the value of `{node, port}`, which viewer the core picked first
/// (`picture`, `text.plain`, `scalar`, …), and the parts a native viewer reads. Value fields are
/// nil until a run has sent the value.
public struct Watched: Codable, Hashable, Sendable {
    public var node: String
    public var port: String
    public var summary: String?
    public var media: String?
    public var viewer: String?
    /// A small value inline (a scalar's).
    public var value: JSONValue?
    /// The full value's reference.
    public var data: JSONValue?
    public var picture: WatchedPicture?
    public var excerpt: WatchedExcerpt?
    public var error: JSONValue?
}

/// The part of `ui.inspect` the kit reads (the rest is skipped).
public struct Inspect: Codable, Hashable, Sendable {
    public var watched: Watched?
    public var run: RunSummary?
    public var undo: String?
    public var redo: String?
    public var dirty: Bool?
    public var title: String?

    public struct RunSummary: Codable, Hashable, Sendable {
        public var running: Bool?
    }
}

// MARK: - The palette (12.7)

public struct PaletteItem: Codable, Hashable, Sendable, Identifiable {
    public var kind: String
    public var label: String
    public var category: String
    /// With a dropped wire: the port a pick connects.
    public var port: String?
    /// The engine choosing it starts (its kinds aren't in the catalogue until it is up; the add
    /// waits for it, N4 slice 1).
    public var starts: String?
    public var id: String { kind }
}

/// `palette.items`: what the open palette offers, in category order.
public struct PaletteItems: Codable, Hashable, Sendable {
    public var items: [PaletteItem]
    public var total: UInt64
    public var query: String?
    public var wire: PaletteWire?
}

// MARK: - Hits, wire targets, locate (9, as JSON)

public enum HitWord: String, Sendable, CaseIterable { case none, node, pin, wire, bend, tag, frame, group }
public typealias HitKind = Open<HitWord>

/// `geometry.hit`'s reply (and a gesture's `hit`, a drag's `target`): what is at a point.
public struct Hit: Codable, Hashable, Sendable {
    public var kind: HitKind
    public var at: Point
    public var nid: UInt64?
    public var node: String?
    public var part: String?
    /// `in` or `out`, for a pin.
    public var side: String?
    public var pin: Int?
    public var type: String?
    public var fits: Int?
    public var wire: String?
    public var index: Int?
    public var net: String?
    public var frame: String?
    public var group: String?
}

/// `geometry.locate`'s reply: where the core has something, at the geometry it has.
public struct Located: Codable, Hashable, Sendable {
    public var at: Point
    public var rect: Rect?
    public var nid: UInt64?
    public var measured: Bool?
}

// MARK: - Gestures (6.3, 8.13)

public enum GestureWord: String, Sendable, CaseIterable { case tap, doubleTap, context, longPress, drag, marquee }
public enum PhaseWord: String, Sendable, CaseIterable { case begin, move, end, cancel }
public enum DragWord: String, Sendable, CaseIterable { case move, wire, marquee, bend, frame, resize, group, scrub, route }

/// A recognised gesture, in graph units (`ui.gesture`'s params).
public struct Gesture: Hashable, Sendable {
    public var kind: GestureWord
    public var phase: PhaseWord?
    public var at: Point?
    public var to: Point?
    public var mods: [ModifierWord]
    /// The hit distance in graph units: the target size over the zoom. Nil: the core's 12.
    public var slop: Double?
    /// The view's zoom (screen points a graph unit): a scrub moves by screen points. Nil: 1.
    public var zoom: Double?

    public init(_ kind: GestureWord, phase: PhaseWord? = nil, at: Point?, to: Point? = nil, mods: [ModifierWord] = [], slop: Double? = nil, zoom: Double? = nil) {
        self.kind = kind; self.phase = phase; self.at = at; self.to = to; self.mods = mods; self.slop = slop; self.zoom = zoom
    }

    public static func tap(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil) -> Gesture { Gesture(.tap, at: at, mods: mods, slop: slop) }
    public static func doubleTap(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil) -> Gesture { Gesture(.doubleTap, at: at, mods: mods, slop: slop) }
    public static func context(_ at: Point, slop: Double? = nil) -> Gesture { Gesture(.context, at: at, slop: slop) }
    public static func marquee(_ from: Point, _ to: Point, mods: [ModifierWord] = []) -> Gesture { Gesture(.marquee, at: from, to: to, mods: mods) }
    public static func dragBegin(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil, zoom: Double? = nil) -> Gesture {
        Gesture(.drag, phase: .begin, at: at, mods: mods, slop: slop, zoom: zoom)
    }
    /// A drag's move as JSON (a shell sends moves typed, `CanvasLink.dragMove`, never this).
    public static func dragMove(_ at: Point, slop: Double? = nil) -> Gesture { Gesture(.drag, phase: .move, at: at, slop: slop) }
    public static func dragEnd(_ at: Point, slop: Double? = nil) -> Gesture { Gesture(.drag, phase: .end, at: at, slop: slop) }
    public static var dragCancel: Gesture { Gesture(.drag, phase: .cancel, at: nil) }

    public var json: JSONValue {
        var o: [String: JSONValue] = ["kind": .string(kind.rawValue)]
        if let phase { o["phase"] = .string(phase.rawValue) }
        if let at { o["at"] = at.json }
        if let to { o["to"] = to.json }
        if !mods.isEmpty { o["mods"] = .array(mods.map { .string($0.rawValue) }) }
        if let slop { o["slop"] = .double(slop) }
        if let zoom { o["zoom"] = .double(zoom) }
        return .object(o)
    }
}

/// A command a gesture ran, and how it went.
public struct CommandRun: Codable, Hashable, Sendable {
    public var command: String
    public var params: JSONValue
    public var ok: Bool
    public var result: JSONValue?
    public var error: String?
}

/// A drag as its `begin` decided it.
public struct DragBegun: Codable, Hashable, Sendable {
    public var serial: UInt64
    public var kind: Open<DragWord>
    /// A move: the faces moving (nids), for `plum_wire_paths`.
    public var faces: [UInt64]?
    /// A new wire's anchor.
    public var from: JSONValue?
    /// A wired input's wire picked up.
    public var picked: PortRef?
    /// A scrub's row (`{node, port, face, index}`) and its value when it began.
    public var row: JSONValue?
    public var value: Double?
}

/// Where a drag is (`ui.gesture`'s move, or the typed `plum_gesture_move`).
public struct DragUpdate: Codable, Hashable, Sendable {
    public var serial: UInt64
    public var dx: Double
    public var dy: Double
    public var rect: Rect?
    public var touches: [String]?
    public var target: Hit?
    /// A scrub's value now.
    public var value: Double?
}

/// `ui.gesture`'s reply.
public struct GestureReply: Codable, Hashable, Sendable {
    public var commands: [CommandRun]
    public var hit: Hit?
    public var drag: DragBegun?
    public var update: DragUpdate?
    /// What `menu.get` takes for the thing under a context gesture.
    public var context: JSONValue?
    public var menu: JSONValue?
    public var cancelled: Bool?
}

// MARK: - Geometry (8.11)

/// One face's measurements in the JSON form of `geometry.set` (what the typed call lacks:
/// `offset`, header `parts`, by face id).
public struct FaceReport: Hashable, Sendable {
    public var node: String
    public var rev: UInt64?
    public var size: Size
    public var header: Double?
    public var inputs: [Double?]
    public var outputs: [Double?]
    public var rows: [Rect?]
    public var parts: [String: Rect]
    public var footer: [Rect]
    public var offset: Point?

    public init(node: String, rev: UInt64? = nil, size: Size, header: Double? = nil, inputs: [Double?] = [], outputs: [Double?] = [],
                rows: [Rect?] = [], parts: [String: Rect] = [:], footer: [Rect] = [], offset: Point? = nil) {
        self.node = node; self.rev = rev; self.size = size; self.header = header; self.inputs = inputs; self.outputs = outputs
        self.rows = rows; self.parts = parts; self.footer = footer; self.offset = offset
    }

    public var json: JSONValue {
        var o: [String: JSONValue] = ["node": .string(node), "size": size.json]
        if let rev { o["rev"] = .uint(rev) }
        if let header { o["header"] = .double(header) }
        let pin: (Double?) -> JSONValue = { $0.map(JSONValue.double) ?? .null }
        o["pins"] = ["in": .array(inputs.map(pin)), "out": .array(outputs.map(pin))]
        if !rows.isEmpty { o["rows"] = .array(rows.map { $0?.json ?? .null }) }
        if !parts.isEmpty || !footer.isEmpty {
            var p: [String: JSONValue] = [:]
            for (k, r) in parts { p[k] = r.json }
            if !footer.isEmpty { p["footer"] = .array(footer.map(\.json)) }
            o["parts"] = .object(p)
        }
        if let offset { o["offset"] = offset.json }
        return .object(o)
    }
}

/// `geometry.set`'s reply.
public struct GeometrySetReply: Codable, Hashable, Sendable {
    public var applied: UInt64
    public var stale: [String]
    public var unknown: [String]?
}

// MARK: - The feed (7.1, as built on the C ABI)

public enum DomainWord: String, Sendable, CaseIterable { case engine, document, menus, scene, prefs, window }
public typealias Domain = Open<DomainWord>

/// A domain of a document that changed. Engine changes carry `connected`, `label`, `ready`; a
/// closing document `closing`; the document's own state (N3 slice 4) `title`, `document`, `path`,
/// `dirty`, `undo`, `redo` and `lastError`.
public struct Changed: Codable, Hashable, Sendable {
    public var doc: UInt64
    public var domain: Domain
    public var gen: UInt64?
    public var connected: Bool?
    public var label: String?
    public var ready: Bool?
    /// An engine change: the warning of an engine in use whose manifest warned, as
    /// `"<name>: <warning>"` (decision 17, 2026-11-21), or null.
    public var warning: String?
    public var closing: Bool?
    public var title: String?
    public var document: String?
    public var path: String?
    public var dirty: Bool?
    public var undo: String?
    public var redo: String?
    public var lastError: String?
    /// How many errors the document has said (B167): the same one twice is two.
    public var errorSeq: UInt64?
    /// The core's autosave (N4 slice 4): `{on, state, error?, saves, failures}`.
    public var autosave: AutosaveState?
    /// An engine install under way (B71): `{repo, status}`; absent or null when none is.
    public var installing: InstallState?
    /// The status line, the core's model (research note 28).
    public var statusLine: StatusLine?
    /// The `window` domain (`platform.document_windows`, contract 4.3): a document New or Open
    /// made (`open: true`; `doc` is the new document), in document `from`, for the `person` or a
    /// `script` (`by`), with its file (`path`) when Open made it.
    public var open: Bool?
    public var from: UInt64?
    public var by: String?
}

/// An engine install under way (B71; contract 7.2's `document` domain): the repository and what
/// it is doing now ("Installing owner/repo: uv is downloading and building it (…)").
public struct InstallState: Codable, Hashable, Sendable {
    public var repo: String
    public var status: String?
    public init(repo: String, status: String? = nil) {
        self.repo = repo; self.status = status
    }
}

/// Where the core's autosave is (N4 slice 4; contract 4.4): `state` is `off`, `clean`, `pending`,
/// `untitled`, `saving`, `saved` or `failed` (then `error` says why).
public struct AutosaveState: Codable, Hashable, Sendable {
    public var on: Bool
    public var state: String
    public var error: String?
    public var saves: UInt64?
    public var failures: UInt64?
    public init(on: Bool = false, state: String = "off", error: String? = nil) {
        self.on = on; self.state = state; self.error = error
    }
}

/// The document as the feed last told it (`changed`, domain `document`): what a window's title,
/// edited dot and Edit menu show.
public struct DocumentState: Hashable, Sendable {
    public var gen: UInt64 = 0
    public var title = ""
    public var name = ""
    public var path: String?
    public var dirty = false
    /// The step Undo would undo ("Add Node"), nil when there is none.
    public var undo: String?
    public var redo: String?
    public var lastError: String?
    /// How many errors were said (B167): a banner follows this, never the text.
    public var errorSeq: UInt64 = 0
    /// The core's autosave, when on.
    public var autosave = AutosaveState()
    /// An engine install under way, while one is (B71).
    public var installing: InstallState?
    /// The status line (research note 28): what the shell's bottom bar draws.
    public var statusLine = StatusLine()

    public init() {}

    /// From a `document` change carrying the state (one with a `gen`).
    public init?(_ c: Changed) {
        guard c.domain.word == .document, let g = c.gen else { return nil }
        gen = g
        title = c.title ?? ""
        name = c.document ?? ""
        path = c.path
        dirty = c.dirty ?? false
        undo = c.undo
        redo = c.redo
        lastError = c.lastError
        errorSeq = c.errorSeq ?? 0
        autosave = c.autosave ?? AutosaveState()
        installing = c.installing
        statusLine = c.statusLine ?? StatusLine()
    }
}

/// The status line (research note 28; contract 7.2's `document` domain): the core's model of
/// what the editor is doing, which a shell draws and never composes. `on` is View › Status Line.
/// Hostile input: a segment that doesn't parse is left out, and a line that doesn't is empty.
public struct StatusLine: Codable, Hashable, Sendable {
    public var on: Bool
    public var segments: [StatusSegment]

    public init(on: Bool = true, segments: [StatusSegment] = []) {
        self.on = on; self.segments = segments
    }

    enum CodingKeys: String, CodingKey { case on, segments }

    public init(from decoder: Decoder) throws {
        let c = try? decoder.container(keyedBy: CodingKeys.self)
        on = (try? c?.decodeIfPresent(Bool.self, forKey: .on)) ?? true
        let raw = (try? c?.decodeIfPresent([Lenient<StatusSegment>].self, forKey: .segments)) ?? []
        segments = raw.compactMap(\.value)
    }

    /// The segments a bar of this many priorities keeps (1: the engine, the run, the newest
    /// message; 3: all), in order.
    public func shown(upTo priority: Int) -> [StatusSegment] {
        segments.filter { $0.priority <= priority }
    }

    /// What fits a line `room` wide (contract 7.2's document domain, the status line): each
    /// segment shown whole or not at all, never cut. In turn, until the line fits: the detail
    /// lines go; the segments of priority 3, then 2, go (the last first); the short forms are
    /// taken (the lowest priority first); priority 1 goes from the end. The newest message
    /// (`message.0`) keeps its place throughout and is the one cut, only when it alone is wider
    /// than the line. `measure` is a segment's width in a form; `spacing` is between segments.
    public func fit(room: Double, spacing: Double, upTo maxPriority: Int = 3,
                    measure: (StatusSegment, StatusForm) -> Double) -> [StatusPlaced] {
        var plan: [StatusPlaced] = segments.filter { $0.priority <= maxPriority }.map {
            StatusPlaced(segment: $0, form: ($0.detail?.isEmpty == false) ? .full : .text)
        }
        func width() -> Double {
            plan.reduce(0) { $0 + measure($1.segment, $1.form) } + spacing * Double(max(plan.count - 1, 0))
        }
        func fits() -> Bool { width() <= room }
        if fits() { return plan }
        // the detail lines
        for k in plan.indices where plan[k].form == .full { plan[k].form = .text }
        if fits() { return plan }
        let keep = "message.0"
        // priority 3, then 2, the last first
        for p in [3, 2] {
            while !fits(), let k = plan.lastIndex(where: { $0.segment.priority == p && $0.segment.id != keep }) {
                plan.remove(at: k)
            }
            if fits() { return plan }
        }
        // the short forms, the lowest priority first, the last first within one
        let order = plan.indices.sorted { a, b in
            plan[a].segment.priority != plan[b].segment.priority ? plan[a].segment.priority > plan[b].segment.priority : a > b
        }
        for k in order where plan[k].segment.short?.isEmpty == false && plan[k].segment.id != keep {
            plan[k].form = .short
            if fits() { return plan }
        }
        // priority 1 from the end
        while !fits(), let k = plan.lastIndex(where: { $0.segment.id != keep }) {
            plan.remove(at: k)
        }
        // the message alone, wider than the line: cut
        if let k = plan.firstIndex(where: { $0.segment.id == keep }), !fits() { plan[k].form = .cut }
        return plan
    }
}

/// How a segment is drawn: with its detail line, its text alone, its short form, or (the newest
/// message alone on a line too narrow for it) cut.
public enum StatusForm: String, Sendable, Hashable { case full, text, short, cut }

/// A segment as a line too narrow for every one shows it.
public struct StatusPlaced: Hashable, Sendable {
    public var segment: StatusSegment
    public var form: StatusForm
    public init(segment: StatusSegment, form: StatusForm) {
        self.segment = segment; self.form = form
    }
    /// The words the form shows.
    public var text: String {
        form == .short ? (segment.short ?? segment.text) : segment.text
    }
}

/// A value that may not parse: `nil` then, never an error.
public struct Lenient<T: Decodable & Sendable>: Decodable, Sendable {
    public var value: T?
    public init(from decoder: Decoder) throws {
        value = try? T(from: decoder)
    }
}

/// A command a segment runs on a click: `{id, params, label?}`.
public struct StatusCommand: Codable, Hashable, Sendable {
    public var id: String
    public var params: JSONValue?
    public var label: String?
    public init(id: String, params: JSONValue? = nil, label: String? = nil) {
        self.id = id; self.params = params; self.label = label
    }
}

public enum StatusTone: String, Sendable, CaseIterable { case info, warn, error }
public enum StatusDot: String, Sendable, CaseIterable { case ok, busy, down }

/// One segment of the status line. `role`: engine, progress, document, selection, hash, run,
/// warning, toggle, action, message; `side`: left or right; `priority` 1 to 3.
public struct StatusSegment: Codable, Hashable, Sendable, Identifiable {
    public var id: String
    public var role: String
    public var side: String?
    public var priority: Int
    public var text: String
    /// The text for a narrow line, where one makes sense ("0.51 ms" for "last run 0.51 ms").
    public var short: String?
    public var detail: String?
    public var detailTone: Open<StatusTone>?
    public var tone: Open<StatusTone>?
    public var dot: Open<StatusDot>?
    public var busy: Bool?
    public var on: Bool?
    public var command: StatusCommand?
    public var tooltip: String?

    public init(id: String, role: String, priority: Int = 1, text: String) {
        self.id = id; self.role = role; self.priority = priority; self.text = text
    }

    enum CodingKeys: String, CodingKey { case id, role, side, priority, text, short, detail, detailTone, tone, dot, busy, on, command, tooltip }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        role = (try? c.decodeIfPresent(String.self, forKey: .role)) ?? ""
        side = try? c.decodeIfPresent(String.self, forKey: .side)
        priority = (try? c.decodeIfPresent(Int.self, forKey: .priority)) ?? 3
        text = (try? c.decodeIfPresent(String.self, forKey: .text)) ?? ""
        short = try? c.decodeIfPresent(String.self, forKey: .short)
        detail = try? c.decodeIfPresent(String.self, forKey: .detail)
        detailTone = try? c.decodeIfPresent(Open<StatusTone>.self, forKey: .detailTone)
        tone = try? c.decodeIfPresent(Open<StatusTone>.self, forKey: .tone)
        dot = try? c.decodeIfPresent(Open<StatusDot>.self, forKey: .dot)
        busy = try? c.decodeIfPresent(Bool.self, forKey: .busy)
        on = try? c.decodeIfPresent(Bool.self, forKey: .on)
        command = try? c.decodeIfPresent(StatusCommand.self, forKey: .command)
        tooltip = try? c.decodeIfPresent(String.self, forKey: .tooltip)
    }
}

public enum MediaWord: String, Sendable, CaseIterable { case ready, pending, missing, failed }
public typealias MediaStatus = Open<MediaWord>

/// A value asked for (`media.get`, a pending `plum_blob`) that arrived, or why not.
public struct MediaNote: Codable, Hashable, Sendable {
    public var key: String
    public var handle: String
    public var want: String
    public var status: MediaStatus
    public var w: UInt32?
    public var h: UInt32?
    public var len: UInt64?
    public var format: String?
    public var reason: String?
    public var dtype: String?
    public var shape: [UInt64]?
    public var channels: UInt32?
    public var bins: UInt64?
}

/// The log has lines past what the shell last read; the worst of them.
public struct LogMark: Codable, Hashable, Sendable {
    public var seq: UInt64
    public var worst: String
}

/// One reply to one request.
public struct FeedReply: Codable, Hashable, Sendable {
    public var id: UInt64
    public var ok: Bool
    public var result: JSONValue?
    public var error: String?
    public var code: String?
    public var tag: JSONValue?
}

/// Everything one `plum_poll` gave.
public struct FeedBatch: Codable, Hashable, Sendable {
    public var replies: [FeedReply]
    public var events: [JSONValue]
    public var asks: [Ask]
    public var asksEnded: [AskEnded]
    public var media: [MediaNote]
    public var changed: [Changed]
    public var log: LogMark?
    public var dropped: UInt64

    public init(replies: [FeedReply] = [], events: [JSONValue] = [], asks: [Ask] = [], asksEnded: [AskEnded] = [], media: [MediaNote] = [],
                changed: [Changed] = [], log: LogMark? = nil, dropped: UInt64 = 0) {
        self.replies = replies; self.events = events; self.asks = asks; self.asksEnded = asksEnded
        self.media = media; self.changed = changed; self.log = log; self.dropped = dropped
    }

    /// Lenient: a part that won't decode is skipped item by item, never the whole poll (an
    /// unknown shape from a newer core loses only itself).
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        func items<T: Decodable>(_ k: CodingKeys) -> [T] {
            guard var u = try? c.nestedUnkeyedContainer(forKey: k) else { return [] }
            var out: [T] = []
            while !u.isAtEnd {
                if let x = try? u.decode(T.self) {
                    out.append(x)
                } else if (try? u.decode(JSONValue.self)) == nil {
                    break
                }
            }
            return out
        }
        replies = items(.replies)
        events = items(.events)
        asks = items(.asks)
        asksEnded = items(.asksEnded)
        media = items(.media)
        changed = items(.changed)
        log = try? c.decodeIfPresent(LogMark.self, forKey: .log)
        dropped = (try? c.decodeIfPresent(UInt64.self, forKey: .dropped)) ?? 0
    }

    /// Nothing for subscribers (replies are the requests').
    public var isQuiet: Bool {
        events.isEmpty && asks.isEmpty && asksEnded.isEmpty && media.isEmpty && changed.isEmpty && log == nil && dropped == 0
    }
}

// MARK: Windows (N4 slice 3)

/// `popout.get {id}`: a pop-out's value (as the Watched panel's was) and its viewer's state.
public struct PopoutValue: Codable, Hashable, Sendable {
    public var id: UInt64
    public var node: String
    public var port: String
    public var title: String?
    public var summary: String?
    public var media: String?
    public var viewer: String?
    public var value: JSONValue?
    public var data: JSONValue?
    public var picture: WatchedPicture?
    public var excerpt: WatchedExcerpt?
    public var error: JSONValue?
    public var text: TextView?
    /// The whole-view zoom asked for: "fit", "1:1", or nil (fit).
    public var fit: String?
    // N4 slice 5: the viewer's state and what it needs beyond the value
    /// The viewer's state, the core's (`viewer.table`, `viewer.tree`, `viewer.audio`,
    /// `viewer.picture`, `viewer.grid`, `viewer.hex`): `{table, tree, audio, picture, grid, hex}`.
    public var state: JSONValue?
    /// The viewers offered for the value, its default first.
    public var viewers: [OfferedViewer]?
    /// The wire type (`TABLE`, `AUDIO`, `x.pack…`).
    public var type: String?
    /// A list's length and the item shown.
    public var items: UInt64?
    public var index: Int64?
    /// A list of lists (interface 7.6 rule 2): the item shown within run `index`, a level at a
    /// time, and each level's count, outermost first (`levels[0]` is `items`); absent otherwise.
    public var path: [Int64]?
    public var levels: [UInt64]?
    /// The full value's form (`{kind: "audio", sample_rate, channels, frames}`, an array's shape…).
    public var form: JSONValue?
    /// The full value's size in bytes, its media type and handle.
    public var bytes: UInt64?
    public var mediaType: String?
    public var handle: String?
    /// An array's channel statistics (`[{name, min, max, nan}]`, numbers as the core's text).
    public var stats: JSONValue?
    /// An array's pictures, one per view (`[{ref, caption, w, h}]`).
    public var views: [ViewPicture]?
    /// A sound's waveform overview (`{ref, bins, channels}`).
    public var peaks: JSONValue?
    /// A list's items (`[{index, label, summary, thumb?: {ref, w, h}}]`).
    public var sheet: JSONValue?
    /// The value has a recipe (only a lineage-tracking engine sends one): the viewer bar offers
    /// its Recipe only then (absent from an older core: none).
    public var hasRecipe: Bool?
    /// B154 slice 1: the view modes the value's wire type offers (an array's `auto`, `color`,
    /// `stretch`, `clip`, `heat`, `labels`, `angle`; floor F51), and the one shown (`{id, params}`),
    /// which `run.view {popout, view, params?}` changes. Kept as JSON: a field of the wrong shape
    /// is skipped, never a value that can't be read.
    public var viewModes: JSONValue?
    public var viewMode: JSONValue?
    /// B159: the latest zoom asked of the pop-out (`viewer.zoom`), `{seq, to, at}`, applied once
    /// each time `seq` rises; null before any. The zoom and view a person makes are the shell's.
    public var zoomAsk: JSONValue?

    /// The view modes offered, in order (none when the value has none).
    public var modes: [String] { (viewModes?.array ?? []).compactMap(\.string) }
    /// The view mode shown (`auto` when the core names none).
    public var mode: String { viewMode?["id"].string ?? "auto" }

    public struct OfferedViewer: Codable, Hashable, Sendable {
        public var id: String
        public var label: String
        /// The Apple shells draw it (else the engine's picture stands in, contract 13.3).
        public var native: Bool?
    }

    public struct ViewPicture: Codable, Hashable, Sendable {
        public var ref: JSONValue
        public var caption: String?
        public var w: UInt32
        public var h: UInt32
        public var handle: String? { ref["handle"].string }
    }

    public struct TextView: Codable, Hashable, Sendable {
        public var wrap: Bool
        public var mono: Bool
        public var all: Bool
    }

    /// The same value as the kit's text helpers take it.
    public var watched: Watched {
        Watched(node: node, port: port, summary: summary, media: media, viewer: viewer, value: value, data: data, picture: picture, excerpt: excerpt, error: error)
    }
}

/// `parameters.get {node}`: every input of a node, in the inspector's sections.
public struct ParametersReply: Codable, Hashable, Sendable {
    public var node: String
    public var title: String
    public var kind: String
    public var display: String
    public var description: String?
    public var about: String?
    public var family: String?
    public var open: Bool
    public var sections: [Section]
    public var count: Int
    public var hidden: [String]

    public struct Section: Codable, Hashable, Sendable {
        public var title: String
        public var rows: [Row]
    }

    public struct Row: Codable, Hashable, Sendable {
        public var row: InRow
        public var onFace: Bool
        public var canHide: Bool
        public var editable: Bool
        public var `default`: JSONValue?
        public var reset: Bool
        public var help: String
    }
}
