// Settings (N5 slice 4; PlumCraftShell's SettingsView, the core's settings model, contract 14.4): a window
// of its own with a NavigationView, a page per section of `settings.get` (Appearance, Colours, Families,
// Theme files, Running, Engines, Cache), each a column of setting cards in the Windows 11 Settings look:
// a ComboBox for a choice (the theme, Match the System among them), a Slider for a number, a
// ToggleSwitch, a colour swatch with the system's ColorPicker, a button for an action. Every control sets
// its value through the command its item names; the section shown is the core's (`app.settings {tab}`,
// the scene's `view.windows.settings`), so a script opens the same page. Text size isn't offered: the
// faces are drawn at the platform's sizes and follow the system's text scaling (N4 card 14), and the core
// leaves it out of this shell's model.

using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using PlumCraft.Kit;
using PlumCraft.Models;
using Border = Microsoft.UI.Xaml.Controls.Border;

namespace PlumCraft.App;

public sealed class SettingsWindow : PanelWindow
{
    readonly SettingsModel settings;
    readonly NavigationView nav = new()
    {
        PaneDisplayMode = NavigationViewPaneDisplayMode.Left, IsSettingsVisible = false, IsBackButtonVisible = NavigationViewBackButtonVisible.Collapsed,
        IsPaneToggleButtonVisible = false, OpenPaneLength = 220, IsTitleBarAutoPaddingEnabled = false,
    };
    readonly StackPanel page = new() { Spacing = 0, Padding = new Thickness(28, 12, 28, 28), MaxWidth = 760, HorizontalAlignment = HorizontalAlignment.Stretch };
    readonly TextBlock error = new() { Foreground = new SolidColorBrush(Microsoft.UI.Colors.IndianRed), TextWrapping = TextWrapping.Wrap };
    string tab = "appearance";
    bool following;
    /// <summary>A slider being dragged: its value shown follows it; it is set when the drag ends.</summary>
    bool sliding;

    public override (int w, int h) Size => (760, 620);

    public SettingsWindow(DocWindow owner, SettingsModel settings) : base(owner, "Settings")
    {
        this.settings = settings;
        nav.Content = new ScrollViewer { Content = page };
        nav.SelectionChanged += async (_, e) =>
        {
            if (following || e.SelectedItem is not NavigationViewItem { Tag: string id }) return;
            tab = id;
            Build();
            await settings.ShowAsync(id);
        };
        var g = new Grid();
        g.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        g.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        g.Children.Add(nav);
        error.Margin = new Thickness(12, 0, 12, 8);
        g.Children.Add(error.At(0, 1));
        Content = Root(g);
        settings.Changed += () => { if (!sliding) { Menu(); Build(); } };
        owner.Model.PrefsChanged += () => _ = settings.RefreshAsync();
        _ = settings.RefreshAsync();
    }

    protected override Task CloseInCoreAsync() => settings.CloseAsync();

    /// <summary>The core's section (the scene's `view.windows.settings`).</summary>
    public void ShowTab(string t)
    {
        if (t == tab && nav.MenuItems.Count > 0) return;
        tab = t;
        Select();
        Build();
    }

    static string Glyph(string id) => id switch
    {
        "appearance" => Ui.G.Palette, "colours" => Ui.G.Swatch, "families" => Ui.G.Stack, "files" => Ui.G.Files,
        "running" => Ui.G.Run, "engines" => Ui.G.Engine, "cache" => Ui.G.Disk, _ => Ui.G.Settings,
    };

    void Menu()
    {
        if (Gone) return;
        var ids = settings.Sections.Select(s => s.Id).ToList();
        var have = nav.MenuItems.OfType<NavigationViewItem>().Select(i => (string)i.Tag).ToList();
        if (!ids.SequenceEqual(have))
        {
            following = true;
            nav.MenuItems.Clear();
            foreach (var s in settings.Sections)
            {
                var item = new NavigationViewItem { Content = s.Label, Tag = s.Id, Icon = Ui.Icon(Glyph(s.Id), 16) };
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(item, $"settings.section.{s.Id}");
                nav.MenuItems.Add(item);
            }
            following = false;
        }
        Select();
    }

