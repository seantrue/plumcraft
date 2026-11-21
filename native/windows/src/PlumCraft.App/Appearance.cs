// The system's appearance, told to the core (`lifecycle.appearance`, contract 14.2): dark or light, high
// contrast, animations off (Reduce Motion) and transparency effects off. The core then shows Paper in
// light and Dusk in dark (the `system` theme), and every document's shell hears it in the feed's `prefs`
// domain and reads theme.get again. The WinUI chrome follows the system by itself; `--appearance`
// fixes both for screenshots.

using Microsoft.UI.Xaml;
using PlumCraft.Core;
using PlumCraft.Models;
using Windows.UI.ViewManagement;

namespace PlumCraft.App;

public static class Appearance
{
    static readonly UISettings ui = new();
    static readonly AccessibilitySettings access = new();
    static PlumCore? core;
    static bool follow;
    static Look? sent;

    public sealed record Look(bool Dark, string Contrast, bool ReduceMotion, bool ReduceTransparency);

    /// <summary>Dark mode is on: the system's background colour is dark.</summary>
    public static bool SystemDark()
    {
        var bg = ui.GetColorValue(UIColorType.Background);
        return bg.R + bg.G + bg.B < 3 * 128;
    }

    public static Look Now(bool dark) => new(dark, access.HighContrast ? "more" : "normal", !ui.AnimationsEnabled, !ui.AdvancedEffectsEnabled);

    /// <summary>The app's fixed appearance, or null to follow the system.</summary>
    public static ElementTheme Theme => CoreHost.Options.Appearance switch
    {
        "dark" => ElementTheme.Dark,
        "light" => ElementTheme.Light,
        _ => ElementTheme.Default,
    };

    public static bool Dark => CoreHost.Options.Appearance switch { "dark" => true, "light" => false, _ => SystemDark() };

    /// <summary>Tell the core now, and again whenever the system's appearance changes (when following it).</summary>
    public static void Follow(PlumCore c, bool followSystem)
    {
        core = c;
        follow = followSystem;
        Canvas.CompositionCanvas.ReduceMotion = !ui.AnimationsEnabled;
        Send();
        try { ui.ColorValuesChanged += (_, _) => Send(); }
        catch (Exception e) { CoreHost.Log($"appearance changes aren't told: {e.Message}"); }
        // Windows 10 2004 and later say when these change; before, they are read with the colours
        if (OperatingSystem.IsWindowsVersionAtLeast(10, 0, 19041))
        {
            try
            {
                ui.AnimationsEnabledChanged += (_, _) => Send();
                ui.AdvancedEffectsEnabledChanged += (_, _) => Send();
            }
            catch (Exception e) { CoreHost.Log($"motion and transparency changes aren't told: {e.Message}"); }
        }
        // a desktop app has no CoreWindow, and some of these refuse a handler without one (0x80070490):
        // then high contrast is read when the colours change, which they do when it is turned on
        try { access.HighContrastChanged += (_, _) => Send(); }
        catch (Exception e) { CoreHost.Log($"high contrast changes aren't told: {e.Message}"); }
    }

    static async void Send()
    {
        if (core is not { IsClosed: false } c) return;
        var look = Now(follow ? SystemDark() : Dark);
        if (look == sent) return;
        sent = look;
        Canvas.CompositionCanvas.ReduceMotion = look.ReduceMotion;
        try
        {
            await c.ResultAsync("lifecycle.appearance", Jsonv.Object(("dark", Jsonv.Of(look.Dark)), ("contrast", Jsonv.Of(look.Contrast)),
                ("reduceMotion", Jsonv.Of(look.ReduceMotion)), ("reduceTransparency", Jsonv.Of(look.ReduceTransparency))));
        }
        catch (Exception e) { CoreHost.Log($"lifecycle.appearance: {e.Message}"); }
        if (follow) foreach (var w in CoreHost.Windows.ToList()) w.DispatcherQueue.TryEnqueue(w.FollowTheme);
    }
}
