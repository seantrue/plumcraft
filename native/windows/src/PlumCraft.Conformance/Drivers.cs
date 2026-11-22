// Two ways a Windows shell runs the suite (docs/shell-interface.md 16.1), as the Swift kit's
// Drivers.swift has them:
//
// - KitDriver: through the kit itself, as the app will: requests through PlumCore, a DocumentModel
//   following the feed, measurements reported typed (CanvasLink.Report, plum_geometry_set), the
//   drag's move through the typed hot call (CanvasLink.DragMove, plum_gesture_move).
// - PortDriver: over a core's control port (JSON lines over TCP, `auth` first), as 16.1 says a
//   shell's test harness drives its own core, from any language; or a running app's port, the app
//   measuring its own faces (`reports: false`).

using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using PlumCraft.Core;
using PlumCraft.Kit;
using PlumCraft.Models;

namespace PlumCraft.Conformance;

/// <summary>The suite through the Windows kit, on a core whose engine is `engine`. Make it, and run the
/// suite, on the model's context (a SerialContext headless).</summary>
public sealed class KitDriver : IConformanceDriver
{
    public PlumCore Core { get; }
    public DocumentModel Model { get; }
    public CanvasLink Canvas { get; }
    public string Engine { get; }

    public KitDriver(PlumCore core, string engine)
    {
        Core = core;
        Engine = engine;
        Model = new DocumentModel(core);
        Canvas = new CanvasLink(Model);
        Model.Start();
    }

    public string Name => $"the Windows kit ({Engine} engine)";

    public async Task<JsonElement> CallAsync(string method, JsonElement parameters)
    {
        JsonElement r;
        try
        {
            r = await Core.RequestAsync(method, parameters);
        }
        catch (Exception e)
        {
            return Jsonv.Object(("ok", Jsonv.False), ("error", Jsonv.Of(e.Message)));
        }
        if (r.Get("ok").IsTrue()) return Jsonv.Object(("ok", Jsonv.True), ("result", r.Get("result") ?? Jsonv.Null));
        var error = r.Get("error") ?? Jsonv.Of("the core refused it");
        return r.Get("code") is { ValueKind: JsonValueKind.String } code
            ? Jsonv.Object(("ok", Jsonv.False), ("error", error), ("code", code))
            : Jsonv.Object(("ok", Jsonv.False), ("error", error));
    }

    public bool Reports => true;

    static float[] Floats(JsonElement? v)
    {
        if (v is not { ValueKind: JsonValueKind.Array } a) return [];
        return [.. a.EnumerateArray().Select(x => x.ValueKind == JsonValueKind.Number ? (float)x.GetDouble() : float.NaN)];
    }

    /// <summary>The faces by graph id, typed by nid (the scene's), through CanvasLink.Report; their header
    /// parts after it (geometry.parts, as the canvas sends them); then wait until the core says each is measured.</summary>
    public async Task ReportGeometryAsync(JsonElement faces)
    {
        var scene = await Core.RequestAsync<Scene>("scene.get");
        var nids = scene.Nodes.ToDictionary(f => f.Id, f => f.Nid, StringComparer.Ordinal);
        if (faces.ValueKind != JsonValueKind.Array) throw new ConformanceException("faces must be a list");
        var typed = new List<PlumCore.FaceMeasure>();
        var parts = new List<JsonElement>();
        foreach (var f in faces.EnumerateArray())
        {
            if (f.Get("node").Str() is not { } id) throw new ConformanceException("a face's node must be an id");
            if (!nids.TryGetValue(id, out var nid)) throw new ConformanceException($"no face {id} in the scene");
            var size = Floats(f.Get("size"));
            float[]? rows = null;
            if (f.Get("rows") is { ValueKind: JsonValueKind.Array } rs && rs.GetArrayLength() > 0)
            {
                // four floats a row, NaN for a row without a widget, as a shell sends them
                var flat = new List<float>();
                foreach (var r in rs.EnumerateArray())
                {
                    var q = Floats(r);
                    flat.AddRange(q.Length == 4 ? q : [float.NaN, float.NaN, float.NaN, float.NaN]);
                }
                rows = [.. flat];
            }
            typed.Add(new PlumCore.FaceMeasure(nid, 0, size.Length > 0 ? size[0] : 0, size.Length > 1 ? size[1] : 0,
                f.Get("header").Double() is { } h ? (float)h : float.NaN,
                Floats(f.Pointer("/pins/in")), Floats(f.Pointer("/pins/out")), rows));
            if (f.Get("parts") is { } p) parts.Add(Jsonv.Object(("node", Jsonv.Of(id)), ("parts", p)));
        }
        Canvas.Report(typed);
        if (parts.Count > 0)
        {
            var r = await CallAsync("command.run", Jsonv.Object(("command", Jsonv.Of("geometry.parts")), ("params", Jsonv.Object(("faces", Jsonv.Array(parts))))));
            if (!r.Get("ok").IsTrue()) throw new ConformanceException($"geometry.parts: {r.Short()}");
        }
        // the core has them once a request after them sees them measured (they apply in order)
        var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(20);
        foreach (var f in faces.EnumerateArray())
        {
            while (true)
            {
                var r = await CallAsync("geometry.locate", Jsonv.Object(("node", f.Get("node") ?? Jsonv.Null)));
                if (r.Pointer("/result/measured").IsTrue()) break;
                if (DateTime.UtcNow > deadline) throw new ConformanceException($"never measured: {(f.Get("node") ?? Jsonv.Null).Text()}");
                await PauseAsync();
            }
        }
    }

