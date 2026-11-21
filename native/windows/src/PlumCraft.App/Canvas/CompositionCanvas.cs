// The canvas (note 24 section 20.2: Composition, as the spike recommended and the maintainer decided):
// a SpriteVisual per face whose picture is drawn once into a CompositionDrawingSurface, a ShapeVisual
// per wire with its bounds tight round it, frames and open groups as their own visuals, all under one
// ContainerVisual whose transform is the view. Pan and zoom change that one transform, so they are the
// compositor's work; nothing crosses to the core per frame (5.3). The Apple apps' LayerCanvas, on
// Windows' compositor.
//
// It follows a DocumentModel by nid (SceneChanged): only the visuals of items that changed or went are
// touched. A face whose `rev` changed is laid out again on a worker (DirectWrite measures there, never
// per frame) and its measurements go to the core from that worker (plum_geometry_set, then
// geometry.parts); the core recomputes wire paths from them, and those wires come back changed.
//
// Faces are drawn at a scale that steps in powers of two of the display's, chosen from the zoom; 200 ms
// after the view last changed, only the visible faces at the wrong scale are drawn again, so faces are
// sharp above zoom 1 (15.2). The selection and the run wash are child visuals, so selecting, or a run's
// animation (a compositor animation), never redraws a face. Faces, wires and frames well outside the
// view are hidden (culling, N3 slice 4's rule) and keep their pictures.

using System.Diagnostics;
using System.Numerics;
using System.Text.Json;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Geometry;
using Microsoft.Graphics.Canvas.UI.Composition;
using Microsoft.Graphics.DirectX;
using Microsoft.UI;
using Microsoft.UI.Composition;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Hosting;
using PlumCraft.Core;
using PlumCraft.Kit;
using PlumCraft.Models;
using Windows.UI;
using WRect = Windows.Foundation.Rect;
using MRect = PlumCraft.Models.Rect;

namespace PlumCraft.App.Canvas;

/// <summary>The canvas transform: screen = graph · zoom + pan (DIPs).</summary>
public readonly record struct Viewport(Vector2 Pan, float Zoom)
{
    public const float MinZoom = 0.1f, MaxZoom = 4;
    public Vector2 ToGraph(Vector2 v) => (v - Pan) / Zoom;
    public Vector2 ToView(Vector2 g) => g * Zoom + Pan;
}

/// <summary>What the canvas did, for measurements.</summary>
public sealed class CanvasCounts
{
    public int FacesLaidOut, FacesDrawn, Reports, WiresPathed, Batches, WorkerDrawFailures;
}

sealed class FaceEntry(ulong nid, Face face, SpriteVisual visual)
{
    public readonly ulong Nid = nid;
    public Face Face = face;
    public FaceLayout? Layout;
    public ulong? LaidRev, DrawnRev;
    public float DrawnScale;
    public int DrawnTheme = -1;
    public readonly SpriteVisual Visual = visual;
    public CompositionDrawingSurface? Surface;
    public ShapeVisual? Select;
    public CompositionRoundedRectangleGeometry? SelectShape;
    public SpriteVisual? Wash;
    public string? WashToken;
    public SpriteVisual? Preview;
    public string? PreviewHandle;
    public WRect Rect => new(Face.Pos.X, Face.Pos.Y, Layout?.W ?? 220, Layout?.H ?? 160);
}

sealed class WireEntry(ShapeVisual visual, CompositionSpriteShape shape)
{
    public readonly ShapeVisual Visual = visual;
    public readonly CompositionSpriteShape Shape = shape;
    public WRect Bounds;
}

public sealed partial class CompositionCanvas
{
    public DocumentModel Model { get; }
    public CanvasLink Link { get; }
    readonly UIElement host;
    readonly Compositor compositor;
    readonly CompositionGraphicsDevice graphics;
    readonly ContainerVisual root, frameLayer, wireLayer, faceLayer, overlayLayer;
    readonly SpriteVisual background;
    readonly Dictionary<ulong, FaceEntry> faces = [];
    readonly Dictionary<string, ulong> faceById = new(StringComparer.Ordinal);
    readonly Dictionary<ulong, WireEntry> wires = [];
    readonly Dictionary<ulong, SpriteVisual> frames = [];
    readonly Dictionary<ulong, SpriteVisual> groups = [];
    readonly Dictionary<Color, CompositionColorBrush> brushes = [];
    readonly HashSet<ulong> needLayout = [], needDraw = [];
    readonly List<IDisposable> retired = [];
    readonly DispatcherQueueTimer settleTimer;
    readonly FaceText text = FaceText.Shared;
    readonly Icons icons;
    readonly Pictures pictures;
    ShapeVisual? marquee;
    CompositionRoundedRectangleGeometry? marqueeShape;
    bool working;
    int themeGen;
    int drawFailureNoted;
    ulong? lastFitSeq, lastRevealSeq;
    WRect? culledFor;
    /// <summary>When the run's colours began to fade, for the generation shown (8.6).</summary>
    TimeSpan? fadeBegin;
    ulong? fadeGen;
    readonly Stopwatch clock = Stopwatch.StartNew();

    public CanvasColors Colors { get; private set; }
    public Viewport View { get; private set; } = new(new Vector2(40, 40), 1);
    public Vector2 ViewSize { get; private set; }
    /// <summary>Device pixels per DIP (the XAML root's rasterization scale).</summary>
    public float DisplayScale { get; set; } = 1;
    public CanvasCounts Counts { get; } = new();
    public bool Culling { get; set; } = true;
    public int Hidden { get; private set; }
    /// <summary>When every face was first laid out and the visible ones drawn (ms on the canvas's clock).</summary>
    public double? FirstDrawnMs { get; private set; }
    /// <summary>Faces drawn after a batch (their nids), and whenever the view moved.</summary>
    public event Action<IReadOnlySet<ulong>>? Drawn;
    public event Action<Viewport>? ViewChanged;
    /// <summary>Something went wrong that the person should hear about (a refused report).</summary>
    public event Action<string>? Trouble;
    /// <summary>Hold a running face's wash still rather than pulse it (the system's Reduce Motion).</summary>
    public static bool ReduceMotion { get; set; }
    /// <summary>Draw faces on the UI thread (set after a worker's draw failed once).</summary>
    public bool DrawOnUiThread { get; private set; }

