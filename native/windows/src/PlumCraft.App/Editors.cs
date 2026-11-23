// The native controls of editing (N5 slice 3), each drawn from the kit's model of the core's state:
//
// - The palette (contract 12.7): a flyout at the palette's place with a search box and a list, keyboard
//   first (type to search, Up and Down, Enter adds, Escape or a click away dismisses), kinds of engines not
//   started marked and their pick shown pending while the engine starts.
// - The field editor (12.10): one control over the row the core has open, at the row's rect and the
//   canvas's zoom, floating at a legible size when zoomed far out (the kit's EditorPlacement): a TextBox for
//   text, a path, several choices and a number (the core's number text, right-aligned in tabular digits), a
//   TextBox taking Enter for several lines (Ctrl+Enter commits), a ComboBox for a choice, a ColorPicker in a
//   flyout for a colour; a toggle is flipped by the core on a click. Enter or focus lost commits, Escape
//   cancels, Tab and Shift+Tab commit and move; a refusal keeps it open with the core's words.
// - A name edited in place (12.16): a TextBox over the core's rect.
//
// None of them decides anything: what they send is row.set, row.edit, name.set, name.edit, palette.choose
// and canvas.palette, and what they show follows the scene.

using System.Numerics;
using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using PlumCraft.App.Canvas;
using PlumCraft.Kit;
using PlumCraft.Models;
using Windows.System;
using XCanvas = Microsoft.UI.Xaml.Controls.Canvas;

namespace PlumCraft.App;

public sealed class Editors
{
    readonly DocWindow window;
    readonly CanvasView view;
    public PaletteModel Palette { get; }
    public RowEditor Rows { get; }
    public NameEditor Names { get; }

    // the palette
    readonly Flyout paletteFly = new() { Placement = FlyoutPlacementMode.BottomEdgeAlignedLeft };
    readonly TextBox search = new() { PlaceholderText = "Search nodes", MinWidth = 300, IsSpellCheckEnabled = false };
    readonly TextBlock headline = new() { FontSize = 12, Opacity = 0.7, Margin = new Thickness(2, 6, 0, 2) };
    readonly TextBlock pending = new() { FontSize = 12, TextWrapping = TextWrapping.Wrap, Visibility = Visibility.Collapsed, Margin = new Thickness(2, 4, 0, 0) };
    readonly ListView list = new() { SelectionMode = ListViewSelectionMode.Single, IsItemClickEnabled = true, MaxHeight = 360, Width = 320 };
    bool paletteShowing, closingFromCore;

    // the field editor and the name editor
    FrameworkElement? field;
    string? fieldFor;
    bool committing;
    TextBox? nameBox;

    public Editors(DocWindow window, CanvasView view, DocumentModel model)
    {
        this.window = window;
        this.view = view;
        Palette = new PaletteModel(model) { FallbackAt = () => Centre() };
        Rows = new RowEditor(model);
        Names = new NameEditor(model);
        BuildPalette();
        Palette.Opened += _ => ShowPalette();
        Palette.Closed += () => { HidePalette(); view.PaletteClosed(); };
        Palette.Updated += FillPalette;
        Rows.Opened += o => ShowField(o);
        Rows.Closed += RemoveField;
        Rows.Noted += (o, text, error) =>
        {
            if (view.Canvas.RowRect(o.Edit.Face, o.Edit.Row) is { } r) view.Note(text, r, error);
        };
        Names.Opened += ShowName;
        Names.Closed += RemoveName;
        Names.Refused += why => { if (Names.Open is { } n) view.Note(why, n.Rect, true); };
        view.Canvas.ViewChanged += _ => Reposition();
        model.SceneChanged += _ => Reposition();
    }

    Point Centre()
    {
        var g = view.Canvas.View.ToGraph(view.Canvas.ViewSize / 2);
        return new Point(g.X, g.Y);
    }

    /// <summary>Node › Search…, Tab, the toolbar's Add Node: the palette at the pointer, else the view's centre.</summary>
    public Task OpenPaletteAsync()
    {
        var v = view.Pointer ?? view.Canvas.ViewSize / 2;
        var g = view.Canvas.View.ToGraph(v);
        return Palette.OpenAsync(new Point(g.X, g.Y));
    }

    /// <summary>What is open, for the status file.</summary>
    public System.Text.Json.Nodes.JsonObject Describe() => new()
    {
        ["palette"] = Palette.IsOpen, ["paletteItems"] = Palette.Items.Count, ["paletteQuery"] = Palette.Query, ["paletteWire"] = Palette.ForWire,
        ["field"] = fieldFor, ["fieldKind"] = field?.GetType().Name, ["fieldError"] = Rows.Error, ["name"] = Names.Open?.Text, ["nameError"] = Names.Error,
    };

    // ---------------- the palette ----------------

