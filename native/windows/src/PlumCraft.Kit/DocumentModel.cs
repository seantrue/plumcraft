// One open document as the views see it: the scene by nid with a revision per item, the theme, the
// menus, the asks pending and the watched value, kept in step with the core by its feed. The Windows
// counterpart of the Swift kit's DocumentModel.
//
// Threading: a DocumentModel lives on one SynchronizationContext (a window's UI thread; in tests a
// SerialContext), the one current when it is made, and is touched only there. It subscribes to the
// core's feed (batches arrive from the pump thread, already parsed, and are posted here) and, when
// the scene changed, asks for scene.get {since}, which the core wrapper decodes on the thread pool;
// only applying the decoded delta (a few dictionary updates) runs here. Nothing here waits for the
// core: every request is awaited.
//
// Redrawing only what changed: each item lives in its own ItemModel, replaced only when its `rev`
// changes (INotifyPropertyChanged on Item), so a view bound to one face is told only when that face
// changes. The tables (`Faces`, `Wires`, ...) change only when items come or go (`Structure`). A
// Composition canvas, which isn't bound, takes `SceneChanged` instead: the nids upserted and removed.

using System.ComponentModel;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using PlumCraft.Core;
using PlumCraft.Models;

namespace PlumCraft.Kit;

/// <summary>One scene item, observable on its own.</summary>
public sealed class ItemModel<T> : INotifyPropertyChanged where T : class, ISceneItem
{
    public T Item { get; private set; }
    public ulong Nid => Item.Nid;
    public ulong Rev => Item.Rev;

    public event PropertyChangedEventHandler? PropertyChanged;

    internal ItemModel(T item) => Item = item;

    /// <summary>Replace the item when its revision changed (8.12: an item's `rev` is the generation it
    /// last changed at, so the same `rev` is the same item); whether it did.</summary>
    internal bool Update(T item)
    {
        if (item.Rev == Item.Rev) return false;
        Item = item;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Item)));
        return true;
    }
}

/// <summary>The items of one kind: by nid, in drawing order, and by id (what removals name).</summary>
public sealed class ItemTable<T> where T : class, ISceneItem
{
    private Dictionary<ulong, ItemModel<T>> byNid = [];
    private Dictionary<string, ulong> nidOf = new(StringComparer.Ordinal);
    private List<ulong> order = [];

    public int Count => order.Count;
    public ItemModel<T>? this[ulong nid] => byNid.GetValueOrDefault(nid);
    public IReadOnlyDictionary<string, ulong> NidOf => nidOf;
    /// <summary>Drawing order: the scene's (sorted by id, bytewise; the last face is on top).</summary>
    public IReadOnlyList<ulong> Order => order;
    public T? ById(string id) => nidOf.TryGetValue(id, out var n) ? byNid.GetValueOrDefault(n)?.Item : null;
    public ItemModel<T>? ModelById(string id) => nidOf.TryGetValue(id, out var n) ? byNid.GetValueOrDefault(n) : null;
    public List<T> Items => [.. order.Select(n => byNid[n].Item)];

    /// <summary>Replace everything with a full scene's items, keeping the models of items still there.</summary>
    internal void ReplaceAll(List<T> items, HashSet<ulong> changed, HashSet<ulong> gone)
    {
        var next = new Dictionary<ulong, ItemModel<T>>(items.Count);
        var ids = new Dictionary<string, ulong>(items.Count, StringComparer.Ordinal);
        foreach (var it in items)
        {
            if (byNid.TryGetValue(it.Nid, out var m))
            {
                if (m.Update(it)) changed.Add(it.Nid);
                next[it.Nid] = m;
            }
            else
            {
                next[it.Nid] = new ItemModel<T>(it);
                changed.Add(it.Nid);
            }
            ids[it.Id] = it.Nid;
        }
        foreach (var nid in byNid.Keys)
            if (!next.ContainsKey(nid)) gone.Add(nid);
        byNid = next;
        nidOf = ids;
        order = [.. items.Select(i => i.Nid)];
    }

