// The scene (docs/shell-interface.md section 8, as built; crates/editor-core/src/scene.rs is the
// wire shape and crates/editor-core/fixtures/scene/*.json the reference). Field names are the
// JSON's. Every item has `id`, `nid` (unique within the document, for hot calls) and `rev` (the
// scene generation it last changed at).

import Foundation

/// An item of the scene: a face, wire, net, frame, group or mark.
public protocol SceneItem: Codable, Hashable, Sendable, Identifiable where ID == String {
    var id: String { get }
    var nid: UInt64 { get }
    var rev: UInt64 { get }
}

// MARK: - Words

/// A node's real run state (8.6's first layer).
public enum RunWord: String, Sendable, CaseIterable {
    case queued, started, progress, preview, done, cached, error, skipped, cancelled
}

/// What the run animation shows (8.6's second layer).
public enum ShownWord: String, Sendable, CaseIterable {
    case waiting, running, done, cached, failed, skipped
}

public enum FaceViewWord: String, Sendable, CaseIterable { case full, compact, collapsed }
public enum SpotWord: String, Sendable, CaseIterable { case spinner, dot, none }
public enum PinShapeWord: String, Sendable, CaseIterable { case circle, square }
public enum RowSourceWord: String, Sendable, CaseIterable { case wired, socket, inherited, own, readonly }
public enum WidgetWord: String, Sendable, CaseIterable { case number, toggle, choice, multichoice, text, multiline, path, color, editor }
public enum WireWord: String, Sendable, CaseIterable { case plain, routed, net }
public enum ToneWord: String, Sendable, CaseIterable { case info, warn, error }
public enum MenuKindWord: String, Sendable, CaseIterable { case node, proxy }

public typealias RunState = Open<RunWord>
public typealias ShownState = Open<ShownWord>
public typealias FaceView = Open<FaceViewWord>
public typealias SpotKind = Open<SpotWord>
public typealias PinShape = Open<PinShapeWord>
public typealias RowSource = Open<RowSourceWord>
public typealias WidgetKind = Open<WidgetWord>
public typealias WireKind = Open<WireWord>
public typealias Tone = Open<ToneWord>

// MARK: - The whole scene and its deltas

/// `scene.get`'s reply without `since` (or with one too old): every item.
public struct Scene: Codable, Hashable, Sendable {
    public var shell: UInt32
    public var doc: UInt64
    public var gen: UInt64
    public var full: Bool
    public var nodes: [Face]
    public var wires: [Wire]
    public var nets: [Net]
    public var frames: [Frame]
    public var groups: [Group]
    public var marks: [Mark]
    public var selection: Picked
    public var focus: Focus?
    public var anim: Anim
    public var view: View

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        shell = try c.decode(UInt32.self, forKey: .shell)
        doc = try c.decode(UInt64.self, forKey: .doc)
        gen = try c.decode(UInt64.self, forKey: .gen)
        full = try c.decode(Bool.self, forKey: .full)
        nodes = try c.decode([Face].self, forKey: .nodes)
        wires = try c.decode([Wire].self, forKey: .wires)
        nets = try c.decode([Net].self, forKey: .nets)
        frames = try c.decode([Frame].self, forKey: .frames)
        groups = try c.decode([Group].self, forKey: .groups)
        marks = try c.decode([Mark].self, forKey: .marks)
        selection = try c.decode(Picked.self, forKey: .selection)
        focus = try c.decodeIfPresent(Focus.self, forKey: .focus)
        anim = try c.decode(Anim.self, forKey: .anim)
        view = try c.decodeIfPresent(View.self, forKey: .view) ?? View()
    }
}

