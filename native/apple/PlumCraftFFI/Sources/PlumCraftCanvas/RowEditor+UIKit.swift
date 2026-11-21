// The iPad's field editor for a face's row (N4 slice 2): one UIKit control at a time, made when
// the core opens an editor (`view.edit`) and taken away when it closes, placed over the row's
// widget at the canvas's zoom (EditorPlacement, a finger's sizes), the canvas scrolled so the row
// stays above the on-screen keyboard.
//
//   text, path, several choices, a number   UITextField: the keyboard for the kind (numbers and
//                                            punctuation for a number, so a minus and a point are
//                                            there), no autocorrection, capitals, smart quotes or
//                                            dashes (values are often code); a bar over the
//                                            keyboard with Previous, Next and Done; Return and
//                                            Done commit, a hardware keyboard's Escape cancels and
//                                            Tab and Shift-Tab commit and move, as on the Mac;
//                                            losing focus commits
//   several lines                            UITextView, monospaced: Return is a new line, Done
//                                            or ⌘Return commits
//   a choice                                 UIKit's own pull-down menu, on the row: a touch on a
//                                            choice's row goes to a button over it whose menu is
//                                            the core's choices (the one set ticked, a disabled one
//                                            greyed with its reason); a pick is `row.set`
//   a colour                                 the system colour picker in a popover; committed when
//                                            it closes
//   a switch                                 drawn as a switch; a tap flips it in the core
//
// The more-button (⋮) is routed the same way as a choice: a touch on it goes to a button over it
// whose menu is the core's node menu, so it is the system's menu, not a sheet.

#if os(iOS)
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import UIKit

@MainActor
public final class FieldEditorHost: NSObject, FieldEditing, UITextFieldDelegate, UITextViewDelegate, UIColorPickerViewControllerDelegate {
    public unowned let canvasView: CanvasView
    public let editor: RowEditor
    public private(set) var control: UIView?
    var finishing = false
    var keyboardTop: CGFloat?
    /// The button a touch on a choice's row or the more-button goes to (the system's menu).
    public let menuButton = UIButton(type: .custom)
    /// Present a view controller (the colour picker) from a rect in the canvas view: the app's.
    public var present: ((UIViewController, CGRect) -> Void)?
    /// The control came or went (the app reports it).
    public var onShown: (() -> Void)?

