#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# The Windows x86_64 package (README.md, "Building and packaging"):
#   DIST/hotdog-simulator-VERSION-windows-x86_64.zip
# one folder of the same name with
#   hotdog-simulator.exe, hotdog-server.exe   the launchers (crates/launcher): each starts the build in bin\ that suits
#                                             the processor, with the same arguments (--cpu v2|v3 or HOTDOG_CPU forces
#                                             one); the game's opens no console window and has the hotdog icon
#   bin\hotdog-simulator-v3.exe, -v2.exe      the game compiled for x86-64-v3 (AVX2, FMA, BMI2) and for x86-64-v2
#   bin\hotdog-server-v3.exe, -v2.exe         the server, the same
#   assets\                                   what the game loads, one copy for both builds
#   README.md, LICENSE, COPYRIGHT, THIRD_PARTY_NOTICES.md, THIRD_PARTY_LICENSES.html, HOW-TO-RUN.txt (with Windows line ends)
# Every program needs only what Windows 10 and 11 have (the Universal C Runtime among it): no DLLs are shipped.
#
#   tools/build-windows.sh                  the package
#   CPU=v3 tools/build-windows.sh           a one-off build of one level (native, v3, v2 or baseline), the programs at
#                                           the top of the folder and no launcher; named -CPU
#
# On Linux (or macOS) it cross-compiles for x86_64-pc-windows-gnullvm with llvm-mingw, a free clang/lld toolchain for
# Windows with the mingw-w64 headers and import libraries (https://github.com/mstorsjo/llvm-mingw/releases: the
# llvm-mingw-*-ucrt-<host>.tar.xz for this machine, unpacked anywhere). LLVM_MINGW names its folder, or its bin/ is on
# the PATH. No Microsoft SDK is needed or downloaded. On Windows itself (Git Bash or MSYS2, as GitHub's Windows
# runners have) it builds for x86_64-pc-windows-msvc with the Visual Studio build tools that are installed, and
# embeds the icon with llvm-rc or rc.exe. WINDOWS_TARGET chooses the target either way.
#
# From the environment: CPU (none: the package with both levels), TARGET_PREFIX (target/dist-: each level builds in
# TARGET_PREFIXwindows-LEVEL, the launchers in TARGET_PREFIXwindows-launcher), DIST (dist), FEATURES,
# HOTDOG_VERSION, MIN_FREE_GB, as tools/build-linux.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
. tools/dist-common.sh
prefix=${TARGET_PREFIX:-target/dist-}windows
if [ -n "${CPU:-}" ]; then levels=$CPU suffix=-$CPU; else levels="v2 v3" suffix=; fi
name=hotdog-simulator-$FILE_VERSION-windows-x86_64$suffix
stage=$prefix-stage/$name
features=()
if [ -n "${FEATURES:-}" ]; then features=(--features "$FEATURES"); fi
mkdir -p "$DIST" "$(dirname "$stage")"
dist_check_disk "$(dirname "$stage")"

case $(uname -s) in
  MINGW*|MSYS*|CYGWIN*) on_windows=1 ;;
  *) on_windows=0 ;;
