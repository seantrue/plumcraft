// The canvas's XAML host (N5 slice 3, editing): it recognises input and hands it to the core as
// `ui.gesture` (the core decides what a gesture means, contract 8.13), and keeps the view's pan and zoom,
// which are the shell's (12.13).
//
// Mouse and pen: a click is a tap (Shift adds, Ctrl takes away), a second within the system's double-click
// time and rectangle a double tap (what it opens is the core's: since egui's parity work the core hit-tests
// it as its first tap was, gesture::DOUBLE_TAP), a right click or Shift+F10 the core's context menu, a drag
// the core's drag (a face or the selection moved with the core's previews, a wire drawn from a pin with its
// live route and its target ringed, a wire's end picked up, a marquee on empty canvas, a frame, a group or a
// bend point, a number scrubbed), autoscrolling near the view's edge; Escape cancels it. Touch: one finger
// on a face, a pin or a frame's tab drags as the mouse does; on empty canvas it pans (or draws the marquee
// in Select mode); two pinch (and cancel a drag under way); a hold is the context menu (the system's
// Holding). The view: the wheel pans (Shift or a horizontal wheel sideways), Ctrl+wheel zooms about the
// pointer, the middle button drags it.
//
// The keyboard: the canvas is a tab stop and takes focus when pressed and when its window is activated;
// arrows nudge the selection (selection.nudge, a key's repeats one undo step), Escape cancels a drag, then
// a run (run.cancel), then a net's selection; Backspace is Delete's edit.delete. The core's shortcuts (Delete,
// F, Tab, Ctrl+Z ...) are the window's (Menus). A field being edited keeps its own keys.

using System.Numerics;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using PlumCraft.Core;
using PlumCraft.Kit;
using PlumCraft.Models;
using Windows.System;
using MRect = PlumCraft.Models.Rect;

namespace PlumCraft.App.Canvas;

public sealed class CanvasView : Grid
{
    public CompositionCanvas Canvas { get; }
    public DocumentModel Model { get; }
    public DragDriver Drag { get; }
    /// <summary>XAML above the canvas: the field editors, the scrub's value, notes.</summary>
    public Microsoft.UI.Xaml.Controls.Canvas Overlay { get; } = new();
    CanvasLink Link => Canvas.Link;
    readonly Grid surface = new() { Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };

    /// <summary>A gesture was answered: what it ran (the window says what isn't built yet).</summary>
    public event Action<GestureReply>? GestureAnswered;
    /// <summary>The core gave a menu for what is at a view point (a right click, the more-button).</summary>
    public event Action<Windows.Foundation.Point, List<MenuItem>>? ContextMenu;
    public event Action<string>? Failed;
    /// <summary>The pointer is over the canvas here (view points), for a palette opened by a key.</summary>
    public Vector2? Pointer { get; private set; }

    // a press that may become a drag
    Vector2? down;
    PointerDeviceType downType;
    List<ModifierWord> downMods = [];
    bool downPans, middlePanning;
    Vector2 last;
    uint? pressPointer;
    // the last click, for a double click by the system's rule
    Vector2 lastClickAt;
    long lastClickTicks;
    // touches
    readonly Dictionary<uint, Vector2> touches = [];
    float pinchDist;
    Vector2 pinchMid;
    Viewport pinchView;
    bool held;
    // the drag's own state for drawing
    Vector2? anchor;
    MRect? ghostFrom;
    bool ghostRound;
    MRect? scrubRow;
    Widget? scrubWidget;
    readonly TextBlock scrubText = new() { FontSize = 12, Visibility = Visibility.Collapsed, IsHitTestVisible = false };
    readonly Microsoft.UI.Xaml.Controls.Border scrubBox = new() { Padding = new Thickness(6, 2, 6, 2), CornerRadius = new CornerRadius(4), Visibility = Visibility.Collapsed, IsHitTestVisible = false };
    readonly Microsoft.UI.Dispatching.DispatcherQueueTimer scroller;
    Vector2? lastView;
    ulong keyPress;

    const float ClickSlop = 4, Edge = 36, MaxScroll = 18;

