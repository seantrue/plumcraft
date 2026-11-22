// The budget run (`--bench OUT --nodes N`), measured as the N5 spike measured it (note 24 section 20.3):
// launch to the first frame (the process's start to the first CompositionTarget.Rendering after every
// face was laid out and the visible ones drawn), four 4-second phases of frames with no hand on the
// input (idle, a pan at zoom 1, a pan with the whole graph in view, a zoom between the fit and 1.5),
// and a one-node change to its face drawn: node.set on a literal's text, its delta, the face laid out,
// reported and drawn again on a worker, and the next frame. "Late" is an interval over 1.5 display
// periods. On a machine that draws in software (WARP) the frame numbers compare, they don't judge.

using System.Diagnostics;
using System.Numerics;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml.Media;
using PlumCraft.App.Canvas;
using PlumCraft.Models;

namespace PlumCraft.App;

public static class Bench
{
    static double Now => CoreHost.Clock.Elapsed.TotalMilliseconds;

    internal static Task<double> NextFrame()
    {
        var t = new TaskCompletionSource<double>(TaskCreationOptions.RunContinuationsAsynchronously);
        void On(object? s, object e)
        {
            CompositionTarget.Rendering -= On;
            t.TrySetResult(Now);
        }
        CompositionTarget.Rendering += On;
        return t.Task;
    }

    internal static JsonObject Stats(List<double> xs)
    {
        if (xs.Count == 0) return new JsonObject { ["n"] = 0 };
        var s = xs.OrderBy(x => x).ToList();
        double P(double q) => Math.Round(s[Math.Min(s.Count - 1, (int)Math.Round((s.Count - 1) * q))], 3);
        return new JsonObject { ["n"] = s.Count, ["p50"] = P(0.5), ["p95"] = P(0.95), ["max"] = Math.Round(s[^1], 3) };
    }

    internal static async Task<JsonObject> Phase(double seconds, double period, Action<double> step)
    {
        var intervals = new List<double>();
        var start = Now;
        double last = -1;
        var cpu0 = Process.GetCurrentProcess().TotalProcessorTime.TotalMilliseconds;
        var done = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        void On(object? s, object e)
        {
            var t = Now;
            if (last >= 0) intervals.Add(t - last);
            last = t;
            var u = (t - start) / (seconds * 1000);
            if (u >= 1)
            {
                CompositionTarget.Rendering -= On;
                done.TrySetResult();
                return;
            }
            step(u);
        }
        CompositionTarget.Rendering += On;
        await done.Task;
        var cpu1 = Process.GetCurrentProcess().TotalProcessorTime.TotalMilliseconds;
        return new JsonObject
        {
            ["frames"] = intervals.Count, ["fps"] = Math.Round(intervals.Count / seconds, 1), ["interval_ms"] = Stats(intervals),
            ["late_frames"] = intervals.Count(x => x > period * 1.5),
            ["late_ms_per_s"] = Math.Round(intervals.Where(x => x > period * 1.5).Sum(x => x - period) / seconds, 1),
            ["app_cpu_ms_per_s"] = Math.Round((cpu1 - cpu0) / seconds, 1),
        };
    }

