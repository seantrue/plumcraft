// What a canvas tells the core and asks it, below its views: measured faces, hits, gestures, and the
// wire paths of faces being dragged. The canvas recognises; the core decides (plan 3, rule 2). The
// Windows counterpart of the Swift kit's CanvasLink.
//
// Units: every point and distance here is in graph units. A canvas converts from its view with its
// own transform (graph = (screen - pan) / zoom) and passes `slop` as its target size over its zoom
// (CanvasLink.Slop).
//
// Hot paths (synchronous, no JSON, never waiting for the core thread): Hit, WireTarget, DragMove,
// WirePaths, Report. Everything else is an async request. Touch it on the canvas's thread (the
// model's context): it keeps the drag under way.

using System.Text.Json;
using PlumCraft.Core;
using PlumCraft.Models;

namespace PlumCraft.Kit;

public sealed class CanvasLink
{
    public PlumCore Core { get; }
    /// <summary>The document's number for the hot calls (0: the first open).</summary>
    public ulong HotDoc { get; }
    /// <summary>The document's number for requests (null: the first open).</summary>
    public ulong? RequestDoc { get; }
    /// <summary>The model to tell when a gesture was answered, so it fetches the scene at once.</summary>
    public DocumentModel? Model { get; }

    private float[] pathBuffer = [];
    /// <summary>The faces moving in the drag under way (its begin's `drag.faces`).</summary>
    public IReadOnlyList<ulong> DraggedFaces { get; private set; } = [];
    /// <summary>The drag under way, as its begin replied.</summary>
    public DragBegun? Drag { get; private set; }

    public CanvasLink(PlumCore core, ulong? doc = null)
    {
        Core = core;
        RequestDoc = doc;
        HotDoc = doc ?? 0;
    }

    public CanvasLink(DocumentModel model) : this(model.Core, model.RequestDoc) => Model = model;

    /// <summary>A target size in screen points as graph units at `zoom` (12 points at zoom 2 is 6).</summary>
    public static double Slop(double points, double zoom) => zoom > 0 ? points / zoom : points;

    // ---------- geometry ----------

    /// <summary>Report measured faces, typed (plum_geometry_set). A shell reports after a face's `rev`
    /// changes, and after the text size or theme changes; all at once when it can.</summary>
    public void Report(IReadOnlyList<PlumCore.FaceMeasure> faces)
    {
        var status = Core.GeometrySet(HotDoc, faces);
        if (status != PlumStatus.Ok) throw new PlumException(PlumErrorKind.Status, $"plum_geometry_set: {PlumStatus.Word(status)}");
    }

    /// <summary>Report measured faces in geometry.set's JSON form: what the typed call can't carry
    /// (`offset`, header and footer `parts`). Faces by graph id.</summary>
    public async Task<GeometrySetReply> ReportJsonAsync(IReadOnlyList<FaceReport> faces)
    {
        var list = Jsonv.Array(faces.Select(f => f.ToJson()));
        var r = await Core.CommandAsync("geometry.set", Jsonv.Object(("faces", list)), RequestDoc).ConfigureAwait(false);
        return Shell.Decode<GeometrySetReply>(r);
    }

    /// <summary>Report faces' header parts (geometry.parts): what the typed plum_geometry_set can't carry,
    /// after it (the same queue keeps them in order). Faces by graph id.</summary>
    public static async Task ReportPartsAsync(PlumCore core, ulong? doc, IReadOnlyList<(string id, IReadOnlyDictionary<string, Rect> parts)> faces)
    {
        if (faces.Count == 0) return;
        var list = Jsonv.Build(w =>
        {
            w.WriteStartArray();
            foreach (var (id, parts) in faces)
            {
                w.WriteStartObject();
                w.WriteString("node", id);
                w.WriteStartObject("parts");
                foreach (var (k, r) in parts)
                {
                    w.WriteStartArray(k);
                    w.WriteNumberValue(r.X0);
                    w.WriteNumberValue(r.Y0);
                    w.WriteNumberValue(r.X1);
                    w.WriteNumberValue(r.Y1);
                    w.WriteEndArray();
                }
                w.WriteEndObject();
                w.WriteEndObject();
            }
            w.WriteEndArray();
        });
        try { await core.CommandAsync("geometry.parts", Jsonv.Object(("faces", list)), doc).ConfigureAwait(false); }
        catch (PlumException) { /* the parts are a refinement: a refusal leaves the typed report standing */ }
    }

    /// <summary>Where the core has a face, pin, wire or frame (geometry.locate).</summary>
    public Task<Located> LocateAsync(JsonElement what) => Core.RequestAsync<Located>("geometry.locate", what, RequestDoc);

    // ---------- hits ----------

    /// <summary>What is at `at` within `slop` (typed).</summary>
    public PlumHit HitAt(Point at, double slop) => Core.HitTest(HotDoc, (float)at.X, (float)at.Y, (float)slop);

