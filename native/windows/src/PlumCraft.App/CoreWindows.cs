// The windows a document's core keeps and draws as models (N5 slice 4; the Mac app's MessagesWindow and
// CoreWindows, PlumCraftShell's ReportView, CompareView and MessagesView): Messages (`view.messages`,
// `messages.get`), the Cache Report (`run.cacheReport`), the graph's Parameters window (`view.params`), a
// value editor (`node.editor`), all three `report.get`, and Compare (`viewer.compare`, `compare.get`).
// Each is a window of its own while the scene's `view.windows` says it is open, opened by the core's
// command (a menu item, a script), closed by its close button (the core's close command) or by the core;
// every button and field in it is a core command. The shell never closes a window the core opened: a
// window is closed here only when the core's windows no longer have it, or the person closed it.

using System.Text.Json;
using Microsoft.Graphics.Canvas;
using Microsoft.Graphics.Canvas.UI.Xaml;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using PlumCraft.App.Canvas;
using PlumCraft.Kit;
using PlumCraft.Models;
using Border = Microsoft.UI.Xaml.Controls.Border;
using Windows.System;

namespace PlumCraft.App;

/// <summary>A document's report, Compare and Messages windows, kept in step with the core's.</summary>
public sealed class CoreWindows(DocWindow owner, DocumentModel model, Pictures pictures)
{
    readonly Dictionary<string, PanelWindow> open = new(StringComparer.Ordinal);
    PanelWindow? messages;
    PanelWindow? acknowledgements;
    SettingsWindow? settings;

    public void Follow(DocumentWindows w)
    {
        var keys = CoreWindowKeys.Of(w);
        foreach (var k in keys.Where(k => !open.ContainsKey(k)).ToList())
        {
            PanelWindow win = k == "compare" ? new CompareWindow(owner, new CompareModel(model), pictures) : new ReportWindow(owner, new ReportModel(model, k));
            win.ClosedByPerson += () => { open.Remove(k); owner.NoteStatus(); };
            open[k] = win;
            Ui.Prepare(win, owner, win.Size.w, win.Size.h, open.Count - 1);
            win.Activate();
        }
        foreach (var k in open.Keys.Where(k => !keys.Contains(k)).ToList())
        {
            open[k].CloseFromCore();
            open.Remove(k);
        }
        foreach (var win in open.Values) win.Follow(w.Reports);
        // Messages
        if (w.Messages == true && messages is null)
        {
            var m = new MessagesWindow(owner, new MessagesModel(model));
            m.ClosedByPerson += () => { messages = null; owner.NoteStatus(); };
            messages = m;
            Ui.Prepare(m, owner, m.Size.w, m.Size.h);
            m.Activate();
        }
        else if (w.Messages != true && messages is not null)
        {
            messages.CloseFromCore();
            messages = null;
        }
        // Acknowledgements (FR4)
        if (w.Acknowledgements == true && acknowledgements is null)
        {
            var a = new AcknowledgementsWindow(owner, new AcknowledgementsModel(model));
            a.ClosedByPerson += () => { acknowledgements = null; owner.NoteStatus(); };
            acknowledgements = a;
            Ui.Prepare(a, owner, a.Size.w, a.Size.h);
            a.Activate();
        }
        else if (w.Acknowledgements != true && acknowledgements is not null)
        {
            acknowledgements.CloseFromCore();
            acknowledgements = null;
        }
        // Settings: open on the core's section
        if (w.Settings is { } tab)
        {
            if (settings is null)
            {
                settings = new SettingsWindow(owner, new SettingsModel(model));
                settings.ClosedByPerson += () => { settings = null; owner.NoteStatus(); };
                Ui.Prepare(settings, owner, 760, 620);
                settings.Activate();
            }
            settings.ShowTab(tab);
        }
        else if (settings is not null)
        {
            settings.CloseFromCore();
            settings = null;
        }
        owner.NoteStatus();
    }

    public void CloseAll()
    {
        foreach (var w in open.Values) w.CloseFromCore();
        open.Clear();
        messages?.CloseFromCore();
        messages = null;
        acknowledgements?.CloseFromCore();
        acknowledgements = null;
        settings?.CloseFromCore();
        settings = null;
    }

