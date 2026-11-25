// Editing below the views (N5 slice 3): the Windows counterparts of the Swift kit's CanvasDrag (its
// platform-free part), PaletteModel and RowEditor, a name edited in place, where a field editor goes,
// and the feed's word that a document wants a window. Each follows the core's state and sends what the
// person did; none decides what a gesture or a key means (the core does, contract 8.13, 12.7, 12.10,
// 12.16). Everything here lives on the model's context (a window's UI thread; headless, a SerialContext).

using System.Text.Json;
using PlumCraft.Core;
using PlumCraft.Models;

namespace PlumCraft.Kit;

/// <summary>What a drag shows now, in graph units: its offset (a move, a frame, a group, a bend point, a
/// size), the marquee's rect, a new wire's target pin (it fits, through a Convert, or not), a scrub's value.</summary>
public readonly record struct DragFrame(DragWord Kind, double Dx, double Dy, Point Pointer, Rect? Marquee, PlumHit? Target, double? Scrub);

/// <summary>A drag on the canvas: the host recognises the press and its movement (a mouse, a pen, one
/// finger on a face) and feeds this driver graph points; `ui.gesture` drag begin decides what the drag is,
/// each move is the typed plum_gesture_move (no JSON, no request), and the end runs one undo step at most;
/// a cancel (Escape, a second finger, the capture lost) leaves the document as it was.</summary>
public sealed class DragDriver(CanvasLink link)
{
    public CanvasLink Link { get; } = link;
    public bool Active { get; private set; }
    /// <summary>What the core made of the drag (null until its begin is answered).</summary>
    public DragWord? Kind { get; private set; }
    public DragBegun? Begun { get; private set; }
    public GestureReply? BeginReply { get; private set; }
    /// <summary>A new wire dropped on empty canvas opened the palette: the host keeps drawing the wire until
    /// the palette closes (ReleaseWire).</summary>
    public bool HoldingWire { get; private set; }
    /// <summary>Where the pointer last was (graph units).</summary>
    public Point? Latest { get; private set; }
    /// <summary>The last frame shown.</summary>
    public DragFrame? Last { get; private set; }

    /// <summary>The core decided what the drag is (a host starts its preview).</summary>
    public event Action<GestureReply>? Began;
    public event Action<DragFrame>? Moved;
    /// <summary>The drag ended (the reply) or was cancelled (null), and what it was.</summary>
    public event Action<DragWord?, GestureReply?>? Ended;
    /// <summary>A new wire's target changed to a pin that takes it.</summary>
    public event Action? Snapped;
    public event Action<string>? Failed;

    Task? beginTask;
    double slop;
    (ulong, uint, uint)? lastTarget;

    /// <summary>A press began a drag at `at` (where it went down, not where the pointer is now).</summary>
    public void Begin(Point at, IReadOnlyList<ModifierWord> mods, double slop, double zoom)
    {
        HoldingWire = false;
        Active = true;
        Kind = null;
        Begun = null;
        BeginReply = null;
        Latest = null;
        Last = null;
        lastTarget = null;
        this.slop = slop;
        beginTask = BeginAsync(at, mods, slop, zoom);
    }

    async Task BeginAsync(Point at, IReadOnlyList<ModifierWord> mods, double s, double zoom)
    {
        try
        {
            var r = await Link.DragBeginAsync(at, mods, s, zoom);
            BeginReply = r;
            Begun = r.Drag;
            Kind = r.Drag?.KindWord;
            if (!Active) return; // let go meanwhile: the end carries on
            Began?.Invoke(r);
            if (Latest is { } p) Update(p, slop);
        }
        catch (Exception e)
        {
            Failed?.Invoke(e.Message);
        }
    }

    /// <summary>The pointer is at `at`: what the drag shows there (null before the core has decided).</summary>
    public DragFrame? Move(Point at, double slop)
    {
        if (!Active) return null;
        Latest = at;
        this.slop = slop;
        return Update(at, slop);
    }

    DragFrame? Update(Point at, double s)
    {
        if (Kind is not { } k) return null;
        var d = Link.DragMove(at, s);
        if (!d.Ok) return null;
        Rect? marquee = k == DragWord.Marquee ? new Rect(d.R0, d.R1, d.R2, d.R3) : null;
        PlumHit? target = null;
        if (k == DragWord.Wire)
        {
            var t = d.Target;
            if (t.Ok && t.KindOf == HitKind.Pin)
            {
                target = t;
                var key = (t.Node, t.Pin, t.Part);
                if (t.Fits != 0 && lastTarget != key) Snapped?.Invoke();
                lastTarget = key;
            }
            else lastTarget = null;
        }
        var f = new DragFrame(k, d.Dx, d.Dy, at, marquee, target, d.ScrubValue);
        Last = f;
        Moved?.Invoke(f);
        return f;
    }