    void Select()
    {
        following = true;
        nav.SelectedItem = nav.MenuItems.OfType<NavigationViewItem>().FirstOrDefault(i => (string)i.Tag == tab) ?? nav.MenuItems.OfType<NavigationViewItem>().FirstOrDefault();
        following = false;
    }

    void Build()
    {
        if (Gone) return;
        error.Text = settings.Error ?? "";
        page.Children.Clear();
        if (settings.Section(tab) is not { } s) return;
        page.Children.Add(new TextBlock { Text = s.Label, Style = Ui.TextStyle("TitleTextBlockStyle"), Margin = new Thickness(0, 8, 0, 12) });
        if (s.Note is { } note) page.Children.Add(Ui.Card(Ui.Caption(note)));
        foreach (var (group, items) in s.Groups)
        {
            if (group is { Length: > 0 }) page.Children.Add(Ui.Heading(group));
            foreach (var it in items) page.Children.Add(Card(it));
        }
        Owner.NoteStatus();
    }

    /// <summary>One setting's card: its label (and note) on the left, its native control on the right.</summary>
    FrameworkElement Card(SettingsItem it)
    {
        var g = Ui.Columns(Ui.Star, Ui.Auto);
        g.MinHeight = 40;
        var words = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        words.Children.Add(new TextBlock { Text = it.Label, TextWrapping = TextWrapping.Wrap });
        if (it.Note is { Length: > 0 } n && it.Kind != "note") words.Children.Add(Ui.Caption(n));
        g.Children.Add(words);
        var c = Control(it);
        c.VerticalAlignment = VerticalAlignment.Center;
        if (c is Control cc) cc.IsEnabled = it.Enabled;
        else if (!it.Enabled) { c.IsHitTestVisible = false; c.Opacity = 0.5; }
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(c, $"setting.{it.Id}");
        g.Children.Add(c.At(1));
        return Ui.Card(g);
    }

