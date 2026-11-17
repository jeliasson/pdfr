#!/bin/sh
# Installs pdfr, or updates it in place, on Linux and macOS. No root needed.
#
#   curl -fsSL https://pdfr.app/install.sh | sh
#
# Run it again to update. Options go after `sh -s --`:
#
#   curl -fsSL https://pdfr.app/install.sh | sh -s -- --version 0.1.0
#   curl -fsSL https://pdfr.app/install.sh | sh -s -- --uninstall
#
# Linux: the app goes in ~/.local/share/pdfr, the pdfr command in
# ~/.local/bin, and pdfr adds itself to the app menu and "Open with".
# macOS: pdfr.app goes in /Applications (or ~/Applications), and the pdfr
# command in /usr/local/bin (or ~/.local/bin).
#
# Every download is checked against the release's SHA256SUMS. The release
# workflow stamps its version below; an unstamped copy installs the newest
# release.
set -eu

VERSION="@VERSION@"
REPO="kjellberg/pdfr"

say() { printf '%s\n' "$*"; }
die() { printf 'pdfr: %s\n' "$*" >&2; exit 1; }

usage() {
	say "Installs pdfr, or updates it in place, on Linux and macOS."
	say ""
	say "  curl -fsSL https://pdfr.app/install.sh | sh"
	say "  curl -fsSL https://pdfr.app/install.sh | sh -s -- --version 0.1.0"
	say "  curl -fsSL https://pdfr.app/install.sh | sh -s -- --uninstall"
}

want=${PDFR_VERSION:-}
action=install
while [ $# -gt 0 ]; do
	case $1 in
	--version) [ $# -ge 2 ] || die "--version needs a version number"; want=$2; shift 2 ;;
	--version=*) want=${1#*=}; shift ;;
	--uninstall) action=uninstall; shift ;;
	-h | --help) usage; exit 0 ;;
	*) die "unknown option: $1 (try --help)" ;;
	esac
done

command -v curl >/dev/null 2>&1 || die "this installer needs curl"

fetch() {
	curl -fsSL --proto '=https' --tlsv1.2 --retry 3 -o "$2" "$1" || die "could not download $1"
}

latest() {
	curl -fsSL --proto '=https' --tlsv1.2 "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null |
		sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1
}

sha256() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | cut -d ' ' -f 1
	else
		shasum -a 256 "$1" | cut -d ' ' -f 1
	fi
}

# The version an installed pdfr reports, or nothing.
installed() {
	[ -x "$1" ] && "$1" --version 2>/dev/null | awk '{print $2}' || true
}

# Downloads a release file into $tmp and checks it against SHA256SUMS.
download() {
	fetch "$base/$1" "$tmp/$1"
	[ -f "$tmp/SHA256SUMS" ] || fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS"
	expected=$(awk -v n="$1" '$2 == n || $2 == "*" n {print $1}' "$tmp/SHA256SUMS")
	[ -n "$expected" ] || die "the release has no checksum for $1"
	[ "$(sha256 "$tmp/$1")" = "$expected" ] || die "$1 is damaged (checksum mismatch); nothing was changed"
}

# Moves $1 into place at $2, keeping the old copy until the new one is in.
swap() {
	rm -rf "$2.old"
	if [ -e "$2" ]; then mv "$2" "$2.old"; fi
	if ! mv "$1" "$2"; then
		if [ -e "$2.old" ]; then mv "$2.old" "$2"; fi
		die "could not write $2"
	fi
	rm -rf "$2.old"
}

on_path() {
	case ":$PATH:" in *":$1:"*) return 0 ;; esac
	return 1
}

os=$(uname -s)
case $os in
Linux | Darwin) ;;
*) die "this installer is for Linux and macOS; other downloads are at https://github.com/$REPO/releases" ;;
esac

# ---------------------------------------------------------------- Linux

linux_dest() {
	data=${XDG_DATA_HOME:-$HOME/.local/share}
	dest="$data/pdfr/app"
	bin="$dest/pdfr"
}

