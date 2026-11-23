// A path row's Browse button (B133): which widgets offer it, its square (as `geometry::browse_rect`), the field
// editor short of it, and what the system dialog is given (its filters and where it starts).

using System.Text.Json;
using PlumCraft.Models;
using Xunit;

namespace PlumCraft.Kit.Tests;

public class PathBrowseTests
{
    static Widget W(string kind, string? mode) => new()
    {
        Kind = kind, Value = JsonDocument.Parse("\"\"").RootElement.Clone(), Default = JsonDocument.Parse("\"\"").RootElement.Clone(),
        Blank = true, Clearable = false, Int = false, Choices = [], PathMode = mode,
    };

    [Fact]
    public void OnlyAPathWidgetWithAModeOffersBrowse()
    {
        foreach (var m in new[] { "open", "save", "dir", "files" }) Assert.True(PathBrowse.Offered(W("path", m)), m);
        Assert.False(PathBrowse.Offered(W("path", null)));
        Assert.False(PathBrowse.Offered(W("path", "sideways")));
        Assert.False(PathBrowse.Offered(W("text", "open")));
        Assert.False(PathBrowse.Offered(null));
    }

    [Fact]
    public void TheSquareIsAtTheRightEndItsSideTheHeightAtMostHalfTheWidth()
    {
        // the core's own numbers (geometry::tests::a_path_rows_browse_button_is_the_square_at_its_right_end)
        Assert.Equal(new Rect(232, 34, 250, 52), PathBrowse.Square(new Rect(100, 34, 250, 52)));
        // a narrow field: half its width
        Assert.Equal(new Rect(10, 0, 20, 18), PathBrowse.Square(new Rect(0, 0, 20, 18)));
        Assert.Equal(new Rect(100, 34, 232, 52), PathBrowse.Field(new Rect(100, 34, 250, 52)));
    }

    [Fact]
    public void TheKindsAreTheFiltersThenAllFiles()
    {
        var f = PathBrowse.Filters([new PathBrowse.Kind("Text", ["txt", ".md"]), new PathBrowse.Kind("", ["csv"]), new PathBrowse.Kind("Any", ["*"])]);
        Assert.Equal(("Text (*.txt, *.md)", "*.txt;*.md"), f[0]);
        Assert.Equal(("*.csv (*.csv)", "*.csv"), f[1]);
        Assert.Equal(("All files (*.*)", "*.*"), f[^1]);
        Assert.Equal(3, f.Length);
        Assert.Single(PathBrowse.Filters([]));
    }

    [Fact]
    public void TheDialogStartsWhereTheValueIsElseWhereTheCoreSays()
    {
        var root = Path.Combine(Support.Scratch("browse"), Guid.NewGuid().ToString("N"));
        var sub = Directory.CreateDirectory(Path.Combine(root, "sub")).FullName;
        try
        {
            var file = Path.Combine(sub, "notes.txt");
            File.WriteAllText(file, "x");
            Assert.Equal(sub, PathBrowse.StartFolder(file, root, false));
            // a file not there yet (a save): its folder
            Assert.Equal(sub, PathBrowse.StartFolder(Path.Combine(sub, "new.txt"), root, false));
            // a folder value: itself for a folder pick, its parent for a file
            Assert.Equal(sub, PathBrowse.StartFolder(sub, null, true));
            Assert.Equal(root, PathBrowse.StartFolder(sub, null, false));
            // several files: the first that resolves
            Assert.Equal(sub, PathBrowse.StartFolder("relative.txt\n" + file, null, false));
            // nothing usable: startIn, then nothing
            Assert.Equal(root, PathBrowse.StartFolder("", root, false));
            Assert.Equal(root, PathBrowse.StartFolder(Path.Combine(root, "gone", "x.txt"), root, false));
            Assert.Null(PathBrowse.StartFolder("relative.txt", Path.Combine(root, "gone"), false));
        }
        finally { Directory.Delete(root, true); }
    }
}
