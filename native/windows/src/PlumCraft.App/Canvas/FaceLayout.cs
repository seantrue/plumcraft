// A node face laid out from the scene's rows, in graph units at zoom 1 (contract 8.11: the shell
// measures): the Swift kit's FaceLayout (PlumCraftCanvas) with the same constants, so a face is the
// same size on every shell. Inputs in a column on the left with their pins on the left edge, outputs on
// the right with theirs on the right edge; then the preview and the footer.
//
// Pure: a face is laid out on a worker when its `rev` changes, never per frame. FaceRenderer draws from
// the same plan, so what is drawn, measured and reported (and so hit-tested in the core) agree.

using System.Text.Json;
using PlumCraft.Core;
using PlumCraft.Models;
using WRect = Windows.Foundation.Rect;
using MRect = PlumCraft.Models.Rect;

namespace PlumCraft.App.Canvas;

public static class FaceMetrics
{
    public const float Header = 28, Row = 22, MultilineRow = 56, PinRadius = 4.5f, Corner = 6, MinWidth = 140, MaxWidth = 440,
        TitleMax = 230, StatusSlot = 88, FooterLine = 15, Icon = 14, More = 18,
        /// <summary>A section header above a group's promoted rows (FR5): the source node's title.</summary>
        Section = 18;
    /// <summary>The face's picture reaches this far past its rect on every side (pins, border, shadow).</summary>
    public const float Margin = 8;
}

public sealed record PlacedText(string Text, float X, TextStyle Style, float Max, bool Right = false);

public sealed record FooterPlan(WRect Rect, List<string> Lines, bool Mono, double? Progress)
{
    /// <summary>The footer item's kind and port (a `save` item is Save Result…, clicked through the core's command).</summary>
    public string Kind { get; init; } = "";
    public string? Port { get; init; }
    public bool Sheet { get; init; }
    /// <summary>Its index in the scene's footer list (`parts.footer[k]`).</summary>
    public int Index { get; init; }
}

public sealed class FaceLayout
{
    public float W, H, Header = FaceMetrics.Header;
    public bool Collapsed;
    public float[] InputPins = [], OutputPins = [];
    public WRect[] InputRows = [], OutputRows = [];
    public WRect?[] Widgets = [];
    /// <summary>Each input row's caption rect where a tap on the row opens its node's value editor (FR1, 8.13: the
    /// row carries `editor` and has no widget); reported as the row's rect, so the core's hit test finds it.</summary>
    public WRect?[] Opens = [];
    public WRect? Preview;
    public List<FooterPlan> Footer = [];
    public float? Chevron;
    public WRect Spot, More;
    public WRect? Icon, Enter;
    public PlacedText Title = new("", 0, TextStyle.Title, 0);
    public float TitleWidth;
    public List<(string Text, WRect Rect)> Badges = [];
    /// <summary>FR5: each section header above a promoted row (its text and rect, face coordinates).</summary>
    public List<(string Text, WRect Rect)> Sections = [];
    public List<PlacedText> Status = [];
    public PlacedText? Engine;

    /// <summary>The header's parts the core hit-tests (geometry.parts): the title, the chevron, the icon,
    /// the more-button, the enter button, the preview.</summary>
    /// <summary>Each footer item's rect in the scene's order (null for one not drawn), reported as
    /// `parts.footer` so the core hit-tests a tap on a line: Save Result… is the core's save (B122).</summary>
    public List<MRect?> FooterParts(int count)
    {
        var o = Enumerable.Repeat<MRect?>(null, count).ToList();
        foreach (var f in Footer)
            if (f.Index >= 0 && f.Index < count) o[f.Index] = new MRect(f.Rect.X, f.Rect.Y, f.Rect.X + f.Rect.Width, f.Rect.Y + f.Rect.Height);
        return o;
    }

