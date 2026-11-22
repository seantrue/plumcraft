// The conformance suite from C# (docs/shell-interface.md 16.1), on a real core: the whole suite
// through the kit headless (the text engine's cases on a core opened on it, the array engine's on a
// core opened on that, as the C ABI's own Rust driver runs them), the read half alone, the whole
// suite over the core's control port, and the runner shown not vacuous.

using System.Diagnostics;
using System.Text.Json;
using PlumCraft.Conformance;
using PlumCraft.Core;
using PlumCraft.Models;
using Xunit;
using Xunit.Abstractions;
using static PlumCraft.Kit.Tests.Support;
using Runner = PlumCraft.Conformance.Conformance;

namespace PlumCraft.Kit.Tests;

public class Suite(ITestOutputHelper output)
{
    void Report(string driver, IReadOnlyList<Outcome> outcomes, TimeSpan took)
    {
        Log(output, Runner.Summary(driver, outcomes) + $" ({took.TotalSeconds:F1} s)");
        foreach (var o in outcomes)
            Log(output, $"CASE {driver} | {(o.Passed ? "pass" : o.Skipped is not null ? "skip" : "FAIL")} | {o.Name} | {o.Steps} steps{(o.Skipped is { } s ? " | " + s : "")}");
    }

    // the array engine's cases (a core opened on it runs them, below), and those for Install Engine…'s form
    // (`install`: these cores are not on a platform that installs engines, as the C ABI's Rust driver has it)
    static List<string> ArrayCases(List<JsonElement> cases) =>
        [.. cases.Where(c => Runner.Needs(c, "engine:array") || Runner.Needs(c, "install")).Select(c => c.Get("name").Str()!)];

    /// <summary>The gate: every case passes through the kit headless. The text engine's core runs every case
    /// but the array engine's, which a core opened on the array engine runs: all 51 pass between them, no
    /// case failed or left out, as the C ABI's Rust driver counts them.</summary>
    [Fact]
    public Task TheWholeSuitePassesThroughTheKit() => OnContext(async () =>
    {
        var cases = Runner.Cases(CasesPath);
        var arrays = ArrayCases(cases);
        Assert.True(arrays.Count >= 5);

        var sw = Stopwatch.StartNew();
        List<Outcome> text;
        using (var core = OpenCore("conf-kit-all"))
        {
            var d = new KitDriver(core, "text");
            text = await Runner.RunAsync(d, cases, null);
            Report(d.Name + ", every case", text, sw.Elapsed);
            // the document model kept up with every case's scene
            var full = await core.RequestAsync<Scene>("scene.get");
            Assert.True(await d.Model.WaitAsync(() => d.Model.Gen == full.Gen), $"model {d.Model.Gen}, core {full.Gen}");
            Assert.True(Shell.Same(d.Model.Faces.Items, full.Nodes));
        }
        Assert.True(text.All(o => o.Error is null), Runner.Summary("the Windows kit", text));
        Assert.Equal(cases.Count, text.Count);
        Assert.Equal(arrays, text.Where(o => o.Skipped is not null).Select(o => o.Name).ToList());

        var t1 = sw.Elapsed;
        List<Outcome> array;
        using (var core = OpenCore("conf-kit-array", ("engine", S("array"))))
        {
            var d = new KitDriver(core, "array");
            array = await Runner.RunAsync(d, cases.Where(c => Runner.Needs(c, "engine:array")), null);
            Report(d.Name + ", the array engine's cases", array, sw.Elapsed - t1);
        }
        Assert.True(array.Count > 0 && array.All(o => o.Passed), Runner.Summary("the Windows kit (array engine)", array));

        int passed = text.Count(o => o.Passed) + array.Count(o => o.Passed);
        Log(output, $"WHOLE SUITE through the Windows kit: {passed} of {cases.Count} cases passed in {sw.Elapsed.TotalSeconds:F1} s");
        // all but Install Engine…'s form, which needs a platform that installs (the app runs it: `--has install`)
        Assert.Equal(cases.Count - cases.Count(c => Runner.Needs(c, "install")), passed);
    });

    /// <summary>N3's gate half, alone: the read half needs nothing the editing half leaves behind.</summary>
    [Fact]
    public Task TheReadHalfPassesAloneThroughTheKit() => OnContext(async () =>
    {
        using var core = OpenCore("conf-kit-read");
        var d = new KitDriver(core, "text");
        var cases = Runner.Cases(CasesPath);
        var sw = Stopwatch.StartNew();
        var outp = await Runner.RunAsync(d, cases, CaseHalf.Read);
        Report(d.Name + ", the read half", outp, sw.Elapsed);
        Assert.True(outp.All(o => o.Error is null), Runner.Summary(d.Name, outp));
        Assert.Equal(cases.Count(c => Runner.HalfOf(c) == "read"), outp.Count);
        var arrays = ArrayCases([.. cases.Where(c => Runner.HalfOf(c) == "read")]);
        Assert.Equal(arrays, outp.Where(o => o.Skipped is not null).Select(o => o.Name).ToList());
    });

