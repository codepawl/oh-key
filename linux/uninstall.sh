#!/usr/bin/env bash
# Removes what install.sh put in place (same PREFIX and DESTDIR). Your settings in
# ~/.config/oh-key are left alone.
set -euo pipefail

prefix="${PREFIX:-/usr}"
destdir="${DESTDIR:-}"

rm -f "$destdir$prefix/libexec/ibus-engine-oh-key" "$destdir$prefix/share/ibus/component/oh-key.xml"
echo "Removed oh-key. Run 'ibus restart' to unload it."
