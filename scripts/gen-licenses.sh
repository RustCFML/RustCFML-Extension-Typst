#!/usr/bin/env bash
# Regenerate THIRD-PARTY.txt — the third-party attribution notice packaged
# inside every published `.rcx` (see about.toml for why it must be there).
#
# THIRD-PARTY.txt is a COMMITTED, GENERATED file. Run this after any dependency
# change and commit the result; CI verifies it is up to date.
#
# Requires cargo-about (pin it to the version CI uses, or the formatting will
# differ and the staleness diff will fail):
#
#   cargo install cargo-about --locked --features cli --version 0.9.1
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v cargo-about >/dev/null 2>&1; then
    echo "error: cargo-about not found." >&2
    echo "       cargo install cargo-about --locked --features cli --version 0.9.1" >&2
    exit 1
fi

cargo about generate --config about.toml about.hbs -o THIRD-PARTY.txt.raw

# Collapse runs of blank lines to one.
#
# WHY: a crate shipping SEVERAL licence files has them harvested in
# filesystem-iteration order, and the number of blank lines joining the pieces
# differs between APFS and ext4. That makes the generated file differ by
# whitespace between a dev machine and a CI runner, which is legally meaningless
# but breaks a byte-exact staleness gate. (This bit the engine repo at v0.590.0.)
# Squeezing only ever removes empty lines — no wording or copyright is altered.
cat -s THIRD-PARTY.txt.raw > THIRD-PARTY.txt
rm -f THIRD-PARTY.txt.raw

echo "Wrote THIRD-PARTY.txt ($(wc -l < THIRD-PARTY.txt | tr -d ' ') lines)"
