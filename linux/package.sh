#!/usr/bin/env bash
# Builds the engine and packs it: target/linux/oh-key-linux-x86_64.tar.gz and
# target/linux/oh-key_<version>_amd64.deb. Run on x86_64 Linux (see build.sh for what it needs).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output="$root/target/linux"
version="$(grep -m1 '^version' "$root/Cargo.toml" | cut -d'"' -f2)"

bash "$root/linux/build.sh"

# Tarball: the binary, the component template and the install scripts.
package="$output/oh-key-linux-x86_64"
rm -rf "$package"
mkdir -p "$package"
cp "$output/ibus-engine-oh-key" "$root/linux/ibus/oh-key.xml.in" \
    "$root/linux/install.sh" "$root/linux/uninstall.sh" \
    "$root/LICENSE" "$root/NOTICE" "$package/"
chmod 755 "$package/install.sh" "$package/uninstall.sh"
tar -C "$output" -czf "$output/oh-key-linux-x86_64.tar.gz" oh-key-linux-x86_64
echo "packed $output/oh-key-linux-x86_64.tar.gz"

# Deb: the same files laid out for /usr, through the install script's DESTDIR.
staging="$output/deb"
rm -rf "$staging"
DESTDIR="$staging" bash "$package/install.sh" > /dev/null
install -d "$staging/DEBIAN" "$staging/usr/share/doc/oh-key"
cp "$root/LICENSE" "$root/NOTICE" "$staging/usr/share/doc/oh-key/"
cat > "$staging/DEBIAN/control" <<EOF
Package: oh-key
Version: $version
Section: utils
Priority: optional
Architecture: amd64
Depends: ibus (>= 1.5.4), libibus-1.0-5, libglib2.0-0
Maintainer: codepawl <noreply@codepawl.com>
Homepage: https://github.com/codepawl/oh-key
Description: Vietnamese typing for IBus (Telex, VNI, VIQR)
 oh-key is a fast Vietnamese input method in the Unikey style. After installing, run
 "ibus restart" and add Vietnamese (oh-key) in the keyboard settings.
EOF
dpkg-deb --build --root-owner-group "$staging" "$output/oh-key_${version}_amd64.deb"
