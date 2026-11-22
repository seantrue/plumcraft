// Wires drawn into tile pictures (N5 gate; the maintainer: "Take the tiles now"), the Apple canvas's
// WireTiles on Composition.
//
// The canvas drew a ShapeVisual per wire with its bounds tight round it (15.2). A wire across the graph
// then has a visual as big as the graph, which the compositor rasterises and blends over its whole
// visible bounds every frame the view moves; on the iPad, with the same scheme in Core Animation, 500
// nodes and 800 wires missed frames in every phase that moved the view, and tiles ended that.
//
// So wires are drawn as faces are: into pictures, on workers, again only when a view change settles or a
// wire changes. The graph is cut into square tiles of TileSize graph units; a tile's picture holds every
// wire that crosses it, in the scene's order, stroked as the shapes were (round caps), with TileMargin
// texels drawn beyond each side so a texel sampled at the tile's edge blends with the wire beyond it and
// no seam shows. Visible tiles are drawn at the settled zoom's scale (the screen's pixels a graph unit),
// those near the view at no more than 1, the rest at 0.5. Pan and zoom then move pictures only.
//
// A wire being moved (a face drag) or picked up off its input leaves the tiles for a ShapeVisual of its
// own, clipped to the view and its margin (so never graph-sized), and goes back once the tiles that draw
// it again are up, so it never vanishes or shows twice for longer than a batch. The wire under the pointer
// is drawn wider over the tiles by one more clipped visual. `--wire-layers` keeps the old drawing, a
// visual per wire, for the bench to compare.

using System.Numerics;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Geometry;
using Microsoft.Graphics.Canvas.UI.Composition;
using Microsoft.Graphics.DirectX;
using Microsoft.UI.Composition;
using PlumCraft.Models;
using Windows.UI;
using WRect = Windows.Foundation.Rect;

namespace PlumCraft.App.Canvas;

/// <summary>A drawn wire: its path (graph units), its box widened by the stroke, its colour.</summary>
sealed record WireGeom(Point[] Path, WRect Box, Color Color);

sealed class WireTile(SpriteVisual visual)
{
    public readonly SpriteVisual Visual = visual;
    public CompositionDrawingSurface? Surface;
    public CompositionSurfaceBrush? Brush;
    public float Scale, Width;
}

public sealed partial class CompositionCanvas
{
    /// <summary>The old drawing, a visual per wire (`--wire-layers`, for the bench).</summary>
    public static bool WireLayers { get; set; }
    public const float TileSize = 512;
    const int TileMargin = 2;

    readonly Dictionary<ulong, WireGeom> wireGeoms = [];
    readonly Dictionary<(int X, int Y), WireTile> tiles = [];
    readonly HashSet<(int X, int Y)> dirtyTiles = [];
    readonly HashSet<ulong> returning = [];
    readonly Dictionary<ulong, IReadOnlyList<Point>> livePaths = [];
    ContainerVisual? tileLayer;
    ShapeVisual? hoverWireVisual;
    bool tileWorking;

    public int TileCount => tiles.Count;
    public int LiveWireCount => WireLayers ? 0 : wires.Count;

    /// <summary>Bytes of the tiles' pictures as drawn.</summary>
    public long TileBytes => tiles.Values.Sum(t => t.Surface is { } s ? (long)s.SizeInt32.Width * s.SizeInt32.Height * 4 : 0);

    ContainerVisual TileLayer()
    {
        if (tileLayer is null)
        {
            tileLayer = compositor.CreateContainerVisual();
            wireLayer.Children.InsertAtBottom(tileLayer);
        }
        return tileLayer;
    }

    /// <summary>Whether a wire is drawn on a visual of its own rather than in the tiles.</summary>
    bool IsLive(ulong nid) => WireLayers || nid == pickedWire || movedWires.Contains(nid);

