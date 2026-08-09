# Group recall action frontier

Status: **source-only maximal prefix; opcode 35 and `Group::action_recall` remain red**.

This pack freezes the complete non-delegating body of `Group::action_recall` and an atomic host
receipt.  It intentionally does not register the planner in the library module graph or edit the
command dispatcher.  A selected air-domain leader reaches the separate
`Group::action_return` receiver; that real action remains a typed tail, so this work makes no
`Complete`, `Orders`, or runtime-equivalence claim.

The isolated source is
`crates/don-sim/src/systems/recall_action_frontier.rs`; its path-import mutation tests are
`crates/don-sim/tests/recall_action_frontier.rs`.

## Authority and extent

Claims are Tier C **[measured]** from `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and the matching
`ron-bin/sbl/rise.pdb`.  Names, sizes, inheritance-adjusted fields, and virtual slots were
cross-checked against `schema/symbols.json`, `schema/types.json`, and direct PE32 disassembly.

| symbol | VA | bytes | role |
|---|---:|---:|---|
| `Group::action_recall` | `0x006FA7E0` | 1,373 | recovered receiver |
| `Group::action_return` | `0x006FAD40` | 1,307 | selected-aircraft open tail |
| `GroupData::find_leader` | `0x0070CCB0` | 313 | non-building leader selection |
| `GroupData::member` | `0x0070F8F0` | 112 | active object plus owner/list admission |
| `Build::clear_gather` | `0x00623180` | 390 | selected-base child transaction |
| `ObjectData::get_inside` | `0x00651A80` | 244 | contained-aircraft lookup |
| `Unit::clear_orders` | `0x005E3860` | 42 | contained-aircraft order clear |
| `Unit::close_orders` | `0x005E37F0` | 100 | returning-aircraft queue reset |
| `Unit::clear_partial_path` | `0x005E3920` | 674 | returning-aircraft path reset |
| `Unit::update_action` | `0x0060A870` | 485 | action refresh |
| `Unit::add_strafe_order` | `0x005E48C0` | 405 | replacement return-to-home order |

Opcode 35 itself is exactly one byte.  The already-recovered handler at `0x00947BD0` first
requires signed `CommandPackage+0x0C >= 0`, then calls `action_recall()` at `0x00947CA4`.
This planner begins at that receiver; `decode_recall` pins the one-byte wire identity without
changing the shared dispatcher.

## Exact receiver order

The body has three top-level arms.

1. At `0x006FA7FB`, scenario `ignore_orders` walks the owner's global selection array.  Every
   nonnegative entry invokes the group `kill(o, who, 0, 0)` virtual.  The receipt binds both the
   walked selection and the resulting complete `GroupData` after-image.  If the post-prelude
   group has `num <= 0`, retail returns before `action_begin`.
2. For a nonempty group, `buildings != 0` chooses sign-extended `list[0]`; otherwise
   `find_leader(nullptr)` chooses the leader.  If that object type has
   `ObjectTypeData::domain(+0x218) == AIR (2)`, `action_recall` calls
   `Group::action_return()` at `0x006FA8BC` and returns immediately.  It has not yet called
   `action_begin`, cleared gather state, or reset formation.
3. Every other leader result enters the main recall arm.  Retail calls `action_begin`
   (`disband = 0`), scans the selected group, resets `form = -1`, and then scans the complete
   owner unit array for aircraft associated with the selected airbase/carrier members.

The group-member scan at `0x006FA8F0` is not a generic building test.  For each exact
`GroupData::list` entry it first calls virtual `+0x0C` (`is_valid_wall`), then virtual `+0x20`
(`is_build`), then `get_build` and `Build::clear_gather`.  `form = -1` is stored only after the
whole clear-gather pass, at `0x006FA987`.

## Owner aircraft scan

The loop at `0x006FA9A0` walks every slot in the owner's unit pointer array; the pointer-array
index is also the integer stored in `ObjectData::launching`.  Admission is instruction ordered:

1. object virtual `+0x08` (`is_valid_unit`) must succeed;
2. `UnitData::is_plane` must succeed;
3. type `obj_masks(+0x1E4) & 0x08000000` must be clear;
4. type `unit_flags(+0x2B4) & 0x20` must be clear.

The shipped `UnitData::is_plane` fast body itself is
`domain == 2 && !(unit_flags & 0x20)`, but retail repeats the `0x20` gate after the virtual and
also excludes the missile object mask.  The planner preserves both post-predicate reads rather
than folding them into a vague `aircraft` boolean.

`ObjectData::get_inside(&who)` then splits the candidate:

- If the returned `(o,who)` passes `GroupData::member(o,who,1)`, retail clears the aircraft's
  orders.  `member(...,1)` requires matching owner, active object bit `+0x08&1`, and an exact
  selected-list hit.  If the inside object's `ObjectData::launching` pointer is non-null and
  contains the actor slot, retail removes that slot.  It does not create a new STRAFE order on
  this arm.
- Otherwise retail resolves the current AirOrder.  A leading `SPECIAL_ANIM (25)` is skipped by
  advancing the order-list node once; every other shape uses `Unit::update_order()`.  The
  resulting `get_air_order()` pointer supplies `oxx`, `whose`, `cruising_alt`, and `sharp_turn`.
  The first pair must itself pass `GroupData::member(...,1)` before any mutation occurs.

For a selected home pair, the exact effect sequence is:

1. write old `AirOrder::returning = 1`;
2. preserve old `cruising_alt` and `sharp_turn`;
3. clear instance `UnitData::unit_masks & 0x04000000`;
4. write `UnitData::path.length = 0` at concrete `Unit+0xC0`;
5. `close_orders(0)`;
6. `clear_partial_path()`;
7. `update_action()`;
8. `add_strafe_order(-1, -1, home_o, home_who, 0, QUEUE_NEW, 0)`;
9. `update_order()->get_air_order()`;
10. restore `cruising_alt`, then `sharp_turn`, on the new order.

The temporary `StrafeOrder` constructor/destructor around that sequence has no additional
checksum-visible publication.  Neither `action_recall` nor these direct children call the
canonical RNG; the plan pins `direct_rng_draws = 0`.

## Atomic receipt and open tail

`RecallReceipt::Applied` is valid only when all facts recompute the exact same group after-image,
effect vector, and closed boundary.  The effect vector includes scenario kills, build gather
clears, launching-list removal, order/path/mask mutations, the exact seven-argument STRAFE call,
and both restored AirOrder fields.  `Unavailable` carries no facts or plan and authorizes no
mutation.

An `OpenActionReturn` plan can be observed but cannot validate as applied.  This matters because
the scenario `ignore_orders` calls occur before retail discovers the air leader: publishing those
kills while omitting `action_return` would be a partial transaction.  A future product adapter
must therefore preflight the return receiver and the recall prefix together or leave all state
unchanged.

Direct disassembly of `action_return` confirms that it is not a leaf alias: it calls
`action_begin`, repeats the scenario prelude, scans selected members, clears and rebuilds STRAFE
orders across contained and airborne cases, and preserves multiple AirOrder fields.  Its own
queue/launching children make a separate atomic recovery the honest next frontier.  This pack
does not summarize that 1,307-byte state machine as a boolean success.

## Honest closure delta and validation boundary

The source is absent from `systems/mod.rs`; the test crate reaches it only with a local `#[path]`
declaration.  Opcode 35 remains `unimplemented`, `Group::action_recall` remains `Port::Todo`, and
generated closure evidence is unchanged.  The honest closure delta is **zero**.

Root convergence formatted the isolated Rust files and validated all 10 focused tests in both
modes:

- hbox debug: `recall-action-20260809T225231Z-90354-14840-cb8b2b915d8e`;
- persvati release: `recall-action-release-20260809T225233Z-90374-13365-cb8b2b915d8e`.

Neither job ran retail. Opcode 35 and `Group::action_recall` remain red exactly as above.
