#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Audit an exact source-release tree. This script never changes the supplied tree.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
SUPPORTED_EXE_SHA256="30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
MAX_COMPACT_LIVE_JSON_BYTES=65536

usage() {
  cat <<'EOF'
usage:
  tools/release-audit.sh --staging-dir DIR
  tools/release-audit.sh --git-archive REF [--repo REPOSITORY]

Audit an explicitly supplied staging directory, including ignored and untracked files, or
generate and audit a temporary `git archive` from an explicit commit/ref. The target is
read-only. Refusals are reported one per path and the command exits 3 when any are found.
EOF
}

die() {
  printf 'release-audit: ERROR: %s\n' "$*" >&2
  exit 2
}

mode=""
target=""
repo="$ROOT"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --staging-dir)
      [[ $# -ge 2 ]] || die "--staging-dir requires a directory"
      [[ -z "$mode" ]] || die "choose exactly one audit mode"
      mode="staging"
      target="$2"
      shift 2
      ;;
    --git-archive)
      [[ $# -ge 2 ]] || die "--git-archive requires a commit or ref"
      [[ -z "$mode" ]] || die "choose exactly one audit mode"
      mode="archive"
      target="$2"
      shift 2
      ;;
    --repo)
      [[ $# -ge 2 ]] || die "--repo requires a repository directory"
      repo="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "$mode" ]] || {
  usage >&2
  exit 2
}
if [[ "$mode" != "archive" && "$repo" != "$ROOT" ]]; then
  die "--repo is valid only with --git-archive"
fi

scratch=""
cleanup() {
  [[ -n "$scratch" ]] || return 0
  case "$scratch" in
    "${TMPDIR:-/tmp}"/don-release-audit.*)
      rm -rf -- "$scratch"
      ;;
    *)
      printf 'release-audit: WARNING: refusing unexpected temporary path cleanup: %s\n' \
        "$scratch" >&2
      ;;
  esac
}
trap cleanup EXIT HUP INT TERM

if [[ "$mode" == "staging" ]]; then
  [[ -d "$target" ]] || die "staging directory does not exist: $target"
  [[ ! -L "$target" ]] || die "staging directory itself must not be a symlink: $target"
  audit_root="$(cd "$target" && pwd -P)"
  audit_label="staging directory $audit_root"
else
  [[ -d "$repo" ]] || die "repository does not exist: $repo"
  [[ ! -L "$repo" ]] || die "repository itself must not be a symlink: $repo"
  repo="$(cd "$repo" && pwd -P)"
  git -C "$repo" rev-parse --is-inside-work-tree >/dev/null 2>&1 \
    || die "not a Git work tree: $repo"
  [[ "$target" != -* ]] || die "git archive ref must not begin with '-': $target"
  commit="$(git -C "$repo" rev-parse --verify "$target^{commit}" 2>/dev/null)" \
    || die "cannot resolve commit/ref: $target"
  scratch="$(mktemp -d "${TMPDIR:-/tmp}/don-release-audit.XXXXXX")"
  mkdir -p "$scratch/tree"
  git -C "$repo" archive --format=tar --output="$scratch/source.tar" "$commit" \
    || die "git archive failed for $target"
  tar -xf "$scratch/source.tar" -C "$scratch/tree" \
    || die "could not extract generated git archive"
  audit_root="$scratch/tree"
  audit_label="generated git archive $commit from $repo"
fi

if command -v shasum >/dev/null 2>&1; then
  sha256_file() {
    shasum -a 256 -- "$1" | awk '{print $1}'
  }
elif command -v sha256sum >/dev/null 2>&1; then
  sha256_file() {
    sha256sum -- "$1" | awk '{print $1}'
  }
else
  die "need shasum or sha256sum to enforce the retail executable hash gate"
fi

refusals=0
record_refusal() {
  refusals=$((refusals + 1))
  printf 'release-audit: REFUSE %s: %s\n' "$1" "$2" >&2
}

# Enumerate without consulting Git so a staging audit includes ignored and untracked files.
# Reject path controls before converting the NUL-delimited walk into a deterministically
# sorted line manifest.
walk="$(mktemp "${TMPDIR:-/tmp}/don-release-audit.walk.XXXXXX")"
manifest="$(mktemp "${TMPDIR:-/tmp}/don-release-audit.manifest.XXXXXX")"
manifest_sorted="$(mktemp "${TMPDIR:-/tmp}/don-release-audit.sorted.XXXXXX")"
cleanup_manifests() {
  rm -f -- "$walk" "$manifest" "$manifest_sorted"
}
trap 'cleanup_manifests; cleanup' EXIT HUP INT TERM

find "$audit_root" -mindepth 1 -print0 > "$walk" \
  || die "could not enumerate the complete audit target: $audit_root"
while IFS= read -r -d '' node; do
  rel="${node#"$audit_root"/}"
  case "$rel" in
    *$'\n'*|*$'\r'*|*$'\t'*)
      record_refusal "<path-with-control-characters>" \
        "release paths may not contain newline, carriage-return, or tab characters"
      continue
      ;;
  esac
  if [[ -L "$node" ]]; then
    record_refusal "$rel" "symlinks are not admitted to a fail-closed source release"
  elif [[ -f "$node" ]]; then
    printf '%s\n' "$rel" >> "$manifest"
  elif [[ ! -d "$node" ]]; then
    record_refusal "$rel" "special filesystem objects are not release source files"
  fi