    /// <summary>Let go at `at`: the core runs what the drag means. Its reply (null if it failed).</summary>
    public async Task<GestureReply?> EndAsync(Point at, double slop)
    {
        if (!Active) return null;
        Active = false;
        if (beginTask is { } b) await b;
        var k = Kind;
        GestureReply? r = null;
        try { r = await Link.DragEndAsync(at, slop); }
        catch (Exception e) { Failed?.Invoke(e.Message); }
        HoldingWire = k == DragWord.Wire && Ran(r, "canvas.palette");
        Kind = null;
        Begun = null;
        Ended?.Invoke(k, r);
        return r;
    }

    /// <summary>Escape, a second finger, the capture lost: nothing changes.</summary>
    public async Task CancelAsync()
    {
        if (!Active) return;
        Active = false;
        if (beginTask is { } b) await b;
        var k = Kind;
        try { await Link.DragCancelAsync(); }
        catch (Exception e) { Failed?.Invoke(e.Message); }
        Kind = null;
        Begun = null;
        Ended?.Invoke(k, null);
    }

    /// <summary>The palette a dropped wire opened has closed: stop drawing the wire. Whether one was held.</summary>
    public bool ReleaseWire()
    {
        if (!HoldingWire) return false;
        HoldingWire = false;
        return true;
    }

    /// <summary>Whether the reply ran `command` and it worked.</summary>
    public static bool Ran(GestureReply? r, string command) => r?.Commands.Any(c => c.Ok && c.Command == command) ?? false;

    /// <summary>A number as its row shows it while scrubbing: whole for an INT, else the widget's decimals.</summary>
    public static string NumberText(double v, Widget? w)
    {
        if (w?.Int == true) return ((long)Math.Round(v)).ToString(System.Globalization.CultureInfo.InvariantCulture);
        var places = Math.Clamp(w?.Decimals ?? 3, 0, 8);
        return v.ToString("F" + places, System.Globalization.CultureInfo.InvariantCulture);
    }
}

/// <summary>The node palette below its views (contract 12.7): the core keeps whether it is open, where and
/// for which dropped wire (the scene's view.palette); this model follows it, asks what it offers for the
/// query (palette.items), keeps the highlighted row for the keyboard, and sends a pick (palette.choose, one
/// undo step) or a dismissal (canvas.palette {open: false}). A kind whose engine isn't started is chosen
/// as any other: the core holds the add while the engine starts, and the palette shows it pending.</summary>
public sealed class PaletteModel
{
    public DocumentModel Model { get; }
    public Palette? Palette { get; private set; }
    public List<PaletteItem> Items { get; private set; } = [];
    public ulong Total { get; private set; }
    public string Query { get; private set; } = "";
    public int Selected { get; private set; }
    public PaletteItem? Pending { get; private set; }
    public string? Error { get; private set; }
    public event Action<Palette>? Opened;
    public event Action? Closed;
    public event Action? Updated;
    /// <summary>Where a palette opened without a place adds (the view's centre, graph units).</summary>
    public Func<Point>? FallbackAt { get; set; }
    int asked;
    bool shownOpen;

    public PaletteModel(DocumentModel model)
    {
        Model = model;
        model.ViewChanged += Follow;
        Follow(model.View);
    }

    public bool IsOpen => shownOpen;
    public bool ForWire => Palette?.Wire is not null;

    /// <summary>The scene's view changed: open, close or retarget as the core says.</summary>
    public void Follow(View view)
    {
        if (view.Palette is { } p)
        {
            var reopened = !shownOpen || p.At != Palette?.At || !Equals(p.Wire, Palette?.Wire);
            Palette = p;
            if (!reopened) return;
            shownOpen = true;
            Query = p.Query ?? "";
            Selected = 0;
            Pending = null;
            Error = null;
            Items = [];
            Opened?.Invoke(p);
            Refresh();
        }
        else if (shownOpen)
        {
            shownOpen = false;
            Palette = null;
            Pending = null;
            Items = [];
            Closed?.Invoke();
        }
    }