    public CanvasView(DocumentModel model)
    {
        Model = model;
        Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        Children.Add(surface);
        Children.Add(Overlay);
        Canvas = new CompositionCanvas(surface, model);
        Drag = new DragDriver(Canvas.Link);
        scrubBox.Child = scrubText;
        Overlay.Children.Add(scrubBox);
        SizeChanged += (_, e) => Canvas.SetViewSize(new Vector2((float)e.NewSize.Width, (float)e.NewSize.Height));
        Loaded += (_, _) =>
        {
            if (XamlRoot is { } xr)
            {
                Canvas.DisplayScale = (float)xr.RasterizationScale;
                xr.Changed += (s, _) => Canvas.DisplayScale = (float)s.RasterizationScale;
            }
            Focus(FocusState.Programmatic);
        };
        IsTabStop = true;
        UseSystemFocusVisuals = false;
        TabFocusNavigation = KeyboardNavigationMode.Local;
        ManipulationMode = ManipulationModes.None;
        PointerPressed += OnPressed;
        PointerMoved += OnMoved;
        PointerReleased += OnReleased;
        PointerCanceled += OnCanceled;
        // a capture this view lets go of on a release is lost too: only a loss it didn't ask for cancels
        // (B50: slice 2 released capture first, and the loss it raised dropped every mouse click)
        PointerCaptureLost += (s, e) => { if (!releasing) OnCanceled(s, e); };
        PointerExited += (_, _) => { Pointer = null; Canvas.Hover(null); PinTip(null, default); };
        // a pin's own words on hover (its row's `hover`: "Type: x.ports.tune", 8.4), as a tooltip; the
        // service would open it over any part of the canvas, so it opens only over a pin
        ToolTipService.SetToolTip(this, pinTip);
        pinTip.Opened += (_, _) => { if (tipText is null) pinTip.IsOpen = false; };
        PointerWheelChanged += OnWheel;
        ContextRequested += OnContextRequested;
        Holding += OnHolding;
        KeyDown += OnKey;
        scroller = Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread().CreateTimer();
        scroller.Interval = TimeSpan.FromMilliseconds(16);
        scroller.Tick += (_, _) => Autoscroll();
        Drag.Began += OnBegan;
        Drag.Moved += OnDragMoved;
        Drag.Ended += OnEnded;
        Drag.Failed += m => Failed?.Invoke(m);
    }

    protected override Microsoft.UI.Xaml.Automation.Peers.AutomationPeer OnCreateAutomationPeer() => new CanvasPeer(this);

    float Slop(PointerDeviceType t) => (float)CanvasLink.Slop(t == PointerDeviceType.Touch ? 22 : 10, Canvas.View.Zoom);

    Point Graph(Vector2 v)
    {
        var g = Canvas.View.ToGraph(v);
        return new Point(g.X, g.Y);
    }

    /// <summary>Is the pointer over the Browse square of a face's path row (B133)? A press there while that row's
    /// field is open is the core's `row.browse`, which closes the field without its text.</summary>
    public bool PointerOverBrowse(string face, int row)
    {
        if (Pointer is not { } p || Canvas.RowRect(face, row) is not { } r) return false;
        var g = Graph(p);
        var sq = PathBrowse.Square(r);
        return g.X >= sq.X0 && g.X <= sq.X1 && g.Y >= sq.Y0 && g.Y <= sq.Y1;
    }

    static Vector2 V(Windows.Foundation.Point p) => new((float)p.X, (float)p.Y);

    static List<ModifierWord> Mods(VirtualKeyModifiers m)
    {
        var o = new List<ModifierWord>();
        if ((m & VirtualKeyModifiers.Shift) != 0) o.Add(ModifierWord.Shift);
        if ((m & VirtualKeyModifiers.Control) != 0) o.Add(ModifierWord.Primary);
        if ((m & VirtualKeyModifiers.Menu) != 0) o.Add(ModifierWord.Alt);
        return o;
    }

    /// <summary>What a one-finger drag pans over (touch, 5.5): empty canvas, a wire, a tag, a frame's body.</summary>
    bool PansHere(Vector2 v)
    {
        if (Model.View.SelectMode) return false;
        var h = Link.HitAt(Graph(v), Slop(PointerDeviceType.Touch));
        if (!h.Ok) return true;
        return h.KindOf switch
        {
            null or HitKind.None or HitKind.Wire or HitKind.Tag => true,
            HitKind.Frame => h.Part == 1,
            _ => false,
        };
    }

