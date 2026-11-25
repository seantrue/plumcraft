// Watching shows the value in a pop-out window (contract 12.20: the maintainer, "Eliminate the side
// column in favor of just the popout window"). The core keeps which values are popped out (the scene's
// `view.windows.popouts`); each is a window here, titled with the node and port, brought forward when the
// core asks again (`raised`), read again when its `stamp` changes (`popout.get`), closed with its close
// button (`view.close`) or when the core closes it.
//
// N5 slice 4 completes the viewers the Apple shells have (Viewers.cs): picture (the channel picker, Fit
// and 1:1, the readout and pinned probe, detail on zoom), SVG, an array's pictures, text (wrap,
// monospaced, the whole text), scalars, table (the core's paging and sort), the numbers grid, the JSON
// outline and YAML, bytes, a list's items, sound (waveform, spectrogram, playback), and what no viewer
// draws (its type, size and the engine's picture). Every viewer's state is the core's (`viewer.*`); what
// stays here is motion: scrolling, a picture's pinch or wheel zoom, a column's width, the playing clock.
//
// The bar every viewer has: View As (the viewers the core offers), a list's item stepper, Drag Out (a
// real file drag of what `value.export` writes), Compare (Pin as A, Pin as B), Copy, Save As… and
// Share… (each the core's command for the person: `value.copy`, `value.saveAs`, `value.share`), Share
// disabled with the core's reason when this shell has no share sheet.

using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using PlumCraft.App.Canvas;
using PlumCraft.Kit;
using PlumCraft.Models;
using Border = Microsoft.UI.Xaml.Controls.Border;

namespace PlumCraft.App;

/// <summary>The pop-out windows of one document, kept in step with the core's.</summary>
public sealed class Popouts(DocWindow owner, DocumentModel model, Pictures pictures)
{
    readonly Dictionary<ulong, ValueWindow> open = [];
    Dictionary<ulong, PopoutWindow> known = [];

    public IReadOnlyDictionary<ulong, ValueWindow> Open => open;

    public void Follow(DocumentWindows w)
    {
        var list = w.Popouts ?? [];
        var d = PopoutDiff.Of(known, list);
        known = list.ToDictionary(p => p.Id);
        foreach (var id in d.Closed)
            if (open.Remove(id, out var gone)) gone.CloseFromCore();
        foreach (var p in d.Opened)
        {
            if (open.ContainsKey(p.Id)) continue;
            var win = new ValueWindow(owner, new PopoutModel(model, p), pictures);
            win.ClosedByPerson += () => { open.Remove(p.Id); owner.NoteStatus(); };
            open[p.Id] = win;
            win.Place(open.Count - 1);
            win.Activate();
            win.Pop.Refresh();
        }
        foreach (var p in d.Changed) if (open.TryGetValue(p.Id, out var win)) win.Pop.Update(p);
        foreach (var p in d.Raised) if (open.TryGetValue(p.Id, out var win)) win.Activate();
        owner.NoteStatus();
    }

    public void CloseAll()
    {
        foreach (var w in open.Values) w.CloseFromCore();
        open.Clear();
    }

    /// <summary>What each window shows, for scripts (the status file).</summary>
    public System.Text.Json.Nodes.JsonArray Describe() => [.. open.Values.Select(w => (System.Text.Json.Nodes.JsonNode)w.Describe())];
}

