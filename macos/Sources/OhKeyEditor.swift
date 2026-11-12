import Cocoa

/// What the editor needs from a text field. InputMethodKit's client adapts to it in the app, and a
/// mock implements it in the tests, so the editing logic runs without InputMethodKit.
protocol OhKeyTextClient: AnyObject {
    func selectedRange() -> NSRange
    func insertText(_ string: String, replacementRange: NSRange)
    func setMarkedText(_ string: String, selectionRange: NSRange, replacementRange: NSRange)
    func bundleIdentifier() -> String?
}

/// A key press reduced to what the editor reads from an NSEvent.
struct OhKeyInput {
    let keyCode: UInt16
    let characters: String?
    let modifiers: NSEvent.ModifierFlags
}

let ohKeyNotFoundRange = NSRange(location: NSNotFound, length: NSNotFound)

private let keyCodeBackspace: UInt16 = 51
private let keyCodeZ: UInt16 = 6

/// The editing logic of one text field. It feeds the keys to the Rust engine and writes the answer
/// into the field by replacing the changed tail of the word (insertText with a replacement
/// range). A field that cannot report its cursor gets marked text instead.
final class OhKeyEditor {
    private let session: OpaquePointer
    /// The word as the field shows it; empty when nothing is being composed.
    private(set) var shownWord = ""
    /// Where the cursor should be after our last edit, to notice a click elsewhere.
    private var expectedCursorLocation = NSNotFound
    private(set) var usesMarkedText = false
    /// Keys since activation where the cursor was not where our last edit left it.
    private var staleCursorCount = 0
    /// Past this many the field reports its cursor late (Chromium, Electron), so we switch to marked text.
    private static let staleCursorLimit = 2
    /// Apps known to report the cursor late; they start in marked-text mode.
    private static let markedTextBundleIdentifiers: Set<String> = [
        "com.google.Chrome", "com.google.Chrome.canary", "com.microsoft.edgemac", "com.brave.Browser",
        "com.vivaldi.Vivaldi", "com.operasoftware.Opera", "company.thebrowser.Browser",
        "com.microsoft.VSCode", "com.todesktop.230313mzl4w4u92", "com.tinyspeck.slackmacgap",
        "com.hnc.Discord", "notion.id", "md.obsidian", "com.figma.Desktop", "com.spotify.client",
    ]
    private static var toggleChordArmed = false

    /// The tests turn this off so the user's own config.toml never changes their results.
    private let loadsSettingsFromDisk: Bool

    init(loadsSettingsFromDisk: Bool = true) {
        session = oh_key_session_new()
        self.loadsSettingsFromDisk = loadsSettingsFromDisk
        if loadsSettingsFromDisk {
            reloadSettings()
        }
    }

    deinit {
        oh_key_session_free(session)
    }

    // MARK: Lifecycle

    func activate(client: OhKeyTextClient?) {
        if loadsSettingsFromDisk {
            reloadSettings()
        }
        finishWord(client: client)
        usesMarkedText = startsInMarkedTextMode(client)
        staleCursorCount = 0
    }

    func deactivate(client: OhKeyTextClient?) {
        finishWord(client: client)
    }

    func reloadSettings() {
        Preferences.load(settings: Preferences.readText(at: Preferences.configURL), into: session)
        Preferences.load(macros: Preferences.readText(at: Preferences.macrosURL), into: session)
    }

    /// Applies config.toml text directly; the tests use it instead of the user's file.
    func load(settingsText: String) {
        Preferences.load(settings: settingsText, into: session)
    }

    var inputMethod: UInt32 {
        return oh_key_session_input_method(session)
    }

    // MARK: Keys

    /// Returns whether the key is consumed; false lets it reach the field.
    func handleKeyDown(_ key: OhKeyInput, client: OhKeyTextClient?) -> Bool {
        OhKeyEditor.toggleChordArmed = false
        if isToggleHotkey(key) {
            toggleVietnamese(client: client)
            return true
        }
        guard Preferences.vietnameseEnabled, !isExcluded(client: client) else {
            return false
        }
        return handleKey(key, client: client)
    }