    static IEnumerable<(int X, int Y)> TileKeys(WRect r)
    {
        if (r.IsEmpty || double.IsInfinity(r.Width) || double.IsNaN(r.X)) yield break;
        int x0 = (int)Math.Floor(r.X / TileSize), x1 = (int)Math.Floor((r.X + r.Width) / TileSize);
        int y0 = (int)Math.Floor(r.Y / TileSize), y1 = (int)Math.Floor((r.Y + r.Height) / TileSize);
        // a wire's box is a graph's at most; never more tiles than a hostile path could ask for
        if (x1 - x0 >= 4096 || y1 - y0 >= 4096) yield break;
        for (int y = y0; y <= y1; y++) for (int x = x0; x <= x1; x++) yield return (x, y);
    }

    static WRect TileRect((int X, int Y) k) => new(k.X * TileSize, k.Y * TileSize, TileSize, TileSize);

    void Dirty(WRect? box)
    {
        if (box is { } b) dirtyTiles.UnionWith(TileKeys(b));
    }

    /// <summary>A wire's path and colour from the scene; the tiles it crosses, before and after, are drawn
    /// again. None for a wire drawn through a net.</summary>
    void TileWire(Wire w)
    {
        if (w.KindWord == WireWord.Net || w.Path.Count < 4)
        {
            DropTiledWire(w.Nid);
            return;
        }
        var pts = w.Path.ToArray();
        var b = FaceRenderer.Bounds(pts);
        var g = new WireGeom(pts, new WRect(b.X - 6, b.Y - 6, b.Width + 12, b.Height + 12), Colors.Of(w.Paint));
        if (wireGeoms.TryGetValue(w.Nid, out var old)) Dirty(old.Box);
        wireGeoms[w.Nid] = g;
        Dirty(g.Box);
        UpdateLive(w.Nid, null);
        if (hoverWire == w.Nid) ShowHoverWire();
    }

    void DropTiledWire(ulong nid)
    {
        if (wireGeoms.Remove(nid, out var g)) Dirty(g.Box);
        DropLive(nid);
        if (hoverWire == nid)
        {
            hoverWire = null;
            ShowHoverWire();
        }
    }

    void DropLive(ulong nid)
    {
        if (wires.Remove(nid, out var e))
        {
            wireLayer.Children.Remove(e.Visual);
            e.Visual.Dispose();
        }
        returning.Remove(nid);
        livePaths.Remove(nid);
    }

    /// <summary>Put a wire on a visual of its own or take it back into the tiles, as IsLive says; its tiles
    /// are drawn again when that changes. `path`: the path to show now (a move's); null: the scene's.</summary>
    void UpdateLive(ulong nid, IReadOnlyList<Point>? path)
    {
        if (!wireGeoms.TryGetValue(nid, out var g))
        {
            DropLive(nid);
            return;
        }
        if (IsLive(nid))
        {
            var was = wires.ContainsKey(nid) && !returning.Contains(nid);
            returning.Remove(nid);
            if (!wires.TryGetValue(nid, out var e))
            {
                var shape = compositor.CreateSpriteShape();
                shape.StrokeStartCap = CompositionStrokeCap.Round;
                shape.StrokeEndCap = CompositionStrokeCap.Round;
                var v = compositor.CreateShapeVisual();
                v.Shapes.Add(shape);
                v.Clip = compositor.CreateInsetClip();
                wireLayer.Children.InsertAtTop(v);
                wires[nid] = e = new WireEntry(v, shape);
            }
            e.Shape.StrokeBrush = Brush(g.Color);
            e.Shape.StrokeThickness = WireWidth;
            if (path is not null || !was) SetLivePath(e, nid, path ?? g.Path);
            e.Visual.IsVisible = nid != pickedWire && !e.Bounds.IsEmpty;
            // out of the tiles: they are drawn again without it
            if (!was) Dirty(g.Box);
            return;
        }
        if (wires.ContainsKey(nid) && returning.Add(nid)) Dirty(g.Box);
    }

