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

    static Vector2 HeaderOf(DocWindow w, string node)
    {
        var c = w.CanvasView.Canvas;
        var r = c.FaceRect(node) ?? throw new InvalidOperationException($"no face {node}");
        return c.View.ToView(new Vector2((float)(r.X + r.Width / 2), (float)r.Y + FaceMetrics.Header / 2));
    }

    static async Task Step(DocWindow w, string step)
    {
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
            default:
                throw new InvalidOperationException($"unknown step {step}");
        }
        w.NoteStatus();
    }
}