    /// <summary>Over the core's control port, JSON lines with `auth` first, as a test harness drives an app's core.</summary>
    [Fact]
    public Task TheWholeSuitePassesOverTheControlPort() => OnContext(async () =>
    {
        var tokenFile = Path.Combine(Scratch("conf-port"), "token");
        using var core = OpenCore("conf-port", ("control", O(("port", Jsonv.Of(0L)), ("token_file", S(tokenFile)))));
        var port = (await core.ResultAsync("core.control")).Get("port").U64()!.Value;
        var token = File.ReadAllText(tokenFile).Trim();
        var model = new PlumCraft.Kit.DocumentModel(core);
        model.Start();
        using var d = await PortDriver.ConnectAsync((int)port, token, following: model);
        d.Also.Add("manifests");
        var cases = Runner.Cases(CasesPath);
        var sw = Stopwatch.StartNew();
        var outp = await Runner.RunAsync(d, cases, null);
        Report(d.Name, outp, sw.Elapsed);
        Assert.True(outp.All(o => o.Error is null), Runner.Summary(d.Name, outp));
        // the port's driver has the text engine; the array engine's cases are the kit's (above)
        Assert.Equal(ArrayCases(cases), outp.Where(o => o.Skipped is not null).Select(o => o.Name).ToList());
    });

    /// <summary>The copied manifest's steps (`copied_manifest`): the kit's core on the suite's copied engines folder
    /// marks the copied engine with a warning, in engine.list, ui.inspect and the Engine menu, and not the probe.</summary>
    [Fact]
    public Task TheCopiedManifestStepsPassThroughTheKit() => OnContext(async () =>
    {
        var root = Scratch("conf-copied");
        var folder = CopiedManifest.Make(root, Fixture("crates/editor-core/fixtures/conformance/engines/probe.toml"));
        string D(string x) { var p = Path.Combine(root, x); Directory.CreateDirectory(p); return p; }
        using var core = OpenCore("conf-copied", ("dirs", O(("documents", S(D("documents"))), ("caches", S(D("caches"))), ("store", S(D("store"))),
            ("scratch", S(D("scratch"))), ("prefs", S(D("prefs"))), ("logs", S(D("logs"))), ("engines", S(folder)))),
            // the copied engine is offered only where programs start (as the Windows app declares)
            ("platform", O(("os", S("windows")), ("subprocess", Jsonv.True))));
        var d = new KitDriver(core, "text");
        d.Also.Add("copied_manifest");
        var cases = Runner.Cases(CasesPath).Where(c => Runner.Needs(c, "manifests")).ToList();
        Assert.NotEmpty(cases);
        var outp = await Runner.RunAsync(d, cases, null);
        Report(d.Name + ", the copied manifest", outp, TimeSpan.Zero);
        Assert.True(outp.All(o => o.Passed), Runner.Summary(d.Name, outp) + " " + string.Join("; ", outp.Select(o => o.Error)));
        Assert.Equal(0, outp.Sum(o => o.StepsSkipped));
    });

    /// <summary>The runner is not vacuous: a wrong want, a wrong ok, a missing value each fail their case.</summary>
    [Fact]
    public Task ACaseThatIsWrongFails() => OnContext(async () =>
    {
        using var core = OpenCore("conf-wrong");
        var d = new KitDriver(core, "text");
        var add = Jsonv.Parse("""{"call": "command.run", "params": {"command": "node.add", "params": {"kind": "text.literal", "pos": [0, 0]}}, "save": {"a": "/result/id"}}""");
        (string step, string says)[] bad =
        [
            ("""{"call": "scene.get", "expect": {"/result/selection/nodes": ["$a"]}}""", "wanted"),
            ("""{"gesture": {"kind": "tap", "at": [0, 0]}, "ok": false}""", "wanted ok false"),
            ("""{"gesture": {"kind": "bogus", "at": [0, 0]}}""", "wanted ok true"),
            ("""{"call": "graph.get", "expect": {"/result/nodes/$a/nothing": 1}}""", "got nothing"),
            ("""{"gesture": {"kind": "tap", "at": {"$locate": {"node": "n404"}}}}""", "geometry.locate"),
            ("""{"call": "scene.get", "save": {"x": "/nope"}}""", "nothing at"),
            ("""{"call": "scene.get", "expect": {"/result/nodes": {"$len": 7}}}""", "$len"),
            ("""{"check": "nidsUnique", "scene": {"nodes": []}}""", "too few"),
        ];
        foreach (var (step, says) in bad)
        {
            var c = Jsonv.Object(("name", S("wrong")), ("about", S("-")), ("half", S("read")), ("steps", Jsonv.Array([add, Jsonv.Parse(step)])));
            var o = await Runner.RunCaseAsync(d, c);
            var e = o.Error ?? "";
            Assert.True(e.StartsWith("step 2:") && e.Contains(says), $"{step}: {e}");
        }
        // and a case needing what the driver lacks is skipped, never passed
        var needs = Jsonv.Object(("name", S("needs")), ("about", S("-")), ("half", S("read")), ("needs", Jsonv.Array([S("engine:nonesuch")])), ("steps", Jsonv.Array([add])));
        var skipped = await Runner.RunCaseAsync(d, needs);
        Assert.True(skipped.Skipped is not null && skipped.Error is null && !skipped.Passed);
    });
}
