// The document model against a real core, headless (the Swift kit's KitTests): it follows the feed,
// applies deltas by nid, tells only the item that changed, and holds the theme, menus, asks and
// watched value. And every live reply the kit decodes, decoded strictly: no field the C# types don't know.

using System.Text.Json;
using PlumCraft.Core;
using PlumCraft.Models;
using Xunit;
using Xunit.Abstractions;
using static PlumCraft.Kit.Tests.Support;

namespace PlumCraft.Kit.Tests;

public class Model(ITestOutputHelper output)
{
    [Fact]
    public Task ItFollowsTheSceneByNidAndTellsOnlyWhatChanged() => OnContext(async () =>
    {
        using var core = OpenCore("model");
        var model = await Started(core);
        var changes = new List<SceneChange>();
        model.SceneChanged += changes.Add;
        var a = await Add(model, "text.literal", 0, 0);
        var b = await Add(model, "text.upper", 500, 0);
        await model.RunAsync("wire.connect", O(("from", PortRef(a, "text")), ("to", PortRef(b, "text"))));
        Assert.True(await model.WaitAsync(() => model.Faces.Count == 2 && model.Wires.Count == 1));
        var fa = model.Faces.ModelById(a)!;
        var fb = model.Faces.ModelById(b)!;
        var (revA, revB) = (fa.Rev, fb.Rev);
        int faTold = 0, fbTold = 0;
        fa.PropertyChanged += (_, _) => faTold++;
        fb.PropertyChanged += (_, _) => fbTold++;

        // one node's value changes: its face's model alone takes a new item
        changes.Clear();
        await model.RunAsync("node.set", O(("node", S(a)), ("port", S("text")), ("value", S("hello"))));
        Assert.True(await model.WaitAsync(() => fa.Rev != revA));
        Assert.Equal("hello", fa.Item.Inputs[0].Widget?.Value.GetString());
        Assert.Same(fb, model.Faces[fb.Nid]);
        Assert.Equal(revB, fb.Rev);
        Assert.True(faTold > 0 && fbTold == 0, $"told a {faTold}, b {fbTold}");
        Assert.Contains(changes, c => c.Changed.Contains(fa.Nid));
        Assert.DoesNotContain(changes, c => c.Changed.Contains(fb.Nid));

        // a tap selects: the selection comes with the delta
        var canvas = new CanvasLink(model);
        var at = (await canvas.LocateAsync(O(("node", S(b)), ("part", S("header"))))).At;
        await canvas.TapAsync(at, slop: 2);
        Assert.True(await model.WaitAsync(() => model.Selection.Nodes.SequenceEqual([b])));

        // a node removed: its face and its wire go
        changes.Clear();
        await model.RunAsync("node.remove", O(("nodes", Jsonv.Array([S(b)]))));
        Assert.True(await model.WaitAsync(() => model.Faces.Count == 1 && model.Wires.Count == 0));
        Assert.Null(model.Faces[fb.Nid]);
        Assert.Contains(changes, c => c.Removed.Contains(fb.Nid));

        // the model is what a full scene says, item for item
        var full = await core.RequestAsync<Scene>("scene.get");
        Assert.True(await model.WaitAsync(() => model.Gen == full.Gen), $"model {model.Gen}, core {full.Gen}");
        Assert.True(Shell.Same(model.Faces.Items, full.Nodes));
        Assert.True(Shell.Same(model.Wires.Items, full.Wires));
        Assert.True(model.Selection.SameAs(full.Selection));
        Assert.Null(model.LastError);
    });

    [Fact]
    public Task ManyFacesComeAndGoInOrder() => OnContext(async () =>
    {
        using var core = OpenCore("model-many");
        var model = await Started(core);
        await core.ResultAsync("spike.scene", O(("nodes", Jsonv.Of(60L)), ("wires", Jsonv.Of(80L))));
        Assert.True(await model.WaitAsync(() => model.Faces.Count == 60 && model.Wires.Count == 80));
        var full = await core.RequestAsync<Scene>("scene.get");
        Assert.Equal(full.Nodes.Select(f => f.Id), model.Faces.Items.Select(f => f.Id));
        await model.RunAsync("graph.new", O(("discard", Jsonv.True)));
        Assert.True(await model.WaitAsync(() => model.Faces.Count == 0 && model.Wires.Count == 0));
        var a = await Add(model, "text.literal", 0, 0);
        Assert.True(await model.WaitAsync(() => model.FaceOf(a) is not null));
    });

