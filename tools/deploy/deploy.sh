#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# Puts a version of the game on its public server: what .github/workflows/deploy.yml runs after every merge to main,
# and what anyone with the deploy key can run by hand (README.md, "Continuous integration and deployment").
#
#   tools/deploy/deploy.sh [OPTIONS] TARGET DEPLOY_PATH PACKAGES...           deploy a version
#   tools/deploy/deploy.sh [OPTIONS] --rollback TARGET DEPLOY_PATH [RELEASE]  back to the release before (rollback.sh)
#   tools/deploy/deploy.sh [OPTIONS] --status TARGET DEPLOY_PATH              the releases there, and which is live
#
# TARGET is USER@HOST, reached over ssh, or `local`: DEPLOY_PATH is then a folder on this machine and the server runs
# as a plain background process on 127.0.0.1:8172, not under systemd, for trying all of this out without a server
# (tools/deploy/test-local.sh). PACKAGES are the Linux package and the web package of one version
# (hotdog-simulator-VERSION-linux-x86_64.tar.gz and hotdog-simulator-VERSION-web.zip, from
# `tools/build-all.sh linux web`), or a folder that holds them (dist/).
#
# A deploy
#   1. takes the server from the Linux package (its launcher, hotdog-server, and the builds it picks from,
#      bin/hotdog-server-v2 and -v3) and the whole web package, as one bundle, with web/assets/release.txt naming the
#      release (the health checks read it back);
#   2. uploads that to DEPLOY_PATH/incoming/ and unpacks it as DEPLOY_PATH/releases/RELEASE, where RELEASE is the UTC
#      time and the version, 20261119T183000Z-0.1.0-bf5e6ec (a version deployed again gets a folder of its own, and
#      the folders sort in the order they were deployed); checks what it holds, and that its server runs there;
#   3. points DEPLOY_PATH/current at it (a new link renamed over the old: atomic) and restarts the service, whose
#      process serves the release it started in until its next restart (hotdog-simulator.service);
#   4. checks it from the server (http://127.0.0.1:SERVICE_PORT answers with this release), then from here at
#      PUBLIC_URL: the page (200, HTML, no-cache), release.txt (this release), each build's script and module from
#      pkg/ and pkg-webgpu/ (application/wasm, no-cache, the brotli copy passed on byte for byte, the module whole when
#      asked uncompressed), /status, /replays, and a WebSocket upgrade (101) at /ws and at /lobby;
#   5. if any of that fails, points current back at the release that was live, restarts, checks that one, deletes
#      the failed release and stops with an error (exit 1); otherwise keeps the newest KEEP releases (always the live
#      one and the one before it) and deletes the rest. DEPLOY_PATH/replays, outside releases/, is never touched.
# Each step on the server holds a lock (DEPLOY_PATH/.lock), and DEPLOY_PATH/.deploying marks a deploy under way from
# its upload to its end, so that two deploys never interleave; DEPLOY_PATH/deploy.log keeps a line for each.
#
# Options, each overriding the environment variable after it:
#   --url URL            PUBLIC_URL            the address checked from here (https://hotdog-simulator.chilos.dev;
#                                              in local mode http://127.0.0.1:SERVICE_PORT); `none` checks only from
#                                              the server
#   --ssh-port N         DEPLOY_PORT           the server's SSH port (22)
#   --identity FILE      DEPLOY_IDENTITY       the SSH private key (ssh's own choice without it)
#   --known-hosts FILE   DEPLOY_KNOWN_HOSTS_FILE  known_hosts pinning the server's host key (~/.ssh/known_hosts
#                                              without it). The host key is always checked: StrictHostKeyChecking=yes
#   --keep N             DEPLOY_KEEP           releases kept (5; at least 2)
#   --scope S            DEPLOY_SCOPE          how the server runs: user (systemctl --user, the default), system
#                                              (a system service restarted with `sudo -n systemctl`: remote-setup.sh
#                                              --system) or process (local mode's background process, its default)
#   --service NAME       DEPLOY_SERVICE        the systemd unit (hotdog-simulator.service)
#   --service-port N     DEPLOY_SERVICE_PORT   the port it listens on at 127.0.0.1 (8150; local mode 8172)
#   --commit SHA         DEPLOY_COMMIT         the commit, written into the release's RELEASE file
# DEPLOY_NOTE, when set, is written there too (the workflow puts its run's address in it). TMPDIR holds the bundle
# while it is made (about twice the web package).
#
# Needs here: bash, curl, tar, gzip, unzip, sha256sum (or shasum), ssh; openssl to check the WebSocket handshake's
# answer (without it, only its 101). On the server: bash 4, GNU coreutils, tar, gzip, flock, curl and systemd
# (remote-setup.sh checks). Exit status: 0 deployed (or rolled back, or listed), 1 failed (after a rollback when one was
# possible), 2 a usage error.
# (Most functions are called by name from a variable, `"r_cmd_$command"`, or through `check`.)
# shellcheck disable=SC2329
set -euo pipefail

