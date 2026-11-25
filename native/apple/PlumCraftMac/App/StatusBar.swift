import AppKit
import PlumCraftKit
import PlumCraftModels

/// The status line at the bottom of a document window (research note 28): the core's segments
/// (`DocumentState.statusLine`), drawn and never composed here. A click runs a segment's command.
/// Each segment is shown whole or not at all (the kit's `StatusLine.fit`): a narrow window drops
/// the details, then priority 3 and 2, then takes the short forms; the newest message keeps its
/// place. View › Status Line hides it (the core's `on`).
@MainActor
final class StatusBar: NSVisualEffectView {
    /// Runs a segment's command (the core's id and params).
    var onCommand: ((StatusCommand) -> Void)?
    /// The width left out segments, or took them back (for the window's status).
    var onFit: (() -> Void)?
    private let left = NSStackView()
    private let right = NSStackView()
    private let rule = NSBox()
    private var line = StatusLine(on: false)
    /// What the line shows now (the fitting rule's plan).
    private var plan: [StatusPlaced] = []
    /// Each segment's width by form and words, measured once.
    private var widths: [String: Double] = [:]

    static let height: CGFloat = 26

    override init(frame: NSRect) {
        super.init(frame: frame)
        material = .titlebar
        blendingMode = .withinWindow
        state = .followsWindowActiveState
        rule.boxType = .separator
        for s in [left, right] {
            s.orientation = .horizontal
            s.spacing = 12
            s.alignment = .centerY
            s.detachesHiddenViews = true
            s.setHuggingPriority(.defaultHigh, for: .horizontal)
        }
        right.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        for v in [rule, left, right] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            addSubview(v)
        }
        NSLayoutConstraint.activate([
            rule.topAnchor.constraint(equalTo: topAnchor),
            rule.leadingAnchor.constraint(equalTo: leadingAnchor),
            rule.trailingAnchor.constraint(equalTo: trailingAnchor),
            left.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            left.centerYAnchor.constraint(equalTo: centerYAnchor, constant: 0.5),
            right.leadingAnchor.constraint(greaterThanOrEqualTo: left.trailingAnchor, constant: 16),
            right.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -10),
            right.centerYAnchor.constraint(equalTo: centerYAnchor, constant: 0.5),
        ])
        setAccessibilityRole(.group)
        setAccessibilityLabel("Status line")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    /// Draw this line (a no-op when it hasn't changed).
    func show(_ l: StatusLine) {
        guard l != line else { return }
        line = l
        plan = []
        fit()
    }

    override func layout() {
        super.layout()
        fit()
    }

    /// The segments that fit the width, whole (the kit's rule), drawn when they change.
    private func fit() {
        guard bounds.width > 0 else { return }
        // the margins, and the gap between the sides being wider than between segments
        let room = Double(bounds.width) - 20 - 4
        let next = line.fit(room: room, spacing: Double(left.spacing)) { seg, form in
            let key = "\(seg.id)|\(form.rawValue)|\(seg.text)|\(seg.short ?? "")|\(seg.detail ?? "")|\(seg.busy == true)"
            if let w = widths[key] { return w }
            let w = Double(view(for: seg, form: form).fittingSize.width)
            if widths.count > 400 { widths = [:] }
            widths[key] = w
            return w
        }
        guard next != plan else { return }
        plan = next
        for s in [left, right] { s.arrangedSubviews.forEach { $0.removeFromSuperview() } }
        for p in plan {
            (p.segment.side == "right" ? right : left).addArrangedSubview(view(for: p.segment, form: p.form))
        }
        onFit?()
    }

    // MARK: Segments

    private func color(_ t: StatusTone?, dim: Bool) -> NSColor {
        switch t {
        case .error: .systemRed
        case .warn: .systemOrange
        default: dim ? .secondaryLabelColor : .labelColor
        }
    }

    private func view(for seg: StatusSegment, form: StatusForm) -> NSView {
        let words = StatusPlaced(segment: seg, form: form).text
        let row = NSStackView()
        row.orientation = .horizontal
        row.spacing = 5
        row.alignment = .centerY
        row.identifier = NSUserInterfaceItemIdentifier("status.\(seg.id)")
        if let d = seg.dot {
            let dot = NSView()
            dot.wantsLayer = true
            dot.layer?.cornerRadius = 4
            dot.layer?.backgroundColor = (d.word == .ok ? NSColor.systemGreen : d.word == .busy ? .systemOrange : .systemRed).cgColor
            dot.translatesAutoresizingMaskIntoConstraints = false
            dot.widthAnchor.constraint(equalToConstant: 8).isActive = true
            dot.heightAnchor.constraint(equalToConstant: 8).isActive = true
            row.addArrangedSubview(dot)
        }
        if seg.busy == true {
            let spin = NSProgressIndicator()
            spin.style = .spinning
            spin.controlSize = .mini
            spin.isIndeterminate = true
            spin.startAnimation(nil)
            row.addArrangedSubview(spin)
        }
        let font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize)
        let dimRoles: Set<String> = ["hash", "selection", "run"]
        let main: NSView
        if seg.role == "toggle" {
            let b = NSButton(checkboxWithTitle: words, target: self, action: #selector(clicked(_:)))
            b.controlSize = .small
            b.font = font
            b.state = seg.on == true ? .on : .off
            main = b
        } else if seg.role == "action" {
            let b = NSButton(title: words, target: self, action: #selector(clicked(_:)))
            b.bezelStyle = .inline
            b.controlSize = .small
            b.font = font
            main = b
        } else {
            let t = NSTextField(labelWithString: words)
            t.font = font
            t.textColor = color(seg.tone?.word, dim: dimRoles.contains(seg.role) && seg.busy != true)
            t.lineBreakMode = .byTruncatingTail
            // only the newest message alone on a line too narrow for it is cut
            if form == .cut { t.setContentCompressionResistancePriority(.defaultLow - 1, for: .horizontal) }
            main = t
        }
        row.addArrangedSubview(main)
        if form == .full, let d = seg.detail, !d.isEmpty {
            let t = NSTextField(labelWithString: d)
            t.font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize - 1)
            t.textColor = color(seg.detailTone?.word, dim: true)
            t.lineBreakMode = .byTruncatingTail
            // an engine's own status text, long, is cut at 240 points (whole on hover), as egui does
            if seg.role == "engine" { t.widthAnchor.constraint(lessThanOrEqualToConstant: 240).isActive = true }
            row.addArrangedSubview(t)
        }
        // a click anywhere on a segment with a command runs it (Cancel is a button of its own)
        if let c = seg.command, seg.role != "toggle", seg.role != "action" {
            if seg.role == "run", c.id == "run.cancel" {
                let b = NSButton(title: c.label ?? "Cancel", target: self, action: #selector(clicked(_:)))
                b.bezelStyle = .inline
                b.controlSize = .mini
                b.identifier = NSUserInterfaceItemIdentifier(seg.id)
                row.addArrangedSubview(b)
            } else {
                row.addGestureRecognizer(NSClickGestureRecognizer(target: self, action: #selector(rowClicked(_:))))
            }
        }
        (main as? NSControl)?.identifier = NSUserInterfaceItemIdentifier(seg.id)
        row.toolTip = seg.tooltip
        for v in row.arrangedSubviews { v.toolTip = seg.tooltip }
        row.setAccessibilityElement(true)
        row.setAccessibilityLabel([seg.text, seg.detail].compactMap { $0 }.joined(separator: ", "))
        return row
    }

    private func segment(_ id: String?) -> StatusSegment? {
        line.segments.first { "status.\($0.id)" == id || $0.id == id }
    }

    @objc private func clicked(_ sender: NSControl) {
        if let c = segment(sender.identifier?.rawValue)?.command { onCommand?(c) }
    }

    @objc private func rowClicked(_ g: NSClickGestureRecognizer) {
        if let c = segment(g.view?.identifier?.rawValue)?.command { onCommand?(c) }
    }

    /// What the bar shows now, for `ui.inspect`'s window status.
    var report: JSONValue {
        ["visible": .bool(!isHidden), "shown": .array(plan.map { .string($0.segment.id) }),
         "forms": .array(plan.map { .string("\($0.segment.id):\($0.form.rawValue):\($0.text)") })]
    }
}
