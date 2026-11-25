// The parameters inspector (N5 slice 4; the Mac app's Parameters.swift): every input of one node, shown
// on its face or not, from the core's model (`node.parameters` opens it, the scene's
// `view.windows.parameters` says which node, `parameters.get` gives the rows). On Windows it is a pane
// docked at the right of the document's window, beside the canvas, rather than the Mac's popover on the
// node: the popover is an attached, transient surface inside the document's window, and the Windows
// counterpart of that is an in-window properties pane (as Visual Studio, Office and Paint put a selection's
// properties), which stays put while the person works on the canvas and never covers the node it is
// for, where a light-dismiss Flyout would close on the first click on the canvas and a window of its own
// would be lost behind the document's.
//
// Each row: its label with the core's help as a tooltip, a full native control for its kind (a Slider
// and a field for a number with a range, a ToggleSwitch, a ComboBox for a choice, a TextBox, a
// several-line TextBox, a colour swatch with the system's ColorPicker), Reset (`row.set {reset}`) when it
// isn't the default, and Show on Face (`node.face`); a wired or inherited input says where its value
// comes from, with Show on Face disabled for the core's reason. Every value goes to the core (`row.set`),
// which checks it as the face's editor does. A row's custom editor (`editor`) is the core's Value Editor
// window (`node.editor`). Escape or the close button closes it in the core (`node.parameters {}`).

using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using PlumCraft.App.Canvas;
using PlumCraft.Kit;
using PlumCraft.Models;
using Border = Microsoft.UI.Xaml.Controls.Border;
using Windows.System;

namespace PlumCraft.App;

public sealed class Inspector : Grid
{
    readonly DocWindow owner;
    public ParametersModel Params { get; }
    readonly TextBlock title = new() { Style = Ui.TextStyle("SubtitleTextBlockStyle"), TextTrimming = TextTrimming.CharacterEllipsis, FontSize = 18 };
    readonly TextBlock kind = Ui.Caption("", wrap: false);
    readonly TextBlock about = Ui.Caption("");
    readonly TextBlock note = new() { TextWrapping = TextWrapping.Wrap, Visibility = Visibility.Collapsed, Margin = new Thickness(0, 4, 0, 0) };
    readonly StackPanel rows = new() { Spacing = 0, Padding = new Thickness(16, 0, 16, 16) };
    readonly Dictionary<string, FrameworkElement> fields = [];
    ParametersReply? reply;
    bool editing, sliding;

    public bool Shown => Visibility == Visibility.Visible;

