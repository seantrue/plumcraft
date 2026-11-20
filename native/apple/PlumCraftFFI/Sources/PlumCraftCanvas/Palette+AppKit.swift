// The node palette on the Mac (N4 slice 1): an NSPopover at the point the palette opened, with a
// search field over a source-list table grouped by the core's categories, drawn from one
// `PaletteModel`. The search field keeps the keyboard: typing filters (the core's `palette.items`),
// ↑ ↓ move the highlight, Return adds, Escape dismisses; a click on a row adds it; a click away
// dismisses (the popover is transient). For a dropped wire the field's prompt says how many kinds
// take it. A kind whose engine isn't started says which engine choosing it starts; while it starts
// the palette shows a spinner and the add's progress instead of closing.

#if os(macOS)
import AppKit
import PlumCraftKit
import PlumCraftModels

@MainActor
public final class PalettePopover: NSObject, NSPopoverDelegate, NSSearchFieldDelegate, NSTableViewDataSource, NSTableViewDelegate {
    public let palette: PaletteModel
    let popover = NSPopover()
    let field = NSSearchField()
    let table = NSTableView()
    let scroll = NSScrollView()
    let footer = NSTextField(labelWithString: "")
    let spinner = NSProgressIndicator()
    /// Rows: a category's header or an item (by its index in `palette.items`).
    enum Row { case header(String), item(Int) }
    var rows: [Row] = []
    var closingFromModel = false
    /// The popover was shown or closed (scripts, screenshots).
    public var onShown: ((Bool) -> Void)?

