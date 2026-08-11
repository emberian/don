# `Group::action_trade` transaction frontier

## Scope and status

This tranche recovers the complete simulation-side decision tree of
`Group::action_trade(int ox, int whom, int oxx, int whose, QueuePos queued)` and the
transitive payload/mutation schedule of `Unit::add_trade_order`.  It is deliberately a
source-only transaction frontier: it does not register a command handler or mutate the
shared command, order, tick, save, schema, or closure owners.

The resulting API accepts a canonical Group before-image, the raw five-word action, and one
coherent host fact bundle.  It returns a totally ordered transaction or an error before any
commit.  Queue-First's heterogeneous order replay remains a typed economy-host boundary; the
frontier never represents it as a generic vector splice.

## Actual retail evidence

| artifact | measured identity |
|---|---|
| `ron-bin/riseofnations.exe` | SHA-256 `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` |
| `ron-bin/sbl/rise.pdb` | SHA-256 `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` |
| PE image base | `0x00400000` |
| PDB symbol | `Group::action_trade`, `groups.cpp:8563..8650` |
| address range | `0x00701CC0..0x007020BE`, 1,022 bytes |
| Capstone 5 x86-32 decode | 299 instructions, all 1,022 bytes consumed |

The Capstone decode ended with the loop back-edge at `0x007020B9`; every normal arm converges
at `0x00701E8C`, then uses the shared epilogue and `ret 0x14` at `0x00701E9C`.

Direct calls named by the PDB and confirmed in the PE are:

| function | VA | PDB bytes | role |
|---|---:|---:|---|
| `Group::action_begin` | `0x00714100` | 8 | clears `GroupData::disband` |
| `GroupData::count` | `0x00711720` | 3,268 | two short-circuited trader counts |
| `Group::set_up_insert` | `0x0070E520` | 256 | Queue-First stash |
| `Group::action_halt` | `0x0070D0C0` | 685 | Queue-First retirement |
| `Group::finish_insert` | `0x0070E620` | 1,104 | heterogeneous stash replay |
| `LinkListBase<UnitOrder *>::clear` | `0x0046F600` | 236 | temporary stash release |
| `WorldData::get_tregion` | `0x006B52E0` | 106 | target/member terrain region |
| `Region::is_coast` | `0x00680F90` | 132 | land/water region adjacency |
| `Unit::add_trade_order` | `0x005E4DC0` | 555 | concrete per-member installer |

The actual PE was decoded directly with Capstone and cross-checked with LLVM objdump.  No
replay checksum or output value was fitted.

## PDB layouts

`GroupData` is 2,508 bytes.  The fields used here are `num +0x0C`, `form +0x10`, `disband
+0x28`, `who +0x4A`, and `list[128] +0x8CC`.  `Group` is 2,516 bytes.

`TradeOrder` is 52 bytes and carries two complete endpoint identities:

| physical offset | field |
|---:|---|
| `+0x08` | `ox` |
| `+0x0C` | `whom` |
| `+0x10` | primary UID, `u16` |
| `+0x14` | `oxx` |
| `+0x18` | `whose` |
| `+0x1C` | `started` |
| `+0x20` | `loaded` |
| `+0x24` | secondary UID, `u16` |
| `+0x30` | virtual-base `UnitOrder::flags` |

The installer initializes `started = 0`, `loaded = 0`, and sets the Group-order flag `0x04`.
The frontier therefore returns the full 52-byte logical payload rather than collapsing the
route to its first city.

## Complete action chronology

1. If `ScenarioData::ignore_orders != 0` and `group.who < 8`, visit that owner's ignore
   array in order.  Skip negative entries and call `Group::kill(o, who, 0, 0)` for every
   nonnegative entry.  This is a complete external receipt because `kill` can recursively
   compact and clear the Group.
2. `Group::action_begin` stores `disband = 0`.
3. Resolve `objects[whom][ox]`.  There is no negative, bounds, or null guard in this body;
   malformed direct callers are a typed retail-crash boundary, not a no-op.
4. Short-circuit entry gates in this order:
   `target->vt[+0x0C]`, `target->vt[+0x4C]`, then
   `target->vt[+0xB0]()->is_trade()`.  The compiler devirtualizes the common `is(0x19E,0)`
   body, but the receiver returned by `+0xB0` remains a distinct fact.
5. Call `GroupData::count(0x11, 0x3B, 0)`.  Only if it returns zero, call
   `count(0x11, 0x13E, 0)`.  Two zero results return without touching `form`.
6. If raw `queued == 0`, execute the Queue-First branch described below.  Otherwise store
   `group.form = -1`, resolve the target terrain region, and walk exactly `group.num` shorts
   from `list +0x8CC` in forward order.
