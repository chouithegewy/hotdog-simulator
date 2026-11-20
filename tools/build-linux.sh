#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# The Linux x86_64 package (README.md, "Building and packaging"):
#   DIST/hotdog-simulator-VERSION-linux-x86_64.tar.gz
# one folder of the same name with
#   hotdog-simulator, hotdog-server       the launchers (crates/launcher): each starts the build in bin/ that suits the
#                                         processor, with the same arguments; --cpu v2|v3 or HOTDOG_CPU forces one
#   bin/hotdog-simulator-v3, -v2          the game compiled for x86-64-v3 (AVX2, FMA, BMI2) and for x86-64-v2
#   bin/hotdog-server-v3, -v2             the server, the same
#   assets/                               what the game loads, one copy for both builds
#   README.md, LICENSE, COPYRIGHT, THIRD_PARTY_NOTICES.md, THIRD_PARTY_LICENSES.html, HOW-TO-RUN.txt
#
#   tools/build-linux.sh                  the package
#   CPU=native tools/build-linux.sh       a one-off build for one level, the programs themselves at the top of the
#                                         folder and no launcher: native (the fastest on the machine that builds it,
#                                         for playing there), v3, v2 or baseline (any 64-bit x86); named -CPU
#
# The programs need at least the glibc they were linked against. With cargo-zigbuild and zig (on the PATH, or named
# by CARGO_ZIGBUILD and ZIG) they are linked against glibc GLIBC, 2.31 unless set (Ubuntu 20.04, Debian 11, RHEL 9,
# Fedora 32 and later); otherwise, or with GLIBC=host, against this machine's own (whatever it is: a new distribution's
# glibc makes a binary that older ones refuse). The script prints which, and the newest glibc symbol the game needs.
#   cargo-zigbuild: https://github.com/rust-cross/cargo-zigbuild/releases (or cargo install cargo-zigbuild)
#   zig:            https://ziglang.org/download/
# Needs the Rust target x86_64-unknown-linux-gnu, and ALSA's and udev's development files to link against
# (libasound2-dev, libudev-dev, libwayland-dev on Debian and Ubuntu; alsa-lib-devel, systemd-devel, wayland-devel on
# Fedora).
#
# From the environment: CPU (none: the package with both levels), TARGET_PREFIX (target/dist-: each level builds in
# TARGET_PREFIXlinux-LEVEL, the launchers in TARGET_PREFIXlinux-launcher), DIST (dist), FEATURES (the game's cargo
# features, none by default), HOTDOG_VERSION, MIN_FREE_GB (25).
set -euo pipefail
cd "$(dirname "$0")/.."
. tools/dist-common.sh
triple=x86_64-unknown-linux-gnu
prefix=${TARGET_PREFIX:-target/dist-}linux
if [ -n "${CPU:-}" ]; then levels=$CPU suffix=-$CPU; else levels="v2 v3" suffix=; fi
name=hotdog-simulator-$FILE_VERSION-linux-x86_64$suffix
stage=$prefix-stage/$name
features=()
if [ -n "${FEATURES:-}" ]; then features=(--features "$FEATURES"); fi
mkdir -p "$DIST" "$(dirname "$stage")"
dist_check_disk "$(dirname "$stage")"

glibc=${GLIBC:-2.31}
zigbuild=${CARGO_ZIGBUILD:-$(command -v cargo-zigbuild || true)}
if [ -n "${ZIG:-}" ]; then export CARGO_ZIGBUILD_ZIG_PATH=$ZIG; fi
if [ "$glibc" != host ] && [ -n "$zigbuild" ] && { [ -n "${ZIG:-}" ] || command -v zig >/dev/null; }; then
  build=("$zigbuild" zigbuild --target "$triple.$glibc")
  # The system's libasound, libudev and libwayland-client are linked against as they are; they were built against
  # this machine's newer glibc, and the user's own copies are the ones loaded at run time.
  link=(-C link-arg=-Wl,--allow-shlib-undefined)
else
  if [ "$glibc" != host ]; then echo "(no cargo-zigbuild and zig: linking against this machine's glibc instead of $glibc)" >&2; fi
  glibc=host
  build=(cargo build --target "$triple")
  link=()
fi

rm -rf "$stage" && mkdir -p "$stage"
for level in $levels; do
  dir=$prefix-$level
  echo "== $VERSION: the game and the server for Linux x86_64, $level (glibc: $glibc)"
  (
    dist_cpu "$triple" "$level"
    if [ ${#link[@]} -gt 0 ]; then dist_rustflags "$triple" "${link[@]}"; fi
    "${build[@]}" --release --target-dir "$dir" ${features[@]+"${features[@]}"}
    "${build[@]}" --release --target-dir "$dir" --manifest-path crates/server/Cargo.toml
  )
  for program in hotdog-simulator hotdog-server; do
    if [ -n "${CPU:-}" ]; then out=$stage/$program; else mkdir -p "$stage/bin" && out=$stage/bin/$program-$level; fi
    cp "$dir/$triple/release/$program" "$out"
    strip "$out" 2>/dev/null || llvm-strip "$out"
  done
done
if [ -z "${CPU:-}" ]; then
  echo "== the launchers (plain x86-64)"
  "${build[@]}" --release --target-dir "$prefix-launcher" --manifest-path crates/launcher/Cargo.toml
  cp "$prefix-launcher/$triple/release/hotdog-launch-game" "$stage/hotdog-simulator"
  cp "$prefix-launcher/$triple/release/hotdog-launch-server" "$stage/hotdog-server"
fi
dist_stage_assets "$stage/assets" native
dist_stage_docs "$stage" linux

programs=$(find "$stage" -maxdepth 2 -type f -name 'hotdog-s*' | sort)
needs=$(for p in $programs; do objdump -T "$p" 2>/dev/null; done | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1 || true)
game=$(echo "$programs" | grep '/bin/hotdog-simulator-[^/]*$\|/hotdog-simulator$' | head -1)
echo "== needs ${needs:-an unknown glibc} or newer; the game links to: $(objdump -p "$game" | awk '/NEEDED/ {print $2}' | tr '\n' ' ')"
tar -C "$(dirname "$stage")" --owner=0 --group=0 --numeric-owner --sort=name -czf "$DIST/$name.tar.gz" "$name"
echo "== $DIST/$name.tar.gz ($(du -h "$DIST/$name.tar.gz" | cut -f1))"
