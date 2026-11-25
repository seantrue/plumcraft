// The scene (docs/shell-interface.md section 8, as built; crates/editor-core/src/scene.rs is the wire
// shape and crates/editor-core/fixtures/scene/*.json the reference), as the Swift kit's Scene.swift
// has it. Property names are the JSON's in Pascal case (the source generator's camelCase policy maps
// them back). Every item has `id`, `nid` (unique within the document, for hot calls) and `rev` (the
// scene generation it last changed at).
//
// What is required: a field the Swift types decode without a default is `required` here (and a
// non-nullable reference may not be null: the options respect nullable annotations), so a reply
// missing one fails to decode as it does in Swift. A field Swift defaults is defaulted here.

using System.Text.Json;
using System.Text.Json.Serialization;

namespace PlumCraft.Models;

/// <summary>An item of the scene: a face, wire, net, frame, group or mark.</summary>
public interface ISceneItem
{
    string Id { get; }
    ulong Nid { get; }
    ulong Rev { get; }
}

// ---------- words ----------

/// <summary>A node's real run state (8.6's first layer).</summary>
public enum RunWord { Queued, Started, Progress, Preview, Done, Cached, Error, Skipped, Cancelled }

/// <summary>What the run animation shows (8.6's second layer).</summary>
public enum ShownWord { Waiting, Running, Done, Cached, Failed, Skipped }

public enum FaceViewWord { Full, Compact, Collapsed }
public enum SpotWord { Spinner, Dot, None }
public enum PinShapeWord { Circle, Square }
public enum RowSourceWord { Wired, Socket, Inherited, Own, Readonly }
public enum WidgetWord { Number, Toggle, Choice, Multichoice, Text, Multiline, Path, Color, Editor }
public enum WireWord { Plain, Routed, Net }
public enum ToneWord { Info, Warn, Error }
public enum PreviewWord { Rgba8, Peaks }
public enum FooterWord { Fields, Convert, Editor, Progress, Error, Save, Excerpt, EngineText, AppText }

// ---------- the whole scene and its deltas ----------

/// <summary>scene.get's reply without `since` (or with one too old): every item.</summary>
public sealed record Scene
{
    public required uint Shell { get; init; }
    public required ulong Doc { get; init; }
    public required ulong Gen { get; init; }
    public required bool Full { get; init; }
    public required List<Face> Nodes { get; init; }
    public required List<Wire> Wires { get; init; }
    public required List<Net> Nets { get; init; }
    public required List<Frame> Frames { get; init; }
    public required List<Group> Groups { get; init; }
    public required List<Mark> Marks { get; init; }
    public required Picked Selection { get; init; }
    public Focus? Focus { get; init; }
    public required Anim Anim { get; init; }
    readonly View? view;
    /// <summary>Absent from an older core: an empty view.</summary>
    public View View { get => view ?? new View(); init => view = value; }
}

/// <summary>scene.get {since}'s reply: the items whose `rev` is past `since`, and the ids gone since.
/// `selection`, `focus`, `anim` and `view` come every time.</summary>
public sealed record Delta
{
    public required uint Shell { get; init; }
    public required ulong Doc { get; init; }
    public required ulong Gen { get; init; }
    public required bool Full { get; init; }
    public required ulong Since { get; init; }
    public required Upsert Upsert { get; init; }
    public required Removed Remove { get; init; }
    public required Picked Selection { get; init; }
    public Focus? Focus { get; init; }
    public required Anim Anim { get; init; }
    readonly View? view;
    /// <summary>Absent from an older core: an empty view.</summary>
    public View View { get => view ?? new View(); init => view = value; }
}

public sealed record Upsert
{
    public required List<Face> Nodes { get; init; }
    public required List<Wire> Wires { get; init; }
    public required List<Net> Nets { get; init; }
    public required List<Frame> Frames { get; init; }
    public required List<Group> Groups { get; init; }
    public required List<Mark> Marks { get; init; }
}

