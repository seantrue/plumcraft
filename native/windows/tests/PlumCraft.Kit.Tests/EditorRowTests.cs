// A row that opens its node's value editor (FR1, contract 8.4 and 8.13): the scene's `editor` on an input row
// decodes, with the words the row's link shows; the row has no widget, so the shell reports its caption.

using PlumCraft.Models;
using Xunit;
using static PlumCraft.Kit.Tests.Support;

namespace PlumCraft.Kit.Tests;

public class EditorRowTests
{
    [Fact]
    public void TheFixturesEditorRowDecodesWithItsLink()
    {
        var scene = Shell.Decode<Scene>(ReadFixture("crates/editor-core/fixtures/scene/full.json"), Shell.Strict);
        var rows = scene.Nodes.SelectMany(f => f.Inputs).Where(r => r.Editor is not null).ToList();
        var r = Assert.Single(rows);
        Assert.Equal("curve", r.Key);
        Assert.Null(r.Widget);
        Assert.Equal("test.dial", r.Editor!.Kind);
        Assert.Equal("Edit Dial…", r.Editor.Label);
        Assert.Equal("Edit the dial in its window", r.Editor.Hover);
    }
}