linux_install() {
	case $(uname -m) in
	x86_64 | amd64) arch=x86_64 ;;
	aarch64 | arm64) arch=aarch64 ;;
	*) die "there is no pdfr for $(uname -m) yet" ;;
	esac
	if ldd --version 2>&1 | grep -qi musl; then
		die "pdfr needs glibc 2.31 or newer; musl systems (Alpine, Void musl) aren't supported yet"
	fi
	linux_dest
	have=$(installed "$bin")
	if [ "$have" = "$want" ]; then
		say "pdfr $want is installed and up to date."
		"$bin" --register >/dev/null 2>&1 || true
		return
	fi
	name="pdfr-v$want-$arch-linux"
	if [ -n "$have" ]; then say "Updating pdfr $have to $want..."; else say "Installing pdfr $want..."; fi
	download "$name.tar.gz"
	tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
	[ -x "$tmp/$name/pdfr" ] || die "the archive has no pdfr in it"
	mkdir -p "$data/pdfr"
	swap "$tmp/$name" "$dest"
	"$bin" --register || die "pdfr is in $dest, but could not add itself to the app menu"
	if [ -x /usr/bin/pdfr ]; then
		say "Note: your package manager also installed a pdfr (/usr/bin/pdfr). The menu entry and ~/.local/bin/pdfr use this one."
	fi
	say "Done: pdfr $want. Find it in your app menu, or run: pdfr"
}

linux_uninstall() {
	linux_dest
	[ -x "$bin" ] || die "pdfr isn't installed in $dest"
	"$bin" --unregister >/dev/null 2>&1 || true
	rm -rf "$data/pdfr/app"
	say "Removed pdfr. Your saved details are kept; delete ~/.config/pdfr to remove them too."
}

# ---------------------------------------------------------------- macOS

mac_dest() {
	apps=""
	for d in /Applications "$HOME/Applications"; do
		if [ -d "$d/pdfr.app" ]; then apps=$d; break; fi
	done
	if [ -z "$apps" ]; then
		if [ -w /Applications ]; then apps=/Applications; else apps="$HOME/Applications"; fi
	fi
	app="$apps/pdfr.app"
	exe="$app/Contents/MacOS/pdfr"
}

mac_link() {
	if [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
		bindir=/usr/local/bin
	else
		bindir="$HOME/.local/bin"
		mkdir -p "$bindir"
	fi
	if [ -L "$bindir/pdfr" ] || [ ! -e "$bindir/pdfr" ]; then
		ln -sf "$exe" "$bindir/pdfr"
	fi
	if ! on_path "$bindir"; then
		say "To type pdfr in a terminal, add $bindir to your PATH."
	fi
}

mac_install() {
	mac_dest
	have=$(installed "$exe")
	if [ "$have" = "$want" ]; then
		say "pdfr $want is installed and up to date."
		mac_link
		return
	fi
	if [ -n "$have" ]; then say "Updating pdfr $have to $want..."; else say "Installing pdfr $want..."; fi
	download "pdfr-v$want-macos.zip"
	mkdir -p "$tmp/app" "$apps"
	ditto -x -k "$tmp/pdfr-v$want-macos.zip" "$tmp/app"
	[ -x "$tmp/app/pdfr.app/Contents/MacOS/pdfr" ] || die "the download has no pdfr.app in it"
	swap "$tmp/app/pdfr.app" "$app"
	# Tell Launch Services about it now, so "Open With" lists it straight away.
	lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
	if [ -x "$lsregister" ]; then "$lsregister" -f "$app" >/dev/null 2>&1 || true; fi
	mac_link
	say "Done: pdfr $want is in $apps. Open it from Launchpad or Spotlight, or run: pdfr"
}

mac_uninstall() {
	mac_dest
	[ -d "$app" ] || die "pdfr isn't installed in /Applications or ~/Applications"
	rm -rf "$app"
	for l in /usr/local/bin/pdfr "$HOME/.local/bin/pdfr"; do
		if [ -L "$l" ] && [ "$(readlink "$l")" = "$exe" ]; then rm -f "$l"; fi
	done
	say "Removed pdfr. Your saved details are kept in ~/Library/Application Support/pdfr."
}

# ---------------------------------------------------------------- main

if [ "$action" = uninstall ]; then
	if [ "$os" = Darwin ]; then mac_uninstall; else linux_uninstall; fi
	exit 0
fi

if [ -z "$want" ]; then
	case $VERSION in
	@*) want=$(latest) ;;
	*) want=$VERSION ;;
	esac
fi
want=${want#v}
[ -n "$want" ] || die "could not find a pdfr release (no stable release yet, or GitHub is unreachable). See https://github.com/$REPO/releases"
base="https://github.com/$REPO/releases/download/v$want"

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t pdfr)
trap 'rm -rf "$tmp"' EXIT
trap 'exit 1' INT TERM

if [ "$os" = Darwin ]; then mac_install; else linux_install; fi