    /// <summary>A delta's upserts and removals; whether items came or went.</summary>
    internal bool Apply(List<T> upsert, List<string> remove, HashSet<ulong> changed, HashSet<ulong> gone)
    {
        bool structural = false;
        foreach (var id in remove)
        {
            if (!nidOf.Remove(id, out var nid)) continue;
            byNid.Remove(nid);
            gone.Add(nid);
            structural = true;
        }
        foreach (var it in upsert)
        {
            if (byNid.TryGetValue(it.Nid, out var m))
            {
                if (m.Update(it)) changed.Add(it.Nid);
                if (!nidOf.TryGetValue(it.Id, out var was) || was != it.Nid)
                {
                    nidOf[it.Id] = it.Nid;
                    structural = true;
                }
            }
            else
            {
                // an id that came back with a new nid replaces the old item
                if (nidOf.TryGetValue(it.Id, out var old) && old != it.Nid)
                {
                    byNid.Remove(old);
                    gone.Add(old);
                }
                byNid[it.Nid] = new ItemModel<T>(it);
                nidOf[it.Id] = it.Nid;
                changed.Add(it.Nid);
                structural = true;
            }
        }
        if (structural)
            order = [.. byNid.Values.Select(m => m.Item).OrderBy(i => i.Id, Utf8Order.Instance).Select(i => i.Nid)];
        return structural;
    }
}

/// <summary>Strings by their UTF-8 bytes, as the core sorts ids.</summary>
public sealed class Utf8Order : IComparer<string>
{
    public static readonly Utf8Order Instance = new();

    public int Compare(string? a, string? b)
    {
        if (ReferenceEquals(a, b)) return 0;
        if (a is null) return -1;
        if (b is null) return 1;
        return Encoding.UTF8.GetBytes(a).AsSpan().SequenceCompareTo(Encoding.UTF8.GetBytes(b));
    }
}

/// <summary>What one apply changed, by nid, for a canvas that redraws by item.</summary>
public sealed record SceneChange(ulong Gen, bool Full, IReadOnlySet<ulong> Changed, IReadOnlySet<ulong> Removed, bool SelectionChanged, bool ViewChanged);

/// <summary>The engine as the feed last said.</summary>
public sealed record EngineState(bool Connected = false, bool Ready = false, string Label = "", string? Warning = null);

public sealed class DocumentModel : INotifyPropertyChanged
{
    public PlumCore Core { get; }
    /// <summary>The document's number for requests; null: the first document open.</summary>
    public ulong? RequestDoc { get; }
    /// <summary>The document's number, once the core said it (the scene's `doc`).</summary>
    public ulong? Doc { get; private set; }
    /// <summary>The context the model lives on.</summary>
    public SynchronizationContext Context { get; }

    // the scene
    public ulong Gen { get; private set; }
    public ItemTable<Face> Faces { get; } = new();
    public ItemTable<Wire> Wires { get; } = new();
    public ItemTable<Net> Nets { get; } = new();
    public ItemTable<Frame> Frames { get; } = new();
    public ItemTable<Group> Groups { get; } = new();
    public ItemTable<Mark> Marks { get; } = new();
    public Picked Selection { get; private set; } = Picked.Empty;
    public Focus? Focus { get; private set; }
    public Anim? Anim { get; private set; }
    public View View { get; private set; } = new();

    // the rest
    public ThemeReply? Theme { get; private set; }
    public List<MenuItem> MenuBar { get; private set; } = [];
    public ulong? MenuGen { get; private set; }
    /// <summary>The toolbar's buttons, from the core (menu.get {context: {kind: "toolbar"}}, N4 slice 3).</summary>
    public List<MenuItem> Toolbar { get; private set; } = [];
    public List<Ask> Asks { get; private set; } = [];
    public Watched? Watched { get; private set; }
    public EngineState Engine { get; private set; } = new();
    /// <summary>Title, path, dirty state, undo and redo steps, as the feed last said (N3 slice 4).</summary>
    public DocumentState State { get; private set; } = new();
    public bool Closing { get; private set; }
    /// <summary>The last error a fetch met (a request refused, a reply that didn't decode); null after a
    /// fetch that went well. Never thrown at the views.</summary>
    public string? LastError { get; private set; }
    /// <summary>Media notes by key (media.get, a pending blob), the latest of each.</summary>
    public Dictionary<string, MediaNote> Media { get; } = [];
    /// <summary>Scenes applied (full and delta), for tests and measurements.</summary>
    public ulong Applied { get; private set; }

