#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Builds the library (tools/build-ffi.sh), compiles examples/smoke.cpp against the static library with the system C++
# compiler, and runs it from the repository's root.
#
#   crates/kitchen-ffi/examples/run-smoke.sh
#   CXX=clang++ CARGO_TARGET_DIR=elsewhere crates/kitchen-ffi/examples/run-smoke.sh
#   SKIP_BUILD=1 ...        # use the library as it was last built
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
crate="$root/crates/kitchen-ffi"
target="${CARGO_TARGET_DIR:-$crate/target}"
case "$target" in /*) ;; *) target="$root/$target" ;; esac
export CARGO_TARGET_DIR="$target"

if [ -z "${SKIP_BUILD:-}" ]; then
    "$root/tools/build-ffi.sh" >/dev/null
fi
lib="$target/release/libkitchen_ffi.a"
out="$target/smoke"
mkdir -p "$out"
# What a Rust static library needs from the system (rustc --print native-static-libs): no C++ runtime of its own.
"${CXX:-c++}" -std=c++17 -O2 -Wall -Wextra -I "$crate/include" "$crate/examples/smoke.cpp" "$lib" \
    -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc -o "$out/smoke"
echo "built $out/smoke" >&2
cd "$root"
exec "$out/smoke" "$root"
