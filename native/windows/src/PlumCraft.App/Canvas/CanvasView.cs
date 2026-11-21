// The canvas's XAML host: it recognises input and hands it to the core as `ui.gesture` (the core decides
// what a gesture means, contract 8.13), and keeps the view's pan and zoom, which are the shell's (5.3,
// 12.13). Read and run (N5 slice 2): a click or tap selects (Shift adds, Ctrl takes away), a double
// click opens what it is on, a right click, Shift+F10 or a touch hold is the core's context menu, a drag
// on empty canvas with the mouse or a pen is the core's marquee. Dragging a face, a pin or a wire is
// editing (slice 3) and does nothing yet.
//
// The view: the mouse wheel pans (Shift or a horizontal wheel sideways), Ctrl+wheel zooms about the
// pointer, the middle button drags the view; one finger on empty canvas pans and two pinch (the spike
// had a touch drag on empty canvas as a marquee: a touch shell pans there, 5.5); a precision touchpad
// comes as wheel events and pans and zooms the same way.

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

namespace PlumCraft.App.Canvas;

public sealed class CanvasView : Grid
{
    public CompositionCanvas Canvas { get; }
    public DocumentModel Model { get; }
    CanvasLink Link => Canvas.Link;

    /// <summary>A gesture was answered: what it ran (the window says what isn't built yet).</summary>
    public event Action<GestureReply>? GestureAnswered;
    /// <summary>The core gave a menu for what is at a view point (a right click, the more-button).</summary>
    public event Action<Windows.Foundation.Point, List<MenuItem>>? ContextMenu;
    public event Action<string>? Failed;

    // a mouse or pen press
    Vector2? down;
    List<ModifierWord> downMods = [];
    bool downOnEmpty, dragging, marqueeing, middlePanning;
    Vector2 last;
    uint? pressPointer;
    // the last click, for a double click
    Vector2 lastClickAt;
    long lastClickTicks;
    // touches
    readonly Dictionary<uint, Vector2> touches = [];
    Vector2? touchStart;
    bool touchMoved, touchPans;
    float pinchDist;
    Vector2 pinchMid;
    Viewport pinchView;

    const float ClickSlop = 4;

    public CanvasView(DocumentModel model)
    {
        Model = model;
        Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        Canvas = new CompositionCanvas(this, model);
        SizeChanged += (_, e) => Canvas.SetViewSize(new Vector2((float)e.NewSize.Width, (float)e.NewSize.Height));
        Loaded += (_, _) =>
        {
            if (XamlRoot is { } xr)
            {
                Canvas.DisplayScale = (float)xr.RasterizationScale;
                xr.Changed += (s, _) => Canvas.DisplayScale = (float)s.RasterizationScale;
            }
        };
        ManipulationMode = ManipulationModes.None;
        PointerPressed += OnPressed;
        PointerMoved += OnMoved;
        PointerReleased += OnReleased;
        PointerCanceled += OnCanceled;
        PointerCaptureLost += OnCanceled;
        PointerWheelChanged += OnWheel;
        ContextRequested += OnContextRequested;
    }

    float Slop(PointerDeviceType t) => (float)CanvasLink.Slop(t == PointerDeviceType.Touch ? 22 : 10, Canvas.View.Zoom);

