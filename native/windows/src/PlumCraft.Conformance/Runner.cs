// The shell conformance suite's runner in C# (docs/shell-interface.md 16.1): the cases are the shared
// data, crates/editor-core/fixtures/conformance/cases.json, read where they are (never copied); this
// is crates/editor-core/src/conformance.rs's step language, value substitution, `$locate`, matchers
// and checks, as the Swift kit's Runner.swift has them, so a case means the same here as in Rust and
// Swift.

using System.Text;
using System.Text.Json;
using PlumCraft.Models;

namespace PlumCraft.Conformance;

/// <summary>A shell the suite drives (conformance.rs's Driver).</summary>
public interface IConformanceDriver
{
    /// <summary>For messages.</summary>
    string Name { get; }
    /// <summary>One control request; the reply {ok, result | error} (never thrown).</summary>
    Task<JsonElement> CallAsync(string method, JsonElement parameters);
    /// <summary>Does this shell take its measurements from the suite (a headless shell), rather than
    /// measure the faces it draws?</summary>
    bool Reports { get; }
    /// <summary>Report `faces` (geometry.set's faces, ids resolved) and wait until the core has them.</summary>
    Task ReportGeometryAsync(JsonElement faces);
    /// <summary>Wait until the shell drew and measured what the last request changed.</summary>
    Task SettleAsync();
    /// <summary>The typed drag move, as plum_gesture_move answers it (over a port its JSON form,
    /// geometry.dragMove); null where the driver has none.</summary>
    Task<JsonElement?> HotMoveAsync(float x, float y, float slop);
    /// <summary>`engine:<pack>`, `media`, `port`, `prefs`, `rows`; `reports` for a driver that measures its
    /// own faces but reports the suite's over them (N5 slice 4; a headless driver's is Reports).</summary>
    bool Has(string capability);
    /// <summary>Let the shell get on before a request is sent again (`until`).</summary>
    Task PauseAsync();
}

/// <summary>A driver over a control port can check the port asks for its token (N4 slice 4).</summary>
public interface IPortCheck
{
    Task AuthRefusedAsync();
}

/// <summary>The halves of the suite (16.1).</summary>
public enum CaseHalf { Read, Edit }

/// <summary>How one case went: passed when neither Error nor Skipped is set.</summary>
public sealed record Outcome(string Name, int Steps, string? Error, string? Skipped)
{
    /// <summary>Steps of the case this driver didn't run (an `only:` it lacks, a `hot` or `geometry` step
    /// it has no form for): N5 slice 4's gate counts them.</summary>
    public int StepsSkipped { get; init; }
    public bool Passed => Error is null && Skipped is null;
}

public sealed class ConformanceException(string message) : Exception(message);

public static class Conformance
{
    /// <summary>The core's default hit distance in graph units (gesture::DEFAULT_SLOP).</summary>
    public const float DefaultSlop = 12;

    /// <summary>The cases in a cases.json.</summary>
    public static List<JsonElement> Cases(string path)
    {
        var v = Jsonv.Parse(File.ReadAllBytes(path));
        if (v.Get("cases") is not { ValueKind: JsonValueKind.Array } cases) throw new ConformanceException($"{path}: no \"cases\" list");
        return [.. cases.EnumerateArray()];
    }

    public static string? HalfOf(JsonElement c) => c.Get("half").Str();

    public static bool Needs(JsonElement c, string capability) =>
        c.Get("needs") is { ValueKind: JsonValueKind.Array } n && n.EnumerateArray().Any(x => x.ValueKind == JsonValueKind.String && x.GetString() == capability);

    /// <summary>Run every case of `half` (null: all) on `d`, in order.</summary>
    public static async Task<List<Outcome>> RunAsync(IConformanceDriver d, IEnumerable<JsonElement> cases, CaseHalf? half)
    {
        var outp = new List<Outcome>();
        var word = half switch { CaseHalf.Read => "read", CaseHalf.Edit => "edit", _ => null };
        foreach (var c in cases)
            if (word is null || HalfOf(c) == word) outp.Add(await RunCaseAsync(d, c));
        return outp;
    }

