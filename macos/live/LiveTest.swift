import Carbon
import Cocoa

// Live end-to-end test: opens its own window with an NSTextView, switches to the oh-key input
// source, posts real key events through the HID tap, reads the text back and restores the
// previous input source. It never types anywhere else, and it does not ask for permissions:
// when macOS blocks synthetic events it reports which permission is missing.

let ohKeyModeIdentifier = "org.codepawl.inputmethod.OhKey.Vietnamese"

// US layout virtual key codes.
let virtualKeys: [Character: CGKeyCode] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11,
    "q": 12, "w": 13, "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21,
    "6": 22, "5": 23, "9": 25, "7": 26, "8": 28, "0": 29, "o": 31, "u": 32, "i": 34, "p": 35,
    "l": 37, "j": 38, "k": 40, "n": 45, "m": 46, " ": 49,
]
let keyCodeReturn: CGKeyCode = 36
let keyCodeDelete: CGKeyCode = 51
let keyCodeLeftArrow: CGKeyCode = 123

func inputSource(withIdentifier identifier: String) -> TISInputSource? {
    let filter = [kTISPropertyInputSourceID as String: identifier] as CFDictionary
    let list = TISCreateInputSourceList(filter, true)?.takeRetainedValue() as? [TISInputSource]
    return list?.first
}

func identifier(of source: TISInputSource) -> String {
    guard let raw = TISGetInputSourceProperty(source, kTISPropertyInputSourceID) else { return "?" }
    return Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
}

func pause(_ seconds: Double) {
    RunLoop.current.run(until: Date(timeIntervalSinceNow: seconds))
}

final class LiveTest {
    let window: NSWindow
    let textView: NSTextView
    var failures = 0
    var checks = 0
    /// Set when the window lost the keyboard focus: nothing more is posted, so no key can reach
    /// another app.
    var abortedForFocus = false

    init() {
        textView = NSTextView(frame: NSRect(x: 0, y: 0, width: 520, height: 200))
        textView.isRichText = false
        textView.isAutomaticTextCompletionEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        window = NSWindow(contentRect: NSRect(x: 80, y: 200, width: 520, height: 200),
                          styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "oh-key live test (automated)"
        window.contentView = textView
    }

    func showWindow() {
        NSApp.setActivationPolicy(.regular)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        window.makeFirstResponder(textView)
        pause(1.0)
    }

    /// The events go to whichever app is in front, so every key first checks that it is this window.
    func windowHoldsTheKeyboard() -> Bool {
        if abortedForFocus { return false }
        if NSApp.isActive && window.isKeyWindow { return true }
        abortedForFocus = true
        print("ABORT: the test window is not the key window; nothing more is posted")
        return false
    }

    func post(_ keyCode: CGKeyCode, shift: Bool = false, command: Bool = false, gap: Double = 0.012, settle: Double = 0.03) {
        guard windowHoldsTheKeyboard() else { return }
        for isDown in [true, false] {
            guard let event = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: isDown) else { continue }
            var flags: CGEventFlags = []
            if shift { flags.insert(.maskShift) }
            if command { flags.insert(.maskCommand) }
            event.flags = flags
            event.post(tap: .cghidEventTap)
            pause(gap)
        }
        pause(settle)
    }

    func type(_ keys: String) {
        for character in keys {
            let lower = Character(character.lowercased())
            guard let keyCode = virtualKeys[lower] else { continue }
            post(keyCode, shift: character.isUppercase)
        }
        pause(0.2)
    }

    /// Keys about 2 ms apart, like a fast typist or a key queue draining after a stall.
    func typeInBurst(_ keys: String) {
        for character in keys {
            guard let keyCode = virtualKeys[Character(character.lowercased())] else { continue }
            post(keyCode, shift: character.isUppercase, gap: 0.001, settle: 0.0)
        }
        pause(0.5)
    }

    func clear() {
        textView.string = ""
        window.makeFirstResponder(textView)
        pause(0.15)
    }

    func check(_ name: String, expect: String) {
        checks += 1
        let actual = textView.string
        if actual == expect {
            print("pass  \(name): \(expect.debugDescription)")
        } else {
            failures += 1
            print("FAIL  \(name): expected \(expect.debugDescription), got \(actual.debugDescription)")
        }
    }

    func run() {
        clear(); type("vieejt "); check("telex word", expect: "việt ")
        clear(); type("Xin chaof cacs banf "); check("telex sentence", expect: "Xin chào các bàn ")
        clear(); type("Vieejt"); check("uppercase first", expect: "Việt")
        clear(); type("text "); check("english restore text", expect: "text ")
        clear(); type("window "); check("english restore window", expect: "window ")
        clear(); type("tooi"); post(keyCodeDelete); check("backspace after tooi", expect: "tô")
        clear(); type("vieejt"); post(keyCodeReturn); type("tooi"); check("return ends word", expect: "việt\ntôi")
        clear(); type("ba"); post(keyCodeLeftArrow); type("s"); check("arrow then typing", expect: "bsa")
        clear(); type("tooi"); post(virtualKeys["a"]!, command: true); type("s")
        check("cmd+a selects all then typing replaces", expect: "s")
        clear()
        for _ in 0..<50 { type("tooi ") }
        check("50 words fast", expect: String(repeating: "tôi ", count: 50))
        runBurstUnderLoad()
    }

