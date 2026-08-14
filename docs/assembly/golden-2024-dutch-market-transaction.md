# Golden 2024 Dutch Market transaction

This note freezes the source-backed retail chronology for the Dutch tribe-22 starting Market.
It does not choose a placement from fitted replay bytes. The executable adapter stops at the
first generated-World-dependent child and exposes the exact request needed to resume there.

## Function identity and caller order

The PDB fixes `Leader::produce_building(int type, int origin_o, int mode)` at `0x006E1400`,
7,406 bytes, exclusive end `0x006E30EE`. The older `Leader::free_build` label for this VA is
incorrect.

`Setup::build_civ_specific` at `0x005AB760` tests tribe bonus 4 before bonus 22. Bonus 4 already
grants a Market; otherwise Dutch bonus 22 calls:

```text
Leader::produce_building(type=436, origin_o=2000, mode=0)
```

This call is before the seven starting-Unit calls. A setup receipt that certifies only center
`o2000` plus those Units is therefore incomplete; the replay binder must require the Market
transaction between them.

The supported Type profile is Market `436`, Build domain, land domain zero, footprint `4x4`, and
`build_flags = 0x80001201`. The fresh first Village is `o2000`, active, linked to City slot zero,
at Coord `(74592,10080)` / WCoord `(97,13)`.

## Placement search and the exact World boundary

The entry resolves the origin object three times through the registry virtual: active Build,
Object CITY bit, then signed `BuildData::city`. It admits City slot zero only when
`CityData::city_flags & 1` is set. Market lacks the `build_flags & 0x10` fallback. The next
instruction after this gate is `0x006E150D`.

`LeaderData::get_radius` returns the Village radius 20, which selects circle index
`(20 + 2) / 4 = 5`. Frame zero skips the later-frame placement policy and resource payment.
Each coarse candidate then executes, in order:

1. WCoord bounds and `WData::flags & 0x4000`;
2. `WorldData::check_building_wcoord(..., need_city=1)`, requiring result greater than one;
3. grade and edge checks for the 4x4 footprint;
4. `WorldData::buildings_allowed` and the land/ocean polarity check;
5. `BuildTypeData::blocked_site(candidate_coord, owner=0, city_constraint=-1, detail=0)`.

`blocked_site` computes the top-left 4x4 TCoord and calls `blocked_tcoord` sixteen times in x
outer / y inner order. On ordinary dry City terrain, Market passes the common bounds, visibility,
blocker, surface and City gates and returns raw zero before `LandData::get_amount`: its flags carry
neither resource arm (`0x40` nor `0x10000000`). With sixteen raw-zero children and the negative
City constraint, the detail and locally-seen accumulators remain zero. The parent next calls
`BuildTypeData::blocked_location` at call site `0x00636D18`.

The first child inside `blocked_location` is:

```text
0x006375F7 -> WorldData::get_tregion(TCoord x, TCoord y) at 0x006B52E0
```

Its return reads the generated World region plane. This is the first independently unowned
boundary. The concrete accepted coarse cell, placement Coord, selected City/region verdict and
fine-probe validity mask must come from that World owner. No static adapter may infer them from
the center coordinate or from a later checksum.

After that boundary, the statically selected Market branch is known even though its concrete
answers are not. Market is neither City, Fort nor Dock, lacks `build_flags & 0x20`, and calls
`BuildTypeData::non_friendly_territory` at `0x0063768B`. A surviving site calls
`BuildTypeData::get_town`; the fresh City must be found, forbidden center types `0x29A/0x29B` must
be absent, and `build_flags & 0x200` invokes `CityData::count_buildings(436,0,0)`. The fresh count
must be zero. The later land-only water count must also accept the footprint. These results remain
World/City-owned inputs to the eventual continuation.

## Score and RNG cadence

After a zero `blocked_site` result, retail calls `BuildTypeData::find_friends`. For the ordinary
fresh Market result of zero it calls `vector_dist` between candidate and origin. The base score is
1,000 through distance four and 333 above four. It then adds `255 - WData::val`. Market is not a
gather enhancer and is not the Farm/Mine cohort, so it consumes **no coarse RNG draw**. Later
equal-scoring candidates replace earlier ones because the comparison is `best <= candidate`.

The chosen coarse cell produces a 2x2 fine scan, again x outer / y inner. Every fine candidate
first calls `blocked_site`. Only a zero result consumes `Random::get(0,65535)` at `0x006E2C00` and
scores `draw % 100`; a rejected probe consumes no draw. The initial fine score is `-1` and later
ties replace earlier sites. The Market consumes exactly one draw for each valid fine probe. The
first fine candidate is the already accepted coarse
site, so it necessarily succeeds again: the exact total is **one through four**, never zero. There
are no later Market-specific RNG calls. The exact draw
count and selected site are unresolved until the World placement receipt supplies the four-probe
mask.

This is the main simulation stream, not a private placement RNG. The retail call site
`0x006E2BF4` loads `ECX` from `0x00C06184`; the PDB identifies that object as
`GameAccess::game_random`. `Random::get` at `0x00A39D70` advances the pointed-to LCG state before
returning the scaled low-word result. `Setup::build_game` also consumes collision-retried draws for
its eight-slot player/start shuffle after `place_all` and before `build_cities`. Consequently the
post-`place_all`, post-shuffle/pre-Market, and post-Market `Setup::build_units` RNG boundaries must
remain independently bound. Only Market-after is equal to the BuildUnits-entry state.

