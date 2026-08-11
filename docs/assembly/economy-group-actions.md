# Economy group actions: REPAIR, TRADE, BOARD_SHIP

Lane `op-econ`, megaswarm wave 2, 2026-08-11. Rows `closure/opcode: RepairCommand` (16),
`TradeCommand` (17), `BoardShipCommand` (15).

**Tier C.** Every number here is `[measured]` by capstone disassembly of
`ron-bin/riseofnations.exe` (sha256 `30478a44…625079`, image base `0x00400000`), named and
layouted from `ron-bin/sbl/rise.pdb`. Nothing has been executed against retail; there is no
differential run and no equivalence claim. The Rust tests below are unit tests, not
verification.

## What the bridge was doing, and why it was wrong

`crates/don-sim/src/command.rs` routed opcodes 15/16/17 (and 19/20) through one shared
`Action::action_target` helper: *install the named `OrderIndex` on every alive member of the
selection*. Its own doc comment says all of them are "[structure] from their decompiled
bodies". Read at instruction level, that shape is wrong for the three rows recovered here in
four separate ways.

| what retail does | what the shared helper did |
|---|---|
| writes `GroupData::form = -1` before installing | never touched `form` |
| `REPAIR` may install a **`CAST_SPELL`** order too | installed only `REPAIR` |
| `REPAIR` retires the **target's** order list and cast mask | left the target alone |
| `BOARD_SHIP` installs `AWAIT_BOARD` **on the ship**, once per passenger, after clearing the ship's list | installed only `BOARD_SHIP` on the passengers |
| `TRADE` carries **two** object endpoints | dropped `oxx`/`whose` entirely — the bridge did not even decode wire `+9`/`+13` |
| each has a specific per-member eligibility ladder | only `Fleet::alive` |

## Devirtualisation: the PDB slot names are not the callees

MSVC folded one-instruction virtuals across unrelated classes, so a slot's PDB name is the
name of *some* function with that body. These bodies decide every gate below.

| body VA | emitted body | what it means at these call sites |
|---|---|---|
| `0x0041BFF0` | `xor eax,eax; ret` | constant `0` |
| `0x0041E0E0` | `mov eax,1; ret` | constant `1` |
| `0x0041C000` | `mov eax,ecx; ret` | identity — `Build`'s `+0xB0` returns `this` |
| `0x0046CDA0` | `movzx eax,[ecx+8]; and eax,1` | `ObjectData` flag byte `+0x08` bit 0 |
| `0x00472350` | `movzx eax,[ecx+8]; and eax,4` | `WallData::is_active` |
| `0x0046CE30` | `movzx eax,[ecx+0x82]; shr eax,15` | `UnitData::is_on_map` |
| `0x0046CE90` | `jmp [type->vt+0x130]` | `UnitData::is_caravan` |
| `0x006424E0` | `is(0x19E, 0)` | `WallData::is_trade` |
| `0x004722F0` | `is(0x1B0, 0)` | `WallData::is_sea_trade` |

`Unit` answers `1` on vslot `+0x08` and `0` on `+0x0C`; `Build` is the reverse. So the member
gate `vt[+0x08]() && vt[+0xBC]()` that all three loops open with is exactly **"a live unit
that is on the map"**, and buildings in the selection are filtered out by construction, not
by an explicit type test.

`0x0046CE40` `UnitData::is_plane` is `type->domain == 2 && !(type->[0x2B4] & 0x20)`, which
independently fixes **domain 2 = AIR** — the value `action_board_ship` rejects.

## `Region::is_coast 0x00680F90` is not a coastline test

It takes the *other* region id, with `this = &regions.array[self]` (stride `0x88`,
`GameAccess::regions` `0x00C061B8`, array at `+0x10`, `self` id at `+0x10`):

```text
self == other                  -> 1
self >= 0x40 && other >= 0x40  -> 0
self <  0x40 && other <  0x40  -> 0
self >= 0x40                   -> bit (self  - 0x40) of regions[other].bits[+0x64]
otherwise                      -> bit (other - 0x40) of regions[self ].bits[+0x64]
```

i.e. "same region, or a land/water pair that touch". All three actions evaluate it as
`regions[get_tregion(member)].is_coast(get_tregion(target))`, where `get_tregion` is
`WorldData::get_tregion` `0x006B52E0` (`this = GameAccess::world 0x00C06188`) applied to the
tile pair `div_3_table[(coord ^ 0x63637) >> 6]`, `div_3_table = 0x00CAE5FC`. Object x is at
`+0x10` and y at `+0x14`, both XOR-obfuscated.

## `Group::action_repair` `0x007020C0` (999 B)

