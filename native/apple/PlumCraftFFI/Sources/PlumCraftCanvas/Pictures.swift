// Pictures the core lends, as CGImages: a face's rgba8 preview, the Watched panel's picture.
// `media.get {handle, want: "rgba"}` says at once whether the picture is ready (then
// `plum_blob(key)` lends its bytes without a copy) or on its way (the feed's `media` says when).
// The bytes become a CGImage on a worker, drawn once into a premultiplied bitmap so the render
// server never decodes on the main thread, and the lent bytes go back. A handle names the same
// bytes forever, so it is the cache's key (contract 10.1).

import CoreGraphics
import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels

/// A picture made from what the core lent: the image and its size in pixels.
public struct LoadedPicture: @unchecked Sendable {
    // CGImage is immutable and thread-safe
    public let image: CGImage
    public let width: Int
    public let height: Int
    public var bytes: Int { width * height * 4 }
}

@MainActor
public final class Pictures {
    public let model: DocumentModel
    /// The most bytes of pictures kept (least recently used go first).
    public var budget: Int
    private var cache: [String: LoadedPicture] = [:]
    private var order: [String] = []
    private var loading: [String: Task<LoadedPicture?, Never>] = [:]
    /// Why the last load of a key failed, for the viewer to say.
    public private(set) var failures: [String: String] = [:]

    public init(model: DocumentModel, budget: Int = 128 << 20) {
        self.model = model
        self.budget = budget
    }

    /// The handle a value reference names (a scene preview's `handle`, a watched picture's `ref`).
    public nonisolated static func handle(of ref: JSONValue) -> String? {
        ref["handle"].string ?? ref.string
    }

    /// A picture already made, without waiting.
    public func cached(_ handle: String) -> LoadedPicture? {
        guard let p = cache[handle] else { return nil }
        touch(handle)
        return p
    }

    /// The rgba8 picture `handle` names: made on a worker, cached by handle. Nil when the core
    /// can't give it (the reason is in `failures`), or it didn't come within `timeout`.
    /// `want`: `rgba` (an engine's rendition), `image` (a picture file decoded at most `px` a side),
    /// `svg` (drawn `px` on its long side) or `spectrogram` (a sound's, N4 slice 5); the cache's key
    /// is the derived key, so each is kept apart.
    public func load(_ handle: String, want: String = "rgba", px: Int? = nil, timeout: Duration = .seconds(30)) async -> LoadedPicture? {
        let key = want == "rgba" ? handle : "\(want)@\(px ?? 0)|\(handle)"
        if let p = cached(key) { return p }
        if let t = loading[key] { return await t.value }
        let t = Task { [weak self] () -> LoadedPicture? in
            guard let self else { return nil }
            return await self.fetch(handle, want: want, px: px, failKey: key, timeout: timeout)
        }
        loading[key] = t
        let p = await t.value
        loading[key] = nil
        if let p {
            put(key, p)
            failures[key] = nil
            if key != handle { failures[handle] = nil }
        } else if key != handle {
            failures[handle] = failures[key]
        }
        return p
    }

    /// The bytes `want` of `handle` names, lent by the core without a copy, once ready (a sound's
    /// samples, its waveform's peaks): the note says their shape. Nil (with the reason in
    /// `failures`) when the core can't give them.
    public func lend(_ handle: String, want: String = "bytes", px: Int? = nil, timeout: Duration = .seconds(60)) async -> (MediaNote, Data)? {
        guard let note = await ready(handle, want: want, px: px, failKey: handle, timeout: timeout) else { return nil }
        let core = model.core
        let key = note.key
        let got = await Task.detached(priority: .userInitiated) { () -> Data? in
            guard case .ready(let blob)? = try? core.blob(key) else { return nil }
            return blob.data
        }.value
        guard let got else {
            failures[handle] = "the bytes went before they were read"
            return nil
        }
        return (note, got)
    }

