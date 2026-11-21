// The contract's other messages a shell reads and sends: theme, menus, asks, watched values, the
// palette, hits and wire targets, gestures, geometry, and the feed itself (the Swift kit's
// Messages.swift).

using System.Text.Json;
using System.Text.Json.Serialization;

namespace PlumCraft.Models;

// ---------- theme (14.1, as built: theme.get) ----------

/// <summary>theme.get: every token a scene may name, resolved, in the theme the scenes are built in.</summary>
public sealed record ThemeReply
{
    public required uint Shell { get; init; }
    public required string Name { get; init; }
    public required Dictionary<string, Rgba> Tokens { get; init; }
    /// <summary>The theme chosen (`system` or a theme's name), the system's appearance as the core has
    /// it, and the look's generation (N4 slice 4).</summary>
    public string? Chosen { get; init; }
    public SystemAppearance? System { get; init; }
    public ulong? Gen { get; init; }
}

/// <summary>What the core has of the system's appearance (lifecycle.appearance, N4 slice 4).</summary>
public sealed record SystemAppearance
{
    public required bool Dark { get; init; }
    public required string Contrast { get; init; }
    public required bool ReduceMotion { get; init; }
    public required bool ReduceTransparency { get; init; }
}

// ---------- menus (12.1) ----------

public enum RoleWord
{
    About, Preferences, Quit, New, Open, Save, SaveAs, Undo, Redo, Cut, Copy, Paste, Delete, SelectAll,
    FileMenu, EditMenu, ViewMenu, HelpMenu, WindowMenu, FullScreen,
}

public enum LookWord { Plain, Check, Radio, Choice, Selected, Note, Text, Dim, Row, Section }

/// <summary>A keyboard shortcut: a key by its name and the modifiers (`primary` is Ctrl here).</summary>
public sealed record Shortcut
{
    readonly List<string>? mods;
    public required string Key { get; init; }
    public List<string> Mods { get => mods ?? []; init => mods = value; }
}

/// <summary>A menu item, a submenu (`items`, or `submenu` when empty) or a separator. Activating one is
/// ui.invoke {command}, or command.run {command, params} when it has `params`; with no `command` the
/// shell does it itself (its `id` says what). Every field may be missing.</summary>
public sealed record MenuItem
{
    // Defaulted fields are backed by a field and read through `??`: the source generator sets every
    // init-only property it has metadata for, a missing one to null, so an initializer alone would be lost.
    readonly string? id, label, look;
    readonly List<MenuItem>? items;

    public string Id { get => id ?? ""; init => id = value; }
    public string Label { get => label ?? ""; init => label = value; }
    public string? Command { get; init; }
    public JsonElement? Params { get; init; }
    public bool Enabled { get; init; }
    public string? Reason { get; init; }
    public bool? Checked { get; init; }
    public Shortcut? Shortcut { get; init; }
    public string? Role { get; init; }
    public string? Hover { get; init; }
    public List<MenuItem> Items { get => items ?? []; init => items = value; }
    public bool Submenu { get; init; }
    public bool Separator { get; init; }
    public string Look { get => look ?? "plain"; init => look = value; }
    public string? Color { get; init; }
    public bool Strong { get; init; }
    /// <summary>A toolbar button's icon (N4 slice 3): a Lucide name from the core's set.</summary>
    public string? Icon { get; init; }

    [JsonIgnore] public bool IsSubmenu => Submenu || Items.Count > 0;
    [JsonIgnore] public RoleWord? RoleWord => Words.Of<RoleWord>(Role);
    [JsonIgnore] public LookWord? LookWord => Words.Of<LookWord>(Look);

    /// <summary>This item and every item under it, depth first.</summary>
    public IEnumerable<MenuItem> Flatten()
    {
        yield return this;
        foreach (var i in Items)
            foreach (var j in i.Flatten())
                yield return j;
    }
}