/// `scene.get {since}`'s reply: the items whose `rev` is past `since`, and the ids gone since.
/// `selection`, `focus`, `anim` and `view` come every time.
public struct Delta: Codable, Hashable, Sendable {
    public var shell: UInt32
    public var doc: UInt64
    public var gen: UInt64
    public var full: Bool
    public var since: UInt64
    public var upsert: Upsert
    public var remove: Removed
    public var selection: Picked
    public var focus: Focus?
    public var anim: Anim
    public var view: View

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        shell = try c.decode(UInt32.self, forKey: .shell)
        doc = try c.decode(UInt64.self, forKey: .doc)
        gen = try c.decode(UInt64.self, forKey: .gen)
        full = try c.decode(Bool.self, forKey: .full)
        since = try c.decode(UInt64.self, forKey: .since)
        upsert = try c.decode(Upsert.self, forKey: .upsert)
        remove = try c.decode(Removed.self, forKey: .remove)
        selection = try c.decode(Picked.self, forKey: .selection)
        focus = try c.decodeIfPresent(Focus.self, forKey: .focus)
        anim = try c.decode(Anim.self, forKey: .anim)
        view = try c.decodeIfPresent(View.self, forKey: .view) ?? View()
    }
}

public struct Upsert: Codable, Hashable, Sendable {
    public var nodes: [Face] = []
    public var wires: [Wire] = []
    public var nets: [Net] = []
    public var frames: [Frame] = []
    public var groups: [Group] = []
    public var marks: [Mark] = []
}

public struct Removed: Codable, Hashable, Sendable {
    public var nodes: [String] = []
    public var wires: [String] = []
    public var nets: [String] = []
    public var frames: [String] = []
    public var groups: [String] = []
    public var marks: [String] = []
}

/// What `scene.get` gave: everything, or what changed.
public enum SceneReply: Codable, Hashable, Sendable {
    case full(Scene)
    case delta(Delta)

    enum Probe: String, CodingKey { case full, since }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: Probe.self)
        // `full: true` is a whole scene, whatever was asked (a since too old, or newer than gen)
        if try c.decode(Bool.self, forKey: .full) {
            self = .full(try Scene(from: decoder))
        } else {
            self = .delta(try Delta(from: decoder))
        }
    }

    public func encode(to encoder: Encoder) throws {
        switch self {
        case .full(let s): try s.encode(to: encoder)
        case .delta(let d): try d.encode(to: encoder)
        }
    }

    public var gen: UInt64 {
        switch self {
        case .full(let s): s.gen
        case .delta(let d): d.gen
        }
    }
}

/// What the core asks the view to show: the latest fit, the palette while open.
public struct View: Codable, Hashable, Sendable {
    public var fit: Fit?
    public var palette: Palette?
    /// The row a field editor is open on (`row.edit`, N4 slice 2).
    public var edit: RowEdit?
    /// Select mode is on (`canvas.selectMode`, N4 slice 3): a one-finger drag on empty canvas
    /// draws a marquee. Absent while off.
    public var select: Bool?
    /// The document's windows (N4 slice 3): the parameters inspector and the pop-outs.
    public var windows: Windows?
    /// A node to reveal (`canvas.reveal`, N4 slice 3): centred once per `seq`.
    public var reveal: Reveal?
    /// A name edited in place (`name.edit`, N4 slice 3).
    public var name: NameEdit?
    /// The latest zoom asked for (`canvas.zoom`, N5 slice 3: the Windows app's zoom keys). The Apple apps
    /// keep their own ⌘+ ⌘− ⌘0 and don't follow it; decoded so the scene is read whole.
    public var zoom: ZoomAsk?
    public init(fit: Fit? = nil, palette: Palette? = nil, edit: RowEdit? = nil, select: Bool? = nil, windows: Windows? = nil, name: NameEdit? = nil) {
        self.fit = fit; self.palette = palette; self.edit = edit; self.select = select; self.windows = windows; self.name = name
    }
    public var selectMode: Bool { select ?? false }
}

/// Zoom the view by `by` or to `to` about its centre, once per `seq` (`canvas.zoom`, N5 slice 3).
public struct ZoomAsk: Codable, Hashable, Sendable {
    public var seq: UInt64
    public var by: Double?
    public var to: Double?
}

