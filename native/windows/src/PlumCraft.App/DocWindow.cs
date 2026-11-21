// A document's window: the core's document `doc` as the kit's DocumentModel, on the Composition canvas,
// with WinUI's own chrome: the content extended into the title bar over a Mica backdrop (Windows 11; a
// plain backdrop on Windows 10), the menu bar and the toolbar from the core's menu model, an InfoBar for
// what the core says or what isn't built yet, and a status line (the engine, the document, the zoom).
//
// Everything the person does here is a core command by id (contract 12.13, the maintainer: "No
// exceptions to the rule"; N5 slice 3 moved the last of slice 2's shell-side actions into the core): New
// and Open are the core's graph.new and graph.open, which open a document and tell the app to show it
// (the feed's `window` change, `platform.document_windows`); Exit is the core's app.quit, which closes
// every document in turn; the close button is the core's document.close for the person (its prompt says
// close, not quit); About shows app.about's words; the zoom keys are canvas.zoom. What stays the shell's:
// the view's pan and zoom by wheel, drag and pinch, and the window's own chrome.

using System.Numerics;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using PlumCraft.App.Canvas;
using PlumCraft.Core;
using PlumCraft.Kit;
using PlumCraft.Models;
using Windows.Storage.Pickers;
using Windows.System;

namespace PlumCraft.App;

public sealed class DocWindow : Window
{
    public ulong Doc { get; }
    public DocumentModel Model { get; }
    public CanvasView CanvasView { get; }
    public IntPtr Hwnd { get; }
    public bool First { get; }
    readonly Menus menus;
    readonly Asks asks;
    readonly Popouts popouts;
    readonly Pictures pictures;
    public Editors Editors { get; }
    readonly Grid root = new();
    readonly Grid titleBar = new() { Height = 32 };
    readonly TextBlock titleText = new() { VerticalAlignment = VerticalAlignment.Center, FontSize = 12, Margin = new Thickness(12, 0, 0, 0) };
    readonly MenuBar menuBar = new();
    readonly CommandBar toolbar = new() { DefaultLabelPosition = CommandBarDefaultLabelPosition.Right, HorizontalAlignment = HorizontalAlignment.Left, IsDynamicOverflowEnabled = true, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
    readonly InfoBar banner = new() { IsOpen = false, IsClosable = true, Margin = new Thickness(8, 0, 8, 4) };
    readonly TextBlock engineText = new() { FontSize = 12, VerticalAlignment = VerticalAlignment.Center };
    readonly TextBlock stateText = new() { FontSize = 12, VerticalAlignment = VerticalAlignment.Center, Opacity = 0.75 };
    readonly TextBlock zoomText = new() { FontSize = 12, VerticalAlignment = VerticalAlignment.Center, Opacity = 0.75 };
    readonly Microsoft.UI.Xaml.Shapes.Ellipse engineDot = new() { Width = 8, Height = 8, VerticalAlignment = VerticalAlignment.Center };
    bool closingFromCore, closed, viewSet;
    public double? FirstFrameMs { get; private set; }
    public DateTime? FirstFrameWall { get; private set; }
    /// <summary>When the window was made (the core's document opened just before), on CoreHost.Clock.</summary>
    public double OpenedMs { get; } = CoreHost.Clock.Elapsed.TotalMilliseconds;

    public ElementTheme ContentTheme => Appearance.Theme;

    public DocWindow(PlumCore core, ulong doc, bool first)
    {
        Doc = doc;
        First = first;
        Hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        Model = new DocumentModel(core, doc);
        Title = CoreHost.AppName;
        ExtendsContentIntoTitleBar = true;
        SystemBackdrop = new MicaBackdrop();
        var o = CoreHost.Options;
        AppWindow.Resize(new Windows.Graphics.SizeInt32 { Width = first ? o.Width : 1200, Height = first ? o.Height : 800 });
        if (first && o.Top)
        {
            AppWindow.Move(new Windows.Graphics.PointInt32 { X = 0, Y = 0 });
            if (AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter op) op.IsAlwaysOnTop = true;
        }

        CanvasView = new CanvasView(Model);
        Editors = new Editors(this, CanvasView, Model);
        pictures = new Pictures(Model);
        menus = new Menus(menuBar, Dispatch);
        asks = new Asks(this, Model);
        popouts = new Popouts(this, Model, pictures);
        Follow();
        // the scene is asked for before the window's controls are made: it is fetched and decoded off
        // this thread meanwhile
        Model.Start();
        Build();
        Closed += (_, _) =>
        {
            closed = true;
            CompositionTarget.Rendering -= OnRendering;
            popouts.CloseAll();
            Model.Stop();
            CoreHost.Closed(this);
        };
        AppWindow.Closing += async (_, e) =>
        {
            if (closingFromCore) return;
            // the close button is the core's document.close for the person (it asks about unsaved work,
            // in words that say the window closes)
            e.Cancel = true;
            await CloseAsPersonAsync();
        };
        CompositionTarget.Rendering += OnRendering;
        // the canvas has the keyboard when the window is activated (not the menu bar's first item)
        Activated += (_, e) =>
        {
            IsActive = e.WindowActivationState != WindowActivationState.Deactivated;
            if (e.WindowActivationState != WindowActivationState.Deactivated)
                DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, FocusCanvas);
        };
        NoteStatus();
    }

