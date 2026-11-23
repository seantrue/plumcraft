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
        return name.Length == 0 ? $"{n} items" : $"{name} · {n} pictures";
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
    public static string Title(IReadOnlyList<string> names, long i, ulong n)
    {
        if (names.Count == 0) return $"item {i + 1} of {n}";
        var name = i >= 0 && i < names.Count ? names[(int)i] : $"item {i + 1}";
        return $"{name} · {i + 1} of {n}";
    }
}
