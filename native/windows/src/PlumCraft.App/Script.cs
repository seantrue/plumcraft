// What the first window does once drawn, for screenshots and checks a script can't make through the
// control port (`--then STEP;STEP`): each step enters where the person's input does (a menu opened as
// UI Automation opens it, a context menu where a right click asks for one, File › Open and the close
// button as their handlers run them), so what is shown is what a person sees. A request over the port
// is a script's, and the app leaves its asks alone (11.1), so these are the person's.
//
//   wait:MS             wait
//   menu:TITLE          open the menu bar's menu TITLE (UI Automation's Expand)
//   context:NODE        the core's context menu for node NODE's header, as a right click there
//   more:NODE           a click on node NODE's more-button
//   open                File › Open (the system's picker)
//   close               the window's close button (the core's app.quit: unsaved changes ask)
//   about               Help › About
//   fit                 View › Fit (the core's canvas.fit)
//   zoom:Z[@NODE]       zoom Z about the view's centre, or centred on NODE
//   run                 Run (the core's run.start, as the toolbar's button)
//   watch:NODE.PORT     the core's run.watch, as a click on that pin would
//
// N5 slice 3 (editing), through the canvas's own entry points (CanvasView.Press, MoveTo, Release: the
// handlers real pointer input reaches), at a place: NODE (its header), NODE.out.K or NODE.in.K (a pin),
// NODE.row.K (an input row's widget), or X,Y (a view point), each with an optional +DX,DY:
//   press:PLACE  move:PLACE  release:PLACE   a mouse's press, move and release
//   click:PLACE  dclick:PLACE                 a click, a double click
//   rclick:PLACE                              a right click (the core's context menu)
//   escape                                    Escape on the canvas (a drag cancelled)
//   type:TEXT                                 the focused text field's text
//   new  quit                                 File › New, File › Exit (the core's graph.new, app.quit)
//   add:KIND@X,Y=ID                           a script's node.add, remembered as ID for the places above
//   wire:A.out.K/B.in.K                       a script's wire.connect between two remembered nodes' ports
using System.Numerics;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using PlumCraft.App.Canvas;
using PlumCraft.Models;

namespace PlumCraft.App;

public static class Script
{
    public static async void Start(DocWindow w)
    {
        var o = CoreHost.Options;
        try
        {
            foreach (var step in o.Then) await Step(w, step);
            CoreHost.Status.Set("then", "done");
            if (o.Bench is not null) await Bench.Run(w);
        }
        catch (Exception e)
        {
            CoreHost.Log($"--then: {e}");
            CoreHost.Status.Set("then", $"failed: {e.Message}");
        }
    }

    static readonly Dictionary<string, string> named = [];

    static string Id(string n) => named.TryGetValue(n, out var id) ? id : n;

    /// <summary>A place in view points (the forms above).</summary>
    static Vector2 Place(DocWindow w, string spec)
    {
        var c = w.CanvasView.Canvas;
        Vector2 extra = Vector2.Zero;
        var plus = spec.IndexOf('+');
        if (plus >= 0)
        {
            var d = spec[(plus + 1)..].Split(',');
            extra = new Vector2(F(d[0]), F(d[1]));
            spec = spec[..plus];
        }
        if (spec.Contains(','))
            return new Vector2(F(spec.Split(',')[0]), F(spec.Split(',')[1])) + extra;
        var parts = spec.Split('.');
        if (parts.Length == 3)
        {
            var node = Id(parts[0]);
            var k = int.Parse(parts[2], System.Globalization.CultureInfo.InvariantCulture);
            Vector2? g = parts[1] switch
            {
                "out" => c.PinPoint(node, true, k),
                "in" => c.PinPoint(node, false, k),
                "row" => c.RowRect(node, k) is { } r ? new Vector2((float)(r.X0 + r.Width / 2), (float)(r.Y0 + r.Height / 2)) : null,
                _ => null,
            };
            return c.View.ToView(g ?? throw new InvalidOperationException($"no place {spec}")) + extra;
        }
        return HeaderOf(w, Id(spec)) + extra;
    }

    static float F(string s) => float.Parse(s, System.Globalization.CultureInfo.InvariantCulture);

    static Vector2 HeaderOf(DocWindow w, string node)
    {
        var c = w.CanvasView.Canvas;
        var r = c.FaceRect(node) ?? throw new InvalidOperationException($"no face {node}");
        return c.View.ToView(new Vector2((float)(r.X + r.Width / 2), (float)r.Y + FaceMetrics.Header / 2));
    }

    static Vector2? lastAt;

