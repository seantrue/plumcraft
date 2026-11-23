// The shell's mask editor (docs/shell-interface.md 12.22; the maintainer's S4b decisions): a
// picture from the node's last run with its mask painted over it. One finger, the Pencil or the
// Mac's pointer paints; two fingers (or the trackpad, the scroll wheel) pan and pinch to zoom; a
// two-finger tap undoes. During a stroke the view paints with the Swift port of the brush
// (PlumCraftModels' `Masks`, bit for bit the core's), redrawing only what the dabs touched; when
// the finger lifts, one `mask.stroke` goes to the core (invoked as a person's, so a document with
// nowhere to write asks for a folder once), and the file the core wrote is read back in place of
// the view's own canvas.

import CoreGraphics
import Foundation
import ImageIO
import PlumCraft
import PlumCraftCanvas
import PlumCraftKit
import PlumCraftModels
import SwiftUI

/// Where the canvas sits in the view: `scale` view points a canvas pixel, its top left at
/// `origin` (view points).
public struct MaskTransform: Equatable, Sendable {
    public var scale: Double = 1
    public var origin: CGPoint = .zero

    public init(scale: Double = 1, origin: CGPoint = .zero) {
        self.scale = scale
        self.origin = origin
    }

    public static let minScale = 0.02
    public static let maxScale = 64.0

    /// The canvas (`w × h` pixels) whole and centred in `view`, inside `margin` points.
    public static func fit(w: Int, h: Int, in view: CGSize, margin: Double = 12) -> MaskTransform {
        guard w > 0, h > 0, view.width > 0, view.height > 0 else { return MaskTransform() }
        let s = min((view.width - 2 * margin) / Double(w), (view.height - 2 * margin) / Double(h))
        let scale = min(max(s, minScale), maxScale)
        return MaskTransform(scale: scale, origin: CGPoint(x: (view.width - Double(w) * scale) / 2, y: (view.height - Double(h) * scale) / 2))
    }

    /// A view point in canvas pixels.
    public func canvas(_ p: CGPoint) -> CGPoint {
        CGPoint(x: (p.x - origin.x) / scale, y: (p.y - origin.y) / scale)
    }

    /// A canvas point in the view.
    public func view(_ p: CGPoint) -> CGPoint {
        CGPoint(x: p.x * scale + origin.x, y: p.y * scale + origin.y)
    }

    /// A canvas rectangle (pixels) in the view.
    public func view(_ r: CGRect) -> CGRect {
        CGRect(x: r.minX * scale + origin.x, y: r.minY * scale + origin.y, width: r.width * scale, height: r.height * scale)
    }

    /// Zoomed by `factor` about `about` (a view point), which keeps its canvas point.
    public func zoomed(by factor: Double, about: CGPoint) -> MaskTransform {
        let s = min(max(scale * factor, Self.minScale), Self.maxScale)
        let c = canvas(about)
        return MaskTransform(scale: s, origin: CGPoint(x: about.x - c.x * s, y: about.y - c.y * s))
    }

    /// Moved by `d` view points.
    public func panned(by d: CGSize) -> MaskTransform {
        MaskTransform(scale: scale, origin: CGPoint(x: origin.x + d.width, y: origin.y + d.height))
    }
}

/// A mask editor's state: the picture, the canvas (the core's file, or a stroke under way), the
/// brush, and what is drawn.
@MainActor @Observable
public final class MaskModel {
    @ObservationIgnored public let report: ReportModel
    @ObservationIgnored let pictures: Pictures?
    public var node: String { String(report.key.dropFirst("editor:".count)) }

    // the brush
    public var tool: MaskTool = .mask
    public var radius: Double = 24
    public var hardness: Double = 0.5
    public var flow: Double = 0.6
    public var density: Double = 1
    public var grey: Double = 0
    public var showMask = true { didSet { recomposite(nil) } }

    // what the core says
    public var w = 0
    public var h = 0
    public var value = ""
    public var file: String?
    public var wired = false
    public var hasBase = false
    public var error: String?
    public var loaded = false
    public var sent = 0
    public var undone = 0

    /// The picture as shown (the mask over it), and a count that moves when it changes.
    public private(set) var image: CGImage?
    public private(set) var version = 0
    /// The canvas pixels changed since the view last drew (nil: all).
    @ObservationIgnored public var dirty: CGRect?
    /// The view's transform, kept here so a reload keeps it.
    public var transform = MaskTransform()
    public var fitted = false

