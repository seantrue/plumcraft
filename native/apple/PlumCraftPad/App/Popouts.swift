// Pop-out windows on the iPad (N4 slice 3; the maintainer, N3 exit card 14: "Eliminate the side
// column in favor of just the popout window"). The core keeps which values are popped out (the
// scene's `view.windows.popouts`); here each is shown as the system allows:
//
//   a sheet (the default)  over the graph, with medium and large detents, the graph still in
//                          sight above it at medium: one value at a time; another value opened
//                          takes its place (the one it replaces is closed in the core, so what is
//                          on screen is what the core has); Done (closing at once) or a swipe
//                          down is `view.close`
//   a window of its own   only when the person asks for one ("Open in New Window" in the
//                          sheet's bar): a scene per value, titled with the node and port, with a
//                          Close button in its own bar that destroys the scene at once and brings
//                          the graph's scene back to the front (`view.close`); the window's own
//                          controls close it too
//
// N4 slice 5, the maintainer: "No way to close popout or get back to the main canvas". Slice 3
// opened a scene of its own for every value whenever the graph's scene wasn't full screen, which
// on iPadOS 26's windowed apps is always; the system showed it maximised over the graph, and
// closing it left the graph's scene behind other windows. The sheet is now the default
// (slice 3's decision 3, answered by the maintainer's use), and a scene only on request.
//
// The viewers, plan 8's first three: `picture` (zoomable; Fit and 1:1 are the core's
// `viewer.zoom`, a double tap toggles them), `text.plain` (Wrap, Monospaced and Show All are the
// core's `viewer.text`), `scalar` (large, selectable); any other kind says "no viewer yet".

import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import SwiftUI
import UIKit

/// The pop-outs of one graph, kept in step with the core's.
@MainActor
final class PadPopouts: NSObject, UIAdaptivePresentationControllerDelegate {
    static let activityType = "com.plumcraft.pad.popout"
    let model: DocumentModel
    let pictures: Pictures
    weak var host: GraphViewController?
    private var known: [UInt64: PopoutWindow] = [:]
    private(set) var models: [UInt64: PopoutModel] = [:]
    /// The pop-outs shown in scenes of their own, by id.
    var scenes: [UInt64: UISceneSession] = [:]
    /// The pop-outs the person asked to see in a window of their own.
    private var asWindow: Set<UInt64> = []
    /// The one shown in a sheet.
    private(set) var sheet: (id: UInt64, nav: UINavigationController, vc: PopoutViewController)?
    var onChange: (() -> Void)?
    static let live = NSHashTable<PadPopouts>.weakObjects()

    init(model: DocumentModel, pictures: Pictures) {
        self.model = model
        self.pictures = pictures
        super.init()
        Self.live.add(self)
    }

    static func of(doc: UInt64) -> PadPopouts? {
        live.allObjects.first { $0.model.doc == doc }
    }

    func follow(_ w: Windows) {
        let d = PopoutDiff.of(known, w.list)
        known = Dictionary(uniqueKeysWithValues: w.list.map { ($0.id, $0) })
        for id in d.closed { closeFromCore(id) }
        for p in d.opened {
            let m = PopoutModel(model: model, window: p)
            models[p.id] = m
            m.refresh()
            present(m)
        }
        for p in d.changed { models[p.id]?.update(p) }
        for p in d.raised where p.raised > 0 { raise(p.id) }
        onChange?()
    }

    private func present(_ m: PopoutModel) {
        if asWindow.contains(m.id) {
            openScene(m.id)
        } else {
            showSheet(m)
        }
    }

    /// The graph's own scene to the front again (a pop-out's window closed over it).
    func bringGraphForward() {
        guard let s = host?.view.window?.windowScene?.session else { return }
        UIApplication.shared.activateSceneSession(for: UISceneSessionActivationRequest(session: s, userActivity: nil, options: nil)) { _ in }
    }

    /// The person closed a pop-out's window with its Close button: gone at once, the graph in
    /// front, and the core told.
    func closeWindow(_ id: UInt64) {
        if let s = scenes.removeValue(forKey: id) {
            UIApplication.shared.requestSceneSessionDestruction(s, options: nil) { _ in }
        }
        asWindow.remove(id)
        bringGraphForward()
        Task { await models[id]?.close() }
        onChange?()
    }

    func openScene(_ id: UInt64) {
        guard let doc = model.doc, let m = models[id] else { return }
        asWindow.insert(id)
        let a = NSUserActivity(activityType: Self.activityType)
        a.userInfo = ["doc": doc, "id": id]
        a.title = m.window.title
        let req = UISceneSessionActivationRequest(role: .windowApplication, userActivity: a, options: nil)
        UIApplication.shared.activateSceneSession(for: req) { [weak self] error in
            // refused (a system that won't open another window now): a sheet instead
            CoreHost.say("warn", "a pop-out's window was refused: \(error.localizedDescription)")
            if let self, let m = self.models[id] {
                self.asWindow.remove(id)
                self.showSheet(m)
            }
        }
    }

