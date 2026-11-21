// The document's windows and in-place names, below the views (N4 slice 3; contract 12.5, 12.14):
// the core keeps which windows are open (the scene's `view.windows`: the parameters inspector and
// the pop-outs) and which name is being edited (`view.name`); these models follow that state and
// send what the person did as the core's commands. Each platform draws them natively: on the Mac
// a popover on the node and a real window per pop-out, on the iPad a popover or sheet and a scene
// (or a sheet) per pop-out. Nothing here keeps state of its own that a script couldn't set.

import Foundation
import PlumCraft
import PlumCraftModels

/// A name edited in place (`name.edit`, `name.set`): one text field over `rect`.
@MainActor
public final class NameEditor {
    public let model: DocumentModel
    public private(set) var open: NameEdit?
    /// Why the last name sent was refused (the field stays), cleared by the next.
    public private(set) var error: String?
    public var onOpen: ((NameEdit) -> Void)?
    public var onClose: (() -> Void)?
    public var onError: ((NameEdit, String) -> Void)?

    public init(model: DocumentModel) { self.model = model }

    /// The scene's view changed: open, move or close the field as the core says.
    public func follow(_ e: NameEdit?) {
        guard let e else {
            if open != nil {
                open = nil
                error = nil
                onClose?()
            }
            return
        }
        if e == open { return }
        if open != nil { onClose?() }
        open = e
        error = nil
        onOpen?(e)
    }

    /// Commit what the field holds. False when refused (the field stays, `error` says why).
    @discardableResult
    public func commit(_ text: String) async -> Bool {
        guard let o = open else { return false }
        do {
            try await model.run("name.set", ["text": .string(text)])
            error = nil
            return true
        } catch {
            let why = "\(error)"
            self.error = why
            onError?(o, why)
            return false
        }
    }

    /// Escape: nothing is renamed.
    public func cancel() async {
        _ = try? await model.run("name.edit", ["open": false])
    }
}

/// The parameters inspector over the core (`node.parameters`, `parameters.get`, `row.set`,
/// `node.face`): every input of one node, shown on its face or not.
@MainActor
public final class ParametersModel {
    public let model: DocumentModel
    /// The node it is open on (the core's `view.windows.parameters`).
    public private(set) var node: String?
    public private(set) var reply: ParametersReply?
    public private(set) var error: String?
    /// The node opened or changed (nil: closed), and its rows read again.
    public var onChange: ((ParametersReply?) -> Void)?
    /// What the core said of a value set (a note, or `error` true for a refusal), by port.
    public var onNote: ((String, String, Bool) -> Void)?
    private var faceRev: UInt64?
    private var busy = false
    private var again = false

    public init(model: DocumentModel) { self.model = model }

    /// The windows changed: open, move to another node, or close.
    public func follow(_ w: Windows) {
        if w.parameters != node {
            node = w.parameters
            reply = nil
            faceRev = nil
            if node == nil {
                onChange?(nil)
                return
            }
        }
        refresh()
    }

    /// The scene changed: read the rows again when the node's face did (a value set, a wire
    /// made, a run's state; the face's `rev` says so).
    public func sceneChanged() {
        guard let n = node else { return }
        let rev = model.face(id: n)?.rev
        if rev != faceRev { refresh() }
    }

    public func refresh() {
        guard let n = node else { return }
        if busy {
            again = true
            return
        }
        busy = true
        faceRev = model.face(id: n)?.rev
        Task {
            do {
                let r = try await model.parameters(node: n)
                if node == n {
                    reply = r
                    error = nil
                    onChange?(r)
                }
                model.poke()
            } catch {
                self.error = "\(error)"
            }
            busy = false
            if again {
                again = false
                refresh()
            }
        }
    }

    /// Set a row's value: typed text, or a value (a switch's, a choice's, a colour, a slider's).
    @discardableResult
    public func set(port: String, text: String? = nil, value: JSONValue? = nil) async -> Bool {
        guard let n = node else { return false }
        var p: [String: JSONValue] = ["node": .string(n), "port": .string(port)]
        if let text { p["text"] = .string(text) }
        if let value { p["value"] = value }
        return await send("row.set", .object(p), port: port)
    }

