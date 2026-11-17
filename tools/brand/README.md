# Brand sources

The app icon is a form field where "pdfr" has just been typed (the "r" in
coral, like the wordmark), with the caret after it. The finished, path-only SVGs live in `assets/icon/`; nothing
here is needed to build pdfr.

To change the icon:

1. Put `InterDisplay-Bold.ttf` (from the Inter 4.1 release zip, `extras/ttf/`,
   SIL Open Font License) in `tools/brand/fonts/` (ignored by git).
2. Edit `brand.py`, then write the SVGs:
   `uv run --with fonttools python tools/brand/brand.py assets/icon`
3. Render the PNG, .ico and .icns files:
   `cargo run -p pdfr --example make_icons`

`concepts.py` holds the two concepts that were not chosen (see
`docs/brand/concepts.png`).

Sizes: `pdfr-tiny.svg` is used for 16–24 px, `pdfr-small.svg` for 32–48 px,
`pdfr.svg` from 64 px up, and `pdfr-macos.svg` (Apple's icon grid, with a
shadow) for the macOS .icns.

Colours: coral gradient `#FF9259` → `#E5336D`, ink `#2B1533`, caret `#E5336D`.
