// The Watched panel (N3 slice 4): an inspector sidebar on the window's right, collapsible
// (View › Show Watched Panel, ⌥⌘I, or the toolbar's button), showing the value the core watches
// (`ui.inspect`'s `watched`) with a native viewer for the first three of plan 8's order:
//
//   picture     the rgba8 rendition (media.get, plum_blob), made on a worker, in a zoomable view
//               (pinch, ⌘-scroll, or the zoom buttons), with its size and type
//   text.plain  selectable text, with a monospaced option; the excerpt first, the whole text on
//               demand from the full value's bytes (media.get {want: "bytes"})
//   scalar      the value, formatted
//
// Any other kind shows its kind, the summary and "no viewer yet in this app".

import AppKit
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels

@MainActor
final class WatchedPanel: NSViewController {
    let model: DocumentModel
    let pictures: Pictures
    private let titleLabel = NSTextField(labelWithString: "Watched")
    private let summaryLabel = NSTextField(wrappingLabelWithString: "")
    private let body = NSView()
    private var shown: Watched?
    private var current: NSView?
    /// Monospaced text, kept for the window.
    private var mono = false
    /// The whole text, once asked for, by the value's reference.
    private var fullText: [String: String] = [:]
    /// What the panel shows now, for scripts (`--status-file`).
    private(set) var describe: [String: JSONValue] = [:]
    var onShown: (() -> Void)?

    init(model: DocumentModel, pictures: Pictures) {
        self.model = model
        self.pictures = pictures
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    override func loadView() {
        let v = NSView(frame: NSRect(x: 0, y: 0, width: 320, height: 600))
        titleLabel.font = .systemFont(ofSize: NSFont.systemFontSize, weight: .semibold)
        titleLabel.lineBreakMode = .byTruncatingMiddle
        summaryLabel.textColor = .secondaryLabelColor
        summaryLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        summaryLabel.isSelectable = true
        for x in [titleLabel, summaryLabel, body] {
            x.translatesAutoresizingMaskIntoConstraints = false
            v.addSubview(x)
        }
        NSLayoutConstraint.activate([
            titleLabel.topAnchor.constraint(equalTo: v.safeAreaLayoutGuide.topAnchor, constant: 12),
            titleLabel.leadingAnchor.constraint(equalTo: v.leadingAnchor, constant: 14),
            titleLabel.trailingAnchor.constraint(equalTo: v.trailingAnchor, constant: -14),
            summaryLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 4),
            summaryLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            summaryLabel.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            body.topAnchor.constraint(equalTo: summaryLabel.bottomAnchor, constant: 10),
            body.leadingAnchor.constraint(equalTo: v.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: v.trailingAnchor),
            body.bottomAnchor.constraint(equalTo: v.bottomAnchor),
            v.widthAnchor.constraint(greaterThanOrEqualToConstant: 240),
        ])
        view = v
        show(model.watched)
    }

    /// Show `w` (nil: nothing watched).
    func show(_ w: Watched?) {
        guard isViewLoaded else { return }
        if w == shown && current != nil { return }
        shown = w
        current?.removeFromSuperview()
        current = nil
        guard let w else {
            titleLabel.stringValue = "Watched"
            summaryLabel.stringValue = "Nothing watched. Click an output's pin, or a wire, to watch its value."
            describe = ["viewer": .null]
            onShown?()
            return
        }
        titleLabel.stringValue = "\(model.face(id: w.node)?.header.title.text ?? w.node) · \(w.port)"
        summaryLabel.stringValue = w.summary ?? (w.media.map { "\($0), not computed yet" } ?? "not computed yet")
        describe = ["viewer": w.viewer.map(JSONValue.string) ?? .null, "node": .string(w.node), "port": .string(w.port)]
        if let e = w.error, e != .null {
            put(message("This value failed: \(e.string ?? e.text)"))
            describe["error"] = e
            onShown?()
            return
        }
        switch w.viewer {
        case "picture"?: put(pictureView(w))
        case "text.plain"?: put(textView(w))
        case "scalar"?: put(scalarView(w))
        case nil: put(message(w.summary == nil ? "Run the graph to see this value." : "No value yet."))
        case let other?:
            put(message("\(other): no viewer yet in this app."))
            describe["unavailable"] = .string(other)
        }
        onShown?()
    }

    private func put(_ v: NSView) {
        v.translatesAutoresizingMaskIntoConstraints = false
        body.addSubview(v)
        NSLayoutConstraint.activate([
            v.topAnchor.constraint(equalTo: body.topAnchor),
            v.leadingAnchor.constraint(equalTo: body.leadingAnchor),
            v.trailingAnchor.constraint(equalTo: body.trailingAnchor),
            v.bottomAnchor.constraint(equalTo: body.bottomAnchor),
        ])
        current = v
    }

