// Three ways to draw the core's scene in WinUI 3, compared by the spike (note 24 section 20):
//
// - Win2DRenderer: one CanvasControl, everything drawn each frame under the view's transform
//   (immediate mode). Faces are cached as CanvasCommandLists (vector), wires as CanvasGeometry.
// - CompositionRenderer: a SpriteVisual per face whose picture is drawn once into a
//   CompositionDrawingSurface (Win2D), a ShapeVisual per wire with its bounds tight around it, all
//   under one ContainerVisual whose transform is the view: Apple's layer canvas (15.2), on
//   Windows.Composition. Pan, zoom and a face's move are the compositor's work.
// - XamlRenderer: plain XAML elements, a Canvas per face of Borders, TextBlocks and shapes, a Path
//   per wire, under one transformed Canvas.
//
// Every renderer draws the same FacePlan; none decides anything (contract rule 2).

using System.Numerics;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Geometry;
using Microsoft.Graphics.Canvas.UI.Composition;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.Foundation;
using Microsoft.Graphics.DirectX;

namespace PlumCraft.CanvasSpike;

public interface ICanvasRenderer
{
    string Name { get; }
    UIElement Element { get; }
    /// <summary>Faces and wires added or changed (laid out already), and gone.</summary>
    void Update(SceneModel s, IEnumerable<Face> faces, IEnumerable<Wire> wires, IEnumerable<ulong> goneFaces, IEnumerable<ulong> goneWires);
    /// <summary>screen = graph · zoom + pan (DIPs).</summary>
    void SetView(float zoom, Vector2 pan);
    /// <summary>Faces moved by offset during a drag, and their wires' paths as the core gave them.</summary>
    void Drag(SceneModel s, IReadOnlyCollection<ulong> faces, Vector2 offset, List<(ulong nid, float[] pts)> paths);
    /// <summary>The time the last frame's drawing took on the UI thread (immediate mode only).</summary>
    double LastDrawMs { get; }
    int DrawCount { get; }
}

static class Pts
{
    public static Vector2[] Of(float[] f)
    {
        var p = new Vector2[f.Length / 2];
        for (int i = 0; i < p.Length; i++) p[i] = new Vector2(f[2 * i], f[2 * i + 1]);
        return p;
    }
}

// ---------------------------------------------------------------------------------------------

public sealed class Win2DRenderer : ICanvasRenderer
{
    readonly CanvasControl control = new() { ClearColor = Colors.Transparent };
    readonly Text text;
    SceneModel? scene;
    readonly Dictionary<ulong, CanvasCommandList> faces = [];
    readonly Dictionary<ulong, (CanvasGeometry geo, Rect bounds)> wires = [];
    float zoom = 1;
    Vector2 pan;
    HashSet<ulong> dragging = [];
    Vector2 dragOffset;
    Dictionary<ulong, CanvasGeometry> dragWires = [];

    public string Name => "win2d";
    public UIElement Element => control;
    public double LastDrawMs { get; private set; }
    public int DrawCount { get; private set; }

    public Win2DRenderer(Text t)
    {
        text = t;
        control.Draw += OnDraw;
    }

    public void Update(SceneModel s, IEnumerable<Face> fs, IEnumerable<Wire> ws, IEnumerable<ulong> goneF, IEnumerable<ulong> goneW)
    {
        scene = s;
        var dev = CanvasDevice.GetSharedDevice();
        foreach (var id in goneF) if (faces.Remove(id, out var c)) c.Dispose();
        foreach (var id in goneW) if (wires.Remove(id, out var g)) g.geo.Dispose();
        foreach (var f in fs)
        {
            if (faces.Remove(f.Nid, out var old)) old.Dispose();
            var cl = new CanvasCommandList(dev);
            using (var ds = cl.CreateDrawingSession()) FacePlan.Draw(ds, f, s, text);
            faces[f.Nid] = cl;
        }
        foreach (var w in ws)
        {
            if (wires.Remove(w.Nid, out var old)) old.geo.Dispose();
            if (FacePlan.WireGeometry(dev, w.Path) is { } geo) wires[w.Nid] = (geo, FacePlan.Bounds(w.Path));
        }
        control.Invalidate();
    }

