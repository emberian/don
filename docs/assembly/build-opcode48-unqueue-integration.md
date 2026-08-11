# Opcode 48 Build unqueue: complete command-path transaction

Status: **complete Build receiver; canonical production host and real packet path wired**.

This tranche closes the residual Build arm of `UnqueueCommand` (opcode 48). The Unit arm was
already complete through the Carrier implicit-queue transaction. A decoded packet can now reach
either concrete receiver, recompute its whole state transaction, bind it to the addressed
`(who, object_index, uid)` identity, and atomically publish through the registered Sim production
host. This is a command-path integration, not a planner-only declaration.

## Why this row

`analysis/scoreboard.py closure` reports **2,081 UnqueueCommands** in the independently decoded
61-recording corpus (585,152 turns, 5,055,253 commands). Before this tranche opcode 48 was the
seventh-heaviest player-decision row still red. Closing it moves the decision-weighted numerator
from 28,886 to 30,967 of 176,807 (**16.3% to 17.5%**) and removes 2,081 commands from the red
decision set. The frequency is reachability evidence only; it does not prove the receiver body.
The chronology is broad rather than one-session noise: 1,348 commands in 2014 recordings, 257 in
2017, 74 in 2018, 59 in 2019, 6 in 2020, 334 in 2024, and 3 in 2025, across 36 recordings.
`analysis/corpus.py verify` independently reproduced the Rust harness's 2,081 count and the whole
5,055,253-command histogram with zero opcode deltas.

## Shipped-image evidence

The source image is `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`. Its shipped matching PDB
names and bounds the reached functions:

| PDB procedure | VA | bytes | full Capstone decode |
|---|---:|---:|---:|
| `Build::action_unqueue(int)` | `0x00620280` | 521 | 521 bytes / 162 instructions |
| `Build::unqueue(int,int)` | `0x006207C0` | 915 | 915 bytes / 260 instructions |
| `LeaderData::get_first_library()` | `0x006DB6C0` | 199 | 199 bytes / 65 instructions |
| `Build::unpay_cost(int)` | `0x006206E0` | 223 | 223 bytes / 61 instructions |

These are complete procedure extents from `schema/pdb-types.json`; Capstone linear decoding of
the PE bytes ends exactly at each PDB boundary. The coherent recovered transaction is:

- a local queue type `0x29A` bypasses Library aggregation; the action-side check clamps a
  negative selector to slot zero, while the virtual-unqueue check requires an actual local slot;
- otherwise a receiver satisfying non-strict `ObjectData::is(0x1B3, 0)` routes through the
  owner's first valid, active, city-attached, assimilated Library and re-enters the action prefix;
- a non-empty queue with repeat bit `0x40` clears the latch first, emits local UI plus sound
  category `0x3D`, and returns immediately for selectors `>= -1`;
- selectors `<= -10`, `-9..=-5`, `-4..=-1`, and `>= 0` cancel all, at most five, the last, or
  the exact slot respectively;
- `Build::unqueue(slot, 1)` advances to the last adjacent identical record, marks the queue dirty,
  zeroes elapsed time, decrements `LeaderData::num_queued[type]` when nonzero, updates the six
  training-family counters, decrements the byte-sized tech/spell queue counters for the exact
  `0x220..=0x274` / `0x275..=0x2AB` intervals, refunds all three stored cost cells in order, and
  compacts only the logical prefix;
- the allocated queue length is not shrunk. Its stale physical tail remains checksum-visible, and
  the repeat bit is cleared again if the logical queue becomes empty. No direct RNG draw occurs.

The implementation is `crates/don-sim/src/systems/build_action_unqueue.rs`. It retains an ordered
step trace plus complete before/facts/after images so a receipt is recomputable rather than a
success token.

## Canonical owner and atomicity

`production::runtime::process_sim_build_unqueue_command` is the sole live commit owner. Before its
first write it materializes and checks every reached state owner:

| retail owner | canonical Sim projection |
|---|---|
| player-major Build band and physical queues | `Sim::world.objects` + `Sim::builds` |
| current producer type / Library predicate | `LiveProductionRuntime::{build_types,types}` |
| assimilation predicate | explicit lazy `build_unassimilated` row |
| per-type and six family counters | `LiveProductionLeader::{queued_counts,carrier_training_queued}` |
| tech/spell queued bytes | `LiveProductionLeader::{ages_queued,epochs_queued}` |
| decoded stockpile mirrors | production leader, economy leader, and step-8 leader, required equal |
| global refund scratch and dirty marker | production runtime / production leader |

Missing reached type, Library, assimilation, unit classification, counter, resource, or mirror
facts return `Unavailable` without mutation. Presentation stays inside the nested action receipt
and can be delivered only after the state commit.

The executable route is:

```text
15-byte UnqueueCommand
  -> Bridge::process_direct_entity_command
  -> Fleet::apply_direct_entity_command_transaction
  -> production::runtime::apply_sim_unqueue_fleet_transaction
  -> process_sim_build_unqueue_command
  -> plan + identity-bound recomputable receipt
  -> atomic Build/counter/resource commit
```

`crates/don-sim/tests/command_carrier_unqueue_integration.rs` drives that route through the
original packet walker and asserts the physical queue, type count, refund mirrors, dirty marker,
and nested receipt. `crates/don-sim/tests/build_action_unqueue.rs` pins selector bands, repeat
presentation, Library routing, the lazy `0x29A` bypass, every external owner, physical allocation,
and mutation rejection.

Convergence gates on 2026-08-11:

- local focused command/receiver tests: 25 passed;
- local `don-sim --lib`: 1,736 passed, 2 ignored, 0 failed;
- local `don-closure` binary tests: 9 passed;
- Persvati focused real-command overlay
  `opcode48-build-unqueue-20260811T173248Z-27696-12026-4d902f60b235`: exit 0;
- Persvati final closure overlay (including the SHA-pinned balance asset)
  `opcode48-final-20260811T174019Z-39806-23498-28b2e5d5c419`: exit 0.

## Fidelity boundary

This is **instruction-derived Tier C at best; no retail differential execution**. The supported
PE/PDB and full Capstone bodies establish the source and control/data dependencies, while Rust
tests establish our internal transaction and real command wiring. They do not elevate the port to
Tier B. No VM or live retail heap was used in this tranche.
