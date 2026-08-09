# Unimplemented group-command dispatcher frontier

This source-only proof pack freezes the largest bounded opcode cohort which is still red in
the checked-in closure: opcodes **5, 6, 23, 24, 25, 28, and 35**.  It adds an unexported
planner and path-import tests, but changes neither the shared dispatcher/table nor generated
closure evidence.

Claims below are Tier C **[measured]** against `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, image base
`0x00400000`) and named/layouted with `ron-bin/sbl/rise.pdb`.  The command and action
inventories were cross-checked against `schema/command-wire.json`,
`crates/don-sim/src/command_tables.rs`, and `schema/simulation-closure.json`.  No retail
execution or equivalence claim is made.

The isolated source is
`crates/don-sim/src/systems/unimplemented_group_command_plans.rs`; its mutation pins are
`crates/don-sim/tests/unimplemented_group_command_plans.rs`.  The source is absent from the
library module graph in this tranche; the test crate reaches it only through its local
`#[path]` declaration.

## Why this is the maximal disjoint cohort

The checked-in `don-closure` invariant counts the 42 group actions as
`[Complete=9, Orders=14, State=0, StateWired=5, Todo=7, NotOnTheWire=7]`.  The seven rows
below are all seven `Port::Todo` actions, and the generated closure contains exactly seven
`bridge_status: "unimplemented"` opcode rows: the same one-to-one set.  There is no eighth
row satisfying this boundary.

This dispatcher-prefix cohort installs no order itself.  Its seven immediate actions do not
directly install `OrderIndex::Garrison` and do not directly delegate to
`Group::action_garrison`.  Their recovered first-hop table edges are:

- `siege_attack` → `attack`, `guard`;
- `swarm_around` → `halt`, `move_to`;
- `build` → `swarm_around`;
- `flight` → `attack`, `guard`, `launch_flight`;
- `recall` → `return`;
- `spell` and `queue_up` have no group-action delegate.

The immediate actions' directly installed order set is `STRAFE`, `CAST_SPELL`, `MOVE_TO`, `ATTACK_TO`,
`EXPLORE_TO`, `FLEE_TO`, `BUILD_AT`, and `REPAIR`; it excludes `GARRISON`.  The pack does
not touch opcode 20, `action_garrison`, `add_garrison_order`, or the active GARRISON executor
work.  Some open tails can later traverse `move_to` → generalized `move_near`, whose table
includes a GARRISON selector.  This pack stops before every action body and therefore neither
implements nor modifies that shared tail; a future `swarm_around`/`build`/`attack` adapter
must consume the settled GARRISON-lane contract rather than reimplement it.

## Dispatcher audit

Every handler returns the PDB command size even when its guarded action call is skipped.
Every field below is a signed little-endian dword except the opcode byte; enum-typed fields
remain raw signed dwords until an action adapter validates their domain.

| op | handler VA | bytes and wire fields | reached action call |
|---:|---:|---|---|
| 5 | `process_siege_attack` `0x00949AE0` | 13: `ox@1, whom@5, queued@9` | `action_siege_attack(ox, whom, queued)` at `0x00949C11` |
| 6 | `process_swarm_around` `0x00949970` | 17: `ox@1, whom@5, queued@9, orders@13` | `action_swarm_around(ox, whom, queued, orders, 1)` at `0x00949AC2` |
| 23 | `process_spell` `0x00948340` | 21: `ox@1, whom@5, type@9, x@13, y@17` | `action_spell(type, ox, whom, x, y)` at `0x009484F2` |
| 24 | `process_queue_up` `0x00948230` | 9: `type@1, num@5` | `action_queue_up(type, num)` at `0x00948329` |
| 25 | `process_build` `0x00948110` | 25: `x@1, y@5, x2@9, y2@13, type@17, queued@21` | `action_build(x, y, x2, y2, type, queued)` at `0x0094821C` |
| 28 | `process_flight` `0x00947DB0` | 25: `ox@1, whom@5, shift@9, ctrl@13, alt@17, orders@21` | `action_flight(ox, whom, orders, shift, ctrl, alt)` at `0x00947ED4` |
| 35 | `process_recall` `0x00947BD0` | 1: no payload | `action_recall()` at `0x00947CA4` |

Three argument details are easy to lose in a generic wire decoder.  Opcode 6 supplies a
literal trailing `1`; opcode 23 moves `type` ahead of the target pair; opcode 28 moves
`orders` ahead of the three modifier fields.  The typed `GroupActionCall` enum pins these
PDB-call orders independently of wire order.

All seven first test signed `CommandPackage+0x0C` and skip the action when it is negative.
Opcodes 5, 6, and 23 additionally treat negative `ox` or `whom` as a sentinel which bypasses
object validation.  When both are nonnegative they resolve `objects[whom][ox]` and call the
action only if byte `ObjectData+0x08` has bit `1` set.  The planner represents that host read
as branch-sensitive `Option<bool>`: it is unnecessary on sentinel/non-target paths and
missing reached evidence fails closed.

The handlers also execute the common command trace/log prelude.  That runtime-owned
diagnostic is deliberately outside this simulation planner.  The recovered claim ends at
selection of the exact action call; it does not claim the action body or the diagnostic
sink.

## Still-open action tails

Every reached plan contains `DelegatedGroupAction` and sets `downstream_required=true`.
Those delegates are intentionally not executable here:

| action | retail VA / size | open state-writing boundary |
|---|---:|---|
| `siege_attack` | `0x00706FF0` / 549 | attack-vs-guard decision and complete downstream transactions |
| `swarm_around` | `0x0070FBE0` / 3044 | insert lifecycle, movement/build/repair/spell order selection and installs |
| `spell` | `0x006FE1A0` / 4100 | CastSpell eligibility, insert lifecycle, and order payload |
| `queue_up` | `0x006FDBB0` / 1516 | producer filtering and `Build::queue_up` mutations |
| `build` | `0x00707510` / 1256 | placement/group preparation and `swarm_around` transaction |
| `flight` | `0x006FB260` / 3398 | aircraft filtering, Strafe setup, and attack/guard/launch branches |
| `recall` | `0x006FA7E0` / 1373 | Strafe setup and `return` transaction |

A recomputable `PlanStatus::Planned` receipt is planner evidence only.  It must never be
treated as an `Applied` action receipt.

## Frozen later integration map

Integration must land atomically per row in the following order.  The symbol names below
are reserved by this pack so parallel action-recovery lanes converge on one seam.

1. In `crates/don-sim/src/command.rs`, add the same private `#[path =
   "systems/unimplemented_group_command_plans.rs"]` declaration used by the other command
   proof modules and import the request/facts/delegate types.  Do not also export it from
   `systems/mod.rs`.
2. Extend `Fleet` with a fail-closed `object_flag_1(whom, ox) -> Option<bool>` read and
   one atomic
   `apply_unimplemented_group_action_transaction(DelegatedGroupAction)` boundary.  The
   latter needs a new `Applied/Unavailable` action receipt; the planner receipt in this pack
   is not that type.  `ObjectTable` and every product host must implement both seams without
   defaulting a missing positive target to either bit value; failed resolution is `None`.
3. In `Bridge::process_one`, replace the unconditional `Port::Todo` early return only for
   `[5, 6, 23, 24, 25, 28, 35]`: decode the exact command slice, snapshot the signed package
   group and the conditional object flag, recompute the plan, and either accept its exact
   no-delegate arm or submit its delegate to the atomic host transaction.  A no-delegate
   arm does not increment action counters.  On a delegate, increment `by_action` before the
   host call; an unavailable/invalid receipt increments `unported`, while a validated
   `Applied` receipt increments `acted` plus only the order/effect counts it actually proves.
4. Recover adapters named `apply_siege_attack_transaction`,
   `apply_swarm_around_transaction`, `apply_spell_transaction`,
   `apply_queue_up_transaction`, `apply_build_transaction`,
   `apply_flight_transaction`, and `apply_recall_transaction`.  A partial adapter must
   recover a real state/queue-writing action prefix, preflight and commit every effect in
   that prefix as one unit, and retain the unrecovered capability/state/action suffix as a
   typed open tail.  Merely forwarding this pack's delegate is not a state-wired adapter.
   An unavailable adapter leaves all group/object/order state unchanged.
5. Add dispatcher-level mutation tests for each no-group, sentinel-target,
   invalid-positive-target, unavailable-tail, and applied-tail path.  Keep the path-import
   tests in this pack as the ABI pins.
6. Only after a row has a validated atomic state-writing prefix with an explicit residual
   tail may that row change from `Port::Todo` to `Port::StateWired`.  If an adapter instead
   reproduces the complete action, classify it `Port::Complete` or `Port::Orders` according
   to the existing `Port` contract.  Then run the `don-closure` generator and review the
   matching `schema/simulation-closure.json` row.  Do not promote a decode-only row or
   batch-promote an unavailable sibling.

The row-to-adapter mapping is exact:

| opcode | table action | reserved adapter |
|---:|---|---|
| 5 | `siege_attack` | `apply_siege_attack_transaction` |
| 6 | `swarm_around` | `apply_swarm_around_transaction` |
| 23 | `spell` | `apply_spell_transaction` |
| 24 | `queue_up` | `apply_queue_up_transaction` |
| 25 | `build` | `apply_build_transaction` |
| 28 | `flight` | `apply_flight_transaction` |
| 35 | `recall` | `apply_recall_transaction` |

## Isolated validation

All eight wire/gate/receipt mutation tests passed in both debug and optimized profiles on
2026-08-09:

- hbox `opcode-todo-prefix-20260809T212310Z-7970-25367-31f5fd0c16b3`;
- persvati `opcode-todo-prefix-release-20260809T212310Z-7971-13803-31f5fd0c16b3`.

These jobs validate the unexported planner only and do not change the zero closure delta.

## Honest closure delta

This tranche has **zero library/runtime-module-graph or generated closure delta**.  The
source is compiled only by its path-import test crate; all seven table rows remain
`Port::Todo`, all seven opcode rows remain `unimplemented`, and the checked-in group-action
count remains `[9, 14, 0, 5, 7, 7]`.

Merely exporting or decoding this planner would still have zero honest closure delta.  If
all seven later land specifically as prefix-only `StateWired` adapters, that scenario's
bounded delta is `Todo -7, StateWired +7`, producing `[9, 14, 0, 12, 0, 7]`; `Complete`
remains unchanged.  A fully recovered row must instead use its stronger `Complete`/`Orders`
classification, so that distribution is not promised.  Both are future outcomes, not a
result of this pack.  Full-green claims remain blocked on the seven action bodies,
queue/checksum effects, product-host receipts, and generated evidence review.