    public Inspector(DocWindow owner, DocumentModel model)
    {
        this.owner = owner;
        Params = new ParametersModel(model);
        Width = 380;
        Visibility = Visibility.Collapsed;
        Background = Ui.Brush("LayerFillColorDefaultBrush");
        BorderBrush = Ui.Brush("CardStrokeColorDefaultBrush");
        BorderThickness = new Thickness(1, 0, 0, 0);
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var head = Ui.Columns(Ui.Star, Ui.Auto);
        head.Padding = new Thickness(16, 12, 8, 8);
        var words = new StackPanel { Spacing = 2 };
        words.Children.Add(title);
        words.Children.Add(kind);
        words.Children.Add(about);
        words.Children.Add(note);
        head.Children.Add(words);
        head.Children.Add(Ui.GlyphButton(Ui.G.Close, "Close (Esc)", () => Params.CloseAsync()).At(1));
        Children.Add(head);
        var scroll = new ScrollViewer { Content = rows, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
        Children.Add(scroll.At(0, 1));
        KeyDown += async (_, e) =>
        {
            if (e.Key == VirtualKey.Escape && !e.Handled)
            {
                e.Handled = true;
                await Params.CloseAsync();
            }
        };
        Params.Changed += Show;
        Params.Noted += (port, text, error) =>
        {
            note.Text = text;
            note.Foreground = error ? new SolidColorBrush(Microsoft.UI.Colors.IndianRed) : Ui.Brush("TextFillColorSecondaryBrush");
            note.Visibility = Visibility.Visible;
            owner.NoteStatus();
        };
    }

    void Show(ParametersReply? r)
    {
        if (r is null)
        {
            reply = null;
            Visibility = Visibility.Collapsed;
            owner.FocusCanvas();
            owner.NoteStatus();
            return;
        }
        var opening = !Shown || reply?.Node != r.Node;
        // a field being typed into, or a slider being dragged, isn't pulled out from under the person: the
        // rows come again once it commits
        if ((editing || sliding) && reply?.Node == r.Node)
        {
            reply = r;
            return;
        }
        reply = r;
        if (opening) note.Visibility = Visibility.Collapsed;
        Visibility = Visibility.Visible;
        title.Text = r.Title;
        kind.Text = $"{r.Display} · {r.Node}";
        about.Text = r.Description ?? "";
        about.Visibility = string.IsNullOrEmpty(r.Description) ? Visibility.Collapsed : Visibility.Visible;
        ToolTipService.SetToolTip(about, r.About);
        rows.Children.Clear();
        fields.Clear();
        var single = r.Sections.Count == 1;
        foreach (var s in r.Sections)
        {
            if (!single || s.Title.Length > 0) rows.Children.Add(Ui.Heading($"{s.Title}  {s.Rows.Count}"));
            foreach (var row in s.Rows) rows.Children.Add(Line(row));
        }
        owner.NoteStatus();
    }

    /// <summary>One input: label, its control (or where its value comes from), Reset, Show on Face.</summary>
    FrameworkElement Line(ParametersReply.Row p)
    {
        var r = p.Input;
        var g = Ui.Columns(Ui.Px(112), Ui.Star, Ui.Auto, Ui.Auto);
        g.MinHeight = 36;
        g.Padding = new Thickness(0, 2, 0, 2);
        var label = new TextBlock { Text = r.Label, VerticalAlignment = VerticalAlignment.Center, TextTrimming = TextTrimming.CharacterEllipsis };
        if (r.Inactive is not null) label.Foreground = Ui.Brush("TextFillColorDisabledBrush");
        if (p.Help.Length > 0) ToolTipService.SetToolTip(label, p.Help);
        g.Children.Add(label);
        var control = p.Editable ? Editor(p) : Source(p);
        control.VerticalAlignment = VerticalAlignment.Center;
        g.Children.Add(control.At(1));
        var reset = Ui.GlyphButton(Ui.G.Reset, $"Reset to the default: {(p.Default is { } d && d.ValueKind != JsonValueKind.Null ? (d.ValueKind == JsonValueKind.String ? d.GetString() : d.GetRawText()) : "blank")}",
            () => Params.ResetAsync(r.Key), 12);
        reset.Visibility = p.Reset ? Visibility.Visible : Visibility.Collapsed;
        g.Children.Add(reset.At(2));
        // Show on Face: the eye open while shown, struck through while hidden (a quiet button, as Windows
        // shows a layer's visibility), disabled for a wired input
        var eye = Ui.Icon(p.OnFace ? Ui.G.Eye : Ui.G.EyeOff, 13);
        if (!p.OnFace) eye.Foreground = Ui.Brush("TextFillColorTertiaryBrush");
        var face = new Button
        {
            Content = eye, IsEnabled = p.CanHide, Padding = new Thickness(6, 4, 6, 4),
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent), BorderThickness = new Thickness(0),
        };
        var faceTip = p.CanHide ? (p.OnFace ? "Shown on the node's face: click to hide it there" : "Hidden from the node's face: click to show it") : "A wired input always shows on the face";
        ToolTipService.SetToolTip(face, faceTip);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(face, $"Show {r.Label} on the face");
        face.Click += async (_, _) => await Params.ShowOnFaceAsync(r.Key, !p.OnFace);
        g.Children.Add(face.At(3));
        return g;
    }

