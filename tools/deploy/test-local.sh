#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# Deploys, end to end, on this machine, with no server and no GitHub (README.md, "Continuous integration and
# deployment"): a folder plays the server, and the game's server runs from it as a plain background process, not under
# systemd. Every step is tools/deploy/deploy.sh's own code, in its local mode.
#
#   tools/deploy/test-local.sh DIST WORK
#
# DIST holds the Linux package and the web package of one version (tools/build-all.sh linux web). WORK is a folder for
# this test alone: it is emptied first. From the environment: PORT (8172), the server's port, and PROXY_PORT (8173)
# and CADDY, a caddy binary: with it, Caddyfile.example (its site address and upstream changed to these ports) runs in
# front, and the checks go through it as they go through the real proxy. KEEP_RUNNING=1 leaves the server (and caddy)
# running at the end, their process IDs in WORK/server/service.pid and WORK/caddy.pid.
#
# In order, each checked:
#   1. a first deploy;
#   2. a second (the first is kept, as the one to go back to);
#   3. a release whose WebGPU module's precompressed copies are older than the module (so the server sends it plain):
#      it goes live, fails the checks from here, is rolled back to 2 and deleted;
#   4. a release whose server never answers: it fails the check on the server, is rolled back to 2 and deleted;
#   5. a release whose server does not run at all: refused before it goes live;
#   6. three more deploys keeping 3: the oldest releases are deleted, the replays folder is not;
#   7. rollback.sh to the release before, and --status.
# Exits 1 if any check failed.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
[ $# -eq 2 ] || { sed -n '8,10p' "$0" | sed 's/^# \{0,1\}//' >&2 && exit 2; }
dist=$(cd "$1" && pwd) work=$2
port=${PORT:-8172} proxy_port=${PROXY_PORT:-8173}
case $(cd "$(dirname "$work")" && pwd)/$(basename "$work") in
  / | "$HOME" | "$here"*) echo "not a folder to empty: $work" >&2 && exit 2 ;;
esac
linux=$(find "$dist" -maxdepth 1 -name 'hotdog-simulator-*-linux-x86_64.tar.gz' | sed -n 1p)
[ -n "$linux" ] || { echo "no Linux package in $dist" >&2 && exit 2; }
version=$(basename "$linux" | sed 's/^hotdog-simulator-\(.*\)-linux-x86_64\.tar\.gz$/\1/')
web=$dist/hotdog-simulator-$version-web.zip
[ -f "$web" ] || { echo "no $web" >&2 && exit 2; }

rm -rf "$work"
mkdir -p "$work/packages"
work=$(cd "$work" && pwd)
server=$work/server
failures=0
pass() { echo "PASS  $*"; }
flunk() {
  echo "FAIL  $*"
  failures=$((failures + 1))
}
expect() {
  local what=$1
  shift
  if "$@"; then pass "$what"; else flunk "$what"; fi
}
live() { basename "$(readlink "$server/current" 2>/dev/null)" 2>/dev/null || true; }
releases() { find "$server/releases" -mindepth 1 -maxdepth 1 -type d -not -name '.*' -printf '%f\n' 2>/dev/null | LC_ALL=C sort; }
count() { releases | wc -l | tr -d ' '; }
step() { echo && echo "######## $*"; }

url=http://127.0.0.1:$port
caddy_stop() { if [ -f "$work/caddy.pid" ]; then kill "$(cat "$work/caddy.pid")" 2>/dev/null || true; fi; }
server_stop() { if [ -f "$server/service.pid" ]; then kill "$(cat "$server/service.pid")" 2>/dev/null || true; fi; }
if [ "${KEEP_RUNNING:-}" != 1 ]; then trap 'server_stop; caddy_stop' EXIT; fi
if [ -n "${CADDY:-}" ]; then
  {
    printf '{\n\tadmin off\n\tauto_https off\n\tstorage file_system %s\n}\n\n' "$work/caddy-data"
    sed -e "s|^hotdog-simulator.chilos.dev {|http://127.0.0.1:$proxy_port {|" -e "s|127.0.0.1:8150|127.0.0.1:$port|" "$here/Caddyfile.example"
  } >"$work/Caddyfile"
  HOME=$work/caddy-home XDG_DATA_HOME=$work/caddy-data XDG_CONFIG_HOME=$work/caddy-config \
    "$CADDY" run --config "$work/Caddyfile" --adapter caddyfile >"$work/caddy.log" 2>&1 </dev/null &
  echo "$!" >"$work/caddy.pid"
  url=http://127.0.0.1:$proxy_port
  for _ in $(seq 40); do
    if curl -s -o /dev/null "$url/"; then break; fi
    sleep 0.25
  done
  echo "caddy (Caddyfile.example) in front, on $url"
fi
deploy() { "$here/deploy.sh" --url "$url" --service-port "$port" --keep "${KEEP:-5}" "$@"; }
fails() { ! "$@"; }

step "1. A first deploy of $version"
expect "the first deploy succeeds" deploy local "$server" "$dist"
r1=$(live)
expect "it is live: $r1" test -n "$r1"
expect "one release" test "$(count)" = 1
mkdir -p "$server/replays"
echo "a file in the replays folder, which no deploy may touch" >"$server/replays/keep-me.txt"

