// The asks on the iPad (contract 11; plan 6's iOS column): the core asks, this graph's view
// answers natively, one modal ask at a time (the core queues the rest). An ask a script started
// (`by: "script"`) is left for the script, never shown, as on the Mac.
//
//   file.open, folder.pick   the document picker (open in place; security-scoped access kept for
//                            the graph's life); a path input's file (purpose `path`, B133) is the
//                            picker's copy, kept in Documents › Picked Files (PickedFiles)
//   file.save                an alert naming the file, saved into the graph's own folder when the
//                            app can write there, else PlumCraft's Documents folder (the contract's
//                            commit flow for a destination without a path, 11.3, isn't in the core);
//                            Save Result… (purpose `result`): the name and format, the file written
//                            into a folder of the app's, then the document picker exports it
//   prompt                   an alert (Save, Don't Save, Cancel; Trust, Cancel)
//   text, form               an alert with text fields
//   clipboard.set, .get      UIPasteboard
//   open.external            a URL opens in its app; a file goes to the share sheet ("Open in…")
//   share                    the share sheet with the file (File › Share…, the core's
//                            `graph.share`, N4 slice 3)
//   screenshot               the canvas drawn offscreen, as a PNG at the path asked

import PhotosUI
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import UIKit
import UniformTypeIdentifiers

