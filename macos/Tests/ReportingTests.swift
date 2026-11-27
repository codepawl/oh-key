import Foundation

// Tests of the pure reporting and update pieces. Called from main.swift.

private var reportingChecks = 0
private var reportingFailures = 0

private func expect<T: Equatable>(_ name: String, _ actual: T, _ expected: T) {
    reportingChecks += 1
    if actual == expected {
        print("pass  \(name)")
    } else {
        reportingFailures += 1
        print("FAIL  \(name): expected \(expected), got \(actual)")
    }
}

func runReportingTests() -> (checks: Int, failures: Int) {
    testVersionCompare()
    testReleaseTag()
    testScrubber()
    testBacktraceNames()
    testCrashRecord()
    testRateLimiter()
    testConfigFlags()
    testPayloadHasNoPersonalData()
    testPayloadMatchesWorkerFixture()
    testLogCap()
    return (reportingChecks, reportingFailures)
}

private func testVersionCompare() {
    expect("0.2.1 is newer than 0.2.0", UpdateCheck.isNewer("0.2.1", than: "0.2.0"), true)
    expect("v0.10.0 is newer than 0.9.9", UpdateCheck.isNewer("v0.10.0", than: "0.9.9"), true)
    expect("same version is not newer", UpdateCheck.isNewer("1.0.0", than: "1.0.0"), false)
    expect("older is not newer", UpdateCheck.isNewer("0.1.9", than: "0.2.0"), false)
    expect("a suffix after a dash is ignored past three numbers", UpdateCheck.isNewer("1.2.3-4", than: "1.2.3"), false)
}

private func testReleaseTag() {
    let tag = UpdateCheck.tag(fromReleaseURL: "https://github.com/anxuanng/oh-key/releases/tag/v0.3.0")
    expect("tag from the redirect", tag, "v0.3.0")
    expect("no tag in a plain page", UpdateCheck.tag(fromReleaseURL: "https://github.com/anxuanng/oh-key/releases"), nil)
}

private func testScrubber() {
    let home = "/Users/someone"
    let line = "7 could not read /Users/someone/Documents/a.txt and /users/Someone/b at 0x00000001049abcde"
    let scrubbed = ReportScrubber.scrub(line, home: home)
    expect("home folder and address removed", scrubbed, "7 could not read ~/Documents/a.txt and ~/b at 0x?")
    let long = String(repeating: "x", count: 500)
    expect("lines are cut to 300 characters", ReportScrubber.scrub(long, home: home).count, 300)
    expect("a name inside another path is replaced", ReportScrubber.scrub("/tmp/someone-cache", home: home), "/tmp/user-cache")
    var log = "1 started\n"
    for number in 0..<12 { log += "\(number) update check failed: \(number)\n" }
    let lines = ReportScrubber.problemLines(in: log, home: home)
    expect("only the newest eight problem lines", lines.count, 8)
    expect("the newest problem line is last", lines.last, "11 update check failed: 11")
    expect("an unknown line is withheld", ReportScrubber.shapedLogLine("5 something failed with xin chao", home: home), "5 (line withheld)")
    expect("a line with no time is withheld", ReportScrubber.shapedLogLine("failed", home: home), "(line withheld)")
    expect("non-ASCII text becomes ?", ReportScrubber.shapedLogLine("5 update check failed: Kh\u{F4}ng", home: home), "5 update check failed: Kh?ng")
    expect("a path keeps its last part", ReportScrubber.outgoing("open /Users/other/work/a.txt now", home: home), "open a.txt now")
}

private func testBacktraceNames() {
    let lines = [
        "0   libsystem_kernel.dylib   0x000000019a1b2c3d __pthread_kill + 8",
        "1   oh-key   0x0000000104a1b2c4 $s6oh_key8UpdaterO5startyyFZ + 52",
        "2   oh-key   0x0000000104a1b2c9 0x0000000104a1b2c9 + 12",
        "3   AppKit   0x000000019b000000 -[NSApplication run] + 600",
    ]
    expect("only our own function names, no addresses", ReportScrubber.ownFunctionNames(fromBacktrace: lines), ["$s6oh_key8UpdaterO5startyyFZ"])
}

private func testCrashRecord() {
    let text = "version 0.2.0\nsignal SIGSEGV\n0   oh-key   0x0000000104a1b2c4 $s6oh_key8UpdaterO5startyyFZ + 52\n"
    let record = CrashRecord.parse(text)
    expect("crash log line", record?.logLine, "crash in 0.2.0: signal SIGSEGV")
    expect("crash kind", record?.event.kind, "crash")
    expect("crash location is the top own frame", record?.event.location, "$s6oh_key8UpdaterO5startyyFZ")
    expect("an exception is a panic", CrashRecord.parse("version 1\nexception NSRangeException\n")?.event.kind, "panic")
    expect("a header alone is no crash", CrashRecord.parse("version 0.2.0\n"), nil)
}

