// Drawing a face once, with Win2D, from its scene item and its layout: the Swift kit's FaceRenderer.
// It draws into a Composition drawing surface on a worker (faces are drawn concurrently; Win2D's device
// is free-threaded), at a scale of pixels per graph unit; the picture reaches FaceMetrics.Margin past
// the face's rect on every side, for the pins on its edges and its border.
//
// Not drawn here, so a change to them never redraws a face: the selection, the run wash (both child
// visuals) and the preview's picture (a child visual whose picture comes from the media service).

using System.Numerics;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Geometry;
using Microsoft.Graphics.Canvas.Svg;
using Microsoft.UI;
using PlumCraft.Models;
using Windows.UI;
using WRect = Windows.Foundation.Rect;
using MRect = PlumCraft.Models.Rect;

namespace PlumCraft.App.Canvas;

public static class FaceRenderer
{
    static int iconFailureNoted;

    /// <summary>Draw `face` laid out as `L` with its top left at the session's origin (graph units).</summary>
    public static void Draw(CanvasDrawingSession ds, Face face, FaceLayout L, CanvasColors C, FaceText T, string? iconSvg)
    {
        var rect = new WRect(0, 0, L.W, L.H);
        const float r = FaceMetrics.Corner;
        // a soft shadow (an offset fill: a blur would cost every face a pass on a software rasteriser)
        ds.FillRoundedRectangle(new WRect(0.5, 1.5, L.W, L.H), r, r, Color.FromArgb((byte)(C.IsDark ? 90 : 36), 0, 0, 0));
        ds.FillRoundedRectangle(rect, r, r, C.Of(face.Tint.Body));
        using (var body = CanvasGeometry.CreateRoundedRectangle(ds, rect, r, r))
        using (ds.CreateLayer(1f, body))
        {
            ds.FillRectangle(new WRect(0, 0, L.W, L.Header), C.Of(face.Tint.Header));
            if (!L.Collapsed) ds.FillRectangle(new WRect(0, L.Header - 0.5, L.W, 1), CanvasColors.WithAlpha(C.Separator, 0.6));
        }
        // the border: the core's (highlight, proxy, stale, run state, watched), 1.5 wider when watched
        if (face.Border is { } b)
        {
            var w = (float)b.Width + (face.Watched ? 1.5f : 0);
            var inset = -w / 2 + 0.5;
            ds.DrawRoundedRectangle(new WRect(inset, inset, L.W - 2 * inset, L.H - 2 * inset), r + w / 2, r + w / 2, C.Of(b.Paint), w);
        }
        else
        {
            ds.DrawRoundedRectangle(rect, r, r, C.FieldBorder, 1);
        }
        Header(ds, face, L, C, T, iconSvg);
        if (!L.Collapsed)
        {
            Rows(ds, face, L, C, T);
            if (L.Preview is { } p) ds.FillRoundedRectangle(p, 3, 3, C.Placeholder);
            Footer(ds, face, L, C, T);
        }
        Pins(ds, face, L, C);
    }

