import Cocoa

/// How a mock field reports its cursor.
enum CursorReporting {
    /// The true selection, like TextEdit.
    case accurate
    /// NSNotFound, like a field that does not support the call.
    case unavailable
    /// NSNotFound in both location and length.
    case unavailableEverywhere
    /// The selection from before the last change, like Chromium before it catches up.
    case lagging
}

/// A text field in memory: a UTF-16 buffer, a selection, an optional marked range, and a log of calls.
final class MockTextClient: OhKeyTextClient {
    private(set) var units: [UInt16] = []
    var selection = NSRange(location: 0, length: 0)
    private var markedRange: NSRange?
    private var previousSelection = NSRange(location: 0, length: 0)
    let cursorReporting: CursorReporting
    let bundle: String?
    /// Many hosts (Terminal, some Electron and Java apps) ignore the replacement range and insert
    /// at the cursor, which is what duplicates letters.
    let ignoresReplacementRange: Bool
    private(set) var calls: [String] = []
    /// Calls with a range outside the buffer: a real client throws or deletes the wrong text.
    private(set) var rangeViolations: [String] = []

    init(cursorReporting: CursorReporting = .accurate, bundle: String? = "com.apple.TextEdit",
         ignoresReplacementRange: Bool = false) {
        self.cursorReporting = cursorReporting
        self.bundle = bundle
        self.ignoresReplacementRange = ignoresReplacementRange
    }

    var text: String {
        return String(decoding: units, as: UTF16.self)
    }

    var markedText: String {
        guard let range = markedRange else { return "" }
        return String(decoding: units[range.location..<(range.location + range.length)], as: UTF16.self)
    }

    // MARK: OhKeyTextClient

    func selectedRange() -> NSRange {
        switch cursorReporting {
        case .accurate: return selection
        case .unavailable: return NSRange(location: NSNotFound, length: 0)
        case .unavailableEverywhere: return NSRange(location: NSNotFound, length: NSNotFound)
        case .lagging: return previousSelection
        }
    }

    func insertText(_ string: String, replacementRange: NSRange) {
        calls.append("insert(\(string), \(describe(replacementRange)))")
        let target = targetRange(for: replacementRange)
        replace(target, with: string)
        markedRange = nil
    }

    func setMarkedText(_ string: String, selectionRange: NSRange, replacementRange: NSRange) {
        calls.append("mark(\(string), \(describe(replacementRange)))")
        let target = targetRange(for: replacementRange)
        let start = target.location
        replace(target, with: string)
        markedRange = NSRange(location: start, length: string.utf16.count)
        selection = NSRange(location: start + selectionRange.location, length: selectionRange.length)
    }

    func bundleIdentifier() -> String? {
        return bundle
    }

    // MARK: Host behaviour for keys the input method does not consume

    func hostInsert(_ string: String) {
        replace(markedRange ?? selection, with: string)
        markedRange = nil
    }

    func hostBackspace() {
        if selection.length > 0 {
            replace(selection, with: "")
        } else if selection.location > 0 {
            replace(NSRange(location: selection.location - 1, length: 1), with: "")
        }
        markedRange = nil
    }

    func hostMoveCursor(by offset: Int) {
        previousSelection = selection
        selection = NSRange(location: max(0, min(units.count, selection.location + offset)), length: 0)
        markedRange = nil
    }

    func hostSelect(_ range: NSRange) {
        previousSelection = selection
        selection = range
        markedRange = nil
    }

    func hostSetText(_ string: String, cursor: Int) {
        units = Array(string.utf16)
        previousSelection = selection
        selection = NSRange(location: cursor, length: 0)
        markedRange = nil
    }

    // MARK: Internals

    private func targetRange(for replacementRange: NSRange) -> NSRange {
        if replacementRange.location == NSNotFound || ignoresReplacementRange {
            return markedRange ?? selection
        }
        let inside = replacementRange.location >= 0
            && replacementRange.length >= 0
            && replacementRange.location + replacementRange.length <= units.count
        if !inside {
            rangeViolations.append(describe(replacementRange))
            let start = max(0, min(units.count, replacementRange.location))
            return NSRange(location: start, length: 0)
        }
        return replacementRange
    }

    private func replace(_ range: NSRange, with string: String) {
        previousSelection = selection
        let replacement = Array(string.utf16)
        let end = min(units.count, range.location + range.length)
        units.replaceSubrange(range.location..<end, with: replacement)
        selection = NSRange(location: range.location + replacement.count, length: 0)
    }

    private func describe(_ range: NSRange) -> String {
        return range.location == NSNotFound ? "any" : "\(range.location)+\(range.length)"
    }
}
