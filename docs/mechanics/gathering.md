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
retail does. Terrain changes must refresh it before the next attachment decision.

## Terrain claims and non-depletion

`Build::find_gather_tiles` `0x00623350` obtains an ordered `MiningList`, marks every base
fine cell with TData bit `0x1000`, and appends deterministic weighted duplicates. The
duplicates affect selection weight, not physical capacity. `verify_gather_tiles` clears the
bit when an entry loses territorial validity. `WorldData::is_gathered_from` and
`World::set_gathered_at` expose that exact reservation bit; it is checksummed as part of the
world TData plane.

There is no remaining-yield pool for normal building gathering. `Unit::do_non_flat_gather`
rotates a selected coordinate to the back of the list; it does not debit it. `World::gather_at`
is a pure six-slot terrain-value reader. Rare/merchant Good-object targeting is a separate,
still-open spatial lane; no invented `GoodNode::remaining` field is retained.

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
`GatherWorker` keyed by `(owner, unit_o)`. The stable sequence is:

1. evaluate retail terrain/type capacity externally;
2. call `site.set_authoritative_capacity(result)`;
3. validate the generational `GatherAssignment` target;
4. call `attach_worker`, `check_gatherers`, or `detach_worker`;
5. count `Active` workers and pass a fully evaluated per-worker six-slot result to
   `site_gross`;
6. feed the gross through `credit_gather_frame`.

Unsupported terrain/resource evaluation stays explicit at this seam. Supplying a guessed
capacity or a synthetic depletion counter would create plausible but retail-incompatible
state and is not a fidelity mode.