    static void Header(CanvasDrawingSession ds, Face face, FaceLayout L, CanvasColors C, FaceText T, string? iconSvg)
    {
        var h = face.Header;
        var ink = C.HeaderText;
        var mid = L.Header / 2;
        if (L.Chevron is { } cx)
        {
            // ▾ open, ▸ closed
            using var pb = new CanvasPathBuilder(ds);
            if (h.Open)
            {
                pb.BeginFigure(cx - 4, mid - 2); pb.AddLine(cx + 4, mid - 2); pb.AddLine(cx, mid + 3);
            }
            else
            {
                pb.BeginFigure(cx - 2, mid - 4); pb.AddLine(cx + 3, mid); pb.AddLine(cx - 2, mid + 4);
            }
            pb.EndFigure(CanvasFigureLoop.Closed);
            using var g = CanvasGeometry.CreatePath(pb);
            ds.FillGeometry(g, CanvasColors.WithAlpha(ink, 0.55));
        }
        var spot = L.Spot;
        var sc = new Vector2((float)(spot.X + spot.Width / 2), (float)(spot.Y + spot.Height / 2));
        switch (h.State.KindWord)
        {
            case SpotWord.Dot:
                ds.FillCircle(sc, 4, h.State.Paint is { } dp ? C.Of(dp) : C.TextDim);
                break;
            case SpotWord.Spinner:
                using (var pb = new CanvasPathBuilder(ds))
                {
                    pb.BeginFigure(sc + new Vector2(4, 0));
                    pb.AddArc(sc, 4, 4, 0, MathF.PI * 1.5f);
                    pb.EndFigure(CanvasFigureLoop.Open);
                    using var g = CanvasGeometry.CreatePath(pb);
                    ds.DrawGeometry(g, h.State.Paint is { } sp ? C.Of(sp) : C.Accent, 1.6f);
                }
                break;
        }
        if (L.Icon is { } ir && iconSvg is not null) DrawIcon(ds, iconSvg, ir, CanvasColors.WithAlpha(ink, 0.85));
        T.Draw(ds, L.Title.Text, TextStyle.Title, ink, new WRect(L.Title.X, 0, L.Title.Max, L.Header));
        for (int k = 0; k < L.Badges.Count && k < h.Badges.Count; k++)
        {
            var (text, br) = L.Badges[k];
            var col = C.Of(h.Badges[k].Paint);
            ds.DrawRoundedRectangle(br, 8, 8, col, 1);
            T.Draw(ds, text, TextStyle.Small, col, new WRect(br.X + 5, br.Y, br.Width - 6, br.Height));
        }
        for (int k = 0; k < L.Status.Count && k < h.Status.Count; k++)
        {
            var s = L.Status[k];
            T.Draw(ds, s.Text, s.Style, C.Of(h.Status[k].Paint), new WRect(s.X - 300, 0, 300, L.Header), Align.Right);
        }
        if (L.Engine is { } e) T.Draw(ds, e.Text, TextStyle.Small, C.TextDim, new WRect(e.X - 300, 0, 300, L.Header), Align.Right);
        if (L.Enter is { } en)
        {
            // a pencil: enter the group
            var pen = CanvasColors.WithAlpha(ink, 0.7);
            ds.DrawLine((float)en.X + 3, (float)(en.Y + en.Height) - 3, (float)(en.X + en.Width) - 3, (float)en.Y + 3, pen, 1.4f);
            ds.DrawLine((float)en.X + 3, (float)(en.Y + en.Height) - 3, (float)en.X + 6, (float)(en.Y + en.Height) - 3, pen, 1.4f);
        }
        // the more-button (the node's menu): three dots
        var m = L.More;
        var mc = new Vector2((float)(m.X + m.Width / 2), (float)(m.Y + m.Height / 2));
        for (int k = -1; k <= 1; k++) ds.FillCircle(mc + new Vector2(0, 4.2f * k), 1.4f, CanvasColors.WithAlpha(ink, 0.7));
    }

    /// <summary>A Lucide icon (the core's SVG, stroked in `currentColor`) in its box.</summary>
    static void DrawIcon(CanvasDrawingSession ds, string svg, WRect box, Color ink)
    {
        try
        {
            var hex = $"#{ink.R:x2}{ink.G:x2}{ink.B:x2}";
            using var doc = CanvasSvgDocument.LoadFromXml(ds, svg.Replace("currentColor", hex, StringComparison.Ordinal));
            // Lucide's box is 24 units, and its root says so (width and height): scaled into the icon's box
            var t = ds.Transform;
            ds.Transform = Matrix3x2.CreateScale((float)box.Width / 24, (float)box.Height / 24) * Matrix3x2.CreateTranslation((float)box.X, (float)box.Y) * t;
            ds.DrawSvg(doc, new Windows.Foundation.Size(24, 24));
            ds.Transform = t;
        }
        catch (Exception e)
        {
            // an icon Direct2D can't read is left out (the core's SVG is hostile input like the rest)
            if (Interlocked.Exchange(ref iconFailureNoted, 1) == 0) CoreHost.Log($"an icon Direct2D can't draw: {e.Message}");
        }
    }