public sealed record Removed
{
    public required List<string> Nodes { get; init; }
    public required List<string> Wires { get; init; }
    public required List<string> Nets { get; init; }
    public required List<string> Frames { get; init; }
    public required List<string> Groups { get; init; }
    public required List<string> Marks { get; init; }
}

/// <summary>What scene.get gave: everything, or what changed. `full: true` is a whole scene, whatever
/// was asked (a since too old, or newer than gen).</summary>
[JsonConverter(typeof(SceneReplyConverter))]
public sealed record SceneReply
{
    public Scene? Scene { get; init; }
    public Delta? Delta { get; init; }
    public bool IsFull => Scene is not null;
    public ulong Gen => Scene?.Gen ?? Delta?.Gen ?? 0;
}

public sealed class SceneReplyConverter : JsonConverter<SceneReply>
{
    public override SceneReply Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o)
    {
        using var doc = JsonDocument.ParseValue(ref r);
        var root = doc.RootElement;
        if (root.ValueKind != JsonValueKind.Object || !root.TryGetProperty("full", out var full)
            || (full.ValueKind != JsonValueKind.True && full.ValueKind != JsonValueKind.False))
            throw new JsonException("a scene reply says whether it is full");
        return full.ValueKind == JsonValueKind.True
            ? new SceneReply { Scene = root.Deserialize((System.Text.Json.Serialization.Metadata.JsonTypeInfo<Scene>)o.GetTypeInfo(typeof(Scene))) }
            : new SceneReply { Delta = root.Deserialize((System.Text.Json.Serialization.Metadata.JsonTypeInfo<Delta>)o.GetTypeInfo(typeof(Delta))) };
    }

    public override void Write(Utf8JsonWriter w, SceneReply v, JsonSerializerOptions o)
    {
        if (v.Scene is { } s) JsonSerializer.Serialize(w, s, o.GetTypeInfo(typeof(Scene)));
        else if (v.Delta is { } d) JsonSerializer.Serialize(w, d, o.GetTypeInfo(typeof(Delta)));
        else w.WriteNullValue();
    }
}

/// <summary>What the core asks the view to show: the latest fit, the palette while open, and the rest.</summary>
public sealed record View
{
    public Fit? Fit { get; init; }
    public Palette? Palette { get; init; }
    /// <summary>The row a field editor is open on (row.edit, N4 slice 2).</summary>
    public RowEdit? Edit { get; init; }
    /// <summary>Select mode is on (canvas.selectMode, N4 slice 3). Absent while off.</summary>
    public bool? Select { get; init; }
    /// <summary>The document's windows (N4 slice 3).</summary>
    public DocumentWindows? Windows { get; init; }
    /// <summary>A node to reveal (canvas.reveal): centred once per `seq`.</summary>
    public Reveal? Reveal { get; init; }
    /// <summary>A name edited in place (name.edit, N4 slice 3).</summary>
    public NameEdit? Name { get; init; }
    /// <summary>Zoom the view by a factor or to a zoom, about its centre, once per `seq` (canvas.zoom, N5 slice 3).</summary>
    public ZoomAsk? Zoom { get; init; }

    [JsonIgnore] public bool SelectMode => Select ?? false;
}

/// <summary>The scene's view.zoom: `by` a factor (Zoom In, Zoom Out) or `to` a zoom (Actual Size).</summary>
public sealed record ZoomAsk
{
    public required ulong Seq { get; init; }
    public double? By { get; init; }
    public double? To { get; init; }
}

public sealed record Reveal
{
    public required ulong Seq { get; init; }
    public required string Node { get; init; }
    public required Rect Rect { get; init; }
}