    [Fact]
    public Task ThemeMenusAndAContextMenu() => OnContext(async () =>
    {
        using var core = OpenCore("model-menus");
        var model = await Started(core);
        var a = await Add(model, "text.literal", 0, 0);
        Assert.True(await model.WaitAsync(() => model.FaceOf(a) is not null));
        var face = model.FaceOf(a)!;
        if (face.Tint.Header.Token is { } t) Assert.True(model.Theme!.Tokens.ContainsKey(t), $"token {t}");
        var titles = model.MenuBar.Select(m => m.Label).ToList();
        Assert.True(titles.Contains("File") && titles.Contains("Edit"), string.Join(", ", titles));
        var canvas = new CanvasLink(model);
        var at = (await canvas.LocateAsync(O(("node", S(a)), ("part", S("header"))))).At;
        var g = await canvas.ContextAsync(at, slop: 2);
        Assert.NotNull(g.Context);
        var menu = await model.ContextMenuAsync(g.Context.Value);
        Assert.Contains(menu.SelectMany(m => m.Flatten()), m => m.Command == "node.remove");
        // a menu item runs: View › Fit is a plain command
        if (model.MenuBar.SelectMany(m => m.Flatten()).FirstOrDefault(m => m.Command == "canvas.fit") is { } fit)
        {
            await model.ActivateAsync(fit);
            Assert.True(await model.WaitAsync(() => model.View.Fit is not null));
        }
        Assert.NotEmpty(model.Toolbar);
    });

    [Fact]
    public Task AnAskArrivesAndEnds() => OnContext(async () =>
    {
        using var core = OpenCore("model-asks");
        var model = await Started(core);
        await model.InvokeAsync("graph.open");
        Assert.True(await model.WaitAsync(() => model.Asks.Count > 0));
        var ask = model.Asks[0];
        Assert.Equal(AskWord.FileOpen, ask.KindWord);
        Assert.True(ask.Modal);
        await model.CancelAsync(ask);
        Assert.True(await model.WaitAsync(() => model.Asks.Count == 0));
    });

    [Fact]
    public Task WatchedTextAndScalarArrive() => OnContext(async () =>
    {
        using var core = OpenCore("model-watched");
        var model = await Started(core);
        var a = await Add(model, "text.literal", 0, 0);
        await model.RunAsync("node.set", O(("node", S(a)), ("port", S("text")), ("value", S("hello"))));
        await model.RunAsync("run.watch", O(("node", S(a)), ("port", S("text"))));
        Assert.True(await model.WaitAsync(() => model.Watched?.Viewer == "text.plain" && model.Watched?.Excerpt is not null, TimeSpan.FromSeconds(60)));
        Assert.StartsWith("hello", model.Watched!.Excerpt!.Text);
        var v = (await model.RunAsync("node.add", O(("kind", S("vary.float")), ("pos", Pt(0, 400))))).Get("id").Str()!;
        await model.RunAsync("node.set", O(("node", S(v)), ("port", S("values")), ("value", S("2.5"))));
        await model.RunAsync("run.watch", O(("node", S(v)), ("port", S("values"))));
        Assert.True(await model.WaitAsync(() => model.Watched?.Viewer == "scalar" && model.Watched?.Value is not null, TimeSpan.FromSeconds(60)));
        // the face of a run node says so
        Assert.True(await model.WaitAsync(() => model.FaceOf(v)?.Run.State is not null));
    });

    [Fact]
    public Task ASecondDocumentHasItsOwnModel() => OnContext(async () =>
    {
        using var core = OpenCore("model-docs");
        var first = await Started(core);
        var opened = await core.ResultAsync("document.open");
        var doc = opened.Get("doc").U64()!.Value;
        var second = new DocumentModel(core, doc);
        second.Start();
        Assert.True(await second.WaitAsync(() => second.Applied > 0));
        Assert.True(second.Doc == doc && first.Doc != doc);
        await second.RunAsync("node.add", O(("kind", S("text.upper")), ("pos", Pt(0, 0))));
        Assert.True(await second.WaitAsync(() => second.Faces.Count == 1));
        Assert.Equal(0, first.Faces.Count);
        // hot calls name the document
        var canvas = new CanvasLink(second);
        Assert.Equal(doc, canvas.HotDoc);
        var f = second.Faces.Items[0];
        Assert.Equal(f.Nid, canvas.HitAt(new Point(f.Pos.X + 10, f.Pos.Y + 10), 2).Node);
    });