    static void Rows(CanvasDrawingSession ds, Face face, FaceLayout L, CanvasColors C, FaceText T)
    {
        // FR5: a promoted row's source node, as a section header: its title and a rule
        foreach (var (text, rect) in L.Sections)
        {
            T.Draw(ds, text, TextStyle.Small, C.TextDim, new WRect(10, rect.Y, rect.Width - 20, rect.Height));
            var tw = Math.Min(T.Width(text, TextStyle.Small), (float)rect.Width - 20);
            if (rect.Width - tw - 26 > 8)
                ds.DrawLine(10 + tw + 6, (float)rect.Y + (float)rect.Height / 2, (float)rect.Width - 10, (float)rect.Y + (float)rect.Height / 2, C.TextDim, 0.5f);
        }
        for (int k = 0; k < face.Inputs.Count && k < L.InputRows.Length; k++)
        {
            var r = face.Inputs[k];
            var row = L.InputRows[k];
            var line = new WRect(0, row.Y, 0, Math.Min(row.Height, FaceMetrics.Row));
            float x = 14;
            if (r.Named is { } n)
            {
                var s = n.Name + " →";
                T.Draw(ds, s, TextStyle.Small, C.Of(n.Paint), new WRect(x, line.Y, 200, line.Height));
                x += T.Width(s, TextStyle.Small) + 6;
            }
            var caption = FaceLayout.InputCaption(r);
            var style = r.SourceWord == RowSourceWord.Inherited ? TextStyle.Italic : TextStyle.Port;
            var lim = (float)((L.Widgets[k]?.X ?? L.W - 8) - x - 6);
            // a row that opens its node's value editor reads as a link, in the accent colour (FR1, 8.4)
            var captionInk = r.Editor is not null ? C.Accent : r.Dim ? C.TextDim : C.Text;
            T.Draw(ds, caption, style, captionInk, new WRect(x, line.Y, Math.Max(lim, 20), line.Height));
            if (r.Badge is { } bd)
            {
                var bx = Math.Min(x + T.Width(caption, style) + 6, L.W - 40);
                var br = new WRect(bx, row.Y + 3, T.Width(bd.Text, TextStyle.Small) + 8, 16);
                ds.DrawRoundedRectangle(br, 4, 4, C.Of(bd.Paint), 1);
                T.Draw(ds, bd.Text, TextStyle.Small, C.Of(bd.Paint), new WRect(br.X + 4, br.Y, br.Width - 4, br.Height));
            }
            if (L.Widgets[k] is { } wr && r.Widget is { } w) Widget(ds, w, r, wr, C, T);
        }
        for (int k = 0; k < face.Outputs.Count && k < L.OutputRows.Length; k++)
        {
            var row = L.OutputRows[k];
            T.Draw(ds, face.Outputs[k].Label, TextStyle.Port, C.Text, new WRect(row.X + 6, row.Y, row.Width - 20, row.Height), Align.Right);
        }
    }

