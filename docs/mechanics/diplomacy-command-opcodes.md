# Diplomacy command opcodes 37–45

Recovery lane: `opcode-diplomacy-frontier`, 2026-08-09.  This is Tier C,
instruction-derived evidence from the supported PE32 executable
`riseofnations.exe` (SHA-256 `30478a44…625079`) and `rise.pdb`.  Nothing in this
lane was executed against retail.

The executable planner is
`crates/don-sim/src/systems/diplomacy_command_plans.rs`; its mutation pins are
`crates/don-sim/tests/diplomacy_command_plans.rs`.  Both are deliberately path-imported
and do not edit the shared command dispatcher.

## Wire and receiver map

Every handler reads a signed dword `who` at wire `+1` and directly indexes the eight
`Leader` records (`0x00E3A390`, stride `0x6EEC`) to form the action receiver.  The action
then uses the receiver's `LeaderData::who` dword at `+0x08` as its relation-matrix
identity.  The planner therefore bounds both wire identities and fails closed when the
retail setup invariant `leader slot == LeaderData::who` is broken.  `Leader::valid` is
**not** a handler gate and is intentionally not invented as one.

| op | handler | action | wire fields after opcode | bytes |
|---:|---|---|---|---:|
| 37 | `process_treaty` `0x009473C0` | `Leader::action_treaty` `0x006D1570` | `who, whom, treaty : i32` | 13 |
| 38 | `process_declare` `0x009472B0` | `Leader::action_declare` `0x006DAB50` | `who, whom, treaty : i32` | 13 |
| 39 | `process_clear_tributes` `0x009471B0` | `Leader::action_clear_tributes` `0x006D1690` | `who, whom : i32` | 9 |
| 40 | `process_clear_all` `0x009470B0` | `Leader::action_clear_all` `0x006D15E0` | `who, whom : i32` | 9 |
| 41 | `process_accept` `0x00946FB0` | `Leader::action_respond(whom, 1)` `0x006D03C0` | `who, whom : i32` | 9 |
| 42 | `process_reject` `0x00946E90` | `Leader::action_respond(whom, mode)` `0x006D03C0` | `who, whom : i32` | 9 |
| 43 | `process_tribute` `0x00946D70` | `Leader::action_offer` `0x006D1780` | `who, whom, good, amount : i32` | 17 |
| 44 | `process_demand_tribute` `0x00946C50` | `Leader::action_offer` `0x006D1780` | same; handler passes `-amount` | 17 |
| 45 | `process_propose_attack` `0x00946B30` | `Leader::action_attack` `0x006D14E0` | `who, whom, whose, onoff : i32` | 17 |

Demand negation is x86 two's-complement negation: `INT_MIN` remains `INT_MIN`.  The
planner uses `wrapping_neg`, and all proposal additions/subtractions likewise retain x86
wrapping behavior.

## The 92-byte pair record

`LeaderData+0x692C + whom*0x5C` is one `Diplomacy` record.  Leader zero's first record is
the global address `0x00E40CBC`, which explains the decompiler's mixture of leader-relative
and absolute expressions.

| record offset | planner field | recovered operation |
|---:|---|---|
| `+0x00` | `agreement_pending` | `clear_agree`'s refund gate; `action_respond` tests `== 1` |
| `+0x04` | `proposal_open` | treaty/offer/attack write 1; `clear_agree` writes 0 |
| `+0x08` | `treaty` | symmetric treaty proposal; `clear_all` sentinel is `-1` |
| `+0x0C..+0x20` | `offers[6]` | `Diplomacy::clear_offers` `0x0047DFD0` |
| `+0x24..+0x38` | `declaration_costs[6]` | `Diplomacy::clear_dows` `0x0047E000` |
| `+0x3C..+0x58` | `attacks[8]` | `Diplomacy::num_attacks` `0x0047DEC0` |

`Diplomacy::clear_all` `0x0047E030` zeros every dword except treaty, which becomes `-1`.
`Leader::clear_agree` `0x006D1AF0` is instruction-ordered and is part of every small
transaction:

1. only when `agreement_pending == 1`, walk the six goods;
2. for each nonzero declaration cost, return `min(LeaderData+0x498, cost)` to the decoded
   resource bucket through `LeaderData::bucket_add` `0x0043ED10`;
3. then do the same for each strictly-positive offer;
4. clear all six declaration costs;
5. unconditionally clear `agreement_pending` and `proposal_open`.

The two six-dword resource views are kept distinct in the planner: the availability gate
reads the encrypted buckets reached through `LeaderData+0x6EB8`, while `clear_agree`
debits the dwords at `+0x498` before adding back to those buckets.

## Complete deterministic transactions

Six opcodes and the common Reject arm have a bounded state transaction.

- **Treaty (37):** click-stamp receipt; set sender `proposal_open`; run `clear_agree` on
  sender→target then target→sender; write the treaty value to both pair records.  The
  apparently redundant open write is real and is cleared again by `clear_agree`.