    public void SetView(float z, Vector2 p)
    {
        zoom = z; pan = p;
        control.Invalidate();
    }

    public void Drag(SceneModel s, IReadOnlyCollection<ulong> fs, Vector2 offset, List<(ulong nid, float[] pts)> paths)
    {
        dragging = [.. fs];
        dragOffset = offset;
        foreach (var g in dragWires.Values) g.Dispose();
        dragWires = [];
        var dev = CanvasDevice.GetSharedDevice();
        foreach (var (nid, pts) in paths)
            if (FacePlan.WireGeometry(dev, Pts.Of(pts)) is { } geo) dragWires[nid] = geo;
        control.Invalidate();
    }

    void OnDraw(CanvasControl sender, CanvasDrawEventArgs args)
    {
        if (scene is null) return;
        var t0 = System.Diagnostics.Stopwatch.GetTimestamp();
        var ds = args.DrawingSession;
        ds.Transform = Matrix3x2.CreateScale(zoom) * Matrix3x2.CreateTranslation(pan);
        var size = sender.Size;
        var vis = new Rect(-pan.X / zoom, -pan.Y / zoom, size.Width / zoom, size.Height / zoom);
        foreach (var (nid, w) in wires)
        {
            if (dragWires.ContainsKey(nid)) continue;
            var b = w.bounds;
            if (b.X > vis.Right || b.Y > vis.Bottom || b.X + b.Width < vis.X || b.Y + b.Height < vis.Y) continue;
            if (scene.Wires.TryGetValue(nid, out var wire)) ds.DrawGeometry(w.geo, wire.Paint, 2.5f);
        }
        foreach (var (nid, g) in dragWires)
            if (scene.Wires.TryGetValue(nid, out var wire)) ds.DrawGeometry(g, wire.Paint, 2.5f);
        foreach (var (nid, cl) in faces)
        {
            if (!scene.Faces.TryGetValue(nid, out var f) || f.Layout is null) continue;
            var pos = f.Pos + (dragging.Contains(nid) ? dragOffset : Vector2.Zero);
            if (pos.X > vis.Right || pos.Y > vis.Bottom || pos.X + f.Layout.W + 8 < vis.X || pos.Y + f.Layout.H + 8 < vis.Y) continue;
            ds.DrawImage(cl, pos);
        }
        LastDrawMs = (System.Diagnostics.Stopwatch.GetTimestamp() - t0) * 1000.0 / System.Diagnostics.Stopwatch.Frequency;
        DrawCount++;
    }
}

// ---------------------------------------------------------------------------------------------

public sealed class CompositionRenderer : ICanvasRenderer
{
    readonly Grid host = new() { Background = new SolidColorBrush(Colors.Transparent) };
    readonly Compositor compositor;
    readonly CompositionGraphicsDevice graphics;
    readonly ContainerVisual root, wireLayer, faceLayer;
    readonly Text text;
    readonly Dictionary<ulong, (SpriteVisual v, CompositionDrawingSurface s)> faces = [];
    readonly Dictionary<ulong, (ShapeVisual v, CompositionSpriteShape shape, Vector2 origin)> wires = [];
    readonly Dictionary<Windows.UI.Color, CompositionColorBrush> brushes = [];
    const float Margin = 8;
    /// <summary>The scale faces are rasterised at (a real shell redraws the visible ones when a zoom settles).</summary>
    public float RasterScale = 1;

    public string Name => "composition";
    public UIElement Element => host;
    public double LastDrawMs => 0;
    public int DrawCount => 0;

    public CompositionRenderer(Text t)
    {
        text = t;
        compositor = ElementCompositionPreview.GetElementVisual(host).Compositor;
        graphics = CanvasComposition.CreateCompositionGraphicsDevice(compositor, CanvasDevice.GetSharedDevice());
        root = compositor.CreateContainerVisual();
        wireLayer = compositor.CreateContainerVisual();
        faceLayer = compositor.CreateContainerVisual();
        root.Children.InsertAtTop(wireLayer);
        root.Children.InsertAtTop(faceLayer);
        ElementCompositionPreview.SetElementChildVisual(host, root);
    }

