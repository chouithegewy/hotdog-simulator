#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# Puts an earlier release of the game back live on its server (README.md, "Continuous integration and deployment"):
#
#   tools/deploy/rollback.sh [OPTIONS] USER@HOST DEPLOY_PATH             the release deployed before the live one
#   tools/deploy/rollback.sh [OPTIONS] USER@HOST DEPLOY_PATH RELEASE     that release (20261119T183000Z-0.1.0-bf5e6ec)
#   tools/deploy/rollback.sh [OPTIONS] --status USER@HOST DEPLOY_PATH    the releases there, and which is live
#
# It points DEPLOY_PATH/current at that release, restarts the service, checks that the server answers with it (and if
# it does not, goes back to the release that was live), then that PUBLIC_URL serves it. Nothing is deleted: the
# releases are those the last deploy kept (deploy.sh keeps 5). The options are deploy.sh's (--ssh-port, --identity,
# --known-hosts, --url, --scope, ...), and `local` works here as there. The next deploy goes ahead from the release
# rolled back to.
#
# Without SSH access to hand, rolling back is also a deploy of the older commit: Actions > deploy > Run workflow, with
# that commit (it must be on main), which builds and checks it again.
set -euo pipefail
for arg in "$@"; do
  if [ "$arg" = --status ]; then exec "$(dirname "$0")/deploy.sh" "$@"; fi
done
exec "$(dirname "$0")/deploy.sh" --rollback "$@"
