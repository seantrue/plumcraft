// JSON values as System.Text.Json's JsonElement (immutable, a null kept apart from a missing field),
// and what the kit and the conformance runner do with them: build one, parse one, follow a JSON
// pointer, compare two as the suite does (numbers within 1e-3), and print one shortened.

using System.Buffers;
using System.Text;
using System.Text.Json;

namespace PlumCraft.Models;

public static class Jsonv
{
    /// <summary>The deepest JSON the kit parses (the core's own limit is lower).</summary>
    public const int MaxDepth = 256;

    /// <summary>A value written by `write`, as an element that outlives the writer.</summary>
    public static JsonElement Build(Action<Utf8JsonWriter> write)
    {
        var buf = new ArrayBufferWriter<byte>(256);
        using (var w = new Utf8JsonWriter(buf, new JsonWriterOptions { SkipValidation = false, MaxDepth = MaxDepth })) write(w);
        return Parse(buf.WrittenSpan);
    }

    public static JsonElement Parse(ReadOnlySpan<byte> utf8)
    {
        var reader = new Utf8JsonReader(utf8, new JsonReaderOptions { MaxDepth = MaxDepth });
        using var d = JsonDocument.ParseValue(ref reader);
        if (reader.Read()) throw new JsonException("more after the value");
        return d.RootElement.Clone();
    }

    public static JsonElement Parse(string text) => Parse(Encoding.UTF8.GetBytes(text));

    public static readonly JsonElement Null = Parse("null"u8);
    public static readonly JsonElement True = Parse("true"u8);
    public static readonly JsonElement False = Parse("false"u8);
    public static readonly JsonElement EmptyObject = Parse("{}"u8);

    public static JsonElement Of(string s) => Build(w => w.WriteStringValue(s));
    public static JsonElement Of(bool b) => b ? True : False;
    public static JsonElement Of(double d) => Build(w => w.WriteNumberValue(d));
    public static JsonElement Of(long n) => Build(w => w.WriteNumberValue(n));
    public static JsonElement Of(ulong n) => Build(w => w.WriteNumberValue(n));

    /// <summary>An object of these fields, in order (an Undefined value is left out).</summary>
    public static JsonElement Object(params (string key, JsonElement value)[] fields) => Build(w =>
    {
        w.WriteStartObject();
        foreach (var (k, v) in fields)
        {
            if (v.ValueKind == JsonValueKind.Undefined) continue;
            w.WritePropertyName(k);
            v.WriteTo(w);
        }
        w.WriteEndObject();
    });

    public static JsonElement Array(IEnumerable<JsonElement> items) => Build(w =>
    {
        w.WriteStartArray();
        foreach (var v in items) v.WriteTo(w);
        w.WriteEndArray();
    });

    /// <summary>The value at a JSON pointer (RFC 6901): "" is the value itself; null when nothing is
    /// there (a JSON null that is there is a Null element).</summary>
    public static JsonElement? Pointer(this JsonElement v, string p)
    {
        if (p.Length == 0) return v;
        if (p[0] != '/') return null;
        var at = v;
        foreach (var raw in p[1..].Split('/'))
        {
            var token = raw.Replace("~1", "/", StringComparison.Ordinal).Replace("~0", "~", StringComparison.Ordinal);
            switch (at.ValueKind)
            {
                case JsonValueKind.Object:
                    if (!at.TryGetProperty(token, out var next)) return null;
                    at = next;
                    break;
                case JsonValueKind.Array:
                    if (token.Length == 0 || !token.All(char.IsAsciiDigit) || !int.TryParse(token, out var i) || i >= at.GetArrayLength()) return null;
                    at = at[i];
                    break;
                default:
                    return null;
            }
        }
        return at;
    }

    /// <summary>A field of an object, or null when `v` isn't one or lacks it.</summary>
    public static JsonElement? Get(this JsonElement v, string key) =>
        v.ValueKind == JsonValueKind.Object && v.TryGetProperty(key, out var x) ? x : null;

