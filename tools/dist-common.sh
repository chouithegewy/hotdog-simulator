# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# What tools/build-linux.sh, build-windows.sh, build-mac.sh and build-all.sh share (sourced from the repository's
# root, never run): the version, the assets a package carries, its documents and HOW-TO-RUN.txt. Written for bash 3.2
# as well (macOS's own), with BSD or GNU tools.

DIST=${DIST:-dist}

# The version every package is named by and the game logs (src/runtime.rs, VERSION): Cargo.toml's, then the commit's
# short hash, then ".dirty" when the working tree differs from that commit (0.1.0+74c7c92.dirty). HOTDOG_VERSION set
# in the environment is taken as it is.
dist_version() {
  if [ -n "${HOTDOG_VERSION:-}" ]; then echo "$HOTDOG_VERSION"; return; fi
  local version hash
  version=$(awk -F'"' '/^\[package\]/ {p = 1; next} /^\[/ {p = 0} p && /^version *=/ {print $2; exit}' Cargo.toml)
  if hash=$(git rev-parse --short=7 HEAD 2>/dev/null); then
    if [ -n "$(git status --porcelain 2>/dev/null | head -1)" ]; then hash="$hash.dirty"; fi
    version="$version+$hash"
  fi
  echo "$version"
}
VERSION=$(dist_version)
export HOTDOG_VERSION=$VERSION
# The version as it goes into file names: 0.1.0-74c7c92.dirty.
FILE_VERSION=$(echo "$VERSION" | tr '+' '-')

# Where the project's source code is published, which every package and the browser game say (HOW-TO-RUN.txt,
# THIRD_PARTY_LICENSES.html, the game's menu): HOTDOG_SOURCE_URL when it is set, otherwise the `repository` line of
# Cargo.toml, the one place the project writes it. (The game's own copy of it is Cargo.toml's, or HOTDOG_SOURCE_URL when
# that is set while it is compiled: src/menu.rs.)
#   dist_source_url
dist_source_url() {
  if [ -n "${HOTDOG_SOURCE_URL:-}" ]; then echo "$HOTDOG_SOURCE_URL"; return; fi
  awk -F'"' '/^repository *=/ {print $2; exit}' Cargo.toml
}

# Adds FLAGS to the rustc flags of the crates built for TRIPLE alone, in this script's environment only:
# CARGO_TARGET_<TRIPLE>_RUSTFLAGS, or RUSTFLAGS when that is set (cargo then reads only it; with --target it still
# leaves build scripts alone). Never a .cargo/config.toml: the browser build and every other build keep their own.
#   dist_rustflags TRIPLE FLAGS...
dist_rustflags() {
  local triple=$1 var
  shift
  if [ -n "${RUSTFLAGS:-}" ]; then
    export RUSTFLAGS="$RUSTFLAGS $*"
  else
    var=CARGO_TARGET_$(echo "$triple" | tr 'a-z-' 'A-Z_')_RUSTFLAGS
    export "$var=${!var:-} $*"
  fi
}

# The x86-64 level a native build is compiled for (-C target-cpu). A Linux or Windows package carries the game and the
# server built twice, for v2 and v3, and launchers that start the one the processor runs best (crates/launcher);
# CPU=LEVEL in tools/build-linux.sh and build-windows.sh makes a one-off build of one level instead.
#   baseline  the target's default (x86-64, SSE2: every 64-bit x86 processor; no flag)
#   v2        x86-64-v2  SSE4.2, SSSE3, POPCNT: Intel Nehalem (2008) and AMD Bulldozer (2011) on
#   v3        x86-64-v3  AVX2, FMA, BMI2: Intel Haswell (2013) and AMD Excavator/Zen (2015) on
#   native    this machine's own processor: the fastest for playing on the machine that builds it, and only there
# The game and the server also check at start that the processor has what their build assumed (src/runtime.rs,
# check_cpu), for when one is started without its launcher.
#   dist_cpu TRIPLE LEVEL
dist_cpu() {
  local cpu
  case $2 in
    baseline) return ;;
    v2) cpu=x86-64-v2 ;;
    v3) cpu=x86-64-v3 ;;
    native) cpu=native ;;
    *) echo "CPU=$2: baseline, v2, v3 or native" >&2; exit 1 ;;
  esac
  dist_rustflags "$1" -C "target-cpu=$cpu"
}

