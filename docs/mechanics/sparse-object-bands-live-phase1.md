# Sparse object bands: live phase 1

Status: compiled canonical state, save/digest owner, and dense dual-read guard. Sparse allocation
is deliberately disabled.

## Installed boundary

`World` now owns `SparseObjectBands<WorldObjectIdentity>` beside the legacy `ObjectRegistry`.
The identity join is explicit:

- Unit addresses store `{handle id, generation}` and resolve through `World::row_of`;
- Build addresses temporarily store the Build pool row;
- Wall addresses temporarily store the Wall pool row.

All existing dense mutation entrances (`spawn_typed`, `allocate_typed_at`, `despawn`, owner
activation, `Sim::spawn_build`, and `Sim::spawn_wall`) still commit through the legacy registry.
After that commit they mirror the exact append or swap-removal receipt into the sparse owner.
These phase-specific mirror operations are constant-time and accept only a gap-free owner; they
do not call `find_free`, reserve a sparse slot, retain a tombstone, or change current allocation
and compaction behavior. Full dense conversion remains the legacy-load and invariant-check seam,
not the per-spawn path.

`World::object_bands_are_dense_equivalent` is the phase guard. It independently derives the
gap-free sparse projection from the live dense registry and compares the complete save-shaped
snapshot: activity, all 30 marks, retained slots, and stable identities.

## Save/load

DoNSave format 8 appends the sparse snapshot to the Objects section. It writes no process-local
storage tokens or reservation tickets. The decoder bounds every owner/band slot count by that
band's structural capacity and validates lifecycle and identity tags.

Format 7 remains readable. Because it could represent only gap-free dense bands, load converts
its unit/build rows exactly into a sparse owner; the next save emits format 8.

During phase 1, format 8 decoding preserves tombstones and sparse marks, but World import rejects
any snapshot that is not exactly dense-equivalent. This is intentional fail-closed behavior:
accepting a gap before script lookup, traversal, and allocation migrate would load the bytes and
then silently erase their semantics in the dense consumers.

## Digest

`World::digest` now composes a domain-separated hash of the allocation owner. It covers owner
activity, owner/band order, every mark, retained-slot count, tombstone reuse fields, and each live
stable identity. It excludes rebuilt storage/projection tokens. Two worlds with equal live Unit
columns but different object-owner state no longer collide in this digest.

This remains DoN's deterministic structural digest, not a byte-identical retail Adler walk.

## Proven behavior

`sparse_object_bands_live_integration.rs` proves:

- Unit spawn and dense swap-despawn leave both address views exactly equal;
- Build and Wall insertion dual-write without sparse allocation;
- owner activity changes the digest and is reversible in both views;
- format-8 save/load/resave preserves the sparse owner and digest.

Save/load unit proofs additionally cover exact format-7 conversion and show that the format-8
codec preserves a tombstone while phase-1 World import refuses it.

## Remaining red seam

This phase closes persistence and checksum loss for the representable gap-free owner. It does not
unlock BHS positive allocation. Before `find_free` can become live, Unit address lookup,
Objects traversal (including inactive hold countdown), scripts, force transport, Ammo/tick views,
Build/Wall stable identities, and despawn must consume the sparse owner directly. Only then can
reserve/complete/retire replace conversion refreshes without creating state that a live consumer
misreads.