/// <summary>The document's windows as the core keeps them (contract 12.5, N4 slices 3 to 6): the scene's
/// `view.windows` (Swift's `Windows`, renamed so it never meets the Windows namespace).</summary>
public sealed record DocumentWindows
{
    public string? Parameters { get; init; }
    public List<PopoutWindow>? Popouts { get; init; }
    public string? Settings { get; init; }
    public bool? Messages { get; init; }
    public bool? Cache { get; init; }
    public bool? Params { get; init; }
    public List<string>? Editors { get; init; }
    public bool? Compare { get; init; }
    /// <summary>Changes when what an open report reads changes: read it again then.</summary>
    public ulong? Reports { get; init; }
    /// <summary>The Acknowledgements window is open (FR4): `acknowledgements.get`.</summary>
    public bool? Acknowledgements { get; init; }
}

/// <summary>One pop-out window: what it follows, its title, when to bring it forward (`raised` rises)
/// and when to read its value again (`stamp` changes).</summary>
public sealed record PopoutWindow
{
    public required ulong Id { get; init; }
    public required string Node { get; init; }
    public required string Port { get; init; }
    public required long Index { get; init; }
    public required string Title { get; init; }
    public string? Viewer { get; init; }
    public required ulong Raised { get; init; }
    public required ulong Stamp { get; init; }
}

/// <summary>A name edited in place: what (the core's target, passed back unread), its text, where the field goes.</summary>
public sealed record NameEdit
{
    public required JsonElement Target { get; init; }
    public required string Text { get; init; }
    public string? Hint { get; init; }
    public required string Label { get; init; }
    public required Rect Rect { get; init; }
}

/// <summary>The row a field editor is open on.</summary>
public sealed record RowEdit
{
    public required string Node { get; init; }
    public required string Port { get; init; }
    public required string Face { get; init; }
    public required int Row { get; init; }
}

/// <summary>Fit `rect` into the view (zoom 0.2 to 1), once per `seq`.</summary>
public sealed record Fit
{
    public required ulong Seq { get; init; }
    public required Rect Rect { get; init; }
}

/// <summary>The node palette while open: where a pick goes, the query, a dropped wire.</summary>
public sealed record Palette
{
    public Point? At { get; init; }
    public string? Query { get; init; }
    public PaletteWire? Wire { get; init; }
}

/// <summary>The end of a dropped wire that stays: the output it leaves, or the input it enters.</summary>
public sealed record PaletteWire
{
    public PortRef? From { get; init; }
    public PortRef? To { get; init; }
}

/// <summary>The selection.</summary>
public sealed record Picked
{
    public required List<string> Nodes { get; init; }
    public required List<string> Groups { get; init; }
    public NetKey? Net { get; init; }

    public static Picked Empty => new() { Nodes = [], Groups = [] };

    public bool SameAs(Picked? o) => o is not null && Nodes.SequenceEqual(o.Nodes) && Groups.SequenceEqual(o.Groups) && Equals(Net, o.Net);
}

/// <summary>The group entered and the breadcrumb to it.</summary>
public sealed record Focus
{
    public required string Group { get; init; }
    public required List<Crumb> Path { get; init; }
}

public sealed record Crumb
{
    public required string Id { get; init; }
    public required string Name { get; init; }
}

/// <summary>The run animation's switches (8.6): the shell computes the wash per frame from these.</summary>
public sealed record Anim
{
    public required bool On { get; init; }
    public required bool Replay { get; init; }
    public required ulong Generation { get; init; }
    [JsonPropertyName("hold_ms")] public required ulong HoldMs { get; init; }
    [JsonPropertyName("fade_ms")] public required ulong FadeMs { get; init; }
    [JsonPropertyName("pulse_ms")] public required ulong PulseMs { get; init; }
}

// ---------- faces ----------

