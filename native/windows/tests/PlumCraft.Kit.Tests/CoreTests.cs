// The core through the C ABI from C#: the spike probe's flow (N0's, crates/ffi/tests/c_abi.rs and the
// Swift PlumCoreTests) as tests, and the hot calls against measured faces.

using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using PlumCraft.Core;
using PlumCraft.Kit;
using PlumCraft.Models;
using Xunit;
using Xunit.Abstractions;
using static PlumCraft.Kit.Tests.Support;

namespace PlumCraft.Kit.Tests;

public class CoreFlow(ITestOutputHelper output)
{
    [Fact]
    public void TheLayoutsAreTheHeadersAndTheVersionIsOne()
    {
        Assert.Equal(24, Marshal.SizeOf<PlumBuf>());
        Assert.Equal(32, Marshal.SizeOf<PlumBytes>());
        Assert.Equal(56, Marshal.SizeOf<PlumHit>());
        Assert.Equal(80, Marshal.SizeOf<PlumFace>());
        Assert.Equal(96, Marshal.SizeOf<PlumDrag>());
        Assert.Equal(Native.AbiVersion, PlumCore.AbiVersion);
    }

    /// <summary>The probe's flow: open, list, build, run on the text engine, node status by the feed,
    /// the 64 MB value borrowed twice without a copy, the lease outliving the core, no leases left.</summary>
    [Fact]
    public async Task OpenListBuildRunAndBorrow()
    {
        var core = OpenCore("flow");
        using var sub = core.Subscribe();
        var kinds = new List<string>();
        var reading = Task.Run(async () =>
        {
            await foreach (var b in sub.Reader.ReadAllAsync())
                lock (kinds)
                    foreach (var e in b.Events)
                        kinds.Add(e.Get("kind").Str() ?? "");
        });

        var list = await core.ResultAsync("command.list");
        Assert.Contains(list.EnumerateArray(), c => c.Get("id").Str() == "node.add" && c.Get("enabled").IsTrue());
        Log(output, $"command.list: {list.GetArrayLength()} commands");

        async Task<string> AddNode(string kind, int x, int y) =>
            (await core.CommandAsync("node.add", O(("kind", S(kind)), ("pos", Pt(x, y))))).Get("id").Str()!;
        var lit = await AddNode("text.literal", 0, 0);
        await core.CommandAsync("node.set", O(("node", S(lit)), ("port", S("text")), ("value", S("The quick brown fox jumps over the lazy dog, then naps. 0123456\n"))));
        var rep = await AddNode("text.repeat", 260, 0);
        await core.CommandAsync("node.set", O(("node", S(rep)), ("port", S("times")), ("value", Jsonv.Of(1_000_000L))));
        var slow = await AddNode("text.slow", 260, 160);
        await core.CommandAsync("node.set", O(("node", S(slow)), ("port", S("seconds")), ("value", Jsonv.Of(0.3))));
        var up = await AddNode("text.upper", 520, 160);
        await core.CommandAsync("wire.connect", O(("from", PortRef(lit, "text")), ("to", PortRef(rep, "text"))));
        await core.CommandAsync("wire.connect", O(("from", PortRef(lit, "text")), ("to", PortRef(slow, "text"))));
        await core.CommandAsync("wire.connect", O(("from", PortRef(slow, "text")), ("to", PortRef(up, "text"))));
        var graph = await core.ResultAsync("graph.get");
        Assert.Equal(4, graph.Get("nodes")!.Value.EnumerateObject().Count());
        Assert.Equal(3, graph.Get("wires")!.Value.GetArrayLength());

        var started = await core.CommandAsync("run.start", O(("watch", Jsonv.Array([PortRef(rep, "text"), PortRef(up, "text")]))));
        Assert.Equal(JsonValueKind.String, started.Get("run_id")?.ValueKind);
        var done = await core.ResultAsync("spike.waitRun", O(("timeout_ms", Jsonv.Of(30_000L))));
        Assert.Equal("done", done.Pointer($"/nodes/{rep}/state").Str());
        Assert.Equal("done", done.Pointer($"/nodes/{up}/state").Str());
        // the run's events come by the feed (the feed may be a poll behind the reply)
        var deadline = DateTime.UtcNow.AddSeconds(10);
        while (DateTime.UtcNow < deadline)
        {
            lock (kinds) if (kinds.Contains("node.status") && kinds.Contains("run.reply")) break;
            await Task.Delay(20);
        }
        lock (kinds)
        {
            Assert.Contains("node.status", kinds);
            Assert.Contains("run.reply", kinds);
            Log(output, $"{kinds.Count(k => k == "node.status")} node.status events; woken {core.Wakes} times, {core.Polls} polls");
        }
        Assert.True(core.Wakes > 0);

        // borrow the 64 MB value, twice
        var handle = done.Pointer("/watch/0/full/handle").Str()!;
        var a = core.Blob(handle, out var sa);
        Assert.True(a is not null && a.Length == 64_000_000, $"plum_blob: {PlumStatus.Word(sa)}, {a?.Length} bytes");
        Assert.Equal("The quick brown fox", Encoding.ASCII.GetString(a!.Span[..19]));
        var b2 = core.Blob(handle, out _);
        Assert.NotNull(b2);
        Assert.Equal(a!.Pointer, b2.Pointer);
        Assert.NotEqual(a.Token, b2.Token);
        var token = a.Token;
        a.Dispose();
        Assert.Equal(PlumStatus.ErrMissing, Native.plum_release(token));

        core.Dispose();
        await reading.WaitAsync(TimeSpan.FromSeconds(10)); // the reader ends when the core closes
        Assert.True(b2.Span[^8..].SequenceEqual("0123456\n"u8), "lent bytes outlive the core");
        b2.Dispose();
        Assert.Equal(0ul, PlumCore.Leases);
    }

