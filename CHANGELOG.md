# Changelog

What changed in each version of pdfr. The release workflow copies a
version's section into its GitHub release notes.

## [Unreleased]

### Security

- **Redaction removes every copy of what you black out.** Annotations,
  replies, alternative text, bookmarks, attachments and the document's
  details (title, author, metadata) no longer keep a hidden copy of the
  redacted text.
- **Extracted and deleted pages really leave the file.** Before, a link or
  bookmark pointing at a removed page could keep it hidden inside the new
  file.
- **Signature checks are much stricter.** These no longer show as valid:
  page edits hidden under the signature's validation data, rewritten
  signature details, swapped fonts, certificates that aren't real
  certificate authorities or aren't meant for signing, self-made timestamps
  and SHA-1. Certification and field-lock rules are enforced.
- **"Valid, revocation could not be checked"** now shows in amber instead
  of a green check. pdfr no longer asks untrusted or local addresses about
  revocation, and an ID card's PIN is dropped right after logging in.
- **Links in a PDF ask before opening a web address.** Ctrl+click opens at
  once, or allow a site with "Always open links to …".
- **Saving keeps the file's permissions and symlinks.** A private (0600)
  form stays private, and Extract pages can no longer overwrite the open
  file.
- **AI agents are kept out of Settings and your questions.** They can no
  longer change settings, see remembered details there, answer the AI
  question or throw away unsaved changes without asking you, and they only
  connect to your own pdfr. AI permission covers only the tool you allowed,
  and two pdfr windows no longer undo each other's settings.
- **Installing pdfr into an agent keeps that agent's settings file intact:**
  its permissions, key order and comments, and a guide you changed
  yourself.
- **Updates are signed.** pdfr and the install script check a signature
  made by pdfr's own release build before installing anything, and refuse
  unsigned or altered downloads. Updates use a more mature encryption
  library for HTTPS.

### Fixed

- **Damaged or hostile PDFs no longer crash or freeze pdfr.** That covers
  huge pages, compressed bombs, looping trees and odd text layouts that
  could crash search and text selection.
- **Opening a file no longer discards changes typed while it loaded.**
- **Selected boxes, the sidebar selection after undo and another
  document's redaction marks are no longer mixed up.** Dark pages no longer
  interrupt finding fields, and huge pages no longer use excessive memory.
- **Form fields keep odd checkbox values exactly**, and impossible font
  sizes are ignored.
- **Super+W (Cmd+W on a Mac) closes Settings** when it is open, instead of
  closing pdfr.

## [0.6.1] - 2026-11-22

Your AI agent can fill in forms with pdfr, and the AI helper works with
more tools, including models on your own computer. Select boxes to remove
or move several at once, certificate signatures look formal, and scanned
and printed text is read more cleanly. (0.6.0 was tagged but never
published: its macOS build failed.)

### Added

- **Your AI agent can use pdfr.** Connect Claude Code, Claude Desktop,
  Cursor or another agent with `claude mcp add pdfr -- pdfr mcp` (Settings →
  AI agents has the line to copy), then ask it to fill in a form. The agent
  opens pdfr, reads the form's boxes, fills them in and saves, in a window
  you can watch. It can also look at the window and use the pointer and
  keyboard the way you do.
  - **Install** in Settings → AI agents adds pdfr to the agents on your
    computer for you (Claude Code, Codex and ChatGPT Desktop, Cursor,
    OpenCode, Antigravity, GitHub Copilot, Grok, Hermes, Pi, Kilo and a dozen
    more), with a short guide for the agent on using pdfr.
  - Click or type yourself and the agent pauses until you press Resume.
  - Signing digitally and saving over a file that is already there ask you
    first. Printing and using remembered details are allowed. Each of these
    can be changed in Settings → AI agents, and agents can be turned off
    there altogether.
  - It runs on your computer: pdfr listens only on a socket that only you
    can reach, and nothing is sent anywhere by pdfr.
