// The core's scene (docs/shell-interface.md 8), kept by nid and applied from scene.get {since}
// deltas: the part of the Swift kit's DocumentModel a canvas needs. Spike code: only the fields the
// canvas draws are read; anything unknown is skipped (contract 3.1, a shell's parsers skip what
// they don't know).

using System.Numerics;
using System.Text.Json;
using Windows.UI;

namespace PlumCraft.CanvasSpike;

public sealed class Row
{
    public string Key = "";
    public string Label = "";
    public string? Text;
    public string Source = "";
    public Color Pin;
    public bool Square;
    public string? Value;
}

public sealed class Face
{
    public ulong Nid;
    public string Id = "";
    public ulong Rev;
    public Vector2 Pos;
    public string Title = "";
    public Color Header, Body, Border;
    public float BorderWidth;
    public string? Run;
    public bool Selected;
    public List<Row> Inputs = [];
    public List<Row> Outputs = [];
    public List<string> Footer = [];
    public List<string> Status = [];
    public FaceLayout? Layout;
}

public sealed class Wire
{
    public ulong Nid;
    public string Id = "";
    public ulong Rev;
    public Color Paint;
    public Vector2[] Path = [];
    public string FromFace = "", ToFace = "";
}

public sealed class SceneModel
{
    public ulong Gen;
    public readonly Dictionary<ulong, Face> Faces = [];
    public readonly Dictionary<ulong, Wire> Wires = [];
    public readonly Dictionary<string, ulong> FaceByAgent = [];
    public Dictionary<string, Color> Tokens = [];
    public float[]? FitRect;
    public long FitSeq;
    public HashSet<string> SelectedIds = [];

    public static Color Rgba(JsonElement a)
    {
        if (a.ValueKind != JsonValueKind.Array || a.GetArrayLength() < 3) return Color.FromArgb(255, 255, 0, 255);
        byte B(int i) => (byte)Math.Clamp(i < a.GetArrayLength() && a[i].TryGetDouble(out var d) ? d : 255, 0, 255);
        return Color.FromArgb(B(3), B(0), B(1), B(2));
    }

    public void SetTheme(JsonElement theme)
    {
        Tokens = [];
        if (theme.TryGetProperty("tokens", out var t) && t.ValueKind == JsonValueKind.Object)
            foreach (var p in t.EnumerateObject()) Tokens[p.Name] = Rgba(p.Value);
    }

    public Color Token(string name, Color fallback) => Tokens.TryGetValue(name, out var c) ? c : fallback;

    /// <summary>A paint: {token} or {color: [r, g, b, a]} (premultiplied, contract 8).</summary>
    public Color Paint(JsonElement p, Color fallback)
    {
        if (p.ValueKind != JsonValueKind.Object) return fallback;
        if (p.TryGetProperty("token", out var tk) && tk.GetString() is { } name) return Token(name, fallback);
        if (p.TryGetProperty("color", out var c)) return Rgba(c);
        return fallback;
    }