# The assets the game loads, as paths under assets/: every file that src/ names by its asset path (the sounds, the
# shaders, any GLB model, the package label), every food texture of the kind the build loads (src/hotdog.rs: PNG
# natively, WebP in a browser) and every map (the server's lobby lists them; a browser fetches some). Not packaged:
# the original MP3 downloads (nothing plays them: tools/build_kungfu_user_sounds.py cut them into
# assets/audio/kungfu/), sounds only the C interface names, and assets/models/ (not loaded). The breakfast intro's
# people are made in code (src/human/) and need no file: every package has the intro.
#   dist_asset_list native|web
dist_asset_list() {
  local kind=$1 texture=png file
  [ "$kind" = web ] && texture=webp
  grep -rhoE '"(audio|shaders|materials)/[A-Za-z0-9_./-]+"|"[A-Za-z0-9_-]+\.glb"' src --include='*.rs' | tr -d '"' | sort -u |
    while read -r file; do
      [ -f "assets/$file" ] || continue
      echo "$file"
    done
  for file in assets/materials/food/*."$texture" assets/maps/*.json; do echo "${file#assets/}"; done
}

# Copies the assets into DEST (a package's assets/ folder), with the notes on the ones that are not the project's own.
#   dist_stage_assets DEST native|web
dist_stage_assets() {
  local dest=$1 kind=$2 file count=0
  mkdir -p "$dest"
  for file in $(dist_asset_list "$kind"); do
    mkdir -p "$dest/$(dirname "$file")"
    cp "assets/$file" "$dest/$file"
    count=$((count + 1))
  done
  cp assets/CHARACTER_CREDITS.md "$dest/"
  echo "== assets: $count files, $(du -sh "$dest" | cut -f1)"
}

# Copies the documents into DEST and writes its HOW-TO-RUN.txt. THIRD_PARTY_LICENSES.html (tools/build-licenses.sh;
# committed, and checked current by CI) goes into every package and into the browser package's root, where the server
# serves it at /THIRD_PARTY_LICENSES.html and the page and the game's menu link to it.
#   dist_stage_docs DEST linux|windows|mac|web
dist_stage_docs() {
  local dest=$1 platform=$2
  if [ ! -f THIRD_PARTY_LICENSES.html ]; then
    echo "THIRD_PARTY_LICENSES.html is missing: tools/build-licenses.sh makes it, and every package carries it" >&2
    exit 1
  fi
  cp README.md LICENSE COPYRIGHT THIRD_PARTY_NOTICES.md THIRD_PARTY_LICENSES.html "$dest/"
  dist_howto "$platform" > "$dest/HOW-TO-RUN.txt"
  if [ "$platform" = windows ]; then
    # Notepad and every other Windows editor read CRLF.
    awk '{ sub(/\r$/, ""); printf "%s\r\n", $0 }' "$dest/HOW-TO-RUN.txt" > "$dest/HOW-TO-RUN.tmp" && mv "$dest/HOW-TO-RUN.tmp" "$dest/HOW-TO-RUN.txt"
  fi
}

# Says how much free space the disk holding DIR has left, in GB, and stops the build below MIN_FREE_GB (25 by default):
# each platform's build takes several GB.
#   dist_check_disk DIR
dist_check_disk() {
  local free
  free=$(df -Pk "$1" | awk 'NR == 2 {print int($4 / 1048576)}')
  if [ "$free" -lt "${MIN_FREE_GB:-25}" ]; then
    echo "Only ${free} GB free on the disk of $1 (MIN_FREE_GB is ${MIN_FREE_GB:-25}): stopping" >&2
    exit 1
  fi
}

# A file's SHA-256, as sha256sum prints it.
dist_sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}

# HOW-TO-RUN.txt for one platform.
dist_howto() {
  local platform=$1 game server config
  case $platform in
    linux) game=./hotdog-simulator server=./hotdog-server config='~/.config/hotdog-simulator/ ($XDG_CONFIG_HOME/hotdog-simulator/ when XDG_CONFIG_HOME is set)' ;;
    windows) game=hotdog-simulator.exe server=hotdog-server.exe config='%APPDATA%\hotdog-simulator\' ;;
    mac) game='"Hotdog Simulator.app/Contents/MacOS/hotdog-simulator"' server='"Hotdog Simulator.app/Contents/MacOS/hotdog-server"' config='~/.config/hotdog-simulator/' ;;
  esac
  echo "HOTDOG SIMULATOR $VERSION"
  echo
  if [ "$platform" = web ]; then
    cat <<EOF
This is the browser game: static files (the page, the WebAssembly builds in pkg/ for WebGL 2 and pkg-webgpu/ for
WebGPU, and the assets). A browser with WebGPU or WebGL 2 runs it; it is never installed.

PLAY IT
  With the game's own server (from the Linux, Windows or macOS package), which also hosts multiplayer, the map editor
  and sends the builds compressed (about 7 MB instead of 45):
      hotdog-server --root /path/to/this/folder
  (or start hotdog-server from inside this folder), then open
      http://localhost:8137/?play     to play against bots
      http://localhost:8137/?demo     to watch five bots play
      http://localhost:8137/?mp       multiplayer (the lobby)
      http://localhost:8137/editor/   the map editor
  Any static web server also serves single player and the demo (uncompressed, no multiplayer), for example
      python3 -m http.server 8000     then http://localhost:8000/?play

HOST A SERVER FOR OTHERS
  hotdog-server --root /path/to/this/folder --host 0.0.0.0
  Players open http://HOST:8137/?mp, where HOST is this machine's address on the network (allow TCP port 8137
  through the firewall). Native players join the same game with  hotdog-simulator --connect HOST:8137
  hotdog-server --help lists every option.

SETTINGS
  The browser keeps them in the site's local storage (and single player's replays in its IndexedDB).
EOF
  else
    cat <<EOF
START THE GAME
EOF
    case $platform in
      linux) cat <<EOF
  Run ./hotdog-simulator from a terminal, or double-click it in your file manager. It needs a graphical session (X11
  or Wayland via XWayland) and a GPU driver with Vulkan (or OpenGL ES 3), and the usual desktop libraries: glibc,
  ALSA (libasound2), libudev and libwayland-client, which every desktop distribution has.
EOF
        ;;
      windows) cat <<EOF
  Double-click hotdog-simulator.exe (Windows 10 or 11, 64-bit, with a DirectX 12 or Vulkan GPU driver). The
  programs are not code-signed: if SmartScreen says "Windows protected your PC", choose More info, then Run anyway.
EOF
        ;;
      mac) cat <<EOF
  Drag "Hotdog Simulator.app" to Applications (or anywhere) and open it. It runs on Apple Silicon and Intel Macs
  (macOS 11 or later). The app is not signed with an Apple Developer ID nor notarised, so the first time:
    - macOS 14 and earlier: right-click (or Control-click) the app, choose Open, then Open again.
    - macOS 15 and later: open it once, close the warning, then System Settings > Privacy & Security, scroll to
      "Hotdog Simulator was blocked", choose Open Anyway and confirm.
    - or, in Terminal:  xattr -dr com.apple.quarantine "/Applications/Hotdog Simulator.app"
EOF
        ;;
    esac
    if [ "$platform" != mac ]; then cat <<EOF

  $game (and $server) starts the build in bin that suits this processor: the one for x86-64-v3
  (AVX2, FMA and BMI2: Intel from 2013, AMD from 2015) when it has those, the one for x86-64-v2 otherwise. --cpu v2 or
  --cpu v3 (or HOTDOG_CPU=v2 in the environment) forces one, for trying things out; the game's log says which runs.
EOF
    fi
    cat <<EOF

  The breakfast intro plays first. The controls are in README.md ("Controls"). Options, given on the command
  line ($game ...):
    --skip-intro            straight into a round, you and four bots
    --demo                  watch the five bots play
    --connect HOST:8137     join a server (see below), with --name NAME and --color red|blue|green|yellow|black
    --map FILE              play a map from the map editor
    --replay FILE           watch a recording
    --no-record             do not record single player's rounds

HOST A SERVER
  The server is $server. From a terminal:
    $server                       this machine only, port 8137
    $server --host 0.0.0.0        everyone on the network (allow TCP port 8137 through the firewall)
  Players then join with
    $game --connect HOST:8137 --name Ann
  where HOST is the server's address on the network. The lobby's leader picks the mode and the map. The server
  records every match into replays/ beside the assets (--replays DIR puts them elsewhere, --replays-keep 0 records
  none); $server --help lists every option, and README.md ("Multiplayer") explains them.
  To let browser players join as well, unpack the web package and start the server with --root <that folder>:
  it then serves the browser game at http://HOST:8137/?mp to the same lobby.
EOF
    if [ "$platform" = mac ]; then cat <<EOF
  Inside the app the server's default replays folder is in the bundle: give it one of your own, for example
    $server --host 0.0.0.0 --replays ~/Documents/hotdog-replays
EOF
    fi
    cat <<EOF

WHERE SETTINGS LIVE
  The game keeps its settings in settings.json and single player's recordings in replays/, both in
    $config
  HOTDOG_SETTINGS=<file> in the environment uses another settings file; --replays-dir DIR another recordings folder.
  Deleting the folder resets everything.
EOF
  fi
  echo
  echo "LICENCE AND SOURCE"
  cat <<EOF
  Hotdog Simulator is free software: you can redistribute it and modify it under the terms of the GNU General Public
  License, version 3 only (the file LICENSE; COPYRIGHT says what that covers). It comes with no warranty.
  The source code of this version ($VERSION: the part after the + is its commit) is published at
      $(dist_source_url)
  with the scripts that build the programs and the browser game in this package. The libraries, fonts and other
  material in it that are not the project's own keep their licences: THIRD_PARTY_LICENSES.html lists every one with its
  full text, and THIRD_PARTY_NOTICES.md says what is not the project's own.
EOF
  if [ "$platform" = web ]; then
    echo "  The game's page links to THIRD_PARTY_LICENSES.html (the server serves it at /THIRD_PARTY_LICENSES.html)."
  elif [ "$platform" = mac ]; then
    echo "  The game's menu names the file (Contents/Resources/THIRD_PARTY_LICENSES.html)."
  else
    echo "  The game's menu names the file, which is beside the assets folder."
  fi
  echo
  echo "FILES"
  if [ "$platform" = web ]; then
    echo "  index.html, lobby.*      the page"
    echo "  pkg/, pkg-webgpu/        the WebGL 2 and the WebGPU builds"
    echo "  assets/                  the game's sounds, shaders, textures and maps"
    echo "  editor/                  the map editor"
  elif [ "$platform" = mac ]; then
    echo "  Hotdog Simulator.app     the game and the server (Contents/MacOS), the assets (Contents/Resources/assets)"
  else
    echo "  assets/                  the game's sounds, shaders, textures and maps (keep them beside the programs)"
    echo "  bin/                     the game and the server, each built for x86-64-v3 and for x86-64-v2"
  fi
  cat <<EOF
  README.md                the full manual
  LICENSE, COPYRIGHT       GPL-3.0-only, for the project's own work
  THIRD_PARTY_LICENSES.html  the licences of the libraries, fonts and other material that are not the project's own, in full
  THIRD_PARTY_NOTICES.md   what is not the project's own, and its terms (assets/CHARACTER_CREDITS.md: the intro's people)
EOF
}