    private func handleKey(_ key: OhKeyInput, client: OhKeyTextClient?) -> Bool {
        let flags = key.modifiers.intersection(.deviceIndependentFlagsMask)
        if !flags.isDisjoint(with: [.command, .control, .option]) {
            finishWord(client: client)
            return false
        }
        guard let client = client else { return false }
        let character = printableCharacter(of: key)
        let selectionExists = hasSelection(client)
        if selectionExists || detectCursorMovedAway(client) {
            finishWord(client: client)
            // The typed letter replaces the selection, so the engine still has to see it as the word's first key.
            if selectionExists && character == nil { return false }
        }
        if key.keyCode == keyCodeBackspace {
            return apply(client: client, key: nil) { oh_key_session_backspace(self.session, $0) }
        }
        if let character = character {
            return apply(client: client, key: character) {
                oh_key_session_process_key(self.session, character.value, $0)
            }
        }
        _ = apply(client: client, key: nil) { oh_key_session_finish_word(self.session, $0) }
        return false
    }

    private func printableCharacter(of key: OhKeyInput) -> Unicode.Scalar? {
        guard let characters = key.characters, characters.unicodeScalars.count == 1 else { return nil }
        let scalar = characters.unicodeScalars.first!
        let isControl = scalar.value < 0x20 || scalar.value == 0x7F
        let isFunctionKey = (0xF700...0xF8FF).contains(scalar.value)
        return (isControl || isFunctionKey) ? nil : scalar
    }

    /// Runs one engine call and writes its answer into the field. Returns whether the key is consumed.
    private func apply(
        client: OhKeyTextClient,
        key: Unicode.Scalar?,
        call: (UnsafeMutablePointer<OhKeyResult>) -> Void
    ) -> Bool {
        var result = OhKeyResult()
        call(&result)
        let text = string(result.text, result.text_length)
        let word = string(result.preedit, result.preedit_length)
        let finished = string(result.finished_text, result.finished_text_length)
        let consumed = result.consume != 0
        let keyLength = consumed ? 0 : (key.map { String($0).utf16.count } ?? 0)
        if usesMarkedText || !canReplace(client) || !tailIsInsideTheField(client, deleting: Int(result.backspaces)) {
            usesMarkedText = true
            return applyAsMarkedText(client: client, word: word, finished: finished, composing: result.composing != 0, consumed: consumed)
        }
        let cursorAfterEdit = replaceTail(client: client, deleting: Int(result.backspaces), inserting: text)
        if consumed, let cursorAfterEdit = cursorAfterEdit, client.selectedRange().location != cursorAfterEdit {
            // The field did not move its cursor where our edit puts it: it reports the cursor late.
            noteUnreliableCursor()
            if usesMarkedText {
                // The word so far is real text in the field, so it stays; marked text starts with the next word.
                forgetWord()
                return consumed
            }
        }
        shownWord = word
        expectedCursorLocation = client.selectedRange().location + keyLength
        if result.composing == 0 { expectedCursorLocation = NSNotFound }
        return consumed
    }

    /// Returns where the cursor should be after the edit, or nil when nothing was edited.
    private func replaceTail(client: OhKeyTextClient, deleting characters: Int, inserting text: String) -> Int? {
        if characters == 0 && text.isEmpty { return nil }
        let deletedUnits = shownWord.unicodeScalars.suffix(characters).reduce(0) { $0 + String($1).utf16.count }
        let cursor = client.selectedRange().location
        let range = (characters == 0) ? ohKeyNotFoundRange : NSRange(location: cursor - deletedUnits, length: deletedUnits)
        client.insertText(text, replacementRange: range)
        return cursor - deletedUnits + text.utf16.count
    }

    private func noteUnreliableCursor() {
        staleCursorCount += 1
        if staleCursorCount >= OhKeyEditor.staleCursorLimit {
            usesMarkedText = true
        }
    }

    // MARK: Marked text fallback

    private func applyAsMarkedText(client: OhKeyTextClient, word: String, finished: String, composing: Bool, consumed: Bool) -> Bool {
        if composing {
            let selection = NSRange(location: word.utf16.count, length: 0)
            client.setMarkedText(word, selectionRange: selection, replacementRange: ohKeyNotFoundRange)
            shownWord = word
            return true
        }
        let previousWord = shownWord
        shownWord = ""
        if !finished.isEmpty {
            client.insertText(finished, replacementRange: ohKeyNotFoundRange)
            return true
        }
        if previousWord.isEmpty { return consumed }
        if consumed {
            // The key (Backspace) erased the whole word: drop the marked text instead of leaving it behind.
            client.setMarkedText("", selectionRange: NSRange(location: 0, length: 0), replacementRange: ohKeyNotFoundRange)
            return true
        }
        // A pass-through key ends the word: commit it, then let the key reach the field.
        client.insertText(previousWord, replacementRange: ohKeyNotFoundRange)
        return false
    }

