#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
#
# `cargo test` with a few named timing-sensitive tests run on their own and given one retry (.github/workflows/ci.yml;
# README.md, "Continuous integration and deployment"):
#
#   tools/ci/cargo-test-retry.sh TEST [TEST ...] -- CARGO_OPTIONS...
#
# TEST is a test's full path, as `cargo test -- --list` names it (blueprint::tests::the_bots_building_costs_little_per_step).
# CARGO_OPTIONS are cargo's own (--release, --manifest-path ..., --target ...), not the test harness's.
#
#   1. Every other test runs once: `cargo test CARGO_OPTIONS -- --exact --skip TEST ...`. A failure there fails at once:
#      nothing else is retried.
#   2. Each TEST then runs alone (`--exact TEST`), with nothing else of the suite competing for the processor, and once
#      more if it fails. A pass on the second go is reported as a warning (an annotation on GitHub); a second failure
#      fails. A TEST that no test answers to fails too: it was renamed, and its retry would quietly cover nothing.
set -euo pipefail

flaky=()
while [ $# -gt 0 ] && [ "$1" != -- ]; do
  flaky+=("$1")
  shift
done
if [ ${#flaky[@]} -eq 0 ] || [ $# -eq 0 ]; then
  echo "usage: $0 TEST [TEST ...] -- CARGO_OPTIONS..." >&2
  exit 2
fi
shift

# A GitHub annotation in a workflow, a plain line elsewhere.
note() {
  if [ "${GITHUB_ACTIONS:-}" = true ]; then echo "::$1 title=$2::$3"; else echo "$1: $2: $3"; fi
}

skips=()
for test in "${flaky[@]}"; do skips+=(--skip "$test"); done
echo "== cargo test $* -- --exact ${skips[*]}"
cargo test "$@" -- --exact "${skips[@]}"

log=$(mktemp)
trap 'rm -f "$log"' EXIT
for test in "${flaky[@]}"; do
  for attempt in 1 2; do
    echo "== cargo test $* -- --exact $test (alone, attempt $attempt of 2)"
    status=0
    cargo test "$@" -- --exact "$test" 2>&1 | tee "$log" || status=$?
    if ! grep -q "^test $test \.\.\. " "$log"; then
      note error "No such test" "$test matched no test (renamed?): update its name in .github/workflows/ci.yml"
      exit 1
    fi
    if [ "$status" -eq 0 ]; then
      if [ "$attempt" -gt 1 ]; then
        note warning "Timing-sensitive test passed on its retry" "$test failed once and passed on its second run"
      fi
      break
    fi
    if [ "$attempt" -eq 2 ]; then
      note error "Timing-sensitive test failed twice" "$test failed on its own, and again on its retry"
      exit "$status"
    fi
    note warning "Retrying a timing-sensitive test" "$test failed on its own; running it once more"
  done
done