    private func showSheet(_ m: PopoutModel) {
        guard let host else { return }
        if let s = sheet, s.id != m.id {
            // one value at a time in a sheet: the one it replaces closes in the core too
            let old = s.id
            sheet = nil
            host.coreWindows?.willBeReplaced(s.nav)
            (s.nav.presentingViewController ?? s.nav).dismiss(animated: false)
            Task { await models[old]?.close() }
        }
        if sheet?.id == m.id { return }
        let vc = PopoutViewController(popout: m, pictures: pictures)
        vc.box.onDescribe = { [weak self] in self?.onChange?() }
        vc.navigationItem.title = m.window.title
        // Done closes at once, whatever the core is doing; the core is told after
        vc.navigationItem.rightBarButtonItem = UIBarButtonItem(systemItem: .done, primaryAction: UIAction { [weak self, weak m] _ in
            guard let self, let m else { return }
            if let s = sheet, s.id == m.id {
                sheet = nil
                host.coreWindows?.willBeReplaced(s.nav)
                (s.nav.presentingViewController ?? s.nav).dismiss(animated: !CoreHost.options.scratchState)
            }
            Task { await m.close() }
            onChange?()
        })
        vc.navigationItem.rightBarButtonItem?.accessibilityIdentifier = "popout-done"
        let more = UIAction(title: "Open in New Window", image: UIImage(systemName: "macwindow.badge.plus")) { [weak self] _ in
            guard let self else { return }
            if let n = sheet?.nav { host.coreWindows?.willBeReplaced(n) }
            sheet.map { $0.nav.presentingViewController ?? $0.nav }?.dismiss(animated: true)
            sheet = nil
            openScene(m.id)
        }
        // a window of its own only where there are windows (not on an iPhone, N4 slice 6)
        if UIApplication.shared.supportsMultipleScenes {
            vc.navigationItem.leftBarButtonItem = UIBarButtonItem(image: UIImage(systemName: "macwindow.badge.plus"), primaryAction: more)
            vc.navigationItem.leftBarButtonItem?.accessibilityLabel = "Open in New Window"
        }
        let nav = UINavigationController(rootViewController: vc)
        // opaque: a value is read, not seen through (iPadOS 26 draws a partial sheet as glass)
        nav.view.backgroundColor = .sheetGround
        nav.modalPresentationStyle = .pageSheet
        if let sp = nav.sheetPresentationController {
            sp.detents = [.medium(), .large()]
            sp.largestUndimmedDetentIdentifier = .medium
            sp.prefersGrabberVisible = true
        }
        nav.presentationController?.delegate = self
        nav.view.accessibilityIdentifier = "popout-sheet"
        sheet = (m.id, nav, vc)
        host.presentReplacing(nav)
        onChange?()
    }

    private func raise(_ id: UInt64) {
        if let s = scenes[id] {
            let req = UISceneSessionActivationRequest(session: s, userActivity: nil, options: nil)
            UIApplication.shared.activateSceneSession(for: req) { _ in }
        } else if let m = models[id], sheet?.id != id {
            showSheet(m)
        }
    }

    private func closeFromCore(_ id: UInt64) {
        models[id] = nil
        asWindow.remove(id)
        if let s = scenes.removeValue(forKey: id) {
            UIApplication.shared.requestSceneSessionDestruction(s, options: nil) { _ in }
            bringGraphForward()
        }
        if let s = sheet, s.id == id, let host {
            sheet = nil
            host.coreWindows?.willBeReplaced(s.nav)
            Presentations.whenSettled(from: host) { (s.nav.presentingViewController ?? s.nav).dismiss(animated: true) }
        }
    }

    /// The sheet was swiped down: the pop-out closes in the core.
    func presentationControllerDidDismiss(_ pc: UIPresentationController) {
        guard let s = sheet, pc.presentedViewController === s.nav else { return }
        sheet = nil
        Task { await models[s.id]?.close() }
        onChange?()
    }

    func closeAll() {
        for (_, s) in scenes { UIApplication.shared.requestSceneSessionDestruction(s, options: nil) { _ in } }
        scenes = [:]
        if let n = sheet?.nav { host?.coreWindows?.willBeReplaced(n) }
        sheet.map { $0.nav.presentingViewController ?? $0.nav }?.dismiss(animated: false)
        sheet = nil
    }

    var describe: JSONValue {
        .array(models.values.sorted { $0.id < $1.id }.map { m in
            var o: [String: JSONValue] = ["id": .uint(m.id), "title": .string(m.window.title), "viewer": m.value?.viewer.map(JSONValue.string) ?? .null,
                                          "shown": .string(scenes[m.id] != nil ? "scene" : (sheet?.id == m.id ? "sheet" : "none"))]
            if sheet?.id == m.id, let vc = sheet?.vc {
                o.merge(vc.describe) { a, _ in a }
                o.merge(vc.box.describe) { a, _ in a }
            }
            return .object(o)
        })
    }
}

