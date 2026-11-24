// PlumCraft for Windows: an array's view modes and its channels' histograms (B154 slice 1; contract 13.2.2).
// What the core gives in `popout.get` read into plain values, kept here so they can be tested without a window:
// the View picker's words (the egui editor's tips), each channel's line, and its histogram's bars as the core
// scaled them (the shell computes nothing).

using System.Text.Json;

namespace PlumCraft.Models;

public static class ArrayViews
{
    /// <summary>What each view mode does, as the egui editor's View bar says it (`array_view.rs` `view_tip`).</summary>
    public static string Tip(string mode) => mode switch
    {
        "auto" => "Chosen for the array: labels for integers, colour for three channels in a colour space, else clip or stretch",
        "color" => "One picture: grey, grey + alpha, RGB (from its colour space) or RGBA",
        "stretch" => "Each channel from its own min to max",
        "clip" => "Each channel's values 0 to 1 as they are",
        "heat" => "Each channel stretched, in the Turbo colour map",
        "labels" => "A colour per integer value (negatives black)",
        "angle" => "Each value as a hue: one turn round the colour wheel",
        _ => "",
    };

    /// <summary>The turn `angle` is shown with: the view's own `params.turn`, else 1 (one full turn).</summary>
    public static double Turn(JsonElement? viewParams) =>
        viewParams.Get("turn") is { ValueKind: JsonValueKind.Number } t && t.GetDouble() is var d && double.IsFinite(d) && d > 0 ? d : 1;

    /// <summary>A channel's histogram as the core sends it: `heights` 0 to 1 already on the scale the toggle sets.</summary>
    public sealed record Histogram(double[] Heights, string Lo, string Hi, long Bins, long Top, bool Log);

    /// <summary>One channel: its name, range, NaN count (0 when none) and histogram (null when the engine sent none).</summary>
    public sealed record Channel(string Name, string Min, string Max, long Nan, Histogram? Histogram)
    {
        /// <summary>The line above its histogram: "escape: 0 … 255", with ", 3 NaN" when there are any.</summary>
        public string Line => $"{Name}: {Min} … {Max}{(Nan > 0 ? $", {Nan:N0} NaN" : "")}";
    }

    /// <summary>The channels of `stats`; an item that isn't an object is skipped, a bar out of 0 to 1 is held to it.</summary>
    public static List<Channel> Channels(JsonElement? stats)
    {
        var o = new List<Channel>();
        if (stats is not { ValueKind: JsonValueKind.Array } arr) return o;
        foreach (var s in arr.EnumerateArray())
        {
            if (s.ValueKind != JsonValueKind.Object) continue;
            Histogram? h = null;
            if (s.Get("histogram") is { ValueKind: JsonValueKind.Object } hv)
            {
                var heights = hv.Get("heights") is { ValueKind: JsonValueKind.Array } ha
                    ? ha.EnumerateArray().Select(x => x.ValueKind == JsonValueKind.Number && x.GetDouble() is var d && double.IsFinite(d) ? Math.Clamp(d, 0, 1) : 0).ToArray()
                    : [];
                h = new Histogram(heights, Text(hv.Get("lo")), Text(hv.Get("hi")), Int(hv.Get("bins")), Int(hv.Get("top")), hv.Get("log").IsTrue());
            }
            o.Add(new Channel(s.Get("name").Str() ?? "", Text(s.Get("min")), Text(s.Get("max")), Int(s.Get("nan")), h));
        }
        return o;
    }

    static string Text(JsonElement? v) => v switch
    {
        { ValueKind: JsonValueKind.String } s => s.GetString() ?? "–",
        { ValueKind: JsonValueKind.Number } n => n.GetRawText(),
        _ => "–",
    };

    static long Int(JsonElement? v) => v is { ValueKind: JsonValueKind.Number } n && n.TryGetInt64(out var i) ? i : 0;
}