    public Dictionary<string, MRect> Parts()
    {
        static MRect R(WRect c) => new(c.X, c.Y, c.X + c.Width, c.Y + c.Height);
        var tw = Math.Min(TitleWidth, Title.Max > 0 ? Title.Max : FaceMetrics.TitleMax);
        var outp = new Dictionary<string, MRect>
        {
            ["title"] = R(new WRect(Title.X - 2, 4, tw + 4, Header - 8)),
            ["menu"] = R(More),
        };
        if (Chevron is { } c) outp["chevron"] = R(new WRect(c - 7, 4, 14, Header - 8));
        if (Icon is { } i) outp["icon"] = R(Inflate(i, 2));
        if (Enter is { } e) outp["enter"] = R(Inflate(e, 2));
        if (Preview is { } p) outp["preview"] = R(p);
        return outp;
    }

    static WRect Inflate(WRect r, double d) => new(r.X - d, r.Y - d, r.Width + 2 * d, r.Height + 2 * d);

    static WRect Union(WRect a, WRect b)
    {
        var x0 = Math.Min(a.X, b.X); var y0 = Math.Min(a.Y, b.Y);
        return new WRect(x0, y0, Math.Max(a.X + a.Width, b.X + b.Width) - x0, Math.Max(a.Y + a.Height, b.Y + b.Height) - y0);
    }

    /// <summary>What plum_geometry_set takes for this face.</summary>
    public PlumCore.FaceMeasure Measure(ulong nid, ulong rev)
    {
        var rows = new float[Widgets.Length * 4];
        for (int k = 0; k < Widgets.Length; k++)
        {
            // a row that opens its node's editor reports its caption (FR1), with its widget as one
            // rect when it has one (B173: the caption to the widget's end; the core keeps the right
            // end for a path's Browse button)
            var o = k < Opens.Length ? Opens[k] : null;
            if ((Widgets[k] is { } wd && o is { } oo ? Union(wd, oo) : Widgets[k] ?? o) is { } w)
            {
                rows[4 * k] = (float)w.X; rows[4 * k + 1] = (float)w.Y; rows[4 * k + 2] = (float)(w.X + w.Width); rows[4 * k + 3] = (float)(w.Y + w.Height);
            }
            else
            {
                rows[4 * k] = rows[4 * k + 1] = rows[4 * k + 2] = rows[4 * k + 3] = float.NaN;
            }
        }
        return new PlumCore.FaceMeasure(nid, rev, W, H, Header, InputPins, OutputPins, rows.Length > 0 ? rows : null);
    }

    // ---------------- making one ----------------