/// A pop-out in a scene of its own: its window shows one value; closing it closes the pop-out.
@MainActor
final class PopoutSceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?
    var popout: PopoutModel?
    weak var popouts: PadPopouts?
    var closingFromCore = false

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let ws = scene as? UIWindowScene else { return }
        let activity = options.userActivities.first { $0.activityType == PadPopouts.activityType } ?? session.stateRestorationActivity
        let info = activity?.userInfo ?? [:]
        let w = UIWindow(windowScene: ws)
        guard let doc = (info["doc"] as? NSNumber)?.uint64Value ?? (info["doc"] as? UInt64), let id = (info["id"] as? NSNumber)?.uint64Value ?? (info["id"] as? UInt64),
              let pops = PadPopouts.of(doc: doc), let m = pops.models[id] else {
            // a pop-out whose graph or value went (the system restored its window after a
            // restart): nothing to show, so the scene goes, and the app is never left without
            // a window: a document browser takes its place when no other scene is showing
            let others = UIApplication.shared.connectedScenes.filter { $0 !== scene && $0.activationState != .unattached }
            UIApplication.shared.requestSceneSessionDestruction(session, options: nil) { _ in }
            if others.isEmpty {
                UIApplication.shared.activateSceneSession(for: UISceneSessionActivationRequest(role: .windowApplication)) { _ in }
            }
            return
        }
        popout = m
        popouts = pops
        pops.scenes[id] = session
        ws.title = m.window.title
        let vc = PopoutViewController(popout: m, pictures: pops.pictures)
        vc.navigationItem.title = m.window.title
        let nav = UINavigationController(rootViewController: vc)
        // its own Close, as the sheet's Done (the window's controls close it too): the scene goes
        // at once and the graph's comes back to the front
        vc.navigationItem.rightBarButtonItem = UIBarButtonItem(title: "Close", primaryAction: UIAction { [weak pops, weak m] _ in
            guard let pops, let m else { return }
            pops.closeWindow(m.id)
        })
        vc.navigationItem.rightBarButtonItem?.accessibilityIdentifier = "popout-done"
        w.rootViewController = nav
        w.makeKeyAndVisible()
        window = w
        // never restored: a pop-out is the document's view state, gone with the process
        session.stateRestorationActivity = nil
        pops.onChange?()
    }

    func sceneDidDisconnect(_ scene: UIScene) {
        guard let m = popout, let pops = popouts else { return }
        // the person closed the window (or the core did, which removed it first)
        if pops.scenes.removeValue(forKey: m.id) != nil {
            Task { await m.close() }
            pops.bringGraphForward()
        }
        pops.onChange?()
    }
}

@MainActor
final class PopoutViewController: UIViewController, UIScrollViewDelegate {
    let popout: PopoutModel
    let pictures: Pictures
    private let summaryLabel = UILabel()
    private let body = UIView()
    private var shown: PopoutValue?
    private var current: UIView?
    private var fullText: [String: String] = [:]
    private weak var imageView: UIImageView?
    private weak var imageScroll: UIScrollView?
    private var restyle: (() -> Void)?
    /// What it shows now, for scripts (`--status-file`).
    private(set) var describe: [String: JSONValue] = [:] { didSet { box.onDescribe?() } }
    /// The shared viewers' state (N4 slice 5): tables, outlines, grids, sound, bytes, items.
    let box: ViewerBox
    private var hosted: UIHostingController<ViewerView>?

    init(popout: PopoutModel, pictures: Pictures) {
        self.popout = popout
        self.pictures = pictures
        box = ViewerBox(popout: popout, pictures: pictures, muted: CoreHost.options.scratchState || CoreHost.options.statusFile != nil || CoreHost.options.statusElement)
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .sheetGround
        body.backgroundColor = .sheetGround
        summaryLabel.font = .preferredFont(forTextStyle: .footnote)
        summaryLabel.adjustsFontForContentSizeCategory = true
        summaryLabel.textColor = .secondaryLabel
        summaryLabel.numberOfLines = 2
        summaryLabel.text = "Reading the value…"
        // the summary keeps its own height: a viewer of a fixed height (the sound's) left the
        // label to take the sheet's spare height, an empty band with the line in its middle
        // (N4 slice 6); the viewer takes it instead
        summaryLabel.setContentHuggingPriority(.required, for: .vertical)
        summaryLabel.setContentCompressionResistancePriority(.required, for: .vertical)
        body.setContentHuggingPriority(.defaultLow - 1, for: .vertical)
        for x in [summaryLabel, body] {
            x.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(x)
        }
        let g = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            summaryLabel.topAnchor.constraint(equalTo: g.topAnchor, constant: 8),
            summaryLabel.leadingAnchor.constraint(equalTo: g.leadingAnchor, constant: 16),
            summaryLabel.trailingAnchor.constraint(equalTo: g.trailingAnchor, constant: -16),
            body.topAnchor.constraint(equalTo: summaryLabel.bottomAnchor, constant: 8),
            body.leadingAnchor.constraint(equalTo: g.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: g.trailingAnchor),
            body.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        view.accessibilityIdentifier = "popout"
        let prev = popout.onChange
        popout.onChange = { [weak self] v in
            prev?(v)
            self?.show(v)
        }
        if let v = popout.value { show(v) }
    }

    override func present(_ vc: UIViewController, animated: Bool, completion: (() -> Void)? = nil) {
        PopoverAnchor.ensure(vc, in: view)
        super.present(vc, animated: animated, completion: completion)
    }

