// Decoding the contract's messages: one source-generated context (no reflection at run time), two
// sets of options over it.
//
// - Shell.Options: what a shell decodes with. A field this kit doesn't know is skipped (a newer
//   core may add fields: additive changes are capabilities, never versions).
// - Shell.Strict: the same, but a field the types don't know is an error. The tests decode the
//   shared fixtures, schema samples and live replies with it, so the C# types can't drift from the
//   contract unseen (the Swift kit's KeyRecorder does the same).
//
// Both respect nullable annotations (a null where the type has no null is an error) and `required`
// (a field missing that the type requires is an error), as Swift's Decodable does.

using System.Diagnostics.CodeAnalysis;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.Json.Serialization.Metadata;

namespace PlumCraft.Models;

[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    RespectNullableAnnotations = true,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    MaxDepth = 256)]
[JsonSerializable(typeof(Scene))]
[JsonSerializable(typeof(Delta))]
[JsonSerializable(typeof(SceneReply))]
[JsonSerializable(typeof(Face))]
[JsonSerializable(typeof(Wire))]
[JsonSerializable(typeof(Net))]
[JsonSerializable(typeof(Frame))]
[JsonSerializable(typeof(Group))]
[JsonSerializable(typeof(Mark))]
[JsonSerializable(typeof(Paint))]
[JsonSerializable(typeof(Point))]
[JsonSerializable(typeof(Rect))]
[JsonSerializable(typeof(Rgba))]
[JsonSerializable(typeof(ThemeReply))]
[JsonSerializable(typeof(MenuReply))]
[JsonSerializable(typeof(ContextMenuReply))]
[JsonSerializable(typeof(MenuItem))]
[JsonSerializable(typeof(Ask))]
[JsonSerializable(typeof(AskEnded))]
[JsonSerializable(typeof(Watched))]
[JsonSerializable(typeof(Inspect))]
[JsonSerializable(typeof(PaletteItems))]
[JsonSerializable(typeof(Hit))]
[JsonSerializable(typeof(Located))]
[JsonSerializable(typeof(GestureReply))]
[JsonSerializable(typeof(DragUpdate))]
[JsonSerializable(typeof(DragBegun))]
[JsonSerializable(typeof(GeometrySetReply))]
[JsonSerializable(typeof(Changed))]
[JsonSerializable(typeof(MediaNote))]
[JsonSerializable(typeof(LogMark))]
[JsonSerializable(typeof(FeedReply))]
[JsonSerializable(typeof(PopoutValue))]
[JsonSerializable(typeof(ParametersReply))]
public sealed partial class ShellJson : JsonSerializerContext
{
}

/// <summary>The kit's decoding: lenient for a shell, strict for tests.</summary>
public static class Shell
{
    static JsonSerializerOptions Make(bool strict) => new()
    {
        TypeInfoResolver = ShellJson.Default,
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        RespectNullableAnnotations = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        MaxDepth = 256,
        UnmappedMemberHandling = strict ? JsonUnmappedMemberHandling.Disallow : JsonUnmappedMemberHandling.Skip,
    };

    /// <summary>A field this kit doesn't know is skipped.</summary>
    public static readonly JsonSerializerOptions Options = Make(false);

    /// <summary>A field this kit doesn't know is an error (tests).</summary>
    public static readonly JsonSerializerOptions Strict = Make(true);

    public static JsonTypeInfo<T> Info<T>(JsonSerializerOptions? o = null) => (JsonTypeInfo<T>)(o ?? Options).GetTypeInfo(typeof(T));

    /// <summary>Decode `v` as T; throws JsonException (or NotSupportedException for a type the context lacks).</summary>
    public static T Decode<T>(JsonElement v, JsonSerializerOptions? o = null) =>
        v.Deserialize(Info<T>(o)) ?? throw new JsonException($"null is not a {typeof(T).Name}");

    /// <summary>Decode bytes as T.</summary>
    public static T Decode<T>(ReadOnlySpan<byte> utf8, JsonSerializerOptions? o = null) =>
        JsonSerializer.Deserialize(utf8, Info<T>(o)) ?? throw new JsonException($"null is not a {typeof(T).Name}");

    /// <summary>Decode, or say it didn't: never throws for what the core sent.</summary>
    public static bool TryDecode<T>(JsonElement v, JsonSerializerOptions? o, [NotNullWhen(true)] out T? value)
    {
        try
        {
            value = v.Deserialize(Info<T>(o));
            return value is not null;
        }
        catch (JsonException)
        {
            value = default;
            return false;
        }
    }

    /// <summary>T as JSON text (what the shell sends, and tests comparing two values).</summary>
    public static string Encode<T>(T value) => JsonSerializer.Serialize(value, Info<T>());

    /// <summary>Two values the same as JSON (records holding lists compare by reference in C#).</summary>
    public static bool Same<T>(T a, T b) => Encode(a) == Encode(b);
}
