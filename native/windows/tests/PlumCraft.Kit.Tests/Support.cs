// What the tests share: the repository's fixtures read where they are, a scratch folder for every
// store and root the core touches (never a real one), cores opened on it, and models started on a
// context of their own.

using System.Runtime.CompilerServices;
using System.Text.Json;
using PlumCraft.Core;
using PlumCraft.Kit;
using PlumCraft.Models;
using Xunit;
using Xunit.Abstractions;

[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace PlumCraft.Kit.Tests;

static class Support
{
    /// <summary>The repository's root, from this file's place in it.</summary>
    public static readonly string Repo = FindRepo();

    static string FindRepo([CallerFilePath] string here = "")
    {
        var d = new DirectoryInfo(Path.GetDirectoryName(here) ?? ".");
        while (d is not null && !File.Exists(Path.Combine(d.FullName, "crates", "editor-core", "fixtures", "conformance", "cases.json"))) d = d.Parent;
        return d?.FullName ?? throw new InvalidOperationException($"no repository above {here}");
    }

    public static string Fixture(string rel) => Path.Combine(Repo, rel.Replace('/', Path.DirectorySeparatorChar));

    public static JsonElement ReadFixture(string rel) => Jsonv.Parse(File.ReadAllBytes(Fixture(rel)));

    public static string CasesPath => Fixture("crates/editor-core/fixtures/conformance/cases.json");

    /// <summary>The scratch root: PLUMCRAFT_TEST_SCRATCH, else the temp folder, a folder per process.</summary>
    public static readonly string ScratchRoot = Path.Combine(
        Environment.GetEnvironmentVariable("PLUMCRAFT_TEST_SCRATCH") is { Length: > 0 } s ? s : Path.GetTempPath(),
        $"plumcraft-test-{Environment.ProcessId}");

    public static string Scratch(string name)
    {
        var d = Path.Combine(ScratchRoot, $"cs-{name}");
        Directory.CreateDirectory(d);
        return d;
    }

    static int environmentSet;

    /// <summary>Never a real store, marks, craft or preferences root, whatever the environment named.</summary>
    public static void Environment_()
    {
        if (Interlocked.Exchange(ref environmentSet, 1) == 1) return;
        Environment.SetEnvironmentVariable("PLUMCRAFT_EDITOR_PREFS", Path.Combine(Scratch("prefs"), "prefs.json"));
        Environment.SetEnvironmentVariable("COMMONKIT_STORE_ROOT", Scratch("store"));
        Environment.SetEnvironmentVariable("COMMONKIT_MARK_DIR", Scratch("marks"));
        Environment.SetEnvironmentVariable("COMMONKIT_TEST_RUN", Scratch("run"));
        Environment.SetEnvironmentVariable("PLUMCRAFT_CRAFT_ROOT", Scratch("craft"));
        if (Environment.GetEnvironmentVariable("COMMONKIT_HASH") is null) Environment.SetEnvironmentVariable("COMMONKIT_HASH", "b3");
        if (Environment.GetEnvironmentVariable("COMMONKIT_STORE_MIN_FREE_GB") is null) Environment.SetEnvironmentVariable("COMMONKIT_STORE_MIN_FREE_GB", "2");
    }

    /// <summary>A core on a store of its own, every root under the scratch folder; `extra` fields over the config.</summary>
    public static PlumCore OpenCore(string name, params (string key, JsonElement value)[] extra)
    {
        Environment_();
        var root = Scratch($"core-{name}");
        string D(string x)
        {
            var p = Path.Combine(root, x);
            Directory.CreateDirectory(p);
            return p;
        }
        var fields = new List<(string, JsonElement)>
        {
            ("store_root", Jsonv.Of(D("store"))),
            ("dirs", Jsonv.Object(("documents", Jsonv.Of(D("documents"))), ("caches", Jsonv.Of(D("caches"))), ("store", Jsonv.Of(D("store"))),
                ("scratch", Jsonv.Of(D("scratch"))), ("prefs", Jsonv.Of(D("prefs"))), ("logs", Jsonv.Of(D("logs"))))),
        };
        foreach (var (k, v) in extra)
        {
            fields.RemoveAll(f => f.Item1 == k);
            fields.Add((k, v));
        }
        return new PlumCore(Jsonv.Object([.. fields]));
    }

    /// <summary>A model of the core's first document, started, once it has its first scene, engine, theme
    /// and menus. Call on a context (Ctx.Run).</summary>
    public static async Task<DocumentModel> Started(PlumCore core)
    {
        var model = new DocumentModel(core);
        model.Start();
        var ok = await model.WaitAsync(() => model.Applied > 0 && model.Engine.Connected && model.Theme is not null && model.MenuBar.Count > 0, TimeSpan.FromSeconds(30));
        Assert.True(ok, $"the model never had its first scene, engine, theme and menus: {model.LastError}");
        return model;
    }

    public static async Task<string> Add(DocumentModel model, string kind, double x, double y)
    {
        var r = await model.RunAsync("node.add", Jsonv.Object(("kind", Jsonv.Of(kind)), ("pos", Pt(x, y))));
        return r.Get("id").Str() ?? throw new InvalidOperationException($"node.add gave no id: {r.Text()}");
    }

    public static JsonElement Pt(double x, double y) => Jsonv.Array([Jsonv.Of(x), Jsonv.Of(y)]);

    public static JsonElement PortRef(string node, string port) => Jsonv.Object(("node", Jsonv.Of(node)), ("port", Jsonv.Of(port)));

    public static JsonElement O(params (string, JsonElement)[] f) => Jsonv.Object(f);

    public static JsonElement S(string s) => Jsonv.Of(s);

    /// <summary>Run `f` on a context of its own (the model's thread), then end the context.</summary>
    public static async Task OnContext(Func<Task> f)
    {
        using var ctx = new SerialContext("plum test");
        await ctx.Run(f);
    }

    /// <summary>A line for the report: to the test's output and to a file in the scratch root.</summary>
    public static void Log(ITestOutputHelper? output, string line)
    {
        output?.WriteLine(line);
        try
        {
            Directory.CreateDirectory(ScratchRoot);
            File.AppendAllText(Path.Combine(ScratchRoot, "k1-test-log.txt"), line + Environment.NewLine);
        }
        catch (IOException) { }
    }
}