/// <summary>menu.get: the menu bar (`gen` rises only when the menus change).</summary>
public sealed record MenuReply
{
    public ulong? Gen { get; init; }
    public required List<MenuItem> Menus { get; init; }
}

/// <summary>menu.get {context}: a context menu, and the kind of thing it is for (`node`, `wire`, ...).</summary>
public sealed record ContextMenuReply
{
    public required string Context { get; init; }
    public required List<MenuItem> Items { get; init; }
}

// ---------- asks (11) ----------

public enum AskWord
{
    FileOpen, FileSave, FolderPick, Prompt, Form, Text, ClipboardSet, ClipboardGet, Share, OpenExternal, Screenshot,
}

/// <summary>Something the core asks the shell (a file, a prompt, a form, the clipboard...), answered with
/// ask.answer {ask, answer} or {ask, cancel: true}. `params` are the kind's (11.2).</summary>
public sealed record Ask
{
    [JsonPropertyName("ask")] public required ulong Id { get; init; }
    public required string Kind { get; init; }
    public bool Modal { get; init; }
    /// <summary>Who started the chain it belongs to: `person` or `script`.</summary>
    public string? By { get; init; }
    public ulong? Doc { get; init; }
    public JsonElement Params { get; init; }

    [JsonIgnore] public AskWord? KindWord => Words.Of<AskWord>(Kind);
}

/// <summary>An ask that ended: `answered`, `cancelled` or `withdrawn`.</summary>
public sealed record AskEnded
{
    public required ulong Ask { get; init; }
    public required string Ended { get; init; }
    public ulong? Doc { get; init; }
}

// ---------- the watched value (ui.inspect's `watched`, 13.1 as built) ----------

public sealed record WatchedPicture
{
    /// <summary>The rgba8 rendition's reference: `handle` is what plum_blob and media.get take.</summary>
    public required JsonElement Ref { get; init; }
    public required uint W { get; init; }
    public required uint H { get; init; }

    [JsonIgnore]
    public string? Handle => Ref.ValueKind == JsonValueKind.Object && Ref.TryGetProperty("handle", out var h) && h.ValueKind == JsonValueKind.String ? h.GetString() : null;
}

public sealed record WatchedExcerpt
{
    public required string Text { get; init; }
    public ulong? Offset { get; init; }
    public ulong? TotalChars { get; init; }
    public ulong? Lines { get; init; }
}

/// <summary>What the Watched panel shows: the value of {node, port}, which viewer the core picked first
/// (`picture`, `text.plain`, `scalar`, ...), and the parts a native viewer reads. Value fields are null
/// until a run has sent the value.</summary>
public sealed record Watched
{
    public required string Node { get; init; }
    public required string Port { get; init; }
    public string? Summary { get; init; }
    public string? Media { get; init; }
    public string? Viewer { get; init; }
    /// <summary>A small value inline (a scalar's).</summary>
    public JsonElement? Value { get; init; }
    /// <summary>The full value's reference.</summary>
    public JsonElement? Data { get; init; }
    public WatchedPicture? Picture { get; init; }
    public WatchedExcerpt? Excerpt { get; init; }
    public JsonElement? Error { get; init; }
}

/// <summary>The part of ui.inspect the kit reads (the rest is skipped: decode it leniently).</summary>
public sealed record Inspect
{
    public Watched? Watched { get; init; }
    public RunSummary? Run { get; init; }
    public string? Undo { get; init; }
    public string? Redo { get; init; }
    public bool? Dirty { get; init; }
    public string? Title { get; init; }
    public InspectEngine? Engine { get; init; }

    public sealed record RunSummary
    {
        public bool? Running { get; init; }
    }

    public sealed record InspectEngine
    {
        public bool? Connected { get; init; }
        public string? Label { get; init; }
    }
}

// ---------- the palette (12.7) ----------

public sealed record PaletteItem
{
    public required string Kind { get; init; }
    public required string Label { get; init; }
    public required string Category { get; init; }
    /// <summary>With a dropped wire: the port a pick connects.</summary>
    public string? Port { get; init; }
    /// <summary>The engine choosing it starts (N4 slice 1).</summary>
    public string? Starts { get; init; }
}