    public CompositionCanvas(UIElement host, DocumentModel model)
    {
        this.host = host;
        Model = model;
        Link = new CanvasLink(model);
        compositor = ElementCompositionPreview.GetElementVisual(host).Compositor;
        graphics = CanvasComposition.CreateCompositionGraphicsDevice(compositor, CanvasDevice.GetSharedDevice());
        root = compositor.CreateContainerVisual();
        background = compositor.CreateSpriteVisual();
        background.RelativeSizeAdjustment = Vector2.One;
        frameLayer = compositor.CreateContainerVisual();
        wireLayer = compositor.CreateContainerVisual();
        faceLayer = compositor.CreateContainerVisual();
        overlayLayer = compositor.CreateContainerVisual();
        root.Children.InsertAtTop(frameLayer);
        root.Children.InsertAtTop(wireLayer);
        root.Children.InsertAtTop(faceLayer);
        root.Children.InsertAtTop(overlayLayer);
        var top = compositor.CreateContainerVisual();
        top.RelativeSizeAdjustment = Vector2.One;
        // a child visual isn't clipped to its element: the canvas would draw over the menus and toolbar
        top.Clip = compositor.CreateInsetClip();
        top.Children.InsertAtBottom(background);
        top.Children.InsertAtTop(root);
        ElementCompositionPreview.SetElementChildVisual(host, top);
        Colors = new CanvasColors(model.Theme);
        background.Brush = Brush(Colors.Canvas);
        icons = new Icons(model);
        pictures = new Pictures(model);
        settleTimer = DispatcherQueue.GetForCurrentThread().CreateTimer();
        settleTimer.Interval = TimeSpan.FromMilliseconds(200);
        settleTimer.IsRepeating = false;
        settleTimer.Tick += (_, _) => Settle();
        Model.SceneChanged += Apply;
        Model.PropertyChanged += (_, e) => { if (e.PropertyName == nameof(DocumentModel.Theme)) ThemeChanged(); };
        ApplyTransform();
    }

    CompositionColorBrush Brush(Color c)
    {
        if (!brushes.TryGetValue(c, out var b)) brushes[c] = b = compositor.CreateColorBrush(c);
        return b;
    }

    double Now => clock.Elapsed.TotalMilliseconds;

    // ---------------- following the model ----------------

    /// <summary>Everything again (the first time, and after the theme changed).</summary>
    public void RebuildAll()
    {
        var all = new HashSet<ulong>();
        all.UnionWith(Model.Faces.Order);
        all.UnionWith(Model.Wires.Order);
        all.UnionWith(Model.Frames.Order);
        all.UnionWith(Model.Groups.Order);
        Apply(new SceneChange(Model.Gen, true, all, new HashSet<ulong>(), true, true));
    }

    void Apply(SceneChange c)
    {
        if (c.Full && FirstDrawnMs is null) CoreHost.Mark("first_scene");
        // a move kept at its drop until the scene has it: the scene has it now
        NoteMoveArrived(c);
        foreach (var nid in c.Removed) Remove(nid);
        foreach (var nid in c.Changed)
        {
            if (Model.Faces[nid]?.Item is { } f)
            {
                if (faces.TryGetValue(nid, out var e))
                {
                    var moved = e.Face.Pos != f.Pos;
                    e.Face = f;
                    if (moved) PlaceFace(e);
                    if (e.LaidRev != f.Rev) needLayout.Add(nid);
                }
                else
                {
                    var v = compositor.CreateSpriteVisual();
                    e = new FaceEntry(nid, f, v);
                    faces[nid] = e;
                    faceLayer.Children.InsertAtTop(v);
                    needLayout.Add(nid);
                }
                faceById[f.Id] = nid;
            }
            else if (Model.Wires[nid]?.Item is { } w) PathWire(w);
            else if (Model.Frames[nid]?.Item is { } fr) PlaceFrame(fr);
            else if (Model.Groups[nid]?.Item is { } g) PlaceGroup(g);
        }
        if (c.Full || c.Changed.Count > 0) Reorder();
        icons.Want(c.Changed.Select(n => Model.Faces[n]?.Item.Header.Icon).OfType<string>(), () =>
        {
            // icons came: the faces are drawn again (those in a batch still out, once it is back)
            foreach (var e in faces.Values) needDraw.Add(e.Nid);
            Pump();
        });
        if (c.SelectionChanged || c.Full) UpdateSelection();
        UpdateWashes(c.Changed);
        if (c.ViewChanged || c.Full) HonourFit();
        if (c.ViewChanged || c.Full) HonourZoom();
        Cull(true);
        Pump();
        if (Idle)
        {
            // an empty graph is drawn as soon as its scene is applied
            if (FirstDrawnMs is null && faces.Count == 0) FirstDrawnMs = Now;
            Drawn?.Invoke(new HashSet<ulong>());
        }
    }

    void Remove(ulong nid)
    {
        if (faces.Remove(nid, out var e))
        {
            faceLayer.Children.Remove(e.Visual);
            if (faceById.TryGetValue(e.Face.Id, out var n) && n == nid) faceById.Remove(e.Face.Id);
            // a surface a worker may be drawing goes once the batch is back
            retired.Add(e.Visual);
            if (e.Surface is { } s) retired.Add(s);
            if (!working) DisposeRetired();
            return;
        }
        if (wires.Remove(nid, out var w)) { wireLayer.Children.Remove(w.Visual); w.Visual.Dispose(); return; }
        if (frames.Remove(nid, out var f)) { frameLayer.Children.Remove(f); f.Dispose(); return; }
        if (groups.Remove(nid, out var g)) { frameLayer.Children.Remove(g); g.Dispose(); }
    }

    void DisposeRetired()
    {
        foreach (var d in retired) d.Dispose();
        retired.Clear();
    }

    /// <summary>Faces in the scene's drawing order (by id; the last on top).</summary>
    void Reorder()
    {
        var order = Model.Faces.Order;
        if (order.Count != faceLayer.Children.Count) return;
        Visual? below = null;
        bool same = true;
        int k = 0;
        foreach (var v in faceLayer.Children)
        {
            if (!faces.TryGetValue(order[k++], out var e) || !ReferenceEquals(e.Visual, v)) { same = false; break; }
        }
        if (same) return;
        faceLayer.Children.RemoveAll();
        foreach (var nid in order)
        {
            if (!faces.TryGetValue(nid, out var e)) continue;
            if (below is null) faceLayer.Children.InsertAtBottom(e.Visual); else faceLayer.Children.InsertAbove(e.Visual, below);
            below = e.Visual;
        }
    }