    public Task SettleAsync() => Task.CompletedTask;

    static readonly string[] Kinds = ["none", "node", "pin", "wire", "bend", "tag", "frame", "group"];

    public Task<JsonElement?> HotMoveAsync(float x, float y, float slop) => Task.FromResult(HotMove(x, y, slop));

    /// <summary>plum_gesture_move, as the Rust driver writes it.</summary>
    public JsonElement? HotMove(float x, float y, float slop)
    {
        var d = Canvas.DragMove(new Point(x, y), slop);
        if (!d.Ok) return Jsonv.Object(("status", Jsonv.Of((long)d.Status)));
        var t = d.Target;
        return Jsonv.Build(w =>
        {
            w.WriteStartObject();
            w.WriteNumber("status", d.Status);
            w.WriteNumber("kind", d.Kind);
            w.WriteNumber("serial", d.Serial);
            w.WriteNumber("dx", d.Dx);
            w.WriteNumber("dy", d.Dy);
            w.WriteStartObject("target");
            w.WriteString("kind", t.Kind < Kinds.Length ? Kinds[t.Kind] : "?");
            w.WriteNumber("nid", t.Node);
            w.WriteNumber("pin", t.Pin);
            w.WriteNumber("fits", t.Fits);
            w.WriteStartArray("at");
            w.WriteNumberValue(t.X);
            w.WriteNumberValue(t.Y);
            w.WriteEndArray();
            w.WriteEndObject();
            if (d.Kind == 3)
            {
                w.WriteStartArray("rect");
                w.WriteNumberValue(d.R0);
                w.WriteNumberValue(d.R1);
                w.WriteNumberValue(d.R2);
                w.WriteNumberValue(d.R3);
                w.WriteEndArray();
            }
            if (d.ScrubValue is { } v) w.WriteNumber("value", v);
            w.WriteEndObject();
        });
    }

    /// <summary>Its engine, and media.get (the C ABI serves it).</summary>
    public bool Has(string capability) => capability == "media" || capability == $"engine:{Engine}";

    /// <summary>The next feed batch the model sees, or 50 ms.</summary>
    public Task PauseAsync() => Model.NextChangeAsync(TimeSpan.FromMilliseconds(50));
}

/// <summary>
/// The suite over a core's control port: a JSON line out, a JSON line back.
///
/// Two uses. A headless core (the kit's tests): the suite gives the measurements (`reports: true`). A
/// running app whose canvas measures its own faces: `reports: false`, so the suite's geometry steps
/// are skipped and Settle waits until the app has measured every face the scene has
/// (geometry.locate's `measured`); `doc` names the app's document, and `engines` what its core has.
/// </summary>
public sealed class PortDriver : IConformanceDriver, IPortCheck, IDisposable
{
    readonly LineConnection conn;
    ulong next;
    readonly DocumentModel? model;
    readonly HashSet<string> engines;
    readonly string token;

    public bool Reports { get; }
    /// <summary>The document every request names (null: the core's focused one).</summary>
    public ulong? Doc { get; }
    public string Host { get; }
    public int Port { get; }
    /// <summary>The app's core keeps its preferences in a file (settings.get {fromDisk} says so).</summary>
    public bool Prefs { get; }
    /// <summary>How long Settle waits for the app to measure.</summary>
    public TimeSpan SettleLimit { get; set; } = TimeSpan.FromSeconds(10);
    /// <summary>Settles that ran out of time (the app never measured every face), for the report.</summary>
    public int SettleTimeouts { get; private set; }

    PortDriver(LineConnection conn, string host, int port, string token, DocumentModel? model, bool reports, ulong? doc, IEnumerable<string> engines, bool prefs)
    {
        this.conn = conn;
        Host = host;
        Port = port;
        this.token = token;
        this.model = model;
        Reports = reports;
        Doc = doc;
        this.engines = [.. engines];
        Prefs = prefs;
    }