    /// The symptom the owner reported: spam or missing tone marks when the machine is busy.
    func runBurstUnderLoad() {
        let load = BusyCores(count: ProcessInfo.processInfo.activeProcessorCount)
        clear()
        for _ in 0..<20 { typeInBurst("tooi ") }
        check("20 words in a 2 ms burst with every core busy", expect: String(repeating: "tôi ", count: 20))
        load.stop()
    }
}

/// Threads that spin until stopped, so the input method competes for the cores.
final class BusyCores {
    private var stopRequested = false
    private let lock = NSLock()

    init(count: Int) {
        for _ in 0..<count {
            Thread.detachNewThread { [self] in
                while !shouldStop() {
                    _ = (0..<1000).reduce(0, +)
                }
            }
        }
    }

    private func shouldStop() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return stopRequested
    }

    func stop() {
        lock.lock()
        stopRequested = true
        lock.unlock()
    }
}

func reportPermissions() -> Bool {
    let post = CGPreflightPostEventAccess()
    let listen = CGPreflightListenEventAccess()
    let accessibility = AXIsProcessTrusted()
    print("permissions: PostEvent=\(post) ListenEvent(InputMonitoring)=\(listen) Accessibility=\(accessibility)")
    if !post && !accessibility {
        let binary = CommandLine.arguments[0]
        print("BLOCKED: synthetic key events need Accessibility (System Settings > Privacy & Security > Accessibility) for \(binary)")
        print("(when run over ssh the responsible process may instead be sshd-keygen-wrapper or Terminal; grant the one macOS lists)")
        return false
    }
    return true
}

let application = NSApplication.shared
let previousSource = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue()
print("previous input source: \(previousSource.map(identifier(of:)) ?? "none")")

func restoreInputSource() {
    if let previousSource = previousSource {
        let status = TISSelectInputSource(previousSource)
        print("restored input source: \(identifier(of: previousSource)) (status \(status))")
    }
}

/// Restores the input source on every way out, including a signal.
func exitRestoringInputSource(_ code: Int32) -> Never {
    restoreInputSource()
    exit(code)
}

var signalSources: [DispatchSourceSignal] = []
for signalNumber in [SIGINT, SIGTERM, SIGHUP] {
    signal(signalNumber, SIG_IGN)
    let source = DispatchSource.makeSignalSource(signal: signalNumber, queue: .main)
    source.setEventHandler { exitRestoringInputSource(130) }
    source.resume()
    signalSources.append(source)
}

/// Seconds since the person at this Mac last touched the keyboard or mouse.
func secondsSinceLastUserInput() -> Double {
    let anyInput = CGEventType(rawValue: UInt32.max)!
    return CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: anyInput)
}
let minimumIdleSeconds = 3.0

guard let ohKey = inputSource(withIdentifier: ohKeyModeIdentifier) else {
    print("NOT LISTED: \(ohKeyModeIdentifier); run macos/install.sh first")
    exit(3)
}
let permitted = reportPermissions()
let idleSeconds = secondsSinceLastUserInput()
let ignoresIdleCheck = ProcessInfo.processInfo.environment["OH_KEY_LIVE_IGNORE_IDLE"] == "1"
if idleSeconds < minimumIdleSeconds && !ignoresIdleCheck {
    print("BUSY: the person at this Mac used it \(idleSeconds) s ago; not typing into their session (exit 5)")
    exit(5)
}
let test = LiveTest()
test.showWindow()
let selectStatus = TISSelectInputSource(ohKey)
print("selecting oh-key: status \(selectStatus)")
pause(1.5)
let current = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue()
let currentIdentifier = current.map(identifier(of:)) ?? "none"
print("current input source: \(currentIdentifier)")

if currentIdentifier != ohKeyModeIdentifier {
    // Another input method (Apple Telex) would turn these keys into Vietnamese on its own and the
    // test would pass without oh-key.
    print("NOT SELECTED: select status \(selectStatus), current \(currentIdentifier); enable oh-key in System Settings > Keyboard > Input Sources (exit 4)")
    exitRestoringInputSource(4)
}

var exitCode: Int32 = 0
if permitted {
    test.run()
    print("\(test.checks - test.failures) of \(test.checks) passed")
    if test.abortedForFocus {
        exitCode = 6
    } else {
        exitCode = test.failures == 0 ? 0 : 1
    }
} else {
    exitCode = 2
}
exitRestoringInputSource(exitCode)
