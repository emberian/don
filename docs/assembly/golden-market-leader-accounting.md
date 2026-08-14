# Golden Market Leader accounting

The 2024 Dutch golden setup creates owner 0's free Market as object 2001, type 436,
linked to City 0. This lane owns the Leader-side accounting after-image for that one
activation. It does not generalize the result to arbitrary Building lifecycles.

## Canonical setup before-image

`mount_frame_zero_build_accounting` derives its image from the ordinary setup receipt,
the exhaustive one-Village Building census, and replay-carried type links. It writes the
canonical `Sim::vic_leaders` owner only at frame zero and accepts no caller counts or type
roots. Per active Leader it source-produces 17,080 bytes:

- `buildings_built` and both six-entry gather-slot arrays;
- `num_buildings[129]` and `high_buildings[129]`;
- region-major `reg_buildings[64][129]`.

The mount is staged and fails closed when an existing canonical value is neither pristine
nor equal to the derived census. Ordinary setup therefore starts the golden owner with one
Village in the aggregate, regional, and high-water mirrors and one completed Building.

## Market transaction

`Sim::prepare_golden_market_leader_accounting` requires the capture revision, native-trace
digest, and exact entry-Sim digest projected from `GoldenStartingMarketSetupEntryAuthority`
into its narrow region authority. It then resolves the live Build registry itself and
requires owner 0, object 2001, type 436, ACTIVE state, City 0, a typed region below 64, and
exact agreement between the victory and step-8 Leader flag mirrors.

The committed receipt preserves retail order:

1. `Build::init` increments `buildings_built` and ORs dirty bit `0x02000000`.
2. `Wall::increment_stats` (`0x00643270`) increments `num_buildings[22]`, then the
   typed-region cell `reg_buildings[region][22]` (`0x006432E2`, `0x00643307`).
3. The Market arm increments wealth `gather_slots[2]` and updates
   `gather_slots_high[2]`.
4. The activation high-water block (`0x00624BA4..0x00624C16`) updates
   `high_buildings[22]` and ORs dirty bit `0x08000000`.

`commit_golden_starting_market_leader_accounting` is the replay-side composition point. It
consumes `GoldenStartingMarketSetupEntryAuthority`, the digest-bound join between the separate
Market lifecycle capture and the schema-v2 setup-entry oracle. It rechecks that authority and its
exact pre-mount entry-Sim snapshot, reads the Market's region from the canonical World cell, and
only then invokes the Sim transaction. No parallel caller-provided region or digest is accepted
there.

Prepare/commit revalidates the live Build link and exact before-image. Any identity, shape,
flag-mirror, or stale-state disagreement publishes no partial Leader write. DoNSave v23
persists every canonical field above; v22 refuses a lossy downgrade when they are nonzero.

This closes 22 unique Leader bytes for the golden Market event (plus the duplicate flag
mirror). It does not reduce the frame-zero checksum frontier: that frontier remains
27,809/28,428 bytes per active row with 619 bytes unsourced. Whole-channel installation is
still false and its survived-turn count remains zero until the residual and later Building
writers are owned.