    /// <summary>The items by category, in the core's order.</summary>
    public List<(string Title, List<PaletteItem> Items)> Sections()
    {
        var o = new List<(string, List<PaletteItem>)>();
        foreach (var it in Items)
        {
            if (o.Count > 0 && o[^1].Item1 == it.Category) o[^1].Item2.Add(it);
            else o.Add((it.Category, [it]));
        }
        return o;
    }

    /// <summary>Open the palette at `at` (graph units), as Node › Search…, Tab and a double click on empty canvas do.</summary>
    public Task OpenAsync(Point at) =>
        Model.RunAsync("canvas.palette", Jsonv.Object(("open", Jsonv.True), ("at", Jsonv.Array([Jsonv.Of(at.X), Jsonv.Of(at.Y)]))));

    /// <summary>The query changed: ask what matches (a reply to an older query is dropped).</summary>
    public void Search(string q)
    {
        if (q == Query) return;
        Query = q;
        Selected = 0;
        Refresh();
    }

    void Refresh() => refreshing = RefreshAsync();

    /// <summary>The latest palette.items under way: Return waits for it, so a pick typed fast is from what
    /// was typed, not the list before it.</summary>
    Task refreshing = Task.CompletedTask;

    async Task RefreshAsync()
    {
        var n = ++asked;
        try
        {
            var r = await Model.Core.RequestAsync<PaletteItems>("palette.items", Jsonv.Object(("query", Jsonv.Of(Query))), Model.RequestDoc);
            if (n != asked || !shownOpen) return;
            Items = r.Items;
            Total = r.Total;
            Selected = Math.Min(Selected, Math.Max(r.Items.Count - 1, 0));
            Error = null;
        }
        catch (Exception e)
        {
            if (n != asked) return;
            Error = e.Message;
        }
        Updated?.Invoke();
        Model.Poke();
    }

    /// <summary>Move the highlight by `by` rows (held at both ends).</summary>
    public void MoveBy(int by)
    {
        if (Items.Count == 0) return;
        Selected = Math.Clamp(Selected + by, 0, Items.Count - 1);
        Updated?.Invoke();
    }

    public void Highlight(int index)
    {
        if (index >= 0 && index < Items.Count) Selected = index;
    }

    /// <summary>Return: the highlighted row.</summary>
    public async Task ChooseSelectedAsync()
    {
        await refreshing;
        if (Selected >= 0 && Selected < Items.Count) await ChooseAsync(Items[Selected]);
    }

    /// <summary>Add `item` where the palette opened (and connect the dropped wire): one undo step. The palette
    /// closes when the scene says so; while the add waits for an engine, `Pending` says so.</summary>
    public async Task ChooseAsync(PaletteItem item)
    {
        if (Pending is not null) return;
        Pending = item;
        Error = null;
        Updated?.Invoke();
        var f = new List<(string, JsonElement)> { ("kind", Jsonv.Of(item.Kind)) };
        if (item.Port is { } port) f.Add(("port", Jsonv.Of(port)));
        if (Palette?.At is null && FallbackAt?.Invoke() is { } at) f.Add(("at", Jsonv.Array([Jsonv.Of(at.X), Jsonv.Of(at.Y)])));
        try
        {
            await Model.RunAsync("palette.choose", Jsonv.Object([.. f]));
            Pending = null;
            if (Model.View.Palette is null) Follow(Model.View);
        }
        catch (Exception e)
        {
            Pending = null;
            Error = e.Message;
            Updated?.Invoke();
        }
        Model.Poke();
    }

    /// <summary>Escape, a click away: the core closes the palette, leaving nothing.</summary>
    public void Dismiss()
    {
        if (!shownOpen || Pending is not null) return;
        shownOpen = false;
        Palette = null;
        Items = [];
        Closed?.Invoke();
        _ = DismissAsync();
    }

    async Task DismissAsync()
    {
        try { await Model.RunAsync("canvas.palette", Jsonv.Object(("open", Jsonv.False))); }
        catch (Exception e) { Error = e.Message; }
    }

    public string? PendingText => Pending is not { } p ? null
        : p.Starts is { } e ? $"Starting the {e} engine… {p.Label} appears when it is ready." : $"Adding {p.Label}…";

    public string Headline => ForWire ? (Total == 1 ? "1 kind takes this wire" : $"{Total} kinds take this wire") : (Total == 1 ? "1 kind" : $"{Total} kinds");
}