    /// <summary>The last pointer events seen (the status file says, for the input probe).</summary>
    public readonly List<string> Seen = [];

    void Note(string what, PointerRoutedEventArgs e)
    {
        var p = e.GetCurrentPoint(this);
        Seen.Add($"{what} {e.Pointer.PointerDeviceType} {(int)p.Position.X},{(int)p.Position.Y}{(p.Properties.IsLeftButtonPressed ? " L" : "")}{(p.Properties.IsRightButtonPressed ? " R" : "")}");
        if (Seen.Count > 8) Seen.RemoveAt(0);
        InputSeen?.Invoke();
    }

    /// <summary>A press or release arrived (the window writes its status: the input probe reads it).</summary>
    public event Action? InputSeen;

    // ---------------- pointer events: what they are, then the same entry points scripts use ----------------

    void OnPressed(object sender, PointerRoutedEventArgs e)
    {
        Note("pressed", e);
        var pt = e.GetCurrentPoint(this);
        var at = V(pt.Position);
        var type = e.Pointer.PointerDeviceType;
        // the canvas takes the keyboard (a field being edited keeps it: its own control is pressed, not this)
        if (ReferenceEquals(e.OriginalSource, this) || ReferenceEquals(e.OriginalSource, surface)) Focus(FocusState.Pointer);
        if (type == PointerDeviceType.Touch)
        {
            CapturePointer(e.Pointer);
            touches[e.Pointer.PointerId] = at;
            if (touches.Count == 1)
            {
                held = false;
                pressPointer = e.Pointer.PointerId;
                Press(at, [], type, PansHere(at));
            }
            else if (touches.Count == 2)
            {
                // a pinch: a drag under way is cancelled, the view follows both fingers
                if (Drag.Active) _ = Drag.CancelAsync();
                down = null;
                var p = touches.Values.ToArray();
                pinchDist = MathF.Max(Vector2.Distance(p[0], p[1]), 1);
                pinchMid = (p[0] + p[1]) / 2;
                pinchView = Canvas.View;
            }
            e.Handled = true;
            return;
        }
        var props = pt.Properties;
        if (props.IsMiddleButtonPressed)
        {
            CapturePointer(e.Pointer);
            middlePanning = true;
            last = at;
            e.Handled = true;
            return;
        }
        if (!props.IsLeftButtonPressed) return; // the right button (and a pen's barrel button): ContextRequested
        CapturePointer(e.Pointer);
        pressPointer = e.Pointer.PointerId;
        Press(at, Mods(e.KeyModifiers), type, false);
        e.Handled = true;
    }

    void OnMoved(object sender, PointerRoutedEventArgs e)
    {
        var at = V(e.GetCurrentPoint(this).Position);
        var type = e.Pointer.PointerDeviceType;
        if (type != PointerDeviceType.Touch) Pointer = at;
        if (type == PointerDeviceType.Touch)
        {
            if (!touches.ContainsKey(e.Pointer.PointerId)) return;
            touches[e.Pointer.PointerId] = at;
            if (touches.Count >= 2)
            {
                var p = touches.Values.Take(2).ToArray();
                var dist = MathF.Max(Vector2.Distance(p[0], p[1]), 1);
                var mid = (p[0] + p[1]) / 2;
                var z = Math.Clamp(pinchView.Zoom * dist / pinchDist, Viewport.MinZoom, Viewport.MaxZoom);
                var g = pinchView.ToGraph(pinchMid);
                Canvas.SetView(new Viewport(mid - g * z, z));
            }
            else if (e.Pointer.PointerId == pressPointer && !held) MoveTo(at);
            e.Handled = true;
            return;
        }
        if (middlePanning)
        {
            Canvas.PanBy(at - last);
            last = at;
            e.Handled = true;
            return;
        }
        if (e.Pointer.PointerId != pressPointer)
        {
            // a mouse or pen hovering with nothing pressed: a ring on a pin, a wire drawn wider (the typed hit test)
            if (pressPointer is null && !Drag.Active) HoverAt(at, type);
            return;
        }
        MoveTo(at);
        e.Handled = true;
    }

    bool releasing;

