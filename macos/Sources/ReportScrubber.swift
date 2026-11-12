import Foundation

/// Takes anything personal out of text before it can leave the machine. The input method sees
/// every key, so reports carry only our own messages, function names and scrubbed log lines.
enum ReportScrubber {
    static let maximumLines = 8
    static let maximumLineCharacters = 300
    static let maximumBacktraceNames = 30
    static let ownModuleName = "oh-key"

    /// One line with the home folder, the user name and raw addresses removed, cut to length.
    static func scrub(_ text: String, home: String = NSHomeDirectory()) -> String {
        var cleaned = text.replacingOccurrences(of: "\n", with: " ").replacingOccurrences(of: "\r", with: " ")
        cleaned = removingAddresses(from: cleaned)
        if home.count > 3 {
            cleaned = cleaned.replacingOccurrences(of: home, with: "~", options: .caseInsensitive)
            let userName = (home as NSString).lastPathComponent
            // A very short name would hit ordinary words.
            if userName.count >= 4 {
                cleaned = cleaned.replacingOccurrences(of: userName, with: "user", options: .caseInsensitive)
            }
        }
        return String(cleaned.prefix(maximumLineCharacters))
    }

    /// The newest log lines that talk about a crash or a failure, in a known shape.
    static func problemLines(in log: String, home: String = NSHomeDirectory()) -> [String] {
        let problems = log.split(separator: "\n").map(String.init).filter(isProblem)
        return problems.suffix(maximumLines).map { shapedLogLine($0, home: home) }
    }

    /// The openings of log lines that may be sent. Any other line that looks like a problem is
    /// replaced by a marker, so a report's privacy does not depend on what a future
    /// `AppLog.write` call says. The same list as the Windows app.
    static let knownLogPrefixes = [
        "crash in ", "update check failed: ", "report failed: ", "update-failed: ", "error: ", "update to ",
    ]
    static let withheldLine = "(line withheld)"

    static func shapedLogLine(_ line: String, home: String = NSHomeDirectory()) -> String {
        let parts = line.split(separator: " ", maxSplits: 1, omittingEmptySubsequences: false)
        guard parts.count == 2, parts[0].count <= 11, !parts[0].isEmpty, parts[0].allSatisfy({ $0.isASCII && $0.isNumber }) else {
            return withheldLine
        }
        let message = String(parts[1])
        guard knownLogPrefixes.contains(where: { message.hasPrefix($0) }) else {
            return "\(parts[0]) \(withheldLine)"
        }
        return "\(parts[0]) " + outgoing(message, home: home, maximumCharacters: maximumLogMessageCharacters)
    }

    static let maximumLogMessageCharacters = 280

    /// Text that leaves the machine: scrubbed, drive and network paths cut to their last part, and
    /// printable ASCII only (anything else becomes `?`, so typed Vietnamese cannot ride along).
    static func outgoing(_ text: String, home: String = NSHomeDirectory(), maximumCharacters: Int = ReportScrubber.maximumLineCharacters) -> String {
        let scrubbed = scrub(text, home: home)
        let withoutPaths = scrubbed.split(separator: " ", omittingEmptySubsequences: false).map { word -> String in
            let isPath = word.hasPrefix("/Users/") || word.hasPrefix("/Volumes/") || word.hasPrefix("/private/")
            return isPath ? String(word.split(separator: "/").last ?? "") : String(word)
        }.joined(separator: " ")
        let ascii = String(withoutPaths.unicodeScalars.map { scalar -> Character in
            return (scalar.value >= 0x20 && scalar.value < 0x7f) ? Character(scalar) : "?"
        })
        return String(ascii.prefix(maximumCharacters))
    }

    static func isProblem(_ line: String) -> Bool {
        return ["crash in", "failed", "error"].contains { line.contains($0) }
    }

    /// Function names of our own module from `backtrace_symbols` style lines
    /// ("3   oh-key   0x0000000104a1b2c4 $s6oh_key... + 52"). Addresses and offsets are dropped.
    static func ownFunctionNames(fromBacktrace lines: [String]) -> [String] {
        var names: [String] = []
        for line in lines {
            let fields = line.split(whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init)
            guard fields.count >= 4, fields[1] == ownModuleName else { continue }
            let symbol = fields[3]
            if symbol.hasPrefix("0x") { continue }
            guard symbol.range(of: "^\\$s6oh_key[A-Za-z0-9_]{0,180}$", options: .regularExpression) != nil else { continue }
            names.append(symbol)
        }
        return Array(names.prefix(maximumBacktraceNames))
    }

    private static func removingAddresses(from text: String) -> String {
        return text.replacingOccurrences(of: "0x[0-9a-fA-F]{6,}", with: "0x?", options: .regularExpression)
    }
}