/// <summary>Editing a value on a node's face (contract 12.10): the core keeps which row a field editor is
/// open on (row.edit, the scene's view.edit) and checks what the field sends (row.set: parsed, made whole,
/// held to its range, a choice its own), one undo step. This follows that state and sends a commit
/// (Enter, focus lost, a pick), a cancel (Escape) or a move (Tab, Shift+Tab). The host puts one native
/// control over the row while it is open, never a control per row.</summary>
public sealed class RowEditor
{
    public sealed record OpenRow(RowEdit Edit, InRow Row, Widget Widget)
    {
        public string Label => Row.Label.Length == 0 ? Edit.Port : Row.Label;
        public WidgetWord? Kind => Widget.KindWord;
    }

    public enum Then { Close, Next, Previous, Stay }

    public DocumentModel Model { get; }
    public OpenRow? Open { get; private set; }
    /// <summary>Why the last value was refused (the editor stays open).</summary>
    public string? Error { get; private set; }
    public event Action<OpenRow>? Opened;
    public event Action? Closed;
    /// <summary>The core said something of a value: a note (held to its range), or a refusal (true).</summary>
    public event Action<OpenRow, string, bool>? Noted;

    public RowEditor(DocumentModel model)
    {
        Model = model;
        model.ViewChanged += Follow;
        model.SceneChanged += _ => { if (Open is null && model.View.Edit is not null) Follow(model.View); };
        Follow(model.View);
    }

    public void Follow(View view)
    {
        if (view.Edit is not { } e)
        {
            if (Open is not null)
            {
                Open = null;
                Error = null;
                Closed?.Invoke();
            }
            return;
        }
        if (e == Open?.Edit) return;
        if (Model.Faces.ById(e.Face) is not { } face || e.Row < 0 || e.Row >= face.Inputs.Count || face.Inputs[e.Row].Widget is not { } w) return;
        var o = new OpenRow(e, face.Inputs[e.Row], w);
        if (Open is not null) Closed?.Invoke();
        Open = o;
        Error = null;
        Opened?.Invoke(o);
    }

    /// <summary>Open the editor on a row (a script, a test; a click goes through ui.gesture).</summary>
    public Task EditAsync(string node, string port) => Model.RunAsync("row.edit", Jsonv.Object(("node", Jsonv.Of(node)), ("port", Jsonv.Of(port))));

    /// <summary>Commit what the field holds, then close it or go on. False when the core refused it.</summary>
    public Task<bool> CommitTextAsync(string text, Then then = Then.Close) => SetAsync(("text", Jsonv.Of(text)), then);

    /// <summary>The field lost focus holding `text` (another control, the canvas, a window coming and going):
    /// it commits only what the person typed. A field left as it opened sets nothing and stays open, so a
    /// value set while it was open (a script's `row.set`, another window) is not put back (B129, as the Apple
    /// shells' `FieldEditorHost.lostFocus`). False when nothing was committed or the core refused it.</summary>
    public Task<bool> FocusLostAsync(string text) =>
        Open is { } o && text != StartText(o.Widget) ? CommitTextAsync(text, Then.Close) : Task.FromResult(false);

    /// <summary>Commit a value (a choice's, a colour's).</summary>
    public Task<bool> CommitValueAsync(JsonElement value, Then then = Then.Close) => SetAsync(("value", value), then);

    async Task<bool> SetAsync((string, JsonElement) what, Then then)
    {
        if (Open is not { } o) return false;
        try
        {
            var r = await Model.RunAsync("row.set", Jsonv.Object(what, ("node", Jsonv.Of(o.Edit.Node)), ("port", Jsonv.Of(o.Edit.Port))));
            Error = null;
            if (r.Get("note").Str() is { } note) Noted?.Invoke(o, note, false);
        }
        catch (Exception e)
        {
            Error = e.Message;
            Noted?.Invoke(o, e.Message, true);
            Model.Poke();
            return false;
        }
        switch (then)
        {
            case Then.Close: await CloseAsync(); break;
            case Then.Next: await StepAsync(1); break;
            case Then.Previous: await StepAsync(-1); break;
        }
        return true;
    }

    /// <summary>Escape: the editor goes, nothing is set.</summary>
    public async Task CloseAsync()
    {
        try { await Model.RunAsync("row.edit", Jsonv.Object(("open", Jsonv.False))); }
        catch (Exception) { /* closed already */ }
    }

    /// <summary>Tab (1) or Shift+Tab (-1): the next or previous row that can be edited.</summary>
    public async Task StepAsync(int by)
    {
        try { await Model.RunAsync("row.edit", Jsonv.Object(("step", Jsonv.Of((long)by)))); }
        catch (Exception) { /* none to go to */ }
    }