    void OnReleased(object sender, PointerRoutedEventArgs e)
    {
        releasing = true;
        try
        {
            Released(e);
            ReleasePointerCapture(e.Pointer);
        }
        finally { releasing = false; }
    }

    async void Released(PointerRoutedEventArgs e)
    {
        Note("released", e);
        var at = V(e.GetCurrentPoint(this).Position);
        var type = e.Pointer.PointerDeviceType;
        if (type == PointerDeviceType.Touch)
        {
            if (!touches.Remove(e.Pointer.PointerId)) return;
            if (touches.Count == 1)
            {
                // the pinch ended: the other finger goes on panning from where it is
                last = touches.Values.First();
                down = last;
                downPans = true;
                pressPointer = touches.Keys.First();
                return;
            }
            if (touches.Count == 0 && !held && e.Pointer.PointerId == pressPointer) await Release(at);
            else if (touches.Count == 0) { down = null; pressPointer = null; }
            e.Handled = true;
            return;
        }
        if (middlePanning)
        {
            middlePanning = false;
            return;
        }
        if (e.Pointer.PointerId != pressPointer) return;
        await Release(at);
    }

    void OnCanceled(object sender, PointerRoutedEventArgs e)
    {
        if (e.Pointer.PointerDeviceType == PointerDeviceType.Touch)
        {
            touches.Remove(e.Pointer.PointerId);
            if (touches.Count > 0) return;
        }
        if (middlePanning) { middlePanning = false; return; }
        if (e.Pointer.PointerId != pressPointer) return;
        down = null;
        pressPointer = null;
        scroller.Stop();
        if (Drag.Active) _ = Drag.CancelAsync();
    }

    void OnHolding(object sender, HoldingRoutedEventArgs e)
    {
        // a touch or pen held still: the core's context menu (a drag under way isn't a hold)
        if (e.HoldingState != Microsoft.UI.Input.HoldingState.Started || Drag.Active) return;
        if (down is { } d && Vector2.Distance(d, last) > ClickSlop * 2) return;
        held = true;
        down = null;
        e.Handled = true;
        _ = ContextAt(V(e.GetPosition(this)), PointerDeviceType.Touch);
    }

    /// <summary>A press at view point `at`: a mouse's or pen's, or a finger's (`pans`: on empty canvas, where a
    /// finger pans). Scripts enter here, as a person's input does.</summary>
    public void Press(Vector2 at, List<ModifierWord> mods, PointerDeviceType type, bool pans)
    {
        down = at;
        last = at;
        downType = type;
        downMods = mods;
        downPans = pans;
        lastView = at;
    }

    /// <summary>The pressed pointer is at `at`: past the click's slop, a drag (or a pan, for a finger on empty canvas).</summary>
    public void MoveTo(Vector2 at)
    {
        if (down is not { } d) return;
        if (downPans)
        {
            if (Vector2.Distance(at, d) > 6 || Drag.Active || last != d) Canvas.PanBy(at - last);
            last = at;
            return;
        }
        if (!Drag.Active && Vector2.Distance(at, d) <= ClickSlop) return;
        if (!Drag.Active) Drag.Begin(Graph(d), downMods, Slop(downType), Canvas.View.Zoom);
        last = at;
        lastView = at;
        Drag.Move(Graph(at), Slop(downType));
        if (ScrollStep(at) != Vector2.Zero && Drag.Kind != DragWord.Scrub) scroller.Start();
    }

    /// <summary>The pressed pointer let go at `at`: the drag's end, or a click.</summary>
    public async Task Release(Vector2 at)
    {
        var d = down;
        var mods = downMods;
        var type = downType;
        var panned = downPans && d is { } p && Vector2.Distance(p, at) > 6;
        down = null;
        pressPointer = null;
        scroller.Stop();
        if (Drag.Active)
        {
            var r = await Drag.EndAsync(Graph(at), Slop(type));
            if (r is not null) GestureAnswered?.Invoke(r);
            return;
        }
        if (d is null || panned) return;
        await Tap(at, mods, type);
    }

    /// <summary>Escape, a second finger: the drag under way is cancelled, the document as it was.</summary>
    public async Task<bool> CancelDragAsync()
    {
        if (!Drag.Active) return false;
        down = null;
        pressPointer = null;
        scroller.Stop();
        await Drag.CancelAsync();
        return true;
    }