    public System.Text.Json.Nodes.JsonObject Describe()
    {
        var o = new System.Text.Json.Nodes.JsonObject();
        foreach (var (k, w) in open) o[k] = w.Describe();
        if (messages is not null) o["messages"] = messages.Describe();
        if (acknowledgements is not null) o["acknowledgements"] = acknowledgements.Describe();
        if (settings is not null) o["settings"] = settings.Describe();
        return o;
    }
}

/// <summary>A window showing one of the core's models: closed by the person (the core told) or the core.</summary>
public abstract class PanelWindow : Window
{
    protected readonly DocWindow Owner;
    bool closingFromCore;
    public event Action? ClosedByPerson;
    public virtual (int w, int h) Size => (560, 600);
    /// <summary>Closed: what comes for it afterwards (a reply, a feed batch) is dropped.</summary>
    protected bool Gone { get; private set; }

    protected PanelWindow(DocWindow owner, string title)
    {
        Owner = owner;
        Title = title;
        Closed += (_, _) => Gone = true;
        AppWindow.Closing += async (_, e) =>
        {
            if (closingFromCore) return;
            e.Cancel = true;
            await PersonClosesAsync();
        };
    }

    /// <summary>The person closed it (the close button, Escape): the core's window goes too, then this one,
    /// and the document's window comes back to the front.</summary>
    public async Task PersonClosesAsync()
    {
        if (closingFromCore) return;
        closingFromCore = true;
        ClosedByPerson?.Invoke();
        await CloseInCoreAsync();
        Close();
        Owner.Activate();
    }

    protected abstract Task CloseInCoreAsync();

    public virtual void Follow(ulong? stamp) { }

    public void CloseFromCore()
    {
        closingFromCore = true;
        Close();
    }

    /// <summary>The root every panel window has: its appearance, Escape closing it as the close button does.</summary>
    protected Grid Root(UIElement body)
    {
        var g = new Grid { RequestedTheme = Appearance.Theme, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        g.Children.Add(body);
        g.KeyDown += async (_, e) =>
        {
            if (e.Key == VirtualKey.Escape && !e.Handled && e.OriginalSource is not TextBox)
            {
                e.Handled = true;
                await PersonClosesAsync();
            }
        };
        return g;
    }

    public abstract System.Text.Json.Nodes.JsonObject Describe();
}

/// <summary>The Cache Report, the graph's Parameters window, or a value editor: `report.get`'s sections of
/// rows (a label and value, a detail, its fields and buttons), in the Windows 11 Settings look.</summary>
public sealed class ReportWindow : PanelWindow
{
    readonly ReportModel report;
    readonly StackPanel body = new() { Spacing = 0, Padding = new Thickness(20, 12, 20, 20) };
    readonly TextBlock heading = new() { Style = Ui.TextStyle("TitleTextBlockStyle"), Margin = new Thickness(20, 16, 20, 0) };

    public override (int w, int h) Size => report.Key == "params" ? (520, 480) : (600, 640);

    public ReportWindow(DocWindow owner, ReportModel report) : base(owner, report.Key == "cache" ? "Cache Report" : report.Key == "params" ? "Parameters" : "Value Editor")
    {
        this.report = report;
        var g = new Grid();
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        g.Children.Add(heading);
        g.Children.Add(new ScrollViewer { Content = body }.At(0, 1));
        Content = Root(g);
        report.Changed += Show;
        report.Reload();
        heading.Text = report.Title;
    }

    protected override Task CloseInCoreAsync() => report.CloseAsync();

    public override void Follow(ulong? stamp) => report.Follow(stamp);

