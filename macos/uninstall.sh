#!/usr/bin/env bash
# Removes oh-key from this Mac: disables its input sources, stops the process and deletes the
# bundle. A logout may be needed before macOS forgets the registration.
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
destination="$HOME/Library/Input Methods/oh-key.app"
tool="${OH_KEY_MAC_OUTPUT:-$repository/target/mac}/tests/register-input-source"

if [ -x "$tool" ]; then
    "$tool" --disable || true
else
    echo "no register-input-source tool at $tool; run macos/install.sh once, or disable oh-key in System Settings > Keyboard > Input Sources"
fi
pkill -x oh-key 2>/dev/null || true
rm -rf "$destination"
echo "removed: $destination"
