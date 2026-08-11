# `MapGreatLakes::make_continents` and the `Map::land_dist` spacing leaf

Lane: `replay-mapmake`. Every address and every claim below is **[measured]** on this
Mac against `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`, image base
`0x00400000`) and `ron-bin/sbl/rise.pdb`. Disassembly is Capstone over the mapped
image; `re/decomp-all/` was used only for orientation. Nothing here is oracle-executed:
the fidelity tier is **C — behaviourally faithful, structure and constants read off the
binary**, with the divergence boundary stated in §5.

## 0. Why this style

`schema/replay-validation.json` measures the corpus's map-style distribution as
12 × 22 files, **14 × 12 files**, 19 × 10, 9 × 6, 20 × 5, 18 × 4, 6 × 1, 8 × 1.
Restricted to the 21 recordings that carry checksums it is 12 × 8, **14 × 6**, 9 × 3,
19 × 2, 6 × 1, 18 × 1. Great Lakes is the second most common style in both counts, and
its whole virtual was blocked on exactly one unported leaf.

## 1. `Map::land_dist` `0x0069d970` — the leaf that was missing

PDB: `public: virtual int __thiscall Map::land_dist(class WCoord, class WCoord, int)`,
351 bytes, **no calls**. It reads only `World` (`[0x00c06188]`) and the canonical
`circle_init` tables (`circle_x @0x00cb7e90`, `circle_y @0x00cbb0e0`,
`ring_end @0x00cbe330`).

```text
0069d9a6  test word [wdata + 28*(xs*y + x)], 0x100   ; WATERHALF -> return 0
0069d9af  movsx ecx, byte [.. + 2]                   ; land
0069d9b5  cmp ecx, 2 / cmp ecx, 1                    ; -> the exact is_ocean predicate
0069d9c6  je  0069dac4                               ; not ocean -> return 0
0069d9cc  eax = 1                                    ; ring index d
0069d9e0  ecx = [0xcbe330 + d*4]                     ; ring_end[d]
0069d9e7  edi = [0xcbe32c + d*4]                     ; ring_end[d-1]  (same table, -1 entry)
0069da00  eax = (i8)circle_y[edi] + y ; ecx = (i8)circle_x[edi] + x
0069da12  js/js/jge/jge 0069da8a                     ; off map
0069da39  test word [wdata + 28*(xs*ny + nx)], 0x100 ; WATERHALF -> return d
0069da65  movsx eax, byte [.. + 2] ; cmp 2 / cmp 1
0069da80  je  0069dab8                               ; not ocean -> return d
0069da8a  cmp dword [ebp+0x10], 0 ; jne 0069dab8     ; off map counts as land iff arg3 != 0
0069daa1  cmp eax, 0x40 ; jle 0069d9e0               ; rings 1 ..= 0x40
0069daac  eax = 0x41                                 ; saturate
```

Four things a plausible model gets wrong:

1. **The origin test is `WorldData::is_ocean`, not "is water".** `WATERHALF` is checked
   *before* the `land` byte in both the origin test and the ring scan, so a half-land
   cell terminates the scan even though its `land` byte still reads deep water.
2. **`0xcbe32c` is not a second table.** It is `ring_end - 1` entry, which is how the
   loop gets ring `d`'s half-open span `[ring_end[d-1], ring_end[d])` from one array.
3. **The metric is the engine's truncating octagon `vector_dist`, not Euclid.** `(1,1)`
   is ring 1 and `(3,0)` is ring 3, so a Euclidean ring model silently reports different
   spacings.
4. **Retail bounds-checks nothing at the origin.** It indexes `wdata[y*xs + x]`
   directly. The port therefore documents an in-bounds precondition rather than
   inventing an out-of-range policy.

Port: `World::land_dist` in `crates/don-sim/src/systems/map_terrain.rs`, with two
focused tests (§4).

## 2. `MapGreatLakes::make_continents` `0x00699e40`, 2,052 bytes

The style grows ordinary **land** regions on an all-ocean world and then calls
`Map::invert_land`, so the regions it created become the lakes.

