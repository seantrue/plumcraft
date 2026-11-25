// The document's windows below the views (N5 slice 4; the Swift kit's Windows.swift, SettingsModel.swift,
// and PlumCraftShell's ReportModel, CompareModel and MessagesModel): the core keeps which windows are open
// (the scene's `view.windows`: the parameters inspector, the pop-outs, Settings, Messages, the Cache
// Report, the graph's Parameters window, value editors and Compare) and what each shows (`parameters.get`,
// `popout.get` and `viewer.data`, `settings.get`, `messages.get`, `report.get`, `compare.get`). These models
// follow that state and send what the person did as the core's commands; a platform draws them natively.
// Nothing here keeps state of its own that a script couldn't set: a field's draft text while a person
// types, and which page a request is waiting for, are all.
//
// Each lives on its DocumentModel's SynchronizationContext (a window's UI thread; headless, a
// SerialContext): call it there, and its events come there.

using System.Text.Json;
using PlumCraft.Models;

namespace PlumCraft.Kit;

/// <summary>The parameters inspector over the core (`node.parameters`, `parameters.get`, `row.set`,
/// `node.face`): every input of one node, shown on its face or not.</summary>
public sealed class ParametersModel(DocumentModel model)
{
    public DocumentModel Model { get; } = model;
    /// <summary>The node it is open on (the core's `view.windows.parameters`).</summary>
    public string? Node { get; private set; }
    public ParametersReply? Reply { get; private set; }
    public string? Error { get; private set; }
    /// <summary>The node opened or changed (null: closed), its rows read again.</summary>
    public event Action<ParametersReply?>? Changed;
    /// <summary>What the core said of a value set, by port: a note, or a refusal (`error` true).</summary>
    public event Action<string, string, bool>? Noted;
    bool busy, again;

    /// <summary>The windows changed: open, move to another node, or close.</summary>
    public void Follow(DocumentWindows w)
    {
        if (w.Parameters != Node)
        {
            Node = w.Parameters;
            Reply = null;
            if (Node is null)
            {
                Changed?.Invoke(null);
                return;
            }
        }
        Refresh();
    }

    /// <summary>The scene changed: read the rows again (a value set elsewhere, a wire made, an undo);
    /// shown again only when they differ.</summary>
    public void SceneChanged()
    {
        if (Node is not null) Refresh();
    }

    public async void Refresh()
    {
        if (Node is not { } n) return;
        if (busy) { again = true; return; }
        busy = true;
        try
        {
            var r = await Model.ParametersAsync(n);
            if (Node == n && (Reply is null || !Shell.Same(r, Reply)))
            {
                Reply = r;
                Error = null;
                Changed?.Invoke(r);
            }
        }
        catch (Exception e)
        {
            Error = e.Message;
        }
        busy = false;
        Model.Poke();
        if (again) { again = false; Refresh(); }
    }

    /// <summary>Set a row: typed text (the core parses it as the face's editor does), or a value (a switch's,
    /// a choice's, a colour, a slider's).</summary>
    public Task<bool> SetAsync(string port, string? text = null, JsonElement? value = null)
    {
        if (Node is not { } n) return Task.FromResult(false);
        var f = new List<(string, JsonElement)> { ("node", Jsonv.Of(n)), ("port", Jsonv.Of(port)) };
        if (text is not null) f.Add(("text", Jsonv.Of(text)));
        if (value is { } v) f.Add(("value", v));
        return SendAsync("row.set", Jsonv.Object([.. f]), port);
    }

    /// <summary>The default back (↺).</summary>
    public Task<bool> ResetAsync(string port) =>
        Node is { } n ? SendAsync("row.set", Jsonv.Object(("node", Jsonv.Of(n)), ("port", Jsonv.Of(port)), ("reset", Jsonv.True)), port) : Task.FromResult(false);

    /// <summary>Show the input on the face, or hide it there (Show on Face).</summary>
    public Task<bool> ShowOnFaceAsync(string port, bool shown) =>
        Node is { } n ? SendAsync("node.face", Jsonv.Object(("node", Jsonv.Of(n)), ("port", Jsonv.Of(port)), ("shown", Jsonv.Of(shown))), port) : Task.FromResult(false);