    /// <summary>A short report: passed, failed with why, skipped with why.</summary>
    public static string Summary(string driver, IReadOnlyList<Outcome> outcomes)
    {
        var failed = outcomes.Where(o => o.Error is not null).Select(o => $"  {o.Name}: {o.Error}").ToList();
        var skipped = outcomes.Where(o => o.Skipped is not null).Select(o => $"  skipped {o.Name}: {o.Skipped}").ToList();
        int run = outcomes.Count - skipped.Count;
        var sb = new StringBuilder($"{driver}: {run - failed.Count} of {run} cases passed, {skipped.Count} skipped");
        // the steps (N5 slice 4): run of the cases' total, and those this driver skipped
        int total = outcomes.Sum(o => o.Steps), notRun = outcomes.Sum(o => o.StepsSkipped);
        sb.Append($"; steps {total - notRun} run of {total}, {notRun} skipped");
        foreach (var f in failed) sb.Append('\n').Append(f);
        foreach (var s in skipped) sb.Append('\n').Append(s);
        return sb.ToString();
    }

    static bool Has(IConformanceDriver d, string capability) => capability switch
    {
        // the suite's measurements reported: by a headless driver, or over an app's own (N5 slice 4)
        "reports" => d.Reports || d.Has("reports"),
        // rows' widgets and header parts measured (N4 slice 2): reported by the suite, or by the app
        "rows" => d.Reports || d.Has("rows"),
        _ => d.Has(capability),
    };

    static JsonElement Obj(params (string, JsonElement)[] f) => Jsonv.Object(f);

    /// <summary>Run one case on a new graph, or skip it when `d` lacks what it needs.</summary>
    /// <summary>Where each case's `$scratch` folder is made (a test's scratch root, or `--scratch`).</summary>
    public static string ScratchRoot { get; set; } = Path.Combine(Path.GetTempPath(), "plumcraft-conformance", Environment.ProcessId.ToString(System.Globalization.CultureInfo.InvariantCulture));
    static int caseCount;

    public static async Task<Outcome> RunCaseAsync(IConformanceDriver d, JsonElement c)
    {
        var name = c.Get("name").Str() ?? "(unnamed)";
        var lacks = new List<string>();
        if (c.Get("needs") is { ValueKind: JsonValueKind.Array } needs)
            foreach (var n in needs.EnumerateArray())
                if (n.ValueKind == JsonValueKind.String && !Has(d, n.GetString()!)) lacks.Add(n.GetString()!);
        if (lacks.Count > 0) return new Outcome(name, 0, null, $"{d.Name} lacks {string.Join(", ", lacks)}");
        var vars = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        // `$scratch`: a folder of this case's own, made here, for what the shell writes (Save Result's
        // files); on the machine the shell runs on, which for an app over its port is this one
        var scratch = Path.Combine(ScratchRoot, $"case-{Interlocked.Increment(ref caseCount)}");
        Directory.CreateDirectory(scratch);
        vars["scratch"] = Jsonv.Of(scratch);
        var fresh = await d.CallAsync("command.run", Obj(("command", Jsonv.Of("graph.new")), ("params", Obj(("discard", Jsonv.True)))));
        if (!fresh.Get("ok").IsTrue()) return new Outcome(name, 0, $"graph.new: {fresh.Short()}", null);
        await d.SettleAsync();
        var steps = c.Get("steps") is { ValueKind: JsonValueKind.Array } a ? a.EnumerateArray().ToList() : new List<JsonElement>();
        int skipped = 0;
        for (int k = 0; k < steps.Count; k++)
        {
            try
            {
                if (!await StepAsync(d, vars, steps[k])) skipped++;
            }
            catch (Exception e)
            {
                return new Outcome(name, k, $"step {k + 1}: {e.Message}", null) { StepsSkipped = skipped };
            }
        }
        // a drag left under way would carry into the next case, and so would an open palette
        await d.CallAsync("ui.gesture", Obj(("kind", Jsonv.Of("drag")), ("phase", Jsonv.Of("cancel"))));
        await d.CallAsync("command.run", Obj(("command", Jsonv.Of("canvas.palette")), ("params", Obj(("open", Jsonv.False)))));
        return new Outcome(name, steps.Count, null, null) { StepsSkipped = skipped };
    }

