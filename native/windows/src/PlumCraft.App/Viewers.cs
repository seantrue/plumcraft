// The viewers of a pop-out (N5 slice 4; PlumCraftShell's Viewers.swift and the Mac app's Popouts.swift),
// each drawing what the core gives (`popout.get`, `viewer.data`, `media.get`) with the platform's own
// controls and sending what the person does as the core's commands. Nothing large is read on the UI
// thread: the core reads and pages on its media workers, and what it lends is copied on a worker.

using System.Globalization;
using System.Numerics;
using System.Text;
using System.Text.Json;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;
using PlumCraft.App.Canvas;
using PlumCraft.Kit;
using PlumCraft.Models;
using Border = Microsoft.UI.Xaml.Controls.Border;
using Windows.Media.Core;
using Windows.Media.Playback;

namespace PlumCraft.App;

/// <summary>One viewer: its element, kept while the value's viewer stays the same, told each new value.</summary>
public interface IViewer
{
    string Kind { get; }
    FrameworkElement View { get; }
    void Update(PopoutValue v);
    System.Text.Json.Nodes.JsonObject? Describe();
    /// <summary>The window closed: stop what plays.</summary>
    void Gone() { }
}

static class V
{
    public static TextBlock Message(string s, bool error = false) => new()
    {
        Text = s, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(16), HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
        TextAlignment = TextAlignment.Center, Foreground = error ? new SolidColorBrush(Microsoft.UI.Colors.IndianRed) : Ui.Brush("TextFillColorSecondaryBrush"), IsTextSelectionEnabled = true,
    };

    public static ToggleButton Toggle(string label, bool on, Func<bool, Task> run)
    {
        var b = new ToggleButton { Content = label, IsChecked = on, Padding = new Thickness(10, 3, 10, 4) };
        b.Click += async (_, _) => await run(b.IsChecked == true);
        return b;
    }

    public static StackPanel Row(params UIElement[] items)
    {
        var s = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Padding = new Thickness(12, 6, 12, 6) };
        foreach (var i in items) s.Children.Add(i);
        return s;
    }

    /// <summary>A page bar: "rows 1–500 of 100,000", the previous and next page (the core's).</summary>
    public static Grid PageBar(string what, ulong start, int shown, ulong total, ulong page, ulong pages, Func<ulong, Task> go)
    {
        var g = Ui.Columns(Ui.Star, Ui.Auto, Ui.Auto, Ui.Auto);
        g.Padding = new Thickness(12, 4, 8, 4);
        g.Children.Add(new TextBlock
        {
            Text = total == 0 ? $"no {what}" : $"{what} {start + 1:N0}–{start + (ulong)shown:N0} of {total:N0}", Style = Ui.TextStyle("CaptionTextBlockStyle"),
            Foreground = Ui.Brush("TextFillColorSecondaryBrush"), VerticalAlignment = VerticalAlignment.Center,
        });
        if (pages > 1)
        {
            var prev = Ui.GlyphButton(Ui.G.Back, "Previous Page", () => go(page - 1), 12);
            prev.IsEnabled = page > 0;
            var next = Ui.GlyphButton(Ui.G.Forward, "Next Page", () => go(page + 1), 12);
            next.IsEnabled = page + 1 < pages;
            g.Children.Add(prev.At(1));
            g.Children.Add(new TextBlock { Text = $"page {page + 1:N0} of {pages:N0}", Style = Ui.TextStyle("CaptionTextBlockStyle"), VerticalAlignment = VerticalAlignment.Center }.At(2));
            g.Children.Add(next.At(3));
        }
        return g;
    }

    public static Image Picture(CanvasBitmap bmp, Stretch stretch = Stretch.Uniform)
    {
        var src = new CanvasImageSource(CanvasDevice.GetSharedDevice(), bmp.SizeInPixels.Width, bmp.SizeInPixels.Height, 96);
        using (var ds = src.CreateDrawingSession(Microsoft.UI.Colors.Transparent)) ds.DrawImage(bmp);
        return new Image { Source = src, Stretch = stretch };
    }

    public static string Stats(JsonElement? stats) =>
        string.Join(" · ", stats.Items().Select(s => $"{s.Get("name").Str()} {s.Get("min").Str() ?? "?"} to {s.Get("max").Str() ?? "?"}"));

    public static Border Frame(UIElement child) => new() { Child = child, Background = Ui.Brush("SubtleFillColorSecondaryBrush"), CornerRadius = new CornerRadius(6) };
}

/// <summary>An array's channels (B154 slice 1; contract 13.2.2): each channel's range and NaN count, and its
/// histogram drawn with Win2D as the core scaled it (`heights` 0 to 1, the axis's ends as `lo` and `hi`), under a
/// Log scale toggle that is the core's `viewer.array {log}`. Nothing here is computed or kept by the shell.</summary>
public sealed class ChannelStatsPanel
{
    readonly PopoutModel pop;
    readonly StackPanel root = new() { Spacing = 6, Padding = new Thickness(12, 6, 12, 8) };
    readonly CheckBox log = new() { Content = "Log scale", MinWidth = 0 };
    readonly StackPanel channels = new() { Spacing = 8 };
    bool following;
    List<ArrayViews.Channel> shown = [];

    public ChannelStatsPanel(PopoutModel pop)
    {
        this.pop = pop;
        ToolTipService.SetToolTip(log, "Each bar on a log scale, ln(1 + count): the core's viewer.array {log}");
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(log, "histogram-log");
        log.Click += async (_, _) => { if (!following) await pop.CommandAsync("viewer.array", ("log", Jsonv.Of(log.IsChecked == true))); };
        root.Children.Add(log);
        root.Children.Add(channels);
        root.Visibility = Visibility.Collapsed;
    }

    public FrameworkElement View => root;

    /// <summary>What the panel shows now, for the app's status (a check reads it back).</summary>
    public System.Text.Json.Nodes.JsonObject Describe() => new()
    {
        ["channels"] = shown.Count, ["log"] = log.IsChecked == true,
        ["bars"] = shown.FirstOrDefault()?.Histogram?.Heights.Count(x => x > 0) ?? 0,
    };

    public void Update(PopoutValue v)
    {
        var cs = ArrayViews.Channels(v.Stats);
        root.Visibility = cs.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        following = true;
        log.IsChecked = v.State.Get("array").Get("log").IsTrue();
        log.Visibility = cs.Any(c => c.Histogram is not null) ? Visibility.Visible : Visibility.Collapsed;
        following = false;
        if (cs.SequenceEqual(shown, ChannelSame.Instance)) return;
        shown = cs;
        channels.Children.Clear();
        foreach (var c in cs.Take(16))
        {
            var block = new StackPanel { Spacing = 2 };
            block.Children.Add(new TextBlock { Text = c.Line, Style = Ui.TextStyle("CaptionTextBlockStyle"), FontFamily = new FontFamily("Cascadia Mono, Consolas"), IsTextSelectionEnabled = true });
            if (c.Histogram is { } h)
            {
                var bars = new CanvasControl { Height = 48, MinWidth = 120 };
                bars.Draw += (s, e) => DrawHistogram(s, e, h);
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(bars, $"{c.Name} histogram, {h.Bins} bins from {h.Lo} to {h.Hi}{(h.Log ? ", log scale" : "")}");
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(bars, "histogram");
                ToolTipService.SetToolTip(bars, $"{h.Bins} bins from {h.Lo} to {h.Hi}, the tallest {h.Top:N0}{(h.Log ? ", log scale" : "")}");
                block.Children.Add(bars);
                var ends = Ui.Columns(Ui.Star, Ui.Auto);
                ends.Children.Add(Ui.Caption(h.Lo, wrap: false));
                var hi = Ui.Caption(h.Hi, wrap: false);
                hi.HorizontalAlignment = HorizontalAlignment.Right;
                ends.Children.Add(hi.At(1));
                block.Children.Add(ends);
            }
            channels.Children.Add(block);
        }
    }

    static void DrawHistogram(CanvasControl sender, CanvasDrawEventArgs e, ArrayViews.Histogram h)
    {
        var ds = e.DrawingSession;
        var w = (float)sender.ActualWidth;
        var ht = (float)sender.ActualHeight;
        var field = Application.Current.Resources["ControlFillColorSecondaryBrush"] is SolidColorBrush fb ? fb.Color : Windows.UI.Color.FromArgb(20, 128, 128, 128);
        ds.FillRoundedRectangle(0, 0, w, ht, 2, 2, field);
        if (h.Heights.Length == 0) return;
        var accent = (Windows.UI.Color)Application.Current.Resources["SystemAccentColor"];
        var bw = w / h.Heights.Length;
        for (int i = 0; i < h.Heights.Length; i++)
        {
            var frac = (float)h.Heights[i];
            if (frac <= 0) continue;
            ds.FillRectangle(i * bw, ht - frac * ht, MathF.Max(bw, 1), frac * ht, accent);
        }
    }

