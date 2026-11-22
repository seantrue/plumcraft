// What the canvas and the pop-outs ask the core for besides the scene: the node icons (`icon.get`, the
// Lucide set the core embeds, drawn by Direct2D's SVG renderer) and pictures (`media.get {handle, want:
// "rgba"}`, then `plum_blob`, which lends the bytes without a copy; they become a Win2D bitmap here and
// go back at once). A handle names the same bytes forever, so it is the cache's key (contract 10.1).

using System.Collections.Concurrent;
using System.Text.Json;
using Microsoft.Graphics.Canvas;
using PlumCraft.Kit;
using PlumCraft.Models;

namespace PlumCraft.App.Canvas;

/// <summary>The node icons fetched so far, by name: any thread reads, the UI thread fills.</summary>
public sealed class Icons(DocumentModel model)
{
    readonly ConcurrentDictionary<string, string> svg = new(StringComparer.Ordinal);
    readonly HashSet<string> asked = new(StringComparer.Ordinal);

    public string? Svg(string? name) => name is not null && svg.TryGetValue(name, out var s) ? s : null;

    /// <summary>Fetch the names not asked for yet; `then` once some came.</summary>
    public async void Want(IEnumerable<string> names, Action then)
    {
        var fresh = names.Where(n => n.Length > 0 && asked.Add(n)).Distinct().ToList();
        if (fresh.Count == 0) return;
        try
        {
            var r = await model.Core.ResultAsync("icon.get", Jsonv.Object(("names", Jsonv.Array(fresh.Select(n => Jsonv.Of(n))))), model.RequestDoc);
            int got = 0;
            if (r.Get("icons") is { ValueKind: JsonValueKind.Object } icons)
                foreach (var p in icons.EnumerateObject())
                    if (p.Value.Get("svg").Str() is { } s && s.Length < 64 * 1024) { svg[p.Name] = s; got++; }
            if (got > 0) then();
        }
        catch (Exception e)
        {
            // no icons from this core: faces are drawn without them
            CoreHost.Log("icon.get failed: " + e.Message);
        }
    }
}

/// <summary>Pictures the core lends, as Win2D bitmaps, cached by handle (the least recently used go
/// first past the budget).</summary>
public sealed class Pictures(DocumentModel model, long budget = 128L << 20)
{
    readonly Dictionary<string, CanvasBitmap> cache = new(StringComparer.Ordinal);
    readonly LinkedList<string> order = new();
    readonly Dictionary<string, Task<CanvasBitmap?>> loading = new(StringComparer.Ordinal);
    long bytes;
    public string? LastFailure { get; private set; }

    /// <summary>The handle a value reference names (a preview's `handle`, a watched picture's `ref`).</summary>
    public static string? HandleOf(JsonElement r) => r.ValueKind == JsonValueKind.String ? r.GetString() : r.Get("handle").Str();

    /// <summary>A picture the core makes of `handle` (N5 slice 4: any `want` that comes back as RGBA8: `rgba`, the
    /// engine's rendition; `image`, a picture file decoded at most `px` a side; `svg`, drawn at `px`;
    /// `spectrogram`), cached by what was asked.</summary>
    public Task<CanvasBitmap?> LoadAsync(string handle, TimeSpan? timeout = null, string want = "rgba", int? px = null)
    {
        var k = want == "rgba" && px is null ? handle : $"{want}@{px}:{handle}";
        if (cache.TryGetValue(k, out var b))
        {
            order.Remove(k);
            order.AddFirst(k);
            return Task.FromResult<CanvasBitmap?>(b);
        }
        if (loading.TryGetValue(k, out var t)) return t;
        t = Fetch(k, handle, want, px, timeout ?? TimeSpan.FromSeconds(30));
        loading[k] = t;
        return t;
    }

    public Task<CanvasBitmap?> LoadAsync(string handle, string want, int? px = null) => LoadAsync(handle, null, want, px);

    /// <summary>Why the last picture of each handle couldn't be had.</summary>
    public Dictionary<string, string> Failures { get; } = new(StringComparer.Ordinal);

