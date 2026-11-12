import Foundation

/// File descriptor of last-crash.txt, opened at launch so the signal handler only calls write().
private var crashFileDescriptor: Int32 = -1

private func signalName(_ signalNumber: Int32) -> String {
    switch signalNumber {
    case SIGSEGV: return "SIGSEGV"
    case SIGABRT: return "SIGABRT"
    case SIGILL: return "SIGILL"
    case SIGBUS: return "SIGBUS"
    case SIGFPE: return "SIGFPE"
    case SIGTRAP: return "SIGTRAP"
    default: return "signal \(signalNumber)"
    }
}

private func handleFatalSignal(_ signalNumber: Int32) {
    var causeLine = Array("signal \(signalName(signalNumber))\n".utf8)
    write(crashFileDescriptor, &causeLine, causeLine.count)
    var frames = [UnsafeMutableRawPointer?](repeating: nil, count: 64)
    let frameCount = backtrace(&frames, 64)
    backtrace_symbols_fd(&frames, frameCount, crashFileDescriptor)
    signal(signalNumber, SIG_DFL)
    raise(signalNumber)
}

/// Keeps what killed the input method process in last-crash.txt; the next launch logs it and
/// sends a report. The macOS system restarts an input method that dies, so nothing respawns it here.
enum CrashCapture {
    static var pendingURL: URL { Preferences.folderURL.appendingPathComponent("last-crash.txt") }

    /// Call after `takePending()`: it opens a fresh file for the handlers.
    static func install() {
        try? FileManager.default.createDirectory(at: Preferences.folderURL, withIntermediateDirectories: true)
        crashFileDescriptor = open(pendingURL.path, O_WRONLY | O_CREAT | O_TRUNC, 0o600)
        writeHeader()
        NSSetUncaughtExceptionHandler { exception in
            // Only the exception's name: its reason can carry arbitrary text.
            let lines = ["exception \(exception.name.rawValue)"] + exception.callStackSymbols
            let text = lines.joined(separator: "\n") + "\n"
            if let data = text.data(using: .utf8) {
                write(crashFileDescriptor, (data as NSData).bytes, data.count)
            }
        }
        for signalNumber in [SIGSEGV, SIGABRT, SIGILL, SIGBUS, SIGFPE, SIGTRAP] {
            signal(signalNumber, handleFatalSignal)
        }
    }

    /// The crash the last process died of, read once; the file is then removed.
    static func takePending() -> CrashRecord? {
        let text = Preferences.readText(at: pendingURL)
        try? FileManager.default.removeItem(at: pendingURL)
        return CrashRecord.parse(text)
    }

    /// Logs a crash from the last run and sends the report.
    static func reportPending() {
        guard let record = takePending() else { return }
        AppLog.write(record.logLine)
        Reporter.report(record.event)
    }

    private static func writeHeader() {
        let header = "version \(Reporter.appVersion)\n"
        header.withCString { _ = write(crashFileDescriptor, $0, strlen($0)) }
    }
}
