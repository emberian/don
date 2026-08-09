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
lengths.

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

Two helpers are exact: `max_flat_gatherers` returns 1, while
`max_knowledge_gatherers` returns 7. Mine and Woodcutter capacity is terrain/access derived.
Known Mine size bands begin at 3/5/6/8/10 and are reduced by eligible-accessible versus total
gather points. Woodcutter accumulation is in sixteenths, includes river coverage, rounds via
`(raw16 + 8) / 16`, and is capped by twice `total_gather_access`. These facts do not yet
constitute the complete authoritative evaluator, so the shared adapter accepts its computed
result and never infers one.

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
  must pass resource bit `0x4000` and `has_gather_access`;
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

1. append only cells returned by the authoritative retail terrain evaluator;
2. call `finish_gather_tile_refresh_with_capacity`, passing the pre-discovery length, main
   simulation RNG, and exact `max_gatherers` result;
3. validate the generational `GatherAssignment` target;
4. call `attach_worker`, `check_gatherers`, `detach_worker`, or `retire_gather_order`;
5. advance non-flat persistent phases through `begin_non_flat_gather_tick`,
   `prepare_non_flat_tile`, the wait primitive, and explicit host path/animation callbacks;
6. count `Active` workers and pass a fully evaluated per-worker six-slot result to
   `site_gross`;
7. feed the gross through `credit_gather_frame`.

Unsupported terrain/resource evaluation stays explicit at this seam. Supplying a guessed
capacity or a synthetic depletion counter would create plausible but retail-incompatible
state and is not a fidelity mode.