done < "$walk"
LC_ALL=C sort "$manifest" > "$manifest_sorted"

while IFS= read -r rel || [[ -n "$rel" ]]; do
  [[ -n "$rel" ]] || continue
  file="$audit_root/$rel"
  lower="$(printf '%s' "$rel" | LC_ALL=C tr '[:upper:]' '[:lower:]')"

  case "$lower" in
    .git|.git/*|*/.git|*/.git/*)
      record_refusal "$rel" "Git repository metadata leaked into the release tree"
      ;;
  esac

  case "$lower" in
    ron-bin/*|*/ron-bin/*)
      record_refusal "$rel" "retail binaries/PDBs are local proprietary inputs"
      ;;
    ron-data/*|*/ron-data/*)
      record_refusal "$rel" "retail rules, scripts, replays, art, or audio are local proprietary inputs"
      ;;
  esac

  case "$lower" in
    *.dll|*.exe|*.pdb|*.lib|*.obj)
      record_refusal "$rel" "compiled DLL/EXE/PDB/LIB/OBJ output is not source-release material"
      ;;
    *.rcx|*.svx)
      record_refusal "$rel" "retail replay/save content must not be redistributed"
      ;;
    *.pyc|*/__pycache__/*)
      record_refusal "$rel" "generated Python bytecode leaked into the release tree"
      ;;
    target/*|*/target/*)
      record_refusal "$rel" "Cargo build output leaked into the release tree"
      ;;
    .ds_store|*/.ds_store)
      record_refusal "$rel" "host metadata leaked into the release tree"
      ;;
  esac

  case "$lower" in
    web/public/data/*|*/web/public/data/*)
      case "$lower" in
        web/public/data/.gitignore|*/web/public/data/.gitignore) ;;
        *)
          record_refusal "$rel" \
            "generated browser data/replay packs are derived from proprietary local inputs"
          ;;
      esac
      ;;
  esac

  case "$lower" in
    schema/live/*|*/schema/live/*)
      live_rel="${lower#*schema/live/}"
      case "$live_rel" in
        .gitignore)
          ;;
        retail-*.json)
          bytes="$(wc -c < "$file" | tr -d '[:space:]')"
          if [[ "$bytes" -gt "$MAX_COMPACT_LIVE_JSON_BYTES" ]]; then
            record_refusal "$rel" \
              "bulk live JSON capture is $bytes bytes; compact evidence limit is $MAX_COMPACT_LIVE_JSON_BYTES"
          fi
          ;;
        *)
          record_refusal "$rel" \
            "bulk/raw schema/live capture is proprietary; only compact retail-*.json evidence is admitted"
          ;;
      esac
      ;;
  esac

  magic="$(LC_ALL=C od -An -tx1 -N32 "$file" | tr -d '[:space:]')"
  case "$magic" in
    4d5a*)
      record_refusal "$rel" "file content has a Windows PE executable header, regardless of its name"
      ;;
    4d6963726f736f667420432f432b2b204d534620372e3030*)
      record_refusal "$rel" "file content has a Microsoft PDB/MSF header, regardless of its name"
      ;;
    444f4e5041434b32*|444f4e504c415931*)
      record_refusal "$rel" "file content is a generated proprietary DONPACK2/DONPLAY1 browser pack"
      ;;
  esac

  digest="$(sha256_file "$file")" || die "could not hash $rel"
  if [[ "$digest" == "$SUPPORTED_EXE_SHA256" ]]; then
    record_refusal "$rel" \
      "content matches the supported retail riseofnations.exe SHA-256, regardless of its name"
  fi
done < "$manifest_sorted"

if [[ "$refusals" -ne 0 ]]; then
  printf 'release-audit: FAILED: %d refusal(s) in %s\n' "$refusals" "$audit_label" >&2
  exit 3
fi

files="$(wc -l < "$manifest_sorted" | tr -d '[:space:]')"
printf 'release-audit: PASS: %s regular files in %s\n' "$files" "$audit_label"
