# Command rows 46–49: market and direct-entity prefixes

Status: **prefixes frozen; all four opcode rows remain red until dispatcher and atomic
state adapters land.**

This note records the disjoint planner lane in
`crates/don-sim/src/systems/direct_entity_command_plans.rs`.  The file is intentionally
path-imported only by `crates/don-sim/tests/direct_entity_command_plans.rs`; this swarm lane
does not mutate the shared command dispatcher or module map.

## Evidence and wire ABI

The fixed layouts agree between `schema/command-wire.json`, PDB command records, and the
handler return sizes:

| op | handler | bytes | exact fields | reached tail |
|---:|---|---:|---|---|
| 46 | `CommandPackage::process_buy` `0x00946A20` | 13 | `i32 who@1, good@5, flags@9` | `Leader::action_buy` `0x006CFA20` → `do_buy` `0x006CFBD0` |
| 47 | `CommandPackage::process_sell` `0x009468A0` | 13 | `i32 who@1, good@5, flags@9` | inline gate → `do_sell` `0x006CFC60` |
| 48 | `CommandPackage::process_unqueue` `0x009466F0` | 15 | `i32 who@1, o@5, type@9; i16 uid@13` | `Unit::action_unqueue` `0x005E1F20` or `Build::action_unqueue` `0x00620280` |
| 49 | `CommandPackage::process_come_out` `0x009465D0` | 11 | `i32 who@1, o@5; i16 uid@9` | `Unit::action_come_out` `0x005E20B0` |

All dwords and UID shorts stay signed in the request ABI.  No planner normalises negative
leaders, object indexes, resource indexes, queue types, flags, or UIDs.  A safe adapter must
validate signed indexes before touching Rust storage and return an unavailable receipt when
the retail pointer lookup cannot be represented safely.

## Opcode 46: buy

`Leader::action_buy` derives its call limit from the low flag bits:

```text
count = (flags & 1) * 4 + 1              // 1 or 5
if flags & 4: count = count == 1 ? 10 : 100
if flags & 2: count = 99999              // final override
```

It then short-circuits on `Leader::can_buy_sell()` (`0x006D53E0`).  If eligible, a nonzero
`get_nuke_embargo()` routes to `Leader::tell_embargo` (`0x006CFAF0`); otherwise it repeatedly
calls `do_buy(good)` and stops at the limit or the first nonzero/refused return.  The existing
`systems/economy.rs::do_buy` owns the recovered market arithmetic.  This planner emits one
typed market-loop delegation rather than duplicating that arithmetic.

## Opcode 47: sell

The handler is not merely a call to `Leader::action_sell`.  Its exact gate is:

```text
eligible_preq = has_tribe_bonus(4) || has_preq(0x2ad)
eligible      = eligible_preq && has_market()
```

Those calls retain retail short-circuit order.  The count is 1/5 from flag bit 0, overridden
to 99,999 by bit 1.  **Flag bit 2 (`0x4`) is ignored for sell.**  After the same embargo gate,
the loop calls `do_sell(good)` until its limit or first refused result.  The existing
`systems/economy.rs::do_sell` remains the state-writing owner.

`Leader::tell_embargo` compares `LeaderData::who` at `+8` with
`Console::display_play +0x298`; only the local match creates UI text and calls
`SoundGlobal::play(0x40)`.  UI and audio are separate ordered effects.  Planning consumes no
RNG.  The future audio adapter must use the sound RNG stream; the simulation planner must not
borrow it.

## Opcodes 48/49: stale-target guard

Opcode 48 first probes the base object's virtual class.  The unit branch reads the direct
unit-table target and calls `Unit::action_unqueue(1)`; the build branch resolves the concrete
build and forwards the signed wire `type` to `Build::action_unqueue(type)`.  Thus the unit
branch deliberately ignores the wire type.

Opcode 49 directly resolves the unit-table target and calls `Unit::action_come_out()`.

Both handlers require active bit `entity+8 & 1` and compare the zero-extended word at
`entity+0x30` with the sign-extended wire UID.  In x86/C conversion terms:

```text
(uint)(ushort)object_uid == (int)(short)wire_uid
```

A negative wire UID therefore does **not** alias object UID `0xffff`.  Inactive or stale-UID
targets are exact no-ops after the diagnostic prefix.  Missing/unsafe pointer resolutions are
unavailable, not fabricated no-ops.

The downstream action bodies are broad state transactions.  `Build::action_unqueue` crosses
queue, refund, population/category-counter, UI/audio, and possible owning-build boundaries;
`Unit::action_unqueue` mutates queued counts and may enter a virtual destruction tail;
`Unit::action_come_out` crosses containment/carrier and placement state.  This lane does not
claim those tails complete and makes no RNG claim about code below the typed delegates.

## Frozen integration map

| row | dispatcher prefix | host fact owner | atomic delegate | completion gate |
|---:|---|---|---|---|
| 46 | exact 13-byte decode; signed leader/good/flags | selected leader plus `Game` market | loop over `economy::do_buy`, stop on `TradeResult::Refused` | authoritative leader+market transaction receipt and checksum coverage |
| 47 | exact 13-byte decode; ordered tribe/preq/market/embargo reads | selected leader plus `Game` market | loop over `economy::do_sell`, stop on `TradeResult::Refused` | same, including tribe-bonus gate vector |
| 48 | exact 15-byte decode; class → concrete target → active/UID guard | Fleet/object resolver | `Unit::action_unqueue(1)` or production-owned `Build::action_unqueue(type)` | authoritative queue/refund/counter receipt for both classes |
| 49 | exact 11-byte decode; direct unit target → active/UID guard | Fleet unit resolver | production/containment-owned `Unit::action_come_out()` | authoritative containment/placement receipt |

Opcode 47 is `Receiver::None` in the current descriptive command table because its handler
inlines the gates instead of naming a receiver action.  Its adapter must still resolve the
leader identified by the signed wire `who`; it must not silently substitute `Package::play`.
Likewise opcode 48's `Receiver::Unit` label is not permission to skip the retail class probe:
the same wire row legitimately reaches the Build tail.

Integration must preserve effect order and execute each reached delegate atomically.  The
planner receipts only recompute the pure prefix; they are deliberately insufficient to turn
the four ledger rows green.  The integration patch may export this module and add the four
dispatcher arms, but it must not copy market, production, containment, UI, or audio mutation
logic into the command decoder.

## Mutation pins

The path-import tests freeze exact lengths/opcodes and signed extrema; the distinct buy/sell
flag matrices; lazy gate reads; local-only ordered embargo presentation; signed market target
delegation; the negative-UID comparison trap; unit/build unqueue routing; inactive/stale
no-ops; the opcode-49 unit-only ABI; and receipt recomputation after one-field mutations.

Per swarm constraint, no compiler, test runner, formatter, or remote job was invoked for this
lane.
