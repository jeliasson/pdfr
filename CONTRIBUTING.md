# Contributing to pdfr

Thanks for helping! Bug reports, blank sample forms that pdfr gets wrong, and
pull requests are all welcome.

## Contribution terms

pdfr is source-available under the [PolyForm Strict License 1.0.0](LICENSE.md),
which on its own does not allow making changes. As the licensor, I add this
permission: **you may make changes to pdfr for the purpose of contributing them
back to this project** (in a fork or branch used for a pull request).

By submitting a contribution (code, documentation, sample files or anything
else) you confirm that you have the right to submit it, and you grant the
licensor a perpetual, worldwide, royalty-free, irrevocable licence to use,
copy, modify, distribute, sublicense and relicense it as part of pdfr or
otherwise, including under paid commercial licences. You keep the copyright in
your contribution.

Please don't attach PDFs that contain anyone's personal details. Blank forms
only.

## Building

Requires a stable Rust toolchain (1.92 or newer). No C or C++ libraries and no
downloads besides crates from crates.io.

```sh
cargo build --release      # target/release/pdfr
cargo test --release       # unit, end-to-end and headless UI tests
cargo clippy --workspace --all-targets
```

On Linux, pdfr loads OpenGL/EGL and xkbcommon at run time (present on any
desktop) and uses the XDG desktop portals for file dialogs, printing, links
and the light/dark preference. Without a file chooser portal (bare i3, Sway,
dwm) it shows its own file dialog.

Release packages are built only by CI (see [docs/RELEASING.md](docs/RELEASING.md)).

The timing-based scrolling test (`steady_scroll_through_300_pages`) is
reliable in release builds; in debug builds it can fail on a busy machine.

