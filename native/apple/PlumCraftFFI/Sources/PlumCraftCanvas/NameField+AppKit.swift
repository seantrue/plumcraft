// The Mac's in-place name field (N4 slice 3): one NSTextField over what is named (a face's title,
// a frame's tab, a group's band, a net's tag, an input's row, a wire's middle), made when the core
// opens a name (`view.name`) and taken away when it closes. Return commits (`name.set`), Escape
// cancels, losing focus commits; a refusal keeps it open with the core's words under it.

#if os(macOS)
import AppKit
import PlumCraftKit
import PlumCraftModels

@MainActor
public final class NameFieldHost: NSObject, NSTextFieldDelegate {
    public unowned let canvasView: CanvasView
    public let editor: NameEditor
    public private(set) var field: NSTextField?
    var finishing = false

    init(canvasView: CanvasView, editor: NameEditor) {
        self.canvasView = canvasView
        self.editor = editor
        super.init()
        editor.onOpen = { [weak self] e in self?.show(e) }
        editor.onClose = { [weak self] in self?.remove() }
        editor.onError = { [unowned canvasView] e, why in
            canvasView.canvas.flashNote(why, under: CGRect(x: e.rect.x0, y: e.rect.y0, width: e.rect.width, height: e.rect.height), error: true)
        }
    }

    func placement(_ e: NameEdit) -> EditorPlacement {
        let c = canvasView.canvas
        let r = CGRect(x: e.rect.x0, y: e.rect.y0, width: max(e.rect.width, 60), height: max(e.rect.height, 18))
        return EditorPlacement.make(row: r, kind: nil, viewport: c.viewport, viewSize: c.viewSize, touch: false)
    }

    func show(_ e: NameEdit) {
        remove()
        guard let sup = canvasView.superview else { return }
        let place = placement(e)
        let f = NSTextField(string: e.text)
        f.font = .systemFont(ofSize: max(place.fontSize, 12), weight: .semibold)
        f.isBezeled = true
        f.bezelStyle = .roundedBezel
        f.focusRingType = .exterior
        f.usesSingleLineMode = true
        f.cell?.isScrollable = true
        f.placeholderString = e.hint
        f.delegate = self
        f.setAccessibilityLabel(e.label)
        f.setAccessibilityIdentifier("name-editor")
        f.frame = sup.convert(place.frame, from: canvasView)
        sup.addSubview(f, positioned: .above, relativeTo: canvasView)
        field = f
        finishing = false
        canvasView.window?.makeFirstResponder(f)
        f.currentEditor()?.selectAll(nil)
    }

    /// The canvas moved or zoomed: the field follows.
    public func follow() {
        guard let e = editor.open, let f = field, let sup = canvasView.superview else { return }
        f.frame = sup.convert(placement(e).frame, from: canvasView)
    }

    func remove() {
        guard let f = field else { return }
        field = nil
        finishing = true
        f.removeFromSuperview()
        canvasView.window?.makeFirstResponder(canvasView)
    }

    /// What a person would type (tests and scripts).
    public func type(_ text: String) { field?.stringValue = text }

    /// Return.
    public func commit() {
        guard !finishing, let f = field else { return }
        finishing = true
        let text = f.stringValue
        let editor = self.editor
        Task { [weak self] in
            if !(await editor.commit(text)) { self?.finishing = false }
        }
    }

    /// Escape.
    public func cancel() {
        finishing = true
        Task { await editor.cancel() }
    }

    public func control(_ control: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        switch sel {
        case #selector(NSResponder.insertNewline(_:)): commit()
        case #selector(NSResponder.cancelOperation(_:)): cancel()
        default: return false
        }
        return true
    }

    public func controlTextDidEndEditing(_ n: Notification) { commit() }
}
#endif
