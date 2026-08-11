# The two air-launch group receivers

`Group::action_scramble` `0x007111C0` (894 B) and `Group::action_launch_patrol` `0x00703580`
(2,043 B), recovered together because they share one body shape and two identical install
branches.

Evidence tier: **C** — capstone over `ron-bin/riseofnations.exe` plus `ron-bin/sbl/rise.pdb`
symbol names and `schema/pdb-types.json` / `schema/types.json` layouts. Nothing here has
been executed against retail. Every VA below was read at instruction level; the Ghidra
output in `re/decomp-all/007111c0.c` and `re/decomp-all/00703580.c` was used only to
navigate, and it loses receivers and argument order in several of the places that matter.

Port: `crates/don-sim/src/systems/air_launch_receivers.rs` (planner) plus the `"scramble"`
and `"launch_patrol"` arms of `crates/don-sim/src/command.rs`.
Tests: `crates/don-sim/tests/air_launch_receivers.rs`.

---

## 1. The claim that was wrong before

The bridge used to serve both rows by iterating the **group's own members** and installing on
each one that answered `Fleet::is_plane`:

```rust
"scramble" => {
    let (who, list) = self.members();
    for o in list {
        if f.alive(who, o) && f.can_move(who, o) && f.is_plane(who, o) { … }
    }
}
```

Retail does not read the member list that way. It walks each member's containment chain:

```text
00711251  movsx eax, word ptr [ebp-0x18]        ; GroupData::list[i]   (+0x8CC, stride 2)
0071126b  mov   eax, dword ptr [0xC0618C]       ; GameAccess::objects
00711270  mov   eax, [eax + who*0x1C + 0x14]    ; Objects::lists[who] element pointer
00711274  mov   eax, [edx + eax]                ; the member Object*
00711277  movsx esi, word ptr [eax + 0x28]      ; ObjectData::inside_down
0071127b  movsx ecx, byte ptr [eax + 0x3E]      ; ObjectData::inside_down_who
00711282  test  esi, esi ; js                   ; a negative link ends the chain
…
007114ff  mov   eax, dword ptr [0xC0618C]       ; and the next link is read from the object
0071150b  movsx esi, word ptr [eax + 0x28]      ; just visited, not from the group member
0071150f  movsx ecx, byte ptr [eax + 0x3E]
00711518  jns   0x711290
```

`+0x28`/`+0x3E` are `ObjectData::inside_down` / `inside_down_who` [`schema/pdb-types.json`],
and `crates/don-sim/src/systems/containment.rs` already models them as a nested chain
(`parent.inside_down = child`, `child.inside_down = nested`). So:

* a selection of **airbases** scrambles the aircraft inside them;
* a selection of **aircraft already on the map** scrambles nothing — the chain is empty.

Both are pinned by tests. The old shape had the two cases exactly the wrong way round.

## 2. Neither receiver calls `Group::action_begin`

`Group::action_scramble` makes exactly three indirect calls — `[eax+0x10]` (`Group::kill`,
the scenario prune), `[eax+0x18]` (`is_unit` on each contained object) and `[edx+0x40]` (the
order's data accessor). `action_launch_patrol` adds `[eax+0x60]` (`ObjectTypeData::is`, three
sites) and `[eax+0x10]` on a `UnitOrder`. **Neither body contains a `call [eax+0x14]`**, so
neither runs `Group::action_begin` `0x00714100` and neither writes `GroupData::disband = 0`.

`crates/don-sim/src/systems/group_action_entry.rs` already had `launch_patrol` right
(`IgnoreOrdersPrune → NumPositive`, nothing else). `scramble` was absent from that table
entirely, so its scenario prune was not running at all; the row is declared here as
`air_launch_receivers::SCRAMBLE_ENTRY` over the same gate alphabet, leaving
`ENTRY_PROGRAMS` as the nine movement/attack rows it documents itself to be.

## 3. `LaunchPatrolCommand` is 25 bytes and all six fields reach the receiver