    /// Show `v`, the core's value and the viewer's state.
    func show(_ v: PopoutValue) {
        guard isViewLoaded else { return }
        box.value = v
        navigationItem.title = v.title ?? popout.window.title
        if ViewerBox.handles(v), !(v.viewer == "array.pictures" && (v.views?.count ?? 0) <= 1), v.error == nil || v.error == .null {
            // a viewer the apps share: SwiftUI follows the value itself
            shown = v
            summaryLabel.text = ValueText.summary(v.watched)
            describe = ["viewer": v.viewer.map(JSONValue.string) ?? .null, "node": .string(v.node), "port": .string(v.port)]
            if hosted == nil || current !== hosted?.view {
                current?.removeFromSuperview()
                restyle = nil
                let h = UIHostingController(rootView: ViewerView(box: box))
                h.view.backgroundColor = .sheetGround
                addChild(h)
                hosted = h
                put(h.view)
                h.didMove(toParent: self)
            }
            return
        }
        let same = shown.map { $0.data == v.data && $0.picture == v.picture && $0.excerpt == v.excerpt && $0.value == v.value && $0.viewer == v.viewer } ?? false
        shown = v
        navigationItem.title = v.title ?? popout.window.title
        view.window?.windowScene?.title = navigationItem.title
        let w = v.watched
        summaryLabel.text = ValueText.summary(w)
        let kept = describe
        describe = ["viewer": v.viewer.map(JSONValue.string) ?? .null, "node": .string(v.node), "port": .string(v.port),
                    "mono": .bool(v.text?.mono ?? false), "wrap": .bool(v.text?.wrap ?? true), "fit": v.fit.map(JSONValue.string) ?? .null]
        // what the viewer drew stays said while only its state changes (the picture loaded, a reading)
        if same { for (key, x) in kept where describe[key] == nil { describe[key] = x } }
        if same, current != nil {
            restyle?()
            return
        }
        current?.removeFromSuperview()
        current = nil
        restyle = nil
        if let e = w.error, e != .null {
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
    private func withBar(_ content: UIView) -> UIView {
        let box = UIView()
        let h = UIHostingController(rootView: BarHost(box: self.box))
        guard let hv = h.view else { return content }
        hv.backgroundColor = .clear
        addChild(h)
        for x in [hv, content] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            hv.topAnchor.constraint(equalTo: box.topAnchor),
            hv.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            hv.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            content.topAnchor.constraint(equalTo: hv.bottomAnchor, constant: 2),
            content.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            content.bottomAnchor.constraint(equalTo: box.bottomAnchor),
        ])
        h.didMove(toParent: self)
        return box
    }

    private func put(_ v: UIView) {
        v.translatesAutoresizingMaskIntoConstraints = false
        v.setContentHuggingPriority(.defaultLow - 1, for: .vertical)
        body.addSubview(v)
        NSLayoutConstraint.activate([
            v.topAnchor.constraint(equalTo: body.topAnchor),
            v.leadingAnchor.constraint(equalTo: body.leadingAnchor),
            v.trailingAnchor.constraint(equalTo: body.trailingAnchor),
            v.bottomAnchor.constraint(equalTo: body.bottomAnchor),
        ])
        current = v
    }

