// Editing a value on a node's face, below its views (N4 slice 2; contract 12.10): the core keeps
// which row a field editor is open on (`row.edit`, the scene's `view.edit`), checks what the field
// sends (`row.set`: parsed, made whole, held to its range, a choice one of its own) and makes it
// one undo step. This model follows that state, and sends what the person did: a commit (Return,
// Done, focus lost, a menu's pick, a colour picker closed), a cancel (Escape) or a move (Tab,
// Shift-Tab). Each platform's host puts one native control over the row while it is open and takes
// it away after (the field-editor pattern, plan 5.4): never a control per row.
//
// A value the core refuses keeps the editor open with the core's words (`error`); a note (a value
// held to its range) goes to `onNote` once the value is set.

import Foundation
import PlumCraft
import PlumCraftModels

@MainActor
public final class RowEditor {
    public let model: DocumentModel

    /// The row open: the core's `view.edit`, with its row and widget as the face shows them.
    public struct Open: Equatable, Sendable {
        public var edit: RowEdit
        public var row: InRow
        public var widget: Widget
        public var label: String { row.label.isEmpty ? edit.port : row.label }
        public var kind: WidgetWord? { widget.kind.word }
    }

    public private(set) var open: Open?
    /// Why the last value sent was refused (the editor stays open), cleared by the next.
    public private(set) var error: String?
    /// The editor opened on a row (the host puts its control there), closed, or the core said
    /// something of a value (a note, or `error` true for a refusal).
    public var onOpen: ((Open) -> Void)?
    public var onClose: (() -> Void)?
    public var onNote: ((Open, String, Bool) -> Void)?
    /// What happens after a value is taken.
    public enum Then: Sendable { case close, next, previous, stay }

    public init(model: DocumentModel) {
        self.model = model
    }

    /// The scene's view changed: open, move or close the editor as the core says.
    public func follow(_ view: View) {
        guard let e = view.edit else {
            if open != nil {
                open = nil
                error = nil
                onClose?()
            }
            return
        }
        if e == open?.edit { return }
        guard let face = model.faces.item(id: e.face), e.row >= 0, e.row < face.inputs.count, let w = face.inputs[e.row].widget else { return }
        let o = Open(edit: e, row: face.inputs[e.row], widget: w)
        if open != nil { onClose?() }
        open = o
        error = nil
        onOpen?(o)
    }

    /// Open the editor on a row (a script, a test; a tap goes through `ui.gesture`).
    public func edit(node: String, port: String) async throws {
        try await model.run("row.edit", ["node": .string(node), "port": .string(port)])
    }

    /// Commit what the field holds; then close it or go to the next or previous row. False when
    /// the core refused it (the editor stays, `error` says why).
    @discardableResult
    public func commit(text: String, then: Then = .close) async -> Bool {
        await set(["text": .string(text)], then: then)
    }

    /// Commit a value (a switch's, a menu's choice, a colour `[r, g, b]` in 0…1).
    @discardableResult
    public func commit(value: JSONValue, then: Then = .close) async -> Bool {
        await set(["value": value], then: then)
    }

    func set(_ what: [String: JSONValue], then: Then) async -> Bool {
        guard let o = open else { return false }
        var p = what
        p["node"] = .string(o.edit.node)
        p["port"] = .string(o.edit.port)
        do {
            let r = try await model.run("row.set", .object(p))
            error = nil
            if let note = r["note"].string { onNote?(o, note, false) }
        } catch {
            let why = "\(error)"
            self.error = why
            onNote?(o, why, true)
            return false
        }
        switch then {
        case .close: await close()
        case .next: await step(1)
        case .previous: await step(-1)
        case .stay: break
        }
        return true
    }

    /// Escape: the editor goes, nothing is set.
    public func close() async {
        _ = try? await model.run("row.edit", ["open": false])
    }

    /// Tab (1) or Shift-Tab (-1): the next or previous row that can be edited.
    public func step(_ by: Int) async {
        _ = try? await model.run("row.edit", ["step": .int(Int64(by))])
    }
}