/// Reveal `node`, whose face is at `rect`, once per `seq`.
public struct Reveal: Codable, Hashable, Sendable {
    public var seq: UInt64
    public var node: String
    public var rect: Rect
}

/// The document's windows as the core keeps them (contract 12.5, N4 slice 3).
public struct Windows: Codable, Hashable, Sendable {
    /// The node the parameters inspector is open on.
    public var parameters: String?
    public var popouts: [PopoutWindow]?
    /// The Settings window is open on this section (N4 slice 4).
    public var settings: String?
    /// The Messages window is open (N4 slice 5).
    public var messages: Bool?
    /// The Cache Report is open (N4 slice 6): `report.get {window: "cache"}`.
    public var cache: Bool?
    /// The graph's Parameters window is open (N4 slice 6): `report.get {window: "params"}`.
    public var params: Bool?
    /// The nodes whose value editor is open (N4 slice 6): `report.get {window: "editor", node}`.
    public var editors: [String]?
    /// Compare is open (N4 slice 6): `compare.get`.
    public var compare: Bool?
    /// Changes when what an open report reads changes: read it again then.
    public var reports: UInt64?
    public init(parameters: String? = nil, popouts: [PopoutWindow]? = nil, settings: String? = nil, messages: Bool? = nil,
                cache: Bool? = nil, params: Bool? = nil, editors: [String]? = nil, compare: Bool? = nil, reports: UInt64? = nil) {
        self.parameters = parameters; self.popouts = popouts; self.settings = settings; self.messages = messages
        self.cache = cache; self.params = params; self.editors = editors; self.compare = compare; self.reports = reports
    }
    /// The report windows open, as `report.get`'s keys: `cache`, `params`, `editor:<node>`.
    public var reportKeys: [String] {
        var out: [String] = []
        if cache == true { out.append("cache") }
        if params == true { out.append("params") }
        for n in editors ?? [] { out.append("editor:\(n)") }
        return out
    }
    public var list: [PopoutWindow] { popouts ?? [] }
}

/// One pop-out window: what it follows, its title, when to bring it forward (`raised` rises)
/// and when to read its value again (`stamp` changes).
public struct PopoutWindow: Codable, Hashable, Sendable {
    public var id: UInt64
    public var node: String
    public var port: String
    public var index: Int64
    public var title: String
    public var viewer: String?
    public var raised: UInt64
    public var stamp: UInt64
}

/// A name edited in place (`view.name`): what, its text, and where the field goes (graph units).
public struct NameEdit: Codable, Hashable, Sendable {
    /// `{node: {node}}`, `{frame: {frame}}`, … (the core's target, passed back unread).
    public var target: JSONValue
    public var text: String
    public var hint: String?
    public var label: String
    public var rect: Rect
}

/// The row a field editor is open on: the node whose input it is (a member's on a collapsed
/// group's face) and its key; the face that shows it and the row's index there.
public struct RowEdit: Codable, Hashable, Sendable {
    public var node: String
    public var port: String
    public var face: String
    public var row: Int
    public init(node: String, port: String, face: String, row: Int) { self.node = node; self.port = port; self.face = face; self.row = row }
}

/// Fit `rect` into the view (zoom 0.2 to 1), once per `seq`.
public struct Fit: Codable, Hashable, Sendable {
    public var seq: UInt64
    public var rect: Rect
}

/// The node palette while open (N3 slice 1): where a pick goes, the query, a dropped wire.
public struct Palette: Codable, Hashable, Sendable {
    public var at: Point?
    public var query: String?
    public var wire: PaletteWire?
}

/// The end of a dropped wire that stays: the output it leaves, or the input it enters.
public struct PaletteWire: Codable, Hashable, Sendable {
    public var from: PortRef?
    public var to: PortRef?
}

