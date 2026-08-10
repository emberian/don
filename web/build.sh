#!/usr/bin/env bash
# Build the wasm shim and drop it where the page expects it.
#
# There is no bundler, no npm install, no transpile. The page is ES modules served as-is;
# the generators are checked source contracts and `don_web.wasm` is the compiled artefact.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd "$here/.." && pwd -P)"

# Cargo incorporates the canonical filesystem location of every path dependency into crate
# disambiguators. `--remap-path-prefix` hides source paths in the finished module, but cannot by
# itself make a checkout build and a `git archive` build byte-identical: their dependency package
# IDs still differ. Build the actual graph from one fresh, locked physical root. The outer
# invocation copies only source inputs there; the inner invocation performs the ordinary checks
# and compilation, then the outer invocation atomically publishes the resulting module.
if [[ "${DON_WEB_CANONICAL_INNER:-0}" != 1 ]]; then
  canonical_root=/tmp/don-web-canonical-source-v1
  canonical_lock=/tmp/don-web-canonical-source-v1.lock
  if ! mkdir "$canonical_lock" 2>/dev/null; then
    echo "refusing concurrent or stale canonical Web build: $canonical_lock exists" >&2
    exit 2
  fi
  if [[ -e "$canonical_root" ]]; then
    rmdir "$canonical_lock"
    echo "refusing stale canonical Web source root: $canonical_root exists" >&2
    exit 2
  fi
  umask 077
  mkdir "$canonical_root"
  # Invoked through the EXIT/INT/TERM trap below.
  # shellcheck disable=SC2329
  cleanup_canonical_build() {
    rm -rf -- "$canonical_root"
    rmdir "$canonical_lock" 2>/dev/null || true
  }
  trap cleanup_canonical_build EXIT INT TERM

  rsync -a \
    --exclude='.git/' \
    --exclude='target/' \
    --exclude='web/public/wasm/don_web.wasm' \
    "$repo_root/" "$canonical_root/"

  DON_WEB_CANONICAL_INNER=1 "$canonical_root/web/build.sh"

  canonical_wasm="$canonical_root/web/public/wasm/don_web.wasm"
  dst="$here/public/wasm/don_web.wasm"
  mkdir -p "$(dirname "$dst")"
  tmp="$(mktemp "${dst}.canonical.XXXXXX")"
  cp "$canonical_wasm" "$tmp"
  mv -f "$tmp" "$dst"
  printf 'published canonical %s (%s bytes)\n' "$dst" "$(wc -c < "$dst" | tr -d ' ')"
  exit 0
fi

# The browser packet codec and replay card are generated source contracts. Refuse a build
# when either has drifted from its authoritative schema rather than pairing fresh Wasm with
# stale JavaScript/Rust glue.
node "$here/tools/gen-wire.mjs" --check
node "$here/tools/gen-readiness.mjs" --check

cd "$here/wasm"
# Rust includes source locations in panic/debug strings even in this stripped release profile.
# Canonicalize the repository prefix so an exact source archive built under /tmp and the same
# commit built in a checkout produce byte-identical Wasm. Ignore ambient encoded flags: release
# evidence must describe this build contract, not an operator's shell customization.
unset CARGO_ENCODED_RUSTFLAGS
RUSTFLAGS="--remap-path-prefix=${repo_root}=/don-src -Cmetadata=don-web-repro-v1" \
  cargo build --locked --release --target wasm32-unknown-unknown

src="$here/wasm/target/wasm32-unknown-unknown/release/don_web.wasm"
dst="$here/public/wasm/don_web.wasm"
mkdir -p "$(dirname "$dst")"
cp "$src" "$dst"

# Rust source can advance without the checked-in browser artefact. Verify the copied module
# before optimization so an old/missing export or an accidental team/victory setter cannot
# reach the page under a misleading generic boot failure.
node "$here/tools/check-play-wasm.mjs" "$dst"

# wasm-opt is a nice-to-have, never a requirement; the module is already tiny because it
# has no bindgen glue and no panic machinery (`panic = "abort"`, `strip = true`).
# (This Homebrew binaryen rejects the module — "error validating input" — because the
# toolchain emits features it predates. The `if` reports what actually happened rather than
# announcing success unconditionally, which is how an unoptimised module shipped once.)
if command -v wasm-opt >/dev/null 2>&1; then
  if wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int \
       --enable-sign-ext "$dst" -o "$dst.opt" 2>/dev/null; then
    mv "$dst.opt" "$dst"
    echo "wasm-opt applied"
  else
    rm -f "$dst.opt"
    echo "wasm-opt declined this module; shipping the rustc output"
  fi
fi

# Optimization is allowed to rewrite the module, never its public contract.
node "$here/tools/check-play-wasm.mjs" "$dst"

printf 'built %s (%s bytes)\n' "$dst" "$(wc -c < "$dst" | tr -d ' ')"
