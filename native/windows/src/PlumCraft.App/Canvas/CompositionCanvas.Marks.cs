// What slices 2 and 3 left undrawn that the scene carries (N5 slice 4; the Apple canvas's placeTags,
// placeMark and hover): a net's tags at its pins (8.8: the driver's with how many inputs it feeds, one at
// each use), an application's marks on wires, the ring on a pin and the wider wire under the pointer, and
// the faces a marquee touches. Each is a few visuals in a layer of its own above the wires; none redraws a
// face. Which faces a marquee touches is the core's answer (`ui.gesture` drag move's `touches`), asked at
// most ten times a second while the marquee moves, never worked out here.

using System.Numerics;
using Microsoft.Graphics.Canvas;
using Microsoft.UI.Composition;
using PlumCraft.Core;
using PlumCraft.Models;
using Windows.UI;
using WRect = Windows.Foundation.Rect;

namespace PlumCraft.App.Canvas;

public sealed partial class CompositionCanvas
{
    ContainerVisual? tagLayer;
    readonly List<SpriteVisual> tagVisuals = [];
    readonly Dictionary<ulong, SpriteVisual> markVisuals = [];
    ShapeVisual? hoverRing;
    ulong? hoverWire;
    readonly Dictionary<ulong, ShapeVisual> touchRings = [];
    public int TagCount => tagVisuals.Count;
    public int MarkCount => markVisuals.Count;
    public int TouchCount => touchRings.Count;
    public string? Hovered { get; private set; }

    ContainerVisual TagLayer()
    {
        if (tagLayer is null)
        {
            tagLayer = compositor.CreateContainerVisual();
            root.Children.InsertAbove(tagLayer, faceLayer);
        }
        return tagLayer;
    }

    /// <summary>Net tags at their pins, where the core puts them for its hit test (`geometry.rs`'s estimate:
    /// 6.6 a character plus 17 wide, 17 high).</summary>
    public void PlaceTags()
    {
        foreach (var v in tagVisuals) { TagLayer().Children.Remove(v); v.Dispose(); }
        tagVisuals.Clear();
        foreach (var n in Model.Nets.Items)
        {
            var col = Colors.Of(n.Paint);
            var name = n.Key.Name;
            var fill = n.Selected ? CanvasColors.WithAlpha(col, 0.45) : Colors.Field;
            if (PinPoint(n.Driver.Face, true, n.Driver.Pin) is { } p)
            {
                var text = n.Driver.Text ?? name;
                var w = 6.6f * Math.Min(text.Length, 64) + 17;
                var v = Pill(text, w, 17, fill, col);
                v.Offset = new Vector3(p.X + (float)n.Driver.At.X, p.Y + (float)n.Driver.At.Y - 8.5f, 0);
                TagLayer().Children.InsertAtTop(v);
                tagVisuals.Add(v);
            }
            var uw = 6.6f * Math.Min(name.Length, 64) + 17;
            foreach (var u in n.Uses)
            {
                if (PinPoint(u.Face, false, u.Pin) is not { } q) continue;
                var v = Pill(name, uw, 17, fill, col);
                v.Offset = new Vector3(q.X + (float)u.At.X - uw, q.Y + (float)u.At.Y - 8.5f, 0);
                TagLayer().Children.InsertAtTop(v);
                tagVisuals.Add(v);
            }
        }
    }

    /// <summary>An application's mark on a wire: its label in a pill outlined in its tone, centred at `at`.</summary>
    void PlaceMark(Mark m)
    {
        if (markVisuals.Remove(m.Nid, out var old)) { TagLayer().Children.Remove(old); old.Dispose(); }
        if (m.At is not { } at) return;
        var tone = m.ToneWord switch
        {
            ToneWord.Warn => Colors.Token("warning", Color.FromArgb(255, 210, 140, 30)),
            ToneWord.Error => Colors.Token("error", Color.FromArgb(255, 200, 60, 50)),
            _ => Colors.Accent,
        };
        var w = (float)text.Width(m.Label, TextStyle.Caption) + 14;
        var v = Pill(m.Label, w, 18, Colors.Field, tone);
        v.Offset = new Vector3((float)at.X - w / 2, (float)at.Y - 9, 0);
        TagLayer().Children.InsertAtTop(v);
        markVisuals[m.Nid] = v;
    }