- **The AI helper works with more than Claude Code.** Settings → AI now has
  a choice of tool: Claude Code, Pi, MiniMax Code, Kimi Code, or a model
  running on your own computer with Ollama or LM Studio (nothing leaves the
  computer then). pdfr only uses a tool it can run as a plain model, with no
  access to your files; agents that cannot be run that way (Codex, Cursor,
  Grok and others) are not offered here.
- **Select boxes to remove or move them.** The new Select tool (toolbar,
  Ctrl+Shift+B) picks the boxes you or pdfr added: click one, Shift-click
  more, drag a frame around several, or Ctrl+A for the whole page. Delete
  removes them all at once; drag them or use the arrow keys (Shift: 10
  points) to move them. Ctrl+click selects boxes without the tool too. Each
  change is one Ctrl+Z.

### Changed

- **Certificate signatures look formal**, like Adobe Acrobat's: your name in
  plain type beside "Digitally signed by", the date and the reason, instead
  of handwriting. A drawn signature is still an option under "Looks like".
- **The text reader downloads from pdfr's own release** on GitHub instead
  of a third-party server (the same files, checked the same way).
- **The Sign dialog stays put** near the top of the window when you switch
  tabs, instead of jumping as its size changes.
- **Comments carry your name.** New comments, highlights and text boxes are
  signed with the name of the person pdfr has filled in most often (you),
  instead of the computer's login name.

### Fixed

- **Editing printed text on a shaded page** covers the old text in the
  page's own colour, not white.
- **Scanned forms: letters are no checkboxes.** A letter whose curve closes
  into a box (the C of "CORRECT") is no longer offered as a checkbox.
- **No stray spaces around wide letters.** Printed text with a wide letter
  (W, M, @) was read with a space on either side of it, so editing
  "ANNA@WEBMAIL.COM" showed "ANNA@ W EBMAIL.COM". pdfr now uses each letter's
  real width.

## [0.5.0] - 2026-11-21

More than forms: pdfr reads scanned text on your computer, checks your
answers before you save, adds comments and highlights, organises and
merges pages, and redacts for real. Signatures get revocation checks and
proof that lasts for years, ID cards work better, and pdfr updates itself.

### Added

- **macOS: sign with the certificates in your Keychain.** Certificates you
  added to the Keychain (and ID cards macOS offers) are listed in the
  signing dialog. The key stays in the Keychain, and macOS asks you before
  pdfr may use it.
- **Update now.** When a new version is out, pdfr updates itself instead of
  handing you a command to copy, then offers to restart into it. Installed
  from the website or a download, it fetches the release and checks it
  against the release's checksums. With Homebrew, apt, dnf or zypper, it
  runs the package manager (your system asks for your password). From the
  AUR, it opens a terminal with yay or paru.
- **Check the form** (Ctrl+Shift+E): lists answers that look wrong (an
  email without @, an IBAN or Spanish DNI/NIE or Swedish personnummer whose
  check digit doesn't match, a birth date in the future, a phone number with
  letters) and the boxes still empty, each with a link to the box. Saving
  asks first when an answer looks wrong (Settings → General → Check before
  saving). All checks run on your computer.
- **Explain this form.** With AI on, the check can explain in plain words
  what the form is for and what each part asks. Only headings and box
  labels are sent, never your answers.
- **Scanned forms can be searched and get suggestions.** pdfr reads the
  text of scanned pages on your computer, so search, copy and suggestions
  work on scans as on digital forms, and boxes that are really printed
  text are no longer offered for filling. The first scan needs a one-time
  13 MB download of the text reader; after that it works offline, and
  nothing about your files is sent. Turn it off in Settings → General →
  Read scanned text.