    void BuildPalette()
    {
        var panel = new StackPanel { Spacing = 2, Width = 320 };
        panel.Children.Add(search);
        panel.Children.Add(headline);
        panel.Children.Add(pending);
        panel.Children.Add(list);
        paletteFly.Content = panel;
        paletteFly.FlyoutPresenterStyle = new Style(typeof(FlyoutPresenter));
        paletteFly.FlyoutPresenterStyle.Setters.Add(new Setter(FrameworkElement.MaxWidthProperty, 400));
        paletteFly.Opened += (_, _) => search.Focus(FocusState.Programmatic);
        paletteFly.Closed += (_, _) =>
        {
            paletteShowing = false;
            if (!closingFromCore) Palette.Dismiss();
            closingFromCore = false;
            view.Focus(FocusState.Programmatic);
            window.NoteStatus();
        };
        search.TextChanged += (_, _) => Palette.Search(search.Text);
        search.KeyDown += async (_, e) =>
        {
            switch (e.Key)
            {
                case VirtualKey.Down: e.Handled = true; Palette.MoveBy(1); break;
                case VirtualKey.Up: e.Handled = true; Palette.MoveBy(-1); break;
                case VirtualKey.PageDown: e.Handled = true; Palette.MoveBy(10); break;
                case VirtualKey.PageUp: e.Handled = true; Palette.MoveBy(-10); break;
                case VirtualKey.Enter: e.Handled = true; await Palette.ChooseSelectedAsync(); FillPalette(); break;
                case VirtualKey.Escape: e.Handled = true; paletteFly.Hide(); break;
            }
        };
        list.ItemClick += async (_, e) =>
        {
            if (e.ClickedItem is FrameworkElement { Tag: PaletteItem it }) { await Palette.ChooseAsync(it); FillPalette(); }
        };
    }

    void ShowPalette()
    {
        search.Text = Palette.Query;
        FillPalette();
        var at = Palette.Palette?.At is { } a ? view.Canvas.View.ToView(new Vector2((float)a.X, (float)a.Y)) : view.Canvas.ViewSize / 2;
        at = Vector2.Clamp(at, Vector2.Zero, Vector2.Max(view.Canvas.ViewSize - new Vector2(40, 40), Vector2.Zero));
        if (paletteShowing) paletteFly.Hide();
        paletteShowing = true;
        paletteFly.ShowAt(view, new FlyoutShowOptions { Position = new Windows.Foundation.Point(at.X, at.Y), ShowMode = FlyoutShowMode.Standard });
        window.NoteStatus();
    }

    void HidePalette()
    {
        if (!paletteShowing) return;
        closingFromCore = true;
        paletteFly.Hide();
        window.NoteStatus();
    }

    void FillPalette()
    {
        headline.Text = Palette.Error ?? Palette.Headline;
        pending.Text = Palette.PendingText ?? "";
        pending.Visibility = Palette.Pending is null ? Visibility.Collapsed : Visibility.Visible;
        list.Items.Clear();
        string? section = null;
        for (int k = 0; k < Palette.Items.Count; k++)
        {
            var it = Palette.Items[k];
            if (it.Category != section)
            {
                section = it.Category;
                // a section's name as the platform heads a group in a list: smaller, secondary, semibold, no
                // rule under it, never selected (slice 3's ListViewHeaderItem drew a row with an underline)
                var heading = new ListViewItem
                {
                    Content = new TextBlock { Text = section, Style = Ui.TextStyle("CaptionTextBlockStyle"), FontWeight = Microsoft.UI.Text.FontWeights.SemiBold, Foreground = Ui.Brush("TextFillColorSecondaryBrush") },
                    IsHitTestVisible = false, IsTabStop = false, AllowFocusOnInteraction = false, MinHeight = 26, Padding = new Thickness(12, k == 0 ? 2 : 10, 0, 2),
                    VerticalContentAlignment = VerticalAlignment.Bottom,
                };
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetHeadingLevel(heading, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level2);
                list.Items.Add(heading);
            }
            var row = new Grid { Tag = it, ColumnSpacing = 8 };
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            row.Children.Add(new TextBlock { Text = it.Label, TextTrimming = TextTrimming.CharacterEllipsis });
            var note = it.Starts is { } eng ? $"starts the {eng} engine" : it.Port is { } port ? $"→ {port}" : null;
            if (note is not null)
            {
                var n = new TextBlock { Text = note, FontSize = 11, Opacity = 0.6, VerticalAlignment = VerticalAlignment.Center };
                Grid.SetColumn(n, 1);
                row.Children.Add(n);
            }
            ToolTipService.SetToolTip(row, it.Kind);
            // the row's words for UI Automation (B94: a screen reader, or the person check, found every kind
            // row unnamed: the row is a Grid, which has no name of its own)
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(row, note is null ? it.Label : $"{it.Label}, {note}");
            list.Items.Add(row);
            if (k == Palette.Selected)
            {
                list.SelectedItem = row;
                list.ScrollIntoView(row);
            }
        }
        window.NoteStatus();
    }

