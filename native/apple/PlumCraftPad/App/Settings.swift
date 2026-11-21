// Settings and About on the iPad (N4 slice 4): an in-app sheet over the graph, as iPadOS apps put
// their own settings (the Settings app bundle is for what must be set before launch; nothing here
// is), from the core's model through the kit's SettingsView: a list of the sections, each a
// grouped page, every control setting its value through the command its item names. Opened from
// the app menu's Settings… (Command-comma, the core's `app.settings`) and the bar's gear. About is
// a small sheet with the core's version information (`app.about`).

import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import SwiftUI
import UIKit

/// A graph's settings sheet, following the core's `view.windows.settings` (`app.settings`, from
/// the menu, the gear, a script or the control channel); closing it closes the core's window.
@MainActor
final class PadSettingsSheet: NSObject, UIAdaptivePresentationControllerDelegate {
    let model: DocumentModel
    weak var host: UIViewController?
    private var sheet: UIViewController?
    private(set) var settings: SettingsModel?

    init(model: DocumentModel) { self.model = model }

    func follow(_ tab: String?) {
        if let tab {
            if sheet == nil { present() }
            settings?.tab = tab
        } else if let sh = sheet, let host {
            sheet = nil
            Presentations.whenSettled(from: host) { sh.dismiss(animated: !CoreHost.options.scratchState) }
        }
    }

    private func present() {
        guard let vc = host else { return }
        let s = SettingsModel(model: model)
        settings = s
        model.onPrefs = { [weak s] in Task { await s?.refresh() } }
        let h = UIHostingController(rootView: SettingsView(settings: s, done: { [weak self] in self?.closed() }))
        h.modalPresentationStyle = .formSheet
        h.view.backgroundColor = .sheetGround
        h.view.accessibilityIdentifier = "settings"
        h.presentationController?.delegate = self
        sheet = h
        Task {
            await s.refresh()
            StatusFile.shared.set("settings", ["sections": .array(s.sections.map { .string($0.id) }), "chosen": .string(s.chosen), "shown": .string(s.shown)])
        }
        Presentations.whenSettled(from: vc) { [weak self, weak vc] in
            guard let self, let vc, self.sheet === h else { return }
            (vc.presentedViewController ?? vc).present(h, animated: !CoreHost.options.scratchState)
        }
    }

    /// Done, or a swipe down: the core's window closes too.
    func closed() {
        if let sh = sheet {
            sheet = nil
            sh.dismiss(animated: true)
        }
        let m = model
        Task { _ = try? await m.run("app.settings", ["open": false]) }
    }

    func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        sheet = nil
        let m = model
        Task { _ = try? await m.run("app.settings", ["open": false]) }
    }
}

@MainActor
enum PadSettings {
    /// About: the app and what it was built from, the core's words.
    static func about(over vc: UIViewController, model: DocumentModel) {
        Task { [weak vc] in
            let a = (try? await model.run("app.about")) ?? .null
            let parts = (a["parts"].array ?? []).compactMap { p -> String? in
                guard let n = p[0].string, let v = p[1].string else { return nil }
                return "\(n) \(v)"
            }
            let view = AboutView(title: a["title"].string ?? "PlumCraft", version: a["version"].string ?? "", parts: parts,
                                 engine: a["engine"].string, done: { [weak vc] in vc?.dismiss(animated: true) })
            let host = UIHostingController(rootView: view)
            host.modalPresentationStyle = .formSheet
            host.preferredContentSize = CGSize(width: 380, height: 360)
            vc?.present(host, animated: !CoreHost.options.scratchState)
            StatusFile.shared.set("about", ["version": .string(a["version"].string ?? "")])
        }
    }
}

struct AboutView: SwiftUI.View {
    let title: String
    let version: String
    let parts: [String]
    let engine: String?
    let done: () -> Void

    var body: some SwiftUI.View {
        NavigationStack {
            VStack(spacing: 10) {
                Image(systemName: "point.3.connected.trianglepath.dotted").font(.system(size: 48)).foregroundStyle(.tint)
                Text(title).font(.title2.bold())
                Text("Version \(version)").foregroundStyle(.secondary)
                ForEach(parts, id: \.self) { Text($0).font(.footnote).foregroundStyle(.secondary) }
                if let e = engine, !e.isEmpty { Text("Engine: \(e)").font(.footnote).foregroundStyle(.secondary) }
            }
            .padding(24)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done", action: done) } }
        }
    }
}