    /// <summary>Run one step: false when this driver skipped it.</summary>
    static async Task<bool> StepAsync(IConformanceDriver d, Dictionary<string, JsonElement> vars, JsonElement s)
    {
        var only = new List<string>();
        switch (s.Get("only"))
        {
            case { ValueKind: JsonValueKind.String } o: only.Add(o.GetString()!); break;
            case { ValueKind: JsonValueKind.Array } o:
                foreach (var x in o.EnumerateArray()) if (x.ValueKind == JsonValueKind.String) only.Add(x.GetString()!);
                break;
        }
        if (only.Any(x => !Has(d, x))) return false;
        if (s.Get("geometry") is { } faces)
        {
            if (!Has(d, "reports")) return false;
            await d.ReportGeometryAsync(Subst(faces, vars));
            return true;
        }
        if (s.Get("hot") is { } h)
        {
            var r = await ResolveAsync(d, Subst(h, vars));
            if (r.Get("at") is not { ValueKind: JsonValueKind.Array } at || at.GetArrayLength() != 2) throw new ConformanceException("hot.at must be [x, y]");
            if (((JsonElement?)at[0]).Double() is not { } x || ((JsonElement?)at[1]).Double() is not { } y) throw new ConformanceException("hot.at must be numbers");
            var slop = r.Get("slop").Double() is { } sl ? (float)sl : DefaultSlop;
            if (await d.HotMoveAsync((float)x, (float)y, slop) is not { } v) return false;
            Expect(Obj(("ok", Jsonv.True), ("result", v)), s.Get("expect"), vars);
            return true;
        }
        if (s.Get("check").Str() is { } check)
        {
            if (check == "authRefused")
            {
                if (d is not IPortCheck p) throw new ConformanceException("authRefused needs a driver over the control port");
                await p.AuthRefusedAsync();
                return true;
            }
            Check(check, Subst(s, vars));
            return true;
        }
        string method;
        JsonElement parameters;
        var call = s.Get("call").Str();
        var gesture = s.Get("gesture");
        if (call is not null && gesture is null)
        {
            method = call;
            parameters = s.Get("params") ?? Jsonv.EmptyObject;
        }
        else if (call is null && gesture is { } g)
        {
            method = "ui.gesture";
            parameters = g;
        }
        else
        {
            throw new ConformanceException($"a step is a call, a gesture, geometry, hot or a check: {s.Short()}");
        }
        var p2 = await ResolveAsync(d, Subst(parameters, vars));
        var reply = await d.CallAsync(method, p2);
        if (s.Get("until") is { } until)
        {
            var limit = Math.Min(Math.Max(s.Get("timeout").Double() ?? 30, 0), 600);
            var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(limit);
            while (true)
            {
                try
                {
                    Expect(reply, until, vars);
                    break;
                }
                catch (ConformanceException e)
                {
                    if (DateTime.UtcNow > deadline) throw new ConformanceException($"{method} {p2.Short()}: not so in {limit} s: {e.Message}");
                }
                await d.PauseAsync();
                reply = await d.CallAsync(method, p2);
            }
        }
        // a condition that must stay so for a while (`holds`, `for`, B43): the runner lacked it until N5
        // slice 4 and passed such a step on its first reply alone (B53)
        if (s.Get("holds") is { } holds)
        {
            var span = TimeSpan.FromSeconds(Math.Min(Math.Max(s.Get("for").Double() ?? 1, 0), 10));
            var t0 = DateTime.UtcNow;
            while (true)
            {
                try
                {
                    Expect(reply, holds, vars);
                }
                catch (ConformanceException e)
                {
                    throw new ConformanceException($"{method} {p2.Short()}: didn't stay so ({(DateTime.UtcNow - t0).TotalSeconds:F2} s in): {e.Message}");
                }
                if (DateTime.UtcNow - t0 >= span) break;
                await d.PauseAsync();
                reply = await d.CallAsync(method, p2);
            }
        }
        var wantOk = s.Get("ok").Bool() ?? true;
        if (reply.Get("ok").IsTrue() != wantOk)
            throw new ConformanceException($"{method} {p2.Short()}: wanted ok {(wantOk ? "true" : "false")}, got {reply.Short()}");
        if (method is "command.run" or "ui.gesture" or "ui.invoke" or "geometry.set") await d.SettleAsync();
        try
        {
            Expect(reply, s.Get("expect"), vars);
        }
        catch (ConformanceException e)
        {
            throw new ConformanceException($"{method} {p2.Short()}: {e.Message}");
        }
        if (s.Get("save") is { ValueKind: JsonValueKind.Object } save)
        {
            foreach (var kv in save.EnumerateObject())
            {
                if (kv.Value.ValueKind != JsonValueKind.String) throw new ConformanceException("save's values are JSON pointers");
                var ptr = kv.Value.GetString()!;
                if (reply.Pointer(SubstStr(ptr, vars)) is not { } x)
                    throw new ConformanceException($"{method}: nothing at {ptr} to save as {kv.Name}: {reply.Short()}");
                vars[kv.Name] = x;
            }
        }
        return true;
    }

