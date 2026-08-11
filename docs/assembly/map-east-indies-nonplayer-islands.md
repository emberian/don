# East Indies non-player-island tail

## Result and boundary

This tranche owns the residual of `MapEastIndies::make_continents(int)` from the
current replay stop at `0x00697b72` through the style virtual's return at
`0x00697da4`. It chooses and grows the non-player islands, applies their final
region metadata, and reports either complete placement or the exact bounded
native escape that left islands unplaced.

The tranche deliberately stops at the virtual return. The common `Map::make`
driver next calls `Regions::clear_all` at `0x00680060`; rebuilding provisional
regions is not owned here. Integration therefore consists of replacing
`ContinentStop::EastIndiesNonplayerIslands` with the new atomic adapter and, on
its successful return, continuing through the existing `HookComplete` common
chain. No edit to that shared caller is part of this lane.

Evidence is fidelity Tier C: instruction-for-instruction transcription of
`ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`),
cross-named by `ron-bin/sbl/rise.pdb` and `schema/rise-procs.tsv`. No replay
checksum was used to choose a branch, byte, draw, or expected value.

| body | PDB source extent | VA / size |
|---|---|---:|
| `MapEastIndies::make_continents(int)` | `map.cpp:3172..3365` | `0x00697540` / 2,182 bytes |
| admitted residual | tail of the same body | `0x00697b72..0x00697da9` |
| next common helper | `Regions::clear_all()` | `0x00680060` |

## Exact schedule

The already-owned prefix has created and fully grown player regions `1..P`.
The residual executes the following schedule:

1. `maximum_area = max(World::size / 20, 90)` and
   `minimum_area = max(World::size / 40, 24)`.
2. Draw at `0x00697b72`. For the positive half-open `Random::get(0,0xffff)`
   result, the native signed-remainder idiom reduces to `raw & 1`.
3. Set `remaining = World::xs / 12 - P + 8 + parity`. Zero returns immediately.
4. For each outer attempt, reduce the current target by `7/22` only for the
   spacing calculation. Run the exact Newton integer-square-root loop at
   `0x00697be0..0x00697bff`, then require
   `land_dist >= max((Map::avoid_continent + 1) / 2 + sqrt, 3)`.
5. Candidate X and Y draw independently at `0x00697c3f` and `0x00697c69`.
   `Map::land_dist(x,y,1)` is tried at most 1,000 times per outer attempt.
6. A spacing-qualified point calls `Map::grow_valid(next_region,x,y)`. On
   acceptance, retail calls `make_region`, then
   `grow_region(next_region,target_area,World::xs,-1,-1,0)`. The growth return
   is intentionally ignored. Only afterwards are the new `Region` fields
   overwritten to `common_factor=5`, `goody_factor=5`, and `climate=1`; flags
   remain the value installed by `make_region`.
7. After every spacing-qualified point, including a `grow_valid` rejection,
   select the next target in `[minimum_area, maximum_area)` using the draw at
   `0x00697d6a`. A range of zero or one consumes no draw.

Two native escape budgets are load-bearing:

- The outer counter is incremented before `cmp eax,0x64` / `jge 0x00697da4`.
  Consequently retail runs 99 consecutive seed attempts, not 100. A successful
  island resets this counter to zero.
- After 1,000 consecutive `land_dist` rejections, a target above the minimum
  collapses to the minimum. If already at the minimum, all three area words
  become `minimum * 13 / 16`; a result below 20 returns immediately.

The receipt preserves `requested`, `placed`, and `remaining` separately because
both exits are successful native returns even when `remaining != 0`. In the
focused 48×48 mutation fixture, the exact schedule requests 10 islands, places
9, and returns through the 99-attempt escape with one island still explicit.
That is an algorithm fact from the port's independent input fixture, not a
retail-checksum claim.

## RNG chronology and atomic ownership

The receipt is one chronological series of spans. Direct call sites are
interleaved with complete `grow_valid` and `grow_region` helper spans, each
carrying before/after RNG states and the helper's internal instruction-site
sequence. Tests require every span's ending state to equal the next span's
starting state, which prevents a plausible direct-site list from hiding a
misordered helper draw.

The public adapter stages and atomically commits all four stores it can mutate:

- `World` (only checksum section 5, `WData`, in the non-empty fixture);
- `Regions` and its per-region coordinate allocation history;
- the main `Random` word;
- `MapGrowthConfig`, including persistent `Map+0x64` edge jitter.

A capacity error is deliberately exercised after the island-count parity draw.
The test proves that World's complete checksum image, Regions, RNG, and growth
configuration all remain byte-/value-identical after the error. A separate
all-land fixture executes 4,001 direct draws, reaches both adaptive-area steps,
and returns at area 19 without changing any World checksum section.

## Residual after this tranche

There is no remaining East Indies style-virtual instruction after the adapter:
its `return_va` is `0x00697da4`. The only continuation is the already-mapped
common map-construction chain beginning with `Regions::clear_all` at
`0x00680060`. Full replay compatibility still depends on that common chain and
later terrain/resource producers; this tranche makes no World checksum match
claim by itself.