    /// <summary>A field of what may be nothing (N5 slice 4: so reads of the core's models chain).</summary>
    public static JsonElement? Get(this JsonElement? v, string key) => v is { } x ? x.Get(key) : null;

    /// <summary>An item of a list, or null.</summary>
    public static JsonElement? At(this JsonElement? v, int i) =>
        v is { ValueKind: JsonValueKind.Array } a && i >= 0 && i < a.GetArrayLength() ? a[i] : null;

    /// <summary>The items of a list (none when it isn't one).</summary>
    public static IEnumerable<JsonElement> Items(this JsonElement? v) =>
        v is { ValueKind: JsonValueKind.Array } a ? a.EnumerateArray() : [];

    public static IEnumerable<JsonElement> Items(this JsonElement v) => ((JsonElement?)v).Items();
    public static JsonElement? At(this JsonElement v, int i) => ((JsonElement?)v).At(i);
    public static string? Str(this JsonElement v) => ((JsonElement?)v).Str();
    public static bool? Bool(this JsonElement v) => ((JsonElement?)v).Bool();
    public static double? Double(this JsonElement v) => ((JsonElement?)v).Double();
    public static ulong? U64(this JsonElement v) => ((JsonElement?)v).U64();

    public static string? Str(this JsonElement? v) => v is { ValueKind: JsonValueKind.String } s ? s.GetString() : null;
    public static bool? Bool(this JsonElement? v) => v?.ValueKind switch { JsonValueKind.True => true, JsonValueKind.False => false, _ => null };
    public static double? Double(this JsonElement? v) => v is { ValueKind: JsonValueKind.Number } n && n.TryGetDouble(out var d) ? d : null;
    public static ulong? U64(this JsonElement? v) => v is { ValueKind: JsonValueKind.Number } n && n.TryGetUInt64(out var d) ? d : null;
    public static bool IsTrue(this JsonElement? v) => v?.ValueKind == JsonValueKind.True;

    /// <summary>The suite's equality (conformance.rs's `same`): numbers within 1e-3, arrays item for
    /// item, objects key for key, anything else exactly.</summary>
    public static bool Same(JsonElement a, JsonElement b)
    {
        if (a.ValueKind == JsonValueKind.Number && b.ValueKind == JsonValueKind.Number)
        {
            if (a.TryGetDouble(out var x) && b.TryGetDouble(out var y)) return Math.Abs(x - y) <= 1.0e-3;
            return a.GetRawText() == b.GetRawText();
        }
        if (a.ValueKind != b.ValueKind) return false;
        switch (a.ValueKind)
        {
            case JsonValueKind.Array:
                if (a.GetArrayLength() != b.GetArrayLength()) return false;
                using (var ea = a.EnumerateArray().GetEnumerator())
                using (var eb = b.EnumerateArray().GetEnumerator())
                    while (ea.MoveNext() && eb.MoveNext())
                        if (!Same(ea.Current, eb.Current)) return false;
                return true;
            case JsonValueKind.Object:
                int na = 0, nb = 0;
                foreach (var _ in a.EnumerateObject()) na++;
                foreach (var _ in b.EnumerateObject()) nb++;
                if (na != nb) return false;
                foreach (var p in a.EnumerateObject())
                    if (!b.TryGetProperty(p.Name, out var q) || !Same(p.Value, q)) return false;
                return true;
            case JsonValueKind.String:
                return a.GetString() == b.GetString();
            default:
                return true; // true, false, null: the kinds matched
        }
    }

    /// <summary>The value as JSON text, compact.</summary>
    public static string Text(this JsonElement v) => v.ValueKind == JsonValueKind.Undefined ? "undefined" : v.GetRawText();

    /// <summary>At most 600 characters of the value's text.</summary>
    public static string Short(this JsonElement v)
    {
        var t = v.Text();
        return t.Length > 600 ? t[..600] + "…" : t;
    }
}
