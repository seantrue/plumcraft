// The C# types against the contract's shared data (the Swift kit's ModelsTests): the scene fixtures
// decode with no field left unknown, every property shell1.json names is known to the type it maps
// to, unknown words decode and read as unknown, and hostile input throws a JsonException and never
// anything else.

using System.Text.Json;
using System.Text.Json.Nodes;
using PlumCraft.Models;
using Xunit;
using static PlumCraft.Kit.Tests.Support;

namespace PlumCraft.Kit.Tests;

public class Fixtures
{
    [Fact]
    public void TheFullSceneDecodesWithNoFieldUnknown()
    {
        var v = ReadFixture("crates/editor-core/fixtures/scene/full.json");
        var scene = Shell.Decode<Scene>(v, Shell.Strict);
        Assert.True(scene.Full);
        Assert.Equal(v.Get("nodes")!.Value.GetArrayLength(), scene.Nodes.Count);
        Assert.Equal(v.Get("wires")!.Value.GetArrayLength(), scene.Wires.Count);
        // every word the fixture uses is one the kit knows (it covers every row source, widget kind,
        // footer kind and run state)
        foreach (var f in scene.Nodes)
        {
            if (f.Run.State is { } s) Assert.True(f.Run.StateWord is not null, $"run state {s}");
            Assert.NotNull(f.ViewWord);
            Assert.NotNull(f.Header.State.KindWord);
            foreach (var r in f.Inputs)
            {
                Assert.True(r.SourceWord is not null, $"row source {r.Source}");
                Assert.NotNull(r.Pin.ShapeWord);
                if (r.Widget is { } w) Assert.True(w.KindWord is not null, $"widget {w.Kind}");
            }
            foreach (var item in f.Footer) Assert.True(item.KindWord is not null, $"footer kind {item.Kind}");
            if (f.Preview is { } p) Assert.True(p.KindWord is not null, $"preview kind {p.Kind}");
        }
        foreach (var w in scene.Wires) Assert.NotNull(w.KindWord);
        var footers = scene.Nodes.SelectMany(f => f.Footer).Select(x => x.Kind).ToHashSet();
        Assert.True(footers.Count >= 9, string.Join(", ", footers));
        var sources = scene.Nodes.SelectMany(f => f.Inputs).Select(r => r.SourceWord).OfType<RowSourceWord>().ToHashSet();
        Assert.True(sources.SetEquals(Enum.GetValues<RowSourceWord>()), string.Join(", ", sources));
        var states = scene.Nodes.Select(f => f.Run.StateWord).OfType<RunWord>().ToHashSet();
        Assert.True(states.SetEquals(Enum.GetValues<RunWord>()), string.Join(", ", states));
    }

    [Fact]
    public void TheDeltaDecodesWithNoFieldUnknown()
    {
        var v = ReadFixture("crates/editor-core/fixtures/scene/delta.json");
        var delta = Shell.Decode<Delta>(v, Shell.Strict);
        Assert.False(delta.Full);
        Assert.Single(delta.Upsert.Nodes);
        Assert.True(delta.Since < delta.Gen);
        // and through the reply type, which tells them apart by `full`
        var r = Shell.Decode<SceneReply>(v, Shell.Strict);
        Assert.NotNull(r.Delta);
        Assert.Null(r.Scene);
        Assert.True(Shell.Same(r.Delta, delta));
        var f = Shell.Decode<SceneReply>(ReadFixture("crates/editor-core/fixtures/scene/full.json"), Shell.Strict);
        Assert.NotNull(f.Scene);
        Assert.True(f.IsFull);
    }

    [Fact]
    public void TheCasesFileParses()
    {
        var cases = PlumCraft.Conformance.Conformance.Cases(CasesPath);
        Assert.True(cases.Count >= 20);
        Assert.True(cases.Count(c => PlumCraft.Conformance.Conformance.HalfOf(c) == "read") >= 10);
    }