7. Per member, resolve `objects[group.who][o]`, then test vslots `+0x08` (live Unit) and
   `+0xBC` (on map).  An admitted member's terrain region is resolved before re-reading the
   destination type virtuals.
8. The destination's own `+0x24` result is read separately for every live on-map member:
   if true, require `UnitData::is_caravan`.  Otherwise call destination `+0x28`; if true,
   require member `is(0x13E,0)` and `regions[member].is_coast(target)`.  If both destination
   virtuals are false, there is no member-type or region filter.
9. Call `Unit::add_trade_order(ox, whom, oxx, whose, queued == 1 ? 1 : 2, 1)` for each
   accepted member.

All refusals after `action_begin` preserve the `disband = 0` store.  The four entry/count
refusals preserve the prior `form`; the direct arm stores `form = -1` even for an empty Group.

## Queue-First and halt boundary

Raw queue value zero performs exactly:

```text
construct temporary OrderList
Group::set_up_insert(&saved)
Group::action_halt(0)
Group::action_trade(ox, whom, oxx, whose, QUEUE_NEW)
Group::finish_insert(&saved)
saved.clear()
return
```

The recursive call repeats the scenario prune, `action_begin`, target gates, and both count
calls; a host may not reuse outer observations.  `Group::action_halt` is represented by a
complete typed receipt so the containment host can apply the already recovered halt body
(mask clears, order close, path clear, action updates) atomically.

`Group::set_up_insert` asks `copy_order` to clone grouped orders, but kind 15 reaches the
null/default arm.  Although `finish_insert` contains a TRADE_ROUTE reissue case, no old
`TradeOrder` can reach it.  A receipt claiming that the stash contains order kind 15 is
therefore rejected.  `finish_insert` can reissue many other order kinds, so the final Group
after-image stays explicitly owned by the reusable economy host rather than being guessed in
this frontier.

## `Unit::add_trade_order` chronology

For queue value two only:

1. `unit_flags &= ~0x04000000`;
2. store zero to the path anchor at `Unit +0xC0`;
3. `close_orders(0)`;
4. `clear_partial_path()`;
5. `update_action()`.

Then for both Queue-Last and Queue-New:

1. `unit_flags &= ~0x00000200`;
2. allocate order kind 15 and obtain its `TradeOrder` projection;
3. store the primary identity and UID;
4. store `loaded = 0`;
5. store the secondary identity and UID2;
6. store `started = 0` and set order flag `0x04`;
7. optionally set `unit_flags |= 0x00800000` through the cross-region transport cone;
8. append to the order list;
9. `update_action()`.

The UID2 gate has a shipped asymmetry at `0x005E4E67..0x005E4E8F`: it loads
`objects[whose][oxx].uid` only when `oxx >= 0 && primary_whom >= 0`.  It does not test
`whose`.  A negative `whose` in that admitted cone can crash; the frontier preserves this as
a typed crash boundary.

For different actor/primary regions, transport capability is derived from the owner's flag
word: bit `0x100` means 3, else bit `0x200` means 2, else bit 10 means 1.  Retail calls
`UnitData::transport_type`, compares capability, and calls `can_ever_transport` only if the
comparison passes.  Same-region installs read none of these facts.

## City, Good, World, object and Group authority

`Group::action_trade` reads no `CityData`, `Good`, or RNG state. Those dependencies begin
later in `Unit::do_trade` (or outside the trade subsystem); inventing them in this action
transaction would merge distinct retail epochs. The action's real dependencies are:

- canonical Group identity and its post-scenario member prefix;
- object registry identities/UIDs for the destination, members, and optional second endpoint;
- target/Unit/ObjectData virtual results in the exact short-circuit schedule;
- complete read-only `WorldData::get_tregion` receipts: object identity, coordinates,
  transformed tile coordinates, terrain region, and equal before/after World digests; plus,
  only on the sea branch, `Region::is_coast`;
- complete Queue-First halt/replay boundaries; and
- per-member unit/order/path/action before-image digests for atomic installation.

## Files and tests

| path | purpose |
|---|---|
| `crates/don-sim/src/systems/group_action_trade_frontier.rs` | exclusive request/fact/transaction owner |
| `crates/don-sim/tests/group_action_trade_frontier.rs` | seven focused direct/refusal/filter/Queue-First/atomicity pins |
| `docs/assembly/group-action-trade-frontier.md` | this evidence and integration ledger |

Focused command:

```text
cargo test -p don-sim --test group_action_trade_frontier
```

The suite proves the full two-endpoint payload, Queue-New installer micro-order, Queue-Last
omission of the retirement prelude, all three member-filter cones, `action_begin` persistence
on refusal, Queue-First boundary order, omission of saved TRADE_ROUTE, the UID2 asymmetry, and
pre-commit rejection of wrong boundary state/address/call shape.