/// The selection.
public struct Picked: Codable, Hashable, Sendable {
    public var nodes: [String] = []
    public var groups: [String] = []
    public var net: NetKey?
    public init(nodes: [String] = [], groups: [String] = [], net: NetKey? = nil) {
        self.nodes = nodes; self.groups = groups; self.net = net
    }
}

/// The group entered and the breadcrumb to it.
public struct Focus: Codable, Hashable, Sendable {
    public var group: String
    public var path: [Crumb]
}

public struct Crumb: Codable, Hashable, Sendable {
    public var id: String
    public var name: String
}

/// The run animation's switches (8.6): the shell computes the wash per frame from these.
public struct Anim: Codable, Hashable, Sendable {
    public var on: Bool
    public var replay: Bool
    public var generation: UInt64
    public var holdMs: UInt64
    public var fadeMs: UInt64
    public var pulseMs: UInt64

    public init(on: Bool = false, replay: Bool = false, generation: UInt64 = 0, holdMs: UInt64 = 1000, fadeMs: UInt64 = 600, pulseMs: UInt64 = 1200) {
        self.on = on; self.replay = replay; self.generation = generation
        self.holdMs = holdMs; self.fadeMs = fadeMs; self.pulseMs = pulseMs
    }

    enum CodingKeys: String, CodingKey {
        case on, replay, generation
        case holdMs = "hold_ms", fadeMs = "fade_ms", pulseMs = "pulse_ms"
    }
}

// MARK: - Faces

/// A node's face, or a collapsed group's (`proxy`).
public struct Face: SceneItem {
    public var id: String
    public var nid: UInt64
    public var rev: UInt64
    /// The node type ("" for a proxy).
    public var kind: String
    public var proxy: ProxyOf?
    public var pos: Point
    public var view: FaceView
    public var watched: Bool
    public var tint: Tint
    public var border: Border?
    public var header: Header
    public var inputs: [InRow]
    public var outputs: [OutRow]
    public var preview: Preview?
    public var footer: [Footer]
    public var run: RunShown
    public var shown: ShownState?
    /// The context menu kind for `menu.get` (`node` or `proxy`).
    public var menu: String
}

public struct ProxyOf: Codable, Hashable, Sendable {
    public var group: String
    public var name: String
}

public struct Tint: Codable, Hashable, Sendable {
    public var header: Paint
    public var body: Paint
}

public struct Border: Codable, Hashable, Sendable {
    public var paint: Paint
    public var width: Double
}

/// The node's real run state.
public struct RunShown: Codable, Hashable, Sendable {
    public var state: RunState?
    public var fraction: Double?
    public var stale: Bool
    public var notRun: Bool
}

public struct Header: Codable, Hashable, Sendable {
    public var open: Bool
    /// A Lucide icon name.
    public var icon: String
    public var about: About
    public var state: Spot
    public var title: Title
    public var badges: [Badge]
    public var engine: EngineMark?
    public var status: [Status]
    public var enter: Bool
}

public struct About: Codable, Hashable, Sendable {
    public var name: String
    public var description: String
    public var family: String?
    public var path: String?
    public var id: String
    public var known: Bool
}

public struct Spot: Codable, Hashable, Sendable {
    public var kind: SpotKind
    public var paint: Paint?
    public var hover: String?
}

public struct Title: Codable, Hashable, Sendable {
    public var text: String
    public var watch: Bool
}

public struct Badge: Codable, Hashable, Sendable {
    public var text: String
    public var paint: Paint
    public var hover: String
}

public struct EngineMark: Codable, Hashable, Sendable {
    public var text: String
    public var pinned: Bool
    public var hover: String
}

public struct Status: Codable, Hashable, Sendable {
    public var text: String
    public var paint: Paint
    public var strong: Bool
    public var hover: String?
}

public struct Pin: Codable, Hashable, Sendable {
    public var shape: PinShape
    public var fill: Paint
    public var wire: Paint
    public var wireable: Bool
}

