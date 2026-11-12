import Foundation

/// Pure parts of the update check, shared with the tests.
enum UpdateCheck {
    static let releasesURL = "https://github.com/codepawl/oh-key-releases/releases/latest"
    static let releasePageBase = "https://github.com/codepawl/oh-key-releases/releases/tag/"

    /// "v0.5.0" from ".../releases/tag/v0.5.0".
    static func tag(fromReleaseURL url: String) -> String? {
        let parts = url.components(separatedBy: "/releases/tag/")
        guard parts.count == 2 else { return nil }
        let tag = parts[1].trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        if tag.isEmpty || tag.contains("/") { return nil }
        return tag
    }

    /// Splits on "." and "-", compares the first three numbers as integers.
    static func isNewer(_ candidate: String, than current: String) -> Bool {
        let left = numbers(in: candidate)
        let right = numbers(in: current)
        for index in 0..<3 where left[index] != right[index] {
            return left[index] > right[index]
        }
        return false
    }

    private static func numbers(in version: String) -> [Int] {
        let trimmed = version.hasPrefix("v") ? String(version.dropFirst()) : version
        var parsed = trimmed.split(whereSeparator: { $0 == "." || $0 == "-" }).prefix(3).map { Int($0) ?? 0 }
        while parsed.count < 3 { parsed.append(0) }
        return parsed
    }
}
