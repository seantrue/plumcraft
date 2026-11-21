// The app's command line.
//
//   PlumCraft.exe [graph.json ...]           each graph in its own window (none: one untitled window)
//     --root DIR                  every root the core and its engines write under DIR (default
//                                 %LOCALAPPDATA%\Plummer): store, marks, preferences, scratch, logs
//     --engine text|array         the engine the core starts on (default text; the core starts the
//                                 others a graph uses, engines.auto)
//     --control-port N --token-file F   open the core's control port on loopback (0: any free port),
//                                 its token written to F (scripts and the conformance suite)
//     --status F                  write the app's state as JSON to F (pid, port, windows, readiness)
//     --appearance light|dark     the app's appearance, whatever the system's (screenshots)
//     --top                       the first window at the screen's top left, above other windows
//     --size W,H                  the first window's size (DIPs; default 1200,800)
//     --nodes N [--wires M]       build a spike.scene of N nodes in the first window (budgets)
//     --view Z,X,Y                after the first frame: zoom Z with graph point (X, Y) at the top left
//     --bench OUT [--exit] [--phase-seconds S]   the budget run (Bench.cs)
//     --then STEP;STEP            what the first window does once drawn (Script.cs)

using System.Globalization;

namespace PlumCraft.App;

public sealed class Options
{
    public List<string> Paths = [];
    public string Root = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), CoreHost.AppName);
    public string Engine = "text";
    public int? ControlPort;
    public string? TokenFile;
    public string? Status;
    public string? Appearance;
    public bool Top;
    public int Width = 1200, Height = 800;
    public int Nodes;
    public int Wires = -1;
    public float[]? View;
    public string? Bench;
    public bool Exit;
    public double PhaseSeconds = 4;
    public List<string> Then = [];
    /// <summary>A scripted launch (a status file or a bench): errors are logged, never a dialog.</summary>
    public bool Scripted => Status is not null || Bench is not null;

    public static Options Parse(string[] a)
    {
        var o = new Options();
        for (int i = 1; i < a.Length; i++)
        {
            string Next() => i + 1 < a.Length ? a[++i] : "";
            static int Int(string s, int d) => int.TryParse(s, NumberStyles.Integer, CultureInfo.InvariantCulture, out var v) ? v : d;
            switch (a[i])
            {
                case "--root": o.Root = Next(); break;
                case "--engine": o.Engine = Next(); break;
                case "--control-port": o.ControlPort = Int(Next(), 0); break;
                case "--token-file": o.TokenFile = Next(); break;
                case "--status": o.Status = Next(); break;
                case "--appearance": o.Appearance = Next(); break;
                case "--top": o.Top = true; break;
                case "--size":
                    var wh = Next().Split(',');
                    if (wh.Length == 2) { o.Width = Int(wh[0], 1200); o.Height = Int(wh[1], 800); }
                    break;
                case "--nodes": o.Nodes = Int(Next(), 0); break;
                case "--wires": o.Wires = Int(Next(), -1); break;
                case "--view":
                    var v = Next().Split(',').Select(x => float.TryParse(x, NumberStyles.Float, CultureInfo.InvariantCulture, out var f) ? f : float.NaN).ToArray();
                    if (v.Length == 3 && v.All(float.IsFinite)) o.View = v;
                    break;
                case "--bench": o.Bench = Next(); break;
                case "--exit": o.Exit = true; break;
                case "--phase-seconds": o.PhaseSeconds = double.TryParse(Next(), NumberStyles.Float, CultureInfo.InvariantCulture, out var ps) ? ps : 4; break;
                case "--then": o.Then = [.. Next().Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)]; break;
                default:
                    if (!a[i].StartsWith("--", StringComparison.Ordinal)) o.Paths.Add(a[i]);
                    break;
            }
        }
        if (o.Wires < 0) o.Wires = o.Nodes * 8 / 5;
        if (o.ControlPort is not null && o.TokenFile is null) o.TokenFile = Path.Combine(o.Root, "control-token");
        return o;
    }
}