    async Task<bool> SendAsync(string command, JsonElement p, string port)
    {
        try
        {
            var r = await Model.RunAsync(command, p);
            if (r.ValueKind == JsonValueKind.Object && ((JsonElement?)r).Get("note").Str() is { } note) Noted?.Invoke(port, note, false);
            Refresh();
            return true;
        }
        catch (Exception e)
        {
            Noted?.Invoke(port, e.Message, true);
            return false;
        }
    }

    /// <summary>Close it (`node.parameters {}`): the person closed the pane.</summary>
    public async Task CloseAsync()
    {
        try { await Model.RunAsync("node.parameters", Jsonv.EmptyObject); } catch (Exception) { }
    }

    /// <summary>Open it on a node (a menu, a script; a double tap goes through `ui.gesture`).</summary>
    public async Task OpenAsync(string node)
    {
        try { await Model.RunAsync("node.parameters", Jsonv.Object(("node", Jsonv.Of(node)))); } catch (Exception) { }
    }
}

/// <summary>One pop-out's value and its viewer's controls: `popout.get`, the viewers' commands
/// (`viewer.zoom`, `viewer.text`, `viewer.table`, `viewer.tree`, `viewer.audio`, `viewer.picture`,
/// `viewer.grid`, `viewer.hex`, `viewer.probe`, `viewer.pin`, `view.viewer`, `run.item`, `viewer.sheet`),
/// the pages the core reads (`viewer.data`), the ways a value goes out (`value.copy`, `value.saveAs`,
/// `value.share`, `value.export`) and `view.close`.</summary>
public sealed class PopoutModel(DocumentModel model, PopoutWindow window)
{
    public DocumentModel Model { get; } = model;
    public ulong Id { get; } = window.Id;
    public PopoutWindow Window { get; private set; } = window;
    public PopoutValue? Value { get; private set; }
    public event Action<PopoutValue>? Changed;
    bool busy, again;

    /// <summary>Its entry in the core's windows changed: read it again when its stamp did.</summary>
    public void Update(PopoutWindow w)
    {
        var changed = w.Stamp != Window.Stamp || Value is null;
        Window = w;
        if (changed) Refresh();
    }

    public async void Refresh()
    {
        if (busy) { again = true; return; }
        busy = true;
        try
        {
            var v = await Model.PopoutAsync(Id);
            Value = v;
            Changed?.Invoke(v);
        }
        catch (Exception) { /* closed meanwhile: the windows say so */ }
        busy = false;
        Model.Poke();
        if (again) { again = false; Refresh(); }
    }

    /// <summary>A viewer command on this pop-out with its params and the pop-out's id; read again after.
    /// The reply, or null when the core refused it.</summary>
    public async Task<JsonElement?> CommandAsync(string command, params (string, JsonElement)[] p)
    {
        JsonElement? r = null;
        try { r = await Model.RunAsync(command, Jsonv.Object([("popout", Jsonv.Of(Id)), .. p])); }
        catch (Exception e) { LastRefusal = e.Message; }
        Refresh();
        return r;
    }

    /// <summary>Why the last command was refused (the viewer says so).</summary>
    public string? LastRefusal { get; private set; }

    /// <summary>Fit, 1:1, or detail (`viewer.zoom`).</summary>
    public Task<JsonElement?> ZoomAsync(string to) => CommandAsync("viewer.zoom", ("to", Jsonv.Of(to)));

    /// <summary>Wrap, a monospaced face, the whole text (`viewer.text`).</summary>
    public Task<JsonElement?> TextAsync(bool? wrap = null, bool? mono = null, bool? all = null)
    {
        var f = new List<(string, JsonElement)>();
        if (wrap is { } w) f.Add(("wrap", Jsonv.Of(w)));
        if (mono is { } m) f.Add(("mono", Jsonv.Of(m)));
        if (all is { } a) f.Add(("all", Jsonv.Of(a)));
        return CommandAsync("viewer.text", [.. f]);
    }

