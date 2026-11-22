// Small pieces the panels and viewers share (N5 slice 4): the platform's text styles and brushes by their
// theme resource names, Segoe Fluent Icons glyphs, a card in the Windows 11 Settings look, and a
// window for a panel of the core's (Mica, its content's theme, placed beside its document's window).

using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace PlumCraft.App;

public static class Ui
{
    public static readonly string IconFont = Environment.OSVersion.Version.Build >= 22000 ? "Segoe Fluent Icons" : "Segoe MDL2 Assets";

    /// <summary>The Segoe Fluent Icons glyphs the panels use.</summary>
    public static class G
    {
        public const string Close = "", Reset = "", Eye = "", EyeOff = "", Copy = "", Save = "", Share = "",
            Compare = "", Play = "", Pause = "", Back = "", Forward = "", Up = "", Down = "",
            Fit = "", Actual = "", Drag = "", Info = "", Warn = "", Error = "", Settings = "",
            Palette = "", Swatch = "", Stack = "", Files = "", Run = "", Engine = "", Disk = "",
            Loop = "", Volume = "", Collapse = "", More = "";
    }

    public static FontIcon Icon(string glyph, double size = 14) => new() { Glyph = glyph, FontFamily = new FontFamily(IconFont), FontSize = size };

    public static T Res<T>(string key) where T : class => (T)Application.Current.Resources[key];

    public static Style? TextStyle(string key) => Application.Current.Resources.TryGetValue(key, out var s) ? s as Style : null;

    public static Brush Brush(string key) => Res<Brush>(key);

    /// <summary>Secondary text (a caption, a note).</summary>
    public static TextBlock Caption(string text, bool wrap = true) => new()
    {
        Text = text, Style = TextStyle("CaptionTextBlockStyle"), Foreground = Brush("TextFillColorSecondaryBrush"),
        TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap, TextTrimming = wrap ? TextTrimming.None : TextTrimming.CharacterEllipsis,
    };

    /// <summary>A section's heading, as Windows Settings writes them.</summary>
    public static TextBlock Heading(string text) => new() { Text = text, Style = TextStyle("BodyStrongTextBlockStyle"), Margin = new Thickness(0, 12, 0, 4) };

    /// <summary>A card: a row of the Windows 11 Settings look (a filled rounded rectangle with a stroke).</summary>
    public static Border Card(UIElement child, Thickness? padding = null) => new()
    {
        Child = child, Background = Brush("CardBackgroundFillColorDefaultBrush"), BorderBrush = Brush("CardStrokeColorDefaultBrush"),
        BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(6), Padding = padding ?? new Thickness(14, 10, 14, 10),
        Margin = new Thickness(0, 0, 0, 3),
    };

    /// <summary>A button showing only a glyph, with its words as the tooltip and the accessible name.</summary>
    public static Button GlyphButton(string glyph, string tip, Func<Task> run, double size = 14)
    {
        var b = new Button { Content = Icon(glyph, size), Padding = new Thickness(6, 4, 6, 4), Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent), BorderThickness = new Thickness(0) };
        ToolTipService.SetToolTip(b, tip);
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(b, tip);
        b.Click += async (_, _) => await run();
        return b;
    }

    /// <summary>A grid of columns of these widths.</summary>
    public static Grid Columns(params GridLength[] widths)
    {
        var g = new Grid { ColumnSpacing = 8 };
        foreach (var w in widths) g.ColumnDefinitions.Add(new ColumnDefinition { Width = w });
        return g;
    }

    public static readonly GridLength Auto = GridLength.Auto, Star = new(1, GridUnitType.Star);

    public static GridLength Px(double v) => new(v);

    public static T At<T>(this T e, int column, int row = 0) where T : FrameworkElement
    {
        Grid.SetColumn(e, column);
        Grid.SetRow(e, row);
        return e;
    }

    /// <summary>The system's title bar in the app's appearance (dark with a dark app, as the document's caption follows it).</summary>
    public static void TitleBar(Window w)
    {
        try { w.AppWindow.TitleBar.PreferredTheme = Appearance.Dark ? Microsoft.UI.Windowing.TitleBarTheme.Dark : Microsoft.UI.Windowing.TitleBarTheme.Light; }
        catch (Exception) { /* an older Windows App SDK runtime: the system's default */ }
    }

    /// <summary>A window of its own for a panel of the core's: Mica, the app's appearance, its size, placed
    /// beside `owner` in that display's work area (over its right edge where there is no room).</summary>
    public static void Prepare(Window w, Window? owner, int width, int height, int slot = 0)
    {
        w.SystemBackdrop = new MicaBackdrop();
        TitleBar(w);
        w.AppWindow.Resize(new Windows.Graphics.SizeInt32 { Width = width, Height = height });
        if (CoreHost.Options.Top && w.AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter op) op.IsAlwaysOnTop = true;
        if (owner is null) return;
        var o = owner.AppWindow.Position;
        var s = owner.AppWindow.Size;
        var area = Microsoft.UI.Windowing.DisplayArea.GetFromWindowId(owner.AppWindow.Id, Microsoft.UI.Windowing.DisplayAreaFallback.Nearest).WorkArea;
        width = Math.Min(width, area.Width);
        height = Math.Min(height, area.Height);
        var right = o.X + s.Width + 8;
        var x = right + width <= area.X + area.Width ? right : o.X + s.Width - width - 24 - slot * 28;
        var y = o.Y + 48 + slot * 28;
        x = Math.Clamp(x, area.X, Math.Max(area.X, area.X + area.Width - width));
        y = Math.Clamp(y, area.Y, Math.Max(area.Y, area.Y + area.Height - height));
        w.AppWindow.Move(new Windows.Graphics.PointInt32 { X = x, Y = y });
    }
}
