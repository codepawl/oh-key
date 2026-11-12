import Cocoa

// Headless tests of OhKeyEditor against a mock text field. Run with macos/test-editor.sh.

let hostKeyLeftArrow: UInt16 = 123
let hostKeyReturn: UInt16 = 36
let hostKeyEscape: UInt16 = 53
let hostKeyBackspace: UInt16 = 51
let hostKeyZ: UInt16 = 6

var failures = 0
var checks = 0

/// Plays keys into an editor and the mock field the way a host app would.
struct Typist {
    let editor: OhKeyEditor
    let field: MockTextClient

    func type(_ keys: String) {
        for character in keys {
            let isUpper = character.isUppercase
            press(keyCode: 0, characters: String(character), modifiers: isUpper ? .shift : [])
        }
    }

    func press(keyCode: UInt16, characters: String, modifiers: NSEvent.ModifierFlags = []) {
        let key = OhKeyInput(keyCode: keyCode, characters: characters, modifiers: modifiers)
        let consumed = editor.handleKeyDown(key, client: field)
        if consumed { return }
        applyHostBehaviour(keyCode: keyCode, characters: characters, modifiers: modifiers)
    }

    private func applyHostBehaviour(keyCode: UInt16, characters: String, modifiers: NSEvent.ModifierFlags) {
        if !modifiers.isDisjoint(with: [.command, .control, .option]) { return }
        switch keyCode {
        case hostKeyBackspace: field.hostBackspace()
        case hostKeyReturn: field.hostInsert("\n")
        case hostKeyLeftArrow: field.hostMoveCursor(by: -1)
        case hostKeyEscape: break
        default: field.hostInsert(characters)
        }
    }

    func backspace(_ times: Int = 1) {
        for _ in 0..<times { press(keyCode: hostKeyBackspace, characters: "\u{7F}") }
    }

    func returnKey() { press(keyCode: hostKeyReturn, characters: "\r") }
    func escape() { press(keyCode: hostKeyEscape, characters: "\u{1B}") }
    func leftArrow() { press(keyCode: hostKeyLeftArrow, characters: "\u{F702}") }
}

func makeEditor() -> OhKeyEditor {
    Preferences.vietnameseEnabled = true
    let editor = OhKeyEditor(loadsSettingsFromDisk: false)
    editor.load(settingsText: "")
    return editor
}

func check(_ name: String, _ field: MockTextClient, expect: String, file: String = #file, line: Int = #line) {
    checks += 1
    if field.text == expect {
        print("pass  \(name): \(expect.debugDescription)")
    } else {
        failures += 1
        print("FAIL  \(name): expected \(expect.debugDescription), got \(field.text.debugDescription)  calls=\(field.calls)")
    }
}

func scenario(_ name: String, cursor: CursorReporting = .accurate, bundle: String? = "com.apple.TextEdit",
              body: (Typist, MockTextClient, OhKeyEditor) -> Void) {
    let editor = makeEditor()
    let field = MockTextClient(cursorReporting: cursor, bundle: bundle)
    editor.activate(client: field)
    body(Typist(editor: editor, field: field), field, editor)
    let textBeforeDeactivate = field.text
    editor.deactivate(client: field)
    checkDeactivateKeepsTheText(name, field, textBefore: textBeforeDeactivate)
    if cursor != .lagging {
        checkNoRangeViolations(name, field)
    }
}

/// Deactivating commits what is composing: the text stays the same and no marked text is left.
func checkDeactivateKeepsTheText(_ name: String, _ field: MockTextClient, textBefore: String) {
    checks += 1
    if field.text == textBefore && field.markedText.isEmpty {
        print("pass  \(name) (after deactivate)")
    } else {
        failures += 1
        print("FAIL  \(name) (after deactivate): text \(textBefore.debugDescription) became \(field.text.debugDescription), marked \(field.markedText.debugDescription)")
    }
}

/// The editor must never ask a client for a range outside its buffer.
func checkNoRangeViolations(_ name: String, _ field: MockTextClient) {
    checks += 1
    if field.rangeViolations.isEmpty {
        print("pass  \(name) (ranges inside the buffer)")
    } else {
        failures += 1
        print("FAIL  \(name) (ranges inside the buffer): \(field.rangeViolations)")
    }
}

// MARK: Telex words

