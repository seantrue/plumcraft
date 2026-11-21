// The spike's window: opens a core (text engine in-process, optionally its control port), builds a
// spike.scene of N nodes, follows the scene by delta, lays out and measures each changed face with
// DirectWrite and reports it typed (plum_geometry_set), and draws it with one of three renderers.
// With --bench it drives pan, zoom and a face drag with no hand on the input and writes timings.
//
//   PlumCraft.CanvasSpike.exe --root DIR [--mode win2d|composition|xaml] [--nodes 500] [--wires N]
//       [--bench out.json] [--phase-seconds 4] [--control-port 0] [--token-file F] [--status F]
//       [--input-log F] [--exit]
//
// Input is a probe (note 24 section 20.6): mouse wheel pans, Ctrl+wheel zooms (the view is the
// shell's), and a press, drag and release on the canvas go to the core as ui.gesture (tap, drag
// begin, plum_gesture_move, end), so the core decides what they are. Nothing here edits the
// document itself.

using System.Diagnostics;
using System.Numerics;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using PlumCraft.Core;

namespace PlumCraft.CanvasSpike;

public sealed class Options
{
    public string Root = Path.Combine(Path.GetTempPath(), "plumcraft-n5-canvas");
    public string Mode = "win2d";
    public int Nodes = 500;
    public int Wires = -1;
    public string? Bench;
    public double PhaseSeconds = 4;
    public int? ControlPort;
    public string? TokenFile;
    public string? Status;
    public string? InputLog;
    public bool Exit;
    /// <summary>A view to show after the fit: zoom and the graph point at the window's top left.</summary>
    public float[]? View;
    /// <summary>At the screen's top left and above other windows (for screenshots on a shared desktop).</summary>
    public bool Top;

    public static Options Parse(string[] a)
    {
        var o = new Options();
        for (int i = 1; i < a.Length; i++)
        {
            string Next() => i + 1 < a.Length ? a[++i] : "";
            switch (a[i])
            {
                case "--root": o.Root = Next(); break;
                case "--mode": o.Mode = Next(); break;
                case "--nodes": o.Nodes = int.Parse(Next()); break;
                case "--wires": o.Wires = int.Parse(Next()); break;
                case "--bench": o.Bench = Next(); break;
                case "--phase-seconds": o.PhaseSeconds = double.Parse(Next(), System.Globalization.CultureInfo.InvariantCulture); break;
                case "--control-port": o.ControlPort = int.Parse(Next()); break;
                case "--token-file": o.TokenFile = Next(); break;
                case "--status": o.Status = Next(); break;
                case "--input-log": o.InputLog = Next(); break;
                case "--exit": o.Exit = true; break;
                case "--top": o.Top = true; break;
                case "--view": o.View = [.. Next().Split(',').Select(x => float.Parse(x, System.Globalization.CultureInfo.InvariantCulture))]; break;
            }
        }
        if (o.Wires < 0) o.Wires = o.Nodes * 8 / 5;
        return o;
    }
}

public sealed class MainWindow : Window
{
    readonly Options opt;
    readonly Grid rootGrid = new();
    readonly TextBlock hud = new() { Margin = new Thickness(8), IsHitTestVisible = false, FontSize = 12 };
    readonly Text text = new();
    readonly SceneModel scene = new();
    ICanvasRenderer renderer = null!;
    PlumCore core = null!;
    float zoom = 1;
    Vector2 pan = new(40, 40);
    bool refreshing, again;
    readonly JsonObject results = new();
    readonly Stopwatch clock = Stopwatch.StartNew();
    double opened;
    bool firstFrameSeen;
    // frame timing
    double lastFrame = -1;
    List<double>? intervals;
    List<double>? work;
    Action<double>? tick;
    TaskCompletionSource<double>? nextFrame;
    StreamWriter? inputLog;