/// <summary>A node's face, or a collapsed group's (`proxy`).</summary>
public sealed record Face : ISceneItem
{
    public required string Id { get; init; }
    public required ulong Nid { get; init; }
    public required ulong Rev { get; init; }
    /// <summary>The node type ("" for a proxy).</summary>
    public required string Kind { get; init; }
    public ProxyOf? Proxy { get; init; }
    public required Point Pos { get; init; }
    public required string View { get; init; }
    public required bool Watched { get; init; }
    public required Tint Tint { get; init; }
    public Border? Border { get; init; }
    public required Header Header { get; init; }
    public required List<InRow> Inputs { get; init; }
    public required List<OutRow> Outputs { get; init; }
    public Preview? Preview { get; init; }
    public required List<Footer> Footer { get; init; }
    public required RunShown Run { get; init; }
    public string? Shown { get; init; }
    /// <summary>The context menu kind for menu.get (`node` or `proxy`).</summary>
    public required string Menu { get; init; }

    [JsonIgnore] public FaceViewWord? ViewWord => Words.Of<FaceViewWord>(View);
    [JsonIgnore] public ShownWord? ShownWord => Words.Of<ShownWord>(Shown);
}

public sealed record ProxyOf
{
    public required string Group { get; init; }
    public required string Name { get; init; }
}

public sealed record Tint
{
    public required Paint Header { get; init; }
    public required Paint Body { get; init; }
}

public sealed record Border
{
    public required Paint Paint { get; init; }
    public required double Width { get; init; }
}

/// <summary>The node's real run state.</summary>
public sealed record RunShown
{
    public string? State { get; init; }
    public double? Fraction { get; init; }
    public required bool Stale { get; init; }
    public required bool NotRun { get; init; }

    [JsonIgnore] public RunWord? StateWord => Words.Of<RunWord>(State);
}

public sealed record Header
{
    public required bool Open { get; init; }
    /// <summary>A Lucide icon name.</summary>
    public required string Icon { get; init; }
    public required About About { get; init; }
    public required Spot State { get; init; }
    public required Title Title { get; init; }
    public required List<Badge> Badges { get; init; }
    public EngineMark? Engine { get; init; }
    public required List<Status> Status { get; init; }
    public required bool Enter { get; init; }
}

public sealed record About
{
    public required string Name { get; init; }
    public required string Description { get; init; }
    public string? Family { get; init; }
    public string? Path { get; init; }
    public required string Id { get; init; }
    public required bool Known { get; init; }
}

public sealed record Spot
{
    public required string Kind { get; init; }
    public Paint? Paint { get; init; }
    public string? Hover { get; init; }

    [JsonIgnore] public SpotWord? KindWord => Words.Of<SpotWord>(Kind);
}

public sealed record Title
{
    public required string Text { get; init; }
    public required bool Watch { get; init; }
}

public sealed record Badge
{
    public required string Text { get; init; }
    public required Paint Paint { get; init; }
    public required string Hover { get; init; }
}

public sealed record EngineMark
{
    public required string Text { get; init; }
    public required bool Pinned { get; init; }
    public required string Hover { get; init; }
}

public sealed record Status
{
    public required string Text { get; init; }
    public required Paint Paint { get; init; }
    public required bool Strong { get; init; }
    public string? Hover { get; init; }
}

public sealed record Pin
{
    public required string Shape { get; init; }
    public required Paint Fill { get; init; }
    public required Paint Wire { get; init; }
    public required bool Wireable { get; init; }

    [JsonIgnore] public PinShapeWord? ShapeWord => Words.Of<PinShapeWord>(Shape);
}

/// <summary>An input row (8.4).</summary>
public sealed record InRow
{
    public required string Key { get; init; }
    public required string Node { get; init; }
    public required string Label { get; init; }
    public required bool List { get; init; }
    public required string Ty { get; init; }
    public required Pin Pin { get; init; }
    public required string Source { get; init; }
    public string? Text { get; init; }
    public string? Hover { get; init; }
    public required bool Dim { get; init; }
    public string? From { get; init; }
    public Named? Named { get; init; }
    public Badge? Badge { get; init; }
    public Widget? Widget { get; init; }
    public string? Inactive { get; init; }
    /// <summary>The value its node names an editor for, not on a wire (FR1, 8.4): a tap on the row opens it.</summary>
    public EditorLink? Editor { get; init; }
    /// <summary>FR5: on a collapsed group's face, a header drawn above this row: the title of the node the promoted
    /// rows from here on come from. Absent for every other row.</summary>
    public string? Section { get; init; }

