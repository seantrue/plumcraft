// What the canvas shows while a drag is under way (N5 slice 3), the Apple LayerCanvas's: the faces moving
// with their wires re-pathed from plum_wire_paths each move (the core's previews, no JSON); a new or
// picked-up wire from its pin to the pointer, snapped to the pin it would land on with that pin ringed
// (it fits: the accent; through a Convert: dashed; it doesn't: the error colour, crossed); the outline of
// a frame, an open group or a bend point at its new place; and the view's zoom the core asks for
// (canvas.zoom). Nothing here decides what a drag is: the core's begin did.

using System.Numerics;
using System.Text.Json;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Geometry;
using Microsoft.UI.Composition;
using PlumCraft.Core;
using PlumCraft.Kit;
using PlumCraft.Models;
using Windows.UI;
using MRect = PlumCraft.Models.Rect;

namespace PlumCraft.App.Canvas;

public sealed partial class CompositionCanvas
{
    /// <summary>Faces moving (nids), drawn at their scene place plus the drag's offset.</summary>
    readonly Dictionary<ulong, Point> moving = [];
    Vector2 moveOffset;
    /// <summary>A move was dropped: its faces stay at the drop until the scene has them there.</summary>
    double? keptSince;
    readonly HashSet<ulong> movedWires = [];
    ShapeVisual? dragWire, targetRing, ghost;
    ulong? pickedWire;
    ulong? lastZoomSeq;

    /// <summary>A move began: these faces follow the drag until it ends.</summary>
    public void BeginMove(IEnumerable<ulong> nids)
    {
        ClearMove();
        foreach (var n in nids) if (faces.TryGetValue(n, out var e)) moving[n] = e.Face.Pos;
        moveOffset = Vector2.Zero;
    }

    public bool Moving => moving.Count > 0;

    /// <summary>The drag's offset now: the faces there, and their wires as the core paths them there.</summary>
    public void MoveFaces(float dx, float dy)
    {
        if (moving.Count == 0) return;
        moveOffset = new Vector2(dx, dy);
        foreach (var n in moving.Keys) if (faces.TryGetValue(n, out var e)) PlaceFace(e);
        foreach (var (nid, pts) in Link.WirePaths([.. moving.Keys], dx, dy))
        {
            var path = new List<Point>(pts.Length / 2);
            for (int k = 0; k + 1 < pts.Length; k += 2) path.Add(new Point(pts[k], pts[k + 1]));
            if (ShowPath(nid, path)) movedWires.Add(nid);
        }
    }

    /// <summary>The move ended: `kept` (it moved things), its faces wait at the drop for the scene; else back.</summary>
    public void EndMove(bool kept)
    {
        if (moving.Count == 0) return;
        if (kept) keptSince = Now;
        else ClearMove();
    }

    void NoteMoveArrived(SceneChange c)
    {
        if (keptSince is not { } t) return;
        if (moving.Keys.Any(c.Changed.Contains) || Now - t > 1500) ClearMove();
    }

    void ClearMove()
    {
        keptSince = null;
        if (moving.Count == 0 && movedWires.Count == 0) return;
        var ns = moving.Keys.ToList();
        moving.Clear();
        moveOffset = Vector2.Zero;
        foreach (var n in ns) if (faces.TryGetValue(n, out var e)) PlaceFace(e);
        foreach (var w in movedWires) if (Model.Wires[w]?.Item is { } wire) PathWire(wire);
        movedWires.Clear();
    }

    /// <summary>A wire's visual drawn along `path` (graph units) for now; false for a wire not shown.</summary>
    bool ShowPath(ulong nid, List<Point> path)
    {
        if (!wires.TryGetValue(nid, out var e) || path.Count < 4) return false;
        var b = FaceRenderer.Bounds(path);
        var origin = new Vector2((float)b.X - 6, (float)b.Y - 6);
        using var geo = FaceRenderer.WireGeometry(CanvasDevice.GetSharedDevice(), path, -origin);
        if (geo is null) return false;
        e.Shape.Geometry = compositor.CreatePathGeometry(new CompositionPath(geo));
        e.Visual.Offset = new Vector3(origin, 0);
        e.Visual.Size = new Vector2((float)b.Width + 12, (float)b.Height + 12);
        e.Visual.IsVisible = true;
        return true;
    }

    // ---------------- a wire being drawn ----------------

    /// <summary>Where a new wire leaves (the drag begin's `from`: a face, a side, a pin), graph units.</summary>
    public Vector2? AnchorPoint(JsonElement? from)
    {
        if (from is not { ValueKind: JsonValueKind.Object } f || f.Get("node").Str() is not { } node || f.Get("pin") is not { ValueKind: JsonValueKind.Number } pin) return null;
        return PinPoint(node, f.Get("side").Str() == "out", pin.GetInt32());
    }