    void Show()
    {
        if (Gone) return;
        Title = report.Title;
        heading.Text = report.Title;
        body.Children.Clear();
        var r = report.Reply;
        if (r is null)
        {
            body.Children.Add(report.Error is { } e ? Ui.Caption(e) : new ProgressRing { IsActive = true, HorizontalAlignment = HorizontalAlignment.Left });
            return;
        }
        if (r.Get("empty").Str() is { } empty)
        {
            var none = new StackPanel { Spacing = 8, Margin = new Thickness(0, 48, 0, 0), HorizontalAlignment = HorizontalAlignment.Center };
            none.Children.Add(new FontIcon { Glyph = report.Key == "cache" ? Ui.G.Disk : Ui.G.Settings, FontFamily = new FontFamily(Ui.IconFont), FontSize = 36, Foreground = Ui.Brush("TextFillColorSecondaryBrush") });
            none.Children.Add(new TextBlock { Text = empty, TextWrapping = TextWrapping.Wrap, TextAlignment = TextAlignment.Center, Foreground = Ui.Brush("TextFillColorSecondaryBrush"), MaxWidth = 360 });
            body.Children.Add(none);
            return;
        }
        if (r.Get("intro").Str() is { } intro) body.Children.Add(Ui.Caption(intro));
        if (report.Error is { } err) body.Children.Add(new TextBlock { Text = err, Foreground = new SolidColorBrush(Microsoft.UI.Colors.IndianRed), TextWrapping = TextWrapping.Wrap });
        foreach (var s in r.Get("sections").Items())
        {
            if (s.Get("title").Str() is { Length: > 0 } t) body.Children.Add(Ui.Heading(t));
            int i = 0;
            foreach (var row in s.Get("rows").Items())
                body.Children.Add(Row(row, row.Get("id").Str() ?? $"{s.Get("title").Str()}#{i++}"));
        }
        Owner.NoteStatus();
    }

    FrameworkElement Row(JsonElement row, string id)
    {
        var col = new StackPanel { Spacing = 6 };
        var line = Ui.Columns(Ui.Star, Ui.Auto);
        line.Children.Add(new TextBlock { Text = row.Get("label").Str() ?? "", TextWrapping = TextWrapping.Wrap, VerticalAlignment = VerticalAlignment.Center });
        var value = new TextBlock
        {
            Text = row.Get("value").Str() ?? "", IsTextSelectionEnabled = true, TextAlignment = TextAlignment.Right, TextWrapping = TextWrapping.Wrap, MaxWidth = 280,
            Foreground = row.Get("warn").IsTrue() ? new SolidColorBrush(Microsoft.UI.Colors.DarkOrange) : Ui.Brush("TextFillColorSecondaryBrush"),
            VerticalAlignment = VerticalAlignment.Center,
        };
        line.Children.Add(value.At(1));
        col.Children.Add(line);
        if (row.Get("detail").Str() is { Length: > 0 } d)
        {
            var dt = Ui.Caption(d);
            dt.IsTextSelectionEnabled = true;
            col.Children.Add(dt);
        }
        foreach (var e in row.Get("edits").Items()) col.Children.Add(Field(e, id));
        var actions = row.Get("actions").Items().ToList();
        if (actions.Count > 0)
        {
            var bar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
            foreach (var a in actions)
            {
                var label = a.Get("label").Str() ?? "";
                var b = new Button { Content = label };
                if (a.Get("destructive").IsTrue()) b.Foreground = new SolidColorBrush(Microsoft.UI.Colors.IndianRed);
                ToolTipService.SetToolTip(b, a.Get("help").Str() ?? label);
                b.Click += async (_, _) => await report.RunAsync(a);
                bar.Children.Add(b);
            }
            col.Children.Add(bar);
        }
        return Ui.Card(col);
    }

