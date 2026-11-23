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
        Assert.Equal("first · 3 pictures", ListNames.Tag(p));
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
