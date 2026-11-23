// Files a person picked for a path input on a platform where a picked file is reachable only
// through a security-scoped URL (iPadOS, iOS: the document picker), B133. The picker makes a
// copy; it is kept in a folder of the app's (the iPad app: Documents › Picked Files, which the
// Files app shows as On My iPad › Plummer › Picked Files), and that copy's path is what the row
// names, so an engine can open it on every run, after a relaunch too.
//
//   by its own name          the copy keeps the picked file's name, so the row shows a name the
//                            person recognises
//   the same file again      the same bytes already kept under that name: that file, no second copy
//   another file, same name  numbered as Finder does: "notes 2.txt", "notes 3.txt", …
//   earlier copies           left as they are: another graph may name them

import Foundation

public enum PickedFiles {
    /// The folder's name in the app's Documents.
    public static let folderName = "Picked Files"

    /// Keep `source` (a file the picker copied, or any readable file) in `dir`: its path there,
    /// or nil when it can't be read or copied. The source itself is left as it is.
    public static func keep(_ source: URL, in dir: URL) -> URL? {
        let fm = FileManager.default
        guard (try? fm.createDirectory(at: dir, withIntermediateDirectories: true)) != nil else { return nil }
        let name = source.lastPathComponent
        let stem = (name as NSString).deletingPathExtension
        let ext = (name as NSString).pathExtension
        for k in 1...999 {
            let candidate = k == 1 ? name : (ext.isEmpty ? "\(stem) \(k)" : "\(stem) \(k).\(ext)")
            let target = dir.appendingPathComponent(candidate, isDirectory: false)
            if fm.fileExists(atPath: target.path) {
                if fm.contentsEqual(atPath: target.path, andPath: source.path) { return target }
                continue
            }
            guard (try? fm.copyItem(at: source, to: target)) != nil else { return nil }
            return target
        }
        return nil
    }

    // MARK: Photos as a source (feat/path-photos)

    /// The extensions of pictures the photo library can hand over (as themselves, or converted
    /// to PNG or JPEG).
    public static let pictureExtensions: Set<String> = ["png", "jpg", "jpeg", "heic", "heif", "gif", "tif", "tiff", "webp", "bmp"]

    /// Whether a path ask's kinds are pictures only, so the photo library can answer it as well
    /// as the files: every extension a picture's, and at least one.
    public static func picturesOnly(_ extensions: [String]) -> Bool {
        let e = extensions.map { $0.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ".* ")) }
        return !e.isEmpty && e.allSatisfy { pictureExtensions.contains($0) }
    }

    /// A photo's file name: the name the library suggests (its original file's stem, such as
    /// IMG_0042), else the moment it was picked, with the extension of the form it is kept in.
    public static func photoName(suggested: String?, ext: String, at date: Date = Date()) -> String {
        let bad = CharacterSet(charactersIn: "/:\\\0")
        let stem = (suggested ?? "").components(separatedBy: bad).joined(separator: "-").trimmingCharacters(in: .whitespaces)
        let base: String
        if stem.isEmpty {
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US_POSIX")
            f.dateFormat = "yyyy-MM-dd 'at' HH.mm.ss"
            base = "Photo \(f.string(from: date))"
        } else {
            base = (stem as NSString).deletingPathExtension.isEmpty ? stem : (stem as NSString).deletingPathExtension
        }
        return ext.isEmpty ? base : "\(base).\(ext.lowercased())"
    }
}