    sealed class ChannelSame : IEqualityComparer<ArrayViews.Channel>
    {
        public static readonly ChannelSame Instance = new();
        public bool Equals(ArrayViews.Channel? a, ArrayViews.Channel? b) =>
            a is not null && b is not null && a.Line == b.Line && (a.Histogram, b.Histogram) switch
            {
                (null, null) => true,
                ({ } x, { } y) => x.Lo == y.Lo && x.Hi == y.Hi && x.Log == y.Log && x.Top == y.Top && x.Heights.SequenceEqual(y.Heights),
                _ => false,
            };
        public int GetHashCode(ArrayViews.Channel c) => c.Line.GetHashCode();
    }
}

/// <summary>A short message in a viewer's place.</summary>
public sealed class MessageViewer(Func<PopoutValue, string> say, bool error = false) : IViewer
{
    readonly Grid g = new();
    string text = "";
    public string Kind => error ? "error" : "none";
    public FrameworkElement View => g;
    public void Update(PopoutValue v)
    {
        text = say(v);
        g.Children.Clear();
        g.Children.Add(V.Message(text, error));
    }
    public System.Text.Json.Nodes.JsonObject? Describe() => new() { ["message"] = text };
}

// ---------------- scalar and text ----------------

public sealed class ScalarViewer : IViewer
{
    readonly StackPanel s = new() { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, Spacing = 4 };
    readonly TextBlock value = new() { FontSize = 40, FontWeight = Microsoft.UI.Text.FontWeights.SemiLight, IsTextSelectionEnabled = true, HorizontalAlignment = HorizontalAlignment.Center };
    readonly TextBlock kind = Ui.Caption("", wrap: false);
    public ScalarViewer()
    {
        kind.HorizontalAlignment = HorizontalAlignment.Center;
        s.Children.Add(value);
        s.Children.Add(kind);
    }
    public string Kind => "scalar";
    public FrameworkElement View => s;
    public void Update(PopoutValue v)
    {
        value.Text = Scalar(v.Value, v.Summary);
        kind.Text = v.Media ?? "scalar";
    }

    /// <summary>A scalar: the core's text when it gave one (its summary), else the value with grouping.</summary>
    public static string Scalar(JsonElement? value, string? summary)
    {
        if (value is not { } v || v.ValueKind is JsonValueKind.Null or JsonValueKind.Undefined) return summary ?? "—";
        return v.ValueKind switch
        {
            JsonValueKind.True => "true",
            JsonValueKind.False => "false",
            JsonValueKind.String => v.GetString() ?? "",
            JsonValueKind.Number when v.TryGetInt64(out var n) => n.ToString("N0", CultureInfo.CurrentCulture),
            JsonValueKind.Number when v.TryGetDouble(out var d) => d.ToString("#,##0.######", CultureInfo.CurrentCulture),
            _ => v.GetRawText(),
        };
    }
    public System.Text.Json.Nodes.JsonObject? Describe() => new() { ["value"] = value.Text };
}

/// <summary>Selectable text; Wrap, Monospaced and Show All are the core's `viewer.text`, the whole text read
/// from the value's own bytes.</summary>
public sealed class TextViewer : IViewer
{
    readonly PopoutModel pop;
    readonly Grid g = new();
    readonly TextBlock info = Ui.Caption("", wrap: false);
    readonly ToggleButton wrap, mono;
    readonly Button all = new() { Content = "Show All", Padding = new Thickness(10, 3, 10, 4) };
    readonly TextBlock text = new() { IsTextSelectionEnabled = true, FontSize = 13, Margin = new Thickness(14, 10, 14, 14) };
    readonly ScrollViewer scroll = new() { HorizontalScrollMode = ScrollMode.Auto };
    readonly Dictionary<string, string> whole = [];
    public TextViewer(PopoutModel pop)
    {
        this.pop = pop;
        wrap = V.Toggle("Wrap", true, on => pop.TextAsync(wrap: on));
        mono = V.Toggle("Monospaced", false, on => pop.TextAsync(mono: on));
        all.Click += async (_, _) => await pop.TextAsync(all: true);
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var bar = Ui.Columns(Ui.Star, Ui.Auto);
        bar.Padding = new Thickness(12, 6, 12, 6);
        info.VerticalAlignment = VerticalAlignment.Center;
        bar.Children.Add(info);
        bar.Children.Add(V.Row(wrap, mono, all).At(1));
        ((StackPanel)bar.Children[1]).Padding = new Thickness(0);
        g.Children.Add(bar);
        scroll.Content = text;
        g.Children.Add(scroll.At(0, 1));
    }
    public string Kind => "text.plain";
    public FrameworkElement View => g;
    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        var tv = v.Text;
        wrap.IsChecked = tv?.Wrap ?? true;
        mono.IsChecked = tv?.Mono ?? false;
        var shown = v.Excerpt?.Text ?? "";
        var total = (long)(v.Excerpt?.TotalChars ?? (ulong)shown.Length);
        var handle = v.Data is { } d ? Pictures.HandleOf(d) : null;
        if (tv?.All == true && handle is not null && total > shown.Length)
        {
            if (!whole.TryGetValue(handle, out var w))
            {
                info.Text = "reading the whole text…";
                w = await Whole(handle);
                if (w is not null) whole[handle] = w;
            }
            if (w is not null) shown = w;
        }
        text.Text = shown;
        text.TextWrapping = tv?.Wrap == false ? TextWrapping.NoWrap : TextWrapping.Wrap;
        text.FontFamily = new FontFamily(tv?.Mono == true ? "Cascadia Mono, Consolas" : FaceText.Family);
        scroll.HorizontalScrollBarVisibility = tv?.Wrap == false ? ScrollBarVisibility.Auto : ScrollBarVisibility.Disabled;
        var cut = shown.Length < total;
        info.Text = cut ? $"first {shown.Length:N0} of {total:N0} characters" : $"{total:N0} characters";
        all.Visibility = cut && handle is not null ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>The value's own bytes as UTF-8 (media.get {want: "bytes"}, then plum_blob), at most 8 MB.</summary>
    async Task<string?> Whole(string handle)
    {
        var model = pop.Model;
        try
        {
            var note = await model.Core.RequestAsync<MediaNote>("media.get", Jsonv.Object(("handle", Jsonv.Of(handle)), ("want", Jsonv.Of("bytes"))), model.RequestDoc);
            var key = note.Key;
            if (note.StatusWord == MediaWord.Pending)
            {
                if (!await model.WaitAsync(() => model.Media.TryGetValue(key, out var m) && m.StatusWord != MediaWord.Pending, TimeSpan.FromSeconds(30))) return null;
                note = model.Media[key];
            }
            if (note.StatusWord != MediaWord.Ready) return null;
            var core = model.Core;
            return await Task.Run(() =>
            {
                using var lease = core.Blob(key, out _);
                if (lease is null) return null;
                var span = lease.Span;
                return Encoding.UTF8.GetString(span[..Math.Min(span.Length, 8 << 20)]);
            });
        }
        catch (Exception) { return null; }
    }
    public System.Text.Json.Nodes.JsonObject? Describe() => new()
    {
        ["text"] = text.Text[..Math.Min(text.Text.Length, 200)], ["shown"] = text.Text.Length, ["wrap"] = wrap.IsChecked == true, ["mono"] = mono.IsChecked == true,
    };
}

// ---------------- pictures ----------------

/// <summary>Where a pop-out's picture comes from (the Swift kit's PictureSource).</summary>
public sealed record PictureSource(string Handle, string Want, int? Px, (int w, int h)? Full)
{
    public static (int w, int h)? FullSize(PopoutValue v)
    {
        var f = v.Form;
        if (f.Get("width").U64() is { } w && f.Get("height").U64() is { } h && w > 0 && h > 0) return ((int)w, (int)h);
        if (f.Get("shape").At(0).U64() is { } sh && f.Get("shape").At(1).U64() is { } sw && sw > 0 && sh > 0) return ((int)sw, (int)sh);
        return null;
    }

    public static PictureSource? Of(PopoutValue v, int svgPx = 2048)
    {
        var full = FullSize(v);
        if (v.Viewer == "svg" && v.Handle is { } sh) return new(sh, "svg", svgPx, null);
        if (v.Viewer == "array.pictures" && v.Views?.FirstOrDefault() is { } p && Pictures.HandleOf(p.Ref) is { } ph) return new(ph, "rgba", null, full);
        if (v.Media == "image" && v.Handle is { } ih && full is { } f) return new(ih, "image", Math.Min(2048, Math.Max(f.w, f.h)), full);
        if (v.Picture is { } pic && (pic.Handle ?? Pictures.HandleOf(pic.Ref)) is { } rh) return new(rh, "rgba", null, full);
        return null;
    }

