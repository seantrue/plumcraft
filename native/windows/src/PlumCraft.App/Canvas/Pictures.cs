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

    public Task<CanvasBitmap?> LoadAsync(string handle, TimeSpan? timeout = null)
    {
        if (cache.TryGetValue(handle, out var b))
        {
            order.Remove(handle);
            order.AddFirst(handle);
            return Task.FromResult<CanvasBitmap?>(b);
        }
        if (loading.TryGetValue(handle, out var t)) return t;
        t = Fetch(handle, timeout ?? TimeSpan.FromSeconds(30));
        loading[handle] = t;
        return t;
    }

    async Task<CanvasBitmap?> Fetch(string handle, TimeSpan timeout)
    {
        try
        {
            var note = await model.Core.RequestAsync<MediaNote>("media.get", Jsonv.Object(("handle", Jsonv.Of(handle)), ("want", Jsonv.Of("rgba"))), model.RequestDoc);
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
            var px = await Task.Run(() =>
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
            if (px is null) { LastFailure = "the bytes went before they were read"; return null; }
            var bmp = CanvasBitmap.CreateFromBytes(CanvasDevice.GetSharedDevice(), px, (int)w, (int)h, Windows.Graphics.DirectX.DirectXPixelFormat.B8G8R8A8UIntNormalized, 96, CanvasAlphaMode.Premultiplied);
            Put(handle, bmp, (long)w * h * 4);
            return bmp;
        }
        catch (Exception e)
        {
            LastFailure = e.Message;
            return null;
        }
        finally
        {
            loading.Remove(handle);
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
