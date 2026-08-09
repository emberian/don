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

This repository does **not** yet provide an end-user installer or a supported owned-install
extractor/bootstrap. The commands in
[`docs/binary-ground-truth.md`](binary-ground-truth.md) document the provenance of the
project's local research corpus; they are not a complete, portable installer and should not
be presented as one. A future distributable edition must either:

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
