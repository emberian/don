# Carrier implicit-queue unqueue frontier

Status: **complete receiver transaction; opcode 48 Unit branch production-wired**.

This pack recovers the bounded Carrier-owned implicit queue cancellation that production's
Build-row adapter explicitly leaves open. The source is
`crates/don-sim/src/systems/carrier_implicit_unqueue_frontier.rs`; mutation-sensitive path-import
tests are `crates/don-sim/tests/carrier_implicit_unqueue_frontier.rs`. The direct-entity adapter
now nests this module and the canonical production runtime consumes it for opcode 48's Unit
receiver without registering another tick system.

## Authority and extent

All claims are Tier C **[measured]** against `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and the matching shipped
`ron-bin/sbl/rise.pdb`. PDB names/field offsets were checked against `schema/pdb-types.json`; the
control flow, virtual slots, integer widths, and write order were checked against direct PE32
instruction disassembly. Ghidra output was navigation only.

| symbol | VA | bytes | role |
|---|---:|---:|---|
| `Unit::action_unqueue(int)` | `0x005E1F20` | 389 | complete implicit-queue receiver |
| `LeaderData::current_upgrade(TypeIndex)` | `0x006E3140` | 342 | resolves current Helicopter from base `0x134` |
| `Type::unpay_cost(int,int,int)` | `0x006682F0` | 136 | complete optional refund leaf |
| `TypeData::is_unit_type()` | `0x004707E0` | 24 | queued-family classification |

The PDB pins `UnitData::queue_time +0x64`, signed-short `num_queued +0xA0`,
`LeaderData::num_queued[806] +0x5A22`, the six named queued-family dwords at
`+0xA10..+0xA24`, `ObjectTypeData::attack +0x1E8`, and `domain +0x218`.
Instruction-level call discovery finds exactly four direct callers: `Build::train`,
`CommandPackage::process_unqueue`, `ScenarioEditor::unqueue_unit`, and
`ScenarioFuncSet::add_unit`. The first is the new-Carrier payload path and the second is opcode
48's Unit branch; the editor/script callers show why the receiver cannot be modeled as merely a
Build-queue bookkeeping shortcut.

## Exact transaction

An empty `UnitData::num_queued` returns before reading owner, upgrade, type, Game, cost, or
resource state. Otherwise retail:

1. decrements the signed 16-bit Carrier count with x86 wrapping semantics;
2. clears `queue_time` only when that stored result is exactly zero;
3. calls `current_upgrade(HELICOPTER=0x134)`;
4. for a nonnegative type, conditionally decrements the owner's unsigned aggregate
   `num_queued[type]` without underflow;
5. calls `is_unit_type`, then for an armed ObjectType classifies `TypeData::where`:
   `0x1AB` Barracks, `0x1AC` Stable, `0x1AE` Factory, `0x1B0` Dock, or a default arm whose
   `domain == AIR (2)` decrements Air;
6. Barracks and Stable each also decrement the shared Combat counter, after their specific
   counter. Every family counter is conditionally decremented only when nonzero;
7. when the action argument is nonzero, calls `Type::unpay_cost(owner,-1,-1)` after every queue
   mutation.

The refund leaf first checks `Game+0x821 & 8` and returns in no-cost mode. Otherwise it visits
goods `0..5` in increasing order, calls `LeaderData::type_avail(good,1)`, and only for an
available good requests
`Type::get_cost(good,owner,-1,-1,1,1,-1)`. It decodes the owner resource with XOR key `0x8221`,
adds the signed cost with 32-bit wrapping arithmetic, publishes the decoded result through
global scratch `0x00CB195C`, then stores the re-encoded resource. The source trace preserves the
scratch-before-resource ordering for every reached good.

Neither body calls canonical game RNG or sound RNG.

## Typed ownership and fail-closed edge

`CarrierImplicitQueueState` binds one Carrier row, one owner, the exact selected aggregate cell,
all six queued-family counters, encrypted resources, and refund scratch. `QueueTypeFacts` keeps
the retail lazy reads explicit: non-unit types cannot provide ObjectType facts; unarmed units
cannot provide `where`; fixed training sites cannot provide `domain`; only the default site must
provide `domain`. Refund availability and cost results are similarly required only when the
retail loop reaches them.

Retail assumes the current Helicopter upgrade is nonnegative when refunding: it uses the result
as a type-table index after the earlier Carrier write. The isolated planner rejects a negative or
out-of-range refund type before authorizing any mutation. This is an intentional safe-host fence,
not a claim that retail has a hidden recovery branch.

`CarrierImplicitUnqueueReceipt::Complete` is recomputable from request, before-image, and typed
facts. Mutation of the state, facts, after-image, or ordered trace invalidates it. `Unavailable`
carries none of those and authorizes no write.

## Honest closure and residual

The canonical Sim adapter now owns the selected Unit row, exact aggregate cell, queued-family
counters, economy stockpile, production stockpile mirror, and refund scratch. It preflights the
installed upgrade/type/cost and economy-availability projections, plans this receiver, validates
the composed direct-entity receipt, and only then publishes the infallible owner updates. Opcode
48's active Unit branch therefore closes; inactive/stale arms keep the earlier lazy complete
no-op. The Build branch is a separate `Build::action_unqueue(type)` transaction; it was open in
this source frontier and is now closed through the canonical production host documented in
`docs/assembly/build-opcode48-unqueue-integration.md`.

Root convergence formatted the isolated files and validated all 13 tests in persvati batch
`gen7-five-pack-20260809T231109Z-3866-5144-5f896c0277b5`. Retail was not run. The focused
reproduction is:

```sh
cargo test -p don-sim --test carrier_implicit_unqueue_frontier
```
