// The core's log into the unified log (plan 9, contract 15: "the unified log on Apple"; N3 slice 5).
// The feed says when the log has lines the shell hasn't read (`log {seq, worst}`); this reads them
// with `log.tail {after}` and writes each to os_log under the app's subsystem, with the core's
// source as the category, so `log stream --predicate 'subsystem == "…"'` on the Mac (or Console,
// or `devicectl` for a device) shows both sides. The shell's own lines go through `say`, which
// writes to the unified log and to the core's (`log.write`), so one tail shows both.

import Foundation
import os
import PlumCraft
import PlumCraftModels

@MainActor
public final class UnifiedLog {
    public let subsystem: String
    private let core: PlumCore
    private var loggers: [String: Logger] = [:]
    private var after: UInt64 = 0
    private var busy = false
    private var again = false
    private var task: Task<Void, Never>?
    /// Lines forwarded so far (for scripts and tests).
    public private(set) var forwarded = 0

    public init(core: PlumCore, subsystem: String) {
        self.core = core
        self.subsystem = subsystem
    }

    /// Follow the feed's log marks until `stop`.
    public func start() {
        guard task == nil else { return }
        let stream = core.feed()
        task = Task { [weak self] in
            for await batch in stream where batch.log != nil {
                self?.pull()
            }
        }
        pull()
    }

    public func stop() {
        task?.cancel()
        task = nil
    }

    func logger(_ category: String) -> Logger {
        if let l = loggers[category] { return l }
        let l = Logger(subsystem: subsystem, category: category)
        loggers[category] = l
        return l
    }

    /// A line of the shell's own: to the unified log at once, and to the core's log as `shell`.
    public func say(_ level: String, _ message: String) {
        write(level: level, source: "app", message: message)
        _ = core.post("log.write", ["level": .string(level), "message": .string(message)])
    }

    private func write(level: String, source: String, message: String) {
        let l = logger(source)
        switch level {
        case "error": l.error("\(message, privacy: .public)")
        case "warn": l.warning("\(message, privacy: .public)")
        case "info": l.info("\(message, privacy: .public)")
        default: l.debug("\(message, privacy: .public)")
        }
    }

    /// Read what the core logged since the last read (coalesced: one read at a time).
    private func pull() {
        if busy {
            again = true
            return
        }
        busy = true
        Task {
            while true {
                guard let r = try? await core.request("log.tail", ["after": .uint(after), "limit": 500]) else { break }
                let entries = r["entries"].array ?? []
                for e in entries {
                    // the app's own lines are already in the unified log
                    let source = e["source"].string ?? "core"
                    if source == "shell" { continue }
                    write(level: e["level"].string ?? "info", source: source, message: e["message"].string ?? "")
                    forwarded += 1
                }
                if let next = r["next"].uint64, next > after { after = next } else { break }
                if entries.count < 500 { break }
            }
            busy = false
            if again {
                again = false
                pull()
            }
        }
    }
}
