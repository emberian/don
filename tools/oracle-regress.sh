#!/usr/bin/env bash
# Run the binary-oracle regression suite on hbox from this Mac, and bring the results back.
#
# This Mac is arm64 and cannot execute 32-bit x86 at all (Rosetta is x86-64 only). hbox is
# the only machine that can, and it is CO-TENANT with another agent's build, so everything
# here runs `nice -n 15 taskset -c 0-3` and nothing here installs a package.
#
#   tools/oracle-regress.sh                 # sync, build, run everything, fetch the JSON
#   tools/oracle-regress.sh --scale 0.02    # a fast smoke run (randomised phases only)
#   tools/oracle-regress.sh --only flank_level
#   tools/oracle-regress.sh --status        # report on the last run, locally, run nothing
#
# Exit codes are the oracle's own, propagated verbatim:
#   0  every registered case ran and agreed
#   1  a mismatch, or a case crashed
#   2  a case was SKIPPED — it produced no evidence
#   3  the harness could not start
#   4  hbox is unreachable, or the remote tree is not set up. NOTHING WAS TESTED.
#
# The one thing this script must never do is exit 0 without a measurement. A regression
# harness that reports success when it could not run is worse than no harness: it converts
# "we do not know" into "we checked".

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOST="${DON_ORACLE_HOST:-hbox}"
REMOTE="${DON_ORACLE_DIR:-~/don-oracle}"
TARGET="i686-unknown-linux-musl"
NICE=(nice -n 15 taskset -c 0-3)
OUT="$REPO/schema/oracle-regression.json"
LOG="$REPO/schema/oracle-regression.log"
SSH_OPTS=(-o ConnectTimeout=15 -o BatchMode=yes)

PASSTHRU=()
STATUS_ONLY=0
BUILD_PROFILE="${DON_ORACLE_PROFILE:-debug}"

while [ $# -gt 0 ]; do
  case "$1" in
    --status) STATUS_ONLY=1; shift ;;
    --release) BUILD_PROFILE=release; shift ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) PASSTHRU+=("$1"); shift ;;
  esac
done

# --------------------------------------------------------------------------------------
# --status: look at the record we already have, and be blunt about its age.
# --------------------------------------------------------------------------------------
if [ "$STATUS_ONLY" = 1 ]; then
  if [ ! -f "$OUT" ]; then
    echo "oracle-regress: no measurements at $OUT"
    echo "  Every Tier-B claim in docs/provenance-ledger.md is currently UNVERIFIED-BY-THIS-TREE."
    exit 4
  fi
  python3 - "$OUT" <<'PY'
import json, sys, time
d = json.load(open(sys.argv[1]))
age = time.time() - d.get("generated_unix", 0)
s = d["summary"]
print(f"oracle-regress: {sys.argv[1]}")
print(f"  measured {age/86400:.1f} days ago on {d.get('host','?')} ({d.get('target','?')})")
print(f"  image sha256 {d['image']['sha256']}")
print(f"  selftest {d['harness_selftest']}   exit code {d.get('exit_code')}")
print(f"  {s['pass']} pass  {s['fail']} fail  {s['skipped']} skipped  "
      f"{s['crashed']} crashed  {s['error']} error   ({s['total_trials']} trials)")
for c in d["cases"]:
    if c["status"] != "pass":
        print(f"  {c['status'].upper():8} {c['id']:26} {c.get('detail') or ''}")
print(f"  {len(d.get('known_gaps',[]))} Tier-B claims are outside the suite entirely.")
sys.exit(0 if d.get("exit_code") == 0 else 1)
PY
  exit $?
fi

# --------------------------------------------------------------------------------------
# Reachability. A skip must be loud.
# --------------------------------------------------------------------------------------
unreachable() {
  echo
  echo "oracle-regress: SKIPPED — $1"
  echo "oracle-regress: NOTHING WAS TESTED. No fidelity claim was re-established by this run."
  echo "oracle-regress: the previous record at $OUT (if any) is unchanged and is now older."
  exit 4
}

echo "oracle-regress: checking $HOST"
ssh "${SSH_OPTS[@]}" "$HOST" true 2>/dev/null || unreachable "$HOST is unreachable over ssh"

# The retail image is copyrighted and is not in this repo, so it is never synced: it must
# already be on the box. Verify the exact bytes rather than the mere presence of a file.
REMOTE_SHA=$(ssh "${SSH_OPTS[@]}" "$HOST" "sha256sum $REMOTE/data/riseofnations.exe 2>/dev/null | cut -d' ' -f1")
if [ -z "$REMOTE_SHA" ]; then
  unreachable "$HOST:$REMOTE/data/riseofnations.exe is missing (copyrighted; not synced by this script)"
fi
LOCAL_SHA=""
if [ -f "$REPO/ron-bin/riseofnations.exe" ]; then
  LOCAL_SHA=$(shasum -a 256 "$REPO/ron-bin/riseofnations.exe" | cut -d' ' -f1)
  if [ "$LOCAL_SHA" != "$REMOTE_SHA" ]; then
    echo "oracle-regress: WARNING — remote image $REMOTE_SHA != local $LOCAL_SHA"
    echo "  The measurement below describes the REMOTE image. Do not attribute it to this tree's binary."
  fi
fi
echo "oracle-regress: image sha256 $REMOTE_SHA"

