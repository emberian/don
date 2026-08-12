# East Meets West remaining active-player start loop

## Result and exact boundary

This tranche continues MapEastMeetsWest::make_continents from the first
World::add_starting_location return at 0x00697466, executes the caller
bookkeeping and every remaining active fixed-player iteration, and freezes
before the post-loop call at 0x00697492. The next native mutator is
Map::check_player_land at 0x0068ef00; this tranche names that call but does not
execute it.

The canonical continuation now executes that separately proved leaf and its
exact local-string cleanup, the centroid-Y free, and its caller bookkeeping,
then the centroid-X free and complete style-virtual epilogue.  The integrated
continuation also executes the first common `Regions::clear_all`; the stop is
now `Regions::find_all` at 0x0067eff0 (caller 0x0068be3c); see
`map-check-player-land-east-meets-west.md`. The boundary below remains the
historical contract of this isolated remaining-start tranche.

Evidence is ron-bin/riseofnations.exe (SHA-256
30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079),
ron-bin/sbl/rise.pdb (SHA-256
334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5),
direct i386 disassembly, the already recovered selector and World writer
bodies, and the two real style-19 replay headers. The PDB identifies
MapEastMeetsWest::make_continents(int) at 0x00696640 with size 3,839 and
Map::check_player_land(int,int,int,int) at 0x0068ef00 with size 1,145.
No VM or live process was used, and no replay checksum was used to choose a
branch, RNG word, coordinate, or write.

## Native caller chronology

The successful common path is:

| anchor | exact caller action |
|---:|---|
| 0x00697466 | recover the recorded-start count and current fixed slot after the World writer returns |
| 0x0069746c, 0x00697473 | store selected X and Y in caller-local recorded-start arrays |
| 0x0069747a | increment the caller-local recorded-start count |
| 0x0069747e | increment the fixed player slot; inactive leaders also branch directly here |
| 0x0069747f | jump to fixed-slot loop head 0x00696d28 |
| 0x00696d2b | terminate when the slot reaches the shipped fixed bound 8 |
| 0x00696d3a | test the current Leader active bit |
| 0x00696d4f, 0x00696d5f | read the active Leader team |
| 0x00696fe3 | call Map::place_start_in_region for this active slot |
| 0x00696fe8 | branch on the selector result |
| 0x00697461 | call World::add_starting_location on success |
| 0x00697484 | exit the fixed-slot loop |
| 0x00697492 | next call, Map::check_player_land; not executed here |

For each active slot the caller repeats the same argument construction already
recovered for the first slot: ordinary-team mapping, post-rebuild region
lookup, centroid angle, continent population spacing, radius/search distance,
and the scaled minimum distance. The adapter selects the next slot only from
TeamContinentPartitionReceipt::active_slots, whose fixed-slot order and team
layout came from the replay header. It retains the complete mapping and
population arrays while doing so. Inactive gaps are receipted but execute no
selector, RNG call, or World write.

The selector failure edge is unchanged: EAX zero at 0x00696fe8 enters
0x00697003. A later failure receipts the successful earlier appends and the
failed slot, then stops without visiting another active slot. Success calls
the complete 570-byte World writer and records its returned prior start index.
Only StartArrays changes in the walked World during this continuation;
occupancy-bit writes remain part of each writer receipt.

## Frozen real receipts

Both real headers contain active slots [0,1,2,3]; the continuation therefore
visits slots 1, 2, and 3, then receipts inactive tail slots [4,5,6,7].
Every real selector succeeds on pass 1 with one draw at 0x0068ac8b.

| replay | slot | team / continent / region | RNG before → raw / anchor → after | output / returned index |
|---|---:|---|---|---|
| 2018-12-01 | 1 | 0 / 1 / 1 | 0x751b5283 → 22021 / 3037 → 0x207d5606 | (26,81) / 1 |
| 2018-12-01 | 2 | 1 / 0 / 2 | 0x207d5606 → 46508 / 1916 → 0x39a8b5ad | (58,63) / 2 |
| 2018-12-01 | 3 | 1 / 0 / 2 | 0x39a8b5ad → 6951 / 1377 → 0x20481b28 | (81,71) / 3 |
| 2019-03-24 | 1 | 1 / 0 / 1 | 0x8e6cf4fc → 52266 / 2244 → 0xce2fcc2b | (52,34) / 1 |
| 2019-03-24 | 2 | 1 / 0 / 1 | 0xce2fcc2b → 29581 / 1791 → 0xb068738e | (18,27) / 2 |
| 2019-03-24 | 3 | 0 / 1 / 2 | 0xb068738e → 26004 / 867 → 0x18a56595 | (19,74) / 3 |

The complete produced start lists are:

| replay | starts in native append order | continuation full World | continuation StartArrays |
|---|---|---:|---:|
| 2018-12-01 | (26,56), (26,81), (58,63), (81,71) | 0x35db0949 → 0x4eb610df | 0xbfa809a9 → 0x8446113f |
| 2019-03-24 | (87,78), (52,34), (18,27), (19,74) | 0xfb5e6c0d → 0x660c7097 | 0xfcbd0b48 → 0x5c970fd2 |

These values freeze outputs of the PE/PDB-derived schedule. They are not a
claim that the generated World equals a later replay-recorded checksum.

## Receipt and ownership boundary

EastMeetsWestRemainingStartsReceipt binds every caller anchor, the exact
active-slot schedule, skipped inactive slots, every repeated call, selector
and mutation receipt, RNG chronology, and before/after World sections. On
complete success the isolated remaining receipt retains the unexecuted leaf
call site 0x00697492. The integrated
ContinentStop::AddStartingLocation::next_va advances through that leaf, the
complete cleanups, and the clear to 0x0068be3c. A later selector failure advances to
0x00697003. The continent owner digest includes the new source, so the
localizer cannot reuse an older implementation proof.

The focused test freezes both real receipts and separately rewinds the first
fixture to its first append, makes the next region contain no valid separated
coordinate, and proves the two-pass selector failure changes no World byte and
does not visit slot 2.

## Validation

- current-source focused remaining-start suite: 2/2;
- continent reconstruction: 4/4; prior selector, append, centroid and edge
  suites: 14/14;
- current-source owner transition suite: 2/2, including the full
  checksum-bearing corpus and transactional late-shape rollback;
- current-source full localizer: 62 recordings opened, 21 checksum-bearing,
  21/21 coherent owner ledgers, and 265,619/265,619 agreeing same-group peer
  comparisons;
- exact corpus census: 13,119,476 walked / 7,917,091 owned / 5,202,385 unknown
  bytes, with 4,484 / 4,274 / 210 in StartArrays;
- exact current integrated boundary set: 2
  map_team_continent_post_checksum_string_close and 19
  place_all_mountains_add_mountain; all 21 earliest lawful unknowns remain
  section 2 + 1;
- Persvati clean-HEAD overlay lib check:
  replay-remaining-starts-check-v3-20260811T212804Z-3895-5126-a7bf776e8e5b.
