# Changelog

What changed in each version of pdfr. The release workflow copies a
version's section into its GitHub release notes.

## [Unreleased]

## [0.1.0] - 2026-11-17

### Added

- First public release.
- Fill in fillable PDF forms, forms that are only drawn, and scanned forms.
- One letter per square in rows of boxes, flowing across date groups.
- Private autocomplete that keeps each person's details separate, with an
  optional AI helper (Claude Code) that sees field labels only.
- Printing with the system's print dialog.
- App icon, app menu entry, "Open with" and a default-app setting.
- One install line for Linux and macOS
  (`curl -fsSL https://pdfr.app/install.sh | sh`) that also updates.
- macOS app (Apple silicon and Intel). Not notarized yet.
- Downloads: a disk image for macOS, a tarball for any Linux, and `.deb` and
  `.rpm` packages.
- An opt-in check for new versions, asked once. The notice shows the line
  that updates pdfr, with a button to copy it.
