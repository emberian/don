#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Synthetic tests for tools/release-audit.sh. No retail bytes are used.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
AUDIT="$ROOT/tools/release-audit.sh"
KNOWN_HASH="30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/don-release-audit-tests.XXXXXX")"
cleanup() {
  case "$tmp" in
    "${TMPDIR:-/tmp}"/don-release-audit-tests.*) rm -rf -- "$tmp" ;;
    *) printf 'test: refusing unexpected temporary path cleanup: %s\n' "$tmp" >&2 ;;
  esac
}
trap cleanup EXIT HUP INT TERM

tests=0
expect_pass() {
  name="$1"
  shift
  tests=$((tests + 1))
  if ! output="$("$@" 2>&1)"; then
    printf 'not ok %d - %s\n%s\n' "$tests" "$name" "$output" >&2
    exit 1
  fi
  printf 'ok %d - %s\n' "$tests" "$name"
}

expect_refusal() {
  name="$1"
  needle="$2"
  shift 2
  tests=$((tests + 1))
  set +e
  output="$("$@" 2>&1)"
  status=$?
  set -e
  if [[ "$status" -ne 3 ]] || ! printf '%s\n' "$output" | grep -Fq -- "$needle"; then
    printf 'not ok %d - %s (status=%d, wanted refusal containing %s)\n%s\n' \
      "$tests" "$name" "$status" "$needle" "$output" >&2
    exit 1
  fi
  printf 'ok %d - %s\n' "$tests" "$name"
}

safe="$tmp/safe"
mkdir -p "$safe/src" "$safe/schema/live" "$safe/web/public/data"
printf '# synthetic source\n' > "$safe/README.md"
printf 'fn main() {}\n' > "$safe/src/main.rs"
printf '{}\n' > "$safe/schema/live/retail-synthetic-proof-v1.json"
printf '*\n!.gitignore\n' > "$safe/web/public/data/.gitignore"
cp -R "$safe" "$tmp/safe-before"
expect_pass "minimal source and compact evidence pass" "$AUDIT" --staging-dir "$safe"
expect_pass "staging audit leaves the exact target unchanged" diff -r "$tmp/safe-before" "$safe"

mkdir -p "$safe/ron-data"
printf '<synthetic/>\n' > "$safe/ron-data/rules.xml"
expect_refusal "retail-data root refuses" "local proprietary inputs" \
  "$AUDIT" --staging-dir "$safe"
rm "$safe/ron-data/rules.xml"
rmdir "$safe/ron-data"

printf '*.dll\n' > "$safe/.gitignore"
printf 'synthetic compiled output\n' > "$safe/injected.DLL"
expect_refusal "ignored untracked DLL still refuses" "compiled DLL/EXE/PDB/LIB/OBJ" \
  "$AUDIT" --staging-dir "$safe"
rm "$safe/injected.DLL"

printf 'row\tvalue\n' > "$safe/schema/live/raw.tsv"
expect_refusal "raw live table refuses" "bulk/raw schema/live capture" \
  "$AUDIT" --staging-dir "$safe"
rm "$safe/schema/live/raw.tsv"

dd if=/dev/zero of="$safe/schema/live/retail-too-large.json" bs=65537 count=1 2>/dev/null
expect_refusal "oversized live JSON refuses" "bulk live JSON capture" \
  "$AUDIT" --staging-dir "$safe"
rm "$safe/schema/live/retail-too-large.json"

printf '{}\n' > "$safe/web/public/data/gamedata.json"
expect_refusal "generated browser data path refuses" "generated browser data/replay packs" \
  "$AUDIT" --staging-dir "$safe"
rm "$safe/web/public/data/gamedata.json"

printf 'DONPACK2 synthetic\n' > "$safe/src/renamed-input.dat"
expect_refusal "renamed generated pack refuses by magic" "DONPACK2/DONPLAY1" \
  "$AUDIT" --staging-dir "$safe"
rm "$safe/src/renamed-input.dat"

