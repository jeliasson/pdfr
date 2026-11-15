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
desktop) and uses the XDG desktop portal for file dialogs and printing.

The timing-based scrolling test (`steady_scroll_through_300_pages`) is
reliable in release builds; in debug builds it can fail on a busy machine.

> The unrelated crate [`pdfrs`](https://crates.io/crates/pdfrs) is one letter
> away. pdfr is not published on crates.io (`publish = false`).

## Project rules

These come from the original brief and are not negotiable without discussion:

- Pure Rust: no C or C++ libraries, no downloads at build or run time other
  than crates from crates.io. Commit `Cargo.lock`.
- The only external program pdfr may run is the user's own AI command-line
  tool, and it is optional.
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
- **Font**: values are always set in the bundled Noto Sans Regular.
- **Saving** writes real form fields with appearance streams and the embedded
  font (searchable, copyable text). Detected fields become real form fields
  other viewers can edit. The first save suggests `<name>-filled.pdf`.
- **Printing** goes through the XDG print portal over D-Bus (Linux only for
  now; needs `xdg-desktop-portal` with a print backend such as
  `xdg-desktop-portal-gtk`).
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

Hidden command-line flags for automated checks: `--demo` (fill every field with
sample values), `--save <path>`, `--screenshot <png>`, `--zoom <f>`,
`--data-dir <dir>`, `--no-ai`, `--settings <section>` (open Settings),
`--ask-remember` (show the remember dialog) and `--hover x,y` (pretend the
pointer is at a point on page 1). `tools/compare_screen.py` compares a
screenshot with a render of the saved PDF.

## Releases and packaging

- `packaging/aur/PKGBUILD`: Arch / Omarchy (builds from the release tarball).
- `packaging/homebrew/pdfr.rb`: formula for the personal tap.
- `cargo deb -p pdfr`: Debian package (metadata in `crates/app/Cargo.toml`).
- `.github/workflows/ci.yml`: tests, and release binaries for Linux, macOS and
  Windows on tags.

Replace the `OWNER` placeholders with the GitHub account before the first
release. Release archives must include `LICENSE.md` and
`assets/LICENSE-NotoSans.txt`. The licences of the Rust crates pdfr is built
with (mostly MIT and Apache-2.0) also ask for their notices to be included
with binaries; a generated third-party notices file (for example with
`cargo about`) should be added to the release archives.
