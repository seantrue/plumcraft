// N5 slice 4, the core's windows below the views: every panel model the app binds, against a real core,
// each opened by the core's command, followed through the scene's `view.windows`, and closed by either
// side: the parameters inspector (a row set, refused, reset, shown on the face), a pop-out's viewer state
// round trips (a table sorted and paged, text wrapped, another viewer chosen, a pin for Compare), Settings
// (a setting set, read back and reset; the section shown; no text size), the Cache Report and the graph's
// Parameters window (report.get), Messages, Compare, and the pop-outs' diff.

using System.Text.Json;
using PlumCraft.Models;
using Xunit;
using static PlumCraft.Kit.Tests.Support;

namespace PlumCraft.Kit.Tests;

public class Panels
{
    [Fact]
    public Task TheParametersInspectorFollowsTheCoreAndSetsThroughIt() => OnContext(async () =>
    {
        using var core = OpenCore("panels-params");
        var model = await Started(core);
        var w = await Add(model, "text.widgets", 0, 0);
        var p = new ParametersModel(model);
        model.WindowsChanged += p.Follow;
        model.SceneChanged += _ => p.SceneChanged();
        ParametersReply? shown = null;
        int closed = 0;
        p.Changed += r => { if (r is null) closed++; else shown = r; };
        var notes = new List<(string port, string text, bool error)>();
        p.Noted += (port, text, error) => notes.Add((port, text, error));

        // opened by the core's command, as a double tap or Node › Edit Parameters… does
        await p.OpenAsync(w);
        Assert.True(await model.WaitAsync(() => shown?.Node == w), p.Error);
        Assert.Equal(w, model.Windows.Parameters);
        var rows = shown!.Sections.SelectMany(s => s.Rows).ToList();
        Assert.True(rows.Count >= 5, $"{rows.Count} rows");
        var label = rows.First(r => r.Input.Key == "label");
        Assert.True(label.Editable);

        // a value typed: set through row.set, read back
        Assert.True(await p.SetAsync("label", text: "corgi"));
        Assert.True(await model.WaitAsync(() => shown!.Sections.SelectMany(s => s.Rows).First(r => r.Input.Key == "label").Reset), "no reset after a set");
        var g = await core.ResultAsync("graph.get");
        Assert.Equal("corgi", g.Pointer($"/nodes/{w}/values/label").Str());
        Assert.Equal("Set Value", model.State.Undo);

        // a number past its range is held to it with the core's note; a choice that isn't one is refused
        var amount = rows.First(r => r.Input.Key == "amount");
        Assert.True(await p.SetAsync("amount", text: "50"));
        var mode = rows.First(r => r.Input.Key == "mode");
        Assert.False(await p.SetAsync("mode", value: Jsonv.Of("no such choice")));
        Assert.Contains(notes, n => n.port == "mode" && n.error);

        // the default back
        Assert.True(await p.ResetAsync("label"));
        Assert.True(await model.WaitAsync(() => !shown!.Sections.SelectMany(s => s.Rows).First(r => r.Input.Key == "label").Reset));

        // Show on Face: a row hidden from the face, then shown again
        Assert.True(await p.ShowOnFaceAsync("label", false));
        Assert.True(await model.WaitAsync(() => shown!.Hidden.Contains("label")));
        Assert.True(await p.ShowOnFaceAsync("label", true));
        Assert.True(await model.WaitAsync(() => !shown!.Hidden.Contains("label")));

        // closed by the person (the pane's close): the core's window goes
        await p.CloseAsync();
        Assert.True(await model.WaitAsync(() => closed > 0 && model.Windows.Parameters is null));
        _ = amount;
        _ = mode;
        model.Stop();
    });

    /// <summary>A literal feeding Text to Table (three rows), run, its table popped out.</summary>
    static async Task<(DocumentModel model, PopoutModel pop)> TablePopout(PlumCraft.Core.PlumCore core)
    {
        var model = await Started(core);
        var a = await Add(model, "text.literal", 0, 0);
        var t = await Add(model, "text.text_to_table", 400, 0);
        await model.RunAsync("node.set", O(("node", S(a)), ("port", S("text")), ("value", S("n,name\n3,c\n1,a\n2.5,b\n"))));
        await model.RunAsync("wire.connect", O(("from", PortRef(a, "text")), ("to", PortRef(t, "text"))));
        await model.RunAsync("view.popout", O(("node", S(t)), ("port", S("table"))));
        Assert.True(await model.WaitAsync(() => model.Windows.Popouts?.Count == 1));
        await model.RunAsync("run.start", Jsonv.EmptyObject);
        var pop = new PopoutModel(model, model.Windows.Popouts![0]);
        model.WindowsChanged += w => { if (w.Popouts?.FirstOrDefault(x => x.Id == pop.Id) is { } e) pop.Update(e); };
        pop.Refresh();
        Assert.True(await model.WaitAsync(() => pop.Value?.Viewer == "table.grid", TimeSpan.FromSeconds(60)), pop.Value?.Viewer ?? "no value");
        return (model, pop);
    }