private func testRateLimiter() {
    var limiter = ReportRateLimiter(maximumPerDay: 3, lowPriorityPerDay: 2, repeatWindow: 1000)
    let start = Date(timeIntervalSince1970: 1_000_000)
    expect("first report passes", limiter.allow(kind: "crash", fingerprint: "a", at: start), true)
    expect("the same problem again is held back", limiter.allow(kind: "crash", fingerprint: "a", at: start.addingTimeInterval(10)), false)
    expect("another problem passes", limiter.allow(kind: "crash", fingerprint: "b", at: start.addingTimeInterval(20)), true)
    expect("a noisy kind stops at its lower cap", limiter.allow(kind: "error", fingerprint: "n", at: start.addingTimeInterval(25)), false)
    expect("a crash still has room", limiter.allow(kind: "crash", fingerprint: "c", at: start.addingTimeInterval(30)), true)
    expect("the daily cap holds", limiter.allow(kind: "crash", fingerprint: "d", at: start.addingTimeInterval(40)), false)
    expect("the cap lifts after a day", limiter.allow(kind: "crash", fingerprint: "d", at: start.addingTimeInterval(86_500)), true)
    expect("the same problem passes after its window", limiter.allow(kind: "crash", fingerprint: "a", at: start.addingTimeInterval(90_000)), true)

    var persisted = ReportRateLimiter(maximumPerDay: 5, lowPriorityPerDay: 3, repeatWindow: 1000)
    _ = persisted.allow(kind: "crash", fingerprint: "crash|x", at: start)
    var restarted = ReportRateLimiter(parsing: persisted.serialized, maximumPerDay: 5, lowPriorityPerDay: 3, repeatWindow: 1000)
    expect("a restart remembers what was sent", restarted.allow(kind: "crash", fingerprint: "crash|x", at: start.addingTimeInterval(5)), false)
    var refunded = persisted
    refunded.refund(fingerprint: "crash|x")
    expect("a failed send is refunded", refunded.allow(kind: "crash", fingerprint: "crash|x", at: start.addingTimeInterval(5)), true)
}

private func testConfigFlags() {
    expect("send_reports defaults to on", ReportingConfig.sendReports(in: ""), true)
    expect("send_reports = no", ReportingConfig.sendReports(in: "spell_check = true\nsend_reports = no\n"), false)
    expect("send_reports with a comment", ReportingConfig.sendReports(in: "send_reports = false # privacy"), false)
    expect("send_reports = on", ReportingConfig.sendReports(in: "send_reports = \"on\"\n"), true)
    expect("an unknown send_reports value fails closed", ReportingConfig.sendReports(in: "send_reports = disabled\n"), false)
    expect("none fails closed", ReportingConfig.sendReports(in: "send_reports = none\n"), false)
    let missing = URL(fileURLWithPath: "/nonexistent-oh-key-config/config.toml")
    expect("a missing config allows reports", ReportingConfig.reportsAllowed(configURL: missing), true)
    expect("updates defaults to automatic", ReportingConfig.updateMode(in: ""), .automatic)
    expect("updates = off", ReportingConfig.updateMode(in: "updates = \"off\"\n"), .off)
    expect("updates = notify", ReportingConfig.updateMode(in: "updates = notify"), .notify)
}

private func testPayloadHasNoPersonalData() {
    let event = ReportEvent(
        kind: "error", message: "failed in /Users/someone/x", location: "Updater.swift:10",
        backtrace: ["f"], counts: ["frames": 1])
    let data = ReportPayload.json(
        event: event, installIdentifier: "id", appVersion: "0.2.0", operatingSystem: "macOS 15", architecture: "arm64",
        logLines: ["3 failed at /Users/someone/Library"], home: "/Users/someone")
    let text = data.flatMap { String(data: $0, encoding: .utf8) } ?? ""
    expect("payload is not empty", text.isEmpty, false)
    expect("payload never names the user", text.lowercased().contains("someone"), false)
    expect("payload names the platform", text.contains("\"platform\":\"macos\""), true)
}

/// The Worker's own tests validate the same file, so the two sides cannot drift apart.
private func testPayloadMatchesWorkerFixture() {
    let fixtureURL = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .appendingPathComponent("server/report-worker/fixtures/macos-report.json")
    guard let fixtureData = try? Data(contentsOf: fixtureURL) else {
        expect("the Worker fixture is found", false, true)
        return
    }
    let event = ReportEvent(
        kind: "crash", message: "signal SIGSEGV", location: "$s6oh_key8UpdaterO5startyyFZ",
        backtrace: ["$s6oh_key8UpdaterO5startyyFZ"], counts: ["frames": 1])
    let payload = ReportPayload.json(
        event: event, installIdentifier: "11111111-2222-4333-8444-555555555555", appVersion: "0.1.0",
        operatingSystem: "macOS 14.5.0", architecture: "arm64",
        logLines: ["1700000000 crash in 0.1.0: signal SIGSEGV"], home: "/Users/someone")
    let sent = payload.flatMap { try? JSONSerialization.jsonObject(with: $0) } as? NSDictionary
    let expected = (try? JSONSerialization.jsonObject(with: fixtureData)) as? NSDictionary
    expect("the macOS payload is the Worker's fixture", sent == expected, true)
    let uppercase = ReportPayload.json(
        event: event, installIdentifier: "ABCDEF12-2222-4333-8444-555555555555", appVersion: "0.1.0",
        operatingSystem: "macOS 14.5.0", architecture: "arm64", logLines: [], home: "/Users/someone")
    let uppercaseText = uppercase.flatMap { String(data: $0, encoding: .utf8) } ?? ""
    expect("the install id is sent in lower case", uppercaseText.contains("abcdef12-2222"), true)
}

private func testLogCap() {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("oh-key-log-test-\(UUID().uuidString)/oh-key.log")
    defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
    AppLog.write("first\nsecond", to: url, at: Date(timeIntervalSince1970: 42))
    expect("log line format and single line", AppLog.readAll(from: url), "42 first second\n")
    let filler = String(repeating: "x", count: 1000)
    for _ in 0..<1100 { AppLog.write(filler, to: url) }
    let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)??.uint64Value ?? 0
    expect("log restarts past the cap", size < AppLog.maximumBytes + 2000, true)
}
