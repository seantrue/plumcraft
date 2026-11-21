// Pop-out windows on the iPad (N4 slice 3; the maintainer, N3 exit card 14: "Eliminate the side
// column in favor of just the popout window"). The core keeps which values are popped out (the
// scene's `view.windows.popouts`); here each is shown as the system allows:
//
//   a window of its own   where the graph's scene isn't full screen (Stage Manager, windowed
//                          apps, Split View, an external display): a scene per value, requested
//                          with its pop-out as the activity, titled with the node and port, closed
//                          with the window's own controls (`view.close`) or by the core
//   a sheet               where the graph's scene is full screen (or a new scene is refused): one
//                          value at a time, with medium and large detents; another value opened
//                          takes its place (the one it replaces is closed in the core, so what is
//                          on screen is what the core has); swiped down or Done is `view.close`;
//                          "Open in New Window" asks for a scene for it
//
// The viewers, plan 8's first three: `picture` (zoomable; Fit and 1:1 are the core's
// `viewer.zoom`, a double tap toggles them), `text.plain` (Wrap, Monospaced and Show All are the
// core's `viewer.text`), `scalar` (large, selectable); any other kind says "no viewer yet".

import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
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

    /// Does the graph's scene leave room for another window (not full screen)?
    var windowed: Bool {
        guard let scene = host?.view.window?.windowScene else { return false }
        return !scene.isFullScreen
    }

    private func present(_ m: PopoutModel) {
        if windowed {
            openScene(m.id)
        } else {
            showSheet(m)
        }
    }

    func openScene(_ id: UInt64) {
        guard let doc = model.doc, let m = models[id] else { return }
        let a = NSUserActivity(activityType: Self.activityType)
        a.userInfo = ["doc": doc, "id": id]
        a.title = m.window.title
        let req = UISceneSessionActivationRequest(role: .windowApplication, userActivity: a, options: nil)
        UIApplication.shared.activateSceneSession(for: req) { [weak self] error in
            // refused (a system that won't open another window now): a sheet instead
            CoreHost.say("warn", "a pop-out's window was refused: \(error.localizedDescription)")
            if let self, let m = self.models[id] { self.showSheet(m) }
        }
    }

    private func showSheet(_ m: PopoutModel) {
        guard let host else { return }
        if let s = sheet, s.id != m.id {
            // one value at a time in a sheet: the one it replaces closes in the core too
            let old = s.id
            sheet = nil
            s.nav.dismiss(animated: false)
            Task { await models[old]?.close() }
        }
        if sheet?.id == m.id { return }
        let vc = PopoutViewController(popout: m, pictures: pictures)
        vc.navigationItem.title = m.window.title
        vc.navigationItem.rightBarButtonItem = UIBarButtonItem(systemItem: .done, primaryAction: UIAction { [weak m] _ in
            guard let m else { return }
            Task { await m.close() }
        })
        vc.navigationItem.rightBarButtonItem?.accessibilityIdentifier = "popout-done"
        let more = UIAction(title: "Open in New Window", image: UIImage(systemName: "macwindow.badge.plus")) { [weak self] _ in
            guard let self else { return }
            sheet?.nav.dismiss(animated: true)
            sheet = nil
            openScene(m.id)
        }
        vc.navigationItem.leftBarButtonItem = UIBarButtonItem(image: UIImage(systemName: "macwindow.badge.plus"), primaryAction: more)
        vc.navigationItem.leftBarButtonItem?.accessibilityLabel = "Open in New Window"
        let nav = UINavigationController(rootViewController: vc)
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
        if let s = scenes.removeValue(forKey: id) {
            UIApplication.shared.requestSceneSessionDestruction(s, options: nil) { _ in }
        }
        if let s = sheet, s.id == id {
            sheet = nil
            s.nav.dismiss(animated: true)
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
        sheet?.nav.dismiss(animated: false)
        sheet = nil
    }

    var describe: JSONValue {
        .array(models.values.sorted { $0.id < $1.id }.map { m in
            var o: [String: JSONValue] = ["id": .uint(m.id), "title": .string(m.window.title), "viewer": m.value?.viewer.map(JSONValue.string) ?? .null,
                                          "shown": .string(scenes[m.id] != nil ? "scene" : (sheet?.id == m.id ? "sheet" : "none"))]
            if sheet?.id == m.id, let vc = sheet?.vc { o.merge(vc.describe) { a, _ in a } }
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
        // its own close, as the sheet's Done (the window's controls close it too)
        vc.navigationItem.rightBarButtonItem = UIBarButtonItem(systemItem: .close, primaryAction: UIAction { [weak m] _ in
            guard let m else { return }
            Task { await m.close() }
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
    private(set) var describe: [String: JSONValue] = [:]

    init(popout: PopoutModel, pictures: Pictures) {
        self.popout = popout
        self.pictures = pictures
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground
        summaryLabel.font = .preferredFont(forTextStyle: .footnote)
        summaryLabel.adjustsFontForContentSizeCategory = true
        summaryLabel.textColor = .secondaryLabel
        summaryLabel.numberOfLines = 2
        summaryLabel.text = "Reading the value…"
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

    /// Show `v`, the core's value and the viewer's state.
    func show(_ v: PopoutValue) {
        guard isViewLoaded else { return }
        let same = shown.map { $0.data == v.data && $0.picture == v.picture && $0.excerpt == v.excerpt && $0.value == v.value && $0.viewer == v.viewer } ?? false
        shown = v
        navigationItem.title = v.title ?? popout.window.title
        view.window?.windowScene?.title = navigationItem.title
        let w = v.watched
        summaryLabel.text = ValueText.summary(w)
        describe = ["viewer": v.viewer.map(JSONValue.string) ?? .null, "node": .string(v.node), "port": .string(v.port),
                    "mono": .bool(v.text?.mono ?? false), "wrap": .bool(v.text?.wrap ?? true), "fit": v.fit.map(JSONValue.string) ?? .null]
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
        case "picture"?: put(pictureView(v))
        case "text.plain"?: put(textView(v))
        case "scalar"?: put(scalarView(v))
        case nil: put(message(v.summary == nil ? "Run the graph to see this value." : "No value yet."))
        case let other?:
            put(message("\(other): no viewer yet in this app."))
            describe["unavailable"] = .string(other)
        }
    }

    private func put(_ v: UIView) {
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
        scroll.minimumZoomScale = 0.1
        scroll.maximumZoomScale = 16
        scroll.delegate = self
        let image = UIImageView()
        image.contentMode = .scaleToFill
        image.layer.magnificationFilter = .nearest
        scroll.addSubview(image)
        imageView = image
        imageScroll = scroll
        let info = small("")
        // Fit and 1:1, the core's (`viewer.zoom`)
        let zoom = UISegmentedControl(items: ["Fit", "1:1"])
        zoom.selectedSegmentIndex = v.fit == "1:1" ? 1 : 0
        let p = popout
        zoom.addAction(UIAction { [weak zoom] _ in
            let to = zoom?.selectedSegmentIndex == 1 ? "1:1" : "fit"
            Task { await p.zoom(to) }
        }, for: .valueChanged)
        zoom.accessibilityIdentifier = "popout-zoom"
        layout(box, bar: bar([info, zoom]), content: scroll)
        let double = UITapGestureRecognizer(target: self, action: #selector(togglePictureZoom(_:)))
        double.numberOfTapsRequired = 2
        scroll.addGestureRecognizer(double)
        restyle = { [weak self, weak zoom] in
            zoom?.selectedSegmentIndex = self?.shown?.fit == "1:1" ? 1 : 0
            self?.applyZoom()
        }
        let w = v.watched
        guard let pic = w.picture, let handle = pic.handle else {
            info.text = "no picture"
            return box
        }
        info.text = "\(pic.w) × \(pic.h) · rgba8\(w.media.map { " · \($0)" } ?? "") · loading…"
        describe["picture"] = ["w": .uint(UInt64(pic.w)), "h": .uint(UInt64(pic.h)), "loaded": false]
        let pics = pictures
        Task { [weak self] in
            guard let got = await pics.load(handle) else {
                info.text = "\(pic.w) × \(pic.h) · rgba8 · \(pics.failures[handle] ?? "couldn't load")"
                return
            }
            image.image = UIImage(cgImage: got.image, scale: 1, orientation: .up)
            image.frame = CGRect(x: 0, y: 0, width: got.width, height: got.height)
            scroll.contentSize = image.frame.size
            info.text = "\(got.width) × \(got.height) px · rgba8\(w.media.map { " · \($0)" } ?? "")"
            self?.view.layoutIfNeeded()
            self?.applyZoom()
            self?.describe["picture"] = ["w": .uint(UInt64(got.width)), "h": .uint(UInt64(got.height)), "loaded": true]
        }
        return box
    }

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