    void ThemeChanged()
    {
        Colors = new CanvasColors(Model.Theme);
        themeGen++;
        background.Brush = Brush(Colors.Canvas);
        foreach (var e in faces.Values) if (e.Layout is not null) needDraw.Add(e.Nid);
        foreach (var w in Model.Wires.Items) PathWire(w);
        foreach (var f in Model.Frames.Items) PlaceFrame(f);
        foreach (var g in Model.Groups.Items) PlaceGroup(g);
        UpdateSelection();
        UpdateWashes([.. faces.Keys]);
        Pump();
    }

    // ---------------- the view ----------------

    public void SetViewSize(Vector2 size)
    {
        var first = ViewSize.X <= 0 || ViewSize.Y <= 0;
        ViewSize = size;
        if (first) HonourFit();
        Cull(false);
    }

    public void SetView(Viewport v)
    {
        var z = Math.Clamp(v.Zoom, Viewport.MinZoom, Viewport.MaxZoom);
        View = v with { Zoom = z };
        ApplyTransform();
        Cull(false);
        settleTimer.Stop();
        settleTimer.Start();
        ViewChanged?.Invoke(View);
    }

    void ApplyTransform() =>
        root.TransformMatrix = Matrix4x4.CreateScale(View.Zoom, View.Zoom, 1) * Matrix4x4.CreateTranslation(View.Pan.X, View.Pan.Y, 0);

    public void PanBy(Vector2 d) => SetView(View with { Pan = View.Pan + d });

    /// <summary>Zoom by `factor` about view point `at`.</summary>
    public void ZoomBy(float factor, Vector2 at)
    {
        var g = View.ToGraph(at);
        var z = Math.Clamp(View.Zoom * factor, Viewport.MinZoom, Viewport.MaxZoom);
        SetView(new Viewport(at - g * z, z));
    }

    public void ZoomTo(float z) => ZoomBy(z / View.Zoom, ViewSize / 2);

    /// <summary>Fit `r` (graph units) into the view, zoom held to lo..hi.</summary>
    public void Fit(MRect r, float lo = 0.2f, float hi = 1)
    {
        if (r.Width <= 0 || r.Height <= 0 || ViewSize.X <= 0 || ViewSize.Y <= 0) return;
        var z = Math.Clamp(MathF.Min(ViewSize.X / (float)r.Width, ViewSize.Y / (float)r.Height), lo, hi);
        var mid = new Vector2((float)(r.X0 + r.Width / 2), (float)(r.Y0 + r.Height / 2));
        SetView(new Viewport(ViewSize / 2 - mid * z, z));
    }

    /// <summary>The core's fit, once per `seq` (8.9), and its reveal (centring a face).</summary>
    void HonourFit()
    {
        var v = Model.View;
        if (v.Reveal is { } rv && rv.Seq != lastRevealSeq)
        {
            lastRevealSeq = rv.Seq;
            var z = MathF.Max(View.Zoom, 0.6f);
            var mid = new Vector2((float)(rv.Rect.X0 + rv.Rect.Width / 2), (float)(rv.Rect.Y0 + rv.Rect.Height / 2));
            SetView(new Viewport(ViewSize / 2 - mid * z, z));
        }
        // a fit that comes before the view has a size waits for it (the scene can arrive first)
        if (v.Fit is { } f && f.Seq != lastFitSeq && ViewSize.X > 0 && ViewSize.Y > 0)
        {
            lastFitSeq = f.Seq;
            Fit(f.Rect);
        }
    }

    /// <summary>Every face's and frame's rect together (null: nothing).</summary>
    public MRect? ContentBounds()
    {
        double x0 = double.MaxValue, y0 = double.MaxValue, x1 = double.MinValue, y1 = double.MinValue;
        foreach (var e in faces.Values)
        {
            var r = e.Rect;
            x0 = Math.Min(x0, r.X); y0 = Math.Min(y0, r.Y); x1 = Math.Max(x1, r.X + r.Width); y1 = Math.Max(y1, r.Y + r.Height);
        }
        foreach (var f in Model.Frames.Items)
        {
            x0 = Math.Min(x0, f.Pos.X); y0 = Math.Min(y0, f.Pos.Y - 22); x1 = Math.Max(x1, f.Pos.X + f.Size.X); y1 = Math.Max(y1, f.Pos.Y + f.Size.Y);
        }
        return x0 <= x1 ? new MRect(x0, y0, x1, y1) : null;
    }

    /// <summary>The scale faces are drawn at for zoom `z`: the display's times a power of two (at most four).</summary>
    public float RasterScale(float z)
    {
        var s = DisplayScale * MathF.Pow(2, MathF.Ceiling(MathF.Log2(MathF.Max(z, 0.01f))));
        return Math.Clamp(s, 0.5f, DisplayScale * 4);
    }

    /// <summary>The visible part of the graph with a fifth of the view each side.</summary>
    public WRect VisibleRect
    {
        get
        {
            var w = ViewSize.X / View.Zoom; var h = ViewSize.Y / View.Zoom;
            return new WRect(-View.Pan.X / View.Zoom - w * 0.2, -View.Pan.Y / View.Zoom - h * 0.2, w * 1.4, h * 1.4);
        }
    }

    static bool Meets(WRect a, WRect b) => a.X <= b.X + b.Width && b.X <= a.X + a.Width && a.Y <= b.Y + b.Height && b.Y <= a.Y + a.Height;

    /// <summary>Hide what is half a view outside the visible rect; again once the view moved a quarter.</summary>
    public void Cull(bool force)
    {
        if (ViewSize.X <= 0) return;
        var vis = VisibleRect;
        if (!force && culledFor is { } last && Math.Abs(last.X + last.Width / 2 - (vis.X + vis.Width / 2)) < vis.Width * 0.25 / 1.4
            && Math.Abs(last.Y + last.Height / 2 - (vis.Y + vis.Height / 2)) < vis.Height * 0.25 / 1.4 && Math.Abs(last.Width - vis.Width) < vis.Width * 0.25)
            return;
        culledFor = vis;
        var keep = new WRect(vis.X - vis.Width * 0.5, vis.Y - vis.Height * 0.5, vis.Width * 2, vis.Height * 2);
        int hidden = 0;
        foreach (var e in faces.Values)
        {
            var show = !Culling || Meets(e.Rect, keep);
            if (e.Visual.IsVisible != show) e.Visual.IsVisible = show;
            if (!show) hidden++;
        }
        foreach (var (nid, w) in wires)
        {
            var show = (!Culling || Meets(w.Bounds, keep)) && nid != pickedWire && !w.Bounds.IsEmpty;
            if (w.Visual.IsVisible != show) w.Visual.IsVisible = show;
            if (!show) hidden++;
        }
        Hidden = hidden;
    }

