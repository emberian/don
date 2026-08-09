# Gathering occupancy, capacity, and payout

Status: **Tier C**. The layouts and transitions below are instruction/PDB/live-data
recoveries, covered by exact Rust unit tests, but have not been differentially executed
against retail. The implementation is `crates/don-sim/src/systems/gathering.rs`.

## The state model

Resource-site occupancy is not `BuildData::gather +0xB8`; that is the trained-unit rally
list. Retail uses an owner-local intrusive chain:

| state | retail field |
|---|---|
| site head | `BuildData::gather_down +0x70 : i16` |
| site capacity | `BuildData::gather_max +0x80 : i8` |
| terrain cells | `BuildData::gather_from +0x98 : MiningList` |
| worker next | `UnitData::gather_down +0x92 : i16` |
| nearby rare Good | `UnitData::good_obj +0x94 : i16` |

All links are owner-local object indices and `-1` is null. `Build::add_gatherer`
`0x0062F640`, `check_gatherers` `0x0062F710`, and `remove_gatherer` `0x0062F8D0`
provide the exact lifecycle. Attachment first tests owner, current count versus the signed
capacity byte, worker validity/type (`0x32..=0x35`), and duplicate membership. Only then
does it prune stale nodes and head-insert. Detach performs ordinary singly-linked surgery
and resets the removed worker link to `-1`.

`BuildData::num_gatherers` `0x00630450` has two modes. Capacity uses `(0,0)` and counts
workers whose current order targets the site. Payout uses `(1,1)`, additionally requiring
`GatherOrder::been_there`. Properties `0x1A6` and `0x1A4` add separately evaluated
`ObjectData::count_inside` contributions for types `0x32` and `0x34`; they are not queue
lengths. Off-map Scholar types `0x34/0x35` whose ultimate `inside_up` container is the
target count in both modes without a `been_there` test.

## Generational targets and commands

A runtime target is `(whom:i32, ox:i32, uid:u16)`. `TargetOrder::target_exists`
`0x0072FF10` checks the live bit and exact UID, preventing a recycled owner-local slot from
becoming the old target. `UnitData::is_gathering_at` `0x00608880` intentionally compares
raw `(whom,ox)`; the order executor owns the UID check.

The resource-gather network command is exactly nine bytes: opcode `0x13`, target object
index `i32`, and `QueuePos i32`. It does not transmit a UID. `Unit::add_gather_order`
`0x0061A5C0` captures the target's current UID when each selected unit constructs its
52-byte `GatherOrder`. The order checksum payload is 31 bytes: one base byte, ten target
bytes `(ox,whom,uid)`, then twenty bytes `tx..been_there`.

## Capacity comes from the world, not a type table

`Build::update_max_gatherers` `0x00623310` stores the low byte from
`BuildTypeData::max_gatherers` `0x0063C430`. The general path invokes the 7,668-byte
`BuildTypeData::calc_gather` `0x00639E40` over real fine cells, the ordered `MiningList`,
access/ownership/diplomacy, mountain/cliff objects, and player modifiers, then clamps a
negative result to zero. Consequently there is no supported Farm/Camp/Mine capacity table,
no radius-area estimate, and no `gather_from.len()` shortcut.

Farm TypeIndex 417 is the exact flat arm and returns 1 without terrain discovery. University
returns 7. The admitted Woodcutter and Mine arms are executable too:

- Woodcutter scans the 21-entry retail ring for radius 8, requires each contributing coarse
  cell's centre in its live `MiningList`, reads the four ordered `LandData.make/num_make`
  entries, adds `16*num_make` for every timber entry, applies footprint river coverage,
  rounds with `(raw16+8)/16`, applies shipped player/wonder gates, then caps the result at
  twice `total_gather_access`.
- Mine uses the retained Mountain/Cliff object id. Mountain capacity chooses base
  3/5/6/8/10 by shipped 100/210/275/400 size thresholds, then scales by eligible
  whitelisted solid cells over all non-forest solid mountain cells. Cliff uses the same
  bands against its ordered point count. Both apply exact territory/diplomacy and list
  membership gates. A zero denominator or missing terrain-object data fails closed.

Neither evaluator draws RNG. Wood capacity requires real `LandData`; forest tile count is
not a substitute. Mine requires Mountain/Cliff identity and ordered coordinates; a
connected-component reconstruction is not a substitute.

Capacity refresh and attachment are separate. The caller stores the evaluator result with
`GatherSite::set_authoritative_capacity`; `attach_worker` reads that persistent value, as
retail does. The building-center terrain update byte drives two independent operations:
bit `0x20` calls `Build::verify_gather_tiles`, while bit `0x10` calls
`Build::find_gather_tiles`. Verification can therefore remove cells without recomputing
`gather_max`; the find path reserves/disorders the list, computes capacity, and stores `AL`.

## Terrain claims and non-depletion

`Build::find_gather_tiles` `0x00623350` incrementally appends newly eligible cells to the
ordered `MiningList`; it does not clear and rebuild it. Existing coordinates retain TData
bit `0x1000`, so the authoritative terrain evaluator rejects them as already reserved.
After discovery, the function sets `0x1000` on the entire list.

