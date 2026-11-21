// The small types the contract's messages share: words a newer core may add to, points and rects in
// graph units, colours, paints, ports. The Windows counterpart of the Swift kit's Basics.swift.
//
// Words: a field that carries a word (a run state, a widget kind, a domain) is kept as the string
// the core sent, so a word a newer core adds decodes and survives (the hostile-input rule: skip what
// you don't know). Each has a typed reading beside it (`StateWord`, `KindWord`, ...), null for a
// word this kit doesn't know.

using System.Text.Json;
using System.Text.Json.Serialization;

namespace PlumCraft.Models;

/// <summary>Reads a word the core sent as one of this kit's enums: null for a word it doesn't know.</summary>
public static class Words
{
    /// <summary>The known word, or null. Matching ignores case and dots ("file.open" is FileOpen).</summary>
    public static T? Of<T>(string? word) where T : struct, Enum =>
        word is not null && Table<T>.Map.TryGetValue(Norm(word), out var v) ? v : null;

    /// <summary>The word as the core spells it: the enum's name with a lower first letter, or the table's spelling.</summary>
    public static string Of<T>(T value) where T : struct, Enum => Table<T>.Spelled.TryGetValue(value, out var s) ? s : value.ToString();

    static string Norm(string s) => s.Replace(".", "", StringComparison.Ordinal).ToLowerInvariant();

    static class Table<T> where T : struct, Enum
    {
        public static readonly Dictionary<string, T> Map = Enum.GetValues<T>().ToDictionary(v => v.ToString().ToLowerInvariant());
        public static readonly Dictionary<T, string> Spelled = Enum.GetValues<T>().ToDictionary(v => v, v => Spell(v));

        static string Spell(T v)
        {
            var n = v.ToString();
            var attr = typeof(T).GetField(n)?.GetCustomAttributes(typeof(WordAttribute), false).OfType<WordAttribute>().FirstOrDefault();
            return attr?.Word ?? char.ToLowerInvariant(n[0]) + n[1..];
        }
    }
}

/// <summary>The spelling of an enum member on the wire, when it isn't the name with a lower first letter.</summary>
[AttributeUsage(AttributeTargets.Field)]
public sealed class WordAttribute(string word) : Attribute
{
    public string Word { get; } = word;
}

/// <summary>A point in graph units, [x, y] on the wire.</summary>
[JsonConverter(typeof(PointConverter))]
public readonly record struct Point(double X, double Y)
{
    public override string ToString() => $"[{X}, {Y}]";
}

/// <summary>A rect in graph units, [x0, y0, x1, y1] on the wire.</summary>
[JsonConverter(typeof(RectConverter))]
public readonly record struct Rect(double X0, double Y0, double X1, double Y1)
{
    public double Width => X1 - X0;
    public double Height => Y1 - Y0;
    public override string ToString() => $"[{X0}, {Y0}, {X1}, {Y1}]";
}

/// <summary>A colour, [r, g, b, a] bytes. Scene colours are premultiplied (8.3); theme tokens too.</summary>
[JsonConverter(typeof(RgbaConverter))]
public readonly record struct Rgba(byte R, byte G, byte B, byte A)
{
    public static readonly Rgba Clear = new(0, 0, 0, 0);
}

/// <summary>
/// A colour in the scene: a theme token by name (resolved with theme.get), or a colour of its own.
/// Exactly one (8, question 13); both is hostile and the token wins, as the core resolves it;
/// neither is drawn clear.
/// </summary>
public sealed record Paint
{
    public string? Token { get; init; }
    public Rgba? Color { get; init; }

    public static Paint OfToken(string t) => new() { Token = t };

    /// <summary>The colour, with a theme's tokens for a token; clear for a token the theme lacks.</summary>
    public Rgba Resolve(IReadOnlyDictionary<string, Rgba>? tokens)
    {
        if (Token is { } t) return tokens is not null && tokens.TryGetValue(t, out var c) ? c : Rgba.Clear;
        return Color ?? Rgba.Clear;
    }
}