    /// <summary>A field whose text a command takes: Enter for one line; Apply and Revert for several.</summary>
    FrameworkElement Field(JsonElement edit, string row)
    {
        var key = ReportModel.DraftKey(edit, row);
        var start = report.Drafts.TryGetValue(key, out var draft) ? draft : edit.Get("value").Str() ?? "";
        var multi = edit.Get("multiline").IsTrue();
        // AcceptsReturn before Text: a TextBox that doesn't take returns keeps only the first line of the text it
        // is given, so a value of several lines (the value window's JSON) showed as "[" and Apply sent that
        var tb = new TextBox
        {
            AcceptsReturn = multi, TextWrapping = multi ? TextWrapping.Wrap : TextWrapping.NoWrap,
            Text = start, Header = edit.Get("label").Str(), PlaceholderText = edit.Get("hint").Str() ?? "",
            IsSpellCheckEnabled = false, MinHeight = multi ? 140 : 0, FontFamily = multi ? new FontFamily("Cascadia Mono, Consolas") : FontFamily.XamlAutoFontFamily,
        };
        tb.TextChanged += (_, _) => report.Drafts[key] = tb.Text;
        var panel = new StackPanel { Spacing = 4 };
        panel.Children.Add(tb);
        if (multi)
        {
            var bar = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
            var apply = new Button { Content = "Apply", Style = Ui.TextStyle("AccentButtonStyle") };
            apply.Click += async (_, _) => await report.CommitAsync(edit, row, tb.Text);
            var revert = new Button { Content = "Revert" };
            revert.Click += (_, _) => { report.Drafts.Remove(key); report.Refused.Remove(key); tb.Text = edit.Get("value").Str() ?? ""; };
            bar.Children.Add(apply);
            bar.Children.Add(revert);
            panel.Children.Add(bar);
        }
        else
        {
            tb.KeyDown += async (_, e) => { if (e.Key == VirtualKey.Enter) { e.Handled = true; await report.CommitAsync(edit, row, tb.Text); } };
        }
        if (report.Refused.TryGetValue(key, out var why)) panel.Children.Add(new TextBlock { Text = why, Foreground = new SolidColorBrush(Microsoft.UI.Colors.IndianRed), TextWrapping = TextWrapping.Wrap });
        return panel;
    }

    public override System.Text.Json.Nodes.JsonObject Describe() => new()
    {
        ["key"] = report.Key, ["title"] = report.Title, ["rows"] = report.Rows, ["error"] = report.Error,
        ["sections"] = report.Reply?.Get("sections").Items().Count() ?? 0,
    };
}

/// <summary>The status bar's messages in full.</summary>
public sealed class MessagesWindow : PanelWindow
{
    readonly MessagesModel messages;
    readonly ListView list = new() { SelectionMode = ListViewSelectionMode.None, Padding = new Thickness(8, 4, 8, 8) };
    readonly StackPanel none = new() { Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };

    public override (int w, int h) Size => (520, 380);

    public MessagesWindow(DocWindow owner, MessagesModel messages) : base(owner, "Messages")
    {
        this.messages = messages;
        none.Children.Add(new FontIcon { Glyph = "", FontFamily = new FontFamily(Ui.IconFont), FontSize = 32, Foreground = Ui.Brush("TextFillColorSecondaryBrush") });
        none.Children.Add(new TextBlock { Text = "No Messages", Style = Ui.TextStyle("SubtitleTextBlockStyle"), HorizontalAlignment = HorizontalAlignment.Center });
        none.Children.Add(Ui.Caption("Errors, a save's outcome and what went wrong show here."));
        var g = new Grid();
        g.Children.Add(list);
        g.Children.Add(none);
        Content = Root(g);
        messages.Changed += Show;
        messages.Start();
        Show();
        Closed += (_, _) => messages.Stop();
    }

    protected override Task CloseInCoreAsync() => messages.CloseAsync();

    void Show()
    {
        if (Gone) return;
        list.Items.Clear();
        none.Visibility = messages.Messages.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        foreach (var m in messages.Messages)
        {
            var tone = m.Get("tone").Str() ?? "info";
            var row = Ui.Columns(Ui.Auto, Ui.Star);
            row.Padding = new Thickness(0, 6, 0, 6);
            var icon = new FontIcon
            {
                Glyph = tone == "error" ? Ui.G.Error : tone == "warn" ? Ui.G.Warn : Ui.G.Info, FontFamily = new FontFamily(Ui.IconFont), FontSize = 16, VerticalAlignment = VerticalAlignment.Top,
                Foreground = tone == "error" ? new SolidColorBrush(Microsoft.UI.Colors.IndianRed) : tone == "warn" ? new SolidColorBrush(Microsoft.UI.Colors.DarkOrange) : Ui.Brush("TextFillColorSecondaryBrush"),
            };
            row.Children.Add(icon);
            var words = new StackPanel { Spacing = 2 };
            words.Children.Add(new TextBlock { Text = m.Get("text").Str() ?? "", TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true });
            if (m.Get("detail").Str() is { Length: > 0 } d) words.Children.Add(Ui.Caption(d));
            row.Children.Add(words.At(1));
            list.Items.Add(row);
        }
        Owner.NoteStatus();
    }

