// Watching shows the value in a pop-out window (contract 12.20: the maintainer, "Eliminate the side
// column in favor of just the popout window"). The core keeps which values are popped out (the scene's
// `view.windows.popouts`); each is a window here, titled with the node and port, brought forward when
// the core asks again (`raised`), read again when its `stamp` changes (`popout.get`), closed with its
// close button (`view.close`) or when the core closes it. Its controls are the core's commands
// (`viewer.zoom`, `viewer.text`).
//
// The viewers of slice 2, plan 8's first three:
//
//   picture     the rgba8 rendition (media.get, plum_blob) in a zoomable view; Fit and Actual Size are
//               the core's `viewer.zoom`
//   text.plain  selectable text; Wrap, Monospaced and Show All are the core's `viewer.text`, the whole
//               text read from the full value's bytes (media.get {want: "bytes"})
//   scalar      the value, formatted
//
// Any other kind shows its kind and summary (slice 4 adds the rest).

using System.Globalization;
using System.Text;
using System.Text.Json;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using PlumCraft.App.Canvas;
using PlumCraft.Kit;
using PlumCraft.Models;

namespace PlumCraft.App;

/// <summary>The pop-out windows of one document, kept in step with the core's.</summary>
public sealed class Popouts(DocWindow owner, DocumentModel model, Pictures pictures)
{
    readonly Dictionary<ulong, ValueWindow> open = [];
    Dictionary<ulong, Seen> known = [];

    sealed record Seen(ulong Raised, ulong Stamp);

    public IReadOnlyDictionary<ulong, ValueWindow> Open => open;

    public void Follow(DocumentWindows w)
    {
        var list = w.Popouts ?? [];
        var now = list.ToDictionary(p => p.Id);
        foreach (var id in open.Keys.ToList())
            if (!now.ContainsKey(id)) { open[id].CloseFromCore(); open.Remove(id); }
        foreach (var p in list)
        {
            if (!open.TryGetValue(p.Id, out var win))
            {
                win = new ValueWindow(owner, model, pictures, p);
                win.ClosedByPerson += () => open.Remove(p.Id);
                open[p.Id] = win;
                win.Place(open.Count - 1);
                win.Activate();
                win.Refresh();
            }
            else
            {
                if (known.TryGetValue(p.Id, out var k))
                {
                    if (k.Stamp != p.Stamp) win.Refresh();
                    if (k.Raised != p.Raised) win.Activate();
                }
                win.Entry = p;
            }
        }
        known = list.ToDictionary(p => p.Id, p => new Seen(p.Raised, p.Stamp));
        owner.NoteStatus();
    }

    public void CloseAll()
    {
        foreach (var w in open.Values) w.CloseFromCore();
        open.Clear();
    }

    /// <summary>What each window shows, for scripts (the status file).</summary>
    public System.Text.Json.Nodes.JsonArray Describe() =>
        [.. open.Values.Select(w => (System.Text.Json.Nodes.JsonNode)w.Describe())];
}

public sealed class ValueWindow : Window
{
    readonly DocWindow owner;
    readonly DocumentModel model;
    readonly Pictures pictures;
    public PlumCraft.Models.PopoutWindow Entry { get; set; }
    public event Action? ClosedByPerson;
    readonly TextBlock summary = new() { Opacity = 0.75, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(12, 8, 12, 4), IsTextSelectionEnabled = true };
    readonly CommandBar bar = new() { DefaultLabelPosition = CommandBarDefaultLabelPosition.Right, HorizontalAlignment = HorizontalAlignment.Left, IsDynamicOverflowEnabled = false };
    readonly Grid body = new();
    PopoutValue? value;
    bool closingFromCore, busy, again;
    string shownKind = "";
    readonly Dictionary<string, string> wholeText = [];

    public ValueWindow(DocWindow owner, DocumentModel model, Pictures pictures, PlumCraft.Models.PopoutWindow entry)
    {
        this.owner = owner;
        this.model = model;
        this.pictures = pictures;
        Entry = entry;
        Title = entry.Title;
        SystemBackdrop = new Microsoft.UI.Xaml.Media.MicaBackdrop();
        var root = new Grid { RequestedTheme = Appearance.Theme };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.Children.Add(summary);
        Grid.SetRow(bar, 1);
        root.Children.Add(bar);
        Grid.SetRow(body, 2);
        body.Margin = new Thickness(12, 4, 12, 12);
        root.Children.Add(body);
        Content = root;
        AppWindow.Resize(new Windows.Graphics.SizeInt32 { Width = 520, Height = 440 });
        if (CoreHost.Options.Top && AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter op) op.IsAlwaysOnTop = true;
        AppWindow.Closing += (s, e) =>
        {
            if (closingFromCore) return;
            // the person closed it: the core's pop-out goes too
            _ = model.RunAsync("view.close", Jsonv.Object(("id", Jsonv.Of(Entry.Id))));
            ClosedByPerson?.Invoke();
        };
    }

