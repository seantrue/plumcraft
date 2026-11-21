// A node face laid out from the scene's rows in graph units at zoom 1 (contract 8.11: the shell
// measures), with DirectWrite through Win2D's CanvasTextLayout, and drawn from the same plan, so
// what is drawn, measured, reported (plum_geometry_set) and hit-tested in the core agree.
// A simplified port of the Swift kit's FaceLayout (same constants), for the spike.

using System.Numerics;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Geometry;
using Microsoft.Graphics.Canvas.Text;
using Windows.Foundation;
using Microsoft.UI;
using Windows.UI;

namespace PlumCraft.CanvasSpike;

public static class M
{
    public const float Header = 28, Row = 22, PinRadius = 4.5f, Corner = 6, MinWidth = 140, MaxWidth = 440, TitleMax = 230, StatusSlot = 88, FooterLine = 15;
}

public sealed class FaceLayout
{
    public float W, H;
    public float[] InPins = [], OutPins = [];
    public float[] Widgets = []; // 4 floats an input row; NaN x0: none
    public float TitleX = 24;
    public float InW, OutW;
}

/// <summary>Text measured and drawn with DirectWrite, measurements cached by text.</summary>
public sealed class Text
{
    public readonly CanvasTextFormat Title = new() { FontFamily = "Segoe UI Variable Text", FontSize = 14, FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, WordWrapping = CanvasWordWrapping.NoWrap, VerticalAlignment = CanvasVerticalAlignment.Center };
    public readonly CanvasTextFormat Port = new() { FontFamily = "Segoe UI Variable Text", FontSize = 13, WordWrapping = CanvasWordWrapping.NoWrap, VerticalAlignment = CanvasVerticalAlignment.Center };
    public readonly CanvasTextFormat Small = new() { FontFamily = "Segoe UI Variable Text", FontSize = 12, WordWrapping = CanvasWordWrapping.NoWrap, VerticalAlignment = CanvasVerticalAlignment.Center };
    readonly Dictionary<(string, int), float> widths = [];
    readonly CanvasDevice device = CanvasDevice.GetSharedDevice();
    public long Measured;
    public long MeasureTicks;

    public float Width(string s, CanvasTextFormat f)
    {
        var key = (s, f == Title ? 0 : f == Port ? 1 : 2);
        lock (widths)
        {
            if (widths.TryGetValue(key, out var w)) return w;
        }
        var t0 = System.Diagnostics.Stopwatch.GetTimestamp();
        using var layout = new CanvasTextLayout(device, s, f, 10000, 100);
        var width = (float)layout.LayoutBounds.Width;
        Interlocked.Add(ref MeasureTicks, System.Diagnostics.Stopwatch.GetTimestamp() - t0);
        Interlocked.Increment(ref Measured);
        lock (widths) widths[key] = width;
        return width;
    }
}

public static class FacePlan
{
    public static string Caption(Row r) => r.Source == "own" ? r.Label : r.Text ?? r.Label;

