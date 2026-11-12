#!/usr/bin/env bash
# Installs the built oh-key.app into ~/Library/Input Methods (replacing an old copy), then
# registers and enables its input mode in the logged-in GUI session. Run after macos/build.sh.
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bundle="${OH_KEY_MAC_OUTPUT:-$repository/target/mac}/oh-key.app"
destination="$HOME/Library/Input Methods/oh-key.app"
tool="${OH_KEY_MAC_OUTPUT:-$repository/target/mac}/tests/register-input-source"

mkdir -p "$(dirname "$tool")" "$HOME/Library/Input Methods"
swiftc -O "$repository/macos/live/register-input-source.swift" -framework Carbon -o "$tool"

pkill -x oh-key 2>/dev/null || true
rm -rf "$destination"
cp -R "$bundle" "$destination"
echo "installed: $destination (version $(plutil -extract CFBundleShortVersionString raw "$destination/Contents/Info.plist" 2>/dev/null || echo unknown))"
# The TIS calls reach the login session from an ssh shell of the same user; asuser needs root and is only a fallback.
# Exit 1 means the tool ran and the mode is not listed: running it a second time would say the same.
status=0
"$tool" "$destination" --list || status=$?
if [ "$status" -ne 0 ] && [ "$status" -ne 1 ]; then
    launchctl asuser "$(id -u)" "$tool" "$destination" --list
fi