    /// <summary>The scene's document windows (`view.windows`).</summary>
    public DocumentWindows Windows => View.Windows ?? new DocumentWindows();

    public event PropertyChangedEventHandler? PropertyChanged;
    /// <summary>After each scene apply, with what changed (a Composition canvas redraws by it).</summary>
    public event Action<SceneChange>? SceneChanged;
    /// <summary>A table's items came or went (a bound list rebuilds).</summary>
    public event Action? Structure;
    /// <summary>Every feed batch's engine events (envelopes), for a log or a status line.</summary>
    public event Action<IReadOnlyList<JsonElement>>? EventsArrived;
    public event Action<DocumentState>? StateChanged;
    public event Action<IReadOnlyList<MenuItem>>? MenusChanged;
    public event Action<IReadOnlyList<Ask>>? AsksChanged;
    public event Action<Watched?>? WatchedChanged;
    public event Action<EngineState>? EngineChanged;
    /// <summary>The scene's `view` changed (a fit asked, the palette opened or closed), after SceneChanged.</summary>
    public event Action<View>? ViewChanged;
    public event Action<DocumentWindows>? WindowsChanged;
    /// <summary>The core's look changed (the feed's `prefs`): settings shown read them again.</summary>
    public event Action? PrefsChanged;
    public event Action<IReadOnlyList<MenuItem>>? ToolbarChanged;
    public event Action<NameEdit?>? NameChanged;

    private FeedSubscription? feed;
    private bool sceneBusy, sceneAgain, watchedBusy, watchedAgain, menusBusy;
    private ulong? prefsGen;
    private readonly List<TaskCompletionSource> waiters = [];
    private HashSet<ulong> watchedNids = [];

    /// <summary>A model of document `doc` (null: the first open) on `core`, living on the current
    /// SynchronizationContext. It does nothing until Start.</summary>
    public DocumentModel(PlumCore core, ulong? doc = null)
    {
        Core = core;
        RequestDoc = doc;
        Doc = doc;
        Context = SynchronizationContext.Current ?? throw new InvalidOperationException("a DocumentModel needs a SynchronizationContext: a UI thread's, or a SerialContext");
    }

