// Pop-out windows (N4 slice 3; the maintainer, N3 exit card 14: "Eliminate the side column in
// favor of just the popout window"). The core keeps which values are popped out (the scene's
// `view.windows.popouts`); each is a real window here: titled with the node and port, listed in
// the Window menu, its place and size remembered per value (`frameAutosaveName`), closed with the
// close button or ⌘W (`view.close`), brought forward when the core asks again (`raised`). Opened
// by a double click on a value (an output pin, a preview, a wire), Pop Out in a menu, or a script.
//
// The viewers, plan 8's first three:
//
//   picture     the rgba8 rendition (media.get, plum_blob), made on a worker, in a zoomable view
//               (pinch, ⌘-scroll); Fit and 1:1 are the core's `viewer.zoom`
//   text.plain  selectable text; Wrap, Monospaced and Show All are the core's `viewer.text`, the
//               whole text read from the full value's bytes (media.get {want: "bytes"})
//   scalar      the value, formatted
//
// Any other kind shows its kind, the summary and "no viewer yet in this app" (slice 5 adds them).
//
// A window opens without taking the keyboard from what the person is doing: in front when the
// app is active, else ordered just above its document's window (a scripted run in the
// background never comes forward).

import AppKit
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell

/// The pop-out windows of one document, kept in step with the core's.
@MainActor
final class PopoutWindows {
    let model: DocumentModel
    let pictures: Pictures
    weak var documentWindow: NSWindow?
    private(set) var open: [UInt64: PopoutWindowController] = [:]
    private var known: [UInt64: PopoutWindow] = [:]
    var onChange: (() -> Void)?

    init(model: DocumentModel, pictures: Pictures) {
        self.model = model
        self.pictures = pictures
    }

    func follow(_ w: Windows) {
        let d = PopoutDiff.of(known, w.list)
        known = Dictionary(uniqueKeysWithValues: w.list.map { ($0.id, $0) })
        for id in d.closed { open.removeValue(forKey: id)?.closeFromCore() }
        for p in d.opened {
            let c = PopoutWindowController(popout: PopoutModel(model: model, window: p), pictures: pictures)
            c.onClosed = { [weak self] in
                self?.open[p.id] = nil
                self?.onChange?()
            }
            open[p.id] = c
            c.show(over: documentWindow, raise: true)
            c.popout.refresh()
        }
        for p in d.changed { open[p.id]?.popout.update(p) }
        for p in d.raised { open[p.id]?.show(over: documentWindow, raise: true) }
        onChange?()
    }

    /// Close them all (the document is closing): the core's state goes with the document.
    func closeAll() {
        for c in open.values { c.closeFromCore() }
        open = [:]
    }

    var describe: JSONValue {
        .array(open.values.sorted { $0.popout.id < $1.popout.id }.map { c in
            var o = c.content.describe
            o["id"] = .uint(c.popout.id)
            o["window"] = .int(Int64(c.window?.windowNumber ?? 0))
            o["title"] = .string(c.window?.title ?? "")
            o["visible"] = .bool(c.window?.isVisible ?? false)
            if let w = c.window {
                o["onScreen"] = .bool(NSScreen.screens.contains { $0.visibleFrame.intersects(w.frame) })
                o["level"] = .int(Int64(w.level.rawValue))
            }
            return .object(o)
        })
    }
}

@MainActor
final class PopoutWindowController: NSWindowController, NSWindowDelegate {
    let popout: PopoutModel
    let content: PopoutViewController
    var onClosed: (() -> Void)?
    private var closingFromCore = false