    [Fact]
    public Task APopoutsViewerStateIsTheCoresAndRoundTrips() => OnContext(async () =>
    {
        using var core = OpenCore("panels-popout");
        var (model, pop) = await TablePopout(core);
        Assert.Contains(pop.Value!.Viewers!, v => v.Id == "tree");

        // the table's page, as the core reads it
        var page = await pop.DataAsync("table");
        Assert.Equal("ready", page.Get("status").Str());
        Assert.Equal(3UL, page.Get("rows").U64());
        Assert.Equal("n", page.Get("columns").At(0).Get("name").Str());
        Assert.Equal("3", page.Get("cells").At(0).At(0).Str());

        // sorted by the core: the state is its, and the page comes sorted
        Assert.NotNull(await pop.CommandAsync("viewer.table", ("sort", Jsonv.Of(0L)), ("desc", Jsonv.False)));
        Assert.True(await model.WaitAsync(() => pop.Value?.State.Get("table").Get("sort").U64() == 0));
        page = await pop.DataAsync("table");
        Assert.Equal("1", page.Get("cells").At(0).At(0).Str());
        await pop.CommandAsync("viewer.table", ("sort", Jsonv.Of(0L)), ("desc", Jsonv.True));
        Assert.True(await model.WaitAsync(() => pop.Value?.State.Get("table").Get("desc").IsTrue() == true));
        page = await pop.DataAsync("table");
        Assert.Equal("3", page.Get("cells").At(0).At(0).Str());
        await pop.CommandAsync("viewer.table", ("sort", Jsonv.Null));
        Assert.True(await model.WaitAsync(() => pop.Value?.State.Get("table").Get("sort") is null or { ValueKind: JsonValueKind.Null }));

        // another of the viewers offered, then back to the default
        await pop.ChooseViewerAsync("tree");
        Assert.True(await model.WaitAsync(() => pop.Value?.Viewer == "tree"));
        var tree = await pop.DataAsync("tree");
        Assert.Equal("ready", tree.Get("status").Str());
        await pop.ChooseViewerAsync(null);
        Assert.True(await model.WaitAsync(() => pop.Value?.Viewer == "table.grid"));

        // pinned as A for Compare: compare.get has it
        Assert.NotNull(await pop.PinAsync("a"));
        var compare = new CompareModel(model);
        await compare.RunAsync("viewer.compare", O(("on", Jsonv.True)));
        Assert.True(await model.WaitAsync(() => model.Windows.Compare == true));
        Assert.NotNull(compare.Reply.Get("a").Get("title").Str());
        await compare.RunAsync("viewer.compare", O(("how", S("split"))));
        Assert.Equal("split", compare.How);
        await compare.RunAsync("viewer.compare", O(("split", Jsonv.Of(0.3))));
        Assert.Equal(0.3, compare.Reply.Get("split").Double() ?? 0, 3);
        await compare.RunAsync("viewer.unpin", O(("slot", S("a"))));
        Assert.True(compare.Reply.Get("a") is null or { ValueKind: JsonValueKind.Null });
        await compare.CloseAsync();
        Assert.True(await model.WaitAsync(() => model.Windows.Compare != true));

        // closed by the person: the core's pop-out goes
        await pop.CloseAsync();
        Assert.True(await model.WaitAsync(() => (model.Windows.Popouts?.Count ?? 0) == 0));
        model.Stop();
    });

    [Fact]
    public Task TextViewerStateRoundTrips() => OnContext(async () =>
    {
        using var core = OpenCore("panels-text");
        var model = await Started(core);
        var a = await Add(model, "text.literal", 0, 0);
        await model.RunAsync("node.set", O(("node", S(a)), ("port", S("text")), ("value", S("a long line of text\nand another"))));
        await model.RunAsync("view.popout", O(("node", S(a)), ("port", S("text"))));
        Assert.True(await model.WaitAsync(() => model.Windows.Popouts?.Count == 1));
        await model.RunAsync("run.start", Jsonv.EmptyObject);
        var pop = new PopoutModel(model, model.Windows.Popouts![0]);
        model.WindowsChanged += w => { if (w.Popouts?.FirstOrDefault(x => x.Id == pop.Id) is { } e) pop.Update(e); };
        pop.Refresh();
        Assert.True(await model.WaitAsync(() => pop.Value?.Viewer == "text.plain", TimeSpan.FromSeconds(60)));
        Assert.True(pop.Value!.Text!.Wrap);
        await pop.TextAsync(wrap: false, mono: true);
        Assert.True(await model.WaitAsync(() => pop.Value?.Text is { Wrap: false, Mono: true }));
        await pop.TextAsync(wrap: true, mono: false);
        Assert.True(await model.WaitAsync(() => pop.Value?.Text is { Wrap: true, Mono: false }));
        model.Stop();
    });

