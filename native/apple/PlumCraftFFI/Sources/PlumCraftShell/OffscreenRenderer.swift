// The bench's `--offscreen` renderer (the wire-hitch work, Mac only): Core Animation's own renderer
// (CARenderer) drawing the canvas's layer tree into a Metal texture, timed frame by frame. It does
// for the texture what the render server does for a window: rasterises shape layers, composites
// layer pictures and blends them, so a layer tree that is costly to render costs here too, in
// this process, measured without Instruments (whose traces filled the Mac's disk).

#if os(macOS)
import Metal
import QuartzCore

@MainActor
final class OffscreenRenderer {
    let size: CGSize
    let scale: CGFloat
    let queue: MTLCommandQueue
    let renderer: CARenderer
    /// The canvas's layer under a layer that scales it to pixels.
    let container = CALayer()

    init?(layer: CALayer, size: CGSize, scale: CGFloat) {
        guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else { return nil }
        let w = Int(size.width * scale), h = Int(size.height * scale)
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .bgra8Unorm, width: w, height: h, mipmapped: false)
        desc.usage = [.renderTarget, .shaderRead]
        desc.storageMode = .private
        guard let texture = device.makeTexture(descriptor: desc) else { return nil }
        self.size = size
        self.scale = scale
        self.queue = queue
        renderer = CARenderer(mtlTexture: texture, options: [kCARendererMetalCommandQueue as String: queue])
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        container.anchorPoint = .zero
        container.bounds = CGRect(x: 0, y: 0, width: w, height: h)
        container.position = .zero
        layer.anchorPoint = .zero
        layer.position = .zero
        layer.bounds = CGRect(origin: .zero, size: size)
        layer.setAffineTransform(CGAffineTransform(scaleX: scale, y: scale))
        container.addSublayer(layer)
        CATransaction.commit()
        renderer.layer = container
        renderer.bounds = container.bounds
    }

    /// One frame: the CPU's part (CARenderer's render, which rasterises what changed and encodes
    /// the GPU's work) and the GPU's (to the end of a command buffer queued after it), in ms.
    func frame() -> (cpu: Double, gpu: Double, total: Double) {
        let t0 = CACurrentMediaTime()
        renderer.beginFrame(atTime: t0, timeStamp: nil)
        renderer.addUpdate(renderer.bounds)
        renderer.render()
        renderer.endFrame()
        let t1 = CACurrentMediaTime()
        if let cb = queue.makeCommandBuffer() {
            cb.commit()
            cb.waitUntilCompleted()
        }
        let t2 = CACurrentMediaTime()
        return ((t1 - t0) * 1000, (t2 - t1) * 1000, (t2 - t0) * 1000)
    }
}
#endif