    /// <summary>Show the value in another of the viewers offered (`view.viewer`; null: its default).</summary>
    public Task<JsonElement?> ChooseViewerAsync(string? viewer) => CommandAsync("view.viewer", ("viewer", viewer is null ? Jsonv.Null : Jsonv.Of(viewer)));

    /// <summary>Pin it as A or B for Compare (`viewer.pin {slot, popout}`).</summary>
    public Task<JsonElement?> PinAsync(string slot) => CommandAsync("viewer.pin", ("slot", Jsonv.Of(slot)));

    /// <summary>A page of what the viewer shows (`viewer.data {popout, part, …}`), read by the core on its
    /// media workers: waits (the feed's `media` names what it waits for, and it asks again at least every
    /// second) until it is ready, failed, or `timeout` passes. `{status, …}`.</summary>
    public async Task<JsonElement> DataAsync(string part, TimeSpan? timeout = null, params (string, JsonElement)[] extra)
    {
        var p = Jsonv.Object([("popout", Jsonv.Of(Id)), ("part", Jsonv.Of(part)), .. extra]);
        var deadline = DateTime.UtcNow + (timeout ?? TimeSpan.FromSeconds(60));
        while (true)
        {
            JsonElement r;
            try { r = await Model.Core.ResultAsync("viewer.data", p, Model.RequestDoc); }
            catch (Exception e) { return Jsonv.Object(("status", Jsonv.Of("failed")), ("error", Jsonv.Of(e.Message))); }
            if (((JsonElement?)r).Get("status").Str() != "pending" || ((JsonElement?)r).Get("wait").Str() is not { } key) return r;
            var left = deadline - DateTime.UtcNow;
            if (left <= TimeSpan.Zero) return Jsonv.Object(("status", Jsonv.Of("failed")), ("error", Jsonv.Of("the value took too long to read")));
            await Model.WaitAsync(() => Model.Media.TryGetValue(key, out var m) && m.StatusWord != MediaWord.Pending, left < TimeSpan.FromSeconds(1) ? left : TimeSpan.FromSeconds(1));
        }
    }

    /// <summary>Copy, Share… or Save As… for the person at it (`ui.invoke {command, params: {popout, …}, by:
    /// person}`): the core raises the ask (clipboard, share, file). The core's refusal, or null.</summary>
    public async Task<string?> TakeOutAsync(string command, params (string, JsonElement)[] extra)
    {
        try
        {
            await Model.InvokeAsync(command, Jsonv.Object([("popout", Jsonv.Of(Id)), .. extra]), person: true);
            return null;
        }
        catch (Exception e) { return e.Message; }
    }

    /// <summary>The value written to a file of its own (`value.export`), for a drag out: its path.</summary>
    public async Task<string?> ExportFileAsync()
    {
        try
        {
            var r = await Model.RunAsync("value.export", Jsonv.Object(("popout", Jsonv.Of(Id))));
            return ((JsonElement?)r).Get("file").Str();
        }
        catch (Exception) { return null; }
    }

    /// <summary>The person closed the window: the core's pop-out goes too.</summary>
    public async Task CloseAsync()
    {
        try { await Model.RunAsync("view.close", Jsonv.Object(("id", Jsonv.Of(Id)))); } catch (Exception) { }
    }
}

/// <summary>What changed in a document's pop-outs: a platform's window manager opens, brings forward and
/// closes by it.</summary>
public sealed record PopoutDiff(List<PopoutWindow> Opened, List<PopoutWindow> Raised, List<PopoutWindow> Changed, List<ulong> Closed)
{
    public static PopoutDiff Of(IReadOnlyDictionary<ulong, PopoutWindow> before, IReadOnlyList<PopoutWindow> now)
    {
        var d = new PopoutDiff([], [], [], []);
        var ids = now.Select(w => w.Id).ToHashSet();
        foreach (var w in now)
        {
            if (!before.TryGetValue(w.Id, out var b)) { d.Opened.Add(w); continue; }
            if (w.Raised > b.Raised) d.Raised.Add(w);
            if (w != b) d.Changed.Add(w);
        }
        d.Closed.AddRange(before.Keys.Where(k => !ids.Contains(k)).Order());
        return d;
    }
}

