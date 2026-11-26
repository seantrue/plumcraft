// The Acknowledgements window (FR4, research note 29): the licences of what the app is built from
// and of each connected engine, as the core's model gives them (`acknowledgements.get`), searched
// by the core. The core keeps whether it is open (the scene's `view.windows.acknowledgements`,
// `app.acknowledgements`); the Mac opens a window of its own, the iPad and iPhone a sheet over
// About. Each licence is a section, closed until opened, with its packages and its full text,
// selectable.

import Foundation
import PlumCraft
import PlumCraftKit
import PlumCraftModels
import SwiftUI

/// The window's model: `acknowledgements.get {shell, query}`, read when it opens and as the
/// search changes.
@MainActor
@Observable
public final class AcknowledgementsModel {
    @ObservationIgnored public let model: DocumentModel
    /// `mac` or `ios`: what only another shell ships is left out.
    @ObservationIgnored public let shell: String
    public var reply: JSONValue = .null
    public var query = "" {
        didSet { if query != oldValue { read() } }
    }
    @ObservationIgnored private var task: Task<Void, Never>?
    /// Called after each read (a scripted launch's status file says what the window shows).
    @ObservationIgnored public var onRead: (() -> Void)?

    public init(model: DocumentModel, shell: String) {
        self.model = model
        self.shell = shell
    }

    /// Read the model (again), the search's last word winning.
    public func read() {
        task?.cancel()
        let q = query
        task = Task { [weak self] in
            guard let self else { return }
            if let r = try? await self.model.core.request("acknowledgements.get", ["shell": .string(self.shell), "query": .string(q)], doc: self.model.requestDoc),
               !Task.isCancelled {
                self.reply = r
                self.onRead?()
            }
        }
    }

    /// The person closed it: the core's window closes too.
    public func close() async {
        _ = try? await model.run("app.acknowledgements", ["open": false])
    }

    /// The editor's list and each engine's, as sections: `(id, title, list or nil, why none)`.
    public var programs: [(id: String, title: String, list: JSONValue?, none: String?)] {
        var out: [(String, String, JSONValue?, String?)] = []
        let editor = reply["editor"]
        if editor != .null { out.append(("editor", editor["name"].string ?? "PlumCraft", editor["list"], nil)) }
        for e in reply["engines"].array ?? [] {
            let title = "Engine: \(e["name"].string ?? e["id"].string ?? "")"
            if e["given"].bool == true {
                out.append(("engine:\(e["id"].string ?? "")", title, e["list"], nil))
            } else {
                let why = e["error"].string.map { "No list given: \($0)" } ?? "No list given."
                out.append(("engine:\(e["id"].string ?? "")", title, nil, why))
            }
        }
        return out
    }

    /// `ui.inspect`'s words for it: what it shows.
    public var describe: JSONValue {
        let editor = reply["editor"]["list"]
        return ["query": .string(query), "rows": .uint(UInt64(editor["rows"].array?.count ?? 0)), "packages": editor["packages"],
                "engines": .array((reply["engines"].array ?? []).map { ["id": $0["id"], "given": $0["given"]] })]
    }
}

/// How many packages a list shows, of how many.
func packagesLine(_ list: JSONValue) -> String {
    let all = list["packages"].uint64 ?? 0
    let shown = list["shownPackages"].uint64 ?? all
    return shown == all ? "\(all) packages" : "\(shown) of \(all) packages"
}

/// A licence's section title: its id and its first packages.
func groupTitle(_ g: JSONValue) -> String {
    // each name once (a crate in two versions is two packages)
    var seen = Set<String>()
    let names = (g["packages"].array ?? []).compactMap { $0["name"].string }.filter { seen.insert($0).inserted }
    let more = names.count > 4 ? " and \(names.count - 4) more" : ""
    return "\(g["licence"].string ?? ""): \(names.prefix(4).joined(separator: ", "))\(more)"
}

public struct AcknowledgementsView: SwiftUI.View {
    @Bindable var acks: AcknowledgementsModel
    let done: (() -> Void)?

    public init(acks: AcknowledgementsModel, done: (() -> Void)? = nil) {
        self.acks = acks
        self.done = done
    }

    public var body: some SwiftUI.View {
        NavigationStack {
            List {
                ForEach(acks.programs, id: \.id) { p in
                    Section {
                        if let none = p.none {
                            Text(none).foregroundStyle(.secondary)
                        } else if let list = p.list {
                            if let note = list["note"].string, !note.isEmpty {
                                Text(note).font(.footnote).foregroundStyle(.secondary).textSelection(.enabled)
                            }
                            // keyed by licence: a row's open state stays with its licence as a search changes the list
                            ForEach((list["rows"].array ?? []).map { (id: $0["licence"].string ?? "", row: $0) }, id: \.id) { r in
                                LicenceGroupRow(group: r.row)
                            }
                        }
                    } header: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(p.title).font(.headline)
                            if let list = p.list { Text(packagesLine(list)).font(.caption).foregroundStyle(.secondary) }
                        }
                    }
                }
            }
            .accessibilityIdentifier("acknowledgements-list")
            .searchable(text: $acks.query, prompt: "A licence or a package")
            .navigationTitle("Acknowledgements")
            .toolbar {
                if let done { ToolbarItem(placement: .confirmationAction) { Button("Done", action: done).accessibilityIdentifier("acknowledgements-done") } }
            }
        }
        .onAppear { acks.read() }
    }
}

/// One licence (the core coalesces its texts): closed, its id and first packages; opened, each
/// package with its own copyright lines, then the licence's text once and any variant under
/// the packages whose it is.
struct LicenceGroupRow: SwiftUI.View {
    let group: JSONValue
    @State private var open = false

    var body: some SwiftUI.View {
        DisclosureGroup(isExpanded: $open) {
            if open {
                if let n = group["name"].string, !n.isEmpty { Text(n).font(.subheadline.bold()) }
                ForEach(Array((group["packages"].array ?? []).enumerated()), id: \.offset) { _, p in
                    let also = (p["also"].array ?? []).compactMap(\.string)
                    let line = [p["name"].string, p["version"].string, p["url"].string].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: "  ")
                        + (also.isEmpty ? "" : "  (also under \(also.joined(separator: ", ")))")
                    VStack(alignment: .leading, spacing: 2) {
                        Text(line).font(.footnote).textSelection(.enabled)
                        ForEach(Array((p["notices"].array ?? []).enumerated()), id: \.offset) { _, n in
                            Text(n.string ?? "").font(.caption2).foregroundStyle(.secondary).textSelection(.enabled).padding(.leading, 12)
                        }
                    }
                }
                ForEach(Array((group["texts"].array ?? []).enumerated()), id: \.offset) { _, t in
                    VStack(alignment: .leading, spacing: 4) {
                        if t["standard"].bool != true {
                            Text("As \((t["packages"].array ?? []).compactMap(\.string).joined(separator: ", ")) have it:").font(.footnote.bold())
                        }
                        Text(t["text"].string ?? "").font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
        } label: {
            Text(groupTitle(group)).lineLimit(2)
        }
    }
}