> The unrelated crate [`pdfrs`](https://crates.io/crates/pdfrs) is one letter
> away. pdfr is not published on crates.io (`publish = false`).

## Project rules

These come from the original brief and are not negotiable without discussion:

- Pure Rust: no C or C++ libraries, no downloads at build or run time other
  than crates from crates.io. Commit `Cargo.lock`.
- The only external program pdfr may run is the user's own AI command-line
  tool, and it is optional. Desktop integration uses freedesktop files and
  the XDG portals over D-Bus, never `xdg-open`, `xdg-mime` or similar.
- The one network request besides the AI tool is the opt-in update check
  (asked once; not in Flatpak or development builds): it asks GitHub for the
  latest version number once a day, sends nothing else, and installs
  nothing. Package installs are shown their update command.
- AI requests send field labels and section headings (with row numbers) only:
  never field values, other page text, images or the PDF. No tools, no
  conversation, at most one retry; replies that don't parse are discarded.
- Never store passwords, card numbers, one-off dates, or anything the user marks
  "don't remember". The memory file is readable only by the user.
- Never overwrite the user's original file silently.

## Workspace

See [ARCHITECTURE.md](ARCHITECTURE.md) for how the app is put together and
where new features go.

- `crates/core`: geometry, the built-in font, the field model, the shared text
  layout (screen and saved file use the same one), the editor, reading real
  form fields, detecting fields on flat and scanned pages, saving, text
  extraction and the rendering wrapper. No UI.
- `crates/assist`: labels, the label dictionary, the AI backend, the value
  memory, profiles and suggestions. No UI.
- `crates/app`: the window, scrolling, zoom, input, background workers,
  theming, printing and the autocomplete interface.

## How it works

### Forms

- **Real form fields** (AcroForm): text, multi-line, password, comb (one
  character per box), checkboxes, radio groups and drop-downs. Read-only and
  hidden flags are honoured.
- **Flat and scanned forms**: fillable spots are detected from the page image:
  boxes, rows of character cells, separate date squares, checkboxes, write-on
  lines, dotted lines, underscore lines, tick-mark rulers and boxes with a
  label at the top. Headings, body text and filled bars are left alone.
- **Exact placement**: one layout routine positions text on screen and in the
  saved file (verified to within about one pixel with
  `tools/compare_screen.py`).
- **Font**: values are always set in the bundled Inter Regular, drawn on screen without hinting and with slightly heavier strokes (crisper small text).
- **Saving** writes real form fields with appearance streams and the embedded
  font (searchable, copyable text). Detected fields become real form fields
  other viewers can edit. The first save suggests `<name>-filled.pdf`.
- **Printing** always uses a flattened copy (`pdfr_core::flatten`): values
  are drawn into the page content, because many printers that read PDFs
  themselves skip form fields. Each system's own dialog is used:
  - Linux: the XDG print portal over D-Bus (needs `xdg-desktop-portal` with a
    print backend such as `xdg-desktop-portal-gtk`); the document's paper
    size and orientation are passed as the starting point. Without a print
    portal, pdfr shows its own small dialog and sends the PDF straight to
    the local CUPS server over IPP (`app/cups.rs`, standard library only).
  - Windows: the standard print dialog (`PrintDlgEx`); pages are rendered by
    pdfr at the printer's resolution (up to 300 dpi) and sent with GDI, at
    actual size, centred.
  - macOS: PDFKit's print operation, as a sheet on the window, with paper,
    orientation, scaling, copies, pages and preview shown.

  The Windows and macOS code is compile-checked here
  (`cargo check --target x86_64-pc-windows-gnu`, `--target
  aarch64-apple-darwin`); test on those systems before a release.
- If a file's structure cannot be parsed for editing, pdfr shows it read-only
  and says so in the status line.

### Autocomplete

Three layers; each is useful alone, and the AI is only asked about what the
cheaper layers cannot answer.

1. **Field labels (no AI).** Every field gets a label: the tooltip or a
   readable field name for real fields, otherwise the nearest text to the
   left, above, or at the top of the same box. Forms written as one sentence
   ("D./Dª ____ con D.N.I. nº ____") are handled too. Date parts and split
   IBANs share one label as a group.
2. **Meanings.** Labels map to keys such as `full_name`, `email`,
   `date_of_birth` or `iban`: first a built-in dictionary (English, Spanish,
   Swedish, plus common German and French), then a permanent local cache, and
   only then the AI.
3. **Value memory and profiles (no AI).** On save, pdfr shows what it would
   keep, grouped by person (or always / never, per Settings). Each person is a
   profile, matched across forms by ID number, name or e-mail. Forms about
   several people are split into people; one person's details are never
   offered in another person's fields. The same form opened again offers its
   previous values field by field, which needs no labels and so works on scans.
   Values are derived by fixed rules (full name ↔ given + family name, dates
   reformatted or split into boxes, IBANs and IDs spread across boxes).

### AI

If Claude Code (`claude`) is installed, it is called in the background with its
cheapest model (`haiku`, changeable in Settings) and all its tools turned off,
for unknown labels and for grouping fields by person on multi-person forms.
Each label is sent at most once ever; each form is grouped at most once. The
first call asks for consent, and the answer is remembered.

Codex is not used: its non-interactive mode cannot turn off its shell tool
(even its read-only sandbox can read any file), so text printed in a hostile
PDF could make it read the user's remembered values.

The memory file:

- Linux: `~/.local/share/pdfr/memory.json`
- macOS: `~/Library/Application Support/pdfr/memory.json`
- Windows: `%APPDATA%\pdfr\memory.json`

## Sample PDFs and tools

`samples/` holds generated test PDFs (a real form, a flat form, the same form
rotated 90°, a scanned version and a 300-page file). Regenerate them with:

```sh
uv run --with reportlab --with pillow --with numpy tools/gen_samples.py   # --big for 300 pages
```

Real-world government forms you test with locally can go in `samples/` too;
`.gitignore` keeps `samples/Anexo*.pdf` out of git.

Dev examples:

```sh
cargo run --release -p pdfr-core --example detect -- form.pdf          # detected fields
cargo run --release -p pdfr-core --example runs -- form.pdf 0          # text runs of page 0
cargo run --release -p pdfr-assist --example labels -- form.pdf        # labels, people, AI request (no call)
```

`pdfr --register` and `--unregister` (in `--help`) add an archive install to the
app menu. Hidden command-line flags for automated checks: `--demo` (fill every field with
sample values), `--save <path>`, `--screenshot <png>`, `--zoom <f>`,
`--data-dir <dir>`, `--no-ai`, `--settings <section>` (open Settings),
`--ask-remember` (show the remember dialog), `--hover x,y` (pretend the
pointer is at a point on page 1) and `--values <json>` (type values at page
positions). `tools/compare_screen.py` compares a
screenshot with a render of the saved PDF.

### README screenshots and videos

`docs/media/` holds the README's media, made with made-up data only, on a
fictional scanned form: `tools/gen_media_form.py` draws a household benefit
application from the made-up Municipality of Northvale, "scans" it (skew,
warm paper, uneven light, blur, dust, JPEG) into
`samples/household_scanned.pdf`, and writes the values to type into it,
by position, to `samples/household_values.json`.

- Videos: `tools/make_demo.sh <fill|remember>` records a scripted scene (see
  `crates/app/src/app/record.rs`) with the hidden `--record <dir> --script
  <name>` flags and encodes a GIF and an MP4 with ffmpeg. The remember scene
  uses the unscanned form, because suggestions need the printed labels as text.
- Screenshots: `--values samples/household_values.json --screenshot out.png`
  (plus `--settings`, `--hover`, `--zoom` as needed). For the Omarchy theme
  pictures, run `pdfr --screenshot out.png` without a file (the empty window
  shows the theme's background) and with `HOME` pointing at a folder whose
  `.config/omarchy/current/theme` links to `/usr/share/omarchy/themes/<name>`.
- `docs/brand/social-preview.png` is the repository's social preview image
  (GitHub → Settings → Social preview; it can only be uploaded there).

## Desktop integration

`crates/app/src/system/`: the default PDF app (Linux: `mimeapps.list`;
macOS: NSWorkspace; Windows: opens Settings → Default apps), install
detection for update hints, `pdfr --register` / `--unregister` for archive
installs (desktop entry, icons and `~/.local/bin/pdfr`), opening links and the
portal colour scheme. macOS files from Finder arrive as Apple Events
(`application:openURLs:`). Settings → General shows all of it.

The app id is `io.github.kjellberg.pdfr` (Wayland app id, desktop file, icon,
AppStream). The icon's sources are in `tools/brand/`; `cargo run -p pdfr
--example make_icons` renders the PNG, .ico and .icns files in `assets/icon/`.

## Releases and packaging

See [docs/RELEASING.md](docs/RELEASING.md). In short:

- `packaging/linux/`: desktop entry, AppStream metadata, `install.sh` (used by
  every Linux package and the tarball).
- `packaging/aur/`: `pdfr` (from source) and `pdfr-bin` (prebuilt).
- `crates/app/Cargo.toml`: `.deb` (cargo-deb) and `.rpm` (cargo-generate-rpm).
- `packaging/flatpak/`, `packaging/homebrew/`, `packaging/macos/`,
  `packaging/windows/`, `packaging/winget/`.
- `.github/workflows/`: `ci.yml`, `build-linux.yml`, `release.yml`.

Every package includes `LICENSE.md`, `assets/LICENSE-Inter.txt`,
`assets/LICENSE-GreatVibes.txt` and `assets/THIRD-PARTY-NOTICES.txt`: the licences of the crates pdfr is built
with (mostly MIT and Apache-2.0), which ask for their notices to ship with
binaries. After changing dependencies, run `tools/notices.sh` (needs
`cargo install cargo-about --locked --features cli`) and commit the file; CI
fails while it is out of date. Every licence pdfr accepts is listed in
`about.toml`: a dependency under any other licence fails the check until
someone has looked at it.
