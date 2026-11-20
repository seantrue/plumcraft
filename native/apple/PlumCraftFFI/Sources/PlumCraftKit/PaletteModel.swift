// The node palette below its views (N4 slice 1; contract 12.7): the core keeps whether it is open,
// where and for which dropped wire (`view.palette` in the scene); this model follows that, asks
// the core what it offers for the query (`palette.items`), keeps the highlighted row for the
// keyboard, and sends a pick (`palette.choose`, one undo step, adding and connecting a dropped
// wire's node) or a dismissal (`canvas.palette {open: false}`). Each platform draws it natively
// (a popover on the Mac and the iPad, a sheet in compact width) from this one model, so what the
// palette does is the same in both apps.
//
// A kind whose engine isn't started is listed with `starts` (the core's, N4 slice 1); choosing it
// starts that engine and the core holds the add until it is up, so the model keeps the palette
// showing the pick as pending ("Starting the array engine…") instead of closing it at once.

import Foundation
import PlumCraft
import PlumCraftModels

@MainActor
public final class PaletteModel {
    public let model: DocumentModel
    /// The core's palette while it is open (nil: closed).
    public private(set) var palette: Palette?
    /// What it offers for the query, in category order.
    public private(set) var items: [PaletteItem] = []
    /// How many match in all (the list shows at most 150).
    public private(set) var total: UInt64 = 0
    public private(set) var query = ""
    /// The highlighted row (an index into `items`), moved by the arrow keys.
    public private(set) var selected = 0
    /// A pick sent and not yet answered: the add waits while its engine starts.
    public private(set) var pending: PaletteItem?
    /// Why the last pick was refused (the palette stays open).
    public private(set) var error: String?
    /// The palette opened (the view presents it), closed (the view dismisses it), or its contents
    /// changed (the view reloads).
    public var onOpen: ((Palette) -> Void)?
    public var onClose: (() -> Void)?
    public var onUpdate: (() -> Void)?
    /// Where a palette opened without a place adds (the view's centre, graph units).
    public var fallbackAt: (() -> Point)?
    private var asked = 0
    private var shownOpen = false

    public init(model: DocumentModel) {
        self.model = model
    }

    /// The scene's view changed: open, close or retarget the palette as the core says.
    public func follow(_ view: View) {
        let p = view.palette
        if let p {
            let reopened = !shownOpen || p.at != palette?.at || p.wire != palette?.wire
            palette = p
            if reopened {
                shownOpen = true
                query = p.query ?? ""
                selected = 0
                pending = nil
                error = nil
                items = []
                onOpen?(p)
                refresh()
            }
        } else if shownOpen {
            shownOpen = false
            palette = nil
            pending = nil
            items = []
            onClose?()
        }
    }

    public var isOpen: Bool { shownOpen }

    /// Whether the palette was opened for a dropped wire (only kinds that take it are offered).
    public var forWire: Bool { palette?.wire != nil }

    /// The items by category, in the core's order.
    public var sections: [(title: String, items: [PaletteItem])] {
        var out: [(String, [PaletteItem])] = []
        for it in items {
            if let last = out.last, last.0 == it.category {
                out[out.count - 1].1.append(it)
            } else {
                out.append((it.category, [it]))
            }
        }
        return out
    }

    // MARK: Asking the core

    /// Open the palette at `at` (graph units), as the menu's Search… and its key do.
    public func open(at: Point) async throws {
        _ = try await model.run("canvas.palette", ["open": true, "at": at.json])
    }

    /// The query changed: ask what matches (a reply to an older query is dropped).
    public func search(_ q: String) {
        guard q != query else { return }
        query = q
        selected = 0
        refresh()
    }

    func refresh() {
        asked += 1
        let n = asked
        let q = query
        let core = model.core
        let doc = model.requestDoc
        Task { [weak self] in
            do {
                let r = try await core.request(PaletteItems.self, "palette.items", ["query": .string(q)], doc: doc)
                guard let self, n == asked, shownOpen else { return }
                items = r.items
                total = r.total
                selected = min(selected, max(r.items.count - 1, 0))
                onUpdate?()
            } catch {
                guard let self, n == asked else { return }
                self.error = "\(error)"
                onUpdate?()
            }
        }
    }

    /// Move the highlight by `by` rows (wrapping at neither end).
    public func move(_ by: Int) {
        guard !items.isEmpty else { return }
        selected = max(0, min(items.count - 1, selected + by))
        onUpdate?()
    }

    public func highlight(_ index: Int) {
        guard items.indices.contains(index) else { return }
        selected = index
    }

    /// Choose the highlighted row (Return).
    public func chooseSelected() {
        guard items.indices.contains(selected) else { return }
        choose(items[selected])
    }

    /// Add `item` where the palette opened (and connect the dropped wire): one undo step. The
    /// palette closes when the core says so (the scene's `view.palette` goes); while the add
    /// waits for an engine to start, `pending` says so.
    public func choose(_ item: PaletteItem) {
        guard pending == nil else { return }
        pending = item
        error = nil
        onUpdate?()
        var p: [String: JSONValue] = ["kind": .string(item.kind)]
        if let port = item.port { p["port"] = .string(port) }
        if palette?.at == nil, let at = fallbackAt?() { p["at"] = at.json }
        let model = self.model
        Task { [weak self] in
            do {
                _ = try await model.run("palette.choose", .object(p))
                guard let self else { return }
                pending = nil
                // the scene's view follows; close now if it is already gone
                if model.view.palette == nil { follow(model.view) }
            } catch {
                guard let self else { return }
                pending = nil
                self.error = "\(error)"
                onUpdate?()
            }
        }
    }

    /// Escape, a click away, the sheet swiped down: the core closes the palette, leaving nothing.
    public func dismiss() {
        guard shownOpen, pending == nil else { return }
        shownOpen = false
        palette = nil
        items = []
        // the view closes what it shows (it may have closed it already), and a dropped wire goes
        onClose?()
        let model = self.model
        Task { _ = try? await model.run("canvas.palette", ["open": false]) }
    }

    /// What the palette says while a pick waits for its engine.
    public var pendingText: String? {
        guard let p = pending else { return nil }
        if let e = p.starts { return "Starting the \(e) engine… \(p.label) appears when it is ready." }
        return "Adding \(p.label)…"
    }

    /// The prompt under the search field.
    public var headline: String {
        if forWire { return total == 1 ? "1 kind takes this wire" : "\(total) kinds take this wire" }
        return total == 1 ? "1 kind" : "\(total) kinds"
    }
}
