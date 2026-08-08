#!/usr/bin/env bash
# Build the wasm shim and drop it where the page expects it.
#
# There is no bundler, no npm install, no transpile. The page is ES modules served as-is,
# so the only build artefact in the whole project is `don_web.wasm`.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cd "$here/wasm"
cargo build --release --target wasm32-unknown-unknown

src="$here/wasm/target/wasm32-unknown-unknown/release/don_web.wasm"
dst="$here/public/wasm/don_web.wasm"
mkdir -p "$(dirname "$dst")"
cp "$src" "$dst"

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

printf 'built %s (%s bytes)\n' "$dst" "$(wc -c < "$dst" | tr -d ' ')"
