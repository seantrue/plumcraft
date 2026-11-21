// The C# face of plumcraft-ffi (crates/ffi/include/plumcraft.h), the Windows counterpart of the
// Swift kit's PlumCore (native/apple/PlumCraftFFI/Sources/PlumCraft/PlumCore.swift).
//
// Threading (docs/shell-interface.md 5.2 and 5.4):
// - The core's wake is an [UnmanagedCallersOnly] function: it only sets an event (no allocation,
//   no lock, no call back into the library), so the core thread never waits on .NET.
// - One pump thread (not the UI thread) waits on that event and polls until the core has nothing
//   more. Each poll is parsed once (System.Text.Json); each reply completes its request's task;
//   the rest of the poll (events, changed, asks, media) goes to Feed subscribers on the pump
//   thread, which hop to the UI thread themselves (DispatcherQueue.TryEnqueue).
// - RequestAsync may be called from any thread; nothing here blocks the caller on the core.
// - The hot calls are synchronous and read the core's latest snapshot (contract 9.4).
// - Nothing here throws for the core's refusals except CallAsync, which turns ok: false into a
//   PlumException carrying the core's words.

using System.Collections.Concurrent;
using System.Diagnostics;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace PlumCraft.Core;

public sealed class PlumException(string message) : Exception(message);

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

public sealed class PlumCore : IDisposable
{
    public ulong Id { get; }
    private readonly AutoResetEvent wake = new(false);
    private readonly GCHandle self;
    private readonly Thread pump;
    private readonly ConcurrentDictionary<ulong, TaskCompletionSource<JsonElement>> pending = new();
    private volatile bool closed;

    public long Wakes;
    public long Polls;
    public long Events;
    /// <summary>Time spent parsing polls on the pump thread, in ticks.</summary>
    public long ParseTicks;

    /// <summary>Everything in a poll but its replies, on the pump thread.</summary>
    public event Action<JsonElement>? Feed;

    public static uint AbiVersion => Native.plum_abi_version();
    public static ulong Leases => Native.plum_leases();

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

