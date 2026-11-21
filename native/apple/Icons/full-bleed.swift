// The iOS icon's full-bleed master from a Mac-style icon (a rounded tile on a clear surround), with
// no new artwork: the largest centred square of the tile that is wholly opaque (inside its rounded
// corners and its edge's antialiasing), cropped and scaled to 1024 px, written with no alpha.
// iOS masks the corners itself, and the App Store refuses an iOS icon with transparency.
//
//   swift full-bleed.swift IN.png OUT.png
//
// Used by make-plum-icons.sh for the egui editor's plums (assets/app-icon/plumcraft.png).
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

func fail(_ s: String) -> Never {
    FileHandle.standardError.write(("full-bleed: " + s + "\n").data(using: .utf8)!)
    exit(1)
}

let args = CommandLine.arguments
guard args.count == 3 else { fail("usage: swift full-bleed.swift IN.png OUT.png") }
guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: args[1]) as CFURL, nil),
      let img = CGImageSourceCreateImageAtIndex(src, 0, nil) else { fail("can't read \(args[1])") }
let w = img.width, h = img.height
guard w == h else { fail("\(args[1]) is \(w)x\(h), not square") }

// the picture's pixels, RGBA, to read the alpha
let srgb = CGColorSpace(name: CGColorSpace.sRGB)!
guard let rgba = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: srgb,
                           bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
      let px = rgba.data?.assumingMemoryBound(to: UInt8.self) else { fail("no context") }
rgba.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
func alpha(_ x: Int, _ y: Int) -> UInt8 { px[(y * w + x) * 4 + 3] }

// the largest centred square whose border is wholly opaque (the tile is convex, so its inside is too)
var half = w / 2
func opaque(_ half: Int) -> Bool {
    let lo = w / 2 - half, hi = w / 2 + half - 1
    for i in lo...hi where alpha(i, lo) < 255 || alpha(i, hi) < 255 || alpha(lo, i) < 255 || alpha(hi, i) < 255 { return false }
    return true
}
while half > 0 && !opaque(half) { half -= 1 }
guard half >= w / 4 else { fail("no opaque square of at least half the picture: is it a tile?") }
let side = half * 2, lo = w / 2 - half
// one more pixel in from each side, clear of any edge that is opaque but blended
let crop = CGRect(x: lo + 1, y: lo + 1, width: side - 2, height: side - 2)
guard let part = img.cropping(to: crop) else { fail("crop failed") }

let out = 1024
guard let c = CGContext(data: nil, width: out, height: out, bitsPerComponent: 8, bytesPerRow: 0, space: srgb,
                        bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { fail("no context") }
c.interpolationQuality = .high
c.draw(part, in: CGRect(x: 0, y: 0, width: out, height: out))
guard let made = c.makeImage(),
      let d = CGImageDestinationCreateWithURL(URL(fileURLWithPath: args[2]) as CFURL, UTType.png.identifier as CFString, 1, nil)
else { fail("can't write \(args[2])") }
CGImageDestinationAddImage(d, made, nil)
guard CGImageDestinationFinalize(d) else { fail("can't write \(args[2])") }
print("full-bleed: \(args[1]) → \(args[2]): the centred \(side - 2) px square of a \(w) px picture, scaled to \(out) px, opaque")
