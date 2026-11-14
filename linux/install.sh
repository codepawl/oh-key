#!/usr/bin/env bash
# Installs the oh-key IBus engine from an unpacked oh-key-linux-x86_64.tar.gz.
#   sudo ./install.sh                  into /usr (where IBus looks for components)
#   PREFIX=/opt/oh-key ./install.sh    elsewhere; IBUS_COMPONENT_PATH must then include its share/ibus/component
# DESTDIR stages the files for a package.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
prefix="${PREFIX:-/usr}"
destdir="${DESTDIR:-}"
engine_path="$prefix/libexec/ibus-engine-oh-key"
component_folder="$destdir$prefix/share/ibus/component"
version="$("$here/ibus-engine-oh-key" --version | awk '{print $2}')"

install -d "$destdir$prefix/libexec" "$component_folder"
install -m 755 "$here/ibus-engine-oh-key" "$destdir$engine_path"
sed -e "s|@ENGINE_PATH@|$engine_path|" -e "s|@VERSION@|$version|" \
    "$here/oh-key.xml.in" > "$component_folder/oh-key.xml"

echo "Installed oh-key $version."
echo "Next: run 'ibus restart', then add Vietnamese (oh-key) in Settings > Keyboard > Input Sources."
