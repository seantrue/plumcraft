// The person check's eyes from outside the app (person-check.sh): what the accessibility API
// gives for a process, as VoiceOver and every assistive app read it. Reads only: it never presses,
// focuses or raises anything, so the app is not activated.
//
//   person-check-ax windows PID   the app's windows: title, role, subrole, position, size, and the
//                                 number of sheets on each
//   person-check-ax press PID T   press the button titled T in a sheet (AXPress)
//   person-check-ax sheet PID     every sheet's elements: role, title, value, description, frame
//                                 (screen points), depth first
//
// Prints JSON; {"trusted": false} when this process may not use accessibility (System Settings ›
// Privacy & Security › Accessibility, for the terminal that runs the check).

import ApplicationServices
import Foundation

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
    var v: AnyObject?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}

func str(_ e: AXUIElement, _ name: String) -> String? {
    if let s = attr(e, name) as? String { return s }
    if let n = attr(e, name) as? NSNumber { return n.stringValue }
    return nil
}

func frame(_ e: AXUIElement) -> [Double]? {
    var p = CGPoint.zero, s = CGSize.zero
    guard let pv = attr(e, kAXPositionAttribute), let sv = attr(e, kAXSizeAttribute) else { return nil }
    // swiftlint:disable force_cast
    guard AXValueGetValue(pv as! AXValue, .cgPoint, &p), AXValueGetValue(sv as! AXValue, .cgSize, &s) else { return nil }
    return [p.x, p.y, s.width, s.height].map(Double.init)
}

func children(_ e: AXUIElement) -> [AXUIElement] {
    (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? []
}

func describe(_ e: AXUIElement) -> [String: Any] {
    var d: [String: Any] = [:]
    for (k, a) in [("role", kAXRoleAttribute), ("subrole", kAXSubroleAttribute), ("title", kAXTitleAttribute),
                   ("value", kAXValueAttribute), ("description", kAXDescriptionAttribute), ("label", "AXLabel"),
                   ("identifier", kAXIdentifierAttribute)] {
        if let s = str(e, a), !s.isEmpty { d[k] = s }
    }
    if let f = frame(e) { d["frame"] = f }
    return d
}

func walk(_ e: AXUIElement, _ depth: Int, into out: inout [[String: Any]]) {
    if depth > 12 { return }
    for c in children(e) {
        var d = describe(c)
        d["depth"] = depth
        out.append(d)
        walk(c, depth + 1, into: &out)
    }
}

let args = CommandLine.arguments
guard args.count >= 3, let pid = pid_t(args[2]) else {
    print("usage: person-check-ax windows|sheet PID")
    exit(2)
}
var result: [String: Any] = ["trusted": AXIsProcessTrusted()]
if AXIsProcessTrusted() {
    let app = AXUIElementCreateApplication(pid)
    let windows = (attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? []
    switch args[1] {
    case "windows":
        result["windows"] = windows.map { w -> [String: Any] in
            var d = describe(w)
            d["sheets"] = children(w).filter { str($0, kAXRoleAttribute) == "AXSheet" }.count
            return d
        }
    case "sheet":
        var sheets: [[String: Any]] = []
        for w in windows {
            for s in children(w) where str(s, kAXRoleAttribute) == "AXSheet" {
                var els: [[String: Any]] = []
                walk(s, 0, into: &els)
                var d = describe(s)
                d["window"] = str(w, kAXTitleAttribute) ?? ""
                d["elements"] = els
                sheets.append(d)
            }
        }
        result["sheets"] = sheets
    case "set" where args.count >= 5:
        // a sheet's text field (by its accessibility identifier) given a value, as dictation or
        // an assistive app types it
        var done = false
        func find(_ e: AXUIElement, _ depth: Int) -> AXUIElement? {
            if depth > 14 { return nil }
            for c in children(e) {
                if str(c, kAXIdentifierAttribute) == args[3] { return c }
                if let f = find(c, depth + 1) { return f }
            }
            return nil
        }
        for w in windows {
            for s in children(w) where str(s, kAXRoleAttribute) == "AXSheet" {
                if let f = find(s, 0) {
                    done = AXUIElementSetAttributeValue(f, kAXValueAttribute as CFString, args[4] as CFString) == .success
                    result["value"] = str(f, kAXValueAttribute) ?? ""
                }
            }
        }
        result["set"] = done
    case "press" where args.count >= 4:
        // a sheet's button pressed as an assistive app presses it (the save panel runs out of
        // process, so the app can't click its buttons itself); never focuses or raises
        var pressed = false
        func find(_ e: AXUIElement, _ depth: Int) -> AXUIElement? {
            if depth > 14 { return nil }
            for c in children(e) {
                if str(c, kAXRoleAttribute) == "AXButton" && [kAXTitleAttribute, kAXDescriptionAttribute, kAXIdentifierAttribute].contains(where: { str(c, $0) == args[3] }) { return c }
                if let f = find(c, depth + 1) { return f }
            }
            return nil
        }
        for w in windows {
            for s in children(w) where str(s, kAXRoleAttribute) == "AXSheet" {
                if let b = find(s, 0) { pressed = AXUIElementPerformAction(b, kAXPressAction as CFString) == .success }
            }
        }
        // no sheet has it: a window's own button (a pop-out's bar), the window titled args[4]
        if !pressed {
            for w in windows where args.count < 5 || str(w, kAXTitleAttribute) == args[4] {
                if let b = find(w, 0) {
                    pressed = AXUIElementPerformAction(b, kAXPressAction as CFString) == .success
                    break
                }
            }
        }
        result["pressed"] = pressed
    default:
        print("usage: person-check-ax windows|sheet PID")
        exit(2)
    }
}
let data = (try? JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])) ?? Data("{}".utf8)
print(String(decoding: data, as: UTF8.self))
