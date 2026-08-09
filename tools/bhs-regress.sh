#!/usr/bin/env bash
# Capture a bounded retail BHS compiler corpus on hbox and run the local differential.
#
# The retail compiler is PE32/i386 and cannot execute on this arm64 Mac. The harness maps
# and runs the supported machine code at Compiler::compile 0x009bf160 on hbox, then emits
# pointer-free JSON for bytecode, constants, scripts, statics, and trigger/name arrays.
# A missing box/image/capture is an error, never a green skip.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST="${DON_BHS_ORACLE_HOST:-hbox}"
REMOTE="${DON_BHS_ORACLE_DIR:-~/don-bhs-oracle}"
TARGET="i686-unknown-linux-musl"
PROFILE="${DON_BHS_ORACLE_PROFILE:-debug}"
EXPECTED_SHA="30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
OUT="$REPO/schema/bhs-compiler-regression.json"
LOG="$REPO/schema/bhs-compiler-regression.log"
SSH=(-o ConnectTimeout=15 -o BatchMode=yes)
NICE=(nice -n 15 taskset -c 0-3)

if [ "${1:-}" = "--status" ]; then
  if [ ! -f "$OUT" ]; then
    echo "bhs-regress: no capture at $OUT"
    exit 4
  fi
  python3 - "$OUT" <<'PY'
import json, sys, time
d = json.load(open(sys.argv[1], encoding="utf-8"))
age = max(0, time.time() - d.get("generated_unix", 0))
print(f"bhs-regress: {sys.argv[1]}")
print(f"  measured {age/86400:.1f} days ago on {d.get('host', '?')} ({d.get('target', '?')})")
print(f"  image sha256 {d.get('image', {}).get('sha256', '?')}")
print(f"  {len(d.get('fixtures', []))} retail compiler fixtures captured")
print(f"  local comparison: {d.get('local_comparison', 'run cargo test -p don-bhs-cc --test retail_bytecode')}")
PY
  exit 0
fi

unreachable() {
  echo "bhs-regress: ERROR — $1"
  echo "bhs-regress: NOTHING WAS CAPTURED; the previous record is unchanged."
  exit 4
}

echo "bhs-regress: checking $HOST"
ssh "${SSH[@]}" "$HOST" true 2>/dev/null || unreachable "$HOST is unreachable"

REMOTE_ABS=$(ssh "${SSH[@]}" "$HOST" "cd $REMOTE 2>/dev/null && pwd")
[ -n "$REMOTE_ABS" ] || unreachable "$REMOTE does not exist"
REMOTE="$REMOTE_ABS"

REMOTE_CRATE="$REMOTE/crates/don-bhs/oracle"
REMOTE_EXE="$REMOTE_CRATE/data/riseofnations.exe"
REMOTE_SHA=$(ssh "${SSH[@]}" "$HOST" "sha256sum $REMOTE_EXE 2>/dev/null | cut -d' ' -f1")
[ -n "$REMOTE_SHA" ] || unreachable "$REMOTE_EXE is missing"
if [ "$REMOTE_SHA" != "$EXPECTED_SHA" ]; then
  unreachable "unsupported retail image $REMOTE_SHA (expected $EXPECTED_SHA)"
fi
echo "bhs-regress: supported image $REMOTE_SHA"

echo "bhs-regress: syncing named harness sources"
ssh "${SSH[@]}" "$HOST" "mkdir -p $REMOTE_CRATE/src $REMOTE_CRATE/fixtures $REMOTE/crates/don-pe" \
  || unreachable "cannot create the bounded remote directories"
rsync -a --delete -e "ssh ${SSH[*]}" \
  "$REPO/crates/don-bhs/oracle/src/" "$HOST:$REMOTE_CRATE/src/" \
  || unreachable "oracle source sync failed"
rsync -a --delete -e "ssh ${SSH[*]}" \
  "$REPO/crates/don-bhs/oracle/fixtures/" "$HOST:$REMOTE_CRATE/fixtures/" \
  || unreachable "fixture sync failed"
rsync -a --delete --exclude target/ -e "ssh ${SSH[*]}" \
  "$REPO/crates/don-pe/" "$HOST:$REMOTE/crates/don-pe/" \
  || unreachable "don-pe dependency sync failed"
scp -q "${SSH[@]}" "$REPO/crates/don-bhs/oracle/Cargo.toml" \
  "$HOST:$REMOTE_CRATE/Cargo.toml" || unreachable "manifest sync failed"

RELFLAG=()
BIN_DIR=debug
if [ "$PROFILE" = "release" ]; then
  RELFLAG=(--release)
  BIN_DIR=release
fi

