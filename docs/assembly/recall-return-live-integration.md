# RECALL / RETURN live command integration

Status: **opcode 35 and both action bodies execute through one atomic host boundary;
StateWired until the production world host supplies the transaction**.

The isolated `Group::action_recall` and `Group::action_return` reconstructions are now private
modules of the command bridge.  `RecallCommand` still crosses the exact recovered handler-prefix
plan, including the signed package-group gate.  A reached `Recall` delegate then issues one
`Fleet::apply_recall_action_transaction` callback instead of being counted unconditionally as an
open tail.

## One publication boundary

`RecallActionReceipt` binds the addressed `GroupData` and the host-owned snapshot of scenario
`ignore_orders`, scenario selection, object/type predicates, containment/launching lists, and
air-order state.  Validation recomputes the complete recall plan.  Its result determines the
rest of the transaction:

- `EmptyGroup` and `MainBody` reject any RETURN evidence;
- `OpenActionReturn` requires the measured tail marker, derives `ReturnRequest` from the exact
  recall after-image plus the same scenario snapshot, and recomputes the complete return plan;
- both plans must report zero direct canonical-RNG draws;
- an unavailable receipt contains no facts or plans and changes no Bridge state.

The Bridge publishes the final group after-image and order statistics only after this combined
receipt validates.  Therefore recall's scenario-kill prefix cannot become visible while the air
leader's RETURN receiver is missing or forged.  The focused test host commits only after it has
constructed a validating combined receipt, which pins the live wire-to-prefix-to-action path,
the RETURN helicopter reset/STRAFE arm, and the failure behavior for a missing RETURN tail.

## Honest closure state

The `ObjectTable` and default external `Fleet` implementations deliberately return
`Unavailable`: they do not yet own the complete scenario, launching-list, path/action, and
AirOrder columns.  Consequently `Group::action_recall` remains `Port::StateWired` and opcode 35
remains closure-red.  A missing production host still increments `open_group_action_tails` and
`unported`, preserving the existing command-prefix accounting.  A validating host increments
`acted` and does not increment either red counter.  `Group::action_return` remains
`NotOnTheWire` because retail reaches it only through RECALL, not through a command row.

No command-table status or generated closure row is promoted by this tranche.

## Source set and parent gates

This source-only integration owns:

- `crates/don-sim/src/command.rs`;
- `crates/don-sim/src/systems/return_action_frontier.rs`;
- `crates/don-sim/tests/command_recall_return_integration.rs`;
- `docs/assembly/recall-return-live-integration.md`.

The landed recall frontier is consumed without modification. Root convergence formatted the
owned Rust files. Persvati job
`recall-return-integration-v2-20260809T232231Z-22533-9079-b8a284f806b0` passed all three combined
integration tests and all five existing group-prefix tests. The first submission named a
nonexistent test target and failed before compilation; no engine defect was hidden. The focused
reproduction is:

```sh
cargo test -p don-sim --test command_recall_return_integration \
  --test group_command_prefix_integration
```

The success criterion is a green combined test with recall still `StateWired`, the
successful host path counted as acted, missing/malformed hosts counted as one open/unported tail,
and no closure status delta.