    /// <summary>The rectangle (graph units) a clipped visual may cover: the view and the cull margin.</summary>
    WRect KeepRect
    {
        get
        {
            var vis = VisibleRect;
            return new WRect(vis.X - vis.Width * 0.5, vis.Y - vis.Height * 0.5, vis.Width * 2, vis.Height * 2);
        }
    }

    static WRect Intersect(WRect a, WRect b)
    {
        var x0 = Math.Max(a.X, b.X); var y0 = Math.Max(a.Y, b.Y);
        var x1 = Math.Min(a.X + a.Width, b.X + b.Width); var y1 = Math.Min(a.Y + a.Height, b.Y + b.Height);
        return x1 > x0 && y1 > y0 ? new WRect(x0, y0, x1 - x0, y1 - y0) : WRect.Empty;
    }

    void SetLivePath(WireEntry e, ulong nid, IReadOnlyList<Point> p)
    {
        livePaths[nid] = p;
        e.Bounds = PlaceShape(e.Visual, e.Shape, p);
    }

    /// <summary>A path on a clipped shape visual: its box tight round the path, cut to the view and its
    /// margin, so a wire across the graph is never a graph-sized visual. The box shown (empty: none).</summary>
    WRect PlaceShape(ShapeVisual v, CompositionSpriteShape s, IReadOnlyList<Point> p)
    {
        var b = FaceRenderer.Bounds(p);
        var box = Intersect(new WRect(b.X - 6, b.Y - 6, b.Width + 12, b.Height + 12), ViewSize.X > 0 ? KeepRect : new WRect(b.X - 6, b.Y - 6, b.Width + 12, b.Height + 12));
        if (box.IsEmpty)
        {
            v.IsVisible = false;
            return WRect.Empty;
        }
        var origin = new Vector2((float)box.X, (float)box.Y);
        using var geo = FaceRenderer.WireGeometry(CanvasDevice.GetSharedDevice(), p, -origin);
        if (geo is null)
        {
            v.IsVisible = false;
            return WRect.Empty;
        }
        s.Geometry = compositor.CreatePathGeometry(new CompositionPath(geo));
        v.Offset = new Vector3(origin, 0);
        v.Size = new Vector2((float)box.Width, (float)box.Height);
        return box;
    }

    /// <summary>The wire under the pointer, wider, over the tiles.</summary>
    void ShowHoverWire()
    {
        if (hoverWire is not { } nid || !wireGeoms.TryGetValue(nid, out var g) || nid == pickedWire)
        {
            if (hoverWireVisual is not null) hoverWireVisual.IsVisible = false;
            return;
        }
        if (hoverWireVisual is null)
        {
            var shape = compositor.CreateSpriteShape();
            shape.StrokeStartCap = CompositionStrokeCap.Round;
            shape.StrokeEndCap = CompositionStrokeCap.Round;
            hoverWireVisual = compositor.CreateShapeVisual();
            hoverWireVisual.Shapes.Add(shape);
            hoverWireVisual.Clip = compositor.CreateInsetClip();
            wireLayer.Children.InsertAtTop(hoverWireVisual);
        }
        var sp = (CompositionSpriteShape)hoverWireVisual.Shapes[0];
        sp.StrokeBrush = Brush(g.Color);
        sp.StrokeThickness = WireWidth * 1.8f;
        var shown = PlaceShape(hoverWireVisual, sp, livePaths.TryGetValue(nid, out var lp) ? lp : g.Path);
        hoverWireVisual.IsVisible = !shown.IsEmpty;
    }

    /// <summary>Culling's part for tiled wires: tiles outside the keep rectangle hidden, live visuals clipped
    /// to the view again. The number hidden.</summary>
    int CullTiles(WRect keep)
    {
        int hidden = 0;
        foreach (var (k, t) in tiles)
        {
            var show = !Culling || Meets(TileRect(k), keep);
            if (t.Visual.IsVisible != show) t.Visual.IsVisible = show;
            if (!show) hidden++;
        }
        foreach (var (nid, e) in wires)
        {
            if (livePaths.TryGetValue(nid, out var p)) SetLivePath(e, nid, p);
            e.Visual.IsVisible = nid != pickedWire && !e.Bounds.IsEmpty;
        }
        if (hoverWire is not null) ShowHoverWire();
        return hidden;
    }

