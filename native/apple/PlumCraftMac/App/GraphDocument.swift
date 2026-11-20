// A graph file as an NSDocument: the system gives File › Open and Open Recent, opening from the
// Dock and Finder, one window per document, the title and its proxy icon. The app only reads: the
// core opens the file itself (`document.open {path}`), so `read` just checks that it is a graph.

import AppKit
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels

@objc(GraphDocument)
final class GraphDocument: NSDocument {
    /// The core's number for this document, once opened.
    var doc: UInt64?
    var model: DocumentModel?

    override class var autosavesInPlace: Bool { false }
    override class func canConcurrentlyReadDocuments(ofType typeName: String) -> Bool { false }

    /// A graph is a JSON object with `nodes`; anything else is refused here with the reason, as
    /// the Mac does for a file an app can't read.
    override nonisolated func read(from data: Data, ofType typeName: String) throws {
        let v = try? JSONSerialization.jsonObject(with: data)
        guard let o = v as? [String: Any], o["nodes"] != nil else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileReadCorruptFileError,
                          userInfo: [NSLocalizedFailureReasonErrorKey: "It isn't a PlumCraft graph (a JSON object with \"nodes\")."])
        }
    }

    override func data(ofType typeName: String) throws -> Data {
        throw NSError(domain: NSCocoaErrorDomain, code: NSFeatureUnsupportedError,
                      userInfo: [NSLocalizedFailureReasonErrorKey: "This build of PlumCraft only shows graphs."])
    }

    override func makeWindowControllers() {
        addWindowController(GraphWindowController(document: self))
    }

    override func close() {
        if let doc, let core = CoreHost.core {
            core.post("document.close", ["discard": true], doc: doc)
            Status.shared.removeWindow(doc)
        }
        model?.stop()
        super.close()
    }
}

/// A drop of graph files on a window opens each in its own window.
final class DropView: NSView {
    override init(frame: NSRect) {
        super.init(frame: frame)
        registerForDraggedTypes([.fileURL])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    func urls(_ s: NSDraggingInfo) -> [URL] {
        (s.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? [])
            .filter { $0.pathExtension.lowercased() == "json" }
    }

    override func draggingEntered(_ s: NSDraggingInfo) -> NSDragOperation { urls(s).isEmpty ? [] : .copy }

    override func performDragOperation(_ s: NSDraggingInfo) -> Bool {
        let list = urls(s)
        for u in list {
            NSDocumentController.shared.openDocument(withContentsOf: u, display: true) { _, _, error in
                if let error { NSApp.presentError(error) }
            }
        }
        return !list.isEmpty
    }
}

@MainActor
final class GraphWindowController: NSWindowController, NSWindowDelegate {
    let container = DropView(frame: NSRect(x: 0, y: 0, width: 1280, height: 820))
    var canvasView: CanvasView?
    let placeholder = NSTextField(labelWithString: "Opening…")
    var shownZoom = -1
    var appliedStart = false

    init(document: GraphDocument) {
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1280, height: 820),
                         styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        w.tabbingMode = .automatic  // tabs when the user prefers them (System Settings), else a window each
        w.minSize = NSSize(width: 480, height: 320)
        w.contentView = container
        placeholder.textColor = .secondaryLabelColor
        placeholder.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(placeholder)
        NSLayoutConstraint.activate([
            placeholder.centerXAnchor.constraint(equalTo: container.centerXAnchor),
            placeholder.centerYAnchor.constraint(equalTo: container.centerYAnchor),
        ])
        super.init(window: w)
        w.delegate = self
        shouldCascadeWindows = true
        w.center()
        open(document)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    /// Open the file in the core, then put the canvas in the window.
    func open(_ document: GraphDocument) {
        guard let core = CoreHost.core, let path = document.fileURL?.path else { return }
        let opened = ContinuousClock.now
        Task { [weak self] in
            await CoreHost.ready?.value
            guard let self else { return }
            do {
                let r = try await core.request("document.open", ["path": .string(path)])
                guard let doc = r["doc"].uint64 else { throw PlumError(.decode, "document.open gave no doc") }
                document.doc = doc
                let model = DocumentModel(core: core, doc: doc)
                document.model = model
                let v = CanvasView(model: model, frame: container.bounds)
                v.autoresizingMask = [.width, .height]
                v.onError = { NSLog("PlumCraft: %@", $0) }
                placeholder.removeFromSuperview()
                container.addSubview(v)
                canvasView = v
                window?.makeFirstResponder(v)
                v.canvas.onViewport = { [weak self] vp in self?.showZoom(vp.zoom) }
                v.canvas.onDrawn = { [weak self] _ in self?.drawn(doc: doc, opened: opened) }
                model.start()
                report(doc: doc, ready: false, opened: opened)
            } catch {
                placeholder.stringValue = "Couldn't open: \(error)"
            }
        }
    }

    func showZoom(_ z: CGFloat) {
        let pct = Int((z * 100).rounded())
        if pct != shownZoom {
            shownZoom = pct
            window?.subtitle = "\(pct)%"
        }
    }

    func drawn(doc: UInt64, opened: ContinuousClock.Instant) {
        guard let v = canvasView else { return }
        let ready = v.canvas.allMeasured && v.canvas.idle && v.model.applied > 0
        report(doc: doc, ready: ready, opened: opened)
        // the start options after the core's own fit (graph.open owes one once every face is measured)
        if ready && !appliedStart && (v.model.view.fit != nil || v.model.faces.count == 0) {
            appliedStart = true
            applyStartOptions(v)
        }
    }

    /// `--zoom`, `--center`, `--select`, once the first frame is there.
    func applyStartOptions(_ v: CanvasView) {
        let o = CoreHost.options
        if !o.select.isEmpty {
            Task { _ = try? await v.model.run("selection.set", ["nodes": .array(o.select.map { .string($0) })]) }
        }
        if let z = o.zoom {
            if let id = o.center, let nid = v.canvas.nid(of: id), let L = v.canvas.faceLayout(nid: nid), let f = v.model.faces[nid]?.item {
                let c = CGPoint(x: f.pos.x + L.size.width / 2, y: f.pos.y + L.size.height / 2)
                let size = v.canvas.viewSize
                v.canvas.setViewport(Viewport(pan: CGPoint(x: size.width / 2 - c.x * z, y: size.height / 2 - c.y * z), zoom: z))
            } else {
                v.canvas.zoom(to: z, animated: false)
            }
        }
    }

    func report(doc: UInt64, ready: Bool, opened: ContinuousClock.Instant) {
        guard let w = window else { return }
        var s: [String: JSONValue] = ["doc": .uint(doc), "window": .int(Int64(w.windowNumber)), "ready": .bool(ready),
                                      "path": .string(document?.fileURL?.path ?? "")]
        if let v = canvasView {
            s["faces"] = .int(Int64(v.canvas.faceCount))
            s["wires"] = .int(Int64(v.canvas.wireLayerCount))
            s["zoom"] = .double(Double(v.canvas.viewport.zoom))
            if let f = v.canvas.firstFrameAt { s["firstFrameMs"] = .double(Double((f - opened).components.attoseconds) / 1e15 + Double((f - opened).components.seconds) * 1000) }
            let sel = v.model.selection.nodes
            s["selection"] = .array(sel.map { .string($0) })
        }
        Status.shared.window(doc, .object(s))
    }

    func windowDidChangeBackingProperties(_ notification: Notification) {
        canvasView?.canvas.settle()
    }
}