scenario("telex word") { typist, field, _ in typist.type("vieejt "); check("telex word", field, expect: "việt ") }
scenario("telex sentence") { typist, field, _ in
    typist.type("Xin chaof cacs banf ")
    check("telex sentence", field, expect: "Xin chào các bàn ")
}
scenario("telex long") { typist, field, _ in
    typist.type("Nguwowif Vieejt Nam ddang hocj lapaj trinhf ")
    check("telex long", field, expect: "Người Việt Nam đang học lập trình ")
}
scenario("uppercase") { typist, field, _ in
    typist.type("VIEEJT ")
    check("uppercase all caps", field, expect: "VIỆT ")
}
scenario("uppercase first") { typist, field, _ in
    typist.type("Vieejt")
    check("uppercase first letter", field, expect: "Việt")
}

// MARK: English restore

scenario("english restore") { typist, field, _ in
    typist.type("text ")
    check("english text", field, expect: "text ")
}
scenario("english restore 2") { typist, field, _ in
    typist.type("window ")
    check("english window", field, expect: "window ")
}
scenario("english restore 3") { typist, field, _ in
    typist.type("github ")
    check("english github", field, expect: "github ")
}
scenario("mixed") { typist, field, _ in
    typist.type("toi dungf Claude ")
    check("mixed vietnamese and english", field, expect: "toi dùng Claude ")
}

// MARK: Backspace

scenario("backspace tail") { typist, field, _ in
    typist.type("tooi")
    typist.backspace()
    check("backspace after tooi", field, expect: "tô")
}
scenario("backspace then retype") { typist, field, _ in
    typist.type("vieejt")
    typist.backspace(2)
    typist.type("tj")
    check("backspace twice then tj", field, expect: "vịt")
}
scenario("backspace whole word") { typist, field, _ in
    typist.type("an")
    typist.backspace(2)
    check("backspace erases word", field, expect: "")
}
scenario("backspace after space") { typist, field, _ in
    typist.type("an ")
    typist.backspace()
    check("backspace after space", field, expect: "an")
}
scenario("backspace on empty field") { typist, field, _ in
    typist.backspace(3)
    check("backspace on empty", field, expect: "")
}

// MARK: Shortcuts and non-text keys

scenario("command shortcut") { typist, field, _ in
    typist.type("tooi")
    typist.press(keyCode: 0, characters: "a", modifiers: .command)
    check("cmd+a leaves text", field, expect: "tôi")
    typist.type("s")
    check("word is closed after cmd+a", field, expect: "tôis")
}
scenario("control shortcut") { typist, field, _ in
    typist.type("tooi")
    typist.press(keyCode: 0, characters: "\u{05}", modifiers: .control)
    typist.type("s")
    check("ctrl+e closes word", field, expect: "tôis")
}
scenario("return") { typist, field, _ in
    typist.type("vieejt")
    typist.returnKey()
    typist.type("tooi")
    check("return ends word", field, expect: "việt\ntôi")
}
scenario("escape") { typist, field, _ in
    typist.type("tooi")
    typist.escape()
    typist.type("s")
    check("escape ends word", field, expect: "tôis")
}
scenario("arrow") { typist, field, _ in
    typist.type("ba")
    typist.leftArrow()
    typist.type("s")
    check("left arrow then typing", field, expect: "bsa")
}

// MARK: Selection and cursor

scenario("selection replace") { typist, field, _ in
    field.hostSetText("abc", cursor: 3)
    field.hostSelect(NSRange(location: 0, length: 3))
    typist.type("tooi")
    check("selection replaced by word", field, expect: "tôi")
}
scenario("selection then backspace") { typist, field, _ in
    field.hostSetText("abc def", cursor: 7)
    field.hostSelect(NSRange(location: 4, length: 3))
    typist.backspace()
    check("backspace deletes selection", field, expect: "abc ")
}
scenario("click away") { typist, field, _ in
    typist.type("toi")
    field.hostSelect(NSRange(location: 0, length: 0))
    typist.type("s")
    check("click away then typing", field, expect: "stoi")
}
scenario("click into middle") { typist, field, _ in
    field.hostSetText("hello world", cursor: 5)
    typist.type("tooi")
    check("typing mid text", field, expect: "hellotôi world")
}

// MARK: Chrome-like fields