    /// <summary>A new document's window: cascaded from the window it was asked in, kept inside that display's
    /// work area (back to its top left when a cascade would leave it).</summary>
    public void PlaceFrom(DocWindow? from)
    {
        if (from is null) return;
        var o = from.AppWindow.Position;
        var size = AppWindow.Size;
        var area = Microsoft.UI.Windowing.DisplayArea.GetFromWindowId(from.AppWindow.Id, Microsoft.UI.Windowing.DisplayAreaFallback.Nearest).WorkArea;
        int x = o.X + 32, y = o.Y + 32;
        if (x + size.Width > area.X + area.Width || y + size.Height > area.Y + area.Height) { x = area.X + 16; y = area.Y + 16; }
        x = Math.Max(area.X, x);
        y = Math.Max(area.Y, y);
        AppWindow.Move(new Windows.Graphics.PointInt32 { X = x, Y = y });
    }

    /// <summary>The window has the system's activation (focus lost to another window isn't a field's commit).</summary>
    public bool IsActive { get; private set; } = true;

    /// <summary>The canvas takes the keyboard (the window activated, a menu or flyout closed), unless an editor has it.</summary>
    public void FocusCanvas()
    {
        if (closed || Editors.Palette.IsOpen || Editors.Rows.Open is not null || Editors.Names.Open is not null) return;
        if (Content?.XamlRoot is { } xr && FocusManager.GetFocusedElement(xr) is TextBox or ComboBox) return;
        CanvasView.Focus(FocusState.Programmatic);
    }

    // ---------------- the window's parts ----------------

    void Build()
    {
        root.RequestedTheme = Appearance.Theme;
        foreach (var h in new[] { GridLength.Auto, GridLength.Auto, GridLength.Auto, GridLength.Auto, new GridLength(1, GridUnitType.Star), GridLength.Auto })
            root.RowDefinitions.Add(new RowDefinition { Height = h });
        // the title bar: the app's name and the document's, dragged as the window's caption
        titleBar.Children.Add(titleText);
        root.Children.Add(titleBar);
        SetTitleBar(titleBar);
        Grid.SetRow(menuBar, 1);
        root.Children.Add(menuBar);
        Grid.SetRow(toolbar, 2);
        root.Children.Add(toolbar);
        Grid.SetRow(banner, 3);
        root.Children.Add(banner);
        var canvasBorder = new Microsoft.UI.Xaml.Controls.Border { Child = CanvasView, BorderThickness = new Thickness(0, 1, 0, 1) };
        canvasBorder.BorderBrush = (Brush)Application.Current.Resources["CardStrokeColorDefaultBrush"];
        Grid.SetRow(canvasBorder, 4);
        root.Children.Add(canvasBorder);
        var status = new Grid { Padding = new Thickness(12, 3, 12, 4), ColumnSpacing = 16 };
        status.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        status.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        status.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var engine = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        engine.Children.Add(engineDot);
        engine.Children.Add(engineText);
        status.Children.Add(engine);
        Grid.SetColumn(stateText, 1);
        status.Children.Add(stateText);
        Grid.SetColumn(zoomText, 2);
        status.Children.Add(zoomText);
        Grid.SetRow(status, 5);
        root.Children.Add(status);
        root.KeyDown += OnKey;
        Content = root;
        FollowTheme();
        ShowEngine(Model.Engine);
        ShowState(Model.State);
    }

