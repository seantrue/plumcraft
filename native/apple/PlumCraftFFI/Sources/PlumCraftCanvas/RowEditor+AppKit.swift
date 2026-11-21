// The Mac's field editor for a face's row (N4 slice 2): one AppKit control at a time, made when
// the core opens an editor (`view.edit`) and taken away when it closes, placed over the row's
// widget at the canvas's zoom (EditorPlacement). The canvas view is layer-hosting (its layer is
// the canvas's), which takes no subviews, so the control goes in its superview, over it.
//
//   text, path, several choices, a number   NSTextField: Return commits, Escape cancels, Tab and
//                                            Shift-Tab commit and go to the next or previous row,
//                                            losing focus commits; no spelling, quote or dash
//                                            substitution (values are often code)
//   several lines                            NSTextView in a scroll view: Return is a new line,
//                                            ⌘Return commits; Escape, Tab as above
//   a choice                                 NSPopUpButton with the core's choices (a disabled one
//                                            greyed with its reason): its menu opens at once for a
//                                            person's click; opened by Tab or a script, it waits for
//                                            a click or Space, so no menu opens on its own
//   a colour                                 NSColorWell: the colour panel opens for a person's
//                                            click; the value is committed when the panel closes
//   a switch                                 a tap flips it in the core; reached by Tab, a checkbox
//
// Every value goes to the core (`row.set`), which checks it; a refusal keeps the field open with
// the core's words by the row.

#if os(macOS)
import AppKit
import PlumCraftKit
import PlumCraftModels