/// An input row (8.4).
public struct InRow: Codable, Hashable, Sendable {
    public var key: String
    public var node: String
    public var label: String
    public var list: Bool
    public var ty: String
    public var pin: Pin
    public var source: RowSource
    public var text: String?
    public var hover: String?
    public var dim: Bool
    public var from: String?
    public var named: Named?
    public var badge: Badge?
    public var widget: Widget?
    public var inactive: String?
    /// The value its node names an editor for (FR1): a tap on the row opens it (`node.editor`
    /// on the row's `node`, the core's gesture). Absent for every other row.
    public var editor: RowOpens?
}

/// What a row whose value has an editor says of it (8.4, the scene's `RowEditor`): the editor a
/// tap on the row opens.
public struct RowOpens: Codable, Hashable, Sendable {
    public var kind: String
    public var label: String
    public var hover: String
}

public struct Named: Codable, Hashable, Sendable {
    public var name: String
    public var paint: Paint
    public var hover: String
}

/// An output row.
public struct OutRow: Codable, Hashable, Sendable {
    public var key: String
    public var node: String
    public var label: String
    public var list: Bool
    public var ty: String
    public var pin: Pin
    public var hover: String
}

/// A widget (8.5): what it shows and what bounds it. Values a shell doesn't understand are
/// passed back unchanged, so `value` and `default` are any JSON.
public struct Widget: Codable, Hashable, Sendable {
    public var kind: WidgetKind
    public var value: JSONValue
    public var `default`: JSONValue
    public var blank: Bool
    public var clearable: Bool
    public var min: Double?
    public var max: Double?
    public var softMin: Double?
    public var softMax: Double?
    public var step: Double?
    public var decimals: Int?
    /// A number as every shell shows it (the core's, N4 slice 4).
    public var text: String?
    public var int: Bool
    public var choices: [Choice]
    public var pathMode: String?
    public var colorFormat: String?
    public var hover: String?

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = try c.decode(WidgetKind.self, forKey: .kind)
        // a null value or default is a value (blank), not a missing one
        value = try c.decodeIfPresent(JSONValue.self, forKey: .value) ?? .null
        `default` = try c.decodeIfPresent(JSONValue.self, forKey: .default) ?? .null
        blank = try c.decode(Bool.self, forKey: .blank)
        clearable = try c.decode(Bool.self, forKey: .clearable)
        min = try c.decodeIfPresent(Double.self, forKey: .min)
        max = try c.decodeIfPresent(Double.self, forKey: .max)
        softMin = try c.decodeIfPresent(Double.self, forKey: .softMin)
        softMax = try c.decodeIfPresent(Double.self, forKey: .softMax)
        step = try c.decodeIfPresent(Double.self, forKey: .step)
        decimals = try c.decodeIfPresent(Int.self, forKey: .decimals)
        text = try c.decodeIfPresent(String.self, forKey: .text)
        int = try c.decode(Bool.self, forKey: .int)
        choices = try c.decode([Choice].self, forKey: .choices)
        pathMode = try c.decodeIfPresent(String.self, forKey: .pathMode)
        colorFormat = try c.decodeIfPresent(String.self, forKey: .colorFormat)
        hover = try c.decodeIfPresent(String.self, forKey: .hover)
    }
}

public struct Choice: Codable, Hashable, Sendable {
    public var value: JSONValue
    public var label: String
    public var disabled: String?

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        value = try c.decodeIfPresent(JSONValue.self, forKey: .value) ?? .null
        label = try c.decode(String.self, forKey: .label)
        disabled = try c.decodeIfPresent(String.self, forKey: .disabled)
    }
}

/// A preview (only in view `full`): a picture by its value reference, or a waveform. `handle`
/// is the value's reference as JSON, what `media.get` takes.
public enum Preview: Codable, Hashable, Sendable {
    case rgba8(Picture)
    case peaks(Peaks)
    /// A kind this kit doesn't know: drawn as nothing.
    case unknown(String)