/// <summary>One setting as `settings.get` gives it: `kind` one of `choice`, `number`, `toggle`, `color`,
/// `action`, `note`.</summary>
public sealed record SettingsItem
{
    public required string Id { get; init; }
    public required string Label { get; init; }
    public required string Kind { get; init; }
    public JsonElement Value { get; init; }
    public string? Command { get; init; }
    public string? Param { get; init; }
    public JsonElement Params { get; init; }
    public List<(JsonElement Value, string Label)> Options { get; init; } = [];
    public double? Min { get; init; }
    public double? Max { get; init; }
    public double? Step { get; init; }
    public string? Unit { get; init; }
    public JsonElement Default { get; init; }
    public bool Changed { get; init; }
    public bool Enabled { get; init; } = true;
    /// <summary>Run as its menu item would, for the person (it may ask for a file).</summary>
    public bool Invoke { get; init; }
    public string? Group { get; init; }
    public string? Note { get; init; }
    public string? Contrast { get; init; }
    /// <summary>`fail`, `low`, `ok`.</summary>
    public string? ContrastLevel { get; init; }
    public string? ResetCommand { get; init; }
    public JsonElement ResetParams { get; init; }

    public static SettingsItem Of(JsonElement v)
    {
        var o = (JsonElement?)v;
        var opts = new List<(JsonElement, string)>();
        if (v.Get("options") is { ValueKind: JsonValueKind.Array } a)
            foreach (var x in a.EnumerateArray())
                opts.Add((x.Get("value") ?? Jsonv.Null, ((JsonElement?)x).Get("label").Str() ?? (x.Get("value") ?? Jsonv.Null).Text()));
        return new SettingsItem
        {
            Id = o.Get("id").Str() ?? "", Label = o.Get("label").Str() ?? "", Kind = o.Get("kind").Str() ?? "note",
            Value = v.Get("value") ?? Jsonv.Null, Command = o.Get("command").Str(), Param = o.Get("param").Str(),
            Params = v.Get("params") is { ValueKind: JsonValueKind.Object } p ? p : Jsonv.EmptyObject, Options = opts,
            Min = o.Get("min").Double(), Max = o.Get("max").Double(), Step = o.Get("step").Double(), Unit = o.Get("unit").Str(),
            Default = v.Get("default") ?? Jsonv.Null, Changed = o.Get("changed").Bool() ?? false, Enabled = o.Get("enabled").Bool() ?? true,
            Invoke = o.Get("invoke").Bool() ?? false, Group = o.Get("group").Str(), Note = o.Get("note").Str(),
            Contrast = ((JsonElement?)v.Get("contrast")).Get("text").Str(), ContrastLevel = ((JsonElement?)v.Get("contrast")).Get("level").Str(),
            ResetCommand = ((JsonElement?)v.Get("reset")).Get("command").Str(), ResetParams = v.Get("reset")?.Get("params") ?? Jsonv.EmptyObject,
        };
    }

    public static JsonElement? Get(JsonElement? v, string k) => v is { ValueKind: JsonValueKind.Object } o ? o.Get(k) : null;
}

/// <summary>A section of settings: its items in runs of the same `group` (a heading within it).</summary>
public sealed record SettingsSection(string Id, string Label, string? Note, List<SettingsItem> Items)
{
    public List<(string? Group, List<SettingsItem> Items)> Groups
    {
        get
        {
            var outp = new List<(string?, List<SettingsItem>)>();
            foreach (var it in Items)
            {
                if (outp.Count > 0 && outp[^1].Item1 == it.Group) outp[^1].Item2.Add(it);
                else outp.Add((it.Group, [it]));
            }
            return outp;
        }
    }
}