    /// <summary>The scale visible tiles are drawn at: the screen's pixels a graph unit at this zoom, so a
    /// settled view shows wires stroked at its own resolution, as the shapes were; from a quarter to twice
    /// the screen's scale (a tile at most 2,048 pixels a side at a scale of 2).</summary>
    float TileScale => Math.Clamp(MathF.Round(DisplayScale * View.Zoom * 64) / 64, 0.25f, DisplayScale * 2);

    /// <summary>After a view change settles: the visible tiles not at the zoom's scale or width; those far
    /// from the view back to the lowest scale.</summary>
    void SettleTiles()
    {
        var want = TileScale;
        var width = WireWidth;
        var vis = ViewRect;
        var keep = KeepRect;
        foreach (var (k, t) in tiles)
        {
            var r = TileRect(k);
            if (Meets(r, vis)) { if (Math.Abs(t.Scale - want) > want * 0.01f || t.Width != width) dirtyTiles.Add(k); }
            else if (!Meets(r, keep) && t.Scale > Math.Min(want, 0.5f)) dirtyTiles.Add(k);
        }
        foreach (var e in wires.Values) e.Shape.StrokeThickness = width;
        PumpTiles();
    }

    /// <summary>The part of the graph in the view, no margin.</summary>
    WRect ViewRect => new(-View.Pan.X / View.Zoom, -View.Pan.Y / View.Zoom, ViewSize.X / View.Zoom, ViewSize.Y / View.Zoom);

    sealed record TileJob((int X, int Y) Key, WRect Rect, float Scale, List<WireGeom> Items)
    {
        public CanvasRenderTarget? Picture;
    }

    /// <summary>Send the tiles waiting to a worker, one batch at a time.</summary>
    async void PumpTiles()
    {
        if (WireLayers || tileWorking || dirtyTiles.Count == 0 || ViewSize.X <= 0) return;
        var want = TileScale;
        var vis = ViewRect;
        var keep = KeepRect;
        var order = new Dictionary<ulong, int>();
        int n = 0;
        foreach (var nid in Model.Wires.Order) order[nid] = n++;
        var drawn = wireGeoms.Where(kv => !IsLive(kv.Key)).OrderBy(kv => order.TryGetValue(kv.Key, out var o) ? o : int.MaxValue).ThenBy(kv => kv.Key).Select(kv => kv.Value).ToList();
        var jobs = new List<TileJob>();
        foreach (var key in dirtyTiles)
        {
            var r = TileRect(key);
            var scale = Meets(r, vis) ? want : Meets(r, keep) ? Math.Min(want, 1) : Math.Min(want, 0.5f);
            jobs.Add(new TileJob(key, r, scale, [.. drawn.Where(g => Meets(g.Box, r))]));
        }
        dirtyTiles.Clear();
        tileWorking = true;
        var back = returning.ToList();
        var width = WireWidth;
        try
        {
            await Task.Run(() => Parallel.ForEach(jobs, new ParallelOptions { MaxDegreeOfParallelism = Math.Max(1, Environment.ProcessorCount - 1) },
                j => j.Picture = DrawTile(j, width)));
            foreach (var j in jobs)
            {
                if (j.Picture is not { } pic)
                {
                    if (tiles.Remove(j.Key, out var gone))
                    {
                        TileLayer().Children.Remove(gone.Visual);
                        gone.Visual.Dispose();
                        gone.Brush?.Dispose();
                        gone.Surface?.Dispose();
                    }
                    continue;
                }
                PutTile(j, pic, width);
                pic.Dispose();
            }
            // the wires that went back into the tiles are drawn there now: their own visuals go
            foreach (var nid in back) if (returning.Contains(nid) && !IsLive(nid)) DropLive(nid);
        }
        catch (Exception ex)
        {
            Trouble?.Invoke($"the canvas's wires: {ex.Message}");
            foreach (var j in jobs) j.Picture?.Dispose();
        }
        tileWorking = false;
        Cull(true);
        PumpTiles();
        if (Idle) Drawn?.Invoke(new HashSet<ulong>());
    }

