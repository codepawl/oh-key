import Cocoa
import InputMethodKit

/// Lets the editor talk to InputMethodKit's text client.
private final class InputClientAdapter: OhKeyTextClient {
    private let client: IMKTextInput

    init(_ client: IMKTextInput) {
        self.client = client
    }

    func selectedRange() -> NSRange {
        return client.selectedRange()
    }

    func insertText(_ string: String, replacementRange: NSRange) {
        client.insertText(string, replacementRange: replacementRange)
    }

    func setMarkedText(_ string: String, selectionRange: NSRange, replacementRange: NSRange) {
        client.setMarkedText(string, selectionRange: selectionRange, replacementRange: replacementRange)
    }

    func bundleIdentifier() -> String? {
        return client.bundleIdentifier()
    }
}

/// One controller per text field. The editing logic lives in OhKeyEditor; this class only
/// connects it to InputMethodKit's events and the menu.
@objc(OhKeyController)
final class OhKeyController: IMKInputController {
    private let editor = OhKeyEditor()

    override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        super.init(server: server, delegate: delegate, client: inputClient)
    }

    private func adapt(_ sender: Any?) -> OhKeyTextClient? {
        guard let client = sender as? IMKTextInput else { return nil }
        return InputClientAdapter(client)
    }

    // MARK: Input method callbacks

    override func recognizedEvents(_ sender: Any!) -> Int {
        let mask: NSEvent.EventTypeMask = [.keyDown, .flagsChanged]
        return Int(mask.rawValue)
    }

    override func activateServer(_ sender: Any!) {
        editor.activate(client: adapt(sender))
    }

    override func deactivateServer(_ sender: Any!) {
        editor.deactivate(client: adapt(sender))
    }

    override func commitComposition(_ sender: Any!) {
        editor.finishWord(client: adapt(sender))
    }

    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event = event else { return false }
        if event.type == .flagsChanged {
            editor.handleModifierChange(event.modifierFlags, client: adapt(sender))
            return false
        }
        guard event.type == .keyDown else { return false }
        let key = OhKeyInput(keyCode: event.keyCode, characters: event.characters, modifiers: event.modifierFlags)
        return editor.handleKeyDown(key, client: adapt(sender))
    }

    // MARK: Menu

    override func menu() -> NSMenu! {
        let menu = NSMenu()
        let mode = Preferences.vietnameseEnabled ? "Tiếng Việt (bấm để chuyển sang English)" : "English (bấm để chuyển sang Tiếng Việt)"
        menu.addItem(menuItem(mode, action: #selector(toggleFromMenu(_:)), checked: Preferences.vietnameseEnabled, tag: -1))
        menu.addItem(NSMenuItem.separator())
        let current = editor.inputMethod
        let methods: [(String, UInt32)] = [
            ("Telex", UInt32(OH_KEY_INPUT_TELEX)), ("VNI", UInt32(OH_KEY_INPUT_VNI)),
            ("VIQR", UInt32(OH_KEY_INPUT_VIQR)), ("Telex đơn giản", UInt32(OH_KEY_INPUT_SIMPLE_TELEX)),
        ]
        for (title, code) in methods {
            menu.addItem(menuItem(title, action: #selector(selectInputMethod(_:)), checked: code == current, tag: Int(code)))
        }
        menu.addItem(NSMenuItem.separator())
        menu.addItem(menuItem("Mở thư mục cấu hình", action: #selector(openConfigFolder(_:)), checked: false, tag: -1))
        menu.addItem(menuItem(Updater.menuTitle, action: #selector(checkForUpdates(_:)), checked: false, tag: -1))
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
        editor.toggleVietnamese(client: adapt(client()))
    }

    @objc private func selectInputMethod(_ sender: Any?) {
        // The system hands over the item itself or a dictionary that holds it.
        let dictionary = sender as? [String: Any]
        guard let item = (sender as? NSMenuItem) ?? (dictionary?["IMKCommandMenuItem"] as? NSMenuItem) else { return }
        editor.selectInputMethod(UInt32(item.tag))
    }

    @objc private func checkForUpdates(_ sender: Any?) {
        Updater.menuItemChosen()
    }

    @objc private func openConfigFolder(_ sender: Any?) {
        try? FileManager.default.createDirectory(at: Preferences.folderURL, withIntermediateDirectories: true)
        NSWorkspace.shared.open(Preferences.folderURL)
    }
}
