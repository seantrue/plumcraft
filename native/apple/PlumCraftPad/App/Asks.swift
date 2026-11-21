// The asks on the iPad (contract 11; plan 6's iOS column): the core asks, this graph's view
// answers natively, one modal ask at a time (the core queues the rest). An ask a script started
// (`by: "script"`) is left for the script, never shown, as on the Mac.
//
//   file.open, folder.pick   the document picker (open in place; security-scoped access kept for
//                            the graph's life)
//   file.save                an alert naming the file, saved into the graph's own folder when the
//                            app can write there, else PlumCraft's Documents folder (the contract's
//                            commit flow for a destination without a path, 11.3, isn't in the core)
//   prompt                   an alert (Save, Don't Save, Cancel; Trust, Cancel)
//   text, form               an alert with text fields
//   clipboard.set, .get      UIPasteboard
//   open.external            a URL opens in its app; a file goes to the share sheet ("Open in…")
//   share                    the share sheet with the file (File › Share…, the core's
//                            `graph.share`, N4 slice 3)
//   screenshot               the canvas drawn offscreen, as a PNG at the path asked

import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import UIKit
import UniformTypeIdentifiers

@MainActor
final class AskPresenter: NSObject, UIDocumentPickerDelegate {
    weak var host: UIViewController?
    let model: DocumentModel
    weak var canvas: CanvasView?
    /// What to say without a dialog.
    var say: (String) -> Void = { _ in }
    /// Called when an alert or picker for an ask opens (scripts take a picture of it).
    var onSheet: ((Ask) -> Void)?
    /// Folders the person picked, kept open for the graph's life (security-scoped).
    private(set) var granted: [URL] = []
    /// The folder a save goes to.
    var saveFolder: () -> URL = { CoreHost.dirs.documents }
    private var showing: UInt64?
    private var shown: UIViewController?
    private var handled: Set<UInt64> = []
    private var picking: Ask?

    init(model: DocumentModel) {
        self.model = model
    }