    /// <summary>Where a value that isn't the node's own comes from: a wire, a bundle, a socket.</summary>
    FrameworkElement Source(ParametersReply.Row p)
    {
        var r = p.Input;
        var text = r.Text ?? (r.Widget is { } w ? RowEditor.StartText(w) : "");
        var t = new TextBlock { Text = text + (r.From is { } f ? $"  {f}" : ""), Foreground = Ui.Brush("TextFillColorSecondaryBrush"), TextTrimming = TextTrimming.CharacterEllipsis };
        ToolTipService.SetToolTip(t, r.Inactive ?? r.Hover ?? text);
        return t;
    }

    /// <summary>A full native control for a row the node owns.</summary>
    FrameworkElement Editor(ParametersReply.Row p)
    {
        var r = p.Input;
        if (r.Widget is not { } w) return Source(p);
        var key = r.Key;
        FrameworkElement made;
        switch (w.KindWord)
        {
            case WidgetWord.Toggle:
                var ts = new ToggleSwitch { IsOn = w.Value.ValueKind == JsonValueKind.True || (w.Value.ValueKind is JsonValueKind.Undefined or JsonValueKind.Null && w.Default.ValueKind == JsonValueKind.True), OnContent = "", OffContent = "", MinWidth = 0 };
                ts.Toggled += async (_, _) => await Params.SetAsync(key, value: Jsonv.Of(ts.IsOn));
                made = ts;
                break;
            case WidgetWord.Choice:
                var cb = new ComboBox { HorizontalAlignment = HorizontalAlignment.Stretch };
                foreach (var c in w.Choices)
                {
                    var item = new ComboBoxItem { Content = c.Label, Tag = c.Value, IsEnabled = c.Disabled is null };
                    if (c.Disabled is { } why) ToolTipService.SetToolTip(item, $"Not available here: {why}");
                    cb.Items.Add(item);
                    if (Shell.Same(c.Value, w.Value)) cb.SelectedItem = item;
                }
                cb.SelectionChanged += async (_, _) => { if (cb.SelectedItem is ComboBoxItem { Tag: JsonElement v }) await Params.SetAsync(key, value: v); };
                made = cb;
                break;
            case WidgetWord.Color:
                var picker = new ColorPicker { IsAlphaEnabled = false, IsMoreButtonVisible = false };
                if (FaceLayout.WidgetColor(w) is { } col) picker.Color = col;
                var fly = new Flyout { Content = picker };
                var swatch = new Button { Width = 48, Height = 26, Background = new SolidColorBrush(picker.Color), Flyout = fly, HorizontalAlignment = HorizontalAlignment.Left };
                ToolTipService.SetToolTip(swatch, "Choose a colour");
                fly.Closed += async (_, _) =>
                {
                    var c = picker.Color;
                    await Params.SetAsync(key, value: Jsonv.Array([Jsonv.Of(c.R / 255.0), Jsonv.Of(c.G / 255.0), Jsonv.Of(c.B / 255.0)]));
                };
                made = swatch;
                break;
            case WidgetWord.Number when w.Min is not null && w.Max is not null:
                // a slider over the span and the field held to the range, as egui's sheet; the field follows
                // the drag and the value is set once, when it ends (one undo step)
                var lo = w.SoftMin ?? w.Min ?? 0;
                var hi = Math.Max(w.SoftMax ?? w.Max ?? 1, lo + 1e-9);
                var now = w.Value.ValueKind == JsonValueKind.Number ? w.Value.GetDouble() : lo;
                var slider = new Slider { Minimum = lo, Maximum = hi, Value = Math.Clamp(now, lo, hi), StepFrequency = w.Step ?? (w.Int ? 1 : (hi - lo) / 1000), IsThumbToolTipEnabled = false, VerticalAlignment = VerticalAlignment.Center };
                if (w.Int && hi - lo <= 20) { slider.TickFrequency = 1; slider.TickPlacement = TickPlacement.BottomRight; }
                var f = Field(p, w);
                f.Width = 72;
                slider.ValueChanged += (_, e) => f.Text = DragDriver.NumberText(e.NewValue, w);
                slider.AddHandler(PointerPressedEvent, new PointerEventHandler((_, _) => sliding = true), true);
                async Task SetSlid()
                {
                    sliding = false;
                    await Params.SetAsync(key, value: Jsonv.Of(w.Int ? Math.Round(slider.Value) : slider.Value));
                }
                slider.AddHandler(PointerReleasedEvent, new PointerEventHandler(async (_, _) => await SetSlid()), true);
                slider.AddHandler(PointerCaptureLostEvent, new PointerEventHandler(async (_, _) => { if (sliding) await SetSlid(); }), true);
                slider.KeyUp += async (_, e) => { if (e.Key is VirtualKey.Left or VirtualKey.Right or VirtualKey.Up or VirtualKey.Down or VirtualKey.Home or VirtualKey.End) await SetSlid(); };
                var pair = Ui.Columns(Ui.Star, Ui.Auto);
                pair.Children.Add(slider);
                pair.Children.Add(f.At(1));
                fields[key] = f;
                return pair;
            case WidgetWord.Multiline:
                var tb = new TextBox
                {
                    AcceptsReturn = true, TextWrapping = TextWrapping.Wrap, Text = w.Value.ValueKind == JsonValueKind.String ? w.Value.GetString() ?? "" : "", Height = 76,
                    FontFamily = new FontFamily("Cascadia Mono, Consolas"), IsSpellCheckEnabled = false, IsTextPredictionEnabled = false,
                };
                tb.GotFocus += (_, _) => editing = true;
                tb.LostFocus += async (_, _) =>
                {
                    editing = false;
                    await Params.SetAsync(key, text: tb.Text);
                };
                made = tb;
                break;
            case WidgetWord.Editor:
                // an application's own editor: the core's Value Editor window (JSON until N6, card 16)
                var open = new Button { Content = r.Text ?? "Edit…" };
                open.Click += async (_, _) => { try { await Params.Model.RunAsync("node.editor", Jsonv.Object(("node", Jsonv.Of(r.Node)))); } catch (Exception e) { owner.Banner(e.Message, true); } };
                made = open;
                break;
            default:
                made = Field(p, w);
                break;
        }
        fields[key] = made;
        return made;
    }