    private func message(_ s: String) -> NSView {
        let box = NSView()
        let l = NSTextField(wrappingLabelWithString: s)
        l.textColor = .secondaryLabelColor
        l.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(l)
        NSLayoutConstraint.activate([
            l.topAnchor.constraint(equalTo: box.topAnchor, constant: 8),
            l.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 14),
            l.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -14),
        ])
        return box
    }

    // MARK: picture

    private func pictureView(_ w: Watched) -> NSView {
        let box = NSView()
        let scroll = NSScrollView()
        scroll.contentView = CenteringClipView()
        scroll.allowsMagnification = true
        scroll.minMagnification = 0.1
        scroll.maxMagnification = 16
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        scroll.drawsBackground = true
        scroll.backgroundColor = .underPageBackgroundColor
        let image = NSImageView()
        image.imageScaling = .scaleNone
        image.imageAlignment = .alignCenter
        scroll.documentView = image
        let info = NSTextField(labelWithString: "")
        info.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        info.textColor = .secondaryLabelColor
        let fit = NSButton(title: "Fit", target: nil, action: nil)
        let one = NSButton(title: "1:1", target: nil, action: nil)
        for b in [fit, one] { b.bezelStyle = .accessoryBarAction; b.controlSize = .small }
        let bar = NSStackView(views: [info, NSView(), fit, one])
        bar.orientation = .horizontal
        for x in [scroll, bar] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: box.topAnchor),
            bar.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 14),
            bar.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -14),
            scroll.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 6),
            scroll.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: box.bottomAnchor),
        ])
        guard let pic = w.picture, let handle = pic.handle else {
            info.stringValue = "no picture"
            return box
        }
        info.stringValue = "\(pic.w) × \(pic.h) · rgba8\(w.media.map { " · \($0)" } ?? "") · loading…"
        let fitAction = ClosureTarget { [weak scroll, weak image] in
            guard let scroll, let size = image?.image?.size, size.width > 0 else { return }
            let s = min(scroll.contentSize.width / size.width, scroll.contentSize.height / size.height)
            scroll.magnification = max(min(s, 1), scroll.minMagnification)
        }
        let oneAction = ClosureTarget { [weak scroll] in scroll?.magnification = 1 }
        fit.target = fitAction; fit.action = #selector(ClosureTarget.fire)
        one.target = oneAction; one.action = #selector(ClosureTarget.fire)
        objc_setAssociatedObject(box, &ClosureTarget.key, [fitAction, oneAction], .OBJC_ASSOCIATION_RETAIN)
        describe["picture"] = ["w": .uint(UInt64(pic.w)), "h": .uint(UInt64(pic.h)), "loaded": false]
        let pics = pictures
        Task { [weak self] in
            // decoded on a worker (Pictures); only setting it here
            guard let got = await pics.load(handle) else {
                info.stringValue = "\(pic.w) × \(pic.h) · rgba8 · \(pics.failures[handle] ?? "couldn't load")"
                return
            }
            let img = NSImage(cgImage: got.image, size: NSSize(width: got.width, height: got.height))
            image.image = img
            image.frame = NSRect(origin: .zero, size: img.size)
            info.stringValue = "\(got.width) × \(got.height) px · rgba8\(w.media.map { " · \($0)" } ?? "")"
            fitAction.fire()
            self?.describe["picture"] = ["w": .uint(UInt64(got.width)), "h": .uint(UInt64(got.height)), "loaded": true]
            self?.onShown?()
        }
        return box
    }

    // MARK: text.plain

    private func textView(_ w: Watched) -> NSView {
        let box = NSView()
        let scroll = NSTextView.scrollableTextView()
        guard let tv = scroll.documentView as? NSTextView else { return box }
        tv.isEditable = false
        tv.isSelectable = true
        tv.textContainerInset = NSSize(width: 10, height: 8)
        let monoBox = NSButton(checkboxWithTitle: "Monospaced", target: nil, action: nil)
        monoBox.controlSize = .small
        monoBox.state = mono ? .on : .off
        let more = NSButton(title: "Show All", target: nil, action: nil)
        more.bezelStyle = .accessoryBarAction
        more.controlSize = .small
        let info = NSTextField(labelWithString: "")
        info.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        info.textColor = .secondaryLabelColor
        let bar = NSStackView(views: [info, NSView(), monoBox, more])
        bar.orientation = .horizontal
        for x in [scroll, bar] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: box.topAnchor),
            bar.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 14),
            bar.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -14),
            scroll.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 6),
            scroll.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: box.bottomAnchor),
        ])
        let ex = w.excerpt
        let total = Int(ex?.totalChars ?? UInt64(ex?.text.count ?? 0))
        let dataHandle = w.data.flatMap { Pictures.handle(of: $0) }
        let setText = { [weak self] (s: String) in
            let mono = self?.mono ?? false
            tv.font = mono ? .monospacedSystemFont(ofSize: 12, weight: .regular) : .systemFont(ofSize: 13)
            tv.string = s
        }
        let whole = dataHandle.flatMap { fullText[$0] }
        setText(whole ?? ex?.text ?? "")
        let cut = whole == nil && total > (ex?.text.count ?? 0)
        info.stringValue = cut ? "first \(ex?.text.count ?? 0) of \(total) characters" : "\(total) characters"
        more.isHidden = !cut || dataHandle == nil
        describe["text"] = ["shown": .uint(UInt64(tv.string.count)), "total": .uint(UInt64(total))]
        let toggle = ClosureTarget { [weak self, weak monoBox] in
            guard let self else { return }
            mono = monoBox?.state == .on
            setText(tv.string)
        }
        monoBox.target = toggle
        monoBox.action = #selector(ClosureTarget.fire)
        let fetch = ClosureTarget { [weak self] in
            guard let self, let h = dataHandle else { return }
            more.isEnabled = false
            info.stringValue = "reading the whole text…"
            Task {
                let t = await self.wholeText(h)
                if let t {
                    self.fullText[h] = t
                    setText(t)
                    info.stringValue = "\(t.count) characters"
                    more.isHidden = true
                    self.describe["text"] = ["shown": .uint(UInt64(t.count)), "total": .uint(UInt64(total))]
                } else {
                    info.stringValue = "the whole text couldn't be read"
                    more.isEnabled = true
                }
            }
        }
        more.target = fetch
        more.action = #selector(ClosureTarget.fire)
        objc_setAssociatedObject(box, &ClosureTarget.key, [toggle, fetch], .OBJC_ASSOCIATION_RETAIN)
        return box
    }

    /// The value's own bytes as UTF-8 (`media.get {want: "bytes"}`, then `plum_blob`), read and
    /// decoded off the main thread.
    private func wholeText(_ handle: String) async -> String? {
        let core = model.core
        guard let note = try? await core.request(MediaNote.self, "media.get", ["handle": .string(handle), "want": "bytes"], doc: model.requestDoc) else { return nil }
        var latest = note
        if latest.status.word == .pending {
            let key = note.key
            _ = await model.wait(timeout: .seconds(30)) { self.model.media[key].map { $0.status.word != .pending } ?? false }
            guard let m = model.media[key] else { return nil }
            latest = m
        }
        guard latest.status.word == .ready else { return nil }
        let key = latest.key
        return await Task.detached {
            guard case .ready(let b)? = try? core.blob(key) else { return nil }
            return String(decoding: b.data, as: UTF8.self)
        }.value
    }

    // MARK: scalar

    private func scalarView(_ w: Watched) -> NSView {
        let box = NSView()
        let v = w.value ?? .null
        let text: String
        switch v {
        case .bool(let b): text = b ? "true" : "false"
        case .string(let s): text = s
        case .null: text = w.summary ?? "—"
        default:
            if let d = v.double {
                let f = NumberFormatter()
                f.numberStyle = .decimal
                f.maximumFractionDigits = if case .double = v { 6 } else { 0 }
                f.usesGroupingSeparator = true
                text = f.string(from: NSNumber(value: d)) ?? v.text
            } else {
                text = v.text
            }
        }
        let l = NSTextField(labelWithString: text)
        l.font = .monospacedDigitSystemFont(ofSize: 28, weight: .medium)
        l.isSelectable = true
        l.lineBreakMode = .byTruncatingTail
        let kind = NSTextField(labelWithString: w.media ?? "scalar")
        kind.textColor = .secondaryLabelColor
        kind.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        for x in [l, kind] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            l.topAnchor.constraint(equalTo: box.topAnchor, constant: 12),
            l.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 14),
            l.trailingAnchor.constraint(lessThanOrEqualTo: box.trailingAnchor, constant: -14),
            kind.topAnchor.constraint(equalTo: l.bottomAnchor, constant: 4),
            kind.leadingAnchor.constraint(equalTo: l.leadingAnchor),
        ])
        describe["scalar"] = .string(text)
        return box
    }
}

/// Keeps a picture smaller than the view in its middle, as Preview does.
final class CenteringClipView: NSClipView {
    override func constrainBoundsRect(_ proposed: NSRect) -> NSRect {
        var r = super.constrainBoundsRect(proposed)
        guard let doc = documentView else { return r }
        let f = doc.frame
        if r.width > f.width { r.origin.x = (f.width - r.width) / 2 }
        if r.height > f.height { r.origin.y = (f.height - r.height) / 2 }
        return r
    }
}

/// A button's action as a closure.
@MainActor
final class ClosureTarget: NSObject {
    nonisolated(unsafe) static var key: UInt8 = 0
    let f: () -> Void
    init(_ f: @escaping () -> Void) { self.f = f }
    @objc func fire() { f() }
}
