// The Mac's document windows from the core (decision 20, "Mac adopts it"; contract 4.3,
// `platform.document_windows`). File › New and File › Open… in a window are the core's commands
// (`graph.new`, `graph.open`): the core makes the new document and the feed's `window` domain,
// `changed {doc, domain: "window", open: true, from, by, path?}`, says so; this makes an NSDocument
// and its window for that document, so the Mac keeps what makes it a Mac app: the title bar with
// its proxy icon and edited dot, Save, Save As, Revert and Rename through NSDocument, Open Recent
// (a file the core opened is noted there, except in a scripted launch, B50), tabs and the Window
// menu. A file the core opens into an untouched window (its rule, as AppKit's) moves the
// document's file there (GraphDocument.follow).
//
// What stays AppKit's, because the two cannot both hold (note 24, the Mac's document windows):
// - with no window open there is no document to run a command in, so New and Open then go to
//   NSDocumentController (and so do the Dock's reopen, a launch with nothing to open, a file from
//   the Finder or Open Recent, and a drop), each opening its document with `document.open`;
// - closing a window is NSDocument's (its Save, Don't Save, Cancel sheet, which runs the Save
//   panel for an untitled graph), then `document.close {discard: true}`;
// - Quit is AppKit's (the review of unsaved documents, and the system's own quit for a log out
//   or a restart, which must be answered from applicationShouldTerminate).

import AppKit
import PlumCraft
import PlumCraftModels
import PlumCraftShell

@MainActor
enum DocumentWindows {
    private static var task: Task<Void, Never>?

    /// Follow the core's `window` domain for as long as it is open.
    static func follow(_ core: PlumCore) {
        let stream = core.feed()
        task = Task { @MainActor in
            for await batch in stream {
                for c in batch.changed where c.domain.word == .window && c.open == true {
                    open(doc: c.doc, path: c.path)
                }
            }
        }
    }

    /// A window for the core's document `doc`, which the core already holds.
    static func open(doc: UInt64, path: String?) {
        let dc = NSDocumentController.shared
        if dc.documents.contains(where: { ($0 as? GraphDocument)?.doc == doc }) { return }
        let d = GraphDocument()
        d.doc = doc
        if let path, !path.isEmpty {
            let url = URL(fileURLWithPath: path)
            d.fileURL = url
            d.fileType = GraphDocument.type(of: url)
            d.fileModificationDate = (try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate]) as? Date
            dc.noteNewRecentDocumentURL(url)
        }
        dc.addDocument(d)
        d.makeWindowControllers()
        d.showWindows()
        Status.shared.set("documentWindows", ["opened": .uint(doc), "path": path.map(JSONValue.string) ?? .null])
    }
}