    TextBox Field(ParametersReply.Row p, Widget w)
    {
        var number = w.KindWord == WidgetWord.Number;
        var f = new TextBox
        {
            Text = RowEditor.StartText(w), PlaceholderText = w.Blank ? "blank" : "", IsSpellCheckEnabled = false, IsTextPredictionEnabled = false,
            TextAlignment = number ? TextAlignment.Right : TextAlignment.Left, HorizontalAlignment = HorizontalAlignment.Stretch,
        };
        if (number) f.FontFamily = new FontFamily("Segoe UI Variable Text, Segoe UI");
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(f, p.Input.Label);
        var key = p.Input.Key;
        async Task Commit()
        {
            editing = false;
            await Params.SetAsync(key, text: f.Text);
        }
        f.GotFocus += (_, _) => editing = true;
        f.KeyDown += async (_, e) =>
        {
            if (e.Key == VirtualKey.Enter) { e.Handled = true; await Commit(); }
            else if (e.Key == VirtualKey.Escape) { e.Handled = true; editing = false; f.Text = RowEditor.StartText(w); Params.Refresh(); }
        };
        f.LostFocus += async (_, _) => { if (editing && owner.IsActive) await Commit(); };
        return f;
    }

    /// <summary>Type into a row's field and commit it, as a person would (scripts and the screenshots).</summary>
    public async Task TypeAsync(string key, string text)
    {
        if (fields.TryGetValue(key, out var f) && f is TextBox tb) tb.Text = text;
        await Params.SetAsync(key, text: text);
    }

    public System.Text.Json.Nodes.JsonObject? Describe() => reply is null ? null : new()
    {
        ["node"] = reply.Node, ["shown"] = Shown, ["rows"] = reply.Count, ["sections"] = reply.Sections.Count,
        ["hidden"] = new System.Text.Json.Nodes.JsonArray([.. reply.Hidden.Select(h => (System.Text.Json.Nodes.JsonNode)h)]),
        ["note"] = note.Visibility == Visibility.Visible ? note.Text : null,
    };
}