/// <summary>palette.items: what the open palette offers, in category order.</summary>
public sealed record PaletteItems
{
    public required List<PaletteItem> Items { get; init; }
    public required ulong Total { get; init; }
    public string? Query { get; init; }
    public PaletteWire? Wire { get; init; }
}

// ---------- hits, wire targets, locate (9, as JSON) ----------

public enum HitWord { None, Node, Pin, Wire, Bend, Tag, Frame, Group }

/// <summary>geometry.hit's reply (and a gesture's `hit`, a drag's `target`): what is at a point.</summary>
public sealed record Hit
{
    public required string Kind { get; init; }
    public required Point At { get; init; }
    public ulong? Nid { get; init; }
    public string? Node { get; init; }
    public string? Part { get; init; }
    /// <summary>`in` or `out`, for a pin.</summary>
    public string? Side { get; init; }
    public int? Pin { get; init; }
    public string? Type { get; init; }
    public int? Fits { get; init; }
    public string? Wire { get; init; }
    public int? Index { get; init; }
    public string? Net { get; init; }
    public string? Frame { get; init; }
    public string? Group { get; init; }

    [JsonIgnore] public HitWord? KindWord => Words.Of<HitWord>(Kind);
}

/// <summary>geometry.locate's reply: where the core has something, at the geometry it has.</summary>
public sealed record Located
{
    public required Point At { get; init; }
    public Rect? Rect { get; init; }
    public ulong? Nid { get; init; }
    public bool? Measured { get; init; }
}

// ---------- gestures (6.3, 8.13) ----------

public enum GestureWord { Tap, DoubleTap, Context, LongPress, Drag, Marquee }
public enum PhaseWord { Begin, Move, End, Cancel }
public enum DragWord { Move, Wire, Marquee, Bend, Frame, Resize, Group, Scrub }

/// <summary>A recognised gesture, in graph units (ui.gesture's params). The shell recognises; the core decides.</summary>
public sealed record Gesture(GestureWord Kind, Point? At, PhaseWord? Phase = null, Point? To = null, IReadOnlyList<ModifierWord>? Mods = null, double? Slop = null, double? Zoom = null)
{
    public static Gesture Tap(Point at, IReadOnlyList<ModifierWord>? mods = null, double? slop = null) => new(GestureWord.Tap, at, Mods: mods, Slop: slop);
    public static Gesture DoubleTap(Point at, IReadOnlyList<ModifierWord>? mods = null, double? slop = null) => new(GestureWord.DoubleTap, at, Mods: mods, Slop: slop);
    public static Gesture ContextAt(Point at, double? slop = null) => new(GestureWord.Context, at, Slop: slop);
    public static Gesture MarqueeOf(Point from, Point to, IReadOnlyList<ModifierWord>? mods = null) => new(GestureWord.Marquee, from, To: to, Mods: mods);
    public static Gesture DragBegin(Point at, IReadOnlyList<ModifierWord>? mods = null, double? slop = null, double? zoom = null) =>
        new(GestureWord.Drag, at, PhaseWord.Begin, Mods: mods, Slop: slop, Zoom: zoom);
    /// <summary>A drag's move as JSON (a shell sends moves typed, CanvasLink.DragMove, never this).</summary>
    public static Gesture DragMove(Point at, double? slop = null) => new(GestureWord.Drag, at, PhaseWord.Move, Slop: slop);
    public static Gesture DragEnd(Point at, double? slop = null) => new(GestureWord.Drag, at, PhaseWord.End, Slop: slop);
    public static Gesture DragCancel => new(GestureWord.Drag, null, PhaseWord.Cancel);