    /// <summary>The caption buttons' colours follow the app's appearance (the content is under them).</summary>
    public void FollowTheme()
    {
        if (!Microsoft.UI.Windowing.AppWindowTitleBar.IsCustomizationSupported()) return;
        var dark = Appearance.Dark;
        var tb = AppWindow.TitleBar;
        tb.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
        tb.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
        tb.ButtonForegroundColor = dark ? Microsoft.UI.Colors.White : Microsoft.UI.Colors.Black;
        tb.ButtonInactiveForegroundColor = dark ? Windows.UI.Color.FromArgb(255, 150, 150, 150) : Windows.UI.Color.FromArgb(255, 120, 120, 120);
        tb.ButtonHoverBackgroundColor = dark ? Windows.UI.Color.FromArgb(40, 255, 255, 255) : Windows.UI.Color.FromArgb(24, 0, 0, 0);
    }

    void Follow()
    {
        Model.MenusChanged += m => menus.Show(m);
        Model.ToolbarChanged += ShowToolbar;
        Model.StateChanged += ShowState;
        Model.EngineChanged += ShowEngine;
        Model.AsksChanged += a => { asks.Follow(a); NoteStatus(); };
        Model.WindowsChanged += w => { popouts.Follow(w); NoteUnbuiltWindows(w); };
        Model.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName == nameof(DocumentModel.Closing) && Model.Closing) CloseFromCore();
            if (e.PropertyName == nameof(DocumentModel.LastError) && Model.LastError is { } err) CoreHost.Log($"doc {Doc}: {err}");
        };
        CanvasView.GestureAnswered += r =>
        {
            LastGesture = string.Join(",", r.Commands.Select(c => c.Command));
            if (NotBuilt(r.Commands) is { } say) Banner(say); else NoteStatus();
        };
        CanvasView.ContextMenu += (p, items) =>
        {
            var f = menus.Flyout(items);
            f.Closed += (_, _) => { LastMenu = 0; FocusCanvas(); NoteStatus(); };
            f.ShowAt(CanvasView, new FlyoutShowOptions { Position = p });
            LastMenu = items.Count;
            NoteStatus();
        };
        CanvasView.Failed += e => Banner(e, true);
        CanvasView.InputSeen += NoteStatus;
        CanvasView.Drag.Ended += (_, _) => NoteStatus();
        CanvasView.EscapeUnhandled += async () =>
        {
            // Escape: a run in flight cancelled (run.cancel, as 12.3 binds it), else a net's selection cleared
            if (FindItem("run.cancel") is { Enabled: true } cancel) await Dispatch(cancel, true);
            else if (Model.Selection.Net is not null) await Run("net.select", Jsonv.EmptyObject);
            else if (Model.Selection.Nodes.Count > 0 || Model.Selection.Groups.Count > 0) await Run("selection.clear", Jsonv.EmptyObject);
        };
        CanvasView.BackspacePressed += async () =>
        {
            if (FindItem("edit.delete") is { } del) { if (del.Enabled) await Dispatch(del, true); }
            else await Run("edit.delete", Jsonv.EmptyObject);
        };
        CanvasView.Canvas.Trouble += e => Banner(e, true);
        var viewNoted = DispatcherQueue.CreateTimer();
        viewNoted.Interval = TimeSpan.FromMilliseconds(300);
        viewNoted.IsRepeating = false;
        viewNoted.Tick += (_, _) => NoteStatus();
        CanvasView.Canvas.ViewChanged += v =>
        {
            zoomText.Text = $"{Math.Round(v.Zoom * 100)}%";
            viewNoted.Stop();
            viewNoted.Start();
        };
        CanvasView.Canvas.Drawn += _ => { if (FirstFrameMs is null) NoteStatus(); };
        zoomText.Text = "100%";
    }

    /// <summary>The commands the last gesture ran (the status file says, for the input probe).</summary>
    public string LastGesture { get; private set; } = "";

    /// <summary>Items in the last context menu shown (the status file says, for scripts).</summary>
    public int LastMenu { get; private set; }

    void ShowState(DocumentState s)
    {
        // the document's name, as Windows titles a document window ("name - App"); the core's longer
        // title (with its engines) is in the status line's engine part
        var name = s.Name.Length > 0 ? s.Name : s.Path is { } p ? Path.GetFileName(p) : "Untitled";
        Title = $"{(s.Dirty ? "*" : "")}{name} - {CoreHost.AppName}";
        titleText.Text = Title;
        stateText.Text = s.LastError ?? (s.Dirty ? "Edited" : s.Path ?? "");
        NoteStatus();
    }

    void ShowEngine(EngineState e)
    {
        var col = e.Ready ? Windows.UI.Color.FromArgb(255, 46, 160, 67) : e.Connected ? Windows.UI.Color.FromArgb(255, 210, 153, 34) : Windows.UI.Color.FromArgb(255, 150, 150, 150);
        engineDot.Fill = new SolidColorBrush(col);
        engineText.Text = e.Label.Length > 0 ? e.Label : e.Connected ? "Engine connected" : "Engine starting";
        NoteStatus();
    }

    /// <summary>The core's toolbar (menu.get {context: {kind: "toolbar"}}): a button a command, a toggle a check.</summary>
    void ShowToolbar(IReadOnlyList<MenuItem> items)
    {
        toolbar.PrimaryCommands.Clear();
        foreach (var it in items)
        {
            if (it.Separator) { toolbar.PrimaryCommands.Add(new AppBarSeparator()); continue; }
            ICommandBarElement el;
            if (it.Checked is not null)
            {
                var t = new AppBarToggleButton { Label = it.Label, IsChecked = it.Checked == true, IsEnabled = it.Enabled, Icon = Glyph(it.Icon) };
                t.Click += async (_, _) => await Dispatch(it, true);
                el = t;
            }
            else
            {
                var b = new AppBarButton { Label = it.Label, IsEnabled = it.Enabled, Icon = Glyph(it.Icon) };
                b.Click += async (_, _) => await Dispatch(it, true);
                el = b;
            }
            ToolTipService.SetToolTip((DependencyObject)el, !it.Enabled && it.Reason is { Length: > 0 } r ? r : it.Hover ?? it.Label);
            toolbar.PrimaryCommands.Add(el);
        }
    }

    static readonly string IconFont = Environment.OSVersion.Version.Build >= 22000 ? "Segoe Fluent Icons" : "Segoe MDL2 Assets";

    /// <summary>The platform's glyph for a toolbar item's Lucide icon (contract 12.18), or none.</summary>
    static IconElement? Glyph(string? lucide)
    {
        if (lucide is null) return null;
        string? g = lucide switch
        {
            _ when lucide.Contains("play") => "",
            _ when lucide.Contains("square") && !lucide.Contains("dashed") || lucide.Contains("stop") => "",
            _ when lucide.Contains("plus") => "",
            _ when lucide.Contains("undo") => "",
            _ when lucide.Contains("redo") => "",
            _ when lucide.Contains("save") => "",
            _ when lucide.Contains("folder") => "",
            _ when lucide.Contains("share") => "",
            _ when lucide.Contains("maximize") || lucide.Contains("scan") || lucide.Contains("expand") || lucide.Contains("fit") => "",
            _ when lucide.Contains("dashed") || lucide.Contains("pointer") => "",
            _ when lucide.Contains("settings") => "",
            _ when lucide.Contains("search") => "",
            _ when lucide.Contains("layout") || lucide.Contains("grid") || lucide.Contains("align") => "",
            _ when lucide.Contains("x") => "",
            _ => null,
        };
        return g is null ? null : new FontIcon { Glyph = g, FontFamily = new FontFamily(IconFont) };
    }

    /// <summary>Say something above the canvas (a refusal, what isn't built yet).</summary>
    public void Banner(string message, bool error = false)
    {
        banner.Severity = error ? InfoBarSeverity.Error : InfoBarSeverity.Informational;
        banner.Message = message;
        banner.IsOpen = true;
        NoteStatus();
    }

    // ---------------- dispatching the core's items ----------------

    /// <summary>Commands whose window this app doesn't have yet (N5 slices 3 and 4): choosing one says so.</summary>
    static readonly Dictionary<string, string> Unbuilt = new()
    {
        ["node.parameters"] = "the parameters inspector",
        ["app.settings"] = "Settings",
        ["view.messages"] = "the Messages window",
        ["run.cacheReport"] = "the Cache Report",
        ["view.params"] = "the graph's Parameters window",
        ["node.editor"] = "value editors",
        ["viewer.compare"] = "Compare",
        ["viewer.pin"] = "Compare",
    };

    /// <summary>The menu model's item for a command (the menu bar's, then the toolbar's).</summary>
    MenuItem? FindItem(string command) =>
        menus.Model.SelectMany(m => m.Flatten()).FirstOrDefault(i => i.Command == command && i.Params is null)
        ?? Model.Toolbar.FirstOrDefault(i => i.Command == command);

    async Task Run(string command, System.Text.Json.JsonElement p)
    {
        try { await Model.RunAsync(command, p); }
        catch (Exception e) { Banner($"{command}: {e.Message}", true); }
    }

    static string? NotBuilt(IEnumerable<CommandRun> ran)
    {
        foreach (var c in ran)
            if (c.Ok && Unbuilt.TryGetValue(c.Command, out var what)) return $"That opens {what}, which isn't in {CoreHost.AppName} for Windows yet.";
        return null;
    }

    void NoteUnbuiltWindows(DocumentWindows w)
    {
        if (w.Parameters is not null) Banner($"The parameters inspector isn't in {CoreHost.AppName} for Windows yet.");
        else if (w.Settings is not null) Banner($"Settings isn't in {CoreHost.AppName} for Windows yet.");
    }

    /// <summary>Run a model item for the person at this window (the menu bar, a context menu, the toolbar,
    /// a shortcut): its command by id.</summary>
    public async Task Dispatch(MenuItem it, bool person)
    {
        if (it.Command is not { } command) return;
        if (Unbuilt.TryGetValue(command, out var what))
        {
            Banner($"{it.Label.Replace("…", "", StringComparison.Ordinal)} isn't in {CoreHost.AppName} for Windows yet ({what}).");
            return;
        }
        try
        {
            switch (command)
            {
                // the core's words for About, shown in the platform's dialog
                case "app.about":
                    await AboutAsync();
                    return;
                // Node › Search…, Tab, Add Node: the palette at the pointer (the core's default is the view's centre)
                case "canvas.palette" when it.Params is null:
                    await Editors.OpenPaletteAsync();
                    return;
            }
            await Model.ActivateAsync(it, person);
        }
        catch (Exception e)
        {
            Banner($"{it.Label}: {e.Message}", true);
        }
    }

    /// <summary>The file picker is showing (scripts screenshot it).</summary>
    public bool Picking { get; private set; }

    /// <summary>Help › About: the core's words (app.about) in a dialog.</summary>
    async Task AboutAsync()
    {
        var a = await Model.RunAsync("app.about");
        var lines = new List<string>();
        if (a.Get("version").Str() is { } v) lines.Add($"Version {v}");
        if (a.Get("parts") is { ValueKind: JsonValueKind.Array } parts)
            foreach (var p in parts.EnumerateArray())
                if (p.ValueKind == JsonValueKind.Array && p.GetArrayLength() >= 2) lines.Add($"{((JsonElement?)p[0]).Str()}: {((JsonElement?)p[1]).Str()}");
        if (a.Get("engine").Str() is { Length: > 0 } e) lines.Add($"Engine: {e}");
        var d = new ContentDialog
        {
            Title = a.Get("title").Str() ?? CoreHost.AppName, XamlRoot = Content.XamlRoot, RequestedTheme = ContentTheme,
            Content = new TextBlock { Text = string.Join("\n", lines), IsTextSelectionEnabled = true }, CloseButtonText = "OK",
        };
        await d.ShowAsync();
    }

    // ---------------- keys ----------------

    async void OnKey(object sender, KeyRoutedEventArgs e)
    {
        if (e.Handled) return;
        // a text field has the keyboard: plain keys are its own, and so are the edit shortcuts (12.2)
        var focused = FocusManager.GetFocusedElement(Content.XamlRoot);
        var text = focused is TextBox or PasswordBox or RichEditBox or ComboBox or AutoSuggestBox;
        if (menus.Match(e.Key, !text) is { } it)
        {
            if (text && (it.Command?.StartsWith("edit.", StringComparison.Ordinal) == true || it.Command == "selection.all")) return;
            e.Handled = true;
            // a shortcut fired in a field runs after the field commits
            if (text) await Editors.CommitOpenFieldAsync();
            if (it.Enabled) await Dispatch(it, true);
            return;
        }
        // the zoom keys, when the core's menus give them to nothing: the core's canvas.zoom (N5 slice 3)
        var ctrl = (Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        if (!ctrl || text) return;
        System.Text.Json.JsonElement? zoom = (int)e.Key switch
        {
            187 or (int)VirtualKey.Add => Jsonv.Object(("by", Jsonv.Of(1.25))),
            189 or (int)VirtualKey.Subtract => Jsonv.Object(("by", Jsonv.Of(0.8))),
            (int)VirtualKey.Number0 or (int)VirtualKey.NumberPad0 => Jsonv.Object(("to", Jsonv.Of(1.0))),
            _ => null,
        };
        if (zoom is { } z)
        {
            e.Handled = true;
            await Run("canvas.zoom", z);
        }
    }

    // ---------------- closing ----------------

    /// <summary>Close as the person would (the close button): the core's document.close for the person,
    /// which asks about unsaved changes (Save, Don't Save, Cancel: "close the window"); whether it closed.</summary>
    public async Task<bool> CloseAsPersonAsync()
    {
        if (closed) return true;
        try
        {
            await Model.Core.ResultAsync("document.close", Jsonv.Object(("doc", Jsonv.Of(Doc)), ("by", Jsonv.Of("person"))));
        }
        catch (Exception e)
        {
            Banner($"Close: {e.Message}", true);
            return false;
        }
        // closed at once, or after the unsaved-changes prompt is answered (or cancelled)
        await Model.WaitAsync(() => Model.Closing || closed || !Model.Asks.Exists(a => a.Kind == "prompt"), TimeSpan.FromHours(1));
        if (!Model.Closing && !closed) await Model.WaitAsync(() => Model.Closing || closed, TimeSpan.FromSeconds(2));
        return Model.Closing || closed;
    }

    void CloseFromCore()
    {
        if (closingFromCore) return;
        closingFromCore = true;
        DispatcherQueue.TryEnqueue(() => { if (!closed) Close(); });
    }

    // ---------------- the first frame, the status file ----------------

    void OnRendering(object? sender, object e)
    {
        var c = CanvasView.Canvas;
        if (!viewSet && c.FirstDrawnMs is not null)
        {
            viewSet = true;
            if (First && CoreHost.Options.View is { } v) c.SetView(new Viewport(new Vector2(-v[1] * v[0], -v[2] * v[0]), v[0]));
        }
        if (FirstFrameMs is null && c.FirstDrawnMs is not null)
        {
            FirstFrameMs = CoreHost.Clock.Elapsed.TotalMilliseconds;
            FirstFrameWall = DateTime.Now;
            if (First) CoreHost.Mark("first_frame");
            NoteStatus();
            if (First) Script.Start(this);
        }
    }

    public void NoteStatus()
    {
        if (CoreHost.Status.Path is null) return;
        var c = CanvasView.Canvas;
        var o = new JsonObject
        {
            ["doc"] = Doc, ["title"] = Title, ["ready"] = FirstFrameMs is not null || (Model.Applied > 0 && c.FaceCount == 0 && c.Idle),
            ["faces"] = c.FaceCount, ["measured"] = c.AllMeasured, ["idle"] = c.Idle, ["hidden"] = c.Hidden,
            ["firstFrameMs"] = FirstFrameMs, ["zoom"] = c.View.Zoom, ["pan"] = new JsonArray(c.View.Pan.X, c.View.Pan.Y),
            ["selected"] = Model.Selection.Nodes.Count, ["gesture"] = LastGesture, ["pointer"] = new JsonArray([.. CanvasView.Seen.Select(x => (JsonNode)x)]), ["engine"] = Model.Engine.Label, ["dirty"] = Model.State.Dirty,
            ["banner"] = banner.IsOpen ? banner.Message : null, ["asks"] = asks.Shown, ["showing"] = asks.Showing, ["picking"] = Picking,
            ["contextMenu"] = LastMenu, ["popouts"] = popouts.Describe(), ["menus"] = menus.Model.Count, ["toolbar"] = toolbar.PrimaryCommands.Count,
            ["drag"] = CanvasView.DragState, ["fits"] = CanvasView.TargetFits, ["note"] = CanvasView.LastNote, ["editors"] = Editors.Describe(),
            ["focus"] = (Content?.XamlRoot is { } xr ? FocusManager.GetFocusedElement(xr)?.GetType().Name : null), ["undo"] = Model.State.Undo,
            ["wires"] = Model.Wires.Count, ["drawOnUi"] = c.DrawOnUiThread, ["counts"] = new JsonObject { ["laid"] = c.Counts.FacesLaidOut, ["drawn"] = c.Counts.FacesDrawn, ["batches"] = c.Counts.Batches },
        };
        CoreHost.Status.Window(Doc, o);
    }

    /// <summary>The menu bar's first item (scripts open it).</summary>
    public MenuBar MenuBar => menuBar;
}