    // ---------------- the field editor ----------------

    void ShowField(RowEditor.OpenRow o)
    {
        RemoveField();
        fieldFor = $"{o.Edit.Node}.{o.Edit.Port}";
        var w = o.Widget;
        FrameworkElement el;
        switch (o.Kind)
        {
            case WidgetWord.Choice:
                var cb = new ComboBox { MinWidth = 80 };
                foreach (var c in w.Choices)
                {
                    var item = new ComboBoxItem { Content = c.Label, Tag = c.Value, IsEnabled = c.Disabled is null };
                    if (c.Disabled is { } why) ToolTipService.SetToolTip(item, why);
                    cb.Items.Add(item);
                    if (Shell.Same(c.Value, w.Value)) cb.SelectedItem = item;
                }
                cb.SelectionChanged += async (_, _) =>
                {
                    if (committing || cb.SelectedItem is not ComboBoxItem { Tag: JsonElement v }) return;
                    committing = true;
                    await Rows.CommitValueAsync(v);
                    committing = false;
                };
                cb.DropDownClosed += async (_, _) => { if (window.IsActive && !committing && Rows.Open is not null) await Rows.CloseAsync(); };
                cb.KeyDown += async (_, e) => await FieldKey(e, null);
                cb.Loaded += (_, _) => { cb.Focus(FocusState.Programmatic); cb.IsDropDownOpen = true; };
                el = cb;
                break;
            case WidgetWord.Color:
                var picker = new ColorPicker { IsAlphaEnabled = false, IsMoreButtonVisible = false };
                if (FaceLayout.WidgetColor(w) is { } col) picker.Color = col;
                var fly = new Flyout { Content = picker };
                var swatch = new Button { Width = 40, Height = 24, Background = new SolidColorBrush(picker.Color), Flyout = fly };
                fly.Closed += async (_, _) =>
                {
                    var c = picker.Color;
                    var v = Jsonv.Array([Jsonv.Of(c.R / 255.0), Jsonv.Of(c.G / 255.0), Jsonv.Of(c.B / 255.0)]);
                    committing = true;
                    await Rows.CommitValueAsync(v);
                    committing = false;
                };
                swatch.Loaded += (_, _) => fly.ShowAt(swatch);
                el = swatch;
                break;
            case WidgetWord.Toggle:
                return; // the core flips it on a click
            default:
                var multi = o.Kind == WidgetWord.Multiline;
                var number = o.Kind == WidgetWord.Number;
                var tb = new TextBox
                {
                    // AcceptsReturn before Text, or a value of several lines is cut to its first
                    AcceptsReturn = multi, TextWrapping = multi ? TextWrapping.Wrap : TextWrapping.NoWrap, Text = RowEditor.StartText(w),
                    IsSpellCheckEnabled = false, IsTextPredictionEnabled = false, Padding = new Thickness(4, 1, 4, 1), MinHeight = 0, MinWidth = 0,
                    TextAlignment = number ? TextAlignment.Right : TextAlignment.Left,
                    FontFamily = number || multi ? new FontFamily("Cascadia Mono, Consolas") : FontFamily.XamlAutoFontFamily,
                };
                if (number) tb.InputScope = new InputScope { Names = { new InputScopeName(InputScopeNameValue.Number) } };
                tb.KeyDown += async (_, e) => await FieldKey(e, tb);
                // focus lost within the window commits (another control, the canvas); the window losing the
                // system's activation (another app, a screenshot) leaves the field open, as the Mac's does
                // only what the person typed (B129): a field left as it opened sets nothing and stays open
                tb.LostFocus += async (_, _) => { if (window.IsActive && !committing && Rows.Open is not null && ReferenceEquals(field, tb)) await FocusLost(tb); };
                tb.Loaded += (_, _) => { tb.Focus(FocusState.Programmatic); tb.SelectAll(); };
                el = tb;
                break;
        }
        field = el;
        view.Overlay.Children.Add(el);
        Reposition();
        window.NoteStatus();
    }