    /// <summary>ui.gesture's params.</summary>
    public void Write(Utf8JsonWriter w)
    {
        w.WriteStartObject();
        w.WriteString("kind", Words.Of(Kind));
        if (Phase is { } p) w.WriteString("phase", Words.Of(p));
        if (At is { } a) WritePoint(w, "at", a);
        if (To is { } t) WritePoint(w, "to", t);
        if (Mods is { Count: > 0 } m)
        {
            w.WriteStartArray("mods");
            foreach (var x in m) w.WriteStringValue(Words.Of(x));
            w.WriteEndArray();
        }
        if (Slop is { } s) w.WriteNumber("slop", s);
        if (Zoom is { } z) w.WriteNumber("zoom", z);
        w.WriteEndObject();
    }

    public JsonElement ToJson() => Jsonv.Build(Write);

    static void WritePoint(Utf8JsonWriter w, string name, Point p)
    {
        w.WriteStartArray(name);
        w.WriteNumberValue(p.X);
        w.WriteNumberValue(p.Y);
        w.WriteEndArray();
    }
}

/// <summary>A command a gesture ran, and how it went.</summary>
public sealed record CommandRun
{
    public required string Command { get; init; }
    public JsonElement Params { get; init; }
    public required bool Ok { get; init; }
    public JsonElement? Result { get; init; }
    public string? Error { get; init; }
}

/// <summary>A drag as its `begin` decided it.</summary>
public sealed record DragBegun
{
    public required ulong Serial { get; init; }
    public required string Kind { get; init; }
    /// <summary>A move: the faces moving (nids), for plum_wire_paths.</summary>
    public List<ulong>? Faces { get; init; }
    /// <summary>A new wire's anchor.</summary>
    public JsonElement? From { get; init; }
    /// <summary>A wired input's wire picked up.</summary>
    public PortRef? Picked { get; init; }
    /// <summary>A scrub's row ({node, port, face, index}) and its value when it began.</summary>
    public JsonElement? Row { get; init; }
    public double? Value { get; init; }

    [JsonIgnore] public DragWord? KindWord => Words.Of<DragWord>(Kind);
}

/// <summary>Where a drag is (ui.gesture's move, or the typed plum_gesture_move).</summary>
public sealed record DragUpdate
{
    public required ulong Serial { get; init; }
    public required double Dx { get; init; }
    public required double Dy { get; init; }
    public Rect? Rect { get; init; }
    public List<string>? Touches { get; init; }
    public Hit? Target { get; init; }
    /// <summary>A scrub's value now.</summary>
    public double? Value { get; init; }
}

/// <summary>ui.gesture's reply.</summary>
public sealed record GestureReply
{
    public required List<CommandRun> Commands { get; init; }
    public Hit? Hit { get; init; }
    public DragBegun? Drag { get; init; }
    public DragUpdate? Update { get; init; }
    /// <summary>What menu.get takes for the thing under a context gesture.</summary>
    public JsonElement? Context { get; init; }
    public JsonElement? Menu { get; init; }
    public bool? Cancelled { get; init; }
}

// ---------- geometry (8.11) ----------