/// <summary>The core's settings, read through a document (every document shares them), each set through
/// the command its item names (contract 14.4). The section shown is the core's too (`app.settings {tab}`,
/// the scene's `view.windows.settings`), so a script shows the same page a person does.</summary>
public sealed class SettingsModel(DocumentModel model)
{
    public DocumentModel Model { get; } = model;
    public List<SettingsSection> Sections { get; private set; } = [];
    /// <summary>The theme chosen (`system` or a theme's name) and the one shown.</summary>
    public string Chosen { get; private set; } = "system";
    public string Shown { get; private set; } = "paper";
    public string? Error { get; private set; }
    public JsonElement? Reply { get; private set; }
    public event Action? Changed;

    /// <summary>Read them (`settings.get`).</summary>
    public async Task RefreshAsync()
    {
        try
        {
            var v = await Model.Core.ResultAsync("settings.get", Jsonv.EmptyObject, Model.RequestDoc);
            Apply(v);
            Error = null;
        }
        catch (Exception e) { Error = e.Message; }
        Changed?.Invoke();
        Model.Poke();
    }

    public void Apply(JsonElement v)
    {
        Reply = v;
        var o = (JsonElement?)v;
        Chosen = o.Get("chosen").Str() ?? Chosen;
        Shown = o.Get("shown").Str() ?? Shown;
        var list = new List<SettingsSection>();
        if (v.Get("sections") is { ValueKind: JsonValueKind.Array } ss)
            foreach (var s in ss.EnumerateArray())
            {
                var items = new List<SettingsItem>();
                if (s.Get("items") is { ValueKind: JsonValueKind.Array } its) foreach (var it in its.EnumerateArray()) items.Add(SettingsItem.Of(it));
                var so = (JsonElement?)s;
                list.Add(new SettingsSection(so.Get("id").Str() ?? "", so.Get("label").Str() ?? "", so.Get("note").Str(), items));
            }
        Sections = list;
    }

    public SettingsSection? Section(string id) => Sections.FirstOrDefault(s => s.Id == id);
    public SettingsItem? Item(string id) => Sections.SelectMany(s => s.Items).FirstOrDefault(i => i.Id == id);

    /// <summary>Set an item to `value` through its command (`command.run {command, params + {param: value}}`).</summary>
    public async Task SetAsync(SettingsItem item, JsonElement value)
    {
        if (item.Command is not { } c) return;
        var f = new List<(string, JsonElement)>();
        if (item.Params.ValueKind == JsonValueKind.Object) foreach (var p in item.Params.EnumerateObject()) if (p.Name != item.Param) f.Add((p.Name, p.Value));
        if (item.Param is { } k) f.Add((k, value));
        await RunAsync(c, Jsonv.Object([.. f]));
    }

    /// <summary>An item's own reset (a colour back to the theme's).</summary>
    public async Task ResetAsync(SettingsItem item)
    {
        if (item.ResetCommand is { } c) await RunAsync(c, item.ResetParams);
    }

    /// <summary>An action: as its menu item would run it (`ui.invoke` for the person, which may ask for a
    /// file), or the command with its params.</summary>
    public async Task ActAsync(SettingsItem item)
    {
        if (item.Command is not { } c) return;
        if (item.Invoke)
        {
            try { await Model.InvokeAsync(c, person: true); Error = null; }
            catch (Exception e) { Error = e.Message; }
            await RefreshAsync();
        }
        else await RunAsync(c, item.Params);
    }

    /// <summary>Show a section (`app.settings {tab}`): the core keeps which.</summary>
    public async Task ShowAsync(string tab)
    {
        try { await Model.RunAsync("app.settings", Jsonv.Object(("tab", Jsonv.Of(tab)))); } catch (Exception e) { Error = e.Message; }
    }

    /// <summary>The person closed the window (`app.settings {open: false}`).</summary>
    public async Task CloseAsync()
    {
        try { await Model.RunAsync("app.settings", Jsonv.Object(("open", Jsonv.False))); } catch (Exception) { }
    }

    async Task RunAsync(string c, JsonElement p)
    {
        try { await Model.RunAsync(c, p); Error = null; }
        catch (Exception e) { Error = e.Message; }
        await RefreshAsync();
    }
}