    /// <summary>More detail than shown, for a zoom past it: a picture file decoded larger, an SVG drawn larger.</summary>
    public PictureSource? Sharper(int shown, PopoutValue v)
    {
        if (Want == "svg") return (Px ?? 0) < 4096 ? this with { Px = 4096 } : null;
        if (v.Media != "image" || v.Handle is not { } h || Full is not { } f || Math.Max(f.w, f.h) <= shown) return null;
        return new(h, "image", Math.Min(4096, Math.Max(f.w, f.h)), Full);
    }

    public static readonly (string id, string label)[] Channels = [("rgba", "All Channels"), ("rgb", "Colour"), ("r", "Red"), ("g", "Green"), ("b", "Blue"), ("a", "Alpha")];

    /// <summary>A picture's one channel as grey, or its colour without alpha (`viewer.picture`'s channel), from
    /// premultiplied BGRA; made on a worker. `rgba` gives it back as it is.</summary>
    public static byte[] Isolate(byte[] bgra, string channel)
    {
        if (channel == "rgba") return bgra;
        var o = new byte[bgra.Length];
        for (int i = 0; i + 3 < bgra.Length; i += 4)
        {
            int b = bgra[i], g = bgra[i + 1], r = bgra[i + 2], a = bgra[i + 3];
            byte Un(int c) => a == 0 ? (byte)0 : (byte)Math.Min(255, c * 255 / a);
            if (channel == "rgb") { o[i] = Un(b); o[i + 1] = Un(g); o[i + 2] = Un(r); o[i + 3] = 255; continue; }
            var v = channel switch { "r" => Un(r), "g" => Un(g), "b" => Un(b), _ => (byte)a };
            o[i] = v; o[i + 1] = v; o[i + 2] = v; o[i + 3] = 255;
        }
        return o;
    }
}

/// <summary>A picture, an SVG drawn by the core, or an array's one picture: Fit and 1:1 the core's
/// `viewer.zoom`, the channel the core's `viewer.picture`, the readout of the value's own numbers under the
/// pointer (`viewer.data probe`), a click pinning the probe (`viewer.probe`), more detail asked for once a
/// zoom passes the picture's own pixels.</summary>
public sealed class PictureViewer : IViewer
{
    readonly PopoutModel pop;
    readonly Pictures pictures;
    readonly Grid g = new();
    readonly TextBlock info = Ui.Caption("", wrap: false);
    readonly TextBlock reading = new() { Style = Ui.TextStyle("CaptionTextBlockStyle"), FontFamily = new FontFamily("Cascadia Mono, Consolas"), Margin = new Thickness(12, 0, 12, 4), TextTrimming = TextTrimming.CharacterEllipsis };
    readonly ComboBox channel = new() { MinWidth = 130 };
    readonly ToggleButton fit = new() { Content = "Fit", Padding = new Thickness(12, 3, 12, 4), CornerRadius = new CornerRadius(4, 0, 0, 4) };
    readonly ToggleButton one = new() { Content = "1:1", Padding = new Thickness(12, 3, 12, 4), CornerRadius = new CornerRadius(0, 4, 4, 0) };
    readonly ScrollViewer scroll = new() { ZoomMode = ZoomMode.Enabled, MinZoomFactor = 0.1f, MaxZoomFactor = 64, HorizontalScrollMode = ScrollMode.Auto, VerticalScrollMode = ScrollMode.Auto };
    readonly Image image = new() { Stretch = Stretch.Fill };
    readonly Window window;
    PictureSource? src;
    CanvasBitmap? original;
    (int w, int h) shownSize;
    string shownChannel = "rgba";
    bool following, sharpened, loading;
    DateTime lastAsk = DateTime.MinValue;
    PopoutValue? value;
    JsonElement? lastReading;
    readonly ChannelStatsPanel stats;

    public PictureViewer(PopoutModel pop, Pictures pictures, Window window)
    {
        this.pop = pop;
        this.pictures = pictures;
        this.window = window;
        stats = new ChannelStatsPanel(pop);
        foreach (var (_, label) in PictureSource.Channels) channel.Items.Add(label);
        ToolTipService.SetToolTip(channel, "Channel");
        ToolTipService.SetToolTip(fit, "Fit the picture to the window");
        ToolTipService.SetToolTip(one, "One pixel of the picture to one of the screen");
        channel.SelectionChanged += async (_, _) =>
        {
            if (following || channel.SelectedIndex < 0) return;
            await pop.CommandAsync("viewer.picture", ("channel", Jsonv.Of(PictureSource.Channels[channel.SelectedIndex].id)));
        };
        fit.Click += async (_, _) => await pop.ZoomAsync("fit");
        one.Click += async (_, _) => await pop.ZoomAsync("1:1");
        foreach (var h in new[] { GridLength.Auto, GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto }) g.RowDefinitions.Add(new RowDefinition { Height = h });
        var bar = Ui.Columns(Ui.Star, Ui.Auto, Ui.Auto);
        bar.Padding = new Thickness(12, 6, 12, 4);
        info.VerticalAlignment = VerticalAlignment.Center;
        bar.Children.Add(info);
        bar.Children.Add(channel.At(1));
        var seg = new StackPanel { Orientation = Orientation.Horizontal };
        seg.Children.Add(fit);
        seg.Children.Add(one);
        bar.Children.Add(seg.At(2));
        g.Children.Add(bar);
        g.Children.Add(reading.At(0, 1));
        scroll.Content = image;
        scroll.Background = Ui.Brush("SolidBackgroundFillColorSecondaryBrush");
        g.Children.Add(scroll.At(0, 2));
        // an array's channels and histograms under its picture (13.2.2); nothing for a picture without stats
        g.Children.Add(stats.View.At(0, 3));
        scroll.SizeChanged += (_, _) => Apply();
        scroll.ViewChanged += (_, e) => { if (!e.IsIntermediate) Detail(); };
        image.PointerMoved += Probe;
        // which value's picture is drawn, read back (a new view mode changes only the picture's handle)
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(image, "view-picture");
        image.PointerPressed += Pin;
    }