    public static FaceLayout Make(Face face, FaceText t)
    {
        var L = new FaceLayout();
        var h = face.Header;
        var collapsed = face.ViewWord == FaceViewWord.Collapsed;
        L.Collapsed = collapsed;
        const float M = FaceMetrics.Header;

        // the header, left to right: chevron, state spot, icon, title, badges; right: status, engine, enter, more
        float x = 8;
        L.Chevron = x + 4;
        x += 14;
        L.Spot = new WRect(x, M / 2 - 4, 8, 8);
        x += 14;
        L.Icon = new WRect(x, (M - FaceMetrics.Icon) / 2, FaceMetrics.Icon, FaceMetrics.Icon);
        x += FaceMetrics.Icon + 6;
        var titleW = Math.Min(t.Width(h.Title.Text, TextStyle.Title), FaceMetrics.TitleMax);
        var titleX = x;
        x += titleW + 8;
        foreach (var b in h.Badges)
        {
            var bw = t.Width(b.Text, TextStyle.Small) + 10;
            L.Badges.Add((b.Text, new WRect(x, M / 2 - 8, bw, 16)));
            x += bw + 4;
        }
        var statusWidths = h.Status.Select(s => t.Width(s.Text, s.Strong ? TextStyle.Caption : TextStyle.Small)).ToArray();
        float statusW = statusWidths.Sum(w => w + 6);
        float engineW = h.Engine is { } en ? t.Width(en.Text, TextStyle.Small) + 8 : 0;
        float enterW = h.Enter ? 20 : 0;
        float moreW = FaceMetrics.More + 2;
        var headerW = x + Math.Max(statusW, FaceMetrics.StatusSlot) + engineW + enterW + moreW + 8;

        // rows
        float inW = 0;
        var inHeights = new float[face.Inputs.Count];
        var widgetSizes = new (float w, float h)?[face.Inputs.Count];
        var labelWidths = new float[face.Inputs.Count];
        for (int k = 0; k < face.Inputs.Count; k++)
        {
            var r = face.Inputs[k];
            var style = r.SourceWord == RowSourceWord.Inherited ? TextStyle.Italic : TextStyle.Port;
            var lw = t.Width(InputCaption(r), style);
            if (r.Named is { } n) lw += t.Width(n.Name + " →", TextStyle.Small) + 6;
            if (r.Badge is { } bd) lw += t.Width(bd.Text, TextStyle.Small) + 14;
            labelWidths[k] = lw;
            if (r.Section is { } sec) inW = Math.Max(inW, 10 + t.Width(sec, TextStyle.Small) + 8);
            (float, float)? ws = r.SourceWord == RowSourceWord.Own && r.Widget is { } wd ? WidgetSize(wd, r, t) : null;
            widgetSizes[k] = ws;
            inHeights[k] = (ws?.Item2 ?? 0) > 30 ? FaceMetrics.MultilineRow : FaceMetrics.Row;
            inW = Math.Max(inW, 14 + lw + (ws is { } s ? s.Item1 + 8 : 0) + 8);
        }
        float outW = 0;
        foreach (var r in face.Outputs) outW = Math.Max(outW, 8 + t.Width(r.Label, TextStyle.Port) + 14);
        var rowsW = inW + outW + (inW > 0 && outW > 0 ? 12 : 0);

        // the preview and the width
        var showPreview = face.ViewWord is FaceViewWord.Full or null;
        var pv = showPreview && face.Preview is { Size: { } sz } ? sz : (Point?)null;
        float previewW = pv is { } p0 ? (float)p0.X + 16 : 0;
        var width = Math.Min(Math.Max(Math.Max(FaceMetrics.MinWidth, headerW), Math.Max(rowsW, previewW)), FaceMetrics.MaxWidth);

        // vertical
        float top = M + 4;
        L.InputRows = new WRect[face.Inputs.Count];
        L.InputPins = new float[face.Inputs.Count];
        L.Widgets = new WRect?[face.Inputs.Count];
        L.Opens = new WRect?[face.Inputs.Count];
        float y = top;
        for (int k = 0; k < face.Inputs.Count; k++)
        {
            var rh = inHeights[k];
            // FR5: the source node's title above its promoted rows
            if (face.Inputs[k].Section is { } sec && !collapsed)
            {
                L.Sections.Add((sec, new WRect(0, y, Math.Max(40, Math.Min(inW, width - outW - (outW > 0 ? 12 : 0))), FaceMetrics.Section)));
                y += FaceMetrics.Section;
            }
            L.InputRows[k] = new WRect(0, y, inW, rh);
            L.InputPins[k] = collapsed ? M / 2 : y + Math.Min(rh, FaceMetrics.Row) / 2;
            if (widgetSizes[k] is { } ws && !collapsed)
            {
                var wx = Math.Min(14 + labelWidths[k] + 8, Math.Max(width - ws.w - 8, 14));
                L.Widgets[k] = new WRect(wx, y + (Math.Min(rh, FaceMetrics.Row) - Math.Min(ws.h, 18)) / 2, ws.w, ws.h);
            }
            // a row that opens its node's editor: its caption is what a tap lands on (as the Apple
            // FaceLayout.opens), beside a widget too (B173)
            if (!collapsed && face.Inputs[k].Editor is not null)
                L.Opens[k] = new WRect(10, y, Math.Max(24, labelWidths[k] + 8), Math.Min(rh, FaceMetrics.Row));
            y += rh;
        }
        var inBottom = y;
        L.OutputRows = new WRect[face.Outputs.Count];
        L.OutputPins = new float[face.Outputs.Count];
        y = top;
        for (int k = 0; k < face.Outputs.Count; k++)
        {
            L.OutputRows[k] = new WRect(width - outW, y, outW, FaceMetrics.Row);
            L.OutputPins[k] = collapsed ? M / 2 : y + FaceMetrics.Row / 2;
            y += FaceMetrics.Row;
        }
        var bottom = Math.Max(inBottom, y);
        if (face.Inputs.Count == 0 && face.Outputs.Count == 0) bottom = M;

        if (pv is { } pp && !collapsed)
        {
            float w = (float)pp.X, hg = (float)pp.Y;
            L.Preview = new WRect((width - w) / 2, bottom + 4, w, hg);
            bottom += hg + 8;
        }

        if (!collapsed)
        {
            var textW = width - 16;
            foreach (var item in face.Footer)
            {
                var lines = new List<string>();
                bool mono = false;
                double? progress = null;
                switch (item.KindWord)
                {
                    case FooterWord.Fields: lines.Add(item.Text ?? ""); break;
                    case FooterWord.Convert: lines.AddRange((item.Lines ?? []).Select(l => l.Text)); break;
                    case FooterWord.Editor: lines.Add(item.Label ?? ""); break;
                    case FooterWord.Progress:
                        progress = item.Fraction ?? 0;
                        if (item.Stage is { } st) lines.Add(st);
                        break;
                    case FooterWord.Error: lines = t.Wrap(item.Text ?? "", TextStyle.Small, textW, 3); break;
                    case FooterWord.Save: lines.Add(item.Sheet == true ? "Save Result…   Contact Sheet…" : "Save Result…"); break;
                    case FooterWord.Excerpt:
                        mono = true;
                        lines = t.Wrap(item.Text ?? "", TextStyle.Mono, textW, 3);
                        break;
                    case FooterWord.EngineText:
                    case FooterWord.AppText:
                        lines = t.Wrap(item.Text ?? "", TextStyle.Small, textW, 2);
                        break;
                    default: continue; // a kind this shell doesn't know: skipped
                }
                var n = lines.Count + (progress is not null ? 0.6f : 0);
                var hg = Math.Max(n, 1) * FaceMetrics.FooterLine + 4;
                L.Footer.Add(new FooterPlan(new WRect(8, bottom + 2, textW, hg), lines, mono, progress) { Kind = item.Kind, Port = item.Port, Sheet = item.Sheet == true, Index = face.Footer.FindIndex(x => ReferenceEquals(x, item)) });
                bottom += hg + 2;
            }
        }

        L.W = width;
        L.H = collapsed ? M : bottom + 6;

        // the header's right side, from the more-button at the end
        var rx = width - 6;
        L.More = new WRect(rx - FaceMetrics.More, (M - FaceMetrics.More) / 2, FaceMetrics.More, FaceMetrics.More);
        rx -= moreW + 2;
        if (h.Enter)
        {
            L.Enter = new WRect(rx - 16, M / 2 - 8, 16, 16);
            rx -= enterW;
        }
        if (h.Engine is { } e)
        {
            L.Engine = new PlacedText(e.Text, rx, TextStyle.Small, 0, true);
            rx -= engineW;
        }
        for (int k = h.Status.Count - 1; k >= 0; k--)
        {
            var s = h.Status[k];
            L.Status.Insert(0, new PlacedText(s.Text, rx, s.Strong ? TextStyle.Caption : TextStyle.Small, 0, true));
            rx -= statusWidths[k] + 6;
        }
        // a title that would run into the status is cut
        var badgesW = L.Badges.Sum(b => (float)b.Rect.Width + 4);
        L.Title = new PlacedText(h.Title.Text, titleX, TextStyle.Title, Math.Max(20, Math.Min(FaceMetrics.TitleMax, rx - titleX - 8 - badgesW)));
        L.TitleWidth = titleW;
        return L;
    }