    public override System.Text.Json.Nodes.JsonObject Describe() => new() { ["open"] = true, ["count"] = messages.Messages.Count };
}

/// <summary>The Acknowledgements window (FR4): the licences of what the app and its engines are built from,
/// the core's search box above a list of programs, each a heading over its licences; a licence is an
/// expander, closed until opened, with its packages and its full text, selectable.</summary>
public sealed class AcknowledgementsWindow : PanelWindow
{
    readonly AcknowledgementsModel acks;
    readonly StackPanel body = new() { Spacing = 4, Padding = new Thickness(16, 4, 16, 16) };

    public override (int w, int h) Size => (680, 640);

    public AcknowledgementsWindow(DocWindow owner, AcknowledgementsModel acks) : base(owner, "Acknowledgements")
    {
        this.acks = acks;
        var search = new TextBox { PlaceholderText = "Search a licence or a package", Margin = new Thickness(16, 12, 16, 8) };
        search.TextChanged += async (_, _) => await acks.ReadAsync(search.Text);
        var g = new Grid();
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        g.Children.Add(search);
        var scroll = new ScrollViewer { Content = body };
        Grid.SetRow(scroll, 1);
        g.Children.Add(scroll);
        Content = Root(g);
        acks.Changed += Show;
        _ = acks.ReadAsync();
    }

    protected override Task CloseInCoreAsync() => acks.CloseAsync();

    void Show()
    {
        if (Gone) return;
        body.Children.Clear();
        foreach (var (_, title, list, none) in acks.Programs())
        {
            body.Children.Add(Ui.Heading(title));
            if (none is not null)
            {
                body.Children.Add(Ui.Caption(none));
                continue;
            }
            body.Children.Add(Ui.Caption(AcknowledgementsModel.PackagesLine(list)));
            if (list.Get("note").Str() is { Length: > 0 } note) body.Children.Add(new TextBlock { Text = note, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true });
            foreach (var group in list.Get("groups").Items())
            {
                var e = new Expander { Header = AcknowledgementsModel.GroupTitle(group), HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Left };
                var g = group;
                // the text is laid out only when the licence is opened
                e.Expanding += (_, _) =>
                {
                    if (e.Content is not null) return;
                    var inner = new StackPanel { Spacing = 4 };
                    if (g.Get("name").Str() is { Length: > 0 } n) inner.Children.Add(new TextBlock { Text = n, Style = Ui.TextStyle("BodyStrongTextBlockStyle") });
                    foreach (var p in g.Get("packages").Items())
                    {
                        var line = string.Join("  ", new[] { p.Get("name").Str(), p.Get("version").Str(), p.Get("url").Str() }.Where(s => !string.IsNullOrEmpty(s)));
                        inner.Children.Add(new TextBlock { Text = line, IsTextSelectionEnabled = true, TextWrapping = TextWrapping.Wrap });
                    }
                    inner.Children.Add(new TextBlock
                    {
                        Text = g.Get("text").Str() ?? "", FontFamily = new FontFamily("Consolas"), FontSize = 12, TextWrapping = TextWrapping.Wrap,
                        IsTextSelectionEnabled = true, Margin = new Thickness(0, 6, 0, 0),
                    });
                    e.Content = inner;
                };
                body.Children.Add(e);
            }
        }
        Owner.NoteStatus();
    }

