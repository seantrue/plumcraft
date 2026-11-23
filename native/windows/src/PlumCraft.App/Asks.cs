// The core's asks (contract 11) answered natively for the person at this window: the file pickers
// (FileOpenPicker, FileSavePicker, FolderPicker, bound to the window), a ContentDialog for a prompt, a
// text and a form, the clipboard (Windows.ApplicationModel.DataTransfer), Launcher for opening a file or
// a link, and the canvas's own picture for a screenshot. An ask a script started (`by: "script"`) is
// left for the script to answer (11.1). Modal asks are shown one at a time; an ask the core withdraws
// closes its dialog.

using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PlumCraft.Kit;
using PlumCraft.Models;
using Windows.ApplicationModel.DataTransfer;
using Windows.Storage;
using Windows.Storage.Pickers;
using Windows.System;

namespace PlumCraft.App;

public sealed class Asks(DocWindow window, DocumentModel model)
{
    readonly HashSet<ulong> taken = [];
    readonly Dictionary<ulong, ContentDialog> dialogs = [];
    readonly SemaphoreSlim modal = new(1, 1);
    /// <summary>Asks answered here, by kind (the status file says, for scripts that raise one).</summary>
    public int Shown { get; private set; }
    public string? Showing { get; private set; }

    public void Follow(IReadOnlyList<Ask> asks)
    {
        var live = asks.Select(a => a.Id).ToHashSet();
        foreach (var (id, d) in dialogs.ToList())
            if (!live.Contains(id)) { dialogs.Remove(id); d.Hide(); }
        foreach (var a in asks)
        {
            if (a.By == "script" || !taken.Add(a.Id)) continue;
            _ = Answer(a);
        }
    }

    async Task Answer(Ask a)
    {
        var isModal = a.Modal || a.KindWord is AskWord.FileOpen or AskWord.FileSave or AskWord.FolderPick or AskWord.Prompt or AskWord.Form or AskWord.Text;
        if (isModal) await modal.WaitAsync();
        try
        {
            // withdrawn while it waited its turn
            if (!model.Asks.Exists(x => x.Id == a.Id)) return;
            Shown++;
            Showing = a.Kind;
            window.NoteStatus();
            var answer = a.KindWord switch
            {
                AskWord.FileOpen => await FileOpen(a.Params),
                AskWord.FileSave => await FileSave(a.Params),
                AskWord.FolderPick => await FolderPick(),
                AskWord.Prompt => await Prompt(a),
                AskWord.Text => await TextAsk(a),
                AskWord.Form => await Form(a),
                AskWord.ClipboardSet => ClipboardSet(a.Params),
                AskWord.ClipboardGet => await ClipboardGet(),
                AskWord.OpenExternal => await OpenExternal(a.Params),
                AskWord.Share => Jsonv.Object(("error", Jsonv.Of($"Sharing isn't available yet in {CoreHost.AppName} for Windows."))),
                AskWord.Screenshot => await Screenshot(a.Params),
                _ => (JsonElement?)null,
            };
            if (!model.Asks.Exists(x => x.Id == a.Id)) return;
            if (answer is { } v) await model.AnswerAsync(a, v);
            else await model.CancelAsync(a);
        }
        catch (Exception e)
        {
            CoreHost.Log($"ask {a.Kind}: {e.Message}");
            try { await model.CancelAsync(a); } catch (Exception) { }
        }
        finally
        {
            Showing = null;
            window.NoteStatus();
            if (isModal) modal.Release();
        }
    }

    static List<string> Extensions(JsonElement p)
    {
        var outp = new List<string>();
        if (p.Get("kinds") is { ValueKind: JsonValueKind.Array } kinds)
            foreach (var k in kinds.EnumerateArray())
                if (k.Get("extensions") is { ValueKind: JsonValueKind.Array } ex)
                    foreach (var e in ex.EnumerateArray())
                        if (e.ValueKind == JsonValueKind.String && e.GetString() is { Length: > 0 } s) outp.Add(s == "*" ? "*" : "." + s.TrimStart('.'));
        return outp.Contains("*") ? [] : outp.Distinct().ToList();
    }

    T Bound<T>(T picker) where T : class
    {
        WinRT.Interop.InitializeWithWindow.Initialize(picker, window.Hwnd);
        return picker;
    }

