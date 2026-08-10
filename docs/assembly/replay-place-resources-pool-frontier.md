# Replay `Map::place_resources` divvy-pool frontier

`crates/don-replay/src/place_resources_pool_frontier.rs` reconstructs the first
deterministic, mutation-owning prefix of retail `Map::place_resources(int)`:

```text
Map::make checksum token 0x1ebe at 0x0068c12a
  -> unresolved caller work begins at 0x0068c12f
  -> direct place_resources call at 0x0068c707
  -> Map::place_resources entry 0x0068f4f0
  -> good scan / ResourceDivvyPool construction
  -> ResourceDivvyPool::done_adding_goods at call 0x0068f592
  -> exact residual 0x0068f597
```

The shipped inputs used for reversal are:

- `ron-bin/riseofnations.exe`, SHA-256
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`;
- `ron-bin/sbl/rise.pdb`, SHA-256
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
- PDB layouts for `Map`, `ResourceDivvyPool`, `DynamicBitMask`, `GoodTypeData`,
  `Lands`, `LandData`, and `Game::semaphore`;
- the selected map-style rare-resource data loaded from shipped `ron-data/mapstyles/*.xml`.

## Why token `0x1ebe` is not treated as the entry RNG state

The direct call is at `0x0068c707`, not immediately after the checksum returns at
`0x0068c12f`. The caller gap performs other map-generation work and is not owned by this
source pack. `PlaceResourcesEntryHandoff` therefore carries both the upstream checksum
identity and a separately captured `random_state_at_entry`. The frontier proves that its
own prefix consumes zero draws; it does not invent equality across the unresolved gap.

This distinction is essential for replay chronology. Root integration must bind the entry
state to a preceding exact caller receipt or a retail capture before advancing the schedule.

## Exact instruction-derived prefix

### Candidate scan (`0x0068f52e..0x0068f58a`)

The loop scans good IDs `6..=49` in ascending order. For each ID:

1. if `Game::semaphore` bit 17 is set and `GoodTypeData::exclude_ctw` (`+0x2f4`)
   is nonzero, skip the good;
2. otherwise require nonzero `GoodTypeData::random_rare_drop` (`+0x2f8`);
3. call `ResourceDivvyPool::add_good` at `0x0068f579` for every survivor.

The global references used by retail are `GameAccessConst::gamec` at `0x00c061e8`
and `GameAccess::goodtypes` at `0x00c061ac`. The live facts require exactly 44 ordered
records, so a shifted or incomplete catalog cannot silently change the scan.

### Divvy classification (`ResourceDivvyPool::add_good`, `0x0068a830`)

`add_good` resolves `ObjectTypeData::age` (`+0x278`; virtual `get_age_slow` when the
stored value is `-1`) and then searches `lands[2].rare[0..num_rare]`. PDB and caller
arithmetic identify land index 2 as Ocean, `num_rare` at `LandData +0x4c`, the
44-element array at `+0x50`, and the active list pointer slot at `0x00e3a380`.

Classification is exact and ordered:

- Ocean-list membership wins before the age test and appends to `water_goods`;
- good ID `6` is appended ten times total, exactly matching the nine-copy loop after
  the first append at `0x0068a8ea..0x0068a91e`;
- non-water ages below 3 append to `early_goods`;
- all remaining accepted ages append to `late_goods`.

There are no RNG calls in either the caller scan or `add_good`.

### Pool finalization (`0x0068f58c..0x0068f597`)

The caller invokes `ResourceDivvyPool::done_adding_goods` (`0x0068a700`) at
`0x0068f592`. It recreates each `DynamicBitMask` with the corresponding list count.
`DynamicBitMask::init` (`0x00a3a3c0`) stores the logical bit count, allocates
`(count + 7) / 8` bytes, and clears them. `done_adding_goods` then clears all three
byte arrays again. The receipt records two clear passes and the final exact logical and
allocated sizes.

The prefix commits the complete six-field `ResourceDivvyPool` projection atomically.
Malformed catalog IDs, an Ocean list longer than the native 44 entries, a stale handoff,
or mismatched capture evidence leaves the previous pool byte projection untouched.
Shared integration binds the following XML owner to this exact result with
`resource_divvy_pool_digest`: a domain-separated, field-tagged, length-delimited FNV-1a
encoding of all three bitmask projections and all three ordered goods arrays. It is a
stable logical receipt digest, not an allocator-dependent retail memory hash.

## Ownership and residual

This prefix owns only `Map::resource_pool` (`Map +0x274`): it appends to the three good
arrays and recreates their three bitmasks. The native body does not clear pre-existing
goods, so a repeated invocation duplicates qualifying entries; the source preserves that
behavior. It performs no World write, does not advance the World checksum or
sourced-byte count, and has zero calls to `Random::get` (`0x00a39d70`).

The pool owner's real residual is `0x0068f597`, immediately after pool finalization.
Shared integration now continues through the selected/default XML bootstrap to
`0x0068fb9d`. BONUS row parsing, later direct and callee RNG draws, and the post-resource
caller checkpoint at `0x0068c72d` (token `0x1ef7`) remain red.

## Source-only proof

`crates/don-replay/tests/place_resources_pool_frontier.rs` freezes:

- all 44 scan decisions and exact add-good call provenance;
- CTW exclusion precedence and nonzero rare-drop semantics;
- the age-3 boundary and Ocean membership precedence;
- weighted water good 6 ordering and cardinality;
- exact ceil-div-eight mask allocation and both zero passes;
- repeated-call append semantics rather than an invented goods-array reset;
- zero RNG draws and unchanged World checksum/sourced bytes;
- exact entry, call, residual, EXE/PDB, and global-reference evidence;
- rollback for malformed catalogs, oversized Ocean lists, and stale evidence;
- mutation sensitivity for age, Ocean membership, CTW state, and exclusion; and
- stable digest coverage of all six logical pool fields.

Root convergence formatted the isolated files. The first compile found only focused-test
shadowing/binding defects; after those were corrected, persvati job
`replay-place-resources-v2-20260809T233428Z-47419-30980-20e8106b2577` passed all nine tests.
Retail was not run. The exact caller-gap RNG/World/checkpoint handoff is now integrated by
`map_make_resource_schedule_integration`; the typed open boundary is now the BONUS row body at
`0x0068fb9d` when XML facts are available.
