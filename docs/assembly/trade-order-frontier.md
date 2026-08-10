# TRADE_ROUTE executor frontier

Status: **source-only executable proof; strict order row 15 remains red**. The isolated
planner in `crates/don-sim/src/systems/trade_order_frontier.rs` freezes the full concrete
payload, installer ordering, owner-major destination fold, route establishment, endpoint
transitions, road/movement boundary, and whole-frame atomic receipt for `Unit::do_trade`.
It is intentionally absent from `systems/mod.rs` and makes no executor, save, tick, or closure
promotion.

This was selected after auditing all seven red order rows. GATHER still depends on the live
gather product cone; GUARD, CAST_SPELL, STRAFE, SPECIAL_ANIM, and GARRISON retain respectively
10, 13, 10, external-world, and 10 typed host tails. TRADE_ROUTE has the most reusable terminal
state: caravan pool/link creation, income recomposition, activation, first-contact arithmetic,
and teardown already exist in `systems::economy`. The audit nevertheless found that this is not
a thin economy splice: the remaining road and production ownership is load-bearing.

## Authority and extent

Ground truth is the shipped `ron-bin/riseofnations.exe` and matching `ron-bin/sbl/rise.pdb`,
cross-checked against `schema/rise-procs.tsv`, `schema/rise-symbols.tsv`, `schema/types.json`,
direct PE32 disassembly, and `re/decomp-all/005ed270.c`.

| symbol | VA | bytes | role |
|---|---:|---:|---|
| `TradeOrder::walk_data` | `0x00483260` | 70 | checksum/save ranges |
| `TradeOrder::clear` | `0x004835C0` | 75 | payload defaults |
| `Unit::add_trade_order` | `0x005E4DC0` | 589 | concrete construction/install |
| `Unit::do_trade` | `0x005ED270` | 4,519 | one trade activation |
| `Unit::think_caravan` | `0x005F5650` | 327 | autonomous retry/city acquisition |
| `Caravan::build_road` | `0x0073DB10` | 880 | parked road search/publication |
| `PathFinder::astar_caravan_road` | `0x00685990` | 2,411 | road-only A* driver |

`Unit::do_trade` occupies exactly `[0x005ED270,0x005EE417)`. The next PDB procedure,
`Unit::do_repair`, begins at `0x005EE420` after alignment.

## Concrete payload and save/checksum framing

`TradeOrder` is 52 bytes. It consists of `TargetOrder@+0`, five concrete fields, a vtordisp,
and the virtual `UnitOrder` at `+0x2C`; `+0x28` is **not** the virtual-base start.

| offset | field | clear value |
|---:|---|---:|
| `+0x08/+0x0C/+0x10` | first `ox/whom/uid` | `-1/-1/0xFFFF` |
| `+0x14/+0x18` | second `oxx/whose` | `-1/-1` |
| `+0x1C` | `started` | `0` |
| `+0x20` | `loaded` | `0` |
| `+0x24` | second `uid2` | `0xFFFF` |
| `+0x28` | compiler vtordisp | `0` |
| `+0x2C` | virtual `UnitOrder` vfptr | compiler-owned |
| `+0x30` | `UnitOrder::flags` | `0` |

The virtual walk submits exact half-open ranges `[+0x30,+0x31)`, `[+0x08,+0x12)`, and
`[+0x14,+0x26)`: **29 payload bytes**. The generated state-schema extractor currently reports
28 because it does not resolve the dynamic virtual-base flag call; the instructions and direct
sum are authoritative. `OrderList::walk_data` prefixes each node with its four-byte order type
and one-byte metric, so a Trade node contributes 34 bytes after the list's four-byte count.
Load clears the list, allocates a 0x34-byte TradeOrder for case 15, returns its UnitOrder view at
`P+0x2C`, reads the metric, and dispatches the same concrete walk.

The installer has a shipped asymmetric UID2 gate. It tests `oxx >= 0 && primary_whom >= 0`, then
reads `objects[whose][oxx].uid`; it never tests `whose`. The proof preserves this at
`0x005E4E67..0x005E4E8F` rather than silently substituting the intuitive second-owner check.
The executor itself never validates either UID; its endpoint lookups use only owner/object slot.
Its exact broad ordering is Queue-New cleanup; clear unit bit `0x200`; allocate type 15; store
primary identity; store `loaded=0`; store secondary identity; store `started=0`; set/clear the
group bit; optionally set unit bit `0x00800000` for the cross-region transport cone; append;
update action.

One related queue quirk remains important for integration. `Group::set_up_insert` asks
`copy_order` to preserve grouped orders for Queue-First, but `copy_order` case 15 falls to its
null default. `finish_insert` has a TRADE_ROUTE reissue arm, yet no cloned TradeOrder can reach
it. The current generic Rust queue-first splice copies every `OrderRec`, so it would incorrectly
preserve an old TradeOrder behind the insertion.

## Executor phase ordering

Every frame first resolves the concrete payload and performs `set_anim(0,0,1)`. A negative unit
caravan slot is a retail assertion path; a caravan with nonzero `making_road` returns immediately.
The first city object and CityData must remain live. Before establishment, the city-centre object
may replace only `order.ox`; retail does not refresh the retained primary UID in that repair.

When `oxx < 0`, retail scans leaders in owner order and cities in slot order. Admission is ordered:
live leader; alliance; foreign-prerequisite/source-owner gate; not the current city; live city;
seen by the actor; empty-route checks from both ends; region/transport compatibility. An admitted
candidate's exact caravan value is compared with strict `>` against a best score initialized to
`-1`, so ties retain the earlier city. The local source-owner scoring arm multiplies by four.
There is no separate candidate-leader liveness predicate. Also, `get_empty_trade_routes` returns
`1 - matching-established-route-count` and retail tests only nonzero: one duplicate rejects, but
two duplicates produce `-1` and pass.
The planner executes this fold over explicit candidate facts; registry traversal and value facts
remain host-owned.