    [JsonIgnore] public RowSourceWord? SourceWord => Words.Of<RowSourceWord>(Source);
}

/// <summary>What a row whose value has an editor says of it (8.4, the schema's `RowEditor`): the editor's kind,
/// and the words of its link.</summary>
public sealed record EditorLink
{
    public required string Kind { get; init; }
    public required string Label { get; init; }
    public required string Hover { get; init; }
}

public sealed record Named
{
    public required string Name { get; init; }
    public required Paint Paint { get; init; }
    public required string Hover { get; init; }
}

/// <summary>An output row.</summary>
public sealed record OutRow
{
    public required string Key { get; init; }
    public required string Node { get; init; }
    public required string Label { get; init; }
    public required bool List { get; init; }
    public required string Ty { get; init; }
    public required Pin Pin { get; init; }
    public required string Hover { get; init; }
}

/// <summary>A widget (8.5): what it shows and what bounds it. Values a shell doesn't understand are
/// passed back unchanged, so `value` and `default` are any JSON; a null one is a value (blank), not a
/// missing one (Undefined).</summary>
public sealed record Widget
{
    public required string Kind { get; init; }
    public JsonElement Value { get; init; }
    public JsonElement Default { get; init; }
    public required bool Blank { get; init; }
    public required bool Clearable { get; init; }
    public double? Min { get; init; }
    public double? Max { get; init; }
    public double? SoftMin { get; init; }
    public double? SoftMax { get; init; }
    public double? Step { get; init; }
    public int? Decimals { get; init; }
    /// <summary>A number as every shell shows it (the core's, N4 slice 4).</summary>
    public string? Text { get; init; }
    public required bool Int { get; init; }
    public required List<Choice> Choices { get; init; }
    public string? PathMode { get; init; }
    public string? ColorFormat { get; init; }
    public string? Hover { get; init; }

    [JsonIgnore] public WidgetWord? KindWord => Words.Of<WidgetWord>(Kind);
}

public sealed record Choice
{
    public JsonElement Value { get; init; }
    public required string Label { get; init; }
    public string? Disabled { get; init; }
}

/// <summary>A preview (only in view `full`): a picture by its value reference (`rgba8`), or a waveform
/// (`peaks`). One record for every kind, its fields those of its kind; a kind this kit doesn't know
/// decodes and is drawn as nothing. `handle` is the value's reference as JSON, what media.get takes.</summary>
public sealed record Preview
{
    public required string Kind { get; init; }
    public JsonElement? Handle { get; init; }
    public uint? W { get; init; }
    public uint? H { get; init; }
    public Point? Size { get; init; }
    public Point? HoverSize { get; init; }
    public ulong? Items { get; init; }
    /// <summary>A list of lists' count at each level, outermost first ([3, 2]: 3 runs of 2; interface 7.6 rule 2);
    /// absent for a list that isn't nested.</summary>
    public List<ulong>? Levels { get; init; }
    /// <summary>The picture's own name (8.7): with `items` above 1, the first of a list of named pictures.</summary>
    public string? Caption { get; init; }
    public string? Summary { get; init; }
    public string? Note { get; init; }
    public bool? Faded { get; init; }
    public ushort? Channels { get; init; }

    [JsonIgnore] public PreviewWord? KindWord => Words.Of<PreviewWord>(Kind);
}