    static async Task Step(DocWindow w, string step)
    {
        if (step.StartsWith("press:", StringComparison.Ordinal)) lastAt = Place(w, step[6..]);
        var (verb, arg) = step.IndexOf(':') is var k and >= 0 ? (step[..k], step[(k + 1)..]) : (step, "");
        CoreHost.Status.Set("step", step);
        var c = w.CanvasView.Canvas;
        switch (verb)
        {
            case "wait":
                await Task.Delay(int.TryParse(arg, out var ms) ? ms : 500);
                break;
            case "menu":
                foreach (var item in w.MenuBar.Items)
                    if (item.Title == arg)
                    {
                        var peer = FrameworkElementAutomationPeer.FromElement(item) as MenuBarItemAutomationPeer ?? new MenuBarItemAutomationPeer(item);
                        peer.Expand();
                    }
                break;
            case "context":
                await w.CanvasView.ContextAt(HeaderOf(w, arg) + new Vector2(-20, 0));
                break;
            case "more":
                var r = c.FaceRect(arg) ?? throw new InvalidOperationException($"no face {arg}");
                await w.CanvasView.ClickAt(c.View.ToView(new Vector2((float)(r.X + r.Width - 15), (float)r.Y + FaceMetrics.Header / 2)));
                break;
            case "open":
                _ = w.Dispatch(new MenuItem { Id = "graph.open", Command = "graph.open", Label = "Open…", Enabled = true }, true);
                break;
            case "close":
                _ = w.CloseAsPersonAsync();
                break;
            case "about":
                _ = w.Dispatch(new MenuItem { Id = "app.about", Command = "app.about", Label = "About", Enabled = true }, true);
                break;
            case "fit":
                await w.Dispatch(new MenuItem { Id = "canvas.fit", Command = "canvas.fit", Label = "Fit", Enabled = true }, true);
                break;
            case "run":
                await w.Dispatch(new MenuItem { Id = "run.start", Command = "run.start", Label = "Run", Enabled = true }, true);
                break;
            case "set":
                // a script's edit (node.set), so a close has unsaved changes to ask about: NODE.PORT=VALUE
                var eq = arg.IndexOf('=');
                var np = arg[..eq];
                var pdot = np.LastIndexOf('.');
                await w.Model.RunAsync("node.set", Jsonv.Object(("node", Jsonv.Of(np[..pdot])), ("port", Jsonv.Of(np[(pdot + 1)..])), ("value", Jsonv.Of(arg[(eq + 1)..]))));
                break;
            case "watch":
                var dot = arg.LastIndexOf('.');
                await w.Model.RunAsync("run.watch", Jsonv.Object(("node", Jsonv.Of(arg[..dot])), ("port", Jsonv.Of(arg[(dot + 1)..]))));
                break;
            case "zoom":
                var parts = arg.Split('@');
                var z = float.Parse(parts[0], System.Globalization.CultureInfo.InvariantCulture);
                if (parts.Length > 1 && c.FaceRect(parts[1]) is { } fr)
                {
                    var mid = new Vector2((float)(fr.X + fr.Width / 2), (float)(fr.Y + fr.Height / 2));
                    c.SetView(new Viewport(c.ViewSize / 2 - mid * z, z));
                }
                else c.ZoomTo(z);
                break;
            case "press":
                w.CanvasView.Press(Place(w, arg), [], Microsoft.UI.Input.PointerDeviceType.Mouse, false);
                break;
            case "move":
                {
                    // in a few steps, as a pointer moves
                    var to = Place(w, arg);
                    for (int i = 1; i <= 6; i++)
                    {
                        w.CanvasView.MoveTo(Vector2.Lerp(lastAt ?? to, to, i / 6f));
                        await Task.Delay(16);
                    }
                    lastAt = to;
                    break;
                }
            case "release":
                await w.CanvasView.Release(Place(w, arg));
                break;
            case "click":
                await w.CanvasView.ClickAt(Place(w, arg));
                break;
            case "dclick":
                await w.CanvasView.DoubleClickAt(Place(w, arg));
                break;
            case "rclick":
                await w.CanvasView.ContextAt(Place(w, arg));
                break;
            case "escape":
                await w.CanvasView.CancelDragAsync();
                break;
            case "type":
                if (Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(w.Content.XamlRoot) is TextBox tb) { tb.Text = arg; tb.SelectionStart = arg.Length; }
                else if (Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(w.Content.XamlRoot) is ComboBox cb) cb.IsDropDownOpen = true;
                break;
            case "new":
                await w.Dispatch(new MenuItem { Id = "graph.new", Command = "graph.new", Label = "New", Enabled = true }, true);
                break;
            case "quit":
                await w.Dispatch(new MenuItem { Id = "app.quit", Command = "app.quit", Label = "Exit", Enabled = true }, true);
                break;
            case "add":
                {
                    var (kind, rest) = (arg[..arg.IndexOf('@')], arg[(arg.IndexOf('@') + 1)..]);
                    var name = rest.Contains('=') ? rest[(rest.IndexOf('=') + 1)..] : null;
                    var xy = (name is null ? rest : rest[..rest.IndexOf('=')]).Split(',');
                    var added = await w.Model.RunAsync("node.add", Jsonv.Object(("kind", Jsonv.Of(kind)), ("pos", Jsonv.Array([Jsonv.Of((double)F(xy[0])), Jsonv.Of((double)F(xy[1]))]))));
                    var id = added.Get("id").Str() ?? throw new InvalidOperationException("node.add gave no id");
                    if (name is not null) named[name] = id;
                    // its face drawn and measured, and the canvas idle
                    for (int i = 0; i < 400 && !(w.CanvasView.Canvas.FaceRect(id) is not null && w.CanvasView.Canvas.Idle && w.CanvasView.Canvas.AllMeasured); i++) await Task.Delay(25);
                    break;
                }
            case "wire":
                {
                    var (a, b) = (arg[..arg.IndexOf('/')].Split('.'), arg[(arg.IndexOf('/') + 1)..].Split('.'));
                    var fa = w.Model.FaceOf(Id(a[0]))!;
                    var fb = w.Model.FaceOf(Id(b[0]))!;
                    var pa = fa.Outputs[int.Parse(a[2], System.Globalization.CultureInfo.InvariantCulture)].Key;
                    var pb = fb.Inputs[int.Parse(b[2], System.Globalization.CultureInfo.InvariantCulture)].Key;
                    await w.Model.RunAsync("wire.connect", Jsonv.Object(("from", Jsonv.Object(("node", Jsonv.Of(fa.Id)), ("port", Jsonv.Of(pa)))), ("to", Jsonv.Object(("node", Jsonv.Of(fb.Id)), ("port", Jsonv.Of(pb))))));
                    break;
                }
            default:
                throw new InvalidOperationException($"unknown step {step}");
        }
        w.NoteStatus();
    }
}
