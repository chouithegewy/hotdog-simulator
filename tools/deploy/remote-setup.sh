#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# The server's one-time setup for deploys (README.md, "Continuous integration and deployment"). Run it once on the
# server, as the unprivileged account that runs the game and that the deploy key logs in as (never as root; it needs
# no root). Running it again is harmless: it changes only what differs (after hotdog-simulator.service changes, say).
#
#   scp tools/deploy/remote-setup.sh tools/deploy/hotdog-simulator.service USER@HOST:
#   ssh USER@HOST ./remote-setup.sh [--path DIR] [--system] [--authorize "PUBLIC KEY"]
#
#   --path DIR        DEPLOY_PATH, ~/hotdog-simulator by default: releases/, incoming/ and replays/ are made in it
#   --system          a system service run by this account, instead of a user service: the unit is written to
#                     DIR/hotdog-simulator.service for root to install, and the steps are printed (deploys then
#                     restart it with `sudo -n systemctl`, so set the repository variable DEPLOY_SCOPE=system)
#   --authorize KEY   adds the deploy key's public half to ~/.ssh/authorized_keys, restricted (no forwarding, no
#                     terminal: a deploy only runs commands), unless it is there already
#
# It checks the tools a deploy uses here, makes the folders, installs the unit with the path filled in (a user unit in
# ~/.config/systemd/user, enabled to start at boot; the first deploy starts it), turns on lingering if it may (so that
# this account's services run with nobody logged in) and prints what is left for root, if anything.
set -euo pipefail

path=$HOME/hotdog-simulator system=0 key=
while [ $# -gt 0 ]; do
  case $1 in
    --path) path=${2:?--path needs a folder} && shift ;;
    --system) system=1 ;;
    --authorize) key=${2:?--authorize needs a public key} && shift ;;
    -h | --help) sed -n '5,20p' "$0" | sed 's/^# \{0,1\}//' && exit 0 ;;
    *) echo "unknown option $1 (--path DIR, --system, --authorize KEY)" >&2 && exit 2 ;;
  esac
  shift
done
die() {
  echo "remote-setup: $*" >&2
  exit 1
}
[ "$(id -u)" != 0 ] || die "run this as the account that runs the game, not as root"
# shellcheck disable=SC2088 # (a literal ~, as it came)
case $path in
  "~") path=$HOME ;;
  "~/"*) path=$HOME/${path#"~/"} ;;
  /*) ;;
  *) path=$PWD/$path ;;
esac
path=${path%/}
[[ $path =~ ^/[A-Za-z0-9._/-]+$ ]] || die "the path may hold letters, digits and . _ - / only: $path"
template=$(dirname "$0")/hotdog-simulator.service
[ -f "$template" ] || die "no $template: copy hotdog-simulator.service beside this script"
user=$(id -un) group=$(id -gn) unit=hotdog-simulator.service
root=() done=()

# The tools a deploy runs here (tools/deploy/deploy.sh, its server side).
missing=()
for tool in tar gzip sha256sum flock curl systemctl loginctl; do command -v "$tool" >/dev/null || missing+=("$tool"); done
[ "${BASH_VERSINFO[0]}" -ge 4 ] || missing+=("bash 4 or later")
if [ ${#missing[@]} -gt 0 ]; then
  root+=("install what deploys need: ${missing[*]} (Debian, Ubuntu: apt-get install tar gzip coreutils util-linux curl systemd)")
fi

mkdir -p "$path/releases" "$path/incoming" "$path/replays"
chmod 750 "$path"
done+=("folders: $path/{releases,incoming,replays} (replays are never deleted by a deploy)")

# The unit, with the path filled in.
unit_text() {
  if [ "$system" = 1 ]; then
    sed -e "s|%h/hotdog-simulator|$path|g" -e 's/^#system: //' -e "s/@USER@/$user/" -e "s/@GROUP@/$group/" \
      -e 's/^WantedBy=default.target$/WantedBy=multi-user.target/' "$template"
  else
    sed -e "s|%h/hotdog-simulator|$path|g" "$template"
  fi
}

if [ "$system" = 1 ]; then
  unit_text >"$path/$unit"
  done+=("the system unit, for root to install: $path/$unit")
  root+=("install the unit:  install -m 644 $path/$unit /etc/systemd/system/$unit && systemctl daemon-reload && systemctl enable $unit")
  root+=("let $user restart it (and nothing else):  printf '%s\\n' '$user ALL=(root) NOPASSWD: /usr/bin/systemctl restart $unit, /usr/bin/systemctl reset-failed $unit' > /etc/sudoers.d/hotdog-simulator && chmod 440 /etc/sudoers.d/hotdog-simulator && visudo -cf /etc/sudoers.d/hotdog-simulator")
  root+=("optionally, let $user read its log when a deploy fails:  usermod -aG systemd-journal $user")
else
  export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
  dest=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/$unit
  mkdir -p "$(dirname "$dest")"
  if [ -f "$dest" ] && [ "$(unit_text)" = "$(cat "$dest")" ]; then
    done+=("the user unit is current: $dest")
  else
    unit_text >"$dest"
    systemctl --user daemon-reload
    done+=("the user unit: $dest")
    if [ -L "$path/current" ] && systemctl --user is-active --quiet "$unit"; then
      systemctl --user restart "$unit"
      done+=("restarted $unit with the new unit")
    fi
  fi
  systemctl --user enable "$unit" >/dev/null 2>&1 || die "systemctl --user enable $unit failed (is this a systemd system, and is this a login session?)"
  done+=("$unit enabled (it starts at boot; the first deploy starts it now)")
  if [ "$(loginctl show-user "$user" --property=Linger --value 2>/dev/null)" = yes ]; then
    done+=("lingering is on for $user")
  elif loginctl enable-linger "$user" 2>/dev/null; then
    done+=("lingering turned on for $user")
  else
    root+=("keep $user's services running with nobody logged in:  loginctl enable-linger $user")
  fi
fi

if [ -n "$key" ]; then
  body=$(awk '{for (i = 1; i <= NF; i++) if ($i ~ /^(ssh-|ecdsa-|sk-)/) {print $(i + 1); exit}}' <<<"$key")
  [ -n "$body" ] || die "--authorize takes a public key line (ssh-ed25519 AAAA... comment)"
  mkdir -p "$HOME/.ssh"
  chmod 700 "$HOME/.ssh"
  touch "$HOME/.ssh/authorized_keys"
  chmod 600 "$HOME/.ssh/authorized_keys"
  if grep -qF "$body" "$HOME/.ssh/authorized_keys"; then
    done+=("the deploy key is in ~/.ssh/authorized_keys already")
  else
    echo "restrict $key" >>"$HOME/.ssh/authorized_keys"
    done+=("the deploy key added to ~/.ssh/authorized_keys (restricted: commands only)")
  fi
fi

echo "Done:"
printf '  - %s\n' "${done[@]}"
if [ ${#root[@]} -gt 0 ]; then
  echo "Still to do as root:"
  printf '  - %s\n' "${root[@]}"
else
  echo "Nothing is left for root here."
fi
cat <<EOF
Then:
  - the HTTPS proxy in front of 127.0.0.1:8150 (as root): tools/deploy/Caddyfile.example or nginx.conf.example,
    with ports 80 and 443 open and nothing else of the game's (8150 listens on 127.0.0.1 only)
  - the repository's deploy settings: DEPLOY_PATH=$path, DEPLOY_USER=$user$([ "$system" = 1 ] && echo ", DEPLOY_SCOPE=system")
    (README.md, "Continuous integration and deployment")
EOF
