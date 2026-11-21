// The iPad's in-place name field (N4 slice 3): one UITextField over what is named, made when the
// core opens a name (`view.name`) and taken away when it closes. Return and Done commit
// (`name.set`), a hardware keyboard's Escape cancels, losing focus commits; a refusal keeps it
// open with the core's words under it. A finger's sizes (EditorPlacement).

#if os(iOS)
import PlumCraftKit
import PlumCraftModels
import UIKit

@MainActor
public final class NameFieldHost: NSObject, UITextFieldDelegate {
    public unowned let canvasView: CanvasView
    public let editor: NameEditor
    public private(set) var field: UITextField?
    var finishing = false
    /// The field came or went (the app reports it).
    public var onShown: (() -> Void)?

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
        let r = CGRect(x: e.rect.x0, y: e.rect.y0, width: max(e.rect.width, 80), height: max(e.rect.height, 22))
        return EditorPlacement.make(row: r, kind: nil, viewport: c.viewport, viewSize: c.viewSize, touch: true)
    }

    func show(_ e: NameEdit) {
        remove()
        let place = placement(e)
        let f = EscapingField(frame: place.frame)
        f.text = e.text
        f.placeholder = e.hint
        f.font = .systemFont(ofSize: max(place.fontSize, 15), weight: .semibold)
        f.borderStyle = .roundedRect
        f.autocorrectionType = .no
        f.autocapitalizationType = .none
        f.smartQuotesType = .no
        f.smartDashesType = .no
        f.returnKeyType = .done
        f.clearButtonMode = .whileEditing
        f.delegate = self
        f.accessibilityLabel = e.label
        f.accessibilityIdentifier = "name-editor"
        f.onEscape = { [weak self] in self?.cancel() }
        f.layer.shadowOpacity = 0.25
        f.layer.shadowRadius = 6
        canvasView.addSubview(f)
        field = f
        finishing = false
        f.becomeFirstResponder()
        f.selectAll(nil)
        onShown?()
    }

    public func follow() {
        guard let e = editor.open, let f = field else { return }
        f.frame = placement(e).frame
    }

    func remove() {
        guard let f = field else { return }
        field = nil
        finishing = true
        f.resignFirstResponder()
        f.removeFromSuperview()
        onShown?()
    }

    /// What a person would type (tests and scripts).
    public func type(_ text: String) { field?.text = text }

    public func commit() {
        guard !finishing, let f = field else { return }
        finishing = true
        let text = f.text ?? ""
        let editor = self.editor
        Task { [weak self] in
            if !(await editor.commit(text)) { self?.finishing = false }
        }
    }

    public func cancel() {
        finishing = true
        Task { await editor.cancel() }
    }

    public func textFieldShouldReturn(_ t: UITextField) -> Bool {
        commit()
        return false
    }

    public func textFieldDidEndEditing(_ t: UITextField) { commit() }
}

/// A text field that hears a hardware keyboard's Escape.
final class EscapingField: UITextField {
    var onEscape: (() -> Void)?
    override var keyCommands: [UIKeyCommand]? {
        [UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(escape))]
    }
    @objc func escape() { onEscape?() }
}
#endif
