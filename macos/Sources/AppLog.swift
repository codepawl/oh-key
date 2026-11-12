import Foundation

/// A small append-only log in ~/Library/Application Support/oh-key/oh-key.log. Each line is
/// "<unix seconds> <message>", the same format as the Windows app. Past the size cap the file
/// is deleted and started again. Callers never pass typed text, keys, clipboard or window titles.
enum AppLog {
    static let maximumBytes: UInt64 = 1_000_000

    static var logURL: URL { Preferences.folderURL.appendingPathComponent("oh-key.log") }

    static func formattedLine(_ message: String, at date: Date) -> String {
        let singleLine = message.replacingOccurrences(of: "\n", with: " ").replacingOccurrences(of: "\r", with: " ")
        return "\(Int(date.timeIntervalSince1970)) \(singleLine)\n"
    }

    static func write(_ message: String, to url: URL = logURL, at date: Date = Date()) {
        let folder = url.deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        deleteWhenTooLarge(url)
        guard let data = formattedLine(message, at: date).data(using: .utf8) else { return }
        if !FileManager.default.fileExists(atPath: url.path) {
            FileManager.default.createFile(atPath: url.path, contents: nil)
        }
        guard let handle = try? FileHandle(forWritingTo: url) else { return }
        defer { try? handle.close() }
        _ = try? handle.seekToEnd()
        try? handle.write(contentsOf: data)
    }

    static func readAll(from url: URL = logURL) -> String {
        return (try? String(contentsOf: url, encoding: .utf8)) ?? ""
    }

    private static func deleteWhenTooLarge(_ url: URL) {
        let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
        let size = (attributes?[.size] as? NSNumber)?.uint64Value ?? 0
        if size > maximumBytes {
            try? FileManager.default.removeItem(at: url)
        }
    }
}