    // ---------------- the drag's previews ----------------

    void OnBegan(GestureReply r)
    {
        anchor = null;
        ghostFrom = null;
        ghostRound = false;
        scrubRow = null;
        Canvas.ShowDragWire(null, Vector2.Zero, 0, false);
        switch (r.Drag?.KindWord)
        {
            case DragWord.Move:
                Canvas.BeginMove(r.Drag.Faces ?? []);
                break;
            case DragWord.Wire:
                anchor = Canvas.AnchorPoint(r.Drag.From);
                if (r.Drag.Picked is { } to) Canvas.PickUp(Canvas.WireInto(to));
                break;
            case DragWord.Frame or DragWord.Resize:
                ghostFrom = Canvas.FrameRect(r.Hit?.Frame);
                break;
            case DragWord.Group:
                ghostFrom = Canvas.GroupRect(r.Hit?.Group);
                break;
            case DragWord.Bend:
                if (r.Hit is { } h) { ghostFrom = new MRect(h.At.X - 6, h.At.Y - 6, h.At.X + 6, h.At.Y + 6); ghostRound = true; }
                break;
            case DragWord.Scrub:
                if (r.Drag.Row is { } row && row.Get("face").Str() is { } face && row.Get("index") is { ValueKind: System.Text.Json.JsonValueKind.Number } k)
                {
                    scrubRow = Canvas.RowRect(face, k.GetInt32());
                    scrubWidget = Model.FaceOf(face)?.Inputs.ElementAtOrDefault(k.GetInt32())?.Widget;
                }
                break;
        }
        DragState = Words.Of(r.Drag?.KindWord ?? DragWord.Move);
    }

    /// <summary>What the drag under way is (the status file says, for scripts and screenshots).</summary>
    public string? DragState { get; private set; }

    /// <summary>A pointer hovering at view point `at` with nothing pressed: the typed hit test, shown.</summary>
    public void HoverAt(Vector2 at, PointerDeviceType type = PointerDeviceType.Mouse)
    {
        var h = Link.HitAt(Graph(at), Slop(type));
        Canvas.Hover(h);
        PinTip(h, at);
    }

    readonly ToolTip pinTip = new() { Placement = Microsoft.UI.Xaml.Controls.Primitives.PlacementMode.Bottom };
    DispatcherTimer? tipTimer;
    string? tipText;