`?action_launch_patrol@Group@@QAEXVCoord@@0W4QueuePos@@HHH@Z`. `process_launch_patrol`
`0x00949230` logs the packet through
`SyncLogger::logToMemory<int&,int&,enum QueuePos&,int&,int&,int&>` at `0x009492A6` with the
addresses `cmd+1`, `cmd+5`, `cmd+9`, `cmd+0xD`, `cmd+0x11`, `cmd+0x15`, and pushes exactly
those six values at `0x00949335..0x0094936A` before
`imul ecx, [this+0xC], 0x9D4; add ecx, [0xE85F20]; call 0x703580`.

| offset | argument | measured effect |
|---|---|---|
| `+1` | `to_x` | patrol destination |
| `+5` | `to_y` | patrol destination |
| `+9` | `QueuePos` | compared against `1` twice, at `0x007038AF` and `0x00703AA4`; never used as a queueing mode |
| `+0xD` | *`force_all`* | skips the `mana_burn` gate, forces the every-candidate arm, suppresses the single-best install |
| `+0x11` | *`bombers_only`* | requires `ObjectData::is(BOMBER = 0x130, 0)` |
| `+0x15` | *`fighters_only`* | requires `ObjectData::is(BIPLANE = 0x11F, 0)`, and **shadows** `bombers_only` — when it is non-zero the bomber test is not emitted (`0x007036FC..0x0070370B`) |

The italicised names are this port's, chosen from measured effect; the PDB names none of
them. The bridge decoded only the first three before this lane, and
`command::build::launch_patrol` filled the remaining twelve bytes with a guessed
`// shift, ctrl, alt`. `ScrambleCommand` (opcode 36, `process_scramble` `0x00947AE0`) is one
byte and carries nothing.

## 4. Per-candidate predicates, in emitted order

| # | test | scramble | launch_patrol |
|---|---|---|---|
| 1 | object vtable `+0x18` (`is_unit`; constant 1 on `UnitData`, 0 on `BuildData`) | `0x007112A7` | `0x007036A1` |
| 2 | `ObjectTypeData::domain` `+0x218 == 2` | `0x007112C1` | `0x007036B9` |
| 3 | `UnitData::is_busy` `0x0060A370` returns 0 | `0x007112D0` | `0x007036D0` |
| 4 | `ObjectTypeData::obj_masks` `+0x1E4 & 0x08000000` clear | `0x007112EC` | `0x007036EC` |
| 5 | type filter | — | `0x00703718` |
| 6 | `UnitData::mana_burn` `+0x96 == 0` | `0x00711308`, unconditional | `0x00703756`, only when `force_all == 0` |

Predicates 2 and 4 are the same pair `recall_action_frontier` already names `AIR_DOMAIN` and
`MISSILE_OBJECT_MASK`. `+0x96` is `UnitData::mana_burn` by PDB name; what the field means for
an aircraft is **not** derived here, only the predicate. A non-zero value in `launch_patrol`
also latches feedback reason 3.

## 5. The launch cost

Only `launch_patrol` scores. The distance is measured from the **group member**, so every
aircraft in one hangar shares it:

```text
00703792  esi = member->y_internal ^ 0x63637 ; esi -= to_y
007037A1  eax = member->x_internal ^ 0x63637 ; eax -= to_x
007037A9..B7  ecx = |dx| ; edx = |dy|
007037B9  call vector_dist 0x0046CFF0        ; __fastcall(ecx, edx)
007037CE  if   ObjectData::is(BIPLANE)     cost /= 10
0070380F  elif ObjectData::is(HELICOPTER)  cost /= 4
0070384B  if the aircraft's current UnitOrder has a non-zero type, cost *= 200
0070389B  if (force_all == 0 && cost < best) best = (chain_who, chain_o), best_cost = cost
```

`vector_dist` `0x0046CFF0` and `find_angle` `0x0092D130` are declared `__cdecl` in the PDB
and are emitted `__fastcall(ecx, edx)` — no arguments are pushed at any call site here.
`crates/don-sim/src/systems/movement.rs` already documents both that way; this is a third
independent confirmation, and a fourth instance of the standing "the PDB names, the
disassembly establishes" finding.