    /// <summary>`$var` in a string: each replaced by its text (a string's own, else its JSON).</summary>
    public static string SubstStr(string s, IReadOnlyDictionary<string, JsonElement> vars)
    {
        var sb = new StringBuilder();
        int i = 0;
        while (i < s.Length)
        {
            int k = s.IndexOf('$', i);
            if (k < 0)
            {
                sb.Append(s, i, s.Length - i);
                break;
            }
            sb.Append(s, i, k - i);
            int end = k + 1;
            while (end < s.Length && (char.IsAsciiLetterOrDigit(s[end]) || s[end] == '_')) end++;
            var name = s[(k + 1)..end];
            if (name.Length > 0 && vars.TryGetValue(name, out var v))
                sb.Append(v.ValueKind == JsonValueKind.String ? v.GetString() : v.Text());
            else
                sb.Append('$').Append(name);
            i = end;
        }
        // "$scratch/name" names a file in the case's scratch folder: on Windows its separators are the platform's
        // (the core reports such a path natively), the same whether it is sent or expected
        if (Path.DirectorySeparatorChar == '\\' && s.StartsWith("$scratch/", StringComparison.Ordinal) && vars.TryGetValue("scratch", out var sv) && sv.ValueKind == JsonValueKind.String)
        {
            var head = sv.GetString()!;
            var whole = sb.ToString();
            if (whole.StartsWith(head, StringComparison.Ordinal)) return head + whole[head.Length..].Replace('/', '\\');
        }
        return sb.ToString();
    }

    /// <summary>"$v" whole is the saved value (any JSON); keys and longer strings take its text.</summary>
    public static JsonElement Subst(JsonElement v, IReadOnlyDictionary<string, JsonElement> vars) => v.ValueKind switch
    {
        JsonValueKind.String when v.GetString() is { } s && s.StartsWith('$') && vars.TryGetValue(s[1..], out var val) => val,
        JsonValueKind.String => Jsonv.Of(SubstStr(v.GetString()!, vars)),
        JsonValueKind.Array => Jsonv.Array(v.EnumerateArray().Select(x => Subst(x, vars)).ToList()),
        JsonValueKind.Object => Jsonv.Object([.. v.EnumerateObject().Select(p => (SubstStr(p.Name, vars), Subst(p.Value, vars)))]),
        _ => v,
    };

    /// <summary>{"$locate": ..., "$plus"?: [dx, dy]} anywhere in `v`: the point geometry.locate gives.</summary>
    static async Task<JsonElement> ResolveAsync(IConformanceDriver d, JsonElement v)
    {
        switch (v.ValueKind)
        {
            case JsonValueKind.Object when v.TryGetProperty("$locate", out var what):
                var r = await d.CallAsync("geometry.locate", what);
                if (!r.Get("ok").IsTrue()) throw new ConformanceException($"geometry.locate {what.Short()}: {r.Short()}");
                if (r.Pointer("/result/at/0").Double() is not { } x || r.Pointer("/result/at/1").Double() is not { } y)
                    throw new ConformanceException($"geometry.locate gave no point: {r.Short()}");
                var plus = v.Get("$plus");
                double dx = plus?.Pointer("/0").Double() ?? 0, dy = plus?.Pointer("/1").Double() ?? 0;
                return Jsonv.Array([Jsonv.Of(x + dx), Jsonv.Of(y + dy)]);
            case JsonValueKind.Object:
                var fields = new List<(string, JsonElement)>();
                foreach (var p in v.EnumerateObject()) fields.Add((p.Name, await ResolveAsync(d, p.Value)));
                return Jsonv.Object([.. fields]);
            case JsonValueKind.Array:
                var items = new List<JsonElement>();
                foreach (var x2 in v.EnumerateArray()) items.Add(await ResolveAsync(d, x2));
                return Jsonv.Array(items);
            default:
                return v;
        }
    }

