// N5 spike: the application. Unpackaged (WindowsPackageType None) and self-contained Windows App
// SDK, so the .exe runs from its folder with nothing installed.

using Microsoft.UI.Xaml;

namespace PlumCraft.CanvasSpike;

public partial class App : Application
{
    MainWindow? window;

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) =>
        {
            try { File.AppendAllText(Path.Combine(Path.GetTempPath(), "plumcraft-n5-crash.log"), $"{DateTime.Now:O} {e.Exception}\n"); } catch (IOException) { }
        };
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        window = new MainWindow(Options.Parse(Environment.GetCommandLineArgs()));
        window.Activate();
    }
}