    public override System.Text.Json.Nodes.JsonObject Describe() => new()
    {
        ["open"] = true, ["query"] = acks.Query,
        ["groups"] = acks.Reply.ValueKind == JsonValueKind.Object ? acks.Reply.Get("editor").Get("list").Get("groups").Items().Count() : 0,
        ["engines"] = acks.Reply.ValueKind == JsonValueKind.Object ? acks.Reply.Get("engines").Items().Count() : 0,
    };
}

/// <summary>Compare: A and B side by side, one over the other with a divider, or their exact difference
/// (`viewer.compare {how, split, diff}`). The difference is the core's (note 26 slice 2b: `compare.get`'s
/// `difference`, its words and its pictures lent by handle), drawn here as the Apple shells draw it.</summary>
public sealed class CompareWindow : PanelWindow
{
    readonly CompareModel compare;
    readonly Pictures pictures;
    readonly SelectorBar how = new();
    readonly SelectorBarItem side = new() { Text = "Side by Side" }, split = new() { Text = "Split" }, diff = new() { Text = "Difference" };
    readonly Grid stage = new();
    readonly Slider divider = new() { Minimum = 0, Maximum = 1, StepFrequency = 0.01, Margin = new Thickness(12, 0, 12, 0), IsThumbToolTipEnabled = false };
    readonly TextBlock error = new() { Foreground = new SolidColorBrush(Microsoft.UI.Colors.IndianRed), TextWrapping = TextWrapping.Wrap };
    readonly Dictionary<string, (string key, CanvasBitmap? bmp)> shown = [];
    readonly TextBlock note = new() { TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true };
    bool following;

    public override (int w, int h) Size => (880, 560);

    public CompareWindow(DocWindow owner, CompareModel compare, Pictures pictures) : base(owner, "Compare")
    {
        this.compare = compare;
        this.pictures = pictures;
        how.Items.Add(side);
        how.Items.Add(split);
        how.Items.Add(diff);
        how.SelectionChanged += async (_, _) =>
        {
            if (following) return;
            var h = how.SelectedItem == split ? "split" : how.SelectedItem == diff ? "difference" : "side";
            await compare.RunAsync("viewer.compare", Jsonv.Object(("how", Jsonv.Of(h))));
        };
        divider.ValueChanged += (_, _) => PlaceSplit();
        divider.AddHandler(UIElement.PointerReleasedEvent, new Microsoft.UI.Xaml.Input.PointerEventHandler(async (_, _) => await compare.RunAsync("viewer.compare", Jsonv.Object(("split", Jsonv.Of(divider.Value))))), true);
        var g = new Grid { Padding = new Thickness(12), RowSpacing = 8 };
        foreach (var h in new[] { GridLength.Auto, GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto, GridLength.Auto })
            g.RowDefinitions.Add(new RowDefinition { Height = h });
        g.Children.Add(how);
        g.Children.Add(error.At(0, 1));
        g.Children.Add(stage.At(0, 2));
        g.Children.Add(divider.At(0, 3));
        g.Children.Add(note.At(0, 4));
        Content = Root(g);
        compare.Changed += () => _ = Show();
        _ = compare.ReloadAsync();
        stage.SizeChanged += (_, _) => PlaceSplit();
    }

    protected override Task CloseInCoreAsync() => compare.CloseAsync();

    public override void Follow(ulong? stamp) => compare.Follow(stamp);

    async Task Show()
    {
        if (Gone) return;
        following = true;
        how.SelectedItem = compare.How switch { "split" => split, "difference" => diff, _ => side };
        following = false;
        error.Text = compare.Error ?? "";
        divider.Visibility = compare.How == "split" ? Visibility.Visible : Visibility.Collapsed;
        if (compare.Reply.Get("split").Double() is { } s) divider.Value = s;
        var a = compare.Reply.Get("a");
        var b = compare.Reply.Get("b");
        note.Text = "";
        if (compare.How == "difference")
        {
            await Difference(compare.Reply.Get("difference"));
            PlaceSplit();
            Owner.NoteStatus();
            return;
        }
        await Load("a", a);
        await Load("b", b);
        stage.Children.Clear();
        stage.ColumnDefinitions.Clear();
        if (compare.How == "split")
        {
            stage.Children.Add(Side("b", b, true));
            var top = Side("a", a, true);
            top.Name = "splitA";
            stage.Children.Add(top);
        }
        else
        {
            stage.ColumnSpacing = 8;
            stage.ColumnDefinitions.Add(new ColumnDefinition { Width = Ui.Star });
            stage.ColumnDefinitions.Add(new ColumnDefinition { Width = Ui.Star });
            stage.Children.Add(Side("a", a, false));
            stage.Children.Add(Side("b", b, false).At(1));
        }
        PlaceSplit();
        Owner.NoteStatus();
    }

