#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# The browser game (README.md, "Browser"): the WebGL 2 build into pkg/ and the WebGPU build into pkg-webgpu/.
#
#   tools/build-web.sh            both
#   tools/build-web.sh webgl2     one of them: webgl2 or webgpu
#
# Each is compiled with cargo's web profile (Cargo.toml), bound with wasm-bindgen, optimised again and stripped of its
# function names with wasm-opt, and given brotli (.br) and gzip (.gz) copies of its module and script, which the server
# (crates/server/src/statics.rs) sends to browsers that take them. The food textures get the WebP copies the browser
# builds load. Needs the wasm32-unknown-unknown target, the wasm-bindgen CLI at the version Cargo.lock pins, wasm-opt
# (binaryen), Node, and Python with Pillow when a food texture has changed.
#
# The module with its function names is kept, not served, at TARGET_DIR/symbols/PKG/hotdog_simulator_bg.wasm. A panic's
# stack in the browser's console names functions by number (wasm-function[1234]); that copy says which:
#   wasm-opt TARGET_DIR/symbols/pkg/hotdog_simulator_bg.wasm --print-function-map | grep '^1234:'
#
# From the environment: TARGET_DIR (target-web); OUT, the directory that gets pkg/ and pkg-webgpu/ (the repository);
# LOCK, a file to hold with flock while they are written; and SIMD, 1 by default: the build uses WebAssembly's 128-bit
# SIMD (`-C target-feature=+simd128` for the wasm32 target alone, `--enable-simd` for wasm-opt), which every current
# browser runs (Chrome and Edge 91, Firefox 89, Safari 16.4; index.html says so to one that cannot); SIMD=0 builds
# without it. The toolchain already turns on bulk memory, non-trapping float-to-int, sign extension, multi-value and
# reference types for wasm32. Changing it rebuilds everything in TARGET_DIR, so keep one target directory for each.
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET_DIR=${TARGET_DIR:-target-web}
OUT=${OUT:-.}
SIMD=${SIMD:-1}
opt_features=()
if [ "$SIMD" = 1 ]; then
  export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="${CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS:-} -C target-feature=+simd128"
  opt_features=(--enable-simd)
fi
if [ $# -eq 0 ]; then set -- webgl2 webgpu; fi

level=$(awk '/^\[profile\.web\]/ {web = 1; next} /^\[/ {web = 0} web && $1 == "opt-level" {gsub(/"/, "", $3); print $3}' Cargo.toml)
pinned=$(awk '$0 == "name = \"wasm-bindgen\"" {getline; gsub(/"/, "", $3); print $3; exit}' Cargo.lock)
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [ "$have" != "$pinned" ]; then
  echo "wasm-bindgen ${have:-is missing}; Cargo.lock pins $pinned: cargo install wasm-bindgen-cli --version $pinned" >&2
  exit 1
fi
command -v wasm-opt >/dev/null || { echo "wasm-opt is missing: binaryen (https://github.com/WebAssembly/binaryen/releases) or cargo install wasm-opt" >&2; exit 1; }
command -v node >/dev/null || { echo "node is missing (it writes the compressed copies)" >&2; exit 1; }

# The food textures as the browser builds load them (src/hotdog.rs): a lossy WebP (quality 92) beside each PNG, about a
# tenth of its size, written again whenever the PNG is newer. Needs Pillow then.
stale=()
for png in assets/materials/food/*.png; do
  if [ ! -e "${png%.png}.webp" ] || [ "$png" -nt "${png%.png}.webp" ]; then stale+=("$png"); fi
done
if [ ${#stale[@]} -gt 0 ]; then
  echo "== WebP copies of ${#stale[@]} food textures"
  python3 -I - "${stale[@]}" <<'PYTHON'
import sys
from pathlib import Path
from PIL import Image
for png in map(Path, sys.argv[1:]):
    Image.open(png).save(png.with_suffix('.webp'), 'WEBP', quality=92, method=6)
PYTHON
fi

# The script and the module first, then their compressed copies: the server sends a copy only while it is no older than
# the file it copies, so a copy left from an earlier build is never sent with a new build.
deploy() {
  local stage=$1 pkg=$2
  mkdir -p "$pkg/snippets"
  if [ -d "$stage/snippets" ]; then cp -r "$stage/snippets/." "$pkg/snippets/"; fi
  for file in hotdog_simulator.d.ts hotdog_simulator_bg.wasm.d.ts hotdog_simulator.js hotdog_simulator_bg.wasm \
    hotdog_simulator.js.gz hotdog_simulator_bg.wasm.gz hotdog_simulator.js.br hotdog_simulator_bg.wasm.br; do
    cp "$stage/$file" "$pkg/.$file.new"
    mv -f "$pkg/.$file.new" "$pkg/$file"
  done
}

for build in "$@"; do
  case $build in
    webgl2) pkg=pkg features=() ;;
    webgpu) pkg=pkg-webgpu features=(--features webgpu) ;;
    *) echo "unknown build: $build (webgl2 or webgpu)" >&2; exit 1 ;;
  esac
  stage=$TARGET_DIR/stage/$pkg symbols=$TARGET_DIR/symbols/$pkg
  echo "== $build: cargo, web profile (opt-level $level)$([ "$SIMD" = 1 ] && echo ', SIMD')"
  cargo build --profile web --target wasm32-unknown-unknown --target-dir "$TARGET_DIR" "${features[@]}"
  rm -rf "$stage" && mkdir -p "$stage" "$symbols"
  wasm-bindgen --target web --out-name hotdog_simulator --out-dir "$stage" "$TARGET_DIR/wasm32-unknown-unknown/web/hotdog-simulator.wasm"
  echo "== $build: wasm-opt -O$level"
  wasm-opt "-O$level" "${opt_features[@]}" -g "$stage/hotdog_simulator_bg.wasm" -o "$symbols/hotdog_simulator_bg.wasm"
  wasm-opt "${opt_features[@]}" --strip-debug --strip-producers "$symbols/hotdog_simulator_bg.wasm" -o "$stage/hotdog_simulator_bg.wasm"
  echo "== $build: brotli and gzip"
  node -e '
    const zlib = require("zlib"), fs = require("fs");
    for (const file of process.argv.slice(1)) {
      const data = fs.readFileSync(file);
      const quality = {[zlib.constants.BROTLI_PARAM_QUALITY]: 11, [zlib.constants.BROTLI_PARAM_LGWIN]: 24, [zlib.constants.BROTLI_PARAM_SIZE_HINT]: data.length};
      fs.writeFileSync(file + ".br", zlib.brotliCompressSync(data, {params: quality}));
      fs.writeFileSync(file + ".gz", zlib.gzipSync(data, {level: 9}));
    }' "$stage/hotdog_simulator_bg.wasm" "$stage/hotdog_simulator.js"
  if [ -n "${LOCK:-}" ]; then exec 9>>"$LOCK"; flock 9; fi
  deploy "$stage" "$OUT/$pkg"
  if [ -n "${LOCK:-}" ]; then flock -u 9; exec 9>&-; fi
  echo "== $build: $OUT/$pkg/"
  ls -l "$OUT/$pkg"/hotdog_simulator_bg.wasm* "$OUT/$pkg"/hotdog_simulator.js*
done
