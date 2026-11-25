// The C# face of plumcraft-ffi (crates/ffi/include/plumcraft.h), the Windows counterpart of the
// Swift kit's PlumCore (native/apple/PlumCraftFFI/Sources/PlumCraft/PlumCore.swift).
//
// Threading (docs/shell-interface.md 5.2 and 5.4):
// - The core's wake is an [UnmanagedCallersOnly] function: it only sets an event (no allocation,
//   no lock, no call back into the library), so the core thread never waits on .NET; wakes between
//   two polls coalesce.
// - One pump thread (not the UI thread) waits on that event and polls until the core has nothing
//   more. Each poll is parsed once (System.Text.Json); each reply completes its request's task
//   (continuations run asynchronously, never on the pump); the rest of the poll goes to every
//   subscriber (Subscribe: a FeedBatch per poll, in order, none dropped) and to the spike's raw
//   Feed event, on the pump thread, which hop to their own threads themselves.
// - Requests may be sent from any thread; nothing here blocks the caller on the core. A typed
//   request decodes its reply on the thread pool, so a 5 MB scene never decodes on a UI thread.
// - The hot calls are synchronous and read the core's latest snapshot (contract 9.4).
// - Nothing here throws for what the core sent but the typed and checked requests, which turn
//   ok: false into a PlumException carrying the core's words and its `code`.
//
// `unsafe`: the constructor, RequestRawAsync, Post, Blob, WirePaths and GeometrySet take pointers to
// pinned managed memory for the length of one C call; Native.cs has the declarations. Nothing else
// in the kit uses pointers.

using System.Buffers;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Threading.Channels;
using PlumCraft.Models;

namespace PlumCraft.Core;

/// <summary>What went wrong.</summary>
public enum PlumErrorKind
{
    /// <summary>The core refused the request (CoreCode is its `code`, when it sent one).</summary>
    Refused,
    /// <summary>The core is closed, or closed while the request waited.</summary>
    Closed,
    /// <summary>plum_open refused the config.</summary>
    Open,
    /// <summary>A C call returned a status that isn't OK (PLUM_ERR_*).</summary>
    Status,
    /// <summary>A reply that isn't the shape asked for.</summary>
    Decode,
}

/// <summary>An error with the core's own words where it gave some.</summary>
public sealed class PlumException : Exception
{
    public PlumErrorKind Kind { get; }
    /// <summary>The core's `code` on a refusal (`bad_params`, `unknown_doc`, ...); sent only for some (6.1).</summary>
    public string? CoreCode { get; }

    public PlumException(string message) : this(PlumErrorKind.Refused, message) { }

    public PlumException(PlumErrorKind kind, string message, string? coreCode = null, Exception? inner = null) : base(message, inner)
    {
        Kind = kind;
        CoreCode = coreCode;
    }
}

/// <summary>Bytes the core lends without a copy; give them back with Dispose (plum_release).</summary>
public sealed class Lease : IDisposable
{
    public IntPtr Pointer { get; }
    public int Length { get; }
    public ulong Token { get; }
    private int released;

    internal Lease(PlumBytes b)
    {
        Pointer = b.Ptr;
        Length = checked((int)b.Len);
        Token = b.Token;
    }

    public ReadOnlySpan<byte> Span => released == 0 ? Native.Span(Pointer, (nuint)Length) : default;

    public void Dispose()
    {
        if (Interlocked.Exchange(ref released, 1) == 0) Native.plum_release(Token);
    }
}

/// <summary>One subscriber's view of the feed: every batch from when it subscribed, in order. Dispose to stop.</summary>
public sealed class FeedSubscription : IDisposable
{
    readonly PlumCore core;
    internal readonly Channel<FeedBatch> Channel = System.Threading.Channels.Channel.CreateUnbounded<FeedBatch>(new UnboundedChannelOptions { SingleReader = true, SingleWriter = true });
    internal FeedSubscription(PlumCore core) => this.core = core;
    public ChannelReader<FeedBatch> Reader => Channel.Reader;
    public void Dispose() => core.Unsubscribe(this);
}