    static void Widget(CanvasDrawingSession ds, Widget w, InRow r, WRect rect, CanvasColors C, FaceText T)
    {
        var value = FaceLayout.WidgetText(w);
        var ink = w.Blank ? C.TextFaint : C.Text;
        switch (w.KindWord)
        {
            case WidgetWord.Toggle:
                var on = w.Value.ValueKind == System.Text.Json.JsonValueKind.True;
                ds.FillRoundedRectangle(rect, 3, 3, on ? C.Accent : C.Field);
                ds.DrawRoundedRectangle(rect, 3, 3, C.FieldBorder, 1);
                if (on)
                {
                    using var pb = new CanvasPathBuilder(ds);
                    pb.BeginFigure((float)rect.X + 3, (float)(rect.Y + rect.Height / 2));
                    pb.AddLine((float)rect.X + 6, (float)(rect.Y + rect.Height) - 3.5f);
                    pb.AddLine((float)(rect.X + rect.Width) - 3, (float)rect.Y + 3.5f);
                    pb.EndFigure(CanvasFigureLoop.Open);
                    using var g = CanvasGeometry.CreatePath(pb);
                    ds.DrawGeometry(g, Colors.White, 1.8f);
                }
                return;
            case WidgetWord.Color:
                ds.FillRoundedRectangle(rect, 3, 3, FaceLayout.WidgetColor(w) ?? C.Field);
                ds.DrawRoundedRectangle(rect, 3, 3, C.FieldBorder, 1);
                return;
            case WidgetWord.Editor:
                ds.FillRoundedRectangle(rect, 3, 3, C.Placeholder);
                ds.DrawRoundedRectangle(rect, 3, 3, C.FieldBorder, 1);
                T.Draw(ds, r.Label, TextStyle.Small, C.Text, rect, Align.Centre);
                return;
        }
        ds.FillRoundedRectangle(rect, 3, 3, C.Field);
        ds.DrawRoundedRectangle(rect, 3, 3, C.FieldBorder, 1);
        var inner = new WRect(rect.X + 5, rect.Y, Math.Max(rect.Width - 10, 1), rect.Height);
        switch (w.KindWord)
        {
            case WidgetWord.Number:
                T.Draw(ds, value, TextStyle.Mono, ink, new WRect(rect.X + 2, rect.Y, Math.Max(rect.Width - 8, 1), rect.Height), Align.Right);
                break;
            case WidgetWord.Choice:
                T.Draw(ds, value, TextStyle.Small, ink, new WRect(rect.X + 6, rect.Y, Math.Max(rect.Width - 20, 1), rect.Height));
                var c = new Vector2((float)(rect.X + rect.Width) - 8, (float)(rect.Y + rect.Height / 2));
                ds.DrawLine(c + new Vector2(-3, -1.5f), c + new Vector2(0, 1.5f), C.TextDim, 1.2f);
                ds.DrawLine(c + new Vector2(0, 1.5f), c + new Vector2(3, -1.5f), C.TextDim, 1.2f);
                break;
            case WidgetWord.Multiline:
                var lines = value.Split('\n').SelectMany(l => { var x = T.Wrap(l, TextStyle.Mono, (float)rect.Width - 10, 3); return x.Count == 0 ? [""] : x; }).Take(3).ToList();
                for (int i = 0; i < lines.Count; i++) T.Draw(ds, lines[i], TextStyle.Mono, ink, new WRect(rect.X + 5, rect.Y + 2 + i * 14, rect.Width - 10, 14));
                break;
            case WidgetWord.Path when PathBrowse.Offered(w):
                {
                    // the path's text, a rule, and Browse in the square at the right end (B133); a tap in the
                    // square is the core's `row.browse`
                    var sq = PathBrowse.Square(new MRect(rect.X, rect.Y, rect.X + rect.Width, rect.Y + rect.Height));
                    T.Draw(ds, value, TextStyle.Small, ink, new WRect(rect.X + 5, rect.Y, Math.Max(sq.X0 - rect.X - 8, 1), rect.Height));
                    ds.DrawLine((float)sq.X0, (float)rect.Y + 3, (float)sq.X0, (float)(rect.Y + rect.Height) - 3, C.FieldBorder, 1);
                    T.Draw(ds, "…", TextStyle.Small, C.Accent, new WRect(sq.X0, sq.Y0, sq.Width, sq.Height), Align.Centre);
                    break;
                }
            case WidgetWord.Path:
                T.Draw(ds, value, TextStyle.Small, ink, new WRect(rect.X + 5, rect.Y, Math.Max(rect.Width - 22, 1), rect.Height));
                T.Draw(ds, "…", TextStyle.Small, C.TextDim, new WRect(rect.X, rect.Y, rect.Width - 5, rect.Height), Align.Right);
                break;
            default:
                T.Draw(ds, value, TextStyle.Small, ink, inner);
                break;
        }
    }