    /// <summary>A model started after its engine connected still learns the engine is ready.</summary>
    [Fact]
    public Task AModelStartedLateKnowsItsEngine() => OnContext(async () =>
    {
        using var core = OpenCore("model-late");
        var first = await Started(core);
        Assert.True(first.Engine.Connected);
        var late = new DocumentModel(core, first.Doc);
        late.Start();
        Assert.True(await late.WaitAsync(() => late.Engine.Connected), "the late model never heard its engine is ready");
        Assert.Contains("connected", late.Engine.Label);
    });

    /// <summary>B93: a graph opened before its model followed the feed (the command line's `document.open
    /// {path}` before the window) is known by its file, not as Untitled.</summary>
    [Fact]
    public Task AGraphOpenedBeforeItsModelIsKnownByItsFile() => OnContext(async () =>
    {
        using var core = OpenCore("model-opened");
        var path = Path.Combine(Scratch("model-opened-graph"), "opened first.json");
        File.WriteAllText(path, """{"version": 1, "nodes": {"t": {"kind": "text.literal", "values": {"text": "hi"}, "pos": [0, 0]}}, "wires": []}""");
        var opened = await core.ResultAsync("document.open", O(("path", S(path))));
        var doc = opened.Get("doc").U64()!.Value;
        // the core said so in the feed before anyone listened; the model starts afterwards
        await Task.Delay(300);
        var model = new DocumentModel(core, doc);
        model.Start();
        Assert.True(await model.WaitAsync(() => model.State.Path is not null), "the model never learned the document's path");
        Assert.Equal(Path.GetFullPath(path), Path.GetFullPath(model.State.Path!));
        Assert.Contains("opened first", model.State.Name.Length > 0 ? model.State.Name : Path.GetFileName(model.State.Path!));
    });

    /// <summary>A picture from the array engine: watched, made by media.get, lent by plum_blob.</summary>
    [Fact]
    public Task AWatchedPictureIsLentWithoutACopy() => OnContext(async () =>
    {
        using var core = OpenCore("model-picture", ("engine", S("array")));
        var model = await Started(core);
        var m = await Add(model, "array.mandelbrot", 0, 0);
        var e = await Add(model, "array.encode_image", 500, 0);
        await model.RunAsync("node.set", O(("node", S(m)), ("port", S("width")), ("value", Jsonv.Of(64L))));
        await model.RunAsync("node.set", O(("node", S(m)), ("port", S("height")), ("value", Jsonv.Of(48L))));
        await model.RunAsync("wire.connect", O(("from", PortRef(m, "escape")), ("to", PortRef(e, "image"))));
        await model.RunAsync("run.watch", O(("node", S(e)), ("port", S("image"))));
        Assert.True(await model.WaitAsync(() => model.Watched?.Viewer == "picture" && model.Watched?.Picture is not null, TimeSpan.FromSeconds(120)));
        var pic = model.Watched!.Picture!;
        var handle = pic.Handle!;
        var got = await core.ResultAsync("media.get", O(("handle", S(handle)), ("want", S("rgba"))));
        var key = got.Get("key").Str() ?? "";
        if (got.Get("status").Str() != "ready")
        {
            Assert.True(await model.WaitAsync(() => model.Media.Values.Any(n => n.Handle == handle && n.StatusWord == MediaWord.Ready), TimeSpan.FromSeconds(60)));
            key = model.Media.Values.First(n => n.Handle == handle).Key;
        }
        using var lease = core.Blob(key, out var status);
        Assert.True(lease is not null, $"the picture: {PlumStatus.Word(status)}");
        Assert.Equal((int)(pic.W * pic.H * 4), lease.Length);
        Log(output, $"a {pic.W}x{pic.H} picture lent: {lease.Length} bytes");
    });
}