    Point Graph(Vector2 v)
    {
        var g = Canvas.View.ToGraph(v);
        return new Point(g.X, g.Y);
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

    bool OnEmpty(Vector2 v, PointerDeviceType t)
    {
        var h = Link.HitAt(Graph(v), Slop(t));
        return !h.Ok || h.KindOf is null or HitKind.None or HitKind.Wire or HitKind.Tag;
    }

    // ---------------- presses ----------------

    /// <summary>The last pointer events seen (the status file says, for the input probe).</summary>
    public readonly List<string> Seen = [];

    void Note(string what, PointerRoutedEventArgs e)
    {
        var p = e.GetCurrentPoint(this);
        Seen.Add($"{what} {e.Pointer.PointerDeviceType} {(int)p.Position.X},{(int)p.Position.Y}{(p.Properties.IsLeftButtonPressed ? " L" : "")}{(p.Properties.IsRightButtonPressed ? " R" : "")}");
        if (Seen.Count > 8) Seen.RemoveAt(0);
    }

    void OnPressed(object sender, PointerRoutedEventArgs e)
    {
        Note("pressed", e);
        var pt = e.GetCurrentPoint(this);
        var at = V(pt.Position);
        var type = e.Pointer.PointerDeviceType;
        if (type == PointerDeviceType.Touch)
        {
            CapturePointer(e.Pointer);
            touches[e.Pointer.PointerId] = at;
            if (touches.Count == 1)
            {
                touchStart = at;
                touchMoved = false;
                touchPans = OnEmpty(at, type);
                last = at;
            }
            else if (touches.Count == 2)
            {
                // a pinch: no tap now, and the view follows both fingers
                touchMoved = true;
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
        if (!props.IsLeftButtonPressed) return; // the right button: ContextRequested
        CapturePointer(e.Pointer);
        pressPointer = e.Pointer.PointerId;
        down = at;
        last = at;
        downMods = Mods(e.KeyModifiers);
        downOnEmpty = OnEmpty(at, type);
        dragging = false;
        marqueeing = false;
        e.Handled = true;
    }

    async void OnMoved(object sender, PointerRoutedEventArgs e)
    {
        var at = V(e.GetCurrentPoint(this).Position);
        var type = e.Pointer.PointerDeviceType;
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
            else if (touchStart is { } s)
            {
                if (!touchMoved && Vector2.Distance(at, s) > 6) touchMoved = true;
                if (touchMoved && touchPans) Canvas.PanBy(at - last);
                last = at;
            }
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
        if (down is not { } d || e.Pointer.PointerId != pressPointer) return;
        if (!dragging)
        {
            if (Vector2.Distance(at, d) <= ClickSlop) return;
            dragging = true;
            // only a drag on empty canvas does anything in a read-only app: the core's marquee
            if (!downOnEmpty) return;
            try
            {
                var r = await Link.DragBeginAsync(Graph(d), downMods, Slop(type), Canvas.View.Zoom);
                if (r.Drag?.KindWord == DragWord.Marquee) marqueeing = true;
                else if (r.Drag is not null) await Link.DragCancelAsync();
            }
            catch (Exception ex) { Failed?.Invoke(ex.Message); }
            if (!dragging) return; // released meanwhile
        }
        if (marqueeing)
        {
            var m = Link.DragMove(Graph(at), Slop(type));
            if (m.Ok && m.KindOf == DragKind.Marquee) Canvas.ShowMarquee(new Rect(m.R0, m.R1, m.R2, m.R3));
        }
        e.Handled = true;
    }

    async void OnReleased(object sender, PointerRoutedEventArgs e)
    {
        Note("released", e);
        var at = V(e.GetCurrentPoint(this).Position);
        var type = e.Pointer.PointerDeviceType;
        ReleasePointerCapture(e.Pointer);
        if (type == PointerDeviceType.Touch)
        {
            if (!touches.Remove(e.Pointer.PointerId)) return;
            if (touches.Count == 1)
            {
                // the pinch ended: the other finger goes on panning from where it is
                last = touches.Values.First();
                touchPans = true;
                return;
            }
            if (touches.Count == 0)
            {
                var tap = !touchMoved && touchStart is not null;
                touchStart = null;
                if (tap) await Tap(at, [], type);
            }
            e.Handled = true;
            return;
        }
        if (middlePanning)
        {
            middlePanning = false;
            return;
        }
        if (down is null || e.Pointer.PointerId != pressPointer) return;
        var wasDragging = dragging;
        var wasMarquee = marqueeing;
        var mods = downMods;
        down = null;
        dragging = false;
        marqueeing = false;
        pressPointer = null;
        if (wasDragging)
        {
            if (wasMarquee)
            {
                try { GestureAnswered?.Invoke(await Link.DragEndAsync(Graph(at), Slop(type))); }
                catch (Exception ex) { Failed?.Invoke(ex.Message); }
                Canvas.ShowMarquee(null);
            }
            return;
        }
        await Tap(at, mods, type);
    }

    void OnCanceled(object sender, PointerRoutedEventArgs e)
    {
        if (e.Pointer.PointerDeviceType == PointerDeviceType.Touch)
        {
            touches.Remove(e.Pointer.PointerId);
            if (touches.Count == 0) touchStart = null;
            return;
        }
        if (middlePanning) { middlePanning = false; return; }
        if (e.Pointer.PointerId != pressPointer) return;
        var was = marqueeing;
        down = null;
        dragging = false;
        marqueeing = false;
        pressPointer = null;
        if (was)
        {
            Canvas.ShowMarquee(null);
            _ = Link.DragCancelAsync();
        }
    }

    /// <summary>A click or tap: `tap`, or `doubleTap` for the second of two close together; a tap on the
    /// more-button replies with the node's menu, shown at the button.</summary>
    async Task Tap(Vector2 at, List<ModifierWord> mods, PointerDeviceType type)
    {
        var now = Environment.TickCount64;
        var isDouble = now - lastClickTicks < 500 && Vector2.Distance(at, lastClickAt) <= ClickSlop * 2;
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

    // ---------------- context menus ----------------

    async void OnContextRequested(UIElement sender, ContextRequestedEventArgs e)
    {
        e.Handled = true;
        Windows.Foundation.Point p = e.TryGetPosition(this, out var q) ? q : new Windows.Foundation.Point(ActualWidth / 2, ActualHeight / 2);
        await ContextAt(V(p));
    }

    /// <summary>The core's menu for what is at view point `v`: a `context` gesture, then menu.get {context}.</summary>
    public async Task ContextAt(Vector2 v)
    {
        try
        {
            var r = await Link.ContextAsync(Graph(v), Slop(PointerDeviceType.Mouse));
            if (r.Context is not { ValueKind: not System.Text.Json.JsonValueKind.Null } ctx) return;
            var items = await Model.ContextMenuAsync(ctx);
            if (items.Count > 0) ContextMenu?.Invoke(new Windows.Foundation.Point(v.X, v.Y), items);
        }
        catch (Exception ex) { Failed?.Invoke(ex.Message); }
    }

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