    /// <summary>The text the field starts with: a number as the core writes it (its decimals, whole for an
    /// INT), else the value as text.</summary>
    public static string StartText(Widget w)
    {
        if (w.Text is { } t) return t;
        return w.Value.ValueKind switch
        {
            JsonValueKind.String => w.Value.GetString() ?? "",
            JsonValueKind.Number => w.Value.GetRawText(),
            JsonValueKind.Array => string.Join(", ", w.Value.EnumerateArray().Select(x => x.ValueKind == JsonValueKind.String ? x.GetString() : x.GetRawText())),
            JsonValueKind.Undefined or JsonValueKind.Null => "",
            _ => w.Value.GetRawText(),
        };
    }
}

/// <summary>A name edited in place (contract 12.16): the core keeps it (name.edit, the scene's view.name with
/// where the field goes); name.set {text} commits through the command that owns the name, one undo step;
/// name.edit {open: false} is Escape; a refusal leaves it open with the core's words.</summary>
public sealed class NameEditor
{
    public DocumentModel Model { get; }
    public NameEdit? Open { get; private set; }
    public string? Error { get; private set; }
    public event Action<NameEdit>? Opened;
    public event Action? Closed;
    public event Action<string>? Refused;

    public NameEditor(DocumentModel model)
    {
        Model = model;
        model.NameChanged += Follow;
        Follow(model.View.Name);
    }

    public void Follow(NameEdit? n)
    {
        if (n is null)
        {
            if (Open is null) return;
            Open = null;
            Error = null;
            Closed?.Invoke();
            return;
        }
        if (Open is not null && Shell.Same(n, Open)) return;
        Open = n;
        Error = null;
        Opened?.Invoke(n);
    }

    public async Task<bool> CommitAsync(string text)
    {
        if (Open is null) return false;
        try
        {
            await Model.RunAsync("name.set", Jsonv.Object(("text", Jsonv.Of(text))));
            Error = null;
            return true;
        }
        catch (Exception e)
        {
            Error = e.Message;
            Refused?.Invoke(e.Message);
            return false;
        }
    }

    public async Task CancelAsync()
    {
        try { await Model.RunAsync("name.edit", Jsonv.Object(("open", Jsonv.False))); }
        catch (Exception) { /* closed already */ }
    }
}

/// <summary>Where a field editor's control goes (contract 12.10, the Apple kit's EditorPlacement): over the
/// row at the canvas's zoom, its text the face's (11 graph units times the zoom); zoomed so far out that the
/// text would be under `legible` points, at that size instead, centred on the row and floating over the
/// face, at least `minW` by `minH`, so editing never moves the view. Moved in from the view's edges.</summary>
public static class EditorPlacement
{
    public readonly record struct Placed(double X, double Y, double W, double H, double FontSize, bool Floating);

    public const double FaceText = 11;

    public static Placed Place(Rect row, double zoom, Point pan, Point view, double legible = 10, double minW = 170, double minH = 22)
    {
        double x = row.X0 * zoom + pan.X, y = row.Y0 * zoom + pan.Y, w = row.Width * zoom, h = row.Height * zoom;
        var font = FaceText * zoom;
        var floating = font < legible;
        if (floating)
        {
            font = legible;
            double cx = x + w / 2, cy = y + h / 2;
            w = Math.Max(minW, w);
            h = Math.Max(minH, h);
            x = cx - w / 2;
            y = cy - h / 2;
        }
        else
        {
            // a field a little taller than the widget drawn, so its text fits
            h = Math.Max(h, font + 10);
            w = Math.Max(w, 48);
        }
        if (view.X > 0) x = Math.Clamp(x, 0, Math.Max(0, view.X - w));
        if (view.Y > 0) y = Math.Clamp(y, 0, Math.Max(0, view.Y - h));
        return new Placed(x, y, w, h, font, floating);
    }
}

/// <summary>A document the core opened for New or Open that wants a window (`changed` domain `window`,
/// `platform.document_windows`, N5 slice 3).</summary>
public sealed record WindowWanted(ulong Doc, ulong? From, bool ForPerson, string? Path)
{
    public static List<WindowWanted> Of(FeedBatch b) =>
        [.. b.Changed.Where(c => c.DomainWord == DomainWord.Window && c.Open == true).Select(c => new WindowWanted(c.Doc, c.From, c.By == "person", c.Path))];
}
