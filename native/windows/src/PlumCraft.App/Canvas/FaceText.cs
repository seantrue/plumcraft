// Text on a face: the system face measured and drawn with DirectWrite through Win2D, in the styles the
// Apple faces use (title 14 SemiBold, port 13, caption 12.5, small 12, mono 12, italic 13, contract
// 14.3). Widths are cached by text and style; formats are made once and only read after, so layout
// and drawing run on workers (Win2D's device and DirectWrite's formats are free-threaded).

using System.Collections.Concurrent;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.Text;
using Windows.Foundation;
using Windows.UI;

namespace PlumCraft.App.Canvas;

public enum TextStyle { Title, Port, Caption, Small, Mono, Italic }

public sealed class FaceText
{
    /// <summary>Segoe UI Variable on Windows 11, Segoe UI before it.</summary>
    public static readonly string Family = Environment.OSVersion.Version.Build >= 22000 ? "Segoe UI Variable Text" : "Segoe UI";
    public static readonly FaceText Shared = new();

    readonly CanvasTextFormat[] left = new CanvasTextFormat[6];
    readonly CanvasTextFormat[] right = new CanvasTextFormat[6];
    readonly CanvasTextFormat[] centre = new CanvasTextFormat[6];
    readonly CanvasTextFormat[] wrap = new CanvasTextFormat[6];
    readonly ConcurrentDictionary<(string, TextStyle), float> widths = new();
    readonly CanvasDevice device = CanvasDevice.GetSharedDevice();
    public long Measured;

    FaceText()
    {
        foreach (TextStyle s in Enum.GetValues<TextStyle>())
        {
            left[(int)s] = Make(s, CanvasHorizontalAlignment.Left, true);
            right[(int)s] = Make(s, CanvasHorizontalAlignment.Right, true);
            centre[(int)s] = Make(s, CanvasHorizontalAlignment.Center, true);
            wrap[(int)s] = Make(s, CanvasHorizontalAlignment.Left, false);
        }
    }

    static CanvasTextFormat Make(TextStyle s, CanvasHorizontalAlignment h, bool oneLine)
    {
        var f = new CanvasTextFormat
        {
            FontFamily = s == TextStyle.Mono ? "Consolas" : Family,
            FontSize = s switch { TextStyle.Title => 14, TextStyle.Port or TextStyle.Italic => 13, TextStyle.Caption => 12.5f, _ => 12 },
            FontWeight = s is TextStyle.Title or TextStyle.Caption ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal,
            FontStyle = s == TextStyle.Italic ? Windows.UI.Text.FontStyle.Italic : Windows.UI.Text.FontStyle.Normal,
            WordWrapping = oneLine ? CanvasWordWrapping.NoWrap : CanvasWordWrapping.Wrap,
            VerticalAlignment = CanvasVerticalAlignment.Center,
            HorizontalAlignment = h,
        };
        if (oneLine)
        {
            f.TrimmingGranularity = CanvasTextTrimmingGranularity.Character;
            f.TrimmingSign = CanvasTrimmingSign.Ellipsis;
        }
        return f;
    }

    public CanvasTextFormat Format(TextStyle s) => left[(int)s];

    /// <summary>The width `s` takes on one line in `style`.</summary>
    public float Width(string s, TextStyle style)
    {
        if (string.IsNullOrEmpty(s)) return 0;
        if (widths.TryGetValue((s, style), out var w)) return w;
        using var layout = new CanvasTextLayout(device, s, left[(int)style], 100000, 100);
        w = (float)layout.LayoutBounds.Width;
        widths[(s, style)] = w;
        Interlocked.Increment(ref Measured);
        return w;
    }

    /// <summary>`text` in at most `lines` lines `width` wide (DirectWrite breaks them), the last cut.</summary>
    public List<string> Wrap(string text, TextStyle style, float width, int lines)
    {
        var flat = string.Join(' ', text.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries));
        var outp = new List<string>();
        if (flat.Length == 0) return outp;
        using var layout = new CanvasTextLayout(device, flat, wrap[(int)style], Math.Max(width, 10), 10000);
        int at = 0;
        foreach (var m in layout.LineMetrics)
        {
            if (outp.Count == lines) break;
            var n = Math.Min(m.CharacterCount, flat.Length - at);
            if (n <= 0) break;
            outp.Add(flat.Substring(at, n).Trim());
            at += n;
        }
        if (at < flat.Length && outp.Count > 0) outp[^1] += "…";
        return outp;
    }

    /// <summary>Draw `s` in a box, vertically centred, cut with an ellipsis to the box's width.</summary>
    public void Draw(CanvasDrawingSession ds, string s, TextStyle style, Color ink, Rect box, Align align = Align.Left)
    {
        if (string.IsNullOrEmpty(s) || box.Width <= 0) return;
        var f = align switch { Align.Right => right[(int)style], Align.Centre => centre[(int)style], _ => left[(int)style] };
        ds.DrawText(s, box, ink, f);
    }
}

public enum Align { Left, Right, Centre }
