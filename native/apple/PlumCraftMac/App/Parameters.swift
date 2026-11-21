// The parameters inspector (N4 slice 3): every input of one node, shown on its face or not, in a
// popover attached to the node (as the Mac's inspectors over a canvas are), from the core's state
// (`node.parameters` opens it, the scene's `view.windows.parameters` says which node,
// `parameters.get` gives the rows). Each row: its label with the core's help as a tooltip, a full
// native control for its kind (a slider and a field for a number with a range, a switch, a pop-up
// for a choice, a field for text, a text view for several lines, a colour well), Show on Face
// (`node.face`) and ↺ (`row.set {reset}`) when it isn't the default; a wired or inherited input
// says where its value comes from. Every value goes to the core (`row.set`), which checks it, as
// the face's editor does. Opened by a double click on a node, the node's menu (⋮ or a secondary
// click), Node › Edit Parameters… (⌘I) or a script; Escape or a click away closes it.

import AppKit
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

@MainActor
final class ParametersPopover: NSObject, NSPopoverDelegate {
    let params: ParametersModel
    unowned let canvasView: CanvasView
    let popover = NSPopover()
    let content: ParametersViewController
    private var shownNode: String?

    init(model: DocumentModel, canvasView: CanvasView) {
        params = ParametersModel(model: model)
        self.canvasView = canvasView
        content = ParametersViewController(params: params)
        super.init()
        popover.contentViewController = content
        popover.behavior = .semitransient
        popover.animates = false
        popover.delegate = self
        params.onChange = { [weak self] r in self?.changed(r) }
        params.onNote = { [weak self] port, text, error in self?.content.note(port: port, text, error: error) }
    }

    private func changed(_ r: ParametersReply?) {
        guard let r else {
            shownNode = nil
            if popover.isShown { popover.performClose(nil) }
            return
        }
        content.show(r)
        if !popover.isShown || shownNode != r.node {
            shownNode = r.node
            present(node: r.node)
        }
    }

    /// Where the node's face is on screen, in the canvas view's points.
    func anchor(_ node: String) -> NSRect {
        let c = canvasView.canvas
        guard let nid = c.nid(of: node), let f = c.model.faces[nid]?.item else {
            return NSRect(x: canvasView.bounds.midX, y: canvasView.bounds.midY, width: 1, height: 1)
        }
        let size = c.faceLayout(nid: nid)?.size ?? CGSize(width: 220, height: 160)
        let a = c.viewport.toView(CGPoint(x: f.pos.x, y: f.pos.y))
        let b = c.viewport.toView(CGPoint(x: f.pos.x + size.width, y: f.pos.y + size.height))
        return NSRect(x: a.x, y: a.y, width: max(b.x - a.x, 1), height: max(b.y - a.y, 1)).intersection(canvasView.bounds.insetBy(dx: 2, dy: 2))
    }

    private func present(node: String) {
        guard canvasView.window != nil else { return }
        var r = anchor(node)
        if r.isNull || r.isEmpty { r = NSRect(x: canvasView.bounds.midX, y: canvasView.bounds.midY, width: 1, height: 1) }
        if popover.isShown {
            popover.positioningRect = r
        } else {
            popover.show(relativeTo: r, of: canvasView, preferredEdge: .maxX)
        }
    }

    /// The canvas moved: the popover stays on its node.
    func follow() {
        guard popover.isShown, let n = shownNode else { return }
        let r = anchor(n)
        if !r.isNull && !r.isEmpty { popover.positioningRect = r }
    }

    func popoverDidClose(_ n: Notification) {
        // the person closed it (Escape, a click away): the core's window goes too
        if params.node != nil {
            let p = params
            Task { await p.close() }
        }
        shownNode = nil
    }

    var describe: JSONValue {
        guard let r = params.reply else { return .null }
        return ["node": .string(r.node), "shown": .bool(popover.isShown), "rows": .uint(UInt64(r.count)),
                "hidden": .array(r.hidden.map { .string($0) }), "window": .int(Int64(popover.contentViewController?.view.window?.windowNumber ?? 0))]
    }
}

@MainActor
final class ParametersViewController: NSViewController, NSTextFieldDelegate {
    let params: ParametersModel
    private let titleLabel = NSTextField(labelWithString: "")
    private let kindLabel = NSTextField(labelWithString: "")
    private let aboutLabel = NSTextField(wrappingLabelWithString: "")
    private let stack = NSStackView()
    private let noteLabel = NSTextField(wrappingLabelWithString: "")
    private var reply: ParametersReply?
    /// Controls by port, for scripts and tests.
    private(set) var fields: [String: NSView] = [:]
    private var targets: [ClosureTarget] = []
    private var editing = false

