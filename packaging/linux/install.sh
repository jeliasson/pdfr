#!/bin/sh
# Installs pdfr's files into a Linux file system tree. Used by the AUR
# packages, the release tarball, Flatpak and anyone installing by hand.
#
#   packaging/linux/install.sh <binary> <destdir> [prefix]
#
# Example: packaging/linux/install.sh target/release/pdfr "$pkgdir" /usr
set -eu
bin=$1
dest=${2%/}
prefix=${3:-/usr}
root=$(cd "$(dirname "$0")/../.." && pwd)
id=io.github.kjellberg.pdfr
share="$dest$prefix/share"

install -Dm755 "$bin" "$dest$prefix/bin/pdfr"
install -Dm644 "$root/packaging/linux/$id.desktop" "$share/applications/$id.desktop"
install -Dm644 "$root/packaging/linux/$id.metainfo.xml" "$share/metainfo/$id.metainfo.xml"
for s in 16 22 24 32 48 64 128 256 512; do
  install -Dm644 "$root/assets/icon/png/pdfr-$s.png" "$share/icons/hicolor/${s}x${s}/apps/$id.png"
done
install -Dm644 "$root/assets/icon/pdfr.svg" "$share/icons/hicolor/scalable/apps/$id.svg"
install -Dm644 "$root/LICENSE.md" "$share/licenses/pdfr/LICENSE.md"
install -Dm644 "$root/assets/LICENSE-Inter.txt" "$share/licenses/pdfr/LICENSE-Inter.txt"
install -Dm644 "$root/assets/LICENSE-GreatVibes.txt" "$share/licenses/pdfr/LICENSE-GreatVibes.txt"
install -Dm644 "$root/assets/THIRD-PARTY-NOTICES.txt" "$share/licenses/pdfr/THIRD-PARTY-NOTICES.txt"
install -Dm644 "$root/README.md" "$share/doc/pdfr/README.md"