    public string Kind => "picture";
    public FrameworkElement View => g;

    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        value = v;
        stats.Update(v);
        channel.Visibility = v.Viewer == "svg" ? Visibility.Collapsed : Visibility.Visible;
        var c = v.State.Get("picture").Get("channel").Str() ?? "rgba";
        following = true;
        channel.SelectedIndex = Math.Max(0, Array.FindIndex(PictureSource.Channels, x => x.id == c));
        following = false;
        var isOne = v.Fit == "1:1";
        fit.IsChecked = !isOne;
        one.IsChecked = isOne;
        var s = PictureSource.Of(v);
        if (s is null)
        {
            info.Text = "no picture: run the graph";
            return;
        }
        if (src is null || src.Handle != s.Handle || src.Want != s.Want || original is null)
        {
            if (loading) return;
            loading = true;
            src = s;
            sharpened = false;
            info.Text = "loading…";
            var bmp = await pictures.LoadAsync(s.Handle, s.Want, s.Px);
            loading = false;
            if (bmp is null)
            {
                info.Text = pictures.Failures.GetValueOrDefault(s.Handle) ?? "couldn't load the picture";
                return;
            }
            original = bmp;
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetItemStatus(image, s.Handle);
            shownSize = ((int)bmp.SizeInPixels.Width, (int)bmp.SizeInPixels.Height);
            shownChannel = "";
            var f = s.Full;
            info.Text = $"{shownSize.w} × {shownSize.h} px{(f is { } ff && (ff.w != shownSize.w || ff.h != shownSize.h) ? $" of {ff.w} × {ff.h}" : "")} · {(s.Want == "svg" ? "SVG" : v.Media ?? "picture")}";
        }
        await Channel(c);
        Apply();
    }

    /// <summary>The channel shown, made from the picture on a worker.</summary>
    async Task Channel(string c)
    {
        if (original is null || c == shownChannel) return;
        shownChannel = c;
        var bmp = original;
        if (c == "rgba")
        {
            image.Source = V.Picture(bmp).Source;
            return;
        }
        var bytes = bmp.GetPixelBytes();
        var w = (int)bmp.SizeInPixels.Width;
        var h = (int)bmp.SizeInPixels.Height;
        var iso = await Task.Run(() => PictureSource.Isolate(bytes, c));
        var made = CanvasBitmap.CreateFromBytes(CanvasDevice.GetSharedDevice(), iso, w, h, Windows.Graphics.DirectX.DirectXPixelFormat.B8G8R8A8UIntNormalized, 96, CanvasAlphaMode.Premultiplied);
        image.Source = V.Picture(made).Source;
    }

    /// <summary>Laid out at the value's own size (so 1:1 is a pixel of the value), fitted unless 1:1.</summary>
    void Apply()
    {
        if (shownSize.w <= 0 || src is null) return;
        var f = src.Full ?? shownSize;
        var scale = (float)(window.Content?.XamlRoot?.RasterizationScale ?? 1);
        if (value?.Fit == "1:1")
        {
            // one pixel of the value to one of the screen, scrolled
            image.Width = f.w / scale;
            image.Height = f.h / scale;
            image.Stretch = Stretch.Fill;
            scroll.HorizontalScrollBarVisibility = ScrollBarVisibility.Auto;
            scroll.VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
            scroll.ChangeView(null, null, 1, true);
        }
        else
        {
            // fitted: the picture held to the window, a pinch or Ctrl+wheel still zooming it
            image.Width = double.NaN;
            image.Height = double.NaN;
            image.Stretch = Stretch.Uniform;
            scroll.HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled;
            scroll.VerticalScrollBarVisibility = ScrollBarVisibility.Disabled;
            scroll.ChangeView(0, 0, 1, true);
        }
    }

    /// <summary>Zoomed in past the picture's own pixels: more detail, once (a picture file decoded larger, an
    /// SVG drawn larger, an array's larger rendition from the core).</summary>
    async void Detail()
    {
        if (sharpened || src is null || value is null || shownSize.w <= 0) return;
        var f = src.Full ?? shownSize;
        var scale = window.Content?.XamlRoot?.RasterizationScale ?? 1;
        _ = f;
        // device pixels a shown pixel covers now
        var screenPerShown = scroll.ZoomFactor * image.ActualWidth * scale / shownSize.w;
        if (screenPerShown <= 1.01) return;
        if (src.Sharper(Math.Max(shownSize.w, shownSize.h), value) is { } sharper)
        {
            sharpened = true;
            info.Text += " · reading more detail…";
            if (await pictures.LoadAsync(sharper.Handle, sharper.Want, sharper.Px) is { } bmp)
            {
                src = sharper;
                original = bmp;
                shownSize = ((int)bmp.SizeInPixels.Width, (int)bmp.SizeInPixels.Height);
                shownChannel = "";
                await Channel(value.State.Get("picture").Get("channel").Str() ?? "rgba");
                info.Text = $"{shownSize.w} × {shownSize.h} px · {(sharper.Want == "svg" ? "drawn" : "decoded")} · detail";
            }
        }
        else if (value.Media == "ndarray" && src.Full is { } full && Math.Max(full.w, full.h) > Math.Max(shownSize.w, shownSize.h))
        {
            sharpened = true;
            info.Text += " · asking for detail…";
            await pop.ZoomAsync("detail");
        }
    }

    (ulong row, ulong col)? PixelAt(PointerRoutedEventArgs e)
    {
        if (src is null || shownSize.w <= 0 || image.ActualWidth <= 0) return null;
        var p = e.GetCurrentPoint(image).Position;
        var f = src.Full ?? shownSize;
        if (p.X < 0 || p.Y < 0 || p.X >= image.ActualWidth || p.Y >= image.ActualHeight) return null;
        var col = Math.Min(f.w - 1, (int)(p.X / image.ActualWidth * f.w));
        var row = Math.Min(f.h - 1, (int)(p.Y / image.ActualHeight * f.h));
        return ((ulong)Math.Max(0, row), (ulong)Math.Max(0, col));
    }

    async void Probe(object sender, PointerRoutedEventArgs e)
    {
        if (value?.Viewer == "svg" || PixelAt(e) is not { } px || (DateTime.UtcNow - lastAsk).TotalMilliseconds < 50) return;
        lastAsk = DateTime.UtcNow;
        var r = await pop.DataAsync("probe", TimeSpan.FromSeconds(5), ("row", Jsonv.Of(px.row)), ("col", Jsonv.Of(px.col)));
        lastReading = r;
        reading.Text = $"row {px.row}, col {px.col}  {r.Get("text").Str() ?? r.Get("unavailable").Str() ?? r.Get("error").Str() ?? ""}";
    }

    async void Pin(object sender, PointerRoutedEventArgs e)
    {
        if (value?.Viewer == "svg" || PixelAt(e) is not { } px) return;
        try { await pop.Model.RunAsync("viewer.probe", Jsonv.Object(("popout", Jsonv.Of(pop.Id)), ("row", Jsonv.Of(px.row)), ("col", Jsonv.Of(px.col)))); } catch (Exception) { }
    }

    public System.Text.Json.Nodes.JsonObject? Describe() => new()
    {
        ["picture"] = $"{shownSize.w}x{shownSize.h}", ["loaded"] = original is not null, ["channel"] = shownChannel, ["fit"] = value?.Fit ?? "fit", ["want"] = src?.Want,
        ["detail"] = sharpened, ["reading"] = lastReading.Get("text").Str(), ["stats"] = stats.Describe(),
    };
}

/// <summary>An array's several pictures, each with its caption; its channels and their histograms above
/// (13.2.2), redrawn on every update (the log toggle changes the bars, not the pictures).</summary>
public sealed class ArrayPicturesViewer : IViewer
{
    readonly Pictures pictures;
    readonly ScrollViewer scroll = new();
    readonly StackPanel outer = new();
    readonly StackPanel panel = new() { Spacing = 8, Padding = new Thickness(12) };
    readonly ChannelStatsPanel stats;
    int count;
    string? key;

    public ArrayPicturesViewer(PopoutModel pop, Pictures pictures)
    {
        this.pictures = pictures;
        stats = new ChannelStatsPanel(pop);
        outer.Children.Add(stats.View);
        outer.Children.Add(panel);
        scroll.Content = outer;
    }

    public string Kind => "array.pictures";
    public FrameworkElement View => scroll;
    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        stats.Update(v);
        var views = v.Views ?? [];
        var k = string.Join("|", views.Select(p => Pictures.HandleOf(p.Ref)));
        if (k == key) return;
        key = k;
        count = views.Count;
        panel.Children.Clear();
        var wrap = new VariableSizedWrapGrid { Orientation = Orientation.Horizontal, ItemWidth = 240, ItemHeight = 210 };
        panel.Children.Add(wrap);
        foreach (var p in views)
        {
            var cell = new Grid { RowSpacing = 3, Margin = new Thickness(0, 0, 10, 10) };
            cell.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
            cell.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var frame = V.Frame(new ProgressRing { IsActive = true, Width = 24, Height = 24 });
            cell.Children.Add(frame);
            cell.Children.Add(Ui.Caption(p.Caption ?? "", wrap: false).At(0, 1));
            wrap.Children.Add(cell);
            if (Pictures.HandleOf(p.Ref) is { } h && await pictures.LoadAsync(h) is { } bmp)
            {
                // named by its caption, its value's handle as its status: which picture is shown, read back
                var img = V.Picture(bmp);
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(img, p.Caption is { Length: > 0 } cap ? cap : "picture");
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(img, "view-picture");
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetItemStatus(img, h);
                frame.Child = img;
            }
            else frame.Child = Ui.Caption("couldn't load");
        }
    }
    public System.Text.Json.Nodes.JsonObject? Describe() => new() { ["pictures"] = count, ["key"] = key, ["stats"] = stats.Describe() };
}

// ---------------- table and grid ----------------

