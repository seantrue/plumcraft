// What the app is animating now (the maintainer, 2026-11-21: "Yes" to "should the app report what
// is animating", for the device re-run of B111): each source of continuing motion says when it
// starts and stops, or how long it lasts, and the shell writes the list to its status (`animating`,
// empty when idle), which the UI tests print on every time-out. Shell state, not the core's.
//
//   pulse(N)        N running faces pulsing (a repeating opacity animation, LayerCanvas)
//   fade            the run's colours fading after it ends
//   viewport        an animated pan or zoom (0.28 s)
//   keyboard-lift   the canvas moved so the row being edited stays above the keyboard
//   momentum        a pan's momentum (the iPad's display link)
//   note            a note flashing under a row

import Foundation
import QuartzCore

@MainActor
public final class Activity {
    public static let shared = Activity()

    private var counts: [String: Int] = [:]
    private var until: [String: CFTimeInterval] = [:]
    private var clear: Task<Void, Never>?
    private var last: [String] = []

    /// Called with the new list whenever it changes (the app writes it to its status).
    public var onChange: (([String]) -> Void)?

    /// `name` is going on `n` times now (0: stopped).
    public func count(_ name: String, _ n: Int) {
        if n > 0 { counts[name] = n } else { counts.removeValue(forKey: name) }
        changed()
    }

    /// `name` goes on for `seconds` from now (an animation of known length).
    public func timed(_ name: String, _ seconds: Double) {
        let end = CACurrentMediaTime() + max(seconds, 0)
        until[name] = max(until[name] ?? 0, end)
        changed()
        clear?.cancel()
        let wait = (until.values.max() ?? end) - CACurrentMediaTime()
        clear = Task { @MainActor [weak self] in
            try? await Task.sleep(for: .seconds(max(wait, 0) + 0.02))
            guard !Task.isCancelled else { return }
            self?.changed()
        }
    }

    /// What is animating now, sorted; empty when idle.
    public var names: [String] {
        let now = CACurrentMediaTime()
        var out = counts.map { $0.value > 1 || $0.key == "pulse" ? "\($0.key)(\($0.value))" : $0.key }
        out += until.filter { $0.value > now }.map(\.key)
        return out.sorted()
    }

    private func changed() {
        let now = CACurrentMediaTime()
        until = until.filter { $0.value > now }
        let n = names
        if n != last {
            last = n
            onChange?(n)
        }
    }
}
