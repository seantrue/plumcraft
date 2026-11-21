// N5 spike, part 1: N0's flow (crates/ffi/tests/c_abi.rs, the Swift PlumCoreTests) from C# through
// P/Invoke on Windows: open a core, list commands, build a small graph, run it on the in-process
// text engine, see node status arrive, borrow the 64 MB value without a copy; then the boundary's
// timings, as note 24 section 15.1 measured them on the Mac.
//
//   PlumCraft.Probe.exe <scratch root> [out.json]
//
// Prints one line per check and exits 1 on the first that fails.

using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using PlumCraft.Core;

static class Program
{
    static int failures;
    static readonly JsonObject results = new();

    static void Check(bool ok, string what)
    {
        Console.WriteLine($"{(ok ? "ok  " : "FAIL")} {what}");
        if (!ok) failures++;
    }

    static double Median(List<double> xs)
    {
        xs.Sort();
        return xs.Count == 0 ? double.NaN : xs[xs.Count / 2];
    }

    static double Ms(long ticks) => ticks * 1000.0 / Stopwatch.Frequency;

    static async Task<int> Main(string[] args)
    {
        var root = args.Length > 0 ? args[0] : Path.Combine(Path.GetTempPath(), "plumcraft-n5-probe");
        var outPath = args.Length > 1 ? args[1] : null;
        Console.WriteLine($"runtime {RuntimeInformation.RuntimeIdentifier}, {RuntimeInformation.FrameworkDescription}, process {RuntimeInformation.ProcessArchitecture}");

        // layouts against the header's natural alignment on a 64-bit target
        Check(Marshal.SizeOf<PlumBuf>() == 24, $"plum_buf is 24 bytes ({Marshal.SizeOf<PlumBuf>()})");
        Check(Marshal.SizeOf<PlumBytes>() == 32, $"plum_bytes is 32 bytes ({Marshal.SizeOf<PlumBytes>()})");
        Check(Marshal.SizeOf<PlumHit>() == 56, $"plum_hit is 56 bytes ({Marshal.SizeOf<PlumHit>()})");
        Check(Marshal.SizeOf<PlumFace>() == 80, $"plum_face is 80 bytes ({Marshal.SizeOf<PlumFace>()})");
        Check(Marshal.SizeOf<PlumDrag>() == 96, $"plum_drag is 96 bytes ({Marshal.SizeOf<PlumDrag>()})");
        Check(PlumCore.AbiVersion == Native.AbiVersion, $"PLUM_ABI_VERSION {PlumCore.AbiVersion}");

        var cfg = new ShellConfig { Root = root };
        cfg.Prepare();
        var t0 = Stopwatch.GetTimestamp();
        using var core = new PlumCore(cfg.ToJson());
        results["open_ms"] = Ms(Stopwatch.GetTimestamp() - t0);
        var events = new List<JsonElement>();
        core.Feed += poll =>
        {
            if (poll.TryGetProperty("events", out var ev))
                lock (events) foreach (var e in ev.EnumerateArray()) events.Add(e);
        };

        // the first request waits for the engine to connect
        t0 = Stopwatch.GetTimestamp();
        var list = await core.CallAsync("command.list");
        results["first_reply_ms"] = Ms(Stopwatch.GetTimestamp() - t0);
        var cmds = list.EnumerateArray().ToList();
        Check(cmds.Any(c => c.GetProperty("id").GetString() == "node.add" && c.GetProperty("enabled").GetBoolean()), $"command.list: {cmds.Count} commands, node.add enabled");
        results["commands"] = cmds.Count;

        // a small graph, as crates/ffi/tests/common/mod.rs build_graph
        async Task<string> Add(string kind, int x, int y) =>
            (await core.RunAsync("node.add", new JsonObject { ["kind"] = kind, ["pos"] = new JsonArray(x, y) })).GetProperty("id").GetString()!;
        var lit = await Add("text.literal", 0, 0);
        await core.RunAsync("node.set", new JsonObject { ["node"] = lit, ["port"] = "text", ["value"] = "The quick brown fox jumps over the lazy dog, then naps. 0123456\n" });
        var rep = await Add("text.repeat", 260, 0);
        await core.RunAsync("node.set", new JsonObject { ["node"] = rep, ["port"] = "times", ["value"] = 1_000_000 });
        var slow = await Add("text.slow", 260, 160);
        await core.RunAsync("node.set", new JsonObject { ["node"] = slow, ["port"] = "seconds", ["value"] = 0.3 });
        var up = await Add("text.upper", 520, 160);
        JsonObject Port(string n, string p) => new() { ["node"] = n, ["port"] = p };
        await core.RunAsync("wire.connect", new JsonObject { ["from"] = Port(lit, "text"), ["to"] = Port(rep, "text") });
        await core.RunAsync("wire.connect", new JsonObject { ["from"] = Port(lit, "text"), ["to"] = Port(slow, "text") });
        await core.RunAsync("wire.connect", new JsonObject { ["from"] = Port(slow, "text"), ["to"] = Port(up, "text") });
        var graph = await core.CallAsync("graph.get");
        Check(graph.GetProperty("nodes").EnumerateObject().Count() == 4 && graph.GetProperty("wires").GetArrayLength() == 3, "graph.get: 4 nodes, 3 wires");

        // run it, watching two outputs
        var started = await core.RunAsync("run.start", new JsonObject { ["watch"] = new JsonArray(Port(rep, "text"), Port(up, "text")) });
        Check(started.TryGetProperty("run_id", out var rid) && rid.ValueKind == JsonValueKind.String, "run.start: a run id");
        t0 = Stopwatch.GetTimestamp();
        var done = await core.CallAsync("spike.waitRun", new JsonObject { ["timeout_ms"] = 30_000 });
        results["run_ms"] = Ms(Stopwatch.GetTimestamp() - t0);
        Check(done.GetProperty("nodes").GetProperty(rep).GetProperty("state").GetString() == "done"
              && done.GetProperty("nodes").GetProperty(up).GetProperty("state").GetString() == "done", "the run: both watched nodes done");
        await Task.Delay(200);
        List<string> kinds;
        lock (events) kinds = events.Select(e => e.TryGetProperty("kind", out var k) ? k.GetString() ?? "" : "").ToList();
        Check(kinds.Contains("node.status") && kinds.Contains("run.reply"), $"node.status and run.reply arrived by the feed ({kinds.Count(k => k == "node.status")} statuses)");
        Check(core.Wakes > 0, $"woken {core.Wakes} times, {core.Polls} polls");

        // borrow the 64 MB value, twice
        var handle = done.GetProperty("watch")[0].GetProperty("full").GetProperty("handle").GetString()!;
        var a = core.Blob(handle, out var sa);
        Check(a != null && a.Length == 64_000_000, $"plum_blob: {PlumStatus.Word(sa)}, {a?.Length} bytes");
        Check(a != null && Encoding.ASCII.GetString(a.Span[..19]) == "The quick brown fox", "the lent bytes are the value's");
        var b = core.Blob(handle, out _);
        Check(a != null && b != null && a.Pointer == b.Pointer && a.Token != b.Token, "a second borrow shares the first's bytes");
        a?.Dispose();
        Check(Native.plum_release(a?.Token ?? 0) == PlumStatus.ErrMissing, "released twice: missing");

        // ---- timings (medians), as note 24 15.1 ----
        var ping = new List<double>();
        for (int i = 0; i < 300; i++)
        {
            t0 = Stopwatch.GetTimestamp();
            await core.CallAsync("ping");
            ping.Add(Ms(Stopwatch.GetTimestamp() - t0));
        }
        results["ping_ms_p50"] = Median(ping);
        Console.WriteLine($"     request to reply, ping: {Median(ping):F4} ms median of 300");

        foreach (var (n, w) in new[] { (500, 800), (2000, 3200) })
        {
            await core.CallAsync("spike.scene", new JsonObject { ["nodes"] = n, ["wires"] = w });
            var gg = new List<double>();
            int size = 0;
            for (int i = 0; i < 15; i++)
            {
                t0 = Stopwatch.GetTimestamp();
                var r = await core.RequestAsync("graph.get");
                gg.Add(Ms(Stopwatch.GetTimestamp() - t0));
                size = r.GetRawText().Length;
            }
            var sg = new List<double>();
            int ssize = 0;
            for (int i = 0; i < 9; i++)
            {
                t0 = Stopwatch.GetTimestamp();
                var r = await core.RequestAsync("scene.get");
                sg.Add(Ms(Stopwatch.GetTimestamp() - t0));
                ssize = r.GetRawText().Length;
            }
            results[$"graph_get_{n}_ms_p50"] = Median(gg);
            results[$"graph_get_{n}_kb"] = size / 1024;
            results[$"scene_get_{n}_ms_p50"] = Median(sg);
            results[$"scene_get_{n}_kb"] = ssize / 1024;
            Console.WriteLine($"     graph.get {n} nodes, {w} wires: {Median(gg):F2} ms ({size / 1024} KB); scene.get {Median(sg):F2} ms ({ssize / 1024} KB)");

            // hot calls on the published scene
            var hit = new List<double>();
            for (int i = 0; i < 2000; i++)
            {
                t0 = Stopwatch.GetTimestamp();
                var h = core.HitTest(0, 130 + i % 7, 60, 4);
                hit.Add(Ms(Stopwatch.GetTimestamp() - t0) * 1000);
                if (i == 0) Check(h.Status == PlumStatus.Ok && h.Kind == 1, $"plum_hit_test on a face at {n}: kind {h.Kind} status {PlumStatus.Word(h.Status)}");
            }
            results[$"hit_test_{n}_us_p50"] = Median(hit);
            Console.WriteLine($"     plum_hit_test {n}: {Median(hit):F2} us");
        }

        // a 64 MiB blob stored plain (mapped), borrowed
        var pub = await core.CallAsync("spike.publish", new JsonObject { ["bytes"] = 64 << 20, ["seed"] = 7 });
        var ph = pub.GetProperty("handle").GetString()!;
        var bl = new List<double>();
        for (int i = 0; i < 50; i++)
        {
            t0 = Stopwatch.GetTimestamp();
            var l = core.Blob(ph, out var st);
            bl.Add(Ms(Stopwatch.GetTimestamp() - t0) * 1000);
            if (i == 0) Check(l != null && l.Length == 64 << 20, $"plum_blob of a 64 MiB plain blob: {PlumStatus.Word(st)}");
            l?.Dispose();
        }
        results["blob_64mib_us_p50"] = Median(bl);
        Console.WriteLine($"     borrow a 64 MiB blob stored plain: {Median(bl):F1} us median of 50");
        results["parse_ms_total"] = Ms(core.ParseTicks);

        core.Dispose();
        Check(b != null && b.Span[^8..].SequenceEqual("0123456\n"u8), "lent bytes outlive the core");
        b?.Dispose();
        Check(PlumCore.Leases == 0, $"no leases left ({PlumCore.Leases})");

        results["failures"] = failures;
        if (outPath != null) File.WriteAllText(outPath, results.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        Console.WriteLine(failures == 0 ? "PROBE PASSED" : $"PROBE FAILED: {failures}");
        return failures == 0 ? 0 : 1;
    }
}