    init(popout: PopoutModel, pictures: Pictures) {
        self.popout = popout
        content = PopoutViewController(popout: popout, pictures: pictures)
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 520, height: 440),
                         styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        w.title = popout.window.title
        w.contentViewController = content
        w.setContentSize(NSSize(width: 520, height: 440))
        w.minSize = NSSize(width: 260, height: 200)
        w.tabbingMode = .disallowed
        // shown over a document window in full screen too (an ordinary window goes to another
        // Space there, where no one sees it: N4 slice 4)
        w.collectionBehavior.insert(.fullScreenAuxiliary)
        w.isReleasedWhenClosed = false
        super.init(window: w)
        w.delegate = self
        // remembered per value: the node and port it follows (a Mac convention for inspectors)
        let key = "PlumCraft Pop-Out \(popout.window.node).\(popout.window.port)"
        shouldCascadeWindows = false
        // a scripted launch (--scratch-state) keeps nothing in the person's defaults
        if CoreHost.options.scratchState {
            w.center()
        } else {
            if !w.setFrameUsingName(key) { w.center() }
            // a frame remembered on a display that is gone (or mostly off every screen) would put
            // the window where no one sees it (N4 slice 4: "popout does not show")
            let onScreen = NSScreen.screens.contains { $0.visibleFrame.intersection(w.frame).width >= 120 && $0.visibleFrame.intersection(w.frame).height >= 80 }
            if !onScreen { w.center() }
            w.setFrameAutosaveName(key)
        }
        popout.onChange = { [weak self] v in
            self?.window?.title = v.title ?? popout.window.title
            self?.content.show(v)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    /// On screen: in front for a person at an active app; else just above the document's window,
    /// never in front of another app's (no activation, no keyboard taken).
    func show(over doc: NSWindow?, raise: Bool) {
        guard let w = window else { return }
        if NSApp.isActive && !CoreHost.options.scratchState {
            w.makeKeyAndOrderFront(nil)
        } else if let doc, doc.isVisible {
            w.order(.above, relativeTo: doc.windowNumber)
        } else {
            w.orderBack(nil)
        }
        _ = raise
    }

    /// The core closed it (`view.close` from a script, the node removed, the document closing).
    func closeFromCore() {
        closingFromCore = true
        window?.close()
    }

    func windowWillClose(_ n: Notification) {
        if !closingFromCore {
            // the person closed it: the core's pop-out goes too
            let p = popout
            Task { await p.close() }
        }
        onClosed?()
    }
}

@MainActor
final class PopoutViewController: NSViewController {
    let popout: PopoutModel
    let pictures: Pictures
    private let summaryLabel = NSTextField(wrappingLabelWithString: "")
    private let body = NSView()
    private var shown: PopoutValue?
    private var current: NSView?
    /// The whole text, once read, by the value's reference.
    private var fullText: [String: String] = [:]
    private var magnification: (() -> Void)?
    /// What the window shows now, for scripts (`--status-file`).
    private(set) var describe: [String: JSONValue] = [:]