    async Task FieldKey(KeyRoutedEventArgs e, TextBox? tb)
    {
        var shift = (Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        var ctrl = (Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        switch (e.Key)
        {
            case VirtualKey.Escape:
                e.Handled = true;
                committing = true;
                await Rows.CloseAsync();
                committing = false;
                view.Focus(FocusState.Programmatic);
                break;
            case VirtualKey.Enter when tb is not null && (!tb.AcceptsReturn || ctrl):
                e.Handled = true;
                await Commit(tb, RowEditor.Then.Close);
                break;
            case VirtualKey.Tab:
                e.Handled = true;
                if (tb is not null) await Commit(tb, shift ? RowEditor.Then.Previous : RowEditor.Then.Next);
                else await Rows.StepAsync(shift ? -1 : 1);
                break;
        }
    }

    async Task Commit(TextBox tb, RowEditor.Then then)
    {
        if (committing) return;
        committing = true;
        var ok = await Rows.CommitTextAsync(tb.Text, then);
        committing = false;
        if (!ok && ReferenceEquals(field, tb)) tb.Focus(FocusState.Programmatic);
        else if (then == RowEditor.Then.Close) view.Focus(FocusState.Programmatic);
    }

    async Task FocusLost(TextBox tb)
    {
        if (committing) return;
        // a press on this row's own Browse: the core's row.browse closes the field without its text (B133), so
        // the focus loss it causes commits nothing
        if (Rows.Open is { } o && PathBrowse.Offered(o.Widget) && view.PointerOverBrowse(o.Edit.Face, o.Edit.Row)) return;
        committing = true;
        var ok = await Rows.FocusLostAsync(tb.Text);
        committing = false;
        if (ok) view.Focus(FocusState.Programmatic);
    }

    /// <summary>A shortcut fired while a field is open: the field commits first (12.2).</summary>
    public async Task CommitOpenFieldAsync()
    {
        if (field is TextBox tb && Rows.Open is not null) await Commit(tb, RowEditor.Then.Close);
        if (nameBox is { } nb && Names.Open is not null) await Names.CommitAsync(nb.Text);
    }

    void RemoveField()
    {
        if (field is null) return;
        var f = field;
        field = null;
        fieldFor = null;
        view.Overlay.Children.Remove(f);
        window.NoteStatus();
    }

    // ---------------- a name in place ----------------

    void ShowName(NameEdit n)
    {
        RemoveName();
        var tb = new TextBox { Text = n.Text, PlaceholderText = n.Hint ?? "", IsSpellCheckEnabled = false, Padding = new Thickness(4, 1, 4, 1), MinHeight = 0 };
        ToolTipService.SetToolTip(tb, n.Label);
        tb.KeyDown += async (_, e) =>
        {
            if (e.Key == VirtualKey.Escape) { e.Handled = true; await Names.CancelAsync(); view.Focus(FocusState.Programmatic); }
            else if (e.Key == VirtualKey.Enter) { e.Handled = true; if (await Names.CommitAsync(tb.Text)) view.Focus(FocusState.Programmatic); }
        };
        tb.LostFocus += async (_, _) => { if (window.IsActive && ReferenceEquals(nameBox, tb) && Names.Open is not null) await Names.CommitAsync(tb.Text); };
        tb.Loaded += (_, _) => { tb.Focus(FocusState.Programmatic); tb.SelectAll(); };
        nameBox = tb;
        view.Overlay.Children.Add(tb);
        Reposition();
        window.NoteStatus();
    }

    void RemoveName()
    {
        if (nameBox is null) return;
        view.Overlay.Children.Remove(nameBox);
        nameBox = null;
        window.NoteStatus();
    }

    // ---------------- where they go ----------------

    void Reposition()
    {
        var c = view.Canvas;
        var pan = new Point(c.View.Pan.X, c.View.Pan.Y);
        var size = new Point(c.ViewSize.X, c.ViewSize.Y);
        if (field is { } f && Rows.Open is { } o && c.RowRect(o.Edit.Face, o.Edit.Row) is { } r)
        {
            var row = o.Kind == WidgetWord.Multiline ? r with { Y1 = Math.Max(r.Y1, r.Y0 + 50) } : r;
            // a path's field stops short of its Browse square, which stays tappable while the text is edited (B133)
            if (PathBrowse.Offered(o.Widget)) row = PathBrowse.Field(row);
            var p = EditorPlacement.Place(row, c.View.Zoom, pan, size);
            Put(f, p);
        }
        if (nameBox is { } nb && Names.Open is { } n)
        {
            var p = EditorPlacement.Place(n.Rect, c.View.Zoom, pan, size);
            Put(nb, p with { W = Math.Max(p.W, 120) });
        }
    }

    static void Put(FrameworkElement el, EditorPlacement.Placed p)
    {
        XCanvas.SetLeft(el, p.X);
        XCanvas.SetTop(el, p.Y);
        el.Width = Math.Max(p.W, 24);
        if (el is TextBox tb)
        {
            tb.FontSize = Math.Clamp(p.FontSize, 8, 40);
            tb.Height = Math.Max(p.H, tb.FontSize + 10);
        }
        else if (el is ComboBox cb)
        {
            cb.FontSize = Math.Clamp(p.FontSize, 8, 40);
            cb.MinHeight = 0;
        }
        if (p.Floating) el.Shadow = new ThemeShadow();
    }
}
