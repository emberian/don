# East Meets West team partition and bounded continent continuation

## Result and fidelity boundary

This tranche closes `Map::fill_cont` at `0x0068a960` and continues
`MapEastMeetsWest::make_continents` through the two caller-owned angle draws,
region seeding, both `Map::grow_region` passes, the exact centroid loop, and
`Map::eliminate_pools(EntireWorld, dead)`. It stops at
`Map::eliminate_edge_canals` `0x0068b360`. It does not synthesize starting
locations or any later draw.

Every address and instruction claim below is **measured** from
`ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`)
and `ron-bin/sbl/rise.pdb`. The implementation is fidelity **Tier C**: a PE/PDB
transcription with mutation-sensitive tests, but no executable retail
differential. No checksum word was used to choose a produced byte.

## Why this boundary was selected

The offline World localizer identified three checksum-bearing recordings that
did not stop at the mountain or oil leaves: two at
`map_team_continent_partition`, one at `map_east_indies_nonplayer_islands`.
The PDB makes the smaller boundary unambiguous:

| function | PDB source extent | VA / size |
|---|---|---:|
| `Map::fill_cont(int*, int*, SimpleArray<int>*)` | `map.cpp:8638..8747` | `0x0068a960` / 660 bytes |
| `MapEastIndies::make_continents(int)` | `map.cpp:3172..3365` | `0x00697540` / 2,182 bytes |
| `MapEastMeetsWest::make_continents(int)` | `map.cpp:3371..3781` | `0x00696640` / 3,839 bytes |
| `Map::find_region_centroid(int, WCoord*, WCoord*)` | `map.cpp:8508..8520` | `0x0068ae50` / 587 bytes |

The leaf ends with `ret 0x0c` at `0x0068abf1`; the next byte at
`0x0068abf4` is padding. The caller passes its two eight-word stack arrays and
a null optional `SimpleArray<int>*` at `0x0069684c..0x0069686a`. The new leaf
models only that proven caller domain.

The two checksum-bearing East Meets West headers are independent inputs, not
expected-output fixtures:

| recording | seed | style / size / edge | replay teams | post-orientation state | recorded turn-2 World |
|---|---:|---|---|---:|---:|
| `Playback___2018.12.01_18_33_16__Sat_.rcx` | `0x01e1c76d` | 19 / 6 / 100 | `[0,0,1,1]` | `0x13dffc27` | `0xfabe984e` |
| `Playback___2019.03.24_11_56_19__Sun_.rcx` | `0x00bb378d` | 19 / 6 / 100 | `[0,1,1,0]` | `0xdcb68147` | `0xf173a4e9` |

The comparison East Indies header is
`Playback___2024.04.10_17_05_19__Wed_.rcx`, seed `0x000452f0`, style 18,
size 6, edge 100, and teams `[0,0,1,1,2,2]`; its recorded turn-2 World value
is `0x91c052d8`. This tranche deliberately leaves its non-player-island tail
unchanged.

## Exact `Map::fill_cont` schedule

The caller-null-list path executes in this order:

1. Scan candidate ordinary teams `0..7`. Append a candidate to the local
   unused-team array only if no active fixed leader slot reports that team.
2. Zero the eight continent-count words and fill the eight team-mapping words
   with `-1`.
3. Scan fixed leader slots `0..7`. An ordinary team reuses its first assigned
   continent. Team `8`, the unteamed sentinel, borrows the next unused ordinary
   team id and receives a separate continent.
4. Scan the ordered pair domain `(left,right)` over `0..8 × 0..8`. For every
   distinct pair whose nonzero counts are equal, call
   `Random::get(0,0xffff)` at `0x0068ab5f`. Exchange the two mapping labels when
   `raw & 0x80000001 != 0`.

The last exchange has a native-looking but load-bearing quirk. At
`0x0068ab70` retail loads the old mapping word once. Both subsequent conditions
test that old value, so `-1` satisfies both stores and the second store wins.
The port therefore does not collapse this into an `if/else` or restrict the
exchange to assigned mappings.

For the first checksum-bearing header the two ordered equal-size calls are:

| pair | RNG before | raw | exchange | RNG after |
|---|---:|---:|---|---:|
| `(0,1)` | `0x13dffc27` | 19,289 | yes | `0x1d154b5a` |
| `(1,0)` | `0x1d154b5a` | 41,712 | no | `0x8e53a2f1` |

