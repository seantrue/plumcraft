// The conformance suite (docs/shell-interface.md 16.1) against a running shell's control port, from C#:
//
//   plumcraft-conformance --port N --token-file F --cases cases.json [--doc D] [--half read|edit|all]
//                         [--measured] [--engine text]... [--case NAME] [--needing CAPABILITY]
//                         [--host H] [--no-prefs]
//   plumcraft-conformance call --port N --token-file F [--doc D] METHOD [PARAMS-JSON | @FILE]
//
// The Swift kit's command (native/apple/PlumCraftFFI, plumcraft-conformance) with the same flags and the
// same CASE lines. `--measured`: the shell measures its own faces (an app with a canvas), so the suite's
// geometry steps are skipped and each settle waits until every face is measured. Exits 0 when no case
// failed (a skipped case is reported, never passed). `call` sends one request and prints its reply (a
// script driving the app the way any control client does).

using System.Diagnostics;
using System.Text.Json;
using PlumCraft.Conformance;
using PlumCraft.Models;

var argv = args.ToList();
string? Value(string k)
{
    var i = argv.IndexOf(k);
    if (i < 0 || i + 1 >= argv.Count) return null;
    var v = argv[i + 1];
    argv.RemoveRange(i, 2);
    return v;
}
bool Flag(string k) => argv.Remove(k);

var call = argv.Count > 0 && argv[0] == "call";
if (call) argv.RemoveAt(0);
var engines = new HashSet<string>();
while (Value("--engine") is { } e) engines.Add(e);
var measured = Flag("--measured");
var prefs = !Flag("--no-prefs");
var host = Value("--host") ?? "127.0.0.1";
var portText = Value("--port");
var tokenFile = Value("--token-file");
var casesPath = Value("--cases");
var doc = Value("--doc") is { } ds && ulong.TryParse(ds, out var dn) ? dn : (ulong?)null;
var halfWord = Value("--half") ?? "read";
var only = Value("--case");
var needing = Value("--needing");
if (portText is null || !int.TryParse(portText, out var port) || tokenFile is null || (!call && casesPath is null))
{
    Console.Error.WriteLine("usage: plumcraft-conformance [--host H] --port N --token-file F --cases cases.json [--doc D] [--half read|edit|all] [--measured] [--engine text] [--case NAME] [--needing CAPABILITY]");
    Console.Error.WriteLine("       plumcraft-conformance call [--host H] --port N --token-file F [--doc D] METHOD [PARAMS-JSON | @FILE]");
    return 2;
}
try
{
    var token = File.ReadAllText(tokenFile).Trim();
    using var d = await PortDriver.ConnectAsync(port, token, host, reports: !measured, doc: doc, engines: engines.Count == 0 ? ["text"] : engines, prefs: prefs);
    if (call)
    {
        if (argv.Count == 0) { Console.Error.WriteLine("call: which method?"); return 2; }
        var text = argv.Count > 1 ? argv[1] : "{}";
        if (text.StartsWith('@')) text = File.ReadAllText(text[1..]);
        var reply = await d.CallAsync(argv[0], Jsonv.Parse(text));
        Console.WriteLine(reply.GetRawText());
        return reply.Get("ok").IsTrue() ? 0 : 1;
    }
    var cases = Conformance.Cases(casesPath!);
    if (only is not null) cases = [.. cases.Where(c => c.Get("name").Str() == only)];
    if (needing is not null) cases = [.. cases.Where(c => Conformance.Needs(c, needing))];
    CaseHalf? half = halfWord switch { "all" => null, "edit" => CaseHalf.Edit, _ => CaseHalf.Read };
    var sw = Stopwatch.StartNew();
    var outcomes = await Conformance.RunAsync(d, cases, half);
    foreach (var o in outcomes)
    {
        var word = o.Passed ? "pass" : o.Skipped is not null ? "skip" : "FAIL";
        Console.WriteLine($"CASE {word} | {o.Name} | {o.Steps} steps{(o.Skipped is { } s ? $" | {s}" : "")}{(o.Error is { } er ? $" | {er}" : "")}");
    }
    Console.WriteLine(Conformance.Summary(d.Name, outcomes));
    Console.WriteLine($"settle timeouts: {d.SettleTimeouts}; took {sw.Elapsed.TotalSeconds:F1} s");
    return outcomes.Any(o => o.Error is not null) ? 1 : 0;
}
catch (Exception ex)
{
    Console.Error.WriteLine($"plumcraft-conformance: {ex.Message}");
    return 2;
}
