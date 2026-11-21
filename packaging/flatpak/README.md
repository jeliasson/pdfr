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

## AI agents

Agent control needs no extra permission. A sandboxed window puts its socket
in `$XDG_RUNTIME_DIR/app/io.github.kjellberg.pdfr/`, the one runtime folder
Flatpak shares with the host, and the agent starts the server with:

```sh
claude mcp add pdfr -- flatpak run --command=pdfr io.github.kjellberg.pdfr mcp
```

(Settings → AI agents shows this line in a Flatpak build.) The sandbox only
sees files the user opened through a portal, so an agent's `open` and
`save_as` with a path work for those and for nothing else. Try this in a
built Flatpak before a Flathub release: it has not been tested there yet.

Submitting to Flathub: see `docs/RELEASING.md`.