    /// <summary>A pin's place in graph units, from the face's layout.</summary>
    public Vector2? PinPoint(string face, bool output, int pin)
    {
        if (NidOf(face) is not { } n || !faces.TryGetValue(n, out var e) || e.Layout is not { } L) return null;
        var pins = output ? L.OutputPins : L.InputPins;
        if (pin < 0 || pin >= pins.Length) return null;
        var off = moving.ContainsKey(n) ? moveOffset : Vector2.Zero;
        return new Vector2((float)e.Face.Pos.X + (output ? L.W : 0), (float)e.Face.Pos.Y + pins[pin]) + off;
    }

    /// <summary>The wire into input `to` (a picked-up wire's), by nid.</summary>
    public ulong? WireInto(PortRef? to) =>
        to is null ? null : Model.Wires.Items.FirstOrDefault(w => w.To is { } t && t.Node == to.Node && t.Port == to.Port)?.Nid;

    /// <summary>Hide the wire whose end is picked up while it is dragged; null shows it again.</summary>
    public void PickUp(ulong? wire)
    {
        if (pickedWire is { } old && wires.TryGetValue(old, out var o) && !o.Bounds.IsEmpty) o.Visual.IsVisible = true;
        pickedWire = wire;
        if (wire is { } n && wires.TryGetValue(n, out var w)) w.Visual.IsVisible = false;
    }

    /// <summary>A new or picked-up wire from `from` to `to` (graph units); `fits` the target's (1 fits, 2 through
    /// a Convert, 0 doesn't or none). Null `from` hides it.</summary>
    public void ShowDragWire(Vector2? from, Vector2 to, uint fits, bool onPin)
    {
        if (from is not { } a)
        {
            if (dragWire is not null) dragWire.IsVisible = false;
            return;
        }
        dragWire ??= Overlay();
        var shape = (CompositionSpriteShape)dragWire.Shapes[0];
        var dx = MathF.Max(40, MathF.Abs(to.X - a.X) / 2);
        // a wire leaves an output to the right; one drawn from an input leaves to the left
        var leftward = to.X < a.X && !onPin;
        var c1 = a + new Vector2(leftward ? -dx : dx, 0);
        var c2 = to + new Vector2(leftward ? dx : -dx, 0);
        using var pb = new CanvasPathBuilder(CanvasDevice.GetSharedDevice());
        pb.BeginFigure(a);
        pb.AddCubicBezier(c1, c2, to);
        pb.EndFigure(CanvasFigureLoop.Open);
        if (onPin && fits == 0)
        {
            // crossed where it would land: it doesn't fit
            const float k = 5;
            pb.BeginFigure(to + new Vector2(-k, -k)); pb.AddLine(to + new Vector2(k, k)); pb.EndFigure(CanvasFigureLoop.Open);
            pb.BeginFigure(to + new Vector2(-k, k)); pb.AddLine(to + new Vector2(k, -k)); pb.EndFigure(CanvasFigureLoop.Open);
        }
        using var geo = CanvasGeometry.CreatePath(pb);
        shape.Geometry = compositor.CreatePathGeometry(new CompositionPath(geo));
        var col = !onPin ? Colors.Wire : fits == 0 ? Colors.Token("state_error", Color.FromArgb(255, 200, 50, 50)) : Colors.Accent;
        shape.StrokeBrush = Brush(col);
        shape.StrokeThickness = WireWidth;
        shape.StrokeDashArray.Clear();
        if (onPin && fits == 2) { shape.StrokeDashArray.Add(3); shape.StrokeDashArray.Add(2); }
        dragWire.Size = new Vector2(100000, 100000);
        dragWire.Offset = Vector3.Zero;
        dragWire.IsVisible = true;
    }

    /// <summary>A ring round the pin a wire would land on (graph units), by whether it fits; null hides it.</summary>
    public void ShowDragTarget(PlumHit? t)
    {
        if (t is not { } h)
        {
            if (targetRing is not null) targetRing.IsVisible = false;
            return;
        }
        if (targetRing is null)
        {
            targetRing = Overlay();
            var ring = compositor.CreateEllipseGeometry();
            ring.Radius = new Vector2(8, 8);
            ring.Center = new Vector2(10, 10);
            ((CompositionSpriteShape)targetRing.Shapes[0]).Geometry = ring;
            targetRing.Size = new Vector2(20, 20);
        }
        var sp = (CompositionSpriteShape)targetRing.Shapes[0];
        var col = h.Fits == 0 ? Colors.Token("state_error", Color.FromArgb(255, 200, 50, 50)) : Colors.Accent;
        sp.StrokeBrush = Brush(col);
        sp.FillBrush = Brush(CanvasColors.WithAlpha(col, 0.18));
        sp.StrokeThickness = 2;
        sp.StrokeDashArray.Clear();
        if (h.Fits == 2) { sp.StrokeDashArray.Add(3); sp.StrokeDashArray.Add(2); }
        targetRing.Offset = new Vector3(h.X - 10, h.Y - 10, 0);
        targetRing.IsVisible = true;
    }