    CompositionColorBrush Brush(Windows.UI.Color c)
    {
        if (!brushes.TryGetValue(c, out var b)) brushes[c] = b = compositor.CreateColorBrush(c);
        return b;
    }

    public void Update(SceneModel s, IEnumerable<Face> fs, IEnumerable<Wire> ws, IEnumerable<ulong> goneF, IEnumerable<ulong> goneW)
    {
        foreach (var id in goneF) if (faces.Remove(id, out var f)) { faceLayer.Children.Remove(f.v); f.v.Dispose(); f.s.Dispose(); }
        foreach (var id in goneW) if (wires.Remove(id, out var w)) { wireLayer.Children.Remove(w.v); w.v.Dispose(); }
        foreach (var f in fs)
        {
            if (f.Layout is null) continue;
            var l = f.Layout;
            var px = new Size(Math.Ceiling((l.W + 2 * Margin) * RasterScale), Math.Ceiling((l.H + 2 * Margin) * RasterScale));
            if (!faces.TryGetValue(f.Nid, out var e))
            {
                var surface = graphics.CreateDrawingSurface(px, DirectXPixelFormat.B8G8R8A8UIntNormalized, DirectXAlphaMode.Premultiplied);
                var v = compositor.CreateSpriteVisual();
                v.Brush = compositor.CreateSurfaceBrush(surface);
                faceLayer.Children.InsertAtTop(v);
                e = (v, surface);
                faces[f.Nid] = e;
            }
            else if (e.s.Size != px)
            {
                e.s.Resize(new Windows.Graphics.SizeInt32 { Width = (int)px.Width, Height = (int)px.Height });
            }
            using (var ds = CanvasComposition.CreateDrawingSession(e.s))
            {
                ds.Clear(Colors.Transparent);
                ds.Transform = Matrix3x2.CreateTranslation(Margin, Margin) * Matrix3x2.CreateScale(RasterScale);
                FacePlan.Draw(ds, f, s, text);
            }
            e.v.Size = new Vector2(l.W + 2 * Margin, l.H + 2 * Margin);
            e.v.Offset = new Vector3(f.Pos.X - Margin, f.Pos.Y - Margin, 0);
        }
        var dev = CanvasDevice.GetSharedDevice();
        foreach (var w in ws) SetWire(dev, w.Nid, w.Paint, w.Path);
    }

    void SetWire(CanvasDevice dev, ulong nid, Windows.UI.Color paint, Vector2[] path)
    {
        if (path.Length < 4) return;
        var b = FacePlan.Bounds(path);
        var origin = new Vector2((float)b.X - 4, (float)b.Y - 4);
        using var geo = FacePlan.WireGeometry(dev, path, -origin);
        if (geo is null) return;
        var pg = compositor.CreatePathGeometry(new CompositionPath(geo));
        if (!wires.TryGetValue(nid, out var e))
        {
            var shape = compositor.CreateSpriteShape(pg);
            shape.StrokeThickness = 2.5f;
            var v = compositor.CreateShapeVisual();
            v.Shapes.Add(shape);
            wireLayer.Children.InsertAtTop(v);
            e = (v, shape, origin);
        }
        else
        {
            e.shape.Geometry = pg;
            e.origin = origin;
        }
        e.shape.StrokeBrush = Brush(paint);
        e.v.Offset = new Vector3(origin, 0);
        e.v.Size = new Vector2((float)b.Width + 8, (float)b.Height + 8);
        wires[nid] = e;
    }

    public void SetView(float zoom, Vector2 pan) =>
        root.TransformMatrix = Matrix4x4.CreateScale(zoom, zoom, 1) * Matrix4x4.CreateTranslation(pan.X, pan.Y, 0);

