# Sparse Objects/Units band authority frontier

Status: source-only side-by-side owner for stable retail `(who,o)` identity. The isolated
implementation is
`crates/don-sim/src/systems/sparse_object_bands_authority_frontier.rs`; its path-import proof is
`crates/don-sim/tests/sparse_object_bands_authority_frontier.rs`. It is intentionally absent
from `systems/mod.rs` and `World` until the migration cuts below are owned together.

## The two identities that must not be conflated

Retail addresses an object by `(owner,o)`, where `o` is a stable signed-16-bit index inside one
of three bands:

| band | range | high-water owner |
|---|---:|---|
| Unit | `[0,2000)` | `unit_mark[10]`, `ObjectsData +0x15C` |
| Build | `[2000,3000)` | `build_mark[10]`, `ObjectsData +0x184` |
| Wall | `[3000,32768)` | `wall_mark[10]`, `ObjectsData +0x1AC` |

The Wall upper bound here is only the structural exclusive limit of the signed-16-bit retail
object-index field. This tranche does not claim that every retail build exposes that many Wall
slots; an installed owner must take any recovered runtime Wall capacity as configuration.

DoN stores live Unit columns densely and moves the last row into a removed row. Its generational
`Handle { id,generation }` survives that move. The canonical join is therefore:

```text
(owner, retail o) -> stable generational identity -> current dense row
```

The sparse owner stores the middle identity, never the row. A caller-supplied resolver maps it
to the current row. The proof moves one identity's row from 19 to 3 without changing either the
registry or its retail address.

The generic identity parameter is deliberate. Unit integration can use the structural
`DenseIdentity` matching `World::Handle`; a full three-band integration needs a tagged identity
enum once Build and Wall stable identities are selected.

## Slot storage, projections, and lifecycle

Every retained slot owns two distinct pointer analogues:

- `object` models `Objects::lists[owner][o]`;
- `unit` models `Units::lists[owner][o]` and exists only for the Unit band.

Player slots 0--7 construct Unit storage; slots 8--9 construct Animal storage. Both carry a
Unit projection. Build and Wall storage has no Unit projection. These storage tokens survive
retirement, high-water reduction, dense compaction, and later reuse. They are process-local
pointer analogues, not simulation identity, so the save-shaped snapshot deliberately rebuilds
them rather than serializing their numeric values.

A slot is one of:

- `Live(stable_identity)`;
- `Tombstone(flags,hold_frames,is_unit,o_up)`, retaining the exact fields used by
  `Objects::find_free` and the inactive hold countdown;
- `Reserved(ticket,prior_tombstone)` between `find_free` and the complete object initializer.

Reservations prevent two joined callers from selecting the same reusable slot. Cancelling a
reservation restores its prior tombstone but deliberately does not undo `find_free`'s storage
construction or high-water advance. That mirrors the non-atomic boundary: an adapter fault is
not permission to invent native rollback. Outstanding reservations are rejected by the
save-shaped snapshot.

## Reuse and high-water behavior

`find_free` scans from the band base to the independent owner/band mark and selects the first
tombstone satisfying:

```text
flags & 1 == 0
hold_frames == 0
not is_unit OR o_up < 0
```

Reuse leaves the mark unchanged. If no reusable slot exists, the mark slot is selected and the
mark advances. Missing storage is constructed with both required registrations; retained
storage beyond a previously lowered mark is selected without reconstruction. At the Unit limit
of 2,000, a fully ineligible scan returns exactly `-1`.

This side owner deliberately exposes the canonical first-free transaction only. The separate
exact-`o` placement path used by create-unit callers remains at the `Objects::init_unit` join and
must not be approximated by forcing this scan to a requested index.

`lower_mark` changes only the mark and refuses to hide any Live or Reserved slot. Tombstone
storage remains addressable beyond the new mark so a later extension recovers the same object
and Unit-projection identities. The three bands and all ten owners hold independent marks.

Traversal enumerates every slot below a mark, including tombstones whose hold count remains
live behavior. Unit owners rotate as `(frame+i)%10`; Build and Wall owners remain fixed 0--7.

## Dense conversion

`from_dense_entries` is the one-way compatibility seam for the current registry. It accepts
gap-free current `(owner,band,o)` entries paired with stable identities, sorts by retail
address, rejects gaps/duplicates, creates parallel storage, and reports the resulting 30 marks.
No dense row is retained.

This is safe while the old registry is gap-free. Once the first tombstone exists, deriving the
sparse owner from live rows would lose future allocator state and must be forbidden.

## Save/load audit

Current `WorldSaveState::validate_and_rebuild` cannot preserve this owner:

- it derives Unit bands solely from live rows' `who/o` columns;
- it requires every index below the largest live `o` to be present;
- it rejects `o >= live`, even though a stable sparse index is independent of global live-row
  count;
- it reconstructs a dense `ObjectRegistry` by append;
- format version 7 serializes live Unit columns and Build row vectors, but no Unit marks,
  retained tombstones, or parallel projection shape.

The isolated `SparseRegistrySnapshot` is only the allocation-owner portion of a future save:
active bits, all 30 marks, every retained slot lifecycle, and stable live identities. It omits
full Unit/Build/Wall bodies, so it is not claimed as a complete save section.

Integration requires a format-version bump and a fail-closed migration rule:

1. version-7 dense saves may be converted exactly because they contain no representable
   tombstones;
2. the new format must serialize marks and retained lifecycle before body reconciliation;
3. load must recreate storage/projection tokens, bind every Live identity exactly once, and
   prove each live body agrees with its `(who,o)`;
4. no loader may fall back to rebuilding sparse state from live rows.

## Checksum audit

The current `World::digest` hashes live Unit rows, keyed by stable Handle, but not object-band
marks or tombstones. Two worlds can therefore have identical live rows and different next
`find_free` results while producing the same digest.

Retail confirms the missing state is checksum/save relevant. `Objects::walk_data` at
`0x006541E0` walks:

- `good_mark/rare_mark`, `[Objects+0x154,Objects+0x15C)`;
- all ten Unit marks, `[+0x15C,+0x180)`;
- all ten Build marks, `[+0x184,+0x1A8)`;
- all ten Wall marks, `[+0x1AC,+0x1D0)`;
- the adjacent object counters, `[+0x1D4,+0x1E6)`;
- the per-owner object arrays through their own walker.

A shared installation must add the canonical sparse snapshot shape to the composed digest and
then converge it with the retail object walkers. Numeric storage tokens and reservation tickets
must not be hashed; marks, tombstone fields, projection presence/class, and live address/body
identity must be.

## Runtime consumer audit

The current dense assumptions are broad and cannot be changed piecemeal:

- `World::despawn` calls `ObjectRegistry::remove`, changes another object's retail `o`, then
  repoints the dense row. The sparse path must instead retire the exact address, compact only
  the row, and leave the moved Handle's `(owner,o)` unchanged.
- `ObjectRegistry::traversal_into` emits only live rows. The canonical traversal must also emit
  tombstones for hold countdown and resolve Live stable identities to their current rows.
- `script_runtime::script_object`, force-transport scans, BHS stat mirrors, `AmmoView`, and many
  tick passes use `slot.band(...).get(o)` or zip a dense band with live views. Each must resolve
  an exact sparse address or intentionally iterate marked slots.
- `World::allocate_typed_at` appends one row and one dense band entry. It neither reserves nor
  commits canonical storage and remains insufficient for `Objects::init_unit`.
- production's `SimFinishedHost::allocate_unit` delegates to that single-row allocator and
  returns only an `object_id`; its claimed `Objects::init_unit` boundary must be replaced by the
  detailed complete receipt.
- the installed BHS create-unit runtime still stops every positive route at
  `PositiveAllocationAuthority`; the detailed BHS allocator frontier is not registered.

## Safe shared integration order

The smallest non-divergent migration is:

1. add the sparse owner beside the dense registry and populate it through
   `from_dense_entries`; dual-read assertions prove every live address/Handle mapping while no
   mutation behavior changes;
2. add versioned save/export/import and checksum coverage for the side owner;
3. make Unit address lookup and traversal resolve stable identities through the sparse owner,
   retaining the dense registry temporarily as a derived diagnostic view;
4. switch spawn/despawn to reserve/commit/retire while leaving dense row compaction untouched;
5. install the complete shared `Objects::init_unit` authority for production, cheat/carrier,
   and BHS;
6. remove the old dense registry only after all Build/Wall consumers have stable identities and
   every direct slice/zip consumer has migrated.

Installing allocation before steps 1--3 would create tombstones that save/load, checksum, and
script lookup silently erase. No shared Sim file was edited in this tranche.

## Verification

```sh
cargo test -p don-sim --test sparse_object_bands_authority_frontier
```

The thirteen tests cover band bounds, ten-owner independent marks, Unit/Animal projection split,
lowest-index reuse, hold/subordinate exclusion, retained storage after mark reduction, stable
identity across dense row movement, reservation/save refusal, snapshot round-trip, dense
conversion/gap rejection, the phase-1 dense append/swap-remove mirror, retail traversal order
including tombstones, and exact Unit-band capacity `-1`.
