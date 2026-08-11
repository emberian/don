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

## Typed boundary

`attest_fresh_starting_village_suffix` reads the canonical `Sim::builds` rows and current
production ptype.  It validates the City/center owner, object id, slot, position, Village
type, exact `0x27` flags, and empty `city_down` chain.  It also rejects every scanned Build
without Object flag `0x20`: such a row reaches the general distance/ownership/AddToCity
fork and belongs to the later general City transaction.

The receipt intentionally reports `constructor_transaction_ready() == false`.  It proves
this suffix, not `Object::add_to_world`, the remaining Wall/Build scalar body, terrain
masking, Leader counters, or atomic publication of the Builds and Cities channels.  The
complete starting-Village constructor must consume this receipt alongside those owners;
it must not reinterpret an inert suffix as a completed Build initializer.

Focused gate:

```sh
cargo test -p don-replay --test starting_village_suffix
```
