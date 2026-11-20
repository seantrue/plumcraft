// The Watched panel on the iPad (N3 slice 5): the value the core watches (`ui.inspect`'s
// `watched`) with native viewers for the first three of plan 8's order. In regular width it is an
// inspector column on the graph's right; in compact width (Split View, a narrow Stage Manager
// window) a sheet with detents (GraphViewController decides).
//
//   picture     the rgba8 rendition (media.get, plum_blob) made on a worker, in a zoomable scroll
//               view (pinch; double tap toggles fit and 1:1), with its size and type
//   text.plain  selectable text with a monospaced switch; the excerpt first, the whole text on
//               demand from the full value's bytes (Show All)
//   scalar      the value, large and selectable
//
// Text is the system's with Dynamic Type. Any other kind says "no viewer yet in this app".

import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import PlumCraftShell
import UIKit

@MainActor
final class WatchedPanel: UIViewController, UIScrollViewDelegate {
    let model: DocumentModel
    let pictures: Pictures
    private let titleLabel = UILabel()
    private let summaryLabel = UILabel()
    private let body = UIView()
    private var shown: Watched?
    private var current: UIView?
    private var mono = false
    private var fullText: [String: String] = [:]
    private weak var imageView: UIImageView?
    private weak var imageScroll: UIScrollView?
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

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .secondarySystemBackground
        titleLabel.font = .preferredFont(forTextStyle: .headline)
        titleLabel.adjustsFontForContentSizeCategory = true
        titleLabel.lineBreakMode = .byTruncatingMiddle
        summaryLabel.font = .preferredFont(forTextStyle: .footnote)
        summaryLabel.adjustsFontForContentSizeCategory = true
        summaryLabel.textColor = .secondaryLabel
        summaryLabel.numberOfLines = 3
        for x in [titleLabel, summaryLabel, body] {
            x.translatesAutoresizingMaskIntoConstraints = false
            view.addSubview(x)
        }
        let g = view.safeAreaLayoutGuide
        NSLayoutConstraint.activate([
            titleLabel.topAnchor.constraint(equalTo: g.topAnchor, constant: 14),
            titleLabel.leadingAnchor.constraint(equalTo: g.leadingAnchor, constant: 16),
            titleLabel.trailingAnchor.constraint(equalTo: g.trailingAnchor, constant: -16),
            summaryLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 4),
            summaryLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            summaryLabel.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            body.topAnchor.constraint(equalTo: summaryLabel.bottomAnchor, constant: 10),
            body.leadingAnchor.constraint(equalTo: g.leadingAnchor),
            body.trailingAnchor.constraint(equalTo: g.trailingAnchor),
            body.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        show(model.watched, force: true)
    }