/// <summary>One face's measurements in the JSON form of geometry.set (what the typed call lacks:
/// `offset`, header `parts`, by face id).</summary>
public sealed record FaceReport(string Node, Point Size, ulong? Rev = null, double? Header = null,
    IReadOnlyList<double?>? Inputs = null, IReadOnlyList<double?>? Outputs = null, IReadOnlyList<Rect?>? Rows = null,
    IReadOnlyDictionary<string, Rect>? Parts = null, IReadOnlyList<Rect>? Footer = null, Point? Offset = null)
{
    public void Write(Utf8JsonWriter w)
    {
        w.WriteStartObject();
        w.WriteString("node", Node);
        w.WriteStartArray("size");
        w.WriteNumberValue(Size.X);
        w.WriteNumberValue(Size.Y);
        w.WriteEndArray();
        if (Rev is { } r) w.WriteNumber("rev", r);
        if (Header is { } h) w.WriteNumber("header", h);
        w.WriteStartObject("pins");
        Pins(w, "in", Inputs);
        Pins(w, "out", Outputs);
        w.WriteEndObject();
        if (Rows is { Count: > 0 } rows)
        {
            w.WriteStartArray("rows");
            foreach (var x in rows)
                if (x is { } rect) RectValue(w, rect); else w.WriteNullValue();
            w.WriteEndArray();
        }
        if (Parts is { Count: > 0 } || Footer is { Count: > 0 })
        {
            w.WriteStartObject("parts");
            foreach (var (k, v) in Parts ?? new Dictionary<string, Rect>())
            {
                w.WritePropertyName(k);
                RectValue(w, v);
            }
            if (Footer is { Count: > 0 } f)
            {
                w.WriteStartArray("footer");
                foreach (var x in f) RectValue(w, x);
                w.WriteEndArray();
            }
            w.WriteEndObject();
        }
        if (Offset is { } o)
        {
            w.WriteStartArray("offset");
            w.WriteNumberValue(o.X);
            w.WriteNumberValue(o.Y);
            w.WriteEndArray();
        }
        w.WriteEndObject();
    }

    public JsonElement ToJson() => Jsonv.Build(Write);

    static void Pins(Utf8JsonWriter w, string name, IReadOnlyList<double?>? pins)
    {
        w.WriteStartArray(name);
        foreach (var p in pins ?? [])
            if (p is { } y) w.WriteNumberValue(y); else w.WriteNullValue();
        w.WriteEndArray();
    }

    internal static void RectValue(Utf8JsonWriter w, Rect r)
    {
        w.WriteStartArray();
        w.WriteNumberValue(r.X0);
        w.WriteNumberValue(r.Y0);
        w.WriteNumberValue(r.X1);
        w.WriteNumberValue(r.Y1);
        w.WriteEndArray();
    }
}

/// <summary>geometry.set's reply.</summary>
public sealed record GeometrySetReply
{
    public required ulong Applied { get; init; }
    public required List<string> Stale { get; init; }
    public List<string>? Unknown { get; init; }
}

// ---------- the feed (7.1, as built on the C ABI) ----------

public enum DomainWord { Engine, Document, Menus, Scene, Prefs, Window }

/// <summary>A domain of a document that changed. Engine changes carry `connected`, `label`, `ready`; a
/// closing document `closing`; the document's own state (N3 slice 4) `title`, `document`, `path`,
/// `dirty`, `undo`, `redo`, `lastError` and (N4 slice 4) `autosave`.</summary>
public sealed record Changed
{
    public required ulong Doc { get; init; }
    public required string Domain { get; init; }
    public ulong? Gen { get; init; }
    public bool? Connected { get; init; }
    public string? Label { get; init; }
    public bool? Ready { get; init; }
    public bool? Closing { get; init; }
    public string? Title { get; init; }
    public string? Document { get; init; }
    public string? Path { get; init; }
    public bool? Dirty { get; init; }
    public string? Undo { get; init; }
    public string? Redo { get; init; }
    public string? LastError { get; init; }
    public AutosaveState? Autosave { get; init; }
    /// <summary>A `window` change (N5 slice 3, `platform.document_windows`): make a window for `doc`, which
    /// New or Open in document `from` made, for the person or a script (`by`), on the file `path` or untitled.</summary>
    public bool? Open { get; init; }
    public ulong? From { get; init; }
    public string? By { get; init; }

    [JsonIgnore] public DomainWord? DomainWord => Words.Of<DomainWord>(Domain);
}

/// <summary>Where the core's autosave is (N4 slice 4; contract 4.4): `state` is `off`, `clean`,
/// `pending`, `untitled`, `saving`, `saved` or `failed` (then `error` says why).</summary>
public sealed record AutosaveState
{
    public required bool On { get; init; }
    public required string State { get; init; }
    public string? Error { get; init; }
    public ulong? Saves { get; init; }
    public ulong? Failures { get; init; }

