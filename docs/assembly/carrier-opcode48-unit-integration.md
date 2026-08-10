# Opcode 48 Carrier Unit integration

Status: **active Unit receiver complete; Build receiver explicitly open; row state-wired**.

`command::Bridge` already routes the exact 15-byte `UnqueueCommand` through
`Fleet::apply_direct_entity_command_transaction`. This tranche closes only the matching active
Unit arm by composing the recovered `Unit::action_unqueue(1)` transaction into that existing
receipt. It does not special-case packets in the dispatcher and does not relabel the separate
`Build::action_unqueue(type)` body.

## Canonical owner transaction

`production::runtime::process_sim_carrier_unqueue_command` resolves `(who, object_index)` through
the Sim `ObjectRegistry` Unit band and reads active/UID before any type fact. Inactive and stale
targets therefore retain the retail lazy complete no-op. A live matching target preflights all
reached state before the first write:

- exact Unit row, UID, and nonnegative installed target type;
- current Helicopter upgrade and its installed Unit/ObjectType projection;
- the selected `queued_counts[type]` value, fenced to retail's unsigned 16-bit width;
- the six named queued-family dwords in `LiveProductionLeader`;
- the authoritative `LeaderSlot::econ.stockpile` and its equal production-side mirror;
- `GatherInputs::type_avail[6]`, exact installed `Type::get_cost` results, and the shared
  no-costs game projection;
- the global decoded resource scratch.

The adapter materializes a `CarrierImplicitQueueState`, plans the isolated receiver, wraps that
proof with `complete_carrier_unit_unqueue_command`, and recomputes the entire command receipt.
Only then does it replace the Unit queue fields, selected aggregate, family counters, economy and
production stockpiles, step-8 economy view, and refund scratch. Missing cost/type/counter state or
an economy-mirror disagreement returns `Unavailable` with no mutation.

The wire `type_index` remains deliberately irrelevant to the Unit arm: retail forwards literal
argument `1` to `Unit::action_unqueue`. The Build arm still forwards the signed wire selector to
its distinct action body.

## Receipt and closure accounting

`DirectEntityCommandTransactionReceipt::unit_unqueue` retains the receiver's recomputable
before/facts/plan proof. `CompleteUnitActionUnqueue` is admitted only for the exact Unit open-tail
identity, argument `1`, matching owner, and a validating refund receiver. Mutation of the command
identity, receiver facts, after-image, or ordered step trace invalidates the Fleet receipt.

The focused dispatcher test is
`crates/don-sim/tests/command_carrier_unqueue_integration.rs`. It covers the real packet walker and
Fleet callback, exact aggregate/family/refund/scratch mutation, fail-closed atomicity on missing
refund facts, the empty-queue lazy boundary, no-cost mode, and the still-open active Build arm.

The honest closure delta is **zero complete rows**: opcode 48 remains `state_wired` because an
active Build can still reach `Build::action_unqueue(type)`. Its Unit branch is no longer part of
that residual. Opcode 49 is unchanged.

Root convergence formatted the owners. The first four-case job passed; a new Build case initially
sent `BuildData::object_id()==0` and correctly exercised Unit object zero instead. After the fixture
used the canonical Build registry mark, persvati job
`carrier-opcode48-v3-20260810T000152Z-22258-18012-cf4a3c9ea7c5` passed all five focused tests.
Retail was not run. The reproducible gates are:

```sh
cargo test -p don-sim --test carrier_implicit_unqueue_frontier
cargo test -p don-sim --test direct_entity_command_integration
cargo test -p don-sim --test command_direct_entity_dispatch
cargo test -p don-sim --test command_carrier_unqueue_integration
cargo run -p don-tools --bin don-closure -- --check schema/simulation-closure.json
```