    /// Show `w` (nil: nothing watched).
    func show(_ w: Watched?, force: Bool = false) {
        guard isViewLoaded else { return }
        if !force && w == shown && current != nil { return }
        shown = w
        current?.removeFromSuperview()
        current = nil
        guard let w else {
            titleLabel.text = "Watched"
            summaryLabel.text = "Nothing watched. Tap an output's pin, or a wire, to watch its value."
            describe = ["viewer": .null]
            onShown?()
            return
        }
        titleLabel.text = "\(model.face(id: w.node)?.header.title.text ?? w.node) · \(w.port)"
        summaryLabel.text = ValueText.summary(w)
        describe = ["viewer": w.viewer.map(JSONValue.string) ?? .null, "node": .string(w.node), "port": .string(w.port)]
        if let e = w.error, e != .null {
            put(message("This value failed: \(e.string ?? e["message"].string ?? e.text)"))
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

    private func button(_ t: String, _ f: @escaping () -> Void) -> UIButton {
        var c = UIButton.Configuration.bordered()
        c.title = t
        c.buttonSize = .small
        return UIButton(configuration: c, primaryAction: UIAction { _ in f() })
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

    private func pictureView(_ w: Watched) -> UIView {
        let box = UIView()
        let scroll = UIScrollView()
        scroll.backgroundColor = .tertiarySystemBackground
        scroll.minimumZoomScale = 0.1
        scroll.maximumZoomScale = 16
        scroll.delegate = self
        scroll.showsHorizontalScrollIndicator = true
        let image = UIImageView()
        image.contentMode = .scaleToFill
        image.layer.magnificationFilter = .nearest
        scroll.addSubview(image)
        imageView = image
        imageScroll = scroll
        let info = small("")
        let fit = button("Fit") { [weak self] in self?.fitPicture() }
        let one = button("1:1") { [weak self] in self?.imageScroll?.setZoomScale(1, animated: true) }
        layout(box, bar: bar([info, fit, one]), content: scroll)
        let double = UITapGestureRecognizer(target: self, action: #selector(togglePictureZoom(_:)))
        double.numberOfTapsRequired = 2
        scroll.addGestureRecognizer(double)
        guard let pic = w.picture, let handle = pic.handle else {
            info.text = "no picture"
            return box
        }
        info.text = "\(pic.w) × \(pic.h) · rgba8\(w.media.map { " · \($0)" } ?? "") · loading…"
        describe["picture"] = ["w": .uint(UInt64(pic.w)), "h": .uint(UInt64(pic.h)), "loaded": false]
        let pics = pictures
        Task { [weak self] in
            // decoded on a worker (Pictures); only setting it here
            guard let got = await pics.load(handle) else {
                info.text = "\(pic.w) × \(pic.h) · rgba8 · \(pics.failures[handle] ?? "couldn't load")"
                return
            }
            image.image = UIImage(cgImage: got.image, scale: 1, orientation: .up)
            image.frame = CGRect(x: 0, y: 0, width: got.width, height: got.height)
            scroll.contentSize = image.frame.size
            info.text = "\(got.width) × \(got.height) px · rgba8\(w.media.map { " · \($0)" } ?? "")"
            self?.view.layoutIfNeeded()
            self?.fitPicture()
            self?.describe["picture"] = ["w": .uint(UInt64(got.width)), "h": .uint(UInt64(got.height)), "loaded": true]
            self?.onShown?()
        }
        return box
    }

    func viewForZooming(in scrollView: UIScrollView) -> UIView? { imageView }

    func scrollViewDidZoom(_ scrollView: UIScrollView) { centrePicture() }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        centrePicture()
    }

    /// Keep a picture smaller than the view in its middle, as Photos does.
    private func centrePicture() {
        guard let s = imageScroll, let v = imageView else { return }
        let dx = max((s.bounds.width - v.frame.width) / 2, 0)
        let dy = max((s.bounds.height - v.frame.height) / 2, 0)
        s.contentInset = UIEdgeInsets(top: dy, left: dx, bottom: dy, right: dx)
    }

    private func fitPicture() {
        guard let s = imageScroll, let size = imageView?.image?.size, size.width > 0, size.height > 0, s.bounds.width > 0 else { return }
        let z = min(s.bounds.width / size.width, s.bounds.height / size.height)
        s.setZoomScale(max(min(z, 8), s.minimumZoomScale), animated: false)
        centrePicture()
    }

    @objc func togglePictureZoom(_ g: UITapGestureRecognizer) {
        guard let s = imageScroll else { return }
        if abs(s.zoomScale - 1) < 0.01 { fitPicture() } else { s.setZoomScale(1, animated: true) }
    }

    // MARK: text.plain

    private func textView(_ w: Watched) -> UIView {
        let box = UIView()
        let tv = UITextView()
        tv.isEditable = false
        tv.isSelectable = true
        tv.backgroundColor = .tertiarySystemBackground
        tv.textContainerInset = UIEdgeInsets(top: 10, left: 12, bottom: 10, right: 12)
        tv.adjustsFontForContentSizeCategory = true
        let ex = ValueText.excerpt(w)
        let handle = ValueText.dataHandle(w)
        let info = small("")
        let monoSwitch = UISwitch()
        monoSwitch.isOn = mono
        let monoLabel = small("Monospaced")
        monoLabel.setContentHuggingPriority(.required, for: .horizontal)
        let setText = { [weak self] (s: String) in
            let mono = self?.mono ?? false
            let body = UIFont.preferredFont(forTextStyle: .body)
            tv.font = mono ? UIFontMetrics(forTextStyle: .body).scaledFont(for: .monospacedSystemFont(ofSize: body.pointSize - 2, weight: .regular)) : body
            tv.text = s
        }
        let whole = handle.flatMap { fullText[$0] }
        setText(whole ?? ex.text)
        let cut = whole == nil && ex.total > ex.shown
        info.text = cut ? "first \(ex.shown) of \(ex.total) characters" : "\(ex.total) characters"
        describe["text"] = ["shown": .uint(UInt64(tv.text.count)), "total": .uint(UInt64(ex.total))]
        monoSwitch.addAction(UIAction { [weak self, weak monoSwitch] _ in
            self?.mono = monoSwitch?.isOn ?? false
            setText(tv.text)
        }, for: .valueChanged)
        var views: [UIView] = [info, monoLabel, monoSwitch]
        if cut, let handle {
            let more = button("Show All") {}
            more.addAction(UIAction { [weak self, weak more] _ in
                guard let self else { return }
                more?.isEnabled = false
                info.text = "reading the whole text…"
                Task {
                    if let t = await ValueText.whole(self.model, handle: handle) {
                        self.fullText[handle] = t
                        setText(t)
                        info.text = "\(t.count) characters"
                        more?.isHidden = true
                        self.describe["text"] = ["shown": .uint(UInt64(t.count)), "total": .uint(UInt64(ex.total))]
                    } else {
                        info.text = "the whole text couldn't be read"
                        more?.isEnabled = true
                    }
                }
            }, for: .primaryActionTriggered)
            views.append(more)
        }
        layout(box, bar: bar(views), content: tv)
        return box
    }

    // MARK: scalar

    private func scalarView(_ w: Watched) -> UIView {
        let box = UIView()
        let text = ValueText.scalar(w)
        // a text view, so the value can be selected and copied
        let v = UITextView()
        v.isEditable = false
        v.isScrollEnabled = false
        v.backgroundColor = .clear
        v.textContainerInset = .zero
        v.textContainer.lineFragmentPadding = 0
        let big = UIFont.monospacedDigitSystemFont(ofSize: 34, weight: .medium)
        v.font = UIFontMetrics(forTextStyle: .largeTitle).scaledFont(for: big)
        v.adjustsFontForContentSizeCategory = true
        v.text = text
        let kind = small(w.media ?? "scalar")
        for x in [v, kind] {
            x.translatesAutoresizingMaskIntoConstraints = false
            box.addSubview(x)
        }
        NSLayoutConstraint.activate([
            v.topAnchor.constraint(equalTo: box.topAnchor, constant: 12),
            v.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16),
            v.trailingAnchor.constraint(lessThanOrEqualTo: box.trailingAnchor, constant: -16),
            kind.topAnchor.constraint(equalTo: v.bottomAnchor, constant: 4),
            kind.leadingAnchor.constraint(equalTo: v.leadingAnchor),
        ])
        describe["scalar"] = .string(text)
        return box
    }
}
