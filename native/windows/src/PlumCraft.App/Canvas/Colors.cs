// The theme's tokens as Windows colours (theme.get, contract 14.1). The core's colours are premultiplied
// (8.3); Win2D and Composition take straight alpha, so they are divided out here.

using PlumCraft.Models;
using Windows.UI;

namespace PlumCraft.App.Canvas;

public sealed class CanvasColors
{
    public IReadOnlyDictionary<string, Rgba> Tokens { get; }
    public bool IsDark { get; }

    public CanvasColors(ThemeReply? theme)
    {
        Tokens = theme?.Tokens ?? new Dictionary<string, Rgba>();
        // dark when the canvas is darker than mid grey (Dusk), whatever the theme is called
        var c = Token("canvas", Color.FromArgb(255, 250, 248, 244));
        IsDark = (c.R * 0.299 + c.G * 0.587 + c.B * 0.114) < 128;
    }

    public static Color Straight(Rgba c)
    {
        if (c.A == 0) return Color.FromArgb(0, 0, 0, 0);
        if (c.A == 255) return Color.FromArgb(255, c.R, c.G, c.B);
        byte U(byte v) => (byte)Math.Min(255, v * 255 / c.A);
        return Color.FromArgb(c.A, U(c.R), U(c.G), U(c.B));
    }

    public Color Token(string name, Color fallback) => Tokens.TryGetValue(name, out var c) ? Straight(c) : fallback;

    /// <summary>A paint: its token in this theme, or its own colour; clear for neither.</summary>
    public Color Of(Paint? p) => p is null ? Color.FromArgb(0, 0, 0, 0) : Straight(p.Resolve(Tokens));

    public static Color WithAlpha(Color c, double a) => Color.FromArgb((byte)Math.Clamp(c.A * a, 0, 255), c.R, c.G, c.B);

    public Color Canvas => Token("canvas", Color.FromArgb(255, 250, 248, 244));
    public Color Text => Token("text", Color.FromArgb(255, 30, 30, 30));
    public Color TextDim => Token("text_dim", Color.FromArgb(255, 110, 110, 110));
    public Color TextFaint => Token("text_faint", Color.FromArgb(255, 150, 150, 150));
    public Color HeaderText => Token("header_text", Text);
    public Color Accent => Token("accent", Color.FromArgb(255, 0, 120, 212));
    public Color Field => Token("field", Color.FromArgb(255, 255, 255, 255));
    public Color FieldBorder => Token("field_border", Color.FromArgb(255, 180, 180, 180));
    public Color Separator => Token("separator", Color.FromArgb(255, 200, 200, 200));
    public Color Placeholder => Token("placeholder", Color.FromArgb(255, 220, 220, 220));
    public Color Grid => Token("grid", Color.FromArgb(255, 230, 230, 230));
    public Color Wire => Token("wire", Color.FromArgb(255, 120, 120, 120));
}