    public init(palette: PaletteModel) {
        self.palette = palette
        super.init()
        let vc = NSViewController()
        let root = NSView(frame: NSRect(x: 0, y: 0, width: 320, height: 380))
        vc.view = root
        field.placeholderString = "Search nodes"
        field.delegate = self
        field.sendsSearchStringImmediately = true
        field.focusRingType = .none
        let col = NSTableColumn(identifier: .init("item"))
        col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.headerView = nil
        table.style = .sourceList
        table.rowSizeStyle = .default
        table.dataSource = self
        table.delegate = self
        table.target = self
        table.action = #selector(clicked(_:))
        table.setAccessibilityLabel("Node kinds")
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        footer.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        footer.textColor = .secondaryLabelColor
        footer.lineBreakMode = .byTruncatingTail
        spinner.style = .spinning
        spinner.controlSize = .small
        spinner.isDisplayedWhenStopped = false
        for v in [field, scroll, footer, spinner] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
            root.addSubview(v)
        }
        NSLayoutConstraint.activate([
            field.topAnchor.constraint(equalTo: root.topAnchor, constant: 10),
            field.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 10),
            field.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -10),
            scroll.topAnchor.constraint(equalTo: field.bottomAnchor, constant: 8),
            scroll.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: footer.topAnchor, constant: -6),
            spinner.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 12),
            spinner.centerYAnchor.constraint(equalTo: footer.centerYAnchor),
            footer.leadingAnchor.constraint(equalTo: spinner.trailingAnchor, constant: 6),
            footer.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -12),
            footer.bottomAnchor.constraint(equalTo: root.bottomAnchor, constant: -8),
        ])
        popover.contentViewController = vc
        popover.contentSize = root.frame.size
        popover.behavior = .transient
        popover.animates = true
        popover.delegate = self
        palette.onUpdate = { [weak self] in self?.reload() }
    }

    public var isShown: Bool { popover.isShown }

    /// Show at view point `p` of `view` (where the palette opened).
    public func show(at p: CGPoint, in view: NSView) {
        field.stringValue = palette.query
        reload()
        let r = NSRect(x: p.x - 1, y: p.y - 1, width: 2, height: 2)
        if popover.isShown {
            popover.positioningRect = r
        } else {
            popover.show(relativeTo: r, of: view, preferredEdge: .maxY)
        }
        view.window?.makeFirstResponder(field)
        onShown?(true)
    }

    /// The core closed the palette (a pick added, or something else closed it).
    public func close() {
        guard popover.isShown, !closingFromModel else { return }
        // until it has closed (the close animates): a close the core asked for isn't a dismissal
        closingFromModel = true
        popover.performClose(nil)
    }

    public func popoverDidClose(_ notification: Notification) {
        if closingFromModel {
            closingFromModel = false
        } else {
            palette.dismiss()
        }
        onShown?(false)
    }

    func reload() {
        rows = []
        var k = 0
        for s in palette.sections {
            rows.append(.header(s.title))
            for _ in s.items {
                rows.append(.item(k))
                k += 1
            }
        }
        table.reloadData()
        if let r = rows.firstIndex(where: { if case .item(let i) = $0 { i == palette.selected } else { false } }) {
            table.selectRowIndexes([r], byExtendingSelection: false)
            table.scrollRowToVisible(r)
        }
        let busy = palette.pendingText
        if busy != nil { spinner.startAnimation(nil) } else { spinner.stopAnimation(nil) }
        footer.stringValue = busy ?? palette.error ?? (palette.items.isEmpty ? (palette.forWire ? "Nothing here takes this wire" : "No kinds match") : palette.headline)
        footer.textColor = palette.error != nil && busy == nil ? .systemRed : .secondaryLabelColor
        field.placeholderString = palette.forWire ? "Search kinds that take this wire" : "Search nodes"
        field.isEnabled = busy == nil
        if popover.isShown { onShown?(true) }
    }

    // MARK: The field keeps the keyboard

    public func controlTextDidChange(_ obj: Notification) {
        palette.search(field.stringValue)
    }

    public func control(_ control: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
        switch sel {
        case #selector(NSResponder.moveUp(_:)):
            palette.move(-1)
            return true
        case #selector(NSResponder.moveDown(_:)):
            palette.move(1)
            return true
        case #selector(NSResponder.insertNewline(_:)):
            palette.chooseSelected()
            return true
        case #selector(NSResponder.cancelOperation(_:)):
            closingFromModel = true
            popover.performClose(nil)
            palette.dismiss()
            return true
        default:
            return false
        }
    }

    // MARK: The table

    public func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

    public func tableView(_ tableView: NSTableView, isGroupRow row: Int) -> Bool {
        if case .header = rows[row] { true } else { false }
    }

    public func tableView(_ tableView: NSTableView, shouldSelectRow row: Int) -> Bool {
        if case .item = rows[row] { true } else { false }
    }

    public func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
        switch rows[row] {
        case .header(let title):
            let t = NSTextField(labelWithString: title)
            t.font = .systemFont(ofSize: NSFont.smallSystemFontSize, weight: .semibold)
            t.textColor = .secondaryLabelColor
            return t
        case .item(let i):
            guard palette.items.indices.contains(i) else { return nil }
            let it = palette.items[i]
            let cell = NSTableCellView()
            let title = NSTextField(labelWithString: it.label)
            title.lineBreakMode = .byTruncatingTail
            var sub: [String] = []
            if let port = it.port { sub.append("into \(port)") }
            if let e = it.starts { sub.append("starts the \(e) engine") }
            let detail = NSTextField(labelWithString: sub.joined(separator: " · "))
            detail.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            detail.textColor = .secondaryLabelColor
            detail.lineBreakMode = .byTruncatingTail
            let stack = NSStackView(views: sub.isEmpty ? [title] : [title, detail])
            stack.orientation = .horizontal
            stack.spacing = 6
            stack.translatesAutoresizingMaskIntoConstraints = false
            cell.addSubview(stack)
            NSLayoutConstraint.activate([
                stack.leadingAnchor.constraint(equalTo: cell.leadingAnchor, constant: 4),
                stack.trailingAnchor.constraint(lessThanOrEqualTo: cell.trailingAnchor, constant: -4),
                stack.centerYAnchor.constraint(equalTo: cell.centerYAnchor),
            ])
            cell.textField = title
            cell.setAccessibilityLabel(it.label)
            return cell
        }
    }

    public func tableViewSelectionDidChange(_ notification: Notification) {
        let r = table.selectedRow
        guard r >= 0, r < rows.count, case .item(let i) = rows[r] else { return }
        palette.highlight(i)
    }

    @objc func clicked(_ sender: Any?) {
        let r = table.clickedRow
        guard r >= 0, r < rows.count, case .item(let i) = rows[r], palette.items.indices.contains(i) else { return }
        palette.choose(palette.items[i])
    }

    /// Choose the row labelled `label` (scripts).
    public func choose(label: String) -> Bool {
        guard let it = palette.items.first(where: { $0.label == label || $0.kind == label }) else { return false }
        palette.choose(it)
        return true
    }
}
#endif
