# Installation and owned-game data boundary

Descent of Nations is currently a source development snapshot, not a finished standalone
installer. The Rust workspace, synthetic tests, and research tooling can be built from this
repository, but the playable browser integration still needs rule and type data extracted
locally from a legally owned copy of *Rise of Nations: Extended Edition*.

## What the repository does and does not include

DoN source is licensed under GPL-3.0-or-later unless a file or subdirectory says otherwise.
Microsoft/Big Huge Games executables, PDBs, rules, scripts, replays, art, audio, live-process
captures, and packs generated from those inputs are not covered by that license and are not
release payloads.

This repository does **not** yet provide a finished standalone game installer. It now does
provide a supported, deliberately narrower owned-data bootstrap for the exact retail build:

```sh
python3 tools/install/bootstrap.py check \
  --retail-root '/absolute/path/to/Rise of Nations'
python3 tools/install/bootstrap.py install \
  --retail-root '/absolute/path/to/Rise of Nations' \
  --workspace /absolute/path/to/don --dry-run
python3 tools/install/bootstrap.py install \
  --retail-root '/absolute/path/to/Rise of Nations' \
  --workspace /absolute/path/to/don
```

The manifest at [`tools/install/owned-inputs.json`](../tools/install/owned-inputs.json) binds
the supported executable identity and 51 shipped data/script inputs by relative path, byte size
and SHA-256. The executable is verified but never copied. Source paths are resolved
case-insensitively for the Windows install layout, with ambiguous names and symlinks refused.
All source bytes and every existing destination are checked before the first write. The installer
then creates only missing exact files beneath the checkout's already-ignored `ron-data/`; it never
overwrites or deletes a user file, downloads content, writes outside that root, or packages the
result. Re-running it is idempotent. `check` and `install --dry-run` are non-mutating.

This closes the reproducible local-input bootstrap boundary, not the product-install boundary.
The commands in [`docs/binary-ground-truth.md`](binary-ground-truth.md) remain provenance notes,
not a portable installer. The current browser still depends on live-derived tables that this
static owned-data bootstrap cannot manufacture honestly, and the independent game is not yet a
complete release. A distributable edition must still either:

1. extract the necessary inputs on the user's machine from a legally owned install without
   redistributing them; or
2. use independently licensed replacement data, art, and audio.

Until that boundary is implemented, do not publish locally populated `ron-bin/`, `ron-data/`,
`schema/live/`, or `web/public/data/` trees.

## Building the source snapshot

Install a current Rust toolchain, then run the source gates from the repository root:

```sh
cargo check --workspace
cargo test --workspace --all-targets
```

The Python extension and browser integration have additional toolchain requirements described
in [`python/README.md`](../python/README.md) and [`web/README.md`](../web/README.md). The browser
pack commands named there consume already-extracted, local inputs; they do not acquire or
license those inputs.

## Auditing a source-release candidate

Audit an exact assembled staging directory before publishing it:

```sh
tools/release-audit.sh --staging-dir /absolute/path/to/exact-release-tree
```

That mode walks every file, including ignored and untracked files. It is the appropriate gate
after assembling a release directory. It does not modify the directory.

To inspect only the files tracked by an explicit Git commit or ref, generate and audit a
temporary Git archive:

```sh
tools/release-audit.sh --git-archive HEAD
```

The source-package manifest is [`.gitattributes`](../.gitattributes). Its `export-ignore`
rules keep raw and bulk retail-derived evidence out of `git archive` while leaving that
evidence available to developers in repository history. Compact, deliberately tracked
`schema/live/retail-*.json` protocol and proof fixtures remain eligible for the source
archive; the audit refuses one once it exceeds the documented compact-evidence limit. Adding
an archive exclusion does not relicense an artifact and does not make it a release payload.

The archive mode deliberately cannot see untracked or ignored workspace files. Use the
staging-directory mode on the final assembled tree as the last gate. Both modes fail closed
and explain each path they refuse, including retail input roots, bulk live captures, generated
browser data, replays/saves, compiled injector outputs, disguised PE/PDB/DoN pack payloads,
and content matching the supported retail executable hash.

Run the audit's synthetic regression suite with:

```sh
tools/release-audit-fixtures/test.sh
```

The tests generate only temporary synthetic text and signatures; they do not copy or require
retail content.