    [Fact]
    public Task SettingsAreTheCoresAndTheWindowsSectionToo() => OnContext(async () =>
    {
        using var core = OpenCore("panels-settings");
        var model = await Started(core);
        var s = new SettingsModel(model);
        await s.RefreshAsync();
        Assert.Null(s.Error);
        Assert.Equal("appearance", s.Sections[0].Id);
        var theme = s.Sections[0].Items[0];
        Assert.Equal("choice", theme.Kind);
        Assert.Equal("view.theme", theme.Command);
        // Match the System is a choice; text size isn't offered (the platform's, N4 card 14)
        Assert.Contains(theme.Options, o => o.Value.Str() == "system");
        Assert.Null(s.Item("textSize"));

        // the window and its section are the core's
        await s.ShowAsync("running");
        Assert.True(await model.WaitAsync(() => model.Windows.Settings == "running"));
        var speed = s.Section("running")!.Items.First(i => i.Command == "view.animationSpeed");
        await s.SetAsync(speed, Jsonv.Of(1000L));
        Assert.Equal(1000.0, s.Item(speed.Id)!.Value.Double());
        await s.SetAsync(speed, Jsonv.Null);
        Assert.NotEqual(1000.0, s.Item(speed.Id)!.Value.Double());
        var accent = s.Item("token.accent")!;
        await s.SetAsync(accent, Jsonv.Of("#123456"));
        Assert.True(s.Item("token.accent")!.Changed);
        await s.ResetAsync(s.Item("token.accent")!);
        Assert.False(s.Item("token.accent")!.Changed);
        await s.CloseAsync();
        Assert.True(await model.WaitAsync(() => model.Windows.Settings is null));
        model.Stop();
    });

    [Fact]
    public Task TheCoresReportWindowsAndMessagesOpenReadAndClose() => OnContext(async () =>
    {
        using var core = OpenCore("panels-reports");
        var model = await Started(core);
        var a = await Add(model, "text.literal", 0, 0);
        var u = await Add(model, "text.upper", 400, 0);
        await model.RunAsync("wire.connect", O(("from", PortRef(a, "text")), ("to", PortRef(u, "text"))));
        await model.RunAsync("run.start", Jsonv.EmptyObject);

        // the Cache Report: opened by its command, read as the core's model, closed by its Close action
        await model.RunAsync("run.cacheReport", O(("open", Jsonv.True)));
        Assert.True(await model.WaitAsync(() => CoreWindowKeys.Of(model.Windows).Contains("cache")));
        var cache = new ReportModel(model, "cache");
        cache.Follow(model.Windows.Reports);
        Assert.True(await model.WaitAsync(() => cache.Reply is not null), cache.Error);
        Assert.Equal("Cache Report", cache.Title);
        await cache.CloseAsync();
        Assert.True(await model.WaitAsync(() => model.Windows.Cache != true));

        // the graph's Parameters window
        await model.RunAsync("view.params", O(("open", Jsonv.True)));
        Assert.True(await model.WaitAsync(() => model.Windows.Params == true));
        var pr = new ReportModel(model, "params");
        pr.Follow(model.Windows.Reports);
        Assert.True(await model.WaitAsync(() => pr.Reply is not null), pr.Error);
        await pr.CloseAsync();
        Assert.True(await model.WaitAsync(() => model.Windows.Params != true));

        // Messages
        await model.RunAsync("view.messages", O(("open", Jsonv.True)));
        Assert.True(await model.WaitAsync(() => model.Windows.Messages == true));
        var m = new MessagesModel(model);
        await m.ReadAsync();
        await m.CloseAsync();
        Assert.True(await model.WaitAsync(() => model.Windows.Messages != true));
        model.Stop();
    });

    [Fact]
    public void ThePopoutsDiffSaysWhatToOpenRaiseAndClose()
    {
        PopoutWindow W(ulong id, ulong raised, ulong stamp) => new() { Id = id, Node = "n1", Port = "out", Index = 0, Title = "t", Raised = raised, Stamp = stamp };
        var before = new Dictionary<ulong, PopoutWindow> { [1] = W(1, 0, 1), [2] = W(2, 0, 1) };
        var d = PopoutDiff.Of(before, [W(2, 1, 2), W(3, 0, 1)]);
        Assert.Equal([3UL], d.Opened.Select(x => x.Id));
        Assert.Equal([2UL], d.Raised.Select(x => x.Id));
        Assert.Equal([2UL], d.Changed.Select(x => x.Id));
        Assert.Equal([1UL], d.Closed);
    }
}