    /// <summary>The strict check isn't vacuous: a field a type doesn't have is an error, deep inside a list
    /// too, and the lenient options skip it.</summary>
    [Fact]
    public void TheStrictOptionsFindFieldsATypeLacks()
    {
        var node = JsonNode.Parse(ReadFixture("crates/editor-core/fixtures/scene/full.json").GetRawText())!;
        node["nodes"]![0]!["header"]!["sparkle"] = true;
        var v = Jsonv.Parse(node.ToJsonString());
        var e = Assert.Throws<JsonException>(() => Shell.Decode<Scene>(v, Shell.Strict));
        Assert.Contains("sparkle", e.Message);
        Assert.Equal(Shell.Decode<Scene>(v).Nodes.Count, node["nodes"]!.AsArray().Count);
        var mark = Jsonv.Parse("""{"id": "a.b", "nid": 1, "rev": 1, "label": "m", "tone": "warn", "paint": {"token": "warning"}, "tooltip": null, "at": null, "glow": 3}""");
        Assert.Throws<JsonException>(() => Shell.Decode<Mark>(mark, Shell.Strict));
        Assert.Equal("m", Shell.Decode<Mark>(mark).Label);
    }
}

/// <summary>A sample of every property a schema type has (each variant of a oneOf by `variant`), so a C#
/// type is checked against fields no fixture happens to fill (the Swift kit's Sampler).</summary>
sealed class Sampler(IReadOnlyDictionary<string, JsonElement> defs, int variant)
{
    public JsonElement Sample(JsonElement s) => Jsonv.Build(w => Write(w, s, 0));

    void Write(Utf8JsonWriter w, JsonElement s, int depth)
    {
        if (depth > 60 || s.ValueKind != JsonValueKind.Object)
        {
            w.WriteNullValue();
            return;
        }
        if (s.Get("$ref").Str() is { } r)
        {
            var name = r.Split('/')[^1];
            if (defs.TryGetValue(name, out var def)) Write(w, def, depth + 1); else w.WriteNullValue();
            return;
        }
        if (s.Get("const") is { } c)
        {
            c.WriteTo(w);
            return;
        }
        if (s.Get("enum") is { ValueKind: JsonValueKind.Array } e && e.GetArrayLength() > 0)
        {
            e[variant % e.GetArrayLength()].WriteTo(w);
            return;
        }
        foreach (var key in new[] { "oneOf", "anyOf" })
        {
            if (s.Get(key) is { ValueKind: JsonValueKind.Array } alts)
            {
                var real = alts.EnumerateArray().Where(a => a.Get("type").Str() != "null").ToList();
                if (real.Count == 0) w.WriteNullValue(); else Write(w, real[variant % real.Count], depth + 1);
                return;
            }
        }
        if (s.Get("allOf") is { ValueKind: JsonValueKind.Array } all && all.GetArrayLength() > 0)
        {
            Write(w, all[0], depth + 1);
            return;
        }
        string? word = s.Get("type") switch
        {
            { ValueKind: JsonValueKind.String } t => t.GetString(),
            { ValueKind: JsonValueKind.Array } ts => ts.EnumerateArray().Select(x => x.GetString()).FirstOrDefault(x => x != "null"),
            _ => s.Get("properties") is not null ? "object" : null,
        };
        switch (word)
        {
            case "object":
                w.WriteStartObject();
                int n = 0;
                if (s.Get("properties") is { ValueKind: JsonValueKind.Object } props)
                    foreach (var p in props.EnumerateObject())
                    {
                        w.WritePropertyName(p.Name);
                        Write(w, p.Value, depth + 1);
                        n++;
                    }
                if (n == 0 && s.Get("additionalProperties") is { ValueKind: JsonValueKind.Object } extra)
                {
                    w.WritePropertyName("x");
                    Write(w, extra, depth + 1);
                }
                w.WriteEndObject();
                break;
            case "array":
                w.WriteStartArray();
                var items = s.Get("items");
                if (items is { ValueKind: JsonValueKind.Array } tuple)
                    foreach (var t in tuple.EnumerateArray()) Write(w, t, depth + 1);
                else
                {
                    var min = s.Get("minItems").U64() is { } m ? (int)m : 1;
                    for (int i = 0; i < Math.Max(min, 1); i++) Write(w, items ?? Jsonv.Null, depth + 1);
                }
                w.WriteEndArray();
                break;
            case "string": w.WriteStringValue("x"); break;
            case "integer": w.WriteNumberValue(1); break;
            case "number": w.WriteNumberValue(1.5); break;
            case "boolean": w.WriteBooleanValue(false); break;
            // null, or any JSON (a `Value` in Rust): nothing to sample
            default: w.WriteNullValue(); break;
        }
    }
}