# `data/rules.xml` is the tokenizer case's shipped corpus. Its absence makes that case
# SKIP rather than shrink, so mirror it if the box does not have it.
if ! ssh "${SSH_OPTS[@]}" "$HOST" "test -f $REMOTE/data/rules.xml"; then
  if [ -f "$REPO/ron-data/rules.xml" ]; then
    echo "oracle-regress: copying rules.xml (shipped corpus for rules_as_scaled)"
    scp -q "$REPO/ron-data/rules.xml" "$HOST:$REMOTE/data/rules.xml"
  else
    echo "oracle-regress: WARNING — no rules.xml locally or remotely; rules_as_scaled will SKIP"
  fi
fi

# `data/balance-real.bin` is the balance_final_table case's injected array — the live
# capture of Balance::final_balance_table at 0x00C12BF4. Without it that case SKIPs rather
# than running against the zero-filled image, which would agree with any indexing at all.
# Copyrighted game content, so it is mirrored only if it is already here.
if ! ssh "${SSH_OPTS[@]}" "$HOST" "test -f $REMOTE/data/balance-real.bin"; then
  if [ -f "$REPO/schema/live/balance-real.bin" ]; then
    echo "oracle-regress: copying balance-real.bin (injected array for balance_final_table)"
    scp -q "$REPO/schema/live/balance-real.bin" "$HOST:$REMOTE/data/balance-real.bin"
  else
    echo "oracle-regress: WARNING — no balance-real.bin locally or remotely; balance_final_table will SKIP"
  fi
fi

# --------------------------------------------------------------------------------------
# Sync sources. Named directories only: the retail image, the sweep outputs and the other
# lanes' scratch on that box are not ours to touch.
# --------------------------------------------------------------------------------------
echo "oracle-regress: syncing sources"
ssh "${SSH_OPTS[@]}" "$HOST" "mkdir -p $REMOTE/crates" || unreachable "cannot create $REMOTE/crates"
# One crate at a time, so a failure names the crate that failed.
SYNC_CRATES=(oracle don-pe don-sim don-rules don-bhs don-bhs-cc)

# A path dependency added to any synced crate but not listed above makes a CLEAN remote fail
# at manifest load, before a single case runs — which is how `don-bhs`/`don-bhs-cc` sat broken
# from 2026-08-09 until an oracle lane hit it. Refuse up front and name the missing crate
# instead of shipping a tree that cannot resolve.
missing=()
for c in "${SYNC_CRATES[@]}"; do
  manifest="$REPO/crates/$c/Cargo.toml"
  [[ -f "$manifest" ]] || unreachable "crates/$c has no Cargo.toml"
  while read -r dep; do
    [[ -n "$dep" ]] || continue
    for known in "${SYNC_CRATES[@]}"; do
      [[ "$dep" == "$known" ]] && continue 2
    done
    missing+=("$dep (required by $c)")
  done < <(sed -n 's#.*path *= *"\.\./\([A-Za-z0-9_-]*\)".*#\1#p' "$manifest" | sort -u)
done
if ((${#missing[@]})); then
  printf 'oracle-regress: path dependencies missing from SYNC_CRATES:\n' >&2
  printf '  %s\n' "${missing[@]}" >&2
  unreachable "add them to SYNC_CRATES so the remote workspace resolves"
fi

for c in "${SYNC_CRATES[@]}"; do
  rsync -a --delete --exclude 'target/' -e "ssh ${SSH_OPTS[*]}" \
    "$REPO/crates/$c/" "$HOST:$REMOTE/crates/$c/" \
    || unreachable "rsync of crates/$c failed"
done
scp -q "$REPO/tools/oracle/remote-Cargo.toml" "$HOST:$REMOTE/Cargo.toml" \
  || unreachable "could not install the remote workspace manifest"

# --------------------------------------------------------------------------------------
# Build and run. Co-tenant box: pinned to four cores, niced, no package installs.
# --------------------------------------------------------------------------------------
RELFLAG=""
[ "$BUILD_PROFILE" = release ] && RELFLAG="--release"
echo "oracle-regress: building ($TARGET, $BUILD_PROFILE) on $HOST"
if ! ssh "${SSH_OPTS[@]}" "$HOST" \
  "cd $REMOTE && ${NICE[*]} cargo build $RELFLAG --target $TARGET --bin regress 2>&1" \
  | tee "$LOG"; then
  echo "oracle-regress: BUILD FAILED on $HOST — see $LOG"
  echo "oracle-regress: NOTHING WAS TESTED."
  exit 4
fi

ARGS="${PASSTHRU[*]:-}"
REMOTE_JSON="$REMOTE/oracle-regression.json"
echo "oracle-regress: running the suite"
ssh "${SSH_OPTS[@]}" "$HOST" \
  "cd $REMOTE && ${NICE[*]} ./target/$TARGET/$BUILD_PROFILE/regress --json $REMOTE_JSON $ARGS 2>&1" \
  | tee -a "$LOG"
RC=${PIPESTATUS[0]}

mkdir -p "$REPO/schema"
if scp -q "$HOST:$REMOTE_JSON" "$OUT"; then
  echo "oracle-regress: wrote $OUT"
else
  echo "oracle-regress: could not fetch the JSON record from $HOST:$REMOTE_JSON"
  [ "$RC" = 0 ] && RC=4
fi

echo "oracle-regress: exit $RC  (0 all-ran-and-passed | 1 mismatch | 2 skipped | 3 harness | 4 unreachable)"
exit "$RC"