/// <summary>A page of a table (`viewer.data table`: the core's sort and page, `viewer.table`) or of an array's
/// numbers (`viewer.data grid`: the plane and page, `viewer.grid`), in a virtualised list under a header of
/// the page's own column widths. A table's header sorts through the core. Which 64 columns of a wide grid
/// show is the shell's, as a scroll position is.</summary>
public sealed class TableViewer : IViewer
{
    readonly PopoutModel pop;
    readonly bool grid;
    readonly Grid g = new();
    readonly Grid header = new();
    readonly ListView list = new() { SelectionMode = ListViewSelectionMode.None, IsItemClickEnabled = false };
    readonly ScrollViewer across = new() { HorizontalScrollMode = ScrollMode.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollMode = ScrollMode.Disabled, VerticalScrollBarVisibility = ScrollBarVisibility.Disabled };
    readonly StackPanel top = new() { Spacing = 2, Padding = new Thickness(12, 6, 12, 4) };
    readonly ContentControl pager = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch };
    double[] widths = [];
    bool numericRows;
    bool[] numeric = [];
    string? asked;
    JsonElement? page;
    int fetches;
    public double LastPageMs { get; private set; }

    sealed record RowItem(ulong Index, string[] Cells);

    public TableViewer(PopoutModel pop, bool grid)
    {
        this.pop = pop;
        this.grid = grid;
        foreach (var h in new[] { GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto }) g.RowDefinitions.Add(new RowDefinition { Height = h });
        g.Children.Add(top);
        var inner = new Grid();
        inner.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        inner.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        header.Padding = new Thickness(12, 0, 0, 0);
        header.HorizontalAlignment = HorizontalAlignment.Left;
        list.HorizontalAlignment = HorizontalAlignment.Left;
        header.BorderBrush = Ui.Brush("DividerStrokeColorDefaultBrush");
        header.BorderThickness = new Thickness(0, 0, 0, 1);
        inner.Children.Add(header);
        inner.Children.Add(list.At(0, 1));
        across.Content = inner;
        g.Children.Add(across.At(0, 1));
        g.Children.Add(pager.At(0, 2));
        list.ItemTemplate = (DataTemplate)XamlReader.Load("<DataTemplate xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"><Grid/></DataTemplate>");
        list.ItemContainerStyle = ContainerStyle();
        list.ContainerContentChanging += Fill;
    }

    static Style ContainerStyle()
    {
        var s = new Style(typeof(ListViewItem));
        s.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(12, 0, 0, 0)));
        s.Setters.Add(new Setter(FrameworkElement.MinHeightProperty, 30.0));
        s.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Stretch));
        return s;
    }

    public string Kind => grid ? "array.grid" : "table.grid";
    public FrameworkElement View => g;
    public ListView List => list;
    public ScrollViewer? Inner => FindScroll(list);

    static ScrollViewer? FindScroll(DependencyObject d)
    {
        for (int i = 0; i < VisualTreeHelper.GetChildrenCount(d); i++)
        {
            var c = VisualTreeHelper.GetChild(d, i);
            if (c is ScrollViewer s) return s;
            if (FindScroll(c) is { } x) return x;
        }
        return null;
    }

    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        var part = grid ? "grid" : "table";
        var st = v.State.Get(part);
        var key = $"{pop.Window.Stamp}|{st?.GetRawText()}";
        if (key == asked && page is not null) return;
        asked = key;
        var t0 = System.Diagnostics.Stopwatch.StartNew();
        // the grid's window of columns is the core's (`viewer.grid {cols}`, `state.grid.col`); viewer.data reads it
        var r = await pop.DataAsync(part);
        if (key != asked) return;
        LastPageMs = t0.Elapsed.TotalMilliseconds;
        fetches++;
        if (r.Get("status").Str() != "ready")
        {
            page = null;
            top.Children.Clear();
            top.Children.Add(V.Message($"{(grid ? "These numbers" : "This table")} can't be shown: {r.Get("error").Str() ?? "the value couldn't be read"}"));
            list.ItemsSource = null;
            return;
        }
        page = r;
        Show(r, v);
    }

    void Show(JsonElement r, PopoutValue v)
    {
        var start = r.Get("start").U64() ?? 0;
        var cells = r.Get("cells").Items().ToList();
        string[] names;
        if (grid)
        {
            var c0 = r.Get("firstCol").U64() ?? 0;
            var width = cells.FirstOrDefault().Items().Count();
            names = [.. Enumerable.Range(0, width).Select(i => (c0 + (ulong)i).ToString(CultureInfo.InvariantCulture))];
            numeric = [.. names.Select(_ => true)];
        }
        else
        {
            var cols = r.Get("columns").Items().ToList();
            names = [.. cols.Select((c, i) => c.Get("name").Str() ?? $"c{i + 1}")];
            numeric = [.. cols.Select(c => c.Get("numeric").IsTrue())];
        }
        numericRows = grid;
        var rows = cells.Select((c, i) => new RowItem(start + (ulong)i, [.. c.Items().Select(x => x.Str() ?? x.GetRawText())])).ToList();
        // each column as wide as its header and the page's cells need, within reason
        widths = [.. names.Select((n, i) =>
        {
            var longest = rows.Take(200).Select(x => i < x.Cells.Length ? x.Cells[i].Length : 0).DefaultIfEmpty(0).Max();
            return Math.Clamp(Math.Max(n.Length + 3, longest) * 7.6 + 24, 64, 360);
        })];
        Header(names, v);
        Top(r, v);
        list.Width = widths.Sum() + (numericRows ? 64 : 0) + 40;
        list.ItemsSource = rows;
        pager.Content = V.PageBar("rows", start, rows.Count, r.Get("rows").U64() ?? 0, r.Get("page").U64() ?? 0, r.Get("pages").U64() ?? 1,
            async n => await pop.CommandAsync(grid ? "viewer.grid" : "viewer.table", ("page", Jsonv.Of(n))));
    }

    void Top(JsonElement r, PopoutValue v)
    {
        top.Children.Clear();
        if (!grid) { top.Visibility = Visibility.Collapsed; return; }
        top.Visibility = Visibility.Visible;
        var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        line.Children.Add(new TextBlock { Text = $"{r.Get("dtype").Str()} · {string.Join("×", r.Get("shape").Items().Select(x => x.U64()?.ToString(CultureInfo.InvariantCulture)))}", Style = Ui.TextStyle("CaptionTextBlockStyle"), VerticalAlignment = VerticalAlignment.Center });
        if (r.Get("planes").U64() is { } planes && planes > 1)
        {
            var plane = r.Get("plane").U64() ?? 0;
            var down = Ui.GlyphButton(Ui.G.Back, "Previous Plane", async () => await pop.CommandAsync("viewer.grid", ("plane", Jsonv.Of(plane == 0 ? 0 : plane - 1))), 12);
            var up = Ui.GlyphButton(Ui.G.Forward, "Next Plane", async () => await pop.CommandAsync("viewer.grid", ("plane", Jsonv.Of(Math.Min(plane + 1, planes - 1)))), 12);
            line.Children.Add(down);
            line.Children.Add(new TextBlock { Text = $"{r.Get("planeName").Str() ?? "plane"} {plane + 1} of {planes}", Style = Ui.TextStyle("CaptionTextBlockStyle"), VerticalAlignment = VerticalAlignment.Center });
            line.Children.Add(up);
        }
        // the window of columns the core shows (decision 26): a caption, and a step either way through
        // `viewer.grid {cols}`, each enabled as the page's prevCols and nextCols say; no column kept here
        var c0 = r.Get("firstCol").U64() ?? 0;
        var c1 = r.Get("endCol").U64() ?? c0;
        var prev = r.Get("prevCols").IsTrue();
        var next = r.Get("nextCols").IsTrue();
        if (prev || next)
        {
            var left = Ui.GlyphButton(Ui.G.Back, "Columns to the left", async () => await pop.CommandAsync("viewer.grid", ("cols", Jsonv.Of("prev"))), 12);
            left.IsEnabled = prev;
            var right = Ui.GlyphButton(Ui.G.Forward, "Columns to the right", async () => await pop.CommandAsync("viewer.grid", ("cols", Jsonv.Of("next"))), 12);
            right.IsEnabled = next;
            var all = r.Get("cols").U64();
            line.Children.Add(left);
            line.Children.Add(new TextBlock { Text = $"columns {c0}–{(c1 > 0 ? c1 - 1 : 0)}{(all is { } a ? $" of {a}" : "")}", Style = Ui.TextStyle("CaptionTextBlockStyle"), VerticalAlignment = VerticalAlignment.Center });
            line.Children.Add(right);
        }
        top.Children.Add(line);
        if (V.Stats(v.Stats) is { Length: > 0 } st) top.Children.Add(Ui.Caption(st));
    }

    void Header(string[] names, PopoutValue v)
    {
        header.Children.Clear();
        header.ColumnDefinitions.Clear();
        var st = v.State.Get("table");
        var sort = st.Get("sort").U64();
        var desc = st.Get("desc").IsTrue();
        int k = 0;
        if (numericRows)
        {
            header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(64) });
            header.Children.Add(new TextBlock { Text = "row", Style = Ui.TextStyle("CaptionTextBlockStyle"), VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(0, 6, 0, 6) });
            k = 1;
        }
        for (int i = 0; i < names.Length; i++)
        {
            header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(widths[i]) });
            var col = i;
            var text = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4, HorizontalAlignment = numeric[i] ? HorizontalAlignment.Right : HorizontalAlignment.Left };
            text.Children.Add(new TextBlock { Text = names[i], FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, TextTrimming = TextTrimming.CharacterEllipsis });
            if (!grid && sort == (ulong)i) text.Children.Add(Ui.Icon(desc ? Ui.G.Down : Ui.G.Up, 10));
            FrameworkElement cell;
            if (grid) cell = new Border { Child = text, Padding = new Thickness(8, 6, 8, 6) };
            else
            {
                var b = new Button { Content = text, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = numeric[i] ? HorizontalAlignment.Right : HorizontalAlignment.Left, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent), BorderThickness = new Thickness(0), Padding = new Thickness(8, 6, 8, 6) };
                ToolTipService.SetToolTip(b, $"Sort by {names[i]}");
                // a click on a header sorts by it, again the other way (the core sorts)
                b.Click += async (_, _) => await pop.CommandAsync("viewer.table", ("sort", Jsonv.Of((long)col)), ("desc", Jsonv.Of(sort == (ulong)col && !desc)));
                cell = b;
            }
            header.Children.Add(cell.At(i + k));
        }
    }

    void Fill(ListViewBase sender, ContainerContentChangingEventArgs e)
    {
        if (e.InRecycleQueue || e.Item is not RowItem r || e.ItemContainer.ContentTemplateRoot is not Grid cellGrid) return;
        var need = widths.Length + (numericRows ? 1 : 0);
        if (cellGrid.ColumnDefinitions.Count != need || cellGrid.Children.Count != need)
        {
            cellGrid.ColumnDefinitions.Clear();
            cellGrid.Children.Clear();
            if (numericRows) cellGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(64) });
            foreach (var w in widths) cellGrid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(w) });
            for (int i = 0; i < need; i++)
            {
                var t = new TextBlock { TextTrimming = TextTrimming.CharacterEllipsis, VerticalAlignment = VerticalAlignment.Center, Padding = new Thickness(8, 0, 8, 0), IsTextSelectionEnabled = false };
                cellGrid.Children.Add(t.At(i));
            }
        }
        int k = 0;
        if (numericRows)
        {
            var rt = (TextBlock)cellGrid.Children[0];
            rt.Text = r.Index.ToString(CultureInfo.InvariantCulture);
            rt.Foreground = Ui.Brush("TextFillColorSecondaryBrush");
            k = 1;
        }
        for (int i = 0; i < widths.Length; i++)
        {
            var t = (TextBlock)cellGrid.Children[i + k];
            t.Text = i < r.Cells.Length ? r.Cells[i] : "";
            t.TextAlignment = numeric[i] ? TextAlignment.Right : TextAlignment.Left;
            Microsoft.UI.Xaml.Documents.Typography.SetNumeralAlignment(t, numeric[i] ? Microsoft.UI.Xaml.FontNumeralAlignment.Tabular : Microsoft.UI.Xaml.FontNumeralAlignment.Normal);
        }
    }

    public System.Text.Json.Nodes.JsonObject? Describe() => page is not { } r ? new() { ["status"] = "loading" } : new()
    {
        ["status"] = "ready", ["rows"] = r.Get("rows").U64(), ["page"] = r.Get("page").U64(), ["pages"] = r.Get("pages").U64(), ["sort"] = r.Get("sort").U64(),
        ["desc"] = r.Get("desc").Bool(), ["first"] = r.Get("cells").At(0).At(0).Str(), ["fetches"] = fetches, ["pageMs"] = Math.Round(LastPageMs, 1),
    };
}

