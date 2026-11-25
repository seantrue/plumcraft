// The panels' and viewers' bench (N5 slice 4, `--bench OUT --bench-panels --nodes 500`): on the budget's
// graph with a few nodes more that make every kind of value, each panel and each viewer is opened by the
// core's command as a menu or a script opens it, and timed from the command to the first frame that shows
// it with its content (the next CompositionTarget.Rendering after the panel says it has drawn what it
// read); then a table of 100,000 rows is popped out and its page scrolled for the phase's seconds, a move
// a frame, counting frames and late frames as the canvas's bench does, and five pages turned through the
// core (`viewer.table {page}`). On the test VM (WARP, 64 Hz) these compare, they don't judge.

using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml.Media;
using PlumCraft.Models;

namespace PlumCraft.App;

public static class PanelBench
{
    static double Now => CoreHost.Clock.Elapsed.TotalMilliseconds;

    static JsonElement O(params (string, JsonElement)[] f) => Jsonv.Object(f);
    static JsonElement S(string s) => Jsonv.Of(s);
    static JsonElement P(string node, string port) => O(("node", S(node)), ("port", S(port)));

    /// <summary>Until `cond` holds, checked every frame, at most `limit`: the frame it held at, or null.</summary>
    static async Task<double?> FrameWhen(Func<bool> cond, TimeSpan limit)
    {
        var end = Now + limit.TotalMilliseconds;
        while (Now < end)
        {
            await Bench.NextFrame();
            if (cond()) return await Bench.NextFrame();
        }
        return null;
    }

    static async Task<string> Add(DocWindow w, string kind, double x, double y)
    {
        var r = await w.Model.RunAsync("node.add", O(("kind", S(kind)), ("pos", Jsonv.Array([Jsonv.Of(x), Jsonv.Of(y)]))));
        return r.Get("id").Str() ?? throw new InvalidOperationException("node.add gave no id");
    }

