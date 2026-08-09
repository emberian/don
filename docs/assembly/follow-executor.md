# FOLLOW executor transaction

Lane `orders-green/follow-executor`, 2026-08-09. Tier C, shared integration landed.

`crates/don-sim/src/systems/follow_executor.rs` recovers `Unit::do_follow` `0x005E65D0`
(1,455 bytes) as a standalone, receipt-recomputable planner. It is deliberately separate
from the group action installer: `Group::action_follow` creates the order, while this body
executes it across simulation frames.

## Concrete order state

The shipped PDB gives `FollowOrder` size 44 bytes. Its complete executor-visible payload is:

| offset | field | source |
|---:|---|---|
| `+0x08` | primary `ox:i32` | `TargetOrder` |
| `+0x0C` | primary `whom:i32` | `TargetOrder` |
| `+0x10` | primary `uid:u16` | `TargetOrder` |
| `+0x14` | fallback `oxx:i32` | `FollowOrder` |
| `+0x18` | fallback `whose:i32` | `FollowOrder` |
| `+0x1C` | fallback `uid2:u16` | `FollowOrder` |

The entry call through the order's vtable `+0x78` is the PDB-named
`FollowOrder::update_follow_order()`.

## Instruction-ordered state machine

1. Resolve primary `P` when `P.o >= 0`.
2. If `P` is not a valid on-map unit, retail may climb containment: only active objects with
   `inside_up >= 0` call `ObjectData::get_inside` `0x00651A80`. A valid on-map outer root `C`
   moves old `P` into fallback and replaces primary with `C`, including its live UID.
3. If primary is still invalid and fallback names a different slot, copy fallback to primary
   and call the actor's full virtual `work()` at vtable `+0x188`. If the slots are equal,
   call bare `kill_current_order(0)` instead.
4. With valid primary and a distinct fallback, compare fallback's live UID with `uid2`. A
   match replaces `oxx` with fallback virtual `get_captain()` and refreshes `uid2` from that
   captain slot. A mismatch collapses fallback to current primary.
5. Primary virtual `is_seen(actor.who,0)` false kills the order. It does not promote fallback.
6. Distance is retail `vector_dist`. Let `L=actor.los()`. Margin is `L*0x60` when actor
   speed is greater than primary speed, otherwise signed-truncating `(L*0x300)/5`; moving
   primary doubles the margin. Radius and cutoff are exactly
   `r=min(0x600,max(0x180,L*0x180-margin))` and `cutoff=r+0xC0`.
7. `distance <= cutoff` calls `set_anim(0,0,1)` and retains FOLLOW.
8. A farther target performs up to three `UnitType::find_nearby_spot` calls, stopping on the
   first zero return:
   - project radius `r` from primary toward the actor; request tuple
     `(0,-1,0,0x55555555,3,actor.o,actor.who,0,0,-1,0,-1)`;
   - project radius `r` behind primary's angle; the same tuple;
   - primary centre with `(r,cutoff,0x30,primary.angle,3,actor.o,actor.who,0,0,-1,0,-1)`.
     If all fail, use primary's raw coordinates.
9. Each chosen coordinate becomes `div_3_table[coord >> 4]`, i.e. mathematical
   `floor((coord >> 4)/3)`. Retail calls
   `add_move_facing_order(x,y,primary.angle,1,0,QUEUE_FIRST,0,-1,-1,-1,0)`, then immediately
   `do_move(update_order())` in the same activation.

PDB and call anchors: `vector_dist` `0x0046CFF0`, `UnitData::speed` `0x0060AAE0`,
`Unit::set_anim` `0x00616F40`, `UnitType::find_nearby_spot` `0x0061DE70`, `project`
`0x0092CF40`, `find_angle` `0x0092D130`, `Unit::add_move_facing_order` `0x005E55C0`,
`Unit::update_order` `0x006179D0`, `Unit::do_move` `0x005F7B30`, and
`Unit::kill_current_order` `0x005E2CB0`. The object virtuals are PDB-identified:
`+0x08 is_valid_unit`, `+0x48 is_seen`, `+0xBC is_on_map`, `+0xD8 is_moving`, and
`+0xE4 get_captain`; actor `+0x128` is `los`.

## Mutation evidence

`crates/don-sim/tests/follow_executor_planner.rs` path-imports the subject and pins the two
invalid-target tails, containment promotion, fallback UID/captain maintenance, unseen kill,
radius arithmetic, all three search shapes, negative floor conversion, same-tick movement,
search cardinality, and whole-receipt recomputation. The shared dispatcher test additionally
pins zero mutation for an unavailable host and the applied idle-animation effect.

No compiler, test, formatter, or remote job was run in this lane.

## Landed shared integration map and coverage delta

The executor promotion landed as one narrow but indivisible shared tranche:

1. add secondary `(oxx,whose,uid2)` storage to `OrderRec` through a concrete FOLLOW payload;
2. expose a dedicated `follow_preflight(actor,order) -> FollowExecutorReceipt` on `WorkWorld`;
3. make an applied receipt guarantee the containment, fallback/captain, speed/motion/angle,
   LOS, and ordered nearby-search observations plus every emitted effect capability;
4. apply the returned `FollowOrderState` before its effects, preserve full `work()` reentry,
   and make move installation immediately invoke existing `do_move` in the same tick;
5. dispatch `OrderIndex::Follow` to that adapter; and
6. serialize both full-width identities and UID snapshots in DoNSave format 5; and
7. promote `order_dispatch::ARMS[11]` from `Unimplemented` to `Implemented` and mirror the
   executor ledger status in `order.rs`.

FOLLOW's exact contribution changes handled order executors from **18/28 to 19/28**. Other
concurrent executor promotions may make the combined dirty-tree aggregate higher; they are
not part of this lane. The group-action tranche separately changes the current action ledger
from **8 complete / 15 orders-partial** to **9 complete / 14 orders-partial**.
