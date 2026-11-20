#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# THIRD_PARTY_LICENSES.html: the licence of every crate the game, the server and the launchers are built from (with its
# text, once), the licence of the game itself, and the fonts, adapted shader code, lookup tables and recordings that are
# not the project's own. Every native package and the browser game carry it (tools/dist-common.sh), and the game's
# menu and the page link to it. README.md ("Third-party licences") says how it is made.
#
#   tools/build-licenses.sh            write THIRD_PARTY_LICENSES.html
#   tools/build-licenses.sh --check    make it again, somewhere else, and fail (exit 1) if the committed file differs
#
# cargo-about 0.9.3 (https://github.com/EmbarkStudios/cargo-about) reads each Cargo.lock for every target a package is
# built for and every feature (about.toml), and fails if a crate has a licence that about.toml does not accept, or none
# that it can read; tools/licenses/render.py joins what it found for the game, the server and the launchers, adds
# tools/licenses/other.json (written by hand) and writes the page. The page holds neither the date nor anything of this
# machine, so that --check can compare it byte for byte. Run this after changing a Cargo.lock, a Cargo.toml's
# dependencies, about.toml, other.json or the texts beside it, and commit the result: a pull request's CI runs --check.
#
# Needs: cargo-about (CARGO_ABOUT names it; otherwise `cargo-about` on the PATH:
#   cargo install --locked --version 0.9.3 --features cli cargo-about
# or the release binary), Python 3.11 or later, and cargo with every dependency downloaded: it runs `cargo fetch --locked`
# first (all platforms' crates), unless OFFLINE=1, when they must be in cargo's cache already.
#
# From the environment: CARGO_ABOUT, OFFLINE (1), HOTDOG_SOURCE_URL (the address of the source code, which is the
# `repository` line of Cargo.toml if unset: the one place the project says where its source is, which the packages and
# the game read as well).
set -euo pipefail
cd "$(dirname "$0")/.."
. tools/dist-common.sh

ABOUT_VERSION=0.9.3
out=THIRD_PARTY_LICENSES.html
check=0
case ${1:-} in
  "") ;;
  --check) check=1 ;;
  *) echo "usage: tools/build-licenses.sh [--check]" >&2; exit 2 ;;
esac

about=${CARGO_ABOUT:-cargo-about}
if ! command -v "$about" >/dev/null; then
  echo "cargo-about is missing: cargo install --locked --version $ABOUT_VERSION --features cli cargo-about" >&2
  exit 1
fi
have=$("$about" about --version 2>/dev/null | awk '{print $2}' || true)
if [ "$have" != "$ABOUT_VERSION" ]; then
  echo "cargo-about ${have:-?} is installed; this script is written for $ABOUT_VERSION (cargo install --locked --version $ABOUT_VERSION --features cli cargo-about)" >&2
  exit 1
fi
command -v python3 >/dev/null || { echo "python3 is missing" >&2; exit 1; }

# The programs: the game (every feature, so that the Ultra tier's crates are in as well), the server and the launchers.
# crates/kitchen-sim is in all of them; crates/kitchen-ffi is a library for other programs, in no package.
manifests=(Cargo.toml crates/server/Cargo.toml crates/launcher/Cargo.toml)

work=$(mktemp -d "${TMPDIR:-/tmp}/hotdog-licenses.XXXXXX")
trap 'rm -rf "$work"' EXIT
json=()
for manifest in "${manifests[@]}"; do
  if [ "${OFFLINE:-0}" != 1 ]; then cargo fetch --locked --manifest-path "$manifest" >/dev/null; fi
  file=$work/$(echo "${manifest%/Cargo.toml}" | tr '/' '_').json
  echo "== cargo-about: $manifest"
  "$about" about generate --locked --offline --fail --all-features --config about.toml --manifest-path "$manifest" --format json --output-file "$file"
  json+=("$file")
done

source_url=$(dist_source_url)
python3 -I tools/licenses/render.py --config about.toml --gpl LICENSE --copyright COPYRIGHT --other tools/licenses/other.json \
  --source-url "$source_url" --output "$work/$out" "${json[@]}"

if [ "$check" = 1 ]; then
  if cmp -s "$work/$out" "$out"; then
    echo "== $out is current"
  else
    echo "$out is out of date (a dependency, a licence, about.toml or tools/licenses changed): run tools/build-licenses.sh and commit it" >&2
    diff <(cat "$out" 2>/dev/null) "$work/$out" | head -20 >&2 || true
    exit 1
  fi
else
  cp "$work/$out" "$out"
  echo "== $out ($(wc -c < "$out" | tr -d ' ') bytes)"
fi
