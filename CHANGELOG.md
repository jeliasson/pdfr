# Changelog

What changed in each version of pdfr. The release workflow copies a
version's section into its GitHub release notes.

## [Unreleased]

### Changed

- A new app icon: a form page with a folded corner and a field being filled
  in, centred on the coral tile with room around it, like other apps' icons.
- A new toolbar: each group of buttons sits in a rounded capsule, and the
  icons are thinner and rounder, in the style of macOS.
- The empty window shows pdfr's icon and a large coral "Open a PDF…" button
  on a drop zone that lights up while you drag a file over it.

### Added

- **Sign with a certificate** (Ctrl+Shift+C). Besides the drawn signature,
  pdfr now makes real digital signatures (PAdES, as eIDAS asks) with the
  certificates already on your computer: those installed in Chrome,
  Chromium or Firefox (such as the Spanish FNMT certificate), ID cards and
  USB tokens, or a .p12 / .pfx file. Choose how it looks (your signature
  with who signed and when, only the details, or invisible), click where
  it goes, and save the signed copy. A trusted timestamp is added by
  default; only a fingerprint of the signature is sent for it, never the
  document. The private key never leaves the browser's store or the card,
  and pdfr never stores a PIN or password.
- **Checks signatures.** Signed PDFs show their signatures in the sidebar:
  who signed, when, whether the document is as it was signed, and whether
  it was changed afterwards (filling in a form or adding a comment is
  allowed; changing the pages is not).
- **Signed files stay signed.** Changes to a signed PDF are added after
  its signatures instead of rewriting the file, so they stay valid.
- **Signature pictures.** The Sign dialog has an Image tab: choose a photo
  or scan of your signature, and pdfr removes the paper around it.
- **Grey panels are fields.** Forms made in Word often mark each answer
  with a light grey panel and a caption under it, without any outline.
  pdfr now finds those panels and lets you type in them.
- **Type over printed answers.** When another app filled in a form and
  flattened the answers into the page, a panel's printed answer becomes
  its field's starting value. Change it and the panel's colour hides the
  old print, on screen and in the saved file; leave it and nothing
  changes. Anywhere else, right-click printed text → **Edit this text**.

### Fixed

- A row of separate small boxes, each with its own label ("Portal",
  "Escalera"), is no longer taken for one box per letter of a single field.
- The line above a page's footer is no longer a field.
- Scanned forms: no more boxes on printed text. On blurred scans the tops
  of a row of letters looked like a line to write on, and a table cell
  holding a label could look like an empty box beside the label.

## [0.3.0] - 2026-11-20

Sign, search and read: pdfr now does what people expect from any PDF
reader, so a form can be filled in, signed and sent without a printer.

### Added

**Signing**

- **Sign without a printer.** Press Ctrl+I (or the pen in the toolbar),
  draw your signature with the mouse or trackpad or type your name in
  handwriting, and click where it goes. On a "Signature" line or the PDF's
  own "sign here" box it fits itself to the spot. Drag a signature to move
  it, drag its corner to resize it, and press Delete to remove it.
- **Saved for next time**, one or more per person, on this computer only.
  Settings → People lists and deletes them.
- **Your choice of how it is saved** (Settings → General, or right-click a
  signature): as a signature other PDF apps can still move, or locked into
  the page. These are drawn signatures, not cryptographic ones.
- **Initials** (Ctrl+Shift+I), drawn or typed like a signature.
  Right-click a signature or initials → "Put on every page", for contracts
  that ask for initials on each page.
- **Today's date** (Ctrl+Shift+D) fills the box you're in, in the format
  its label asks for, or goes where you click next.

**Finding and copying**

- **Search the text** with Ctrl+F. Matches are listed with the words around
  them and highlighted on the pages, and browsing the list shows each one.
  Case and accents don't matter ("malmo" finds "Malmö"), and phrases are
  found across line breaks and hyphenated words. Boxes whose label matches
  are listed first. Next and previous match are in the command palette, and
  Ctrl+F remembers the last search.
- **Select and copy** the page's own text: drag over it, double-click a
  word or triple-click a line, then Ctrl+C or right-click → Copy. The
  right-click menu can also search for the selected words.
- **Links work.** Web links open in your browser, links within the
  document jump to their page (Ctrl+Shift+G goes back), and web and e-mail
  addresses printed as plain text can be clicked too. Hovering shows where
  a link goes. Only web and e-mail links are opened.

**Reading**

- **Page sidebar** (Ctrl+B, or the button at the left of the toolbar):
  thumbnails of every page, marked where boxes are filled in, and the
  document's outline (bookmarks) when it has one. Go to page (Ctrl+G) also
  finds the outline's sections by name.
- **Recent files** (Ctrl+E), each reopened on the page and at the zoom
  where you left it. With no document open, pdfr shows them as cards with
  a picture of their first page; right-click a card to open it in a new
  window or take it off the list. Turn it off or clear the list in
  Settings → General.
- **Show fields** (Ctrl+H, or the toolbar button): turn off the tint on
  the boxes you can fill in to see the form as other PDF apps will show
  it. Typed text, ticks and signatures stay.
- **Turn sideways or upside-down pages.** Ctrl+R turns the current page a
  quarter turn clockwise, Ctrl+Shift+R every page (anticlockwise is in the
  command palette). The page is saved turned too, and pdfr finds the boxes
  again on the upright page. Undo turns it back. Pages with the PDF's own
  form fields can't be turned.
- **Fit page** (Ctrl+3) shows the whole page in the window.
- **Dark pages** (Ctrl+M, or Settings → General): pages light on dark, with
  colours kept recognisable. Screen only: saving and printing are
  unchanged.
- **Open in a new window** (Ctrl+Shift+O), say the instructions next to
  the form. In Recent files, Ctrl+Enter opens one in a new window. Windows
  share what pdfr remembers safely: each keeps what the other saved.
- **Two pages side by side**, like an open book, and **presentation mode**
  (one page at a time, full screen; arrow keys or a click turn pages, Esc
  ends). Both are in the command palette.
- **Password-protected PDFs open.** pdfr asks for the password and never
  keeps it. The saved copy is not password-protected, and pdfr says so
  when saving.

### Changed

- A new app icon: a white text cursor on the coral tile, without the
  "pdfr" wordmark, so it sits with the other icons in the Dock and app
  menus.
- Ctrl+F searches the document's text (and boxes by label). Finding a box
  by its label alone moved to Ctrl+Shift+F.

### Fixed

- macOS: the Dock icon was larger than other apps' icons while pdfr ran.
- Settings → People: "Show details" on a remembered person crashed pdfr.
- A password-protected PDF said "not a readable PDF file".
- Automated runs (`--save`, `--screenshot`) stop with an error when a file
  can't be opened, instead of waiting.
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
