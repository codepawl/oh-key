import Foundation

/// What config.toml says about reporting and updating. Parsed here because the shared settings
/// type does not carry these two keys yet; the file format is the same as the Windows app's.
enum UpdateMode: Equatable {
    case automatic
    case notify
    case off
}

enum ReportingConfig {
    /// Fails closed: a value that is not understood (`disabled`, `none`, `opt-out`) counts as "no".
    static func sendReports(in configText: String) -> Bool {
        guard let value = value(of: "send_reports", in: configText) else { return true }
        switch value {
        case "true", "yes", "on", "1": return true
        default: return false
        }
    }

    /// Whether a report may be sent now. A missing config file is a first start and allows it; a
    /// file that exists but cannot be read does not.
    static func reportsAllowed(configURL: URL) -> Bool {
        guard FileManager.default.fileExists(atPath: configURL.path) else { return true }
        guard let text = try? String(contentsOf: configURL, encoding: .utf8) else { return false }
        return sendReports(in: text)
    }

    static func updateMode(in configText: String) -> UpdateMode {
        switch value(of: "updates", in: configText) {
        case "notify": return .notify
        case "off": return .off
        default: return .automatic
        }
    }

    private static func value(of key: String, in configText: String) -> String? {
        var found: String?
        for rawLine in configText.split(separator: "\n") {
            let line = rawLine.split(separator: "#", maxSplits: 1, omittingEmptySubsequences: false).first ?? ""
            let parts = line.split(separator: "=", maxSplits: 1)
            guard parts.count == 2 else { continue }
            guard parts[0].trimmingCharacters(in: .whitespaces) == key else { continue }
            found = parts[1].trimmingCharacters(in: CharacterSet(charactersIn: " \t\r\"")).lowercased()
        }
        return found
    }
}

/// At most five reports a day (three for the noisy kinds, which leaves room for a crash), and the
/// same problem once a day, so one bug cannot flood the endpoint. The state is kept in a file:
/// macOS restarts a dead input method, so a limiter in memory would start from zero on every
/// crash.
struct ReportRateLimiter {
    let maximumPerDay: Int
    let lowPriorityPerDay: Int
    let repeatWindow: TimeInterval
    private var sentTimes: [Date] = []
    private var lastSentByFingerprint: [String: Date] = [:]

    init(maximumPerDay: Int = 5, lowPriorityPerDay: Int = 3, repeatWindow: TimeInterval = 24 * 3600) {
        self.maximumPerDay = maximumPerDay
        self.lowPriorityPerDay = lowPriorityPerDay
        self.repeatWindow = repeatWindow
    }

    static func isHighPriority(kind: String) -> Bool {
        return ["crash", "panic", "update-failed"].contains(kind)
    }

    mutating func allow(kind: String, fingerprint: String, at now: Date) -> Bool {
        let key = ReportRateLimiter.key(for: fingerprint)
        sentTimes = sentTimes.filter { now.timeIntervalSince($0) < 24 * 3600 }
        let limit = ReportRateLimiter.isHighPriority(kind: kind) ? maximumPerDay : lowPriorityPerDay
        if sentTimes.count >= limit { return false }
        if let last = lastSentByFingerprint[key], now.timeIntervalSince(last) < repeatWindow { return false }
        sentTimes.append(now)
        lastSentByFingerprint[key] = now
        return true
    }

    /// A report that did not get through does not use up the day.
    mutating func refund(fingerprint: String) {
        _ = sentTimes.popLast()
        lastSentByFingerprint[ReportRateLimiter.key(for: fingerprint)] = nil
    }

    /// FNV-1a: the file holds a hash, not the fingerprint text.
    static func key(for fingerprint: String) -> String {
        var hash: UInt64 = 0xcbf2_9ce4_8422_2325
        for byte in fingerprint.utf8 {
            hash ^= UInt64(byte)
            hash = hash &* 0x0100_0000_01b3
        }
        return String(hash, radix: 16)
    }

    var serialized: String {
        var lines = sentTimes.map { "sent \(Int($0.timeIntervalSince1970))" }
        for (key, date) in lastSentByFingerprint {
            lines.append("seen \(key) \(Int(date.timeIntervalSince1970))")
        }
        return lines.joined(separator: "\n") + "\n"
    }

    init(parsing text: String, maximumPerDay: Int = 5, lowPriorityPerDay: Int = 3, repeatWindow: TimeInterval = 24 * 3600) {
        self.init(maximumPerDay: maximumPerDay, lowPriorityPerDay: lowPriorityPerDay, repeatWindow: repeatWindow)
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: " ").map(String.init)
            if parts.count == 2, parts[0] == "sent", let seconds = TimeInterval(parts[1]) {
                sentTimes.append(Date(timeIntervalSince1970: seconds))
            } else if parts.count == 3, parts[0] == "seen", let seconds = TimeInterval(parts[2]) {
                lastSentByFingerprint[parts[1]] = Date(timeIntervalSince1970: seconds)
            }
        }
    }
}

/// One event to report. Messages come from our own code only.
struct ReportEvent {
    let kind: String
    let message: String
    let location: String
    let backtrace: [String]
    let counts: [String: Int]

    var fingerprint: String { "\(kind)|\(location)|\(message.prefix(80))" }
}

/// The report format both platforms send and the Worker accepts: the field names of
/// `server/report-worker/src/schema.ts`. `server/report-worker/fixtures/macos-report.json` pins it
/// on both sides. The event's `location` is only used to tell problems apart on this machine.
enum ReportPayload {
    static func json(
        event: ReportEvent, installIdentifier: String, appVersion: String, operatingSystem: String,
        architecture: String, logLines: [String], home: String = NSHomeDirectory()
    ) -> Data? {
        var body: [String: Any] = [
            "app": "oh-key",
            "app_version": appVersion,
            "platform": "macos",
            "os_version": operatingSystem,
            "arch": architecture,
            "install_id": installIdentifier.lowercased(),
            "kind": event.kind,
            "message": ReportScrubber.outgoing(event.message, home: home),
        ]
        if !event.backtrace.isEmpty {
            body["backtrace"] = event.backtrace.map { ReportScrubber.outgoing($0, home: home) }
        }
        if !event.counts.isEmpty { body["counts"] = event.counts }
        if !logLines.isEmpty { body["log_lines"] = logLines.map { ReportScrubber.shapedLogLine($0, home: home) } }
        return try? JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
    }
}
