// The asks (contract 11; plan 6): the core asks, this window answers natively. Sheets attach to
// the document's window, one modal ask at a time (the core queues the rest). An ask a script
// started (`by: "script"`) is left for the script, never shown: an automated run puts no panel on
// the screen. The non-modal kinds a person started (the clipboard, opening in another app) are
// answered at once; `screenshot` is answered whoever asked, from the canvas's own picture.
//
//   file.open, folder.pick   NSOpenPanel as a sheet
//   file.save                NSSavePanel as a sheet
//   prompt                   NSAlert as a sheet (Save, Don't Save, Cancel; Trust, Cancel)
//   text, form               NSAlert as a sheet with text fields
//   clipboard.set, .get      NSPasteboard
//   open.external            NSWorkspace (an app named, or the system's default; Finder reveals)
//   screenshot               the canvas drawn offscreen, as a PNG at the path asked
//   share                    NSSharingServicePicker with the file, under the toolbar (File ›
//                            Share…, the core's `graph.share`, N4 slice 3); answered when a
//                            service is picked, cancelled when the picker closes without one

import AppKit
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

@MainActor
final class AskPresenter {
    weak var window: NSWindow?
    let model: DocumentModel
    weak var canvas: CanvasView?
    /// What to say in the window without a dialog.
    var say: (String) -> Void = { _ in }
    /// The ask a sheet is open for, and how to close it if the core ends it first.
    private var showing: UInt64?
    private var closeSheet: (() -> Void)?
    private var handled: Set<UInt64> = []
    /// Called when a sheet for an ask opens (scripts take a picture of it), with the ask.
    var onSheet: ((Ask) -> Void)?

    init(model: DocumentModel) {
        self.model = model
    }

    /// The pending asks changed: close a sheet whose ask ended; show the next.
    func update(_ asks: [Ask]) {
        if let s = showing, !asks.contains(where: { $0.ask == s }) {
            showing = nil
            closeSheet?()
            closeSheet = nil
        }
        for a in asks where !handled.contains(a.ask) {
            let person = a.by != "script"
            switch a.kind.word {
            case .screenshot?:
                handled.insert(a.ask)
                screenshot(a)
            case .clipboardSet?, .clipboardGet?, .openExternal?, .share?:
                guard person else { continue }
                handled.insert(a.ask)
                nonModal(a)
            case .fileOpen?, .fileSave?, .folderPick?, .prompt?, .text?, .form?:
                guard person else {
                    if !handled.contains(a.ask) {
                        handled.insert(a.ask)
                        say("Waiting for a script to answer: \(a.params["title"].string ?? a.kind.raw)")
                    }
                    continue
                }
                if showing != nil { continue }
                handled.insert(a.ask)
                modal(a)
            case nil:
                // a kind this app doesn't know: refused, so the chain ends
                guard person else { continue }
                handled.insert(a.ask)
                say("\(a.kind.raw) isn't available yet in this app.")
                cancel(a)
            }
        }
    }

    private func answer(_ a: Ask, _ v: JSONValue) {
        let m = model
        Task {
            do { try await m.answer(a, v) } catch { say("\(error)") }
        }
    }

    private func cancel(_ a: Ask) {
        let m = model
        Task { try? await m.cancel(a) }
    }

    // MARK: Modal: sheets on the window