    public void Drag(SceneModel s, IReadOnlyCollection<ulong> fs, Vector2 offset, List<(ulong nid, float[] pts)> paths)
    {
        foreach (var id in fs)
            if (faces.TryGetValue(id, out var e) && s.Faces.TryGetValue(id, out var f))
                e.v.Offset = new Vector3(f.Pos.X + offset.X - Margin, f.Pos.Y + offset.Y - Margin, 0);
        var dev = CanvasDevice.GetSharedDevice();
        foreach (var (nid, pts) in paths)
            if (s.Wires.TryGetValue(nid, out var w)) SetWire(dev, nid, w.Paint, Pts.Of(pts));
    }
}

// ---------------------------------------------------------------------------------------------

public sealed class XamlRenderer : ICanvasRenderer
{
    readonly Canvas host = new() { Background = new SolidColorBrush(Colors.Transparent) };
    readonly Canvas root = new();
    readonly Canvas wireLayer = new(), faceLayer = new();
    readonly MatrixTransform view = new();
    readonly Dictionary<ulong, Canvas> faces = [];
    readonly Dictionary<ulong, Microsoft.UI.Xaml.Shapes.Path> wires = [];
    readonly Dictionary<Windows.UI.Color, SolidColorBrush> brushes = [];
    readonly FontFamily font = new("Segoe UI Variable Text");

    public string Name => "xaml";
    public UIElement Element => host;
    public double LastDrawMs => 0;
    public int DrawCount => 0;

    public XamlRenderer()
    {
        root.RenderTransform = view;
        root.Children.Add(wireLayer);
        root.Children.Add(faceLayer);
        host.Children.Add(root);
    }

    SolidColorBrush Brush(Windows.UI.Color c)
    {
        if (!brushes.TryGetValue(c, out var b)) brushes[c] = b = new SolidColorBrush(c);
        return b;
    }

    TextBlock Label(string s, double size, Windows.UI.Color c, double x, double y, double w, bool bold = false)
    {
        var t = new TextBlock { Text = s, FontSize = size, FontFamily = font, Foreground = Brush(c), Width = w, TextTrimming = TextTrimming.CharacterEllipsis, IsHitTestVisible = false };
        if (bold) t.FontWeight = Microsoft.UI.Text.FontWeights.SemiBold;
        Canvas.SetLeft(t, x);
        Canvas.SetTop(t, y);
        return t;
    }

    Canvas Build(Face f, SceneModel s)
    {
        var l = f.Layout!;
        var c = new Canvas { Width = l.W, Height = l.H };
        c.Children.Add(new Border
        {
            Width = l.W, Height = l.H, CornerRadius = new CornerRadius(M.Corner), Background = Brush(f.Body),
            BorderBrush = Brush(f.Selected ? s.Token("accent", Colors.DodgerBlue) : f.Border), BorderThickness = new Thickness(f.Selected ? 2.5 : Math.Max(1, f.BorderWidth)),
        });
        c.Children.Add(new Border { Width = l.W, Height = M.Header, CornerRadius = new CornerRadius(M.Corner, M.Corner, 0, 0), Background = Brush(f.Header) });
        c.Children.Add(Label(f.Title, 14, s.Token("header_text", Colors.Black), l.TitleX, 4, M.TitleMax, true));
        var top = M.Header + 4;
        var body = s.Token("text", Colors.Black);
        for (int i = 0; i < f.Inputs.Count; i++)
        {
            var r = f.Inputs[i];
            c.Children.Add(Label(FacePlan.Caption(r), 13, body, 14, top + i * M.Row + 2, l.W - 28));
            if (!float.IsNaN(l.Widgets[i * 4]) && r.Value != null)
            {
                var b = new Border
                {
                    Width = l.Widgets[i * 4 + 2] - l.Widgets[i * 4], Height = l.Widgets[i * 4 + 3] - l.Widgets[i * 4 + 1], CornerRadius = new CornerRadius(3),
                    Background = Brush(s.Token("field", Colors.White)), BorderBrush = Brush(s.Token("field_border", Colors.Gray)), BorderThickness = new Thickness(1),
                    Child = new TextBlock { Text = r.Value, FontSize = 12, FontFamily = font, Margin = new Thickness(5, 1, 3, 0), TextTrimming = TextTrimming.CharacterEllipsis },
                };
                Canvas.SetLeft(b, l.Widgets[i * 4]);
                Canvas.SetTop(b, l.Widgets[i * 4 + 1]);
                c.Children.Add(b);
            }
            c.Children.Add(PinShape(0, l.InPins[i], r));
        }
        for (int i = 0; i < f.Outputs.Count; i++)
        {
            var r = f.Outputs[i];
            var t = Label(r.Label, 13, body, l.W - 14 - l.OutW, top + i * M.Row + 2, l.OutW);
            t.TextAlignment = TextAlignment.Right;
            c.Children.Add(t);
            c.Children.Add(PinShape(l.W, l.OutPins[i], r));
        }
        c.Tag = f.Nid;
        return c;
    }