printf 'MZsynthetic executable\n' > "$safe/src/renamed-pe.dat"
expect_refusal "renamed PE refuses by magic" "Windows PE executable header" \
  "$AUDIT" --staging-dir "$safe"
rm "$safe/src/renamed-pe.dat"

printf 'Microsoft C/C++ MSF 7.00 synthetic\n' > "$safe/src/renamed-pdb.dat"
expect_refusal "renamed PDB refuses by magic" "Microsoft PDB/MSF header" \
  "$AUDIT" --staging-dir "$safe"
rm "$safe/src/renamed-pdb.dat"

# Exercise the production hash comparison without possessing the retail image: patch a
# temporary copy of the script so the blocked digest is the digest of a synthetic sentinel.
printf 'synthetic hash sentinel\n' > "$safe/src/hash-sentinel.dat"
if command -v shasum >/dev/null 2>&1; then
  sentinel_hash="$(shasum -a 256 "$safe/src/hash-sentinel.dat" | awk '{print $1}')"
else
  sentinel_hash="$(sha256sum "$safe/src/hash-sentinel.dat" | awk '{print $1}')"
fi
sed "s/$KNOWN_HASH/$sentinel_hash/" "$AUDIT" > "$tmp/audit-with-synthetic-hash.sh"
chmod +x "$tmp/audit-with-synthetic-hash.sh"
expect_refusal "known-hash gate refuses renamed bytes" "supported retail riseofnations.exe SHA-256" \
  "$tmp/audit-with-synthetic-hash.sh" --staging-dir "$safe"
rm "$safe/src/hash-sentinel.dat"

repo="$tmp/repo"
mkdir -p "$repo"
git -C "$repo" init -q
git -C "$repo" config user.name "Synthetic Release Audit"
git -C "$repo" config user.email "release-audit@example.invalid"
printf 'safe tracked source\n' > "$repo/source.txt"
git -C "$repo" add source.txt
git -C "$repo" commit -qm "synthetic source"
expect_pass "generated git archive of safe commit passes" \
  "$AUDIT" --git-archive HEAD --repo "$repo"

printf 'ignored binary\n' > "$repo/leak.dll"
printf '*.dll\n' > "$repo/.gitignore"
expect_refusal "staging audit sees ignored/untracked leakage" "compiled DLL/EXE/PDB/LIB/OBJ" \
  "$AUDIT" --staging-dir "$repo"
expect_pass "git archive contains only the explicit commit" \
  "$AUDIT" --git-archive HEAD --repo "$repo"

git -C "$repo" add -f leak.dll
git -C "$repo" commit -qm "synthetic tracked leak"
expect_refusal "git archive rejects tracked leakage" "compiled DLL/EXE/PDB/LIB/OBJ" \
  "$AUDIT" --git-archive HEAD --repo "$repo"

manifest_repo="$tmp/manifest-repo"
mkdir -p "$manifest_repo/schema/live"
git -C "$manifest_repo" init -q
git -C "$manifest_repo" config user.name "Synthetic Release Audit"
git -C "$manifest_repo" config user.email "release-audit@example.invalid"
printf '/schema/live/*.tsv export-ignore\n' > "$manifest_repo/.gitattributes"
printf 'synthetic raw research evidence\n' > "$manifest_repo/schema/live/raw.tsv"
printf '{}\n' > "$manifest_repo/schema/live/retail-synthetic-proof-v1.json"
git -C "$manifest_repo" add .gitattributes schema/live/raw.tsv \
  schema/live/retail-synthetic-proof-v1.json
git -C "$manifest_repo" commit -qm "synthetic archive manifest"
expect_refusal "staging tree retains and refuses raw research evidence" \
  "bulk/raw schema/live capture" "$AUDIT" --staging-dir "$manifest_repo"
expect_pass "git archive manifest excludes raw evidence but keeps compact fixture" \
  "$AUDIT" --git-archive HEAD --repo "$manifest_repo"

printf '1..%d\n' "$tests"