// ---------------- JSON: the outline and YAML ----------------

/// <summary>The JSON outline: the core opens and closes members (`viewer.tree {expand | collapse |
/// collapseAll}`) and lists every open member's children (`viewer.data tree`), drawn in the system's TreeView.</summary>
public sealed class TreeViewer : IViewer
{
    readonly PopoutModel pop;
    readonly Grid g = new();
    readonly TreeView tree = new() { SelectionMode = TreeViewSelectionMode.None, CanDragItems = false, CanReorderItems = false };
    readonly TextBlock rootText = Ui.Caption("", wrap: false);
    string? asked;
    bool following;
    int open;

    public TreeViewer(PopoutModel pop)
    {
        this.pop = pop;
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var bar = Ui.Columns(Ui.Star, Ui.Auto);
        bar.Padding = new Thickness(12, 6, 12, 4);
        rootText.VerticalAlignment = VerticalAlignment.Center;
        bar.Children.Add(rootText);
        var collapse = new HyperlinkButton { Content = "Collapse All" };
        collapse.Click += async (_, _) => await pop.CommandAsync("viewer.tree", ("collapseAll", Jsonv.True));
        bar.Children.Add(collapse.At(1));
        g.Children.Add(bar);
        g.Children.Add(tree.At(0, 1));
        tree.Expanding += async (_, e) =>
        {
            if (following || e.Node.Content is not Member m) return;
            await pop.CommandAsync("viewer.tree", ("expand", Jsonv.Of(m.Pointer)));
        };
        tree.Collapsed += async (_, e) =>
        {
            if (following || e.Node.Content is not Member m) return;
            await pop.CommandAsync("viewer.tree", ("collapse", Jsonv.Of(m.Pointer)));
        };
        // a node's member as "key  text" (the template shows the node's content); Copy Value on a right click
        tree.ItemTemplate = (DataTemplate)XamlReader.Load(
            "<DataTemplate xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"><TreeViewItem Content=\"{Binding Content}\"/></DataTemplate>");
        tree.RightTapped += (_, e) =>
        {
            if (e.OriginalSource is not FrameworkElement fe || Node(fe)?.Content is not Member m) return;
            var copy = new MenuFlyout();
            var c = new MenuFlyoutItem { Text = "Copy Value" };
            c.Click += async (_, _) => await pop.TakeOutAsync("value.copy", ("pointer", Jsonv.Of(m.Pointer)));
            copy.Items.Add(c);
            copy.ShowAt(fe, e.GetPosition(fe));
            e.Handled = true;
        };
    }

    /// <summary>The outline's node an element under the pointer belongs to.</summary>
    static TreeViewNode? Node(DependencyObject? d)
    {
        for (; d is not null; d = VisualTreeHelper.GetParent(d))
            if (d is FrameworkElement { DataContext: TreeViewNode n }) return n;
        return null;
    }

    sealed record Member(string Key, string Pointer, string Text, bool IsString)
    {
        public override string ToString() => $"{Key}  {Text}";
    }

    public string Kind => "tree";
    public FrameworkElement View => g;

    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        var key = $"{pop.Window.Stamp}|{v.State.Get("tree")?.GetRawText()}";
        if (key == asked) return;
        asked = key;
        var r = await pop.DataAsync("tree");
        if (key != asked) return;
        if (r.Get("status").Str() != "ready")
        {
            rootText.Text = $"This value can't be outlined: {r.Get("error").Str()}";
            return;
        }
        rootText.Text = r.Get("root").Get("text").Str() ?? "";
        var nodes = r.Get("nodes");
        following = true;
        tree.RootNodes.Clear();
        open = 0;
        foreach (var n in Children(nodes, "")) tree.RootNodes.Add(n);
        following = false;
    }

    IEnumerable<TreeViewNode> Children(JsonElement? nodes, string at)
    {
        var node = nodes.Get(at);
        if (node is null) yield break;
        open++;
        foreach (var c in node.Get("children").Items())
        {
            var pointer = c.Get("pointer").Str() ?? "";
            var kind = c.Get("kind").Str();
            var container = (kind is "object" or "array") && (c.Get("count").U64() ?? 0) > 0;
            var m = new Member(c.Get("key").Str() ?? "", pointer, c.Get("text").Str() ?? "", kind == "string");
            var tn = new TreeViewNode { Content = m };
            if (container)
            {
                var isOpen = nodes.Get(pointer) is not null;
                if (isOpen) foreach (var k in Children(nodes, pointer)) tn.Children.Add(k);
                else tn.HasUnrealizedChildren = true;
                tn.IsExpanded = isOpen;
            }
            yield return tn;
        }
        if (node.Get("more").U64() is { } more && more > 0)
            yield return new TreeViewNode { Content = new Member($"{more:N0} more", at, "not listed", false) };
    }

    public System.Text.Json.Nodes.JsonObject? Describe() => new() { ["open"] = open, ["root"] = rootText.Text, ["members"] = tree.RootNodes.Count };
}

public sealed class YamlViewer(PopoutModel pop) : IViewer
{
    readonly ScrollViewer scroll = new() { HorizontalScrollMode = ScrollMode.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto };
    readonly TextBlock text = new() { FontFamily = new FontFamily("Cascadia Mono, Consolas"), FontSize = 13, IsTextSelectionEnabled = true, Margin = new Thickness(14, 10, 14, 14) };
    string? asked;
    public string Kind => "yaml";
    public FrameworkElement View { get { scroll.Content ??= text; return scroll; } }
    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        var key = $"{pop.Window.Stamp}";
        if (key == asked) return;
        asked = key;
        var r = await pop.DataAsync("yaml");
        text.Text = r.Get("status").Str() == "ready" ? r.Get("text").Str() ?? "" : $"This value can't be shown as YAML: {r.Get("error").Str()}";
        if (r.Get("cut").IsTrue()) text.Text += $"\n… (the first {text.Text.Length:N0} of {r.Get("totalChars").U64():N0} characters)";
    }
    public System.Text.Json.Nodes.JsonObject? Describe() => new() { ["yaml"] = text.Text[..Math.Min(80, text.Text.Length)] };
}

// ---------------- bytes, items, opaque ----------------

public sealed class HexViewer : IViewer
{
    readonly PopoutModel pop;
    readonly Grid g = new();
    readonly TextBlock text = new() { FontFamily = new FontFamily("Cascadia Mono, Consolas"), FontSize = 12, IsTextSelectionEnabled = true, Margin = new Thickness(14, 10, 14, 10) };
    readonly ContentControl pager = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch };
    string? asked;
    public HexViewer(PopoutModel pop)
    {
        this.pop = pop;
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.Children.Add(new ScrollViewer { Content = text, HorizontalScrollMode = ScrollMode.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto });
        g.Children.Add(pager.At(0, 1));
    }
    public string Kind => "hex";
    public FrameworkElement View => g;
    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        var key = $"{pop.Window.Stamp}|{v.State.Get("hex")?.GetRawText()}";
        if (key == asked) return;
        asked = key;
        var r = await pop.DataAsync("hex");
        if (r.Get("status").Str() != "ready") { text.Text = $"These bytes can't be read: {r.Get("error").Str()}"; return; }
        text.Text = string.Join("\n", r.Get("lines").Items().Select(l => l.Str()));
        var start = r.Get("start").U64() ?? 0;
        var total = r.Get("bytes").U64() ?? 0;
        pager.Content = V.PageBar("bytes", start, (int)Math.Min(r.Get("pageBytes").U64() ?? 4096, total - Math.Min(start, total)), total, r.Get("page").U64() ?? 0, r.Get("pages").U64() ?? 1,
            async n => await pop.CommandAsync("viewer.hex", ("page", Jsonv.Of(n))));
    }
    public System.Text.Json.Nodes.JsonObject? Describe() => new() { ["lines"] = text.Text.Split('\n').Length };
}