    private func message(_ s: String) -> UIView {
        let box = UIView()
        let l = UILabel()
        l.text = s
        l.numberOfLines = 0
        l.font = .preferredFont(forTextStyle: .body)
        l.adjustsFontForContentSizeCategory = true
        l.textColor = .secondaryLabel
        l.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(l)
        NSLayoutConstraint.activate([
            l.topAnchor.constraint(equalTo: box.topAnchor, constant: 8),
            l.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16),
            l.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -16),
        ])
        return box
    }

    private func bar(_ views: [UIView]) -> UIStackView {
        let s = UIStackView(arrangedSubviews: views)
        s.axis = .horizontal
        s.spacing = 10
        s.alignment = .center
        return s
    }

    private func small(_ t: String) -> UILabel {
        let l = UILabel()
        l.text = t
        l.font = .preferredFont(forTextStyle: .caption1)
        l.adjustsFontForContentSizeCategory = true
        l.textColor = .secondaryLabel
        l.setContentHuggingPriority(.defaultLow, for: .horizontal)
        l.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return l
    }

    private func layout(_ box: UIView, bar: UIView, content: UIView) {
        for x in [bar, content] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            bar.topAnchor.constraint(equalTo: box.topAnchor),
            bar.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16),
            bar.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -16),
            content.topAnchor.constraint(equalTo: bar.bottomAnchor, constant: 8),
            content.leadingAnchor.constraint(equalTo: box.leadingAnchor),
            content.trailingAnchor.constraint(equalTo: box.trailingAnchor),
            content.bottomAnchor.constraint(equalTo: box.bottomAnchor),
        ])
    }

    // MARK: picture

    private func pictureView(_ v: PopoutValue) -> UIView {
        let box = UIView()
        let scroll = UIScrollView()
        scroll.backgroundColor = .tertiarySystemBackground
        scroll.minimumZoomScale = 0.05
        scroll.maximumZoomScale = 64
        scroll.delegate = self
        let image = UIImageView()
        image.contentMode = .scaleToFill
        image.layer.magnificationFilter = .nearest
        image.isUserInteractionEnabled = true
        scroll.addSubview(image)
        imageView = image
        imageScroll = scroll
        let info = small("")
        let reading = small("")
        reading.textColor = .label
        reading.accessibilityIdentifier = "popout-reading"
        // Fit and 1:1, the core's (`viewer.zoom`)
        let zoom = UISegmentedControl(items: ["Fit", "1:1"])
        zoom.selectedSegmentIndex = v.fit == "1:1" ? 1 : 0
        let p = popout
        zoom.addAction(UIAction { [weak zoom] _ in
            let to = zoom?.selectedSegmentIndex == 1 ? "1:1" : "fit"
            Task { await p.zoom(to) }
        }, for: .valueChanged)
        zoom.accessibilityIdentifier = "popout-zoom"
        // the channel shown, the core's (`viewer.picture`): a pull-down of the channels
        let channel = UIButton(configuration: .borderless())
        channel.showsMenuAsPrimaryAction = true
        channel.changesSelectionAsPrimaryAction = true
        let current = v.state?["picture"]["channel"].string ?? "rgba"
        channel.menu = UIMenu(children: PictureSource.channels.map { id, label in
            UIAction(title: label, state: id == current ? .on : .off) { _ in Task { await p.command("viewer.picture", ["channel": .string(id)]) } }
        })
        channel.accessibilityIdentifier = "popout-channel"
        channel.isHidden = v.viewer == "svg"
        let column = UIStackView(arrangedSubviews: [bar([info, channel, zoom]), reading])
        column.axis = .vertical
        column.spacing = 2
        layout(box, bar: column, content: scroll)
        let double = UITapGestureRecognizer(target: self, action: #selector(togglePictureZoom(_:)))
        double.numberOfTapsRequired = 2
        scroll.addGestureRecognizer(double)
        guard let first = PictureSource.of(v) else {
            info.text = "no picture"
            return box
        }
        let ps = PicState(src: first, shown: (Int(v.picture?.w ?? 0), Int(v.picture?.h ?? 0)))
        objc_setAssociatedObject(scroll, &PicState.key, ps, .OBJC_ASSOCIATION_RETAIN)
        let pics = pictures
        ps.set = { [weak image, weak self, weak ps] (img: CGImage) in
            ps?.original = img
            let c = self?.shown?.state?["picture"]["channel"].string ?? "rgba"
            Task.detached(priority: .userInitiated) {
                let shown = PictureSource.isolate(img, channel: c) ?? img
                await MainActor.run { image?.image = UIImage(cgImage: shown, scale: 1, orientation: .up) }
            }
        }
        restyle = { [weak self, weak zoom] in
            zoom?.selectedSegmentIndex = self?.shown?.fit == "1:1" ? 1 : 0
            if let img = ps.original { ps.set?(img) }
            self?.describe["channel"] = .string(self?.shown?.state?["picture"]["channel"].string ?? "rgba")
            self?.applyZoom()
        }
        // a tap reads the value's own numbers there (the core's, never the 8-bit picture's) and
        // pins the probe (`viewer.probe`)
        if v.viewer != "svg" {
            let tap = UITapGestureRecognizer(target: nil, action: nil)
            tap.require(toFail: double)
            let probe = ProbeTap { [weak self, weak image] g in
                guard let image else { return }
                let pt = g.location(in: image)
                let fw = image.bounds.width, fh = image.bounds.height
                guard fw > 0, fh > 0, let px = PictureSource.pixel(x: Double(pt.x / fw) * Double(ps.shown.w), y: Double(pt.y / fh) * Double(ps.shown.h), shown: ps.shown, full: ps.src.full) else { return }
                Task {
                    _ = try? await p.model.run("viewer.probe", ["popout": .uint(p.id), "row": .uint(px.row), "col": .uint(px.col)])
                    let r = await p.data("probe", ["row": .uint(px.row), "col": .uint(px.col)], timeout: .seconds(5))
                    reading.text = "row \(px.row), col \(px.col)  " + (r["text"].string ?? r["unavailable"].string ?? r["error"].string ?? "")
                    self?.describe["reading"] = r
                }
            }
            tap.addTarget(probe, action: #selector(ProbeTap.fire(_:)))
            objc_setAssociatedObject(image, &ProbeTap.key, probe, .OBJC_ASSOCIATION_RETAIN)
            image.addGestureRecognizer(tap)
        }
        // zoomed in past the picture's own pixels: more detail, once (egui's "scrolling in past
        // the picture asks for detail"): a picture file decoded larger, an SVG drawn larger, an
        // array's larger rendition from the core (`viewer.zoom {to: "detail"}`)
        let media = v.media ?? ""
        pastPixels = { [weak self, weak scroll, weak image] in
            guard let scroll, let image, !ps.sharpened, ps.shown.w > 0 else { return }
            guard scroll.zoomScale * (self?.view.window?.screen.scale ?? 2) > 1.01 else { return }
            if let sharper = ps.src.sharper(than: max(ps.shown.w, ps.shown.h), v: v) {
                ps.sharpened = true
                info.text = (info.text ?? "") + " · reading more detail…"
                Task {
                    if let got = await pics.load(sharper.handle, want: sharper.want, px: sharper.px) {
                        let ratio = CGFloat(ps.shown.w) / CGFloat(max(got.width, 1))
                        ps.src = PictureSource(handle: sharper.handle, want: sharper.want, px: sharper.px, full: ps.src.full)
                        ps.shown = (got.width, got.height)
                        let z = scroll.zoomScale
                        scroll.zoomScale = 1
                        image.frame = CGRect(x: 0, y: 0, width: got.width, height: got.height)
                        scroll.contentSize = image.frame.size
                        scroll.zoomScale = z * ratio
                        ps.set?(got.image)
                        info.text = "\(got.width) × \(got.height) px · detail"
                        self?.describe["detail"] = ["w": .uint(UInt64(got.width)), "h": .uint(UInt64(got.height))]
                    }
                }
            } else if media == "ndarray", let full = ps.src.full, max(full.w, full.h) > max(ps.shown.w, ps.shown.h) {
                ps.sharpened = true
                info.text = (info.text ?? "") + " · asking for detail…"
                self?.describe["detail"] = "asked"
                Task { await p.zoom("detail") }
            }
        }
        info.text = "\(ps.shown.w) × \(ps.shown.h) · loading…"
        describe["picture"] = ["w": .uint(UInt64(ps.shown.w)), "h": .uint(UInt64(ps.shown.h)), "loaded": false]
        Task { [weak self] in
            guard let got = await pics.load(ps.src.handle, want: ps.src.want, px: ps.src.px) else {
                info.text = pics.failures[ps.src.handle] ?? "couldn't load"
                self?.describe["failed"] = .string(pics.failures[ps.src.handle] ?? "couldn't load")
                return
            }
            ps.shown = (got.width, got.height)
            image.frame = CGRect(x: 0, y: 0, width: got.width, height: got.height)
            scroll.contentSize = image.frame.size
            ps.set?(got.image)
            image.image = UIImage(cgImage: got.image, scale: 1, orientation: .up)
            info.text = "\(got.width) × \(got.height) px\(ps.src.full.map { " of \($0.w) × \($0.h)" } ?? "") · \(ps.src.want == "svg" ? "SVG" : (v.media ?? "picture"))"
            self?.view.layoutIfNeeded()
            self?.applyZoom()
            self?.describe["picture"] = ["w": .uint(UInt64(got.width)), "h": .uint(UInt64(got.height)), "loaded": true]
        }
        return box
    }

    /// Called when a pinch ends past the picture's own pixels.
    private var pastPixels: (() -> Void)?

    func scrollViewDidEndZooming(_ scrollView: UIScrollView, with view: UIView?, atScale scale: CGFloat) { pastPixels?() }

    func viewForZooming(in scrollView: UIScrollView) -> UIView? { imageView }
    func scrollViewDidZoom(_ scrollView: UIScrollView) { centrePicture() }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        applyZoom()
    }

    private func centrePicture() {
        guard let s = imageScroll, let v = imageView else { return }
        let dx = max((s.bounds.width - v.frame.width) / 2, 0)
        let dy = max((s.bounds.height - v.frame.height) / 2, 0)
        s.contentInset = UIEdgeInsets(top: dy, left: dx, bottom: dy, right: dx)
    }

    /// The core's zoom: 1:1, else fit.
    private func applyZoom() {
        guard let s = imageScroll, let size = imageView?.image?.size, size.width > 0, size.height > 0, s.bounds.width > 0 else { return }
        if shown?.fit == "1:1" {
            s.setZoomScale(1, animated: false)
        } else {
            let z = min(s.bounds.width / size.width, s.bounds.height / size.height)
            s.setZoomScale(max(min(z, 8), s.minimumZoomScale), animated: false)
        }
        centrePicture()
    }

    @objc func togglePictureZoom(_ g: UITapGestureRecognizer) {
        let p = popout
        let to = shown?.fit == "1:1" ? "fit" : "1:1"
        Task { await p.zoom(to) }
    }

    // MARK: text.plain

    private func textView(_ v: PopoutValue) -> UIView {
        let box = UIView()
        let tv = UITextView()
        tv.isEditable = false
        tv.isSelectable = true
        tv.backgroundColor = .tertiarySystemBackground
        tv.textContainerInset = UIEdgeInsets(top: 10, left: 12, bottom: 10, right: 12)
        tv.adjustsFontForContentSizeCategory = true
        tv.accessibilityIdentifier = "popout-text"
        let w = v.watched
        let ex = ValueText.excerpt(w)
        let handle = ValueText.dataHandle(w)
        let info = small("")
        let p = popout
        let wrapSwitch = UISwitch()
        let monoSwitch = UISwitch()
        monoSwitch.accessibilityLabel = "Monospaced"
        wrapSwitch.accessibilityLabel = "Wrap"
        let wrapLabel = small("Wrap")
        let monoLabel = small("Monospaced")
        for l in [wrapLabel, monoLabel] { l.setContentHuggingPriority(.required, for: .horizontal) }
        let whole = handle.flatMap { fullText[$0] }
        tv.text = whole ?? ex.text
        let cut = whole == nil && ex.total > ex.shown
        info.text = cut ? "first \(ex.shown) of \(ex.total) characters" : "\(ex.total) characters"
        describe["text"] = ["shown": .uint(UInt64(tv.text.count)), "total": .uint(UInt64(ex.total))]
        let fetch = { [weak self] in
            guard let self, let handle, fullText[handle] == nil else { return }
            info.text = "reading the whole text…"
            Task {
                if let t = await ValueText.whole(self.popout.model, handle: handle) {
                    self.fullText[handle] = t
                    tv.text = t
                    info.text = "\(t.count) characters"
                    self.describe["text"] = ["shown": .uint(UInt64(t.count)), "total": .uint(UInt64(ex.total))]
                } else {
                    info.text = "the whole text couldn't be read"
                }
            }
        }
        restyle = { [weak self, weak tv, weak wrapSwitch, weak monoSwitch] in
            guard let self, let tv else { return }
            let t = shown?.text
            let mono = t?.mono ?? false
            let wrap = t?.wrap ?? true
            monoSwitch?.isOn = mono
            wrapSwitch?.isOn = wrap
            let body = UIFont.preferredFont(forTextStyle: .body)
            tv.font = mono ? UIFontMetrics(forTextStyle: .body).scaledFont(for: .monospacedSystemFont(ofSize: body.pointSize - 2, weight: .regular)) : body
            tv.textContainer.lineBreakMode = wrap ? .byWordWrapping : .byClipping
            tv.textContainer.widthTracksTextView = wrap
            if !wrap { tv.textContainer.size = CGSize(width: 100_000, height: CGFloat.greatestFiniteMagnitude) }
            if t?.all == true { fetch() }
        }
        restyle?()
        wrapSwitch.addAction(UIAction { [weak wrapSwitch] _ in Task { await p.text(wrap: wrapSwitch?.isOn ?? true) } }, for: .valueChanged)
        monoSwitch.addAction(UIAction { [weak monoSwitch] _ in Task { await p.text(mono: monoSwitch?.isOn ?? false) } }, for: .valueChanged)
        var views: [UIView] = [info, wrapLabel, wrapSwitch, monoLabel, monoSwitch]
        if cut, handle != nil {
            var c = UIButton.Configuration.bordered()
            c.title = "Show All"
            c.buttonSize = .small
            views.append(UIButton(configuration: c, primaryAction: UIAction { _ in Task { await p.text(all: true) } }))
        }
        layout(box, bar: bar(views), content: tv)
        return box
    }

    // MARK: scalar

    private func scalarView(_ v: PopoutValue) -> UIView {
        let box = UIView()
        let w = v.watched
        let text = ValueText.scalar(w)
        let tv = UITextView()
        tv.isEditable = false
        tv.isScrollEnabled = false
        tv.backgroundColor = .clear
        tv.textContainerInset = .zero
        tv.textContainer.lineFragmentPadding = 0
        let big = UIFont.monospacedDigitSystemFont(ofSize: 44, weight: .medium)
        tv.font = UIFontMetrics(forTextStyle: .largeTitle).scaledFont(for: big)
        tv.adjustsFontForContentSizeCategory = true
        tv.text = text
        tv.textAlignment = .center
        let kind = small(w.media ?? "scalar")
        for x in [tv, kind] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            tv.centerYAnchor.constraint(equalTo: box.centerYAnchor, constant: -20),
            tv.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16),
            tv.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -16),
            kind.topAnchor.constraint(equalTo: tv.bottomAnchor, constant: 4),
            kind.centerXAnchor.constraint(equalTo: box.centerXAnchor),
        ])
        describe["scalar"] = .string(text)
        return box
    }
}

