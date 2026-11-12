import Cocoa
import InputMethodKit

private let keyCodeBackspace: UInt16 = 51
private let keyCodeZ: UInt16 = 6
private let notFoundRange = NSRange(location: NSNotFound, length: NSNotFound)

/// One controller per text field. It feeds the keys to the Rust engine and writes the answer
/// into the field by replacing the changed tail of the word (insertText with a replacement
/// range). A field that cannot report its cursor gets marked text instead.
@objc(OhKeyController)
final class OhKeyController: IMKInputController {
    private let session: OpaquePointer
    /// The word as the field shows it; empty when nothing is being composed.
    private var shownWord = ""
    /// Where the cursor should be after our last edit, to notice a click elsewhere.
    private var expectedCursorLocation = NSNotFound
    private var usesMarkedText = false
    /// Keys in a row where the cursor was not where our last edit left it.
    private var staleCursorStreak = 0
    /// Past this many in a row the field reports its cursor late (Chromium, Electron), so we switch to marked text.
    private static let staleCursorLimit = 3
    /// Apps known to report the cursor late; they start in marked-text mode.
    private static let markedTextBundleIdentifiers: Set<String> = [
        "com.google.Chrome", "com.google.Chrome.canary", "com.microsoft.edgemac", "com.brave.Browser",
        "com.vivaldi.Vivaldi", "com.operasoftware.Opera", "company.thebrowser.Browser",
        "com.microsoft.VSCode", "com.todesktop.230313mzl4w4u92", "com.tinyspeck.slackmacgap",
        "com.hnc.Discord", "notion.id", "md.obsidian", "com.figma.Desktop", "com.spotify.client",
    ]
    private static var toggleChordArmed = false