    /// <summary>Over its document's window, from its right side: the first four side by side, two by two,
    /// so each shows; more cascade over them.</summary>
    public void Place(int k)
    {
        var o = owner.AppWindow.Position;
        var s = owner.AppWindow.Size;
        int w = 460, h = 330;
        AppWindow.Resize(new Windows.Graphics.SizeInt32 { Width = w, Height = h });
        var col = k % 2;
        var row = (k / 2) % 2;
        var extra = k / 4 * 28;
        var x = Math.Max(0, o.X + s.Width - (w + 20) * (col + 1) + extra);
        var y = Math.Max(0, o.Y + 120 + (h + 10) * row + extra);
        AppWindow.Move(new Windows.Graphics.PointInt32 { X = x, Y = y });
    }

    public void CloseFromCore()
    {
        closingFromCore = true;
        Close();
    }

    /// <summary>Read the value again (popout.get), coalesced.</summary>
    public async void Refresh()
    {
        if (busy) { again = true; return; }
        busy = true;
        try
        {
            var v = await model.PopoutAsync(Entry.Id);
            value = v;
            await Show(v);
        }
        catch (Exception e)
        {
            summary.Text = $"Couldn't read the value: {e.Message}";
        }
        busy = false;
        owner.NoteStatus();
        if (again) { again = false; Refresh(); }
    }

    async Task Run(string command, params (string, JsonElement)[] p)
    {
        try { await model.RunAsync(command, Jsonv.Object([("popout", Jsonv.Of(Entry.Id)), .. p])); }
        catch (Exception e) { summary.Text = e.Message; }
        Refresh();
    }

    AppBarButton Button(string label, Symbol icon, Func<Task> run)
    {
        var b = new AppBarButton { Label = label, Icon = new SymbolIcon(icon) };
        b.Click += async (_, _) => await run();
        return b;
    }

    AppBarToggleButton Toggle(string label, bool on, Func<bool, Task> run)
    {
        var b = new AppBarToggleButton { Label = label, IsChecked = on };
        b.Click += async (_, _) => await run(b.IsChecked == true);
        return b;
    }

    async Task Show(PopoutValue v)
    {
        Title = v.Title ?? Entry.Title;
        summary.Text = v.Summary ?? (v.Media is { } m ? $"{m}, not computed yet" : "not computed yet");
        var viewer = v.Viewer ?? Entry.Viewer ?? "";
        bar.PrimaryCommands.Clear();
        shownKind = viewer;
        if (v.Error is { } err && err.ValueKind != JsonValueKind.Null)
        {
            body.Children.Clear();
            body.Children.Add(new TextBlock { Text = err.ValueKind == JsonValueKind.String ? err.GetString() : err.GetRawText(), TextWrapping = TextWrapping.Wrap, Foreground = new SolidColorBrush(Microsoft.UI.Colors.IndianRed), IsTextSelectionEnabled = true });
            return;
        }
        switch (viewer)
        {
            case "picture":
                bar.PrimaryCommands.Add(Button("Fit", Symbol.ZoomOut, () => Run("viewer.zoom", ("to", Jsonv.Of("fit")))));
                bar.PrimaryCommands.Add(Button("Actual Size", Symbol.FullScreen, () => Run("viewer.zoom", ("to", Jsonv.Of("1:1")))));
                await ShowPicture(v);
                break;
            case "text.plain":
                var tv = v.Text;
                bar.PrimaryCommands.Add(Toggle("Wrap", tv?.Wrap ?? true, on => Run("viewer.text", ("wrap", Jsonv.Of(on)))));
                bar.PrimaryCommands.Add(Toggle("Monospaced", tv?.Mono ?? false, on => Run("viewer.text", ("mono", Jsonv.Of(on)))));
                bar.PrimaryCommands.Add(Toggle("Show All", tv?.All ?? false, on => Run("viewer.text", ("all", Jsonv.Of(on)))));
                await ShowText(v);
                break;
            case "scalar":
                body.Children.Clear();
                body.Children.Add(new TextBlock
                {
                    Text = Scalar(v.Value, v.Summary), FontSize = 40, FontWeight = Microsoft.UI.Text.FontWeights.SemiLight,
                    HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, IsTextSelectionEnabled = true,
                });
                break;
            default:
                body.Children.Clear();
                body.Children.Add(new TextBlock { Text = $"PlumCraft for Windows has no {(viewer.Length > 0 ? viewer : "viewer")} viewer yet.\n{v.Summary}", TextWrapping = TextWrapping.Wrap, Opacity = 0.8 });
                break;
        }
    }