The best-cost sentinel is `0x0098967F = 9,999,999` (`0x00703616`), and the busy multiplier
is `imul esi, esi, 0xC8` (`0x0070388E`).

## 6. The two install branches

`UnitTypeData::unit_flags` `+0x2B4 & 0x20` selects, at `0x007112D0`/`0x007038D1`/`0x00703AEE`.

**Clear** → `Unit::add_air_patrol_order` `0x005E4350`
`(Coord x, Coord y, int home_o, int home_who, int 1, QueuePos)`.

*The sixth argument is never read.* The body is `ret 0x18` and no instruction in its 527
bytes touches `[ebp+0x1C]`; its helicopter arm forwards `[ebp+0x18]` (argument 5) to
`Unit::add_move_facing_order` and hard-codes `QueuePos = 2` there. All three call sites in
these two receivers push a leftover register into that slot: `0x007114E8` pushes the base's
`y` coordinate, `0x00703C64` and `0x00703C87` push a `Unit*`.
`order_dispatch::install_air_patrol` already records that the installer ignores its
`QueuePos`; this is the call-site half of the same fact, and it is the same shape as
op-econ's `Unit::add_await_board_order` finding.

The home pair differs by arm — measured, not inferred:

| arm | `home_o` / `home_who` | site |
|---|---|---|
| `action_scramble` | `ObjectData::get_inside` `0x00651A80` of the aircraft | `0x007114EB` / `0x007114F3` |
| `launch_patrol`, every candidate | the **group member** and `GroupData::who` | `0x00703C6B` / `0x00703C6C` |
| `launch_patrol`, single best | `ObjectData::get_inside` of that aircraft | `0x00703AD7` |

`ObjectData::get_inside` reads `UnitData::inside_up` `+0x82` / `inside_up_who` `+0xB4` after
a vtable `+0x18` (`is_unit`) guard, and writes `-1` into its out-parameter first
(`0x00651A8F`), so a non-unit answers `(-1, -1)`.

`action_scramble` patrols the aircraft over **the member's own Coord**
(`0x0071134E..0x00711360`), not the aircraft's and not a commanded point — it has no
commanded point, the packet is one byte. `launch_patrol` uses `(to_x, to_y)`.

**Set** → the body inlines `Unit::add_move_facing_order`'s `MOVE_TO` construction rather than
calling it. Field by field, with the concrete `OrderRec` offsets
`crates/don-sim/src/systems/order_dispatch.rs` documents:

| store | offset | `OrderRec` field | value |
|---|---|---|---|
| `0x007113EE` | `+4` | `x` | `div_3_table[cx >> 4] * 0x30 + 0x18` |
| `0x007113FD` | `+8` | `y` | `div_3_table[cy >> 4] * 0x30 + 0x18` |
| `0x00711400` | `+0xC` | `angle` | `find_angle(cx - plane_x, cy - plane_y)` |
| `0x00711403` | `+0x10` | `dest` | `0` |
| `0x0071140A` | `+0x2C` | `dest_x` | same as `x` |
| `0x0071140D` | `+0x30` | `dest_y` | same as `y` |
| `0x00711410` | `+0x34` | `last_x` | `-1` |
| `0x00711417` | `+0x38` | `last_y` | `-1` |
| `0x00711449` | `+0x4C` | `off_x` | `x % 0x300` |
| `0x00711467` | `+0x4E` | `off_y` | `y % 0x300` |
| `0x0071146B` | `+0x44` | `orig_x` | `-1` |
| `0x00711472` | `+0x48` | `orig_y` | `-1` |
| `0x00711481` | `+0x18` | `pause` | `0` |
| `0x00711488` | `+0x1C` | `retry` | `0` |
| `0x0071148F` | `+0x24` | `timer` | `0` |
| `0x00711496` | `+0x28` | `facing` | `-1` |