    override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        session = oh_key_session_new()
        super.init(server: server, delegate: delegate, client: inputClient)
        reloadSettings()
    }

    deinit {
        oh_key_session_free(session)
    }

    // MARK: Input method callbacks

    override func recognizedEvents(_ sender: Any!) -> Int {
        let mask: NSEvent.EventTypeMask = [.keyDown, .flagsChanged]
        return Int(mask.rawValue)
    }

    override func activateServer(_ sender: Any!) {
        reloadSettings()
        finishWord(client: sender)
        usesMarkedText = startsInMarkedTextMode(sender)
        staleCursorStreak = 0
    }

    override func deactivateServer(_ sender: Any!) {
        finishWord(client: sender)
    }

    override func commitComposition(_ sender: Any!) {
        finishWord(client: sender)
    }

    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event = event else { return false }
        if event.type == .flagsChanged {
            handleModifierChange(event)
            return false
        }
        guard event.type == .keyDown else { return false }
        OhKeyController.toggleChordArmed = false
        if isToggleHotkey(event) {
            toggleVietnamese(client: sender)
            return true
        }
        guard Preferences.vietnameseEnabled, !isExcluded(client: sender) else {
            return false
        }
        return handleKey(event, client: sender)
    }

    // MARK: Keys

    private func handleKey(_ event: NSEvent, client sender: Any?) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        if !flags.isDisjoint(with: [.command, .control, .option]) {
            finishWord(client: sender)
            return false
        }
        guard let client = sender as? IMKTextInput else { return false }
        let character = printableCharacter(of: event)
        let selectionExists = hasSelection(client)
        if selectionExists || detectCursorMovedAway(client) {
            finishWord(client: client)
            // The typed letter replaces the selection, so the engine still has to see it as the word's first key.
            if selectionExists && character == nil { return false }
        }
        if event.keyCode == keyCodeBackspace {
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

    private func printableCharacter(of event: NSEvent) -> Unicode.Scalar? {
        guard let characters = event.characters, characters.unicodeScalars.count == 1 else { return nil }
        let scalar = characters.unicodeScalars.first!
        let isControl = scalar.value < 0x20 || scalar.value == 0x7F
        let isFunctionKey = (0xF700...0xF8FF).contains(scalar.value)
        return (isControl || isFunctionKey) ? nil : scalar
    }

    /// Runs one engine call and writes its answer into the field. Returns whether the key is consumed.
    private func apply(
        client: IMKTextInput,
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
        if usesMarkedText || !canReplace(client) {
            usesMarkedText = true
            return applyAsMarkedText(client: client, word: word, finished: finished, composing: result.composing != 0, consumed: consumed)
        }
        replaceTail(client: client, deleting: Int(result.backspaces), inserting: text)
        shownWord = word
        expectedCursorLocation = client.selectedRange().location + keyLength
        if result.composing == 0 { expectedCursorLocation = NSNotFound }
        return consumed
    }

    private func replaceTail(client: IMKTextInput, deleting characters: Int, inserting text: String) {
        if characters == 0 && text.isEmpty { return }
        let deletedUnits = shownWord.unicodeScalars.suffix(characters).reduce(0) { $0 + String($1).utf16.count }
        let cursor = client.selectedRange().location
        let range = (characters == 0) ? notFoundRange : NSRange(location: cursor - deletedUnits, length: deletedUnits)
        client.insertText(text, replacementRange: range)
    }

    // MARK: Marked text fallback

    private func applyAsMarkedText(client: IMKTextInput, word: String, finished: String, composing: Bool, consumed: Bool) -> Bool {
        if composing {
            let selection = NSRange(location: word.utf16.count, length: 0)
            client.setMarkedText(word, selectionRange: selection, replacementRange: notFoundRange)
            shownWord = word
            return true
        }
        let previousWord = shownWord
        shownWord = ""
        if !finished.isEmpty {
            client.insertText(finished, replacementRange: notFoundRange)
            return true
        }
        if previousWord.isEmpty { return consumed }
        if consumed {
            // The key (Backspace) erased the whole word: drop the marked text instead of leaving it behind.
            client.setMarkedText("", selectionRange: NSRange(location: 0, length: 0), replacementRange: notFoundRange)
            return true
        }
        // A pass-through key ends the word: commit it, then let the key reach the field.
        client.insertText(previousWord, replacementRange: notFoundRange)
        return false
    }

    private func startsInMarkedTextMode(_ sender: Any?) -> Bool {
        guard let client = sender as? IMKTextInput, let identifier = client.bundleIdentifier() else { return false }
        return OhKeyController.markedTextBundleIdentifiers.contains(identifier)
    }

    private func canReplace(_ client: IMKTextInput) -> Bool {
        return client.selectedRange().location != NSNotFound
    }

    private func hasSelection(_ client: IMKTextInput) -> Bool {
        return client.selectedRange().length > 0
    }

    private func detectCursorMovedAway(_ client: IMKTextInput) -> Bool {
        if shownWord.isEmpty || usesMarkedText || expectedCursorLocation == NSNotFound { return false }
        let moved = client.selectedRange().location != expectedCursorLocation
        staleCursorStreak = moved ? staleCursorStreak + 1 : 0
        if staleCursorStreak >= OhKeyController.staleCursorLimit {
            usesMarkedText = true
        }
        return moved
    }

    // MARK: Finishing and state

    private func finishWord(client sender: Any?) {
        defer { forgetWord() }
        guard usesMarkedText, !shownWord.isEmpty, let client = sender as? IMKTextInput else { return }
        client.insertText(shownWord, replacementRange: notFoundRange)
    }

    private func forgetWord() {
        oh_key_session_reset(session)
        shownWord = ""
        expectedCursorLocation = NSNotFound
    }

    private func reloadSettings() {
        Preferences.load(settings: Preferences.readText(at: Preferences.configURL), into: session)
        Preferences.load(macros: Preferences.readText(at: Preferences.macrosURL), into: session)
    }

    private func isExcluded(client sender: Any?) -> Bool {
        guard let client = sender as? IMKTextInput, let identifier = client.bundleIdentifier() else { return false }
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

    private func isToggleHotkey(_ event: NSEvent) -> Bool {
        let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        return oh_key_session_toggle_hotkey(session) == UInt32(OH_KEY_HOTKEY_ALT_Z)
            && event.keyCode == keyCodeZ && flags == .option
    }

    /// Ctrl+Shift pressed and released with no other key between.
    private func handleModifierChange(_ event: NSEvent) {
        guard oh_key_session_toggle_hotkey(session) == UInt32(OH_KEY_HOTKEY_CTRL_SHIFT) else { return }
        let held = event.modifierFlags.intersection([.control, .shift, .option, .command])
        if held == [.control, .shift] {
            OhKeyController.toggleChordArmed = true
        } else if held.isEmpty {
            let completed = OhKeyController.toggleChordArmed
            OhKeyController.toggleChordArmed = false
            if completed { toggleVietnamese(client: client()) }
        } else if !held.isSubset(of: [.control, .shift]) {
            OhKeyController.toggleChordArmed = false
        }
    }

    private func toggleVietnamese(client sender: Any?) {
        finishWord(client: sender)
        Preferences.vietnameseEnabled.toggle()
    }

    // MARK: Menu

    override func menu() -> NSMenu! {
        let menu = NSMenu()
        let mode = Preferences.vietnameseEnabled ? "Tiếng Việt (bấm để chuyển sang English)" : "English (bấm để chuyển sang Tiếng Việt)"
        menu.addItem(menuItem(mode, action: #selector(toggleFromMenu(_:)), checked: Preferences.vietnameseEnabled, tag: -1))
        menu.addItem(NSMenuItem.separator())
        let current = oh_key_session_input_method(session)
        let methods: [(String, UInt32)] = [
            ("Telex", UInt32(OH_KEY_INPUT_TELEX)), ("VNI", UInt32(OH_KEY_INPUT_VNI)),
            ("VIQR", UInt32(OH_KEY_INPUT_VIQR)), ("Telex đơn giản", UInt32(OH_KEY_INPUT_SIMPLE_TELEX)),
        ]
        for (title, code) in methods {
            menu.addItem(menuItem(title, action: #selector(selectInputMethod(_:)), checked: code == current, tag: Int(code)))
        }
        menu.addItem(NSMenuItem.separator())
        menu.addItem(menuItem("Mở thư mục cấu hình", action: #selector(openConfigFolder(_:)), checked: false, tag: -1))
        return menu
    }

    private func menuItem(_ title: String, action: Selector, checked: Bool, tag: Int) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
        item.target = self
        item.state = checked ? .on : .off
        item.tag = tag
        return item
    }

    @objc private func toggleFromMenu(_ sender: Any?) {
        toggleVietnamese(client: client())
    }

    @objc private func selectInputMethod(_ sender: Any?) {
        // The system hands over the item itself or a dictionary that holds it.
        let dictionary = sender as? [String: Any]
        guard let item = (sender as? NSMenuItem) ?? (dictionary?["IMKCommandMenuItem"] as? NSMenuItem) else { return }
        Preferences.saveInputMethod(UInt32(item.tag))
        reloadSettings()
        forgetWord()
    }

    @objc private func openConfigFolder(_ sender: Any?) {
        try? FileManager.default.createDirectory(at: Preferences.folderURL, withIntermediateDirectories: true)
        NSWorkspace.shared.open(Preferences.folderURL)
    }
}