    /// The pending asks changed: close what the core ended; show the next.
    func update(_ asks: [Ask]) {
        if let s = showing, !asks.contains(where: { $0.ask == s }) {
            showing = nil
            shown?.dismiss(animated: true)
            shown = nil
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
                    handled.insert(a.ask)
                    say("Waiting for a script to answer: \(a.params["title"].string ?? a.kind.raw)")
                    continue
                }
                if showing != nil { continue }
                handled.insert(a.ask)
                modal(a)
            case nil:
                guard person else { continue }
                handled.insert(a.ask)
                say("\(a.kind.raw) isn't available yet in this app.")
                cancel(a)
            }
        }
    }

    func stop() {
        for u in granted { u.stopAccessingSecurityScopedResource() }
        granted = []
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

    private func present(_ vc: UIViewController, for a: Ask) {
        guard let host else { return cancel(a) }
        showing = a.ask
        shown = vc
        // over whatever the graph shows now (a sheet of its own, a pop-out's)
        var top = host
        while let p = top.presentedViewController, !p.isBeingDismissed { top = p }
        top.present(vc, animated: true) { [weak self] in
            if let self, self.showing == a.ask { self.onSheet?(a) }
        }
    }

    /// The alert or picker for `a` closed: the next modal ask may show.
    private func done(_ a: Ask) {
        if showing == a.ask {
            showing = nil
            shown = nil
        }
        let pending = model.asks
        Task { @MainActor [weak self] in self?.update(pending) }
    }

    // MARK: Modal

    private func modal(_ a: Ask) {
        let p = a.params
        switch a.kind.word {
        case .fileOpen?, .folderPick?:
            let types: [UTType]
            if a.kind.word == .folderPick {
                types = [.folder]
            } else {
                let exts = AskWords.extensions(p)
                types = exts.isEmpty ? [.item] : exts.compactMap { UTType(filenameExtension: $0) }
            }
            let picker = UIDocumentPickerViewController(forOpeningContentTypes: types, asCopy: false)
            picker.allowsMultipleSelection = p["multiple"].bool ?? false
            if let start = p["startIn"].string { picker.directoryURL = URL(fileURLWithPath: start, isDirectory: true) }
            picker.delegate = self
            picking = a
            present(picker, for: a)
        case .fileSave?:
            let alert = UIAlertController(title: p["title"].string ?? "Save", message: "Saved in \(saveFolder().lastPathComponent).", preferredStyle: .alert)
            alert.addTextField { f in
                f.text = p["name"].string ?? "untitled.json"
                f.clearButtonMode = .whileEditing
            }
            alert.addAction(UIAlertAction(title: "Cancel", style: .cancel) { [weak self] _ in
                self?.done(a)
                self?.cancel(a)
            })
            alert.addAction(UIAlertAction(title: "Save", style: .default) { [weak self, weak alert] _ in
                guard let self else { return }
                self.done(a)
                let name = (alert?.textFields?.first?.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                guard !name.isEmpty, !name.contains("/") else { return self.cancel(a) }
                self.answer(a, ["path": .string(self.saveFolder().appendingPathComponent(name).path)])
            })
            present(alert, for: a)
        case .prompt?:
            let alert = UIAlertController(title: p["title"].string ?? "Plummer", message: p["text"].string, preferredStyle: .alert)
            let answers = (p["answers"].array ?? []).compactMap(\.string)
            for ans in answers.isEmpty ? ["ok"] : answers {
                let style: UIAlertAction.Style = ans == "cancel" ? .cancel : ans == "discard" ? .destructive : .default
                alert.addAction(UIAlertAction(title: AskWords.buttonTitle(ans, kind: p["kind"].string), style: style) { [weak self] _ in
                    guard let self else { return }
                    self.done(a)
                    if answers.isEmpty { return self.cancel(a) }
                    self.answer(a, ["answer": .string(ans)])
                })
            }
            present(alert, for: a)
        case .text?, .form?:
            let alert = UIAlertController(title: p["title"].string ?? "Plummer",
                                          message: p["intro"].string ?? p["hint"].string ?? p["label"].string, preferredStyle: .alert)
            var keys: [String] = []
            if a.kind.word == .text {
                keys = ["text"]
                alert.addTextField { f in
                    f.text = p["text"].string
                    f.placeholder = p["placeholder"].string
                }
            } else {
                for fd in p["fields"].array ?? [] {
                    guard let key = fd["key"].string else { continue }
                    keys.append(key)
                    alert.addTextField { f in
                        f.text = p["values"][key].string
                        f.placeholder = fd["label"].string ?? fd["hint"].string ?? key
                        f.isSecureTextEntry = fd["kind"].string == "secret"
                    }
                }
            }
            alert.addAction(UIAlertAction(title: "Cancel", style: .cancel) { [weak self] _ in
                self?.done(a)
                self?.cancel(a)
            })
            alert.addAction(UIAlertAction(title: p["submit"].string ?? "OK", style: .default) { [weak self, weak alert] _ in
                guard let self else { return }
                self.done(a)
                let texts = alert?.textFields?.map { $0.text ?? "" } ?? []
                if a.kind.word == .text {
                    self.answer(a, ["text": .string(texts.first ?? "")])
                } else {
                    var values: [String: JSONValue] = [:]
                    for (k, t) in zip(keys, texts) { values[k] = .string(t) }
                    self.answer(a, ["values": .object(values)])
                }
            })
            present(alert, for: a)
        default:
            done(a)
            cancel(a)
        }
    }

    // MARK: The document picker

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        guard let a = picking else { return }
        picking = nil
        done(a)
        // open in place: access is kept while the graph is open
        for u in urls where u.startAccessingSecurityScopedResource() { granted.append(u) }
        if a.kind.word == .folderPick, let u = urls.first {
            answer(a, ["path": .string(u.path)])
        } else if !urls.isEmpty {
            answer(a, ["paths": .array(urls.map { .string($0.path) })])
        } else {
            cancel(a)
        }
    }

    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        guard let a = picking else { return }
        picking = nil
        done(a)
        cancel(a)
    }

    // MARK: Non-modal

    private func nonModal(_ a: Ask) {
        let p = a.params
        switch a.kind.word {
        case .clipboardSet?:
            UIPasteboard.general.string = p["text"].string ?? ""
            answer(a, ["notice": "Copied"])
        case .clipboardGet?:
            if let t = UIPasteboard.general.string { answer(a, ["text": .string(t)]) } else { cancel(a) }
        case .openExternal?:
            if let u = p["url"].string, let url = URL(string: u), ["http", "https", "mailto"].contains(url.scheme ?? "") {
                UIApplication.shared.open(url)
                return answer(a, [:])
            }
            guard let path = p["path"].string else { return answer(a, ["error": "nothing to open"]) }
            shareSheet(a, file: URL(fileURLWithPath: path), done: "Opened")
        default:
            // the contract's `file` (graph.share's), or `path`
            guard let path = p["file"].string ?? p["path"].string else { return answer(a, ["error": "nothing to share"]) }
            shareSheet(a, file: URL(fileURLWithPath: path), done: "Shared")
        }
    }

    /// The share sheet with `file`; the answer once the person is done with it.
    private func shareSheet(_ a: Ask, file: URL, done word: String) {
        guard let host, FileManager.default.fileExists(atPath: file.path) else { return answer(a, ["error": "the file isn't there"]) }
        let sheet = UIActivityViewController(activityItems: [file], applicationActivities: nil)
        sheet.popoverPresentationController?.sourceView = host.view
        sheet.popoverPresentationController?.sourceRect = CGRect(x: host.view.bounds.midX, y: host.view.bounds.midY, width: 1, height: 1)
        sheet.completionWithItemsHandler = { [weak self] _, completed, _, error in
            Task { @MainActor in
                guard let self else { return }
                if let error { self.answer(a, ["error": .string(error.localizedDescription)]) }
                else if completed { self.answer(a, ["notice": .string(word)]) }
                else { self.cancel(a) }
            }
        }
        host.present(sheet, animated: true)
    }

    /// `ui.screenshot`: the canvas as it draws itself.
    private func screenshot(_ a: Ask) {
        guard let path = a.params["path"].string, let v = canvas, let png = v.snapshotPNG(), let img = UIImage(data: png) else {
            return answer(a, ["error": "no canvas to draw yet"])
        }
        do {
            try png.write(to: URL(fileURLWithPath: path), options: .atomic)
            answer(a, ["path": .string(path), "width": .int(Int64(img.size.width * img.scale)), "height": .int(Int64(img.size.height * img.scale)),
                       "source": "offscreen"])
        } catch {
            answer(a, ["error": .string("\(error)")])
        }
    }
}
