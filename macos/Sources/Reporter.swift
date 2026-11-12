import Foundation

/// The only place the report endpoint is written down. The placeholder is replaced after the
/// Cloudflare Worker is deployed.
let REPORT_ENDPOINT = "https://oh-key-report.codepawl.com/v1/report"

/// Sends error and crash reports. Everything in a report is built by ReportPayload from our own
/// messages; nothing here reads typed text, the clipboard or window titles.
enum Reporter {
    private static let queue = DispatchQueue(label: "org.codepawl.oh-key.reporter")

    static var appVersion: String {
        return Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "unknown"
    }

    static func report(_ event: ReportEvent) {
        queue.async {
            guard ReportingConfig.reportsAllowed(configURL: Preferences.configURL) else { return }
            guard !REPORT_ENDPOINT.contains("REPLACE_ME") else { return }
            var limiter = ReportRateLimiter(parsing: Preferences.readText(at: limiterURL))
            guard limiter.allow(kind: event.kind, fingerprint: event.fingerprint, at: Date()) else {
                AppLog.write("report skipped: rate limit")
                return
            }
            saveLimiter(limiter)
            if !send(event) {
                limiter.refund(fingerprint: event.fingerprint)
                saveLimiter(limiter)
            }
        }
    }

    private static var limiterURL: URL { Preferences.folderURL.appendingPathComponent("report-state.txt") }

    private static func saveLimiter(_ limiter: ReportRateLimiter) {
        try? FileManager.default.createDirectory(at: Preferences.folderURL, withIntermediateDirectories: true)
        try? limiter.serialized.write(to: limiterURL, atomically: true, encoding: .utf8)
    }

    /// True when the server took the report.
    private static func send(_ event: ReportEvent) -> Bool {
        let logLines = ReportScrubber.problemLines(in: AppLog.readAll())
        guard let identifier = installIdentifier() else {
            AppLog.write("report not sent: no install id could be made")
            return false
        }
        let body = ReportPayload.json(
            event: event, installIdentifier: identifier, appVersion: appVersion,
            operatingSystem: operatingSystem, architecture: architecture, logLines: logLines)
        guard let body = body, let url = URL(string: REPORT_ENDPOINT) else { return false }
        var request = URLRequest(url: url, timeoutInterval: 20)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = body
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        let semaphore = DispatchSemaphore(value: 0)
        var delivered = false
        URLSession(configuration: configuration).dataTask(with: request) { _, response, error in
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            if error != nil || !(200..<300).contains(status) {
                AppLog.write("report failed: the server answered \(status)")
            } else {
                delivered = true
            }
            semaphore.signal()
        }.resume()
        _ = semaphore.wait(timeout: .now() + 25)
        return delivered
    }

    /// `macOS 14.5.0`: the shape the Worker checks, not the long system string.
    static var operatingSystem: String {
        let version = ProcessInfo.processInfo.operatingSystemVersion
        return "macOS \(version.majorVersion).\(version.minorVersion).\(version.patchVersion)"
    }

    private static var architecture: String {
        #if arch(arm64)
        return "arm64"
        #else
        return "x86_64"
        #endif
    }

    /// A random id kept in the data folder. Deleting install-id.txt makes a new one.
    private static func installIdentifier() -> String? {
        let url = Preferences.folderURL.appendingPathComponent("install-id.txt")
        let existing = Preferences.readText(at: url).trimmingCharacters(in: .whitespacesAndNewlines)
        if UUID(uuidString: existing) != nil { return existing.lowercased() }
        let created = UUID().uuidString.lowercased()
        try? FileManager.default.createDirectory(at: Preferences.folderURL, withIntermediateDirectories: true)
        do {
            try created.write(to: url, atomically: true, encoding: .utf8)
        } catch {
            return nil
        }
        return created
    }
}
