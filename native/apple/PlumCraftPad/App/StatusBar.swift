import PlumCraftKit
import PlumCraftModels
import UIKit

/// The status line along the bottom of the canvas (research note 28): the core's segments
/// (`DocumentState.statusLine`), drawn and never composed here. It sits on the keyboard layout
/// guide, so it is above the home indicator and rises with a keyboard; the banner sits above it.
/// A tap runs a segment's command. A compact width (an iPhone) keeps priority 1 only (the engine,
/// the run, the newest message); a regular one leaves out 3, then 2, as the width runs out.
@MainActor
final class StatusBar: UIVisualEffectView {
    var onCommand: ((StatusCommand) -> Void)?
    private let left = UIStackView()
    private let right = UIStackView()
    private var line = StatusLine(on: false)
    private var placed: [(view: UIView, priority: Int, right: Bool)] = []
    private(set) var shownLevel = 3
    /// The most priorities this width class allows (1 in a compact width).
    var maxLevel = 3 {
        didSet {
            guard maxLevel != oldValue else { return }
            // drawn again: a compact width has no detail lines
            let l = line
            line = StatusLine(on: false, segments: [])
            show(l)
        }
    }

    static let rowHeight: CGFloat = 30

    init() {
        super.init(effect: UIBlurEffect(style: .systemChromeMaterial))
        let rule = UIView()
        rule.backgroundColor = .separator
        for s in [left, right] {
            s.axis = .horizontal
            s.spacing = 12
            s.alignment = .center
        }
        right.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        for v in [rule, left, right] {
            v.translatesAutoresizingMaskIntoConstraints = false
            contentView.addSubview(v)
        }
        NSLayoutConstraint.activate([
            rule.topAnchor.constraint(equalTo: contentView.topAnchor),
            rule.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
            rule.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
            rule.heightAnchor.constraint(equalToConstant: 1 / max(UIScreen.main.scale, 1)),
            left.leadingAnchor.constraint(equalTo: contentView.safeAreaLayoutGuide.leadingAnchor, constant: 12),
            left.centerYAnchor.constraint(equalTo: contentView.topAnchor, constant: Self.rowHeight / 2),
            right.leadingAnchor.constraint(greaterThanOrEqualTo: left.trailingAnchor, constant: 12),
            right.trailingAnchor.constraint(equalTo: contentView.safeAreaLayoutGuide.trailingAnchor, constant: -12),
            right.centerYAnchor.constraint(equalTo: left.centerYAnchor),
        ])
        accessibilityIdentifier = "statusLine"
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    func show(_ l: StatusLine) {
        guard l != line else { return }
        line = l
        for s in [left, right] { s.arrangedSubviews.forEach { $0.removeFromSuperview() } }
        placed = []
        for seg in l.segments {
            let isRight = seg.side == "right"
            let v = view(for: seg)
            (isRight ? right : left).addArrangedSubview(v)
            placed.append((v, seg.priority, isRight))
        }
        setNeedsLayout()
    }

    override func layoutSubviews() {
        fit()
        super.layoutSubviews()
    }

    private func fit() {
        let room = bounds.width - safeAreaInsets.left - safeAreaInsets.right - 36
        var level = maxLevel
        while level > 1 {
            var w: CGFloat = 0
            for p in placed where p.priority <= level {
                let fw = p.view.systemLayoutSizeFitting(UIView.layoutFittingCompressedSize).width
                w += (p.right ? min(fw, 140) : fw) + 12
            }
            if w <= room { break }
            level -= 1
        }
        shownLevel = level
        for p in placed where p.view.isHidden != (p.priority > level) { p.view.isHidden = p.priority > level }
    }

    private func color(_ t: StatusTone?, dim: Bool) -> UIColor {
        switch t {
        case .error: .systemRed
        case .warn: .systemOrange
        default: dim ? .secondaryLabel : .label
        }
    }

    private func view(for seg: StatusSegment) -> UIView {
        let row = UIStackView()
        row.axis = .horizontal
        row.spacing = 5
        row.alignment = .center
        row.accessibilityIdentifier = "status.\(seg.id)"
        if let d = seg.dot {
            let dot = UIView()
            dot.backgroundColor = d.word == .ok ? .systemGreen : d.word == .busy ? .systemOrange : .systemRed
            dot.layer.cornerRadius = 4
            dot.translatesAutoresizingMaskIntoConstraints = false
            dot.widthAnchor.constraint(equalToConstant: 8).isActive = true
            dot.heightAnchor.constraint(equalToConstant: 8).isActive = true
            row.addArrangedSubview(dot)
        }
        if seg.busy == true {
            let spin = UIActivityIndicatorView(style: .medium)
            spin.transform = CGAffineTransform(scaleX: 0.7, y: 0.7)
            spin.startAnimating()
            row.addArrangedSubview(spin)
        }
        let font = UIFont.preferredFont(forTextStyle: .footnote)
        if seg.role == "toggle" || seg.role == "action" {
            var cfg: UIButton.Configuration = seg.role == "toggle" && seg.on == true ? .tinted() : .plain()
            cfg.title = seg.text
            cfg.contentInsets = NSDirectionalEdgeInsets(top: 2, leading: 6, bottom: 2, trailing: 6)
            cfg.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { a in
                var a = a
                a.font = font
                return a
            }
            let b = UIButton(configuration: cfg, primaryAction: UIAction { [weak self] _ in self?.run(seg.command) })
            b.accessibilityIdentifier = seg.id
            if seg.role == "toggle" { b.accessibilityTraits.insert(seg.on == true ? .selected : []) }
            row.addArrangedSubview(b)
        } else {
            let t = UILabel()
            t.text = seg.text
            t.font = font
            t.adjustsFontForContentSizeCategory = true
            t.textColor = color(seg.tone?.word, dim: ["hash", "selection", "run"].contains(seg.role) && seg.busy != true)
            t.lineBreakMode = .byTruncatingTail
            if maxLevel == 1 {
                // a compact width (an iPhone): the engine's name and the run cut before the
                // newest message, which keeps up to 45% of the row
                t.setContentCompressionResistancePriority(seg.role == "message" ? .defaultLow + 1 : .defaultLow, for: .horizontal)
                if seg.role == "message" { t.widthAnchor.constraint(lessThanOrEqualToConstant: 170).isActive = true }
            } else if seg.role == "message" {
                t.setContentCompressionResistancePriority(.defaultLow - 1, for: .horizontal)
                t.widthAnchor.constraint(lessThanOrEqualToConstant: 360).isActive = true
            }
            row.addArrangedSubview(t)
        }
        // a detail gives way first: a compact width has none, and a narrow regular one cuts it
        // before the message
        if let d = seg.detail, !d.isEmpty, maxLevel > 1 {
            let t = UILabel()
            t.text = d
            t.font = .preferredFont(forTextStyle: .caption2)
            t.textColor = color(seg.detailTone?.word, dim: true)
            t.lineBreakMode = .byTruncatingTail
            t.setContentCompressionResistancePriority(.defaultLow - 2, for: .horizontal)
            if seg.role == "engine" { t.widthAnchor.constraint(lessThanOrEqualToConstant: 220).isActive = true }
            row.addArrangedSubview(t)
        }
        if let c = seg.command, seg.role != "toggle", seg.role != "action" {
            if c.id == "run.cancel" {
                var cfg = UIButton.Configuration.plain()
                cfg.title = c.label ?? "Cancel"
                cfg.contentInsets = NSDirectionalEdgeInsets(top: 2, leading: 6, bottom: 2, trailing: 6)
                let b = UIButton(configuration: cfg, primaryAction: UIAction { [weak self] _ in self?.run(c) })
                b.accessibilityIdentifier = "\(seg.id).cancel"
                row.addArrangedSubview(b)
            } else {
                row.addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped(_:))))
            }
        }
        if let tip = seg.tooltip {
            row.addInteraction(UIToolTipInteraction(defaultToolTip: tip))
        }
        row.isAccessibilityElement = seg.command == nil || seg.role == "message" || seg.role == "warning"
        row.accessibilityLabel = [seg.text, seg.detail].compactMap { $0 }.joined(separator: ", ")
        if seg.command != nil, row.isAccessibilityElement { row.accessibilityTraits = .button }
        return row
    }

    private func run(_ c: StatusCommand?) {
        if let c { onCommand?(c) }
    }

    @objc private func tapped(_ g: UITapGestureRecognizer) {
        let id = g.view?.accessibilityIdentifier
        run(line.segments.first { "status.\($0.id)" == id }?.command)
    }

    /// What the bar shows now, for `ui.inspect`'s window status.
    var report: JSONValue {
        let shown = zip(line.segments, placed).filter { !$0.1.view.isHidden }.map { JSONValue.string($0.0.id) }
        return ["visible": .bool(!isHidden), "level": .int(Int64(shownLevel)), "shown": .array(shown)]
    }
}
