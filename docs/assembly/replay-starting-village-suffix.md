# Fresh starting-Village City scan suffix

This tranche closes the fresh, starting-town-1 path through `City::find_buildings`,
`City::check_upgrade`, and `City::regen_roads`.  Evidence is the supported
`ron-bin/riseofnations.exe`, matching `rise.pdb`, Capstone instruction reads, the PDB
`CityData`/`BuildData` layouts, and shipped `ron-data/rules.xml`.  No replay checksum or
live-memory value is an input.

## One-center result

The result is exactly **no checksum-visible City mutation**.

`City::find_buildings` (`0x007384c0`) scans the Build and Wall object ranges of the eight
Leader rows.  At `0x0073858f` it tests `ObjectData::flags +8` with `0x20` and jumps past the
candidate at `0x00738593` when the bit is set.  The ordinary center returns from activation
with flags `0x27` (VALID, STARTED, ACTIVE, CITY), so it never reaches the current-city,
owner/civilization, distance, or `Build::add_to_city` forks.  All other starting-town-1
rows are City centers with the same rejection bit; starting Units are in the Unit range
and are not part of this scan.

After all rows, `0x00738954` calls `City::check_upgrade` (`0x00738b20`).  The first call at
`0x00738b41` is `CityData::ready_to_upgrade` (`0x00736480`).  For current type Village
(`0x19e`) it asks `LeaderData::type_avail(TOWN, 1)` and, only if true, calls
`CityData::can_upgrade(TOWN)` (`0x007383c0`).  The latter passes
`Constants::city_buildings + 1` to `CityData::enough_kinds` (`0x00736540`).  The shipped
constant is 5, so Town requires six distinct active building kinds.  The fresh chain starts
at the Village center and immediately ends at `city_down == -1`, yielding one kind.  Thus
`ready_to_upgrade` is false and none of `check_upgrade`'s Build replacement, mask, Leader,
population, or presentation body executes.  This remains true whether Town availability
short-circuits before `can_upgrade` or permits that one-kind census.

`City::regen_roads` (`0x00738aa0`) first checks the City active bit, loads the center Build,
then begins at `center.city_down` (`0x00738ac6..0x00738acc`).  It would OR `0x100` into each
member's `BuildData::build_masks` at `0x00738af8`, following the next `city_down` at
`0x00738b0f`.  A fresh center has `city_down == -1`, so there are zero member visits and
zero Build writes.  The center itself is not modified.

Consequently the combined suffix has:

- zero `CityData` POD writes;
- zero `BuildData::city`/`city_down` writes and zero road-mask writes;
- zero Leader, World, registry, or population writes; and
- zero main RNG draws.

This also means a first-checkpoint Cities checksum that falsifies the immediate
`City::init` image cannot be repaired by inventing a fresh `find_buildings` field suffix.
Its source must be earlier constructor input or later pre-checkpoint simulation.

One such later mutation is now source-located.  `Leader::plan_strategy` (`0x006b9620`,
11,108 PDB bytes) walks each live City beginning near `0x006bb600`.  It clears fourteen
terrain-census bytes within City object offsets `+0x62..=+0x71`, then scans the surrounding
WData circle and recomputes `ocean`, `land`, `filled`, `ocean_filled`, `dock_tile`,
`space[3]`, and `ter[6]`.  (`bordering` at `+0x65` and `was_capital_flags` at `+0x68` are
not part of this clear.)  Those fourteen bytes are in the 108-byte checksum-walked City POD.
The first known recorded Cities value is stable from turns 2 through 31, so the constructor
image is not the right comparison boundary until this post-setup strategy census is owned.
The typed receipt therefore keeps `first_checksum_city_image_ready == false`; no output
checksum is fitted back into those bytes.

## Exact strategy census

The census body is now transcribed as an atomic typed transaction.  Function identity and
layout come from the GUID-matched `rise.pdb`; control flow below was reread with Capstone
against the supported PE.  Its direct/nested source boundary is:

