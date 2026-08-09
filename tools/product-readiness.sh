#!/usr/bin/env bash
# Fail closed unless both canonical editions are admissible on every non-research product
# surface. Research harnesses are intentionally outside this gate; they cannot be shipped
# or cited as fidelity evidence, but they need not be feature-complete to remain useful.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/target/release/don-deviations"

cargo build --release -p don-sim --bin don-deviations --manifest-path "$ROOT/Cargo.toml" -q

status=0
for mode in fidelity improved; do
  if ! "$BIN" --mode "$mode" --assert-ready product; then
    status=3
  fi
done

if [[ "$status" -ne 0 ]]; then
  echo >&2
  echo "product readiness failed; the blockers above must be removed, not waived" >&2
fi
exit "$status"