    /// <summary>A face's waveform thumbnail: the peaks rendition the core lends (min and max a column, a lane a
    /// channel, at most four), drawn in the theme's waveform colours into the face's preview visual.</summary>
    async Task LoadWaveform(FaceEntry e, string handle, string key, bool faded)
    {
        if (await pictures.LendAsync(handle, "peaks") is not { } got) return;
        if (!faces.TryGetValue(e.Nid, out var now) || !ReferenceEquals(now, e) || e.PreviewHandle != key || e.Layout?.Preview is not { } r) return;
        var ch = Math.Clamp((int)(got.note.Channels ?? 1), 1, 4);
        var bins = (int)(got.note.Bins ?? 0);
        if (bins <= 0) return;
        var f = new float[got.bytes.Length / 4];
        Buffer.BlockCopy(got.bytes, 0, f, 0, f.Length * 4);
        var fg = Colors.Token("waveform", Color.FromArgb(255, 47, 163, 154));
        var bg = Colors.Token("waveform_bg", Color.FromArgb(255, 236, 240, 238));
        var scale = DisplayScale * 2;
        var size = new Windows.Graphics.SizeInt32 { Width = Math.Max(1, (int)(r.Width * scale)), Height = Math.Max(1, (int)(r.Height * scale)) };
        var surface = graphics.CreateDrawingSurface2(size, Microsoft.Graphics.DirectX.DirectXPixelFormat.B8G8R8A8UIntNormalized, Microsoft.Graphics.DirectX.DirectXAlphaMode.Premultiplied);
        using (var ds = Microsoft.Graphics.Canvas.UI.Composition.CanvasComposition.CreateDrawingSession(surface))
        {
            ds.Units = CanvasUnits.Pixels;
            ds.Clear(bg);
            float w = size.Width, h = size.Height, lh = h / ch, stroke = MathF.Max(1, w / bins);
            for (int c = 0; c < ch; c++)
            {
                var mid = lh * (c + 0.5f);
                for (int b = 0; b < bins; b++)
                {
                    var k = c * bins * 2 + b * 2;
                    if (k + 1 >= f.Length) break;
                    var lo = Math.Clamp(f[k], -1, 1);
                    var hi = Math.Clamp(f[k + 1], -1, 1);
                    var x = (float)b / bins * w;
                    ds.DrawLine(x, mid - hi * lh / 2, x, mid - lo * lh / 2 + 1, fg, stroke);
                }
            }
        }
        var v = e.Preview ?? compositor.CreateSpriteVisual();
        var brush = compositor.CreateSurfaceBrush(surface);
        brush.Stretch = CompositionStretch.Fill;
        v.Brush = brush;
        v.Opacity = faded ? 0.45f : 1;
        if (e.Preview is null)
        {
            e.Visual.Children.InsertAtTop(v);
            e.Preview = v;
        }
        PlaceFace(e);
    }

    /// <summary>The tags and marks again when the scene has any (or had: they go).</summary>
    void NoteTagsAndMarks()
    {
        if (Model.Nets.Count > 0 || tagVisuals.Count > 0) PlaceTags();
        if (Model.Marks.Count > 0 || markVisuals.Count > 0) PlaceMarks();
    }

    /// <summary>Every mark again (a theme, a run's marks).</summary>
    public void PlaceMarks()
    {
        foreach (var nid in markVisuals.Keys.Where(k => Model.Marks[k] is null).ToList())
            if (markVisuals.Remove(nid, out var gone)) { TagLayer().Children.Remove(gone); gone.Dispose(); }
        foreach (var m in Model.Marks.Items) PlaceMark(m);
    }

