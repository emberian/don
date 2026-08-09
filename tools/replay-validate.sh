#!/usr/bin/env bash
# Re-run the replay-driven validation over the whole recording corpus and
# refresh schema/replay-validation.json.
#
#   tools/replay-validate.sh              # full corpus, rewrite the record
#   tools/replay-validate.sh --status     # age + headline of the last record
#   tools/replay-validate.sh --limit 5    # quick pass over the first 5 files
#   tools/replay-validate.sh --phase after
#
# Exit codes, mirroring tools/oracle-regress.sh:
#   0  ran and produced a record
#   2  the corpus is missing (SKIPPED, which is never a pass)
#   3  the harness itself failed
#
# The number that matters is per-channel `survived`: consecutive turns on which
# our checksum channel equalled the one retail recorded. It is expected to be
# small. It is the project's progress metric, so it is written to schema/ and
# tracked run over run rather than printed and forgotten.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/schema/replay-validation.json"

if [[ "${1:-}" == "--status" ]]; then
  if [[ ! -f "$OUT" ]]; then
    echo "no record at $OUT — run tools/replay-validate.sh"
    exit 2
  fi
  echo "record: $OUT"
  if stat -f %Sm "$OUT" >/dev/null 2>&1; then
    echo "written: $(stat -f %Sm "$OUT")"
  else
    echo "written: $(stat -c %y "$OUT")"
  fi
  python3 - "$OUT" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
t = d["totals"]
print(f"files {t['files']} ({t['files_with_checksums']} with checksums), "
      f"turns {t['turns']}, checksum packets {t['checksum_packets']}")
per = t["per_channel"]
best = max(((k, v["best_survived_turns"]) for k, v in per.items() if k != "all"),
           key=lambda kv: kv[1])
print(f"HEADLINE: {best[1]} turns survived on channel `{best[0]}`")
for k, v in per.items():
    if v["best_survived_turns"]:
        print(f"  {k:<16} best {v['best_survived_turns']:>6}  "
              f"matches {v['matches']}/{v['compares']}  trivial {v['trivial']}")
PY
  exit 0
fi

if ! compgen -G "$ROOT/ron-data/replays/**/*.rcx" >/dev/null 2>&1 \
   && [[ -z "$(find "$ROOT/ron-data/replays" -name '*.rcx' 2>/dev/null | head -1)" ]]; then
  cat >&2 <<'EOF'

  SKIPPED — NOT A PASS. No .rcx under ron-data/replays/.
  ron-data/ is gitignored copyrighted game content. Without it this script
  establishes nothing at all; a vacuous green is the failure mode the whole
  harness exists to prevent.

EOF
  exit 2
fi

cargo build --release -p don-replay --manifest-path "$ROOT/Cargo.toml" -q || exit 3
exec "$ROOT/target/release/don-replay" validate --corpus --quiet --json "$OUT" "$@"