    public static async Task Run(DocWindow w)
    {
        var o = CoreHost.Options;
        var m = w.Model;
        var results = new JsonObject { ["faces"] = w.CanvasView.Canvas.FaceCount };
        var (hz, _, _) = Interop.Display();
        var period = hz > 0 ? 1000.0 / hz : 16.7;
        var opened = new JsonObject();
        try
        {
            // the values: a 100,000-row table, JSON, text and a count, an array, its picture, sound and SVG
            var x0 = -2000.0;
            var widgets = await Add(w, "text.widgets", x0, 0);
            var csv = new StringBuilder("n,name,weight\n");
            for (int i = 0; i < 100_000; i++) csv.Append(i * 7919 % 100_003).Append(",plum ").Append(i).Append(',').Append((i % 997) / 10.0).Append('\n');
            var lit = await Add(w, "text.literal", x0, 400);
            var table = await Add(w, "text.text_to_table", x0 + 400, 400);
            await m.RunAsync("node.set", O(("node", S(lit)), ("port", S("text")), ("value", S(csv.ToString()))));
            await m.RunAsync("wire.connect", O(("from", P(lit, "text")), ("to", P(table, "text"))));
            var jl = await Add(w, "text.literal", x0, 700);
            var json = await Add(w, "text.text_to_json", x0 + 400, 700);
            await m.RunAsync("node.set", O(("node", S(jl)), ("port", S("text")), ("value", S("{\"plums\": [{\"name\": \"Victoria\", \"ripe\": true}, {\"name\": \"Damson\"}], \"trees\": 17}"))));
            await m.RunAsync("wire.connect", O(("from", P(jl, "text")), ("to", P(json, "text"))));
            var words = await Add(w, "text.words", x0 + 400, 1000);
            await m.RunAsync("wire.connect", O(("from", P(jl, "text")), ("to", P(words, "text"))));
            var mb = await Add(w, "array.mandelbrot", x0, 1300);
            await m.WaitAsync(() => m.FaceOf(mb) is not null, TimeSpan.FromSeconds(30));
            await m.RunAsync("node.set", O(("node", S(mb)), ("port", S("width")), ("value", Jsonv.Of(160L))));
            await m.RunAsync("node.set", O(("node", S(mb)), ("port", S("height")), ("value", Jsonv.Of(120L))));
            var img = await Add(w, "array.encode_image", x0 + 400, 1300);
            var snd = await Add(w, "array.to_audio", x0 + 400, 1600);
            var svg = await Add(w, "array.array_to_svg", x0 + 400, 1900);
            foreach (var (to, port) in new[] { (img, "image"), (snd, "array"), (svg, "image") })
                await m.RunAsync("wire.connect", O(("from", P(mb, "escape")), ("to", P(to, port))));
            var t0 = Now;
            await m.RunAsync("run.start", O(("targets", Jsonv.Array([S(table), S(json), S(words), S(img), S(snd), S(svg), S(widgets)]))));
            await m.Core.ResultAsync("ui.waitRun", O(("timeout", Jsonv.Of(300L))), m.RequestDoc);
            results["run_ms"] = Math.Round(Now - t0, 1);

            JsonObject Panels() => w.PanelsNow();

            async Task Time(string name, Func<Task> open, Func<JsonObject, bool> shown, Func<Task> close)
            {
                await Task.Delay(300);
                var t = Now;
                await open();
                var f = await FrameWhen(() => shown(Panels()), TimeSpan.FromSeconds(60));
                opened[name] = f is { } at ? Math.Round(at - t, 1) : "never";
                await close();
                await Task.Delay(300);
            }

            // the core's panels
            await Time("inspector", () => m.RunAsync("node.parameters", O(("node", S(widgets)))),
                p => p["inspector"]?["shown"]?.GetValue<bool>() == true && (p["inspector"]?["rows"]?.GetValue<int>() ?? 0) > 0,
                () => m.RunAsync("node.parameters", Jsonv.EmptyObject));
            await Time("settings", () => m.RunAsync("app.settings", O(("tab", S("appearance")))),
                p => (p["coreWindows"]?["settings"]?["items"]?.GetValue<int>() ?? 0) > 0,
                () => m.RunAsync("app.settings", O(("open", Jsonv.False))));
            await Time("messages", () => m.RunAsync("view.messages", O(("open", Jsonv.True))),
                p => p["coreWindows"]?["messages"] is not null,
                () => m.RunAsync("view.messages", O(("open", Jsonv.False))));
            await Time("cache_report", () => m.RunAsync("run.cacheReport", O(("open", Jsonv.True))),
                p => (p["coreWindows"]?["cache"]?["rows"]?.GetValue<int>() ?? 0) > 0,
                () => m.RunAsync("run.cacheReport", O(("open", Jsonv.False))));
            await Time("graph_parameters", () => m.RunAsync("view.params", O(("open", Jsonv.True))),
                p => p["coreWindows"]?["params"]?["sections"] is not null && p["coreWindows"]?["params"]?["title"] is not null,
                () => m.RunAsync("view.params", O(("open", Jsonv.False))));

            // the viewers, each in a pop-out
            async Task Viewer(string name, string node, string port, Func<JsonObject, bool> drawn, string? viewAs = null)
            {
                ulong id = 0;
                await Time(name, async () =>
                {
                    var r = await m.RunAsync("view.popout", P(node, port));
                    id = r.Get("id").U64() ?? m.Windows.Popouts?.LastOrDefault()?.Id ?? 0;
                    if (viewAs is not null) await m.RunAsync("view.viewer", O(("popout", Jsonv.Of(id)), ("viewer", S(viewAs))));
                }, p => p["popouts"] is JsonArray a && a.OfType<JsonObject>().FirstOrDefault(x => x["id"]?.GetValue<ulong>() == id) is { } v && drawn(v),
                () => m.RunAsync("view.close", O(("id", Jsonv.Of(id)))));
            }
            static bool Is(JsonObject v, string k, string want) => v[k]?.ToString() == want;
            await Viewer("table", table, "table", v => Is(v, "status", "ready"));
            await Viewer("yaml", json, "json", v => Is(v, "drawn", "yaml") && (v["yaml"]?.ToString().Length ?? 0) > 0);
            await Viewer("tree", json, "json", v => Is(v, "drawn", "tree") && (v["members"]?.GetValue<int>() ?? 0) > 0, "tree");
            await Viewer("text", jl, "text", v => Is(v, "drawn", "text.plain") && (v["shown"]?.GetValue<int>() ?? 0) > 0);
            await Viewer("scalar", words, "count", v => Is(v, "drawn", "scalar") && v["value"]?.ToString() is { Length: > 0 } s && s != "—");
            await Viewer("picture", img, "image", v => Is(v, "drawn", "picture") && v["loaded"]?.GetValue<bool>() == true);
            await Viewer("svg", svg, "svg", v => Is(v, "drawn", "picture") && v["loaded"]?.GetValue<bool>() == true);
            await Viewer("array_pictures", mb, "escape", v => (Is(v, "drawn", "picture") && v["loaded"]?.GetValue<bool>() == true) || (Is(v, "drawn", "array.pictures") && (v["pictures"]?.GetValue<int>() ?? 0) > 0));
            await Viewer("array_grid", mb, "escape", v => Is(v, "drawn", "array.grid") && Is(v, "status", "ready"), "array.grid");
            await Viewer("audio", snd, "audio", v => Is(v, "drawn", "audio") && (v["peaks"]?.GetValue<int>() ?? 0) > 0 && v["spectrogram"]?.GetValue<bool>() == true);
            // Compare: two pictures pinned (the picture and the array's), side by side
            var pa = await m.RunAsync("view.popout", P(img, "image"));
            var pb = await m.RunAsync("view.popout", P(mb, "escape"));
            await Task.Delay(1500);
            await m.RunAsync("viewer.pin", O(("popout", Jsonv.Of(pa.Get("id").U64() ?? 0)), ("slot", S("a"))));
            await m.RunAsync("viewer.pin", O(("popout", Jsonv.Of(pb.Get("id").U64() ?? 0)), ("slot", S("b"))));
            await m.RunAsync("view.close", O(("id", Jsonv.Of(pa.Get("id").U64() ?? 0))));
            await m.RunAsync("view.close", O(("id", Jsonv.Of(pb.Get("id").U64() ?? 0))));
            await Time("compare", () => m.RunAsync("viewer.compare", O(("on", Jsonv.True), ("how", S("side")))),
                p => (p["coreWindows"]?["compare"]?["pictures"]?.GetValue<int>() ?? 0) >= 2,
                () => m.RunAsync("viewer.compare", O(("on", Jsonv.False))));
            results["open_ms"] = opened;

            // the 100,000-row table: its page scrolled a move a frame, then five pages turned
            var tr = await m.RunAsync("view.popout", P(table, "table"));
            var tid = tr.Get("id").U64() ?? 0;
            var vw = await WaitFor(() => w.PopoutWindows.Open.TryGetValue(tid, out var win) && win.Viewer is TableViewer { List.Items.Count: > 0 } ? win : null);
            if (vw?.Viewer is TableViewer tv)
            {
                await Task.Delay(800);
                var sv = tv.Inner;
                results["table_rows_in_page"] = tv.List.Items.Count;
                if (sv is not null)
                {
                    var range = Math.Max(0, sv.ExtentHeight - sv.ViewportHeight);
                    var scroll = new JsonArray();
                    for (int run = 0; run < 3; run++)
                    {
                        var ph = await Bench.Phase(o.PhaseSeconds, period, u =>
                        {
                            // down the page and back, twice
                            var k = 0.5 - 0.5 * Math.Cos(u * 2 * Math.PI * 2);
                            sv.ChangeView(null, k * range, null, true);
                        });
                        ph["range_px"] = Math.Round(range);
                        scroll.Add(ph);
                    }
                    results["table_scroll"] = scroll;
                }
                var turns = new List<double>();
                for (ulong pg = 1; pg <= 5; pg++)
                {
                    var t = Now;
                    await m.RunAsync("viewer.table", O(("popout", Jsonv.Of(tid)), ("page", Jsonv.Of(pg * 37))));
                    var want = pg * 37;
                    var f = await FrameWhen(() => tv.Describe() is { } d && d["page"]?.GetValue<ulong>() == want && d["status"]?.ToString() == "ready", TimeSpan.FromSeconds(60));
                    if (f is { } at) turns.Add(at - t);
                }
                results["table_page_turn_ms"] = Bench.Stats(turns);
                results["table_page_fetch_ms"] = Math.Round(tv.LastPageMs, 1);
            }
            else results["table_scroll"] = "the table never showed";
        }
        catch (Exception e)
        {
            results["error"] = e.ToString();
            results["open_ms"] = opened;
        }
        var proc = Process.GetCurrentProcess();
        results["memory_mb"] = new JsonObject { ["working_set"] = proc.WorkingSet64 / 1048576, ["private"] = proc.PrivateMemorySize64 / 1048576 };
        File.WriteAllText(o.Bench!, results.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        CoreHost.Status.Set("bench", "done");
        if (o.Exit)
        {
            await Task.Delay(300);
            foreach (var win in CoreHost.Windows.ToList())
                if (CoreHost.Core is { } core) await core.ResultAsync("document.close", Jsonv.Object(("doc", Jsonv.Of(win.Doc)), ("discard", Jsonv.True)));
        }
    }

    static async Task<T?> WaitFor<T>(Func<T?> f) where T : class
    {
        for (int i = 0; i < 600; i++)
        {
            if (f() is { } x) return x;
            await Task.Delay(100);
        }
        return null;
    }
}