    /// <summary>The outline of a frame, a group or a bend point where it would go (graph units); null hides it.</summary>
    public void ShowGhost(MRect? r, bool round = false)
    {
        if (r is not { } q)
        {
            if (ghost is not null) ghost.IsVisible = false;
            return;
        }
        ghost ??= Overlay();
        var sp = (CompositionSpriteShape)ghost.Shapes[0];
        CompositionGeometry g;
        if (round)
        {
            var e = compositor.CreateEllipseGeometry();
            e.Radius = new Vector2((float)q.Width / 2, (float)q.Height / 2);
            e.Center = new Vector2((float)q.Width / 2, (float)q.Height / 2);
            g = e;
        }
        else
        {
            var rr = compositor.CreateRoundedRectangleGeometry();
            rr.CornerRadius = new Vector2(6, 6);
            rr.Size = new Vector2((float)q.Width, (float)q.Height);
            g = rr;
        }
        sp.Geometry = g;
        sp.StrokeBrush = Brush(Colors.Accent);
        sp.FillBrush = Brush(CanvasColors.WithAlpha(Colors.Accent, 0.06));
        sp.StrokeThickness = 1.5f / View.Zoom;
        sp.StrokeDashArray.Clear();
        sp.StrokeDashArray.Add(4);
        sp.StrokeDashArray.Add(3);
        ghost.Offset = new Vector3((float)q.X0, (float)q.Y0, 0);
        ghost.Size = new Vector2((float)q.Width + 4, (float)q.Height + 4);
        ghost.IsVisible = true;
    }

    ShapeVisual Overlay()
    {
        var v = compositor.CreateShapeVisual();
        var sp = compositor.CreateSpriteShape();
        sp.StrokeStartCap = CompositionStrokeCap.Round;
        sp.StrokeEndCap = CompositionStrokeCap.Round;
        v.Shapes.Add(sp);
        overlayLayer.Children.InsertAtTop(v);
        return v;
    }

    /// <summary>Every drag preview gone (a drag ended or was cancelled).</summary>
    public void ClearDrag(bool keepWire = false)
    {
        ShowMarquee(null);
        ShowGhost(null);
        ShowDragTarget(null);
        PickUp(null);
        if (!keepWire) ShowDragWire(null, Vector2.Zero, 0, false);
    }

    // ---------------- lookups for the drag and the editors ----------------

    /// <summary>A face's input row's widget, graph units (where its field editor goes).</summary>
    public MRect? RowRect(string face, int row)
    {
        if (NidOf(face) is not { } n || !faces.TryGetValue(n, out var e) || e.Layout is not { } L || row < 0 || row >= L.Widgets.Length) return null;
        var w = L.Widgets[row] ?? (row < L.InputRows.Length ? L.InputRows[row] : null);
        if (w is not { } r) return null;
        return new MRect(e.Face.Pos.X + r.X, e.Face.Pos.Y + r.Y, e.Face.Pos.X + r.X + r.Width, e.Face.Pos.Y + r.Y + r.Height);
    }

    public MRect? FrameRect(string? id) =>
        id is not null && Model.Frames.ById(id) is { } f ? new MRect(f.Pos.X, f.Pos.Y, f.Pos.X + f.Size.X, f.Pos.Y + f.Size.Y) : null;

    public MRect? GroupRect(string? id) => id is not null && Model.Groups.ById(id) is { Rect: { } r } ? r : null;

    // ---------------- the core's zoom (canvas.zoom) ----------------

    /// <summary>The scene's view.zoom, once per seq, about the view's centre.</summary>
    void HonourZoom()
    {
        if (Model.View.Zoom is not { } z || z.Seq == lastZoomSeq) return;
        var first = lastZoomSeq is null && FirstDrawnMs is null;
        lastZoomSeq = z.Seq;
        // one asked for before this window showed the document isn't this window's to follow
        if (first && Model.Applied <= 1) return;
        if (z.By is { } by) ZoomBy((float)by, ViewSize / 2);
        else if (z.To is { } to) ZoomTo((float)to);
    }
}