    /// <summary>The tooltip of the pin hit at view point `at`: its row's hover, shown after a moment as a
    /// tooltip is; none off a pin.</summary>
    void PinTip(PlumHit? h, Vector2 at)
    {
        string? text = null;
        if (h is { Kind: 2 } p && Model.Faces[p.Node]?.Item is { } f)
            text = p.IsOutput ? f.Outputs.ElementAtOrDefault((int)p.Pin)?.Hover : f.Inputs.ElementAtOrDefault((int)p.Pin)?.Hover;
        if (string.IsNullOrWhiteSpace(text)) text = null;
        if (text == tipText) return;
        tipText = text;
        tipTimer?.Stop();
        pinTip.IsOpen = false;
        if (text is null) return;
        pinTip.Content = text;
        pinTip.PlacementRect = new Windows.Foundation.Rect(at.X - 8, at.Y - 8, 16, 16);
        if (tipTimer is null)
        {
            tipTimer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(500) };
            tipTimer.Tick += (_, _) => { tipTimer?.Stop(); if (tipText is not null) pinTip.IsOpen = true; };
        }
        tipTimer.Start();
    }

    DateTime touchesAsked = DateTime.MinValue;
    bool touchesBusy;

    /// <summary>The faces a marquee touches, as the core decides (`ui.gesture` drag move's `touches`, JSON),
    /// asked at most ten times a second while it moves; the typed moves draw the marquee itself.</summary>
    Point? touchesWanted;

    async void AskTouches(Point pointer)
    {
        touchesWanted = pointer;
        if (touchesBusy) return;
        touchesBusy = true;
        try
        {
            // at most ten a second, and always once more for where the pointer last was
            while (touchesWanted is not null)
            {
                var wait = 100 - (DateTime.UtcNow - touchesAsked).TotalMilliseconds;
                if (wait > 0) await Task.Delay((int)wait);
                if (touchesWanted is not { } at) break;
                touchesWanted = null;
                touchesAsked = DateTime.UtcNow;
                if (!Drag.Active) break;
                var r = await Link.GestureAsync(Gesture.DragMove(at, Slop(PointerDeviceType.Mouse)));
                if (Environment.GetEnvironmentVariable("PLUMMER_TRACE") is { Length: > 0 }) CoreHost.Log($"touches at {at}: {string.Join(",", r.Update?.Touches ?? [])} active {Drag.Active} kind {Drag.Kind}");
                if (Drag.Active && Drag.Kind == DragWord.Marquee) Canvas.ShowTouches(r.Update?.Touches);
            }
        }
        catch (Exception e) { CoreHost.Log($"marquee touches: {e.Message}"); }
        touchesBusy = false;
    }

    void OnDragMoved(DragFrame f)
    {
        switch (f.Kind)
        {
            case DragWord.Move:
                Canvas.MoveFaces((float)f.Dx, (float)f.Dy);
                break;
            case DragWord.Marquee:
                Canvas.ShowMarquee(f.Marquee);
                AskTouches(f.Pointer);
                break;
            case DragWord.Wire:
                var on = f.Target is { } t;
                var snap = f.Target is { } tt ? new Vector2(tt.X, tt.Y) : new Vector2((float)f.Pointer.X, (float)f.Pointer.Y);
                Canvas.ShowDragWire(anchor, snap, f.Target?.Fits ?? 0, on);
                Canvas.ShowDragTarget(f.Target);
                TargetFits = f.Target?.Fits;
                break;
            case DragWord.Frame or DragWord.Group or DragWord.Bend:
                if (ghostFrom is { } g) Canvas.ShowGhost(new MRect(g.X0 + f.Dx, g.Y0 + f.Dy, g.X1 + f.Dx, g.Y1 + f.Dy), ghostRound);
                break;
            case DragWord.Resize:
                if (ghostFrom is { } q) Canvas.ShowGhost(new MRect(q.X0, q.Y0, Math.Max(q.X0, q.X1 + f.Dx), Math.Max(q.Y0, q.Y1 + f.Dy)));
                break;
            case DragWord.Scrub:
                if (f.Scrub is { } v) ShowScrub(DragDriver.NumberText(v, scrubWidget));
                break;
        }
    }

    /// <summary>The wire's target's fit now (null: over no pin), for the status file.</summary>
    public uint? TargetFits { get; private set; }

    void OnEnded(DragWord? k, GestureReply? r)
    {
        var kept = DragDriver.Ran(r, "edit.move");
        Canvas.EndMove(kept);
        Canvas.ClearDrag(keepWire: Drag.HoldingWire);
        TargetFits = null;
        ShowScrub(null);
        DragState = Drag.HoldingWire ? "wire held" : null;
        if (scrubRow is { } row && r?.Commands.FirstOrDefault(c => c.Command == "row.set") is { } set)
            Note(set.Error ?? set.Result?.Get("note").Str(), row, set.Error is not null);
        scrubRow = null;
    }

    /// <summary>The palette a dropped wire opened has closed: the wire goes.</summary>
    public void PaletteClosed()
    {
        if (Drag.ReleaseWire()) Canvas.ShowDragWire(null, Vector2.Zero, 0, false);
        if (DragState == "wire held") DragState = null;
    }

    void ShowScrub(string? text)
    {
        if (text is null || scrubRow is not { } r)
        {
            scrubBox.Visibility = Visibility.Collapsed;
            return;
        }
        scrubText.Text = text;
        scrubText.Foreground = new SolidColorBrush(Microsoft.UI.Colors.White);
        scrubBox.Background = new SolidColorBrush(Canvas.Colors.Accent);
        var p = Canvas.View.ToView(new Vector2((float)r.X0, (float)r.Y0));
        Microsoft.UI.Xaml.Controls.Canvas.SetLeft(scrubBox, p.X);
        Microsoft.UI.Xaml.Controls.Canvas.SetTop(scrubBox, p.Y - 26);
        scrubBox.Visibility = Visibility.Visible;
    }

    /// <summary>The core's words about a value, by its row, for a moment.</summary>
    public void Note(string? text, MRect r, bool error)
    {
        if (string.IsNullOrEmpty(text)) return;
        var tb = new TextBlock { Text = text, FontSize = 12, TextWrapping = TextWrapping.Wrap, MaxWidth = 280, Foreground = new SolidColorBrush(Microsoft.UI.Colors.White) };
        var b = new Microsoft.UI.Xaml.Controls.Border
        {
            Child = tb, Padding = new Thickness(6, 3, 6, 3), CornerRadius = new CornerRadius(4), IsHitTestVisible = false,
            Background = new SolidColorBrush(error ? Windows.UI.Color.FromArgb(240, 196, 43, 28) : Windows.UI.Color.FromArgb(230, 60, 60, 60)),
        };
        var p = Canvas.View.ToView(new Vector2((float)r.X0, (float)r.Y1));
        Microsoft.UI.Xaml.Controls.Canvas.SetLeft(b, p.X);
        Microsoft.UI.Xaml.Controls.Canvas.SetTop(b, p.Y + 4);
        Overlay.Children.Add(b);
        var t = DispatcherQueue.CreateTimer();
        t.Interval = TimeSpan.FromSeconds(error ? 4 : 2.5);
        t.IsRepeating = false;
        t.Tick += (_, _) => Overlay.Children.Remove(b);
        t.Start();
        LastNote = text;
    }

    public string? LastNote { get; private set; }

    // ---------------- autoscroll ----------------

    Vector2 ScrollStep(Vector2 v)
    {
        var size = Canvas.ViewSize;
        if (size.X <= Edge * 3 || size.Y <= Edge * 3) return Vector2.Zero;
        static float Axis(float p, float len) =>
            p < Edge ? MaxScroll * MathF.Min((Edge - p) / Edge, 1) : p > len - Edge ? -MaxScroll * MathF.Min((p - (len - Edge)) / Edge, 1) : 0;
        return new Vector2(Axis(v.X, size.X), Axis(v.Y, size.Y));
    }

    void Autoscroll()
    {
        if (!Drag.Active || lastView is not { } p || Drag.Kind == DragWord.Scrub) { scroller.Stop(); return; }
        var st = ScrollStep(p);
        if (st == Vector2.Zero) { scroller.Stop(); return; }
        Canvas.PanBy(st);
        Drag.Move(Graph(p), Slop(downType));
    }

    // ---------------- taps ----------------

    /// <summary>A click or tap: `tap`, or `doubleTap` for the second of two within the system's double-click
    /// time and rectangle; a tap on the more-button replies with the node's menu, shown at the button.</summary>
    async Task Tap(Vector2 at, List<ModifierWord> mods, PointerDeviceType type)
    {
        var now = Environment.TickCount64;
        var (dw, dh) = Interop.DoubleClickSize;
        var scale = XamlRoot?.RasterizationScale ?? 1;
        var near = Math.Abs(at.X - lastClickAt.X) * scale <= Math.Max(dw, 4) / 2.0 + (type == PointerDeviceType.Touch ? 20 : 0)
            && Math.Abs(at.Y - lastClickAt.Y) * scale <= Math.Max(dh, 4) / 2.0 + (type == PointerDeviceType.Touch ? 20 : 0);
        var isDouble = lastClickTicks != 0 && now - lastClickTicks <= Interop.DoubleClickMs && near;
        lastClickTicks = isDouble ? 0 : now;
        lastClickAt = at;
        try
        {
            var g = Graph(at);
            var s = Slop(type);
            var r = isDouble ? await Link.DoubleTapAsync(g, mods, s) : await Link.TapAsync(g, mods, s);
            GestureAnswered?.Invoke(r);
            if (!isDouble && r.Context is { ValueKind: not System.Text.Json.JsonValueKind.Null } ctx)
                ContextMenu?.Invoke(new Windows.Foundation.Point(at.X, at.Y), await Model.ContextMenuAsync(ctx));
        }
        catch (Exception ex) { Failed?.Invoke(ex.Message); }
    }

    /// <summary>A click at view point `v`, as the mouse's (scripts: the more-button).</summary>
    public Task ClickAt(Vector2 v) => Tap(v, [], PointerDeviceType.Mouse);

    /// <summary>A double click at view point `v` (scripts): two clicks as the mouse's.</summary>
    public async Task DoubleClickAt(Vector2 v)
    {
        await Tap(v, [], PointerDeviceType.Mouse);
        await Tap(v, [], PointerDeviceType.Mouse);
    }

    // ---------------- context menus ----------------

    async void OnContextRequested(UIElement sender, ContextRequestedEventArgs e)
    {
        e.Handled = true;
        if (Drag.Active || held) return;
        Windows.Foundation.Point p = e.TryGetPosition(this, out var q) ? q : Pointer is { } pt ? new Windows.Foundation.Point(pt.X, pt.Y) : new Windows.Foundation.Point(ActualWidth / 2, ActualHeight / 2);
        await ContextAt(V(p));
    }

    /// <summary>The core's menu for what is at view point `v`: a `context` gesture, then menu.get {context}.</summary>
    public async Task ContextAt(Vector2 v, PointerDeviceType type = PointerDeviceType.Mouse)
    {
        try
        {
            var r = await Link.ContextAsync(Graph(v), Slop(type));
            GestureAnswered?.Invoke(r);
            if (r.Context is not { ValueKind: not System.Text.Json.JsonValueKind.Null } ctx) return;
            var items = await Model.ContextMenuAsync(ctx);
            if (items.Count > 0) ContextMenu?.Invoke(new Windows.Foundation.Point(v.X, v.Y), items);
        }
        catch (Exception ex) { Failed?.Invoke(ex.Message); }
    }

    // ---------------- keys ----------------

    /// <summary>A field over the canvas has the keyboard: the canvas's own keys are its.</summary>
    bool FieldHasKeys() => XamlRoot is { } xr && FocusManager.GetFocusedElement(xr) is TextBox or ComboBox or PasswordBox or RichEditBox or ListViewBase;

    async void OnKey(object sender, KeyRoutedEventArgs e)
    {
        if (FieldHasKeys()) return;
        var shift = (InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        var ctrl = (InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        switch (e.Key)
        {
            case VirtualKey.Left or VirtualKey.Right or VirtualKey.Up or VirtualKey.Down when !ctrl:
                e.Handled = true;
                if (!e.KeyStatus.WasKeyDown) keyPress++;
                var n = shift ? 10 : 1;
                var (dx, dy) = e.Key switch { VirtualKey.Left => (-n, 0), VirtualKey.Right => (n, 0), VirtualKey.Up => (0, -n), _ => (0, n) };
                if (Model.Selection.Nodes.Count == 0 && Model.Selection.Groups.Count == 0) return;
                try { await Link.NudgeAsync(dx, dy, keyPress); } catch (Exception ex) { Failed?.Invoke(ex.Message); }
                return;
            case VirtualKey.Escape:
                e.Handled = true;
                if (await CancelDragAsync()) return;
                EscapeUnhandled?.Invoke();
                return;
            case VirtualKey.Back or VirtualKey.Delete when !ctrl:
                // Edit › Delete: the model's shortcut is Backspace; Delete is the same command (12.3)
                e.Handled = true;
                BackspacePressed?.Invoke();
                return;
        }
    }

    /// <summary>Escape with no drag under way (the window: a run cancelled, a net's selection cleared).</summary>
    public event Action? EscapeUnhandled;
    /// <summary>Backspace: Delete's command (the window dispatches edit.delete).</summary>
    public event Action? BackspacePressed;

    // ---------------- the wheel ----------------

    void OnWheel(object sender, PointerRoutedEventArgs e)
    {
        var pt = e.GetCurrentPoint(this);
        var delta = pt.Properties.MouseWheelDelta;
        var at = V(pt.Position);
        var mods = e.KeyModifiers;
        if ((mods & VirtualKeyModifiers.Control) != 0)
            Canvas.ZoomBy(MathF.Pow(1.0015f, delta), at);
        else if (pt.Properties.IsHorizontalMouseWheel)
            Canvas.PanBy(new Vector2(-delta, 0));
        else if ((mods & VirtualKeyModifiers.Shift) != 0)
            Canvas.PanBy(new Vector2(delta, 0));
        else
            Canvas.PanBy(new Vector2(0, delta));
        e.Handled = true;
    }
}
