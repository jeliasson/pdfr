# Releasing pdfr

Everything is built by GitHub Actions; nothing is built for release on a
developer's machine.

- `.github/workflows/ci.yml`: every push. Clippy and tests on Linux, macOS
  and Windows, then the Linux packages (`build-linux.yml`), installed and
  started on Ubuntu 22.04 and 24.04, Debian 12, Fedora, openSUSE Tumbleweed and
  Arch (X11), and under Sway (Wayland).
- `.github/workflows/release.yml`: a `v*` tag. The same Linux packages, a
  GitHub release, and the AUR, Homebrew and apt/rpm repositories.

## Making a release

1. Add what changed under `## [Unreleased]` in `CHANGELOG.md`, for users.
2. `tools/release.sh 0.2.0`. This sets the version in `Cargo.toml` and
   `Cargo.lock`, dates the changelog, and adds the release to the AppStream
   file and the PKGBUILDs.
3. Review, then commit and tag (signed):

   ```sh
   git commit -am "pdfr 0.2.0"
   git tag -s v0.2.0 -m "pdfr 0.2.0"
   git push && git push origin v0.2.0
   ```

4. Watch the Release workflow. A tag with a `-` (`v0.2.0-rc1`) makes a
   pre-release on GitHub only, for trying the packages first.

## What the release contains

| File | For |
|---|---|
| `pdfr-vX-x86_64-linux.tar.gz`, `…-aarch64-linux.tar.gz` | any Linux; `./pdfr --register` or `sudo ./install.sh` |
| `pdfr_X-1_amd64.deb`, `…_arm64.deb` | Debian, Ubuntu, Mint, Pop!_OS |
| `pdfr-X-1.x86_64.rpm`, `….aarch64.rpm` | Fedora, RHEL and rebuilds, openSUSE |
| `SHA256SUMS` | checksums |

The binaries need glibc 2.31 or newer (Debian 11, Ubuntu 20.04, RHEL 9 and
later). CI checks this.

## One-time setup

Each publishing step is skipped until its secret exists, so these can be done
one at a time.

### AUR (`pdfr` and `pdfr-bin`)

1. Make an account on <https://aur.archlinux.org> and add an SSH public key.
2. Create both packages once by hand: `git clone ssh://aur@aur.archlinux.org/pdfr.git`
   (and `pdfr-bin.git`), copy in the PKGBUILD from `packaging/aur/`, fill in the
   `Maintainer:` e-mail, `makepkg --printsrcinfo > .SRCINFO`, commit, push.
3. Repository secrets: `AUR_SSH_PRIVATE_KEY`, `AUR_USERNAME`, `AUR_EMAIL`.

### Homebrew tap

1. The public repository `kjellberg/homebrew-tap`.
2. A deploy key with write access on it; its private half is the secret
   `TAP_DEPLOY_KEY`.

### apt and rpm repositories

1. The public repository `kjellberg/packages` with GitHub Pages on (branch
   `main`, root). It is served at <https://kjellberg.github.io/packages>.
2. A signing key just for this (RSA, which apt, dnf and zypper all accept; no
   passphrase, so CI can use it), as the secret `REPO_GPG_PRIVATE_KEY`:

   ```sh
   gpg --batch --passphrase '' --quick-gen-key "pdfr packages" rsa4096 sign never
   gpg --armor --export-secret-keys "pdfr packages"
   ```

   Keep a copy somewhere safe (a password manager): GitHub secrets can't be
   read back, and a new key means every user has to import it again.
3. A deploy key with write access on `kjellberg/packages`; its private half
   is the secret `PACKAGES_DEPLOY_KEY`.

### Flathub

The release workflow builds a Flatpak bundle as a check. Publishing on
Flathub is a one-time review:

1. Fork <https://github.com/flathub/flathub>, branch `new-pr`.
2. Add `io.github.kjellberg.pdfr.yml` (from `packaging/flatpak/`, with the
   `git` source and tag instead of `dir`) and `cargo-sources.json`.
3. Open the pull request and answer the review. Flathub then builds every
   new tag on its own.

## Before announcing: try it on the desktops

CI covers starting and drawing a form on each distribution. These need a
person: check them in a virtual machine (e.g. GNOME Boxes or quickemu) for a
first release and after big changes.

| Desktop | Session | Check |
|---|---|---|
| GNOME (Fedora, Ubuntu) | Wayland, X11 | title bar, light/dark, file dialog, print, Open with, default app |
| KDE Plasma (Kubuntu, Fedora KDE) | Wayland, X11 | same |
| Cosmic (Pop!_OS) | Wayland | same |
| XFCE, Cinnamon (Mint) | X11 | same |
| Hyprland (Omarchy) | Wayland | same, Super+Space launcher |
| Sway, i3 without portals | Wayland / X11 | pdfr's own file dialog, printing message |
| Any | HiDPI and fractional scaling | sharp text, correct sizes |

## macOS

Released since 0.1.0 (`RELEASE_MACOS=true`), unsigned: the app is signed ad
hoc, and users click **Open Anyway** once (README, Install). The build has its
own workflow, `.github/workflows/build-macos.yml`, which mounts the disk image
and starts the app; run it by hand with
`gh workflow run build-macos.yml -f version=0.0.0-test` to try a change.

- `packaging/macos/Info.plist`: bundle id `io.github.kjellberg.pdfr`, opens PDFs,
  so it appears under "Open With".
- `packaging/macos/make_app.sh <version> <out>`: builds a universal
  `pdfr.app`, a `.zip` and a `.dmg`; signs and notarizes when the `APPLE_*`
  secrets exist.
- `packaging/homebrew/pdfr-cask.rb`: the cask (`brew install --cask pdfr`), which
  also puts `pdfr` on the PATH.

Still to check on a real Mac: opening from
Finder (double-click, Open With, dropping on the Dock icon), the default-app
button, printing, and the menu bar. Notarizing needs an Apple Developer ID
($99/year) and the six `APPLE_*` secrets; `make_app.sh` then signs and
notarizes by itself.

## Windows (later, on a PC)

- `packaging/windows/pdfr.wxs`: a per-user installer (no admin needed) with a
  Start menu entry, "Open with", Default apps registration and `pdfr` on the
  PATH. `packaging/windows/build-msi.ps1` builds it with WiX.
- `crates/app/build.rs` embeds the icon and version in `pdfr.exe`.
- `packaging/winget/`: manifest templates for `winget install pdfr`.

To finish: build and install the MSI, check the Start menu, double-clicking a
PDF, "Open with", the Default apps page from Settings → General, and
printing. Then set `RELEASE_WINDOWS=true`.
