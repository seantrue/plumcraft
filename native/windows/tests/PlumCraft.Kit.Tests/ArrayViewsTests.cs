// An array's view modes and its channels' histograms as `popout.get` gives them (B154 slice 1, contract 13.2.2):
// read into plain values, the bars as the core scaled them, a hostile item skipped, the View picker's words.

using System.Text.Json;
using PlumCraft.Models;
using Xunit;

namespace PlumCraft.Kit.Tests;

public class ArrayViewsTests
{
    static JsonElement J(string s) => JsonDocument.Parse(s).RootElement.Clone();

    [Fact]
    public void AChannelReadsItsRangeNanAndHistogramAsGiven()
    {
        var cs = ArrayViews.Channels(J("""
            [{"name": "escape", "min": "0", "max": "255", "nan": 3,
              "histogram": {"bins": 4, "lo": "0", "hi": "255", "top": 900, "log": true, "counts": [900, 0, 10, 1], "heights": [1.0, 0.0, 0.35, 0.1]}},
             {"name": "b", "min": 0.5, "max": null, "nan": 0}]
            """));
        Assert.Equal(2, cs.Count);
        Assert.Equal("escape: 0 … 255, 3 NaN", cs[0].Line);
        var h = cs[0].Histogram!;
        Assert.Equal(new[] { 1.0, 0.0, 0.35, 0.1 }, h.Heights);
        Assert.Equal(("0", "255", 4L, 900L, true), (h.Lo, h.Hi, h.Bins, h.Top, h.Log));
        // no NaN: nothing said of it; a number as its text; a missing value as a dash
        Assert.Equal("b: 0.5 … –", cs[1].Line);
        Assert.Null(cs[1].Histogram);
    }

    [Fact]
    public void HostileStatsAreSkippedOrHeldNeverThrown()
    {
        Assert.Empty(ArrayViews.Channels(null));
        Assert.Empty(ArrayViews.Channels(J("""{"name": "not a list"}""")));
        var cs = ArrayViews.Channels(J("""[7, "x", {"name": "c", "histogram": {"heights": [2.5, -1, "tall", null, 0.5]}}]"""));
        var c = Assert.Single(cs);
        Assert.Equal(new[] { 1.0, 0.0, 0.0, 0.0, 0.5 }, c.Histogram!.Heights);
        Assert.Equal(("–", "–", 0L, 0L, false), (c.Histogram.Lo, c.Histogram.Hi, c.Histogram.Bins, c.Histogram.Top, c.Histogram.Log));
    }

    [Fact]
    public void TheViewPickersWordsAndTheTurn()
    {
        foreach (var m in new[] { "auto", "color", "stretch", "clip", "heat", "labels", "angle" }) Assert.NotEqual("", ArrayViews.Tip(m));
        Assert.Equal("", ArrayViews.Tip("plaid"));
        Assert.Equal(1, ArrayViews.Turn(null));
        Assert.Equal(360, ArrayViews.Turn(J("""{"turn": 360}""")));
        Assert.Equal(1, ArrayViews.Turn(J("""{"turn": -2}""")));
        Assert.Equal(1, ArrayViews.Turn(J("""{"turn": "x"}""")));
    }

    [Fact]
    public void APopoutDecodesItsViewModes()
    {
        var p = Shell.Decode<PopoutValue>(J("""
            {"id": 3, "node": "n1", "port": "escape", "viewModes": ["auto", "heat", "angle"], "viewMode": {"id": "angle", "params": {"turn": 6.283}}}
            """), Shell.Strict);
        Assert.Equal(new[] { "auto", "heat", "angle" }, p.ViewModes);
        Assert.Equal("angle", p.ViewMode!.Id);
        Assert.Equal(6.283, ArrayViews.Turn(p.ViewMode.Params));
    }
}