/// A tap's action as a closure.
@MainActor
final class ProbeTap: NSObject {
    nonisolated(unsafe) static var key: UInt8 = 0
    let f: (UITapGestureRecognizer) -> Void
    init(_ f: @escaping (UITapGestureRecognizer) -> Void) { self.f = f }
    @objc func fire(_ g: UITapGestureRecognizer) { f(g) }
}

/// The viewers' bar for the value the box holds now.
struct BarHost: SwiftUI.View {
    @Bindable var box: ViewerBox
    var body: some SwiftUI.View {
        if let v = box.value { ViewerBar(box: box, value: v) }
    }
}

/// The Messages sheet (N4 slice 5): the status bar's messages in full, open while the core's
/// `view.windows.messages` is; Done or a swipe down closes it in the core too.
@MainActor
final class PadMessages: NSObject, UIAdaptivePresentationControllerDelegate {
    let messages: MessagesModel
    weak var host: GraphViewController?
    private var sheet: UIViewController?

    init(model: DocumentModel) {
        messages = MessagesModel(model: model)
    }

    func follow(_ open: Bool) {
        if open, sheet == nil, let host {
            let h = UIHostingController(rootView: MessagesView(messages: messages, done: { [weak self] in self?.closed() }))
            h.modalPresentationStyle = .formSheet
            h.view.backgroundColor = .sheetGround
            h.presentationController?.delegate = self
            h.view.accessibilityIdentifier = "messages"
            sheet = h
            host.presentReplacing(h)
        } else if !open, let s = sheet, let host {
            sheet = nil
            // once its presentation has run (a dismissal asked for before then is lost, B43)
            Presentations.whenSettled(from: host) { s.dismiss(animated: !CoreHost.options.scratchState) }
        }
    }