    /// The default back (↺).
    @discardableResult
    public func reset(port: String) async -> Bool {
        guard let n = node else { return false }
        return await send("row.set", ["node": .string(n), "port": .string(port), "reset": true], port: port)
    }

    /// Show the input on the face, or hide it.
    @discardableResult
    public func showOnFace(port: String, _ shown: Bool) async -> Bool {
        guard let n = node else { return false }
        return await send("node.face", ["node": .string(n), "port": .string(port), "shown": .bool(shown)], port: port)
    }

    private func send(_ command: String, _ p: JSONValue, port: String) async -> Bool {
        do {
            let r = try await model.run(command, p)
            if let note = r["note"].string { onNote?(port, note, false) }
            refresh()
            return true
        } catch {
            onNote?(port, "\(error)", true)
            return false
        }
    }

    /// Close the inspector (`node.parameters {}`).
    public func close() async {
        _ = try? await model.run("node.parameters", [:])
    }

    /// Open it on a node (a menu, a script; a double tap goes through `ui.gesture`).
    public func open(_ node: String) async {
        _ = try? await model.run("node.parameters", ["node": .string(node)])
    }
}

/// One pop-out's value and its viewer's controls (`popout.get`, `viewer.zoom`, `viewer.text`,
/// `view.close`).
@MainActor
public final class PopoutModel {
    public let model: DocumentModel
    public let id: UInt64
    public private(set) var window: PopoutWindow
    public private(set) var value: PopoutValue?
    public var onChange: ((PopoutValue) -> Void)?
    private var busy = false
    private var again = false

    public init(model: DocumentModel, window: PopoutWindow) {
        self.model = model
        self.id = window.id
        self.window = window
    }

    /// Its entry in the core's windows changed (the value a run sent, the viewer's state).
    public func update(_ w: PopoutWindow) {
        let changed = w.stamp != window.stamp || value == nil
        window = w
        if changed { refresh() }
    }

    public func refresh() {
        if busy {
            again = true
            return
        }
        busy = true
        Task {
            if let v = try? await model.popout(id: id) {
                value = v
                onChange?(v)
            }
            model.poke()
            busy = false
            if again {
                again = false
                refresh()
            }
        }
    }

    /// Fit or 1:1 (`viewer.zoom`).
    public func zoom(_ to: String) async {
        _ = try? await model.run("viewer.zoom", ["popout": .uint(id), "to": .string(to)])
        refresh()
    }

    /// Wrap, a monospaced face, the whole text (`viewer.text`).
    public func text(wrap: Bool? = nil, mono: Bool? = nil, all: Bool? = nil) async {
        var p: [String: JSONValue] = ["popout": .uint(id)]
        if let wrap { p["wrap"] = .bool(wrap) }
        if let mono { p["mono"] = .bool(mono) }
        if let all { p["all"] = .bool(all) }
        _ = try? await model.run("viewer.text", .object(p))
        refresh()
    }

    /// The person closed the window.
    public func close() async {
        _ = try? await model.run("view.close", ["id": .uint(id)])
    }
}

/// The pop-outs a document has, by id, kept in step with the core: a platform's window manager
/// asks it what to open, bring forward and close.
@MainActor
public struct PopoutDiff {
    public var opened: [PopoutWindow] = []
    public var raised: [PopoutWindow] = []
    public var changed: [PopoutWindow] = []
    public var closed: [UInt64] = []

    /// What changed from `before` to `now`.
    public static func of(_ before: [UInt64: PopoutWindow], _ now: [PopoutWindow]) -> PopoutDiff {
        var d = PopoutDiff()
        let ids = Set(now.map(\.id))
        for w in now {
            guard let b = before[w.id] else {
                d.opened.append(w)
                continue
            }
            if w.raised > b.raised { d.raised.append(w) }
            if w != b { d.changed.append(w) }
        }
        d.closed = before.keys.filter { !ids.contains($0) }.sorted()
        return d
    }
}
