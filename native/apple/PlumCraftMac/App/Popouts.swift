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
import SwiftUI
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
            c.content.box.onDescribe = { [weak self] in self?.onChange?() }
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
            o.merge(c.content.box.describe) { a, _ in a }
            o["id"] = .uint(c.popout.id)
            o["stamp"] = .uint(c.popout.window.stamp)
            o["window"] = .int(Int64(c.window?.windowNumber ?? 0))
            o["title"] = .string(c.window?.title ?? "")
            o["visible"] = .bool(c.window?.isVisible ?? false)
            // whether its bar offers Recipe (the value has one: a lineage-tracking engine's)
            o["hasRecipe"] = .bool(c.popout.value?.hasRecipe == true)
            o["node"] = .string(c.popout.window.node)
            o["port"] = .string(c.popout.window.port)
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
    /// The document's window, put back in front when the person closes this one.
    private weak var over: NSWindow?

    init(popout: PopoutModel, pictures: Pictures) {
        self.popout = popout
        content = PopoutViewController(popout: popout, pictures: pictures)
        let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 520, height: 440),
                         styleMask: [.titled, .closable, .miniaturizable, .resizable], backing: .buffered, defer: false)
        w.title = popout.window.title
        // `ui.screenshot {window: "popout:<id>"}` draws this window's content (B159)
        w.identifier = NSUserInterfaceItemIdentifier("core-popout:\(popout.id)")
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
        over = doc
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
            // the graph's window comes back to the front for the person at it
            if NSApp.isActive, !CoreHost.options.scratchState, let doc = over, doc.isVisible { doc.makeKeyAndOrderFront(nil) }
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
    /// B159: the picture's zoom and view a pinch, a pan or a zoom ask made, in the picture's own
    /// terms (`PictureZoom`); none: it fits, or shows 1:1 when the core says so.
    private var keptZoom: PictureZoom?
    /// The zoom ask (`zoomAsk.seq`) last taken, and one waiting for the picture to be laid out.
    private var askSeen: UInt64 = 0
    private var pendingAsk: JSONValue?
    /// What the window shows now, for scripts (`--status-file`).
    private(set) var describe: [String: JSONValue] = [:] { didSet { box.onDescribe?() } }
    /// The shared viewers' state (N4 slice 5): tables, outlines, grids, sound, bytes, items.
    let box: ViewerBox
    private var hosted: NSHostingView<ViewerView>?

    init(popout: PopoutModel, pictures: Pictures) {
        self.popout = popout
        self.pictures = pictures
        box = ViewerBox(popout: popout, pictures: pictures, muted: CoreHost.options.scratchState || CoreHost.options.statusFile != nil)
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
        box.value = v
        // the core's stamp for it: a change that leaves the value as it was (a chart chosen, an undo)
        // still draws again (the value set again unchanged drew nothing on the slice 7 Mac launch)
        box.stamp = popout.window.stamp
        // a zoom asked of the pop-out (`viewer.zoom`), taken once (B159)
        if let a = v.zoomAsk, let seq = a["seq"].uint64, seq > askSeen {
            askSeen = seq
            pendingAsk = a
        }
        if ViewerBox.handles(v), !(v.viewer == "array.pictures" && (v.views?.count ?? 0) <= 1), v.error == nil || v.error == .null {
            // a viewer the apps share: SwiftUI follows the value itself
            shown = v
            summaryLabel.stringValue = ValueText.summary(v.watched)
            describe = ["viewer": v.viewer.map(JSONValue.string) ?? .null, "node": .string(v.node), "port": .string(v.port)]
                .merging(ArrayStats.facts(v)) { a, _ in a }
            if hosted == nil || current !== hosted {
                current?.removeFromSuperview()
                restyle = nil
                magnification = nil
                let h = NSHostingView(rootView: ViewerView(box: box))
                hosted = h
                put(h)
            }
            return
        }
        let same = shown.map { $0.data == v.data && $0.picture == v.picture && $0.views == v.views && $0.excerpt == v.excerpt && $0.value == v.value && $0.viewer == v.viewer } ?? false
        shown = v
        let w = v.watched
        summaryLabel.stringValue = ValueText.summary(w)
        let kept = describe
        describe = ["viewer": v.viewer.map(JSONValue.string) ?? .null, "node": .string(v.node), "port": .string(v.port),
                    "mono": .bool(v.text?.mono ?? false), "wrap": .bool(v.text?.wrap ?? true), "fit": v.fit.map(JSONValue.string) ?? .null]
            .merging(ArrayStats.facts(v)) { a, _ in a }
        // what the viewer drew stays said while only its state changes (the picture loaded, a reading)
        if same { for (key, x) in kept where describe[key] == nil { describe[key] = x } }
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
        // B159: the zoom and view kept (`keptZoom`) outlive the picture's view, which is made
        // again for every item of a list: the next item shows at the same zoom and centre
        if let e = v.error, e != .null {
            put(message("This value failed: \(e.string ?? e["message"].string ?? e.text)"))
            describe["error"] = e
            return
        }
        switch v.viewer {
        case "picture"?, "svg"?, "array.pictures"?: put(withBar(pictureView(v)))
        case "text.plain"?: put(withBar(textView(v)))
        case "scalar"?: put(withBar(scalarView(v)))
        case nil: put(message(v.summary == nil ? "Run the graph to see this value." : "No value yet."))
        case let other?:
            put(message("\(other): no viewer yet in this app."))
            describe["unavailable"] = .string(other)
        }
    }

    /// The bar every viewer has (View As, Copy, Share, Save As) over a viewer of this app's own.
    private func withBar(_ content: NSView) -> NSView {
        let box = NSView()
        let bar = NSHostingView(rootView: BarHost(box: self.box))
        for x in [bar, content] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: box.topAnchor),
            bar.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            bar.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            content.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 2),
            content.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            content.bottomAnchor.constraint(equalTo: box.bottomAnchor),
        ])
        return box
    }

    private var restyle: (() -> Void)?

    /// The window's size changed (or it was first laid out): a fitted picture fits again.
    override func viewDidLayout() {
        super.viewDidLayout()
        if shown?.fit != "1:1" || keptZoom != nil || pendingAsk != nil { magnification?() }
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

    /// The view `scroll` shows of a picture `pic` value pixels in size (its document is laid out
    /// at that size, unflipped), in the picture's own terms; nil before it is laid out.
    static func viewShown(_ scroll: NSScrollView, _ pic: CGSize) -> PictureZoom? {
        let r = scroll.documentVisibleRect
        guard r.width > 0, r.height > 0, pic.width > 0, pic.height > 0 else { return nil }
        let c = CGPoint(x: min(max(r.midX / pic.width, 0), 1), y: min(max(1 - r.midY / pic.height, 0), 1))
        return PictureZoom(scale: Double(scroll.magnification), centre: c)
    }

    private func pictureView(_ v: PopoutValue) -> NSView {
        let box = NSView()
        let scroll = NSScrollView()
        scroll.contentView = CenteringClipView()
        scroll.allowsMagnification = true
        scroll.minMagnification = 0.05
        scroll.maxMagnification = 64
        scroll.hasVerticalScroller = true
        scroll.hasHorizontalScroller = true
        scroll.drawsBackground = true
        scroll.backgroundColor = .underPageBackgroundColor
        let image = ProbeImageView()
        image.imageScaling = .scaleAxesIndependently
        image.imageAlignment = .alignCenter
        scroll.documentView = image
        let info = NSTextField(labelWithString: "")
        info.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        info.textColor = .secondaryLabelColor
        info.lineBreakMode = .byTruncatingTail
        info.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        let reading = NSTextField(labelWithString: "")
        reading.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
        reading.lineBreakMode = .byTruncatingTail
        reading.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        // the channel shown is the core's (`viewer.picture`)
        let channel = NSPopUpButton(frame: .zero, pullsDown: false)
        channel.controlSize = .small
        for (_, label) in PictureSource.channels { channel.addItem(withTitle: label) }
        channel.isHidden = v.viewer == "svg"
        // Fit and 1:1 are the core's (`viewer.zoom`), a segmented control as Preview's
        let zoom = NSSegmentedControl(labels: ["Fit", "1:1"], trackingMode: .selectOne, target: nil, action: nil)
        zoom.controlSize = .small
        zoom.setToolTip("Fit the picture to the window", forSegment: 0)
        zoom.setToolTip("One pixel of the picture to one of the screen", forSegment: 1)
        let column = NSStackView(views: [bar([info, NSView(), channel, zoom]), bar([reading])])
        column.orientation = .vertical
        column.alignment = .leading
        column.spacing = 2
        layout(box, bar: column, content: scroll)
        guard let first = PictureSource.of(v) else {
            info.stringValue = "no picture"
            return box
        }
        let p = popout
        let ps = PicState(src: first, shown: (Int(v.picture?.w ?? 0), Int(v.picture?.h ?? 0)))
        objc_setAssociatedObject(scroll, &PicState.key, ps, .OBJC_ASSOCIATION_RETAIN)
        let pics = pictures
        let media = v.media ?? ""
        let apply: @MainActor @Sendable () -> Void = { [weak self, weak scroll, weak image, weak zoom] in
            guard let self, let scroll, let image, ps.shown.w > 0 else { return }
            let one = self.shown?.fit == "1:1"
            zoom?.selectedSegment = one ? 1 : 0
            // the picture is laid out at the value's own size, so 1:1 is a pixel of the value
            let f = ps.src.full ?? ps.shown
            let pic = CGSize(width: f.w, height: f.h)
            image.frame = NSRect(origin: .zero, size: pic)
            let fit = PictureZoom.fit(content: scroll.contentSize, picture: pic)
            // a zoom asked of the pop-out, taken once the view's size is known (B159)
            if let a = self.pendingAsk, let fit {
                self.pendingAsk = nil
                let now = self.keptZoom ?? Self.viewShown(scroll, pic) ?? PictureZoom(scale: one ? 1 : fit, centre: CGPoint(x: 0.5, y: 0.5))
                self.keptZoom = PictureZoom.asked(a, current: now, backing: Double(scroll.window?.backingScaleFactor ?? 2), picture: pic)
            }
            // Fit and 1:1 lit only when shown: a kept zoom is neither
            if self.keptZoom != nil { zoom?.selectedSegment = -1 }
            if let k = self.keptZoom, fit != nil {
                // the zoom and view kept, in the picture's own terms: the same scale, the same
                // relative centre (AppKit's y runs up)
                scroll.magnification = k.scale
                let clip = scroll.contentView
                let vis = clip.bounds.size
                clip.scroll(to: clip.constrainBoundsRect(NSRect(origin: NSPoint(x: k.centre.x * pic.width - vis.width / 2, y: (1 - k.centre.y) * pic.height - vis.height / 2), size: vis)).origin)
                scroll.reflectScrolledClipView(clip)
            } else if self.keptZoom != nil {
                return  // kept, and not laid out yet: applied when it is
            } else if one {
                scroll.magnification = 1
            } else if let fit {
                scroll.magnification = max(fit, scroll.minMagnification)
            } else {
                // B159: not laid out yet (a picture already decoded arrives before its new view
                // has a size): fitting now gave the smallest zoom (5%); the size's arrival applies it
                return
            }
            self.describe["zoom"] = (Self.viewShown(scroll, pic) ?? PictureZoom(scale: scroll.magnification, centre: CGPoint(x: 0.5, y: 0.5)))
                .facts(kept: self.keptZoom != nil, fit: fit)
        }
        magnification = apply
        // the view's size arriving (its first layout, a resize) applies the zoom (B159)
        scroll.postsFrameChangedNotifications = true
        NotificationCenter.default.addObserver(forName: NSView.frameDidChangeNotification, object: scroll, queue: .main) { _ in
            MainActor.assumeIsolated { apply() }
        }
        zoom.selectedSegment = v.fit == "1:1" ? 1 : 0
        ps.set = { [weak image, weak channel, weak self, weak ps] (img: CGImage) in
            ps?.original = img
            let c = self?.shown?.state?["picture"]["channel"].string ?? "rgba"
            channel?.selectItem(at: PictureSource.channels.firstIndex { $0.0 == c } ?? 0)
            Task.detached(priority: .userInitiated) {
                let shown = PictureSource.isolate(img, channel: c) ?? img
                await MainActor.run { image?.image = NSImage(cgImage: shown, size: NSSize(width: img.width, height: img.height)) }
            }
        }
        restyle = { [weak self] in
            guard let img = ps.original else { return }
            ps.set?(img)
            self?.describe["channel"] = .string(self?.shown?.state?["picture"]["channel"].string ?? "rgba")
        }
        let pick = ClosureTarget { [weak zoom] in
            let to = zoom?.selectedSegment == 1 ? "1:1" : "fit"
            Task { await p.zoom(to) }
        }
        zoom.target = pick
        zoom.action = #selector(ClosureTarget.fire)
        let chosen = ClosureTarget { [weak channel] in
            let i = channel?.indexOfSelectedItem ?? 0
            Task { await p.command("viewer.picture", ["channel": .string(PictureSource.channels[max(0, min(i, PictureSource.channels.count - 1))].0)]) }
        }
        channel.target = chosen
        channel.action = #selector(ClosureTarget.fire)
        objc_setAssociatedObject(box, &ClosureTarget.key, [pick, chosen], .OBJC_ASSOCIATION_RETAIN)
        // the readout: the value's own numbers under the pointer, read by the core (never from
        // the 8-bit picture); a click pins the probe (`viewer.probe`)
        let pixelAt = { (pt: NSPoint) -> (row: UInt64, col: UInt64)? in
            let f = ps.src.full ?? ps.shown
            // the view is flipped neither way: AppKit's y runs up
            return PictureSource.pixel(x: Double(pt.x) / Double(f.w) * Double(ps.shown.w), y: Double(CGFloat(f.h) - pt.y) / Double(f.h) * Double(ps.shown.h),
                                       shown: ps.shown, full: ps.src.full)
        }
        if v.viewer != "svg" {
            image.onMove = { [weak self] pt in
                guard let px = pixelAt(pt), Date().timeIntervalSince(ps.lastAsk) > 0.05 else { return }
                ps.lastAsk = Date()
                Task {
                    let r = await p.data("probe", ["row": .uint(px.row), "col": .uint(px.col)], timeout: .seconds(5))
                    reading.stringValue = "row \(px.row), col \(px.col)  " + (r["text"].string ?? r["unavailable"].string ?? r["error"].string ?? "")
                    self?.describe["reading"] = r
                }
            }
            image.onClick = { pt in
                guard let px = pixelAt(pt) else { return }
                Task { _ = try? await p.model.run("viewer.probe", ["popout": .uint(p.id), "row": .uint(px.row), "col": .uint(px.col)]) }
            }
        }
        // zoomed in past the picture's own pixels: more detail is asked for, once (egui's
        // "scrolling in past the picture asks for detail"): a picture file decoded larger, an SVG
        // drawn larger, an array's larger rendition from the core (`viewer.zoom {to: "detail"}`)
        // a pinch or a pan a person ends is the view kept through the items (B159)
        let keep: @MainActor @Sendable (Bool) -> Void = { [weak self, weak scroll] always in
            guard let self, let scroll, ps.shown.w > 0, always || self.keptZoom != nil else { return }
            let f = ps.src.full ?? ps.shown
            let pic = CGSize(width: f.w, height: f.h)
            self.keptZoom = Self.viewShown(scroll, pic)
            self.describe["zoom"] = self.keptZoom?.facts(kept: true, fit: PictureZoom.fit(content: scroll.contentSize, picture: pic))
        }
        NotificationCenter.default.addObserver(forName: NSScrollView.didEndLiveScrollNotification, object: scroll, queue: .main) { _ in
            MainActor.assumeIsolated { keep(false) }
        }
        NotificationCenter.default.addObserver(forName: NSScrollView.didEndLiveMagnifyNotification, object: scroll, queue: .main) { [weak self, weak scroll] _ in
            MainActor.assumeIsolated {
                keep(true)
                guard let scroll, !ps.sharpened, ps.shown.w > 0 else { return }
                let f = ps.src.full ?? ps.shown
                let screenPerShown = scroll.magnification * CGFloat(f.w) / CGFloat(ps.shown.w) * (scroll.window?.backingScaleFactor ?? 2)
                guard screenPerShown > 1.01 else { return }
                if let sharper = ps.src.sharper(than: max(ps.shown.w, ps.shown.h), v: v) {
                    ps.sharpened = true
                    info.stringValue += " · reading more detail…"
                    Task {
                        if let got = await pics.load(sharper.handle, want: sharper.want, px: sharper.px) {
                            ps.src = PictureSource(handle: sharper.handle, want: sharper.want, px: sharper.px, full: ps.src.full)
                            ps.shown = (got.width, got.height)
                            ps.set?(got.image)
                            info.stringValue = "\(got.width) × \(got.height) px · \(sharper.want == "svg" ? "drawn" : "decoded") · detail"
                            self?.describe["detail"] = ["w": .uint(UInt64(got.width)), "h": .uint(UInt64(got.height))]
                        }
                    }
                } else if media == "ndarray", let full = ps.src.full, max(full.w, full.h) > max(ps.shown.w, ps.shown.h) {
                    ps.sharpened = true
                    info.stringValue += " · asking for detail…"
                    self?.describe["detail"] = "asked"
                    Task { await p.zoom("detail") }
                }
            }
        }
        info.stringValue = "\(ps.shown.w) × \(ps.shown.h) · loading…"
        describe["picture"] = ["w": .uint(UInt64(ps.shown.w)), "h": .uint(UInt64(ps.shown.h)), "loaded": false, "handle": .string(ps.src.handle)]
        Task { [weak self] in
            // decoded on a worker (Pictures); only setting it here
            guard let got = await pics.load(ps.src.handle, want: ps.src.want, px: ps.src.px) else {
                info.stringValue = "\(pics.failures[ps.src.handle] ?? "couldn't load")"
                self?.describe["failed"] = .string(pics.failures[ps.src.handle] ?? "couldn't load")
                return
            }
            ps.shown = (got.width, got.height)
            ps.set?(got.image)
            let f = ps.src.full
            info.stringValue = "\(got.width) × \(got.height) px\(f.map { " of \($0.w) × \($0.h)" } ?? "") · \(ps.src.want == "svg" ? "SVG" : (v.media ?? "picture"))"
            apply()
            self?.describe["picture"] = ["w": .uint(UInt64(got.width)), "h": .uint(UInt64(got.height)), "loaded": true, "handle": .string(ps.src.handle)]
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

/// A picture view that says where the pointer is and where a click falls (the readout, the
/// pinned probe), in its own coordinates.
final class ProbeImageView: NSImageView {
    var onMove: ((NSPoint) -> Void)?
    var onClick: ((NSPoint) -> Void)?
    private var area: NSTrackingArea?

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let a = area { removeTrackingArea(a) }
        let a = NSTrackingArea(rect: bounds, options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(a)
        area = a
    }

    override func mouseMoved(with event: NSEvent) {
        onMove?(convert(event.locationInWindow, from: nil))
    }

    override func mouseDown(with event: NSEvent) {
        if event.clickCount == 1 { onClick?(convert(event.locationInWindow, from: nil)) }
        super.mouseDown(with: event)
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

/// The viewers' bar for the value the box holds now.
struct BarHost: SwiftUI.View {
    @Bindable var box: ViewerBox
    var body: some SwiftUI.View {
        if let v = box.value {
            VStack(spacing: 0) {
                ViewerBar(box: box, value: v)
                // an array's channels and their histograms (B154 slice 1), above its picture
                if v.viewer == "array.pictures", ArrayStats.shows(v) { ArrayStats(box: box, value: v) }
            }
        }
    }
}

/// The Messages window (N4 slice 5): the status bar's messages in full, open while the core's
/// `view.windows.messages` is (`view.messages`); its close button closes it in the core too.
@MainActor
final class MessagesWindow: NSObject, NSWindowDelegate {
    let messages: MessagesModel
    private(set) var window: NSWindow?
    private var closingFromCore = false

    init(model: DocumentModel) {
        messages = MessagesModel(model: model)
    }

    func follow(_ open: Bool, over doc: NSWindow?) {
        if open, window == nil {
            let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 460, height: 320), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
            w.title = "Messages"
            w.contentView = NSHostingView(rootView: MessagesView(messages: messages))
            w.isReleasedWhenClosed = false
            w.collectionBehavior.insert(.fullScreenAuxiliary)
            w.delegate = self
            w.center()
            window = w
            // in front for the person at the app; else just above the document's window
            if NSApp.isActive && !CoreHost.options.scratchState {
                w.makeKeyAndOrderFront(nil)
            } else if let doc, doc.isVisible {
                w.order(.above, relativeTo: doc.windowNumber)
            } else {
                w.orderBack(nil)
            }
        } else if !open, let w = window {
            closingFromCore = true
            w.close()
            closingFromCore = false
        }
    }

    func windowWillClose(_ n: Notification) {
        if !closingFromCore {
            let m = messages
            Task { await m.close() }
        }
        window = nil
    }

    var describe: JSONValue {
        ["open": .bool(window != nil), "window": .int(Int64(window?.windowNumber ?? 0)), "count": .uint(UInt64(messages.messages.count))]
    }
}

/// The Acknowledgements window (FR4): the licences of what the app and its engines are built
/// from, open while the core's `view.windows.acknowledgements` is (`app.acknowledgements`,
/// beside About in the application menu); its close button closes it in the core too.
@MainActor
final class AcknowledgementsWindow: NSObject, NSWindowDelegate {
    let acks: AcknowledgementsModel
    /// The status file is written again (what the window shows changed).
    var onChange: (() -> Void)?
    private(set) var window: NSWindow?
    private var closingFromCore = false

    init(model: DocumentModel) {
        acks = AcknowledgementsModel(model: model, shell: "mac")
    }

    func follow(_ open: Bool, over doc: NSWindow?) {
        if open, window == nil {
            let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 620), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
            w.title = "Acknowledgements"
            // a script's `ui.screenshot {window: "acknowledgements"}` draws it
            w.identifier = NSUserInterfaceItemIdentifier("core-acknowledgements")
            w.contentView = NSHostingView(rootView: AcknowledgementsView(acks: acks))
            acks.onRead = { [weak self] in self?.onChange?() }
            w.isReleasedWhenClosed = false
            w.collectionBehavior.insert(.fullScreenAuxiliary)
            w.delegate = self
            w.center()
            window = w
            // in front for the person at the app; else just above the document's window
            if NSApp.isActive && !CoreHost.options.scratchState {
                w.makeKeyAndOrderFront(nil)
            } else if let doc, doc.isVisible {
                w.order(.above, relativeTo: doc.windowNumber)
            } else {
                w.orderBack(nil)
            }
        } else if !open, let w = window {
            closingFromCore = true
            w.close()
            closingFromCore = false
        }
    }

    func windowWillClose(_ n: Notification) {
        if !closingFromCore {
            let a = acks
            Task { await a.close() }
        }
        window = nil
    }

    var describe: JSONValue {
        var d = acks.describe.object ?? [:]
        d["open"] = .bool(window != nil)
        d["window"] = .int(Int64(window?.windowNumber ?? 0))
        return .object(d)
    }
}

/// A core window's undo manager: the document's undo history, the core's. With a SwiftUI window
/// in front (the curves editor's) Edit › Undo and ⌘Z reach the window, which answers them with its
/// undo manager: AppKit's own, empty, so Undo was greyed after a drag and did nothing. This one
/// says what the core can undo, under the core's name for it, and asks the core to.
final class CoreUndoManager: UndoManager {
    private weak var model: DocumentModel?

    init(model: DocumentModel) {
        self.model = model
        super.init()
    }

    private func onMain<T: Sendable>(_ f: @MainActor (DocumentModel?) -> T) -> T {
        MainActor.assumeIsolated { f(model) }
    }

    override var canUndo: Bool { onMain { $0?.state.undo != nil } }
    override var canRedo: Bool { onMain { $0?.state.redo != nil } }
    override var undoActionName: String { onMain { $0?.state.undo ?? "" } }
    override var redoActionName: String { onMain { $0?.state.redo ?? "" } }

    override func undo() {
        onMain { m in
            guard let m else { return }
            Task { _ = try? await m.run("edit.undo") }
        }
    }

    override func redo() {
        onMain { m in
            guard let m else { return }
            Task { _ = try? await m.run("edit.redo") }
        }
    }
}

/// The windows the core keeps and draws as a model (N4 slice 6): the Cache Report, the graph's
/// Parameters window, value editors (`report.get`) and Compare (`compare.get`), each a window of
/// its own while the scene's `view.windows` says it is open; its close button closes it in the
/// core too.
@MainActor
final class CoreWindows: NSObject, NSWindowDelegate, UndoActions {
    let model: DocumentModel
    let pictures: Pictures
    private(set) var windows: [String: NSWindow] = [:]
    private(set) var reports: [String: ReportModel] = [:]
    private(set) var compare: CompareModel?
    private var closingFromCore = false

    init(model: DocumentModel, pictures: Pictures) {
        self.model = model
        self.pictures = pictures
    }

    func follow(_ w: Windows, over doc: NSWindow?) {
        let keys = CoreWindowKeys.of(w)
        for k in keys where windows[k] == nil { open(k, over: doc) }
        for (k, win) in windows where !keys.contains(k) {
            closingFromCore = true
            win.close()
            closingFromCore = false
            windows[k] = nil
            reports[k] = nil
            if k == "compare" { compare = nil }
        }
        for (_, r) in reports { r.follow(w.reports) }
        compare?.follow(w.reports)
    }

    private func open(_ key: String, over doc: NSWindow?) {
        let content: NSView
        let title: String
        var size = NSSize(width: 520, height: 560)
        if key == "compare" {
            let c = CompareModel(model: model, pictures: pictures)
            compare = c
            content = NSHostingView(rootView: CompareView(compare: c))
            title = "Compare"
            size = NSSize(width: 820, height: 520)
        } else {
            let r = ReportModel(model: model, key: key)
            reports[key] = r
            // a value editor: the native one for its kind, else the core's value window
            content = key.hasPrefix("editor:") ? NSHostingView(rootView: AnyView(ValueEditorView(report: r, pictures: pictures))) : NSHostingView(rootView: AnyView(ReportView(report: r)))
            title = r.title
            if key == "params" { size = NSSize(width: 460, height: 420) }
        }
        let w = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: [.titled, .closable, .resizable, .miniaturizable], backing: .buffered, defer: false)
        w.title = title
        w.identifier = NSUserInterfaceItemIdentifier("core-\(key)")
        w.contentView = content
        w.isReleasedWhenClosed = false
        w.collectionBehavior.insert(.fullScreenAuxiliary)
        w.delegate = self
        w.center()
        windows[key] = w
        // in front for the person at the app; else just above the document's window
        if NSApp.isActive && !CoreHost.options.scratchState {
            w.makeKeyAndOrderFront(nil)
        } else if let doc, doc.isVisible {
            w.order(.above, relativeTo: doc.windowNumber)
        } else {
            w.orderBack(nil)
        }
    }

    // Undo and Redo in a window the core keeps (a value editor, Parameters, the Cache Report):
    // the window's delegate is in the responder chain, so ⌘Z there undoes the document (S4b:
    // a mask stroke undone from the mask window), and the menu bar shows that document's undo
    func undo(_ sender: Any?) {
        let m = model
        Task { _ = try? await m.run("edit.undo") }
    }

    func redo(_ sender: Any?) {
        let m = model
        Task { _ = try? await m.run("edit.redo") }
    }

    func windowDidBecomeKey(_ n: Notification) {
        MenuBar.shared.show(model)
    }

    private lazy var undoManager = CoreUndoManager(model: model)

    /// The document's undo history for whatever in the window asks for it (see CoreUndoManager)
    func windowWillReturnUndoManager(_ window: NSWindow) -> UndoManager? {
        undoManager
    }

    func windowWillClose(_ n: Notification) {
        guard let w = n.object as? NSWindow, let key = windows.first(where: { $0.value === w })?.key else { return }
        if !closingFromCore {
            if key == "compare", let c = compare {
                Task { await c.close() }
            } else if let r = reports[key] {
                Task { await r.close() }
            }
        }
        windows[key] = nil
        reports[key] = nil
        if key == "compare" { compare = nil }
        // the document's window back in front for a person at the app
        if NSApp.isActive && !CoreHost.options.scratchState { NSApp.mainWindow?.makeKeyAndOrderFront(nil) }
    }

    /// Close every window (the document closed).
    func closeAll() {
        closingFromCore = true
        for (_, w) in windows { w.close() }
        closingFromCore = false
        windows = [:]
        reports = [:]
        compare = nil
    }

    var describe: JSONValue {
        var o: [String: JSONValue] = [:]
        for (k, w) in windows {
            var d: [String: JSONValue] = ["window": .int(Int64(w.windowNumber)), "title": .string(w.title)]
            if let r = reports[k] { d["report"] = r.describe }
            if k == "compare", let c = compare { d["compare"] = c.describe }
            o[k] = .object(d)
        }
        return .object(o)
    }
}
