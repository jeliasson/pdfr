#!/bin/sh
# Builds the Linux release archive from a compiled binary.
#   tools/package_linux.sh <binary> <version> <arch>   ->  pdfr-v<version>-<arch>-linux.tar.gz
set -eu
bin=$1; version=$2; arch=$3
root=$(cd "$(dirname "$0")/.." && pwd)
name="pdfr-v$version-$arch-linux"
work=$(mktemp -d)
d="$work/$name"
mkdir -p "$d/packaging/linux" "$d/assets/icon/png"
install -m755 "$bin" "$d/pdfr"
install -m755 "$root/packaging/linux/tarball-install.sh" "$d/install.sh"
install -m755 "$root/packaging/linux/install.sh" "$d/packaging/linux/"
cp "$root/packaging/linux/io.github.kjellberg.pdfr.desktop" "$root/packaging/linux/io.github.kjellberg.pdfr.metainfo.xml" "$d/packaging/linux/"
cp "$root"/assets/icon/png/pdfr-*.png "$d/assets/icon/png/"
rm -f "$d/assets/icon/png/pdfr-1024.png"
cp "$root/assets/icon/pdfr.svg" "$d/assets/icon/"
cp "$root/assets/LICENSE-Inter.txt" "$root/assets/THIRD-PARTY-NOTICES.txt" "$d/assets/"
cp "$root/README.md" "$root/LICENSE.md" "$d/"
tar -C "$work" -czf "$name.tar.gz" --owner=0 --group=0 "$name"
rm -rf "$work"
echo "$name.tar.gz"