    /// <summary>200 ms after the view last changed: the visible faces not at the zoom's scale (or the
    /// theme, or their revision) are drawn again; wires get a width that stays visible far out.</summary>
    public int Settle()
    {
        settleTimer.Stop();
        var want = RasterScale(View.Zoom);
        var vis = VisibleRect;
        int n = 0;
        foreach (var e in faces.Values)
            if (e.Layout is not null && Meets(e.Rect, vis) && (e.DrawnScale != want || e.DrawnTheme != themeGen || e.DrawnRev != e.Face.Rev))
            {
                needDraw.Add(e.Nid);
                n++;
            }
        var width = WireWidth;
        foreach (var w in wires.Values) if (w.Shape.StrokeThickness != width) w.Shape.StrokeThickness = width;
        Pump();
        return n;
    }

    /// <summary>A wire's width in graph units: 2.5, and never under 1.2 DIPs on screen.</summary>
    float WireWidth => MathF.Max(2.5f, 1.2f / View.Zoom);

    // ---------------- laying out, reporting and drawing faces ----------------

    /// <summary>Nothing waits to be laid out or drawn, and no batch is out.</summary>
    public bool Idle => !working && needLayout.Count == 0 && needDraw.Count == 0;

    /// <summary>Every face the model has is laid out at its current `rev` (and so reported).</summary>
    public bool AllMeasured => Model.Faces.Order.All(n => faces.TryGetValue(n, out var e) && e.LaidRev == e.Face.Rev);

    public int FaceCount => faces.Count;

    sealed record Job(FaceEntry Entry, Face Face, FaceLayout? Layout, bool Relayout, float? Scale, string? Icon)
    {
        public FaceLayout? Laid;
        public CompositionDrawingSurface? Surface;
        /// <summary>The face drawn on a worker, offscreen; copied into its surface on the UI thread.</summary>
        public CanvasRenderTarget? Picture;
        public bool DrawnOk;
    }

    /// <summary>Send the faces waiting to a worker: lay out those whose `rev` changed and report them, then
    /// draw those that need it. One batch at a time; what comes while one is out waits for the next.</summary>
    async void Pump()
    {
        if (working || (needLayout.Count == 0 && needDraw.Count == 0)) return;
        var visible = VisibleRect;
        var want = RasterScale(View.Zoom);
        var jobs = new List<Job>();
        var ids = new HashSet<ulong>(needLayout);
        ids.UnionWith(needDraw);
        needLayout.Clear();
        needDraw.Clear();
        // a visible face at the zoom's scale; one out of sight at the lowest, once, but only in a batch
        // with nothing in sight to draw (so the first frame waits for no face nobody sees)
        var anySeen = ids.Any(n => faces.TryGetValue(n, out var e) && Meets(e.Rect, visible));
        foreach (var nid in ids)
        {
            if (!faces.TryGetValue(nid, out var e)) continue;
            var relayout = e.LaidRev != e.Face.Rev || e.Layout is null;
            var seen = Meets(e.Rect, visible);
            var stale = e.DrawnRev != e.Face.Rev || e.DrawnTheme != themeGen || relayout;
            float? scale = seen ? want : stale && !anySeen ? MathF.Min(want, DisplayScale) : null;
            if (!seen && stale && anySeen) needDraw.Add(nid);
            if (!relayout && scale is null) continue;
            jobs.Add(new Job(e, e.Face, relayout ? null : e.Layout, relayout, scale, icons.Svg(e.Face.Header.Icon)));
        }
        if (jobs.Count == 0) return;
        working = true;
        Counts.Batches++;
        var core = Model.Core;
        var hot = Link.HotDoc;
        var doc = Link.RequestDoc;
        var colors = Colors;
        var theme = themeGen;
        try
        {
            // 1. laid out and reported, then drawn, on workers: the typed report goes first (what the core
            // hit-tests), the header's parts after it on the same queue while the faces are drawn;
            // drawn into offscreen pictures (a Composition drawing surface takes drawing on the
            // compositor's thread only: the lifted compositor refuses it from a worker), each then
            // copied into its face's surface here, one blit a face
            var offscreen = !DrawOnUiThread;
            var opts = new ParallelOptions { MaxDegreeOfParallelism = Math.Max(1, Environment.ProcessorCount - 1) };
            await Task.Run(async () =>
            {
                Parallel.ForEach(jobs, opts, j => j.Laid = j.Layout ?? FaceLayout.Make(j.Face, text));
                var measured = jobs.Where(j => j.Relayout).Select(j => j.Laid!.Measure(j.Entry.Nid, j.Face.Rev)).ToList();
                Task parts = Task.CompletedTask;
                if (measured.Count > 0)
                {
                    var st = core.GeometrySet(hot, measured);
                    if (st != PlumStatus.Ok) throw new PlumException(PlumErrorKind.Status, $"plum_geometry_set: {PlumStatus.Word(st)}");
                    parts = CanvasLink.ReportPartsAsync(core, doc, [.. jobs.Where(j => j.Relayout).Select(j => (j.Face.Id, (IReadOnlyDictionary<string, MRect>)j.Laid!.Parts()))]);
                }
                if (offscreen)
                    Parallel.ForEach(jobs, opts, j =>
                    {
                        if (j.Scale is not { } s || j.Laid is null) return;
                        j.Picture = Offscreen(j.Face, j.Laid, s, colors, j.Icon);
                    });
                await parts.ConfigureAwait(false);
            });
            Counts.FacesLaidOut += jobs.Count(j => j.Relayout);
            Counts.Reports += jobs.Any(j => j.Relayout) ? 1 : 0;
            if (offscreen)
            {
                if (jobs.Any(j => j.Scale is not null && j.Laid is not null && j.Picture is null))
                {
                    Counts.WorkerDrawFailures++;
                    DrawOnUiThread = true;
                }
            }
            foreach (var j in jobs)
            {
                if (j.Scale is not { } s || j.Laid is null) continue;
                j.Surface = SurfaceFor(j.Entry, j.Laid, s);
                j.DrawnOk = j.Picture is { } pic ? Blit(j.Surface, pic) : DrawInto(j.Surface, j.Face, j.Laid, s, colors, j.Icon);
                j.Picture?.Dispose();
                j.Picture = null;
            }
        }
        catch (Exception ex)
        {
            Trouble?.Invoke($"the canvas: {ex.Message}");
        }
        working = false;
        DisposeRetired();
        if (Counts.Batches == 1) CoreHost.Mark("first_drawn");
        Finish(jobs, theme);
        Pump();
    }