/// <summary>One report window's model (`report.get {window, node?}`: the Cache Report, the graph's
/// Parameters window, a value editor), read again when the core's `view.windows.reports` stamp moves. Its
/// buttons and fields are core commands.</summary>
public sealed class ReportModel(DocumentModel model, string key)
{
    public DocumentModel Model { get; } = model;
    /// <summary>`cache`, `params` or `editor:<node>`.</summary>
    public string Key { get; } = key;
    public JsonElement? Reply { get; private set; }
    public string? Error { get; private set; }
    /// <summary>What a field holds while a person types (by its edit's command and row): the shell's draft.</summary>
    public Dictionary<string, string> Drafts { get; } = [];
    /// <summary>Why a field's text was refused, by the same key.</summary>
    public Dictionary<string, string> Refused { get; } = [];
    public event Action? Changed;
    ulong? stamp;
    bool busy, again;

    public string Title => Reply.Get("title").Str() ?? (Key == "cache" ? "Cache Report" : Key == "params" ? "Parameters" : "Value Editor");

    JsonElement Params => Key.StartsWith("editor:", StringComparison.Ordinal)
        ? Jsonv.Object(("window", Jsonv.Of("editor")), ("node", Jsonv.Of(Key["editor:".Length..])))
        : Jsonv.Object(("window", Jsonv.Of(Key)));

    /// <summary>The core's stamp: read again when it moved.</summary>
    public void Follow(ulong? s)
    {
        if (Reply is not null && s == stamp) return;
        stamp = s;
        Reload();
    }

    public async void Reload()
    {
        if (busy) { again = true; return; }
        busy = true;
        try
        {
            var r = await Model.Core.ResultAsync("report.get", Params, Model.RequestDoc);
            if (Reply is not { } old || !Jsonv.Same(old, r)) { Reply = r; Changed?.Invoke(); }
            Error = null;
        }
        catch (Exception e) { Error = e.Message; Changed?.Invoke(); }
        busy = false;
        Model.Poke();
        if (again) { again = false; Reload(); }
    }

    /// <summary>A button's command (`{command, params}`).</summary>
    public async Task RunAsync(JsonElement action)
    {
        if (((JsonElement?)action).Get("command").Str() is not { } c) return;
        try { await Model.RunAsync(c, action.Get("params") ?? Jsonv.EmptyObject); Error = null; }
        catch (Exception e) { Error = e.Message; }
        Reload();
    }

    public static string DraftKey(JsonElement edit, string row) => $"{((JsonElement?)edit).Get("command").Str() ?? ""}|{row}";

    /// <summary>A field's text, sent as its command takes it (`{…params, key: text}`; read as JSON when the
    /// edit says `json`). True when taken.</summary>
    public async Task<bool> CommitAsync(JsonElement edit, string row, string text)
    {
        var e = (JsonElement?)edit;
        if (e.Get("command").Str() is not { } c || e.Get("key").Str() is not { } k) return false;
        var dk = DraftKey(edit, row);
        if (text == (e.Get("value").Str() ?? "")) { Drafts.Remove(dk); return true; }
        JsonElement v;
        if (e.Get("json").IsTrue())
        {
            try { v = Jsonv.Parse(text); }
            catch (JsonException) { Refused[dk] = "That isn't JSON."; Changed?.Invoke(); return false; }
        }
        else v = Jsonv.Of(text);
        var f = new List<(string, JsonElement)>();
        if (edit.Get("params") is { ValueKind: JsonValueKind.Object } p) foreach (var x in p.EnumerateObject()) if (x.Name != k) f.Add((x.Name, x.Value));
        f.Add((k, v));
        try
        {
            await Model.RunAsync(c, Jsonv.Object([.. f]));
            Refused.Remove(dk);
            Drafts.Remove(dk);
            Reload();
            return true;
        }
        catch (Exception ex)
        {
            Refused[dk] = ex.Message;
            Changed?.Invoke();
            return false;
        }
    }

