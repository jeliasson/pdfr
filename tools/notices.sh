#!/bin/sh
# Writes assets/THIRD-PARTY-NOTICES.txt from Cargo.lock (needs
# `cargo install cargo-about --locked --features cli`).
set -eu
cd "$(dirname "$0")/.."
cargo about generate --locked --fail about.hbs -o assets/THIRD-PARTY-NOTICES.txt