public sealed class ValueWindow : Window
{
    readonly DocWindow owner;
    public PopoutModel Pop { get; }
    readonly Pictures pictures;
    public event Action? ClosedByPerson;
    readonly TextBlock summary = new() { TextWrapping = TextWrapping.Wrap, Margin = new Thickness(14, 10, 14, 2), IsTextSelectionEnabled = true, MaxLines = 3 };
    readonly StackPanel left = new() { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(12, 0, 0, 0) };
    readonly StackPanel steps = new() { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(12, 0, 0, 6), Visibility = Visibility.Collapsed };
    readonly CommandBar bar = new() { DefaultLabelPosition = CommandBarDefaultLabelPosition.Collapsed, HorizontalAlignment = HorizontalAlignment.Right, IsDynamicOverflowEnabled = false, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
    readonly Grid body = new();
    IViewer? viewer;
    public IViewer? Viewer => viewer;
    bool closingFromCore;
    string? shareRefused;
    bool shareKnown;

    public ValueWindow(DocWindow owner, PopoutModel pop, Pictures pictures)
    {
        this.owner = owner;
        Pop = pop;
        this.pictures = pictures;
        Title = pop.Window.Title;
        SystemBackdrop = new MicaBackdrop();
        Ui.TitleBar(this);
        summary.Foreground = Ui.Brush("TextFillColorSecondaryBrush");
        summary.Style = Ui.TextStyle("CaptionTextBlockStyle");
        var root = new Grid { RequestedTheme = Appearance.Theme };
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.Children.Add(summary);
        var barRow = Ui.Columns(Ui.Star, Ui.Auto);
        barRow.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        barRow.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        barRow.Children.Add(left);
        barRow.Children.Add(bar.At(1));
        // a list's stepper on a line of its own under the bar: an item's name ("logit · 5 of 11") would not
        // fit beside View As and the bar's buttons in a pop-out's width
        Grid.SetColumnSpan(steps, 2);
        barRow.Children.Add(steps.At(0, 1));
        root.Children.Add(barRow.At(0, 1));
        root.Children.Add(new Border { BorderBrush = Ui.Brush("DividerStrokeColorDefaultBrush"), BorderThickness = new Thickness(0, 1, 0, 0), Child = body }.At(0, 2));
        root.KeyDown += async (_, e) =>
        {
            if (e.Key == Windows.System.VirtualKey.Escape && !e.Handled && e.OriginalSource is not TextBox) { e.Handled = true; await PersonClosesAsync(); }
        };
        Content = root;
        AppWindow.Resize(new Windows.Graphics.SizeInt32 { Width = 560, Height = 460 });
        if (CoreHost.Options.Top && AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter op) op.IsAlwaysOnTop = true;
        AppWindow.Closing += async (_, e) =>
        {
            if (closingFromCore) return;
            e.Cancel = true;
            await PersonClosesAsync();
        };
        Closed += (_, _) => { gone = true; pop.Changed -= OnValue; viewer?.Gone(); };
        pop.Changed += OnValue;
    }

    /// <summary>The person closed it: the core's pop-out goes too, and the document's window comes forward.</summary>
    async Task PersonClosesAsync()
    {
        if (closingFromCore) return;
        closingFromCore = true;
        ClosedByPerson?.Invoke();
        await Pop.CloseAsync();
        Close();
        owner.Activate();
    }

    /// <summary>Beside its document's window, on the right where that display's work area has room, else over
    /// the window's right edge; the first four stacked down the side, more cascading over them.</summary>
    public void Place(int k)
    {
        var o = owner.AppWindow.Position;
        var s = owner.AppWindow.Size;
        var area = Microsoft.UI.Windowing.DisplayArea.GetFromWindowId(owner.AppWindow.Id, Microsoft.UI.Windowing.DisplayAreaFallback.Nearest).WorkArea;
        int w = Math.Min(560, area.Width), h = Math.Min(440, Math.Max(240, area.Height / 2));
        AppWindow.Resize(new Windows.Graphics.SizeInt32 { Width = w, Height = h });
        var slot = k % 4;
        var extra = k / 4 * 28;
        var right = o.X + s.Width + 8;
        var x = right + w <= area.X + area.Width ? right : o.X + s.Width - w - 16;
        x += extra;
        var y = o.Y + 40 + slot * (h / 3 + 24) + extra;
        x = Math.Clamp(x, area.X, Math.Max(area.X, area.X + area.Width - w));
        y = Math.Clamp(y, area.Y, Math.Max(area.Y, area.Y + area.Height - h));
        AppWindow.Move(new Windows.Graphics.PointInt32 { X = x, Y = y });
        Placed = (x, y, w, h);
    }

    /// <summary>Where Place put it (the status file says).</summary>
    public (int x, int y, int w, int h) Placed { get; private set; }

    public void CloseFromCore()
    {
        closingFromCore = true;
        Close();
    }

    bool gone;

    void OnValue(PopoutValue v)
    {
        if (!gone) Show(v);
    }

    async void Show(PopoutValue v)
    {
        try { await ShowAsync(v); }
        catch (Exception e) { CoreHost.Log($"pop-out {Pop.Id}: {e}"); }
    }

    async Task ShowAsync(PopoutValue v)
    {
        if (gone) return;
        Title = v.Title ?? Pop.Window.Title;
        summary.Text = v.Summary ?? (v.Media is { } m ? $"{m}, not computed yet" : "Run the graph to see this value.");
        if (!shareKnown) await ReadShare();
        if (gone) return;
        Bar(v);
        var kind = ViewerKind(v);
        if (viewer is null || viewer.Kind != kind)
        {
            viewer?.Gone();
            viewer = Make(kind);
            body.Children.Clear();
            body.Children.Add(viewer.View);
        }
        viewer.Update(v);
        owner.NoteStatus();
    }

    /// <summary>Which viewer draws `v`: the core's choice, an error, or none yet.</summary>
    static string ViewerKind(PopoutValue v)
    {
        if (v.Error is { ValueKind: not JsonValueKind.Null and not JsonValueKind.Undefined }) return "error";
        var id = v.Viewer ?? "";
        if (id == "array.pictures" && (v.Views?.Count ?? 0) <= 1) return "picture";
        return id switch
        {
            "picture" or "svg" => "picture",
            "text.plain" or "scalar" or "table.grid" or "array.grid" or "tree" or "yaml" or "hex" or "audio" or "items" or "array.pictures" => id,
            "" => "none",
            _ => "opaque",
        };
    }

    IViewer Make(string kind) => kind switch
    {
        "picture" => new PictureViewer(Pop, pictures, this),
        "text.plain" => new TextViewer(Pop),
        "scalar" => new ScalarViewer(),
        "table.grid" => new TableViewer(Pop, grid: false),
        "array.grid" => new TableViewer(Pop, grid: true),
        "tree" => new TreeViewer(Pop),
        "yaml" => new YamlViewer(Pop),
        "hex" => new HexViewer(Pop),
        "audio" => new AudioViewer(Pop, pictures),
        "items" => new ItemsViewer(Pop, pictures),
        "array.pictures" => new ArrayPicturesViewer(Pop, pictures),
        "error" => new MessageViewer(v => $"This value failed: {(v.Error is { ValueKind: JsonValueKind.String } e ? e.GetString() : v.Error.Get("message").Str() ?? v.Error?.GetRawText())}", error: true),
        "none" => new MessageViewer(v => v.Summary is null ? "Run the graph to see this value." : "No value yet."),
        _ => new OpaqueViewer(pictures),
    };

    /// <summary>Whether this shell may share (the core's `command.list` says `value.share` is enabled, or why not).</summary>
    async Task ReadShare()
    {
        try
        {
            var list = await Pop.Model.Core.ResultAsync("command.list", Jsonv.EmptyObject, Pop.Model.RequestDoc);
            var rows = list.ValueKind == JsonValueKind.Array ? list.EnumerateArray() : list.Get("commands").Items();
            foreach (var r in rows)
                if (r.Get("id").Str() == "value.share")
                {
                    shareRefused = r.Get("enabled").Bool() == false ? r.Get("disabledReason").Str() ?? "not available" : null;
                    break;
                }
            shareKnown = true;
        }
        catch (Exception) { }
    }

    void Bar(PopoutValue v)
    {
        left.Children.Clear();
        steps.Children.Clear();
        steps.Visibility = v.Items is > 1 ? Visibility.Visible : Visibility.Collapsed;
        if (v.Viewers is { Count: > 1 } offered)
        {
            var asCombo = new ComboBox { MinWidth = 140 };
            foreach (var o in offered)
            {
                var item = new ComboBoxItem { Content = o.Native == false ? $"{o.Label} (picture)" : o.Label, Tag = o.Id };
                asCombo.Items.Add(item);
                if (o.Id == v.Viewer) asCombo.SelectedItem = item;
            }
            ToolTipService.SetToolTip(asCombo, "View As");
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(asCombo, "View As");
            asCombo.SelectionChanged += async (_, _) => { if (asCombo.SelectedItem is ComboBoxItem { Tag: string id } && id != v.Viewer) await Pop.ChooseViewerAsync(id); };
            left.Children.Add(asCombo);
        }
        // the array's view modes (B154 slice 1, 13.2.2): a View picker beside View As, each a `run.view` the core
        // runs; the pictures that follow arrive in `views` or `picture`, and `viewMode` names the one shown
        if (v.ViewModes is { Count: > 0 } modes)
        {
            var shownMode = v.ViewMode?.Id is { Length: > 0 } sm ? sm : "auto";
            var turn = ArrayViews.Turn(v.ViewMode?.Params);
            var viewCombo = new ComboBox { MinWidth = 110 };
            foreach (var m in modes)
            {
                var item = new ComboBoxItem { Content = m, Tag = m };
                ToolTipService.SetToolTip(item, ArrayViews.Tip(m));
                viewCombo.Items.Add(item);
                if (m == shownMode) viewCombo.SelectedItem = item;
            }
            ToolTipService.SetToolTip(viewCombo, "View: how the array's values become pictures");
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(viewCombo, "View");
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(viewCombo, "view-mode");
            viewCombo.SelectionChanged += async (_, _) =>
            {
                if (viewCombo.SelectedItem is not ComboBoxItem { Tag: string id } || id == shownMode) return;
                if (id == "angle") await Pop.CommandAsync("run.view", ("view", Jsonv.Of(id)), ("params", Jsonv.Object(("turn", Jsonv.Of(turn)))));
                else await Pop.CommandAsync("run.view", ("view", Jsonv.Of(id)));
            };
            left.Children.Add(viewCombo);
            if (shownMode == "angle")
            {
                // one full turn in the array's units: 1 for turns, 360 for degrees, 6.283 for radians
                var turnBox = new NumberBox { Value = turn, Minimum = 0.001, Maximum = 1e6, SmallChange = 0.1, Width = 90, SpinButtonPlacementMode = NumberBoxSpinButtonPlacementMode.Compact };
                ToolTipService.SetToolTip(turnBox, "turn: one full turn in the array's units (1 for turns, 360 for degrees, 6.283 for radians)");
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(turnBox, "turn");
                turnBox.ValueChanged += async (_, e) =>
                {
                    if (double.IsFinite(e.NewValue) && e.NewValue > 0 && e.NewValue != turn)
                        await Pop.CommandAsync("run.view", ("view", Jsonv.Of("angle")), ("params", Jsonv.Object(("turn", Jsonv.Of(e.NewValue)))));
                };
                left.Children.Add(turnBox);
            }
        }
        if (v.Items is { } n && n > 1)
        {
            var i = v.Index ?? 0;
            var prev = Ui.GlyphButton(Ui.G.Back, "Previous Item", async () => await Pop.CommandAsync("run.item", ("index", Jsonv.Of(i - 1))));
            prev.IsEnabled = i > 0;
            var next = Ui.GlyphButton(Ui.G.Forward, "Next Item", async () => await Pop.CommandAsync("run.item", ("index", Jsonv.Of(i + 1))));
            next.IsEnabled = (ulong)(i + 1) < n;
            steps.Children.Add(prev);
            // a list of named pictures (contract 8.7): each item by its sheet label, with a pick list of all
            // of them ("logit · 5 of 11"); a list without names reads "item 5 of 11"
            var names = ListNames.Names(v.Sheet);
            var title = ListNames.Title(names, i, n, ListNames.OuterWord(v.Levels));
            if (names.Count == 0)
                steps.Children.Add(new TextBlock { Text = title, VerticalAlignment = VerticalAlignment.Center, Style = Ui.TextStyle("CaptionTextBlockStyle") });
            else
            {
                var menu = new MenuFlyout { Placement = Microsoft.UI.Xaml.Controls.Primitives.FlyoutPlacementMode.Bottom };
                for (var k = 0; k < names.Count; k++)
                {
                    var at = k;
                    var item = new ToggleMenuFlyoutItem { Text = names[k], IsChecked = k == i };
                    item.Click += async (_, _) => { if (at != i) await Pop.CommandAsync("run.item", ("index", Jsonv.Of(at))); };
                    menu.Items.Add(item);
                }
                var pick = new DropDownButton { Content = title, Flyout = menu, VerticalAlignment = VerticalAlignment.Center, FontSize = 12 };
                ToolTipService.SetToolTip(pick, "Go to an item by its name");
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(pick, title);
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(pick, "viewer-items");
                steps.Children.Add(pick);
            }
            steps.Children.Add(next);
            // a list of lists (decision 6A): a stepper a level inside the run, each asking run.item {index, path}
            foreach (var s in ListNames.Steps(v.Path, v.Levels))
            {
                var word = char.ToUpperInvariant(s.Word[0]) + s.Word[1..];
                var before = Ui.GlyphButton(Ui.G.Back, $"Previous {word} in Run", async () => await Pop.CommandAsync("run.item", ("index", Jsonv.Of(i)), ("path", Jsonv.Array(s.PathTo(s.At - 1).Select(x => Jsonv.Of(x))))));
                before.IsEnabled = s.At > 0;
                var after = Ui.GlyphButton(Ui.G.Forward, $"Next {word} in Run", async () => await Pop.CommandAsync("run.item", ("index", Jsonv.Of(i)), ("path", Jsonv.Array(s.PathTo(s.At + 1).Select(x => Jsonv.Of(x))))));
                after.IsEnabled = (ulong)(s.At + 1) < s.Of;
                var where = new TextBlock { Text = $"{s.Word} {s.At + 1} of {s.Of}", VerticalAlignment = VerticalAlignment.Center, Style = Ui.TextStyle("CaptionTextBlockStyle"), Margin = new Thickness(8, 0, 0, 0) };
                Microsoft.UI.Xaml.Automation.AutomationProperties.SetAutomationId(where, $"viewer-level-{s.Level}");
                steps.Children.Add(before);
                steps.Children.Add(where);
                steps.Children.Add(after);
            }
        }
        if (bar.PrimaryCommands.Count > 0) return;
        // Drag Out: a real file drag of what value.export writes
        var handle = new Border { Child = Ui.Icon(Ui.G.Drag, 16), Padding = new Thickness(10, 8, 10, 8), CanDrag = true, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        ToolTipService.SetToolTip(handle, "Drag the value out as a file");
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(handle, "Drag Out");
        handle.DragStarting += async (_, e) =>
        {
            var deferral = e.GetDeferral();
            try
            {
                if (await Pop.ExportFileAsync() is { } path)
                {
                    var file = await Windows.Storage.StorageFile.GetFileFromPathAsync(path);
                    e.Data.SetStorageItems([file]);
                    e.Data.RequestedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.Copy;
                    LastDrag = path;
                }
                else e.Cancel = true;
            }
            catch (Exception ex) { e.Cancel = true; CoreHost.Log($"drag out: {ex.Message}"); }
            deferral.Complete();
        };
        bar.PrimaryCommands.Add(new AppBarElementContainer { Content = handle, VerticalAlignment = VerticalAlignment.Center });
        var compare = new AppBarButton { Label = "Compare", Icon = Ui.Icon(Ui.G.Compare, 16) };
        var pins = new MenuFlyout();
        var pa = new MenuFlyoutItem { Text = "Pin as A" };
        pa.Click += async (_, _) => await Pop.PinAsync("a");
        var pb = new MenuFlyoutItem { Text = "Pin as B" };
        pb.Click += async (_, _) => await Pop.PinAsync("b");
        pins.Items.Add(pa);
        pins.Items.Add(pb);
        compare.Flyout = pins;
        ToolTipService.SetToolTip(compare, "Pin this value as A or B and compare");
        bar.PrimaryCommands.Add(compare);
        // Recipe: what made the value (`viewer.recipe {popout}`), its Copy the core's (`{copy: true}`)
        var recipe = new AppBarButton { Label = "Recipe", Icon = Ui.Icon(Ui.G.Info, 16) };
        ToolTipService.SetToolTip(recipe, "What made this value: its style, parameters and graph");
        var recipeText = new TextBlock { FontFamily = new FontFamily("Cascadia Mono, Consolas"), FontSize = 12, IsTextSelectionEnabled = true, TextWrapping = TextWrapping.NoWrap };
        var copyRecipe = new Button { Content = "Copy Recipe" };
        copyRecipe.Click += async (_, _) => await Pop.CommandAsync("viewer.recipe", ("copy", Jsonv.True));
        var recipePanel = new StackPanel { Spacing = 8, MaxWidth = 520 };
        recipePanel.Children.Add(new ScrollViewer { Content = recipeText, MaxHeight = 360, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollMode = ScrollMode.Auto });
        recipePanel.Children.Add(copyRecipe);
        var recipeFly = new Flyout { Content = recipePanel };
        recipeFly.Opening += async (_, _) =>
        {
            recipeText.Text = "Reading the recipe…";
            var r = await Pop.CommandAsync("viewer.recipe");
            var rec = r?.Get("recipe");
            var has = rec is { ValueKind: not JsonValueKind.Null and not JsonValueKind.Undefined };
            recipeText.Text = has ? JsonSerializer.Serialize(rec!.Value, RecipeJson) : Pop.LastRefusal ?? "No recipe: this value wasn't made by a run with one.";
            copyRecipe.IsEnabled = has;
            LastRecipe = has;
        };
        recipe.Flyout = recipeFly;
        // only a value with a recipe offers it (`hasRecipe`)
        if (v.HasRecipe == true) bar.PrimaryCommands.Add(recipe);
        bar.PrimaryCommands.Add(Out("Copy", Ui.G.Copy, "Copy the value", "value.copy"));
        bar.PrimaryCommands.Add(Out("Save As…", Ui.G.Save, "Save the value to a file", "value.saveAs"));
        var share = Out("Share…", Ui.G.Share, shareRefused ?? "Share the value", "value.share");
        if (shareRefused is { } why)
        {
            // disabled with the core's reason, said where a disabled button's tooltip can't be
            share.IsEnabled = false;
            var wrap = new Border { Child = new AppBarButton { Label = "Share…", Icon = Ui.Icon(Ui.G.Share, 16), IsEnabled = false, LabelPosition = CommandBarLabelPosition.Collapsed, Width = 48 }, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
            ToolTipService.SetToolTip(wrap, $"{why} (value.share)");
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetHelpText(wrap, why);
            bar.PrimaryCommands.Add(new AppBarElementContainer { Content = wrap });
        }
        else bar.PrimaryCommands.Add(share);
    }

    AppBarButton Out(string label, string glyph, string tip, string command)
    {
        var b = new AppBarButton { Label = label, Icon = Ui.Icon(glyph, 16) };
        ToolTipService.SetToolTip(b, tip);
        b.Click += async (_, _) =>
        {
            if (await Pop.TakeOutAsync(command) is { } refused) summary.Text = refused;
        };
        return b;
    }

    static readonly JsonSerializerOptions RecipeJson = new() { WriteIndented = true, TypeInfoResolver = new System.Text.Json.Serialization.Metadata.DefaultJsonTypeInfoResolver() };

    /// <summary>Whether the last recipe read had one.</summary>
    public bool? LastRecipe { get; private set; }

    /// <summary>The file the last drag out carried.</summary>
    public string? LastDrag { get; private set; }

    public System.Text.Json.Nodes.JsonObject Describe()
    {
        var o = new System.Text.Json.Nodes.JsonObject
        {
            ["id"] = Pop.Id, ["title"] = Title, ["viewer"] = Pop.Value?.Viewer, ["drawn"] = viewer?.Kind, ["summary"] = summary.Text,
            ["share"] = shareRefused is null ? "enabled" : $"disabled: {shareRefused}", ["drag"] = LastDrag,
            ["placed"] = new System.Text.Json.Nodes.JsonArray(Placed.x, Placed.y, Placed.w, Placed.h),
        };
        if (viewer?.Describe() is { } d) foreach (var (k, v) in d) o[k] = v?.DeepClone();
        return o;
    }
}
