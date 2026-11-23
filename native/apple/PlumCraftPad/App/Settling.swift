// What is still moving that an action sent now would be lost to (decision 8B, the maintainer,
// 2026-11-23: the UI tests wait on the app's own state in place of a fixed pause before each
// action). Written to the status as `settling`, a sorted list, empty when nothing is moving, only
// while something reads the status (a script or a UI test):
//
//   presenting   a sheet, popover, alert or pushed screen is coming or going (a view controller
//                being presented or dismissed, or with a transition under way)
//   keyboard     the on-screen keyboard is moving (between its will- and did- notifications, at
//                most the animation's own length)
//   menu         a menu is opening or closing (its container still has animations on its layers)
//
// Shell state, not the core's. A display link looks once a frame while the status is read.

import PlumCraftModels
import PlumCraftShell
import QuartzCore
import UIKit

@MainActor
final class Settling: NSObject {
    static let shared = Settling()

    private var link: CADisplayLink?
    private var keyboardUntil: CFTimeInterval = 0
    private var last: [String]?

    func start() {
        let nc = NotificationCenter.default
        let will = [UIResponder.keyboardWillShowNotification, UIResponder.keyboardWillHideNotification, UIResponder.keyboardWillChangeFrameNotification]
        for name in will {
            nc.addObserver(forName: name, object: nil, queue: .main) { n in
                let d = (n.userInfo?[UIResponder.keyboardAnimationDurationUserInfoKey] as? Double) ?? 0.25
                MainActor.assumeIsolated {
                    // a little past the animation's own length: its last frame is drawn after it
                    Settling.shared.keyboardUntil = max(Settling.shared.keyboardUntil, CACurrentMediaTime() + d + 0.05)
                }
            }
        }
        for name in [UIResponder.keyboardDidShowNotification, UIResponder.keyboardDidHideNotification, UIResponder.keyboardDidChangeFrameNotification] {
            nc.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { Settling.shared.keyboardUntil = 0 }
            }
        }
        let l = CADisplayLink(target: self, selector: #selector(tick))
        l.add(to: .main, forMode: .common)
        link = l
    }

    @objc private func tick() {
        guard StatusFile.shared.active else { return }
        let n = names()
        guard n != last else { return }
        last = n
        StatusFile.shared.set("settling", .array(n.map(JSONValue.string)))
    }

    /// What is moving now, sorted; empty when nothing is.
    func names() -> [String] {
        var out = Set<String>()
        if CACurrentMediaTime() < keyboardUntil { out.insert("keyboard") }
        let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.flatMap(\.windows)
        for w in windows where !w.isHidden {
            if Self.transitioning(w.rootViewController) { out.insert("presenting") }
            if Self.menuMoving(in: w, depth: 0) { out.insert("menu") }
        }
        return out.sorted()
    }

    /// A view controller in the chain from `vc` (what it presents, its children) coming or going.
    private static func transitioning(_ vc: UIViewController?) -> Bool {
        guard let vc else { return false }
        if vc.isBeingPresented || vc.isBeingDismissed || vc.transitionCoordinator != nil { return true }
        if vc.children.contains(where: { transitioning($0) }) { return true }
        // the presented one, unless it is this one's (a child's presentation is its parent's too)
        if let p = vc.presentedViewController, p.presentingViewController === vc { return transitioning(p) }
        return false
    }

    /// A menu's container near the top of the window with animations still on its layers.
    private static func menuMoving(in v: UIView, depth: Int) -> Bool {
        for s in v.subviews where !s.isHidden {
            if String(describing: type(of: s)).contains("ContextMenu") {
                if animating(s.layer, depth: 0) { return true }
            } else if depth < 3, menuMoving(in: s, depth: depth + 1) {
                return true
            }
        }
        return false
    }

    private static func animating(_ l: CALayer, depth: Int) -> Bool {
        if !(l.animationKeys() ?? []).isEmpty { return true }
        guard depth < 8 else { return false }
        return (l.sublayers ?? []).contains { animating($0, depth: depth + 1) }
    }
}
