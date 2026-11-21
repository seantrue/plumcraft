// N5 slice 3, editing below the views: the drag driver (a face moved, a wire connected, a wire dropped
// on nothing, a cancel), the palette model, the field editor, a name in place, where an editor goes, the
// zoom command, and a window per document (New, Open, the close prompt's words).

using PlumCraft.Core;
using PlumCraft.Models;
using Xunit;
using static PlumCraft.Kit.Tests.Support;

namespace PlumCraft.Kit.Tests;

public class Editing
{
    /// <summary>Two faces, a literal at (0, 0) and an upper case at (500, 0), measured typed: 180 by 100 and
    /// 160 by 90, the literal's output at 60, the upper's input and output at 44.</summary>
    static async Task<(DocumentModel model, CanvasLink link, string a, string b)> TwoMeasured(PlumCore core)
    {
        var model = await Started(core);
        var a = await Add(model, "text.literal", 0, 0);
        var b = await Add(model, "text.upper", 500, 0);
        Assert.True(await model.WaitAsync(() => model.Faces.Count == 2));
        var fa = model.FaceOf(a)!;
        var fb = model.FaceOf(b)!;
        var link = new CanvasLink(model);
        link.Report([new PlumCore.FaceMeasure(fa.Nid, fa.Rev, 180, 100, 28, [], [60], null), new PlumCore.FaceMeasure(fb.Nid, fb.Rev, 160, 90, 28, [44], [44], null)]);
        bool measured = false;
        for (int i = 0; i < 200 && !measured; i++)
            measured = (await core.RequestAsync<Located>("geometry.locate", O(("node", S(b))))).Measured == true;
        Assert.True(measured);
        return (model, link, a, b);
    }

    [Fact]
    public Task TheDragDriverMovesConnectsDropsAndCancels() => OnContext(async () =>
    {
        using var core = OpenCore("edit-drag");
        var (model, link, a, b) = await TwoMeasured(core);
        var drag = new DragDriver(link);
        var frames = new List<DragFrame>();
        drag.Moved += f => frames.Add(f);
        int snaps = 0;
        drag.Snapped += () => snaps++;

        // a face's header: a move, previewed by its offset, one undo step at the end
        drag.Begin(new Point(520, 10), [], 2, 1);
        drag.Move(new Point(540, 20), 2);
        Assert.True(await model.WaitAsync(() => drag.Kind == DragWord.Move));
        var f = drag.Move(new Point(560, 40), 2);
        Assert.Equal((40.0, 30.0), (f!.Value.Dx, f.Value.Dy));
        var r = await drag.EndAsync(new Point(560, 40), 2);
        Assert.True(DragDriver.Ran(r, "edit.move"), string.Join(",", r!.Commands.Select(c => c.Command)));
        Assert.True(await model.WaitAsync(() => model.FaceOf(b)!.Pos == new Point(540, 30) && model.State.Undo == "Move Node"), $"{model.FaceOf(b)!.Pos} {model.State.Undo}");

        // a wire from the literal's output to the upper's input: the target rings as it fits, then connects
        frames.Clear();
        drag.Begin(new Point(180, 60), [], 4, 1);
        Assert.True(await model.WaitAsync(() => drag.Kind == DragWord.Wire));
        var over = drag.Move(new Point(541, 74), 4);
        Assert.True(over?.Target is { } t && t.Fits == 1, $"{over}");
        Assert.Equal(1, snaps);
        r = await drag.EndAsync(new Point(541, 74), 4);
        Assert.True(DragDriver.Ran(r, "wire.connect"), string.Join(",", r!.Commands.Select(c => $"{c.Command} {c.Error}")));
        Assert.True(await model.WaitAsync(() => model.Wires.Count == 1 && model.State.Undo == "Connect"), model.State.Undo);

        // a new wire from the upper's output dropped on nothing: the palette opens for it, the wire held
        drag.Begin(new Point(700, 74), [], 4, 1);
        Assert.True(await model.WaitAsync(() => drag.Kind == DragWord.Wire));
        drag.Move(new Point(900, 300), 4);
        r = await drag.EndAsync(new Point(900, 300), 4);
        Assert.True(DragDriver.Ran(r, "canvas.palette") && drag.HoldingWire);
        var palette = new PaletteModel(model);
        Assert.True(await model.WaitAsync(() => palette.IsOpen && palette.ForWire && palette.Items.Count > 0), palette.Error);
        Assert.All(palette.Items, it => Assert.NotNull(it.Port));
        palette.Dismiss();
        Assert.True(await model.WaitAsync(() => model.View.Palette is null));
        Assert.True(drag.ReleaseWire());
        Assert.Equal(1, model.Wires.Count);

        // a cancelled move changes nothing
        var undo = model.State.Undo;
        drag.Begin(new Point(20, 10), [], 2, 1);
        Assert.True(await model.WaitAsync(() => drag.Kind == DragWord.Move));
        drag.Move(new Point(200, 200), 2);
        await drag.CancelAsync();
        Assert.Null(link.Drag);
        await core.ResultAsync("ui.inspect");
        Assert.Equal(new Point(0, 0), model.FaceOf(a)!.Pos);
        Assert.Equal(undo, model.State.Undo);

        // a marquee on empty canvas: its rect each move, the faces it touches selected at the end
        drag.Begin(new Point(-40, -40), [], 2, 1);
        Assert.True(await model.WaitAsync(() => drag.Kind == DragWord.Marquee));
        var m = drag.Move(new Point(100, 50), 2);
        Assert.Equal(new Rect(-40, -40, 100, 50), m!.Value.Marquee);
        await drag.EndAsync(new Point(100, 50), 2);
        Assert.True(await model.WaitAsync(() => model.Selection.Nodes.SequenceEqual([a])), string.Join(",", model.Selection.Nodes));
        Assert.Equal("1.50", DragDriver.NumberText(1.5, new Widget { Kind = "number", Blank = false, Clearable = false, Int = false, Choices = [], Decimals = 2 }));
        Assert.Equal("2", DragDriver.NumberText(1.6, new Widget { Kind = "number", Blank = false, Clearable = false, Int = true, Choices = [] }));
    });