    init(popout: PopoutModel, pictures: Pictures) {
        self.popout = popout
        self.pictures = pictures
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    override func loadView() {
        let v = NSView(frame: NSRect(x: 0, y: 0, width: 520, height: 440))
        summaryLabel.textColor = .secondaryLabelColor
        summaryLabel.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        summaryLabel.isSelectable = true
        for x in [summaryLabel, body] {
            x.translatesAutoresizingMaskIntoConstraints = false
            v.addSubview(x)
        }
        NSLayoutConstraint.activate([
            summaryLabel.topAnchor.constraint(equalTo: v.safeAreaLayoutGuide.topAnchor, constant: 10),
            summaryLabel.leadingAnchor.constraint(equalTo: v.leadingAnchor, constant: 14),
            summaryLabel.trailingAnchor.constraint(equalTo: v.trailingAnchor, constant: -14),
            body.topAnchor.constraint(equalTo: summaryLabel.bottomAnchor, constant: 8),
            body.leadingAnchor.constraint(equalTo: v.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: v.trailingAnchor),
            body.bottomAnchor.constraint(equalTo: v.bottomAnchor),
        ])
        view = v
        summaryLabel.stringValue = "Reading the value…"
        if let s = popout.value { show(s) }
    }

    /// Show `v`, the core's value and the viewer's state.
    func show(_ v: PopoutValue) {
        guard isViewLoaded else { return }
        let same = shown.map { $0.data == v.data && $0.picture == v.picture && $0.excerpt == v.excerpt && $0.value == v.value && $0.viewer == v.viewer } ?? false
        shown = v
        let w = v.watched
        summaryLabel.stringValue = ValueText.summary(w)
        describe = ["viewer": v.viewer.map(JSONValue.string) ?? .null, "node": .string(v.node), "port": .string(v.port),
                    "mono": .bool(v.text?.mono ?? false), "wrap": .bool(v.text?.wrap ?? true), "fit": v.fit.map(JSONValue.string) ?? .null]
        if same, current != nil {
            // only the viewer's state changed: follow it in place
            restyle?()
            magnification?()
            return
        }
        current?.removeFromSuperview()
        current = nil
        restyle = nil
        magnification = nil
        if let e = v.error, e != .null {
            put(message("This value failed: \(e.string ?? e["message"].string ?? e.text)"))
            describe["error"] = e
            return
        }
        switch v.viewer {
        case "picture"?: put(pictureView(v))
        case "text.plain"?: put(textView(v))
        case "scalar"?: put(scalarView(v))
        case nil: put(message(v.summary == nil ? "Run the graph to see this value." : "No value yet."))
        case let other?:
            put(message("\(other): no viewer yet in this app."))
            describe["unavailable"] = .string(other)
        }
    }

    private var restyle: (() -> Void)?

    /// The window's size changed (or it was first laid out): a fitted picture fits again.
    override func viewDidLayout() {
        super.viewDidLayout()
        if shown?.fit != "1:1" { magnification?() }
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

    private func bar(_ views: [NSView]) -> NSStackView {
        let bar = NSStackView(views: views)
        bar.orientation = .horizontal
        return bar
    }

    private func layout(_ box: NSView, bar: NSView, content: NSView) {
        for x in [content, bar] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: box.topAnchor),
            bar.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 14),
            bar.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -14),
            content.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 6),
            content.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            content.bottomAnchor.constraint(equalTo: box.bottomAnchor),
        ])
    }

    // MARK: picture

    private func pictureView(_ v: PopoutValue) -> NSView {
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
        // Fit and 1:1 are the core's (`viewer.zoom`), a segmented control as Preview's
        let zoom = NSSegmentedControl(labels: ["Fit", "1:1"], trackingMode: .selectOne, target: nil, action: nil)
        zoom.controlSize = .small
        zoom.setToolTip("Fit the picture to the window", forSegment: 0)
        zoom.setToolTip("One pixel of the picture to one of the screen", forSegment: 1)
        layout(box, bar: bar([info, NSView(), zoom]), content: scroll)
        let w = v.watched
        guard let pic = w.picture, let handle = pic.handle else {
            info.stringValue = "no picture"
            return box
        }
        info.stringValue = "\(pic.w) × \(pic.h) · rgba8\(w.media.map { " · \($0)" } ?? "") · loading…"
        let apply = { [weak self, weak scroll, weak image, weak zoom] in
            guard let scroll, let size = image?.image?.size, size.width > 0 else { return }
            let one = self?.shown?.fit == "1:1"
            zoom?.selectedSegment = one ? 1 : 0
            if one {
                scroll.magnification = 1
            } else {
                // fit the window, a small picture enlarged too (at most 8×), as the iPad's
                let s = min(scroll.contentSize.width / size.width, scroll.contentSize.height / size.height)
                scroll.magnification = max(min(s, 8), scroll.minMagnification)
            }
        }
        magnification = apply
        zoom.selectedSegment = v.fit == "1:1" ? 1 : 0
        let p = popout
        let pick = ClosureTarget { [weak zoom] in
            let to = zoom?.selectedSegment == 1 ? "1:1" : "fit"
            Task { await p.zoom(to) }
        }
        zoom.target = pick
        zoom.action = #selector(ClosureTarget.fire)
        objc_setAssociatedObject(box, &ClosureTarget.key, [pick], .OBJC_ASSOCIATION_RETAIN)
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
            apply()
            self?.describe["picture"] = ["w": .uint(UInt64(got.width)), "h": .uint(UInt64(got.height)), "loaded": true]
        }
        return box
    }

    // MARK: text.plain

    private func textView(_ v: PopoutValue) -> NSView {
        let box = NSView()
        let scroll = NSTextView.scrollableTextView()
        guard let tv = scroll.documentView as? NSTextView else { return box }
        tv.isEditable = false
        tv.isSelectable = true
        tv.textContainerInset = NSSize(width: 10, height: 8)
        let wrapBox = NSButton(checkboxWithTitle: "Wrap", target: nil, action: nil)
        let monoBox = NSButton(checkboxWithTitle: "Monospaced", target: nil, action: nil)
        for b in [wrapBox, monoBox] { b.controlSize = .small }
        let more = NSButton(title: "Show All", target: nil, action: nil)
        more.bezelStyle = .accessoryBarAction
        more.controlSize = .small
        let info = NSTextField(labelWithString: "")
        info.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        info.textColor = .secondaryLabelColor
        layout(box, bar: bar([info, NSView(), wrapBox, monoBox, more]), content: scroll)
        let w = v.watched
        let ex = w.excerpt
        let total = ValueText.excerpt(w).total
        let dataHandle = ValueText.dataHandle(w)
        // the core's viewer state: wrap, a monospaced face, all of it
        let style = { [weak self, weak tv, weak scroll, weak wrapBox, weak monoBox] in
            guard let self, let tv, let scroll else { return }
            let t = shown?.text
            let mono = t?.mono ?? false
            let wrap = t?.wrap ?? true
            monoBox?.state = mono ? .on : .off
            wrapBox?.state = wrap ? .on : .off
            tv.font = mono ? .monospacedSystemFont(ofSize: 12, weight: .regular) : .systemFont(ofSize: 13)
            scroll.hasHorizontalScroller = !wrap
            tv.isHorizontallyResizable = !wrap
            tv.textContainer?.widthTracksTextView = wrap
            tv.textContainer?.containerSize = NSSize(width: wrap ? scroll.contentSize.width : CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
            if !wrap { tv.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude) }
        }
        let whole = dataHandle.flatMap { fullText[$0] }
        tv.string = whole ?? ex?.text ?? ""
        let cut = whole == nil && total > (ex?.text.count ?? 0)
        info.stringValue = cut ? "first \(ex?.text.count ?? 0) of \(total) characters" : "\(total) characters"
        more.isHidden = !cut || dataHandle == nil
        describe["text"] = ["shown": .uint(UInt64(tv.string.count)), "total": .uint(UInt64(total))]
        let p = popout
        let fetchWhole = { [weak self] in
            guard let self, let h = dataHandle, fullText[h] == nil else { return }
            more.isEnabled = false
            info.stringValue = "reading the whole text…"
            Task {
                if let t = await ValueText.whole(self.popout.model, handle: h) {
                    self.fullText[h] = t
                    tv.string = t
                    info.stringValue = "\(t.count) characters"
                    more.isHidden = true
                    self.describe["text"] = ["shown": .uint(UInt64(t.count)), "total": .uint(UInt64(total))]
                } else {
                    info.stringValue = "the whole text couldn't be read"
                    more.isEnabled = true
                }
            }
        }
        restyle = { [weak self] in
            style()
            if self?.shown?.text?.all == true { fetchWhole() }
        }
        restyle?()
        let toggles = ClosureTarget { [weak wrapBox, weak monoBox] in
            Task { await p.text(wrap: wrapBox?.state == .on, mono: monoBox?.state == .on) }
        }
        for b in [wrapBox, monoBox] {
            b.target = toggles
            b.action = #selector(ClosureTarget.fire)
        }
        let all = ClosureTarget { Task { await p.text(all: true) } }
        more.target = all
        more.action = #selector(ClosureTarget.fire)
        objc_setAssociatedObject(box, &ClosureTarget.key, [toggles, all], .OBJC_ASSOCIATION_RETAIN)
        return box
    }

    // MARK: scalar

    private func scalarView(_ v: PopoutValue) -> NSView {
        let box = NSView()
        let w = v.watched
        let text = ValueText.scalar(w)
        let l = NSTextField(labelWithString: text)
        l.font = .monospacedDigitSystemFont(ofSize: 34, weight: .medium)
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
            l.centerYAnchor.constraint(equalTo: box.centerYAnchor, constant: -12),
            l.centerXAnchor.constraint(equalTo: box.centerXAnchor),
            l.leadingAnchor.constraint(greaterThanOrEqualTo: box.leadingAnchor, constant: 14),
            kind.topAnchor.constraint(equalTo: l.bottomAnchor, constant: 4),
            kind.centerXAnchor.constraint(equalTo: l.centerXAnchor),
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
