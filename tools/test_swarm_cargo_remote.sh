#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
wrapper="$repo_root/tools/swarm-cargo-remote"

plan=$("$wrapper" --plan submit persvati test-lane \
  --path tools/swarm-cargo-remote --jobs 6 --nice 10 -- check -p don-sim --lib)
printf '%s\n' "$plan" | grep -qx 'HOST=persvati'
printf '%s\n' "$plan" | grep -qx 'LANE=test-lane'
printf '%s\n' "$plan" | grep -qx 'JOBS=6'
printf '%s\n' "$plan" | grep -qx 'NICE=10'
printf '%s\n' "$plan" | grep -qx 'OVERLAY=tools/swarm-cargo-remote'
printf '%s\n' "$plan" | grep -qx 'CARGO_ARG=don-sim'

if "$wrapper" --plan submit elsewhere lane -- check >/dev/null 2>&1; then
  printf 'unknown host unexpectedly succeeded\n' >&2
  exit 1
fi
if "$wrapper" --plan submit hbox Uppercase -- check >/dev/null 2>&1; then
  printf 'unsafe lane unexpectedly succeeded\n' >&2
  exit 1
fi
if "$wrapper" --plan submit hbox lane --path tools -- check >/dev/null 2>&1; then
  printf 'directory overlay unexpectedly succeeded\n' >&2
  exit 1
fi
if "$wrapper" --plan submit hbox lane -- check '../escape' >/dev/null 2>&1; then
  printf 'unsafe Cargo argument unexpectedly succeeded\n' >&2
  exit 1
fi
if "$wrapper" --plan submit hbox lane -- check --target-dir=/tmp/shared >/dev/null 2>&1; then
  printf 'target override unexpectedly succeeded\n' >&2
  exit 1
fi

printf 'swarm-cargo-remote shell tests passed\n'