    public static async Task Run(DocWindow w)
    {
        var o = CoreHost.Options;
        var c = w.CanvasView.Canvas;
        var results = new JsonObject();
        var (hz, pw, ph) = Interop.Display();
        var period = hz > 0 ? 1000.0 / hz : 16.7;
        results["display"] = new JsonObject
        {
            ["hz"] = hz, ["pixels"] = $"{pw}x{ph}", ["scale"] = c.DisplayScale,
            ["adapters"] = new JsonArray([.. Interop.Adapters().Select(a => (JsonNode)a!)]),
        };
        var start = Process.GetCurrentProcess().StartTime;
        results["launch_to_first_frame_ms"] = Math.Round((w.FirstFrameWall!.Value - start).TotalMilliseconds, 1);
        results["open_to_first_frame_ms"] = Math.Round(w.FirstFrameMs!.Value - w.OpenedMs, 1);
        results["core_open_ms"] = CoreHost.Status.Get("core_open_ms");
        results["faces"] = c.FaceCount;
        results["wires"] = w.Model.Wires.Count;
        results["first_scene"] = new JsonObject { ["laid"] = c.Counts.FacesLaidOut, ["drawn"] = c.Counts.FacesDrawn, ["batches"] = c.Counts.Batches, ["texts_measured"] = FaceText.Shared.Measured };
        results["draw_on_ui_thread"] = c.DrawOnUiThread;
        await Task.Delay(1000);

        var size = c.ViewSize;
        var b = c.ContentBounds() ?? new Rect(0, 0, 100, 100);
        var centre = new Vector2((float)(b.X0 + b.Width / 2), (float)(b.Y0 + b.Height / 2));
        var fz = Math.Clamp(MathF.Min((size.X - 40) / (float)b.Width, (size.Y - 40) / (float)b.Height), 0.1f, 4);
        var phases = new JsonObject();
        c.SetView(new Viewport(size / 2 - centre, 1));
        await Task.Delay(400);
        phases["idle"] = await Phase(o.PhaseSeconds, period, _ => { });
        phases["pan"] = await Phase(o.PhaseSeconds, period, u =>
        {
            var a = (float)(u * 2 * Math.PI * 2);
            var g = centre + new Vector2(MathF.Sin(a) * 900, MathF.Cos(a) * 450);
            c.SetView(new Viewport(size / 2 - g, 1));
        });
        phases["pan_fit"] = await Phase(o.PhaseSeconds, period, u =>
        {
            var a = (float)(u * 2 * Math.PI * 2);
            c.SetView(new Viewport(size / 2 - centre * fz + new Vector2(MathF.Sin(a) * 120, MathF.Cos(a) * 60), fz));
        });
        phases["zoom"] = await Phase(o.PhaseSeconds, period, u =>
        {
            var k = (float)(0.5 - 0.5 * Math.Cos(u * 2 * Math.PI * 2));
            var z = fz + (1.5f - fz) * k;
            c.SetView(new Viewport(size / 2 - centre * z, z));
        });
        // N5 slice 3: a face dragged and a new wire dragged, through the canvas's own pointer entry points
        // (the handlers real input reaches) a frame at a time; "pointer to screen" is from the move to the
        // next frame the compositor shows
        var face = w.Model.Faces.Items.OrderBy(f => Vector2.Distance(new Vector2((float)f.Pos.X, (float)f.Pos.Y), centre)).FirstOrDefault();
        if (face is not null && c.FaceRect(face.Id) is { } dr)
        {
            var head = new Vector2((float)(dr.X + 30), (float)dr.Y + 10);
            c.SetView(new Viewport(size / 2 - head, 1));
            c.Settle();
            await Task.Delay(500);
            phases["drag_face"] = await DragPhase(w, size / 2, o.PhaseSeconds, period, release: true);
            await w.Model.RunAsync("edit.undo");
            await Task.Delay(500);
            if (w.CanvasView.Canvas.PinPoint(face.Id, true, 0) is { } pin)
            {
                c.SetView(new Viewport(size / 2 - pin, 1));
                await Task.Delay(500);
                phases["drag_wire"] = await DragPhase(w, size / 2, o.PhaseSeconds, period, release: false);
            }
        }
        results["phases"] = phases;

        // a one-node change to its face drawn, at zoom 1 with the face in view
        var lit = w.Model.Faces.Items.FirstOrDefault(f => f.Inputs.Any(r => r.Key == "text" && r.SourceWord == RowSourceWord.Own));
        var change = new List<double>();
        if (lit is not null && c.FaceRect(lit.Id) is { } fr)
        {
            var mid = new Vector2((float)(fr.X + fr.Width / 2), (float)(fr.Y + fr.Height / 2));
            c.SetView(new Viewport(size / 2 - mid, 1));
            c.Settle();
            await Task.Delay(500);
            for (int i = 0; i < 14; i++)
            {
                var before = w.Model.FaceOf(lit.Id)?.Rev ?? 0;
                var t0 = Now;
                await w.Model.RunAsync("node.set", Jsonv.Object(("node", Jsonv.Of(lit.Id)), ("port", Jsonv.Of("text")), ("value", Jsonv.Of($"change {i}"))));
                var drawn = await Wait(w, () => w.Model.FaceOf(lit.Id) is { } f && f.Rev != before && c.Drew(lit.Id, f.Rev), TimeSpan.FromSeconds(3));
                var t1 = await NextFrame();
                if (i > 1 && drawn) change.Add(t1 - t0);
                await Task.Delay(50);
            }
        }
        results["one_node_change_ms"] = Stats(change);
        var p = Process.GetCurrentProcess();
        results["memory_mb"] = new JsonObject { ["working_set"] = p.WorkingSet64 / 1048576, ["private"] = p.PrivateMemorySize64 / 1048576 };
        results["counts"] = new JsonObject { ["laid"] = c.Counts.FacesLaidOut, ["drawn"] = c.Counts.FacesDrawn, ["batches"] = c.Counts.Batches, ["worker_draw_failures"] = c.Counts.WorkerDrawFailures, ["tiles_drawn"] = c.Counts.TilesDrawn, ["tiles"] = c.TileCount, ["tile_mb"] = c.TileBytes / 1048576.0, ["wire_layers"] = CompositionCanvas.WireLayers };
        File.WriteAllText(o.Bench!, results.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        CoreHost.Status.Set("bench", "done");
        if (o.Exit)
        {
            await Task.Delay(300);
            // closed without asking: the bench's graph is a script's, not the person's work
            foreach (var win in CoreHost.Windows.ToList())
                if (CoreHost.Core is { } core) await core.ResultAsync("document.close", Jsonv.Object(("doc", Jsonv.Of(win.Doc)), ("discard", Jsonv.True)));
        }
    }

    /// <summary>A drag from view point `from` along a loop for `seconds`, a move each frame; then let go (one
    /// undo step) or cancelled (Escape). Frames as Phase's, and each move's time to the next frame.</summary>
    static async Task<JsonObject> DragPhase(DocWindow w, Vector2 from, double seconds, double period, bool release)
    {
        var v = w.CanvasView;
        var toScreen = new List<double>();
        double? moved = null;
        void On(object? s, object e)
        {
            if (moved is { } m) { toScreen.Add(Now - m); moved = null; }
        }
        CompositionTarget.Rendering += On;
        v.Press(from, [], Microsoft.UI.Input.PointerDeviceType.Mouse, false);
        Vector2 at = from;
        var r = await Phase(seconds, period, u =>
        {
            var a = (float)(u * 2 * Math.PI);
            at = from + new Vector2(MathF.Sin(a) * 260, (1 - MathF.Cos(a)) * 120);
            var t = Now;
            v.MoveTo(at);
            moved ??= t;
        });
        CompositionTarget.Rendering -= On;
        r["kind"] = v.DragState;
        if (release) await v.Release(at); else await v.CancelDragAsync();
        r["pointer_to_screen_ms"] = Stats(toScreen);
        return r;
    }

    /// <summary>Until `cond` holds, checked as each canvas batch finishes (so the next frame is the one that
    /// shows it), at most `limit`.</summary>
    static async Task<bool> Wait(DocWindow w, Func<bool> cond, TimeSpan limit)
    {
        if (cond()) return true;
        var c = w.CanvasView.Canvas;
        var t = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        void On(IReadOnlySet<ulong> _) { if (cond()) t.TrySetResult(true); }
        c.Drawn += On;
        try { return await Task.WhenAny(t.Task, Task.Delay(limit)) == t.Task; }
        finally { c.Drawn -= On; }
    }
}