    /// <summary>What the core refuses comes back as refusals, a closed core refuses at once, and a second
    /// close does nothing.</summary>
    [Fact]
    public async Task HostileInputAndASecondClose()
    {
        var core = OpenCore("hostile");
        foreach (var raw in new[] { "{not json"u8.ToArray(), new byte[] { 0xff, 0xfe }, "{\"method\":\"no.such\"}"u8.ToArray(), "[1,2]"u8.ToArray() })
        {
            var bad = await core.RequestRawAsync(raw).WaitAsync(TimeSpan.FromSeconds(30));
            Assert.False(bad.Get("ok").IsTrue(), bad.Text());
        }
        Assert.Null(core.Blob("b3:raw:0000", out var st1));
        Assert.NotEqual(PlumStatus.Ok, st1);
        Assert.Null(core.Blob("", out _));
        // hot calls on a document that doesn't exist: a status, never a crash
        Assert.Equal(PlumStatus.ErrMissing, core.HitTest(999, 0, 0, 4).Status);
        Assert.NotEqual(PlumStatus.Ok, core.GestureMove(0, float.NaN, 0, 4).Status);
        var buf = Array.Empty<float>();
        Assert.Equal(0, core.WirePaths(999, [1, 2], 0, 0, ref buf));
        Assert.NotEqual(PlumStatus.Ok, core.GeometrySet(0, [new PlumCore.FaceMeasure(1, 0, 1, 1, float.NaN, Enumerable.Repeat(1f, 5000).ToArray(), [], null)]));
        Assert.Equal(PlumErrorKind.Open, Assert.Throws<PlumException>(() => OpenCore("bad-engine", ("engine", S("vectorcraft")))).Kind);
        var unknown = await core.RequestAsync("no.such.method", Jsonv.EmptyObject);
        Assert.False(unknown.Get("ok").IsTrue());
        var e = await Assert.ThrowsAsync<PlumException>(() => core.ResultAsync("scene.get", O(("since", S("yesterday")))));
        Assert.Equal(PlumErrorKind.Refused, e.Kind);
        var doc = await Assert.ThrowsAsync<PlumException>(() => core.ResultAsync("scene.get", default, 9999));
        Log(output, $"a request for no such document: {doc.Message} (code {doc.CoreCode})");
        Assert.Equal(PlumErrorKind.Decode, (await Assert.ThrowsAsync<PlumException>(() => core.RequestAsync<Scene>("ping"))).Kind);
        // the core still serves
        Assert.True((await core.RequestAsync("ping", Jsonv.EmptyObject)).Get("ok").IsTrue());
        core.Dispose();
        core.Dispose();
        Assert.Equal(PlumErrorKind.Closed, (await Assert.ThrowsAsync<PlumException>(() => core.ResultAsync("ping"))).Kind);
        Assert.Equal(0ul, core.Post("ping", Jsonv.EmptyObject));
        Assert.Equal(PlumStatus.ErrCore, Native.plum_close(core.Id));
    }