    public struct Picture: Codable, Hashable, Sendable {
        public var handle: JSONValue
        public var w: UInt32
        public var h: UInt32
        public var size: Size
        public var hoverSize: Size
        public var items: UInt64
        public var summary: String
        /// The picture's own name (a list of named pictures: the one shown), when the engine gives one.
        public var caption: String?
        public var note: String?
        public var faded: Bool
    }

    public struct Peaks: Codable, Hashable, Sendable {
        public var handle: JSONValue
        public var channels: UInt16
        public var size: Size
        public var note: String?
        public var faded: Bool
    }

    enum Kind: String, CodingKey { case kind }

    public init(from decoder: Decoder) throws {
        let kind = try decoder.container(keyedBy: Kind.self).decode(String.self, forKey: .kind)
        switch kind {
        case "rgba8": self = .rgba8(try Picture(from: decoder))
        case "peaks": self = .peaks(try Peaks(from: decoder))
        default: self = .unknown(kind)
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: Kind.self)
        switch self {
        case .rgba8(let p): try c.encode("rgba8", forKey: .kind); try p.encode(to: encoder)
        case .peaks(let p): try c.encode("peaks", forKey: .kind); try p.encode(to: encoder)
        case .unknown(let k): try c.encode(k, forKey: .kind)
        }
    }

    /// The box it is drawn in (graph units); none for a kind this kit doesn't know.
    public var size: Size? {
        switch self {
        case .rgba8(let p): p.size
        case .peaks(let p): p.size
        case .unknown: nil
        }
    }
}

/// A footer item, in the order drawn (8.7).
public enum Footer: Codable, Hashable, Sendable {
    case fields(text: String, hover: String, all: Bool)
    case convert(lines: [Line])
    case editor(label: String, hover: String)
    case progress(fraction: Double, stage: String?)
    case error(text: String, full: String, paint: Paint)
    case save(port: String, sheet: Bool)
    case excerpt(text: String, faded: Bool, hover: String)
    case engineText(text: String, full: String)
    case appText(text: String, full: String)
    /// A kind this kit doesn't know: skipped.
    case unknown(String)

    enum K: String, CodingKey { case kind, text, hover, all, lines, label, fraction, stage, full, paint, port, sheet, faded }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: K.self)
        let kind = try c.decode(String.self, forKey: .kind)
        switch kind {
        case "fields": self = .fields(text: try c.decode(String.self, forKey: .text), hover: try c.decode(String.self, forKey: .hover), all: try c.decode(Bool.self, forKey: .all))
        case "convert": self = .convert(lines: try c.decode([Line].self, forKey: .lines))
        case "editor": self = .editor(label: try c.decode(String.self, forKey: .label), hover: try c.decode(String.self, forKey: .hover))
        case "progress": self = .progress(fraction: try c.decode(Double.self, forKey: .fraction), stage: try c.decodeIfPresent(String.self, forKey: .stage))
        case "error": self = .error(text: try c.decode(String.self, forKey: .text), full: try c.decode(String.self, forKey: .full), paint: try c.decode(Paint.self, forKey: .paint))
        case "save": self = .save(port: try c.decode(String.self, forKey: .port), sheet: try c.decode(Bool.self, forKey: .sheet))
        case "excerpt": self = .excerpt(text: try c.decode(String.self, forKey: .text), faded: try c.decode(Bool.self, forKey: .faded), hover: try c.decode(String.self, forKey: .hover))
        case "engineText": self = .engineText(text: try c.decode(String.self, forKey: .text), full: try c.decode(String.self, forKey: .full))
        case "appText": self = .appText(text: try c.decode(String.self, forKey: .text), full: try c.decode(String.self, forKey: .full))
        default: self = .unknown(kind)
        }
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: K.self)
        switch self {
        case .fields(let text, let hover, let all):
            try c.encode("fields", forKey: .kind); try c.encode(text, forKey: .text); try c.encode(hover, forKey: .hover); try c.encode(all, forKey: .all)
        case .convert(let lines):
            try c.encode("convert", forKey: .kind); try c.encode(lines, forKey: .lines)
        case .editor(let label, let hover):
            try c.encode("editor", forKey: .kind); try c.encode(label, forKey: .label); try c.encode(hover, forKey: .hover)
        case .progress(let fraction, let stage):
            try c.encode("progress", forKey: .kind); try c.encode(fraction, forKey: .fraction); try c.encodeIfPresent(stage, forKey: .stage)
        case .error(let text, let full, let paint):
            try c.encode("error", forKey: .kind); try c.encode(text, forKey: .text); try c.encode(full, forKey: .full); try c.encode(paint, forKey: .paint)
        case .save(let port, let sheet):
            try c.encode("save", forKey: .kind); try c.encode(port, forKey: .port); try c.encode(sheet, forKey: .sheet)
        case .excerpt(let text, let faded, let hover):
            try c.encode("excerpt", forKey: .kind); try c.encode(text, forKey: .text); try c.encode(faded, forKey: .faded); try c.encode(hover, forKey: .hover)
        case .engineText(let text, let full):
            try c.encode("engineText", forKey: .kind); try c.encode(text, forKey: .text); try c.encode(full, forKey: .full)
        case .appText(let text, let full):
            try c.encode("appText", forKey: .kind); try c.encode(text, forKey: .text); try c.encode(full, forKey: .full)
        case .unknown(let k):
            try c.encode(k, forKey: .kind)
        }
    }
}