    init(canvasView: CanvasView, editor: RowEditor) {
        self.canvasView = canvasView
        self.editor = editor
        super.init()
        editor.onOpen = { [weak self] o in self?.show(o) }
        editor.onClose = { [weak self] in self?.remove() }
        editor.onNote = { [unowned canvasView] o, text, error in
            guard let nid = canvasView.canvas.nid(of: o.edit.face), let r = canvasView.canvas.rowRect(face: nid, row: o.edit.row) else { return }
            canvasView.canvas.flashNote(text, under: r, error: error)
        }
        menuButton.showsMenuAsPrimaryAction = true
        menuButton.isHidden = true
        menuButton.accessibilityIdentifier = "face-menu"
        canvasView.addSubview(menuButton)
        NotificationCenter.default.addObserver(self, selector: #selector(keyboard(_:)), name: UIResponder.keyboardWillChangeFrameNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(keyboardHidden(_:)), name: UIResponder.keyboardWillHideNotification, object: nil)
    }

    func placement(_ o: RowEditor.Open) -> EditorPlacement? {
        let c = canvasView.canvas
        guard let nid = c.nid(of: o.edit.face), let row = c.rowRect(face: nid, row: o.edit.row) else { return nil }
        return EditorPlacement.make(row: row, kind: o.kind, viewport: c.viewport, viewSize: c.viewSize, touch: true)
    }

    // MARK: The keyboard's accessory bar

    func bar() -> UIToolbar {
        let t = UIToolbar(frame: CGRect(x: 0, y: 0, width: 320, height: 44))
        let prev = UIBarButtonItem(image: UIImage(systemName: "chevron.up"), style: .plain, target: self, action: #selector(previous))
        prev.accessibilityLabel = "Previous"
        let next = UIBarButtonItem(image: UIImage(systemName: "chevron.down"), style: .plain, target: self, action: #selector(nextRow))
        next.accessibilityLabel = "Next"
        let done = UIBarButtonItem(barButtonSystemItem: .done, target: self, action: #selector(done))
        done.accessibilityIdentifier = "row-editor-done"
        t.items = [prev, next, UIBarButtonItem(systemItem: .flexibleSpace), done]
        t.sizeToFit()
        return t
    }

    func plain(_ t: UITextField) {
        t.autocorrectionType = .no
        t.autocapitalizationType = UITextAutocapitalizationType.none
        t.spellCheckingType = .no
        t.smartQuotesType = .no
        t.smartDashesType = .no
        t.smartInsertDeleteType = .no
    }

    func plain(_ t: UITextView) {
        t.autocorrectionType = .no
        t.autocapitalizationType = UITextAutocapitalizationType.none
        t.spellCheckingType = .no
        t.smartQuotesType = .no
        t.smartDashesType = .no
        t.smartInsertDeleteType = .no
    }

    // MARK: Showing and following

    func show(_ o: RowEditor.Open) {
        remove()
        guard let place = placement(o) else { return }
        let w = o.widget
        let made: UIView
        switch o.kind {
        case .multiline?:
            let t = KeyedTextView()
            t.font = UIFont.monospacedSystemFont(ofSize: place.fontSize, weight: .regular)
            t.text = w.value.string ?? (w.value.isNull ? "" : w.value.text)
            plain(t)
            t.delegate = self
            t.inputAccessoryView = bar()
            t.layer.borderWidth = 1
            t.layer.borderColor = UIColor.tintColor.cgColor
            t.layer.cornerRadius = 4
            t.host = self
            made = t
        case .toggle?:
            let s = UISwitch()
            s.isOn = w.value.bool ?? w.default.bool ?? false
            s.addTarget(self, action: #selector(switched(_:)), for: .valueChanged)
            made = s
        case .color?:
            let b = UIButton(type: .system)
            b.backgroundColor = FaceLayout.widgetColor(w).map { UIColor(cgColor: $0) } ?? .systemGray
            b.layer.cornerRadius = 4
            b.addTarget(self, action: #selector(pickColor), for: .touchUpInside)
            made = b
        case .choice?:
            let b = UIButton(type: .system)
            b.showsMenuAsPrimaryAction = true
            b.menu = choiceMenu(node: o.edit.node, port: o.edit.port, w)
            b.setTitle(FaceLayout.widgetText(w), for: .normal)
            b.titleLabel?.font = .systemFont(ofSize: place.fontSize)
            b.backgroundColor = .secondarySystemBackground
            b.layer.cornerRadius = 4
            made = b
        default:
            let f = KeyedTextField()
            f.text = Self.fieldText(w)
            f.font = o.kind == .number ? .monospacedDigitSystemFont(ofSize: place.fontSize, weight: .regular) : .systemFont(ofSize: place.fontSize)
            f.textAlignment = o.kind == .number ? .right : .natural
            f.borderStyle = .roundedRect
            f.keyboardType = o.kind == .number ? .numbersAndPunctuation : (o.kind == .path ? .URL : .default)
            f.returnKeyType = .done
            f.clearButtonMode = .whileEditing
            f.placeholder = w.blank ? "blank" : nil
            plain(f)
            f.delegate = self
            f.inputAccessoryView = bar()
            f.host = self
            f.accessibilityLabel = o.label
            made = f
        }
        made.frame = place.frame
        if place.floating {
            made.layer.shadowOpacity = 0.3
            made.layer.shadowRadius = 8
            made.layer.shadowOffset = CGSize(width: 0, height: 2)
        }
        made.accessibilityIdentifier = "row-editor"
        canvasView.addSubview(made)
        control = made
        finishing = false
        if made is UITextField || made is UITextView {
            made.becomeFirstResponder()
            if let f = made as? UITextField { f.selectAll(nil) }
        }
        keepAboveKeyboard()
        onShown?()
    }

    public static func fieldText(_ w: Widget) -> String {
        if w.blank || w.value.isNull { return "" }
        if w.kind.word == .number, let d = w.value.double {
            if w.int { return String(Int64(d.rounded())) }
            return w.value.text
        }
        return w.value.string ?? w.value.text
    }

    /// The canvas moved or zoomed: the control follows its row.
    public func follow() {
        guard let o = editor.open, let c = control, let place = placement(o) else { return }
        c.frame = place.frame
        if let f = c as? UITextField, let font = f.font { f.font = font.withSize(place.fontSize) }
        if let t = c as? UITextView, let font = t.font { t.font = font.withSize(place.fontSize) }
    }

    func remove() {
        guard let c = control else { return }
        control = nil
        finishing = true
        c.resignFirstResponder()
        c.removeFromSuperview()
        onShown?()
    }

    // MARK: The keyboard

    @objc func keyboard(_ n: Notification) {
        guard let end = n.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect, let win = canvasView.window else { return }
        let inView = canvasView.convert(win.convert(end, from: nil), from: nil)
        keyboardTop = inView.minY < canvasView.bounds.maxY ? inView.minY : nil
        keepAboveKeyboard()
        onShown?()
    }

    @objc func keyboardHidden(_ n: Notification) { keyboardTop = nil }

    /// Scroll the canvas so the row being edited is above the keyboard.
    func keepAboveKeyboard() {
        guard let top = keyboardTop, let c = control else { return }
        let lift = EditorPlacement.lift(c.frame, keyboardTop: top)
        guard lift != 0 else { return }
        let v = canvasView.canvas.viewport
        canvasView.canvas.setViewport(Viewport(pan: CGPoint(x: v.pan.x, y: v.pan.y + lift), zoom: v.zoom), animated: true)
    }

    /// Where the keyboard's top is in the view (nil: none on screen), for tests and status.
    public var keyboardTopInView: CGFloat? { keyboardTop }

    // MARK: Commits

    func text() -> String? {
        if let f = control as? UITextField { return f.text ?? "" }
        if let t = control as? UITextView { return t.text ?? "" }
        return nil
    }

    func commit(then: RowEditor.Then) {
        guard !finishing, let t = text() else { return }
        finishing = true
        let editor = self.editor
        Task { [weak self] in
            let ok = await editor.commit(text: t, then: then)
            if !ok { self?.finishing = false }
        }
    }

    @objc func done() { commit(then: .close) }
    @objc func nextRow() { commit(then: .next) }
    @objc func previous() { commit(then: .previous) }
    func escape() {
        finishing = true
        Task { await editor.close() }
    }

    /// What a person would type, then Done (tests and scripts).
    public func type(_ s: String) {
        if let f = control as? UITextField { f.text = s } else if let t = control as? UITextView { t.text = s }
    }
    public func pressDone() { done() }
    public func commitField() { pressDone() }
    public func pressTab(back: Bool = false) { back ? previous() : nextRow() }
    public func pressEscape() { escape() }

    @objc func switched(_ s: UISwitch) {
        finishing = true
        Task { await editor.commit(value: .bool(s.isOn)) }
    }

    @objc func pickColor() {
        guard let o = editor.open, let c = control else { return }
        let p = UIColorPickerViewController()
        p.supportsAlpha = false
        if let cg = FaceLayout.widgetColor(o.widget) { p.selectedColor = UIColor(cgColor: cg) }
        p.delegate = self
        present?(p, c.frame)
    }

    public func colorPickerViewControllerDidFinish(_ vc: UIColorPickerViewController) {
        commitColor(vc.selectedColor.cgColor)
    }

    /// Commit a colour as the picker closing does (tests and scripts).
    public func commitColor(_ cg: CGColor) {
        let srgb = cg.converted(to: CGColorSpace(name: CGColorSpace.sRGB) ?? cg.colorSpace ?? CGColorSpaceCreateDeviceRGB(), intent: .defaultIntent, options: nil) ?? cg
        let c = srgb.components ?? [0, 0, 0]
        let rgb = c.count >= 3 ? [c[0], c[1], c[2]] : [c[0], c[0], c[0]]
        finishing = true
        Task { await editor.commit(value: .array(rgb.map { .double(Double($0)) })) }
    }

    // MARK: UITextFieldDelegate, UITextViewDelegate

    public func textFieldShouldReturn(_ f: UITextField) -> Bool {
        done()
        return false
    }

    public func textFieldDidEndEditing(_ f: UITextField, reason: UITextField.DidEndEditingReason) {
        commit(then: .close)
    }

    public func textViewDidEndEditing(_ t: UITextView) {
        commit(then: .close)
    }

    // MARK: The system's menus on the face (a choice's row, the more-button)

    /// The core's choices as a menu: the one set ticked, a disabled one greyed with its reason; a
    /// pick is `row.set` (one undo step).
    public func choiceMenu(node: String, port: String, _ w: Widget) -> UIMenu {
        let model = editor.model
        let actions: [UIMenuElement] = w.choices.map { c in
            let a = UIAction(title: c.label, subtitle: c.disabled.map { "Not available here: \($0)" }) { _ in
                Task { _ = try? await model.run("row.set", ["node": .string(node), "port": .string(port), "value": c.value]) }
            }
            a.state = c.value.same(w.value) ? .on : .off
            if c.disabled != nil { a.attributes = .disabled }
            return a
        }
        return UIMenu(options: .singleSelection, children: actions)
    }

    /// Where a touch at `p` (view points) goes when it is on a choice's row or the more-button:
    /// the menu button, placed over it with that menu. Nil otherwise.
    func routeTouch(at p: CGPoint, hit h: HotHit) -> UIView? {
        let c = canvasView.canvas
        if h.nodePart == .menu, let r = c.moreRect(face: h.node) {
            menuButton.frame = viewRect(r).insetBy(dx: -6, dy: -6)
            let g = canvasView.graph(p)
            let link = c.link
            let model = editor.model
            let make = canvasView.contextMenuItems
            menuButton.menu = UIMenu(children: [UIDeferredMenuElement.uncached { done in
                Task { @MainActor in
                    var items: [MenuItem] = []
                    if let r = try? await link.tap(g, slop: 4), let ctx = r.context, ctx != .null {
                        items = (try? await model.contextMenu(ctx)) ?? []
                    }
                    let els = make?(items) ?? []
                    done(els.isEmpty ? [UIAction(title: "Nothing here", attributes: .disabled) { _ in }] : els)
                }
            }])
            menuButton.accessibilityLabel = "More"
            menuButton.isHidden = false
            return menuButton
        }
        let index = Int(h.index)
        if h.nodePart == .row, let f = editor.model.faces[h.node]?.item, index >= 0, index < f.inputs.count,
           let w = f.inputs[index].widget, w.kind.word == .choice, f.inputs[index].source.word == .own,
           let r = c.rowRect(face: h.node, row: index) {
            menuButton.frame = viewRect(r)
            menuButton.menu = choiceMenu(node: f.inputs[index].node, port: f.inputs[index].key, w)
            menuButton.accessibilityLabel = f.inputs[index].label
            menuButton.isHidden = false
            return menuButton
        }
        return nil
    }

    func viewRect(_ r: CGRect) -> CGRect {
        let v = canvasView.canvas.viewport
        return CGRect(x: r.minX * v.zoom + v.pan.x, y: r.minY * v.zoom + v.pan.y, width: r.width * v.zoom, height: r.height * v.zoom)
    }
}

/// A text field that takes a hardware keyboard's Escape, Tab and Shift-Tab before the system.
final class KeyedTextField: UITextField {
    weak var host: FieldEditorHost?
    override var keyCommands: [UIKeyCommand]? {
        let cmds = [
            UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(esc)),
            UIKeyCommand(input: "\t", modifierFlags: [], action: #selector(tab)),
            UIKeyCommand(input: "\t", modifierFlags: .shift, action: #selector(backTab)),
        ]
        cmds.forEach { $0.wantsPriorityOverSystemBehavior = true }
        return cmds
    }
    @objc func esc() { host?.pressEscape() }
    @objc func tab() { host?.pressTab() }
    @objc func backTab() { host?.pressTab(back: true) }
}

/// A text view that commits on ⌘Return, and takes Escape and Tab as the field does.
final class KeyedTextView: UITextView {
    weak var host: FieldEditorHost?
    override var keyCommands: [UIKeyCommand]? {
        let cmds = [
            UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(esc)),
            UIKeyCommand(input: "\r", modifierFlags: .command, action: #selector(commit)),
            UIKeyCommand(input: "\t", modifierFlags: [], action: #selector(tab)),
            UIKeyCommand(input: "\t", modifierFlags: .shift, action: #selector(backTab)),
        ]
        cmds.forEach { $0.wantsPriorityOverSystemBehavior = true }
        return cmds
    }
    @objc func esc() { host?.pressEscape() }
    @objc func commit() { host?.pressDone() }
    @objc func tab() { host?.pressTab() }
    @objc func backTab() { host?.pressTab(back: true) }
}
#endif