    /// <summary>One tile's picture, its wires stroked in order with the margin round it (any thread: Win2D's
    /// device is free-threaded); none when no wire crosses it.</summary>
    static CanvasRenderTarget? DrawTile(TileJob j, float width)
    {
        if (j.Items.Count == 0) return null;
        var inner = (int)Math.Ceiling(j.Rect.Width * j.Scale);
        var px = inner + 2 * TileMargin;
        if (inner <= 0 || px > 4096) return null;
        var dev = CanvasDevice.GetSharedDevice();
        var rt = new CanvasRenderTarget(dev, px, px, 96);
        using (var ds = rt.CreateDrawingSession())
        {
            ds.Clear(Microsoft.UI.Colors.Transparent);
            ds.Transform = Matrix3x2.CreateTranslation((float)-j.Rect.X, (float)-j.Rect.Y) * Matrix3x2.CreateScale(j.Scale) * Matrix3x2.CreateTranslation(TileMargin, TileMargin);
            using var style = new CanvasStrokeStyle { StartCap = CanvasCapStyle.Round, EndCap = CanvasCapStyle.Round };
            foreach (var g in j.Items)
            {
                using var geo = FaceRenderer.WireGeometry(dev, g.Path, Vector2.Zero);
                if (geo is not null) ds.DrawGeometry(geo, g.Color, width, style);
            }
        }
        return rt;
    }

    /// <summary>A tile's picture into its surface (the UI thread): the visual covers the tile alone, and its
    /// brush shows the picture from inside its margin.</summary>
    void PutTile(TileJob j, CanvasRenderTarget pic, float width)
    {
        var size = new Windows.Graphics.SizeInt32 { Width = (int)pic.SizeInPixels.Width, Height = (int)pic.SizeInPixels.Height };
        if (!tiles.TryGetValue(j.Key, out var t))
        {
            var v = compositor.CreateSpriteVisual();
            v.Offset = new Vector3((float)j.Rect.X, (float)j.Rect.Y, 0);
            v.Size = new Vector2(TileSize, TileSize);
            TileLayer().Children.InsertAtTop(v);
            tiles[j.Key] = t = new WireTile(v);
        }
        if (t.Surface is null)
        {
            t.Surface = graphics.CreateDrawingSurface2(size, DirectXPixelFormat.B8G8R8A8UIntNormalized, DirectXAlphaMode.Premultiplied);
            t.Brush = compositor.CreateSurfaceBrush(t.Surface);
            t.Brush.Stretch = CompositionStretch.None;
            t.Brush.HorizontalAlignmentRatio = 0;
            t.Brush.VerticalAlignmentRatio = 0;
            t.Visual.Brush = t.Brush;
        }
        else if (t.Surface.SizeInt32.Width != size.Width || t.Surface.SizeInt32.Height != size.Height) t.Surface.Resize(size);
        using (var ds = CanvasComposition.CreateDrawingSession(t.Surface))
        {
            ds.Clear(Microsoft.UI.Colors.Transparent);
            ds.Units = CanvasUnits.Pixels;
            ds.DrawImage(pic);
        }
        // texel (margin, margin) at the tile's corner, one texel 1/scale graph units
        t.Brush!.TransformMatrix = Matrix3x2.CreateTranslation(-TileMargin, -TileMargin) * Matrix3x2.CreateScale(1 / j.Scale);
        t.Scale = j.Scale;
        t.Width = width;
        Counts.TilesDrawn++;
    }
}