public class LiveReplies
{
    /// <summary>Every reply a shell decodes, from a real core, has no field the C# types don't know.</summary>
    [Fact]
    public Task NoFieldIsUnknown() => OnContext(async () =>
    {
        using var core = OpenCore("strict");
        var model = await Started(core);
        var a = await Add(model, "text.literal", 0, 0);
        var b = await Add(model, "text.upper", 500, 0);
        await model.RunAsync("wire.connect", O(("from", PortRef(a, "text")), ("to", PortRef(b, "text"))));
        await model.RunAsync("frame.add", O(("pos", Pt(-600, 600)), ("size", Pt(300, 200))));
        await model.RunAsync("group.create", O(("nodes", Jsonv.Array([S(b)])), ("collapsed", Jsonv.True)));
        var problems = new List<string>();
        void Check<T>(string what, JsonElement v)
        {
            try { Shell.Decode<T>(v, Shell.Strict); }
            catch (JsonException e) { problems.Add($"{what}: {e.Message}"); }
        }
        var full = await core.ResultAsync("scene.get");
        Check<SceneReply>("scene.get", full);
        await model.RunAsync("node.set", O(("node", S(a)), ("port", S("text")), ("value", S("changed"))));
        Check<SceneReply>("scene.get {since}", await core.ResultAsync("scene.get", O(("since", full.Get("gen")!.Value))));
        Check<ThemeReply>("theme.get", await core.ResultAsync("theme.get"));
        Check<MenuReply>("menu.get", await core.ResultAsync("menu.get"));
        var at = await core.ResultAsync("geometry.locate", O(("node", S(a)), ("part", S("header"))));
        Check<Located>("geometry.locate", at);
        var p = Shell.Decode<Located>(at).At;
        Check<Hit>("geometry.hit", await core.ResultAsync("geometry.hit", O(("at", Pt(p.X, p.Y)), ("slop", Jsonv.Of(2.0)))));
        foreach (var g in new[] { Gesture.Tap(p, slop: 2), Gesture.DoubleTap(new Point(-3000, -3000)), Gesture.ContextAt(p, 2), Gesture.MarqueeOf(new Point(-50, -50), new Point(40, 40)) })
            Check<GestureReply>($"ui.gesture {g.Kind}", await core.ResultAsync("ui.gesture", g.ToJson()));
        var ctx = await core.ResultAsync("ui.gesture", Gesture.ContextAt(p, 2).ToJson());
        Check<ContextMenuReply>("menu.get {context}", await core.ResultAsync("menu.get", O(("context", ctx.Get("context")!.Value))));
        var pin = (await core.RequestAsync<Located>("geometry.locate", O(("node", S(a)), ("pin", O(("side", S("out")), ("index", Jsonv.Of(0L))))))).At;
        Check<GestureReply>("drag begin", await core.ResultAsync("ui.gesture", Gesture.DragBegin(pin, slop: 4).ToJson()));
        Check<GestureReply>("drag move", await core.ResultAsync("ui.gesture", Gesture.DragMove(new Point(300, 300), 4).ToJson()));
        Check<GestureReply>("drag end", await core.ResultAsync("ui.gesture", Gesture.DragEnd(new Point(300, 300), 4).ToJson()));
        // the wire dropped on nothing opened the palette for it
        Check<PaletteItems>("palette.items", await core.ResultAsync("palette.items", O(("limit", Jsonv.Of(20L)))));
        Check<SceneReply>("scene.get with the palette open", await core.ResultAsync("scene.get"));
        var canvas = new CanvasLink(model);
        await canvas.DragCancelAsync();
        Check<GeometrySetReply>("geometry.set", await core.CommandAsync("geometry.set", O(("faces", Jsonv.Array([new FaceReport(a, new Point(100, 80), Outputs: [40]).ToJson()])))));
        // a run, then the watched value
        await model.RunAsync("run.watch", O(("node", S(a)), ("port", S("text"))));
        await core.ResultAsync("ui.waitRun", O(("timeout", Jsonv.Of(60L))));
        Assert.True(await model.WaitAsync(() => model.Watched?.Excerpt is not null, TimeSpan.FromSeconds(60)));
        var inspect = await core.ResultAsync("ui.inspect");
        Check<Watched>("ui.inspect's watched", inspect.Get("watched")!.Value);
        Check<SceneReply>("scene.get after a run", await core.ResultAsync("scene.get"));
        Check<ParametersReply>("parameters.get", await core.ResultAsync("parameters.get", O(("node", S(a)))));
        Assert.True(problems.Count == 0, string.Join("\n", problems));
    });
}