`void __thiscall (int ox, int whom, QueuePos queued)`, `ret 0xC`.
`process_repair` `0x00948CB0` resolves `objects[whom][ox]`, requires its flag byte `+0x08`
bit 0, and calls with the three wire dwords `(ox@1, whom@5, queued@9)` `[0x00948DE1]`.

1. `ScenarioData::ignore_orders` `0x00CC02F8` prelude, then concrete `Group::action_begin`.
2. `queued == QUEUE_FIRST` → `set_up_insert` / `action_halt(0)` / re-enter with `QUEUE_NEW`
   / `finish_insert` `[0x0070215C]`.
3. `group.form = -1` `[0x007021D8]`.
4. `target_region = get_tregion(objects[whom][ox])`.
5. Per member, in list order:
   * `vt[+0x08]()` and `vt[+0xBC]()` — live unit, on map;
   * `units[who][o]->type->[+0x04]` must be `0x32` or `0x33` `[0x007022C3]`;
   * `regions[member_region].is_coast(target_region)` `[0x00702329]`;
   * `queued == QUEUE_LAST` `[0x00702336]`:
     * `spelltypes[0x293]->is_castable(o, who, 0)` → `add_cast_order(-1,-1,x,y,0x293,1,1)`
       `[0x0070236B]`, then `add_repair_order(ox, whom, 1, 1)` `[0x007023AA]`;
   * otherwise: `add_repair_order(ox, whom, 2, 1)` `[0x007023D1]`, then the same castable
     test → `add_cast_order(-1,-1,x,y,0x293,0,1)` `[0x0070240A]`;
   * then, on `units[whom][ox]` — the **repair target**, once per accepted member
     `[0x0070244A]`: `unit_masks &= ~0x04000000`, `[+0xC0] = 0`, `Unit::close_orders(0)`,
     `Unit::clear_partial_path`, `Unit::update_action`.

`spelltypes[0x293]` is `[[GameAccess::spelltypes 0x00C061A8]+0x10]+0xA4C`; `0x293` is inside
`cast_order_frontier`'s recovered spell range `[0x275, 0x2AC)`.

## `Group::action_trade` `0x00701CC0` (1022 B)

`void __thiscall (int ox, int whom, int oxx, int whose, QueuePos queued)`, `ret 0x14`.
`process_trade` `0x00948B20` validates **both** `(whom, ox)` and `(whose, oxx)` against the
same flag bit and passes all four through `[0x00948C9D]`. The wire is
`ox@1 whom@5 oxx@9 whose@13 queued@17`, matching `TradeOrder`'s two identities
(`OX/WHOM/UID` at `+0x08..+0x12`, `OXX/WHOSE/UID2` at `+0x14..+0x26`, already frozen in
`crates/don-sim/src/systems/trade_order_frontier.rs`).

Entry gates, all of which return without touching `form`:

1. `target->vt[+0x0C]()` — a live `Build` `[0x00701D85]`;
2. `target->vt[+0x4C]()` — `WallData::is_active` `[0x00701D94]`;
3. `target->vt[+0xB0]()->vt[+0x24]()` — `WallData::is_trade` `[0x00701DB3]`;
4. `group.count(0x11, 0x3B, 0) != 0 || group.count(0x11, 0x13E, 0) != 0` `[0x00701DFF]`.

Then the `QUEUE_FIRST` dance, `group.form = -1`, and per member the `vt[+0x08]`/`vt[+0xBC]`
gate followed by a filter chosen by re-reading the destination's **own** virtuals — a
different receiver from gate 3, which read the `+0xB0` result:

| destination answers | member must |
|---|---|
| `vt[+0x24]() != 0` | `UnitData::is_caravan` `[0x00701FD6]` |
| else `vt[+0x28]() != 0` | `is(0x13E, 0)` **and** `is_coast(dest_region)` `[0x00702012, 0x0070205B]` |
| else | nothing — no member filter at all |

then `add_trade_order(ox, whom, oxx, whose, queued == 1 ? 1 : 2, 1)` `[0x00702095]`.

## `Group::action_board_ship` `0x00700010` (1149 B)

`void __thiscall (int ox, QueuePos queued)`, `ret 8`. There is no owner field: the ship is
`objects[group.who][ox]` `[0x00700184]`.

After the prelude, `action_begin` and (for `QUEUE_FIRST`) the dance, `group.form = -1` and
`ship_region = get_tregion(ship)`. Per member:

* `vt[+0x08]()`, `vt[+0xBC]()`;
* `units[who][o]->type->[+0x218] != 2` — reject AIR `[0x00700272]`;
* `!UnitData::is_busy(unit)` `[0x00700289]`;
* `regions[member_region].is_coast(ship_region)` `[0x00700301]`;
* `ship->can_carry(o, who)` `0x006483C0`; a zero increments a local counter and skips
  `[0x0070032B]`;
