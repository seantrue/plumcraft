// The Apple apps' icon pictures, drawn with Core Graphics (no other tool needed), for make-icons.sh.
//
//   swift icon.swift placeholder-ios OUT.png    a 1024 px opaque square (iOS masks the corners)
//   swift icon.swift placeholder-mac OUT.png    a 1024 px rounded tile on a clear surround (the Mac's grid)
//   swift icon.swift opaque IN.png OUT.png      IN at 1024 px over a solid plum, with no alpha left
//                                               (the App Store refuses an iOS icon with transparency)
//
// The placeholder is a PLACEHOLDER: a plain plum tile with two node faces and a wire. It is there so
// archives validate; the real icon replaces it by dropping one 1024 px picture into make-icons.sh.
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

let size = 1024
let plum = CGColor(srgbRed: 0.44, green: 0.18, blue: 0.40, alpha: 1)
let plumDeep = CGColor(srgbRed: 0.30, green: 0.10, blue: 0.28, alpha: 1)
let face = CGColor(srgbRed: 0.98, green: 0.95, blue: 0.97, alpha: 1)
let wire = CGColor(srgbRed: 0.98, green: 0.80, blue: 0.40, alpha: 1)

func context(alpha: Bool) -> CGContext? {
    let info = alpha ? CGImageAlphaInfo.premultipliedLast.rawValue : CGImageAlphaInfo.noneSkipLast.rawValue
    return CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                     space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: info)
}

/// Two node faces and a wire between them, inside `r`.
func glyph(_ c: CGContext, in r: CGRect) {
    let w = r.width, h = r.height
    let a = CGRect(x: r.minX + w * 0.08, y: r.minY + h * 0.52, width: w * 0.34, height: h * 0.26)
    let b = CGRect(x: r.minX + w * 0.58, y: r.minY + h * 0.22, width: w * 0.34, height: h * 0.26)
    let out = CGPoint(x: a.maxX, y: a.midY), inn = CGPoint(x: b.minX, y: b.midY)
    let p = CGMutablePath()
    p.move(to: out)
    p.addCurve(to: inn, control1: CGPoint(x: out.x + w * 0.16, y: out.y), control2: CGPoint(x: inn.x - w * 0.16, y: inn.y))
    c.setStrokeColor(wire); c.setLineWidth(w * 0.035); c.setLineCap(.round)
    c.addPath(p); c.strokePath()
    for f in [a, b] {
        c.setFillColor(face)
        c.addPath(CGPath(roundedRect: f, cornerWidth: w * 0.04, cornerHeight: w * 0.04, transform: nil)); c.fillPath()
    }
    c.setFillColor(wire)
    for pin in [out, inn] { c.fillEllipse(in: CGRect(x: pin.x - w * 0.03, y: pin.y - w * 0.03, width: w * 0.06, height: w * 0.06)) }
}

func tile(_ c: CGContext, _ r: CGRect, radius: CGFloat) {
    let path = CGPath(roundedRect: r, cornerWidth: radius, cornerHeight: radius, transform: nil)
    c.saveGState(); c.addPath(path); c.clip()
    let g = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [plum, plumDeep] as CFArray, locations: [0, 1])!
    c.drawLinearGradient(g, start: CGPoint(x: r.midX, y: r.maxY), end: CGPoint(x: r.midX, y: r.minY), options: [])
    c.restoreGState()
}

func write(_ c: CGContext, _ path: String) -> Bool {
    guard let img = c.makeImage(),
          let d = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil)
    else { return false }
    CGImageDestinationAddImage(d, img, nil)
    return CGImageDestinationFinalize(d)
}

func fail(_ s: String) -> Never { FileHandle.standardError.write(Data("icon.swift: \(s)\n".utf8)); exit(2) }

let args = CommandLine.arguments
guard args.count >= 3 else { fail("usage: placeholder-ios OUT | placeholder-mac OUT | opaque IN OUT") }
let full = CGRect(x: 0, y: 0, width: size, height: size)
switch args[1] {
case "placeholder-ios":
    guard let c = context(alpha: false) else { fail("no context") }
    tile(c, full, radius: 0)
    glyph(c, in: full.insetBy(dx: 120, dy: 120))
    if !write(c, args[2]) { fail("can't write \(args[2])") }
case "placeholder-mac":
    guard let c = context(alpha: true) else { fail("no context") }
    // the Mac's icon grid: an 824 px tile centred in 1024, corner radius about 185
    let t = full.insetBy(dx: 100, dy: 100)
    tile(c, t, radius: 185)
    glyph(c, in: t.insetBy(dx: 90, dy: 90))
    if !write(c, args[2]) { fail("can't write \(args[2])") }
case "opaque":
    guard args.count >= 4 else { fail("usage: opaque IN OUT") }
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: args[2]) as CFURL, nil),
          let img = CGImageSourceCreateImageAtIndex(src, 0, nil) else { fail("can't read \(args[2])") }
    guard let c = context(alpha: false) else { fail("no context") }
    c.setFillColor(plum); c.fill(full)
    c.interpolationQuality = .high
    c.draw(img, in: full)
    if !write(c, args[3]) { fail("can't write \(args[3])") }
default:
    fail("unknown mode \(args[1])")
}