public class Schema
{
    readonly Dictionary<string, JsonElement> defs;

    public Schema()
    {
        defs = [];
        if (ReadFixture("crates/editor-core/schema/shell1.json").Get("$defs") is { ValueKind: JsonValueKind.Object } d)
            foreach (var p in d.EnumerateObject()) defs[p.Name] = p.Value;
    }

    void Check<T>(string name)
    {
        Assert.True(defs.TryGetValue(name, out var def), $"shell1.json has no {name}");
        for (int variant = 0; variant < 12; variant++)
        {
            var v = new Sampler(defs, variant).Sample(def);
            try
            {
                Shell.Decode<T>(v, Shell.Strict);
            }
            catch (JsonException e)
            {
                Assert.Fail($"{name} (variant {variant}) as {typeof(T).Name}: {e.Message}\n{v.Short()}");
            }
        }
    }

    [Fact]
    public void EveryMessageTheShellReadsIsKnown()
    {
        Check<Scene>("Scene");
        Check<Delta>("Delta");
        Check<Hit>("Hit");
        Check<Located>("LocateReply");
        Check<GestureReply>("GestureReply");
        Check<ThemeReply>("ThemeReply");
        Check<GeometrySetReply>("GeometrySetReply");
        Check<DragUpdate>("DragUpdate");
    }

    HashSet<string> Props(string name) =>
        defs.TryGetValue(name, out var d) && d.Get("properties") is { ValueKind: JsonValueKind.Object } p ? [.. p.EnumerateObject().Select(x => x.Name)] : [];

    /// <summary>What the shell sends uses only fields the schema has.</summary>
    [Fact]
    public void WhatTheShellSendsIsInTheSchema()
    {
        var kinds = new HashSet<string>();
        if (defs.TryGetValue("GestureKind", out var gk) && gk.Get("oneOf") is { ValueKind: JsonValueKind.Array } one)
            foreach (var alt in one.EnumerateArray())
            {
                if (alt.Get("enum") is { ValueKind: JsonValueKind.Array } en) foreach (var x in en.EnumerateArray()) kinds.Add(x.GetString()!);
                else if (alt.Get("const").Str() is { } c) kinds.Add(c);
            }
        // not `zoom` (contract 8.13, N4 slice 2): shell1.json's Gesture lacks it, a gap in the schema the
        // Windows kit found (the slice 1 report); the core takes it
        Gesture[] gestures = [Gesture.Tap(new Point(1, 2), [ModifierWord.Shift], 4), Gesture.MarqueeOf(new Point(0, 0), new Point(5, 5)), Gesture.DragBegin(new Point(1, 1)), Gesture.DragCancel];
        foreach (var g in gestures)
        {
            var keys = g.ToJson().EnumerateObject().Select(p => p.Name).ToHashSet();
            Assert.True(keys.IsSubsetOf(Props("Gesture")), string.Join(", ", keys.Except(Props("Gesture"))));
            Assert.True(kinds.Contains(Words.Of(g.Kind)), $"{g.Kind} not in {string.Join(", ", kinds)}");
        }
        var r = new FaceReport("n1", new Point(10, 20), Rev: 3, Header: 28, Inputs: [4, null], Outputs: [5], Rows: [new Rect(0, 0, 1, 1), null],
            Parts: new Dictionary<string, Rect> { ["title"] = new(1, 1, 2, 2) }, Footer: [new Rect(0, 9, 9, 10)], Offset: new Point(-7, -7));
        var j = r.ToJson();
        var rk = j.EnumerateObject().Select(p => p.Name).ToHashSet();
        Assert.True(rk.IsSubsetOf(Props("FaceReport")), string.Join(", ", rk.Except(Props("FaceReport"))));
        var parts = j.Get("parts")!.Value.EnumerateObject().Select(p => p.Name).ToHashSet();
        Assert.True(parts.IsSubsetOf(Props("PartsReport")), string.Join(", ", parts.Except(Props("PartsReport"))));
        Assert.Equal("doubleTap", Words.Of(GestureWord.DoubleTap));
    }
}