- **Comments and markup.** Add notes (Ctrl+Shift+A), highlight selected
  text (Ctrl+Shift+H), underline or strike it through, type a text box
  anywhere (Ctrl+Shift+T), and draw rectangles, ellipses and arrows in six
  colours from the new Markup button. Select one to move, resize, recolour
  or delete it. They are saved as standard PDF comments, so other PDF apps
  show them too, and comments already in a file can be edited or removed.
  A Comments tab in the sidebar lists them all. Adding comments to a
  digitally signed file keeps its signature valid.
- **Organise pages.** Drag thumbnails in the sidebar to move pages; Ctrl
  or Shift+click picks several. Right-click a page to turn, delete or
  extract it, add a blank page, or insert pages from a file: another PDF
  (its fields come along) or a JPEG/PNG picture. Ctrl+Shift+M inserts pages
  after the current one. Every change can be undone. A digitally signed
  file asks first, because changing its pages removes the signatures.
- **Redact** (Ctrl+Shift+X): drag boxes, mark the selected text, or mark
  every place a word or number appears. Applying removes the text,
  pictures and form fields under the boxes for good, not just covers them,
  and saves a new copy ("name (redacted).pdf"); the original stays as it
  is.
- **Revoked certificates are caught.** Checking a signed file now also
  checks whether the signer's certificate was revoked, from the proof
  inside the file or by asking the certificate's issuer (Settings →
  Signing with a certificate; on by default). A signature made after its
  certificate was revoked is not valid; one timestamped before stays
  valid. The signature card says how it was checked.
- **Signatures that last.** Files you sign with a timestamp now carry the
  proof needed to check them years from now, even offline and after your
  certificate expires (PAdES B-LT, sealed with a document timestamp as
  B-LTA). If that proof can't be fetched, the signature is still made and
  the status line says so.
- **Says why your ID card doesn't show up.** When no card or token is
  found, the signing dialog names the first thing to fix: smartcard support
  not installed, the smartcard service (pcscd) not running, a reader with no
  card pdfr can read, or no reader at all. A link leads to the step-by-step
  guide on pdfr.app.
- **PIN tries left, before you type.** When a card reports that a wrong
  PIN was entered before, the dialog says a few tries are left, or that the
  next wrong PIN locks the card. A locked card says how to unlock it.
- **XFA forms are explained.** Forms made only for Adobe Reader (XFA),
  which no other PDF app can show, now say so when opened instead of
  showing a "Please wait…" page.

### Changed

- **Signing with a certificate is a tab of the signing dialog.** Draw,
  Type, Image and Certificate sit side by side, so you can find it without
  knowing Ctrl+Shift+C.
- **The Signatures sidebar matches the rest of pdfr:** one card per
  signature, with who signed and the verdict on top and the details below.
  The sidebar's tabs are a segmented control.
- **The sidebar fits what it shows:** narrow for page thumbnails, wider
  for the outline and the signatures. Each tab keeps its own width if you
  drag the edge.

### Fixed

- **Signing with national ID cards** whose signing key asks for the PIN
  again at the moment of signing (as on the Spanish DNIe, Belgian eID and
  Estonian ID card) works; it failed with "user not logged in". Not yet
  tried with each of these cards: please report how yours does.
- **A wrong PIN brings the signing dialog back**, with the same
  certificate selected and how many tries are left, instead of closing it.
  pdfr never tries a PIN again by itself.
- **The signing certificate comes first.** ID cards hold one certificate
  to log in to websites and one to sign; the signing one is now listed
  first and chosen by default, and the other is marked "for logging in".
- **No blank pages while scrolling long files.** One background thread is
  kept for drawing pages, so a page scrolled into view no longer waits
  for box detection to finish on every thread.

## [0.4.0] - 2026-11-20

Real signatures: sign with the certificate already on your computer (the
FNMT certificate in your browser, an ID card, or a .p12 file), check the
signatures of PDFs you receive, and sign with a picture of your signature.

### Changed

- The "save changes?" question when closing or opening another file looks
  like pdfr's other dialogs, with Save, Cancel and Don't save.
- macOS: Cmd+W minimises the window (quit with Cmd+Q).
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
