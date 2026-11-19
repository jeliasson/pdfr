# Brand sources

The app icon is a text cursor (I-beam) standing on the line of a form
field, white on the coral tile: click and type. No text, like other apps'
icons. The finished, path-only SVGs live in `assets/icon/`; nothing here is
needed to build pdfr.

To change the icon:

1. Edit `brand.py`, then write the SVGs:
   `python3 tools/brand/brand.py assets/icon`
2. Render the PNG, .ico and .icns files:
   `cargo run -p pdfr --example make_icons`

`concepts.py` holds earlier concepts (see `docs/brand/concepts.png`); they
need `InterDisplay-Bold.ttf` in `tools/brand/fonts/`.

Sizes: `pdfr-tiny.svg` (boldest) is used for 16–24 px, `pdfr-small.svg` for
32–48 px, `pdfr.svg` from 64 px up, and `pdfr-macos.svg` (Apple's icon grid,
with a shadow) for the macOS .icns and for `png/pdfr-macos-512.png`, the
icon macOS shows in the Dock while pdfr runs.

Colours: coral gradient `#FF9259` → `#E5336D`, cursor white, shadow `#6B0E33`.
