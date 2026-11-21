// The parameters inspector on the iPad (N4 slice 3): every input of one node, shown on its face
// or not, from the core's state (`node.parameters` opens it, the scene's
// `view.windows.parameters` names the node, `parameters.get` gives the rows). A popover from the
// node in regular width, a sheet with medium and large detents in compact width; an inset-grouped
// list by the core's sections. Each row: its label (the core's help below it when it has some),
// a native control for its kind (a slider and a field for a number with a range, a switch, a
// pull-down menu for a choice, a field for text, a text view for several lines, a colour well),
// Show on Face (the eye, `node.face`) and ↺ (`row.set {reset}`). A wired or inherited input says
// where its value comes from. Every value goes to the core (`row.set`), which checks it.
// Opened by a double tap on a node, the node's menu (⋮ or a long press), Node › Edit Parameters…
// (⌘I) or a script; Done, a tap away or a swipe down closes it (`node.parameters {}`).

import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import UIKit

@MainActor
final class PadParameters: NSObject, UIPopoverPresentationControllerDelegate, UIAdaptivePresentationControllerDelegate {
    let params: ParametersModel
    weak var host: GraphViewController?
    private(set) var nav: UINavigationController?
    private(set) var list: ParametersList?
    private var closingFromCore = false

    init(model: DocumentModel) {
        params = ParametersModel(model: model)
        super.init()
        params.onChange = { [weak self] r in self?.changed(r) }
        params.onNote = { [weak self] _, text, error in self?.list?.note(text, error: error) }
    }

    private func changed(_ r: ParametersReply?) {
        guard let r else {
            if let n = nav {
                closingFromCore = true
                nav = nil
                list = nil
                n.dismiss(animated: true)
            }
            host?.reportNow()
            return
        }
        if let l = list, l.reply?.node == r.node {
            l.show(r)
        } else {
            present(r)
        }
        host?.reportNow()
    }

    private func present(_ r: ParametersReply) {
        guard let host, let v = host.canvasView else { return }
        if let n = nav { n.dismiss(animated: false) }
        let l = ParametersList(params: params)
        l.show(r)
        l.navigationItem.title = r.title
        l.navigationItem.rightBarButtonItem = UIBarButtonItem(systemItem: .done, primaryAction: UIAction { [weak self] _ in
            guard let self else { return }
            // closes at once, whatever the core is doing; the core is told after
            if let n = nav {
                closingFromCore = true
                nav = nil
                list = nil
                n.dismiss(animated: true)
            }
            Task { await self.params.close() }
        })
        l.navigationItem.rightBarButtonItem?.accessibilityIdentifier = "parameters-done"
        let n = UINavigationController(rootViewController: l)
        n.view.accessibilityIdentifier = "parameters"
        if host.traitCollection.horizontalSizeClass == .compact {
            n.modalPresentationStyle = .pageSheet
            if let s = n.sheetPresentationController {
                s.detents = [.medium(), .large()]
                s.prefersGrabberVisible = true
            }
        } else {
            n.modalPresentationStyle = .popover
            n.preferredContentSize = CGSize(width: 420, height: 560)
            if let p = n.popoverPresentationController {
                p.sourceView = v
                p.sourceRect = anchor(r.node, in: v)
                p.permittedArrowDirections = [.left, .right, .up, .down]
                p.delegate = self
            }
        }
        n.presentationController?.delegate = self
        nav = n
        list = l
        closingFromCore = false
        host.presentReplacing(n)
    }

    /// The node's face on screen, in the canvas view.
    func anchor(_ node: String, in v: CanvasView) -> CGRect {
        let c = v.canvas
        guard let nid = c.nid(of: node), let f = c.model.faces[nid]?.item else { return CGRect(x: v.bounds.midX, y: v.bounds.midY, width: 1, height: 1) }
        let size = c.faceLayout(nid: nid)?.size ?? CGSize(width: 220, height: 160)
        let a = c.viewport.toView(CGPoint(x: f.pos.x, y: f.pos.y))
        let b = c.viewport.toView(CGPoint(x: f.pos.x + size.width, y: f.pos.y + size.height))
        let r = CGRect(x: a.x, y: a.y, width: max(b.x - a.x, 1), height: max(b.y - a.y, 1)).intersection(v.bounds.insetBy(dx: 4, dy: 4))
        return r.isNull || r.isEmpty ? CGRect(x: v.bounds.midX, y: v.bounds.midY, width: 1, height: 1) : r
    }