public class Hostile
{
    static JsonNode Face() => JsonNode.Parse(ReadFixture("crates/editor-core/fixtures/scene/full.json").Get("nodes")!.Value[3].GetRawText())!;

    [Fact]
    public void UnknownWordsDecodeAndReadAsUnknown()
    {
        var f = Face();
        f["run"]!["state"] = "teleporting";
        f["view"] = "holographic";
        f["shown"] = "dancing";
        f["footer"] = new JsonArray(new JsonObject { ["kind"] = "hologram", ["text"] = "?" });
        f["preview"] = new JsonObject { ["kind"] = "volume", ["voxels"] = 9 };
        f["futureField"] = new JsonObject { ["anything"] = new JsonArray(1, 2) };
        var d = Shell.Decode<Face>(Jsonv.Parse(f.ToJsonString()));
        Assert.Equal("teleporting", d.Run.State);
        Assert.Null(d.Run.StateWord);
        Assert.Equal("holographic", d.View);
        Assert.Null(d.ViewWord);
        Assert.Null(d.ShownWord);
        Assert.Equal("hologram", Assert.Single(d.Footer).Kind);
        Assert.Null(d.Footer[0].KindWord);
        Assert.Equal("volume", d.Preview?.Kind);
        Assert.Null(d.Preview?.KindWord);
        var w = Shell.Decode<Changed>(Jsonv.Parse("""{"doc": 1, "domain": "weather", "gen": 3}"""));
        Assert.Null(w.DomainWord);
        Assert.Equal("weather", w.Domain);
        var a = Shell.Decode<Ask>(Jsonv.Parse("""{"ask": 4, "kind": "telepathy", "modal": true, "params": {}}"""));
        Assert.Null(a.KindWord);
        Assert.Equal(AskWord.FileOpen, Shell.Decode<Ask>(Jsonv.Parse("""{"ask": 4, "kind": "file.open"}""")).KindWord);
        var p = Shell.Decode<Paint>(Jsonv.Parse("""{"token": "accent", "color": [1, 2, 3, 4]}"""));
        Assert.Equal(new Rgba(9, 9, 9, 9), p.Resolve(new Dictionary<string, Rgba> { ["accent"] = new(9, 9, 9, 9) }));
        Assert.Equal(Rgba.Clear, Shell.Decode<Paint>(Jsonv.Parse("{}")).Resolve(null));
    }

    /// <summary>A field a type defaults keeps its default when missing (the source generator sets a missing
    /// init-only property to null: an initializer alone was lost, the slice's first test run found).</summary>
    [Fact]
    public void AMissingFieldKeepsItsDefault()
    {
        var m = Shell.Decode<MenuItem>(Jsonv.Parse("{}"));
        Assert.Empty(m.Items);
        Assert.Equal("plain", m.Look);
        Assert.Equal(LookWord.Plain, m.LookWord);
        Assert.Equal("", m.Id);
        Assert.Empty(Shell.Decode<Shortcut>(Jsonv.Parse("""{"key": "A"}""")).Mods);
        var node = JsonNode.Parse(ReadFixture("crates/editor-core/fixtures/scene/full.json").GetRawText())!.AsObject();
        node.Remove("view");
        var s = Shell.Decode<Scene>(Jsonv.Parse(node.ToJsonString()), Shell.Strict);
        Assert.NotNull(s.View);
        Assert.False(s.View.SelectMode);
    }

