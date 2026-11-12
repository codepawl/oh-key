#!/usr/bin/env bash
# Live end-to-end test on a Mac with oh-key installed (macos/install.sh): a test app types real
# key events into its own window and reads the text back. Exit 0 = all passed, 1 = a case failed,
# 2 = macOS blocked synthetic events (the output says which permission to grant), 3 = oh-key is
# not installed or older than the build, 4 = the oh-key input source could not be selected, 5 = the
# person at the Mac was typing (set OH_KEY_LIVE_IGNORE_IDLE=1 to skip that check), 6 = the test window
# lost the keyboard focus. Do not run it while someone is typing: the events go through the HID tap to the
# front app, and the test app brings its own window to the front first.
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output="${OH_KEY_MAC_OUTPUT:-$repository/target/mac}/tests"
bundle="$output/oh-key-live-test.app"

installed="$HOME/Library/Input Methods/oh-key.app/Contents/MacOS/oh-key"
built="${OH_KEY_MAC_OUTPUT:-$repository/target/mac}/oh-key.app/Contents/MacOS/oh-key"
if [ ! -f "$installed" ]; then
    echo "oh-key is not installed; run macos/install.sh first"
    exit 3
fi
echo "installed oh-key: $installed ($(stat -f '%Sm' "$installed"))"
if [ -f "$built" ] && [ "$built" -nt "$installed" ]; then
    echo "the installed oh-key is older than the build in $built; run macos/install.sh first"
    exit 3
fi

# Rebuilding and signing again changes the signature, and macOS ties the Accessibility grant to it:
# the test app is built only when its source is newer.
if [ -x "$bundle/Contents/MacOS/oh-key-live-test" ] && [ ! "$repository/macos/live/LiveTest.swift" -nt "$bundle/Contents/MacOS/oh-key-live-test" ]; then
    exec "$bundle/Contents/MacOS/oh-key-live-test"
fi

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS"
swiftc -O "$repository/macos/live/LiveTest.swift" -framework Cocoa -framework Carbon \
    -o "$bundle/Contents/MacOS/oh-key-live-test"
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>oh-key-live-test</string>
<key>CFBundleIdentifier</key><string>org.codepawl.ohkey.livetest</string>
<key>CFBundleName</key><string>oh-key-live-test</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
"$bundle/Contents/MacOS/oh-key-live-test"
