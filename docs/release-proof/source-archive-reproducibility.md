# Source-archive product-Wasm reproducibility

`release/source-archive-reproducibility.json` is the build-produced link between the public
source projection and the checked-in browser Wasm candidate. Capture extracts a clean
`git archive HEAD`, runs the repository's canonical `web/build.sh`, and refuses unless the final
post-optimization bytes exactly equal `web/public/wasm/don_web.wasm` from that archive.

The current exact result is a 975,828-byte raw rustc module at SHA-256
`0ddd4285f313911962199dc671f2677b8952ae9aa328fe93771f2961c99b6327`, transformed by
Binaryen 130 into the 771,141-byte checked-in candidate at SHA-256
`b7c71e5db4b6ceaa94c3f32bcc91a1004cea667e6367c0326f8ba6e5e8b679c5`.

The artifact binds:

- the candidate, component-provenance record, Wasm manifest, lockfile, build script, generator
  inputs, generated contracts, ABI checker inputs, Rust/Cargo/Node/Binaryen toolchain, and both the
  raw rustc and final optimized Wasm identities;
- every repository source file named by rustc's build-produced `don_web.d`, with an aggregate
  source-closure digest (141 files in the current build);
- the absence of `schema/live/live-tables-unit.tsv` from both `git archive` and the depfile, plus
  the removal of its former compile-time `include_str!` consumer;
- an isolated `cargo test --locked -p don-sim --lib` result from that same clean source archive.

The exact product Wasm and 1,579 passing `don-sim` library tests (2 ignored) therefore build without live or owned
retail inputs. This is not a whole-workspace/all-targets archive test, and the artifact forces
that broader claim false. It supplies no legal review, third-party obligation decision, independent
presentation-content clearance, installer proof, or whole-product payload claim.

Capture performs the isolated build and writes the artifact only on a byte-identical result:

```sh
python3 tools/release-proof/archive_reproducibility.py probe \
  --output release/source-archive-reproducibility.json
```

Normal verification is offline. It re-hashes the current committed candidate, all recorded
pipeline and depfile inputs, the live-input exclusion, and the test-only blocker without compiling
or reading excluded inputs:

```sh
python3 tools/release-proof/archive_reproducibility.py verify \
  --artifact release/source-archive-reproducibility.json
python3 -m unittest tools/release-proof/test_archive_reproducibility.py
```

Any compiled Rust source or pipeline input change invalidates the record and requires a clean
archive rebuild. Refreshing hashes without reproducing the candidate is not an admissible update.
The next archive-hardening tranche should extend the same clean projection to the complete
workspace/all-targets test matrix.
