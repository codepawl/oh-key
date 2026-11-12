#!/usr/bin/env bash
# Builds and runs the headless editor tests: OhKeyEditor against a mock text field, no
# InputMethodKit and no window. Needs Rust and swiftc on a Mac; exit 0 means every case passed.
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
output="${OH_KEY_MAC_OUTPUT:-$repository/target/mac}/tests"
mkdir -p "$output"

cargo build --release --locked -p oh-key-ffi --manifest-path "$repository/Cargo.toml" \
    --target-dir "$repository/target/mac-rust-host"

swiftc -O -swift-version 5 \
    -import-objc-header "$repository/macos/Sources/OhKeyBridge.h" \
    -Xcc -I"$repository/crates/oh-key-ffi/include" \
    -framework Cocoa \
    "$repository/macos/Sources/OhKeyEditor.swift" \
    "$repository/macos/Sources/Preferences.swift" \
    "$repository/macos/Tests/MockTextClient.swift" \
    "$repository/macos/Tests/main.swift" \
    "$repository/target/mac-rust-host/release/liboh_key_ffi.a" \
    -o "$output/editor-tests"

"$output/editor-tests"