scenario("chrome cursor unavailable", cursor: .unavailable) { typist, field, _ in
    typist.type("vieejt ")
    check("cursor unavailable word", field, expect: "việt ")
}
scenario("chrome cursor unavailable sentence", cursor: .unavailable) { typist, field, _ in
    typist.type("Xin chaof cacs banf ")
    check("cursor unavailable sentence", field, expect: "Xin chào các bàn ")
}
scenario("chrome cursor unavailable everywhere", cursor: .unavailableEverywhere) { typist, field, _ in
    typist.type("vieejt ")
    check("cursor not found in both fields", field, expect: "việt ")
}
scenario("chrome bundle", bundle: "com.google.Chrome") { typist, field, _ in
    typist.type("Xin chaof cacs banf ")
    check("chrome bundle uses marked text", field, expect: "Xin chào các bàn ")
}
scenario("chrome backspace", cursor: .unavailable) { typist, field, _ in
    typist.type("tooi")
    typist.backspace()
    typist.type("i")
    check("marked text backspace", field, expect: "tôi")
}
scenario("chrome backspace whole word", cursor: .unavailable) { typist, field, _ in
    typist.type("an")
    typist.backspace(2)
    check("marked text erase word", field, expect: "")
}
scenario("chrome return", cursor: .unavailable) { typist, field, _ in
    typist.type("vieejt")
    typist.returnKey()
    check("marked text return", field, expect: "việt\n")
}
scenario("chrome lagging cursor", cursor: .lagging) { typist, field, _ in
    typist.type("Xin chaof cacs banf ")
    // Worst case, not a pass/fail case: a field that reports its cursor one change late can corrupt the
    // first word before the editor notices and switches to marked text. Reported, not asserted.
    print("info  lagging cursor (one change late): \(field.text.debugDescription)")
}

// MARK: Toggle and settings

scenario("alt z toggles") { typist, field, editor in
    editor.load(settingsText: "toggle_hotkey = \"alt+z\"\n")
    typist.press(keyCode: hostKeyZ, characters: "z", modifiers: .option)
    typist.type("tooi ")
    check("english after alt+z", field, expect: "tooi ")
    typist.press(keyCode: hostKeyZ, characters: "z", modifiers: .option)
    typist.type("tooi ")
    check("vietnamese again after alt+z", field, expect: "tooi tôi ")
}
scenario("ctrl shift toggles") { typist, field, editor in
    editor.handleModifierChange([.control], client: field)
    editor.handleModifierChange([.control, .shift], client: field)
    editor.handleModifierChange([], client: field)
    typist.type("tooi ")
    check("english after ctrl+shift", field, expect: "tooi ")
    editor.handleModifierChange([.control, .shift], client: field)
    typist.type("a")
    editor.handleModifierChange([], client: field)
    typist.type("tooi ")
    check("chord with a key between does not toggle", field, expect: "tooi atooi ")
}
scenario("excluded app",bundle: "com.example.Terminal") { typist, field, editor in
    editor.load(settingsText: "excluded_apps = \"com.example.Terminal\"\n")
    typist.type("tooi ")
    check("excluded app keeps keys", field, expect: "tooi ")
}
scenario("vni") { typist, field, editor in
    editor.load(settingsText: "input_method = \"vni\"\n")
    typist.type("vie6t5 ")
    check("vni word", field, expect: "việt ")
}

// MARK: Rapid and long input (the reported spam / missing tone symptoms)

scenario("rapid 2000 keys") { typist, field, _ in
    for _ in 0..<200 { typist.type("tooi ") }
    check("2000 keys", field, expect: String(repeating: "tôi ", count: 200))
}

// MARK: Hosts that ignore the replacement range, and a cursor that is too small

do {
    let editor = makeEditor()
    let field = MockTextClient(bundle: "com.example.Terminal", ignoresReplacementRange: true)
    editor.activate(client: field)
    let typist = Typist(editor: editor, field: field)
    typist.type("tooi tooi tooi ")
    checks += 1
    if editor.usesMarkedText {
        print("pass  a host that ignores the replacement range switches the editor to marked text")
    } else {
        failures += 1
        print("FAIL  a host that ignores the replacement range was not detected: \(field.text.debugDescription)")
    }
    print("info  range-ignoring host text: \(field.text.debugDescription)")
}
do {
    let editor = makeEditor()
    let field = MockTextClient()
    editor.activate(client: field)
    let typist = Typist(editor: editor, field: field)
    typist.type("too")
    field.hostSelect(NSRange(location: 0, length: 0))
    typist.type("i")
    checkNoRangeViolations("cursor smaller than the word", field)
}

let reportingResult = runReportingTests()
checks += reportingResult.checks
failures += reportingResult.failures

print("\n\(checks - failures) of \(checks) passed")
exit(failures == 0 ? 0 : 1)