    private func modal(_ a: Ask) {
        guard let window else { return cancel(a) }
        showing = a.ask
        let p = a.params
        switch a.kind.word {
        case .fileOpen?, .folderPick?:
            let panel = NSOpenPanel()
            panel.message = p["prompt"].string ?? p["title"].string ?? ""
            panel.prompt = a.kind.word == .folderPick ? "Choose" : "Open"
            panel.canChooseDirectories = a.kind.word == .folderPick
            panel.canChooseFiles = a.kind.word != .folderPick
            panel.allowsMultipleSelection = p["multiple"].bool ?? false
            if let start = p["startIn"].string { panel.directoryURL = URL(fileURLWithPath: start, isDirectory: true) }
            let exts = AskWords.extensions(p)
            if !exts.isEmpty { panel.allowedContentTypes = exts.compactMap { .init(filenameExtension: $0) } }
            closeSheet = { [weak window, weak panel] in if let panel { window?.endSheet(panel, returnCode: .cancel) } }
            panel.beginSheetModal(for: window) { [weak self] r in
                guard let self else { return }
                self.done(a)
                guard r == .OK else { return self.cancel(a) }
                if a.kind.word == .folderPick, let u = panel.url {
                    self.answer(a, ["path": .string(u.path)])
                } else if !panel.urls.isEmpty {
                    self.answer(a, ["paths": .array(panel.urls.map { .string($0.path) })])
                } else {
                    self.cancel(a)
                }
            }
            onSheet?(a)
        case .fileSave?:
            let panel = NSSavePanel()
            panel.message = p["title"].string ?? ""
            panel.nameFieldStringValue = p["name"].string ?? "untitled"
            if let start = p["startIn"].string { panel.directoryURL = URL(fileURLWithPath: start, isDirectory: true) }
            let exts = AskWords.extensions(p)
            if !exts.isEmpty { panel.allowedContentTypes = exts.compactMap { .init(filenameExtension: $0) } }
            panel.canCreateDirectories = true
            closeSheet = { [weak window, weak panel] in if let panel { window?.endSheet(panel, returnCode: .cancel) } }
            panel.beginSheetModal(for: window) { [weak self] r in
                guard let self else { return }
                self.done(a)
                guard r == .OK, let u = panel.url else { return self.cancel(a) }
                self.answer(a, ["path": .string(u.path)])
            }
            onSheet?(a)
        case .prompt?:
            let alert = NSAlert()
            alert.messageText = p["title"].string ?? "Plummer"
            alert.informativeText = p["text"].string ?? ""
            let answers = (p["answers"].array ?? []).compactMap(\.string)
            for ans in answers { alert.addButton(withTitle: Self.buttonTitle(ans, kind: p["kind"].string)) }
            if answers.isEmpty { alert.addButton(withTitle: "OK") }
            if let k = answers.firstIndex(of: "discard") { alert.buttons[k].hasDestructiveAction = true }
            if let k = answers.firstIndex(of: "cancel") { alert.buttons[k].keyEquivalent = "\u{1b}" }
            closeSheet = { [weak window, weak alert] in if let s = alert?.window { window?.endSheet(s, returnCode: .cancel) } }
            alert.beginSheetModal(for: window) { [weak self] r in
                guard let self else { return }
                self.done(a)
                let k = r.rawValue - NSApplication.ModalResponse.alertFirstButtonReturn.rawValue
                guard k >= 0, k < answers.count else { return self.cancel(a) }
                self.answer(a, ["answer": .string(answers[k])])
            }
            onSheet?(a)
        case .text?, .form?:
            let alert = NSAlert()
            alert.messageText = p["title"].string ?? "Plummer"
            alert.informativeText = p["intro"].string ?? p["hint"].string ?? p["label"].string ?? ""
            var fields: [(String, NSTextField)] = []
            // a form is a grid, as the system's own are: each label right-aligned beside its
            // field, on the field's baseline (B73: the labels were stacked above all the fields)
            var rows: [[NSView]] = []
            let fieldWidth: CGFloat = 280
            if a.kind.word == .text {
                let f = NSTextField(string: p["text"].string ?? "")
                f.placeholderString = p["placeholder"].string
                fields.append(("text", f))
                rows.append([f])
            } else {
                for fd in p["fields"].array ?? [] {
                    guard let key = fd["key"].string else { continue }
                    let f: NSTextField = fd["kind"].string == "secret" ? NSSecureTextField() : NSTextField()
                    f.stringValue = p["values"][key].string ?? ""
                    f.placeholderString = fd["hint"].string
                    let label = NSTextField(labelWithString: (fd["label"].string ?? key) + ":")
                    label.alignment = .right
                    f.setAccessibilityLabel(fd["label"].string ?? key)
                    fields.append((key, f))
                    rows.append([label, f])
                }
            }
            for (_, f) in fields {
                f.translatesAutoresizingMaskIntoConstraints = false
                f.widthAnchor.constraint(equalToConstant: fieldWidth).isActive = true
            }
            let grid = NSGridView(views: rows)
            grid.rowSpacing = 8
            grid.columnSpacing = 8
            grid.rowAlignment = .firstBaseline
            if grid.numberOfColumns > 1 { grid.column(at: 0).xPlacement = .trailing }
            // each field tabs to the next, and the last back to the first
            for (k, (_, f)) in fields.enumerated() where fields.count > 1 { f.nextKeyView = fields[(k + 1) % fields.count].1 }
            grid.translatesAutoresizingMaskIntoConstraints = true
            grid.frame = NSRect(origin: .zero, size: grid.fittingSize)
            alert.accessoryView = grid
            alert.addButton(withTitle: p["submit"].string ?? "OK")
            alert.addButton(withTitle: "Cancel").keyEquivalent = "\u{1b}"
            alert.window.initialFirstResponder = fields.first?.1
            closeSheet = { [weak window, weak alert] in if let s = alert?.window { window?.endSheet(s, returnCode: .cancel) } }
            alert.beginSheetModal(for: window) { [weak self] r in
                guard let self else { return }
                self.done(a)
                guard r == .alertFirstButtonReturn else { return self.cancel(a) }
                if a.kind.word == .text {
                    self.answer(a, ["text": .string(fields.first?.1.stringValue ?? "")])
                } else {
                    var values: [String: JSONValue] = [:]
                    for (k, f) in fields { values[k] = .string(f.stringValue) }
                    self.answer(a, ["values": .object(values)])
                }
            }
            onSheet?(a)
        default:
            done(a)
            cancel(a)
        }
    }

