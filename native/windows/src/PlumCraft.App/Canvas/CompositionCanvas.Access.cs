// What UI Automation can see of the canvas (N5 gate): every face on it, with its title, its state and
// footer lines, and each pin, at its place in the view. The canvas draws with Composition, so without
// this a screen reader, or a check that drives the app as a person does, finds one blank pane.
// Nothing here is worked out anew: it reads the faces' layouts the canvas already holds.

using System.Numerics;
using WRect = Windows.Foundation.Rect;

namespace PlumCraft.App.Canvas;

/// <summary>A part of the canvas UI Automation shows: a face (its pins as children) or a pin.</summary>
public sealed record CanvasPart(string Id, string Name, string Role, WRect View, string Status, List<CanvasPart> Children);

public sealed partial class CompositionCanvas
{
    /// <summary>The faces in the view (and each face's pins), in view points.</summary>
    public List<CanvasPart> AccessibleParts()
    {
        var o = new List<CanvasPart>();
        foreach (var e in faces.Values.OrderBy(f => f.Face.Pos.Y).ThenBy(f => f.Face.Pos.X))
        {
            if (e.Layout is not { } L) continue;
            var r = e.Rect;
            var tl = View.ToView(new Vector2((float)r.X, (float)r.Y));
            var view = new WRect(tl.X, tl.Y, r.Width * View.Zoom, r.Height * View.Zoom);
            if (view.X > ViewSize.X || view.Y > ViewSize.Y || view.X + view.Width < 0 || view.Y + view.Height < 0) continue;
            var pins = new List<CanvasPart>();
            WRect Pin(float x, float y)
            {
                var c = View.ToView(new Vector2((float)r.X + x, (float)r.Y + y));
                var h = Math.Max(6, 8 * View.Zoom);
                return new WRect(c.X - h / 2, c.Y - h / 2, h, h);
            }
            for (int k = 0; k < e.Face.Inputs.Count && k < L.InputPins.Length; k++)
            {
                var row = e.Face.Inputs[k];
                pins.Add(new CanvasPart($"in.{e.Nid}.{row.Key}", row.Label, "input", Pin(0, L.InputPins[k]), row.Text ?? (row.Widget is { } wd ? FaceLayout.WidgetText(wd) : ""), []));
            }
            for (int k = 0; k < e.Face.Outputs.Count && k < L.OutputPins.Length; k++)
            {
                var row = e.Face.Outputs[k];
                pins.Add(new CanvasPart($"out.{e.Nid}.{row.Key}", row.Label, "output", Pin(L.W, L.OutputPins[k]), row.Ty, []));
            }
            // a path row's Browse square (B133), a button on its face
            for (int k = 0; k < e.Face.Inputs.Count && k < L.Widgets.Length; k++)
            {
                if (L.Widgets[k] is not { } wr || !PlumCraft.Models.PathBrowse.Offered(e.Face.Inputs[k].Widget)) continue;
                var sq = PlumCraft.Models.PathBrowse.Square(new PlumCraft.Models.Rect(wr.X, wr.Y, wr.X + wr.Width, wr.Y + wr.Height));
                var btl = View.ToView(new Vector2((float)(r.X + sq.X0), (float)(r.Y + sq.Y0)));
                pins.Add(new CanvasPart($"browse.{e.Nid}.{e.Face.Inputs[k].Key}", "Browse", "button", new WRect(btl.X, btl.Y, sq.Width * View.Zoom, sq.Height * View.Zoom), e.Face.Inputs[k].Label, []));
            }
            // the footer's Save Result… line, a button on its face
            foreach (var f in L.Footer.Where(f => f.Kind == "save" && f.Port is not null))
            {
                var ftl = View.ToView(new Vector2((float)(r.X + f.Rect.X), (float)(r.Y + f.Rect.Y)));
                pins.Add(new CanvasPart($"save.{e.Nid}.{f.Port}", "Save Result…", "button", new WRect(ftl.X, ftl.Y, Math.Min(f.Rect.Width, 96) * View.Zoom, f.Rect.Height * View.Zoom), f.Port!, []));
            }
            // a list of named pictures: the face's picture, named by its tag ("caption · n pictures", 8.7)
            if (e.ListTagText is { } tag && L.Preview is { } pr)
            {
                var ptl = View.ToView(new Vector2((float)(r.X + pr.X), (float)(r.Y + pr.Y)));
                pins.Add(new CanvasPart($"picture.{e.Nid}", tag, "picture", new WRect(ptl.X, ptl.Y, pr.Width * View.Zoom, pr.Height * View.Zoom), e.Face.Preview?.Summary ?? "", []));
            }
            var lines = L.Footer.SelectMany(f => f.Lines).ToList();
            var state = e.Face.Header.State.Hover ?? e.Face.Header.State.Kind;
            o.Add(new CanvasPart($"node.{e.Nid}", e.Face.Header.Title.Text, "node", view, string.Join(" | ", lines.Prepend(state)), pins));
        }
        return o;
    }
}
