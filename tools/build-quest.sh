#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# The Meta Quest app (README.md, "Virtual reality"): the game in virtual reality (src/vr.rs, the `vr` feature), for a
# Quest 2 and later, to be put on a headset in developer mode:
#   DIST/hotdog-simulator-VERSION-quest.apk
#   tools/build-quest.sh                  the app
#   tools/build-quest.sh install          and puts it on the headset that adb sees (adb install -r), and starts it
#
# In the app: lib/arm64-v8a/libquest.so (the game: the `quest` example of Cargo.toml, a shared library that Android's
# NativeActivity loads), libopenxr_loader.so (Khronos' OpenXR loader for Android, from Maven Central, checked against
# the SHA-256 below), libc++_shared.so (the NDK's, which both need), and assets/ (what the game loads).
#
# Needs:
#   the Rust target      rustup target add aarch64-linux-android
#   cargo-ndk            cargo install cargo-ndk
#   the Android SDK      in ANDROID_HOME (~/Android/Sdk unless set), with build-tools, a platform and the NDK:
#                          sdkmanager "platforms;android-34" "build-tools;34.0.0" "ndk;27.2.12479018"
#                        (sdkmanager is in Google's "command line tools"; it asks to accept Google's licences)
#   a key to sign with   ~/.android/debug.keystore, which the SDK's tools make (adb, or
#                          keytool -genkey -v -keystore ~/.android/debug.keystore -storepass android -alias
#                          androiddebugkey -keypass android -keyalg RSA -keysize 2048 -validity 10000
#                          -dname "CN=Android Debug,O=Android,C=US");
#                        QUEST_KEYSTORE, QUEST_KEY_ALIAS and QUEST_KEYSTORE_PASS name another
#   curl, unzip, zip
#
# From the environment: ANDROID_HOME, ANDROID_NDK_HOME (the newest in ANDROID_HOME/ndk), TARGET (target/dist-quest),
# DIST (dist), FEATURES (more cargo features), HOTDOG_VERSION.
set -euo pipefail
cd "$(dirname "$0")/.."
. tools/dist-common.sh
triple=aarch64-linux-android
target=${TARGET:-target/dist-quest}
name=hotdog-simulator-$FILE_VERSION-quest
stage=$target/stage
loader_version=1.1.47
loader_sha256=125d0537f99fc348c65bd84f67e43b482940aa0cb25c5e73a20f2827feb92677

sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}
newest() { ls -d "$@" 2>/dev/null | sort -V | tail -1 || true; }
ndk=${ANDROID_NDK_HOME:-$(newest "$sdk"/ndk/*)}
tools=$(newest "$sdk"/build-tools/*)
platform=$(newest "$sdk"/platforms/android-*)
missing=
[ -n "$ndk" ] && [ -d "$ndk" ] || missing="$missing the NDK (\"ndk;27.2.12479018\"),"
[ -n "$tools" ] && [ -x "$tools/aapt2" ] || missing="$missing build tools (\"build-tools;34.0.0\"),"
[ -n "$platform" ] && [ -f "$platform/android.jar" ] || missing="$missing a platform (\"platforms;android-34\"),"
if [ -n "$missing" ]; then
  echo "The Android SDK in $sdk lacks:$missing which sdkmanager installs (the top of this script says how)." >&2
  exit 1
fi
command -v cargo-ndk > /dev/null || { echo "cargo-ndk is missing: cargo install cargo-ndk" >&2; exit 1; }
rustup target list --installed 2>/dev/null | grep -qx "$triple" || { echo "The Rust target is missing: rustup target add $triple" >&2; exit 1; }
keystore=${QUEST_KEYSTORE:-$HOME/.android/debug.keystore}
[ -f "$keystore" ] || { echo "No key to sign with: $keystore (the top of this script says how to make one)" >&2; exit 1; }
dist_check_disk .

echo "== $name: the game for $triple"
features=vr${FEATURES:+,$FEATURES}
ANDROID_NDK_HOME=$ndk cargo ndk --target arm64-v8a --platform 29 build --release --example quest --features "$features" --target-dir "$target"

rm -rf "${stage:?}"
mkdir -p "$stage/lib/arm64-v8a" "$DIST"
cp "$target/$triple/release/examples/libquest.so" "$stage/lib/arm64-v8a/"
"$ndk"/toolchains/llvm/prebuilt/*/bin/llvm-strip --strip-debug "$stage/lib/arm64-v8a/libquest.so"
cp "$ndk"/toolchains/llvm/prebuilt/*/sysroot/usr/lib/$triple/libc++_shared.so "$stage/lib/arm64-v8a/"

# Khronos' loader: the one file of the app that is neither built here nor the NDK's.
aar=$target/openxr_loader_for_android-$loader_version.aar
if [ ! -f "$aar" ]; then
  curl -fsSL -o "$aar.part" "https://repo1.maven.org/maven2/org/khronos/openxr/openxr_loader_for_android/$loader_version/openxr_loader_for_android-$loader_version.aar"
  mv "$aar.part" "$aar"
fi
echo "$loader_sha256  $aar" | sha256sum -c - > /dev/null || { echo "$aar is not the file this script expects (its SHA-256 differs): delete it and run again" >&2; exit 1; }
unzip -p "$aar" jni/arm64-v8a/libopenxr_loader.so > "$stage/lib/arm64-v8a/libopenxr_loader.so"

dist_stage_assets "$stage/assets" native
cp LICENSE COPYRIGHT THIRD_PARTY_NOTICES.md THIRD_PARTY_LICENSES.html "$stage/assets/"

# Android's version code: one whole number that grows (the commits so far).
code=$(git rev-list --count HEAD 2>/dev/null || echo 1)
sed -e "s/@VERSION@/$VERSION/" -e "s/@CODE@/$code/" tools/quest/AndroidManifest.xml > "$stage/AndroidManifest.xml"
"$tools/aapt2" link -o "$stage/unsigned.apk" --manifest "$stage/AndroidManifest.xml" -I "$platform/android.jar" -A "$stage/assets"
(cd "$stage" && zip -q -r unsigned.apk lib)
"$tools/zipalign" -f -p 4 "$stage/unsigned.apk" "$stage/aligned.apk"
"$tools/apksigner" sign --ks "$keystore" --ks-key-alias "${QUEST_KEY_ALIAS:-androiddebugkey}" --ks-pass "pass:${QUEST_KEYSTORE_PASS:-android}" --out "$DIST/$name.apk" "$stage/aligned.apk"
rm -f "$DIST/$name.apk.idsig"
echo "== $DIST/$name.apk ($(du -h "$DIST/$name.apk" | cut -f1))"

if [ "${1:-}" = install ]; then
  adb=$sdk/platform-tools/adb
  [ -x "$adb" ] || adb=adb
  "$adb" install -r "$DIST/$name.apk"
  "$adb" shell am start -n dev.chilos.hotdogsimulator/android.app.NativeActivity
fi