    init(params: ParametersModel) {
        self.params = params
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    override func loadView() {
        let v = NSView(frame: NSRect(x: 0, y: 0, width: 440, height: 520))
        titleLabel.font = .systemFont(ofSize: 17, weight: .semibold)
        titleLabel.lineBreakMode = .byTruncatingTail
        kindLabel.textColor = .secondaryLabelColor
        kindLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        aboutLabel.textColor = .secondaryLabelColor
        aboutLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        aboutLabel.maximumNumberOfLines = 3
        noteLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        noteLabel.isHidden = true
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 6
        stack.edgeInsets = NSEdgeInsets(top: 4, left: 0, bottom: 12, right: 0)
        let scroll = NSScrollView()
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        let doc = FlippedView()
        doc.translatesAutoresizingMaskIntoConstraints = false
        stack.translatesAutoresizingMaskIntoConstraints = false
        doc.addSubview(stack)
        scroll.documentView = doc
        for x in [titleLabel, kindLabel, aboutLabel, noteLabel, scroll] {
            x.translatesAutoresizingMaskIntoConstraints = false
            v.addSubview(x)
        }
        NSLayoutConstraint.activate([
            titleLabel.topAnchor.constraint(equalTo: v.topAnchor, constant: 14),
            titleLabel.leadingAnchor.constraint(equalTo: v.leadingAnchor, constant: 16),
            titleLabel.trailingAnchor.constraint(equalTo: v.trailingAnchor, constant: -16),
            kindLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 2),
            kindLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            kindLabel.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            aboutLabel.topAnchor.constraint(equalTo: kindLabel.bottomAnchor, constant: 4),
            aboutLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            aboutLabel.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            noteLabel.topAnchor.constraint(equalTo: aboutLabel.bottomAnchor, constant: 4),
            noteLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            noteLabel.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            scroll.topAnchor.constraint(equalTo: noteLabel.bottomAnchor, constant: 6),
            scroll.leadingAnchor.constraint(equalTo: v.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: v.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: v.bottomAnchor),
            doc.leadingAnchor.constraint(equalTo: scroll.contentView.leadingAnchor),
            doc.trailingAnchor.constraint(equalTo: scroll.contentView.trailingAnchor),
            doc.topAnchor.constraint(equalTo: scroll.contentView.topAnchor),
            stack.leadingAnchor.constraint(equalTo: doc.leadingAnchor, constant: 16),
            stack.trailingAnchor.constraint(equalTo: doc.trailingAnchor, constant: -16),
            stack.topAnchor.constraint(equalTo: doc.topAnchor),
            stack.bottomAnchor.constraint(equalTo: doc.bottomAnchor),
            v.widthAnchor.constraint(equalToConstant: 440),
            v.heightAnchor.constraint(equalToConstant: 520),
        ])
        view = v
    }

    func show(_ r: ParametersReply) {
        _ = view
        // a field being typed into isn't pulled out from under the person: the rows come again
        // once it commits
        if editing, reply?.node == r.node {
            reply = r
            return
        }
        reply = r
        titleLabel.stringValue = r.title
        kindLabel.stringValue = "\(r.display) · \(r.node)"
        aboutLabel.stringValue = r.description ?? ""
        aboutLabel.toolTip = r.about
        aboutLabel.isHidden = (r.description ?? "").isEmpty
        for v in stack.arrangedSubviews { v.removeFromSuperview() }
        fields = [:]
        targets = []
        let single = r.sections.count == 1
        for s in r.sections {
            if !single {
                let h = NSTextField(labelWithString: "\(s.title.uppercased())  \(s.rows.count)")
                h.font = .systemFont(ofSize: NSFont.smallSystemFontSize, weight: .semibold)
                h.textColor = .secondaryLabelColor
                stack.addArrangedSubview(h)
                stack.setCustomSpacing(4, after: h)
            }
            for row in s.rows { stack.addArrangedSubview(line(row)) }
        }
    }

    /// The core's words for a value set: a note, or why it was refused.
    func note(port: String, _ text: String, error: Bool) {
        noteLabel.stringValue = text
        noteLabel.textColor = error ? .systemRed : .secondaryLabelColor
        noteLabel.isHidden = false
    }