    CompositionDrawingSurface SurfaceFor(FaceEntry e, FaceLayout L, float scale)
    {
        const float m = FaceMetrics.Margin;
        var pw = Math.Clamp((int)Math.Ceiling((L.W + 2 * m) * scale), 1, 4096);
        var ph = Math.Clamp((int)Math.Ceiling((L.H + 2 * m) * scale), 1, 4096);
        if (e.Surface is { } s)
        {
            if ((int)s.SizeInt32.Width != pw || (int)s.SizeInt32.Height != ph) s.Resize(new Windows.Graphics.SizeInt32 { Width = pw, Height = ph });
            return s;
        }
        var surface = graphics.CreateDrawingSurface2(new Windows.Graphics.SizeInt32 { Width = pw, Height = ph }, DirectXPixelFormat.B8G8R8A8UIntNormalized, DirectXAlphaMode.Premultiplied);
        e.Surface = surface;
        var brush = compositor.CreateSurfaceBrush(surface);
        brush.Stretch = CompositionStretch.Fill;
        e.Visual.Brush = brush;
        return surface;
    }

    /// <summary>A face drawn offscreen at `scale` (any thread: Win2D's device is free-threaded); null when it
    /// couldn't be.</summary>
    CanvasRenderTarget? Offscreen(Face face, FaceLayout L, float scale, CanvasColors colors, string? icon)
    {
        try
        {
            const float m = FaceMetrics.Margin;
            var pw = Math.Clamp((int)Math.Ceiling((L.W + 2 * m) * scale), 1, 4096);
            var ph = Math.Clamp((int)Math.Ceiling((L.H + 2 * m) * scale), 1, 4096);
            var rt = new CanvasRenderTarget(CanvasDevice.GetSharedDevice(), pw, ph, 96);
            using (var ds = rt.CreateDrawingSession())
            {
                ds.Clear(Microsoft.UI.Colors.Transparent);
                ds.Transform = Matrix3x2.CreateTranslation(m, m) * Matrix3x2.CreateScale(scale);
                FaceRenderer.Draw(ds, face, L, colors, text, icon);
            }
            return rt;
        }
        catch (Exception e)
        {
            if (Interlocked.Exchange(ref drawFailureNoted, 1) == 0)
                CoreHost.Log($"drawing a face on a worker: {e.GetType().Name} 0x{e.HResult:x8} {e.Message}; faces are drawn on the UI thread from now on");
            return null;
        }
    }

    /// <summary>Copy a face drawn offscreen into its surface (the UI thread).</summary>
    static bool Blit(CompositionDrawingSurface surface, CanvasRenderTarget picture)
    {
        try
        {
            using var ds = CanvasComposition.CreateDrawingSession(surface);
            ds.Clear(Microsoft.UI.Colors.Transparent);
            ds.DrawImage(picture);
            return true;
        }
        catch (Exception e)
        {
            CoreHost.Log($"copying a face into its surface: 0x{e.HResult:x8} {e.Message}");
            return false;
        }
    }

    bool DrawInto(CompositionDrawingSurface surface, Face face, FaceLayout L, float scale, CanvasColors colors, string? icon)
    {
        try
        {
            using var ds = CanvasComposition.CreateDrawingSession(surface);
            ds.Clear(Microsoft.UI.Colors.Transparent);
            ds.Transform = Matrix3x2.CreateTranslation(FaceMetrics.Margin, FaceMetrics.Margin) * Matrix3x2.CreateScale(scale);
            ds.Units = CanvasUnits.Pixels;
            FaceRenderer.Draw(ds, face, L, colors, text, icon);
            return true;
        }
        catch (Exception e)
        {
            CoreHost.Log($"drawing a face: 0x{e.HResult:x8} {e.Message}");
            return false;
        }
    }

    void Finish(List<Job> jobs, int theme)
    {
        var touched = new HashSet<ulong>();
        foreach (var j in jobs)
        {
            var e = j.Entry;
            if (!faces.TryGetValue(e.Nid, out var now) || !ReferenceEquals(now, e)) continue;
            if (e.Face.Rev != j.Face.Rev)
            {
                // it changed again while out: once more
                needLayout.Add(e.Nid);
                continue;
            }
            if (j.Laid is null) continue;
            e.Layout = j.Laid;
            e.LaidRev = j.Face.Rev;
            if (j.DrawnOk && j.Scale is { } s)
            {
                e.DrawnScale = s;
                e.DrawnRev = j.Face.Rev;
                e.DrawnTheme = theme;
                Counts.FacesDrawn++;
            }
            PlaceFace(e);
            UpdatePreview(e);
            touched.Add(e.Nid);
        }
        UpdateSelection();
        UpdateWashes(touched);
        Cull(true);
        if (FirstDrawnMs is null && faces.Count > 0)
        {
            var vis = VisibleRect;
            if (faces.Values.All(e => e.Layout is not null && (e.DrawnRev is not null || !Meets(e.Rect, vis)))) FirstDrawnMs = Now;
        }
        Drawn?.Invoke(touched);
    }