    /// <summary>Many requests in flight each get their own reply, and wakes coalesce.</summary>
    [Fact]
    public async Task ManyRequestsInFlightAndWakesCoalesce()
    {
        using var core = OpenCore("many");
        await core.ResultAsync("ping");
        // the document's first request is held until its engine connects (B52: the core holds at most
        // 256 for a document still connecting, so 500 sent before then could be refused); `ping` is the
        // core's own and isn't held, `scene.get` is the document's
        await core.ResultAsync("scene.get");
        var tasks = Enumerable.Range(0, 500).Select(i => core.ResultAsync("command.run", O(("command", S("node.add")), ("params", O(("kind", S("text.literal")), ("pos", Pt(i * 10, 0))))))).ToList();
        var replies = await Task.WhenAll(tasks);
        Assert.Equal(500, replies.Select(r => r.Get("id").Str()).Distinct().Count());
        Log(output, $"500 requests: {core.Wakes} wakes, {core.Polls} polls");
        Assert.True(core.Polls < 500 + 50, $"{core.Polls} polls");
    }
}

public class Hot
{
    /// <summary>Two faces measured typed, then every hot call against what the core has.</summary>
    [Fact]
    public Task HitTargetPathsAndADrag() => OnContext(async () =>
    {
        using var core = OpenCore("hot");
        var a = (await core.CommandAsync("node.add", O(("kind", S("text.literal")), ("pos", Pt(0, 0))))).Get("id").Str()!;
        var b = (await core.CommandAsync("node.add", O(("kind", S("text.upper")), ("pos", Pt(500, 0))))).Get("id").Str()!;
        await core.CommandAsync("wire.connect", O(("from", PortRef(a, "text")), ("to", PortRef(b, "text"))));
        var scene = await core.RequestAsync<Scene>("scene.get");
        var fa = scene.Nodes.Single(f => f.Id == a);
        var fb = scene.Nodes.Single(f => f.Id == b);
        var canvas = new CanvasLink(core);
        canvas.Report([new PlumCore.FaceMeasure(fa.Nid, fa.Rev, 180, 100, 28, [], [60], null), new PlumCore.FaceMeasure(fb.Nid, fb.Rev, 160, 90, 28, [44], [44], null)]);
        // measured once a request after it says so
        bool measured = false;
        for (int i = 0; i < 200 && !measured; i++)
            measured = (await core.RequestAsync<Located>("geometry.locate", O(("node", S(a))))).Measured == true;
        Assert.True(measured);

        var pin = core.HitTest(0, 180, 60, 4);
        Assert.True(pin.Ok && pin.KindOf == HitKind.Pin && pin.IsOutput && pin.Node == fa.Nid && pin.Pin == 0, $"{pin.Kind} {pin.Part} {pin.Node}");
        var head = core.HitTest(0, 520, 10, 2);
        Assert.True(head.KindOf == HitKind.Node && head.PartOf == NodePart.Header && head.Node == fb.Nid, $"{head.Kind} {head.Part}");
        Assert.Equal(HitKind.None, core.HitTest(0, -3000, -3000, 4).KindOf);
        var target = canvas.WireTarget(fa.Nid, true, 0, new Point(502, 44), 8);
        Assert.True(target.KindOf == HitKind.Pin && target.Node == fb.Nid && target.Fits == 1, $"{target.Kind} {target.Node} {target.Fits}");

        // wire paths: the wire's own path at no offset, moved by the offset when asked
        var wire = (await core.RequestAsync<Scene>("scene.get")).Wires.Single();
        var still = canvas.WirePaths([fb.Nid], 0, 0);
        Assert.Single(still);
        Assert.Equal(wire.Nid, still[0].nid);
        Assert.Equal(wire.Path.Count, still[0].pts.Length / 2);
        for (int k = 0; k < wire.Path.Count; k++) Assert.True(Math.Abs(still[0].pts[2 * k] - wire.Path[k].X) < 0.01);
        var moved = canvas.WirePaths([fb.Nid], 10, 20);
        var last = wire.Path[^1];
        Assert.True(Math.Abs(moved[0].pts[^2] - (last.X + 10)) < 0.01 && Math.Abs(moved[0].pts[^1] - (last.Y + 20)) < 0.01);

        // a drag of face b: begin (JSON), moves (typed), the paths, end (one edit.move)
        var begun = await canvas.DragBeginAsync(new Point(520, 50), slop: 2);
        Assert.Equal(DragWord.Move, begun.Drag?.KindWord);
        Assert.Equal(new[] { fb.Nid }, canvas.DraggedFaces);
        var mv = canvas.DragMove(new Point(560, 80), 2);
        Assert.True(mv.Ok && mv.KindOf == DragKind.Move && mv.Dx == 40 && mv.Dy == 30, $"{mv.Kind} {mv.Dx} {mv.Dy}");
        Assert.Single(canvas.WirePaths(mv.Dx, mv.Dy));
        var end = await canvas.DragEndAsync(new Point(560, 80), 2);
        Assert.Equal("edit.move", end.Commands.FirstOrDefault()?.Command);
        Assert.Null(canvas.Drag);
        var after = (await core.RequestAsync<Scene>("scene.get")).Nodes.Single(f => f.Id == b);
        Assert.Equal(new Point(540, 30), after.Pos);
        Assert.Equal(PlumStatus.ErrMissing, core.GestureMove(0, 0, 0, 1).Status);
    });