RELEASE_PATTERN='^[0-9]{8}T[0-9]{6}Z-[0-9A-Za-z._-]+$'
# What may be passed to the server on a command line, unquoted: no spaces and nothing a shell reads specially (a
# leading ~/ is meant to be expanded there).
SAFE_PATTERN='^[0-9A-Za-z@%+=:,./_~-]+$'

# --------------------------------------------------------------------------------------------------------------------
# The server's side: `deploy.sh --remote COMMAND DEPLOY_PATH ARGS...`, run there by bash reading this very file from
# ssh's standard input (or, in local mode, run here). Lines for people go to stderr, `key=value` answers to stdout.

r_say() { echo "  [server] $*" >&2; }
r_die() {
  r_say "ERROR: $*"
  exit 1
}

# DEPLOY_PATH as an absolute path, ~ expanded.
r_base() {
  local base=$1
  # shellcheck disable=SC2088 # (a literal ~, as it came)
  case $base in
    "~") base=$HOME ;;
    "~/"*) base=$HOME/${base#"~/"} ;;
  esac
  case $base in /*) ;; *) base=$PWD/$base ;; esac
  echo "${base%/}"
}

# The lock every step holds while it changes anything (released when this process ends).
r_lock() {
  mkdir -p "$base"
  exec 9>"$base/.lock"
  flock -w 120 9 || r_die "another deploy step has held $base/.lock for two minutes"
}

r_log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*" >>"$base/deploy.log"; }

# The live release's name (empty before the first deploy).
r_current() {
  local target
  target=$(readlink "$base/current" 2>/dev/null) || return 0
  basename "$target"
}

# Every release, oldest first.
r_releases() {
  local dir name
  for dir in "$base"/releases/*/; do
    name=$(basename "$dir")
    if [[ $name =~ $RELEASE_PATTERN ]]; then echo "$name"; fi
  done | LC_ALL=C sort
}

# The release deployed just before NAME (empty if none).
r_before() {
  local name before=
  while read -r name; do
    if [ "$name" = "$1" ]; then
      echo "$before"
      return
    fi
    before=$name
  done < <(r_releases)
}

# Points current at releases/NAME: a new link renamed over the old one, so that it is never missing.
r_point() {
  ln -sfn "releases/$1" "$base/.current.new"
  mv -Tf "$base/.current.new" "$base/current"
}

# The marker of a deploy under way: refuses while another one's is younger than an hour.
r_marker_check() {
  local other since age
  [ -f "$base/.deploying" ] || return 0
  read -r other since <"$base/.deploying" || true
  age=$(($(date +%s) - ${since:-0}))
  if [ "$age" -lt 3600 ]; then
    r_die "a deploy of $other has been under way for $age s (remove $base/.deploying if it is dead)"
  fi
  r_say "warning: a deploy of $other never finished ($age s ago); going on"
}
r_marker_take() {
  r_marker_check
  echo "$1 $(date +%s)" >"$base/.deploying"
}
r_marker_drop() { rm -f "$base/.deploying"; }

# The service: user, system or process (scope, service and port are set by the command).
r_systemctl() {
  if [ "$scope" = system ]; then sudo -n systemctl "$@"; else systemctl --user "$@"; fi
}
r_restart() {
  r_say "restarting the server ($scope)"
  restarted_at=$(date +%s)
  case $scope in
    process)
      r_process_stop
      r_process_start
      ;;
    *)
      # (A unit that hit its start limit would refuse a restart.)
      r_systemctl reset-failed "$service" 2>/dev/null || true
      r_systemctl restart "$service"
      ;;
  esac
}
r_alive() {
  case $scope in
    process) r_process_alive "$(cat "$base/service.pid" 2>/dev/null || true)" ;;
    user) systemctl --user is-active --quiet "$service" ;;
    system) systemctl is-active --quiet "$service" ;;
  esac
}
# What the server said since its last restart (its last 30 lines).
r_service_log() {
  r_say "the server's log since its restart:"
  case $scope in
    process) awk '/^== .* starting / { lines = "" } { lines = lines $0 "\n" } END { printf "%s", lines }' "$base/service.log" 2>/dev/null | tail -n 30 ;;
    user) journalctl --user -u "$service" --since "@${restarted_at:-0}" -n 30 --no-pager -o cat 2>/dev/null || systemctl --user status "$service" --no-pager -n 30 ;;
    system) journalctl -u "$service" --since "@${restarted_at:-0}" -n 30 --no-pager -o cat 2>/dev/null || systemctl status "$service" --no-pager -n 30 ;;
  esac | sed 's/^/    | /' >&2 || true
}