/// <summary>A list's items as a sheet of thumbnails: one chosen shows that item (`viewer.sheet {on: false, index}`).</summary>
public sealed class ItemsViewer(PopoutModel pop, Pictures pictures) : IViewer
{
    readonly ScrollViewer scroll = new();
    int count;
    public string Kind => "items";
    public FrameworkElement View => scroll;
    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        var cells = v.Sheet.Items().ToList();
        count = cells.Count;
        var wrap = new VariableSizedWrapGrid { Orientation = Orientation.Horizontal, ItemWidth = 168, ItemHeight = 168, Margin = new Thickness(12) };
        scroll.Content = wrap;
        for (int i = 0; i < cells.Count; i++)
        {
            var c = cells[i];
            var index = i;
            var stack = new StackPanel { Spacing = 3 };
            var frame = V.Frame(new TextBlock());
            frame.Height = 120;
            stack.Children.Add(frame);
            stack.Children.Add(Ui.Caption(c.Get("label").Str() ?? $"item {i + 1}"));
            var b = new Button { Content = stack, Margin = new Thickness(0, 0, 8, 8), HorizontalContentAlignment = HorizontalAlignment.Stretch, Width = 160 };
            b.Click += async (_, _) => await pop.CommandAsync("viewer.sheet", ("on", Jsonv.False), ("index", Jsonv.Of((long)index)));
            wrap.Children.Add(b);
            if (c.Get("thumb").Get("ref").Get("handle").Str() is { } h && await pictures.LoadAsync(h) is { } bmp) frame.Child = V.Picture(bmp);
        }
    }
    public System.Text.Json.Nodes.JsonObject? Describe() => new() { ["items"] = count };
}

/// <summary>A value no viewer here draws: what it is (its type, media type and size), the summary, and the
/// engine's picture of it if it sent one. Its bytes are never guessed at.</summary>
public sealed class OpaqueViewer(Pictures pictures) : IViewer
{
    readonly StackPanel s = new() { Spacing = 8, Padding = new Thickness(14) };
    string? type;
    public string Kind => "opaque";
    public FrameworkElement View => s;
    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        s.Children.Clear();
        type = v.Type;
        var g = Ui.Columns(Ui.Auto, Ui.Star);
        g.RowSpacing = 4;
        int row = 0;
        void Line(string k, string? val)
        {
            if (val is not { Length: > 0 }) return;
            g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            g.Children.Add(new TextBlock { Text = k, Foreground = Ui.Brush("TextFillColorSecondaryBrush") }.At(0, row));
            g.Children.Add(new TextBlock { Text = val, IsTextSelectionEnabled = true, TextWrapping = TextWrapping.Wrap }.At(1, row));
            row++;
        }
        Line("Type", v.Type);
        Line("Media", v.MediaType);
        if (v.Bytes is { } b) Line("Size", b < 1024 ? $"{b} bytes" : b < 1 << 20 ? $"{b / 1024.0:0.#} KB" : $"{b / 1048576.0:0.#} MB");
        Line("Summary", v.Summary);
        if (v.Viewer is { } id) Line("Viewer", $"{id}: not drawn by this app; the engine's picture stands in");
        s.Children.Add(g);
        if (v.Picture is { } pic && (pic.Handle ?? Pictures.HandleOf(pic.Ref)) is { } h && await pictures.LoadAsync(h) is { } bmp) s.Children.Add(V.Picture(bmp));
        else s.Children.Add(Ui.Caption("The engine sent no picture of it."));
    }
    public System.Text.Json.Nodes.JsonObject? Describe() => new() { ["type"] = type };
}

// ---------------- sound ----------------

/// <summary>A sound: the waveform (peaks from every sample, the core's) with the playhead, the spectrogram
/// (a picture the core makes on its workers), and playback through Windows' MediaPlayer, fed a WAVE made
/// from the samples the core lends. Play, pause, a seek, the level and loop are the core's (`viewer.audio`);
/// the playing clock is the shell's. In a scripted run (a status file or a control port) the audio device
/// is never opened and the clock runs silent.</summary>
public sealed class AudioViewer : IViewer
{
    readonly PopoutModel pop;
    readonly Pictures pictures;
    readonly Grid g = new() { Padding = new Thickness(12), RowSpacing = 8 };
    readonly Button play = new() { MinWidth = 96 };
    readonly ToggleButton loop = new() { Content = "Loop" };
    readonly TextBlock clock = new() { VerticalAlignment = VerticalAlignment.Center, MinWidth = 96 };
    readonly Slider level = new() { Minimum = 0, Maximum = 1, StepFrequency = 0.01, Width = 120, IsThumbToolTipEnabled = false, VerticalAlignment = VerticalAlignment.Center };
    readonly CanvasControl wave = new() { MinHeight = 110 };
    readonly Border spectro = V.Frame(new ProgressRing { IsActive = true, Width = 24, Height = 24 });
    readonly TextBlock error = new() { Foreground = new SolidColorBrush(Microsoft.UI.Colors.IndianRed), TextWrapping = TextWrapping.Wrap };
    readonly DispatcherTimer tick = new() { Interval = TimeSpan.FromMilliseconds(33) };
    readonly bool muted = CoreHost.Options.Status is not null || CoreHost.Options.ControlPort is not null;
    float[][] peaks = [];
    string? handle;
    PopoutValue? value;
    // the player: MediaPlayer, or the silent clock when muted
    MediaPlayer? player;
    byte[]? wav;
    int frames, channels;
    double rate;
    bool playing;
    double startAt;
    System.Diagnostics.Stopwatch? since;
    ulong seq;
    bool spectrogram;

    public AudioViewer(PopoutModel pop, Pictures pictures)
    {
        this.pop = pop;
        this.pictures = pictures;
        foreach (var h in new[] { GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto })
            g.RowDefinitions.Add(new RowDefinition { Height = h });
        var bar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        bar.Children.Add(play);
        bar.Children.Add(loop);
        bar.Children.Add(clock);
        bar.Children.Add(Ui.Icon(Ui.G.Volume, 14));
        bar.Children.Add(level);
        if (muted) bar.Children.Add(new TextBlock { Text = "muted (scripted run)", Style = Ui.TextStyle("CaptionTextBlockStyle"), VerticalAlignment = VerticalAlignment.Center });
        g.Children.Add(bar);
        g.Children.Add(V.Frame(wave).At(0, 1));
        g.Children.Add(Ui.Caption("Spectrogram").At(0, 2));
        g.Children.Add(spectro.At(0, 3));
        g.Children.Add(error.At(0, 4));
        play.Click += async (_, _) =>
        {
            CoreHost.Log("sound: Play/Pause clicked");
            var st = value?.State.Get("audio");
            if (st.Get("playing").IsTrue()) await pop.CommandAsync("viewer.audio", ("play", Jsonv.False), ("at", Jsonv.Of(Position)));
            else await pop.CommandAsync("viewer.audio", ("play", Jsonv.True));
        };
        play.KeyboardAccelerators.Add(new KeyboardAccelerator { Key = Windows.System.VirtualKey.Space });
        // what reaches Play, for the autoplay question (N5 gate): pointer, keys, or neither
        play.AddHandler(UIElement.PointerPressedEvent, new PointerEventHandler((_, e) => CoreHost.Log($"sound: Play pressed by {e.Pointer.PointerDeviceType}")), true);
        play.AddHandler(UIElement.PointerReleasedEvent, new PointerEventHandler((_, e) => CoreHost.Log($"sound: Play released by {e.Pointer.PointerDeviceType}")), true);
        play.AddHandler(UIElement.KeyDownEvent, new KeyEventHandler((_, e) => CoreHost.Log($"sound: key {e.Key} on Play")), true);
        play.GotFocus += (_, e) => CoreHost.Log($"sound: Play got focus ({play.FocusState})");
        loop.Click += async (_, _) => await pop.CommandAsync("viewer.audio", ("loop", Jsonv.Of(loop.IsChecked == true)));
        level.AddHandler(UIElement.PointerReleasedEvent, new PointerEventHandler(async (_, _) => await pop.CommandAsync("viewer.audio", ("level", Jsonv.Of(level.Value)))), true);
        wave.Draw += DrawWave;
        wave.PointerReleased += async (_, e) =>
        {
            var x = e.GetCurrentPoint(wave).Position.X;
            if (Seconds > 0 && wave.ActualWidth > 0) await pop.CommandAsync("viewer.audio", ("at", Jsonv.Of(Math.Clamp(x / wave.ActualWidth * Seconds, 0, Seconds))));
        };
        tick.Tick += (_, _) => Tick();
        tick.Start();
    }