    FrameworkElement Control(SettingsItem it)
    {
        switch (it.Kind)
        {
            case "choice":
                var cb = new ComboBox { MinWidth = 200 };
                foreach (var (ov, label) in it.Options)
                {
                    var item = new ComboBoxItem { Content = label, Tag = ov };
                    cb.Items.Add(item);
                    if (Jsonv.Same(ov, it.Value)) cb.SelectedItem = item;
                }
                cb.SelectionChanged += async (_, _) => { if (cb.SelectedItem is ComboBoxItem { Tag: JsonElement v } && !Jsonv.Same(v, it.Value)) await settings.SetAsync(it, v); };
                return cb;
            case "number":
                var lo = it.Min ?? 0;
                var hi = Math.Max(it.Max ?? 1, lo + 0.001);
                var now = it.Value.Double() ?? lo;
                var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
                var shown = new TextBlock { Text = Num(now, it), MinWidth = 64, TextAlignment = TextAlignment.Right, VerticalAlignment = VerticalAlignment.Center };
                var slider = new Slider { Minimum = lo, Maximum = hi, Value = Math.Clamp(now, lo, hi), StepFrequency = it.Step ?? 0.5, Width = 180, IsThumbToolTipEnabled = false, VerticalAlignment = VerticalAlignment.Center };
                slider.ValueChanged += (_, e) => shown.Text = Num(e.NewValue, it);
                slider.AddHandler(UIElement.PointerPressedEvent, new Microsoft.UI.Xaml.Input.PointerEventHandler((_, _) => sliding = true), true);
                slider.AddHandler(UIElement.PointerReleasedEvent, new Microsoft.UI.Xaml.Input.PointerEventHandler(async (_, _) =>
                {
                    sliding = false;
                    await settings.SetAsync(it, Jsonv.Of(slider.Value));
                }), true);
                slider.KeyUp += async (_, _) => await settings.SetAsync(it, Jsonv.Of(slider.Value));
                row.Children.Add(slider);
                row.Children.Add(shown);
                if (it.Default.Double() is { } d && it.Value.Double() is { } cur && Math.Abs(d - cur) > 1e-9)
                {
                    var back = new HyperlinkButton { Content = $"Default ({d:0} {it.Unit})".Replace(" )", ")") };
                    back.Click += async (_, _) => await settings.SetAsync(it, Jsonv.Null);
                    row.Children.Add(back);
                }
                return row;
            case "toggle":
                var ts = new ToggleSwitch { IsOn = it.Value.ValueKind == JsonValueKind.True, OnContent = "On", OffContent = "Off" };
                ts.Toggled += async (_, _) => { if (ts.IsOn != (it.Value.ValueKind == JsonValueKind.True)) await settings.SetAsync(it, Jsonv.Of(ts.IsOn)); };
                return ts;
            case "color":
                var color = Hex(it.Value.Str());
                var picker = new ColorPicker { IsAlphaEnabled = false, IsMoreButtonVisible = false, Color = color };
                var fly = new Flyout { Content = picker };
                var swatch = new Button { Width = 56, Height = 28, Background = new SolidColorBrush(color), Flyout = fly };
                ToolTipService.SetToolTip(swatch, it.Value.Str() ?? "");
                fly.Closed += async (_, _) =>
                {
                    var c = picker.Color;
                    var hex = $"#{c.R:x2}{c.G:x2}{c.B:x2}";
                    if (hex != it.Value.Str()) await settings.SetAsync(it, Jsonv.Of(hex));
                };
                var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
                if (it.Contrast is { } ct)
                    line.Children.Add(new TextBlock
                    {
                        Text = ct, VerticalAlignment = VerticalAlignment.Center, Style = Ui.TextStyle("CaptionTextBlockStyle"),
                        Foreground = it.ContrastLevel == "fail" ? new SolidColorBrush(Microsoft.UI.Colors.IndianRed) : it.ContrastLevel == "low" ? new SolidColorBrush(Microsoft.UI.Colors.DarkOrange) : Ui.Brush("TextFillColorSecondaryBrush"),
                    });
                if (it.Changed)
                {
                    var reset = new HyperlinkButton { Content = "Reset" };
                    reset.Click += async (_, _) => await settings.ResetAsync(it);
                    line.Children.Add(reset);
                }
                line.Children.Add(swatch);
                return line;
            case "action":
                var b = new Button { Content = it.Label };
                b.Click += async (_, _) => await settings.ActAsync(it);
                return b;
            default:
                return new TextBlock { Text = it.Value.ValueKind == JsonValueKind.String ? it.Value.GetString() : it.Value.ValueKind is JsonValueKind.Undefined or JsonValueKind.Null ? "" : it.Value.GetRawText(), Foreground = Ui.Brush("TextFillColorSecondaryBrush"), IsTextSelectionEnabled = true };
        }
    }

    static string Num(double v, SettingsItem it) => $"{v:0.#}{(it.Unit is { } u ? " " + u : "")}";

    static Windows.UI.Color Hex(string? s)
    {
        if (s is { Length: 7 } && s[0] == '#' && uint.TryParse(s.AsSpan(1), System.Globalization.NumberStyles.HexNumber, null, out var n))
            return Windows.UI.Color.FromArgb(255, (byte)(n >> 16), (byte)(n >> 8), (byte)n);
        return Microsoft.UI.Colors.Transparent;
    }

    public override System.Text.Json.Nodes.JsonObject Describe() => new()
    {
        ["tab"] = tab, ["sections"] = settings.Sections.Count, ["items"] = settings.Section(tab)?.Items.Count ?? 0, ["chosen"] = settings.Chosen,
        ["shown"] = settings.Shown, ["error"] = settings.Error, ["textSize"] = settings.Item("textSize") is not null,
    };
}
