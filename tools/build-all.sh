#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# Every package of the game, into dist/ (README.md, "Building and packaging"):
#
#   tools/build-all.sh linux windows web    those three (in that order)
#   tools/build-all.sh                      what this machine can build: linux, windows and web on Linux; mac and web
#                                           on macOS; windows and web on Windows
#
#   linux    tools/build-linux.sh     hotdog-simulator-VERSION-linux-x86_64.tar.gz
#   windows  tools/build-windows.sh   hotdog-simulator-VERSION-windows-x86_64.zip
#   mac      tools/build-mac.sh       hotdog-simulator-VERSION-macos-universal.dmg and .zip (on a Mac only)
#   web      tools/build-web.sh       hotdog-simulator-VERSION-web.zip: the browser game's static files, both builds
#                                     (WEB_BUILDS, "webgl2 webgpu" by default), its assets and the map editor
#
# The Linux and Windows packages each carry the game and the server built for x86-64-v2 and for x86-64-v3, and
# launchers that start the one the processor runs best (tools/build-linux.sh; crates/launcher).
#
# Every package is named by one version (tools/dist-common.sh: Cargo.toml's and the commit's), and DIST/SHA256SUMS
# lists the packages of that version. Each platform builds in target directories of its own, TARGET_PREFIX<platform>-*
# (target/dist-linux-v2, ...), and stops when the disk has less than MIN_FREE_GB (25) free. DIST, FEATURES and the
# per-platform settings at the top of each script are read from the environment.
set -euo pipefail
cd "$(dirname "$0")/.."
. tools/dist-common.sh
export DIST TARGET_PREFIX=${TARGET_PREFIX:-target/dist-}
unset TARGET_DIR CPU
if [ $# -eq 0 ]; then
  case $(uname -s) in
    Darwin) set -- mac web ;;
    MINGW*|MSYS*|CYGWIN*) set -- windows web ;;
    *) set -- linux windows web ;;
  esac
fi
for target in "$@"; do
  case $target in
    linux|windows|mac|web) ;;
    macos) ;;
    *) echo "unknown target: $target (linux, windows, mac or web)" >&2; exit 1 ;;
  esac
done

# The browser game: tools/build-web.sh builds into a staging folder (not the repository's pkg/), and the page, its
# scripts, the map editor and the assets go beside it.
package_web() {
  local target_dir=${TARGET_PREFIX}web name=hotdog-simulator-$FILE_VERSION-web stage dist_dir
  stage=$target_dir/stage/$name
  mkdir -p "$target_dir" "$DIST"
  dist_check_disk "$target_dir"
  dist_dir=$(cd "$DIST" && pwd)
  rm -rf "$stage" && mkdir -p "$stage"
  # (build-web.sh reads TARGET_DIR and OUT, and writes OUT/pkg and OUT/pkg-webgpu.)
  TARGET_DIR=$target_dir OUT=$stage tools/build-web.sh ${WEB_BUILDS:-webgl2 webgpu}
  # Only what is served: the type declarations are for TypeScript.
  rm -f "$stage"/pkg*/*.d.ts
  cp index.html dev.html lobby.js lobby.css touch.js touch.css tune.js tune.css devlink.js "$stage/"
  mkdir -p "$stage/editor/fonts" "$stage/tools/network"
  for file in editor/*; do
    case $file in *.test.mjs) ;; *) [ -f "$file" ] && cp "$file" "$stage/editor/" ;; esac
  done
  cp editor/fonts/* "$stage/editor/fonts/"
  cp tools/network/map.mjs tools/network/presets.mjs "$stage/tools/network/"
  dist_stage_assets "$stage/assets" web
  dist_stage_docs "$stage" web
  rm -f "$dist_dir/$name.zip"
  (cd "$(dirname "$stage")" && zip -qr -9 "$dist_dir/$name.zip" "$name")
  echo "== $DIST/$name.zip ($(du -h "$dist_dir/$name.zip" | cut -f1))"
}

for target in "$@"; do
  echo "==== $target ($VERSION)"
  case $target in
    linux) tools/build-linux.sh ;;
    windows) tools/build-windows.sh ;;
    mac|macos) tools/build-mac.sh ;;
    web) package_web ;;
  esac
done

# The checksums of this version's packages.
(
  cd "$DIST"
  packages=()
  for file in hotdog-simulator-"$FILE_VERSION"-*; do
    case $file in *.tar.gz|*.zip|*.dmg) packages+=("$file") ;; esac
  done
  if [ ${#packages[@]} -gt 0 ]; then dist_sha256 "${packages[@]}" > SHA256SUMS; fi
)
echo "==== $DIST:"
ls -l "$DIST"