# Local mode's server: a background process in a session of its own, started in the live release (as the unit
# starts it), its process ID in DEPLOY_PATH/service.pid and its output in DEPLOY_PATH/service.log.
r_process_alive() {
  local pid=$1 state
  [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null || return 1
  state=$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null || echo Z)
  [ "$state" != Z ]
}
r_process_start() {
  mkdir -p "$base/replays"
  echo "== $(date -u +%Y-%m-%dT%H:%M:%SZ) starting $(r_current)" >>"$base/service.log"
  # (Not holding the lock's descriptor, 9, which would keep the lock for as long as the server runs.)
  (cd "$base/current" && exec setsid "$base/current/server/hotdog-server" --host 127.0.0.1 --port "$port" --root web --replays "$base/replays") \
    >>"$base/service.log" 2>&1 </dev/null 9>&- &
  echo "$!" >"$base/service.pid"
}
r_process_stop() {
  local pid i command=
  pid=$(cat "$base/service.pid" 2>/dev/null || true)
  if r_process_alive "$pid"; then command=$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null || true); fi
  # Only this deploy path's own server: a process ID can be reused.
  if [[ $command == *"--replays $base/replays"* ]]; then
    kill -TERM "$pid" 2>/dev/null || true
    for i in $(seq 40); do
      r_process_alive "$pid" || break
      sleep 0.25
    done
    if r_process_alive "$pid"; then kill -KILL "$pid" 2>/dev/null || true; fi
  fi
  rm -f "$base/service.pid"
}

# Waits up to 30 s for the server on 127.0.0.1:port to answer with release NAME.
r_serving() {
  local name=$1 i answer=
  for i in $(seq 60); do
    answer=$(curl -fsS --max-time 2 "http://127.0.0.1:$port/assets/release.txt" 2>/dev/null || true)
    if [ "$answer" = "$name" ]; then
      r_say "the server answers on 127.0.0.1:$port with $name"
      return 0
    fi
    if [ "$i" -gt 6 ] && ! r_alive; then
      r_say "the server is not running"
      break
    fi
    sleep 0.5
  done
  r_say "no answer with $name from 127.0.0.1:$port${answer:+ (it says $answer)}"
  return 1
}

# Back to release NAME after FAILED: current, restart, check. Returns whether NAME answers.
r_back_to() {
  local name=$1 failed=$2
  r_say "rolling back from $failed to $name"
  r_point "$name"
  r_restart || true
  if r_serving "$name"; then
    r_log "rolled back from $failed to $name"
    return 0
  fi
  r_service_log
  r_log "rolled back from $failed to $name, which does not answer either"
  return 1
}

