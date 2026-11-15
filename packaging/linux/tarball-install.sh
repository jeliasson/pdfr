#!/bin/sh
# pdfr from the release archive.
#
# Just for you, no root needed (app menu, "Open with", the pdfr command):
#   ./pdfr --register
#
# For everyone on this computer:
#   sudo ./install.sh                 (installs into /usr/local)
#   ./install.sh <destdir> [prefix]   (for packagers)
set -eu
dir=$(cd "$(dirname "$0")" && pwd)
exec "$dir/packaging/linux/install.sh" "$dir/pdfr" "${1:-}" "${2:-/usr/local}"