    public unsafe PlumCore(JsonNode config)
    {
        self = GCHandle.Alloc(this, GCHandleType.Normal);
        var bytes = Encoding.UTF8.GetBytes(config.ToJsonString());
        ulong id;
        fixed (byte* p = bytes)
        {
            id = Native.plum_open(p, (nuint)bytes.Length, &OnWake, GCHandle.ToIntPtr(self));
        }
        if (id == 0)
        {
            self.Free();
            throw new PlumException("plum_open: " + Encoding.UTF8.GetString(Native.Take(Native.plum_last_error())));
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
                var t0 = Stopwatch.GetTimestamp();
                JsonDocument doc;
                try { doc = JsonDocument.Parse(bytes); }
                catch (JsonException) { continue; } // hostile input: skipped
                using (doc)
                {
                    var root = doc.RootElement;
                    Interlocked.Add(ref ParseTicks, Stopwatch.GetTimestamp() - t0);
                    if (root.ValueKind != JsonValueKind.Object) continue;
                    if (root.TryGetProperty("replies", out var replies) && replies.ValueKind == JsonValueKind.Array)
                    {
                        foreach (var r in replies.EnumerateArray())
                        {
                            if (!r.TryGetProperty("id", out var idv) || !idv.TryGetUInt64(out var rid)) continue;
                            TaskCompletionSource<JsonElement>? tcs;
                            // the same lock RequestRawAsync holds across plum_request and its add
                            lock (pending) pending.TryRemove(rid, out tcs);
                            tcs?.TrySetResult(r.Clone());
                        }
                    }
                    if (root.TryGetProperty("events", out var ev) && ev.ValueKind == JsonValueKind.Array)
                        Interlocked.Add(ref Events, ev.GetArrayLength());
                    var feed = Feed;
                    if (feed != null)
                    {
                        bool more = false;
                        foreach (var p in root.EnumerateObject())
                            if (p.Name != "replies") { more = true; break; }
                        if (more) feed(root.Clone());
                    }
                }
            }
        }
    }

    /// <summary>Queue a request; the reply ({id, ok, result | error}) completes the task.</summary>
    public Task<JsonElement> RequestAsync(string method, JsonNode? parameters = null, ulong? doc = null)
    {
        var req = new JsonObject { ["method"] = method, ["params"] = parameters ?? new JsonObject() };
        if (doc is { } d) req["doc"] = d;
        return RequestRawAsync(Encoding.UTF8.GetBytes(req.ToJsonString()));
    }

    public unsafe Task<JsonElement> RequestRawAsync(byte[] json)
    {
        if (closed) return Task.FromException<JsonElement>(new PlumException("the core is closed"));
        var tcs = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        // the reply may come out of a poll on the pump thread before plum_request has returned
        // here; the pump looks replies up under this same lock, so it waits until the entry is in.
        lock (pending)
        {
            ulong id;
            fixed (byte* p = json) id = Native.plum_request(Id, p, (nuint)json.Length);
            if (id == 0) return Task.FromException<JsonElement>(new PlumException("the core is closed"));
            pending[id] = tcs;
        }
        return tcs.Task;
    }

    /// <summary>A request whose result is wanted: ok: false throws with the core's words.</summary>
    public async Task<JsonElement> CallAsync(string method, JsonNode? parameters = null, ulong? doc = null)
    {
        var r = await RequestAsync(method, parameters, doc).ConfigureAwait(false);
        if (r.TryGetProperty("ok", out var ok) && ok.ValueKind == JsonValueKind.True)
            return r.TryGetProperty("result", out var res) ? res : default;
        var err = r.TryGetProperty("error", out var e) ? e.ToString() : r.ToString();
        throw new PlumException($"{method}: {err}");
    }

    /// <summary>command.run, the way every user action enters the core.</summary>
    public Task<JsonElement> RunAsync(string command, JsonNode? parameters = null, ulong? doc = null) =>
        CallAsync("command.run", new JsonObject { ["command"] = command, ["params"] = parameters ?? new JsonObject() }, doc);

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

    /// <summary>The paths of every wire on these faces, moved by (dx, dy): wire nid → points.</summary>
    public unsafe int WirePaths(ulong doc, ReadOnlySpan<ulong> faces, float dx, float dy, ref float[] buffer)
    {
        fixed (ulong* f = faces)
        {
            nuint need;
            fixed (float* o = buffer) need = Native.plum_wire_paths(Id, doc, f, (nuint)faces.Length, dx, dy, o, (nuint)buffer.Length);
            if (need > (nuint)buffer.Length)
            {
                buffer = new float[(int)need * 2];
                fixed (float* o = buffer) need = Native.plum_wire_paths(Id, doc, f, (nuint)faces.Length, dx, dy, o, (nuint)buffer.Length);
            }
            return (int)need;
        }
    }

    /// <summary>Decode plum_wire_paths' floats: per wire (nid, points).</summary>
    public static List<(ulong nid, float[] pts)> DecodePaths(float[] buf, int n)
    {
        var outp = new List<(ulong, float[])>();
        int i = 0;
        while (i + 3 <= n)
        {
            uint lo = BitConverter.SingleToUInt32Bits(buf[i]), hi = BitConverter.SingleToUInt32Bits(buf[i + 1]), count = BitConverter.SingleToUInt32Bits(buf[i + 2]);
            i += 3;
            int len = (int)count * 2;
            if (i + len > n) break;
            outp.Add((((ulong)hi << 32) | lo, buf.AsSpan(i, len).ToArray()));
            i += len;
        }
        return outp;
    }

    /// <summary>One face's measurement for GeometrySet.</summary>
    public readonly record struct FaceMeasure(ulong Face, ulong Rev, float W, float H, float Header, float[] Inputs, float[] Outputs, float[]? Rows);

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
                    Inputs = Pin(f.Inputs), NInputs = (nuint)f.Inputs.Length,
                    Outputs = Pin(f.Outputs), NOutputs = (nuint)f.Outputs.Length,
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

    public void Dispose()
    {
        if (closed) return;
        closed = true;
        Native.plum_close(Id);
        wake.Set();
        pump.Join(2000);
        foreach (var kv in pending) kv.Value.TrySetException(new PlumException("the core closed"));
        pending.Clear();
        self.Free();
    }
}
