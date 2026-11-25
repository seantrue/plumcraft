// macOS host app. `--bench out.json [--nodes N --wires M --phase-seconds S --modes perWire,batched,hybrid]`
// runs the scripted benchmark and quits; `--screenshot out.png` writes a picture after launch and quits.
import AppKit
import CanvasKit

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!

    func applicationDidFinishLaunching(_ note: Notification) {
        let args = CommandLine.arguments
        let options = BenchOptions.parse(args)
        let frame = NSRect(x: 0, y: 0, width: 1440, height: 900)
        window = NSWindow(contentRect: frame, styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        window.title = options == nil ? "Canvas Spike: 1/2/3 wire modes, f fits" : "Canvas Spike: benchmark"
        let view = CanvasHostView(frame: frame, options: options)
        window.contentView = view
        window.center()
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(view)
        NSApp.activate()
        print("window: \(window.windowNumber)")
        if options == nil, let i = args.firstIndex(of: "--zoom"), i + 1 < args.count, let z = Double(args[i + 1]) {
            let b = view.canvas.scene.bounds
            view.canvas.center(on: CGPoint(x: b.midX, y: b.midY), zoom: z)
            view.canvas.settle()
        }
        if let i = args.firstIndex(of: "--screenshot"), i + 1 < args.count {
            let path = args[i + 1]
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(1.5))
                view.canvas.settle()
                try? await Task.sleep(for: .seconds(0.5))
                view.snapshot(to: path)
                if !args.contains("--stay") { NSApp.terminate(nil) }
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

@main
enum Main {
    @MainActor static func main() {
        setvbuf(stdout, nil, _IOLBF, 0)
        let app = NSApplication.shared
        let delegate = AppDelegate()
        app.delegate = delegate
        app.setActivationPolicy(.regular)
        app.run()
    }
}
