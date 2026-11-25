// What the kit costs at 500 nodes and 800 wires (the canvas row's size, note 24 section 10): a whole
// scene decoded into the models and applied, and a one-node change from its command to the model.
// Printed for the report, not held to a budget here (the machine is a VM on WARP).

using System.Diagnostics;
using PlumCraft.Models;
using Xunit;
using Xunit.Abstractions;
using static PlumCraft.Kit.Tests.Support;

namespace PlumCraft.Kit.Tests;

public class Measure(ITestOutputHelper output)
{
    static double Ms(long ticks) => ticks * 1000.0 / Stopwatch.Frequency;

    static double Median(List<double> xs)
    {
        xs.Sort();
        return xs.Count == 0 ? double.NaN : xs[xs.Count / 2];
    }

    [Fact]
    public Task AFullSceneAndAOneNodeChangeAt500Nodes() => OnContext(async () =>
    {
        using var core = OpenCore("measure");
        var model = await Started(core);
        await core.ResultAsync("spike.scene", O(("nodes", Jsonv.Of(500L)), ("wires", Jsonv.Of(800L))));
        Assert.True(await model.WaitAsync(() => model.Faces.Count == 500 && model.Wires.Count == 800, TimeSpan.FromSeconds(60)));

        // a whole scene: request to reply (the core's build, the boundary, the parse), then decode and apply
        var reply = new List<double>();
        var decode = new List<double>();
        var apply = new List<double>();
        int bytes = 0;
        for (int i = 0; i < 7; i++)
        {
            var t0 = Stopwatch.GetTimestamp();
            var raw = await core.ResultAsync("scene.get");
            var t1 = Stopwatch.GetTimestamp();
            var s = Shell.Decode<SceneReply>(raw);
            var t2 = Stopwatch.GetTimestamp();
            model.Apply(s);
            var t3 = Stopwatch.GetTimestamp();
            reply.Add(Ms(t1 - t0));
            decode.Add(Ms(t2 - t1));
            apply.Add(Ms(t3 - t2));
            bytes = raw.GetRawText().Length;
        }
        Log(output, $"MEASURE 500 nodes, 800 wires: whole scene {bytes / 1024} KB; scene.get request to reply {Median(reply):F1} ms, decoded into the models {Median(decode):F1} ms, applied {Median(apply):F2} ms (medians of 7)");

        // a one-node change: node.set to the model having its face's new revision (the kit's read-your-writes fetch)
        var lit = model.Faces.Items.First(f => f.Inputs.Any(r => r.Key == "text" && r.SourceWord == RowSourceWord.Own));
        var face = model.Faces.ModelById(lit.Id)!;
        var change = new List<double>();
        for (int i = 0; i < 15; i++)
        {
            var rev = face.Rev;
            var t0 = Stopwatch.GetTimestamp();
            await model.RunAsync("node.set", O(("node", S(lit.Id)), ("port", S("text")), ("value", S($"change {i}"))));
            Assert.True(await model.WaitAsync(() => face.Rev != rev, TimeSpan.FromSeconds(10)));
            change.Add(Ms(Stopwatch.GetTimestamp() - t0));
        }
        Log(output, $"MEASURE 500 nodes: a one-node change, node.set to the model {Median(change):F1} ms p50, {change.Max():F1} ms max (15)");
    });
}