    /// The canvas moved: the popover's arrow stays on its node.
    func follow() {
        guard let n = nav, let p = n.popoverPresentationController, let v = host?.canvasView, let node = params.node else { return }
        p.sourceRect = anchor(node, in: v)
    }

    // a tap away or a swipe down: the core's window goes too
    func presentationControllerDidDismiss(_ pc: UIPresentationController) {
        nav = nil
        list = nil
        if !closingFromCore {
            let p = params
            Task { await p.close() }
        }
    }

    func adaptivePresentationStyle(for controller: UIPresentationController, traitCollection: UITraitCollection) -> UIModalPresentationStyle {
        traitCollection.horizontalSizeClass == .compact ? .pageSheet : .none
    }

    var describe: JSONValue {
        guard let r = params.reply else { return .null }
        return ["node": .string(r.node), "shown": .bool(nav?.presentingViewController != nil), "rows": .uint(UInt64(r.count)),
                "hidden": .array(r.hidden.map { .string($0) }),
                "style": .string(nav?.modalPresentationStyle == .popover ? "popover" : "sheet")]
    }
}

@MainActor
final class ParametersList: UITableViewController, UITextFieldDelegate, UITextViewDelegate {
    let params: ParametersModel
    private(set) var reply: ParametersReply?
    private var typing = false
    /// A slider is being dragged: its field follows; the rows aren't reloaded under it.
    private var sliding = false
    private let noteLabel = UILabel()

