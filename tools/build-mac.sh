#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# The macOS package (README.md, "Building and packaging"), for Apple Silicon and Intel Macs alike:
#   DIST/hotdog-simulator-VERSION-macos-universal.dmg   a disk image: the app, an Applications link and the documents
#   DIST/hotdog-simulator-VERSION-macos-universal.zip   the same folder as a zip (without the link)
# "Hotdog Simulator.app" holds the game (its executable) and the server in Contents/MacOS, each one universal binary
# (arm64 and x86_64, joined with lipo), and in Contents/Resources the assets (where the game and the server look for
# them inside an app: src/runtime.rs, crates/server/src/config.rs), the icon and the documents.
#
#   tools/build-mac.sh
#
# Runs on a Mac only: it links against Apple's macOS SDK (Xcode, or the Command Line Tools: xcode-select --install),
# which Apple licenses for use on Apple hardware. GitHub's macOS runners have it (.github/workflows/release.yml).
# Needs rustup's aarch64-apple-darwin and x86_64-apple-darwin targets (added here when rustup is there).
#
# Signing: by default the app is signed ad hoc (codesign -s -), which Apple Silicon requires to run at all; macOS
# Gatekeeper still warns about an app from the internet that is not signed with a Developer ID and notarised by Apple
# (HOW-TO-RUN.txt says how to open it anyway). With an Apple Developer Program membership:
#   MAC_SIGN_IDENTITY="Developer ID Application: NAME (TEAMID)"   signs with that certificate (in the keychain), with
#                                                                  the hardened runtime and a secure timestamp
#   MAC_NOTARY_PROFILE=PROFILE   then sends the disk image to Apple's notary service and staples the ticket to it
#                                (a profile stored once with: xcrun notarytool store-credentials PROFILE ...)
#
# The Intel half is compiled for x86-64-v2 (MAC_X86_CPU, as CPU in tools/build-linux.sh): every Intel Mac that runs
# macOS 11 has it. v3 (AVX2) would leave out the 2013 Mac Pro (Ivy Bridge), which macOS 11 and 12 still support; the
# Apple Silicon half always has NEON, the only SIMD it has.
#
# From the environment: TARGET_DIR (TARGET_PREFIXmac, target/dist-mac by default), DIST (dist), MAC_ARCHS ("aarch64
# x86_64"; one of them makes a single-architecture app), MAC_X86_CPU (v2), MACOSX_DEPLOYMENT_TARGET (11.0),
# MAC_BUNDLE_ID (from Cargo.toml's repository), FEATURES, HOTDOG_VERSION, MIN_FREE_GB, as tools/build-linux.sh.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ]; then
  echo "tools/build-mac.sh runs on macOS (it needs Apple's SDK); see .github/workflows/release.yml for building on GitHub's Mac runners" >&2
  exit 1
fi
. tools/dist-common.sh
TARGET_DIR=${TARGET_DIR:-${TARGET_PREFIX:-target/dist-}mac}
archs=${MAC_ARCHS:-aarch64 x86_64}
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
dist_cpu x86_64-apple-darwin "${MAC_X86_CPU:-v2}"
count=0
for arch in $archs; do count=$((count + 1)); done
if [ "$count" -gt 1 ]; then kind=universal; else kind=$archs; fi
name=hotdog-simulator-$FILE_VERSION-macos-$kind
stage=$TARGET_DIR/stage/$name
app="$stage/Hotdog Simulator.app"
features=()
if [ -n "${FEATURES:-}" ]; then features=(--features "$FEATURES"); fi
mkdir -p "$TARGET_DIR" "$DIST"
dist_check_disk "$TARGET_DIR"
dist_dir=$(cd "$DIST" && pwd)

# A bundle identifier from the repository's address (https://github.com/OWNER/REPO: io.github.OWNER.REPO).
repository=$(awk -F'"' '/^repository *=/ {print $2; exit}' Cargo.toml)
default_id=$(echo "$repository" | awk -F/ '$3 == "github.com" && NF >= 5 {print "io.github." $4 "." $5}' | tr -cd 'A-Za-z0-9.-')
bundle_id=${MAC_BUNDLE_ID:-${default_id:-org.hotdog-simulator.game}}

for arch in $archs; do
  if command -v rustup >/dev/null; then rustup target add "$arch-apple-darwin" >/dev/null; fi
  echo "== $VERSION: the game for $arch-apple-darwin (macOS $MACOSX_DEPLOYMENT_TARGET and later)"
  cargo build --release --target "$arch-apple-darwin" --target-dir "$TARGET_DIR" ${features[@]+"${features[@]}"}
  echo "== the server for $arch-apple-darwin"
  cargo build --release --target "$arch-apple-darwin" --target-dir "$TARGET_DIR" --manifest-path crates/server/Cargo.toml
done

rm -rf "$stage" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for program in hotdog-simulator hotdog-server; do
  thin=()
  for arch in $archs; do thin+=("$TARGET_DIR/$arch-apple-darwin/release/$program"); done
  lipo -create "${thin[@]}" -output "$app/Contents/MacOS/$program"
  strip -x "$app/Contents/MacOS/$program"
  lipo -info "$app/Contents/MacOS/$program"
done
cp tools/packaging/hotdog.icns "$app/Contents/Resources/"
dist_stage_assets "$app/Contents/Resources/assets" native
dist_stage_docs "$stage" mac
cp "$stage/README.md" "$stage/LICENSE" "$stage/COPYRIGHT" "$stage/THIRD_PARTY_NOTICES.md" "$stage/THIRD_PARTY_LICENSES.html" "$app/Contents/Resources/"
short=$(echo "$VERSION" | sed 's/[+-].*//')
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>Hotdog Simulator</string>
  <key>CFBundleExecutable</key><string>hotdog-simulator</string>
  <key>CFBundleIconFile</key><string>hotdog</string>
  <key>CFBundleIdentifier</key><string>$bundle_id</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>Hotdog Simulator</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$short</string>
  <key>CFBundleVersion</key><string>$short</string>
  <key>HotdogSimulatorBuild</key><string>$VERSION</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.action-games</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>Copyright (C) 2026 David Nunes-Childs. GPL-3.0-only.</string>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist"

# The server first (code inside the bundle is signed before the bundle), then the app.
if [ -n "${MAC_SIGN_IDENTITY:-}" ]; then
  sign=(codesign --force --options runtime --timestamp --sign "$MAC_SIGN_IDENTITY")
else
  sign=(codesign --force --sign -)
fi
"${sign[@]}" "$app/Contents/MacOS/hotdog-server"
"${sign[@]}" "$app"
codesign --verify --strict --verbose=2 "$app"

rm -f "$dist_dir/$name.zip" "$dist_dir/$name.dmg"
ditto -c -k --keepParent "$stage" "$dist_dir/$name.zip"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "Hotdog Simulator" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dist_dir/$name.dmg"
rm "$stage/Applications"
if [ -n "${MAC_SIGN_IDENTITY:-}" ]; then codesign --force --timestamp --sign "$MAC_SIGN_IDENTITY" "$dist_dir/$name.dmg"; fi
if [ -n "${MAC_NOTARY_PROFILE:-}" ]; then
  xcrun notarytool submit "$dist_dir/$name.dmg" --keychain-profile "$MAC_NOTARY_PROFILE" --wait
  xcrun stapler staple "$dist_dir/$name.dmg"
fi
echo "== $DIST/$name.dmg ($(du -h "$dist_dir/$name.dmg" | cut -f1)), $DIST/$name.zip ($(du -h "$dist_dir/$name.zip" | cut -f1))"
