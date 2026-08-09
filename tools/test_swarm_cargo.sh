#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
wrapper="$repo_root/tools/swarm-cargo"
test_root=$(mktemp -d "${TMPDIR:-/tmp}/don-swarm-cargo-test.XXXXXX")
trap 'rm -rf -- "$test_root"' EXIT

lane_a=$(DON_SWARM_TARGET_ROOT="$test_root/targets" DON_SWARM_SCCACHE=0 \
  "$wrapper" --print-env alpha)
lane_b=$(DON_SWARM_TARGET_ROOT="$test_root/targets" DON_SWARM_SCCACHE=0 \
  DON_SWARM_BUILD_JOBS=3 "$wrapper" --print-env beta)

canonical_target_root=$(unset CDPATH; cd -- "$test_root/targets" && pwd -P)
target_a=$(printf '%s\n' "$lane_a" | sed -n 's/^CARGO_TARGET_DIR=//p')
target_b=$(printf '%s\n' "$lane_b" | sed -n 's/^CARGO_TARGET_DIR=//p')
[[ -n "$target_a" && -n "$target_b" && "$target_a" != "$target_b" ]]
[[ "$target_a" == "$canonical_target_root/alpha" ]]
[[ "$target_b" == "$canonical_target_root/beta" ]]
printf '%s\n' "$lane_a" | grep -qx 'CARGO_BUILD_JOBS=2'
printf '%s\n' "$lane_b" | grep -qx 'CARGO_BUILD_JOBS=3'
[[ -d "$target_a" && -d "$target_b" ]]

if DON_SWARM_TARGET_ROOT="$test_root/targets" "$wrapper" --print-env '../escape' >/dev/null 2>&1; then
  printf 'unsafe lane name unexpectedly succeeded\n' >&2
  exit 1
fi

if DON_SWARM_TARGET_ROOT="$test_root/targets" DON_SWARM_BUILD_JOBS=0 \
  "$wrapper" --print-env invalid-jobs >/dev/null 2>&1; then
  printf 'invalid job count unexpectedly succeeded\n' >&2
  exit 1
fi

if command -v sccache >/dev/null 2>&1; then
  cached=$(env -u CARGO_INCREMENTAL -u RUSTC_WRAPPER -u SCCACHE_DIR \
    DON_SWARM_TARGET_ROOT="$test_root/targets" \
    DON_SWARM_SCCACHE_DIR="$test_root/cache" "$wrapper" --print-env cached)
  printf '%s\n' "$cached" | grep -qx 'CARGO_INCREMENTAL=0'
  printf '%s\n' "$cached" | grep -q '^RUSTC_WRAPPER=.*/sccache$'
fi

printf 'swarm-cargo shell tests passed\n'
