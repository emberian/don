# Replay channel-12 byte-owner frontier

## Result

`crates/don-replay/src/world_owner_frontier.rs` replaces the ambiguous idea of
“N sourced World bytes” with an isolated, typed byte-owner ledger over the exact
`World::walk_data(-1)` stream. It is not wired into the shared replay bridge yet; this
tranche deliberately avoids the live replay schedule and library convergence surface.

The ledger does four bounded things:

1. captures all thirteen retail sections independently, then proves they concatenate to
   the same image, byte count and adler-32 value as the complete walker;
2. admits the initial replay prefix as exact ranges, not a scalar count;
3. advances ownership across a receipt-bound exact-port transition, assigning only bytes
   which actually changed and rejecting any write outside the producer's declared section
   set; and
4. keeps a recorded checksum in the comparator role. It cannot own or localise a byte. A
   captured retail walk may localise the first difference only after its image hashes to a
   peer-agreed recorded channel-12 value.

The focused integration tests live in the exclusive
`crates/don-replay/tests/world_owner_frontier.rs` file and path-include the frontier until
the shared library owner chooses to expose it.

## The first substantive mismatch

The 61-file corpus artifact `schema/replay-validation.json` contains 21 recordings with
checksums. Across those recordings:

| fact | measured value |
|---|---:|
| files | 61 |
| files with `CheckSumsCommand` | 21 |
| channel-12 comparisons | 222,938 |
| nontrivial model walks | 222,938 |
| matches | 0 |
| first divergence | turn 2 in all 21 files |
| prefix-owned bytes per walk | 76 |

The lexically first checksummed recording is
`Playback___2018.11.17_13_21_42__Sat_.rcx`. At turn 2 retail records
`0xd63a3a53`; the prefix-only model produces `0x1389cbbb`. The model walks 780,168 bytes,
of which 780,092 are unsourced. This is the first substantive channel-12 mismatch in the
current corpus: both sides are nonempty, all same-group retail peers agree, and the model
does not claim agreement.

The remaining 20 exact `(retail, model, walked, unknown)` tuples are frozen in the focused
test. Every row has `walked - unknown == 76` and `retail != model`.

## Exactly which 76 bytes are owned

The prefix constructor admits four disjoint ranges:

| section | section offset | bytes | source |
|---:|---:|---:|---|
| 1 | 0 | 8 | `xs`, `ys`, derived from the one-byte replay map-size selector through the shipped `[40,50,60,70,80,90,100]` table |
| 4 | 0 | 40 | `size` through `reg_size`, exact `World::init` `0x006b76f0` arithmetic over that edge |
| 4 | 48 | 24 | both territory-limit triplets, admitted only with Rules checkpoints `after_constants == 0x50625668` and final `0x12ba3104` |
| 4 | 116 | 4 | `World::seed`, bit-copied from the four-byte replay `GameInfo::seed` span by `Map::make` `0x0068bc90` |

Map/sea-map words, generated counters, start/oil arrays, WData, TData/fog, danger,
collision blocks and the four Terrain arrays remain unknown. Zero is a value, not a source.
If the Rules projection is absent, its 24 bytes also remain unknown instead of being
inferred from a checksum target.

Each replay claim is bound to an exact, non-overlapping decompressed source span and a
nonzero content-digest identity. The Rules claim additionally requires the measured
serialized size 1,024,221, both independently reconstructed checkpoints, and the shipped
`44/4/4` territory triplet. A wrong selector, seed width, missing digest, overlapping
source, checkpoint, territory value, or current World value fails before a claim is
installed. Digest identities are caller-carried evidence in this isolated adapter; it
validates their shape but does not reopen the copyrighted source to authenticate them.

## Generator transitions

An `ExactPortTransitionProof` binds:

- entry and resume VAs;
- implementation and receipt SHA-256 identities;
- a proof-document identity;
- the exact before/after full World checksums; and
- an explicit set of sections the routine is allowed to mutate.

The adapter captures the after-image transactionally, rejects changed walk shape or any
out-of-scope byte, coalesces changed bytes into section-local ranges, and transfers only
those bytes to the new producer. Previous owners survive on unchanged bytes; previously
unknown neighbouring zeroes remain unknown. This is intentionally narrower than saying
“the completed function owns its whole output domain”: proving a routine ran does not prove
that every unchanged byte has the right value or provenance.

The first consumer should be the receipt chain already reconstructed under
`Map::make`: continent/common tail, fertility, `place_all`, `check_player_forest`, and
`nubify_forest`. Each receipt already carries before/after `WorldChecksum` values and a
known section scope. The ledger supplies the missing byte-level composition between them.

## Retail-walk capture contract

`World::walk_data` is fixed by the shipped PDB and the complete body at
`re/decomp-all/006b5cf0.c`: thirteen section guards, with channel 12 conditionally entered
when `World+0x134` (`wdata`) is non-null. A future live capture must therefore provide the
actual visitor byte stream, not a memory dump of the 372-byte owner object (most walked
data is pointer-owned).

The capture adapter requires:

- a nonzero retail executable digest;
- a nonzero capture digest;
- two or more same-group peer values which agree at the recorded turn; and
- `adler32(1, captured_walk) == recorded_world_checksum`.

Only then does it report a first global byte and map that byte back to a section-local
offset. The checksum-only corpus cannot do this: adler-32 disagreement does not reveal the
first wrong byte. This distinction preserves the current honest stopping point while
making the next retail capture immediately useful.

## Evidence and claims not made

Primary structural truth:

- PDB `World::walk_data(DataWalk*, int)`, VA `0x006b5cf0`;
- `re/decomp-all/006b5cf0.c`, complete thirteen-section body;
- `World` / `WorldData` PDB layouts and the recovered walker table;
- `Map::make` seed stores at `0x0068bcc8` and `0x0068bcd0`;
- `World::init` dimension arithmetic at `0x006b76f0`; and
- replay Rules checkpoints `0x50625668` / `0x12ba3104`.

This tranche does **not** claim a channel-12 match, a complete generated map, provenance for
unchanged generator outputs, or a captured retail walk. It does not add the module to
`lib.rs`, alter `state.rs`, change cached checksums, change replay timing, or move the
scoreboard. Its closure delta is zero; it turns the existing 76-byte scalar into a precise
and composable ownership contract.

## Validation boundary

Per lane order, no Cargo, rustc, formatter, build, stage, commit or push was run. The only
local gate for this isolated pack is static `git diff --check`. The convergence owner should
wire the module, then run the focused integration test before adopting it in `SimBridge`.

The isolated owner ledger passed both independent profiles on 2026-08-09: hbox
`replay-world-owner-20260809T223815Z-77949-24644-fede3d11b0b3` and persvati release
`replay-world-owner-release-20260809T223816Z-77954-6016-fede3d11b0b3`, each with 6/6 tests
and exit 0. Warnings are confined to evidence-only path-import items; channel 12 remains red.