    /// <summary>The same as JSON, with ids (geometry.hit).</summary>
    public Task<Hit> HitJsonAsync(Point at, double slop) =>
        Core.RequestAsync<Hit>("geometry.hit", Jsonv.Object(("at", Jsonv.Build(w => { w.WriteStartArray(); w.WriteNumberValue(at.X); w.WriteNumberValue(at.Y); w.WriteEndArray(); })), ("slop", Jsonv.Of(slop))), RequestDoc);

    /// <summary>Where a wire from a pin may land (typed): `output` true for a wire from an output.</summary>
    public PlumHit WireTarget(ulong face, bool output, uint pin, Point at, double slop) =>
        Core.WireTarget(HotDoc, face, output ? 2u : 1u, pin, (float)at.X, (float)at.Y, (float)slop);

    // ---------- gestures ----------

    /// <summary>Send a recognised gesture (ui.gesture): the core hit-tests it and runs what it means.</summary>
    public async Task<GestureReply> GestureAsync(Gesture g)
    {
        var r = await Core.RequestAsync<GestureReply>("ui.gesture", g.ToJson(), RequestDoc);
        if (r.Commands.Count > 0) Model?.AfterOwnRequest();
        if (g.Kind == GestureWord.Drag)
        {
            switch (g.Phase)
            {
                case PhaseWord.Begin:
                    Drag = r.Drag;
                    DraggedFaces = r.Drag?.Faces ?? [];
                    break;
                case PhaseWord.End:
                case PhaseWord.Cancel:
                    Drag = null;
                    DraggedFaces = [];
                    pathBuffer = [];
                    break;
            }
        }
        return r;
    }

    public Task<GestureReply> TapAsync(Point at, IReadOnlyList<ModifierWord>? mods = null, double? slop = null) => GestureAsync(Gesture.Tap(at, mods, slop));
    public Task<GestureReply> DoubleTapAsync(Point at, IReadOnlyList<ModifierWord>? mods = null, double? slop = null) => GestureAsync(Gesture.DoubleTap(at, mods, slop));
    /// <summary>A secondary click or long press: the reply's `context` and `menu`.</summary>
    public Task<GestureReply> ContextAsync(Point at, double? slop = null) => GestureAsync(Gesture.ContextAt(at, slop));
    public Task<GestureReply> MarqueeAsync(Point from, Point to, IReadOnlyList<ModifierWord>? mods = null) => GestureAsync(Gesture.MarqueeOf(from, to, mods));
    /// <summary>Begin a drag: the core decides what it is (a move, a wire, a marquee...) and replies.</summary>
    public Task<GestureReply> DragBeginAsync(Point at, IReadOnlyList<ModifierWord>? mods = null, double? slop = null, double? zoom = null) => GestureAsync(Gesture.DragBegin(at, mods, slop, zoom));
    /// <summary>End the drag: the core runs what it means (one undo step at most).</summary>
    public Task<GestureReply> DragEndAsync(Point at, double? slop = null) => GestureAsync(Gesture.DragEnd(at, slop));
    /// <summary>Cancel the drag: the document stays as it was.</summary>
    public Task<GestureReply> DragCancelAsync() => GestureAsync(Gesture.DragCancel);

    /// <summary>Where the drag is, typed (plum_gesture_move): never JSON, never a request. Nothing changes until the end.</summary>
    public PlumDrag DragMove(Point at, double slop) => Core.GestureMove(HotDoc, (float)at.X, (float)at.Y, (float)slop);

    /// <summary>The paths of the wires on the faces being dragged, moved by (dx, dy) (the drag's offset from
    /// DragMove), reusing one buffer for the drag: per wire its nid and points (x, y, ...).</summary>
    public List<(ulong nid, float[] pts)> WirePaths(float dx, float dy) => WirePaths(DraggedFaces, dx, dy);

    /// <summary>The paths of the wires on `faces`, moved by (dx, dy).</summary>
    public List<(ulong nid, float[] pts)> WirePaths(IReadOnlyList<ulong> faces, float dx, float dy)
    {
        var f = faces as ulong[] ?? [.. faces];
        var n = Core.WirePaths(HotDoc, f, dx, dy, ref pathBuffer);
        return PlumCore.DecodePaths(pathBuffer, n);
    }

    // ---------- keys ----------

    /// <summary>An arrow key nudges the selection (selection.nudge), naming the key press so its repeats
    /// join one undo step (12.6): pass the same `press` for a key's repeats.</summary>
    public async Task<JsonElement> NudgeAsync(double dx, double dy, ulong press)
    {
        var by = Jsonv.Build(w => { w.WriteStartArray(); w.WriteNumberValue(dx); w.WriteNumberValue(dy); w.WriteEndArray(); });
        var r = await Core.CommandAsync("selection.nudge", Jsonv.Object(("by", by), ("drag", Jsonv.Of($"key:{press}"))), RequestDoc);
        Model?.AfterOwnRequest();
        return r;
    }
}