    /// <summary>Connect to host:port (loopback by default) and authenticate with `token`.</summary>
    public static async Task<PortDriver> ConnectAsync(int port, string token, string host = "127.0.0.1", DocumentModel? following = null,
        bool reports = true, ulong? doc = null, IEnumerable<string>? engines = null, bool prefs = false)
    {
        var conn = await LineConnection.OpenAsync(host, port);
        var auth = await conn.SendAsync(Jsonv.Object(("id", Jsonv.Of("auth")), ("method", Jsonv.Of("auth")), ("params", Jsonv.Object(("token", Jsonv.Of(token))))));
        if (!auth.Get("ok").IsTrue())
        {
            conn.Dispose();
            throw new ConformanceException($"auth: {auth.Short()}");
        }
        return new PortDriver(conn, host, port, token, following, reports, doc, engines ?? ["text"], prefs);
    }

    public string Name => Reports ? "the core's control port" : "an app's control port, measuring its own faces";

    public async Task<JsonElement> CallAsync(string method, JsonElement parameters)
    {
        next++;
        var fields = new List<(string, JsonElement)> { ("id", Jsonv.Of(next)), ("method", Jsonv.Of(method)), ("params", parameters) };
        if (Doc is { } d) fields.Add(("doc", Jsonv.Of(d)));
        try
        {
            return await conn.SendAsync(Jsonv.Object([.. fields]));
        }
        catch (Exception e)
        {
            return Jsonv.Object(("ok", Jsonv.False), ("error", Jsonv.Of(e.Message)));
        }
    }

    /// <summary>A running app measures its own faces; with SuiteGeometry (the default since N5 slice 4) the
    /// suite's `geometry` steps still run: report the suite's measurements over the app's once the app has
    /// measured those faces, and wait until the core holds them (the app reports a face again only when
    /// it changes).</summary>
    public bool SuiteGeometry { get; set; } = true;

    public async Task ReportGeometryAsync(JsonElement faces)
    {
        if (!Reports)
        {
            // the app's own report first, so the suite's comes after it
            var until = DateTime.UtcNow + SettleLimit;
            foreach (var f in faces.EnumerateArray())
                while (!(await CallAsync("geometry.locate", Jsonv.Object(("node", f.Get("node") ?? Jsonv.Null)))).Pointer("/result/measured").IsTrue() && DateTime.UtcNow < until)
                    await Task.Delay(10);
        }
        var r = await CallAsync("command.run", Jsonv.Object(("command", Jsonv.Of("geometry.set")), ("params", Jsonv.Object(("faces", faces)))));
        if (!r.Get("ok").IsTrue()) throw new ConformanceException($"geometry.set: {r.Short()}");
        if (Reports) return;
        // held: each face's rect is the suite's size (a face the app laid out again meanwhile is reported again)
        var deadline = DateTime.UtcNow + SettleLimit;
        for (int round = 0; ; round++)
        {
            bool all = true;
            foreach (var f in faces.EnumerateArray())
            {
                var at = await CallAsync("geometry.locate", Jsonv.Object(("node", f.Get("node") ?? Jsonv.Null)));
                var size = f.Get("size");
                var rect = at.Pointer("/result/rect");
                if (size is not { ValueKind: JsonValueKind.Array } sz || rect is not { ValueKind: JsonValueKind.Array } rc || rc.GetArrayLength() != 4) { all = false; break; }
                double w = rc[2].GetDouble() - rc[0].GetDouble(), h = rc[3].GetDouble() - rc[1].GetDouble();
                if (Math.Abs(w - sz[0].GetDouble()) > 1e-3 || Math.Abs(h - sz[1].GetDouble()) > 1e-3) { all = false; break; }
            }
            if (all) return;
            if (DateTime.UtcNow > deadline) throw new ConformanceException("the app's own measurements kept replacing the suite's");
            await Task.Delay(20);
            if (round % 10 == 9)
            {
                var again = await CallAsync("command.run", Jsonv.Object(("command", Jsonv.Of("geometry.set")), ("params", Jsonv.Object(("faces", faces)))));
                if (!again.Get("ok").IsTrue()) throw new ConformanceException($"geometry.set: {again.Short()}");
            }
        }
    }

    /// <summary>A headless core: nothing to wait for. An app: until it has measured every face in the
    /// scene, waiting on that condition (never a fixed time), at most SettleLimit.</summary>
    public async Task SettleAsync()
    {
        if (Reports) return;
        var deadline = DateTime.UtcNow + SettleLimit;
        while (DateTime.UtcNow < deadline)
        {
            if (await EveryFaceMeasured()) return;
            await Task.Delay(10);
        }
        SettleTimeouts++;
    }