    /// <summary>The caption an input row shows: its label, or its text for a wired, socket, inherited or
    /// read-only row.</summary>
    public static string InputCaption(InRow r) => r.SourceWord == RowSourceWord.Own ? r.Label : r.Text ?? r.Label;

    /// <summary>A widget's size at rest (the Mac's: a checkbox for a toggle).</summary>
    static (float, float) WidgetSize(Widget w, InRow r, FaceText t) => w.KindWord switch
    {
        WidgetWord.Number => (Math.Max(56, t.Width(WidgetText(w), TextStyle.Mono) + 14), 18),
        WidgetWord.Toggle => (14, 14),
        WidgetWord.Choice => (120, 18),
        WidgetWord.Multichoice or WidgetWord.Text or WidgetWord.Path => (140, 18),
        WidgetWord.Multiline => (140, 50),
        WidgetWord.Color => (28, 16),
        WidgetWord.Editor => (t.Width(r.Label, TextStyle.Small) + 16, 18),
        _ => (100, 18),
    };

    static bool IsNull(JsonElement v) => v.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined;

    static string Plain(JsonElement v) => v.ValueKind switch
    {
        JsonValueKind.String => v.GetString() ?? "",
        JsonValueKind.Undefined => "",
        _ => v.GetRawText(),
    };