    /// Done: closed at once, the core told after.
    func closed() {
        if let s = sheet {
            sheet = nil
            s.dismiss(animated: true)
        }
        let m = messages
        Task { await m.close() }
    }

    func presentationControllerDidDismiss(_ pc: UIPresentationController) {
        sheet = nil
        let m = messages
        Task { await m.close() }
    }
}

/// The windows the core keeps and draws as a model (N4 slice 6): the Cache Report, the graph's
/// Parameters window, value editors (`report.get`) and Compare (`compare.get`), each a sheet
/// while the scene's `view.windows` says it is open (the newest shown: one presentation at a
/// time); Done or a swipe down closes it in the core too.
@MainActor
final class PadCoreWindows: NSObject, UIAdaptivePresentationControllerDelegate {
    let model: DocumentModel
    let pictures: Pictures
    weak var host: GraphViewController?
    private var open: [String] = []
    private var reports: [String: ReportModel] = [:]
    private var compare: CompareModel?
    private(set) var shown: (key: String, vc: UIViewController)?
    /// The sheet shown has been presented (its presentation ran): only then does its having no
    /// presenter mean something took it away (B43: one waiting to be presented has none yet).
    private var presented = false

    init(model: DocumentModel, pictures: Pictures) {
        self.model = model
        self.pictures = pictures
    }

