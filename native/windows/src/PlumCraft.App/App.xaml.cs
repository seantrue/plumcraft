// PlumCraft for Windows (N5 slice 2): a WinUI 3 app on the Windows kit. One core per process (the kit's
// PlumCore, in-process through the C ABI), one core document per window (decision 8), each window a
// DocWindow showing its graph on the Composition canvas. Unpackaged (WindowsPackageType None) with the
// Windows App SDK carried in its folder, so the .exe runs from there with nothing installed.

using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using PlumCraft.Core;
using PlumCraft.Kit;
using PlumCraft.Models;

namespace PlumCraft.App;

public partial class App : Application
{
    public App()
    {
        CoreHost.Mark("app");
        // the app's theme is the appearance asked for (`--appearance`), else the system's: the panels' theme
        // brushes (cards, secondary text) are read from the app's resources, so they follow it (N5 slice 4)
        switch (Options.Parse(Environment.GetCommandLineArgs()).Appearance)
        {
            case "dark": RequestedTheme = ApplicationTheme.Dark; break;
            case "light": RequestedTheme = ApplicationTheme.Light; break;
        }
        InitializeComponent();
        // a diagnostic for a fault the platform raises past the handler below (a stowed exception): every
        // exception thrown, as it is thrown, when PLUMMER_TRACE is set
        if (Environment.GetEnvironmentVariable("PLUMMER_TRACE") is { Length: > 0 })
            AppDomain.CurrentDomain.FirstChanceException += (_, e) => CoreHost.Log($"thrown: {e.Exception.GetType().Name} 0x{e.Exception.HResult:x8} {e.Exception.Message}\n{Environment.StackTrace}");
        UnhandledException += (_, e) =>
        {
            CoreHost.Log($"unhandled: {e.Exception}");
            // the person's work is in the core; a fault in a view is logged, not a crash (rule 1)
            e.Handled = true;
        };
    }

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        var o = Options.Parse(Environment.GetCommandLineArgs());
        CoreHost.Options = o;
        CoreHost.Status.Path = o.Status;
        CoreHost.Mark("launched");
        try
        {
            await CoreHost.StartAsync();
        }
        catch (Exception e)
        {
            CoreHost.Log($"the core didn't open: {e}");
            CoreHost.Status.Set("error", e.Message);
            Exit();
        }
    }
}

/// <summary>The process's one core, its windows and the status file scripts wait on.</summary>
public static class CoreHost
{
    /// <summary>The app's name (the maintainer, 2026-11-21: "Plummer will be the new base app/platform"): window
    /// titles, About, the executable, the data folder. The project, kit, core and protocol stay PlumCraft.</summary>
    public const string AppName = "Plummer";
    public const string AppId = "com.swapwizard.plummer.windows";

    public static Options Options { get; set; } = new();
    public static PlumCore? Core { get; private set; }
    public static readonly List<DocWindow> Windows = [];
    public static readonly StatusFile Status = new();
    public static readonly Stopwatch Clock = Stopwatch.StartNew();
    static readonly object logLock = new();

    static readonly System.Text.Json.Nodes.JsonObject marks = [];

    /// <summary>When something happened, in ms since the process started (the status file's `marks`).</summary>
    public static void Mark(string what)
    {
        try
        {
            var ms = Math.Round((DateTime.Now - Process.GetCurrentProcess().StartTime).TotalMilliseconds, 1);
            lock (marks) marks[what] = ms;
            Status.Set("marks", marks.DeepClone());
        }
        catch (Exception) { }
    }

    /// <summary>The display's frame period (EnumDisplaySettings), for the core's coalescing.</summary>
    public static double FrameMs { get; private set; } = 16.7;

