// The core's config for a Windows shell (docs/shell-interface.md 4.1), the counterpart of the
// Swift kit's ShellConfig (PlumCraftShell). Every root is under one folder; the environment the
// store and the core read (COMMONKIT_STORE_ROOT wins over store_root) is set to the same folder
// before the core opens, so nothing a launcher inherited can point the core at another store.

using System.Text.Json.Nodes;

namespace PlumCraft.Core;

public sealed class ShellConfig
{
    /// <summary>The folder every root goes under (an app: %LOCALAPPDATA%\PlumCraft).</summary>
    public required string Root { get; init; }
    public string Engine { get; init; } = "text";
    public bool Demo { get; init; }
    /// <summary>A control port (0 picks a free one) and the file its token is written to; null: none.</summary>
    public int? ControlPort { get; init; }
    public string? TokenFile { get; init; }
    public double FrameMs { get; init; } = 16.7;
    public double Scale { get; init; } = 1.0;
    public bool Dark { get; init; }
    public string ShellVersion { get; init; } = "0.0.1-spike";
    /// <summary>Whether this machine can run Python engines (`platform.python`): by default, whether uv is
    /// found (the maintainer: "Declare Python when uv is found"), which installs a Python of its own for
    /// each engine. Install Engine… is offered when it is (B71).</summary>
    public bool? Python { get; init; }

    public string Dir(string name) => Path.Combine(Root, name);

    /// <summary>Make the folders and set the environment the store reads, for this process.</summary>
    public void Prepare()
    {
        foreach (var d in new[] { "documents", "caches", "store", "scratch", "prefs", "logs", "marks", "run", "craft" })
            Directory.CreateDirectory(Dir(d));
        Environment.SetEnvironmentVariable("COMMONKIT_STORE_ROOT", Dir("store"));
        Environment.SetEnvironmentVariable("COMMONKIT_MARK_DIR", Dir("marks"));
        Environment.SetEnvironmentVariable("COMMONKIT_TEST_RUN", Dir("run"));
        Environment.SetEnvironmentVariable("COMMONKIT_HASH", "b3");
        // commonkit declines every blob with under 8 GiB free by default (note 24, 15.4)
        Environment.SetEnvironmentVariable("COMMONKIT_STORE_MIN_FREE_GB", "2");
        Environment.SetEnvironmentVariable("PLUMCRAFT_CRAFT_ROOT", Dir("craft"));
        Environment.SetEnvironmentVariable("PLUMCRAFT_EDITOR_PREFS", Path.Combine(Dir("prefs"), "prefs.json"));
    }

    public JsonObject ToJson()
    {
        var o = new JsonObject
        {
            ["shell"] = 1,
            ["store_root"] = Dir("store"),
            ["demo"] = Demo,
            ["app"] = new JsonObject { ["id"] = "com.plumcraft.windows", ["name"] = "PlumCraft", ["version"] = ShellVersion, ["build"] = "spike" },
            ["dirs"] = new JsonObject
            {
                ["documents"] = Dir("documents"), ["caches"] = Dir("caches"), ["store"] = Dir("store"),
                ["scratch"] = Dir("scratch"), ["prefs"] = Dir("prefs"), ["logs"] = Dir("logs"),
            },
            ["platform"] = new JsonObject
            {
                ["os"] = "windows", ["subprocess"] = true, ["craft_apps"] = false, ["python"] = Python ?? Uv.Find() is not null, ["multi_window"] = true,
                ["pointer"] = true, ["keyboard"] = true, ["hover"] = true, ["pencil"] = true, ["max_picture_px"] = 4096,
            },
            ["display"] = new JsonObject { ["locale"] = "en-US", ["scale"] = Scale, ["text_size"] = 14.0, ["dark"] = Dark, ["frame_ms"] = FrameMs },
            ["engines"] = new JsonObject { ["default"] = Engine, ["mode"] = "inproc" },
        };
        if (ControlPort is { } port)
            o["control"] = new JsonObject { ["port"] = port, ["token_file"] = TokenFile };
        return o;
    }
}

/// <summary>Where `uv` is, looked for as the core looks for a program (`plumcraft_host::manifest::program`):
/// `$PLUMCRAFT_UV` when it names a file, then PATH, then the usual install folders under the home folder
/// (`bin`, `.local\bin`, uv's own installer's, and `.cargo\bin`), each name with PATHEXT's extensions. An app
/// started from the desktop has the system's PATH and the user's, not a terminal's.</summary>
public static class Uv
{
    public static string? Find()
    {
        if (Environment.GetEnvironmentVariable("PLUMCRAFT_UV") is { Length: > 0 } given && File.Exists(given)) return given;
        var dirs = (Environment.GetEnvironmentVariable("PATH") ?? "").Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries).ToList();
        var home = Environment.GetEnvironmentVariable("HOME") is { Length: > 0 } h ? h : Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        if (home.Length > 0) dirs.AddRange([Path.Combine(home, "bin"), Path.Combine(home, ".local", "bin"), Path.Combine(home, ".cargo", "bin")]);
        var exts = (Environment.GetEnvironmentVariable("PATHEXT") is { Length: > 0 } pe ? pe : ".COM;.EXE;.BAT;.CMD")
            .Split(";", StringSplitOptions.RemoveEmptyEntries);
        foreach (var d in dirs)
            foreach (var e in exts)
            {
                try
                {
                    var f = Path.Combine(d.Trim('"'), "uv" + e.ToLowerInvariant());
                    if (File.Exists(f)) return f;
                }
                catch (ArgumentException) { }
            }
        return null;
    }
}