public struct Line: Codable, Hashable, Sendable {
    public var text: String
    public var paint: Paint
    public var hover: String?
}

// MARK: - Wires, nets, frames, groups, marks

/// A wire, keyed by the input it feeds. `path` is a cubic chain: the start, then two control
/// points and the end a segment; empty for a wire drawn through a net.
public struct Wire: SceneItem {
    public var id: String
    public var nid: UInt64
    public var rev: UInt64
    public var from: PortRef
    public var to: PortRef
    public var fromFace: String
    public var fromPin: Int
    public var toFace: String
    public var toPin: Int
    public var paint: Paint
    public var kind: WireKind
    public var via: [Point]
    public var slice: String?
    public var net: NetKey?
    public var path: [Point]
}

public struct Net: SceneItem {
    public var id: String
    public var nid: UInt64
    public var rev: UInt64
    public var key: NetKey
    public var paint: Paint
    public var selected: Bool
    public var driver: NetDriver
    public var uses: [NetUse]
}

public struct NetDriver: Codable, Hashable, Sendable {
    /// The driver's tag: the net's name and how many inputs it feeds ("picture ×3", N4 slice 3).
    public var text: String?
    public var from: PortRef
    public var face: String
    public var pin: Int
    /// The tag's offset from its pin.
    public var at: Point
}

public struct NetUse: Codable, Hashable, Sendable {
    public var to: PortRef
    public var face: String
    public var pin: Int
    public var at: Point
}

public struct Frame: SceneItem {
    public var id: String
    public var nid: UInt64
    public var rev: UInt64
    public var title: String
    public var pos: Point
    public var size: Size
    public var paint: Paint
}

/// A group: a collapsed one is drawn as its proxy face, an open one as `rect` around its members.
public struct Group: SceneItem {
    public var id: String
    public var nid: UInt64
    public var rev: UInt64
    public var name: String
    public var collapsed: Bool
    public var parent: String?
    public var depth: Int
    public var proxy: Bool
    public var rect: Rect?
}

/// An application's mark on a wire.
public struct Mark: SceneItem {
    public var id: String
    public var nid: UInt64
    public var rev: UInt64
    public var label: String
    public var tone: Tone
    public var paint: Paint
    public var tooltip: String?
    public var at: Point?
}