    @ObservationIgnored var picture: [UInt8] = []
    @ObservationIgnored var base: [Float]?
    @ObservationIgnored var canvas: MaskCanvas?
    @ObservationIgnored var stroke: MaskStroke?
    @ObservationIgnored var shown: [UInt8] = []
    @ObservationIgnored var pictureHandle: String?
    @ObservationIgnored var baseHandle: String?
    @ObservationIgnored var loadedFile: String?
    /// The canvases this view has had, by the pixel hash an edit file's name carries: a stroke's
    /// own result (bit for bit the core's file) and what undo brings back, without a file read.
    @ObservationIgnored var byHash: [UInt64: MaskCanvas] = [:]
    @ObservationIgnored var hashOrder: [UInt64] = []
    /// The last edit file read approximately (its decoder premultiplied the alpha).
    public var approximate = false
    @ObservationIgnored private var busy = false
    @ObservationIgnored private var again = false

    public init(report: ReportModel, pictures: Pictures?) {
        self.report = report
        self.pictures = pictures
    }

    public var stroking: Bool { stroke != nil }

    public var brush: MaskBrush {
        MaskBrush(tool: tool, radius: min(max(radius, MaskBrush.minRadius), MaskBrush.maxRadius), hardness: hardness, flow: Float(flow),
                  density: Float(density), value: UInt8(min(max(grey.rounded(), 0), 255)))
    }

    // MARK: Loading

    public func reload() {
        if busy {
            again = true
            return
        }
        busy = true
        Task {
            await load()
            busy = false
            if again {
                again = false
                reload()
            }
        }
    }

    func load() async {
        guard stroke == nil else { return }
        do {
            let d = try await report.model.run("node.editorData", ["node": .string(node)])
            wired = d["wired"].bool ?? false
            let data = d["data"]
            error = data["error"].string
            let ph = Pictures.handle(of: data["picture"]["ref"])
            let bh = Pictures.handle(of: data["base"]["ref"])
            let (nw, nh) = (Int(data["w"].double ?? 0), Int(data["h"].double ?? 0))
            var fresh = false
            if let ph, ph != pictureHandle || nw != w || nh != h, let pictures {
                guard let p = await pictures.load(ph), let px = Self.pixels(p.image, w: nw, h: nh) else {
                    error = "the picture didn't load"
                    return
                }
                picture = px
                pictureHandle = ph
                (w, h) = (nw, nh)
                fresh = true
            } else if ph == nil {
                picture = []
                (w, h) = (0, 0)
                canvas = nil
                image = nil
                loaded = true
                return
            }
            if bh != baseHandle {
                baseHandle = bh
                if let bh, let pictures, let p = await pictures.load(bh), let px = Self.pixels(p.image, w: w, h: h) {
                    base = stride(from: 0, to: px.count, by: 4).map { Float(px[$0]) / 255 }
                } else {
                    base = nil
                }
                fresh = true
            }
            hasBase = base != nil
            let g = try await report.model.run("mask.get", ["node": .string(node)])
            value = g["value"].string ?? ""
            file = g["file"].string
            guard stroke == nil else { return }
            if fresh || file != loadedFile || canvas == nil {
                if fresh { byHash = [:]; hashOrder = [] }
                if let f = file {
                    if let h = Self.hash(of: f), let c = byHash[h] {
                        canvas = c
                    } else if let (c, exact) = Self.readEdit(f, w: w, h: h) {
                        canvas = c
                        approximate = !exact
                        remember(c)
                    } else {
                        error = "couldn't read the edit \(value)"
                        canvas = start
                    }
                } else {
                    canvas = start
                }
                loadedFile = file
                recomposite(nil)
            }
            loaded = true
        } catch {
            self.error = "\(error)"
        }
    }

    var start: MaskCanvas { MaskCanvas(w: w, h: h, picture: picture, base: base) }

    /// The pixel hash an edit file's name carries (`<node>-<16 hex>.png`).
    nonisolated static func hash(of file: String) -> UInt64? {
        let name = (file as NSString).lastPathComponent
        guard name.hasSuffix(".png"), let dash = name.lastIndex(of: "-") else { return nil }
        let hex = name[name.index(after: dash)..<name.index(name.endIndex, offsetBy: -4)]
        return hex.count == 16 ? UInt64(hex, radix: 16) : nil
    }