    /// <summary>The face's visual where its scene position and layout say.</summary>
    void PlaceFace(FaceEntry e)
    {
        const float m = FaceMetrics.Margin;
        var L = e.Layout;
        float w = L?.W ?? 220, h = L?.H ?? 160;
        e.Visual.Size = new Vector2(w + 2 * m, h + 2 * m);
        var off = moving.ContainsKey(e.Nid) ? moveOffset : Vector2.Zero;
        e.Visual.Offset = new Vector3((float)e.Face.Pos.X - m + off.X, (float)e.Face.Pos.Y - m + off.Y, 0);
        if (e.Wash is { } wash && L is not null)
        {
            wash.Offset = new Vector3(m, m + L.Header, 0);
            wash.Size = new Vector2(w, MathF.Max(h - L.Header, 0));
            if (wash.Clip is CompositionGeometricClip { Geometry: CompositionRoundedRectangleGeometry g })
            {
                g.Offset = new Vector2(0, -FaceMetrics.Corner);
                g.Size = new Vector2(w, MathF.Max(h - L.Header, 0) + FaceMetrics.Corner);
            }
        }
        if (e.Select is { } sel && e.SelectShape is { } ss)
        {
            sel.Size = e.Visual.Size;
            ss.Offset = new Vector2(m - 3.5f, m - 3.5f);
            ss.Size = new Vector2(w + 7, h + 7);
        }
        if (e.Preview is { } pv && L?.Preview is { } pr)
        {
            pv.Offset = new Vector3((float)pr.X + m, (float)pr.Y + m, 0);
            pv.Size = new Vector2((float)pr.Width, (float)pr.Height);
        }
    }

    // ---------------- the selection and the run wash ----------------

    void UpdateSelection()
    {
        var sel = Model.Selection;
        var nodes = sel.Nodes.ToHashSet(StringComparer.Ordinal);
        var groupsSel = sel.Groups.ToHashSet(StringComparer.Ordinal);
        foreach (var e in faces.Values)
        {
            var on = nodes.Contains(e.Face.Id) || (e.Face.Proxy is { } p && groupsSel.Contains(p.Group));
            if (on)
            {
                if (e.Select is null)
                {
                    var g = compositor.CreateRoundedRectangleGeometry();
                    g.CornerRadius = new Vector2(FaceMetrics.Corner + 3, FaceMetrics.Corner + 3);
                    var shape = compositor.CreateSpriteShape(g);
                    shape.StrokeThickness = 2.5f;
                    var v = compositor.CreateShapeVisual();
                    v.Shapes.Add(shape);
                    e.Visual.Children.InsertAtTop(v);
                    e.Select = v;
                    e.SelectShape = g;
                }
                if (e.Select.Shapes[0] is CompositionSpriteShape sp) sp.StrokeBrush = Brush(Colors.Accent);
                e.Select.IsVisible = true;
                PlaceFace(e);
            }
            else if (e.Select is { } s) s.IsVisible = false;
        }
    }

    /// <summary>The run's fill while it shows (8.6): a child visual over the body in the state's colour at
    /// the table's amount; the running pulse is a compositor animation, so nothing is drawn per frame.</summary>
    void UpdateWashes(IEnumerable<ulong> changed)
    {
        var anim = Model.Anim;
        var on = anim?.On ?? false;
        foreach (var nid in changed)
        {
            if (!faces.TryGetValue(nid, out var e) || e.Layout is null) continue;
            var shown = on ? e.Face.ShownWord : null;
            if (shown is not { } sw)
            {
                if (e.Wash is { } old)
                {
                    e.Visual.Children.Remove(old);
                    old.Dispose();
                    e.Wash = null;
                }
                continue;
            }
            var (token, amount) = sw switch
            {
                ShownWord.Waiting => ("field", 0.55f),
                ShownWord.Running => ("state_running", 0.42f),
                ShownWord.Done => ("state_done", 0.40f),
                ShownWord.Cached => ("state_cached", 0.24f),
                ShownWord.Failed => ("state_error", 0.45f),
                _ => ("state_skipped", 0.60f),
            };
            if (e.Wash is null)
            {
                var w = compositor.CreateSpriteVisual();
                var clipShape = compositor.CreateRoundedRectangleGeometry();
                clipShape.CornerRadius = new Vector2(FaceMetrics.Corner, FaceMetrics.Corner);
                w.Clip = compositor.CreateGeometricClip(clipShape);
                e.Visual.Children.InsertAtBottom(w);
                e.Wash = w;
            }
            var wash = e.Wash;
            wash.Brush = Brush(Colors.Token(token, Color.FromArgb(255, 128, 128, 128)));
            wash.StopAnimation("Opacity");
            wash.Opacity = amount;
            e.WashToken = token;
            if (fadeBegin is { } b && fadeGen == anim?.Generation && sw is not (ShownWord.Running or ShownWord.Waiting))
                Fade(wash, amount, b);
            // Reduce Motion: a running face holds its wash rather than pulsing (N4 slice 4)
            if (sw == ShownWord.Running && !ReduceMotion && anim is not null)
            {
                var a = compositor.CreateScalarKeyFrameAnimation();
                a.InsertKeyFrame(0, 0.28f);
                a.InsertKeyFrame(1, 0.56f);
                a.Duration = TimeSpan.FromMilliseconds(Math.Max(100, anim.PulseMs / 2.0));
                a.IterationBehavior = AnimationIterationBehavior.Forever;
                a.Direction = AnimationDirection.Alternate;
                wash.StartAnimation("Opacity", a);
            }
            PlaceFace(e);
        }
        NoteRunFinished();
    }

    /// <summary>The run (or its replay) is over once no face shows waiting or running: hold the colours for
    /// `hold_ms`, then fade them over `fade_ms` (8.6), once per generation. The core drops `anim.on` at about
    /// the same time, and the washes go then.</summary>
    void NoteRunFinished()
    {
        var a = Model.Anim;
        if (a is null || !a.On)
        {
            fadeBegin = null;
            fadeGen = null;
            return;
        }
        if (fadeGen != a.Generation) fadeBegin = null;
        var busy = faces.Values.Any(e => e.Face.ShownWord is ShownWord.Waiting or ShownWord.Running);
        if (busy)
        {
            fadeBegin = null;
            fadeGen = null;
            return;
        }
        if (fadeBegin is not null) return;
        fadeBegin = clock.Elapsed + TimeSpan.FromMilliseconds(a.HoldMs);
        fadeGen = a.Generation;
        foreach (var e in faces.Values) if (e.Wash is { } w) Fade(w, w.Opacity, fadeBegin.Value);
    }