Wood discovery uses the retail global ring tables generated by `0x006817F0`: shells use
ascending `dx` outer / `dy` inner order and retain a pair only when retail
`vector_dist(dx,dy)` equals the shell. Radius 8 uses cumulative shell 2, exactly 21 entries.
Each in-bounds, precise-distance, usable-territory W cell needs an unreserved centre; its
sixteen fine tiles are traversed x-fast and only tree-surface/unreserved tiles append.

Mine discovery never uses that ring. It searches the building region for the nearest
Mountain and Cliff objects, chooses the smaller inclusive distance (Mountain wins a tie),
rejects beyond `6*192`, stores the selected signed-byte id, and walks that object's stable
coordinate array. Forest-surface, hostile-territory, and reserved points are rejected.

The next step is easily mistranscribed. Retail does **not** append weighted duplicates.
When `old_len < new_len`, it performs exactly `4 * new_len` move-to-back operations:

```text
index = Random::get(0, 0xffff) % new_len   // no draw at all when new_len == 1
coord = list[index]
remove_first_by_value(coord)
append(coord)
```

Length remains constant. Lists longer than one consume exactly `4N` draws from the main
simulation RNG; one-cell lists execute four rotations with zero draws; an unchanged list
does neither. This is not Fisher-Yates, but its order is checksum-visible and later affects
strict tie-breaking and the `index >> 2` term in worker selection.

`verify_gather_tiles` clears `0x1000` when an entry loses territorial validity and removes
that coordinate first-match without shrinking capacity. It does not test whether the
resource terrain itself changed, reset `mtn`/`cliff`, or update `gather_max`.
`WorldData::is_gathered_from` and `World::set_gathered_at` expose the reservation bit; it is
checksummed as part of the world TData plane.

There is no remaining-yield pool for normal building gathering. `Unit::do_non_flat_gather`
rotates a selected coordinate to the back of the list; it does not debit it. `World::gather_at`
is a pure six-slot terrain-value reader. Rare/merchant Good-object targeting is a separate,
still-open spatial lane; no invented `GoodNode::remaining` field is retained.

`Build::close`, not the misleadingly named `Build::clear_gather`, owns terrain teardown.
For a non-flat gather building it clears `0x1000` on every listed cell, marks the center
update record with `0x20`, writes `mtn=cliff=-1`, and sets list length to zero. Allocation
and capacity survive; `gather_max` is not zeroed. `clear_gather` instead deletes the trained
unit `GatherPointList` at `+0xB8` and never touches `MiningList +0x98`.

## Persistent non-flat choreography

`GatherOrder`'s checksum-visible suffix is the exact twenty-byte sequence
`tx, ty, build_type, wait, goto_build, non_flat_gather, dist_mod, been_there`. In
`Unit::do_non_flat_gather` `0x005F0170`, `goto_build` is the phase latch:

- every call writes `UnitData::group +0x80 = -1`;
- when `been_there != 0`, the first call against a target whose build mask lacks `0x800`
  increments `BuildData::recharging` and sets that latch;
- `goto_build == 0` clears unit-mask bits `0x78000000`; if `been_there` was zero, it sets
  it and marks the leader economy dirty;
- a missing/inaccessible tile causes selection from the ordered `MiningList`; candidates
  must pass resource bit `0x4000` and `has_gather_access(tile, owner, 1, 0)`;
- candidate score is
  `max(vector_dist(candidate, building-center), 3) * dist_mod + (index >> 2)`, with strict
  lower-than replacement, then the selected value is moved to the list tail;
- selection consumes one main-RNG draw and sets `wait = draw % 200 + 400`, then writes
  `goto_build=0`. Property `0x1A2` sets unit mask `0x10000000`; without it retail sets
  `0x40000000` and replaces the wait with `1,000,000`;
- a failed nearby-spot search writes `tx=ty=wait=-1`, writes `goto_build=1`, decrements
  nonzero `dist_mod`, and removes a held doober for worker-gated units. It does not rewrite
  `been_there`.

The two observed wait loops draw only when decrement reaches exactly zero and
`Build::all_gathering` is false. Animation `0x19` reschedules to `draw % 100 + 300`; the
within-`0x140` tile loop uses `draw % 50 + 100`. A true `all_gathering` result sets
`wait=-1` without a draw.

Gather retirement marks the leader economy dirty and resolves the target. Unlike outer
order execution, this epilogue does not compare `GatherOrder::uid`: it detaches when the
target is still live and raw `(whom,ox)`/owner identity matches. A guarded live-game arm can
substitute the unit's `inside_down` object identity before detaching. It then clears
`0x78000000`, removes the held doober for types `0x32..=0x35`, and only afterward removes
the order. A vanished target cannot be detached immediately; clearing the order makes the
chain entry stale and `Build::check_gatherers` prunes it later.

