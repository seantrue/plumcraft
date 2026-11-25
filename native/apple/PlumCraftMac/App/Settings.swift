// Settings and About on the Mac (N4 slice 4). Settings is a window from the app menu's Settings…
// (Command-comma, the core's `app.settings`), a tab per section of the core's model (the kit's
// SettingsView, a grouped Form as System Settings shows), each control setting its value through
// the command its item names. About is the standard About panel with the core's version
// information (`app.about`). A scripted launch never activates the app: its windows are ordered
// just above the document's window, never in front of another app's.

import AppKit
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import SwiftUI

@MainActor
final class SettingsWindow: NSObject, NSWindowDelegate {
    static let shared = SettingsWindow()
    private var window: NSWindow?
    private(set) var settings: SettingsModel?

    /// The core's windows changed: Settings opens while `view.windows.settings` names a section
    /// (`app.settings`, from the menu, a script or the control channel) and closes when it goes.
    func follow(_ tab: String?, model: DocumentModel, over doc: NSWindow?) {
        if let tab {
            show(model: model, over: doc)
            settings?.tab = tab
        } else if let s = settings, s.model === model, window?.isVisible == true {
            closingFromCore = true
            window?.close()
            closingFromCore = false
        }
    }

    private var closingFromCore = false

    func windowWillClose(_ notification: Notification) {
        // the person closed it: the core's window goes too
        if !closingFromCore, let m = settings?.model {
            Task { _ = try? await m.run("app.settings", ["open": false]) }
        }
    }

    /// Show the settings of `model`'s core (every document shares them).
    func show(model: DocumentModel, over doc: NSWindow?) {
        let s: SettingsModel
        var made = false
        if let have = settings, have.model === model {
            s = have
        } else {
            s = SettingsModel(model: model)
            settings = s
            made = true
            model.onPrefs = { [weak s] in Task { await s?.refresh() } }
        }
        let w: NSWindow
        if let have = window {
            w = have
            // another document's core view: the same settings, read through it
            if made { w.contentViewController = NSHostingController(rootView: SettingsView(settings: s)) }
        } else {
            w = make(s)
            window = w
        }
        if w.isVisible { return }
        Task {
            await s.refresh()
            Status.shared.set("settings", describe)
        }
        if NSApp.isActive && !CoreHost.options.scratchState {
            w.makeKeyAndOrderFront(nil)
        } else if let doc, doc.isVisible {
            w.order(.above, relativeTo: doc.windowNumber)
        } else {
            w.orderBack(nil)
        }
    }

    private func make(_ s: SettingsModel) -> NSWindow {
        let w = NSWindow(contentViewController: NSHostingController(rootView: SettingsView(settings: s)))
        w.title = "Plummer Settings"
        w.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        w.tabbingMode = .disallowed
        w.isReleasedWhenClosed = false
        w.setContentSize(NSSize(width: 580, height: 600))
        w.delegate = self
        if CoreHost.options.scratchState {
            w.center()
        } else {
            w.setFrameAutosaveName("PlumCraft Settings")
        }
        return w
    }

    func close() { window?.close() }

    var windowNumber: Int { window?.windowNumber ?? 0 }

    var describe: JSONValue {
        guard let s = settings else { return .null }
        return ["window": .int(Int64(windowNumber)), "visible": .bool(window?.isVisible ?? false),
                "chosen": .string(s.chosen), "shown": .string(s.shown),
                "sections": .array(s.sections.map { .string($0.id) }), "error": s.error.map { .string($0) } ?? .null]
    }
}

@MainActor
enum AboutPanel {
    /// The panel's credits as words, for a script's check (the build line is the first).
    static func credits(_ o: [NSApplication.AboutPanelOptionKey: Any]) -> String {
        (o[.credits] as? NSAttributedString)?.string ?? ""
    }

    /// The standard About panel, with the core's words for the application and what it was
    /// built from (`app.about`: title, version, parts, the engine and its components).
    /// What the core last said for About: shown when no document is open to ask (B169).
    static var lastAbout: JSONValue?

    static func show(model: DocumentModel?, over doc: NSWindow?) {
        Task {
            var options: [NSApplication.AboutPanelOptionKey: Any] = [:]
            var said: JSONValue?
            if let m = model { said = try? await m.run("app.about") }
            if let a = said { lastAbout = a }
            if let a = said ?? lastAbout {
                options[.applicationName] = a["title"].string ?? "Plummer"
                options[.applicationVersion] = a["version"].string ?? ""
                let parts = (a["parts"].array ?? []).compactMap { p -> String? in
                    guard let n = p[0].string, let v = p[1].string else { return nil }
                    return "\(n) \(v)"
                }
                options[.version] = parts.first { $0.hasPrefix("Build ") }.map { String($0.dropFirst(6)) } ?? ""
                var credits = parts.filter { !$0.hasPrefix("Build ") }
                if let e = a["engine"].string, !e.isEmpty { credits.append("Engine: \(e)") }
                // which build this is (B151), first: the panel's credits are selectable text
                if let line = BuildStamp.line(about: a) { credits.insert(line, at: 0) }
                let para = NSMutableParagraphStyle()
                para.alignment = .center
                options[.credits] = NSAttributedString(string: credits.joined(separator: "\n"), attributes: [
                    .font: NSFont.systemFont(ofSize: NSFont.smallSystemFontSize), .foregroundColor: NSColor.secondaryLabelColor, .paragraphStyle: para,
                ])
            }
            let before = Set(NSApp.windows.map(\.windowNumber))
            NSApp.orderFrontStandardAboutPanel(options: options)
            // a scripted launch: just above the document's window, never in front of another app's
            if CoreHost.options.scratchState || !NSApp.isActive,
               let panel = NSApp.windows.first(where: { !before.contains($0.windowNumber) && $0.isVisible }) {
                if let doc, doc.isVisible { panel.order(.above, relativeTo: doc.windowNumber) }
                Status.shared.set("about", ["window": .int(Int64(panel.windowNumber)), "credits": .string(credits(options))])
            } else if let panel = NSApp.windows.first(where: { !before.contains($0.windowNumber) }) {
                Status.shared.set("about", ["window": .int(Int64(panel.windowNumber)), "credits": .string(credits(options))])
            }
        }
    }
}
