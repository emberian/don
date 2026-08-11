# `Map::eliminate_edge_canals` exact body

This lane recovers all 1,318 bytes of `Map::eliminate_edge_canals` at
`0x0068b360` from the shipped PE/PDB, direct Capstone disassembly and
`re/decomp-all/0068b360.c`.  It is Tier C: exact static transcription and real
replay execution, not a live executable differential.  No VM or running retail
process is touched.

The function uses two fixed `char[100]` stack arrays. It visits the left,
right, top and bottom edges in that order. At the beginning of each pass it
snapshots the raw `land == OCEAN` predicate for the outer edge and first inward
row or column. For every three-cell outer run, it changes the fertile middle
cell one step inward to ocean. That write updates the local inward snapshot;
when the inward three-cell run is then complete, it changes the fertile middle
cell two steps inward as well. The overlapping three-cell windows make the
in-pass snapshot update load-bearing.

Detection compares only the signed land byte at `WData+2` to literal `2`. It
does not call `WorldData::is_ocean`, so `WATERHALF` does not suppress a raw
ocean byte. Targets must be literal fertile byte `0`; coastal and all other
values remain untouched. Each accepted target receives one 16-bit store of
`0x0002`, setting land to ocean and land-subtype to zero without touching
flags, region or region2.

After all four sides, retail calls `Regions::clear_all` and `Regions::find_all`.
The port uses the existing exact transactional rebuild and retains its complete
`RegionBuildReceipt`. The body consumes no RNG. The only shipped caller is the
East Meets West style at `0x00696d0f`; caller execution resumes at
`0x00696d14`. Capstone pins the native tail to `pop ebp` at `0x0068b884`, `ret`
at `0x0068b885`, and the end-exclusive boundary `0x0068b886`; ten `int3`
padding bytes precede the next function at `0x0068b890`. The next common
external start-placement body is `Map::place_start_in_region` `0x0068ac00`,
first called at `0x00696fe3`; this tranche parks at that boundary and does not
execute the selector.

The native body indexes both axes with `World::xs` and the stack snapshots cap
the edge at 100. Procedural caller maps are square and at least three cells.
The safe port rejects non-square, undersized and over-100 direct inputs before
mutation.

## Frozen style-19 receipts

Both checksum-bearing East Meets West headers execute the complete four-pass
body with zero terrain writes and an empty direct-RNG-site list. The mandatory
region tail finds two land components and one sea component, performs no
consolidations, and records four non-input pumps in both cases.

| replay | writes L/R/T/B | full checksum before → after | WData before → after |
|---|---:|---:|---:|
| 2018-12-01 | `0/0/0/0` | `0x32b60f72 → 0xb2eeff92` | `0x69cc02c7 → 0x2eaaf2e7` |
| 2019-03-24 | `0/0/0/0` | `0xeb4960c6 → 0xeb4960c6` | `0x69f054b2 → 0x69f054b2` |

The 2018 checksum change with zero canal writes is the region rebuild changing
the walked region bytes. The exact continent receipt preserves the growth RNG
word across this body and advances the coherent stop to
`PlaceStartInRegion { primitive_va: 0x0068ac00, first_call_va: 0x00696fe3 }`.
The caller-owned boundary receipt also materializes the first selector's exact
arguments. For 2018 it selects slot/team `0/0`, assigned and selected continent
1, current region 1, centroid `(19,46)`, angle `-1707059882`, search distance
27 and minimum start distance 24. For 2019 the corresponding values are
`0/0`, continent 1, current region 2, centroid `(52,80)`, angle `1389843798`,
distance 27 and minimum 24. Both have population 2, so neither executes the
conditional direct draw at `0x00696f82`. A population-1 mutation pin proves
that branch consumes exactly one draw before the boundary and records its RNG
before/raw/after words.

## Validation

- local focused suites: edge canals 4/4, continent reconstruction 4/4 and
  centroid recovery 4/4;
- local reconstruction/ownership suites: 6/6;
- local two-header localizer: both boundaries are
  `map_team_continent_place_start`, both owner ledgers are coherent, and all
  65,081 same-group peer comparisons agree;
- Persvati clean-HEAD overlay job
  `replay-edge-canals-v2-20260811T184039Z-36613-30596-ff4bbec4ab74`: the three
  asset-independent synthetic tests pass 3/3. The real replay corpus is not a
  remote overlay asset, so its two fixture receipts are gated locally.