    func remember(_ c: MaskCanvas) {
        let h = c.pixelHash
        if byHash[h] == nil { hashOrder.append(h) }
        byHash[h] = c
        while hashOrder.count > 12 { byHash[hashOrder.removeFirst()] = nil }
    }

    /// `image`'s pixels as RGBA bytes, when it is `w × h` (opaque pictures: exact).
    nonisolated static func pixels(_ image: CGImage, w: Int, h: Int) -> [UInt8]? {
        guard w > 0, h > 0, image.width == w, image.height == h, let space = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        var out = [UInt8](repeating: 0, count: w * h * 4)
        let ok = out.withUnsafeMutableBytes { buf -> Bool in
            guard let ctx = CGContext(data: buf.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: space,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
            return true
        }
        return ok ? out : nil
    }

    /// An edit file's canvas, and whether it is exact: its own bytes (straight alpha) as the core
    /// reads them; a decoder that premultiplied the alpha is undone, approximately.
    nonisolated static func readEdit(_ path: String, w: Int, h: Int) -> (MaskCanvas, Bool)? {
        // its own bytes first: exact on every platform
        if let data = FileManager.default.contents(atPath: path), let png = PNGRGBA.decode(data) {
            guard png.w == w, png.h == h, let c = MaskCanvas(w: w, h: h, rgba: png.rgba) else { return nil }
            return (c, true)
        }
        guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
              let img = CGImageSourceCreateImageAtIndex(src, 0, [kCGImageSourceShouldCache: false] as CFDictionary),
              img.width == w, img.height == h, img.bitsPerComponent == 8, img.bitsPerPixel == 32,
              [.last, .premultipliedLast].contains(img.alphaInfo), let data = img.dataProvider?.data as Data?
        else { return nil }
        let premultiplied = img.alphaInfo == .premultipliedLast
        let row = img.bytesPerRow
        var rgba = [UInt8](repeating: 0, count: w * h * 4)
        data.withUnsafeBytes { src in
            for y in 0..<h {
                for x in 0..<(w * 4) where y * row + x < src.count {
                    rgba[y * w * 4 + x] = src[y * row + x]
                }
            }
        }
        if premultiplied {
            for i in stride(from: 0, to: rgba.count, by: 4) where rgba[i + 3] > 0 && rgba[i + 3] < 255 {
                let a = Double(rgba[i + 3])
                for k in 0..<3 { rgba[i + k] = UInt8(min(255, (Double(rgba[i + k]) * 255 / a).rounded())) }
            }
        }
        return MaskCanvas(w: w, h: h, rgba: rgba).map { ($0, !premultiplied) }
    }

    // MARK: Drawing

    /// Composite the canvas (`r`: only these pixels; nil all) into what is shown.
    func recomposite(_ r: (Int, Int, Int, Int)?) {
        guard let c = canvas, c.w > 0, c.h > 0 else {
            image = nil
            return
        }
        if shown.count != c.w * c.h * 4 {
            shown = [UInt8](repeating: 255, count: c.w * c.h * 4)
        }
        let (x0, y0, x1, y1) = r ?? (0, 0, c.w - 1, c.h - 1)
        let tint: [Float] = [40, 110, 255]
        for y in max(y0, 0)...min(y1, c.h - 1) {
            for x in max(x0, 0)...min(x1, c.w - 1) {
                let i = y * c.w + x
                let a = showMask ? c.mask[i] * 0.55 : 0
                for k in 0..<3 {
                    let p = Float(c.rgb[i * 3 + k])
                    shown[i * 4 + k] = UInt8(min(max((p * (1 - a) + tint[k] * a).rounded(), 0), 255))
                }
            }
        }
        let data = Data(shown) as CFData
        if let provider = CGDataProvider(data: data), let space = CGColorSpace(name: CGColorSpace.sRGB) {
            image = CGImage(width: c.w, height: c.h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: c.w * 4, space: space,
                            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue), provider: provider, decode: nil,
                            shouldInterpolate: false, intent: .defaultIntent)
        }
        dirty = r.map { CGRect(x: $0.0, y: $0.1, width: $0.2 - $0.0 + 1, height: $0.3 - $0.1 + 1) }
        version &+= 1
    }

    // MARK: A stroke (canvas pixels)

    public func begin(_ p: CGPoint) {
        guard let c = canvas, !wired else { return }
        stroke = Masks.begin(c, brush)
        extend(p)
    }

