import Foundation

/// Settings shared by every text field, kept in the same config.toml format as the Windows app.
///
/// The files live in ~/Library/Application Support/oh-key. The mode (Vietnamese or English) is
/// global and in memory only: the file's start_vietnamese says how a fresh start begins.
enum Preferences {
    static var vietnameseEnabled: Bool = loadStartsVietnamese()

    static var folderURL: URL {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent("oh-key", isDirectory: true)
    }

    static var configURL: URL { folderURL.appendingPathComponent("config.toml") }
    static var macrosURL: URL { folderURL.appendingPathComponent("macros.txt") }

    static func readText(at url: URL) -> String {
        return (try? String(contentsOf: url, encoding: .utf8)) ?? ""
    }

    static func saveInputMethod(_ code: UInt32) {
        guard let session = oh_key_session_new() else { return }
        defer { oh_key_session_free(session) }
        load(settings: readText(at: configURL), into: session)
        oh_key_session_set_input_method(session, code)
        var textPointer: UnsafePointer<UInt8>? = nil
        var textLength = 0
        oh_key_session_settings_text(session, &textPointer, &textLength)
        guard let textPointer = textPointer else { return }
        let data = Data(bytes: textPointer, count: textLength)
        try? FileManager.default.createDirectory(at: folderURL, withIntermediateDirectories: true)
        let temporaryURL = configURL.appendingPathExtension("tmp")
        try? data.write(to: temporaryURL)
        _ = try? FileManager.default.replaceItemAt(configURL, withItemAt: temporaryURL)
    }

    static func load(settings text: String, into session: OpaquePointer) {
        let bytes = Array(text.utf8)
        bytes.withUnsafeBufferPointer { buffer in
            buffer.baseAddress?.withMemoryRebound(to: CChar.self, capacity: bytes.count) { pointer in
                oh_key_session_load_settings(session, pointer, bytes.count)
            }
        }
    }

    static func load(macros text: String, into session: OpaquePointer) {
        let bytes = Array(text.utf8)
        bytes.withUnsafeBufferPointer { buffer in
            buffer.baseAddress?.withMemoryRebound(to: CChar.self, capacity: bytes.count) { pointer in
                oh_key_session_load_macros(session, pointer, bytes.count)
            }
        }
    }

    private static func loadStartsVietnamese() -> Bool {
        guard let session = oh_key_session_new() else { return true }
        defer { oh_key_session_free(session) }
        load(settings: readText(at: configURL), into: session)
        return oh_key_session_starts_vietnamese(session) != 0
    }
}