    static void Footer(CanvasDrawingSession ds, Face face, FaceLayout L, CanvasColors C, FaceText T)
    {
        var items = face.Footer.Where(f => f.KindWord is not null).ToList();
        for (int k = 0; k < L.Footer.Count && k < items.Count; k++)
        {
            var plan = L.Footer[k];
            var item = items[k];
            var y = (float)plan.Rect.Y;
            var ink = C.TextDim;
            switch (item.KindWord)
            {
                case FooterWord.Error: ink = C.Of(item.Paint); break;
                case FooterWord.Excerpt: ink = item.Faded == true ? C.TextFaint : C.Text; break;
                case FooterWord.Convert:
                    var ls = item.Lines ?? [];
                    for (int i = 0; i < ls.Count; i++)
                        T.Draw(ds, ls[i].Text, TextStyle.Small, C.Of(ls[i].Paint), new WRect(plan.Rect.X, y + i * FaceMetrics.FooterLine, plan.Rect.Width, FaceMetrics.FooterLine));
                    continue;
                case FooterWord.Fields:
                case FooterWord.Save:
                case FooterWord.Editor:
                    ink = C.Accent; break;
            }
            if (plan.Progress is { } p)
            {
                var bar = new WRect(plan.Rect.X, y + 4, plan.Rect.Width, 5);
                ds.FillRoundedRectangle(bar, 2.5f, 2.5f, C.Field);
                ds.FillRoundedRectangle(new WRect(bar.X, bar.Y, bar.Width * Math.Clamp(p, 0, 1), bar.Height), 2.5f, 2.5f, C.Token("state_running", Color.FromArgb(255, 66, 165, 245)));
                y += FaceMetrics.FooterLine * 0.6f;
            }
            for (int i = 0; i < plan.Lines.Count; i++)
                T.Draw(ds, plan.Lines[i], plan.Mono ? TextStyle.Mono : TextStyle.Small, ink, new WRect(plan.Rect.X, y + i * FaceMetrics.FooterLine, plan.Rect.Width, FaceMetrics.FooterLine));
        }
    }

    static void Pins(CanvasDrawingSession ds, Face face, FaceLayout L, CanvasColors C)
    {
        void DrawPin(Pin p, float x, float y)
        {
            if (!p.Wireable) return;
            const float r = FaceMetrics.PinRadius;
            var fill = C.Of(p.Fill);
            if (p.ShapeWord == PinShapeWord.Square)
            {
                var box = new WRect(x - r + 0.5, y - r + 0.5, 2 * r - 1, 2 * r - 1);
                ds.FillRectangle(box, fill);
                ds.DrawRectangle(box, C.Canvas, 1.2f);
            }
            else
            {
                ds.FillCircle(x, y, r, fill);
                ds.DrawCircle(x, y, r, C.Canvas, 1.2f);
            }
        }
        for (int k = 0; k < face.Inputs.Count && k < L.InputPins.Length; k++) DrawPin(face.Inputs[k].Pin, 0, L.InputPins[k]);
        for (int k = 0; k < face.Outputs.Count && k < L.OutputPins.Length; k++) DrawPin(face.Outputs[k].Pin, L.W, L.OutputPins[k]);
    }

    /// <summary>A wire's cubic chain (the start, then two control points and the end a segment) as a
    /// geometry, moved by `offset`; null for fewer than one segment.</summary>
    public static CanvasGeometry? WireGeometry(ICanvasResourceCreator rc, IReadOnlyList<Point> p, Vector2 offset)
    {
        if (p.Count < 4) return null;
        using var pb = new CanvasPathBuilder(rc);
        Vector2 V(Point q) => new Vector2((float)q.X, (float)q.Y) + offset;
        pb.BeginFigure(V(p[0]));
        for (int i = 1; i + 2 < p.Count; i += 3) pb.AddCubicBezier(V(p[i]), V(p[i + 1]), V(p[i + 2]));
        pb.EndFigure(CanvasFigureLoop.Open);
        return CanvasGeometry.CreatePath(pb);
    }

    /// <summary>The bounds of a list of points.</summary>
    public static WRect Bounds(IReadOnlyList<Point> p)
    {
        if (p.Count == 0) return WRect.Empty;
        double x0 = p[0].X, y0 = p[0].Y, x1 = x0, y1 = y0;
        foreach (var q in p) { x0 = Math.Min(x0, q.X); y0 = Math.Min(y0, q.Y); x1 = Math.Max(x1, q.X); y1 = Math.Max(y1, q.Y); }
        return new WRect(x0, y0, x1 - x0, y1 - y0);
    }
}