step "2. A second deploy"
sleep 1
expect "the second deploy succeeds" deploy local "$server" "$linux" "$web"
r2=$(live)
expect "it is live: $r2 (not $r1)" test -n "$r2" -a "$r2" != "$r1"
expect "two releases" test "$(count)" = 2

step "3. A release whose WebGPU module's compressed copies are stale: live, fails the checks, rolled back"
stale=$work/packages/stale
top=$(unzip -Z1 "$web" | sed -n 1p | cut -d/ -f1)
mkdir -p "$stale/src"
cp "$web" "$stale/"
ln "$linux" "$stale/" 2>/dev/null || cp "$linux" "$stale/"
(cd "$stale/src" && unzip -q "$web" "$top/pkg-webgpu/hotdog_simulator_bg.wasm.br" "$top/pkg-webgpu/hotdog_simulator_bg.wasm.gz" &&
  touch -d '2000-01-01' "$top"/pkg-webgpu/hotdog_simulator_bg.wasm.* && zip -q "$stale/$(basename "$web")" "$top"/pkg-webgpu/hotdog_simulator_bg.wasm.*)
expect "the deploy fails" fails deploy local "$server" "$stale/$(basename "$linux")" "$stale/$(basename "$web")"
expect "release 2 is live again: $(live)" test "$(live)" = "$r2"
expect "the server answers with release 2" test "$(curl -s "$url/assets/release.txt")" = "$r2"
expect "the failed release is gone (two releases)" test "$(count)" = 2

step "4. A release whose server never answers: rolled back on the server"
dead=$work/packages/dead
mkdir -p "$dead/src/hotdog-simulator-$version-linux-x86_64/bin"
tar -xzf "$linux" -C "$dead/src" "hotdog-simulator-$version-linux-x86_64/hotdog-server"
for level in v2 v3; do
  printf '%s\n' '#!/bin/sh' '# A server that starts and stops at once: it prints its usage, then gives up.' \
    "echo 'hotdog-server [--host 127.0.0.1] [--port 8137] (a broken build, for tools/deploy/test-local.sh)' >&2" 'exit 1' \
    >"$dead/src/hotdog-simulator-$version-linux-x86_64/bin/hotdog-server-$level"
  chmod +x "$dead/src/hotdog-simulator-$version-linux-x86_64/bin/hotdog-server-$level"
done
tar -C "$dead/src" -czf "$dead/$(basename "$linux")" "hotdog-simulator-$version-linux-x86_64"
ln "$web" "$dead/" 2>/dev/null || cp "$web" "$dead/"
expect "the deploy fails" fails deploy local "$server" "$dead/$(basename "$linux")" "$dead/$(basename "$web")"
expect "release 2 is live again: $(live)" test "$(live)" = "$r2"
expect "the server answers with release 2" test "$(curl -s "$url/assets/release.txt")" = "$r2"
expect "the failed release is gone (two releases)" test "$(count)" = 2

step "5. A release whose server does not run here: refused before it goes live"
for level in v2 v3; do
  printf '%s\n' '#!/bin/sh' "echo 'Illegal instruction' >&2" 'exit 132' >"$dead/src/hotdog-simulator-$version-linux-x86_64/bin/hotdog-server-$level"
done
tar -C "$dead/src" -czf "$dead/$(basename "$linux")" "hotdog-simulator-$version-linux-x86_64"
expect "the deploy fails" fails deploy local "$server" "$dead/$(basename "$linux")" "$dead/$(basename "$web")"
expect "release 2 stayed live" test "$(live)" = "$r2"
expect "nothing was added (two releases)" test "$(count)" = 2
expect "no deploy is left marked under way" test ! -e "$server/.deploying"

step "6. Three more deploys, keeping 3"
for n in 3 4 5; do
  sleep 1
  expect "deploy $n succeeds" env KEEP=3 "$here/deploy.sh" --url "$url" --service-port "$port" --keep 3 local "$server" "$dist"
done
r5=$(live)
expect "three releases: $(releases | tr '\n' ' ')" test "$(count)" = 3
expect "releases 1 and 2 were deleted" test ! -e "$server/releases/$r1" -a ! -e "$server/releases/$r2"
expect "the newest is live: $r5" test -d "$server/releases/$r5"
expect "the replays folder kept its file" test -f "$server/replays/keep-me.txt"

step "7. rollback.sh to the release before, then --status"
r4=$(releases | tail -n 2 | sed -n 1p)
expect "the rollback succeeds" "$here/rollback.sh" --url "$url" --service-port "$port" local "$server"
expect "release 4 is live: $(live)" test "$(live)" = "$r4"
expect "the server answers with release 4" test "$(curl -s "$url/assets/release.txt")" = "$r4"
expect "nothing was deleted (three releases)" test "$(count)" = 3
"$here/rollback.sh" --status local "$server"

echo
if [ "$failures" -eq 0 ]; then echo "ALL PASSED"; else echo "$failures FAILED"; fi
[ "$failures" -eq 0 ]