    public static FaceLayout Lay(Face f, Text t)
    {
        var l = new FaceLayout();
        var titleW = Math.Min(t.Width(f.Title, t.Title), M.TitleMax);
        var statusW = f.Status.Sum(s => t.Width(s, t.Small) + 6);
        var headerW = 24 + titleW + 8 + Math.Max(M.StatusSlot, statusW) + 30;
        float inW = 0;
        var labelW = new float[f.Inputs.Count];
        var widgetW = new float[f.Inputs.Count];
        for (int i = 0; i < f.Inputs.Count; i++)
        {
            var r = f.Inputs[i];
            labelW[i] = t.Width(Caption(r), t.Port);
            widgetW[i] = r.Source == "own" && r.Value != null ? Math.Clamp(t.Width(r.Value, t.Small) + 12, 44, 160) : 0;
            inW = Math.Max(inW, 14 + labelW[i] + (widgetW[i] > 0 ? widgetW[i] + 8 : 0) + 8);
        }
        float outW = 0;
        foreach (var r in f.Outputs) outW = Math.Max(outW, 8 + t.Width(r.Label, t.Port) + 14);
        var rowsW = inW + outW + (inW > 0 && outW > 0 ? 12 : 0);
        var footW = f.Footer.Count > 0 ? f.Footer.Max(s => t.Width(s, t.Small)) + 16 : 0;
        l.W = Math.Min(M.MaxWidth, Math.Max(M.MinWidth, Math.Max(headerW, Math.Max(rowsW, Math.Min(footW, 300)))));
        l.InW = inW; l.OutW = outW;
        var top = M.Header + 4;
        l.InPins = new float[f.Inputs.Count];
        l.Widgets = new float[f.Inputs.Count * 4];
        for (int i = 0; i < f.Inputs.Count; i++)
        {
            var y = top + i * M.Row;
            l.InPins[i] = y + M.Row / 2;
            if (widgetW[i] > 0)
            {
                var wx = Math.Min(14 + labelW[i] + 8, Math.Max(l.W - widgetW[i] - 8, 14));
                l.Widgets[i * 4] = wx; l.Widgets[i * 4 + 1] = y + 2; l.Widgets[i * 4 + 2] = wx + widgetW[i]; l.Widgets[i * 4 + 3] = y + M.Row - 2;
            }
            else
            {
                l.Widgets[i * 4] = float.NaN;
            }
        }
        l.OutPins = new float[f.Outputs.Count];
        for (int i = 0; i < f.Outputs.Count; i++) l.OutPins[i] = top + i * M.Row + M.Row / 2;
        var bottom = Math.Max(top + f.Inputs.Count * M.Row, top + f.Outputs.Count * M.Row);
        if (f.Inputs.Count == 0 && f.Outputs.Count == 0) bottom = M.Header;
        bottom += Math.Min(f.Footer.Count, 3) * M.FooterLine + (f.Footer.Count > 0 ? 6 : 0);
        l.H = bottom + 6;
        return l;
    }

    static Color RunColor(SceneModel s, string? run) => run switch
    {
        "done" => s.Token("state_done", Colors.Green),
        "cached" => s.Token("state_cached", Colors.Teal),
        "error" => s.Token("state_error", Colors.Red),
        "started" or "progress" or "preview" => s.Token("state_running", Colors.Orange),
        "queued" => s.Token("state_queued", Colors.Gray),
        _ => Color.FromArgb(0, 0, 0, 0),
    };

