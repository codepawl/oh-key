#!/usr/bin/env bash
# Builds oh-key.app, the macOS input method, as a universal (arm64 + x86_64) bundle, signs it
# ad hoc and zips it. Needs Rust (rustup) and the Xcode command line tools; no Xcode project.
#
#   macos/build.sh              -> target/mac/oh-key.app and target/mac/oh-key-macos.zip
#
# Install by hand: copy oh-key.app to ~/Library/Input Methods, log out and in, then add
# oh-key under System Settings > Keyboard > Input Sources > Vietnamese.
set -euo pipefail

repository="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
output="${OH_KEY_MAC_OUTPUT:-$repository/target/mac}"
bundle="$output/oh-key.app"
work="$output/work"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repository/Cargo.toml" | head -1)"
minimum_macos="12.0"

rm -rf "$bundle" "$work" "$output/oh-key-macos.zip"
mkdir -p "$work"

echo "== Rust static library (arm64 + x86_64)"
for triple in aarch64-apple-darwin x86_64-apple-darwin; do
    MACOSX_DEPLOYMENT_TARGET="$minimum_macos" cargo build --release --locked \
        -p oh-key-ffi --target "$triple" --manifest-path "$repository/Cargo.toml" \
        --target-dir "$repository/target/mac-rust"
done
lipo -create \
    "$repository/target/mac-rust/aarch64-apple-darwin/release/liboh_key_ffi.a" \
    "$repository/target/mac-rust/x86_64-apple-darwin/release/liboh_key_ffi.a" \
    -output "$work/liboh_key_ffi.a"

echo "== Icons"
swift "$repository/macos/make-icon.swift" "$work"
iconutil -c icns "$work/oh-key.iconset" -o "$work/oh-key.icns"

echo "== Swift app (arm64 + x86_64)"
sources=("$repository"/macos/Sources/*.swift)
for architecture in arm64 x86_64; do
    swiftc -O -swift-version 5 -target "$architecture-apple-macos$minimum_macos" \
        -import-objc-header "$repository/macos/Sources/OhKeyBridge.h" \
        -Xcc -I"$repository/crates/oh-key-ffi/include" \
        -framework Cocoa -framework InputMethodKit \
        "${sources[@]}" "$work/liboh_key_ffi.a" -o "$work/oh-key-$architecture"
done
lipo -create "$work/oh-key-arm64" "$work/oh-key-x86_64" -output "$work/oh-key"

echo "== Bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp "$work/oh-key" "$bundle/Contents/MacOS/oh-key"
cp "$work/oh-key.icns" "$work/menu-icon.tiff" "$bundle/Contents/Resources/"
sed "s/__VERSION__/$version/g" "$repository/macos/Info.plist" > "$bundle/Contents/Info.plist"
printf 'APPL????' > "$bundle/Contents/PkgInfo"
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --sign - --timestamp=none "$bundle"
codesign --verify --strict "$bundle"

echo "== Zip"
(cd "$output" && ditto -c -k --keepParent oh-key.app oh-key-macos.zip)
file "$bundle/Contents/MacOS/oh-key"
echo "built $output/oh-key-macos.zip"
