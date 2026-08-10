# Source-archive reproducibility blocker

`release/source-archive-reproducibility.json` records an isolated build of the exact public
`git archive` projection. It is a negative, fail-closed artifact: it proves why the checked-in
browser Wasm cannot currently be linked back to distributable source. It does not put the
missing input into the archive and does not promote any release gate.

The 2026-08-09 probe used the recorded Rust/Cargo toolchain and this canonical command inside a
fresh temporary extraction:

```sh
cargo build --locked --release --target wasm32-unknown-unknown \
  --manifest-path web/wasm/Cargo.toml
```

Compilation stopped with exit code 101. `crates/don-sim/src/systems/leaders.rs` contains a
compile-time `include_str!` for `schema/live/live-tables-unit.tsv`, while `.gitattributes`
correctly marks live TSV captures `export-ignore`. The archive contains the Rust consumer and
omits the retail/live-derived table. The artifact binds the consumer, required input, candidate
Wasm, component provenance, manifest, lockfile, include site, export state, toolchain, and
normalized failure classification by size and SHA-256 where applicable.

This is a real distribution blocker, not an invitation to ship the TSV. The table contains
post-load retail type facts and remains outside a public archive. The minimal source-side repair
is to remove the compile-time include from the normal `don-sim` build and inject the derived
type-stat rows through an explicit runtime/content authority boundary. A clean archive must
compile with that provider absent, and the affected mechanic must remain unresolved/fail-closed
instead of substituting plausible rows. Local fidelity runs may load an exact owned-input
provider separately. That source repair belongs to the simulation lane; this release artifact
only records and guards the boundary.

Capture reruns the isolated build and writes the negative artifact only if it fails at the exact
known input:

```sh
python3 tools/release-proof/archive_reproducibility.py probe \
  --output release/source-archive-reproducibility.json
```

Normal verification is offline and re-derives hashes, the `include_str!` site, `export-ignore`,
and a selective `git archive` projection without compiling or accessing retail:

```sh
python3 tools/release-proof/archive_reproducibility.py verify \
  --artifact release/source-archive-reproducibility.json
python3 -m unittest tools/release-proof/test_archive_reproducibility.py
```

When the simulation boundary is repaired, verification will deliberately fail because the
recorded blocker is no longer true. The next release wave must then replace this negative record
with a successful archive rebuild whose output is byte-linked to the assembled candidate; it
must not merely refresh hashes or weaken `export-ignore`.