    [Fact]
    public Task ThePaletteFollowsTheCoreAndAPickIsOneStep() => OnContext(async () =>
    {
        using var core = OpenCore("edit-palette");
        var model = await Started(core);
        var palette = new PaletteModel(model);
        int opened = 0, closed = 0;
        palette.Opened += _ => opened++;
        palette.Closed += () => closed++;
        await palette.OpenAsync(new Point(120, 80));
        Assert.True(await model.WaitAsync(() => palette.IsOpen && palette.Items.Count > 0), palette.Error);
        Assert.Equal(new Point(120, 80), palette.Palette!.At);
        Assert.DoesNotContain(palette.Items, it => it.Kind is "text.widgets" or "text.slow");
        Assert.Contains("kind", palette.Headline);
        var all = palette.Total;
        palette.Search("upper");
        Assert.True(await model.WaitAsync(() => palette.Items.Count > 0 && palette.Total < all && palette.Items.Any(i => i.Kind == "text.upper")));
        palette.MoveBy(-5);
        Assert.Equal(0, palette.Selected);
        palette.Highlight(palette.Items.FindIndex(i => i.Kind == "text.upper"));
        await palette.ChooseSelectedAsync();
        Assert.True(await model.WaitAsync(() => !palette.IsOpen && model.Faces.Count == 1 && model.State.Undo == "Add Node"), model.State.Undo);
        Assert.Equal("text.upper", model.Faces.Items[0].Kind);
        Assert.Equal(new Point(120, 80), model.Faces.Items[0].Pos);
        Assert.Equal((1, 1), (opened, closed));
        // dismissed: nothing added, the core's palette closed
        await palette.OpenAsync(new Point(0, 0));
        Assert.True(await model.WaitAsync(() => palette.IsOpen));
        palette.Dismiss();
        Assert.True(await model.WaitAsync(() => model.View.Palette is null));
        Assert.Equal(1, model.Faces.Count);
    });

    [Fact]
    public Task TheFieldEditorOpensCommitsMovesAndRefuses() => OnContext(async () =>
    {
        using var core = OpenCore("edit-rows");
        var model = await Started(core);
        var n = await Add(model, "text.widgets", 0, 0);
        Assert.True(await model.WaitAsync(() => model.FaceOf(n) is not null));
        var rows = new RowEditor(model);
        RowEditor.OpenRow? seen = null;
        var notes = new List<(string, bool)>();
        rows.Opened += o => seen = o;
        rows.Noted += (_, t, e) => notes.Add((t, e));
        // text
        await rows.EditAsync(n, "label");
        Assert.True(await model.WaitAsync(() => rows.Open?.Edit.Port == "label"));
        Assert.Equal(WidgetWord.Text, seen!.Kind);
        Assert.Equal("hello", RowEditor.StartText(seen.Widget));
        Assert.True(await rows.CommitTextAsync("corgi"));
        Assert.True(await model.WaitAsync(() => rows.Open is null && model.State.Undo == "Set Value"));
        // a number: the core's text, a refusal keeps the editor open, a value past the range held with a note
        await rows.EditAsync(n, "amount");
        Assert.True(await model.WaitAsync(() => rows.Open?.Edit.Port == "amount"));
        Assert.Equal("0.50", RowEditor.StartText(rows.Open!.Widget));
        Assert.False(await rows.CommitTextAsync("lots"));
        Assert.NotNull(rows.Error);
        Assert.Contains(notes, x => x.Item2);
        Assert.NotNull(rows.Open);
        Assert.True(await rows.CommitTextAsync("7", RowEditor.Then.Next));
        Assert.Contains(notes, x => !x.Item2);
        Assert.True(await model.WaitAsync(() => rows.Open is not null && rows.Open.Edit.Port != "amount"), rows.Open?.Edit.Port);
        // a choice by value, then Escape
        await rows.EditAsync(n, "mode");
        Assert.True(await model.WaitAsync(() => rows.Open?.Edit.Port == "mode"));
        Assert.Equal(3, rows.Open!.Widget.Choices.Count);
        Assert.True(await rows.CommitValueAsync(rows.Open.Widget.Choices[1].Value, RowEditor.Then.Stay));
        Assert.NotNull(rows.Open);
        await rows.CloseAsync();
        Assert.True(await model.WaitAsync(() => rows.Open is null));
        var face = model.FaceOf(n)!;
        Assert.Equal("corgi", face.Inputs.Single(i => i.Key == "label").Widget!.Value.GetString());
        Assert.Equal("lower", face.Inputs.Single(i => i.Key == "mode").Widget!.Value.GetString());
        Assert.Equal(1.0, face.Inputs.Single(i => i.Key == "amount").Widget!.Value.GetDouble());
    });

