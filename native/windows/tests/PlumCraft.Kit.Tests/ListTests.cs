// A list value as named pictures (contract 8.7): the face picture's tag and the pop-out stepper's names,
// as the Apple shells write them (`LayerCanvas.listTagText`, `ViewerBar.itemNames` and `itemTitle`).

using System.Text.Json;
using PlumCraft.Models;
using Xunit;
using static PlumCraft.Kit.Tests.Support;

namespace PlumCraft.Kit.Tests;

public class ListNamesTests
{
    static JsonElement J(string s) => JsonDocument.Parse(s).RootElement.Clone();

    [Fact]
    public void TheFixturesListPictureIsTaggedByItsCaption()
    {
        var scene = Shell.Decode<Scene>(ReadFixture("crates/editor-core/fixtures/scene/full.json"), Shell.Strict);
        var p = scene.Nodes.Select(f => f.Preview).First(p => p?.Items > 1)!;
        Assert.Equal("first", p.Caption);
        Assert.Equal("first · 3 × 2 pictures", ListNames.Tag(p));
    }

    [Fact]
    public void AListOfListsCountsEachLevelAndStepsInsideARun()
    {
        Assert.Equal("3 × 2 items", ListNames.Tag(new Preview { Kind = "rgba8", Items = 3, Levels = [3, 2] }));
        Assert.Equal("3 items", ListNames.Tag(new Preview { Kind = "rgba8", Items = 3, Levels = [3] }));
        Assert.Equal("run", ListNames.OuterWord([3, 2]));
        Assert.Equal("item", ListNames.OuterWord(null));
        Assert.Equal("run 2 of 3", ListNames.Title([], 1, 3, "run"));
        Assert.Empty(ListNames.Steps(null, [3]));
        var steps = ListNames.Steps([1, 2], [3, 2, 4]);
        Assert.Equal(new[] { "run 2 of 2", "item 3 of 4" }, steps.Select(s => $"{s.Word} {s.At + 1} of {s.Of}"));
        Assert.Equal(new List<long> { 0 }, steps[0].PathTo(0));
        Assert.Equal(new List<long> { 1, 3 }, steps[1].PathTo(3));
        var v = Shell.Decode<PopoutValue>(J("""{"id": 1, "node": "n", "port": "p", "items": 3, "index": 1, "path": [1], "levels": [3, 2]}"""), Shell.Strict);
        Assert.Equal(new List<long> { 1 }, v.Path);
        Assert.Equal(new List<ulong> { 3, 2 }, v.Levels);
    }

    [Fact]
    public void ATagOnlyForAListAndItsCountWithNoCaption()
    {
        Assert.Null(ListNames.Tag(null));
        Assert.Null(ListNames.Tag(new Preview { Kind = "rgba8", Items = 1, Caption = "one" }));
        Assert.Null(ListNames.Tag(new Preview { Kind = "rgba8" }));
        Assert.Equal("11 items", ListNames.Tag(new Preview { Kind = "rgba8", Items = 11 }));
        Assert.Equal("11 items", ListNames.Tag(new Preview { Kind = "rgba8", Items = 11, Caption = "  " }));
        Assert.Equal("Lab · 11 pictures", ListNames.Tag(new Preview { Kind = "rgba8", Items = 11, Caption = " Lab " }));
    }

    [Fact]
    public void TheStepperNamesItemsByTheirSheetLabels()
    {
        var sheet = J("""[{"label": "Lab", "summary": "a"}, {"label": "", "summary": "b"}, {"label": "logit"}, {"summary": "d"}]""");
        var names = ListNames.Names(sheet);
        Assert.Equal(new List<string> { "Lab", "item 2", "logit", "item 4" }, names);
        Assert.Equal("logit · 3 of 4", ListNames.Title(names, 2, 4));
        Assert.Equal("Lab · 1 of 4", ListNames.Title(names, 0, 4));
        // an index past the names the sheet gave (it describes at most 256)
        Assert.Equal("item 300 · 300 of 400", ListNames.Title(names, 299, 400));
    }

    [Fact]
    public void AListWithNoNamesSaysItemKOfN()
    {
        Assert.Empty(ListNames.Names(null));
        Assert.Empty(ListNames.Names(J("""[{"label": ""}, {"summary": "x"}]""")));
        Assert.Empty(ListNames.Names(J("""{"label": "not a list"}""")));
        Assert.Equal(new List<string> { "item 1", "b" }, ListNames.Names(J("""[7, {"label": "b"}]""")));
        Assert.Equal("item 5 of 11", ListNames.Title([], 4, 11));
    }
}
