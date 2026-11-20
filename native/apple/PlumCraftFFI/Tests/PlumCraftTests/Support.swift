// What the kit's tests share: a scratch folder per process, the environment a test core runs in,
// a few measurement helpers.

import Foundation
import PlumCraft
import PlumCraftModels

/// The repository's root, from this file's place in it.
let repo: URL = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()

/// Under the scratch root the Rust tests sweep once this process has exited
/// (`plumcraft-test-<pid>`, `plumcraft_scratch::root`).
func scratch(_ name: String) -> String {
    let dir = FileManager.default.temporaryDirectory
        .appendingPathComponent("plumcraft-test-\(ProcessInfo.processInfo.processIdentifier)")
        .appendingPathComponent("swift-\(name)")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.path
}

/// The environment every test core runs in, set before the first opens: preferences in scratch
/// (never the maintainer's), BLAKE3 hashes, a store that takes blobs with 2 GB free.
let testEnvironment: Void = {
    setenv("PLUMCRAFT_EDITOR_PREFS", scratch("prefs") + "/prefs.json", 1)
    // the store's root and marks in scratch whatever the environment says: COMMONKIT_STORE_ROOT
    // wins over the config's store_root, and a login shell may name the maintainer's own store
    // (N3 slice 4 found the tests had been reaching it that way)
    setenv("COMMONKIT_STORE_ROOT", scratch("store"), 1)
    setenv("COMMONKIT_MARK_DIR", scratch("marks"), 1)
    setenv("COMMONKIT_TEST_RUN", scratch("run"), 1)
    setenv("PLUMCRAFT_CRAFT_ROOT", scratch("craft"), 1)
    setenv("COMMONKIT_HASH", "b3", 0)
    setenv("COMMONKIT_STORE_MIN_FREE_GB", "2", 0)
}()

/// A core on a fresh store under scratch, with `extra` config.
func openCore(_ name: String, _ extra: [String: JSONValue] = [:]) throws -> PlumCore {
    _ = testEnvironment
    var config: [String: JSONValue] = ["store_root": .string(scratch("store-\(name)"))]
    for (k, v) in extra { config[k] = v }
    return try PlumCore(config: .object(config))
}

func nowNs() -> UInt64 { DispatchTime.now().uptimeNanoseconds }

func micros(since t0: UInt64) -> Double { Double(nowNs() - t0) / 1000 }

/// Median, p99, min, max of `us`.
func stats(_ us: [Double]) -> JSONValue {
    let s = us.sorted()
    guard let lo = s.first, let hi = s.last else { return [:] }
    let at = { (q: Double) in s[Int((Double(s.count - 1) * q).rounded())] }
    return ["n": .int(Int64(s.count)), "median_us": .double(at(0.5)), "p99_us": .double(at(0.99)), "min_us": .double(lo), "max_us": .double(hi)]
}

/// One `MEASURE {json}` line on stdout, as the Rust measurements print.
func report(_ name: String, _ v: JSONValue) {
    print("MEASURE " + JSONValue.object(["name": .string(name), "from": "swift", "v": v]).text)
}

/// The process's physical footprint (what Xcode's memory gauge shows), in KiB.
func footprintKB() -> UInt64 {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
    let kr = withUnsafeMutablePointer(to: &info) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) }
    }
    return kr == KERN_SUCCESS ? info.phys_footprint / 1024 : 0
}

/// The measurements run only when asked (`PLUMCRAFT_MEASURE=1`), so `swift test` stays quick.
let measuring = ProcessInfo.processInfo.environment["PLUMCRAFT_MEASURE"] == "1"
