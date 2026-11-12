import Carbon
import Foundation

// Registers and enables the oh-key input method for the logged-in user, then lists what the
// system sees. Usage: register-input-source <path to oh-key.app> [--select] [--list] [--disable]
// It works from an ssh shell of the same user; launchctl asuser $(id -u) is the fallback.
let modeIdentifier = "org.codepawl.inputmethod.OhKey.Vietnamese"

func sourceProperty(_ source: TISInputSource, _ key: CFString) -> String {
    guard let raw = TISGetInputSourceProperty(source, key) else { return "" }
    return Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
}

func isFlagSet(_ source: TISInputSource, _ key: CFString) -> Bool {
    guard let raw = TISGetInputSourceProperty(source, key) else { return false }
    return CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque(raw).takeUnretainedValue())
}

func ohKeySources() -> [TISInputSource] {
    let filter = [kTISPropertyInputSourceID as String: modeIdentifier] as CFDictionary
    let list = TISCreateInputSourceList(filter, true)?.takeRetainedValue() as? [TISInputSource]
    return list ?? []
}

let arguments = CommandLine.arguments
if arguments.count > 1, !arguments[1].hasPrefix("--") {
    let url = URL(fileURLWithPath: arguments[1]) as CFURL
    print("TISRegisterInputSource: \(TISRegisterInputSource(url))")
}

// The input method itself first, then its Vietnamese mode.
let parentFilter = [kTISPropertyInputSourceID as String: "org.codepawl.inputmethod.OhKey"] as CFDictionary
let parents = TISCreateInputSourceList(parentFilter, true)?.takeRetainedValue() as? [TISInputSource] ?? []
for parent in parents {
    print("enable input method: \(TISEnableInputSource(parent))")
    print("parent: enabled=\(isFlagSet(parent, kTISPropertyInputSourceIsEnabled)) "
        + "selectCapable=\(isFlagSet(parent, kTISPropertyInputSourceIsSelectCapable))")
}
if parents.isEmpty {
    print("parent input method not listed: \(parentFilter)")
}
for source in ohKeySources() {
    print("enable mode: \(TISEnableInputSource(source))")
}

if arguments.contains("--disable") {
    for source in ohKeySources() {
        print("disable mode: \(TISDisableInputSource(source))")
    }
    for parent in parents {
        print("disable input method: \(TISDisableInputSource(parent))")
    }
    exit(0)
}

if arguments.contains("--select") {
    for source in ohKeySources() {
        let status = TISSelectInputSource(source)
        print("select: \(status)")
        if status == -50 {
            print("select failed (-50): enable it by hand once in System Settings > Keyboard > Input Sources > Vietnamese > oh-key")
        }
    }
}

let sources = ohKeySources()
if sources.isEmpty {
    print("NOT LISTED: \(modeIdentifier)")
    exit(1)
}
for source in sources {
    print("listed: id=\(sourceProperty(source, kTISPropertyInputSourceID)) "
        + "enabled=\(isFlagSet(source, kTISPropertyInputSourceIsEnabled)) "
        + "selectCapable=\(isFlagSet(source, kTISPropertyInputSourceIsSelectCapable)) "
        + "selected=\(isFlagSet(source, kTISPropertyInputSourceIsSelected))")
}
