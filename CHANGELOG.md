# Changelog

What changed in each version of pdfr. The release workflow copies a
version's section into its GitHub release notes.

## [Unreleased]

### Added

- Register your copy: get a free lifetime Personal key at pdfr.app/get, or a
  Business key for using pdfr at work, and paste it in Settings → License
  (or Register pdfr in the command palette). Keys are checked offline. An
  unregistered copy now and then asks to be registered, after a week's
  grace; pdfr keeps working fully either way.
- A registered copy checks its key once a day when online, sending only the
  licence id and a random install id, so a key stays on its three computers
  and a refunded key stops working. Without a connection nothing changes.
- Keyboard-first: everything works without a mouse. Ctrl+N jumps to the next
  empty box, Ctrl+L puts letters on every box in view to jump to, Ctrl+F
  finds a box by its label, and Ctrl+Shift+P (or Ctrl+Space) opens a command
  palette. F1 shows every shortcut.
- vim/zathura-style Ctrl chords: Ctrl+J/K scroll (or move through an open
  list), Ctrl+D/U half a page, Ctrl+Shift+J/K next and previous page,
  Ctrl+G go to page, Ctrl+2 fit width, Ctrl+, settings.
- Drop-downs from the keyboard: Space or Alt+↓ opens, ↑/↓ or a letter
  chooses, Enter picks.
- Ctrl+Backspace deletes a word; Ctrl+Shift+Delete removes a box pdfr added.
- Esc puts down the add-box tool, cancels "unsaved changes" (Enter or Ctrl+S
  saves) and answers "Not now" or "Don't remember" in pdfr's questions.

### Fixed

- Boxes drawn by hand (wobbly, slanted, with corners that overshoot) are
  found as fields. Before, only straight lines were, so most of a
  hand-drawn form was missed. The loop of a handwritten letter is no
  longer mistaken for a tiny box.
- Buttons in Settings and pdfr's questions no longer shrink a little when
  the pointer is over them.

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
