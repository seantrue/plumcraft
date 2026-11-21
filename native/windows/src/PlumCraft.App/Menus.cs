// The menu bar, context menus and the toolbar are the core's menu model (`menu.get`, contract 12.1 and
// 12.4; the toolbar `menu.get {context: {kind: "toolbar"}}`, N4 slice 3), drawn with WinUI's own MenuBar,
// MenuFlyout and CommandBar. Disabled items carry the core's reason as their tooltip; checks and radio
// choices are the core's `checked`; shortcuts are the core's, shown in the items and matched here
// (12.2: `primary` is Ctrl on Windows). Choosing an item dispatches the core's command by id
// (DocWindow.Dispatch): nothing here does a user action itself.

using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using PlumCraft.Models;
using Windows.System;
using Windows.UI.Core;

namespace PlumCraft.App;

public sealed class Menus(MenuBar bar, Func<MenuItem, bool, Task> dispatch)
{
    readonly MenuBar bar = bar;
    readonly Dictionary<MenuItem, MenuFlyoutItemBase> shown = new(ReferenceEqualityComparer.Instance);
    string? signature;
    List<MenuItem> model = [];
    public IReadOnlyList<MenuItem> Model => model;

    /// <summary>The menu bar from the core's menus: rebuilt when their shape changed, else updated in place.</summary>
    public void Show(IReadOnlyList<MenuItem> menus)
    {
        var sig = Signature(menus);
        if (sig == signature && shown.Count > 0)
        {
            // the same shape: labels, enabled, checks and reasons in place, so an open menu stays open
            var old = shown.Keys.ToList();
            var now = menus.SelectMany(m => m.Items.SelectMany(i => i.Flatten())).ToList();
            var next = new Dictionary<MenuItem, MenuFlyoutItemBase>(ReferenceEqualityComparer.Instance);
            for (int k = 0; k < Math.Min(old.Count, now.Count); k++)
            {
                var el = shown[old[k]];
                Update(el, now[k]);
                next[now[k]] = el;
            }
            shown.Clear();
            foreach (var kv in next) shown[kv.Key] = kv.Value;
            model = [.. menus];
            return;
        }
        signature = sig;
        model = [.. menus];
        shown.Clear();
        bar.Items.Clear();
        var keys = new HashSet<char>();
        foreach (var top in menus)
        {
            var mbi = new MenuBarItem { Title = top.Label };
            // Alt opens the bar with a key for each menu: its first letter not taken, else its next
            foreach (var ch in top.Label.ToUpperInvariant())
                if (char.IsLetter(ch) && keys.Add(ch)) { mbi.AccessKey = ch.ToString(); break; }
            foreach (var it in top.Items) if (Make(it, top.Id) is { } el) mbi.Items.Add(el);
            bar.Items.Add(mbi);
        }
    }

    static string Signature(IEnumerable<MenuItem> items) =>
        string.Join("|", items.Select(i => $"{i.Id}:{i.Label}:{i.Separator}:{i.Look}:{i.Checked.HasValue}[{Signature(i.Items)}]"));

    /// <summary>A flyout's items for a context menu (the same rules as the menu bar's).</summary>
    public MenuFlyout Flyout(IEnumerable<MenuItem> items)
    {
        var f = new MenuFlyout();
        foreach (var it in items) if (Make(it, "context", context: true) is { } el) f.Items.Add(el);
        return f;
    }

    MenuFlyoutItemBase? Make(MenuItem it, string group, bool context = false)
    {
        if (it.Separator) return new MenuFlyoutSeparator();
        if (it.IsSubmenu)
        {
            var sub = new MenuFlyoutSubItem { Text = it.Label, IsEnabled = it.Items.Count > 0 };
            foreach (var c in it.Items) if (Make(c, it.Id, context) is { } el) sub.Items.Add(el);
            if (!context) shown[it] = sub;
            return sub;
        }
        MenuFlyoutItem item = it.LookWord switch
        {
            LookWord.Radio or LookWord.Choice => new RadioMenuFlyoutItem { GroupName = group },
            _ when it.Checked is not null || it.LookWord == LookWord.Check => new ToggleMenuFlyoutItem(),
            _ => new MenuFlyoutItem(),
        };
        Update(item, it);
        // the item is read when chosen (Tag): an update in place gives it the model's latest
        item.Click += async (s, _) => { if (s is FrameworkElement { Tag: MenuItem now }) await dispatch(now, true); };
        if (!context) shown[it] = item;
        return item;
    }