    /// <summary>The picture a side shows, as a pop-out's would: an SVG drawn, an array's first view, a picture
    /// file decoded, else the engine's rendition.</summary>
    static (string handle, string want, int? px)? Source(JsonElement? s)
    {
        if (s is not { ValueKind: JsonValueKind.Object } v) return null;
        static string? H(JsonElement? x) => x.Get("handle").Str() ?? x.Get("ref").Get("handle").Str();
        if (v.Get("viewer").Str() == "svg" && v.Get("handle").Str() is { } sh) return (sh, "svg", 1024);
        if (H(v.Get("views").At(0)) is { } vh) return (vh, "rgba", null);
        if (v.Get("media").Str() == "image" && v.Get("handle").Str() is { } ih) return (ih, "image", 2048);
        if (H(v.Get("picture")) is { } ph) return (ph, "rgba", null);
        return null;
    }

    async Task Load(string slot, JsonElement? s)
    {
        if (Source(s) is not { } src) { shown.Remove(slot); return; }
        var key = $"{src.want}{src.handle}";
        if (shown.TryGetValue(slot, out var had) && had.key == key) return;
        shown[slot] = (key, await pictures.LoadAsync(src.handle, src.want, src.px));
    }

    FrameworkElement Side(string slot, JsonElement? s, bool overlaid)
    {
        var g = new Grid { RowSpacing = 4 };
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var head = Ui.Columns(Ui.Auto, Ui.Star, Ui.Auto);
        head.Children.Add(new TextBlock { Text = slot.ToUpperInvariant(), Style = Ui.TextStyle("BodyStrongTextBlockStyle") });
        var pinned = s is { ValueKind: JsonValueKind.Object };
        head.Children.Add(new TextBlock { Text = pinned ? s.Get("title").Str() ?? "" : "Not pinned", Foreground = Ui.Brush("TextFillColorSecondaryBrush"), TextTrimming = TextTrimming.CharacterEllipsis }.At(1));
        if (pinned)
        {
            var unpin = new HyperlinkButton { Content = "Unpin", Padding = new Thickness(4, 0, 4, 0) };
            unpin.Click += async (_, _) => await compare.RunAsync("viewer.unpin", Jsonv.Object(("slot", Jsonv.Of(slot))));
            head.Children.Add(unpin.At(2));
        }
        if (overlaid && slot == "a") head.HorizontalAlignment = HorizontalAlignment.Left;
        if (overlaid && slot == "b") head.HorizontalAlignment = HorizontalAlignment.Right;
        g.Children.Add(head);
        var frame = new Border { Background = Ui.Brush("SubtleFillColorSecondaryBrush"), CornerRadius = new CornerRadius(6) };
        if (shown.TryGetValue(slot, out var p) && p.bmp is { } bmp)
        {
            var src = new CanvasImageSource(CanvasDevice.GetSharedDevice(), bmp.SizeInPixels.Width, bmp.SizeInPixels.Height, 96);
            using (var ds = src.CreateDrawingSession(Microsoft.UI.Colors.Transparent)) ds.DrawImage(bmp);
            frame.Child = new Image { Source = src, Stretch = Stretch.Uniform };
        }
        else
        {
            frame.Child = new TextBlock
            {
                Text = !pinned ? $"Pin a value as {slot.ToUpperInvariant()} from its viewer's bar." : s.Get("summary").Str() ?? "Reading…",
                Foreground = Ui.Brush("TextFillColorSecondaryBrush"), TextWrapping = TextWrapping.Wrap, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(12),
            };
        }
        g.Children.Add(frame.At(0, 1));
        return g;
    }