    void Fade(SpriteVisual w, float from, TimeSpan begin)
    {
        var a = Model.Anim;
        if (a is null) return;
        var f = compositor.CreateScalarKeyFrameAnimation();
        f.InsertKeyFrame(0, from);
        f.InsertKeyFrame(1, 0);
        var delay = begin - clock.Elapsed;
        f.DelayTime = delay > TimeSpan.Zero ? delay : TimeSpan.Zero;
        f.Duration = TimeSpan.FromMilliseconds(Math.Max(1, a.FadeMs));
        w.StopAnimation("Opacity");
        w.StartAnimation("Opacity", f);
    }

    /// <summary>Whether the face shows the run's wash now (tests and the bench).</summary>
    public bool Washed(string id) => faceById.TryGetValue(id, out var n) && faces.TryGetValue(n, out var e) && e.Wash is not null;

    // ---------------- previews ----------------

    /// <summary>A face's rgba8 preview: a child visual whose picture comes from the media service.</summary>
    void UpdatePreview(FaceEntry e)
    {
        var pv = e.Face.Preview;
        var handle = pv is { KindWord: PreviewWord.Rgba8, Handle: { } h } ? Pictures.HandleOf(h) : null;
        if (handle is null || e.Layout?.Preview is null)
        {
            if (e.Preview is { } old) { e.Visual.Children.Remove(old); old.Dispose(); e.Preview = null; e.PreviewHandle = null; }
            return;
        }
        if (e.Preview is not null && e.PreviewHandle == handle)
        {
            e.Preview.Opacity = pv!.Faded == true ? 0.45f : 1;
            PlaceFace(e);
            return;
        }
        e.PreviewHandle = handle;
        var entry = e;
        _ = LoadPreview(entry, handle, pv!.Faded == true);
    }

    async Task LoadPreview(FaceEntry e, string handle, bool faded)
    {
        var bmp = await pictures.LoadAsync(handle);
        if (bmp is null || !faces.TryGetValue(e.Nid, out var now) || !ReferenceEquals(now, e) || e.PreviewHandle != handle) return;
        var size = new Windows.Graphics.SizeInt32 { Width = (int)bmp.SizeInPixels.Width, Height = (int)bmp.SizeInPixels.Height };
        var surface = graphics.CreateDrawingSurface2(size, DirectXPixelFormat.B8G8R8A8UIntNormalized, DirectXAlphaMode.Premultiplied);
        using (var ds = CanvasComposition.CreateDrawingSession(surface))
        {
            ds.Units = CanvasUnits.Pixels;
            ds.Clear(Microsoft.UI.Colors.Transparent);
            ds.DrawImage(bmp);
        }
        var v = e.Preview ?? compositor.CreateSpriteVisual();
        var brush = compositor.CreateSurfaceBrush(surface);
        brush.Stretch = CompositionStretch.Uniform;
        v.Brush = brush;
        v.Opacity = faded ? 0.45f : 1;
        if (e.Preview is null)
        {
            e.Visual.Children.InsertAtTop(v);
            e.Preview = v;
        }
        PlaceFace(e);
    }

    // ---------------- wires, frames, groups ----------------

    /// <summary>A wire's visual, its bounds tight round its path (15.2), stroked in its colour; none for a
    /// wire drawn through a net.</summary>
    void PathWire(Wire w)
    {
        if (!wires.TryGetValue(w.Nid, out var e))
        {
            var shape = compositor.CreateSpriteShape();
            shape.StrokeStartCap = CompositionStrokeCap.Round;
            shape.StrokeEndCap = CompositionStrokeCap.Round;
            var v = compositor.CreateShapeVisual();
            v.Shapes.Add(shape);
            wireLayer.Children.InsertAtTop(v);
            e = new WireEntry(v, shape);
            wires[w.Nid] = e;
        }
        Counts.WiresPathed++;
        e.Shape.StrokeBrush = Brush(Colors.Of(w.Paint));
        e.Shape.StrokeThickness = WireWidth;
        if (w.KindWord == WireWord.Net || w.Path.Count < 4)
        {
            e.Visual.IsVisible = false;
            e.Bounds = WRect.Empty;
            return;
        }
        var b = FaceRenderer.Bounds(w.Path);
        var origin = new Vector2((float)b.X - 6, (float)b.Y - 6);
        using var geo = FaceRenderer.WireGeometry(CanvasDevice.GetSharedDevice(), w.Path, -origin);
        if (geo is null) return;
        e.Shape.Geometry = compositor.CreatePathGeometry(new CompositionPath(geo));
        e.Visual.Offset = new Vector3(origin, 0);
        e.Visual.Size = new Vector2((float)b.Width + 12, (float)b.Height + 12);
        e.Bounds = new WRect(origin.X, origin.Y, b.Width + 12, b.Height + 12);
        e.Visual.IsVisible = pickedWire != w.Nid;
    }

    /// <summary>A frame: its body washed in its colour, an outline, and its title on a tab above.</summary>
    void PlaceFrame(Frame f)
    {
        if (frames.Remove(f.Nid, out var old)) { frameLayer.Children.Remove(old); old.Dispose(); }
        var col = Colors.Of(f.Paint);
        var size = new Vector2((float)f.Size.X, (float)f.Size.Y + 22);
        var v = Label(size, ds =>
        {
            ds.FillRoundedRectangle(new WRect(0, 22, f.Size.X, f.Size.Y), 6, 6, CanvasColors.WithAlpha(col, 0.08));
            ds.DrawRoundedRectangle(new WRect(0.5, 22.5, f.Size.X - 1, f.Size.Y - 1), 6, 6, CanvasColors.WithAlpha(col, 0.6), 1);
            var tw = Math.Min(text.Width(f.Title, TextStyle.Caption) + 16, f.Size.X);
            ds.FillRoundedRectangle(new WRect(0, 2, tw, 20), 4, 4, CanvasColors.WithAlpha(col, 0.25));
            text.Draw(ds, f.Title, TextStyle.Caption, Colors.Text, new WRect(8, 2, tw - 12, 20));
        });
        v.Offset = new Vector3((float)f.Pos.X, (float)f.Pos.Y - 22, 0);
        frames[f.Nid] = v;
        frameLayer.Children.InsertAtBottom(v);
    }