    public func extend(_ p: CGPoint) {
        guard var s = stroke, var c = canvas else { return }
        Masks.strokeTo(&c, &s, (Double(p.x), Double(p.y)))
        let d = s.takeDirty()
        canvas = c
        stroke = s
        if let d { recomposite(d) }
    }

    /// Lift: one `mask.stroke`, invoked as the person's (it may ask for a folder first).
    public func end() {
        guard let s = stroke else { return }
        stroke = nil
        guard !s.points.isEmpty else { return }
        // the view's own result, bit for bit the file the core will name by this hash
        if let c = canvas { remember(c) }
        let b = s.brush
        let params: JSONValue = ["node": .string(node), "points": .array(s.points.map { .array($0.map { .double($0) }) }), "tool": .string(b.tool.rawValue),
                                 "radius": .double(b.radius), "hardness": .double(b.hardness), "flow": .double(Double(b.flow)),
                                 "density": .double(Double(b.density)), "value": .double(Double(b.value))]
        sent += 1
        Task {
            do {
                _ = try await report.model.invoke("mask.stroke", params: params, person: true)
                error = nil
            } catch {
                self.error = "\(error)"
            }
            reload()
        }
    }

    public func undo() {
        undone += 1
        Task {
            _ = try? await report.model.run("edit.undo")
            reload()
        }
    }

    public func redo() {
        Task {
            _ = try? await report.model.run("edit.redo")
            reload()
        }
    }

    public func clear() {
        Task {
            _ = try? await report.model.run("mask.clear", ["node": .string(node)])
            reload()
        }
    }

    public func runGraph() {
        Task { _ = try? await report.model.run("run.start") }
    }

    /// For scripts (`--status-file`).
    public var describe: JSONValue {
        ["kind": "mask", "node": .string(node), "w": .int(Int64(w)), "h": .int(Int64(h)), "value": .string(value), "file": file.map(JSONValue.string) ?? .null,
         "tool": .string(tool.rawValue), "radius": .double(radius), "showMask": .bool(showMask), "base": .bool(hasBase), "wired": .bool(wired),
         "picture": .bool(!picture.isEmpty), "stroking": .bool(stroke != nil), "sent": .uint(UInt64(sent)), "undone": .uint(UInt64(undone)),
         "scale": .double(transform.scale), "approximate": .bool(approximate), "masked": .uint(UInt64(canvas?.mask.filter { $0 > 0 }.count ?? 0)), "loaded": .bool(loaded),
         "error": error.map(JSONValue.string) ?? .null]
    }
}

/// The mask editor: the brush's controls over the canvas.
public struct MaskEditorView: SwiftUI.View {
    @Bindable var model: MaskModel
    let done: (() -> Void)?

    public var body: some SwiftUI.View {
        NavigationStack {
            VStack(spacing: 8) {
                controls
                if model.image != nil {
                    MaskCanvasHost(model: model)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .clipped()
                } else if model.loaded {
                    VStack(spacing: 12) {
                        Text("Run the graph to paint on its picture.").foregroundStyle(.secondary)
                        Button("Run") { model.runGraph() }.accessibilityIdentifier("mask-run")
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                if let e = model.error { Text(e).font(.footnote).foregroundStyle(.red).accessibilityIdentifier("mask-error") }
                if model.wired { Text("A wire feeds this value: edit it where it comes from.").font(.footnote).foregroundStyle(.secondary) }
            }
            .padding(.horizontal)
            .navigationTitle("Mask")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                if let done { ToolbarItem(placement: .confirmationAction) { Button("Done", action: done).accessibilityIdentifier("report-done") } }
            }
        }
        .accessibilityIdentifier("mask-editor")
        .onAppear { model.reload() }
        .onChange(of: model.report.followed) { model.reload() }
    }

    private var controls: some SwiftUI.View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Picker("Brush", selection: $model.tool) {
                    Text("Mask").tag(MaskTool.mask)
                    Text("Erase").tag(MaskTool.erase)
                    Text("Paint").tag(MaskTool.paint)
                }
                .pickerStyle(.segmented)
                .fixedSize()
                .accessibilityIdentifier("mask-tool")
                Toggle("Show Mask", isOn: $model.showMask).fixedSize().accessibilityIdentifier("mask-show")
                Spacer()
                Button("Fit") { model.fitted = false }.accessibilityIdentifier("mask-fit")
                Button("Clear", role: .destructive) { model.clear() }.disabled(model.value.isEmpty).accessibilityIdentifier("mask-clear")
            }
            HStack(spacing: 14) {
                slider("Size", $model.radius, 1...200, "%.0f")
                slider("Hardness", $model.hardness, 0...1, "%.2f")
                slider("Flow", $model.flow, 0...1, "%.2f")
                slider("Density", $model.density, 0...1, "%.2f")
                if model.tool == .paint { slider("Grey", $model.grey, 0...255, "%.0f") }
            }
        }
        .padding(.top, 6)
    }

    private func slider(_ label: String, _ v: Binding<Double>, _ range: ClosedRange<Double>, _ fmt: String) -> some SwiftUI.View {
        VStack(alignment: .leading, spacing: 0) {
            Text("\(label) \(String(format: fmt, v.wrappedValue))").font(.caption).foregroundStyle(.secondary)
            Slider(value: v, in: range).frame(minWidth: 80)
        }
        .accessibilityIdentifier("mask-\(label.lowercased())")
    }
}