    public string Kind => "audio";
    public FrameworkElement View => g;
    double Seconds => rate > 0 ? frames / rate : 0;

    double Position
    {
        get
        {
            if (!playing || since is null) return startAt;
            var p = startAt + since.Elapsed.TotalSeconds;
            return loop.IsChecked == true && Seconds > 0 ? p % Seconds : Math.Min(p, Seconds);
        }
    }

    static string Clock(double s)
    {
        var t = Math.Max(0, double.IsFinite(s) ? s : 0);
        return $"{(int)t / 60}:{t % 60:00.0}";
    }

    public async void Update(PopoutValue v)
    {
        try { await UpdateAsync(v); }
        catch (Exception e) { CoreHost.Log($"viewer {Kind}: {e}"); }
    }

    async Task UpdateAsync(PopoutValue v)
    {
        value = v;
        var form = v.Form;
        rate = form.Get("sample_rate").Double() ?? 0;
        frames = (int)(form.Get("frames").Double() ?? 0);
        if (v.Handle is { } h && h != handle)
        {
            handle = h;
            peaks = [];
            Load(h);
        }
        Follow();
        await Task.CompletedTask;
    }

    async void Load(string h)
    {
        // the waveform, from every sample, on the core's workers
        if (await pictures.LendAsync(h, "waveform", 1600) is { } w)
        {
            var ch = (int)(w.note.Channels ?? 1);
            var bins = (int)(w.note.Bins ?? 0);
            var f = new float[w.bytes.Length / 4];
            Buffer.BlockCopy(w.bytes, 0, f, 0, f.Length * 4);
            peaks = [.. Enumerable.Range(0, ch).Select(c => f.Skip(c * bins * 2).Take(bins * 2).ToArray())];
            wave.Invalidate();
        }
        else error.Text = pictures.Failures.GetValueOrDefault(h) ?? "the waveform couldn't be made";
        // the spectrogram, a picture on the core's workers too
        if (await pictures.LoadAsync(h, "spectrogram") is { } sp)
        {
            spectro.Child = V.Picture(sp, Stretch.Fill);
            spectrogram = true;
        }
        else spectro.Child = Ui.Caption(pictures.Failures.GetValueOrDefault(h) ?? "no spectrogram");
        // the samples, made into a WAVE for the platform's player
        if (await pictures.LendAsync(h, "bytes") is { } s)
        {
            var shape = s.note.Shape ?? [];
            frames = shape.Count > 0 ? (int)shape[0] : frames;
            channels = shape.Count > 1 ? (int)shape[^1] : 1;
            if (rate <= 0 || frames <= 0) { error.Text = "this sound has no samples to play"; return; }
            var bytes = s.bytes;
            var (ch, fr, rt) = (channels, frames, rate);
            wav = await Task.Run(() => PlumCraft.Kit.Sound.Wave(bytes, ch, fr, (int)rt));
            // PLUMMER_DUMP_WAV=<folder>: the samples as lent and the WAVE handed to MediaPlayer, for checking
            if (Environment.GetEnvironmentVariable("PLUMMER_DUMP_WAV") is { Length: > 0 } dump)
            {
                try
                {
                    Directory.CreateDirectory(dump);
                    File.WriteAllBytes(Path.Combine(dump, "samples.f32"), bytes);
                    File.WriteAllBytes(Path.Combine(dump, "player.wav"), wav);
                    File.WriteAllText(Path.Combine(dump, "note.txt"), $"shape [{string.Join(", ", shape)}] channels {ch} frames {fr} rate {rt} bytes {bytes.Length} form {value?.Form}");
                }
                catch (IOException e) { CoreHost.Log($"dumping the sound: {e.Message}"); }
            }
            Follow();
        }
        else error.Text = pictures.Failures.GetValueOrDefault(h) ?? "the samples couldn't be read";
    }

    /// <summary>The core's state: playing or not, from where, the level and loop.</summary>
    void Follow()
    {
        var st = value?.State.Get("audio");
        var want = st.Get("playing").IsTrue();
        var s = st.Get("seq").U64() ?? 0;
        if (want != playing) CoreHost.Log($"sound: the core's state says playing {want} (seq {s}, at {st.Get("at").Double()})");
        var at = (st.Get("at").Double() ?? 0);
        loop.IsChecked = st.Get("loop").IsTrue();
        var lv = st.Get("level").Double() ?? 1;
        if (Math.Abs(level.Value - lv) > 1e-6) level.Value = lv;
        play.Content = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, Children = { Ui.Icon(want ? Ui.G.Pause : Ui.G.Play, 14), new TextBlock { Text = want ? "Pause" : "Play" } } };
        if (want && (!playing || s != seq)) Start(at);
        else if (!want && playing) Stop(at);
        else if (!want && s != seq) startAt = at;
        seq = s;
        if (player is not null)
        {
            player.Volume = lv;
            player.IsLoopingEnabled = loop.IsChecked == true;
        }
    }

    void Start(double at)
    {
        startAt = at;
        playing = true;
        since = System.Diagnostics.Stopwatch.StartNew();
        if (muted || wav is null) return;
        try
        {
            if (player is null)
            {
                var stream = new Windows.Storage.Streams.InMemoryRandomAccessStream();
                var bytes = wav;
                var writer = new Windows.Storage.Streams.DataWriter(stream);
                writer.WriteBytes(bytes);
                _ = writer.StoreAsync().AsTask().ContinueWith(_ => { });
                stream.Seek(0);
                player = new MediaPlayer { AutoPlay = false, Source = MediaSource.CreateFromStream(stream, "audio/wav") };
                player.MediaEnded += (_, _) => g.DispatcherQueue.TryEnqueue(async () => await pop.CommandAsync("viewer.audio", ("play", Jsonv.False), ("at", Jsonv.Of(0.0))));
            }
            player.PlaybackSession.Position = TimeSpan.FromSeconds(at);
            player.Play();
        }
        catch (Exception e) { error.Text = $"Windows couldn't play it: {e.Message}"; }
    }

    void Stop(double at)
    {
        startAt = at;
        playing = false;
        since = null;
        player?.Pause();
    }

    void Tick()
    {
        clock.Text = $"{Clock(Position)} / {Clock(Seconds)}";
        if (playing && muted && loop.IsChecked != true && Position >= Seconds && Seconds > 0)
        {
            playing = false;
            _ = pop.CommandAsync("viewer.audio", ("play", Jsonv.False), ("at", Jsonv.Of(0.0)));
        }
        if (playing) wave.Invalidate();
    }

    void DrawWave(CanvasControl sender, CanvasDrawEventArgs e)
    {
        var ds = e.DrawingSession;
        var w = (float)sender.ActualWidth;
        var h = (float)sender.ActualHeight;
        var lanes = peaks.Take(8).ToArray();
        var accent = (Windows.UI.Color)Application.Current.Resources["SystemAccentColor"];
        if (lanes.Length > 0)
        {
            var lh = h / lanes.Length;
            for (int li = 0; li < lanes.Length; li++)
            {
                var lane = lanes[li];
                var bins = lane.Length / 2;
                if (bins == 0) continue;
                var mid = lh * (li + 0.5f);
                var stroke = MathF.Max(1, w / bins);
                for (int b = 0; b < bins; b++)
                {
                    var x = (float)b / bins * w;
                    var lo = Math.Clamp(lane[b * 2], -1, 1);
                    var hi = Math.Clamp(lane[b * 2 + 1], -1, 1);
                    ds.DrawLine(x, mid - hi * lh / 2, x, mid - lo * lh / 2 + 0.5f, accent, stroke);
                }
            }
        }
        if (Seconds > 0)
        {
            var x = (float)(Position / Seconds) * w;
            ds.FillRectangle(x - 0.75f, 0, 1.5f, h, sender.ActualTheme == ElementTheme.Dark ? Microsoft.UI.Colors.White : Microsoft.UI.Colors.Black);
        }
    }

    public void Gone()
    {
        tick.Stop();
        player?.Dispose();
        player = null;
    }

    public System.Text.Json.Nodes.JsonObject? Describe() => new()
    {
        ["playing"] = playing, ["muted"] = muted, ["at"] = Math.Round(Position, 2), ["seconds"] = Math.Round(Seconds, 3), ["peaks"] = peaks.FirstOrDefault()?.Length / 2 ?? 0,
        ["spectrogram"] = spectrogram, ["wav"] = wav?.Length ?? 0, ["player"] = player is null ? "none" : "MediaPlayer",
    };
}