    private func ready(_ handle: String, want: String, px: Int?, failKey: String, timeout: Duration) async -> MediaNote? {
        let core = model.core
        var p: [String: JSONValue] = ["handle": .string(handle), "want": .string(want)]
        if let px { p["px"] = .uint(UInt64(max(1, min(px, 4096)))) }
        let note: MediaNote
        do {
            note = try await core.request(MediaNote.self, "media.get", .object(p), doc: model.requestDoc)
        } catch {
            failures[failKey] = "\(error)"
            return nil
        }
        var latest = note
        if latest.status.word == .pending {
            let key = note.key
            let ok = await model.wait(timeout: timeout) {
                self.model.media[key].map { $0.status.word != .pending } ?? false
            }
            guard ok, let m = model.media[key] else {
                failures[failKey] = "the value didn't come in time"
                return nil
            }
            latest = m
        }
        guard latest.status.word == .ready else {
            failures[failKey] = latest.reason ?? "the value is \(latest.status)"
            return nil
        }
        return latest
    }

    private func fetch(_ handle: String, want: String, px: Int?, failKey: String, timeout: Duration) async -> LoadedPicture? {
        let core = model.core
        let note: MediaNote
        do {
            var p: [String: JSONValue] = ["handle": .string(handle), "want": .string(want)]
            if let px { p["px"] = .uint(UInt64(max(1, min(px, 4096)))) }
            note = try await core.request(MediaNote.self, "media.get", .object(p), doc: model.requestDoc)
        } catch {
            failures[failKey] = "\(error)"
            return nil
        }
        var latest = note
        if latest.status.word == .pending {
            let key = note.key
            let ok = await model.wait(timeout: timeout) {
                self.model.media[key].map { $0.status.word != .pending } ?? false
            }
            guard ok, let m = model.media[key] else {
                failures[failKey] = "the picture didn't come in time"
                return nil
            }
            latest = m
        }
        guard latest.status.word == .ready, let w = latest.w, let h = latest.h, w > 0, h > 0 else {
            failures[failKey] = latest.reason ?? "the picture is \(latest.status)"
            return nil
        }
        let key = latest.key
        let made = await Task.detached(priority: .userInitiated) { () -> Result<LoadedPicture, PlumError> in
            do {
                guard case .ready(let blob) = try core.blob(key) else { return .failure(PlumError(.decode, "the picture went pending again")) }
                guard let p = Pictures.make(blob.data, width: Int(w), height: Int(h)) else {
                    return .failure(PlumError(.decode, "\(blob.data.count) bytes aren't a \(w)×\(h) rgba8 picture"))
                }
                return .success(p)
            } catch let e as PlumError {
                return .failure(e)
            } catch {
                return .failure(PlumError(.decode, "\(error)"))
            }
        }.value
        switch made {
        case .success(let p): return p
        case .failure(let e):
            failures[failKey] = e.description
            return nil
        }
    }

    /// rgba8 (straight alpha, sRGB, packed rows, contract 10.2) into a premultiplied bitmap:
    /// one copy, made here, off the main thread, so drawing it never decodes.
    public nonisolated static func make(_ data: Data, width: Int, height: Int) -> LoadedPicture? {
        let stride = width * 4
        guard width > 0, height > 0, data.count >= stride * height, let space = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        guard let provider = CGDataProvider(data: data as CFData),
              let straight = CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: stride, space: space,
                                     bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue), provider: provider, decode: nil,
                                     shouldInterpolate: true, intent: .defaultIntent),
              let ctx = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        ctx.draw(straight, in: CGRect(x: 0, y: 0, width: width, height: height))
        guard let image = ctx.makeImage() else { return nil }
        return LoadedPicture(image: image, width: width, height: height)
    }

    private func touch(_ k: String) {
        if let i = order.firstIndex(of: k) { order.remove(at: i) }
        order.append(k)
    }

    private func put(_ k: String, _ p: LoadedPicture) {
        cache[k] = p
        touch(k)
        var total = cache.values.reduce(0) { $0 + $1.bytes }
        while total > budget, order.count > 1, let first = order.first {
            order.removeFirst()
            total -= cache.removeValue(forKey: first)?.bytes ?? 0
        }
    }

    /// Keep a picture made here (a face's waveform), under its own key.
    public func keep(_ key: String, _ p: LoadedPicture) {
        put(key, p)
    }

    /// Drop everything kept (a memory warning).
    public func drop() {
        cache = [:]
        order = []
    }
}