    public static AutosaveState Off => new() { On = false, State = "off" };
}

/// <summary>The document as the feed last told it (a `document` change): what a window's title, edited
/// dot and Edit menu show.</summary>
public sealed record DocumentState
{
    public ulong Gen { get; init; }
    public string Title { get; init; } = "";
    public string Name { get; init; } = "";
    public string? Path { get; init; }
    public bool Dirty { get; init; }
    /// <summary>The step Undo would undo ("Add Node"), null when there is none.</summary>
    public string? Undo { get; init; }
    public string? Redo { get; init; }
    public string? LastError { get; init; }
    public AutosaveState Autosave { get; init; } = AutosaveState.Off;

    /// <summary>From a `document` change carrying the state (one with a `gen`); null for any other.</summary>
    public static DocumentState? From(Changed c)
    {
        if (c.DomainWord != Models.DomainWord.Document || c.Gen is not { } g) return null;
        return new DocumentState
        {
            Gen = g, Title = c.Title ?? "", Name = c.Document ?? "", Path = c.Path, Dirty = c.Dirty ?? false,
            Undo = c.Undo, Redo = c.Redo, LastError = c.LastError, Autosave = c.Autosave ?? AutosaveState.Off,
        };
    }
}

public enum MediaWord { Ready, Pending, Missing, Failed }

/// <summary>A value asked for (media.get, a pending plum_blob) that arrived, or why not.</summary>
public sealed record MediaNote
{
    public required string Key { get; init; }
    public required string Handle { get; init; }
    public required string Want { get; init; }
    public required string Status { get; init; }
    public uint? W { get; init; }
    public uint? H { get; init; }
    public ulong? Len { get; init; }
    public string? Format { get; init; }
    public string? Reason { get; init; }
    public string? Dtype { get; init; }
    public List<ulong>? Shape { get; init; }
    public uint? Channels { get; init; }
    public ulong? Bins { get; init; }

    [JsonIgnore] public MediaWord? StatusWord => Words.Of<MediaWord>(Status);
}

/// <summary>The log has lines past what the shell last read; the worst of them.</summary>
public sealed record LogMark
{
    public required ulong Seq { get; init; }
    public required string Worst { get; init; }
}

/// <summary>One reply to one request.</summary>
public sealed record FeedReply
{
    public required ulong Id { get; init; }
    public required bool Ok { get; init; }
    public JsonElement? Result { get; init; }
    public string? Error { get; init; }
    public string? Code { get; init; }
    public JsonElement? Tag { get; init; }
}

/// <summary>Everything one plum_poll gave, but its replies (the requests'). Lenient: an item that won't
/// decode is skipped by itself, never the whole poll (an unknown shape from a newer core loses only itself).</summary>
public sealed record FeedBatch
{
    public List<JsonElement> Events { get; init; } = [];
    public List<Ask> Asks { get; init; } = [];
    public List<AskEnded> AsksEnded { get; init; } = [];
    public List<MediaNote> Media { get; init; } = [];
    public List<Changed> Changed { get; init; } = [];
    public LogMark? Log { get; init; }
    public ulong Dropped { get; init; }

    /// <summary>Nothing for subscribers.</summary>
    public bool IsQuiet => Events.Count == 0 && Asks.Count == 0 && AsksEnded.Count == 0 && Media.Count == 0 && Changed.Count == 0 && Log is null && Dropped == 0;