    /// <summary>The person closed the window: its Close action in the core, else its window's own command.</summary>
    public async Task CloseAsync()
    {
        try
        {
            if (Reply?.Get("actions") is { ValueKind: JsonValueKind.Array } acts && acts.EnumerateArray().FirstOrDefault(a => ((JsonElement?)a).Get("label").Str() == "Close") is { ValueKind: JsonValueKind.Object } close
                && ((JsonElement?)close).Get("command").Str() is { } cc)
                await Model.RunAsync(cc, close.Get("params") ?? Jsonv.EmptyObject);
            else if (Key == "cache") await Model.RunAsync("run.cacheReport", Jsonv.Object(("open", Jsonv.False)));
            else if (Key == "params") await Model.RunAsync("view.params", Jsonv.Object(("open", Jsonv.False)));
            else if (Key.StartsWith("editor:", StringComparison.Ordinal))
                await Model.RunAsync("node.editor", Jsonv.Object(("node", Jsonv.Of(Key["editor:".Length..])), ("open", Jsonv.False)));
        }
        catch (Exception) { }
    }

    public int Rows => Reply?.Get("sections") is { ValueKind: JsonValueKind.Array } ss ? ss.EnumerateArray().Sum(s => s.Get("rows") is { ValueKind: JsonValueKind.Array } r ? r.GetArrayLength() : 0) : 0;
}

/// <summary>Compare's state and sides (`compare.get`), read again when the core's stamp moves; `viewer.compare`
/// and `viewer.unpin` change it.</summary>
public sealed class CompareModel(DocumentModel model)
{
    public DocumentModel Model { get; } = model;
    public JsonElement? Reply { get; private set; }
    public string? Error { get; private set; }
    public event Action? Changed;
    ulong? stamp;

    public void Follow(ulong? s)
    {
        if (Reply is not null && s == stamp) return;
        stamp = s;
        _ = ReloadAsync();
    }

    public async Task ReloadAsync()
    {
        try
        {
            var r = await Model.Core.ResultAsync("compare.get", Jsonv.EmptyObject, Model.RequestDoc);
            if (Reply is not { } old || !Jsonv.Same(old, r)) { Reply = r; Changed?.Invoke(); }
            Error = null;
        }
        catch (Exception e) { Error = e.Message; Changed?.Invoke(); }
        Model.Poke();
    }

    /// <summary>`viewer.compare {…}` (how, split, on) or `viewer.unpin {slot}`.</summary>
    public async Task RunAsync(string command, JsonElement p)
    {
        try { await Model.RunAsync(command, p); Error = null; }
        catch (Exception e) { Error = e.Message; }
        await ReloadAsync();
    }

    public async Task CloseAsync()
    {
        try { await Model.RunAsync("viewer.compare", Jsonv.Object(("on", Jsonv.False))); } catch (Exception) { }
    }

    /// <summary>`side` or `split`.</summary>
    public string How => Reply.Get("how").Str() == "split" ? "split" : "side";
}

/// <summary>The status bar's messages in full (`messages.get`), read again while the window is open: on every
/// feed batch the document sees, at most twice a second, and every second.</summary>
public sealed class MessagesModel(DocumentModel model)
{
    public DocumentModel Model { get; } = model;
    public List<JsonElement> Messages { get; private set; } = [];
    public event Action? Changed;
    CancellationTokenSource? running;

    public void Start()
    {
        if (running is not null) return;
        running = new CancellationTokenSource();
        _ = Loop(running.Token);
    }

    public void Stop()
    {
        running?.Cancel();
        running = null;
    }

    async Task Loop(CancellationToken t)
    {
        while (!t.IsCancellationRequested)
        {
            await ReadAsync();
            try { await Task.Delay(1000, t); } catch (TaskCanceledException) { return; }
        }
    }