@MainActor
public final class FieldEditorHost: NSObject, FieldEditing, NSTextFieldDelegate, NSTextViewDelegate {
    public unowned let canvasView: CanvasView
    public let editor: RowEditor
    /// The control over the row while an editor is open.
    public private(set) var control: NSView?
    var scroll: NSScrollView?
    var finishing = false
    /// A person clicked a row a moment ago: a pop-up's menu or the colour panel may open at once.
    var clickedAt: Date?
    var colorObserver: NSObjectProtocol?

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
    }

    /// The person's mouse went down on a row (the core will say whether it opens an editor).
    func noteClick() { clickedAt = Date() }
    var byPerson: Bool { clickedAt.map { Date().timeIntervalSince($0) < 1.5 } ?? false }

    func placement(_ o: RowEditor.Open) -> EditorPlacement? {
        let c = canvasView.canvas
        guard let nid = c.nid(of: o.edit.face), let row = c.rowRect(face: nid, row: o.edit.row) else { return nil }
        return EditorPlacement.make(row: row, kind: o.kind, viewport: c.viewport, viewSize: c.viewSize, touch: false)
    }

    func show(_ o: RowEditor.Open) {
        remove()
        guard let place = placement(o), let sup = canvasView.superview else { return }
        let font = NSFont.systemFont(ofSize: place.fontSize)
        let mono = NSFont.monospacedSystemFont(ofSize: place.fontSize, weight: .regular)
        let w = o.widget
        var made: NSView
        switch o.kind {
        case .choice?:
            let b = NSPopUpButton(frame: .zero, pullsDown: false)
            b.font = font
            b.controlSize = place.fontSize < 12 ? .small : .regular
            b.autoenablesItems = false
            for c in w.choices {
                b.addItem(withTitle: c.label)
                if let item = b.lastItem {
                    item.isEnabled = c.disabled == nil
                    item.toolTip = c.disabled.map { "Not available here: \($0)" }
                    if c.value.same(w.value) { b.select(item) }
                }
            }
            b.target = self
            b.action = #selector(picked(_:))
            made = b
        case .color?:
            let well = NSColorWell(style: .minimal)
            if let cg = FaceLayout.widgetColor(w), let ns = NSColor(cgColor: cg) { well.color = ns }
            made = well
        case .toggle?:
            let b = NSButton(checkboxWithTitle: "", target: self, action: #selector(toggled(_:)))
            b.state = (w.value.bool ?? w.default.bool ?? false) ? .on : .off
            made = b
        case .multiline?:
            let s = NSScrollView()
            s.hasVerticalScroller = true
            s.borderType = .bezelBorder
            let t = MultilineView()
            t.isRichText = false
            t.font = mono
            t.string = w.value.string ?? (w.value.isNull ? "" : w.value.text)
            t.delegate = self
            t.onCommit = { [weak self] in self?.commitText(then: .close) }
            plain(t)
            t.autoresizingMask = [.width]
            t.isVerticallyResizable = true
            s.documentView = t
            scroll = s
            made = s
        default:
            let f = NSTextField(string: Self.fieldText(w))
            f.font = o.kind == .number ? mono : font
            f.alignment = o.kind == .number ? .right : .natural
            f.isBezeled = true
            f.bezelStyle = .squareBezel
            f.focusRingType = .exterior
            f.lineBreakMode = .byClipping
            f.usesSingleLineMode = true
            f.cell?.isScrollable = true
            f.delegate = self
            f.placeholderString = w.blank ? "blank" : nil
            f.setAccessibilityLabel(o.label)
            made = f
        }
        made.frame = sup.convert(place.frame, from: canvasView)
        // a pop-up's or a well's bezel is partly clear: the face's own drawing of the value
        // mustn't show through it
        if made is NSPopUpButton || made is NSColorWell || made is NSButton {
            made.wantsLayer = true
            made.layer?.backgroundColor = NSColor.controlBackgroundColor.cgColor
            made.layer?.cornerRadius = 4
        }
        if place.floating {
            made.wantsLayer = true
            made.shadow = NSShadow()
            made.layer?.shadowOpacity = 0.35
            made.layer?.shadowRadius = 6
        }
        made.setAccessibilityIdentifier("row-editor")
        sup.addSubview(made, positioned: .above, relativeTo: canvasView)
        control = made
        finishing = false
        if let s = scroll, let t = s.documentView as? NSTextView {
            canvasView.window?.makeFirstResponder(t)
            t.selectAll(nil)
        } else {
            canvasView.window?.makeFirstResponder(made)
        }
        // a person's click opens a pop-up's menu or the colour panel at once; Tab or a script
        // leaves them for a click (no menu or panel opens on its own)
        if byPerson {
            clickedAt = nil
            if let b = made as? NSPopUpButton {
                DispatchQueue.main.async { b.performClick(nil) }
            } else if let well = made as? NSColorWell {
                watchColorPanel()
                well.activate(true)
            }
        } else if made is NSColorWell {
            watchColorPanel()
        }
    }

    /// What a text field shows: the value as the face words it, blank as nothing.
    public static func fieldText(_ w: Widget) -> String {
        if w.blank || w.value.isNull { return "" }
        if w.kind.word == .number, let d = w.value.double {
            if w.int { return String(Int64(d.rounded())) }
            // the core's text: at the decimals the value needs, so typing over it loses nothing,
            // and a slider's float noise never shows (N4 slice 4)
            if let t = w.text { return t }
            // the whole value, not the face's rounding: typing over it loses nothing
            return w.value.text
        }
        return w.value.string ?? w.value.text
    }

    func plain(_ t: NSTextView) {
        t.isContinuousSpellCheckingEnabled = false
        t.isGrammarCheckingEnabled = false
        t.isAutomaticSpellingCorrectionEnabled = false
        t.isAutomaticQuoteSubstitutionEnabled = false
        t.isAutomaticDashSubstitutionEnabled = false
        t.isAutomaticTextReplacementEnabled = false
        t.isAutomaticLinkDetectionEnabled = false
    }

    /// The canvas moved or zoomed: the control follows its row.
    public func follow() {
        guard let o = editor.open, let c = control, let place = placement(o), let sup = canvasView.superview else { return }
        c.frame = sup.convert(place.frame, from: canvasView)
        if let f = c as? NSTextField {
            f.font = NSFont(descriptor: f.font?.fontDescriptor ?? NSFont.systemFont(ofSize: 11).fontDescriptor, size: place.fontSize)
        }
    }

    func remove() {
        if let o = colorObserver { NotificationCenter.default.removeObserver(o) }
        colorObserver = nil
        guard let c = control else { return }
        control = nil
        scroll = nil
        finishing = true
        c.removeFromSuperview()
        canvasView.window?.makeFirstResponder(canvasView)
    }

    // MARK: Commits

    func commitText(then: RowEditor.Then) {
        guard !finishing, let c = control else { return }
        let text: String
        if let f = c as? NSTextField {
            text = f.stringValue
        } else if let t = scroll?.documentView as? NSTextView {
            text = t.string
        } else {
            return
        }
        finishing = true
        let editor = self.editor
        Task { [weak self] in
            let ok = await editor.commit(text: text, then: then)
            if !ok { self?.finishing = false }
        }
    }

    /// The field's own value now (tests and scripts: what a person would have typed).
    public func type(_ text: String) {
        if let f = control as? NSTextField { f.stringValue = text } else if let t = scroll?.documentView as? NSTextView { t.string = text }
    }

    /// Commit as Return (or ⌘Return) would (tests and scripts).
    public func pressReturn() { commitText(then: .close) }
    public func commitField() { pressReturn() }
    public func pressTab(back: Bool = false) { commitText(then: back ? .previous : .next) }
    public func pressEscape() {
        finishing = true
        Task { await editor.close() }
    }

    @objc func picked(_ b: NSPopUpButton) {
        let k = b.indexOfSelectedItem
        guard let o = editor.open, k >= 0, k < o.widget.choices.count else { return }
        let v = o.widget.choices[k].value
        finishing = true
        Task { await editor.commit(value: v) }
    }

    /// Pick choice `k` as the pop-up's menu would (tests and scripts).
    public func pick(_ k: Int) {
        guard let b = control as? NSPopUpButton, k >= 0, k < b.numberOfItems else { return }
        b.selectItem(at: k)
        picked(b)
    }

    @objc func toggled(_ b: NSButton) {
        finishing = true
        Task { await editor.commit(value: .bool(b.state == .on)) }
    }

    func watchColorPanel() {
        colorObserver = NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: NSColorPanel.shared, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.commitWell() }
        }
    }

    /// The colour panel closed (or a script set the colour): commit the well's colour.
    public func commitColor(_ set: CGColor) {
        guard let well = control as? NSColorWell, let ns = NSColor(cgColor: set) else { return }
        well.color = ns
        commitWell()
    }

    func commitWell() {
        guard let well = control as? NSColorWell else { return }
        guard let c = well.color.usingColorSpace(.sRGB) else { return }
        finishing = true
        let v: JSONValue = .array([.double(Double(c.redComponent)), .double(Double(c.greenComponent)), .double(Double(c.blueComponent))])
        Task { await editor.commit(value: v) }
    }

    // MARK: NSTextFieldDelegate, NSTextViewDelegate

    public func controlTextDidBeginEditing(_ n: Notification) {
        if let t = n.userInfo?["NSFieldEditor"] as? NSTextView { plain(t) }
    }

    public func control(_ control: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        plain(textView)
        switch sel {
        case #selector(NSResponder.insertNewline(_:)): pressReturn()
        case #selector(NSResponder.cancelOperation(_:)): pressEscape()
        case #selector(NSResponder.insertTab(_:)): pressTab()
        case #selector(NSResponder.insertBacktab(_:)): pressTab(back: true)
        default: return false
        }
        return true
    }

    public func controlTextDidEndEditing(_ n: Notification) {
        // focus went elsewhere: what the field holds is committed
        commitText(then: .close)
    }

    public func textView(_ textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        switch sel {
        case #selector(NSResponder.cancelOperation(_:)): pressEscape()
        case #selector(NSResponder.insertTab(_:)): pressTab()
        case #selector(NSResponder.insertBacktab(_:)): pressTab(back: true)
        default: return false
        }
        return true
    }

    public func textDidEndEditing(_ n: Notification) {
        commitText(then: .close)
    }
}

/// A text view that commits on ⌘Return (Return is a new line).
final class MultilineView: NSTextView {
    var onCommit: (() -> Void)?
    override func keyDown(with e: NSEvent) {
        if e.keyCode == 36 || e.keyCode == 76, e.modifierFlags.contains(.command) {
            onCommit?()
            return
        }
        super.keyDown(with: e)
    }
}
#endif