    func follow(_ w: Windows) {
        // ours went with something under it (dismissed by another presentation): closed in the core
        if let s = shown, presented, s.vc.presentingViewController == nil, !s.vc.isBeingPresented {
            shown = nil
            tellClosed(s.key)
        }
        let keys = CoreWindowKeys.of(w)
        let added = keys.filter { !open.contains($0) }
        open = keys
        for k in reports.keys where !keys.contains(k) { reports[k] = nil }
        if !keys.contains("compare") { compare = nil }
        for (_, r) in reports { r.follow(w.reports) }
        compare?.follow(w.reports)
        if let s = shown, !keys.contains(s.key), let host {
            shown = nil
            Presentations.whenSettled(from: host) { s.vc.dismiss(animated: !CoreHost.options.scratchState) }
        }
        if let k = added.last { show(k, stamp: w.reports) }
        host?.reportNow()
    }

    private func show(_ key: String, stamp: UInt64?) {
        guard let host else { return }
        let vc: UIViewController
        if key == "compare" {
            let c = compare ?? CompareModel(model: model, pictures: pictures)
            compare = c
            c.follow(stamp)
            vc = UIHostingController(rootView: CompareView(compare: c, done: { [weak self] in self?.closed(key) }))
        } else {
            let r = reports[key] ?? ReportModel(model: model, key: key)
            reports[key] = r
            r.follow(stamp)
            vc = UIHostingController(rootView: ReportView(report: r, done: { [weak self] in self?.closed(key) }))
        }
        vc.view.backgroundColor = .sheetGround
        vc.modalPresentationStyle = .pageSheet
        if let sp = vc.sheetPresentationController {
            sp.detents = [.medium(), .large()]
            sp.prefersGrabberVisible = true
        }
        vc.presentationController?.delegate = self
        vc.view.accessibilityIdentifier = "core-\(key)"
        let old = shown
        shown = (key, vc)
        presented = false
        // one change at a time, once UIKit's transitions have run (B43): the one replaced goes
        // first, then this one over what is shown then (Settings, a value's sheet), as a window
        // over another on the Mac; closing it shows that again
        Presentations.whenSettled(from: host) { [weak self, weak host] in
            guard let self, let host else { return }
            if let o = old, o.vc !== vc, o.vc.presentingViewController != nil { o.vc.dismiss(animated: false) }
            // still wanted: the core may have closed it, or another replaced it, while it waited
            guard let s = self.shown, s.vc === vc else { return }
            Presentations.whenSettled(from: host) { [weak self, weak host] in
                guard let self, let host, let s = self.shown, s.vc === vc else { return }
                var top: UIViewController = host
                while let p = top.presentedViewController, !p.isBeingDismissed { top = p }
                top.present(vc, animated: !CoreHost.options.scratchState) { [weak self] in
                    guard let self, let s = self.shown, s.vc === vc else { return }
                    self.presented = true
                    self.host?.reportNow()
                }
            }
        }
    }

    /// Done: closed at once, the core told after.
    func closed(_ key: String) {
        if let s = shown, s.key == key {
            shown = nil
            s.vc.dismiss(animated: true)
        }
        tellClosed(key)
    }

    private func tellClosed(_ key: String) {
        if key == "compare" {
            let c = compare ?? CompareModel(model: model, pictures: pictures)
            Task { await c.close() }
        } else {
            let r = reports[key] ?? ReportModel(model: model, key: key)
            Task { await r.close() }
        }
    }

    func presentationControllerDidDismiss(_ pc: UIPresentationController) {
        guard let s = shown, pc.presentedViewController === s.vc else { return }
        shown = nil
        tellClosed(s.key)
    }

    // (a value's sheet is dismissed from its presenter, so a report over it goes with it: N4 slice 6)
    /// The graph is about to dismiss `vc` to show something else (a value's sheet, the palette):
    /// if it is ours, or ours is shown over it, it closes in the core too, so the core never keeps
    /// a window open that nothing shows (and asking for it again shows it).
    func willBeReplaced(_ vc: UIViewController) {
        guard let s = shown else { return }
        var v: UIViewController? = vc
        while let x = v {
            if x === s.vc {
                shown = nil
                tellClosed(s.key)
                return
            }
            v = x.presentedViewController
        }
    }

    var describe: JSONValue {
        var o: [String: JSONValue] = ["open": .array(open.map { .string($0) }), "shown": shown.map { .string($0.key) } ?? .null]
        if let s = shown {
            if let r = reports[s.key] { o["report"] = r.describe }
            if s.key == "compare", let c = compare { o["compare"] = c.describe }
        }
        return .object(o)
    }
}

extension UIColor {
    /// The ground of a sheet over the graph: opaque, light or dark (N4 slice 6). iPadOS 26 draws a
    /// partial-height sheet as glass and resolves `.systemBackground` inside it to see-through, so
    /// the graph showed blurred through a value's sheet in dark; a value is read, not seen through.
    static let sheetGround = UIColor { t in
        t.userInterfaceStyle == .dark ? UIColor(red: 0.110, green: 0.110, blue: 0.118, alpha: 1) : UIColor.white
    }
}