esac
if [ "$on_windows" = 1 ]; then triple=${WINDOWS_TARGET:-x86_64-pc-windows-msvc}; else triple=${WINDOWS_TARGET:-x86_64-pc-windows-gnullvm}; fi
# Absolute paths, as the Windows programs below (rc, the linker) read them: C:/... in Git Bash.
absolute() {
  local path=$1
  case $path in /*) ;; *) path=$PWD/$path ;; esac
  if [ "$on_windows" = 1 ]; then cygpath -m "$path"; else echo "$path"; fi
}
dist_dir=$(cd "$DIST" && pwd)

# The icon and the version shown in the .exe's properties, as a resource script compiled for the linker.
version_numbers=$(echo "$VERSION" | awk -F'[.+-]' '{printf "%d,%d,%d,0", $1, $2, $3}')
rc=$(dirname "$stage")/hotdog-simulator.rc
cat > "$rc" <<RC
1 ICON "$(absolute tools/packaging/hotdog.ico)"
1 VERSIONINFO
FILEVERSION $version_numbers
PRODUCTVERSION $version_numbers
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Hotdog Simulator"
      VALUE "FileVersion", "$VERSION"
      VALUE "InternalName", "hotdog-simulator"
      VALUE "LegalCopyright", "Copyright (C) 2026 David Nunes-Childs. GPL-3.0-only."
      VALUE "OriginalFilename", "hotdog-simulator.exe"
      VALUE "ProductName", "Hotdog Simulator"
      VALUE "ProductVersion", "$VERSION"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
RC

case $triple in
  *-gnullvm|*-gnu)
    bin=${LLVM_MINGW:+$LLVM_MINGW/bin/}
    clang=${bin}x86_64-w64-mingw32-clang
    command -v "$clang" >/dev/null || { echo "$clang is missing: unpack llvm-mingw (see the top of this script) and set LLVM_MINGW" >&2; exit 1; }
    upper=$(echo "$triple" | tr 'a-z-' 'A-Z_') lower=$(echo "$triple" | tr '-' '_')
    export "CARGO_TARGET_${upper}_LINKER=$clang" "CC_$lower=$clang" "CXX_$lower=${clang}++" "AR_$lower=${bin}llvm-ar"
    resource=$(absolute "$(dirname "$stage")/hotdog-simulator-resource.o")
    "${bin}x86_64-w64-mingw32-windres" "$rc" -O coff -o "$resource"
    strip=${bin}llvm-strip
    ;;
  *-msvc)
    resource=$(absolute "$(dirname "$stage")/hotdog-simulator.res")
    if command -v llvm-rc >/dev/null; then
      llvm-rc -fo "$resource" "$rc"
    elif command -v rc.exe >/dev/null; then
      rc.exe -nologo -fo "$resource" "$rc"
    else
      echo "(neither llvm-rc nor rc.exe: the .exe gets no icon)" >&2
      resource=
    fi
    strip=
    ;;
  *) echo "unknown Windows target $triple" >&2; exit 1 ;;
esac
# The resource goes to the game's link alone (cargo rustc passes the flags after -- to that crate only).
link=()
if [ -n "$resource" ]; then link=(-C "link-arg=$resource"); fi

rm -rf "$stage" && mkdir -p "$stage"
for level in $levels; do
  dir=$prefix-$level
  echo "== $VERSION: the game and the server for Windows x86_64, $level ($triple)"
  (
    dist_cpu "$triple" "$level"
    cargo rustc --release --target "$triple" --target-dir "$dir" --bin hotdog-simulator ${features[@]+"${features[@]}"} -- ${link[@]+"${link[@]}"}
    cargo build --release --target "$triple" --target-dir "$dir" --manifest-path crates/server/Cargo.toml
  )
  for program in hotdog-simulator hotdog-server; do
    if [ -n "${CPU:-}" ]; then out=$stage/$program.exe; else mkdir -p "$stage/bin" && out=$stage/bin/$program-$level.exe; fi
    cp "$dir/$triple/release/$program.exe" "$out"
    if [ -n "$strip" ]; then "$strip" "$out"; fi
  done
done
if [ -z "${CPU:-}" ]; then
  echo "== the launchers (plain x86-64)"
  cargo rustc --release --target "$triple" --target-dir "$prefix-launcher" --manifest-path crates/launcher/Cargo.toml --bin hotdog-launch-game -- ${link[@]+"${link[@]}"}
  cargo build --release --target "$triple" --target-dir "$prefix-launcher" --manifest-path crates/launcher/Cargo.toml --bin hotdog-launch-server
  cp "$prefix-launcher/$triple/release/hotdog-launch-game.exe" "$stage/hotdog-simulator.exe"
  cp "$prefix-launcher/$triple/release/hotdog-launch-server.exe" "$stage/hotdog-server.exe"
fi
dist_stage_assets "$stage/assets" native
dist_stage_docs "$stage" windows

readobj=$(command -v llvm-readobj || command -v "${LLVM_MINGW:-.}/bin/llvm-readobj" || true)
if [ -n "$readobj" ]; then
  for exe in $(cd "$stage" && find . -maxdepth 2 -name '*.exe' | sort); do
    echo "== $exe loads: $("$readobj" --coff-imports "$stage/$exe" | awk '/Name: .*\.dll/ {print $2}' | sort -uf | tr '\n' ' ')"
  done
fi
rm -f "$dist_dir/$name.zip"
if command -v zip >/dev/null; then
  (cd "$(dirname "$stage")" && zip -qr -9 "$dist_dir/$name.zip" "$name")
elif command -v 7z >/dev/null; then
  (cd "$(dirname "$stage")" && 7z a -tzip -mx=9 "$(absolute "$dist_dir/$name.zip")" "$name" >/dev/null)
else
  powershell -NoProfile -Command "Compress-Archive -Path '$(cygpath -w "$PWD/$stage")' -DestinationPath '$(cygpath -w "$dist_dir/$name.zip")'"
fi
echo "== $DIST/$name.zip ($(du -h "$dist_dir/$name.zip" | cut -f1))"
