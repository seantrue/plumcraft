// The node palette on the iPad (N4 slice 1): a popover at the point it opened in regular width, a
// sheet with detents in compact width, drawn from the kit's `PaletteModel` (the same model as the
// Mac's popover). A search field over an inset-grouped list by the core's categories; a tap on a
// row adds it (one undo step, with a dropped wire connected); with a keyboard, ↑ ↓ move the
// highlight, Return adds and Escape dismisses; swiping the sheet down or tapping outside the
// popover dismisses it, leaving nothing. A kind whose engine isn't started says which it starts,
// and while it starts the palette shows a spinner and what it is adding.

#if os(iOS)
import PlumCraftKit
import PlumCraftModels
import UIKit

@MainActor
public final class PaletteController: UIViewController, UITableViewDataSource, UITableViewDelegate, UITextFieldDelegate,
    UIPopoverPresentationControllerDelegate, UIAdaptivePresentationControllerDelegate {
    public let palette: PaletteModel
    let field = UISearchTextField()
    let table = UITableView(frame: .zero, style: .insetGrouped)
    let footer = UILabel()
    let spinner = UIActivityIndicatorView(style: .medium)
    var closingFromModel = false
    /// Shown or gone (scripts and UI tests read it).
    public var onShown: ((Bool) -> Void)?

    public init(palette: PaletteModel) {
        self.palette = palette
        super.init(nibName: nil, bundle: nil)
        palette.onUpdate = { [weak self] in self?.reload() }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    public override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemGroupedBackground
        field.placeholder = "Search nodes"
        field.delegate = self
        field.returnKeyType = .go
        field.autocorrectionType = .no
        field.autocapitalizationType = .none
        field.addTarget(self, action: #selector(changed(_:)), for: .editingChanged)
        field.accessibilityIdentifier = "PaletteSearch"
        table.dataSource = self
        table.delegate = self
        table.accessibilityIdentifier = "Palette"
        table.register(UITableViewCell.self, forCellReuseIdentifier: "kind")
        table.keyboardDismissMode = .onDrag
        footer.font = .preferredFont(forTextStyle: .footnote)
        footer.adjustsFontForContentSizeCategory = true
        footer.textColor = .secondaryLabel
        footer.numberOfLines = 2
        spinner.hidesWhenStopped = true
        let bottom = UIStackView(arrangedSubviews: [spinner, footer])
        bottom.spacing = 8
        bottom.alignment = .center
        for v in [field, table, bottom] as [UIView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(v)
        }
        let g = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            field.topAnchor.constraint(equalTo: g.topAnchor, constant: 12),
            field.leadingAnchor.constraint(equalTo: g.leadingAnchor, constant: 16),
            field.trailingAnchor.constraint(equalTo: g.trailingAnchor, constant: -16),
            table.topAnchor.constraint(equalTo: field.bottomAnchor, constant: 4),
            table.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            table.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            table.bottomAnchor.constraint(equalTo: bottom.topAnchor, constant: -6),
            bottom.leadingAnchor.constraint(equalTo: g.leadingAnchor, constant: 20),
            bottom.trailingAnchor.constraint(lessThanOrEqualTo: g.trailingAnchor, constant: -20),
            bottom.bottomAnchor.constraint(equalTo: g.bottomAnchor, constant: -10),
        ])
        preferredContentSize = CGSize(width: 340, height: 440)
        reload()
    }

    /// Present over `host`, pointing at view point `p` of `source`; a sheet in compact width.
    public func present(from host: UIViewController, source: UIView, at p: CGPoint, focus: Bool) {
        if presentingViewController != nil {
            popoverPresentationController?.sourceRect = CGRect(x: p.x - 1, y: p.y - 1, width: 2, height: 2)
            return
        }
        let compact = host.traitCollection.horizontalSizeClass == .compact
        if compact {
            modalPresentationStyle = .pageSheet
            if let s = sheetPresentationController {
                s.detents = [.medium(), .large()]
                s.prefersGrabberVisible = true
            }
        } else {
            modalPresentationStyle = .popover
            if let pop = popoverPresentationController {
                pop.sourceView = source
                pop.sourceRect = CGRect(x: p.x - 1, y: p.y - 1, width: 2, height: 2)
                pop.permittedArrowDirections = .any
                pop.delegate = self
            }
        }
        presentationController?.delegate = self
        field.text = palette.query
        host.present(self, animated: true) { [weak self] in
            if focus { self?.field.becomeFirstResponder() }
            self?.onShown?(true)
        }
    }

    /// The core closed the palette.
    public func close() {
        guard presentingViewController != nil, !isBeingDismissed, !closingFromModel else { return }
        closingFromModel = true
        dismiss(animated: true) { [weak self] in
            self?.closingFromModel = false
            self?.onShown?(false)
        }
    }

    /// Stays a popover in compact height (a sheet only in compact width, by `present`).
    public func adaptivePresentationStyle(for controller: UIPresentationController, traitCollection: UITraitCollection) -> UIModalPresentationStyle {
        traitCollection.horizontalSizeClass == .compact ? .pageSheet : .none
    }

    public func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
        if !closingFromModel { palette.dismiss() }
        onShown?(false)
    }

    func dismissByPerson() {
        // the model's dismissal closes this view through the host (`close`)
        palette.dismiss()
    }

    // MARK: Contents

    func reload() {
        guard isViewLoaded else { return }
        table.reloadData()
        if let ip = indexPath(of: palette.selected), table.numberOfSections > ip.section {
            table.selectRow(at: ip, animated: false, scrollPosition: .none)
        }
        let busy = palette.pendingText
        if busy != nil { spinner.startAnimating() } else { spinner.stopAnimating() }
        footer.text = busy ?? palette.error ?? (palette.items.isEmpty ? (palette.forWire ? "Nothing here takes this wire" : "No kinds match") : palette.headline)
        footer.textColor = palette.error != nil && busy == nil ? .systemRed : .secondaryLabel
        field.placeholder = palette.forWire ? "Search kinds that take this wire" : "Search nodes"
        field.isEnabled = busy == nil
        table.allowsSelection = busy == nil
        if presentingViewController != nil { onShown?(true) }
    }

    func indexPath(of index: Int) -> IndexPath? {
        var k = 0
        for (s, sec) in palette.sections.enumerated() {
            if index < k + sec.items.count { return IndexPath(row: index - k, section: s) }
            k += sec.items.count
        }
        return nil
    }

    func index(of ip: IndexPath) -> Int {
        palette.sections.prefix(ip.section).reduce(0) { $0 + $1.items.count } + ip.row
    }

    @objc func changed(_ f: UITextField) {
        palette.search(f.text ?? "")
    }

    public func textFieldShouldReturn(_ textField: UITextField) -> Bool {
        palette.chooseSelected()
        return false
    }

    public func numberOfSections(in tableView: UITableView) -> Int { palette.sections.count }

    public func tableView(_ tableView: UITableView, titleForHeaderInSection section: Int) -> String? {
        palette.sections.indices.contains(section) ? palette.sections[section].title : nil
    }

    public func tableView(_ tableView: UITableView, numberOfRowsInSection section: Int) -> Int {
        palette.sections.indices.contains(section) ? palette.sections[section].items.count : 0
    }

    public func tableView(_ tableView: UITableView, cellForRowAt ip: IndexPath) -> UITableViewCell {
        let cell = tableView.dequeueReusableCell(withIdentifier: "kind", for: ip)
        let sec = palette.sections[ip.section]
        let it = sec.items[ip.row]
        var c = cell.defaultContentConfiguration()
        c.text = it.label
        var sub: [String] = []
        if let port = it.port { sub.append("into \(port)") }
        if let e = it.starts { sub.append("starts the \(e) engine") }
        c.secondaryText = sub.isEmpty ? nil : sub.joined(separator: " · ")
        c.secondaryTextProperties.color = .secondaryLabel
        cell.contentConfiguration = c
        cell.accessibilityLabel = it.label
        return cell
    }

    public func tableView(_ tableView: UITableView, didSelectRowAt ip: IndexPath) {
        let i = index(of: ip)
        guard palette.items.indices.contains(i) else { return }
        palette.highlight(i)
        palette.choose(palette.items[i])
    }

    // MARK: A keyboard

    public override var keyCommands: [UIKeyCommand]? {
        let up = UIKeyCommand(input: UIKeyCommand.inputUpArrow, modifierFlags: [], action: #selector(up(_:)))
        let down = UIKeyCommand(input: UIKeyCommand.inputDownArrow, modifierFlags: [], action: #selector(down(_:)))
        let esc = UIKeyCommand(input: UIKeyCommand.inputEscape, modifierFlags: [], action: #selector(escape(_:)))
        let ret = UIKeyCommand(input: "\r", modifierFlags: [], action: #selector(enter(_:)))
        for k in [up, down, esc, ret] { k.wantsPriorityOverSystemBehavior = true }
        return [up, down, esc, ret]
    }

    @objc func up(_ c: UIKeyCommand) { palette.move(-1) }
    @objc func down(_ c: UIKeyCommand) { palette.move(1) }
    @objc func escape(_ c: UIKeyCommand) { dismissByPerson() }
    @objc func enter(_ c: UIKeyCommand) { palette.chooseSelected() }

    /// Choose the row labelled `label` (scripts).
    public func choose(label: String) -> Bool {
        guard let it = palette.items.first(where: { $0.label == label || $0.kind == label }) else { return false }
        palette.choose(it)
        return true
    }
}
#endif
