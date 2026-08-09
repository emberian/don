# BHS type session owner

Status: source-complete opaque integration seam; synchronized rules composition, channel 13,
and DoNSave ownership remain red. Implementation: `crates/don-sim/src/bhs_session.rs`. Frozen
tests: `crates/don-sim/tests/bhs_session_owner.rs`.

## Result

`BhsSession` is the first production-shaped owner that keeps all of these values in one lifetime:

- the unique, non-cloneable `Sim`;
- the persistent `ScriptRuntime` consumed at retail tick step 4;
- the factory-admitted `TypeBuiltinState` installed inside that runtime; and
- the exact `TypeBuiltinProvenance` returned with that state.

Construction accepts the three witnessed components required by
`produce_type_builtin_state`. It rejects a `ScriptRuntime` that already contains a type owner or
has already entered step 4, runs the canonical factory, retains the returned provenance, installs
the returned state, and only then returns the session. `ScriptRuntime` sets its setup latch before
running either binding, so even a failed first script attempt cannot later acquire the owner. A
state installed earlier through the legacy state-only runtime API cannot be paired after the fact
with an unrelated provenance witness.

This is an ownership fix, not a new type-data derivation. The retail executable/PDB/data anchors,
806/24/8 cardinalities, canonical relation requirements, immutable-backup capture, and exact BHS
handler bodies remain the ones frozen in `bhs-type-factory.md` and `bhs-type-table.md`.

## Why the owner is opaque

Before this seam, three separately valid APIs could be composed incorrectly:

1. `Sim::do_frame()` could execute a script-free frame while a neighboring `ScriptRuntime`
   carried the canonical type state;
2. `save_sim(&Sim)` could serialize DoNSave v6 without seeing that external mutable owner; and
3. `Sim::channel_digest()` could emit the current partial digest without admitting missing retail
   checksum channel 13.

`BhsSession` consumes both values and exposes no `Sim` accessor, `Deref`, `AsRef`, `Clone`, or
`into_parts` escape. Since `Sim` itself is not cloneable, there is no second handle on which the
legacy APIs can be called after installation. The session exposes only:

- `do_frame`, which always calls `Sim::do_frame_with_scripts`;
- `save_v6`, which always calls the combined BHS admission before the v6 writer; and
- `partial_channel_digest`, which always calls the BHS channel-13 admission before the existing
  partial digest.

Read-only status, provenance, type-state, and last-receipt views do not expose the enclosed `Sim`
or permit replacement of the source witness.

## Honest red boundaries

The owner does not fabricate channel 13 and does not serialize unsupported state.

- A pristine session refuses `save_v6` with
  `SaveOwnerUnowned { mutation_revision: 0, dirty: false }`.
- A mutated session refuses with the live revision and dirty bit.
- Every session refuses `partial_channel_digest` with `Channel13ProjectionUnowned`.

The provenance is retained so a future complete loader can prove which synchronized rules/mod
composition created the immutable restore backups. Retention alone does not make DoNSave v6 able
to reconstruct those backups, and it is not mixed into a made-up checksum. The executed shipped
call coverage of this source-only seam therefore remains zero until a real synchronized composer
constructs the input and production creates `BhsSession`.

## Frozen source checks

The proof pack freezes four ownership claims:

1. the factory identity is retained before frame zero, and builtin 290 reaches that exact owner on
   the first script frame;
2. save and partial-digest admission refuse both before and after the mutation;
3. an already populated, provenance-free runtime is rejected;
4. any runtime which already attempted step 4 is rejected, including a failed attempt; and
5. mixed factory witnesses fail before a session is exposed.

Root convergence formatted the new owner/test files and validated all five focused tests in both
modes:

- hbox debug: `bhs-session-owner-20260809T225238Z-90379-20593-cb8b2b915d8e`;
- persvati release: `bhs-session-owner-release-20260809T225238Z-90395-6986-cb8b2b915d8e`.

Neither job ran retail. Runtime coverage, save completeness, and checksum fidelity are not
claimed.
