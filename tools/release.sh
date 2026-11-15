#!/bin/bash
# Prepares a release: sets the version, dates the changelog, adds the
# release to the AppStream file and the PKGBUILDs. You then review, commit
# and tag (signed), and CI builds and publishes everything.
#
#   tools/release.sh 0.2.0
set -euo pipefail
v=${1:?usage: tools/release.sh <version>}
[[ $v =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "not a version: $v" >&2; exit 1; }
cd "$(dirname "$0")/.."
today=$(date +%Y-%m-%d)

sed -i "0,/^version = \".*\"/s//version = \"$v\"/" Cargo.toml
cargo update --workspace --offline -q 2>/dev/null || cargo update --workspace -q

if [[ $v != *-* ]]; then
  grep -q "^## \[Unreleased\]" CHANGELOG.md || { echo "CHANGELOG.md has no [Unreleased] section" >&2; exit 1; }
  sed -i "s/^## \[Unreleased\]/## [Unreleased]\n\n## [$v] - $today/" CHANGELOG.md
  meta=packaging/linux/io.github.kjellberg.pdfr.metainfo.xml
  grep -q "version=\"$v\"" $meta || sed -i "s|  <releases>|  <releases>\n    <release version=\"$v\" date=\"$today\"/>|" $meta
  for p in packaging/aur/PKGBUILD packaging/aur/pdfr-bin/PKGBUILD; do
    sed -i "s/^pkgver=.*/pkgver=$v/; s/^pkgrel=.*/pkgrel=1/" $p
  done
fi

cat <<MSG
Version set to $v. Review the changes, then:

  git commit -am "pdfr $v"
  git tag -s v$v -m "pdfr $v"
  git push && git push origin v$v

CI then builds, tests and publishes the release (see docs/RELEASING.md).
MSG