| VA | PDB symbol | role |
|---:|---|---|
| `0x006bb400` | block inside `Leader::plan_strategy` | per-City clear and circle scan |
| `0x006e1370` | `LeaderData::has_tribe_bonus` | Indians radius bonus `0x15` |
| `0x00737dc0` | `City::count_gather_slots` | one-center result is zero |
| `0x00680760` | `Region::num_coasts` | dock-region eligibility |
| `0x00636700` | `BuildTypeData::is_dock_tile` | exact WData/TData shore predicate |
| `0x006b26e0` | `WorldData::check_building_wcoord` | inner-cell placement grade |
| `0x006b27f0` | `WorldData::space_at_corner` | exact 4x4 TData probe |
| `0x006b07f0` | `World::gather_at` | installed `LandData`/`GoodTypeData` quantities |

For a center of level 1/2/3 the tile radius is
`CITY_CENTER_RADIUS + (level-1)*CITY_CENTER_POP_RADIUS`, plus
`INDIANS_CITY_RADIUS` when tribe bonus `0x15` is present, and clamped above at 64.  The
shipped values are 20, 4, and 4.  `Leader::plan_strategy` converts that to the circle-table
index by signed `(radius + 2) / 4`.  Thus an ordinary Village uses index 5; an Indian
Village uses index 6.

The table walk has a non-obvious off-by-one which matters to every byte count.  Retail
stores loop counter `i = 0` but reads `circle_x[i+1]` / `circle_y[i+1]`.  Its bottom test is
`(i+1) < circle_radius[index+1]`.  Therefore the actual Rust-style table range is
`1..circle_radius[index+1]`: the sole `(0,0)` entry at table index zero is skipped, while
the final outer entry is included.  The inner arm is admitted exactly when the same table
index is `< circle_radius[index]`.  For a normal Village the cumulative endpoints are
105/145, producing 104 inner and 144 total cells.  An Indian Village produces 144 inner
and 184 total cells.  Order is the order generated by `circle_init` `0x006817f0`, reused
through the canonical `CircleTable` owner.

Each in-bounds WData cell then follows this exact sequence:

1. Reject it completely when signed `who >= 0 && who != Leader::who`.  Unowned `-1` and
   self-owned cells are admitted.
2. When `WData::flags & 0x100` (`WATERHALF`) is clear and signed `land` is 1 or 2, increment
   `City::ocean` with byte wrapping.  Resolve `WData::region`, and consider a dock when
   `Region::num_coasts() > 1 || Region::size >= World::size / 10`.  Until `dock_tile`
   becomes nonzero, run the complete 4x2-shore `is_dock_tile` predicate.  The water arm
   then jumps to the loop bottom; it never enters the inner land path.
3. Otherwise, only table indices inside the inner endpoint whose WData region equals the
   City region continue.  `WATERHALF` deliberately reaches this path even when its signed
   `land` byte is 1 or 2.
4. When `flags & 0x70 == 0`, increment `land`, then call
   `check_building_wcoord(wx, wy, owner, unused_region, 0, 0, 1, 1)`.  The unused region
   argument is real stack traffic; the effective Rust call is radius 0/0, max distance 1,
   `need_city=true`.  Grades 0 and 1 both increment `filled`; grade 2 increments
   `space[0]` and `filled`; grade 3 increments `space[0..2]` and `filled`; grade 4 increments
   all three `space` bytes and continues to gathering.  The existing exact
   `check_building_wcoord -> space_at_corner` World implementation supplies the result.
5. An impassable `flags & 0x70` cell, or placement grade 4, calls
   `World::gather_at(..., mode=1)`.  Each of the six `ter` bytes becomes signed
   `max(current_u8, output_i32)` followed by the machine's low-byte store.  This call reads
   installed ordered `LandData`, `GoodTypeData` virtual predicates, and center TData in
   addition to WData.  Those objects do not live in `map_terrain::World`, so the transaction
   accepts coordinate-keyed typed results and fails atomically if a reached result is
   missing.  It never derives a gather value from the replay checksum.