    private func target(_ f: @escaping () -> Void) -> ClosureTarget {
        let t = ClosureTarget(f)
        targets.append(t)
        return t
    }

    /// One input: label, its control (or where its value comes from), Show on Face, reset.
    private func line(_ p: ParametersReply.Row) -> NSView {
        let r = p.row
        let label = NSTextField(labelWithString: r.label)
        label.lineBreakMode = .byTruncatingTail
        label.toolTip = p.help
        label.textColor = r.inactive == nil ? .labelColor : .tertiaryLabelColor
        label.widthAnchor.constraint(equalToConstant: 120).isActive = true
        let control = p.editable ? editor(p) : source(p)
        control.setContentHuggingPriority(.defaultLow, for: .horizontal)
        let face = NSButton(image: NSImage(systemSymbolName: p.onFace ? "eye" : "eye.slash", accessibilityDescription: p.onFace ? "Shown on the face" : "Hidden from the face") ?? NSImage(),
                            target: nil, action: nil)
        face.bezelStyle = .accessoryBarAction
        face.isBordered = false
        face.toolTip = p.canHide ? (p.onFace ? "Shown on the node's face: click to hide it there" : "Hidden from the node's face: click to show it") : "A wired input always shows on the face"
        face.isEnabled = p.canHide
        face.setAccessibilityIdentifier("face.\(r.key)")
        let params = self.params
        face.target = target { Task { await params.showOnFace(port: r.key, !p.onFace) } }
        face.action = #selector(ClosureTarget.fire)
        let reset = NSButton(title: "↺", target: nil, action: nil)
        reset.bezelStyle = .accessoryBarAction
        reset.isBordered = false
        reset.toolTip = "Reset to the default: \(p.default.map { $0.string ?? $0.text } ?? "blank")"
        reset.isHidden = !p.reset
        reset.setAccessibilityIdentifier("reset.\(r.key)")
        reset.target = target { Task { await params.reset(port: r.key) } }
        reset.action = #selector(ClosureTarget.fire)
        let h = NSStackView(views: [label, control, reset, face])
        h.orientation = .horizontal
        h.alignment = .centerY
        h.spacing = 8
        h.widthAnchor.constraint(equalToConstant: 408).isActive = true
        h.setAccessibilityIdentifier("param.\(r.key)")
        return h
    }

    /// Where a value that isn't the node's own comes from: a wire, a bundle, a socket.
    private func source(_ p: ParametersReply.Row) -> NSView {
        let r = p.row
        let text = r.text ?? (r.widget.map { $0.value.string ?? $0.value.text } ?? "")
        let l = NSTextField(labelWithString: text + (r.from.map { "  \($0)" } ?? ""))
        l.textColor = .secondaryLabelColor
        l.lineBreakMode = .byTruncatingTail
        l.toolTip = r.hover
        return l
    }

