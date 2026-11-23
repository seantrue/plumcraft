// PlumCraft for Windows: a path widget's Browse button (B133; contract 8.5, "A path widget's Browse button").
// The core decides what a tap there does (`selection.set` then `row.browse`, its gesture); the shell only
// draws the square and keeps the field editor out of it. Kept here, without a window, so it can be tested.

namespace PlumCraft.Models;

public static class PathBrowse
{
    /// <summary>The path modes that offer Browse (the hint's `mode`).</summary>
    public static readonly IReadOnlyList<string> Modes = ["open", "save", "dir", "files"];

    /// <summary>Does this row's widget draw a Browse button: a path widget with a `pathMode`.</summary>
    public static bool Offered(Widget? w) => w is { KindWord: WidgetWord.Path, PathMode: { } m } && Modes.Contains(m);

    /// <summary>The Browse square in a widget's rect: at its right end, its side the rect's height, at most half
    /// its width (`geometry::browse_rect`).</summary>
    public static Rect Square(Rect widget)
    {
        var side = Math.Max(0, Math.Min(widget.Height, widget.Width / 2));
        return new Rect(widget.X1 - side, widget.Y0, widget.X1, widget.Y1);
    }

    /// <summary>The part of the widget's rect the path's text and its field editor use: all of it short of the
    /// square, so Browse can be tapped while the text is edited.</summary>
    public static Rect Field(Rect widget) => widget with { X1 = Square(widget).X0 };

    /// <summary>A file type a path's dialog offers: its label and extensions without the dot (`*` for any).</summary>
    public sealed record Kind(string Label, IReadOnlyList<string> Extensions);

    /// <summary>Where a path's dialog starts: the current value's folder (itself for a folder), else the folder
    /// the core says these dialogs last used (`startIn`); null when neither exists.</summary>
    public static string? StartFolder(string? current, string? startIn, bool folder)
    {
        foreach (var c in (current ?? "").Split('\n', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
        {
            try
            {
                if (!Path.IsPathRooted(c)) continue;
                if (Directory.Exists(c)) return folder ? c : Path.GetDirectoryName(c) ?? c;
                if (Path.GetDirectoryName(c) is { } dir && Directory.Exists(dir)) return dir;
            }
            catch (ArgumentException) { }
        }
        return startIn is { Length: > 0 } s && Directory.Exists(s) ? s : null;
    }

    /// <summary>The kinds as the dialog's filters ("Text (*.txt)", "*.txt;*.md"), then "All files".</summary>
    public static (string Name, string Spec)[] Filters(IReadOnlyList<Kind> kinds)
    {
        var o = new List<(string, string)>();
        foreach (var k in kinds)
        {
            var ex = k.Extensions.Where(e => e.Length > 0 && e != "*").Select(e => "*." + e.TrimStart('.')).Distinct().ToList();
            if (ex.Count > 0) o.Add(($"{(k.Label.Length > 0 ? k.Label : ex[0])} ({string.Join(", ", ex)})", string.Join(";", ex)));
        }
        o.Add(("All files (*.*)", "*.*"));
        return [.. o];
    }

}