/// <summary>A footer item, in the order drawn (8.7). One record for every kind, its fields those of its
/// kind; a kind this kit doesn't know decodes and is skipped.</summary>
public sealed record Footer
{
    public required string Kind { get; init; }
    public string? Text { get; init; }
    public string? Hover { get; init; }
    public bool? All { get; init; }
    public List<Line>? Lines { get; init; }
    public string? Label { get; init; }
    public double? Fraction { get; init; }
    public string? Stage { get; init; }
    public string? Full { get; init; }
    public Paint? Paint { get; init; }
    public string? Port { get; init; }
    public bool? Sheet { get; init; }
    public bool? Faded { get; init; }

    [JsonIgnore] public FooterWord? KindWord => Words.Of<FooterWord>(Kind);
}

public sealed record Line
{
    public required string Text { get; init; }
    public required Paint Paint { get; init; }
    public string? Hover { get; init; }
}

// ---------- wires, nets, frames, groups, marks ----------

/// <summary>A wire, keyed by the input it feeds. `path` is a cubic chain: the start, then two control
/// points and the end a segment; empty for a wire drawn through a net.</summary>
public sealed record Wire : ISceneItem
{
    public required string Id { get; init; }
    public required ulong Nid { get; init; }
    public required ulong Rev { get; init; }
    public required PortRef From { get; init; }
    public required PortRef To { get; init; }
    public required string FromFace { get; init; }
    public required int FromPin { get; init; }
    public required string ToFace { get; init; }
    public required int ToPin { get; init; }
    public required Paint Paint { get; init; }
    public required string Kind { get; init; }
    public required List<Point> Via { get; init; }
    public string? Slice { get; init; }
    public NetKey? Net { get; init; }
    public required List<Point> Path { get; init; }

    [JsonIgnore] public WireWord? KindWord => Words.Of<WireWord>(Kind);
}

public sealed record Net : ISceneItem
{
    public required string Id { get; init; }
    public required ulong Nid { get; init; }
    public required ulong Rev { get; init; }
    public required NetKey Key { get; init; }
    public required Paint Paint { get; init; }
    public required bool Selected { get; init; }
    public required NetDriver Driver { get; init; }
    public required List<NetUse> Uses { get; init; }
}

public sealed record NetDriver
{
    /// <summary>The driver's tag: the net's name and how many inputs it feeds ("picture ×3").</summary>
    public string? Text { get; init; }
    public required PortRef From { get; init; }
    public required string Face { get; init; }
    public required int Pin { get; init; }
    /// <summary>The tag's offset from its pin.</summary>
    public required Point At { get; init; }
}

public sealed record NetUse
{
    public required PortRef To { get; init; }
    public required string Face { get; init; }
    public required int Pin { get; init; }
    public required Point At { get; init; }
}

public sealed record Frame : ISceneItem
{
    public required string Id { get; init; }
    public required ulong Nid { get; init; }
    public required ulong Rev { get; init; }
    public required string Title { get; init; }
    public required Point Pos { get; init; }
    public required Point Size { get; init; }
    public required Paint Paint { get; init; }
}

/// <summary>A group: a collapsed one is drawn as its proxy face, an open one as `rect` round its members.</summary>
public sealed record Group : ISceneItem
{
    public required string Id { get; init; }
    public required ulong Nid { get; init; }
    public required ulong Rev { get; init; }
    public required string Name { get; init; }
    public required bool Collapsed { get; init; }
    public string? Parent { get; init; }
    public required int Depth { get; init; }
    public required bool Proxy { get; init; }
    public Rect? Rect { get; init; }
}

/// <summary>An application's mark on a wire.</summary>
public sealed record Mark : ISceneItem
{
    public required string Id { get; init; }
    public required ulong Nid { get; init; }
    public required ulong Rev { get; init; }
    public required string Label { get; init; }
    public required string Tone { get; init; }
    public required Paint Paint { get; init; }
    public string? Tooltip { get; init; }
    public Point? At { get; init; }

    [JsonIgnore] public ToneWord? ToneWord => Words.Of<ToneWord>(Tone);
}
