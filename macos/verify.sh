#!/usr/bin/env bash
# Checks the built bundle: the engine tests on this Mac, the plist, the signature and both architectures.
set -euo pipefail
repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
bundle="${OH_KEY_MAC_OUTPUT:-$repository/target/mac}/oh-key.app"

bash "$repository/macos/test-editor.sh" 2>&1 | grep -v -E '^pass| Compiling '
cargo test --manifest-path "$repository/Cargo.toml" -p oh-key-ffi 2>&1 | grep -E "^test result|FAILED|^error"
plutil -lint "$bundle/Contents/Info.plist"
codesign --verify --strict --verbose=2 "$bundle"
echo "architectures: $(lipo -archs "$bundle/Contents/MacOS/oh-key")"
for key in InputMethodConnectionName InputMethodServerControllerClass LSBackgroundOnly CFBundleIdentifier; do
    plutil -p "$bundle/Contents/Info.plist" | grep "\"$key\""
done
for symbol in oh_key_session_new oh_key_session_process_key oh_key_session_finish_word; do
    nm -g "$bundle/Contents/MacOS/oh-key" | grep -q "_$symbol" && echo "symbol $symbol: present"
done