    async Task<JsonElement?> FileOpen(JsonElement p)
    {
        var picker = Bound(new FileOpenPicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary, ViewMode = PickerViewMode.List });
        var exts = Extensions(p);
        if (exts.Count == 0) picker.FileTypeFilter.Add("*");
        foreach (var e in exts) picker.FileTypeFilter.Add(e);
        var paths = new List<string>();
        if (p.Get("multiple").IsTrue())
        {
            var files = await picker.PickMultipleFilesAsync();
            if (files is not null) paths.AddRange(files.Select(f => f.Path));
        }
        else if (await picker.PickSingleFileAsync() is { } f) paths.Add(f.Path);
        return paths.Count == 0 ? null : Jsonv.Object(("paths", Jsonv.Array(paths.Select(x => Jsonv.Of(x)))));
    }

    async Task<JsonElement?> FileSave(JsonElement p)
    {
        var picker = Bound(new FileSavePicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary });
        if (p.Get("kinds") is { ValueKind: JsonValueKind.Array } kinds)
            foreach (var k in kinds.EnumerateArray())
            {
                var ex = new List<string>();
                if (k.Get("extensions") is { ValueKind: JsonValueKind.Array } a)
                    foreach (var e in a.EnumerateArray())
                        if (e.ValueKind == JsonValueKind.String && e.GetString() is { Length: > 0 } s && s != "*") ex.Add("." + s.TrimStart('.'));
                if (ex.Count > 0) picker.FileTypeChoices[k.Get("label").Str() ?? ex[0]] = ex;
            }
        if (picker.FileTypeChoices.Count == 0) picker.FileTypeChoices["Any file"] = [".json"];
        if (p.Get("name").Str() is { Length: > 0 } name) picker.SuggestedFileName = Path.GetFileNameWithoutExtension(name);
        var f = await picker.PickSaveFileAsync();
        return f is null ? null : Jsonv.Object(("path", Jsonv.Of(f.Path)));
    }

    async Task<JsonElement?> FolderPick()
    {
        var picker = Bound(new FolderPicker { SuggestedStartLocation = PickerLocationId.DocumentsLibrary });
        picker.FileTypeFilter.Add("*");
        var f = await picker.PickSingleFolderAsync();
        return f is null ? null : Jsonv.Object(("path", Jsonv.Of(f.Path)));
    }

    /// <summary>Windows' words for the core's answers (Save, Don't Save, Cancel).</summary>
    static string Word(string answer, string? kind) => answer switch
    {
        "save" => "Save",
        "discard" => kind == "unsavedChanges" ? "Don't Save" : "Discard",
        "cancel" => "Cancel",
        "trust" => "Trust",
        _ => answer.Length > 0 ? char.ToUpperInvariant(answer[0]) + answer[1..] : answer,
    };

    ContentDialog Dialog(Ask a, string title)
    {
        var d = new ContentDialog { Title = title, XamlRoot = window.Content.XamlRoot, RequestedTheme = window.ContentTheme, DefaultButton = ContentDialogButton.Primary };
        dialogs[a.Id] = d;
        return d;
    }

    static string Detail(JsonElement p)
    {
        var d = p.Get("detail");
        if (d is not { } v) return "";
        // the save's question: its text already says the shape; say where the NumPy file would go
        if (p.Get("kind").Str() == "saveShape")
            return v.Get("npy").Str() is { Length: > 0 } npy ? "As NumPy, it would be saved as " + System.IO.Path.GetFileName(npy) + "." : "";
        if (v.ValueKind == JsonValueKind.String) return v.GetString() ?? "";
        if (v.ValueKind == JsonValueKind.Array)
            return string.Join("\n", v.EnumerateArray().Select(x => x.ValueKind == JsonValueKind.Object
                ? string.Join("  ", x.EnumerateObject().Select(pp => pp.Value.ValueKind == JsonValueKind.String ? pp.Value.GetString() : pp.Value.GetRawText()))
                : x.ValueKind == JsonValueKind.String ? x.GetString() : x.GetRawText()));
        return v.GetRawText();
    }

    async Task<JsonElement?> Prompt(Ask a)
    {
        var p = a.Params;
        var kind = p.Get("kind").Str();
        var answers = p.Get("answers") is { ValueKind: JsonValueKind.Array } arr ? arr.EnumerateArray().Select(x => x.ValueKind == JsonValueKind.String ? x.GetString() ?? "" : "").Where(s => s.Length > 0).ToList() : ["cancel"];
        var d = Dialog(a, p.Get("title").Str() ?? CoreHost.AppName);
        var text = p.Get("text").Str() ?? "";
        var detail = Detail(p);
        d.Content = new TextBlock { Text = detail.Length > 0 ? text + "\n\n" + detail : text, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true };
        var others = answers.Where(x => x != "cancel").ToList();
        // the prompt's own words for its buttons (`labels`, by answer: "Save as PNG anyway"), else Windows' words;
        // Windows says Don't Save for unsaved changes, as every Windows app does
        var labels = p.Get("labels");
        string Label(string answer) =>
            kind == "unsavedChanges" && answer == "discard" ? Word(answer, kind)
            : labels?.Get(answer).Str() is { Length: > 0 } l ? l : Word(answer, kind);
        if (others.Count > 0) d.PrimaryButtonText = Label(others[0]);
        if (others.Count > 1) d.SecondaryButtonText = Label(others[1]);
        if (answers.Contains("cancel")) d.CloseButtonText = Label("cancel");
        var r = await d.ShowAsync();
        dialogs.Remove(a.Id);
        string? chosen = r switch
        {
            ContentDialogResult.Primary => others.ElementAtOrDefault(0),
            ContentDialogResult.Secondary => others.ElementAtOrDefault(1),
            _ => answers.Contains("cancel") ? "cancel" : null,
        };
        return chosen is null ? null : Jsonv.Object(("answer", Jsonv.Of(chosen)));
    }

    async Task<JsonElement?> TextAsk(Ask a)
    {
        var p = a.Params;
        var d = Dialog(a, p.Get("title").Str() ?? CoreHost.AppName);
        var box = new TextBox { Text = p.Get("text").Str() ?? "", PlaceholderText = p.Get("placeholder").Str() ?? "", Header = p.Get("label").Str(), MinWidth = 320 };
        var panel = new StackPanel { Spacing = 8 };
        panel.Children.Add(box);
        if (p.Get("hint").Str() is { Length: > 0 } hint) panel.Children.Add(new TextBlock { Text = hint, Opacity = 0.7, TextWrapping = TextWrapping.Wrap });
        d.Content = panel;
        d.PrimaryButtonText = "OK";
        d.CloseButtonText = "Cancel";
        box.Loaded += (_, _) => { box.Focus(FocusState.Programmatic); box.SelectAll(); };
        var r = await d.ShowAsync();
        dialogs.Remove(a.Id);
        return r == ContentDialogResult.Primary ? Jsonv.Object(("text", Jsonv.Of(box.Text))) : null;
    }

    async Task<JsonElement?> Form(Ask a)
    {
        var p = a.Params;
        var d = Dialog(a, p.Get("title").Str() ?? CoreHost.AppName);
        var panel = new StackPanel { Spacing = 10, MinWidth = 360 };
        if (p.Get("intro").Str() is { Length: > 0 } intro) panel.Children.Add(new TextBlock { Text = intro, TextWrapping = TextWrapping.Wrap });
        if (p.Get("error").Str() is { Length: > 0 } err) panel.Children.Add(new InfoBar { Severity = InfoBarSeverity.Error, Message = err, IsOpen = true, IsClosable = false });
        var values = p.Get("values");
        var fields = new List<(string key, Func<string> read)>();
        if (p.Get("fields") is { ValueKind: JsonValueKind.Array } fs)
            foreach (var f in fs.EnumerateArray())
            {
                var key = f.Get("key").Str() ?? "";
                var label = f.Get("label").Str() ?? key;
                var initial = values?.Get(key).Str() ?? "";
                switch (f.Get("kind").Str())
                {
                    case "secret":
                        var pw = new PasswordBox { Header = label, Password = initial, PlaceholderText = f.Get("hint").Str() ?? "" };
                        panel.Children.Add(pw);
                        fields.Add((key, () => pw.Password));
                        break;
                    case "text":
                        var tb = new TextBox { Header = label, Text = initial, PlaceholderText = f.Get("hint").Str() ?? "", AcceptsReturn = true, TextWrapping = TextWrapping.Wrap, MinHeight = 80 };
                        panel.Children.Add(tb);
                        fields.Add((key, () => tb.Text));
                        break;
                    case "pick":
                        // a pick list (decision 5, N4 person gate): the core's choices in a ComboBox under its label,
                        // editable for any other when `other` is set; the answer is the chosen value or the text typed
                        var choices = f.Get("choices").Items().Select(c => (value: c.Get("value").Str() ?? "", label: c.Get("label").Str() ?? c.Get("value").Str() ?? "")).ToList();
                        var other = f.Get("other").IsTrue();
                        var cb = new ComboBox { Header = label, IsEditable = other, PlaceholderText = f.Get("hint").Str() ?? "", MinWidth = 320, HorizontalAlignment = HorizontalAlignment.Stretch };
                        foreach (var c in choices) cb.Items.Add(c.label);
                        var at = choices.FindIndex(c => c.value == initial);
                        if (at >= 0) cb.SelectedIndex = at;
                        else if (other && initial.Length > 0) cb.Text = initial;
                        panel.Children.Add(cb);
                        fields.Add((key, () =>
                        {
                            var typed = cb.Text ?? "";
                            if (cb.SelectedIndex is var i && i >= 0 && i < choices.Count && (!other || typed.Length == 0 || typed == choices[i].label)) return choices[i].value;
                            var byLabel = choices.FindIndex(c => c.label == typed);
                            return byLabel >= 0 ? choices[byLabel].value : other ? typed.Trim() : "";
                        }));
                        break;
                    default:
                        var line = new TextBox { Header = label, Text = initial, PlaceholderText = f.Get("hint").Str() ?? "" };
                        panel.Children.Add(line);
                        fields.Add((key, () => line.Text));
                        break;
                }
            }
        d.Content = new ScrollViewer { Content = panel, MaxHeight = 480 };
        d.PrimaryButtonText = p.Get("submit").Str() ?? "OK";
        d.CloseButtonText = "Cancel";
        var r = await d.ShowAsync();
        dialogs.Remove(a.Id);
        if (r != ContentDialogResult.Primary) return null;
        return Jsonv.Object(("values", Jsonv.Object([.. fields.Select(f => (f.key, Jsonv.Of(f.read())))])));
    }

    static JsonElement ClipboardSet(JsonElement p)
    {
        var dp = new DataPackage { RequestedOperation = DataPackageOperation.Copy };
        dp.SetText(p.Get("text").Str() ?? "");
        Clipboard.SetContent(dp);
        Clipboard.Flush();
        return Jsonv.EmptyObject;
    }

    static async Task<JsonElement?> ClipboardGet()
    {
        var c = Clipboard.GetContent();
        if (!c.Contains(StandardDataFormats.Text)) return null;
        return Jsonv.Object(("text", Jsonv.Of(await c.GetTextAsync())));
    }

    static async Task<JsonElement?> OpenExternal(JsonElement p)
    {
        bool ok;
        if (p.Get("url").Str() is { Length: > 0 } url && Uri.TryCreate(url, UriKind.Absolute, out var uri))
            ok = await Launcher.LaunchUriAsync(uri);
        else if (p.Get("path").Str() is { Length: > 0 } path)
        {
            if (Directory.Exists(path)) ok = await Launcher.LaunchFolderPathAsync(path);
            else ok = await Launcher.LaunchFileAsync(await StorageFile.GetFileFromPathAsync(path));
        }
        else return Jsonv.Object(("error", Jsonv.Of("nothing to open")));
        return ok ? Jsonv.EmptyObject : Jsonv.Object(("error", Jsonv.Of("Windows has no app to open it")));
    }

    async Task<JsonElement?> Screenshot(JsonElement p)
    {
        var path = p.Get("path").Str();
        if (path is null) return null;
        var (w, h) = await window.CanvasView.Canvas.SnapshotAsync(path);
        return Jsonv.Object(("path", Jsonv.Of(path)), ("width", Jsonv.Of((long)w)), ("height", Jsonv.Of((long)h)), ("source", Jsonv.Of("offscreen")));
    }
}