The final zero in `has_gather_access(tile, owner, 1, 0)` selects the mode-0 arm, not the
harvestable-centre helper elsewhere in `map_terrain::World`. The centre must carry TData
`GATHER_EDGE 0x8000`; flag 1 bypasses centre ownership, so each N/E/S/W neighbour instead
requires usable territory, non-water surface, and no `BLOCKED 0x4000`. Wood capacity calls
the arm with flag 0: centre territory is checked once and neighbour territory is not.
`AuthoritativeGatherTerrain::is_allied` is fallible through `Option`; missing diplomacy is
a hard error rather than an enemy default.

## Checksum-critical representation

`MiningList`'s constructor uses length/capacity zero, increment `-1`, flags zero, and
`mtn=cliff=-1`. Its first append grows capacity to four, then capacity doubles. Under the
building `must_walk` gate, `BuildData::walk_data` includes `gather_max` in `[+0x70,+0x86)`,
then later emits:

```text
mtn:i8, cliff:i8
length:i32
if length != 0:
    capacity:i32
    increment:i16
    flags & ~0x40:u8
    ordered TCoordData[length]     // tx:i32, ty:i32
```

The pointer, current index, and unused allocation are excluded. Consequently random
disordering, worker move-to-back, and capacity-growth history are all lockstep-visible.

## Rates, caps, and credit

Shipped rules values are `GATHER_RATE=450`, `PEASANT_RATE=2560`, and `OIL_RATE=8960`.
The latter two are 8.8 fixed point. One occupied ordinary slot therefore contributes
`16*2560/256 = 160` gross units; oil contributes 560. The six scholar values are stored in
8.8 as `[1280,1792,2560,3840,5120,6400]`, producing `[80,112,160,240,320,400]`
knowledge gross per scholar, with at most seven active scholars.

`Leader::do_gather` divides by `GATHER_RATE << 4 = 7200` every frame and carries the
remainder. Thus gross unit 16 becomes one stockpile resource over 450 frames: ordinary
workers yield 10, oil workers 35, and scholars 5/7/10/15/20/25 per period before other
bonuses/caps. Direct `UnitData::calc_gather` uses a different crowding tail: it divides every
resource slot independently by `competitors + 1` with C truncation.

## Shared adapter contract

Consumers such as the arena keep `GatherSite` keyed by `(owner, build_o)` and
`GatherWorker` keyed by `(owner, unit_o)`, plus a `GatherMiningList` whose engine capacity
header survives clear/remove operations. The stable sequence is:

1. retain exact `LandData` and Mountain/Cliff identity/order in the map host;
2. call `discover_gather_terrain` for Wood/Mine; Farm skips discovery;
3. evaluate `woodcutter_gather_capacity` or `mine_gather_capacity` against that list;
4. call `finish_gather_tile_refresh_with_capacity`, passing pre-discovery length, main RNG,
   and computed capacity. Discovery/capacity consume no RNG; this post-growth step owns its
   exact 4N draws;
5. validate the generational target and call `attach_worker`; ordinary Farm, Woodcutter,
   and Mine workers remain on-map—inside-container mutation is not attachment;
6. perform nearby movement only through `GatherNearbySpotRequest`. Building approach uses
   `min_r=min(x_size,y_size)*0x60+0x30,max_r=-1,step=0`; terrain approach uses
   `min_r=0xC0,max_r=0x100,step=2`. Both retain filter 3 and tail
   `[0,0,-1,0,-1]`. A found point is a queued move destination, never a teleport;
7. advance non-flat persistent phases through `begin_non_flat_gather_tick`,
   `prepare_non_flat_tile`, the wait primitive, and explicit host path/animation callbacks;
8. call `check_gatherers`, `detach_worker`, or `retire_gather_order` at their retail order
   boundaries;
9. count `Active` workers and pass a fully evaluated per-worker six-slot result to
   `site_gross`;
10. feed the gross through `credit_gather_frame`.

`find_nearby_spot` returns 0 on success and performs no RNG draw. Its host must retain the
worker UnitType spatial/domain fields, inclusive radius stepping, phase order
`0,+1,-1,...,+15,-15`, the retail angle quirk, UCoord-centre snap, bounds/region/terrain,
and both ordinary and ordered collision queries. Missing any of those inputs is a hard
failure, not permission to choose the nearest apparently free Arena tile. Scholar
`go_inside`/`come_out(0)` remains a separate full world-removal and identity-chain
lifecycle; the explicit mode is retained at the host boundary.

Farm is on-map too, but its movement has separate exact RNG boundaries. After a one-in-256
deterministic low-byte gate, the initial move calls `rnd(x_size/2)` then `rnd(y_size/2)` and
targets `(corner+1+offset)`; each call draws only when its argument exceeds one. General
footprint relocation calls `rnd(x_size)` then `rnd(y_size)`. Both queue a move.
`initial_farm_gather_move` and `farm_gather_relocation` expose their direct draw count so an
Arena adapter cannot accidentally merge these draws into terrain refresh.

Unsupported terrain/resource evaluation stays explicit at this seam. Supplying a guessed
capacity or a synthetic depletion counter would create plausible but retail-incompatible
state and is not a fidelity mode.