    public MainWindow(Options o)
    {
        opt = o;
        Title = $"PlumCraft N5 spike ({o.Mode}, {o.Nodes} nodes)";
        AppWindow.Resize(new Windows.Graphics.SizeInt32 { Width = 1200, Height = 800 });
        Content = rootGrid;
        if (o.Top)
        {
            AppWindow.Move(new Windows.Graphics.PointInt32 { X = 0, Y = 0 });
            if (AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter op) op.IsAlwaysOnTop = true;
        }
        rootGrid.Background = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 250, 248, 244));
        if (o.InputLog != null) inputLog = new StreamWriter(o.InputLog) { AutoFlush = true };
        CompositionTarget.Rendering += OnRendering;
        rootGrid.Loaded += async (_, _) => await Start();
        Closed += (_, _) => { CompositionTarget.Rendering -= OnRendering; core?.Dispose(); inputLog?.Dispose(); };
    }

    double Now => clock.Elapsed.TotalMilliseconds;

    void OnRendering(object? sender, object e)
    {
        var t0 = Now;
        if (lastFrame >= 0) intervals?.Add(t0 - lastFrame);
        lastFrame = t0;
        tick?.Invoke(t0);
        work?.Add(Now - t0);
        var nf = nextFrame;
        if (nf != null) { nextFrame = null; nf.TrySetResult(t0); }
    }

    Task<double> NextFrame()
    {
        var t = new TaskCompletionSource<double>(TaskCreationOptions.RunContinuationsAsynchronously);
        nextFrame = t;
        return t.Task;
    }

    async Task Start()
    {
        var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        var (hz, w, h) = Interop.Display();
        var (dwmHr, dwm) = Interop.DwmTiming();
        results["display"] = new JsonObject
        {
            ["enum_display_settings_hz"] = hz, ["pixels"] = $"{w}x{h}", ["dpi"] = Interop.Dpi(hwnd),
            ["dwm_hr"] = dwmHr, ["dwm_rate"] = dwm.rateRefreshDen == 0 ? 0 : (double)dwm.rateRefreshNum / dwm.rateRefreshDen,
            ["dwm_period_qpc"] = dwm.qpcRefreshPeriod, ["qpc_frequency"] = Stopwatch.Frequency,
            ["adapters"] = new JsonArray([.. Interop.Adapters().Select(a => (JsonNode)a!)]),
        };
        renderer = opt.Mode switch
        {
            "composition" => new CompositionRenderer(text),
            "xaml" => new XamlRenderer(),
            _ => new Win2DRenderer(text),
        };
        rootGrid.Children.Add(renderer.Element);
        rootGrid.Children.Add(hud);
        HookInput();

        var cfg = new ShellConfig { Root = opt.Root, ControlPort = opt.ControlPort, TokenFile = opt.TokenFile, FrameMs = hz > 0 ? 1000.0 / hz : 16.7 };
        cfg.Prepare();
        var t0 = Now;
        core = new PlumCore(cfg.ToJson());
        core.Feed += OnFeed;
        scene.SetTheme(await core.CallAsync("theme.get"));
        opened = Now - t0;
        if (opt.Nodes > 0) await core.CallAsync("spike.scene", new JsonObject { ["nodes"] = opt.Nodes, ["wires"] = opt.Wires });
        await Refresh();
        await NextFrame();
        var first = Now;
        firstFrameSeen = true;
        var start = Process.GetCurrentProcess().StartTime;
        results["launch_to_first_frame_ms"] = (DateTime.Now - start).TotalMilliseconds - (Now - first);
        results["open_to_first_frame_ms"] = first - t0;
        results["core_open_and_theme_ms"] = opened;
        Fit();
        if (opt.View is { Length: 3 } v)
        {
            zoom = v[0];
            pan = new Vector2(-v[1] * zoom, -v[2] * zoom);
            renderer.SetView(zoom, pan);
        }
        if (opt.ControlPort != null)
        {
            var c = await core.CallAsync("core.control");
            WriteStatus(c);
        }
        else WriteStatus(null);
        if (opt.Bench != null) await RunBench();
    }

    void WriteStatus(JsonElement? control)
    {
        if (opt.Status is null) return;
        var o = new JsonObject
        {
            ["pid"] = Environment.ProcessId, ["mode"] = renderer.Name, ["ready"] = true, ["faces"] = scene.Faces.Count,
            ["control"] = control is { } c ? JsonNode.Parse(c.GetRawText()) : null,
        };
        File.WriteAllText(opt.Status, o.ToJsonString());
    }

    void OnFeed(JsonElement poll)
    {
        if (!poll.TryGetProperty("changed", out var ch) || ch.ValueKind != JsonValueKind.Array) return;
        foreach (var c in ch.EnumerateArray())
            if (c.TryGetProperty("domain", out var d) && d.GetString() == "scene")
            {
                DispatcherQueue.TryEnqueue(() => _ = Refresh());
                return;
            }
    }

    /// <summary>scene.get since the last generation; lay out, measure and report changed faces; draw.</summary>
    async Task Refresh()
    {
        if (refreshing) { again = true; return; }
        refreshing = true;
        try
        {
            do
            {
                again = false;
                var p = scene.Faces.Count == 0 && scene.Gen == 0 ? new JsonObject() : new JsonObject { ["since"] = scene.Gen };
                var r = await core.CallAsync("scene.get", p);
                var t0 = Now;
                var (faces, wires, goneF, goneW) = scene.Apply(r);
                var t1 = Now;
                var measures = new List<PlumCore.FaceMeasure>();
                var parts = new JsonArray();
                foreach (var f in faces)
                {
                    f.Layout = FacePlan.Lay(f, text);
                    var l = f.Layout;
                    measures.Add(new PlumCore.FaceMeasure(f.Nid, f.Rev, l.W, l.H, M.Header, l.InPins, l.OutPins, l.Widgets));
                    parts.Add(new JsonObject { ["node"] = f.Id, ["parts"] = new JsonObject
                    {
                        ["title"] = new JsonArray(l.TitleX - 2, 4, l.TitleX + Math.Min(text.Width(f.Title, text.Title), M.TitleMax) + 2, M.Header - 4),
                        // the more-button (the node's menu), drawn by FacePlan at the header's right end
                        ["menu"] = new JsonArray(l.W - 24, 5, l.W - 6, M.Header - 5),
                    } });
                }
                var t2 = Now;
                if (measures.Count > 0)
                {
                    var st = core.GeometrySet(0, measures);
                    if (st != PlumStatus.Ok) Log($"plum_geometry_set: {PlumStatus.Word(st)}");
                    _ = core.RunAsync("geometry.parts", new JsonObject { ["faces"] = parts }).ContinueWith(t => { if (t.IsFaulted) Log($"geometry.parts: {t.Exception?.GetBaseException().Message}"); }, TaskScheduler.Default);
                }
                var t3 = Now;
                renderer.Update(scene, faces, wires, goneF, goneW);
                var t4 = Now;
                var key = !firstFrameSeen && !results.ContainsKey("refresh_first") ? "refresh_first" : "refresh_later";
                if (faces.Count > 0 && (key == "refresh_first" || faces.Count > 50))
                    results[key] = new JsonObject
                    {
                        ["faces"] = faces.Count, ["wires"] = wires.Count, ["apply_ms"] = t1 - t0, ["layout_ms"] = t2 - t1,
                        ["geometry_set_ms"] = t3 - t2, ["renderer_update_ms"] = t4 - t3, ["texts_measured"] = text.Measured,
                        ["measure_ms"] = text.MeasureTicks * 1000.0 / Stopwatch.Frequency,
                    };
                if (scene.FitRect != null && faces.Count > 0 && !firstFrameSeen) Fit();
                hud.Text = $"{renderer.Name} | {scene.Faces.Count} faces, {scene.Wires.Count} wires | gen {scene.Gen}";
            } while (again);
        }
        catch (Exception e)
        {
            Log($"refresh: {e.Message}");
        }
        finally { refreshing = false; }
    }

    void Log(string s)
    {
        Debug.WriteLine(s);
        var log = Path.Combine(opt.Root, "spike.log");
        try { File.AppendAllText(log, $"{DateTime.Now:O} {s}\n"); } catch (IOException) { }
    }

    (float x0, float y0, float x1, float y1) GraphBounds()
    {
        float x0 = float.MaxValue, y0 = float.MaxValue, x1 = float.MinValue, y1 = float.MinValue;
        foreach (var f in scene.Faces.Values)
        {
            var l = f.Layout;
            x0 = Math.Min(x0, f.Pos.X); y0 = Math.Min(y0, f.Pos.Y);
            x1 = Math.Max(x1, f.Pos.X + (l?.W ?? 220)); y1 = Math.Max(y1, f.Pos.Y + (l?.H ?? 160));
        }
        return scene.Faces.Count == 0 ? (0, 0, 100, 100) : (x0, y0, x1, y1);
    }

    float FitZoom()
    {
        var (x0, y0, x1, y1) = GraphBounds();
        var w = (float)rootGrid.ActualWidth; var h = (float)rootGrid.ActualHeight;
        return Math.Clamp(Math.Min((w - 40) / (x1 - x0), (h - 40) / (y1 - y0)), 0.02f, 4);
    }

    void Fit()
    {
        var (x0, y0, x1, y1) = GraphBounds();
        zoom = FitZoom();
        var w = (float)rootGrid.ActualWidth; var h = (float)rootGrid.ActualHeight;
        pan = new Vector2(w / 2 - (x0 + x1) / 2 * zoom, h / 2 - (y0 + y1) / 2 * zoom);
        renderer.SetView(zoom, pan);
    }

    Vector2 ToGraph(Windows.Foundation.Point p) => (new Vector2((float)p.X, (float)p.Y) - pan) / zoom;

    // ---------------- the bench ----------------

    async Task<JsonObject> Phase(string name, Action<double> step)
    {
        var dwm0 = Interop.DwmTiming().info;
        var cpu0 = Cpu();
        intervals = []; work = [];
        var start = Now;
        var dur = opt.PhaseSeconds * 1000;
        var done = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var draws0 = renderer.DrawCount;
        var seen = draws0;
        var drawMs = new List<double>();
        tick = t =>
        {
            var u = (t - start) / dur;
            if (u >= 1) { tick = null; done.TrySetResult(); return; }
            if (renderer.DrawCount > seen) { seen = renderer.DrawCount; drawMs.Add(renderer.LastDrawMs); }
            step(u);
        };
        await done.Task;
        var dwm1 = Interop.DwmTiming().info;
        var cpu1 = Cpu();
        var iv = intervals; var wk = work;
        intervals = null; work = null;
        var (hz, _, _) = Interop.Display();
        var period = hz > 0 ? 1000.0 / hz : 16.7;
        var late = iv.Count(x => x > period * 1.5);
        var o = new JsonObject
        {
            ["frames"] = iv.Count, ["fps"] = iv.Count / opt.PhaseSeconds, ["interval_ms"] = Stats(iv), ["late_frames"] = late,
            ["late_ms_per_s"] = iv.Where(x => x > period * 1.5).Sum(x => x - period) / opt.PhaseSeconds,
            ["ui_work_ms"] = Stats(wk), ["draws"] = renderer.DrawCount - draws0, ["draw_ms"] = Stats(drawMs),
            ["dwm_frames"] = (long)(dwm1.cFrame - dwm0.cFrame), ["dwm_refreshes"] = (long)(dwm1.cRefresh - dwm0.cRefresh),
            ["dwm_dropped"] = (long)(dwm1.cFramesDropped - dwm0.cFramesDropped), ["dwm_missed"] = (long)(dwm1.cFramesMissed - dwm0.cFramesMissed),
            // CPU a second of wall time: this process (UI thread, Win2D and WARP's rasteriser threads) and dwm.exe
            // (the compositor, rasterising on WARP too); -1 when Windows refused to say
            ["app_cpu_ms_per_s"] = Math.Round((cpu1.app - cpu0.app) / opt.PhaseSeconds, 1),
            ["dwm_cpu_ms_per_s"] = cpu0.dwm < 0 || cpu1.dwm < 0 ? -1 : Math.Round((cpu1.dwm - cpu0.dwm) / opt.PhaseSeconds, 1),
        };
        Log($"phase {name}: {o.ToJsonString()}");
        return o;
    }

    static (double app, double dwm) Cpu()
    {
        double app = Process.GetCurrentProcess().TotalProcessorTime.TotalMilliseconds, dwm = 0;
        try { foreach (var p in Process.GetProcessesByName("dwm")) dwm += p.TotalProcessorTime.TotalMilliseconds; }
        catch (Exception) { dwm = -1; }
        return (app, dwm);
    }

    static JsonObject Stats(List<double> xs)
    {
        if (xs.Count == 0) return new JsonObject { ["n"] = 0 };
        var s = xs.OrderBy(x => x).ToList();
        double P(double q) => Math.Round(s[Math.Min(s.Count - 1, (int)Math.Round((s.Count - 1) * q))], 3);
        return new JsonObject { ["n"] = s.Count, ["p50"] = P(0.5), ["p95"] = P(0.95), ["p99"] = P(0.99), ["max"] = Math.Round(s[^1], 3) };
    }

    async Task RunBench()
    {
        await Task.Delay(1000);
        var phases = new JsonObject();
        var w = (float)rootGrid.ActualWidth; var h = (float)rootGrid.ActualHeight;
        var (x0, y0, x1, y1) = GraphBounds();
        var center = new Vector2((x0 + x1) / 2, (y0 + y1) / 2);
        // nothing changing: the cadence of CompositionTarget.Rendering on this machine
        zoom = 1;
        pan = new Vector2(w / 2, h / 2) - center;
        renderer.SetView(zoom, pan);
        phases["idle"] = await Phase("idle", _ => { });
        if (scene.Faces.Count == 0)
        {
            results["phases"] = phases;
            File.WriteAllText(opt.Bench!, results.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
            if (opt.Exit) { await Task.Delay(300); Close(); Application.Current.Exit(); }
            return;
        }
        // pan at zoom 1 over the middle of the graph
        phases["pan"] = await Phase("pan", u =>
        {
            zoom = 1;
            var a = (float)(u * 2 * Math.PI * 2);
            var c = center + new Vector2(MathF.Sin(a) * 900, MathF.Cos(a) * 450);
            pan = new Vector2(w / 2, h / 2) - c;
            renderer.SetView(zoom, pan);
        });
        // pan with the whole graph in view
        var fz = FitZoom();
        phases["pan_fit"] = await Phase("pan_fit", u =>
        {
            zoom = fz;
            var a = (float)(u * 2 * Math.PI * 2);
            pan = new Vector2(w / 2, h / 2) - center * fz + new Vector2(MathF.Sin(a) * 120, MathF.Cos(a) * 60);
            renderer.SetView(zoom, pan);
        });
        // zoom between the fit and 1.5 about the middle of the window
        phases["zoom"] = await Phase("zoom", u =>
        {
            var k = (float)(0.5 - 0.5 * Math.Cos(u * 2 * Math.PI * 2));
            zoom = fz + (1.5f - fz) * k;
            pan = new Vector2(w / 2, h / 2) - center * zoom;
            renderer.SetView(zoom, pan);
        });
        // drag the face nearest the middle, at zoom 1, through the core (ui.gesture and plum_gesture_move)
        var face = scene.Faces.Values.Where(f => f.Layout != null).OrderBy(f => Vector2.Distance(f.Pos, center)).First();
        zoom = 1;
        pan = new Vector2(w / 2, h / 2) - face.Pos;
        renderer.SetView(zoom, pan);
        var grab = face.Pos + new Vector2(face.Layout!.W / 2, M.Header / 2);
        var begin = await core.CallAsync("ui.gesture", new JsonObject { ["kind"] = "drag", ["phase"] = "begin", ["at"] = new JsonArray(grab.X, grab.Y) });
        var moving = new List<ulong>();
        if (begin.TryGetProperty("drag", out var dr) && dr.TryGetProperty("faces", out var fl))
            foreach (var x in fl.EnumerateArray()) if (x.TryGetUInt64(out var nid)) moving.Add(nid);
        if (moving.Count == 0) moving.Add(face.Nid);
        var buf = new float[4096];
        var hot = new List<double>();
        phases["drag"] = await Phase("drag", u =>
        {
            var a = (float)(u * 2 * Math.PI * 2);
            var at = grab + new Vector2(MathF.Sin(a) * 300, (1 - MathF.Cos(a)) * 150);
            var t0 = Now;
            var d = core.GestureMove(0, at.X, at.Y, 12);
            var n = core.WirePaths(0, moving.ToArray(), d.Dx, d.Dy, ref buf);
            var paths = PlumCore.DecodePaths(buf, n);
            hot.Add(Now - t0);
            renderer.Drag(scene, moving, new Vector2(d.Dx, d.Dy), paths);
        });
        ((JsonObject)phases["drag"]!)["hot_calls_ms"] = Stats(hot);
        // let go 60 units right of where it began: one edit.move, one undo step, then undone
        var end = await core.CallAsync("ui.gesture", new JsonObject { ["kind"] = "drag", ["phase"] = "end", ["at"] = new JsonArray(grab.X + 60, grab.Y) });
        var undo = await core.RequestAsync("command.run", new JsonObject { ["command"] = "edit.undo" });
        ((JsonObject)phases["drag"]!)["undo_ok"] = undo.TryGetProperty("ok", out var uok) && uok.ValueKind == JsonValueKind.True;
        ((JsonObject)phases["drag"]!)["end_commands"] = JsonNode.Parse(end.TryGetProperty("commands", out var cm) ? cm.GetRawText() : "[]");
        results["phases"] = phases;

        // a one-node change to its face drawn: node.set on the literal, scene.get since, layout, report, draw
        var lit = scene.Faces.Values.FirstOrDefault(f => f.Inputs.Any(r => r.Key == "text" && r.Source == "own"));
        var change = new List<double>();
        if (lit != null)
        {
            for (int i = 0; i < 12; i++)
            {
                var t0 = Now;
                await core.RunAsync("node.set", new JsonObject { ["node"] = lit.Id, ["port"] = "text", ["value"] = $"change {i}" });
                await Refresh();
                await NextFrame();
                if (i > 1) change.Add(Now - t0);
            }
        }
        results["one_node_change_ms"] = Stats(change);
        var p = Process.GetCurrentProcess();
        results["memory_mb"] = new JsonObject { ["working_set"] = p.WorkingSet64 / 1048576, ["private"] = p.PrivateMemorySize64 / 1048576 };
        results["mode"] = renderer.Name;
        results["nodes"] = scene.Faces.Count;
        results["wires"] = scene.Wires.Count;
        results["parse_ms_total"] = core.ParseTicks * 1000.0 / Stopwatch.Frequency;
        File.WriteAllText(opt.Bench!, results.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        Fit();
        if (opt.Exit) { await Task.Delay(300); Close(); Application.Current.Exit(); }
    }

    // ---------------- input (a probe) ----------------

    ulong dragFaceNid;
    List<ulong> dragFaces = [];
    bool dragging;
    Windows.Foundation.Point pressAt;

    void HookInput()
    {
        var el = rootGrid;
        el.PointerPressed += (s, e) => { LogPointer("pressed", e); OnPress(e); };
        el.PointerMoved += (s, e) => { LogPointer("moved", e); OnMove(e); };
        el.PointerReleased += (s, e) => { LogPointer("released", e); OnRelease(e); };
        el.PointerWheelChanged += (s, e) => { LogPointer("wheel", e); OnWheel(e); };
        el.PointerCanceled += (s, e) => LogPointer("canceled", e);
        el.PointerCaptureLost += (s, e) => LogPointer("captureLost", e);
        el.ManipulationMode = ManipulationModes.None;
    }

    void LogPointer(string what, PointerRoutedEventArgs e)
    {
        if (inputLog is null) return;
        var pt = e.GetCurrentPoint(rootGrid);
        var pp = pt.Properties;
        inputLog.WriteLine(JsonSerializer.Serialize(new
        {
            t = Math.Round(Now, 2), what, device = e.Pointer.PointerDeviceType.ToString(), id = e.Pointer.PointerId, x = pt.Position.X, y = pt.Position.Y,
            inContact = pt.IsInContact, left = pp.IsLeftButtonPressed, right = pp.IsRightButtonPressed, middle = pp.IsMiddleButtonPressed,
            wheel = pp.MouseWheelDelta, horizontal = pp.IsHorizontalMouseWheel, pressure = pp.Pressure, xTilt = pp.XTilt, yTilt = pp.YTilt,
            barrel = pp.IsBarrelButtonPressed, eraser = pp.IsEraser, inverted = pp.IsInverted, mods = e.KeyModifiers.ToString(),
        }));
    }

    async void OnPress(PointerRoutedEventArgs e)
    {
        pressAt = e.GetCurrentPoint(rootGrid).Position;
        rootGrid.CapturePointer(e.Pointer);
        var g = ToGraph(pressAt);
        var r = await core.RequestAsync("ui.gesture", new JsonObject { ["kind"] = "drag", ["phase"] = "begin", ["at"] = new JsonArray(g.X, g.Y), ["slop"] = 12 / zoom });
        dragFaces = [];
        if (r.TryGetProperty("result", out var res) && res.TryGetProperty("drag", out var d) && d.TryGetProperty("faces", out var fl))
            foreach (var x in fl.EnumerateArray()) if (x.TryGetUInt64(out var nid)) dragFaces.Add(nid);
        dragging = true;
        dragFaceNid = dragFaces.FirstOrDefault();
    }

    float[] dragBuf = new float[4096];

    void OnMove(PointerRoutedEventArgs e)
    {
        if (!dragging) return;
        var g = ToGraph(e.GetCurrentPoint(rootGrid).Position);
        var d = core.GestureMove(0, g.X, g.Y, 12 / zoom);
        if (d.Status != PlumStatus.Ok || d.Kind != 1 || dragFaces.Count == 0) return;
        var n = core.WirePaths(0, dragFaces.ToArray(), d.Dx, d.Dy, ref dragBuf);
        renderer.Drag(scene, dragFaces, new Vector2(d.Dx, d.Dy), PlumCore.DecodePaths(dragBuf, n));
    }

    async void OnRelease(PointerRoutedEventArgs e)
    {
        rootGrid.ReleasePointerCapture(e.Pointer);
        if (!dragging) return;
        dragging = false;
        var at = e.GetCurrentPoint(rootGrid).Position;
        var g = ToGraph(at);
        var moved = Math.Abs(at.X - pressAt.X) + Math.Abs(at.Y - pressAt.Y) > 4;
        JsonElement r;
        if (moved)
            r = await core.RequestAsync("ui.gesture", new JsonObject { ["kind"] = "drag", ["phase"] = "end", ["at"] = new JsonArray(g.X, g.Y) });
        else
        {
            await core.RequestAsync("ui.gesture", new JsonObject { ["kind"] = "drag", ["phase"] = "cancel", ["at"] = new JsonArray(g.X, g.Y) });
            r = await core.RequestAsync("ui.gesture", new JsonObject { ["kind"] = "tap", ["at"] = new JsonArray(g.X, g.Y), ["slop"] = 12 / zoom });
        }
        // what the core made of it (the input probe reads this)
        Log($"gesture {(moved ? "drag end" : "tap")} by {e.Pointer.PointerDeviceType}: {r.GetRawText()}");
    }

    void OnWheel(PointerRoutedEventArgs e)
    {
        var pt = e.GetCurrentPoint(rootGrid);
        var delta = pt.Properties.MouseWheelDelta;
        if ((e.KeyModifiers & Windows.System.VirtualKeyModifiers.Control) != 0)
        {
            var g = ToGraph(pt.Position);
            zoom = Math.Clamp(zoom * MathF.Pow(1.0015f, delta), 0.05f, 4);
            pan = new Vector2((float)pt.Position.X, (float)pt.Position.Y) - g * zoom;
        }
        else if (pt.Properties.IsHorizontalMouseWheel) pan.X -= delta;
        else pan.Y += delta;
        renderer.SetView(zoom, pan);
        e.Handled = true;
    }
}
