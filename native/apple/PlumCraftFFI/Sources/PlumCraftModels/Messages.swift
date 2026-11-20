// The contract's other messages a shell reads and sends: theme, menus, asks, watched values,
// the palette, hits and wire targets, gestures, geometry, and the feed itself.

import Foundation

// MARK: - Theme (14.1, as built: theme.get)

/// `theme.get`: every token a scene may name, resolved, in the theme the scenes are built in.
public struct ThemeReply: Codable, Hashable, Sendable {
    public var shell: UInt32
    public var name: String
    public var tokens: [String: RGBA]
}

// MARK: - Menus (12.1)

public enum RoleWord: String, Sendable, CaseIterable {
    case about, preferences, quit, new, open, save, saveAs, undo, redo, cut, copy, paste, delete, selectAll
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

    public var isSubmenu: Bool { submenu || !items.isEmpty }

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
public enum DragWord: String, Sendable, CaseIterable { case move, wire, marquee, bend, frame, resize, group }

/// A recognised gesture, in graph units (`ui.gesture`'s params).
public struct Gesture: Hashable, Sendable {
    public var kind: GestureWord
    public var phase: PhaseWord?
    public var at: Point?
    public var to: Point?
    public var mods: [ModifierWord]
    /// The hit distance in graph units: the target size over the zoom. Nil: the core's 12.
    public var slop: Double?

    public init(_ kind: GestureWord, phase: PhaseWord? = nil, at: Point?, to: Point? = nil, mods: [ModifierWord] = [], slop: Double? = nil) {
        self.kind = kind; self.phase = phase; self.at = at; self.to = to; self.mods = mods; self.slop = slop
    }

    public static func tap(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil) -> Gesture { Gesture(.tap, at: at, mods: mods, slop: slop) }
    public static func doubleTap(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil) -> Gesture { Gesture(.doubleTap, at: at, mods: mods, slop: slop) }
    public static func context(_ at: Point, slop: Double? = nil) -> Gesture { Gesture(.context, at: at, slop: slop) }
    public static func marquee(_ from: Point, _ to: Point, mods: [ModifierWord] = []) -> Gesture { Gesture(.marquee, at: from, to: to, mods: mods) }
    public static func dragBegin(_ at: Point, mods: [ModifierWord] = [], slop: Double? = nil) -> Gesture { Gesture(.drag, phase: .begin, at: at, mods: mods, slop: slop) }
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
}

/// Where a drag is (`ui.gesture`'s move, or the typed `plum_gesture_move`).
public struct DragUpdate: Codable, Hashable, Sendable {
    public var serial: UInt64
    public var dx: Double
    public var dy: Double
    public var rect: Rect?
    public var touches: [String]?
    public var target: Hit?
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

public enum DomainWord: String, Sendable, CaseIterable { case engine, document, menus, scene }
public typealias Domain = Open<DomainWord>

/// A domain of a document that changed. Engine changes carry `connected`, `label`, `ready`; a
/// closing document `closing`.
public struct Changed: Codable, Hashable, Sendable {
    public var doc: UInt64
    public var domain: Domain
    public var gen: UInt64?
    public var connected: Bool?
    public var label: String?
    public var ready: Bool?
    public var closing: Bool?
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