# The release's layout, before it goes live.
r_check_release() {
  local dir=$1 file help
  for file in server/hotdog-server web/index.html web/assets/release.txt web/pkg/hotdog_simulator.js \
    web/pkg/hotdog_simulator_bg.wasm web/pkg-webgpu/hotdog_simulator.js web/pkg-webgpu/hotdog_simulator_bg.wasm; do
    [ -s "$dir/$file" ] || r_die "the release has no $file"
  done
  ls "$dir"/web/assets/maps/*.json >/dev/null 2>&1 || r_die "the release has no maps (web/assets/maps)"
  [ -x "$dir/server/hotdog-server" ] || r_die "server/hotdog-server is not executable"
  # It runs on this machine (its glibc, its processor): its usage, which --help prints (and exits 1).
  help=$("$dir/server/hotdog-server" --help 2>&1 || true)
  case $help in
    *"hotdog-server [--host"*) ;;
    *) r_die "the release's server does not run here: $(echo "$help" | tail -n 3)" ;;
  esac
}

# info DEPLOY_PATH SCOPE SERVICE: the absolute path and the live release; refuses if a deploy is under way or the
# service is not installed.
r_cmd_info() {
  base=$(r_base "$1") scope=$2 service=$3
  local tool missing=()
  for tool in tar gzip sha256sum flock curl readlink mv; do command -v "$tool" >/dev/null || missing+=("$tool"); done
  [ "$scope" = process ] || command -v systemctl >/dev/null || missing+=(systemctl)
  [ ${#missing[@]} -eq 0 ] || r_die "missing on the server: ${missing[*]}"
  mkdir -p "$base/releases" "$base/incoming" "$base/replays"
  case $scope in
    user) systemctl --user cat "$service" >/dev/null 2>&1 || r_die "no user unit $service: run tools/deploy/remote-setup.sh on the server first" ;;
    system) systemctl cat "$service" >/dev/null 2>&1 || r_die "no system unit $service: install it as remote-setup.sh --system says" ;;
  esac
  r_marker_check
  echo "base=$base"
  echo "current=$(r_current)"
}

# activate DEPLOY_PATH NAME SHA256 SCOPE SERVICE PORT: unpacks incoming/NAME.tar.gz, checks it, makes it live and
# checks it from here. On failure there, goes back to the release before and exits 3.
r_cmd_activate() {
  base=$(r_base "$1") scope=$4 service=$5 port=$6
  local name=$2 sum=$3 upload partial release previous
  [[ $name =~ $RELEASE_PATTERN ]] || r_die "not a release name: $name"
  r_lock
  r_marker_take "$name"
  # Until the release is live and answers, any way out of here ends the deploy (the marker stays only when it goes on).
  trap r_marker_drop EXIT
  upload=$base/incoming/$name.tar.gz partial=$base/releases/.$name.partial release=$base/releases/$name
  [ -f "$upload" ] || r_die "no upload at $upload"
  echo "$sum  $upload" | sha256sum -c --quiet - >/dev/null 2>&1 || r_die "the upload is damaged (its SHA-256 differs)"
  [ ! -e "$release" ] || r_die "$release exists already"
  rm -rf "$partial"
  mkdir -p "$partial"
  tar -xzf "$upload" -C "$partial"
  (r_check_release "$partial") || {
    rm -rf "$partial" "$upload"
    r_log "refused $name: its contents"
    exit 1
  }
  mv "$partial" "$release"
  rm -f "$upload"
  previous=$(r_current)
  r_point "$name"
  r_say "current -> releases/$name (was ${previous:-nothing})"
  r_log "live: $name (was ${previous:-nothing})"
  if r_restart && r_serving "$name"; then
    trap - EXIT
    echo "previous=$previous"
    return 0
  fi
  r_service_log
  if [ -n "$previous" ] && [ -d "$base/releases/$previous" ]; then
    r_back_to "$previous" "$name" || true
    rm -rf "$release"
    r_say "deleted the failed release $name"
  else
    r_say "there is no earlier release to go back to: $name stays where it is"
  fi
  exit 3
}

# revert DEPLOY_PATH FAILED PREVIOUS SCOPE SERVICE PORT: after a failed public check, back to PREVIOUS; deletes FAILED.
r_cmd_revert() {
  base=$(r_base "$1") scope=$4 service=$5 port=$6
  local failed=$2 previous=$3 status=0
  [[ $failed =~ $RELEASE_PATTERN && $previous =~ $RELEASE_PATTERN ]] || r_die "not release names: $failed, $previous"
  [ -d "$base/releases/$previous" ] || r_die "no release $previous to go back to"
  r_lock
  r_back_to "$previous" "$failed" || status=1
  if [ "$(r_current)" = "$previous" ] && [ "$failed" != "$previous" ]; then
    rm -rf "${base:?}/releases/$failed"
    r_say "deleted the failed release $failed"
  fi
  r_marker_drop
  return "$status"
}

# finish DEPLOY_PATH NAME KEEP: the deploy of NAME passed its checks; keeps the newest KEEP releases.
r_cmd_finish() {
  base=$(r_base "$1")
  local name=$2 keep=$3 live before all count excess release
  r_lock
  live=$(r_current)
  [ "$live" = "$name" ] || r_die "$name is not live ($live is)"
  [ "$keep" -ge 2 ] 2>/dev/null || keep=2
  before=$(r_before "$live")
  mapfile -t all < <(r_releases)
  count=${#all[@]}
  excess=$((count - keep))
  for release in "${all[@]}"; do
    [ "$excess" -gt 0 ] || break
    if [ "$release" = "$live" ] || [ "$release" = "$before" ]; then continue; fi
    rm -rf "${base:?}/releases/$release"
    r_say "deleted the old release $release"
    r_log "deleted $release"
    excess=$((excess - 1))
  done
  # What failed uploads and unpacks left behind.
  rm -rf "$base"/releases/.*.partial
  find "$base/incoming" -maxdepth 1 -type f -mmin +60 -delete 2>/dev/null || true
  r_marker_drop
  r_log "deployed $name (keeping $keep)"
  r_say "$name is live; $(r_releases | wc -l) releases kept"
}

# rollback DEPLOY_PATH RELEASE|- SCOPE SERVICE PORT: back to RELEASE, or to the one deployed before the live one.
r_cmd_rollback() {
  base=$(r_base "$1") scope=$3 service=$4 port=$5
  local name=$2 live
  r_lock
  r_marker_check
  live=$(r_current)
  [ -n "$live" ] || r_die "nothing is deployed in $base"
  if [ "$name" = - ]; then
    name=$(r_before "$live")
    [ -n "$name" ] || r_die "there is no release older than $live"
  fi
  [[ $name =~ $RELEASE_PATTERN ]] && [ -d "$base/releases/$name" ] || r_die "no release $name (deploy.sh --status lists them)"
  [ "$name" != "$live" ] || r_die "$name is already live"
  if r_back_to "$name" "$live"; then
    echo "release=$name"
    echo "previous=$live"
    return 0
  fi
  r_say "$name does not answer: back to $live"
  r_back_to "$live" "$name" || true
  r_die "the rollback to $name failed; $live is live again"
}

# status DEPLOY_PATH SCOPE SERVICE: the releases, which is live, and the service.
r_cmd_status() {
  base=$(r_base "$1") scope=$2 service=$3
  local live release mark
  live=$(r_current)
  echo "Deploy path: $base"
  echo "Releases (oldest first; * is live):"
  while read -r release; do
    mark=" "
    [ "$release" != "$live" ] || mark="*"
    echo "  $mark $release   $(grep -h '^commit=' "$base/releases/$release/RELEASE" 2>/dev/null | cut -c1-19)"
  done < <(r_releases)
  [ -n "$live" ] || echo "  (none live)"
  [ ! -f "$base/.deploying" ] || echo "Under way: $(cat "$base/.deploying")"
  echo "Replays: $(find "$base/replays" -maxdepth 1 -type f 2>/dev/null | wc -l) files in $base/replays"
  case $scope in
    process) if r_alive; then echo "Server: running (process $(cat "$base/service.pid"))"; else echo "Server: not running"; fi ;;
    *) echo "Server: $(r_systemctl is-active "$service" 2>/dev/null || true) ($scope unit $service)" ;;
  esac
  echo "Last deploys:"
  tail -n 8 "$base/deploy.log" 2>/dev/null | sed 's/^/  /' || true
}

remote_main() {
  local command=$1
  shift
  # A user service is reached through the user's own manager, whose socket is in XDG_RUNTIME_DIR.
  export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
  case $command in
    info | activate | revert | finish | rollback | status) "r_cmd_$command" "$@" ;;
    *) r_die "unknown command $command" ;;
  esac
}

# --------------------------------------------------------------------------------------------------------------------
# This side.

SELF=$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)/$(basename "${BASH_SOURCE[0]:-$0}")
in_actions() { [ "${GITHUB_ACTIONS:-}" = true ]; }
say() { echo "$*" >&2; }
group() { if in_actions; then echo "::group::$*" >&2; else echo "== $*" >&2; fi; }
endgroup() { if in_actions; then echo "::endgroup::" >&2; fi; }
fail() {
  local message=$*
  if in_actions; then echo "::error title=Deploy failed::$message" >&2; fi
  {
    echo
    echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
    echo "!! DEPLOY FAILED: $message"
    echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
  } >&2
  summary "failed: $message"
  exit 1
}
usage() {
  sed -n '8,10p' "$SELF" | sed 's/^# \{0,1\}//' >&2
  echo "(the comment at the top of $SELF has the rest)" >&2
  exit 2
}
# A line of the run's summary page, in a workflow.
summary() {
  if in_actions && [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then echo "- $*" >>"$GITHUB_STEP_SUMMARY"; fi
}

# Runs a command of the server's side: over ssh (bash reading this file), or here in local mode.
remote() {
  if [ "$target" = local ]; then
    bash "$SELF" --remote "$@"
  else
    ssh "${ssh_options[@]}" "$target" bash -s -- --remote "$@" <"$SELF"
  fi
}

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | awk '{print $1}'; }

# The two packages of one version: from the arguments, or the only version in a folder.
find_packages() {
  local item version
  linux_package='' web_package=''
  for item in "$@"; do
    if [ -d "$item" ]; then
      local found=()
      while IFS= read -r version; do found+=("$version"); done < <(
        find "$item" -maxdepth 1 -name 'hotdog-simulator-*-linux-x86_64.tar.gz' | sed 's/.*hotdog-simulator-\(.*\)-linux-x86_64\.tar\.gz$/\1/' | sort -u
      )
      [ ${#found[@]} -eq 1 ] || fail "$item holds ${#found[@]} Linux packages (${found[*]:-none}): name the two packages instead"
      linux_package=$item/hotdog-simulator-${found[0]}-linux-x86_64.tar.gz
      web_package=$item/hotdog-simulator-${found[0]}-web.zip
    else
      case $item in
        *-linux-x86_64.tar.gz) linux_package=$item ;;
        *-web.zip) web_package=$item ;;
        *) fail "not a package this deploys: $item (a -linux-x86_64.tar.gz and a -web.zip)" ;;
      esac
    fi
  done
  [ -f "$linux_package" ] || fail "no Linux package (${linux_package:-none given})"
  [ -f "$web_package" ] || fail "no web package (${web_package:-none given})"
  version=$(basename "$linux_package" | sed 's/^hotdog-simulator-\(.*\)-linux-x86_64\.tar\.gz$/\1/')
  [ "$(basename "$web_package")" = "hotdog-simulator-$version-web.zip" ] || fail "the packages are of different versions: $linux_package, $web_package"
  [[ $version =~ ^[0-9A-Za-z._-]+$ ]] || fail "a version this cannot name a release by: $version"
  package_version=$version
}

# The bundle: server/ (the launcher and its builds) and web/ (the web package), with RELEASE and
# web/assets/release.txt; checks the packages hold what the server needs.
make_bundle() {
  local stage=$work/release top members=() member
  mkdir -p "$stage/server" "$work/web"
  # (Listed once, into a file: `tar | head` could end tar with SIGPIPE, which pipefail would take for a failure.)
  tar -tzf "$linux_package" >"$work/linux.list"
  top=$(head -n 1 "$work/linux.list" | cut -d/ -f1)
  while IFS= read -r member; do
    case $member in "$top/hotdog-server" | "$top"/bin/hotdog-server-*) members+=("$member") ;; esac
  done <"$work/linux.list"
  [ ${#members[@]} -gt 0 ] || fail "$linux_package has no hotdog-server"
  tar -xzf "$linux_package" -C "$work" "${members[@]}"
  mv "$work/$top"/* "$stage/server/"
  [ -x "$stage/server/hotdog-server" ] || fail "$linux_package has no hotdog-server at its top"
  if [ -d "$stage/server/bin" ]; then
    [ -x "$stage/server/bin/hotdog-server-v2" ] && [ -x "$stage/server/bin/hotdog-server-v3" ] || fail "$linux_package lacks bin/hotdog-server-v2 or -v3"
  fi
  unzip -q "$web_package" -d "$work/web"
  [ "$(find "$work/web" -mindepth 1 -maxdepth 1 | wc -l)" -eq 1 ] || fail "$web_package is not one folder"
  mv "$work"/web/*/ "$stage/web"
  for member in index.html pkg/hotdog_simulator.js pkg/hotdog_simulator_bg.wasm pkg/hotdog_simulator_bg.wasm.br \
    pkg-webgpu/hotdog_simulator.js pkg-webgpu/hotdog_simulator_bg.wasm pkg-webgpu/hotdog_simulator_bg.wasm.br; do
    [ -s "$stage/web/$member" ] || fail "$web_package has no $member"
  done
  echo "$release" >"$stage/web/assets/release.txt"
  {
    echo "release=$release"
    echo "version=${package_version/-/+}"
    echo "commit=${commit:-unknown}"
    echo "packages=$(basename "$linux_package") $(basename "$web_package")"
    echo "deployed_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "deployed_by=$(id -un)@$(hostname)"
    if [ -n "${DEPLOY_NOTE:-}" ]; then echo "note=$DEPLOY_NOTE"; fi
  } >"$stage/RELEASE"
  tar -C "$stage" -czf "$work/bundle.tar.gz" .
  bundle_sum=$(sha256 "$work/bundle.tar.gz")
  say "bundle: $(du -h "$work/bundle.tar.gz" | cut -f1), server $(du -sh "$stage/server" | cut -f1), web $(du -sh "$stage/web" | cut -f1)"
}