    /// <summary>|A − B|: the core's words, then each picture it lends (the mean of the channels, or one a
    /// channel, `viewer.compare {diff}`), NaN in magenta; asked again while it is still being computed.</summary>
    async Task Difference(JsonElement? d)
    {
        stage.Children.Clear();
        stage.ColumnDefinitions.Clear();
        var status = d.Get("status").Str();
        if (status == "failed") { note.Text = d.Get("error").Str() ?? "The difference couldn't be computed."; return; }
        if (status != "ready")
        {
            note.Text = "Computing |A − B|…";
            await Task.Delay(300);
            if (!Gone) _ = compare.ReloadAsync();
            return;
        }
        note.Text = d.Get("text").Str() ?? "";
        // the probe (note 26 slice 8): a tap on a picture reads A, B and |A − B| there, the core's numbers
        var probe = d.Get("probe");
        note.Text += probe.Get("outside").IsTrue() ? "\nOutside the pictures" : probe.Get("text").Str() is { } pt ? $"\n{pt}" : "\nTap a picture to read A, B and |A − B| there";
        var rows = d.Get("rows").Double() ?? 0;
        var cols = d.Get("cols").Double() ?? 0;
        var panel = new StackPanel { Spacing = 10 };
        var mode = new ComboBox { Items = { "Mean of the Channels", "Each Channel" }, SelectedIndex = compare.Reply.Get("diff").Str() == "channels" ? 1 : 0 };
        mode.SelectionChanged += async (_, _) => await compare.RunAsync("viewer.compare", Jsonv.Object(("diff", Jsonv.Of(mode.SelectedIndex == 1 ? "channels" : "mean"))));
        panel.Children.Add(mode);
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        int i = 0;
        foreach (var p in d.Get("pictures").Items())
        {
            if (p.Get("handle").Str() is not { } h) continue;
            var slot = $"diff{i++}";
            if (!shown.TryGetValue(slot, out var had) || had.key != h) shown[slot] = (h, await pictures.LoadAsync(h, "rgba", null));
            var cell = new StackPanel { Spacing = 3 };
            if (shown[slot].bmp is { } bmp)
            {
                var src = new CanvasImageSource(CanvasDevice.GetSharedDevice(), bmp.SizeInPixels.Width, bmp.SizeInPixels.Height, 96);
                using (var ds = src.CreateDrawingSession(Microsoft.UI.Colors.Transparent)) ds.DrawImage(bmp);
                var img = new Image { Source = src, Stretch = Stretch.Uniform, MaxHeight = 360, MaxWidth = 420 };
                img.Tapped += async (_, e) =>
                {
                    var at = e.GetPosition(img);
                    if (rows <= 0 || cols <= 0 || img.ActualWidth <= 0 || img.ActualHeight <= 0) return;
                    var r = (long)Math.Clamp(Math.Floor(at.Y / img.ActualHeight * rows), 0, rows - 1);
                    var c = (long)Math.Clamp(Math.Floor(at.X / img.ActualWidth * cols), 0, cols - 1);
                    await compare.RunAsync("viewer.compare", Jsonv.Object(("probe", Jsonv.Array([Jsonv.Of(r), Jsonv.Of(c)]))));
                };
                cell.Children.Add(img);
            }
            cell.Children.Add(Ui.Caption($"{p.Get("caption").Str()} · largest {p.Get("top").Str()}"));
            row.Children.Add(cell);
        }
        panel.Children.Add(new ScrollViewer { Content = row, HorizontalScrollMode = ScrollMode.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollMode = ScrollMode.Disabled });
        stage.Children.Add(panel);
    }

    /// <summary>A's part of the split: a clip at the divider (the drag is the shell's; its end the core's).</summary>
    void PlaceSplit()
    {
        if (compare.How != "split") return;
        foreach (var c in stage.Children)
            if (c is FrameworkElement { Name: "splitA" } a)
                a.Clip = new RectangleGeometry { Rect = new Windows.Foundation.Rect(0, 0, Math.Max(0, stage.ActualWidth * divider.Value), Math.Max(0, stage.ActualHeight)) };
    }

    public override System.Text.Json.Nodes.JsonObject Describe() => new()
    {
        ["how"] = compare.How, ["a"] = compare.Reply.Get("a").Get("title").Str(), ["b"] = compare.Reply.Get("b").Get("title").Str(),
        ["split"] = compare.Reply.Get("split").Double(), ["pictures"] = shown.Count(x => x.Value.bmp is not null),
    };
}