plus `flags &= ~ORDER_PATHED`, `flags |= ORDER_GROUP`, `flags &= ~ORDER_DISEMBARK` through
the `*(int*)(*order + 4)` base adjustor, and before all of it
`unit_masks &= ~0x04000000`, `Unit::close_orders(0)` `0x005E37F0`,
`Unit::clear_partial_path` `0x005E3920`, `Unit::update_action` `0x0060A870`. The order index
is the literal `push 1` at `0x007113D2` / `0x0070395D` / `0x00703B5E` — `OrderIndex::MOVE_TO`.
`launch_patrol` writes the same sixteen fields at `0x00703985..0x00703A18` and again at
`0x00703B85..0x00703C18`.

## 7. Feedback, and two unreachable arms

When nothing was launched and no best candidate was recorded, `launch_patrol` falls into a
local-player feedback tail gated on `GroupData::who == Console::who` (`[[0x00C06210]+0x298]`,
`0x00703CAF`). It switches on the reason slot `[ebp-0x18]`:

| reason | string | `loc_str_array_orig` ordinal |
|---|---|---|
| 1 | `0x00C8CD00 + 0x841C` | 1691 |
| 2 | `0x00C8CD00 + 0x83F4` | 1689 |
| 3 | `0x00C8CD00 + 0x8408` | 1690 |
| 4 (initial) | — | no message; `0x00703CD3` returns |

`[0x00C8CD00]` is `loc_str_array_orig + 0x10`, a `StringTable` element pointer over 20-byte
`String` records, so byte offset ÷ 20 is the ordinal [standing finding from lane taunt-body;
all three offsets here are exact multiples of 20].

**Reasons 1 and 2 are unreachable in this body.** The slot is initialised to 4 at
`0x007035AD` and the only other write in the whole 2,043 bytes is
`mov dword ptr [ebp-0x18], 3` at `0x00703769`. Their `MessageWin::add_feedback` /
`SoundGlobal::play(0x40)` arms are emitted and can never run. The planner keeps the mapping
so the fact is recorded rather than lost.

## 8. What still blocks `Port::Complete`

Both rows deliberately stay `Port::Orders`. Named, so the next lane does not have to
re-derive the boundary:

1. **`Unit::add_air_patrol_order` `0x005E4350` (527 B) is consumed, not audited.** This lane
   calls `order_dispatch::install_air_patrol`, which is another lane's model of its
   non-helicopter arm; its `unit_flags & 0x20` arm (`Unit::add_move_facing_order`
   `0x005E55C0` with `QueuePos = 2`) is not modelled there. That arm is unreachable *from
   these two receivers*, which test the same flag first, but it is reachable from
   `Group::action_air_patrol` `0x007029D0`.
2. **The inline `MOVE_TO` branch's `Unit::clear_partial_path` `0x005E3920` and
   `Unit::update_action` `0x0060A870` are no-ops here**, the same treatment
   `Group::action_stop_spell` and `air_containment_host` already give them.
3. **No production host answers `Fleet::inside_down`.** `command::ObjectTable` reads it from
   `Slot::follow_inside_down`, which `add_follow_order` populates; `don_env::EnvWorld` does
   not override it and therefore refuses both commands, which is the intended fail-closed
   behaviour but is not a closed row.
4. **The feedback tail is dropped by the command arm.** The planner computes the reason and
   its string ordinal; `Action::action_launch_patrol` does not surface it, because the
   bridge has no `Console::who` for this row. Presentation, but it is a real branch.

## 9. A generated-table discrepancy worth knowing

`crates/don-sim/src/command_tables.rs` lists `installs: &[OrderIndex::AirPatrol]` for both
rows. The only `OrdersMemManager::get_obj` call *inside either body* takes the immediate `1`
— `MOVE_TO`. `AIR_PATROL` (`get_obj(0x11)` at `0x005E43EC`) is allocated inside
`Unit::add_air_patrol_order`, which the table's own comment counts as "directly, via a
`Unit::add_*_order` it calls itself". So the rows are missing their inline `MOVE_TO`. The
file is `@generated`; this is recorded here and in comments on the rows rather than
hand-edited into the data.