    FrameworkElement PinShape(float x, float y, Row r)
    {
        Shape e = r.Square ? new Microsoft.UI.Xaml.Shapes.Rectangle() : new Ellipse();
        e.Width = e.Height = 2 * M.PinRadius;
        e.Fill = Brush(r.Pin);
        Canvas.SetLeft(e, x - M.PinRadius);
        Canvas.SetTop(e, y - M.PinRadius);
        return e;
    }

    static Geometry PathOf(Vector2[] p)
    {
        var fig = new PathFigure { StartPoint = new Point(p[0].X, p[0].Y), IsClosed = false, IsFilled = false };
        for (int i = 1; i + 2 < p.Length; i += 3)
            fig.Segments.Add(new BezierSegment { Point1 = new Point(p[i].X, p[i].Y), Point2 = new Point(p[i + 1].X, p[i + 1].Y), Point3 = new Point(p[i + 2].X, p[i + 2].Y) });
        var g = new PathGeometry();
        g.Figures.Add(fig);
        return g;
    }

    public void Update(SceneModel s, IEnumerable<Face> fs, IEnumerable<Wire> ws, IEnumerable<ulong> goneF, IEnumerable<ulong> goneW)
    {
        foreach (var id in goneF) if (faces.Remove(id, out var c)) faceLayer.Children.Remove(c);
        foreach (var id in goneW) if (wires.Remove(id, out var p)) wireLayer.Children.Remove(p);
        foreach (var f in fs)
        {
            if (f.Layout is null) continue;
            if (faces.Remove(f.Nid, out var old)) faceLayer.Children.Remove(old);
            var c = Build(f, s);
            Canvas.SetLeft(c, f.Pos.X);
            Canvas.SetTop(c, f.Pos.Y);
            faceLayer.Children.Add(c);
            faces[f.Nid] = c;
        }
        foreach (var w in ws)
        {
            if (w.Path.Length < 4) continue;
            if (!wires.TryGetValue(w.Nid, out var p))
            {
                p = new Microsoft.UI.Xaml.Shapes.Path { StrokeThickness = 2.5, IsHitTestVisible = false };
                wireLayer.Children.Add(p);
                wires[w.Nid] = p;
            }
            p.Stroke = Brush(w.Paint);
            p.Data = PathOf(w.Path);
        }
    }

    public void SetView(float zoom, Vector2 pan) => view.Matrix = new Microsoft.UI.Xaml.Media.Matrix(zoom, 0, 0, zoom, pan.X, pan.Y);

    public void Drag(SceneModel s, IReadOnlyCollection<ulong> fs, Vector2 offset, List<(ulong nid, float[] pts)> paths)
    {
        foreach (var id in fs)
            if (faces.TryGetValue(id, out var c) && s.Faces.TryGetValue(id, out var f))
            {
                Canvas.SetLeft(c, f.Pos.X + offset.X);
                Canvas.SetTop(c, f.Pos.Y + offset.Y);
            }
        foreach (var (nid, pts) in paths)
            if (wires.TryGetValue(nid, out var p) && pts.Length >= 8) p.Data = PathOf(Pts.Of(pts));
    }
}