The output counts are `[2,2,0,0,0,0,0,0]`; the final team mapping is
`[1,0,0,0,0,0,0,0]`. Starting from the second header's independently
localized `0xdcb68147`, the corresponding raw values are 52,729 and 1,296 and
the final RNG state is `0x3ac90511`.

`TeamContinentPartitionReceipt` binds the entry/return addresses, fixed active
slot order, ascending unused-team list, both arrays, derived continent count,
every call site's before/raw/decision/after tuple, and the final RNG word. The
function receives no `World*`; `world_walked_bytes_changed` is therefore
exactly zero.

## Exact East Meets West caller prefix

The caller schedule transcribed from `0x00696743..0x00696c82` is:

| stage | instruction anchor | exact operation |
|---|---:|---|
| growth spacing | `0x00696743` | `Map+0x30 = clamp(scale_land_area(AVOID_CONTINENT, players), 4, 16)` |
| wipe / clear | `0x00696829`, `0x0069682e` | wipe World, then clear Regions |
| side count | `0x00696833` | `max(Game::num_sides, 1)`; the replay adapter derives the same count from the fixed-slot partition |
| partition | `0x0069686a` | call the leaf above with a null optional list |
| radius | `0x0069687b..0x0069689c` | `(min(xs,ys) / 2) * 3 / 4`, signed truncation |
| angle | `0x0069689f`, `0x006968b7` | two `get(0,0xffff)` calls; `(first % 0xffff) << 16 + second % 0xffff` |
| region area | `0x006968bf..0x006968ea` | `scale_land_area((world.size / players) * 5 / 9, players)` |
| seed loop | `0x00696980..0x006969ff` | project from World centre at the radius; seed regions `1..=sides` |
| angle step | `0x006969ce..0x006969f9` | add logical halves of the wrapped current and next `count * (0xffffffff / players)` products |
| first growth pass | `0x00696a30..0x00696b34` | target `(region_area / 2) * count`, max distance `radius * 3 / 4` |
| second growth pass | `0x00696b60..0x00696c5f` | target `region_area * count`, same max distance |
| centroid continuation | `0x00696c82` | exact one-based `find_region_centroid` loop |
| pool cleanup | `0x00696d0a` | exact `eliminate_pools(EntireWorld, dead)` |
| next external body | `0x00696d0f` | `eliminate_edge_canals()` |

Each growth's dynamic RNG sites are appended in execution order by the existing
`execute_grow_region` receipt. A nonzero retail growth return is exposed as
`RetryGeneration`; this bounded call does not pretend it executed the caller's
whole-pass retry loop. On success it reports
`EliminateEdgeCanals { primitive_va: 0x0068b360, ... }` with the complete
centroid receipt retained inside the stop.

For the 100×100, four-player fixture the frozen prefix has two regions of area
1,388, growth targets `(1,1388)`, `(2,1388)`, `(1,2776)`, `(2,2776)`, and max
distance 27. Before/after World channel values are `0xd293cb35` and
`0x193d611d`; the isolated WData values are `0x4f28bf8c` and `0xa9965574`.
WData is the only changed checksum section. These are regression outputs of the
binary-derived schedule, not values fitted to any recording.

## Tests and integration

`crates/don-replay/tests/continent_reconstruction.rs` checks:

- both checksum-bearing headers' exact two leaf draws, exchange decisions,
  output arrays, final RNG words, and zero World mutation;
- full style-19 caller ordering (orientation, two partition calls, two angle
  calls, then growth-internal calls), exact seed/growth parameters, centroid
  arrays, the pool transaction, and the edge-canal stop;
- an exact full-World and isolated-WData checksum mutation with every other
  World section unchanged;
- transactional validation through the existing continent entry point.

The centroid source is a public replay module because its full typed receipt is
retained by `ContinentStop::EliminateEdgeCanals`. The surgical `initial.rs`
mapping names the new `map_team_continent_edge_canals` boundary.

The remaining tail starts with the 1,318-byte edge-canal mutator, then
balances/builds regions, selects per-leader starts, and eventually reaches the
third direct style draw at `0x00696f82`. None of those residual mutations or
draws is represented here, and this work makes no retail World checksum-match
claim. The exact centroid body and replay fixtures are documented in
`docs/assembly/map-find-region-centroid.md`.