# --- The checks from here, at the public address.

# fetch URL [curl options]: the answer's status in $code, its headers in $work/h, its body in $work/b.
fetch() {
  local url=$1
  shift
  rm -f "$work/h" "$work/b"
  code=$(curl -sS --max-time 60 -D "$work/h" -o "$work/b" -w '%{http_code}' "$@" "$url" 2>"$work/e") || code=000
}
# A header of the last answer (its name in lower case), or nothing.
header() { tr -d '\r' <"$work/h" 2>/dev/null | awk -v name="$1" 'tolower($0) ~ "^" name ":" { sub(/^[^:]*:[ \t]*/, ""); value = $0 } END { print value }'; }
# check DESCRIPTION CONDITION...: one line, ok or FAIL; counts the failures.
check() {
  local what=$1
  shift
  if "$@"; then
    say "  ok    $what"
  else
    say "  FAIL  $what${detail:+: $detail}"
    failures=$((failures + 1))
  fi
  detail=
}
is() { [ "$1" = "$2" ] || { detail="got '${1:-nothing}', expected '$2'"; return 1; }; }
has() { case $1 in *"$2"*) return 0 ;; *) detail="got '${1:-nothing}', expected it to contain '$2'"; return 1 ;; esac; }

# A WebSocket handshake at URL: is the answer 101, with the Sec-WebSocket-Accept the key calls for?
websocket() {
  local url=$1 key expected status accept
  key=$(head -c 16 /dev/urandom | base64)
  rm -f "$work/wh"
  # (The upgraded connection stays open: curl gives up after its time, having written the answer's headers.)
  curl -sS --http1.1 --max-time 4 -o /dev/null -D "$work/wh" -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
    -H 'Sec-WebSocket-Version: 13' -H "Sec-WebSocket-Key: $key" "$url" 2>/dev/null || true
  status=$(head -n 1 "$work/wh" 2>/dev/null | tr -d '\r' | awk '{print $2}')
  [ "$status" = 101 ] || { detail="got HTTP ${status:-nothing}, expected 101"; return 1; }
  if command -v openssl >/dev/null; then
    expected=$(printf '%s' "${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11" | openssl dgst -sha1 -binary | base64)
    accept=$(tr -d '\r' <"$work/wh" | awk 'tolower($0) ~ /^sec-websocket-accept:/ {print $2}')
    [ "$accept" = "$expected" ] || { detail="Sec-WebSocket-Accept '$accept', expected '$expected'"; return 1; }
  fi
}