No candidate produces its own terminal choreography: optional local feedback, actor idle byte
99, bare current-order kill, and—only for unit flag `0x00040000`—the next-city recovery call.
This is distinct from the common invalid tail, which kills and calls `think_caravan(1)` only if
the unit has no surviving order.

Before route establishment, either both endpoints belong to the actor or prerequisite `0x2AC`
must be present, and both directions must still have capacity. Retail compares actor distance to
the two CityData centres and writes the nearer endpoint first; a tie retains the original first.
It then appends the caravan link near then far, writes both city-slot/owner pairs into the Caravan,
sets active and established bits, stores `started=1`, recomputes near then far, and calls
`Caravan::build_road`. A `-1` return inserts a direct MOVE to the near endpoint's CityData x/y and
returns; zero/one continue into the endpoint/movement cone. This local swap does not rewrite the
TradeOrder, but remains effective for the rest of the current activation.

The loaded word selects the endpoint-transition check, not the later movement target:

- `loaded != 0` checks the first endpoint, then sets `loaded=0`, unit bit `0x200`, and—when the
  unit is not inside—earning bit 4, both city recomputes, and first-contact publication;
- `loaded == 0` requires the second object to pass `is_active_wallbuild` and
  `get_wall()->is_trade`,
  checks the same rectangular radius, then sets `loaded=1` and publishes first contact when
  outside;
- every surviving frame advances toward the **opposite** endpoint.

The radius is `max(x_size,y_size)*0x60 + 0x306` independently on x and y. This is an axis-aligned
arrival gate, not `vector_dist`.

## Road stack and movement choreography

When the Caravan road stack is nonempty, retail orients it relative to the actor, assigns it to
the unit path, and for adjacent vertices with `abs(dx),abs(dy) <= 0xC0` applies
`x += trunc(dy/3); y -= trunc(dx/3)`. It then pops the next waypoint, inserts MOVE,
conditionally pushes a flag-4 join
point when terrain distance changes, and inverts the retained stack. The frontier requires one
atomic digest/after-image for this non-commutative sequence.

With an empty road stack, retail calls the trade-specific filter-3 `find_nearby_spot` tuple around
the opposite city with `[radius,radius+0xC0]`. A returned point creates MOVE only when its actor
distance is at most `radius+0xC6`; the call return is ignored. Otherwise the frame holds. The existing general unit MOVE arm
is reusable on later ticks, but this trade-specific insertion/search boundary is not yet wired.

`Unit::do_trade` itself has no direct RNG call. The nested road A* does: `calc_road_cost` consumes
the canonical simulation stream per edge relaxation. The frontier therefore binds a half-open
RNG epoch/draw count and search/road/terrain digests in `RoadBuildReceipt`.

## Exact reusable owners and residual

The production integration should reuse these existing economy owners:

- `establish_caravan_route_in_city_pool` for two ordered links, endpoints, and established flags;
- `recompute_city_pool_caravan_income_and_dirty` for both endpoint caches and leader dirtying;
- `activate_caravan_income` at the first earning transition;
- `award_new_caravan_contact` as the arithmetic leaf of a still-missing CityPool/Leader wrapper;
- `end_caravan_route_in_city_pool_from_order` and `close_caravan_unit_in_city_pool` for teardown;
- `OrderQueue` MOVE insertion, `movement::PathData/PathStack`, and `World::set_road_at` as leaves.

The missing runtime ownership is larger than the reusable seam:

1. the 21-byte wire command and Group `action_trade`; the current bridge drops `oxx/whose`;
2. concrete TradeOrder fields in both `Order` and `OrderRec`, exact install/save/resume, and the
   replay generator's unresolved flags byte;
3. the Queue-First null-copy quirk;
4. authoritative owner-major Objects/City/Leader traversal, visibility, capacity, diplomacy, and
   transport facts;
5. Caravan road stack plus `making_road`, `reset_road`, parked open/openref/closed trees, offset,
   end coordinate, and traversed count;
6. the 3,200-expansion road A*, exact edge RNG, validity/cost functions, road renderer effects,
   restart/verify/reset/global invalidation helpers;
7. trade-specific path assignment/smoothing/join and nearby-spot binding;
8. CityPool/CaravanPools/leader state in production `Sim` and one atomic lifecycle adapter;
9. the separate `Unit::work` pre-`do_job` Caravan::build_road continuation arm;
10. one production `Sim::do_frame` commit route.

`movement::PathFinder` must not be reused as the road engine: it is the general unit A*, while
`astar_caravan_road` is a separate parked search with distinct costs, validity, RNG, and state.

## Honest closure delta and validation

`TRADE_OPEN_TAILS` names thirteen still-unowned surfaces, including wire decode, payload/save, Group insertion,
registry scan, road A*/RNG, renderer effects, path mutation, economy atomicity, the pre-job road
continuation, and live tick. Consequently this tranche earns **0 strict order rows**. Marking row
15 Implemented now would be accepted-no-effect and is forbidden.

Focused validation:

```text
cargo test -p don-sim --test trade_order_frontier
```

The proof currently contains eleven executable tests for payload/walk framing, installer ordering
and UID asymmetry, strict destination selection, no-candidate recovery, endpoint orientation,
parked-road handling, activation ordering, nearby-radius refusal, atomic revalidation, and the
honest red-tail inventory. Retail and the VM were not touched.