    /// <summary>A scalar, formatted: grouping, a whole number without decimals, up to six otherwise.</summary>
    public static string Scalar(JsonElement? value, string? summary)
    {
        if (value is not { } v || v.ValueKind == JsonValueKind.Null || v.ValueKind == JsonValueKind.Undefined) return summary ?? "—";
        switch (v.ValueKind)
        {
            case JsonValueKind.True: return "true";
            case JsonValueKind.False: return "false";
            case JsonValueKind.String: return v.GetString() ?? "";
            case JsonValueKind.Number:
                if (v.TryGetInt64(out var n)) return n.ToString("N0", CultureInfo.CurrentCulture);
                if (v.TryGetDouble(out var d)) return d.ToString("#,##0.######", CultureInfo.CurrentCulture);
                return v.GetRawText();
            default: return v.GetRawText();
        }
    }

    async Task ShowPicture(PopoutValue v)
    {
        var handle = v.Picture?.Handle ?? (v.Picture is { } pic ? Pictures.HandleOf(pic.Ref) : null);
        if (handle is null)
        {
            body.Children.Clear();
            body.Children.Add(new TextBlock { Text = "No picture yet: run the graph.", Opacity = 0.7 });
            return;
        }
        var bmp = await pictures.LoadAsync(handle);
        if (bmp is null)
        {
            body.Children.Clear();
            body.Children.Add(new TextBlock { Text = $"Couldn't read the picture: {pictures.LastFailure}", TextWrapping = TextWrapping.Wrap });
            return;
        }
        var w = (float)bmp.SizeInPixels.Width;
        var h = (float)bmp.SizeInPixels.Height;
        var src = new CanvasImageSource(CanvasDevice.GetSharedDevice(), w, h, 96);
        using (var ds = src.CreateDrawingSession(Microsoft.UI.Colors.Transparent)) ds.DrawImage(bmp);
        var actual = v.Fit == "1:1";
        var scale = Content?.XamlRoot?.RasterizationScale ?? 1;
        var img = new Image
        {
            Source = src, Stretch = actual ? Stretch.None : Stretch.Uniform,
            HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center,
        };
        if (actual) { img.Width = w / scale; img.Height = h / scale; }
        var sv = new ScrollViewer
        {
            Content = img, ZoomMode = ZoomMode.Enabled, MinZoomFactor = 0.1f, MaxZoomFactor = 16,
            HorizontalScrollBarVisibility = actual ? ScrollBarVisibility.Auto : ScrollBarVisibility.Disabled,
            VerticalScrollBarVisibility = actual ? ScrollBarVisibility.Auto : ScrollBarVisibility.Disabled,
            HorizontalScrollMode = ScrollMode.Auto, VerticalScrollMode = ScrollMode.Auto,
        };
        body.Children.Clear();
        body.Children.Add(sv);
    }

    async Task ShowText(PopoutValue v)
    {
        var tv = v.Text;
        var text = v.Excerpt?.Text ?? "";
        var total = (long)(v.Excerpt?.TotalChars ?? (ulong)text.Length);
        var handle = v.Data is { } d ? Pictures.HandleOf(d) : null;
        if (tv?.All == true && handle is not null && total > text.Length)
        {
            if (!wholeText.TryGetValue(handle, out var whole))
            {
                whole = await Whole(handle);
                if (whole is not null) wholeText[handle] = whole;
            }
            if (whole is not null) text = whole;
        }
        var block = new TextBlock
        {
            Text = text, IsTextSelectionEnabled = true, TextWrapping = tv?.Wrap == false ? TextWrapping.NoWrap : TextWrapping.Wrap,
            FontFamily = new FontFamily(tv?.Mono == true ? "Consolas" : FaceText.Family), FontSize = 13,
        };
        var panel = new StackPanel { Spacing = 6 };
        panel.Children.Add(block);
        if (text.Length < total) panel.Children.Add(new TextBlock { Text = $"{text.Length:N0} of {total:N0} characters shown", Opacity = 0.6 });
        body.Children.Clear();
        body.Children.Add(new ScrollViewer
        {
            Content = panel, HorizontalScrollBarVisibility = tv?.Wrap == false ? ScrollBarVisibility.Auto : ScrollBarVisibility.Disabled,
            HorizontalScrollMode = ScrollMode.Auto,
        });
    }

    /// <summary>The value's own bytes as UTF-8 (media.get {want: "bytes"}, then plum_blob), at most 8 MB.</summary>
    async Task<string?> Whole(string handle)
    {
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

    public System.Text.Json.Nodes.JsonObject Describe() => new()
    {
        ["id"] = Entry.Id, ["title"] = Title, ["viewer"] = shownKind, ["summary"] = summary.Text,
        ["value"] = value?.Viewer == "scalar" ? Scalar(value.Value, value.Summary) : null,
        ["text"] = value?.Excerpt?.Text is { } t ? t[..Math.Min(t.Length, 200)] : null,
        ["picture"] = value?.Picture is { } p ? $"{p.W}x{p.H}" : null,
    };
}