# public_checks URL RELEASE [WEB_DIR]: every check of the public address; returns whether all passed. Without WEB_DIR
# (the release's own files), the builds are not compared byte for byte.
public_checks() {
  local url=${1%/} name=$2 web=${3:-} pkg i answer
  failures=0 detail=
  group "Checks at $url"
  # The release first, given a little time: the proxy in front may be a moment behind.
  for i in $(seq 10); do
    fetch "$url/assets/release.txt"
    answer=$(cat "$work/b" 2>/dev/null)
    if [ "$code" = 200 ] && [ "$answer" = "$name" ]; then break; fi
    sleep 3
  done
  check "the release at $url/assets/release.txt is $name" is "$code $answer" "200 $name"
  # (--compressed: the proxy may compress the page itself, and curl then decodes it.)
  fetch "$url/" --compressed
  check "the page: 200" is "$code" 200
  check "the page is HTML" has "$(header content-type)" text/html
  check "the page is the game's" grep -q '<title>Hotdog Simulator</title>' "$work/b"
  check "the page says no-cache (a deploy is picked up at the next load)" has "$(header cache-control)" no-cache
  for pkg in pkg pkg-webgpu; do
    fetch "$url/$pkg/hotdog_simulator.js" -H 'Accept-Encoding: br, gzip'
    check "$pkg/hotdog_simulator.js: 200, JavaScript" is "$code $(header content-type)" "200 text/javascript; charset=utf-8"
    fetch "$url/$pkg/hotdog_simulator_bg.wasm" -H 'Accept-Encoding: br, gzip'
    check "$pkg/hotdog_simulator_bg.wasm: 200, application/wasm" is "$code $(header content-type)" "200 application/wasm"
    check "$pkg/hotdog_simulator_bg.wasm: sent brotli-compressed, once" is "$(header content-encoding)" br
    check "$pkg/hotdog_simulator_bg.wasm: no-cache" has "$(header cache-control)" no-cache
    if [ -n "$web" ]; then
      check "$pkg/hotdog_simulator_bg.wasm: the .br copy byte for byte ($(wc -c <"$web/$pkg/hotdog_simulator_bg.wasm.br" | tr -d ' ') bytes)" \
        cmp -s "$work/b" "$web/$pkg/hotdog_simulator_bg.wasm.br"
      fetch "$url/$pkg/hotdog_simulator_bg.wasm" -I -H 'Accept-Encoding: identity'
      check "$pkg/hotdog_simulator_bg.wasm uncompressed: 200, the whole module, no encoding" \
        is "$code $(header content-length) $(header content-encoding)" "200 $(wc -c <"$web/$pkg/hotdog_simulator_bg.wasm" | tr -d ' ') "
    fi
  done
  fetch "$url/status"
  check "/status: 200, the server's status" is "$code $(grep -m 1 -o '"protocol"' "$work/b")" '200 "protocol"'
  fetch "$url/replays"
  check "/replays: 200, the list of replays" is "$code $(grep -m 1 -o '"replays"' "$work/b")" '200 "replays"'
  check "a WebSocket at /ws upgrades (101)" websocket "$url/ws"
  check "a WebSocket at /lobby upgrades (101)" websocket "$url/lobby"
  endgroup
  [ "$failures" -eq 0 ]
}