    /// <summary>An open group: an outline round its members with its name; a collapsed one is its face.</summary>
    void PlaceGroup(Group g)
    {
        if (groups.Remove(g.Nid, out var old)) { frameLayer.Children.Remove(old); old.Dispose(); }
        if (g.Collapsed || g.Rect is not { } r) return;
        var col = Colors.Token("plum", Colors.Accent);
        var size = new Vector2((float)r.Width, (float)r.Height + 20);
        var v = Label(size, ds =>
        {
            ds.FillRoundedRectangle(new WRect(0, 20, r.Width, r.Height), 8, 8, CanvasColors.WithAlpha(col, 0.06));
            ds.DrawRoundedRectangle(new WRect(0.75, 20.75, r.Width - 1.5, r.Height - 1.5), 8, 8, CanvasColors.WithAlpha(col, 0.7), 1.5f);
            text.Draw(ds, "▾ " + g.Name, TextStyle.Caption, CanvasColors.WithAlpha(col, 0.95), new WRect(6, 0, r.Width - 12, 20));
        });
        v.Offset = new Vector3((float)r.X0, (float)r.Y0 - 20, 0);
        groups[g.Nid] = v;
        frameLayer.Children.InsertAtTop(v);
    }

    /// <summary>A small picture drawn here and now (frames and groups: few, and rarely redrawn).</summary>
    SpriteVisual Label(Vector2 size, Action<CanvasDrawingSession> draw)
    {
        var scale = DisplayScale * 2;
        var px = new Windows.Graphics.SizeInt32 { Width = Math.Clamp((int)Math.Ceiling(size.X * scale), 1, 4096), Height = Math.Clamp((int)Math.Ceiling(size.Y * scale), 1, 4096) };
        var surface = graphics.CreateDrawingSurface2(px, DirectXPixelFormat.B8G8R8A8UIntNormalized, DirectXAlphaMode.Premultiplied);
        using (var ds = CanvasComposition.CreateDrawingSession(surface))
        {
            ds.Clear(Microsoft.UI.Colors.Transparent);
            ds.Units = CanvasUnits.Pixels;
            ds.Transform = Matrix3x2.CreateScale(px.Width / MathF.Max(size.X, 1), px.Height / MathF.Max(size.Y, 1));
            draw(ds);
        }
        var v = compositor.CreateSpriteVisual();
        var brush = compositor.CreateSurfaceBrush(surface);
        brush.Stretch = CompositionStretch.Fill;
        v.Brush = brush;
        v.Size = size;
        return v;
    }

    // ---------------- the marquee ----------------

    /// <summary>The marquee a drag on empty canvas shows (graph units), or none.</summary>
    public void ShowMarquee(MRect? r)
    {
        if (r is not { } q)
        {
            if (marquee is not null) marquee.IsVisible = false;
            return;
        }
        if (marquee is null)
        {
            marqueeShape = compositor.CreateRoundedRectangleGeometry();
            marqueeShape.CornerRadius = new Vector2(2, 2);
            var shape = compositor.CreateSpriteShape(marqueeShape);
            shape.StrokeDashArray.Add(4);
            shape.StrokeDashArray.Add(3);
            marquee = compositor.CreateShapeVisual();
            marquee.Shapes.Add(shape);
            overlayLayer.Children.InsertAtTop(marquee);
        }
        var sp = (CompositionSpriteShape)marquee.Shapes[0];
        sp.StrokeBrush = Brush(Colors.Accent);
        sp.FillBrush = Brush(CanvasColors.WithAlpha(Colors.Accent, 0.12));
        sp.StrokeThickness = 1.2f / View.Zoom;
        marquee.Offset = new Vector3((float)Math.Min(q.X0, q.X1), (float)Math.Min(q.Y0, q.Y1), 0);
        var size = new Vector2((float)Math.Abs(q.Width), (float)Math.Abs(q.Height));
        marquee.Size = size + new Vector2(2, 2);
        marqueeShape!.Size = size;
        marquee.IsVisible = true;
    }

    // ---------------- lookups ----------------

    public ulong? NidOf(string id) => faceById.TryGetValue(id, out var n) ? n : null;
    public WRect? FaceRect(string id) => NidOf(id) is { } n && faces.TryGetValue(n, out var e) && e.Layout is not null ? e.Rect : null;
    public bool Drew(string id, ulong rev) => NidOf(id) is { } n && faces.TryGetValue(n, out var e) && e.DrawnRev == rev;
    public float? DrawnScaleOf(string id) => NidOf(id) is { } n && faces.TryGetValue(n, out var e) ? e.DrawnScale : null;

    // ---------------- a picture of the canvas (the `screenshot` ask) ----------------

    /// <summary>The canvas as it is shown, drawn again offscreen at the display's scale into a PNG: the
    /// canvas colour, frames, wires, then faces at the view's transform. The washes and the selection are
    /// not in it.</summary>
    public async Task<(int w, int h)> SnapshotAsync(string path)
    {
        var dev = CanvasDevice.GetSharedDevice();
        var w = Math.Max(1, (int)(ViewSize.X * DisplayScale));
        var h = Math.Max(1, (int)(ViewSize.Y * DisplayScale));
        using var target = new CanvasRenderTarget(dev, w, h, 96);
        using (var ds = target.CreateDrawingSession())
        {
            ds.Clear(Colors.Canvas);
            ds.Transform = Matrix3x2.CreateScale(View.Zoom) * Matrix3x2.CreateTranslation(View.Pan) * Matrix3x2.CreateScale(DisplayScale);
            foreach (var wire in Model.Wires.Items)
            {
                if (wire.Path.Count < 4 || wire.KindWord == WireWord.Net) continue;
                using var g = FaceRenderer.WireGeometry(dev, wire.Path, Vector2.Zero);
                if (g is not null) ds.DrawGeometry(g, Colors.Of(wire.Paint), WireWidth);
            }
            var vis = VisibleRect;
            foreach (var nid in Model.Faces.Order)
            {
                if (!faces.TryGetValue(nid, out var e) || e.Layout is null || !Meets(e.Rect, vis)) continue;
                var t = ds.Transform;
                ds.Transform = Matrix3x2.CreateTranslation((float)e.Face.Pos.X, (float)e.Face.Pos.Y) * t;
                FaceRenderer.Draw(ds, e.Face, e.Layout, Colors, text, icons.Svg(e.Face.Header.Icon));
                ds.Transform = t;
            }
        }
        await target.SaveAsync(path, CanvasBitmapFileFormat.Png);
        return (w, h);
    }
}
