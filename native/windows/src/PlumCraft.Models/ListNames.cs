// PlumCraft for Windows: a list value as named pictures (contract 8.7; interface 10.2). The words the
// face's tag and the pop-out's stepper show, as the Apple shells write them (`LayerCanvas.listTagText`,
// `ViewerBar.itemNames` and `itemTitle`), kept here so both can be tested without a window.

using System.Text.Json;

namespace PlumCraft.Models;

public static class ListNames
{
    /// <summary>The face picture's tag for a list: "caption · n pictures", or "n items" with no caption;
    /// null for a single picture.</summary>
    public static string? Tag(Preview? p)
    {
        if (p?.Items is not { } n || n <= 1) return null;
        var name = (p.Caption ?? "").Trim();
        // a list of lists: each level's count, outermost first ("3 × 2 items"; contract 8.7)
        var count = p.Levels is { Count: > 1 } l ? string.Join(" × ", l) : n.ToString();
        return name.Length == 0 ? $"{count} items" : $"{name} · {count} pictures";
    }

    /// <summary>The outer stepper's word: "run" for a list of lists, else "item".</summary>
    public static string OuterWord(IReadOnlyList<ulong>? levels) => levels is { Count: > 1 } ? "run" : "item";

    /// <summary>One inner level's stepper in a list of lists (decision 6A): what it steps ("item" for the innermost,
    /// "run" between), where it is (`At` of `Of`), and the path that shows item j.</summary>
    public sealed record LevelStep(string Word, long At, ulong Of, int Level, IReadOnlyList<long> Shown)
    {
        /// <summary>The levels above kept, those below back at their first.</summary>
        public List<long> PathTo(long j) => [.. Shown.Take(Level), j];
    }

    /// <summary>The steppers inside the run shown, outermost first; none for a list that isn't nested.</summary>
    public static List<LevelStep> Steps(IReadOnlyList<long>? path, IReadOnlyList<ulong>? levels)
    {
        var l = levels ?? [];
        var p = path ?? [];
        var deep = Math.Max(l.Count - 1, 0);
        return Enumerable.Range(0, deep).Select(k => new LevelStep(k + 1 == deep ? "item" : "run", k < p.Count ? p[k] : 0, l[k + 1], k, p)).ToList();
    }

    /// <summary>Each item's name from the pop-out's sheet (its cells' `label`), "item k" for one without;
    /// empty when no item has a name, so the stepper says "item 5 of 11" as before.</summary>
    public static List<string> Names(JsonElement? sheet)
    {
        if (sheet is not { ValueKind: JsonValueKind.Array } cells) return [];
        var names = cells.EnumerateArray().Select(c => c.ValueKind == JsonValueKind.Object ? (c.Get("label").Str() ?? "") : "").ToList();
        if (!names.Any(x => x.Trim().Length > 0)) return [];
        return names.Select((x, k) => x.Trim().Length == 0 ? $"item {k + 1}" : x).ToList();
    }

    /// <summary>The stepper's words for item `i` of `n`: its name and where it is ("logit · 5 of 11"), or
    /// "item 5 of 11" when the items have no names.</summary>
    public static string Title(IReadOnlyList<string> names, long i, ulong n, string word = "item")
    {
        if (names.Count == 0) return $"{word} {i + 1} of {n}";
        var name = i >= 0 && i < names.Count ? names[(int)i] : $"item {i + 1}";
        return $"{name} · {i + 1} of {n}";
    }
}