    /// <summary>What a widget shows as text at rest.</summary>
    public static string WidgetText(Widget w)
    {
        if (w.Blank || IsNull(w.Value)) return "blank";
        switch (w.KindWord)
        {
            case WidgetWord.Number:
                // the core's text (N4 slice 4): an INT whole, a FLOAT at its decimals
                if (w.Text is { } tx) return tx;
                if (w.Value.ValueKind != JsonValueKind.Number || !w.Value.TryGetDouble(out var d)) return Plain(w.Value);
                if (w.Int) return Math.Round(d).ToString("0", System.Globalization.CultureInfo.InvariantCulture);
                var places = Math.Clamp(w.Decimals ?? 3, 0, 8);
                return d.ToString("F" + places, System.Globalization.CultureInfo.InvariantCulture);
            case WidgetWord.Choice:
                foreach (var c in w.Choices)
                    if (Jsonv.Same(c.Value, w.Value)) return c.Label;
                return Plain(w.Value);
            case WidgetWord.Multichoice:
                if (w.Value.ValueKind == JsonValueKind.Array) return string.Join(", ", w.Value.EnumerateArray().Select(Plain));
                return Plain(w.Value);
            case WidgetWord.Toggle:
                return w.Value.ValueKind == JsonValueKind.True ? "on" : "off";
            default:
                return Plain(w.Value);
        }
    }

    /// <summary>A colour widget's colour: `#rrggbb`, `r,g,b` in 0 to 1, or `[r, g, b]` in 0 to 1.</summary>
    public static Windows.UI.Color? WidgetColor(Widget w)
    {
        static Windows.UI.Color C(double r, double g, double b) =>
            Windows.UI.Color.FromArgb(255, (byte)Math.Clamp(r * 255, 0, 255), (byte)Math.Clamp(g * 255, 0, 255), (byte)Math.Clamp(b * 255, 0, 255));
        var v = w.Value;
        if (v.ValueKind == JsonValueKind.String && v.GetString() is { } s)
        {
            if (s.StartsWith('#') && s.Length == 7 && uint.TryParse(s.AsSpan(1), System.Globalization.NumberStyles.HexNumber, null, out var hex))
                return Windows.UI.Color.FromArgb(255, (byte)(hex >> 16), (byte)(hex >> 8), (byte)hex);
            var parts = s.Split(',').Select(p => double.TryParse(p.Trim(), System.Globalization.CultureInfo.InvariantCulture, out var x) ? x : double.NaN).ToArray();
            if (parts.Length >= 3 && parts.Take(3).All(double.IsFinite)) return C(parts[0], parts[1], parts[2]);
        }
        if (v.ValueKind == JsonValueKind.Array && v.GetArrayLength() >= 3 && v[0].TryGetDouble(out var r0) && v[1].TryGetDouble(out var g0) && v[2].TryGetDouble(out var b0))
            return C(r0, g0, b0);
        return null;
    }
}
