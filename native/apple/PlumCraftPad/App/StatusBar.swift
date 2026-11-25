import PlumCraftKit
import PlumCraftModels
import UIKit

/// The status line along the bottom of the canvas (research note 28): the core's segments
/// (`DocumentState.statusLine`), drawn and never composed here. It sits on the keyboard layout
/// guide, so it is above the home indicator and rises with a keyboard; the banner sits above it.
/// A tap runs a segment's command. Each segment is shown whole or not at all (the kit's
/// `StatusLine.fit`): a compact width (an iPhone) keeps priority 1 only (the engine, the run, the
/// newest message), in their short forms where it must; a regular one drops the details, then
/// priority 3 and 2, as the width runs out. The newest message keeps its place.
@MainActor
final class StatusBar: UIVisualEffectView {
    var onCommand: ((StatusCommand) -> Void)?
    private let left = UIStackView()
    private let right = UIStackView()
    private var line = StatusLine(on: false)
    /// What the line shows now (the fitting rule's plan).
    private var plan: [StatusPlaced] = []
    private var widths: [String: Double] = [:]
    /// The most priorities this width class allows (1 in a compact width).
    var maxLevel = 3 {
        didSet { if maxLevel != oldValue { setNeedsLayout() } }
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
        plan = []
        setNeedsLayout()
    }

    override func layoutSubviews() {
        fit()
        super.layoutSubviews()
    }

    /// The segments that fit the width, whole (the kit's rule), drawn when they change.
    private func fit() {
        guard bounds.width > 0 else { return }
        let room = Double(bounds.width - safeAreaInsets.left - safeAreaInsets.right) - 24 - 2
        let size = traitCollection.preferredContentSizeCategory.rawValue
        let next = line.fit(room: room, spacing: Double(left.spacing), upTo: maxLevel) { seg, form in
            let key = "\(seg.id)|\(form.rawValue)|\(seg.text)|\(seg.short ?? "")|\(seg.detail ?? "")|\(seg.busy == true)|\(seg.on == true)|\(size)"
            if let w = widths[key] { return w }
            let w = Double(view(for: seg, form: form).systemLayoutSizeFitting(UIView.layoutFittingCompressedSize).width)
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

    /// The width left out segments or took them back (for the window's status).
    var onFit: (() -> Void)?

    private func color(_ t: StatusTone?, dim: Bool) -> UIColor {
        switch t {
        case .error: .systemRed
        case .warn: .systemOrange
        default: dim ? .secondaryLabel : .label
        }
    }

    private func view(for seg: StatusSegment, form: StatusForm) -> UIView {
        let words = StatusPlaced(segment: seg, form: form).text
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
            cfg.title = words
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
            t.text = words
            t.font = font
            t.adjustsFontForContentSizeCategory = true
            t.textColor = color(seg.tone?.word, dim: ["hash", "selection", "run"].contains(seg.role) && seg.busy != true)
            t.lineBreakMode = .byTruncatingTail
            // only the newest message alone on a line too narrow for it is cut
            if form == .cut { t.setContentCompressionResistancePriority(.defaultLow - 1, for: .horizontal) }
            row.addArrangedSubview(t)
        }
        if form == .full, let d = seg.detail, !d.isEmpty {
            let t = UILabel()
            t.text = d
            t.font = .preferredFont(forTextStyle: .caption2)
            t.textColor = color(seg.detailTone?.word, dim: true)
            t.lineBreakMode = .byTruncatingTail
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
        ["visible": .bool(!isHidden), "shown": .array(plan.map { .string($0.segment.id) }),
         "forms": .array(plan.map { .string("\($0.segment.id):\($0.form.rawValue):\($0.text)") })]
    }
}