    private void Notify([CallerMemberName] string? name = null) => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(name));

    /// <summary>Follow the feed, and fetch the scene, theme and menus (the first scene.get and menu.get
    /// also start their feed changes).</summary>
    public void Start()
    {
        if (SynchronizationContext.Current != Context)
        {
            Context.Post(_ => Start(), null);
            return;
        }
        if (feed is not null) return;
        feed = Core.Subscribe();
        _ = FollowFeed(feed);
        RefreshScene();
        _ = RefreshThemeAsync();
        RefreshMenus();
        RefreshEngine();
        RefreshDocument();
    }

    /// <summary>Read the document's state once (ui.inspect's `document`, `path`, `dirty`, `undo`, `redo`): the
    /// feed says when it changes, but a document opened before this model followed the feed (a graph named on
    /// the command line, `document.open {path}` before the window) said so before it listened (B93: its window
    /// was titled Untitled). A state the feed brought meanwhile is newer and wins.</summary>
    public async void RefreshDocument()
    {
        try
        {
            var r = await Core.ResultAsync("ui.inspect", default, RequestDoc);
            if (State.Gen > 0) return;
            var st = new DocumentState
            {
                Gen = 0, Title = r.Get("title").Str() ?? "", Name = r.Get("document").Str() ?? "", Path = r.Get("path").Str(),
                Dirty = r.Get("dirty").IsTrue(), Undo = r.Get("undo").Str(), Redo = r.Get("redo").Str(),
            };
            if (st == State) return;
            State = st;
            Notify(nameof(State));
            StateChanged?.Invoke(st);
        }
        catch (Exception) { /* a closed core: nothing to say */ }
        finally { Pulse(); }
    }

    /// <summary>Stop following the feed (the core stays open).</summary>
    public void Stop()
    {
        feed?.Dispose();
        feed = null;
        Pulse();
    }

    private async Task FollowFeed(FeedSubscription s)
    {
        try
        {
            await foreach (var batch in s.Reader.ReadAllAsync())
            {
                if (feed != s) return;
                Handle(batch);
            }
        }
        catch (Exception e)
        {
            LastError = $"the feed: {e.Message}";
        }
        Pulse();
    }

    /// <summary>Read the engine's state once (ui.inspect's `engine`): the feed says when it changes, but an
    /// engine that connected before this model followed the feed said so before it listened.</summary>
    public async void RefreshEngine()
    {
        try
        {
            var r = await Core.ResultAsync("ui.inspect", default, RequestDoc);
            var connected = r.Pointer("/engine/connected").Bool();
            if (connected is not { } c) return;
            var e = new EngineState(c, Engine.Ready || c, r.Pointer("/engine/label").Str() ?? Engine.Label, Engine.Warning);
            // the feed's word, if it came meanwhile, is newer
            if (Engine == new EngineState() && e != Engine)
            {
                Engine = e;
                Notify(nameof(Engine));
                EngineChanged?.Invoke(e);
            }
        }
        catch (Exception) { /* a closed core: nothing to say */ }
        finally { Pulse(); }
    }

    // ---------- the feed ----------

    private bool Mine(ulong? d) => d is not { } x || Doc is not { } doc || x == doc;

    internal void Handle(FeedBatch batch)
    {
        foreach (var c in batch.Changed)
        {
            if (!Mine(c.Doc)) continue;
            switch (c.DomainWord)
            {
                case DomainWord.Scene:
                    RefreshScene();
                    break;
                case DomainWord.Menus:
                    if (c.Gen != MenuGen) RefreshMenus();
                    break;
                case DomainWord.Engine:
                    var e = new EngineState(c.Connected ?? Engine.Connected, c.Ready ?? Engine.Ready, c.Label ?? Engine.Label, c.Warning);
                    if (e != Engine)
                    {
                        Engine = e;
                        Notify(nameof(Engine));
                        EngineChanged?.Invoke(e);
                    }
                    break;
                case DomainWord.Document:
                    if (c.Closing == true && !Closing)
                    {
                        Closing = true;
                        Notify(nameof(Closing));
                    }
                    if (DocumentState.From(c) is { } st && st.Gen >= State.Gen && st != State)
                    {
                        State = st;
                        Notify(nameof(State));
                        StateChanged?.Invoke(st);
                    }
                    break;
                case DomainWord.Prefs:
                    // a setting, the theme or the system's appearance changed: the tokens again, and
                    // whoever shows the settings
                    if (c.Gen != prefsGen)
                    {
                        prefsGen = c.Gen;
                        _ = RefreshThemeAsync();
                        PrefsChanged?.Invoke();
                    }
                    break;
                default:
                    break; // a domain this kit doesn't know: skipped
            }
        }
        bool asksChanged = false;
        var asks = new List<Ask>(Asks);
        foreach (var a in batch.Asks)
        {
            if (!Mine(a.Doc) || asks.Exists(x => x.Id == a.Id)) continue;
            asks.Add(a);
            asksChanged = true;
        }
        if (batch.AsksEnded.Count > 0)
        {
            var ended = batch.AsksEnded.Select(x => x.Ask).ToHashSet();
            asksChanged |= asks.RemoveAll(x => ended.Contains(x.Id)) > 0;
        }
        if (asksChanged)
        {
            Asks = asks;
            Notify(nameof(Asks));
            AsksChanged?.Invoke(asks);
        }
        foreach (var m in batch.Media) Media[m.Key] = m;
        if (batch.Events.Count > 0) EventsArrived?.Invoke(batch.Events);
        Pulse();
    }

    // ---------- the scene ----------

    /// <summary>Ask for what changed since the scene this model has (all of it the first time). Calls made
    /// while one is under way coalesce into one more after it.</summary>
    public async void RefreshScene()
    {
        if (sceneBusy)
        {
            sceneAgain = true;
            return;
        }
        sceneBusy = true;
        try
        {
            var p = Applied == 0 ? Jsonv.EmptyObject : Jsonv.Object(("since", Jsonv.Of(Gen)));
            var r = await Core.RequestAsync<SceneReply>("scene.get", p, RequestDoc);
            Apply(r);
            if (LastError is not null)
            {
                LastError = null;
                Notify(nameof(LastError));
            }
        }
        catch (Exception e)
        {
            LastError = e.Message;
            Notify(nameof(LastError));
        }
        sceneBusy = false;
        if (sceneAgain)
        {
            sceneAgain = false;
            RefreshScene();
        }
        Pulse();
    }

    /// <summary>Apply a scene reply (public for tests and for a shell that fetched one itself).</summary>
    public void Apply(SceneReply reply)
    {
        var changed = new HashSet<ulong>();
        var removed = new HashSet<ulong>();
        var selBefore = Selection;
        var viewBefore = View;
        bool structural = false;
        if (reply.Scene is { } s)
        {
            Doc ??= s.Doc;
            Faces.ReplaceAll(s.Nodes, changed, removed);
            Wires.ReplaceAll(s.Wires, changed, removed);
            Nets.ReplaceAll(s.Nets, changed, removed);
            Frames.ReplaceAll(s.Frames, changed, removed);
            Groups.ReplaceAll(s.Groups, changed, removed);
            Marks.ReplaceAll(s.Marks, changed, removed);
            structural = true;
            SetIfChanged(s.Selection, s.Focus, s.Anim, s.View);
            Gen = s.Gen;
        }
        else if (reply.Delta is { } d)
        {
            Doc ??= d.Doc;
            structural |= Faces.Apply(d.Upsert.Nodes, d.Remove.Nodes, changed, removed);
            structural |= Wires.Apply(d.Upsert.Wires, d.Remove.Wires, changed, removed);
            structural |= Nets.Apply(d.Upsert.Nets, d.Remove.Nets, changed, removed);
            structural |= Frames.Apply(d.Upsert.Frames, d.Remove.Frames, changed, removed);
            structural |= Groups.Apply(d.Upsert.Groups, d.Remove.Groups, changed, removed);
            structural |= Marks.Apply(d.Upsert.Marks, d.Remove.Marks, changed, removed);
            SetIfChanged(d.Selection, d.Focus, d.Anim, d.View);
            Gen = d.Gen;
        }
        else
        {
            return;
        }
        Applied++;
        Notify(nameof(Gen));
        NoteWatched(changed, removed);
        bool viewChanged = !Shell.Same(viewBefore, View);
        if (structural) Structure?.Invoke();
        SceneChanged?.Invoke(new SceneChange(Gen, reply.IsFull, changed, removed, !selBefore.SameAs(Selection), viewChanged));
        if (viewChanged)
        {
            ViewChanged?.Invoke(View);
            if (!Shell.Same(viewBefore.Windows ?? new DocumentWindows(), View.Windows ?? new DocumentWindows())) WindowsChanged?.Invoke(Windows);
            if (!Shell.Same(viewBefore.Name, View.Name)) NameChanged?.Invoke(View.Name);
            // Select mode is a toolbar button's check
            if (viewBefore.Select != View.Select) RefreshToolbar();
        }
    }

    private void SetIfChanged(Picked sel, Focus? foc, Anim an, View v)
    {
        if (!sel.SameAs(Selection)) { Selection = sel; Notify(nameof(Selection)); }
        if (!Shell.Same(foc, Focus)) { Focus = foc; Notify(nameof(Focus)); }
        if (an != Anim) { Anim = an; Notify(nameof(Anim)); }
        if (!Shell.Same(v, View)) { View = v; Notify(nameof(View)); }
    }

    /// <summary>The face of a graph node (or a collapsed group's `group:<id>`).</summary>
    public Face? FaceOf(string id) => Faces.ById(id);

    // ---------- watched ----------

    /// <summary>A watched face changed, or which face is watched did: read the watched value again.</summary>
    private void NoteWatched(HashSet<ulong> changed, HashSet<ulong> removed)
    {
        var now = new HashSet<ulong>(watchedNids);
        now.ExceptWith(removed);
        bool touched = false;
        foreach (var nid in changed)
        {
            if (Faces[nid]?.Item is not { } f) continue;
            if (f.Watched)
            {
                now.Add(nid);
                touched = true;
            }
            else if (now.Remove(nid))
            {
                touched = true;
            }
        }
        if (!now.SetEquals(watchedNids)) touched = true;
        watchedNids = now;
        if (touched) RefreshWatched();
    }

    /// <summary>Read the watched value (ui.inspect's `watched`). Coalesced like the scene.</summary>
    public async void RefreshWatched()
    {
        if (watchedBusy)
        {
            watchedAgain = true;
            return;
        }
        watchedBusy = true;
        try
        {
            var i = await Core.RequestAsync<Inspect>("ui.inspect", default, RequestDoc);
            if (!Shell.Same(i.Watched, Watched))
            {
                Watched = i.Watched;
                Notify(nameof(Watched));
                WatchedChanged?.Invoke(Watched);
            }
        }
        catch (Exception e)
        {
            LastError = e.Message;
            Notify(nameof(LastError));
        }
        watchedBusy = false;
        if (watchedAgain)
        {
            watchedAgain = false;
            RefreshWatched();
        }
        Pulse();
    }

    // ---------- theme, menus, toolbar ----------

    /// <summary>Read the theme's tokens (theme.get).</summary>
    public async Task RefreshThemeAsync()
    {
        try
        {
            Theme = await Core.RequestAsync<ThemeReply>("theme.get", default, RequestDoc);
            Notify(nameof(Theme));
        }
        catch (Exception e)
        {
            LastError = e.Message;
            Notify(nameof(LastError));
        }
        Pulse();
    }

    /// <summary>A paint's colour in the current theme (clear before the theme arrives).</summary>
    public Rgba Color(Paint p) => p.Resolve(Theme?.Tokens);

    /// <summary>Read the menu bar again (menu.get; after it the feed says when menus change).</summary>
    public async void RefreshMenus()
    {
        if (menusBusy) return;
        menusBusy = true;
        try
        {
            var m = await Core.RequestAsync<MenuReply>("menu.get", default, RequestDoc);
            bool changed = m.Gen != MenuGen || !Shell.Same(m.Menus, MenuBar);
            MenuBar = m.Menus;
            MenuGen = m.Gen;
            if (changed)
            {
                Notify(nameof(MenuBar));
                MenusChanged?.Invoke(m.Menus);
            }
            if (changed || Toolbar.Count == 0) RefreshToolbar();
        }
        catch (Exception e)
        {
            LastError = e.Message;
            Notify(nameof(LastError));
        }
        menusBusy = false;
        Pulse();
    }

    /// <summary>Read the toolbar's buttons again.</summary>
    public async void RefreshToolbar()
    {
        try
        {
            var m = await Core.RequestAsync<ContextMenuReply>("menu.get", Jsonv.Object(("context", Jsonv.Object(("kind", Jsonv.Of("toolbar"))))), RequestDoc);
            if (!Shell.Same(m.Items, Toolbar))
            {
                Toolbar = m.Items;
                Notify(nameof(Toolbar));
                ToolbarChanged?.Invoke(m.Items);
            }
        }
        catch (Exception) { /* no toolbar from this core: none shown */ }
        Pulse();
    }

    /// <summary>A context menu (menu.get {context}; a gesture's reply carries the `context`).</summary>
    public async Task<List<MenuItem>> ContextMenuAsync(JsonElement context) =>
        (await Core.RequestAsync<ContextMenuReply>("menu.get", Jsonv.Object(("context", context)), RequestDoc)).Items;

    /// <summary>parameters.get {node} (N4 slice 3): every input of the node for the inspector.</summary>
    public Task<ParametersReply> ParametersAsync(string node) => Core.RequestAsync<ParametersReply>("parameters.get", Jsonv.Object(("node", Jsonv.Of(node))), RequestDoc);

    /// <summary>popout.get {id} (N4 slice 3): a pop-out's value and its viewer's state.</summary>
    public Task<PopoutValue> PopoutAsync(ulong id) => Core.RequestAsync<PopoutValue>("popout.get", Jsonv.Object(("id", Jsonv.Of(id))), RequestDoc);

    /// <summary>Activate a menu item: command.run with its params, else ui.invoke (which may ask). `person`:
    /// the person at the shell chose it, so what it asks is the shell's to show (ui.invoke {by: "person"}).</summary>
    public Task<JsonElement> ActivateAsync(MenuItem item, bool person = false)
    {
        if (item.Command is not { } command) throw new PlumException(PlumErrorKind.Refused, $"{item.Id} is the shell's to do");
        if (item.Params is { } p) return RunAsync(command, p);
        return InvokeAsync(command, person: person);
    }

    // ---------- asks ----------

    /// <summary>Answer an ask (ask.answer {ask, answer}); it leaves Asks when the feed says it ended.</summary>
    public Task AnswerAsync(Ask ask, JsonElement answer) =>
        Core.ResultAsync("ask.answer", Jsonv.Object(("ask", Jsonv.Of(ask.Id)), ("answer", answer)), RequestDoc);

    public Task CancelAsync(Ask ask) =>
        Core.ResultAsync("ask.answer", Jsonv.Object(("ask", Jsonv.Of(ask.Id)), ("cancel", Jsonv.True)), RequestDoc);

    // ---------- commands ----------

    /// <summary>command.run. Once it is answered the model fetches what changed at once rather than wait for
    /// the feed's look (read your writes, contract 7.3: the reply comes after its changes).</summary>
    public async Task<JsonElement> RunAsync(string command, JsonElement parameters = default)
    {
        var r = await Core.CommandAsync(command, parameters, RequestDoc);
        AfterOwnRequest();
        return r;
    }

    /// <summary>ui.invoke {command, params?}: the command as a menu would run it (it may raise asks).</summary>
    public async Task<JsonElement> InvokeAsync(string command, JsonElement parameters = default, bool person = false)
    {
        var fields = new List<(string, JsonElement)> { ("command", Jsonv.Of(command)) };
        if (parameters.ValueKind != JsonValueKind.Undefined) fields.Add(("params", parameters));
        if (person) fields.Add(("by", Jsonv.Of("person")));
        var r = await Core.ResultAsync("ui.invoke", Jsonv.Object([.. fields]), RequestDoc);
        AfterOwnRequest();
        return r;
    }

    /// <summary>A request this model's shell sent was answered: fetch the scene now, once it has one.</summary>
    public void AfterOwnRequest()
    {
        if (SynchronizationContext.Current != Context)
        {
            Context.Post(_ => AfterOwnRequest(), null);
            return;
        }
        if (Applied > 0 && feed is not null) RefreshScene();
    }

    // ---------- waiting (tests, and a shell that must have something before it goes on) ----------

    /// <summary>Wake whoever waits on this model: something built on it changed what they may wait for.</summary>
    public void Poke() => Pulse();

    private void Pulse()
    {
        if (waiters.Count == 0) return;
        var w = waiters.ToArray();
        waiters.Clear();
        foreach (var t in w) t.TrySetResult();
    }

    /// <summary>Wait until `condition` holds, checking again after each feed batch and each fetch this model
    /// makes, for at most `timeout`. Whether it held. Never a fixed sleep: the clock only bounds the wait.
    /// Call on the model's context.</summary>
    public async Task<bool> WaitAsync(Func<bool> condition, TimeSpan? timeout = null)
    {
        var deadline = DateTime.UtcNow + (timeout ?? TimeSpan.FromSeconds(10));
        while (!condition())
        {
            var left = deadline - DateTime.UtcNow;
            if (left <= TimeSpan.Zero) return condition();
            await NextChangeAsync(left);
        }
        return true;
    }

    /// <summary>The next feed batch or fetch this model handles, or `within`, whichever first.</summary>
    public async Task NextChangeAsync(TimeSpan within)
    {
        var t = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        waiters.Add(t);
        await Task.WhenAny(t.Task, Task.Delay(within));
        waiters.Remove(t);
    }
}