@MainActor
final class AskPresenter: NSObject, UIDocumentPickerDelegate, PHPickerViewControllerDelegate {
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
    /// The kinds the photo being picked must be kept as (feat/path-photos).
    private var photoTypes: [UTType] = []
    /// The delegates that keep a closed pick field to its choices, while its form is up.
    var pickGuards: [PickOnly] = []
    /// The refusal each form was last shown again with.
    private var refusedShown: [UInt64: String] = [:]

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
        // a form the core refused stays open with its `error`: shown again with the reason
        for a in asks where handled.contains(a.ask) && showing != a.ask && a.kind.word == .form && a.by != "script" {
            if let e = a.params["error"].string, refusedShown[a.ask] != e {
                refusedShown[a.ask] = e
                handled.remove(a.ask)
            }
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
            // a path input's file (purpose `path`, B133) is copied: the picker hands back a
            // security-scoped URL an engine can't open later, so the copy is what the row names
            // (PickedFiles.keep); a folder is opened in place, for the graph's life
            let copy = p["purpose"].string == "path" && a.kind.word == .fileOpen
            // a path that takes pictures only: the photo library can answer it as well as the
            // files (feat/path-photos), so the person is asked which, as part of answering this
            // one ask; a script answers the ask itself, with a path
            if copy, PickedFiles.picturesOnly(AskWords.extensions(p)) {
                return chooseSource(a, types: types)
            }
            files(a, types: types, copy: copy)
        case .fileSave? where p["purpose"].string == "result":
            saveResult(a)
        case .fileSave?:
            let alert = UIAlertController(title: p["title"].string ?? "Save", message: "Saved in \(saveFolder().lastPathComponent).", preferredStyle: .alert)
            alert.addTextField { f in
                f.text = p["name"].string ?? GraphFiles.newName().lowercased()
                f.clearButtonMode = .whileEditing
            }
            alert.addAction(UIAlertAction(title: "Cancel", style: .cancel) { [weak self] _ in
                self?.done(a)
                self?.cancel(a)
            })
            alert.addAction(UIAlertAction(title: "Save", style: .default) { [weak self, weak alert] _ in
                guard let self else { return }
                self.done(a)
                var name = (alert?.textFields?.first?.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                guard !name.isEmpty, !name.contains("/") else { return self.cancel(a) }
                // a name without an extension takes the first kind's (a graph's: the core's `.plum`)
                if (name as NSString).pathExtension.isEmpty, let k = AskWords.kinds(p).first { name = AskWords.renamed(name, to: k) }
                self.answer(a, ["path": .string(self.saveFolder().appendingPathComponent(name).path)])
            })
            present(alert, for: a)
        case .prompt?:
            let alert = UIAlertController(title: p["title"].string ?? AppID.name, message: AskWords.promptMessage(p), preferredStyle: .alert)
            let answers = (p["answers"].array ?? []).compactMap(\.string)
            for ans in answers.isEmpty ? ["ok"] : answers {
                let style: UIAlertAction.Style = ans == "cancel" ? .cancel : ans == "discard" ? .destructive : .default
                alert.addAction(UIAlertAction(title: AskWords.buttonTitle(ans, prompt: p), style: style) { [weak self] _ in
                    guard let self else { return }
                    self.done(a)
                    if answers.isEmpty { return self.cancel(a) }
                    self.answer(a, ["answer": .string(ans)])
                })
            }
            present(alert, for: a)
        case .text?, .form?:
            let intro = p["intro"].string ?? p["hint"].string ?? p["label"].string
            // a refused answer's reason, with what it is about
            let message = p["error"].string.map { e in intro.map { "\($0)\n\n\(e)" } ?? e } ?? intro
            let alert = UIAlertController(title: p["title"].string ?? AppID.name, message: message, preferredStyle: .alert)
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
                    let pick = AskWords.pick(fd)
                    alert.addTextField { [weak self] f in
                        f.text = pick == nil ? p["values"][key].string : AskWords.pickShown(fd, value: p["values"][key].string ?? "")
                        f.placeholder = fd["label"].string ?? fd["hint"].string ?? key
                        f.isSecureTextEntry = fd["kind"].string == "secret"
                        f.accessibilityLabel = fd["label"].string ?? key
                        guard let pick else { return }
                        // the pick-list field (decision 5): the choices in the field's menu
                        // button; typing only when any other text may be given (`other`)
                        let b = UIButton(type: .system)
                        b.setImage(UIImage(systemName: "chevron.up.chevron.down"), for: .normal)
                        b.accessibilityLabel = "Choices for \(fd["label"].string ?? key)"
                        b.showsMenuAsPrimaryAction = true
                        b.menu = UIMenu(children: pick.choices.map { ch in
                            UIAction(title: ch.label, subtitle: ch.hint) { [weak f] _ in f?.text = ch.label }
                        })
                        f.rightView = b
                        f.rightViewMode = .always
                        if !pick.other {
                            let guardDelegate = PickOnly()
                            f.delegate = guardDelegate
                            self?.pickGuards.append(guardDelegate)
                        }
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
                    let byKey = Dictionary((p["fields"].array ?? []).compactMap { fd in fd["key"].string.map { ($0, fd) } }, uniquingKeysWith: { a, _ in a })
                    for (k, t) in zip(keys, texts) { values[k] = .string(byKey[k].map { AskWords.pickAnswer($0, shown: t) } ?? t) }
                    self.pickGuards = []
                    self.answer(a, ["values": .object(values)])
                }
            })
            present(alert, for: a)
        default:
            done(a)
            cancel(a)
        }
    }

    // MARK: Save Result… (purpose `result`)

    /// Save Result…: the name and format in an alert (the output's formats, by the core, in the
    /// name field's menu; choosing one renames the file), the file written by the engine into a
    /// folder of the app's own, then the system's document picker exports it where the person
    /// chooses (Files, a cloud drive), as iPadOS apps export. The format's options, when it has
    /// any, the core asks for between the two with its form.
    private func saveResult(_ a: Ask) {
        let p = a.params
        let kinds = AskWords.kinds(p)
        let alert = UIAlertController(title: p["title"].string ?? "Save Result", message: "Then choose where it goes.", preferredStyle: .alert)
        alert.addTextField { f in
            f.text = p["name"].string ?? "Result"
            f.clearButtonMode = .whileEditing
            f.accessibilityIdentifier = "save-name"
            guard kinds.count > 1 else { return }
            let b = UIButton(type: .system)
            b.setImage(UIImage(systemName: "chevron.up.chevron.down"), for: .normal)
            b.accessibilityLabel = "Format"
            b.accessibilityIdentifier = "save-format"
            b.showsMenuAsPrimaryAction = true
            b.menu = UIMenu(title: "Format", children: kinds.map { k in
                UIAction(title: k.label, subtitle: "." + k.extensions[0]) { [weak f] _ in
                    f?.text = AskWords.renamed(f?.text ?? "", to: k)
                }
            })
            f.rightView = b
            f.rightViewMode = .always
        }
        alert.addAction(UIAlertAction(title: "Cancel", style: .cancel) { [weak self] _ in
            self?.done(a)
            self?.cancel(a)
        })
        alert.addAction(UIAlertAction(title: "Save", style: .default) { [weak self, weak alert] _ in
            guard let self else { return }
            self.done(a)
            var name = (alert?.textFields?.first?.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            guard !name.isEmpty, !name.contains("/") else { return self.cancel(a) }
            // a name without a format's extension takes the first format's
            if !kinds.isEmpty, (name as NSString).pathExtension.isEmpty { name = AskWords.renamed(name, to: kinds[0]) }
            let dir = FileManager.default.temporaryDirectory.appendingPathComponent("plummer-export-\(UUID().uuidString.prefix(8))", isDirectory: true)
            do { try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true) } catch { return self.cancel(a) }
            let m = self.model
            Task { [weak self] in
                let before = (try? await m.inspect())?["save"]["last"]["request"] ?? .null
                guard let self else { return }
                self.answer(a, ["path": .string(dir.appendingPathComponent(name).path)])
                await self.exportWhenSaved(before: before)
            }
        })
        present(alert, for: a)
    }

    /// Wait (on the core's `save`, at most 5 minutes, the save's own limit) for the save just
    /// asked for, then offer its files to the document picker to export. Nothing pending and no
    /// ask open for a few looks: the person cancelled the options.
    private func exportWhenSaved(before first: JSONValue) async {
        var before = first
        let deadline = Date().addingTimeInterval(300)
        var quiet = 0
        while Date() < deadline {
            try? await Task.sleep(for: .milliseconds(300))
            guard let s = try? await model.inspect() else { continue }
            let save = s["save"]
            let last = save["last"]
            let pending = !save["pending"].isNull
            // the engine asked instead of writing (the save's question, decision 2A): the core's
            // prompt is up; its answer saves again (or nothing, cancelled), so that is the one to wait for
            if !pending, s["prompt"]["kind"].string == "saveShape" {
                before = last["request"]
                quiet = 0
                continue
            }
            if !pending, last["request"] != before, !last["request"].isNull {
                let paths = (last["paths"].array ?? []).compactMap(\.string).map { URL(fileURLWithPath: $0) }
                    .filter { FileManager.default.fileExists(atPath: $0.path) }
                guard last["ok"].bool == true, !paths.isEmpty else {
                    return say("Couldn't save: \(last["error"].string ?? "the engine wrote nothing")")
                }
                StatusFile.shared.set("exporting", .array(paths.map { .string($0.lastPathComponent) }))
                let picker = UIDocumentPickerViewController(forExporting: paths, asCopy: true)
                picker.delegate = exportDelegate
                guard let host else { return }
                var top: UIViewController = host
                while let q = top.presentedViewController, !q.isBeingDismissed { top = q }
                top.present(picker, animated: true)
                return
            }
            quiet = !pending && model.asks.isEmpty ? quiet + 1 : 0
            if quiet >= 10 { return }
        }
    }

    /// The export picker's answer, said (the file is the picker's to copy).
    private lazy var exportDelegate = ExportDelegate { [weak self] urls in
        StatusFile.shared.set("exporting", .null)
        if let u = urls.first { self?.say("Saved \(u.lastPathComponent).") }
    }

    // MARK: The document picker

    /// The document picker for an open or a folder ask.
    private func files(_ a: Ask, types: [UTType], copy: Bool) {
        let p = a.params
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: types, asCopy: copy)
        picker.allowsMultipleSelection = p["multiple"].bool ?? false
        // the folder this ask last used, while it is still there (a path remembered by an earlier
        // install names a container that is gone); else the app's own folder, Plummer (B147)
        if let start = p["startIn"].string, FileManager.default.fileExists(atPath: start) {
            picker.directoryURL = URL(fileURLWithPath: start, isDirectory: true)
        } else {
            picker.directoryURL = CoreHost.dirs.documents
        }
        picker.delegate = self
        picking = a
        // the picker up, for scripts and the UI tests: its ask's purpose and kind
        StatusFile.shared.set("picking", ["purpose": .string(p["purpose"].string ?? ""), "kind": .string(a.kind.raw), "copy": .bool(copy), "source": .string("files")])
        present(picker, for: a)
    }

    // MARK: Photos or Files, for a path that takes pictures (feat/path-photos)

    /// Which source answers this ask: Photos, Files or neither (the ask cancelled). Part of
    /// answering the one ask, as the document picker's own places are.
    private func chooseSource(_ a: Ask, types: [UTType]) {
        let p = a.params
        let sheet = UIAlertController(title: p["title"].string, message: "Choose the picture from", preferredStyle: .actionSheet)
        sheet.view.accessibilityIdentifier = "path-source"
        sheet.addAction(UIAlertAction(title: "Photos", style: .default) { [weak self] _ in
            guard let self else { return }
            self.done(a)
            self.photos(a, types: types)
        })
        sheet.addAction(UIAlertAction(title: "Files", style: .default) { [weak self] _ in
            guard let self else { return }
            self.done(a)
            self.files(a, types: types, copy: true)
        })
        sheet.addAction(UIAlertAction(title: "Cancel", style: .cancel) { [weak self] _ in
            StatusFile.shared.set("picking", .null)
            self?.done(a)
            self?.cancel(a)
        })
        // an iPad shows it as a popover: from the middle of the graph
        if let pop = sheet.popoverPresentationController, let v = host?.view {
            pop.sourceView = v
            pop.sourceRect = CGRect(x: v.bounds.midX, y: v.bounds.midY, width: 1, height: 1)
            pop.permittedArrowDirections = []
        }
        StatusFile.shared.set("picking", ["purpose": .string(p["purpose"].string ?? ""), "kind": .string(a.kind.raw), "copy": .bool(true), "source": .string("choice")])
        present(sheet, for: a)
    }

    /// The photo library's picker, pictures only (it needs no permission: only what the person
    /// picks reaches the app).
    private func photos(_ a: Ask, types: [UTType]) {
        var config = PHPickerConfiguration()
        config.filter = .images
        config.selectionLimit = (a.params["multiple"].bool ?? false) ? 0 : 1
        config.preferredAssetRepresentationMode = .current
        let picker = PHPickerViewController(configuration: config)
        picker.delegate = self
        picking = a
        photoTypes = types
        StatusFile.shared.set("picking", ["purpose": .string(a.params["purpose"].string ?? ""), "kind": .string(a.kind.raw), "copy": .bool(true), "source": .string("photos")])
        present(picker, for: a)
    }

    func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
        StatusFile.shared.set("picking", .null)
        guard let a = picking else { return }
        picking = nil
        picker.dismiss(animated: true)
        done(a)
        if results.isEmpty { return cancel(a) }
        let wanted = photoTypes
        let into = CoreHost.dirs.documents.appendingPathComponent(PickedFiles.folderName, isDirectory: true)
        Task { @MainActor [weak self] in
            var kept: [URL] = []
            var failed: [String] = []
            for r in results {
                if let u = await Self.keepPhoto(r.itemProvider, wanted: wanted, in: into) { kept.append(u) } else { failed.append(r.itemProvider.suggestedName ?? "a photo") }
            }
            guard let self else { return }
            if kept.isEmpty {
                self.say("Couldn't use \(failed.joined(separator: ", ")) as \(wanted.compactMap(\.preferredFilenameExtension).joined(separator: " or ")).")
                return self.cancel(a)
            }
            self.say(kept.count == 1 ? "Copied \(kept[0].lastPathComponent) into Picked Files." : "Copied \(kept.count) photos into Picked Files.")
            StatusFile.shared.set("picked", .array(kept.map { .string($0.path) }))
            self.answer(a, ["paths": .array(kept.map { .string($0.path) })])
        }
    }

    /// A picked photo kept in Picked Files in a form the path takes: as it is when that form is
    /// one of the path's kinds, else converted to the first kind the app can write (PNG or JPEG);
    /// nil when neither is possible.
    private static func keepPhoto(_ provider: NSItemProvider, wanted: [UTType], in dir: URL) async -> URL? {
        let tmp = FileManager.default.temporaryDirectory.appendingPathComponent("photo-\(UUID().uuidString)", isDirectory: true)
        defer { try? FileManager.default.removeItem(at: tmp) }
        guard (try? FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)) != nil else { return nil }
        // as it is: a form the provider has that the path takes
        if let t = provider.registeredContentTypes.first(where: { have in wanted.contains { have.conforms(to: $0) } }) {
            let name = PickedFiles.photoName(suggested: provider.suggestedName, ext: t.preferredFilenameExtension ?? "")
            let copied: URL? = await withCheckedContinuation { done in
                _ = provider.loadFileRepresentation(for: t) { url, _, _ in
                    // the provider's file lasts only for this call: copied out before it returns
                    guard let url else { return done.resume(returning: nil) }
                    let to = tmp.appendingPathComponent(name)
                    done.resume(returning: (try? FileManager.default.copyItem(at: url, to: to)) != nil ? to : nil)
                }
            }
            if let copied { return PickedFiles.keep(copied, in: dir) }
        }
        // converted: PNG or JPEG, the first of them the path takes
        guard let target = wanted.first(where: { $0.conforms(to: .png) || $0.conforms(to: .jpeg) }), provider.canLoadObject(ofClass: UIImage.self) else { return nil }
        let image: UIImage? = await withCheckedContinuation { done in
            _ = provider.loadObject(ofClass: UIImage.self) { obj, _ in done.resume(returning: obj as? UIImage) }
        }
        guard let image, let data = target.conforms(to: .png) ? image.pngData() : image.jpegData(compressionQuality: 0.95) else { return nil }
        let to = tmp.appendingPathComponent(PickedFiles.photoName(suggested: provider.suggestedName, ext: target.preferredFilenameExtension ?? "png"))
        guard (try? data.write(to: to)) != nil else { return nil }
        return PickedFiles.keep(to, in: dir)
    }

    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        StatusFile.shared.set("picking", .null)
        guard let a = picking else { return }
        picking = nil
        done(a)
        // a path input's files (B133): the picker's copies kept in Picked Files, by name
        if a.kind.word == .fileOpen, a.params["purpose"].string == "path" {
            let into = CoreHost.dirs.documents.appendingPathComponent(PickedFiles.folderName, isDirectory: true)
            let kept = urls.compactMap { PickedFiles.keep($0, in: into) }
            if kept.isEmpty { return cancel(a) }
            say(kept.count == 1 ? "Copied \(kept[0].lastPathComponent) into Picked Files." : "Copied \(kept.count) files into Picked Files.")
            StatusFile.shared.set("picked", .array(kept.map { .string($0.path) }))
            return answer(a, ["paths": .array(kept.map { .string($0.path) })])
        }
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
        StatusFile.shared.set("picking", .null)
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

/// The export picker's delegate: where the files went (nothing when it was cancelled).
@MainActor
final class ExportDelegate: NSObject, UIDocumentPickerDelegate {
    let done: ([URL]) -> Void
    init(_ done: @escaping ([URL]) -> Void) { self.done = done }
    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) { done(urls) }
    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) { done([]) }
}

/// A pick field with no other text allowed: its text comes from its choices menu only.
@MainActor
final class PickOnly: NSObject, UITextFieldDelegate {
    func textField(_ textField: UITextField, shouldChangeCharactersIn range: NSRange, replacementString string: String) -> Bool { false }
}