public sealed class PlumCore : IDisposable
{
    public ulong Id { get; }
    private readonly AutoResetEvent wake = new(false);
    private readonly GCHandle self;
    private readonly Thread pump;
    private readonly ConcurrentDictionary<ulong, TaskCompletionSource<JsonElement>> pending = new();
    private readonly List<FeedSubscription> subscribers = [];
    private volatile bool closed;

    public long Wakes;
    public long Polls;
    public long Events;
    /// <summary>Time spent parsing polls on the pump thread, in ticks.</summary>
    public long ParseTicks;

    /// <summary>Everything in a poll but its replies, raw, on the pump thread (the spike's way; the kit subscribes).</summary>
    public event Action<JsonElement>? Feed;

    public static uint AbiVersion => Native.plum_abi_version();
    public static ulong Leases => Native.plum_leases();

    public bool IsClosed => closed;

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    private static void OnWake(IntPtr ctx)
    {
        if (ctx == IntPtr.Zero) return;
        if (GCHandle.FromIntPtr(ctx).Target is PlumCore c)
        {
            Interlocked.Increment(ref c.Wakes);
            c.wake.Set();
        }
    }

    public PlumCore(JsonNode config) : this(Encoding.UTF8.GetBytes(config.ToJsonString())) { }

    public PlumCore(JsonElement config) : this(Encoding.UTF8.GetBytes(config.GetRawText())) { }

    public unsafe PlumCore(byte[] configJson)
    {
        self = GCHandle.Alloc(this, GCHandleType.Normal);
        ulong id;
        fixed (byte* p = configJson)
        {
            id = Native.plum_open(p, (nuint)configJson.Length, &OnWake, GCHandle.ToIntPtr(self));
        }
        if (id == 0)
        {
            self.Free();
            throw new PlumException(PlumErrorKind.Open, "plum_open: " + Encoding.UTF8.GetString(Native.Take(Native.plum_last_error())));
        }
        Id = id;
        pump = new Thread(Pump) { IsBackground = true, Name = "plum pump" };
        pump.Start();
    }

    private void Pump()
    {
        while (!closed)
        {
            // a wake, or a look every half second in case one was missed
            wake.WaitOne(500);
            while (!closed)
            {
                var bytes = Native.Take(Native.plum_poll(Id));
                if (bytes.Length == 0) break;
                Interlocked.Increment(ref Polls);
                Route(bytes);
            }
        }
    }

    private void Route(byte[] bytes)
    {
        var t0 = Stopwatch.GetTimestamp();
        JsonDocument doc;
        try { doc = JsonDocument.Parse(bytes, new JsonDocumentOptions { MaxDepth = Jsonv.MaxDepth }); }
        catch (JsonException) { return; } // hostile input: skipped
        using (doc)
        {
            var root = doc.RootElement;
            Interlocked.Add(ref ParseTicks, Stopwatch.GetTimestamp() - t0);
            if (root.ValueKind != JsonValueKind.Object) return;
            if (root.TryGetProperty("replies", out var replies) && replies.ValueKind == JsonValueKind.Array)
            {
                foreach (var r in replies.EnumerateArray())
                {
                    if (r.ValueKind != JsonValueKind.Object || !r.TryGetProperty("id", out var idv) || !idv.TryGetUInt64(out var rid)) continue;
                    TaskCompletionSource<JsonElement>? tcs;
                    // the same lock RequestRawAsync holds across plum_request and its add
                    lock (pending) pending.TryRemove(rid, out tcs);
                    tcs?.TrySetResult(r.Clone());
                }
            }
            if (root.TryGetProperty("events", out var ev) && ev.ValueKind == JsonValueKind.Array)
                Interlocked.Add(ref Events, ev.GetArrayLength());
            bool more = false;
            foreach (var p in root.EnumerateObject())
                if (p.Name != "replies" && p.Name != "shell" && p.Name != "seq") { more = true; break; }
            if (!more) return;
            FeedSubscription[] subs;
            lock (subscribers) subs = [.. subscribers];
            if (subs.Length > 0)
            {
                var batch = FeedBatch.Parse(root);
                if (!batch.IsQuiet)
                    foreach (var s in subs) s.Channel.Writer.TryWrite(batch);
            }
            var feed = Feed;
            if (feed != null)
            {
                var clone = root.Clone();
                try { feed(clone); }
                catch (Exception) { /* a subscriber's fault is its own; the pump goes on */ }
            }
        }
    }