* `add_board_order(ox, who, queued == 1 ? 1 : 2, <unread>)` `[0x0070035A]`;
* if no passenger has boarded yet, `Unit::clear_orders()` on the **ship** `[0x00700372]`;
* `add_await_board_order(o, group.who, <unread>, 1)` on the **ship** `[0x00700397]`.

The tail (`0x007003BD..0x0070047A`) is local-player presentation only: `MessageWin::add_message`
or `add_feedback` plus `SoundGlobal::play(0x40)` when the sender is `Console+0x298` and at
least one passenger was refused. It writes no simulation state.

### Two argument facts worth keeping

`Unit::add_board_order` `0x005E4D10` has `ret 0x10` (four args) but never reads `[ebp+0x14]`;
`Unit::add_await_board_order` `0x005E4C80` also has `ret 0x10` and never reads `[ebp+0x10]`.
Both call sites push a leftover `ECX` into the unread slot. So **`AWAIT_BOARD` has no
`QueuePos` at all**: `0x005E4C80` has no `queued == 2` arm and always appends through
`LinkListBase<UnitOrder*>::add` `0x0046D5A0`, with the order's flag bit `0x04` set because
its fourth argument is the literal `1`.

`add_board_order`'s `queued == 2` arm is the shared retirement prologue —
`unit_masks &= ~0x04000000`, `[+0xC0] = 0`, `close_orders(0)`, `clear_partial_path()`,
`update_action()` — which is the same five steps `action_repair` performs on its *target*.

## What is deliberately not here

* **List position.** `LinkListBase<UnitOrder*,unsigned char,RecycledOrderNode>::add`
  `0x0046D5A0` inserts before the list's *current* node and then makes the new node current.
  This lane did not chase that far enough to state where `CAST_SPELL` lands relative to
  `REPAIR`, so the recovered plan carries the exact `QueuePos` retail passes and leaves
  placement to the receiver's queue model. The bridge's `OrderQueue` appends, which is the
  model it already used for every other `add_*_order`.
* **`Order` cannot carry the trade route's second endpoint.** `crates/don-sim/src/order.rs`
  has `follow`, `special_anim` and `form_order` payloads but no `TradeOrder` payload for
  `(oxx, whose, uid2)`. Adding one touches `systems/save_load.rs` (checksummed order state)
  and `systems/order_dispatch.rs`, both of which another lane holds, so the recovered
  endpoints are carried on the plan step and in the drained receipt instead. **This keeps
  `TradeCommand` red**: a `TRADE_ROUTE` order installed from the wire still names only one
  city.
* **`Order` cannot carry a spell type.** The recovered `CastOrder` payload has `SPELL` at
  `+0x20` (`systems/cast_order_frontier.rs`); `Order` has no field for it, so the installed
  `CAST_SPELL` order records position only.
* **`BOARD_SHIP`'s `QUEUE_FIRST` arm.** Retail does *not* run the generic dance: it builds a
  temporary stack `Group` (vtable `0x00B47C34`), `Group::clear(-1)` twice, `Group::add(ox,
  group.who, 0, 0)`, and hands **that** group to `Group::finish_insert` `0x0070E620`
  `[0x00700111..0x00700161]` — so the stashed orders are replayed against a selection
  containing only the ship. The bridge still replays to the original owners. Recovering
  `finish_insert`'s 1,104-byte replay is the blocker.
* **`GroupData::count`'s `CountIndex`.** Index `0x11` is passed with type arguments `0x3B`
  and `0x13E`; the enumerator's meaning is not recovered, so the bridge asks the host for
  the count verbatim and models nothing.

## Files

| path | what |
|---|---|
| `crates/don-sim/src/systems/economy_group_actions.rs` | the recovered planners, host-fact types, and 11 branch pins |
| `crates/don-sim/tests/economy_group_actions.rs` | 9 dispatcher-level pins driving `Bridge::process_one` with real wire bytes |
| `crates/don-sim/src/command.rs` | nine defaulted fail-closed `Fleet` reads, eight `Slot` columns, the three `Action::run` arms, `Bridge::take_economy_action_receipts` |

## Closure status after this tranche

All three rows stay **`orders_partial`**. `Port::Complete` for any of them needs the items
under "deliberately not here": `Order` payload growth for `TRADE`, `finish_insert` for
`BOARD_SHIP`, and the `[+0xC0]`/`clear_partial_path`/`update_action` unit columns for
`REPAIR`'s target retirement. The gap is now enumerated per gate at runtime —
`Bridge::take_economy_action_receipts()` names every retail read the host declined — instead
of being invisible inside a shared helper.