    /// <summary>Draw a face at the origin of the session's transform (its top left).</summary>
    public static void Draw(CanvasDrawingSession ds, Face f, SceneModel s, Text t)
    {
        var l = f.Layout ?? throw new InvalidOperationException("a face drawn before it was laid out");
        var rect = new Rect(0, 0, l.W, l.H);
        ds.FillRoundedRectangle(rect, M.Corner, M.Corner, f.Body);
        using (var hdr = CanvasGeometry.CreateRoundedRectangle(ds, rect, M.Corner, M.Corner))
        using (var clip = ds.CreateLayer(1f, new Rect(0, 0, l.W, M.Header)))
            ds.FillGeometry(hdr, f.Header);
        ds.DrawRoundedRectangle(rect, M.Corner, M.Corner, f.Selected ? s.Token("accent", Colors.DodgerBlue) : f.Border, f.Selected ? 2.5f : Math.Max(1, f.BorderWidth));
        var dot = RunColor(s, f.Run);
        if (dot.A > 0) ds.FillCircle(13, M.Header / 2, 4, dot);
        var text = s.Token("header_text", Colors.Black);
        ds.DrawText(f.Title, new Rect(l.TitleX, 0, M.TitleMax, M.Header), text, t.Title);
        // the more-button (the node's menu)
        for (int k = -1; k <= 1; k++) ds.FillCircle(l.W - 15, M.Header / 2 + 4 * k, 1.4f, text);
        float sx = l.W - 28;
        foreach (var st in Enumerable.Reverse(f.Status))
        {
            var w = t.Width(st, t.Small);
            ds.DrawText(st, new Rect(sx - w, 0, w + 2, M.Header), s.Token("text_dim", Colors.DimGray), t.Small);
            sx -= w + 6;
        }
        var body = s.Token("text", Colors.Black);
        var dim = s.Token("text_dim", Colors.DimGray);
        var field = s.Token("field", Colors.White);
        var fieldBorder = s.Token("field_border", Colors.Gray);
        var top = M.Header + 4;
        for (int i = 0; i < f.Inputs.Count; i++)
        {
            var r = f.Inputs[i];
            var y = top + i * M.Row;
            ds.DrawText(Caption(r), new Rect(14, y, l.W - 28, M.Row), r.Source == "own" ? body : dim, t.Port);
            if (!float.IsNaN(l.Widgets[i * 4]) && r.Value != null)
            {
                var wr = new Rect(l.Widgets[i * 4], l.Widgets[i * 4 + 1], l.Widgets[i * 4 + 2] - l.Widgets[i * 4], l.Widgets[i * 4 + 3] - l.Widgets[i * 4 + 1]);
                ds.FillRoundedRectangle(wr, 3, 3, field);
                ds.DrawRoundedRectangle(wr, 3, 3, fieldBorder, 1);
                ds.DrawText(r.Value, new Rect(wr.X + 5, wr.Y + 1, wr.Width - 8, wr.Height), body, t.Small);
            }
            Pin(ds, 0, l.InPins[i], r);
        }
        for (int i = 0; i < f.Outputs.Count; i++)
        {
            var r = f.Outputs[i];
            var y = top + i * M.Row;
            var w = t.Width(r.Label, t.Port);
            ds.DrawText(r.Label, new Rect(l.W - 14 - w, y, w + 4, M.Row), body, t.Port);
            Pin(ds, l.W, l.OutPins[i], r);
        }
        var fy = Math.Max(top + f.Inputs.Count * M.Row, top + f.Outputs.Count * M.Row) + 2;
        foreach (var line in f.Footer.Take(3))
        {
            ds.DrawText(line, new Rect(8, fy, l.W - 16, M.FooterLine), dim, t.Small);
            fy += M.FooterLine;
        }
    }

    static void Pin(CanvasDrawingSession ds, float x, float y, Row r)
    {
        if (r.Square) ds.FillRectangle(x - M.PinRadius, y - M.PinRadius, 2 * M.PinRadius, 2 * M.PinRadius, r.Pin);
        else ds.FillCircle(x, y, M.PinRadius, r.Pin);
    }

    /// <summary>A wire's cubic chain (start, then three points a segment) as a geometry.</summary>
    public static CanvasGeometry? WireGeometry(ICanvasResourceCreator rc, Vector2[] p, Vector2 offset = default)
    {
        if (p.Length < 4) return null;
        using var pb = new CanvasPathBuilder(rc);
        pb.BeginFigure(p[0] + offset);
        for (int i = 1; i + 2 < p.Length; i += 3) pb.AddCubicBezier(p[i] + offset, p[i + 1] + offset, p[i + 2] + offset);
        pb.EndFigure(CanvasFigureLoop.Open);
        return CanvasGeometry.CreatePath(pb);
    }

    public static Rect Bounds(Vector2[] p)
    {
        if (p.Length == 0) return Rect.Empty;
        float x0 = p[0].X, y0 = p[0].Y, x1 = x0, y1 = y0;
        foreach (var q in p) { x0 = Math.Min(x0, q.X); y0 = Math.Min(y0, q.Y); x1 = Math.Max(x1, q.X); y1 = Math.Max(y1, q.Y); }
        return new Rect(x0, y0, x1 - x0, y1 - y0);
    }
}
