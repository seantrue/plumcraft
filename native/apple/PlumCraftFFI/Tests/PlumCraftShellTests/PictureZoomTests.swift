// B159: a pop-out picture's zoom and view in the picture's own terms (`PictureZoom`), which the
// Mac and iPad picture views keep through a list's items.

import CoreGraphics
import Foundation
import PlumCraftModels
import PlumCraftShell
import Testing

@Suite struct PictureZoomTests {
    let pic = CGSize(width: 2400, height: 1600)

    /// The second fault: a view not laid out yet has no fit (it once gave the smallest zoom, 5%).
    @Test func noFitBeforeTheViewHasASize() {
        #expect(PictureZoom.fit(content: .zero, picture: pic) == nil)
        #expect(PictureZoom.fit(content: CGSize(width: 520, height: 0), picture: pic) == nil)
        #expect(PictureZoom.fit(content: CGSize(width: 520, height: 400), picture: .zero) == nil)
        #expect(PictureZoom.fit(content: CGSize(width: 480, height: 400), picture: pic) == 0.2)
        #expect(PictureZoom.fit(content: CGSize(width: 4800, height: 3200), picture: CGSize(width: 10, height: 10)) == 8, "at most 8")
    }

    @Test func asksMakeAKeptZoomOrForgetIt() {
        let now = PictureZoom(scale: 0.2, centre: CGPoint(x: 0.5, y: 0.5))
        // 4 screen pixels a value pixel on a 2x screen: 200%, about row 400, column 600
        let z = PictureZoom.asked(["seq": 1, "to": 4, "at": [400, 600]], current: now, backing: 2, picture: pic)
        #expect(z?.scale == 2)
        #expect(abs((z?.centre.x ?? 0) - 600.5 / 2400) < 1e-9)
        #expect(abs((z?.centre.y ?? 0) - 400.5 / 1600) < 1e-9)
        #expect(PictureZoom.asked(["to": "in"], current: now, backing: 2, picture: pic)?.scale == 0.4)
        #expect(PictureZoom.asked(["to": "out"], current: now, backing: 2, picture: pic)?.scale == 0.1)
        #expect(PictureZoom.asked(["to": "fit"], current: now, backing: 2, picture: pic) == nil, "fit forgets what was kept")
        #expect(PictureZoom.asked(["to": "1:1"], current: now, backing: 2, picture: pic) == nil)
        // only a centre: the zoom stays; a far pixel is held to the picture
        let c = PictureZoom.asked(["at": [99999, 0]], current: now, backing: 2, picture: pic)
        #expect(c?.scale == 0.2)
        #expect(c?.centre.y == 1)
        // junk changes nothing it can't read; the scale is held to the range
        #expect(PictureZoom.asked(["to": "sideways"], current: now, backing: 2, picture: pic) == now)
        #expect(PictureZoom.asked(["to": 100_000], current: now, backing: 1, picture: pic)?.scale == PictureZoom.maxScale)
    }

    /// The rule: the same scale and the same relative centre, on an item of the same size and on
    /// one of another; the view's origin puts that centre in the middle, held inside the picture.
    @Test func theViewIsKeptInThePicturesOwnTerms() {
        let k = PictureZoom(scale: 2, centre: CGPoint(x: 0.25, y: 0.25))
        let view = CGSize(width: 500, height: 400)
        let o = k.origin(content: view, picture: pic)
        #expect(o == CGPoint(x: 0.25 * 4800 - 250, y: 0.25 * 3200 - 200))
        let back = PictureZoom.shown(scale: 2, origin: o, content: view, picture: pic)
        #expect(back == k, "what is shown reads back as what was kept")
        // half as wide: the same relative point
        let narrow = CGSize(width: 1200, height: 1600)
        let o2 = k.origin(content: view, picture: narrow)
        #expect(PictureZoom.shown(scale: 2, origin: o2, content: view, picture: narrow).centre == k.centre)
        // a corner is held inside; a picture smaller than the view is centred
        #expect(PictureZoom(scale: 2, centre: .zero).origin(content: view, picture: pic) == .zero)
        #expect(PictureZoom(scale: 0.1, centre: CGPoint(x: 0.9, y: 0.9)).origin(content: view, picture: pic) == CGPoint(x: (240 - 500) / 2, y: (160 - 400) / 2))
        let facts = k.facts(kept: true, fit: 0.2)
        #expect(facts["percent"].double == 200)
        #expect(facts["kept"].bool == true)
    }
}