    /// <summary>geometry.set's JSON form carries what the typed call can't: parts and an offset.</summary>
    [Fact]
    public async Task TheJsonFormCarriesPartsAndOffset()
    {
        using var core = OpenCore("hot-json");
        var a = (await core.CommandAsync("node.add", O(("kind", S("text.upper")), ("pos", Pt(0, 0))))).Get("id").Str()!;
        var canvas = new CanvasLink(core);
        var r = await canvas.ReportJsonAsync([new FaceReport(a, new Point(200, 120), Header: 30, Inputs: [44], Outputs: [44],
            Parts: new Dictionary<string, Rect> { ["title"] = new(20, 4, 120, 24), ["menu"] = new(180, 4, 196, 24) }, Offset: new Point(0, 0))]);
        Assert.True(r.Applied == 1 && (r.Unknown is null || r.Unknown.Count == 0));
        var title = await canvas.HitJsonAsync(new Point(60, 14), 1);
        Assert.True(title.KindWord == HitWord.Node && title.Part == "title" && title.Node == a, Shell.Encode(title));
        Assert.Equal(NodePart.Menu, canvas.HitAt(new Point(188, 14), 1).PartOf);
        await Assert.ThrowsAsync<PlumException>(() => canvas.ReportJsonAsync([new FaceReport(a, new Point(1, 1), Parts: new Dictionary<string, Rect> { ["sparkle"] = new(0, 0, 1, 1) })]));
    }
}