- **Clear Tributes (39):** run both `clear_agree` calls; if target is local and any of the
  sender's six offers remains nonzero, emit the local-notice receipt; clear only the six
  offers in both directions.  Treaty and attacks survive.
- **Clear All (40):** run both `clear_agree` calls; local notification is gated by
  `LeaderData::any_proposals` `0x006D5AD0` (treaty != -1, any offer, or any attack); then
  execute `Diplomacy::clear_all` in both directions.
- **Tribute / Demand (43/44):** positive deltas are refused when
  `existing_offer + amount > sender_bucket`, using signed wrapping addition.  Negative
  deltas skip that gate.  On success, set open, clear both agreements, add the signed
  delta to sender→target and subtract it from target→sender.  Good indices outside 0..5
  are a product boundary rather than an out-of-record memory access.
- **Propose Attack (45):** always set sender open.  A negative `whose` returns immediately
  with that marker intact.  Otherwise clear both agreements and write `onoff` into the
  same attack slot in both directions.  Indices outside 0..7 fail closed.
- **Reject (42), pending-agreement arm:** `process_reject` chooses mode zero exactly when
  the receiver's target `Diplomacy` record dword `+0x00` is 1; it does not call symmetric
  `get_diplo` or inspect either raw declaration.  `action_respond` clears
  `LeaderData+0x314/+0x334` for target, runs only
  sender→target `clear_agree`, and may emit a local rejection notice.

Click stamps, local text, and sounds are retained as ordered presentation receipts.  They
do not mutate walked simulation state and do not authorize skipping a state boundary.

## Exact diplomacy and team gates

The planner reproduces `LeaderData::get_diplo` `0x006EBA50`:

```text
self                                  => 2
either directional declaration == 0  => 0 (enemy)
both directional declarations == 2   => 2 (ally)
otherwise                             => 1 (neutral)
```

The runtime team query is **not copied** here.  It delegates to
`systems::setup_diplomacy::SetupDiplomacy::is_team(..., IsTeamArg::Zero)`, which owns the
frame-zero configured-team and post-frame mutual-alliance behavior recovered from
`LeaderData::is_team` `0x006EBD30`.

The recovered Declare prefix is also executable:

1. if symmetric `is_enemy(whom)` is already true, return with no mutation;
2. a requested declaration 0 against an ally is rewritten to effective declaration 1
   before re-entering the action spine;
3. otherwise declaration 0 takes the no-rush gate using
   `max(sender.declaration_frame[target], target.declaration_frame[sender])`; a zero stamp
   bypasses it, and a nonzero stamp rejects while
   `Constants.no_rush > Game.frame - stamp`;
4. the next call is `LeaderData::afford_dow` `0x006D5CE0`, so the planner stops.

## Deliberate atomic boundaries

These are not TODO-shaped no-ops.  `DiplomacyPlanDecision::Boundary` carries the exact
decoded identities and prefix evidence, but `DiplomacyCommandReceipt::validates` refuses
to validate it as applied.

- **Declare (38):** `afford_dow` and `pay_dow` consume type-table costs and buckets;
  `Leader::set_diplo` `0x006EC6A0` then changes declarations, ejects allied transports,
  adjusts ally visibility, may call `Leader::victory`, notifies `Armies::diplo_change`,
  marks the interface dirty, and fans a new alliance through third-party team queries.
- **Accept (41):** the 3,988-byte `action_respond(..., 1)` checks type availability and
  scaled resources, transfers six goods, resolves attack proposals and treaty state,
  calls `set_diplo`, and emits product callbacks.  Its first two counter clears are prefix
  evidence only; they are not partially committed.
- **Reject (42), non-pending arm:** mode 2 can recursively call `action_declare` from an
  attack counterproposal before clearing both records.  It therefore shares the same
  retarget/vision/victory/event boundary.

This is the reason the lane does not reduce diplomacy to a relation matrix: doing so would
silently omit deterministic resource, unit, fog, victory, army, and event effects.

## Root integration map

No shared files were edited.  Convergence can land this pack independently, then:

1. expose `pub mod diplomacy_command_plans;` beside `setup_diplomacy` in
   `crates/don-sim/src/systems/mod.rs`;
2. extend `command::Fleet` with one atomic diplomacy transaction callback carrying
   `DiplomacyCommandRequest` / `DiplomacyCommandReceipt`;
3. dispatch receiver-`Leader` opcodes 37–45 before the current group-only action gate;
4. count `Apply` decisions as acted only after receipt validation; count `Boundary` or an
   unavailable receipt as unported, leaving bridge and world untouched;
5. only then mark 37/39/40/43/44/45 and the pending-agreement Reject arm state-wired in the command
   ledger.  Opcodes 38/41 and hostile Reject remain red until the complete host
   transaction includes resource transfer plus the `set_diplo` tail.

Focused convergence command (not run in this token-only lane):

```sh
cargo test -p don-sim --test diplomacy_command_plans
```