    static string? Str(JsonElement o, string k) => o.ValueKind == JsonValueKind.Object && o.TryGetProperty(k, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

    static ulong U64(JsonElement o, string k) => o.TryGetProperty(k, out var v) && v.TryGetUInt64(out var u) ? u : 0;

    Row ReadRow(JsonElement r)
    {
        var row = new Row { Key = Str(r, "key") ?? "", Label = Str(r, "label") ?? "", Text = Str(r, "text"), Source = Str(r, "source") ?? "" };
        if (r.TryGetProperty("pin", out var pin) && pin.ValueKind == JsonValueKind.Object)
        {
            row.Pin = pin.TryGetProperty("fill", out var f) ? Paint(f, Token("pin_default", Color.FromArgb(255, 128, 128, 128))) : Token("pin_default", Color.FromArgb(255, 128, 128, 128));
            row.Square = Str(pin, "shape") == "square";
        }
        if (r.TryGetProperty("widget", out var w) && w.ValueKind == JsonValueKind.Object && w.TryGetProperty("value", out var v))
            row.Value = v.ValueKind switch
            {
                JsonValueKind.String => v.GetString(),
                JsonValueKind.Number => v.GetRawText(),
                JsonValueKind.True => "on",
                JsonValueKind.False => "off",
                _ => null,
            };
        return row;
    }

    Face ReadFace(JsonElement n)
    {
        var f = new Face { Nid = U64(n, "nid"), Id = Str(n, "id") ?? "", Rev = U64(n, "rev") };
        if (n.TryGetProperty("pos", out var pos) && pos.GetArrayLength() >= 2) f.Pos = new Vector2((float)pos[0].GetDouble(), (float)pos[1].GetDouble());
        if (n.TryGetProperty("header", out var h) && h.ValueKind == JsonValueKind.Object)
        {
            if (h.TryGetProperty("title", out var t)) f.Title = Str(t, "text") ?? "";
            if (h.TryGetProperty("status", out var st) && st.ValueKind == JsonValueKind.Array)
                foreach (var s in st.EnumerateArray()) if (Str(s, "text") is { } tx) f.Status.Add(tx);
        }
        var defH = Token("default_header", Color.FromArgb(255, 200, 200, 200));
        var defB = Token("default_body", Color.FromArgb(255, 240, 240, 240));
        if (n.TryGetProperty("tint", out var tint) && tint.ValueKind == JsonValueKind.Object)
        {
            f.Header = tint.TryGetProperty("header", out var th) ? Paint(th, defH) : defH;
            f.Body = tint.TryGetProperty("body", out var tb) ? Paint(tb, defB) : defB;
        }
        else { f.Header = defH; f.Body = defB; }
        f.Border = Token("separator", Color.FromArgb(255, 160, 160, 160));
        f.BorderWidth = 1;
        if (n.TryGetProperty("border", out var b) && b.ValueKind == JsonValueKind.Object)
        {
            if (b.TryGetProperty("paint", out var bp)) f.Border = Paint(bp, f.Border);
            if (b.TryGetProperty("width", out var bw) && bw.TryGetDouble(out var d)) f.BorderWidth = (float)d;
        }
        if (n.TryGetProperty("run", out var run) && run.ValueKind == JsonValueKind.Object) f.Run = Str(run, "state");
        if (n.TryGetProperty("inputs", out var ins) && ins.ValueKind == JsonValueKind.Array) foreach (var r in ins.EnumerateArray()) f.Inputs.Add(ReadRow(r));
        if (n.TryGetProperty("outputs", out var outs) && outs.ValueKind == JsonValueKind.Array) foreach (var r in outs.EnumerateArray()) f.Outputs.Add(ReadRow(r));
        if (n.TryGetProperty("footer", out var foot) && foot.ValueKind == JsonValueKind.Array)
            foreach (var it in foot.EnumerateArray())
                if (Str(it, "text") is { } tx) f.Footer.Add(tx);
                else if (it.TryGetProperty("lines", out var ls) && ls.ValueKind == JsonValueKind.Array)
                    foreach (var l in ls.EnumerateArray()) if (Str(l, "text") is { } lt) f.Footer.Add(lt);
        f.Selected = SelectedIds.Contains(f.Id);
        return f;
    }

    Wire ReadWire(JsonElement w)
    {
        var x = new Wire { Nid = U64(w, "nid"), Id = Str(w, "id") ?? "", Rev = U64(w, "rev"), FromFace = Str(w, "fromFace") ?? "", ToFace = Str(w, "toFace") ?? "" };
        x.Paint = w.TryGetProperty("paint", out var p) ? Paint(p, Token("wire", Color.FromArgb(255, 120, 120, 120))) : Token("wire", Color.FromArgb(255, 120, 120, 120));
        if (w.TryGetProperty("path", out var path) && path.ValueKind == JsonValueKind.Array)
        {
            var pts = new Vector2[path.GetArrayLength()];
            int i = 0;
            foreach (var pt in path.EnumerateArray()) pts[i++] = new Vector2((float)pt[0].GetDouble(), (float)pt[1].GetDouble());
            x.Path = pts;
        }
        return x;
    }

    /// <summary>Apply a scene.get reply (full or delta). Returns the faces and wires that changed.</summary>
    public (List<Face> faces, List<Wire> wires, List<ulong> goneFaces, List<ulong> goneWires) Apply(JsonElement s)
    {
        var faces = new List<Face>();
        var wires = new List<Wire>();
        var goneF = new List<ulong>();
        var goneW = new List<ulong>();
        if (s.TryGetProperty("gen", out var g) && g.TryGetUInt64(out var gen)) Gen = gen;
        bool full = s.TryGetProperty("full", out var fl) && fl.ValueKind == JsonValueKind.True;
        if (s.TryGetProperty("selection", out var sel) && sel.TryGetProperty("nodes", out var sn) && sn.ValueKind == JsonValueKind.Array)
            SelectedIds = sn.EnumerateArray().Select(e => e.GetString() ?? "").ToHashSet();
        if (s.TryGetProperty("view", out var view) && view.ValueKind == JsonValueKind.Object && view.TryGetProperty("fit", out var fit) && fit.ValueKind == JsonValueKind.Object)
        {
            var seq = fit.TryGetProperty("seq", out var sq) && sq.TryGetInt64(out var q) ? q : 0;
            if (seq != FitSeq && fit.TryGetProperty("rect", out var r) && r.GetArrayLength() == 4)
            {
                FitSeq = seq;
                FitRect = [(float)r[0].GetDouble(), (float)r[1].GetDouble(), (float)r[2].GetDouble(), (float)r[3].GetDouble()];
            }
        }
        JsonElement nodes, wl;
        if (full)
        {
            goneF.AddRange(Faces.Keys);
            goneW.AddRange(Wires.Keys);
            Faces.Clear(); Wires.Clear(); FaceByAgent.Clear();
            nodes = s.TryGetProperty("nodes", out var n) ? n : default;
            wl = s.TryGetProperty("wires", out var w) ? w : default;
        }
        else
        {
            var up = s.TryGetProperty("upsert", out var u) ? u : default;
            nodes = up.ValueKind == JsonValueKind.Object && up.TryGetProperty("nodes", out var n) ? n : default;
            wl = up.ValueKind == JsonValueKind.Object && up.TryGetProperty("wires", out var w) ? w : default;
            if (s.TryGetProperty("remove", out var rm) && rm.ValueKind == JsonValueKind.Object)
            {
                if (rm.TryGetProperty("nodes", out var rn) && rn.ValueKind == JsonValueKind.Array)
                    foreach (var id in rn.EnumerateArray())
                        if (id.GetString() is { } sid && FaceByAgent.Remove(sid, out var nid)) { Faces.Remove(nid); goneF.Add(nid); }
                if (rm.TryGetProperty("wires", out var rw) && rw.ValueKind == JsonValueKind.Array)
                    foreach (var id in rw.EnumerateArray())
                    {
                        var sid = id.GetString();
                        var hit = Wires.Values.FirstOrDefault(x => x.Id == sid);
                        if (hit != null) { Wires.Remove(hit.Nid); goneW.Add(hit.Nid); }
                    }
            }
        }
        if (nodes.ValueKind == JsonValueKind.Array)
            foreach (var n in nodes.EnumerateArray())
            {
                var f = ReadFace(n);
                Faces[f.Nid] = f;
                FaceByAgent[f.Id] = f.Nid;
                faces.Add(f);
            }
        if (wl.ValueKind == JsonValueKind.Array)
            foreach (var w in wl.EnumerateArray())
            {
                var x = ReadWire(w);
                Wires[x.Nid] = x;
                wires.Add(x);
            }
        // the selection changes a face's look without its rev changing
        foreach (var f in Faces.Values)
        {
            var now = SelectedIds.Contains(f.Id);
            if (now != f.Selected) { f.Selected = now; if (!faces.Contains(f)) faces.Add(f); }
        }
        return (faces, wires, goneF, goneW);
    }
}
