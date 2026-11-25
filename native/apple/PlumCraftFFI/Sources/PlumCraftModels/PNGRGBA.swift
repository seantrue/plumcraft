// An 8-bit RGBA PNG's own bytes, read without the platform's image decoder (S4b): the mask
// editor's files keep the picture's colours where the mask (alpha) is 0, and iOS's decoder
// premultiplies, which turns them black. Reads what the core writes (8 bits a channel, RGBA or
// RGB, not interlaced); anything else is nil, and the caller falls back. Hostile input is
// bounded and never traps.

import Foundation

public enum PNGRGBA {
    /// The largest picture read (pixels).
    public static let maxPixels = 64 << 20

    /// `(w, h, rgba)`, or nil when `data` isn't such a PNG.
    public static func decode(_ data: Data) -> (w: Int, h: Int, rgba: [UInt8])? {
        let b = [UInt8](data)
        guard b.count > 8, b[0..<8].elementsEqual([137, 80, 78, 71, 13, 10, 26, 10]) else { return nil }
        func u32(_ i: Int) -> Int? {
            guard i >= 0, i + 4 <= b.count else { return nil }
            return Int(b[i]) << 24 | Int(b[i + 1]) << 16 | Int(b[i + 2]) << 8 | Int(b[i + 3])
        }
        var i = 8
        var w = 0, h = 0, channels = 0
        var idat = [UInt8]()
        while i + 8 <= b.count {
            guard let len = u32(i), len <= b.count - i - 12 else { return nil }
            let type = String(decoding: b[(i + 4)..<(i + 8)], as: UTF8.self)
            let body = b[(i + 8)..<(i + 8 + len)]
            switch type {
            case "IHDR":
                guard len == 13, let ww = u32(i + 8), let hh = u32(i + 12) else { return nil }
                let depth = body[body.startIndex + 8], color = body[body.startIndex + 9], interlace = body[body.startIndex + 12]
                guard depth == 8, interlace == 0, color == 6 || color == 2 else { return nil }
                (w, h, channels) = (ww, hh, color == 6 ? 4 : 3)
                guard w > 0, h > 0, w <= maxPixels / h else { return nil }
            case "IDAT":
                idat.append(contentsOf: body)
            case "IEND":
                i = b.count
                continue
            default:
                break
            }
            i += 12 + len
        }
        guard w > 0, channels > 0, idat.count > 6 else { return nil }
        // zlib: a 2-byte header and a 4-byte check around a raw deflate stream
        let raw = Data(idat[2..<(idat.count - 4)])
        guard let inflated = try? (raw as NSData).decompressed(using: .zlib) as Data else { return nil }
        let stride = w * channels
        let rows = [UInt8](inflated)
        guard rows.count >= h * (stride + 1) else { return nil }
        var out = [UInt8](repeating: 0, count: w * h * 4)
        var prev = [UInt8](repeating: 0, count: stride)
        var cur = [UInt8](repeating: 0, count: stride)
        for y in 0..<h {
            let row = y * (stride + 1)
            let filter = rows[row]
            for x in 0..<stride {
                let v = rows[row + 1 + x]
                let a = x >= channels ? Int(cur[x - channels]) : 0
                let up = Int(prev[x])
                let c = x >= channels ? Int(prev[x - channels]) : 0
                let p: Int
                switch filter {
                case 0: p = 0
                case 1: p = a
                case 2: p = up
                case 3: p = (a + up) / 2
                case 4:
                    let base = a + up - c
                    let (pa, pb, pc) = (abs(base - a), abs(base - up), abs(base - c))
                    p = pa <= pb && pa <= pc ? a : (pb <= pc ? up : c)
                default: return nil
                }
                cur[x] = UInt8(truncatingIfNeeded: Int(v) + p)
            }
            for x in 0..<w {
                let o = (y * w + x) * 4
                out[o] = cur[x * channels]
                out[o + 1] = cur[x * channels + 1]
                out[o + 2] = cur[x * channels + 2]
                out[o + 3] = channels == 4 ? cur[x * channels + 3] : 255
            }
            swap(&prev, &cur)
        }
        return (w, h, out)
    }
}