    static JsonNode? Set(JsonNode root, IReadOnlyList<string> path, JsonNode? x)
    {
        var copy = root.DeepClone();
        if (path.Count == 0) return x;
        JsonNode? at = copy;
        for (int i = 0; i < path.Count - 1; i++)
            at = at is JsonObject o ? o[path[i]] : at is JsonArray a ? a[int.Parse(path[i])] : null;
        if (at is JsonObject po) po[path[^1]] = x;
        else if (at is JsonArray pa) pa[int.Parse(path[^1])] = x;
        return copy;
    }

    static void Leaves(JsonNode? v, List<string> p, List<List<string>> outp)
    {
        switch (v)
        {
            case JsonObject o:
                foreach (var kv in o) Leaves(kv.Value, [.. p, kv.Key], outp);
                break;
            case JsonArray a:
                for (int i = 0; i < a.Count; i++) Leaves(a[i], [.. p, i.ToString()], outp);
                break;
            default:
                outp.Add(p);
                break;
        }
        if (p.Count > 0 && v is JsonObject or JsonArray) outp.Add(p);
    }

    /// <summary>Every leaf of a face replaced, one at a time, by each of a few wrong values: decoding throws a
    /// JsonException or succeeds, and never anything else.</summary>
    [Fact]
    public void WrongValuesThrowJsonExceptionsAndNothingElse()
    {
        var f = Face();
        var paths = new List<List<string>>();
        Leaves(f, [], paths);
        string[] wrong = ["null", "\"x\"", "-1", "1e300", "18446744073709551615", "[]", "{}", "[1, 2, 3, 4, 5]", "true"];
        int threw = 0, took = 0;
        foreach (var p in paths)
        {
            foreach (var w in wrong)
            {
                var doc = Set(f, p, JsonNode.Parse(w));
                var v = Jsonv.Parse(doc?.ToJsonString() ?? "null");
                try
                {
                    Shell.Decode<Face>(v);
                    took++;
                }
                catch (JsonException)
                {
                    threw++;
                }
            }
        }
        Assert.True(threw > 0 && took > 0, $"{threw} threw, {took} decoded");
        // the poll itself: a broken item costs only itself
        var poll = Jsonv.Parse("""
            {"replies": [{"id": 1, "ok": true, "result": 5}], "events": [],
             "asks": [{"ask": 1, "kind": "prompt", "modal": true, "params": {}}, {"nonsense": true}, 7],
             "changed": [{"doc": 1, "domain": "scene", "gen": 2}, "oops"], "dropped": "many"}
            """);
        var b = FeedBatch.Parse(poll);
        Assert.Single(b.Asks);
        Assert.Single(b.Changed);
        Assert.Equal(0ul, b.Dropped);
        Assert.True(FeedBatch.Parse(Jsonv.Parse("[1, 2]")).IsQuiet);
    }

    [Fact]
    public void DeepNestingIsRefusedNotAStackOverflow()
    {
        var deep = new string('[', 100_000) + new string(']', 100_000);
        Assert.ThrowsAny<JsonException>(() => Jsonv.Parse(deep));
        var ok = string.Concat(Enumerable.Repeat("[{\"a\":", 64)) + "1" + string.Concat(Enumerable.Repeat("}]", 64));
        Assert.True(Jsonv.Same(Jsonv.Parse(ok), Jsonv.Parse(ok)));
    }
}

public class Values
{
    [Fact]
    public void PointersFindWhatIsThereAndNothingElse()
    {
        var v = Jsonv.Parse("""{"a": [{"b/c": 1, "d~e": 2}], "n": null}""");
        Assert.Equal(1, v.Pointer("/a/0/b~1c")!.Value.GetInt32());
        Assert.Equal(2, v.Pointer("/a/0/d~0e")!.Value.GetInt32());
        Assert.Null(v.Pointer("/a/1"));
        Assert.Equal(JsonValueKind.Null, v.Pointer("/n")!.Value.ValueKind);
        Assert.Null(v.Pointer("/nope"));
        Assert.True(Jsonv.Same(v.Pointer("")!.Value, v));
        Assert.Null(v.Pointer("a"));
        Assert.Null(v.Pointer("/a/-1"));
        Assert.Null(v.Pointer("/a/+0"));
    }