    public static FeedBatch Parse(JsonElement poll)
    {
        if (poll.ValueKind != JsonValueKind.Object) return new FeedBatch();
        var o = Shell.Options;
        List<T> Items<T>(string name)
        {
            var outp = new List<T>();
            if (!poll.TryGetProperty(name, out var a) || a.ValueKind != JsonValueKind.Array) return outp;
            foreach (var x in a.EnumerateArray())
            {
                if (Shell.TryDecode<T>(x, o, out var v)) outp.Add(v);
            }
            return outp;
        }
        var events = new List<JsonElement>();
        if (poll.TryGetProperty("events", out var ev) && ev.ValueKind == JsonValueKind.Array)
            foreach (var e in ev.EnumerateArray()) events.Add(e.Clone());
        LogMark? log = null;
        if (poll.TryGetProperty("log", out var l)) Shell.TryDecode(l, o, out log);
        ulong dropped = poll.TryGetProperty("dropped", out var d) && d.ValueKind == JsonValueKind.Number && d.TryGetUInt64(out var n) ? n : 0;
        return new FeedBatch
        {
            Events = events, Asks = Items<Ask>("asks"), AsksEnded = Items<AskEnded>("asksEnded"), Media = Items<MediaNote>("media"),
            Changed = Items<Changed>("changed"), Log = log, Dropped = dropped,
        };
    }
}

// ---------- windows (N4 slice 3) ----------

/// <summary>popout.get {id}: a pop-out's value (as the Watched panel's was) and its viewer's state.</summary>
public sealed record PopoutValue
{
    public required ulong Id { get; init; }
    public required string Node { get; init; }
    public required string Port { get; init; }
    public string? Title { get; init; }
    public string? Summary { get; init; }
    public string? Media { get; init; }
    public string? Viewer { get; init; }
    public JsonElement? Value { get; init; }
    public JsonElement? Data { get; init; }
    public WatchedPicture? Picture { get; init; }
    public WatchedExcerpt? Excerpt { get; init; }
    public JsonElement? Error { get; init; }
    public TextView? Text { get; init; }
    /// <summary>The whole-view zoom asked for: "fit", "1:1", or null (fit).</summary>
    public string? Fit { get; init; }
    /// <summary>The viewer's state, the core's ({table, tree, audio, picture, grid, hex}).</summary>
    public JsonElement? State { get; init; }
    public List<OfferedViewer>? Viewers { get; init; }
    public string? Type { get; init; }
    public ulong? Items { get; init; }
    public long? Index { get; init; }
    public JsonElement? Form { get; init; }
    public ulong? Bytes { get; init; }
    public string? MediaType { get; init; }
    public string? Handle { get; init; }
    public JsonElement? Stats { get; init; }
    public List<ViewPicture>? Views { get; init; }
    public JsonElement? Peaks { get; init; }
    public JsonElement? Sheet { get; init; }

    public sealed record OfferedViewer
    {
        public required string Id { get; init; }
        public required string Label { get; init; }
        /// <summary>The native shells draw it (else the engine's picture stands in, contract 13.3).</summary>
        public bool? Native { get; init; }
    }

    public sealed record ViewPicture
    {
        public required JsonElement Ref { get; init; }
        public string? Caption { get; init; }
        public required uint W { get; init; }
        public required uint H { get; init; }
    }

    public sealed record TextView
    {
        public required bool Wrap { get; init; }
        public required bool Mono { get; init; }
        public required bool All { get; init; }
    }
}

/// <summary>parameters.get {node}: every input of a node, in the inspector's sections.</summary>
public sealed record ParametersReply
{
    public required string Node { get; init; }
    public required string Title { get; init; }
    public required string Kind { get; init; }
    public required string Display { get; init; }
    public string? Description { get; init; }
    public string? About { get; init; }
    public string? Family { get; init; }
    public required bool Open { get; init; }
    public required List<Section> Sections { get; init; }
    public required int Count { get; init; }
    public required List<string> Hidden { get; init; }

    public sealed record Section
    {
        public required string Title { get; init; }
        public required List<Row> Rows { get; init; }
    }

    public sealed record Row
    {
        /// <summary>The input as a face's row shows it (`row` on the wire).</summary>
        [JsonPropertyName("row")] public required InRow Input { get; init; }
        public required bool OnFace { get; init; }
        public required bool CanHide { get; init; }
        public required bool Editable { get; init; }
        public JsonElement? Default { get; init; }
        public required bool Reset { get; init; }
        public required string Help { get; init; }
    }
}