    async Task<bool> EveryFaceMeasured()
    {
        var scene = await CallAsync("scene.get", Jsonv.EmptyObject);
        if (!scene.Get("ok").IsTrue()) return false;
        if (scene.Pointer("/result/nodes") is { ValueKind: JsonValueKind.Array } nodes)
            foreach (var f in nodes.EnumerateArray())
            {
                var r = await CallAsync("geometry.locate", Jsonv.Object(("node", f.Get("id") ?? Jsonv.Null)));
                if (!r.Pointer("/result/measured").IsTrue()) return false;
            }
        return true;
    }

    /// <summary>The typed move's JSON form over the port (geometry.dragMove, N5 slice 4).</summary>
    public async Task<JsonElement?> HotMoveAsync(float x, float y, float slop)
    {
        var r = await CallAsync("geometry.dragMove", Jsonv.Object(("at", Jsonv.Array([Jsonv.Of((double)x), Jsonv.Of((double)y)])), ("slop", Jsonv.Of((double)slop))));
        return r.Get("ok").IsTrue() ? r.Get("result") ?? Jsonv.Null : Jsonv.Object(("status", Jsonv.Of(-1L)), ("error", r.Get("error") ?? Jsonv.Null));
    }

    /// <summary>The engines the core has; media.get (a C ABI core's port serves it since N5 slice 4); an
    /// app's canvas measures its rows' widgets and header parts (`rows`), and with SuiteGeometry the suite's
    /// measurements are reported over its own (`reports`).</summary>
    public bool Has(string capability) => capability switch
    {
        "rows" => !Reports,
        "reports" => !Reports && SuiteGeometry,
        "media" => true,
        "port" => true,
        "prefs" => Prefs,
        _ => capability.StartsWith("engine:", StringComparison.Ordinal) && engines.Contains(capability[7..]),
    };

    /// <summary>The core's next feed batch when this process has the core, else 20 ms.</summary>
    public Task PauseAsync() => model is not null ? model.NextChangeAsync(TimeSpan.FromMilliseconds(50)) : Task.Delay(20);

    /// <summary>The port asks for its token: a second connection with a wrong token is refused, and so is
    /// a request sent before authenticating.</summary>
    public async Task AuthRefusedAsync()
    {
        var chars = new char[token.Length];
        for (int i = 0; i < token.Length; i++) chars[i] = token[token.Length - 1 - i] == '0' ? '1' : '0';
        var wrong = new string(chars);
        using (var a = await LineConnection.OpenAsync(Host, Port))
        {
            var r = await a.SendAsync(Jsonv.Object(("id", Jsonv.Of(1L)), ("method", Jsonv.Of("auth")), ("params", Jsonv.Object(("token", Jsonv.Of(wrong))))));
            if (r.Get("ok").Bool() != false || r.Get("error").Str() != "authentication required") throw new ConformanceException($"a wrong token was taken: {r.Short()}");
        }
        using (var b = await LineConnection.OpenAsync(Host, Port))
        {
            var g = await b.SendAsync(Jsonv.Object(("id", Jsonv.Of(2L)), ("method", Jsonv.Of("graph.get")), ("params", Jsonv.EmptyObject)));
            if (g.Get("ok").Bool() != false || g.Get("error").Str() != "authentication required") throw new ConformanceException($"a request before auth was served: {g.Short()}");
        }
    }

    public void Dispose() => conn.Dispose();
}

/// <summary>JSON lines over a TCP connection, one request at a time.</summary>
sealed class LineConnection : IDisposable
{
    readonly TcpClient client;
    readonly NetworkStream stream;
    readonly StreamReader reader;
    static readonly TimeSpan Limit = TimeSpan.FromSeconds(60);

    LineConnection(TcpClient client)
    {
        this.client = client;
        stream = client.GetStream();
        reader = new StreamReader(stream, new UTF8Encoding(false), false, 1 << 16, leaveOpen: true);
    }

    public static async Task<LineConnection> OpenAsync(string host, int port)
    {
        var c = new TcpClient { NoDelay = true };
        using var cts = new CancellationTokenSource(Limit);
        try
        {
            await c.ConnectAsync(host, port, cts.Token);
        }
        catch
        {
            c.Dispose();
            throw;
        }
        return new LineConnection(c);
    }

    /// <summary>Send one request and read one line back.</summary>
    public async Task<JsonElement> SendAsync(JsonElement request)
    {
        var line = Encoding.UTF8.GetBytes(request.GetRawText() + "\n");
        using var cts = new CancellationTokenSource(Limit);
        await stream.WriteAsync(line, cts.Token);
        var text = await reader.ReadLineAsync(cts.Token) ?? throw new ConformanceException("the port closed");
        try
        {
            return Jsonv.Parse(text);
        }
        catch (JsonException e)
        {
            return Jsonv.Object(("ok", Jsonv.False), ("error", Jsonv.Of($"not JSON: {e.Message}")));
        }
    }

    public void Dispose()
    {
        reader.Dispose();
        stream.Dispose();
        client.Dispose();
    }
}
