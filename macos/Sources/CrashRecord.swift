import Foundation

/// What last-crash.txt holds: "version <v>", then "signal <NAME>" or "exception <NAME>", then
/// the backtrace lines. Written by the crash handlers, read on the next launch.
struct CrashRecord: Equatable {
    let version: String
    let summary: String
    let isException: Bool
    let backtrace: [String]

    static func parse(_ text: String) -> CrashRecord? {
        let lines = text.split(separator: "\n").map(String.init)
        guard lines.count >= 2, lines[0].hasPrefix("version ") else { return nil }
        let version = String(lines[0].dropFirst("version ".count))
        let cause = lines[1]
        let isException = cause.hasPrefix("exception ")
        let summary = isException ? "uncaught " + cause : cause
        let backtrace = ReportScrubber.ownFunctionNames(fromBacktrace: Array(lines.dropFirst(2)))
        return CrashRecord(version: version, summary: summary, isException: isException, backtrace: backtrace)
    }

    var logLine: String { "crash in \(version): \(summary)" }

    var event: ReportEvent {
        return ReportEvent(
            kind: isException ? "panic" : "crash", message: summary,
            location: backtrace.first ?? "unknown", backtrace: backtrace, counts: ["frames": backtrace.count])
    }
}