    [Fact]
    public void SameIsTheSuitesEquality()
    {
        Assert.True(Jsonv.Same(Jsonv.Parse("5"), Jsonv.Parse("5.0004")));
        Assert.False(Jsonv.Same(Jsonv.Parse("5"), Jsonv.Parse("5.01")));
        Assert.True(Jsonv.Same(Jsonv.Parse("""{"a": [1, "x", null, true]}"""), Jsonv.Parse("""{"a": [1.0, "x", null, true]}""")));
        Assert.False(Jsonv.Same(Jsonv.Parse("""{"a": 1}"""), Jsonv.Parse("""{"a": 1, "b": 2}""")));
        Assert.False(Jsonv.Same(Jsonv.Parse("null"), Jsonv.Parse("false")));
        Assert.True(Jsonv.Same(Jsonv.Parse("18446744073709551615"), Jsonv.Parse("18446744073709551615")));
    }

    [Fact]
    public void ARunnerMatcherMeansWhatItSays()
    {
        void Ok(string got, string want) => PlumCraft.Conformance.Conformance.Matches(got == "" ? null : Jsonv.Parse(got), Jsonv.Parse(want));
        void Bad(string got, string want) => Assert.Throws<PlumCraft.Conformance.ConformanceException>(() => Ok(got, want));
        Ok("[1, 2]", """{"$len": 2}""");
        Bad("[1, 2]", """{"$len": 3}""");
        Ok("""{"a": 1}""", """{"$len": 1}""");
        Ok("", """{"$exists": false}""");
        Bad("null", """{"$exists": false}""");
        Ok("[1, 2]", """{"$contains": 2}""");
        Ok("""{"x": {"y": [{"k": 1, "j": 2}]}}""", """{"$deep": {"k": 1}}""");
        Ok("3", """{"$not": 4}""");
        Ok("", """{"$not": 4}""");
        Ok("3", """{"$one": [1, 3]}""");
        Ok("3", """{"$min": 2.5}""");
        Bad("2", """{"$min": 2.5}""");
        Ok("""[{"id": "a", "v": 1}, {"id": "b", "v": 2}]""", """{"$find": {"where": {"id": "b"}, "want": {"/v": 2}}}""");
        Bad("""[{"id": "a", "v": 1}]""", """{"$find": {"where": {"id": "b"}}}""");
        Bad("1", """{"$sparkle": 1}""");
        var vars = new Dictionary<string, JsonElement> { ["a"] = Jsonv.Of("n1"), ["k"] = Jsonv.Of(7L) };
        Assert.Equal("/result/nodes/n1/x7", PlumCraft.Conformance.Conformance.SubstStr("/result/nodes/$a/x$k", vars));
        Assert.Equal("$zz", PlumCraft.Conformance.Conformance.SubstStr("$zz", vars));
        Assert.Equal(7, PlumCraft.Conformance.Conformance.Subst(Jsonv.Of("$k"), vars).GetInt32());
    }
}

/// <summary>The feed's `document` change (N3 slice 4): the state a window shows, and what isn't one.</summary>
public class DocumentChanges
{
    [Fact]
    public void ADocumentChangeCarriesTheStateAWindowShows()
    {
        var c = Shell.Decode<Changed>(Jsonv.Parse("""{"doc": 2, "domain": "document", "gen": 3, "title": "a.json • — PlumCraft", "document": "a.json", "path": "/x/a.json", "dirty": true, "undo": "Add Node", "redo": null, "lastError": null}"""), Shell.Strict);
        var st = DocumentState.From(c);
        Assert.NotNull(st);
        Assert.True(st.Gen == 3 && st.Dirty && st.Undo == "Add Node" && st.Redo is null && st.Path == "/x/a.json" && st.Name == "a.json");
        Assert.Null(DocumentState.From(Shell.Decode<Changed>(Jsonv.Parse("""{"doc": 2, "domain": "document", "closing": true}"""))));
        Assert.Null(DocumentState.From(Shell.Decode<Changed>(Jsonv.Parse("""{"doc": 2, "domain": "engine", "gen": 1, "connected": true}"""))));
    }
}