The clear/write set is exactly `ocean +0x62`, `land +0x63`, `filled +0x64`,
`ocean_filled +0x66`, `dock_tile +0x67`, `space[3] +0x69..+0x6b`, and
`ter[6] +0x6c..+0x71`.  `ocean_filled` is cleared but not incremented in this block.
`bordering +0x65` and `was_capital_flags +0x68` are neither cleared nor written.

The receipt also retains the adjacent Leader joins instead of hiding them: current City
population is a candidate for the maximum at `LeaderData +0x84c`; the fresh center-only
`count_gather_slots` contribution is zero; `city_flags & 2` contributes zero for fresh
flags `0x4011`; `(land-filled < 2)` contributes to `LeaderData +0x9d0`; and the signed-short
`land-filled` result contributes to the City region accumulator at `+0x13de`.  The City
census itself makes zero World, Build, registry, or main-RNG writes.

### Real style-6/style-9 boundary

The source transaction was exercised over both starting centers in the three admitted
recordings, using the exact replay-carried camera centers and the reconstructed style-6/9
Worlds:

| replay | map | source-censused partial-setup Cities | first recorded Cities |
|---|---|---:|---:|
| 2018-11-17 | Himalayas (9) | `e5c40bd2` | `53130d2c` |
| 2020-02-08 | Old World (6) | `993e0b1e` | `dd170c24` |
| 2020-02-21 | Himalayas (9) | `42680ae2` | `7d2d0bd4` |

The recorded column is comparison-only.  It was never read by the producer.  In the
current partial setup World every reached inner cell returns placement grade zero because
the activation/territory prefix has not installed the required TData `CITY` bit; the exact
result is `land == filled` (104 for radius 20, 144 for radius 24) and zero gather calls.
The mismatch therefore remains a useful, explicitly localized residual rather than a value
to fit: the post-activation World/TData producer and remaining frame-zero schedule must be
joined before this is the retail census input.

Nor is this the only remaining City mutation.  Earlier in `Leader::plan_strategy`, every
live City has `gatherers`, `busy`, and `free` cleared and `peasant_dist` set to 100; the
intervening Unit census can rewrite `free`, `busy`, and `peasant_dist`.  Those Unit/Leader
inputs, the exact activation/territory World, and the complete pre-checkpoint scheduling
boundary are not yet owned here.  Consequently both the constructor-suffix receipt and the
new census receipt keep `first_checksum_city_image_ready == false` even though the fourteen
terrain bytes themselves have an executable, source-bounded owner.

## Typed boundary

`attest_fresh_starting_village_suffix` reads the canonical `Sim::builds` rows and current
production ptype.  It validates the City/center owner, object id, slot, position, Village
type, exact `0x27` flags, and empty `city_down` chain.  It also rejects every scanned Build
without Object flag `0x20`: such a row reaches the general distance/ownership/AddToCity
fork and belongs to the later general City transaction.

`apply_fresh_starting_village_terrain_census` first consumes that suffix receipt, then
stages the complete City mutation against the canonical World and Regions owners.  Missing
World shape, Region identity, impossible placement grades, or reached installed-content
gather facts refuse before the caller's City changes.  Its receipt preserves exact table
endpoints/order, branch counts, placement histogram, nested call counts, all fourteen bytes
before/after, the two intentionally untouched bytes, Leader deltas, and zero-effect joins.

The receipt intentionally reports `constructor_transaction_ready() == false`.  It proves
this suffix, not `Object::add_to_world`, the remaining Wall/Build scalar body, terrain
masking, Leader counters, or atomic publication of the Builds and Cities channels.  The
complete starting-Village constructor must consume this receipt alongside those owners;
it must not reinterpret an inert suffix as a completed Build initializer.

Focused gate:

```sh
cargo test -p don-replay --test starting_village_suffix
```