    init(params: ParametersModel) {
        self.params = params
        super.init(style: .insetGrouped)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    override func viewDidLoad() {
        super.viewDidLoad()
        tableView.allowsSelection = false
        tableView.keyboardDismissMode = .interactive
        tableView.estimatedRowHeight = 64
        noteLabel.numberOfLines = 0
        noteLabel.font = .preferredFont(forTextStyle: .footnote)
        noteLabel.adjustsFontForContentSizeCategory = true
        header()
    }

    private func header() {
        guard isViewLoaded, let r = reply else { return }
        let box = UIView(frame: CGRect(x: 0, y: 0, width: 400, height: 10))
        let kind = UILabel()
        kind.text = "\(r.display) · \(r.node)"
        kind.font = .preferredFont(forTextStyle: .footnote)
        kind.textColor = .secondaryLabel
        let about = UILabel()
        about.text = r.description
        about.numberOfLines = 3
        about.font = .preferredFont(forTextStyle: .footnote)
        about.textColor = .secondaryLabel
        let s = UIStackView(arrangedSubviews: [kind, about, noteLabel])
        s.axis = .vertical
        s.spacing = 4
        s.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(s)
        NSLayoutConstraint.activate([
            s.topAnchor.constraint(equalTo: box.topAnchor, constant: 8),
            s.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 20),
            s.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -20),
            s.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -4),
        ])
        box.frame.size = box.systemLayoutSizeFitting(CGSize(width: tableView.bounds.width > 0 ? tableView.bounds.width : 400, height: 0),
                                                     withHorizontalFittingPriority: .required, verticalFittingPriority: .fittingSizeLevel)
        tableView.tableHeaderView = box
    }

    func show(_ r: ParametersReply) {
        let same = reply?.node == r.node
        reply = r
        navigationItem.title = r.title
        // a field being typed into isn't pulled out from under the person
        if (typing || sliding) && same { return }
        header()
        tableView.reloadData()
    }

    func note(_ text: String, error: Bool) {
        noteLabel.text = text
        noteLabel.textColor = error ? .systemRed : .secondaryLabel
        header()
    }

    override func numberOfSections(in tableView: UITableView) -> Int { reply?.sections.count ?? 0 }

    override func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        guard let r = reply, r.sections.count > 1 else { return nil }
        return r.sections[section].title
    }

    override func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        reply?.sections[section].rows.count ?? 0
    }

    override func tableView(_ tableView: UITableView, cellForRowAt ip: IndexPath) -> UITableViewCell {
        let cell = UITableViewCell(style: .default, reuseIdentifier: nil)
        guard let p = reply?.sections[ip.section].rows[ip.row] else { return cell }
        let r = p.row
        cell.accessibilityIdentifier = "param.\(r.key)"
        let label = UILabel()
        label.text = r.label
        label.font = .preferredFont(forTextStyle: .body)
        label.adjustsFontForContentSizeCategory = true
        label.textColor = r.inactive == nil ? .label : .tertiaryLabel
        label.setContentHuggingPriority(.required, for: .horizontal)
        let control = p.editable ? editor(p) : source(p)
        let face = UIButton(type: .system)
        face.setImage(UIImage(systemName: p.onFace ? "eye" : "eye.slash"), for: .normal)
        face.isEnabled = p.canHide
        face.accessibilityLabel = p.onFace ? "Shown on the face" : "Hidden from the face"
        face.accessibilityIdentifier = "face.\(r.key)"
        let params = self.params
        face.addAction(UIAction { _ in Task { await params.showOnFace(port: r.key, !p.onFace) } }, for: .primaryActionTriggered)
        let reset = UIButton(type: .system)
        reset.setImage(UIImage(systemName: "arrow.uturn.backward"), for: .normal)
        reset.isHidden = !p.reset
        reset.accessibilityLabel = "Reset to the default"
        reset.accessibilityIdentifier = "reset.\(r.key)"
        reset.addAction(UIAction { _ in Task { await params.reset(port: r.key) } }, for: .primaryActionTriggered)
        let line = UIStackView(arrangedSubviews: [label, control, reset, face])
        line.spacing = 10
        line.alignment = .center
        let stack = UIStackView(arrangedSubviews: [line])
        stack.axis = .vertical
        stack.spacing = 2
        if r.source.raw == "own", let tip = p.help.split(separator: "\n\n").first, r.hover != nil || p.help.contains("·") {
            let help = UILabel()
            help.text = String(tip)
            help.font = .preferredFont(forTextStyle: .caption1)
            help.textColor = .secondaryLabel
            help.numberOfLines = 2
            stack.addArrangedSubview(help)
        }
        stack.translatesAutoresizingMaskIntoConstraints = false
        cell.contentView.addSubview(stack)
        let m = cell.contentView.layoutMarginsGuide
        NSLayoutConstraint.activate([
            stack.topAnchor.constraint(equalTo: m.topAnchor),
            stack.bottomAnchor.constraint(equalTo: m.bottomAnchor),
            stack.leadingAnchor.constraint(equalTo: m.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: m.trailingAnchor),
            label.widthAnchor.constraint(lessThanOrEqualToConstant: 140),
        ])
        return cell
    }

    private func source(_ p: ParametersReply.Row) -> UIView {
        let r = p.row
        let l = UILabel()
        l.text = (r.text ?? (r.widget.map { $0.value.string ?? $0.value.text } ?? "")) + (r.from.map { "  \($0)" } ?? "")
        l.textColor = .secondaryLabel
        l.font = .preferredFont(forTextStyle: .callout)
        l.textAlignment = .right
        l.numberOfLines = 2
        return l
    }

    private func editor(_ p: ParametersReply.Row) -> UIView {
        let r = p.row
        guard let w = r.widget else { return source(p) }
        let params = self.params
        let key = r.key
        let made: UIView
        switch w.kind.word {
        case .toggle?:
            let s = UISwitch()
            s.isOn = w.value.bool ?? w.default.bool ?? false
            s.addAction(UIAction { [weak s] _ in Task { await params.set(port: key, value: .bool(s?.isOn ?? false)) } }, for: .valueChanged)
            made = s
        case .choice?:
            var c = UIButton.Configuration.gray()
            c.title = w.choices.first { $0.value.same(w.value) }?.label ?? (w.value.string ?? w.value.text)
            c.indicator = .popup
            let b = UIButton(configuration: c)
            b.showsMenuAsPrimaryAction = true
            b.menu = UIMenu(children: w.choices.map { ch in
                UIAction(title: ch.label, attributes: ch.disabled == nil ? [] : .disabled, state: ch.value.same(w.value) ? .on : .off) { _ in
                    Task { await params.set(port: key, value: ch.value) }
                }
            })
            made = b
        case .color?:
            let well = UIColorWell()
            if let cg = FaceLayout.widgetColor(w) { well.selectedColor = UIColor(cgColor: cg) }
            well.supportsAlpha = false
            well.addAction(UIAction { [weak well] _ in
                guard let c = well?.selectedColor else { return }
                var (r, g, b, a): (CGFloat, CGFloat, CGFloat, CGFloat) = (0, 0, 0, 0)
                c.getRed(&r, green: &g, blue: &b, alpha: &a)
                Task { await params.set(port: key, value: .array([.double(Double(r)), .double(Double(g)), .double(Double(b))])) }
            }, for: .valueChanged)
            made = well
        case .number? where w.min != nil && w.max != nil:
            let lo = w.softMin ?? w.min ?? 0
            let hi = w.softMax ?? w.max ?? 1
            let s = UISlider()
            s.minimumValue = Float(lo)
            s.maximumValue = Float(max(hi, lo + 1e-9))
            s.value = Float(w.value.double ?? lo)
            // continuous: the field follows the finger; the value is set once, when it lifts
            // (one undo step), on the core's step and decimals (N4 slice 4)
            s.isContinuous = true
            s.accessibilityIdentifier = "slider.\(key)"
            let f = field(p, w)
            f.widthAnchor.constraint(equalToConstant: 64).isActive = true
            let commit = { [weak s] in
                guard let s else { return }
                let v = SliderValue.of(Double(s.value), w)
                Task { await params.set(port: key, value: v) }
            }
            s.addAction(UIAction { [weak s, weak f, weak self] _ in
                guard let s else { return }
                let v = SliderValue.of(Double(s.value), w)
                f?.text = CanvasDrag.numberText(v.double ?? Double(s.value), w)
                self?.sliding = s.isTracking
                // VoiceOver's increments and a hardware keyboard move it without tracking
                if !s.isTracking { commit() }
            }, for: .valueChanged)
            s.addAction(UIAction { [weak self] _ in
                self?.sliding = false
                commit()
            }, for: [.touchUpInside, .touchUpOutside])
            s.addAction(UIAction { [weak self] _ in self?.sliding = false }, for: .touchCancel)
            let h = UIStackView(arrangedSubviews: [s, f])
            h.spacing = 8
            made = h
        case .multiline?:
            let t = UITextView()
            t.text = w.value.string ?? ""
            t.font = .monospacedSystemFont(ofSize: 14, weight: .regular)
            t.autocorrectionType = .no
            t.autocapitalizationType = .none
            t.smartQuotesType = .no
            t.smartDashesType = .no
            t.layer.cornerRadius = 6
            t.backgroundColor = .tertiarySystemFill
            t.accessibilityIdentifier = "edit.\(key)"
            t.heightAnchor.constraint(equalToConstant: 72).isActive = true
            t.delegate = self
            t.tag = key.hashValue
            multiline[t.tag] = key
            made = t
        default:
            made = field(p, w)
        }
        made.setContentHuggingPriority(.defaultLow, for: .horizontal)
        return made
    }

    private var multiline: [Int: String] = [:]
    private var fieldKeys: [ObjectIdentifier: String] = [:]

    private func field(_ p: ParametersReply.Row, _ w: Widget) -> UITextField {
        let f = UITextField()
        f.text = FieldEditorHost.fieldText(w)
        f.placeholder = w.blank ? "blank" : p.row.label
        f.borderStyle = .roundedRect
        f.autocorrectionType = .no
        f.autocapitalizationType = .none
        f.smartQuotesType = .no
        f.smartDashesType = .no
        f.returnKeyType = .done
        if w.kind.word == .number {
            f.keyboardType = .numbersAndPunctuation
            f.textAlignment = .right
            f.font = .monospacedDigitSystemFont(ofSize: 17, weight: .regular)
        }
        f.accessibilityIdentifier = "edit.\(p.row.key)"
        f.accessibilityLabel = p.row.label
        f.delegate = self
        fieldKeys[ObjectIdentifier(f)] = p.row.key
        return f
    }

    // MARK: Text

    func textFieldDidBeginEditing(_ t: UITextField) {
        typing = true
        // what is there is selected, so typing replaces it (a number, mostly)
        DispatchQueue.main.async { t.selectAll(nil) }
    }

    func textFieldShouldReturn(_ t: UITextField) -> Bool {
        t.resignFirstResponder()
        return false
    }

    func textFieldDidEndEditing(_ t: UITextField) {
        typing = false
        guard let key = fieldKeys[ObjectIdentifier(t)] else { return }
        let text = t.text ?? ""
        let params = self.params
        Task { await params.set(port: key, text: text) }
    }

    func textViewDidBeginEditing(_ t: UITextView) { typing = true }

    func textViewDidEndEditing(_ t: UITextView) {
        typing = false
        guard let key = multiline[t.tag] else { return }
        let text = t.text ?? ""
        let params = self.params
        Task { await params.set(port: key, text: text) }
    }
}
