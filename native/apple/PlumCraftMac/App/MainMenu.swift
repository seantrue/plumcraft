// The minimal main menu for this slice (slice 4 builds the real one from `menu.get`): the
// application menu, File (Open, Open Recent, Close), View (Fit, Zoom In, Zoom Out, Actual Size,
// Full Screen) and Window. The View items go down the responder chain to the canvas.

import AppKit

@MainActor
enum MainMenu {
    static func make() -> NSMenu {
        let bar = NSMenu()
        let name = "PlumCraft"

        let app = submenu(bar, name)
        app.addItem(withTitle: "About \(name)", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        app.addItem(.separator())
        let services = NSMenu(title: "Services")
        app.addItem(withTitle: "Services", action: nil, keyEquivalent: "").submenu = services
        NSApp.servicesMenu = services
        app.addItem(.separator())
        app.addItem(withTitle: "Hide \(name)", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let others = app.addItem(withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
        others.keyEquivalentModifierMask = [.command, .option]
        app.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        app.addItem(.separator())
        app.addItem(withTitle: "Quit \(name)", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")

        let file = submenu(bar, "File")
        file.addItem(withTitle: "Open…", action: #selector(NSDocumentController.openDocument(_:)), keyEquivalent: "o")
        let recent = NSMenu(title: "Open Recent")
        // the document controller fills this menu itself: it finds it by its Clear Menu item
        recent.addItem(withTitle: "Clear Menu", action: #selector(NSDocumentController.clearRecentDocuments(_:)), keyEquivalent: "")
        file.addItem(withTitle: "Open Recent", action: nil, keyEquivalent: "").submenu = recent
        file.addItem(.separator())
        file.addItem(withTitle: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")

        let view = submenu(bar, "View")
        view.addItem(withTitle: "Fit", action: Selector(("fitView:")), keyEquivalent: "9")
        view.addItem(withTitle: "Zoom In", action: Selector(("zoomIn:")), keyEquivalent: "+")
        view.addItem(withTitle: "Zoom Out", action: Selector(("zoomOut:")), keyEquivalent: "-")
        view.addItem(withTitle: "Actual Size", action: Selector(("actualSize:")), keyEquivalent: "0")
        view.addItem(.separator())
        let full = view.addItem(withTitle: "Enter Full Screen", action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f")
        full.keyEquivalentModifierMask = [.command, .control]

        let window = submenu(bar, "Window")
        window.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        window.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        window.addItem(.separator())
        window.addItem(withTitle: "Bring All to Front", action: #selector(NSApplication.arrangeInFront(_:)), keyEquivalent: "")
        NSApp.windowsMenu = window

        let help = submenu(bar, "Help")
        NSApp.helpMenu = help
        return bar
    }

    static func submenu(_ bar: NSMenu, _ title: String) -> NSMenu {
        let m = NSMenu(title: title)
        bar.addItem(withTitle: title, action: nil, keyEquivalent: "").submenu = m
        return m
    }
}