    /// <summary>Follow the feed: every batch from now on, in order, none dropped (asks must not be lost).
    /// The reader completes when the core closes.</summary>
    public FeedSubscription Subscribe()
    {
        var s = new FeedSubscription(this);
        lock (subscribers)
        {
            if (closed) s.Channel.Writer.TryComplete();
            else subscribers.Add(s);
        }
        return s;
    }

    internal void Unsubscribe(FeedSubscription s)
    {
        lock (subscribers) subscribers.Remove(s);
        s.Channel.Writer.TryComplete();
    }

    // ---------- requests ----------

    /// <summary>Queue a request; the reply ({id, ok, result | error, code?}) completes the task.</summary>
    public Task<JsonElement> RequestAsync(string method, JsonNode? parameters = null, ulong? doc = null)
    {
        var req = new JsonObject { ["method"] = method, ["params"] = parameters ?? new JsonObject() };
        if (doc is { } d) req["doc"] = d;
        return RequestRawAsync(Encoding.UTF8.GetBytes(req.ToJsonString()));
    }

    /// <summary>The same with params as an element (Undefined: none).</summary>
    public Task<JsonElement> RequestAsync(string method, JsonElement parameters, ulong? doc = null) => RequestRawAsync(Envelope(method, parameters, doc));

    /// <summary>{method, params, doc?} as bytes.</summary>
    public static byte[] Envelope(string method, JsonElement parameters, ulong? doc)
    {
        var buf = new ArrayBufferWriter<byte>(128);
        using (var w = new Utf8JsonWriter(buf))
        {
            w.WriteStartObject();
            w.WriteString("method", method);
            w.WritePropertyName("params");
            if (parameters.ValueKind == JsonValueKind.Undefined) { w.WriteStartObject(); w.WriteEndObject(); }
            else parameters.WriteTo(w);
            if (doc is { } d) w.WriteNumber("doc", d);
            w.WriteEndObject();
        }
        return buf.WrittenSpan.ToArray();
    }

    public unsafe Task<JsonElement> RequestRawAsync(byte[] json)
    {
        if (closed) return Task.FromException<JsonElement>(new PlumException(PlumErrorKind.Closed, "the core is closed"));
        var tcs = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        // the reply may come out of a poll on the pump thread before plum_request has returned
        // here; the pump looks replies up under this same lock, so it waits until the entry is in.
        lock (pending)
        {
            if (closed) return Task.FromException<JsonElement>(new PlumException(PlumErrorKind.Closed, "the core is closed"));
            ulong id;
            fixed (byte* p = json) id = Native.plum_request(Id, p, (nuint)json.Length);
            if (id == 0) return Task.FromException<JsonElement>(new PlumException(PlumErrorKind.Closed, "the core is closed"));
            pending[id] = tcs;
        }
        return tcs.Task;
    }

    /// <summary>Queue a request and don't wait (its reply is dropped). Its number, or 0 when closed.</summary>
    public unsafe ulong Post(string method, JsonElement parameters, ulong? doc = null)
    {
        if (closed) return 0;
        var json = Envelope(method, parameters, doc);
        fixed (byte* p = json) return Native.plum_request(Id, p, (nuint)json.Length);
    }

