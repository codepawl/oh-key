#!/usr/bin/env bash
# Builds the IBus engine: the Rust key handling as a static library, linked into the C glue.
# Needs: a Rust toolchain, gcc, pkg-config and the libibus development files
# (Debian and Ubuntu: libibus-1.0-dev, Fedora: ibus-devel).
# Output: target/linux/ibus-engine-oh-key
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output="$root/target/linux"
version="$(grep -m1 '^version' "$root/Cargo.toml" | cut -d'"' -f2)"

cargo build -p oh-key-ibus --release --locked --manifest-path "$root/Cargo.toml"

mkdir -p "$output"
# The native libraries Rust's standard library needs when linked from C.
cc -O2 -Wall \
    -DVERSION_STRING="\"$version\"" \
    -I"$root/crates/oh-key-ibus/include" \
    -o "$output/ibus-engine-oh-key" \
    "$root/linux/ibus/main.c" \
    "$root/target/release/liboh_key_ibus.a" \
    $(pkg-config --cflags --libs ibus-1.0) \
    -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc

echo "built $output/ibus-engine-oh-key"
