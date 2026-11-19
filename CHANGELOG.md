# Changelog

What changed in each version of pdfr. The release workflow copies a
version's section into its GitHub release notes.

## [Unreleased]

### Added

- Sign forms without a printer: press Ctrl+I (or the pen in the toolbar),
  draw your signature with the mouse or trackpad or type your name in
  handwriting, and click where it goes. On a "Signature" line or the PDF's
  own "sign here" box it fits itself to the spot. Drag a signature to move
  it, drag its corner to resize it, and press Delete to remove it.
- Signatures can be saved for next time, one or more per person, on this
  computer only (Settings → People lists and deletes them).
- Choose how signatures are saved (Settings → General, or right-click one):
  as a signature other PDF apps can still move, or locked into the page.
  These are drawn signatures, not cryptographic ones.
- Search the document's text with Ctrl+F. Matches are listed with the words
  around them and highlighted on the pages; browsing the list shows each
  one. Case and accents don't matter ("malmo" finds "Malmö"), and phrases
  are found across line breaks and hyphenated words. Boxes whose label
  matches are listed first. Next and previous match are in the command
  palette, and Ctrl+F remembers the last search.

- Select and copy the page's own text: drag over it, double-click a word or
  triple-click a line, then press Ctrl+C or right-click → Copy. The
  right-click menu can also search for the selected words.
- Links work: web links open in your browser, links within the document
  jump to their page (Ctrl+Shift+G goes back), and web and e-mail addresses
  printed as plain text can be clicked too. Hovering shows where a link
  goes. Only web and e-mail links are opened.
- A page sidebar (Ctrl+B, or the button at the left of the toolbar):
  thumbnails of every page, marked where boxes are filled in, and the
  document's outline (bookmarks) when it has one. Ctrl+G also finds the
  outline's sections by name.

### Changed

- Finding a box by its label (without the text) moved to Ctrl+Shift+F.

### Fixed

- macOS: “Confirm in the dialog macOS shows” no longer stays in Settings
  after you answer. pdfr sees the new default app within a second.

## [0.2.0] - 2026-11-19

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