| stage | VA | effect | RNG |
|---|---:|---|---|
| avoid-continent rescale | `0x00699e80` | `Map+0x30 = max(scale_land_area(Map+0x30, players), 6)` | none |
| player-land radius | `0x00699e92`–`0x00699eb1` | `ceil(max_range / 4)` from `[[[0x00c061fc]+0x10]+0x574]+0x1fc` | none |
| wipe / clear | `0x00699f63`, `0x00699f68` | `World::wipe`, `Regions::clear_all` | none |
| angle | `0x00699f88`, `0x00699fa0` | `(a % 0xffff) << 16 + (b % 0xffff)` | 2 draws |
| areas | `0x00699fb5`–`0x00699ff4` | `area_max = max(size/20, 90)`, `area_min = max(size/40, 24)` | none |
| lake count | `0x0069a002` | `remaining = xs/12 + (draw & 1)` | 1 draw |
| loop head | `0x0069a043` | `attempts += 1`; `attempts >= 100` ends the loop | none |
| spacing threshold | `0x0069a060`–`0x0069a0c3` | `required = max((Map+0x30 + 1)/2 + isqrt(area*7/22), 3)` | none |
| candidate | `0x0069a150`–`0x0069a1a9` | `x = draw % xs`, `y = draw % ys`, each skipped when the axis is ≤ 1 | 2 draws per candidate |
| spacing test | `0x0069a255` | `Map::land_dist(x, y, 1) >= required` | none |
| give up | `0x0069a272` | 1,000 rejections jump to the shrink arm | none |
| validity | `0x0069a32d` | `Map::grow_valid(last_region + 1, x, y)` | up to 4 draws |
| seed + grow | `0x0069a3fe`, `0x0069a415` | `Map::make_region(region, x, y, area)`, `Map::grow_region(region, area, xs, -1, -1, 0)` | grow-internal |
| bookkeeping | `0x0069a41a` | `remaining -= 1`, `attempts = 0` | none |
| area re-pick | `0x0069a424`–`0x0069a455` | `area = area_min + draw % (area_max - area_min)` when that span > 1, else `area_min` | 0 or 1 draw |
| shrink | `0x0069a45a`–`0x0069a487` | `area > area_min` ? `area_max = area = area_min` : `area_min = area_min*13/16` (and stop below 20) | none |
| loop tail | `0x0069a489` | repeat while `remaining != 0` | none |
| pools + invert | `0x0069a499`, `0x0069a4a1` | `Map::eliminate_pools(EntireWorld)`, `Map::invert_land` | none |
| starts | `0x0069a4c0`–`0x0069a5ef` | per player, spiral inward from `(xs*3)/2` in steps of −3 | none |
| player land | `0x0069a617` | `Map::check_player_land(1, Map+0x30, radius, _)` | none |

Two branch facts a reader of the decompiled C would miss:

- **`Map::grow_region`'s return is never tested** (`0x0069a415` is followed directly by
  `dec dword [ebp-0x2c]`). Unlike Mediterranean and East Indies, this style has **no
  whole-pass retry**, so a stalled growth still consumes a lake.
- The shrink arm compares the **current** `area` (`[ebp-0x20]`) against `area_min`
  (`[ebp-0x14]`), not `area_max` against `area_min`, and consumes **no** draw. Ghidra
  renders this as `local_18 < local_24` with the two roles swapped.

### 2.1 Start placement, `0x0069a4c0`

```text
angle  += 0xffffffff / players            ; unsigned divide, computed once at 0x00699f6d
radius  = (xs * 3) / 2                    ; signed, truncating
retry:  project(xs/2, xs/2, angle, radius, &x, &y)   ; BOTH centre args are xs >> 1
        radius -= 3
        edge = any of the five int offsets at 0x00add250 (x) / 0x00add210 (y)
               = [(0,0), (0,-1), (1,0), (0,1), (-1,0)]
               landing off map or on row/column 0 or xs-1 / ys-1
        if (x,y) off map            -> retry
        if WorldData::is_ocean(x,y) -> retry      ; i.e. it fell in a lake
        if edge                     -> retry
        World::add_starting_location(x, y)
```

`0x00add250`/`0x00add210` are the shipped four-ring `dx`/`dy` tables shifted back one
entry, so the candidate itself is included in the cross. The values were read from the
image, not inferred from the neighbouring tables.

The retail loop has no attempt cap; the port bounds it at `wdata.len() * 32` and raises
`ContinentError::StartPlacementUnavailable` rather than hanging.

## 3. Boundary movement

`InitialItemBoundary` for map style 14:

| | before | after |
|---|---|---|
| boundary name | `map_land_distance` | `terrain_groups_fill_fertile` |
| exact retail VA | `0x0069d970` (`Map::land_dist`) | `0x006a6f90` (`TerrainGroups::fill_fertile`) |
| style virtual | stopped inside `0x00699e40` | returns at `0x0069a641` |