    /// <summary>A reply's result, or the core's refusal thrown.</summary>
    public static JsonElement Result(string method, JsonElement reply)
    {
        if (reply.ValueKind == JsonValueKind.Object && reply.TryGetProperty("ok", out var ok) && ok.ValueKind == JsonValueKind.True)
            return reply.TryGetProperty("result", out var res) ? res : Jsonv.Null;
        string err = reply.Get("error").Str() ?? reply.Get("error")?.GetRawText() ?? "the core refused it";
        throw new PlumException(PlumErrorKind.Refused, err, reply.Get("code").Str());
    }

    /// <summary>A request whose result is wanted: ok: false throws with the core's words (the spike's form,
    /// its message prefixed with the method).</summary>
    public async Task<JsonElement> CallAsync(string method, JsonNode? parameters = null, ulong? doc = null)
    {
        var r = await RequestAsync(method, parameters, doc).ConfigureAwait(false);
        try { return Result(method, r); }
        catch (PlumException e) { throw new PlumException(e.Kind, $"{method}: {e.Message}", e.CoreCode); }
    }

    /// <summary>A request's result: ok: false throws a PlumException with the core's words and code.</summary>
    public async Task<JsonElement> ResultAsync(string method, JsonElement parameters = default, ulong? doc = null)
    {
        var r = await RequestAsync(method, parameters, doc).ConfigureAwait(false);
        return Result(method, r);
    }

    /// <summary>A request whose result is decoded as T (lenient: fields this kit doesn't know are
    /// skipped), on the thread pool.</summary>
    public async Task<T> RequestAsync<T>(string method, JsonElement parameters = default, ulong? doc = null)
    {
        var r = await RequestAsync(method, parameters, doc).ConfigureAwait(false);
        var v = Result(method, r);
        try { return Shell.Decode<T>(v); }
        catch (JsonException e) { throw new PlumException(PlumErrorKind.Decode, $"{method}: the reply isn't a {typeof(T).Name}: {e.Message}", null, e); }
    }

    /// <summary>command.run, the way every user action enters the core.</summary>
    public Task<JsonElement> RunAsync(string command, JsonNode? parameters = null, ulong? doc = null) =>
        CallAsync("command.run", new JsonObject { ["command"] = command, ["params"] = parameters ?? new JsonObject() }, doc);

    /// <summary>command.run {command, params}: its result, or the refusal thrown.</summary>
    public Task<JsonElement> CommandAsync(string command, JsonElement parameters = default, ulong? doc = null) =>
        ResultAsync("command.run", Jsonv.Object(("command", Jsonv.Of(command)), ("params", parameters.ValueKind == JsonValueKind.Undefined ? Jsonv.EmptyObject : parameters)), doc);

    // ---------- bytes ----------

    /// <summary>Borrow a blob by handle: null when pending or missing (status says which).</summary>
    public unsafe Lease? Blob(string handle, out int status)
    {
        var h = Encoding.UTF8.GetBytes(handle);
        PlumBytes b;
        fixed (byte* p = h) b = Native.plum_blob(Id, p, (nuint)h.Length);
        status = b.Status;
        return b.Status == PlumStatus.Ok ? new Lease(b) : null;
    }

    // ---------- hot calls ----------

    public PlumHit HitTest(ulong doc, float x, float y, float slop) => Native.plum_hit_test(Id, doc, x, y, slop);

    public PlumHit WireTarget(ulong doc, ulong face, uint side, uint pin, float x, float y, float slop) =>
        Native.plum_wire_target(Id, doc, face, side, pin, x, y, slop);

    public PlumDrag GestureMove(ulong doc, float x, float y, float slop) => Native.plum_gesture_move(Id, doc, x, y, slop);

    /// <summary>The paths of every wire on these faces, moved by (dx, dy), into `buffer` (grown when it is
    /// too small; a canvas keeps one for a drag). The floats written, 0 for none; never more than the buffer.</summary>
    public unsafe int WirePaths(ulong doc, ReadOnlySpan<ulong> faces, float dx, float dy, ref float[] buffer)
    {
        if (faces.IsEmpty) return 0;
        // the snapshot may grow between two calls: ask again with room, a few times at most
        for (int attempt = 0; attempt < 4; attempt++)
        {
            nuint need;
            fixed (ulong* f = faces)
            fixed (float* o = buffer)
                need = Native.plum_wire_paths(Id, doc, f, (nuint)faces.Length, dx, dy, o, (nuint)buffer.Length);
            if (need == 0) return 0;
            if (need <= (nuint)buffer.Length) return (int)need;
            if (need > int.MaxValue / 2) return 0;
            buffer = new float[(int)need + (int)need / 4 + 64];
        }
        return 0;
    }

