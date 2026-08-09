# FOLLOW group-action transaction

Lane `orders-green/follow`, 2026-08-09. Tier C.

`crates/don-sim/src/systems/follow_action.rs` is the standalone, atomic recovery of
`Group::action_follow` `0x006FD510` (645 bytes). It supersedes the preliminary FOLLOW
prefix in `group_action_frontier.rs`; the latter is not the integration contract.

## Exact branch order

The retail body performs these operations in order:

1. `Group::action_begin` clears `GroupData::disband`.
2. Building selections and either negative target identity return immediately.
3. The scenario `ignore_orders` prelude calls the group's virtual `kill` for each owned
   scenario object. The product host supplies the resulting group snapshot atomically.
4. Raw queue value zero performs `set_up_insert`, `action_halt(0)`, recursively issues the
   action with literal queue value two, and then calls `finish_insert`.
5. The direct/recursive arm stores `GroupData::form = -1` before it asks whether the target
   is a valid on-map non-plane unit. The store therefore survives a missing or rejected
   target.
6. Members are visited in group-list order. Only valid, on-map, non-plane units receive
   `Unit::add_follow_order`.
7. The self-target test compares the member index with the target's virtual
   `ObjectData::get_o` result and compares owners. The installed order still receives the
   raw `(target_o, target_who)` command pair.

The disassembly anchors are `0x006FD52E` (begin), `0x006FD533` (building gate),
`0x006FD5F2..0x006FD617` (queue-first transaction), `0x006FD64C` (form store),
`0x006FD66A..0x006FD6A4` (target gates), `0x006FD6DF..0x006FD720` (member gates), and
`0x006FD734..0x006FD76A` (canonical self-test and install).

## Why the host boundary is atomic

`add_follow_order` owns order allocation, both target UID snapshots, containment-root
identity, queue-new order/path retirement, and `update_action`. The queue-first wrapper also
calls the already-recovered complete HALT transaction and reissues the saved order list via
group actions. A host must preflight and commit every emitted `FollowEffect` or return
`Unavailable` with no mutation. There is deliberately no `can_install => skip member`
fallback.

Unknown nonzero queue dwords are retained. Retail only branches specially on zero at the
group layer and two inside `add_follow_order`; converting an unknown value to queue-first
would manufacture a halt/replay transaction absent from retail.

## Mutation evidence

`crates/don-sim/tests/follow_action_planner.rs` path-imports the subject without changing
the shared systems module map. It pins:

- the no-fact building and signed-target returns;
- the target-rejected `form = -1` after-image;
- queue-first effect ordering and literal queue-new recursion;
- canonical self-target rejection while preserving the raw install identity;
- per-member valid/on-map/non-plane filters;
- pass-through of unknown nonzero queue values;
- immutable failure behavior; and
- full receipt recomputation against spliced effects.

No local compiler, formatter, or test command was run in this authoring lane.

## Landed integration map

The narrow shared integration is now live:

1. path-declare `follow_action` from `command.rs` and import its request/receipt/effect types;
2. change `Fleet::apply_follow_transaction` to the dedicated types;
3. route opcode 30 directly to `Action::action_follow`, preserving its raw queue dword;
4. validate an applied receipt before committing its `FollowPlan::group` after-image and
   accounting `AddFollowOrder` effects;
5. remove FOLLOW from the generic `action_target` call sites so its self-target rule no
   longer leaks into BOARD_SHIP, TRADE, REPAIR, GUARD, GARRISON, or GATHER; and
6. promote only the `follow` action-table row from `Port::Orders` to `Port::Complete`.

With STOP_SPELL already complete in the committed baseline, this promotion changes group
actions from **8 complete / 15 orders-partial** to **9 complete / 14 orders-partial**. The
separate executor transaction below also landed; FOLLOW contributes exactly one handled
order-executor row and no other action-table row earns a status change here.