    [Fact]
    public Task ANameIsEditedInPlace() => OnContext(async () =>
    {
        using var core = OpenCore("edit-name");
        var model = await Started(core);
        var n = await Add(model, "text.literal", 0, 0);
        var names = new NameEditor(model);
        await model.RunAsync("name.edit", O(("node", S(n))));
        Assert.True(await model.WaitAsync(() => names.Open is not null));
        Assert.True(names.Open!.Rect.Width > 0);
        Assert.False(await names.CommitAsync("bad\u0001name"));
        Assert.NotNull(names.Error);
        Assert.True(await names.CommitAsync("Greeting"));
        Assert.True(await model.WaitAsync(() => names.Open is null && model.FaceOf(n)!.Header.Title.Text == "Greeting"), model.FaceOf(n)!.Header.Title.Text);
    });

    [Fact]
    public void AnEditorSitsOverItsRowOrFloatsLegibly()
    {
        var row = new Rect(100, 40, 200, 58);
        var near = EditorPlacement.Place(row, 1, new Point(10, 20), new Point(1000, 800));
        Assert.Equal((110.0, 60.0, 100.0, 11.0, false), (near.X, near.Y, near.W, near.FontSize, near.Floating));
        var far = EditorPlacement.Place(row, 0.25, new Point(300, 300), new Point(1000, 800));
        Assert.True(far.Floating && far.FontSize == 10 && far.W == 170 && Math.Abs(far.X + far.W / 2 - 337.5) < 1e-9, $"{far}");
        var edge = EditorPlacement.Place(new Rect(-50, -50, 50, -32), 1, new Point(0, 0), new Point(400, 300));
        Assert.True(edge.X == 0 && edge.Y == 0, $"{edge}");
    }

    [Fact]
    public Task TheZoomKeysAreACommandTheViewFollows() => OnContext(async () =>
    {
        using var core = OpenCore("edit-zoom");
        var model = await Started(core);
        var r = await model.RunAsync("canvas.zoom", O(("by", Jsonv.Of(1.25))));
        Assert.True(await model.WaitAsync(() => model.View.Zoom?.By == 1.25));
        var seq = model.View.Zoom!.Seq;
        await model.RunAsync("canvas.zoom", O(("to", Jsonv.Of(1.0))));
        Assert.True(await model.WaitAsync(() => model.View.Zoom is { To: 1.0, By: null } z && z.Seq > seq));
        await Assert.ThrowsAsync<PlumException>(() => model.RunAsync("canvas.zoom", O(("by", Jsonv.Of(1.25)), ("to", Jsonv.Of(1.0)))));
        Assert.Equal(1.25, r.Get("by")!.Value.GetDouble());
    });

    [Fact]
    public Task WithAWindowPerDocumentNewIsADocumentAndClosingSaysClose() => OnContext(async () =>
    {
        using var core = OpenCore("edit-windows", ("platform", O(("document_windows", Jsonv.True))));
        var wanted = new List<WindowWanted>();
        var feed = core.Subscribe();
        var reading = Task.Run(async () =>
        {
            await foreach (var b in feed.Reader.ReadAllAsync()) lock (wanted) wanted.AddRange(WindowWanted.Of(b));
        });
        var model = await Started(core);
        await Add(model, "text.literal", 0, 0);
        await model.InvokeAsync("graph.new", person: true);
        Assert.True(await model.WaitAsync(() => { lock (wanted) return wanted.Count == 1; }, TimeSpan.FromSeconds(10)));
        var w = wanted[0];
        Assert.True(w.ForPerson && w.From == model.Doc && w.Path is null && w.Doc != model.Doc, $"{w}");
        Assert.Equal(1, model.Faces.Count);
        // the first's window closing over its unsaved change: a prompt that says close, the person's
        var r = await core.ResultAsync("document.close", O(("doc", Jsonv.Of(model.Doc!.Value)), ("by", S("person"))));
        Assert.Equal(false, r.Get("closing").Bool());
        Assert.True(await model.WaitAsync(() => model.Asks.Exists(a => a.Kind == "prompt")));
        var p = model.Asks.Single(a => a.Kind == "prompt");
        Assert.Equal("close", p.Params.Get("then").Str());
        Assert.Equal("person", p.By);
        await model.AnswerAsync(p, O(("answer", S("discard"))));
        Assert.True(await model.WaitAsync(() => model.Closing));
        feed.Dispose();
        await reading.WaitAsync(TimeSpan.FromSeconds(5));
    });
}