    /// <summary>Decode plum_wire_paths' floats: per wire (nid, points as x, y, ...). Stops at a count past
    /// the end (the C side's bug, never a crash here).</summary>
    public static List<(ulong nid, float[] pts)> DecodePaths(float[] buf, int n)
    {
        var outp = new List<(ulong, float[])>();
        n = Math.Min(n, buf.Length);
        int i = 0;
        while (i + 3 <= n)
        {
            uint lo = BitConverter.SingleToUInt32Bits(buf[i]), hi = BitConverter.SingleToUInt32Bits(buf[i + 1]), count = BitConverter.SingleToUInt32Bits(buf[i + 2]);
            i += 3;
            long len = (long)count * 2;
            if (i + len > n) break;
            outp.Add((((ulong)hi << 32) | lo, buf.AsSpan(i, (int)len).ToArray()));
            i += (int)len;
        }
        return outp;
    }

    /// <summary>One face's measurement for GeometrySet: graph units at zoom 1. Pins in row order (NaN: not
    /// measured), Header NaN when not said, Rows four floats a row (NaN for a row without a widget), or null.</summary>
    public readonly record struct FaceMeasure(ulong Face, ulong Rev, float W, float H, float Header, float[] Inputs, float[] Outputs, float[]? Rows);

    /// <summary>Report measured faces, typed (plum_geometry_set): copied before it returns, applied on the
    /// core's thread. The status (PLUM_OK, ...).</summary>
    public unsafe int GeometrySet(ulong doc, IReadOnlyList<FaceMeasure> faces)
    {
        var handles = new List<GCHandle>(faces.Count * 3);
        var arr = new PlumFace[faces.Count];
        try
        {
            IntPtr Pin(float[]? a)
            {
                if (a is null || a.Length == 0) return IntPtr.Zero;
                var h = GCHandle.Alloc(a, GCHandleType.Pinned);
                handles.Add(h);
                return h.AddrOfPinnedObject();
            }
            for (int i = 0; i < faces.Count; i++)
            {
                var f = faces[i];
                arr[i] = new PlumFace
                {
                    Face = f.Face, Rev = f.Rev, W = f.W, H = f.H, Header = f.Header,
                    Inputs = Pin(f.Inputs), NInputs = (nuint)(f.Inputs?.Length ?? 0),
                    Outputs = Pin(f.Outputs), NOutputs = (nuint)(f.Outputs?.Length ?? 0),
                    Rows = Pin(f.Rows), NRows = (nuint)((f.Rows?.Length ?? 0) / 4),
                };
            }
            fixed (PlumFace* p = arr) return Native.plum_geometry_set(Id, doc, p, (nuint)arr.Length);
        }
        finally
        {
            foreach (var h in handles) h.Free();
        }
    }

    /// <summary>Close the core: its documents, runs and threads (the C side waits for its thread). Requests
    /// still waiting fail as closed; subscribers' readers complete. A second close does nothing.</summary>
    public void Dispose()
    {
        lock (pending)
        {
            if (closed) return;
            closed = true;
        }
        Native.plum_close(Id);
        wake.Set();
        pump.Join(2000);
        foreach (var kv in pending) kv.Value.TrySetException(new PlumException(PlumErrorKind.Closed, "the core closed"));
        pending.Clear();
        FeedSubscription[] subs;
        lock (subscribers)
        {
            subs = [.. subscribers];
            subscribers.Clear();
        }
        foreach (var s in subs) s.Channel.Writer.TryComplete();
        self.Free();
    }
}
