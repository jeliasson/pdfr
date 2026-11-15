# Flatpak

`io.github.kjellberg.pdfr.yml` builds pdfr for Flatpak and Flathub. Flathub
builds offline, so the crates are listed in `cargo-sources.json`, which is
generated from `Cargo.lock` (CI does this; it is not committed):

```sh
curl -LO https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
uv run --with aiohttp --with toml python flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
flatpak-builder --user --install --force-clean build-flatpak packaging/flatpak/io.github.kjellberg.pdfr.yml
flatpak run io.github.kjellberg.pdfr
```

Submitting to Flathub: see `docs/RELEASING.md`.
