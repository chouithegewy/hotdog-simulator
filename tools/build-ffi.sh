#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Builds crates/kitchen-ffi (the C interface to the simulation) in release, libkitchen_ffi.a and libkitchen_ffi.so,
# and regenerates its header, crates/kitchen-ffi/include/kitchen_sim.h, with cbindgen (cargo install cbindgen).
#
#   tools/build-ffi.sh                      # into crates/kitchen-ffi/target/release
#   CARGO_TARGET_DIR=elsewhere tools/build-ffi.sh
#
# Prints the static library's path last. The crate is a package of its own (its own lock file and target directory,
# as crates/server is), so this never touches the game client's build.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
crate="$root/crates/kitchen-ffi"
target="${CARGO_TARGET_DIR:-$crate/target}"
case "$target" in /*) ;; *) target="$root/$target" ;; esac

cargo build --release --manifest-path "$crate/Cargo.toml" --target-dir "$target"

if command -v cbindgen >/dev/null 2>&1; then
    (cd "$crate" && cbindgen --quiet --config cbindgen.toml --crate kitchen-ffi --output include/kitchen_sim.h .)
    echo "header: $crate/include/kitchen_sim.h" >&2
else
    echo "cbindgen not found (cargo install cbindgen): $crate/include/kitchen_sim.h left as it is" >&2
fi
echo "$target/release/libkitchen_ffi.a"
