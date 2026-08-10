# AIR_PATROL building-search frontier

Status: **registered executable contract; live `Sim` bridge remains red.**

This frontier owns the exact read-only transaction at the mod-32 tail of
`Unit::do_air_patrol` `0x005EA620`. The executable owner and focused tests are in
`crates/don-sim/src/systems/air_patrol_building_search_frontier.rs` and
`crates/don-sim/tests/air_patrol_building_search_frontier.rs`. Registration makes the snapshot
owner callable by adapters but does not claim live integration.

## Call order and fixed arguments

Retail reaches the branch only after `Unit::do_air_physics` succeeds and the actor's animal
virtual returns false. The last-waypoint retirement and the mod-16 unit/bomber target arm run
first; either can return before the building query. The remaining cadence is the signed MSVC
remainder idiom:

```text
(sign_extend(actor.o) + Game::frame) % 32 == 0
```

The query point is the **current waypoint after the arrival transition**. A non-strict type
query for `0x134` identifies the fighter-bomber transform: when its stored patrol-home object
is live, retail adds the home's decoded `Coord` and calls `WorldData::restrict`. It then maps
each coordinate through `div_3_table[coord >> 6]`, yielding TCoord tiles, and calls:

```text
ObjectsData::find_building_at(tx, ty, SEARCH_ENEMY /* 3 */,
                              actor.who, FILTER_ALL /* 0 */, 0, ignored)
```

The final stack dword is unused by `find_building_at`; the two meaningful tail arguments are
exactly `FILTER_ALL, 0`.

## Exact `find_building_at` traversal

`ObjectsData::find_building_at` `0x0065AB40` first stores `-1` in
`ObjectsData::find_who +0x200`. It rejects an out-of-range TCoord and rejects any tile whose
`TData::mask & 3` is not the building blocker value `3`. Only then does it compute
`(tx >> 2, ty >> 2)` and visit up to nine WCoord cells:

| ordinal | offset |
|---:|---:|
| 0 | centre `(0,0)` |
| 1 | north-west `(-1,-1)` |
| 2 | north `(0,-1)` |
| 3 | north-east `(1,-1)` |
| 4 | east `(1,0)` |
| 5 | south-east `(1,1)` |
| 6 | south `(0,1)` |
| 7 | south-west `(-1,1)` |
| 8 | west `(-1,0)` |

Out-of-range WCoord cells are skipped. Each admitted cell starts at
`WData::down/down_who +0x08/+0x0A`. While `o >= 0`, retail resolves `objects.lists[who][o]`
and reads the successor `ObjectData::down/down_who +0x2C/+0x2E` before applying gates. The
candidate gates, in order, are:

1. `who < 8`; a negative `who` bypasses `Search::valid_search`, while `who >= 8` fails.
2. `Search::valid_search(SEARCH_ENEMY, actor.who)`. Case 3 calls
   `LeaderData::is_enemy`: owners differ and either directional diplomacy dword is zero.
3. virtual `BuildData::is_valid_build` at vtable `+0x0C`.
4. virtual `WallData::is_active` at vtable `+0x4C`.
5. virtual `Build::get_build` at `+0x40`, then `WallData::tile_corner` `0x00643440`.
6. half-open footprint membership:
   `left <= tx < left + ObjectTypeData::x_size` and
   `top <= ty < top + ObjectTypeData::y_size`.
7. The optional filter arm. `FILTER_ALL == 0` bypasses it at this callsite.

The first hit stores its chain owner in `ObjectsData::find_who` and returns its `o`. This is
not a nearest-distance search and it does not sort the linked list.

## The post-return gate is `BuildData::ever_seen`

The callsite reloads `(find_who, returned_o)`, calls virtual `Build::get_build` at vtable
`+0xAC`, and reads byte `+0x62`. The PDB layout identifies that byte as
`WallData::ever_seen`, inherited by `BuildData`:

```text
(u32(build.ever_seen) & (1u32 << (actor.who & 31))) != 0
```

It is **not** `ObjectTypeData+0x62`; that offset lies inside `TypeData::name`. A passing bit
inserts `STRAFE` with `mandatory=1`, `QUEUE_FIRST`, and `group=0`. A failing bit ends the
branch. Retail does not resume `find_building_at`, so an unseen first hit hides any later
visible candidate in the same or subsequent cell.

## Existing support and red host facts

Useful state already exists, but no single live owner assembles it:

- `map_terrain::World` exactly owns `tile_xs/tile_ys`, `TData::mask`, and
  `WData::down/down_who`, including `World::set_down`.
- `ObjectRegistry` owns the ordered per-player band-2000 rows, and save/load preserves the
  build-row traversal.
- `Sim::builds` owns `BuildData` rows. Its typed projection includes `flags`, `who`, `uid`,
  and `ever_seen`; the byte image retains `o`, decoded-position storage, and the down link.
- `production::Footprint` already owns `ObjectTypeData::x_size/y_size` geometry.
- `leaders::Leader` owns the directional eight-slot diplomacy dwords.

The live bridge remains red for all of these reasons:

1. Build creation/removal does not prove that `map.world.wdata[*].down/down_who`, each
   `BuildData` down link, `world.objects`, and `Sim::builds` are maintained atomically as one
   retail spatial chain.
2. No canonical live building-type table connects each building's current `ptype` to the
   `x_size/y_size` required by `tile_corner` and footprint admission.
3. `BuildData`'s object `o`, decoded `Coord`, and down-link fields remain opaque bytes; there
   is no checked accessor/identity round-trip for this query.
4. The RL `AirPatrolHost::find_building_target` still has no adapter over the map, registry,
   build rows, type facts, and diplomacy in one preflight snapshot.
5. The live AIR_PATROL dispatchers now advance the waypoint before querying and return after
   an accepted mod-16 target before invoking this host; focused `don-env` tests freeze both
   sequencing constraints.
6. The shared patrol result now names this dynamic fact `ever_seen_by_actor`; a live adapter
   must source it from the returned `BuildData`, not its type row.

Until those facts are closed together, returning no building is the only honest live-host
behavior. The frontier owner accepts a coherent explicit frame and fails closed on missing
objects, malformed dimensions, duplicate identities, and cyclic down chains; these safety
faults are host protections, not claims about retail's behavior on corrupt memory.
