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
                ["os"] = "windows", ["subprocess"] = true, ["craft_apps"] = false, ["python"] = false, ["multi_window"] = true,
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