    /// A full native control for a row the node owns.
    private func editor(_ p: ParametersReply.Row) -> NSView {
        let r = p.row
        guard let w = r.widget else { return source(p) }
        let params = self.params
        let key = r.key
        let made: NSView
        switch w.kind.word {
        case .toggle?:
            let s = NSSwitch()
            s.state = (w.value.bool ?? w.default.bool ?? false) ? .on : .off
            s.target = target { [weak s] in Task { await params.set(port: key, value: .bool(s?.state == .on)) } }
            s.action = #selector(ClosureTarget.fire)
            made = s
        case .choice?:
            let b = NSPopUpButton(frame: .zero, pullsDown: false)
            b.autoenablesItems = false
            for c in w.choices {
                b.addItem(withTitle: c.label)
                if let item = b.lastItem {
                    item.isEnabled = c.disabled == nil
                    item.toolTip = c.disabled.map { "Not available here: \($0)" }
                    if c.value.same(w.value) { b.select(item) }
                }
            }
            b.target = target { [weak b] in
                guard let b, b.indexOfSelectedItem >= 0, b.indexOfSelectedItem < w.choices.count else { return }
                let v = w.choices[b.indexOfSelectedItem].value
                Task { await params.set(port: key, value: v) }
            }
            b.action = #selector(ClosureTarget.fire)
            made = b
        case .color?:
            let well = NSColorWell(style: .minimal)
            if let cg = FaceLayout.widgetColor(w), let ns = NSColor(cgColor: cg) { well.color = ns }
            well.target = target { [weak well] in
                guard let c = well?.color.usingColorSpace(.sRGB) else { return }
                let v: JSONValue = .array([.double(Double(c.redComponent)), .double(Double(c.greenComponent)), .double(Double(c.blueComponent))])
                Task { await params.set(port: key, value: v) }
            }
            well.action = #selector(ClosureTarget.fire)
            made = well
        case .number? where w.min != nil && w.max != nil:
            // a slider over the span and the field held to the range, as egui's sheet
            let lo = w.softMin ?? w.min ?? 0
            let hi = w.softMax ?? w.max ?? 1
            let s = NSSlider(value: w.value.double ?? lo, minValue: lo, maxValue: max(hi, lo + 1e-9), target: nil, action: nil)
            s.isContinuous = false
            if w.int { s.numberOfTickMarks = Int(min(max(hi - lo + 1, 2), 21)); s.allowsTickMarkValuesOnly = hi - lo <= 20 }
            let f = textField(p, w)
            f.widthAnchor.constraint(equalToConstant: 64).isActive = true
            s.target = target { [weak s, weak f] in
                guard let s else { return }
                let v: JSONValue = w.int ? .int(Int64(s.doubleValue.rounded())) : .double(s.doubleValue)
                f?.stringValue = w.int ? String(Int64(s.doubleValue.rounded())) : String(format: "%.\(w.decimals ?? 3)g", s.doubleValue)
                Task { await params.set(port: key, value: v) }
            }
            s.action = #selector(ClosureTarget.fire)
            s.setAccessibilityIdentifier("slider.\(key)")
            let h = NSStackView(views: [s, f])
            h.orientation = .horizontal
            h.spacing = 6
            fields[key] = f
            return h
        case .multiline?:
            let s = NSScrollView()
            s.hasVerticalScroller = true
            s.borderType = .bezelBorder
            let t = NSTextView()
            t.isRichText = false
            t.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
            t.string = w.value.string ?? ""
            t.isAutomaticQuoteSubstitutionEnabled = false
            t.isAutomaticDashSubstitutionEnabled = false
            t.isContinuousSpellCheckingEnabled = false
            t.autoresizingMask = [.width]
            s.documentView = t
            s.heightAnchor.constraint(equalToConstant: 70).isActive = true
            let commit = NotificationCenter.default
            let tv = t
            commit.addObserver(forName: NSText.didEndEditingNotification, object: t, queue: .main) { _ in
                MainActor.assumeIsolated {
                    let text = tv.string
                    Task { await params.set(port: key, text: text) }
                }
            }
            made = s
        default:
            made = textField(p, w)
        }
        made.setAccessibilityIdentifier("edit.\(key)")
        fields[key] = made
        return made
    }

    private func textField(_ p: ParametersReply.Row, _ w: Widget) -> NSTextField {
        let f = NSTextField(string: FieldEditorHost.fieldText(w))
        f.placeholderString = w.blank ? "blank" : nil
        f.usesSingleLineMode = true
        f.cell?.isScrollable = true
        f.lineBreakMode = .byClipping
        if w.kind.word == .number {
            f.alignment = .right
            f.font = .monospacedDigitSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        }
        f.delegate = self
        f.identifier = NSUserInterfaceItemIdentifier(p.row.key)
        f.setAccessibilityLabel(p.row.label)
        return f
    }

    // MARK: NSTextFieldDelegate

    func controlTextDidBeginEditing(_ n: Notification) {
        editing = true
        if let t = n.userInfo?["NSFieldEditor"] as? NSTextView {
            t.isAutomaticQuoteSubstitutionEnabled = false
            t.isAutomaticDashSubstitutionEnabled = false
            t.isAutomaticSpellingCorrectionEnabled = false
        }
    }

    func controlTextDidEndEditing(_ n: Notification) {
        editing = false
        guard let f = n.object as? NSTextField, let key = f.identifier?.rawValue else { return }
        let text = f.stringValue
        let params = self.params
        Task { await params.set(port: key, text: text) }
    }

    /// Type into a row's field and commit it, as a person would (scripts and the screenshots).
    func type(_ key: String, _ text: String) {
        guard let f = fields[key] as? NSTextField else { return }
        f.stringValue = text
        let params = self.params
        Task { await params.set(port: key, text: text) }
    }
}

/// A flipped view, so a scroll view's stack starts at the top.
final class FlippedView: NSView {
    override var isFlipped: Bool { true }
}
