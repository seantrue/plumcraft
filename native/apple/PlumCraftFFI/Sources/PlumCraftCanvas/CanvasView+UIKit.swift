// The iPad's host for the layer canvas, kept small until slice 5 builds the iPad app: the canvas
// itself is the same as the Mac's; only recognition differs (plan 5.5's iPadOS column). Here:
// pan with one finger on empty canvas or two anywhere, pinch zooms about the fingers, a tap is
// `ui.gesture` tap, a double tap fits the node under it (or everything).

#if os(iOS)
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import QuartzCore
import UIKit

@MainActor
public final class CanvasView: UIView, UIGestureRecognizerDelegate {
    public let canvas: LayerCanvas
    public var model: DocumentModel { canvas.model }
    var lastPan = CGPoint.zero
    public var onError: ((String) -> Void)?

    public init(model: DocumentModel, frame: CGRect = CGRect(x: 0, y: 0, width: 1024, height: 768)) {
        let host = CALayer()
        canvas = LayerCanvas(model: model, host: host, screenScale: UITraitCollection.current.displayScale)
        super.init(frame: frame)
        layer.addSublayer(host)
        host.frame = bounds
        host.masksToBounds = true
        canvas.viewSize = frame.size
        let pan = UIPanGestureRecognizer(target: self, action: #selector(onPan(_:)))
        pan.maximumNumberOfTouches = 2
        pan.delegate = self
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(onPinch(_:)))
        pinch.delegate = self
        let tap = UITapGestureRecognizer(target: self, action: #selector(onTap(_:)))
        let double = UITapGestureRecognizer(target: self, action: #selector(onDoubleTap(_:)))
        double.numberOfTapsRequired = 2
        tap.require(toFail: double)
        for g in [pan, pinch, tap, double] as [UIGestureRecognizer] { addGestureRecognizer(g) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    public override func layoutSubviews() {
        super.layoutSubviews()
        canvas.host.frame = bounds
        canvas.viewSize = bounds.size
    }

    public func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith o: UIGestureRecognizer) -> Bool { true }

    func graph(_ v: CGPoint) -> Point {
        let g = canvas.viewport.toGraph(v)
        return Point(Double(g.x), Double(g.y))
    }

    /// 44 points on screen, in graph units (plan 5.5's touch target).
    var slop: Double { CanvasLink.slop(points: 22, zoom: Double(canvas.viewport.zoom)) }

    @objc func onPan(_ g: UIPanGestureRecognizer) {
        let p = g.location(in: self)
        switch g.state {
        case .began:
            lastPan = p
        case .changed:
            canvas.pan(by: CGPoint(x: p.x - lastPan.x, y: p.y - lastPan.y), elastic: true)
            lastPan = p
        default:
            canvas.endPan()
        }
    }

    @objc func onPinch(_ g: UIPinchGestureRecognizer) {
        guard g.state == .changed || g.state == .began else { return }
        canvas.zoom(by: g.scale, at: g.location(in: self))
        g.scale = 1
    }

    @objc func onTap(_ g: UITapGestureRecognizer) {
        let at = graph(g.location(in: self))
        let s = slop
        let link = canvas.link
        Task { [weak self] in
            do { try await link.tap(at, slop: s) } catch { self?.onError?("\(error)") }
        }
    }

    @objc func onDoubleTap(_ g: UITapGestureRecognizer) {
        let h = canvas.link.hit(graph(g.location(in: self)), slop: slop)
        if h.ok, h.kind == .node { canvas.fitFace(h.node) } else { canvas.requestFit() }
    }
}
#endif
