#!/bin/bash
# Adds new .deb and .rpm files to the apt and rpm repositories served from
# GitHub Pages (repo kjellberg/packages), then rebuilds and signs their
# indexes. Run by the release workflow; needs apt-utils, createrepo-c, rpm
# and gnupg, with the signing key imported and its id in $GPG_KEY_ID.
#
#   tools/publish_repos.sh <packages-repo-checkout> <dir-with-new-packages>
set -euo pipefail
site=$(realpath "$1")
new=$(realpath "$2")
: "${GPG_KEY_ID:?set GPG_KEY_ID}"
base=https://kjellberg.github.io/packages

# --- apt --------------------------------------------------------------------
deb="$site/deb"
mkdir -p "$deb/pool/main/p/pdfr"
cp -n "$new"/*.deb "$deb/pool/main/p/pdfr/" 2>/dev/null || true
cd "$deb"
for arch in amd64 arm64; do
  d="dists/stable/main/binary-$arch"
  mkdir -p "$d"
  apt-ftparchive --arch "$arch" packages pool > "$d/Packages"
  gzip -9kf "$d/Packages"
done
apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=pdfr \
  -o APT::FTPArchive::Release::Label=pdfr \
  -o APT::FTPArchive::Release::Suite=stable \
  -o APT::FTPArchive::Release::Codename=stable \
  -o APT::FTPArchive::Release::Architectures="amd64 arm64" \
  -o APT::FTPArchive::Release::Components=main \
  release dists/stable > dists/stable/Release
gpg --batch --yes -u "$GPG_KEY_ID" --clearsign -o dists/stable/InRelease dists/stable/Release
gpg --batch --yes -u "$GPG_KEY_ID" -abs -o dists/stable/Release.gpg dists/stable/Release

# --- rpm --------------------------------------------------------------------
rpm_dir="$site/rpm"
for f in "$new"/*.rpm; do
  [ -e "$f" ] || continue
  arch=$(rpm -qp --qf '%{ARCH}' "$f" 2>/dev/null)
  mkdir -p "$rpm_dir/$arch"
  cp -n "$f" "$rpm_dir/$arch/"
  rpmsign --define "_gpg_name $GPG_KEY_ID" --addsign "$rpm_dir/$arch/$(basename "$f")"
done
for a in "$rpm_dir"/*/; do
  createrepo_c --update "$a"
  gpg --batch --yes -u "$GPG_KEY_ID" --detach-sign --armor "$a/repodata/repomd.xml"
done
cat > "$rpm_dir/pdfr.repo" <<REPO
[pdfr]
name=pdfr
baseurl=$base/rpm/\$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$base/pdfr.asc
REPO

# --- keys and a landing page ---------------------------------------------------
gpg --export "$GPG_KEY_ID" > "$site/pdfr.gpg"
gpg --export --armor "$GPG_KEY_ID" > "$site/pdfr.asc"
touch "$site/.nojekyll"
cat > "$site/index.html" <<HTML
<!doctype html><meta charset="utf-8"><title>pdfr packages</title>
<p>Package repositories for <a href="https://github.com/kjellberg/pdfr">pdfr</a>.
See the README for how to add them.</p>
HTML
