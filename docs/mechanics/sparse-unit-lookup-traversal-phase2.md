# Sparse Unit lookup and traversal: live phase 2

Status: canonical step-14 traversal and a bounded exact-address consumer set are live. Sparse
allocation remains disabled, and saves containing gaps remain fail-closed at World import.

## Migrated runtime boundary

Both executable object passes now retain a buffer of sparse traversal entries rather than dense
`ObjectRegistry` tuples:

- `World::objects_process_all`, used by the core World scheduler;
- `Sim::objects_process_all`, used by the assembled 29-step scheduler.

The canonical owner supplies Unit entries in `(frame + i) % 10` owner order, followed by Build
and Wall entries for owners 0--7. Live Unit entries carry a generational identity and resolve the
current dense row at the point of use. Dense row movement therefore does not rewrite the
traversal owner. Build and Wall traversal also comes from the sparse owner, but their identities
remain explicit pool-row identities until those pools gain stable handles.

`SparseObjectBands::traversal_into` preserves the caller's allocation between frames. It walks
every slot below each independent mark, including tombstones; it is not a live-entry filter.

## Unit tombstone hold

An inactive Unit tombstone with nonzero `hold_frames` is visited in owner rotation and decremented
in the canonical sparse slot. It has no fabricated dense row and does not dispatch `Unit::process`.
The mutation is already part of `World::digest` and the format-8-and-later sparse snapshot.

Live but inactive Units retain their existing behavior: their generated Unit column's
`hold_frames` decrements. That is a distinct lifecycle from a retained sparse tombstone.

Active-flag tombstones have no executable object body in this phase. They cannot enter a live
World through save/load, and the pass does not invent a virtual call for them.

## Exact Unit lookup

`World::unit_row_at(who, o)` is now the canonical address resolver:

```text
(who, o) -> sparse live Unit identity -> generation-checked Handle -> current dense row
```

It returns no row for tombstones, stale handles, wrong-band identities, invalid owners, or indices
outside `[0,2000)`. The bounded consumers migrated with it are:

- core `World::do_attack` target lookup;
- assembled `Sim::do_attack`, preserving its exact Handle/UID/address preflight and explicitly
  non-authoritative legacy path;
- projectile `AmmoView::object` target lookup;
- projectile `AmmoView::find_unit_near` owner/mark scan;
- step-15 projectile damage writeback.

The projectile scan preserves retail object indices when it skips tombstones; it never compacts a
marked sparse band into a new enumeration.

## Save and digest guard

No phase-2 format change was needed. Format 8 introduced serialization of marks and tombstone
lifecycle, which later formats retain, and the World digest already hashes activity, marks,
retained slots, tombstone reuse fields, and stable identities. A focused World proof retires a Unit
in the sparse owner, advances one object frame, observes hold `3 -> 2`, and observes the digest
mutation.

World import/export still requires the sparse snapshot to equal the gap-free dense compatibility
view. The codec can preserve a tombstone, but the live World refuses it. This remains necessary
because the consumer set below still indexes or zips the dense Unit band and would misread a gap.

## Remaining consumers before allocation

Sparse `find_free`/reserve/commit is still not reachable from production or BHS. The next migration
must preserve marked indices, using inactive placeholders where a consumer's arrays are indexed by
retail `o`, across:

- script object lookup, containment/captain chains, and owner Unit scans;
- SPECIAL_ANIM target resolution and movement/collision's live object-row adapter;
- step-8 and immediate-BHS stat mirrors and their writeback paths;
- defeated-owner group/member and Unit sweeps;
- production's University gatherer scan, finished-unit callbacks, and direct Unit commands;
- the step-15 `Objects::inc_time` Unit traversal adapter;
- save-time step-8 mirror validation.

Only after that complete set reads sparse addresses can World import admit Unit tombstones and the
allocator switch replace the phase-1 dense append/swap-remove mirrors.

## Verification

Focused proofs cover exact lookup across dense row compaction, invalid/tombstone lookup refusal,
retained-buffer owner rotation, Build/Wall tail order, tombstone hold/digest mutation, and the
existing thirteen sparse allocation-owner invariants.