    /// The sheet for `a` closed: the next modal ask may show.
    private func done(_ a: Ask) {
        if showing == a.ask {
            showing = nil
            closeSheet = nil
        }
        // show what queued meanwhile
        let pending = model.asks
        Task { @MainActor [weak self] in self?.update(pending) }
    }

    /// The Mac's words for the core's answers (the kit's, shared with the iPad).
    static func buttonTitle(_ answer: String, kind: String?) -> String { AskWords.buttonTitle(answer, kind: kind) }

    // MARK: Non-modal

    private func nonModal(_ a: Ask) {
        let p = a.params
        switch a.kind.word {
        case .clipboardSet?:
            let pb = NSPasteboard.general
            pb.clearContents()
            pb.setString(p["text"].string ?? "", forType: .string)
            answer(a, ["notice": "Copied"])
        case .clipboardGet?:
            if let t = NSPasteboard.general.string(forType: .string) {
                answer(a, ["text": .string(t)])
            } else {
                cancel(a)
            }
        case .openExternal?:
            openExternal(a)
        default:
            share(a)
        }
    }

    /// The share ask: the system's picker for the file, shown from the window's top right.
    private func share(_ a: Ask) {
        guard let path = a.params["file"].string, let content = window?.contentView else {
            return answer(a, ["error": "nothing to share"])
        }
        let picker = NSSharingServicePicker(items: [URL(fileURLWithPath: path)])
        let d = ShareDelegate { [weak self] chosen in
            guard let self else { return }
            if let chosen { answer(a, ["notice": .string("Shared with \(chosen)")]) } else { cancel(a) }
            sharing = nil
        }
        sharing = d
        picker.delegate = d
        let r = NSRect(x: content.bounds.maxX - 60, y: content.bounds.maxY - 4, width: 1, height: 1)
        picker.show(relativeTo: r, of: content, preferredEdge: .minY)
    }

    private var sharing: ShareDelegate?

    /// Open a file in another app, or the system's default for it, or reveal it in Finder.
    private func openExternal(_ a: Ask) {
        let p = a.params
        let ws = NSWorkspace.shared
        if let u = p["url"].string, let url = URL(string: u), ["http", "https", "mailto"].contains(url.scheme ?? "") {
            ws.open(url)
            return answer(a, [:])
        }
        guard let path = p["path"].string else { return answer(a, ["error": "nothing to open"]) }
        let file = URL(fileURLWithPath: path)
        let app = p["app"].string
        if app == "Finder" || app == "reveal" {
            ws.activateFileViewerSelecting([file])
            return answer(a, ["notice": "Shown in Finder"])
        }
        let config = NSWorkspace.OpenConfiguration()
        let done: @Sendable (NSRunningApplication?, (any Error)?) -> Void = { [weak self] _, error in
            Task { @MainActor in
                guard let self else { return }
                if let error {
                    self.answer(a, ["error": .string(error.localizedDescription)])
                } else {
                    self.answer(a, [:])
                }
            }
        }
        if let app, let appURL = Self.application(app) {
            ws.open([file], withApplicationAt: appURL, configuration: config, completionHandler: done)
        } else if app != nil {
            answer(a, ["error": .string("there is no application \(app ?? "") here")])
        } else {
            ws.open(file, configuration: config, completionHandler: done)
        }
    }

    /// An application by path, bundle id or name.
    static func application(_ app: String) -> URL? {
        if app.hasPrefix("/") { return URL(fileURLWithPath: app) }
        if let u = NSWorkspace.shared.urlForApplication(withBundleIdentifier: app) { return u }
        for dir in ["/Applications", "/System/Applications", "/Applications/Utilities", "/System/Applications/Utilities"] {
            let u = URL(fileURLWithPath: dir).appendingPathComponent(app.hasSuffix(".app") ? app : app + ".app")
            if FileManager.default.fileExists(atPath: u.path) { return u }
        }
        return nil
    }

    /// `ui.screenshot`: the canvas as it draws itself (not the window server's picture).
    private func screenshot(_ a: Ask) {
        guard let path = a.params["path"].string, let v = canvas, let png = v.snapshotPNG(), let img = NSBitmapImageRep(data: png) else {
            return answer(a, ["error": "no canvas to draw yet"])
        }
        do {
            try png.write(to: URL(fileURLWithPath: path), options: .atomic)
            answer(a, ["path": .string(path), "width": .int(Int64(img.pixelsWide)), "height": .int(Int64(img.pixelsHigh)), "source": "offscreen"])
        } catch {
            answer(a, ["error": .string("\(error)")])
        }
    }
}

/// The share picker's answer: the service chosen, or none.
@MainActor
final class ShareDelegate: NSObject, NSSharingServicePickerDelegate {
    let done: (String?) -> Void
    init(_ done: @escaping (String?) -> Void) { self.done = done }
    nonisolated func sharingServicePicker(_ picker: NSSharingServicePicker, didChoose service: NSSharingService?) {
        let title = service?.title
        MainActor.assumeIsolated { done(title) }
    }
}