    async Task<CanvasBitmap?> Fetch(string k, string handle, string want, int? px, TimeSpan timeout)
    {
        LastFailure = null;
        try
        {
            var ask = new List<(string, JsonElement)> { ("handle", Jsonv.Of(handle)), ("want", Jsonv.Of(want)) };
            if (px is { } p) ask.Add(("px", Jsonv.Of((long)p)));
            var note = await model.Core.RequestAsync<MediaNote>("media.get", Jsonv.Object([.. ask]), model.RequestDoc);
            var key = note.Key;
            if (note.StatusWord == MediaWord.Pending)
            {
                var ok = await model.WaitAsync(() => model.Media.TryGetValue(key, out var m) && m.StatusWord != MediaWord.Pending, timeout);
                if (!ok || !model.Media.TryGetValue(key, out var m2)) { LastFailure = "the picture didn't come in time"; return null; }
                note = m2;
            }
            if (note.StatusWord != MediaWord.Ready || note.W is not { } w || note.H is not { } h) { LastFailure = note.Reason ?? $"the picture is {note.Status}"; return null; }
            var core = model.Core;
            // the bytes, straight RGBA (10.2), premultiplied into BGRA for Direct2D on a worker
            var pixels = await Task.Run(() =>
            {
                using var lease = core.Blob(key, out _);
                if (lease is null || lease.Length < (long)w * h * 4) return null;
                var src = lease.Span;
                var dst = new byte[(int)(w * h * 4)];
                for (int i = 0; i < dst.Length; i += 4)
                {
                    int r = src[i], g = src[i + 1], b = src[i + 2], a = src[i + 3];
                    dst[i] = (byte)(b * a / 255); dst[i + 1] = (byte)(g * a / 255); dst[i + 2] = (byte)(r * a / 255); dst[i + 3] = (byte)a;
                }
                return dst;
            });
            if (pixels is null) { LastFailure = "the bytes went before they were read"; return null; }
            var bmp = CanvasBitmap.CreateFromBytes(CanvasDevice.GetSharedDevice(), pixels, (int)w, (int)h, Windows.Graphics.DirectX.DirectXPixelFormat.B8G8R8A8UIntNormalized, 96, CanvasAlphaMode.Premultiplied);
            Put(k, bmp, (long)w * h * 4);
            return bmp;
        }
        catch (Exception e)
        {
            LastFailure = e.Message;
            return null;
        }
        finally
        {
            loading.Remove(k);
            if (LastFailure is { } why) Failures[handle] = why;
        }
    }

    /// <summary>What the core makes of `handle` as bytes (N5 slice 4: a waveform's peaks, a sound's samples,
    /// a value's own bytes), copied out of the lent buffer on a worker, with the note that says their shape;
    /// null when they couldn't be had (`Failures` says why). At most `cap` bytes.</summary>
    public async Task<(MediaNote note, byte[] bytes)?> LendAsync(string handle, string want, int? px = null, long cap = 512L << 20, TimeSpan? timeout = null)
    {
        try
        {
            var ask = new List<(string, JsonElement)> { ("handle", Jsonv.Of(handle)), ("want", Jsonv.Of(want)) };
            if (px is { } p) ask.Add(("px", Jsonv.Of((long)p)));
            var note = await model.Core.RequestAsync<MediaNote>("media.get", Jsonv.Object([.. ask]), model.RequestDoc);
            var key = note.Key;
            if (note.StatusWord == MediaWord.Pending)
            {
                var ok = await model.WaitAsync(() => model.Media.TryGetValue(key, out var m) && m.StatusWord != MediaWord.Pending, timeout ?? TimeSpan.FromSeconds(60));
                if (!ok || !model.Media.TryGetValue(key, out var m2)) { Failures[handle] = "it didn't come in time"; return null; }
                note = m2;
            }
            if (note.StatusWord != MediaWord.Ready) { Failures[handle] = note.Reason ?? $"it is {note.Status}"; return null; }
            var core = model.Core;
            var copy = await Task.Run(() =>
            {
                using var lease = core.Blob(key, out _);
                if (lease is null || lease.Length > cap) return null;
                return lease.Span.ToArray();
            });
            if (copy is null) { Failures[handle] = "the bytes went before they were read, or are too many"; return null; }
            return (note, copy);
        }
        catch (Exception e)
        {
            Failures[handle] = e.Message;
            return null;
        }
    }

    void Put(string handle, CanvasBitmap b, long size)
    {
        cache[handle] = b;
        order.AddFirst(handle);
        bytes += size;
        while (bytes > budget && order.Count > 1 && order.Last is { } last)
        {
            order.RemoveLast();
            if (cache.Remove(last.Value, out var old))
            {
                bytes -= (long)old.SizeInPixels.Width * old.SizeInPixels.Height * 4;
                // not disposed: a visual may still draw from a surface made from it (surfaces copy)
            }
        }
    }
}