    public async Task ReadAsync()
    {
        try
        {
            var r = await Model.Core.ResultAsync("messages.get", Jsonv.EmptyObject, Model.RequestDoc);
            var list = r.Get("messages") is { ValueKind: JsonValueKind.Array } a ? a.EnumerateArray().ToList() : [];
            if (list.Count != Messages.Count || list.Zip(Messages).Any(p => !Jsonv.Same(p.First, p.Second)))
            {
                Messages = list;
                Changed?.Invoke();
            }
        }
        catch (Exception) { }
        Model.Poke();
    }

    /// <summary>The person closed it: the core's window closes too.</summary>
    public async Task CloseAsync()
    {
        try { await Model.RunAsync("view.messages", Jsonv.Object(("open", Jsonv.False))); } catch (Exception) { }
    }
}

/// <summary>The Acknowledgements window's model (FR4): `acknowledgements.get {shell: "windows", query}`, read
/// when it opens and as the search changes, the last search winning.</summary>
public sealed class AcknowledgementsModel(DocumentModel model)
{
    public DocumentModel Model { get; } = model;
    public JsonElement Reply { get; private set; }
    public string Query { get; private set; } = "";
    public event Action? Changed;
    int asked;

    public async Task ReadAsync(string? query = null)
    {
        if (query is not null) Query = query;
        var mine = ++asked;
        try
        {
            var r = await Model.Core.ResultAsync("acknowledgements.get", Jsonv.Object(("shell", Jsonv.Of("windows")), ("query", Jsonv.Of(Query))), Model.RequestDoc);
            if (mine != asked) return;
            Reply = r;
            Changed?.Invoke();
        }
        catch (Exception) { }
    }

    /// <summary>The editor's list and each engine's: (id, title, list or null, why none).</summary>
    public List<(string Id, string Title, JsonElement? List, string? None)> Programs()
    {
        var out_ = new List<(string, string, JsonElement?, string?)>();
        if (Reply.ValueKind != JsonValueKind.Object) return out_;
        var editor = Reply.Get("editor");
        if (editor is { ValueKind: JsonValueKind.Object } e) out_.Add(("editor", e.Get("name").Str() ?? "PlumCraft", e.Get("list"), null));
        foreach (var x in Reply.Get("engines").Items())
        {
            var title = $"Engine: {x.Get("name").Str() ?? x.Get("id").Str() ?? ""}";
            var id = $"engine:{x.Get("id").Str()}";
            if (x.Get("given") is { ValueKind: JsonValueKind.True }) out_.Add((id, title, x.Get("list"), null));
            else out_.Add((id, title, null, x.Get("error").Str() is { } why ? $"No list given: {why}" : "No list given."));
        }
        return out_;
    }

    /// <summary>A licence's section title: its id and first packages.</summary>
    public static string GroupTitle(JsonElement g)
    {
        var names = g.Get("packages").Items().Select(p => p.Get("name").Str()).OfType<string>().ToList();
        var more = names.Count > 4 ? $" and {names.Count - 4} more" : "";
        return $"{g.Get("licence").Str()}: {string.Join(", ", names.Take(4))}{more}";
    }

    /// <summary>How many packages a list shows, of how many.</summary>
    public static string PackagesLine(JsonElement? list)
    {
        var all = list?.Get("packages").U64() ?? 0;
        var shown = list?.Get("shownPackages").U64() ?? all;
        return shown == all ? $"{all} packages" : $"{shown} of {all} packages";
    }

    /// <summary>The person closed it: the core's window closes too.</summary>
    public async Task CloseAsync()
    {
        try { await Model.RunAsync("app.acknowledgements", Jsonv.Object(("open", Jsonv.False))); } catch (Exception) { }
    }
}

/// <summary>The windows of the core's own models a document has open, by key: `cache`, `params`,
/// `editor:<node>` (report.get) and `compare`.</summary>
public static class CoreWindowKeys
{
    public static List<string> Of(DocumentWindows w)
    {
        var outp = new List<string>();
        if (w.Cache == true) outp.Add("cache");
        if (w.Params == true) outp.Add("params");
        foreach (var n in w.Editors ?? []) outp.Add("editor:" + n);
        if (w.Compare == true) outp.Add("compare");
        return outp;
    }
}