    SpriteVisual Pill(string label, float w, float h, Color fill, Color stroke) => Label(new Vector2(w, h), ds =>
    {
        ds.FillRoundedRectangle(new WRect(0.5, 0.5, w - 1, h - 1), h / 2, h / 2, fill);
        ds.DrawRoundedRectangle(new WRect(0.5, 0.5, w - 1, h - 1), h / 2, h / 2, stroke, 1);
        text.Draw(ds, label, TextStyle.Caption, Colors.Text, new WRect(6, 0, w - 12, h));
    });

    /// <summary>What the pointer is over (a mouse or pen hovering, no button down): a ring on a pin, a wire
    /// drawn wider; nothing for anything else.</summary>
    public void Hover(PlumHit? h)
    {
        if (!WireLayers)
        {
            var want = h is { Kind: 3 } hw0 ? hw0.Wire : h is { Kind: 4 } hn && hn.Wire == hoverWire ? hoverWire : null;
            if (want != hoverWire)
            {
                hoverWire = want;
                ShowHoverWire();
            }
        }
        else
        {
        if (hoverWire is { } old && (h is not { Kind: 3 or 4 } x || x.Wire != old))
        {
            hoverWire = null;
            if (wires.TryGetValue(old, out var ow)) ow.Shape.StrokeThickness = WireWidth;
        }
        if (h is { Kind: 3 } hw && wires.TryGetValue(hw.Wire, out var we))
        {
            hoverWire = hw.Wire;
            we.Shape.StrokeThickness = WireWidth * 1.8f;
        }
        }
        if (h is { Kind: 2 } hp)
        {
            if (hoverRing is null)
            {
                var g = compositor.CreateEllipseGeometry();
                g.Radius = new Vector2(8, 8);
                g.Center = new Vector2(10, 10);
                var s = compositor.CreateSpriteShape(g);
                s.StrokeThickness = 2;
                hoverRing = compositor.CreateShapeVisual();
                hoverRing.Size = new Vector2(20, 20);
                hoverRing.Shapes.Add(s);
                overlayLayer.Children.InsertAtTop(hoverRing);
            }
            ((CompositionSpriteShape)hoverRing.Shapes[0]).StrokeBrush = Brush(Colors.Accent);
            hoverRing.Offset = new Vector3(hp.X - 10, hp.Y - 10, 0);
            hoverRing.IsVisible = true;
        }
        else if (hoverRing is not null) hoverRing.IsVisible = false;
        Hovered = h is { } k ? k.KindName : null;
    }

    /// <summary>The faces a marquee touches now (the core's `touches`), each with a light ring; none to clear.</summary>
    public void ShowTouches(IReadOnlyCollection<string>? ids)
    {
        var want = new HashSet<ulong>((ids ?? []).Select(NidOf).OfType<ulong>());
        foreach (var nid in touchRings.Keys.Where(n => !want.Contains(n)).ToList())
        {
            if (touchRings.Remove(nid, out var r) && faces.TryGetValue(nid, out var fe)) fe.Visual.Children.Remove(r);
            r?.Dispose();
        }
        foreach (var nid in want)
        {
            if (touchRings.ContainsKey(nid) || !faces.TryGetValue(nid, out var e) || e.Layout is not { } L) continue;
            const float m = FaceMetrics.Margin;
            var g = compositor.CreateRoundedRectangleGeometry();
            g.CornerRadius = new Vector2(FaceMetrics.Corner + 3, FaceMetrics.Corner + 3);
            g.Offset = new Vector2(m - 3.5f, m - 3.5f);
            g.Size = new Vector2(L.W + 7, L.H + 7);
            var s = compositor.CreateSpriteShape(g);
            s.StrokeThickness = 2f;
            s.StrokeBrush = Brush(CanvasColors.WithAlpha(Colors.Accent, 0.55));
            s.FillBrush = Brush(CanvasColors.WithAlpha(Colors.Accent, 0.06));
            var v = compositor.CreateShapeVisual();
            v.Size = e.Visual.Size;
            v.Shapes.Add(s);
            e.Visual.Children.InsertAtTop(v);
            touchRings[nid] = v;
        }
    }
}