    static void Update(MenuFlyoutItemBase el, MenuItem it)
    {
        if (el is MenuFlyoutSubItem sub)
        {
            sub.Text = it.Label;
            return;
        }
        if (el is not MenuFlyoutItem item) return;
        item.Tag = it;
        item.Text = it.Label;
        var plain = it.LookWord is LookWord.Note or LookWord.Text or LookWord.Dim or LookWord.Section;
        item.IsEnabled = it.Enabled && it.Command is not null && !plain;
        ToolTipService.SetToolTip(item, !it.Enabled && it.Reason is { Length: > 0 } r ? r : it.Hover);
        if (it.Shortcut is { } sc) item.KeyboardAcceleratorTextOverride = Describe(sc);
        if (item is ToggleMenuFlyoutItem t) t.IsChecked = it.Checked == true;
        if (item is RadioMenuFlyoutItem rd) rd.IsChecked = it.Checked == true;
    }

    // ---------------- shortcuts ----------------

    /// <summary>A shortcut as Windows writes it: Ctrl+Shift+S.</summary>
    public static string Describe(Shortcut s)
    {
        var parts = new List<string>();
        if (s.Mods.Contains("primary") || s.Mods.Contains("ctrl")) parts.Add("Ctrl");
        if (s.Mods.Contains("alt")) parts.Add("Alt");
        if (s.Mods.Contains("shift")) parts.Add("Shift");
        parts.Add(s.Key switch
        {
            "Period" => ".", "Comma" => ",", "Plus" => "+", "Minus" => "-", "Equals" => "=", "Backspace" => "Backspace",
            "ArrowUp" => "Up", "ArrowDown" => "Down", "ArrowLeft" => "Left", "ArrowRight" => "Right", "Escape" => "Esc",
            _ => s.Key,
        });
        return string.Join("+", parts);
    }

    /// <summary>The egui key name of a Windows key (contract 12.2), or null.</summary>
    static string? KeyName(VirtualKey k) => k switch
    {
        >= VirtualKey.A and <= VirtualKey.Z => ((char)('A' + (k - VirtualKey.A))).ToString(),
        >= VirtualKey.Number0 and <= VirtualKey.Number9 => ((char)('0' + (k - VirtualKey.Number0))).ToString(),
        >= VirtualKey.F1 and <= VirtualKey.F12 => $"F{k - VirtualKey.F1 + 1}",
        VirtualKey.Enter => "Enter",
        VirtualKey.Escape => "Escape",
        VirtualKey.Tab => "Tab",
        VirtualKey.Space => "Space",
        VirtualKey.Delete => "Delete",
        VirtualKey.Back => "Backspace",
        VirtualKey.Up => "ArrowUp",
        VirtualKey.Down => "ArrowDown",
        VirtualKey.Left => "ArrowLeft",
        VirtualKey.Right => "ArrowRight",
        (VirtualKey)190 => "Period",
        (VirtualKey)188 => "Comma",
        (VirtualKey)189 => "Minus",
        (VirtualKey)187 => "Equals",
        VirtualKey.Add => "Plus",
        VirtualKey.Subtract => "Minus",
        _ => null,
    };

    static bool Down(VirtualKey k) =>
        (Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(k) & CoreVirtualKeyStates.Down) == CoreVirtualKeyStates.Down;

    /// <summary>The model's item a key press with the keys held now is the shortcut of, the most specific
    /// first; plain keys only when `plainKeys` (no text field has the keyboard).</summary>
    public MenuItem? Match(VirtualKey key, bool plainKeys)
    {
        if (KeyName(key) is not { } name) return null;
        bool ctrl = Down(VirtualKey.Control), shift = Down(VirtualKey.Shift), alt = Down(VirtualKey.Menu);
        if (!ctrl && !alt && !plainKeys) return null;
        foreach (var it in model.SelectMany(m => m.Flatten()))
        {
            if (it.Shortcut is not { } s || it.Command is null) continue;
            var keyOk = string.Equals(s.Key, name, StringComparison.OrdinalIgnoreCase) || (name == "Equals" && s.Key == "Plus");
            if (!keyOk) continue;
            var wantCtrl = s.Mods.Contains("primary") || s.Mods.Contains("ctrl");
            if (wantCtrl == ctrl && s.Mods.Contains("shift") == shift && s.Mods.Contains("alt") == alt) return it;
        }
        return null;
    }

    /// <summary>Handle a key the window got: the core's shortcut, dispatched by id (a disabled command still
    /// takes its keys, 12.2).</summary>
    public async Task<bool> KeyAsync(KeyRoutedEventArgs e, bool plainKeys)
    {
        if (Match(e.Key, plainKeys) is not { } it) return false;
        e.Handled = true;
        if (it.Enabled) await dispatch(it, true);
        return true;
    }
}