    public static void Log(string s)
    {
        Debug.WriteLine(s);
        try
        {
            Directory.CreateDirectory(Path.Combine(Options.Root, "logs"));
            lock (logLock) File.AppendAllText(Path.Combine(Options.Root, "logs", "app.log"), $"{DateTime.Now:O} {s}\n");
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }

    public static async Task StartAsync()
    {
        var o = Options;
        Status.Path = o.Status;
        var (hz, _, _) = Interop.Display();
        if (hz > 1) FrameMs = 1000.0 / hz;
        var dark = Appearance.SystemDark();
        if (o.Appearance == "dark") dark = true;
        if (o.Appearance == "light") dark = false;
        var cfg = new ShellConfig
        {
            Root = o.Root, Engine = o.Engine, ControlPort = o.ControlPort, TokenFile = o.TokenFile, FrameMs = FrameMs, Dark = dark,
            ShellVersion = typeof(App).Assembly.GetName().Version?.ToString(3) ?? "0.1.0",
        };
        cfg.Prepare();
        var json = cfg.ToJson();
        json["app"]!["build"] = "n5-slice3";
        // the shell names itself: the core's default (PlumCraft, the egui editor's) stays
        json["app"]!["name"] = AppName;
        json["app"]!["id"] = AppId;
        json["engines"]!["auto"] = true;
        // a window per document: New, Open and Quit are the core's, and it says which documents to show
        // and no share sheet yet: the core disables Share with its reason (platform.share, 4.2)
        if (json["platform"] is not System.Text.Json.Nodes.JsonObject plat) json["platform"] = plat = new System.Text.Json.Nodes.JsonObject();
        plat["document_windows"] = true;
        plat["share"] = false;
        var display = json["display"]!.AsObject();
        var look = Appearance.Now(dark);
        display["contrast"] = look.Contrast;
        display["reduce_motion"] = look.ReduceMotion;
        display["reduce_transparency"] = look.ReduceTransparency;
        var t0 = Clock.Elapsed.TotalMilliseconds;
        Mark("config");
        var core = new PlumCore(json);
        Mark("core");
        Core = core;
        // the core's own first document closes: each window opens its own (document.open)
        try
        {
            var list = await core.ResultAsync("document.list");
            if (list.Get("docs") is { ValueKind: JsonValueKind.Array } docs)
                foreach (var d in docs.EnumerateArray())
                    if (d.Get("doc").U64() is { } id) await core.ResultAsync("document.close", Jsonv.Object(("discard", Jsonv.True)), id);
        }
        catch (PlumException e) { Log($"closing the core's first document: {e.Message}"); }
        Status.Set("core_open_ms", Clock.Elapsed.TotalMilliseconds - t0);
        if (o.ControlPort is not null)
        {
            try
            {
                var c = await core.ResultAsync("core.control");
                if (c.Get("port").U64() is { } port) Status.Set("port", port);
                Status.Set("tokenFile", o.TokenFile);
            }
            catch (PlumException e) { Log($"core.control: {e.Message}"); }
        }
        Appearance.Follow(core, o.Appearance is null);
        _ = FollowWindows(core);
        if (o.Paths.Count == 0) await OpenWindowAsync(null, first: true);
        else
            for (int i = 0; i < o.Paths.Count; i++) await OpenWindowAsync(Path.GetFullPath(o.Paths[i]), first: i == 0);
    }

    /// <summary>The documents the core opens for New and Open (`changed` domain `window`): a window each,
    /// placed from the window it was asked in.</summary>
    static async Task FollowWindows(PlumCore core)
    {
        var feed = core.Subscribe();
        try
        {
            await foreach (var b in feed.Reader.ReadAllAsync())
                foreach (var w in WindowWanted.Of(b))
                {
                    if (Windows.Any(x => x.Doc == w.Doc)) continue;
                    var from = Windows.FirstOrDefault(x => x.Doc == w.From);
                    var win = new DocWindow(core, w.Doc, false);
                    Windows.Add(win);
                    win.PlaceFrom(from);
                    win.Activate();
                    Mark("window_from_core");
                }
        }
        catch (Exception e) { Log($"following the core's windows: {e.Message}"); }
    }

    /// <summary>A new document (`document.open`, a graph's path or none) in a new window: what the app was
    /// launched with (its command line is the platform's way of opening files).</summary>
    public static async Task<DocWindow?> OpenWindowAsync(string? path, bool first = false)
    {
        if (Core is not { } core) return null;
        try
        {
            var p = path is null ? Jsonv.EmptyObject : Jsonv.Object(("path", Jsonv.Of(path)));
            var r = await core.ResultAsync("document.open", p);
            if (r.Get("doc").U64() is not { } doc) throw new PlumException("document.open gave no doc");
            // the budgets' graph (--nodes): built before the window, so its first frame has every face
            if (first && Options.Nodes > 0)
            {
                Mark("document");
                await core.ResultAsync("spike.scene", Jsonv.Object(("nodes", Jsonv.Of((long)Options.Nodes)), ("wires", Jsonv.Of((long)Options.Wires))), doc);
                Mark("scene_built");
            }
            var w = new DocWindow(core, doc, first);
            Mark("window");
            if (!first) w.PlaceFrom(Windows.LastOrDefault());
            Windows.Add(w);
            w.Activate();
            return w;
        }
        catch (Exception e)
        {
            Log($"opening {path ?? "a new graph"}: {e.Message}");
            Windows.FirstOrDefault()?.Banner($"Couldn't open {(path is null ? "a new graph" : Path.GetFileName(path))}: {e.Message}", true);
            return null;
        }
    }

    /// <summary>A window's document closed and the window went: the last one quits the app.</summary>
    public static void Closed(DocWindow w)
    {
        Windows.Remove(w);
        Status.RemoveWindow(w.Doc);
        if (Windows.Count == 0)
        {
            Status.Set("quit", true);
            Core?.Dispose();
            Core = null;
            Application.Current.Exit();
        }
    }
}

/// <summary>What the app writes to `--status`: scripts wait on it with a time limit, never on a process
/// name. Written atomically on every change.</summary>
public sealed class StatusFile
{
    public string? Path;
    readonly JsonObject state = new() { ["pid"] = Environment.ProcessId };
    readonly SortedDictionary<ulong, JsonObject> windows = [];

    public JsonNode? Get(string key) => state[key]?.DeepClone();

    public void Set(string key, JsonNode? v)
    {
        state[key] = v;
        Write();
    }

    public void Window(ulong doc, JsonObject v)
    {
        windows[doc] = v;
        Write();
    }

    public void RemoveWindow(ulong doc)
    {
        windows.Remove(doc);
        Write();
    }

    void Write()
    {
        if (Path is null) return;
        var s = state.DeepClone().AsObject();
        s["windows"] = new JsonArray([.. windows.Values.Select(w => w.DeepClone())]);
        try
        {
            var tmp = Path + ".tmp";
            File.WriteAllText(tmp, s.ToJsonString());
            File.Move(tmp, Path, true);
        }
        catch (IOException) { }
        catch (UnauthorizedAccessException) { }
    }
}
