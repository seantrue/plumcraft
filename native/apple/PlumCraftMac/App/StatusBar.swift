import AppKit
import PlumCraftKit
import PlumCraftModels

/// The status line at the bottom of a document window (research note 28): the core's segments
/// (`DocumentState.statusLine`), drawn and never composed here. A click runs a segment's command;
/// a narrow window leaves out priority 3, then 2, keeping the engine, the run and the newest
/// message. View › Status Line hides it (the core's `on`).
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
    /// The views of the line now, each with its priority, for fitting the width.
    private var placed: [(view: NSView, priority: Int)] = []
    private(set) var shownLevel = 3

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
        for s in [left, right] { s.arrangedSubviews.forEach { $0.removeFromSuperview() } }
        placed = []
        for seg in l.segments {
            let v = view(for: seg)
            (seg.side == "right" ? right : left).addArrangedSubview(v)
            placed.append((v, seg.priority))
        }
        fit()
    }

    override func layout() {
        super.layout()
        fit()
    }

    /// The most priorities that fit the width: 3, then 2, then 1 (the messages' text is cut
    /// before any is left out).
    private func fit() {
        let room = bounds.width - 20 - 16
        var level = 3
        while level > 1 {
            let views = placed.filter { $0.priority <= level }.map(\.view)
            let leftW = views.filter { $0.superview === left }.reduce(CGFloat(0)) { $0 + $1.fittingSize.width + left.spacing }
            // a message may be cut to about 160 points before a left segment goes
            let rightW = views.filter { $0.superview === right }.reduce(CGFloat(0)) { $0 + min($1.fittingSize.width, 160) + right.spacing }
            if leftW + rightW <= room { break }
            level -= 1
        }
        if level != shownLevel || placed.contains(where: { $0.view.isHidden != ($0.priority > level) }) {
            shownLevel = level
            for (v, p) in placed { v.isHidden = p > level }
            onFit?()
        }
    }

    // MARK: Segments

    private func color(_ t: StatusTone?, dim: Bool) -> NSColor {
        switch t {
        case .error: .systemRed
        case .warn: .systemOrange
        default: dim ? .secondaryLabelColor : .labelColor
        }
    }

    private func view(for seg: StatusSegment) -> NSView {
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
            let b = NSButton(checkboxWithTitle: seg.text, target: self, action: #selector(clicked(_:)))
            b.controlSize = .small
            b.font = font
            b.state = seg.on == true ? .on : .off
            main = b
        } else if seg.role == "action" {
            let b = NSButton(title: seg.text, target: self, action: #selector(clicked(_:)))
            b.bezelStyle = .inline
            b.controlSize = .small
            b.font = font
            main = b
        } else {
            let t = NSTextField(labelWithString: seg.text)
            t.font = font
            t.textColor = color(seg.tone?.word, dim: dimRoles.contains(seg.role) && seg.busy != true)
            t.lineBreakMode = .byTruncatingTail
            if seg.role == "message" {
                t.setContentCompressionResistancePriority(.defaultLow - 1, for: .horizontal)
                t.widthAnchor.constraint(lessThanOrEqualToConstant: 420).isActive = true
            }
            main = t
        }
        row.addArrangedSubview(main)
        if let d = seg.detail, !d.isEmpty {
            let t = NSTextField(labelWithString: d)
            t.font = NSFont.systemFont(ofSize: NSFont.smallSystemFontSize - 1)
            t.textColor = color(seg.detailTone?.word, dim: true)
            t.lineBreakMode = .byTruncatingTail
            // a detail gives way before the message
            t.setContentCompressionResistancePriority(.defaultLow - 2, for: .horizontal)
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
        let shown = placed.enumerated().filter { !$0.element.view.isHidden }.compactMap { line.segments.indices.contains($0.offset) ? line.segments[$0.offset].id : nil }
        return ["visible": .bool(!isHidden), "level": .int(Int64(shownLevel)), "shown": .array(shown.map(JSONValue.string))]
    }
}