# --------------------------------------------------------------------------------------------------------------------

main() {
  if [ "${1:-}" = --remote ]; then
    shift
    remote_main "$@"
    return
  fi
  local mode=deploy positional=() arg
  url=${PUBLIC_URL:-} ssh_port=${DEPLOY_PORT:-22} identity=${DEPLOY_IDENTITY:-} known_hosts=${DEPLOY_KNOWN_HOSTS_FILE:-}
  keep=${DEPLOY_KEEP:-5} scope=${DEPLOY_SCOPE:-} service=${DEPLOY_SERVICE:-hotdog-simulator.service}
  port=${DEPLOY_SERVICE_PORT:-} commit=${DEPLOY_COMMIT:-}
  while [ $# -gt 0 ]; do
    arg=$1
    case $arg in
      --rollback | --status) mode=${arg#--} ;;
      --url | --ssh-port | --identity | --known-hosts | --keep | --scope | --service | --service-port | --commit)
        [ $# -ge 2 ] || usage
        case $arg in
          --url) url=$2 ;;
          --ssh-port) ssh_port=$2 ;;
          --identity) identity=$2 ;;
          --known-hosts) known_hosts=$2 ;;
          --keep) keep=$2 ;;
          --scope) scope=$2 ;;
          --service) service=$2 ;;
          --service-port) port=$2 ;;
          --commit) commit=$2 ;;
        esac
        shift
        ;;
      -h | --help) usage ;;
      -*) say "unknown option $arg" && usage ;;
      *) positional+=("$arg") ;;
    esac
    shift
  done
  [ ${#positional[@]} -ge 2 ] || usage
  target=${positional[0]} base=${positional[1]}
  positional=("${positional[@]:2}")

  if [ "$target" = local ]; then
    mkdir -p "$base"
    base=$(cd "$base" && pwd)
    scope=${scope:-process} port=${port:-8172}
    url=${url:-http://127.0.0.1:$port}
  else
    scope=${scope:-user} port=${port:-8150}
    url=${url:-https://hotdog-simulator.chilos.dev}
    [[ $target =~ ^[A-Za-z0-9._-]+@[A-Za-z0-9.:_-]+$ || $target =~ ^[A-Za-z0-9.:_-]+$ ]] || fail "not a USER@HOST: $target"
  fi
  case $scope in user | system | process) ;; *) fail "--scope takes user, system or process, not $scope" ;; esac
  [[ $ssh_port =~ ^[0-9]+$ && $port =~ ^[0-9]+$ && $keep =~ ^[0-9]+$ ]] || fail "--ssh-port, --service-port and --keep take numbers"
  [[ $base =~ $SAFE_PATTERN ]] || fail "DEPLOY_PATH may hold letters, digits and . _ - / ~ only: $base"
  [[ $service =~ ^[A-Za-z0-9@._-]+$ ]] || fail "not a unit name: $service"
  [ -z "$commit" ] || [[ $commit =~ ^[0-9a-f]{7,40}$ ]] || fail "not a commit: $commit"

  ssh_options=(-p "$ssh_port" -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=20
    -o ServerAliveInterval=15 -o ServerAliveCountMax=4 -o LogLevel=ERROR)
  if [ -n "$identity" ]; then ssh_options+=(-i "$identity" -o IdentitiesOnly=yes); fi
  if [ -n "$known_hosts" ]; then
    ssh_options+=(-o "UserKnownHostsFile=$known_hosts" -o GlobalKnownHostsFile=/dev/null)
    local host=${target#*@}
    [ "$ssh_port" = 22 ] || host="[$host]:$ssh_port"
    if [ "$target" != local ] && ! ssh-keygen -F "$host" -f "$known_hosts" >/dev/null 2>&1; then
      fail "$known_hosts has no key for $host (ssh-keyscan -p $ssh_port ${target#*@} makes the line)"
    fi
  fi

  work=$(mktemp -d "${TMPDIR:-/tmp}/hotdog-deploy.XXXXXX")
  trap 'rm -rf "$work"' EXIT

  case $mode in
    status)
      remote status "$base" "$scope" "$service"
      return
      ;;
    rollback)
      rollback "${positional[0]:--}"
      return
      ;;
  esac
  [ ${#positional[@]} -gt 0 ] || usage
  deploy "${positional[@]}"
}

deploy() {
  local info abs live answer previous status=0
  find_packages "$@"
  release=$(date -u +%Y%m%dT%H%M%SZ)-$package_version
  group "Bundle $release from $(basename "$linux_package") and $(basename "$web_package")"
  make_bundle
  endgroup

  group "Upload to $target:$base"
  info=$(remote info "$base" "$scope" "$service") || fail "the server is not ready for a deploy (see above)"
  abs=$(sed -n 's/^base=//p' <<<"$info")
  live=$(sed -n 's/^current=//p' <<<"$info")
  [[ $abs =~ $SAFE_PATTERN ]] || fail "the server's DEPLOY_PATH is $abs"
  say "live now: ${live:-nothing}"
  if [ "$target" = local ]; then
    cp "$work/bundle.tar.gz" "$abs/incoming/$release.tar.gz"
  else
    # shellcheck disable=SC2029 # (expanded here on purpose: a checked path)
    ssh "${ssh_options[@]}" "$target" "cat > $abs/incoming/$release.tar.gz" <"$work/bundle.tar.gz" || fail "the upload failed"
  fi
  endgroup

  group "Activate $release"
  answer=$(remote activate "$abs" "$release" "$bundle_sum" "$scope" "$service" "$port") || status=$?
  endgroup
  case $status in
    0) ;;
    3) fail "$release did not answer on the server; it was rolled back${live:+ to $live} and deleted" ;;
    *) fail "$release could not be put live (see above); ${live:-nothing} is still live" ;;
  esac
  previous=$(sed -n 's/^previous=//p' <<<"$answer")

  if [ "$url" = none ]; then
    say "(PUBLIC_URL is none: no checks from here)"
  elif ! public_checks "$url" "$release" "$work/release/web"; then
    if [ -z "$previous" ]; then
      fail "$release failed the checks at $url, and there is no earlier release to go back to: it stays live"
    fi
    group "Roll back to $previous"
    remote revert "$abs" "$release" "$previous" "$scope" "$service" "$port" || say "(the server could not bring $previous back cleanly: see above)"
    endgroup
    if public_checks "$url" "$previous"; then
      say "$previous answers at $url again"
    else
      say "!! $previous does not pass the checks at $url either: the fault may be in front of the server (the proxy)"
    fi
    fail "$release failed the checks at $url: rolled back to $previous, and $release deleted"
  fi

  group "Keep the newest $keep releases"
  remote finish "$abs" "$release" "$keep" || fail "$release is live and checked, but cleaning up failed"
  endgroup
  say "Deployed $release to $target:$abs${url:+ ($url)}"
  summary "deployed **$release** (commit ${commit:-unknown}) to $url; the release before: ${previous:-none}"
}

rollback() {
  local name=$1 answer now was
  [ "$name" = - ] || [[ $name =~ $RELEASE_PATTERN ]] || fail "not a release name: $name"
  group "Roll back on $target:$base"
  answer=$(remote rollback "$base" "$name" "$scope" "$service" "$port") || fail "the rollback failed (see above)"
  endgroup
  now=$(sed -n 's/^release=//p' <<<"$answer")
  was=$(sed -n 's/^previous=//p' <<<"$answer")
  if [ "$url" != none ]; then
    for _ in $(seq 10); do
      fetch "$url/assets/release.txt"
      [ "$code $(cat "$work/b")" != "200 $now" ] || break
      sleep 3
    done
    [ "$code $(cat "$work/b")" = "200 $now" ] || fail "rolled back to $now on the server, but $url answers with '$(cat "$work/b")' ($code)"
    say "  ok    $url serves $now"
  fi
  say "Rolled back from $was to $now"
  summary "rolled back from $was to **$now**"
}

main "$@"
exit