echo "bhs-regress: building on $HOST ($TARGET, $PROFILE)"
if ! ssh "${SSH[@]}" "$HOST" \
  "cd $REMOTE_CRATE && ${NICE[*]} env RUSTFLAGS='-C llvm-args=-stackrealign -Awarnings' cargo build --quiet ${RELFLAG[*]} --target $TARGET --bin bhsoracle" \
  2>&1 | tee "$LOG"; then
  echo "bhs-regress: BUILD FAILED — see $LOG"
  exit 4
fi

REMOTE_CAP="$REMOTE_CRATE/bhs-regression-captures"
ssh "${SSH[@]}" "$HOST" "rm -rf $REMOTE_CAP && mkdir -p $REMOTE_CAP" \
  || unreachable "cannot reset the bounded remote capture directory"

captured=0
for local_fixture in "$REPO"/crates/don-bhs/oracle/fixtures/*.bhs; do
  id=$(basename "$local_fixture" .bhs)
  echo "bhs-regress: retail compile $id"
  remote_fixture="fixtures/$id.bhs"
  remote_json="$REMOTE_CAP/$id.json"
  remote_log="$REMOTE_CAP/$id.log"
  if ! ssh "${SSH[@]}" "$HOST" \
    "cd $REMOTE_CRATE && ${NICE[*]} ./target/$TARGET/$BIN_DIR/bhsoracle compile $remote_fixture --json $remote_json >$remote_log 2>&1"; then
    echo "bhs-regress: retail compiler failed for $id"
    ssh "${SSH[@]}" "$HOST" "tail -80 $remote_log" || true
    exit 1
  fi
  if ! ssh "${SSH[@]}" "$HOST" \
    "python3 -c 'import json; d=json.load(open(\"$remote_json\")); f=d[\"files\"][0]; assert d[\"compile_return\"] == 0 and len(d[\"files\"]) == 1 and f[\"code_hex\"] and len(f[\"scripts\"]) == 1 and \"unsupported\" not in json.dumps(f)'"; then
    echo "bhs-regress: incomplete normalized capture for $id"
    ssh "${SSH[@]}" "$HOST" "tail -80 $remote_log" || true
    exit 1
  fi
  captured=$((captured + 1))
done
[ "$captured" -gt 0 ] || unreachable "no fixtures were discovered"

TMP=$(mktemp -d "${TMPDIR:-/tmp}/don-bhs-regress.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
scp -q "${SSH[@]}" "$HOST:$REMOTE_CAP/*.json" "$TMP/" \
  || unreachable "could not fetch normalized captures"

mkdir -p "$REPO/schema"
python3 - "$TMP" "$REPO" "$OUT" "$HOST" "$TARGET" "$REMOTE_SHA" <<'PY'
import hashlib, json, pathlib, sys, time

capture_dir, repo, out, host, target, image_sha = sys.argv[1:]
capture_dir = pathlib.Path(capture_dir)
repo = pathlib.Path(repo)
fixtures = []
for path in sorted(capture_dir.glob("*.json")):
    capture = json.load(open(path, encoding="utf-8"))
    fixture_id = path.stem
    source_rel = f"crates/don-bhs/oracle/fixtures/{fixture_id}.bhs"
    source = (repo / source_rel).read_bytes()
    fixtures.append({
        "id": fixture_id,
        "source_path": source_rel,
        "source_sha256": hashlib.sha256(source).hexdigest(),
        "compile_return": capture["compile_return"],
        "retail": capture["files"][0],
    })

record = {
    "schema_version": 1,
    "generated_unix": int(time.time()),
    "host": host,
    "target": target,
    "image": {
        "sha256": image_sha,
        "compiler_va": "0x009bf160",
        "compiler_root_va": "0x00eb6a90",
    },
    "normalization": [
        "ASLR pointers and container backing addresses are excluded",
        "bytecode is exact and unmodified",
        "constants and initialized statics retain scalar type tags and payload bits",
        "script arrays retain logical order; capacity/growth metadata is outside this record",
    ],
    "fixtures": fixtures,
    "local_comparison": "cargo test -p don-bhs-cc --test retail_bytecode",
    "fidelity_boundary": (
        "Finite compiler differential only. The captured image is shipped machine-code output; "
        "mismatches are compiler debt, not waived equivalence."
    ),
}
with open(out, "w", encoding="utf-8") as f:
    json.dump(record, f, indent=2, sort_keys=True)
    f.write("\n")
PY

echo "bhs-regress: wrote $OUT ($captured actual-retail fixtures)"
echo "bhs-regress: running the local mutation-sensitive differential"
cargo test -p don-bhs-cc --test retail_bytecode