Style 14 now lands on exactly the same common boundary Mediterranean reaches. The local
capture has no `ron-data/tilesets.xml`, so the fertility stage cannot be admitted and the
recorded stop is `TerrainGroups::fill_fertile`; with that file installed the same path
continues one stage further to `terrain_groups_place_all` / `0x006a70d0`.

`Map::grow_valid` `0x0069d000` was already ported as a private helper of
`Map::grow_region`; this lane exposed it as `growth::execute_grow_valid` so the style
virtual can call it directly, which is what retail does at `0x0069a32d`. Its own
`ring_end[avoid_continent]` scan, `vector_dist(|x - xs/2|, |y - ys/2|) < Map+0x2c`
rejection, and per-margin `Map+0x64` jitter draw at `0x0069d2ab` were re-read against
the disassembly and match the existing port.

Measured on `ron-data/replays/multi/Playback___2024.02.24_21_25_53__Sat_.rcx`
(style 14, 100×100, seed `0x00017eed`, 33,690 checksum packets): the `world` channel
previously walked 780,168 bytes and produced `0x744fcb8d`; it now walks 780,476 bytes
and produces `0x0aa120ea`. Retail records `0xe81faf2a`. **The channel still diverges on
the first checksummed turn** — the generated bytes remain `unsourced` (76 sourced, as
before), because terrain groups, resources, coastline tail and rivers are still
unported and because nothing here is oracle-executed.

## 4. Tests that can fail

| test | file | what it would catch |
|---|---|---|
| `land_dist_measures_rings_to_the_first_non_ocean_cell` | `crates/don-sim/src/systems/map_terrain.rs` | a Euclidean ring model, testing `land` before `WATERHALF`, or scanning ring 0 |
| `land_dist_edge_flag_selects_whether_off_map_counts_as_land` | same | ignoring the third argument, or always/never counting off-map offsets |
| `standalone_grow_valid_draws_per_margin_and_honours_the_avoid_continent_gate` | `crates/don-replay/tests/growth_reconstruction.rs` | a direct `grow_valid` call that draws the wrong number of jitter values, that errors instead of returning retail's `0` above radius 64, or that indexes `ring_end` below zero |
| `four_complex_styles_reach_distinct_concrete_calls_without_skipping_draws` | `crates/don-replay/tests/continent_reconstruction.rs` | a lake accepted below its own spacing threshold, a start placed in a lake or on the map edge, a `grow_valid` acceptance that skipped a margin draw |
| `checksum_bearing_great_lakes_replay_runs_its_whole_style_virtual` | `crates/don-replay/tests/initial_item_reconstruction.rs` | the production replay path failing to reach `HookComplete`, or a spurious retry stop |

Both `land_dist` tests were mutation-checked: deleting the `edge_is_land` return makes
the second fail (`left: 65, right: 1`), and testing `land` before `WATERHALF` makes the
first fail (`left: 65, right: 2`).

## 5. What is deliberately not claimed

- **Mostly no oracle case.** `Map::land_dist` **is** now Tier B: registered in
  `crates/oracle` and differentially compared over 100,041 trials with zero mismatches,
  including its answer distribution and termination reasons
  (`schema/oracle-regression.json`, case `land_dist`). Its ABI note carries one correction
  to this document's reading — the PDB marks the function virtual, but the body never reads
  `ECX` and both `WCoord`s arrive **by value** at `[ebp+8]`/`[ebp+0xc]`.

  Everything else here remains Tier C. `Map::grow_valid` and
  `MapGreatLakes::make_continents` still have no executable retail differential, and the
  `land_dist` case proves the leaf over generated `WData` planes — it claims nothing about
  candidate selection, the spacing threshold, or whether a seeded generator ever produces
  the planes fed to it.
- **The `check_player_land` radius is a carried constant.** `MAP_PLAYER_LAND_RADIUS = 6`
  comes from the unit-type chain above; no crate in this workspace loads
  `unittypes.items[349]` yet, so the value is inherited from the Mediterranean lane's
  reading of `NAVAL_ROSTER`/`unitrules.xml` rather than measured here.
- **`Map+0x18` (the `make_region` flags source) is still zero by assumption.** It is
  read by `Map::make_region` at `0x0069d3f0` and written by `Map::load_map_data`; this
  lane did not derive it and reused the existing crate-wide value.
- **`Map::grow_region`'s own fidelity is unchanged.** Great Lakes calls it with
  `max_distance = xs`, a domain the Mediterranean and East Indies tests do not cover.
- **Generated bytes are still `unsourced`.** A ported generator is our model, not
  replay-carried state; the accounting in `InitialWorld` deliberately keeps them out of
  `sourced_walked_bytes`.