    static void Expect(JsonElement reply, JsonElement? want, IReadOnlyDictionary<string, JsonElement> vars)
    {
        if (want is not { ValueKind: JsonValueKind.Object } w) return;
        foreach (var kv in w.EnumerateObject())
        {
            var p = SubstStr(kv.Name, vars);
            var wv = Subst(kv.Value, vars);
            try
            {
                Matches(reply.Pointer(p), wv);
            }
            catch (ConformanceException e)
            {
                throw new ConformanceException($"at {p}: {e.Message}; the reply: {reply.Short()}");
            }
        }
    }

    static bool DeepHas(JsonElement v, JsonElement fields)
    {
        switch (v.ValueKind)
        {
            case JsonValueKind.Object:
                bool all = true;
                foreach (var f in fields.EnumerateObject())
                    if (!FieldMatches(v, f.Name, f.Value)) { all = false; break; }
                return all || v.EnumerateObject().Any(p => DeepHas(p.Value, fields));
            case JsonValueKind.Array:
                return v.EnumerateArray().Any(x => DeepHas(x, fields));
            default:
                return false;
        }
    }

    /// <summary>A `$deep` field: a plain want compared as a value; a want that is a matcher (one `$` key)
    /// matched against the field, which may be missing (docs/shell-interface.md 16, copied manifest).</summary>
    static bool FieldMatches(JsonElement v, string name, JsonElement want)
    {
        JsonElement? got = v.TryGetProperty(name, out var x) ? x : null;
        if (IsMatcher(want))
        {
            try { Matches(got, want); return true; }
            catch (ConformanceException) { return false; }
        }
        return got is { } g && Jsonv.Same(g, want);
    }

    static bool IsMatcher(JsonElement w) =>
        w.ValueKind == JsonValueKind.Object && w.EnumerateObject().Count() == 1 && w.EnumerateObject().First().Name.StartsWith('$');

    static bool HasAll(JsonElement item, JsonElement fields) =>
        item.ValueKind == JsonValueKind.Object && fields.EnumerateObject().All(f => item.TryGetProperty(f.Name, out var x) && Jsonv.Same(x, f.Value));

    static string Nothing(JsonElement? got) => got is { } g ? g.Short() : "nothing";

    /// <summary>`got` (null: nothing there) against `want`: a value, or a one-key `$matcher` object.</summary>
    public static void Matches(JsonElement? got, JsonElement want)
    {
        if (want.ValueKind == JsonValueKind.Object)
        {
            var props = want.EnumerateObject().ToList();
            if (props.Count == 1 && props[0].Name.StartsWith('$'))
            {
                var k = props[0].Name;
                var arg = props[0].Value;
                bool ok;
                switch (k)
                {
                    case "$exists":
                        ok = (got is not null) == (((JsonElement?)arg).Bool() ?? true);
                        break;
                    case "$len":
                        long? n = got switch
                        {
                            { ValueKind: JsonValueKind.Array } g => g.GetArrayLength(),
                            { ValueKind: JsonValueKind.Object } g => g.EnumerateObject().Count(),
                            _ => null,
                        };
                        ok = n is { } count && ((JsonElement?)arg).U64() is { } want2 && (ulong)count == want2;
                        break;
                    case "$contains":
                        ok = got is { ValueKind: JsonValueKind.Array } ga && ga.EnumerateArray().Any(x => Jsonv.Same(x, arg));
                        break;
                    case "$deep":
                        ok = arg.ValueKind == JsonValueKind.Object && got is { } gd && DeepHas(gd, arg);
                        break;
                    case "$includes":
                        ok = arg.ValueKind == JsonValueKind.String && got is { ValueKind: JsonValueKind.String } gs && gs.GetString()!.Contains(arg.GetString()!, StringComparison.Ordinal);
                        break;
                    case "$not":
                        ok = got is not { } gn || !Jsonv.Same(gn, arg);
                        break;
                    case "$one":
                        ok = arg.ValueKind == JsonValueKind.Array && got is { } go && arg.EnumerateArray().Any(x => Jsonv.Same(go, x));
                        break;
                    case "$min":
                        ok = ((JsonElement?)arg).Double() is { } min && got.Double() is { } gv && gv >= min;
                        break;
                    case "$find":
                        if (arg.Get("where") is not { ValueKind: JsonValueKind.Object } where) throw new ConformanceException("$find needs where: {field: value}");
                        JsonElement? found = got is { ValueKind: JsonValueKind.Array } gf ? gf.EnumerateArray().Where(x => HasAll(x, where)).Select(x => (JsonElement?)x).FirstOrDefault() : null;
                        if (found is not { } item) throw new ConformanceException($"no item with {where.Text()} in {Nothing(got)}");
                        if (arg.Get("want") is { ValueKind: JsonValueKind.Object } wants)
                        {
                            foreach (var wp in wants.EnumerateObject())
                            {
                                try { Matches(item.Pointer(wp.Name), wp.Value); }
                                catch (ConformanceException e) { throw new ConformanceException($"the item with {where.Text()}: at {wp.Name}: {e.Message}"); }
                            }
                        }
                        ok = true;
                        break;
                    default:
                        throw new ConformanceException($"unknown matcher {k}");
                }
                if (!ok) throw new ConformanceException($"wanted {k} {arg.Text()}, got {Nothing(got)}");
                return;
            }
        }
        if (got is not { } have) throw new ConformanceException($"wanted {want.Text()}, got nothing");
        if (!Jsonv.Same(have, want)) throw new ConformanceException($"wanted {want.Text()}, got {have.Short()}");
    }

