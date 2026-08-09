#!/usr/bin/env bash
# Build the native extension and drop it next to the Python package.
#
# maturin is not required: the crate carries a build.rs that emits the one macOS link
# argument a CPython extension needs, so plain cargo is enough.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${1:-release}"

flags=(--features extension-module -p don-env)
[ "$profile" = "release" ] && flags+=(--release)

cargo build --manifest-path "$root/Cargo.toml" "${flags[@]}"

case "$(uname -s)" in
  Darwin) built="$root/target/$profile/libdon_env.dylib" ;;
  *)      built="$root/target/$profile/libdon_env.so" ;;
esac

dst="$root/python/don_env/_don_env.so"
cp "$built" "$dst"
echo "built $dst"
python3 - <<'PY'
import sys, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[0]))
PY
PYTHONPATH="$root/python" python3 -c "
import don_env, numpy as np
e = don_env.DonVecEnv(num_envs=2, num_agents=2, grid_w=32, grid_h=32)
print('import ok:', e.spec['num_envs'], 'envs,', len(e.unit_heads), 'unit heads,',
      len(e.spec['unit_verbs']), 'unit verbs')
"