    private func startsInMarkedTextMode(_ client: OhKeyTextClient?) -> Bool {
        guard let identifier = client?.bundleIdentifier() else { return false }
        return OhKeyEditor.markedTextBundleIdentifiers.contains(identifier)
    }

    /// A stale or small cursor would give the replacement a negative or wrong location, which
    /// throws in the client or deletes the user's real text: such a key falls back to marked text.
    private func tailIsInsideTheField(_ client: OhKeyTextClient, deleting characters: Int) -> Bool {
        if characters == 0 { return true }
        let deletedUnits = shownWord.unicodeScalars.suffix(characters).reduce(0) { $0 + String($1).utf16.count }
        return client.selectedRange().location >= deletedUnits
    }

    private func canReplace(_ client: OhKeyTextClient) -> Bool {
        return client.selectedRange().location != NSNotFound
    }

    /// A field that cannot report its cursor may answer with NSNotFound in the length too; that is not a selection.
    private func hasSelection(_ client: OhKeyTextClient) -> Bool {
        let range = client.selectedRange()
        return range.location != NSNotFound && range.length != NSNotFound && range.length > 0
    }

    private func detectCursorMovedAway(_ client: OhKeyTextClient) -> Bool {
        if shownWord.isEmpty || usesMarkedText || expectedCursorLocation == NSNotFound { return false }
        let moved = client.selectedRange().location != expectedCursorLocation
        if moved { noteUnreliableCursor() }
        return moved
    }

    // MARK: Finishing and state

    func finishWord(client: OhKeyTextClient?) {
        defer { forgetWord() }
        guard usesMarkedText, !shownWord.isEmpty, let client = client else { return }
        client.insertText(shownWord, replacementRange: ohKeyNotFoundRange)
    }

    func forgetWord() {
        oh_key_session_reset(session)
        shownWord = ""
        expectedCursorLocation = NSNotFound
    }

    private func isExcluded(client: OhKeyTextClient?) -> Bool {
        guard let identifier = client?.bundleIdentifier() else { return false }
        let bytes = Array(identifier.utf8)
        return bytes.withUnsafeBufferPointer { buffer in
            buffer.baseAddress?.withMemoryRebound(to: CChar.self, capacity: bytes.count) { pointer in
                oh_key_session_is_app_excluded(session, pointer, bytes.count) != 0
            } ?? false
        }
    }

    private func string(_ pointer: UnsafePointer<UInt8>?, _ length: Int) -> String {
        guard let pointer = pointer, length > 0 else { return "" }
        return String(decoding: UnsafeBufferPointer(start: pointer, count: length), as: UTF8.self)
    }

    // MARK: Vietnamese and English

    private func isToggleHotkey(_ key: OhKeyInput) -> Bool {
        let flags = key.modifiers.intersection(.deviceIndependentFlagsMask)
        return oh_key_session_toggle_hotkey(session) == UInt32(OH_KEY_HOTKEY_ALT_Z)
            && key.keyCode == keyCodeZ && flags == .option
    }

    /// Ctrl+Shift pressed and released with no other key between.
    func handleModifierChange(_ modifiers: NSEvent.ModifierFlags, client: OhKeyTextClient?) {
        guard oh_key_session_toggle_hotkey(session) == UInt32(OH_KEY_HOTKEY_CTRL_SHIFT) else { return }
        let held = modifiers.intersection([.control, .shift, .option, .command])
        if held == [.control, .shift] {
            OhKeyEditor.toggleChordArmed = true
        } else if held.isEmpty {
            let completed = OhKeyEditor.toggleChordArmed
            OhKeyEditor.toggleChordArmed = false
            if completed { toggleVietnamese(client: client) }
        } else if !held.isSubset(of: [.control, .shift]) {
            OhKeyEditor.toggleChordArmed = false
        }
    }

    func toggleVietnamese(client: OhKeyTextClient?) {
        finishWord(client: client)
        Preferences.vietnameseEnabled.toggle()
    }

    func selectInputMethod(_ code: UInt32) {
        Preferences.saveInputMethod(code)
        reloadSettings()
        forgetWord()
    }
}