## Allocation, City link and activation chronology

Given a World-approved fine site, the remaining source-selected call order is:

1. `Objects::init_build(owner=0,type=436,x,y,0,-1)` at call site `0x006E2CA3`.
2. `Objects::find_free(0,2000,3000,&build_mark,-1)` allocates the fresh Build-band object. With
   center `o2000` and mark 2001 this is `o2001`, and the mark advances to 2002.
3. `BuildTypeData::snap_center`, registry resolution, then virtual
   `Build::init(0,436,2001,x,y,0)` at `0x00629740`.
4. `Wall::init -> Object::init -> SubObject::init -> Object::add_to_world`; only after this chain
   does the Build exist in the Object/WData down chains.
5. `Build::clear_gather`; mode zero increments `LeaderData::buildings_built`; the remaining Build
   sentinel/type/owner/founder state is initialized.
6. `Build::find_city -> BuildTypeData::get_town -> Build::add_to_city`. This is before ACTIVE:
   Market writes `city=0`, the former chain tail `o2000.city_down=2001`, and
   `o2001.city_down=-1`. Because the Market is still inactive, `add_to_city` neither increments
   building statistics nor sets the Market City bit.
7. Market's training-building classification initializes `BuildQueue` with capacity 20.
8. `Objects::init_build` returns `o2001`; its land/non-resource presentation suffix emits the
   Build particle.
9. `Leader::produce_building` resolves `o2001` and calls virtual
   `Build::activate(0,1,0)` at `0x00623E20`.
10. `Build::activate` calls `Wall::activate(0,1,0)`. `Wall::activate` marks the Game dirty,
    calls `Build::start(1)` when not already started, then sets ACTIVE, mask `0x1000`, and clears
    both construction counters.
11. `Build::start -> Wall::start` sets STARTED and the frame stamp, kills competing buildings,
    gets the footprint corner, calls `Terrain::object_placed`, masks the footprint, updates owner
    and visibility planes, calls `Wall::check_ever_seen`, and marks behind tiles.
12. Now that `o2001` is active and City-linked, `Wall::increment_stats` increments the Leader's
    type and region building counts. `Wall::activate` then ORs the Leader dirty bits, removes the
    covering doober, and returns.
13. The exact Market branch ORs `City.city_flags` with `0x800`, then calls
    `City::regen_roads`. It increments Leader gather slot two and its high-water mirror, then
    updates the type-436 building high-water count.
14. The common activation suffix calls `Wall::update_hits(0)`, `Wall::update_los()`,
    `City::check_upgrade()` for the linked non-City Build, and `Object::update_seen(0)`.
15. Back in `Leader::produce_building`, `City.filled` increments once. For the selected
    `check_building_wcoord` grade, bytes `City.space[0..grade-2]` each decrement once, clamped at
    zero. The function returns native zero.

Frame zero skips `TypeData::pay_cost`, builder/Group selection and Build orders. Market also skips
Farm registration, gather-tile initialization, wonder logic, attack counters and Dock transport
checks. Thus this starting call creates no Group or Unit mutation.

## Owner ledger

| owner | exact Market effects |
|---|---|
| Objects / Builds | allocate `o2001`; Object and WData chain insertion; type/owner/founder and Build sentinels; STARTED/ACTIVE; mask `0x1000`; construction counters zero; City link; queue capacity 20; hit/LOS/seen state |
| City slot 0 | chain tail `o2000 -> o2001 -> -1`; Market flag `|= 0x800`; road regeneration; `filled += 1`; grade-selected space-byte decrements; upgrade check |
| Leader 0 | `buildings_built += 1`; `num_buildings[22] += 1`; when region is below 64, `region_buildings[region][22] += 1`; `gather_slots[2] += 1`; both high-water mirrors take `max`; dirty bits `0x02000000` then `0x08000000` |
| World / Terrain | object/occupancy down chains, footprint placed/masked state, owner/visibility and behind-tile state, covering-doober removal, road regeneration effects, seen projection |
| Game | activation dirty scalar/byte projection; no frame-zero payment or AI production counters |
| Random | no Market coarse draw; one fine draw per valid fine probe, one through four total; earlier setup shuffle draws remain a separate owner |
| Groups / Units | no mutation on the frame-zero path |

The adjacent golden capture reports grade four. If that independent capture is accepted, the
fresh first-Village after-image is `City.flags 0x4011 -> 0x4811`, `filled 1 -> 2`, all three
`City.space` bytes decremented once, `o2000.city_down=2001`, and
`o2001.{city,city_down}={0,-1}`. Grade four is capture authority, not a static placement guess.

## Executable seam and fail-closed integration

`LeaderProduceBuildingBlockedSiteRawZeroFootprintReceipt` owns the complete sixteen-tile
raw-zero cohort. It validates the exact x/y visit order and refuses any nonzero verdict or
resource continuation. `LeaderProduceBuildingMarketBlockedLocationRequest` then validates the
installed Market profile and records the exact placement Coord/TCoord, footprint corner, owner,
City constraint and first World child:

```text
plan_sim_leader_produce_building_market_blocked_location_request(...)
  -> request at 0x00636D18 / blocked_location 0x006375B0
  -> first World child 0x006375F7 / get_tregion 0x006B52E0
```

Both APIs are read-only. They do not certify placement, consume RNG or mutate City/Build/World.
The setup discovery remains fail-closed until the City/Build capture binder consumes this request,
supplies the generated-World result and validates the full post-Market receipt before the seven
Units.