/// <summary>A port of a graph node: {node, port}.</summary>
public sealed record PortRef
{
    public required string Node { get; init; }
    public required string Port { get; init; }
}

/// <summary>A net's key: its name, and the group it is local to (none: the top level).</summary>
public sealed record NetKey
{
    public required string Name { get; init; }
    public string? Group { get; init; }
}

/// <summary>A modifier held during a gesture or named by a shortcut. Primary is Ctrl on Windows.</summary>
public enum ModifierWord { Primary, Ctrl, Shift, Alt }

static class Num
{
    public static double Read(ref Utf8JsonReader r, string what)
    {
        if (r.TokenType != JsonTokenType.Number || !r.TryGetDouble(out var d) || !double.IsFinite(d))
            throw new JsonException($"{what}: a number");
        return d;
    }

    public static void Next(ref Utf8JsonReader r, string what)
    {
        if (!r.Read()) throw new JsonException($"{what}: cut short");
    }
}

public sealed class PointConverter : JsonConverter<Point>
{
    public override Point Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o)
    {
        if (r.TokenType != JsonTokenType.StartArray) throw new JsonException("a point is [x, y]");
        Num.Next(ref r, "a point");
        var x = Num.Read(ref r, "a point");
        Num.Next(ref r, "a point");
        var y = Num.Read(ref r, "a point");
        Num.Next(ref r, "a point");
        if (r.TokenType != JsonTokenType.EndArray) throw new JsonException("a point is [x, y]");
        return new Point(x, y);
    }

    public override void Write(Utf8JsonWriter w, Point p, JsonSerializerOptions o)
    {
        w.WriteStartArray();
        w.WriteNumberValue(p.X);
        w.WriteNumberValue(p.Y);
        w.WriteEndArray();
    }
}

public sealed class RectConverter : JsonConverter<Rect>
{
    public override Rect Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o)
    {
        if (r.TokenType != JsonTokenType.StartArray) throw new JsonException("a rect is [x0, y0, x1, y1]");
        Span<double> v = stackalloc double[4];
        for (int i = 0; i < 4; i++)
        {
            Num.Next(ref r, "a rect");
            v[i] = Num.Read(ref r, "a rect");
        }
        Num.Next(ref r, "a rect");
        if (r.TokenType != JsonTokenType.EndArray) throw new JsonException("a rect is [x0, y0, x1, y1]");
        return new Rect(v[0], v[1], v[2], v[3]);
    }

    public override void Write(Utf8JsonWriter w, Rect p, JsonSerializerOptions o)
    {
        w.WriteStartArray();
        w.WriteNumberValue(p.X0);
        w.WriteNumberValue(p.Y0);
        w.WriteNumberValue(p.X1);
        w.WriteNumberValue(p.Y1);
        w.WriteEndArray();
    }
}

public sealed class RgbaConverter : JsonConverter<Rgba>
{
    public override Rgba Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o)
    {
        if (r.TokenType != JsonTokenType.StartArray) throw new JsonException("a colour is [r, g, b, a]");
        Span<byte> v = stackalloc byte[4];
        for (int i = 0; i < 4; i++)
        {
            Num.Next(ref r, "a colour");
            if (r.TokenType != JsonTokenType.Number || !r.TryGetByte(out v[i])) throw new JsonException("a colour's parts are bytes");
        }
        Num.Next(ref r, "a colour");
        if (r.TokenType != JsonTokenType.EndArray) throw new JsonException("a colour is [r, g, b, a]");
        return new Rgba(v[0], v[1], v[2], v[3]);
    }

    public override void Write(Utf8JsonWriter w, Rgba c, JsonSerializerOptions o)
    {
        w.WriteStartArray();
        w.WriteNumberValue(c.R);
        w.WriteNumberValue(c.G);
        w.WriteNumberValue(c.B);
        w.WriteNumberValue(c.A);
        w.WriteEndArray();
    }
}