    /// <summary>The runner's own checks: tokensResolved {scene, theme} and nidsUnique {scene}.</summary>
    public static void Check(string name, JsonElement s)
    {
        switch (name)
        {
            case "tokensResolved":
                if (s.Pointer("/theme/tokens") is not { ValueKind: JsonValueKind.Object } tokens) throw new ConformanceException("theme.tokens is not a map");
                var named = new HashSet<string>(StringComparer.Ordinal);
                var bad = new List<string>();
                Paints(s.Get("scene") ?? Jsonv.Null, named, bad);
                if (bad.Count > 0) throw new ConformanceException($"paints with both a token and a colour, or neither: {string.Join(", ", bad.Take(5))}");
                if (named.Count == 0) throw new ConformanceException("the scene names no token");
                var missing = named.Where(t => !tokens.TryGetProperty(t, out _)).Order(StringComparer.Ordinal).ToList();
                if (missing.Count > 0) throw new ConformanceException($"theme.get lacks {string.Join(", ", missing)}");
                break;
            case "nidsUnique":
                var seen = new Dictionary<ulong, (string kind, JsonElement id)>();
                foreach (var kind in new[] { "nodes", "wires", "nets", "frames", "groups", "marks" })
                {
                    if (s.Pointer($"/scene/{kind}") is not { ValueKind: JsonValueKind.Array } items) continue;
                    foreach (var item in items.EnumerateArray())
                    {
                        if (item.Get("nid").U64() is not { } nid) throw new ConformanceException($"{kind} item without a nid: {item.Short()}");
                        var id = item.Get("id") ?? Jsonv.Null;
                        if (seen.TryGetValue(nid, out var other)) throw new ConformanceException($"nid {nid} is both {other.kind} {other.id.Text()} and {kind} {id.Text()}");
                        seen[nid] = (kind, id);
                    }
                }
                if (seen.Count < 2) throw new ConformanceException("too few items to check");
                break;
            default:
                throw new ConformanceException($"unknown check {name}");
        }
    }

    static void Paints(JsonElement v, HashSet<string> named, List<string> bad)
    {
        switch (v.ValueKind)
        {
            case JsonValueKind.Object:
                var token = v.Get("token").Str();
                var color = v.Get("color") is { ValueKind: JsonValueKind.Array } c && c.GetArrayLength() == 4 ? c : (JsonElement?)null;
                bool paintish = (token is not null || color is not null) && v.EnumerateObject().All(p => p.Name is "token" or "color");
                if (paintish)
                {
                    if (token is not null && color is null) named.Add(token);
                    else if (token is null && color is not null) { }
                    else bad.Add(v.Text());
                }
                else
                {
                    foreach (var p in v.EnumerateObject()) Paints(p.Value, named, bad);
                }
                break;
            case JsonValueKind.Array:
                foreach (var x in v.EnumerateArray()) Paints(x, named, bad);
                break;
        }
    }
}