// MARK: The canvas, a platform view (touches and the Pencil, or the pointer and the trackpad)

#if os(iOS)
import UIKit

struct MaskCanvasHost: UIViewRepresentable {
    let model: MaskModel
    func makeUIView(context: Context) -> MaskCanvasView { MaskCanvasView(model: model) }
    func updateUIView(_ v: MaskCanvasView, context: Context) { v.follow() }
}

/// One finger or the Pencil paints; two fingers pan and pinch; a two-finger tap undoes.
final class MaskCanvasView: UIView, UIGestureRecognizerDelegate {
    let model: MaskModel
    private var drawn = -1

    init(model: MaskModel) {
        self.model = model
        super.init(frame: .zero)
        backgroundColor = .secondarySystemBackground
        isMultipleTouchEnabled = true
        accessibilityIdentifier = "mask-canvas"
        isAccessibilityElement = true
        accessibilityLabel = "Mask canvas"
        accessibilityTraits = .allowsDirectInteraction
        let paint = UIPanGestureRecognizer(target: self, action: #selector(paint(_:)))
        paint.maximumNumberOfTouches = 1
        let pan = UIPanGestureRecognizer(target: self, action: #selector(pan(_:)))
        pan.minimumNumberOfTouches = 2
        let pinch = UIPinchGestureRecognizer(target: self, action: #selector(pinch(_:)))
        let undo = UITapGestureRecognizer(target: self, action: #selector(undo(_:)))
        undo.numberOfTouchesRequired = 2
        for g in [pan, pinch] as [UIGestureRecognizer] { g.delegate = self }
        // a pinch or a two-finger pan isn't a tap
        undo.require(toFail: pinch)
        undo.require(toFail: pan)
        for g in [paint, pan, pinch, undo] as [UIGestureRecognizer] { addGestureRecognizer(g) }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }

    func follow() {
        if !model.fitted, bounds.width > 0 {
            model.transform = MaskTransform.fit(w: model.w, h: model.h, in: bounds.size)
            model.fitted = true
            setNeedsDisplay()
        }
        guard model.version != drawn else { return }
        drawn = model.version
        if let d = model.dirty { setNeedsDisplay(model.transform.view(d).insetBy(dx: -2, dy: -2)) } else { setNeedsDisplay() }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        if !model.fitted { follow() }
    }

    override func draw(_ rect: CGRect) {
        guard let ctx = UIGraphicsGetCurrentContext(), let img = model.image else { return }
        ctx.interpolationQuality = model.transform.scale >= 2 ? .none : .medium
        let r = model.transform.view(CGRect(x: 0, y: 0, width: model.w, height: model.h))
        // CoreGraphics draws images bottom up: flip around the image's own rectangle
        ctx.saveGState()
        ctx.translateBy(x: 0, y: r.minY + r.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.draw(img, in: r)
        ctx.restoreGState()
    }

    @objc func paint(_ g: UIPanGestureRecognizer) {
        let p = model.transform.canvas(g.location(in: self))
        switch g.state {
        case .began: model.begin(p)
        case .changed: model.extend(p)
        case .ended: model.extend(p); model.end()
        case .cancelled, .failed: model.end()
        default: break
        }
        follow()
    }

    @objc func pan(_ g: UIPanGestureRecognizer) {
        let t = g.translation(in: self)
        model.transform = model.transform.panned(by: CGSize(width: t.x, height: t.y))
        g.setTranslation(.zero, in: self)
        setNeedsDisplay()
    }

    @objc func pinch(_ g: UIPinchGestureRecognizer) {
        model.transform = model.transform.zoomed(by: g.scale, about: g.location(in: self))
        g.scale = 1
        setNeedsDisplay()
    }

    @objc func undo(_ g: UITapGestureRecognizer) {
        if g.state == .ended { model.undo() }
    }
}
#else
import AppKit

struct MaskCanvasHost: NSViewRepresentable {
    let model: MaskModel
    func makeNSView(context: Context) -> MaskCanvasView { MaskCanvasView(model: model) }
    func updateNSView(_ v: MaskCanvasView, context: Context) { v.follow() }
}

/// The pointer paints; the scroll wheel or two fingers on the trackpad pan, ⌘-scroll and a pinch
/// zoom; `[` and `]` change the size.
final class MaskCanvasView: NSView {
    let model: MaskModel
    private var drawn = -1

    init(model: MaskModel) {
        self.model = model
        super.init(frame: .zero)
        setAccessibilityIdentifier("mask-canvas")
        setAccessibilityElement(true)
        setAccessibilityRole(.image)
        setAccessibilityLabel("Mask canvas")
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    func follow() {
        if !model.fitted, bounds.width > 0 {
            model.transform = MaskTransform.fit(w: model.w, h: model.h, in: bounds.size)
            model.fitted = true
            needsDisplay = true
        }
        guard model.version != drawn else { return }
        drawn = model.version
        if let d = model.dirty { setNeedsDisplay(model.transform.view(d).insetBy(dx: -2, dy: -2)) } else { needsDisplay = true }
    }

    override func layout() {
        super.layout()
        if !model.fitted { follow() }
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.windowBackgroundColor.setFill()
        dirtyRect.fill()
        guard let ctx = NSGraphicsContext.current?.cgContext, let img = model.image else { return }
        ctx.interpolationQuality = model.transform.scale >= 2 ? .none : .medium
        let r = model.transform.view(CGRect(x: 0, y: 0, width: model.w, height: model.h))
        ctx.saveGState()
        ctx.translateBy(x: 0, y: r.minY + r.maxY)
        ctx.scaleBy(x: 1, y: -1)
        ctx.draw(img, in: r)
        ctx.restoreGState()
    }

    private func at(_ e: NSEvent) -> CGPoint { model.transform.canvas(convert(e.locationInWindow, from: nil)) }

    override func mouseDown(with e: NSEvent) {
        // focus here, so Undo and `[` `]` come to the canvas
        window?.makeFirstResponder(self)
        model.begin(at(e))
        follow()
    }

    /// Edit › Undo and Redo while the canvas has the focus: the document's (the core's), not the
    /// window's own undo manager, which knows nothing of strokes.
    @objc func undo(_ sender: Any?) {
        model.undo()
    }

    @objc func redo(_ sender: Any?) {
        model.redo()
    }

    override func mouseDragged(with e: NSEvent) {
        model.extend(at(e))
        follow()
    }

    override func mouseUp(with e: NSEvent) {
        model.extend(at(e))
        model.end()
        follow()
    }

    override func scrollWheel(with e: NSEvent) {
        if e.modifierFlags.contains(.command) {
            model.transform = model.transform.zoomed(by: exp(e.scrollingDeltaY * 0.01), about: convert(e.locationInWindow, from: nil))
        } else {
            model.transform = model.transform.panned(by: CGSize(width: e.scrollingDeltaX, height: e.scrollingDeltaY))
        }
        needsDisplay = true
    }

    override func magnify(with e: NSEvent) {
        model.transform = model.transform.zoomed(by: 1 + e.magnification, about: convert(e.locationInWindow, from: nil))
        needsDisplay = true
    }

    override func keyDown(with e: NSEvent) {
        switch e.charactersIgnoringModifiers {
        case "["?: model.radius = max(1, model.radius / 1.25)
        case "]"?: model.radius = min(200, model.radius * 1.25)
        default: super.keyDown(with: e)
        }
    }
}
#endif

/// For tests: an edit file read as the editor reads it, and the picture a model paints on.
public enum MaskEditorProbe {
    public static func canvas(_ path: String, w: Int, h: Int) -> MaskCanvas? { MaskModel.readEdit(path, w: w, h: h)?.0 }
    @MainActor public static func picture(_ m: MaskModel) -> [UInt8] { m.picture }
    /// A model on `report` with its own picture loader.
    @MainActor public static func model(_ report: ReportModel) -> MaskModel { MaskModel(report: report, pictures: Pictures(model: report.model)) }
}
