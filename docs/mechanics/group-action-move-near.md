# `Group::action_move_near` `0x00704990`

Lane `move-near`, megaswarm wave 3. 2026-08-11. **Tier C** — everything below is
`[measured]` from capstone disassembly of `ron-bin/riseofnations.exe` plus a Ghidra
decompilation of the same body, named from `ron-bin/sbl/rise.pdb`. Nothing has been
executed against retail.

This is the root of the movement/attack family. `Group::action_move_to` `0x0070FBA0` is a
49-byte argument shuffle that tail-calls it with `tolerance = 0`, and `form`, `attack`,
`swarm_around` and `siege_attack` all reach it through `move_to`. Nine `closure/opcode`
rows sit behind it.

## 0. Why there was no decompilation, and how to get one

`re/scripts/BulkDecomp.java` skips any function whose body exceeds its `maxBodyBytes`
argument; `re/decomp-all/` was generated with 8,192 and this body is **9,205** bytes
(`S_GPROC32`), so it was recorded as `skipped_large` in the corpus manifest and every lane
that looked for `re/decomp-all/00704990.c` correctly found nothing and incorrectly
concluded the body was inaccessible.

It is not. Ghidra decompiles it in seconds:

```sh
cp -R re/ghidra /tmp/ghidra-lane          # the project has a single-writer lock
"$(brew --prefix ghidra)/libexec/support/analyzeHeadless" /tmp/ghidra-lane ron \
  -process -noanalysis -scriptPath re/scripts \
  -postScript DecompileOne.java 00704990 900 /tmp/00704990.c
```

The result (1,287 lines) is now at `re/decomp-all/00704990.c`. That directory is
gitignored, so this is a local corpus repair rather than a commit. **Every other
`skipped_large` entry is reachable the same way** — "no file in `re/decomp-all/`" means
"over 8 KiB", not "undecompilable".

## 1. Signature and frame

```text
public: void __thiscall Group::action_move_near(
    Coord x, Coord y, int tolerance, QueuePos queued, int set_angle, int angle,
    OrderIndex orders, int form, int width, int disembark, int caller)
```

`ret 0x2c` — eleven stack dwords plus `this` in `ECX`. Two whole `Group` objects live on
the frame, at `ebp-0xA40` (call it **A**) and `ebp-0x1414` (**B**), each `0x9D4` bytes,
each `Group::clear(-1)`ed and then given `id = -1` explicitly at `0x007049FE` and
`0x00704A30`.

Ghidra's `local_N` names are `ebp - (N - 4)`; calibrate on `local_10 == ExceptionList`
before reading any offset out of the decompiled C. `local_a38`/`local_140c` are therefore
`A.num`/`B.num`, not `A.army`/`B.army`.

## 2. The entry gates

`is_on_map` → scenario `ignore_orders` prune → `num > 0` → `[0x00EE1540] = 0` →
`action_begin` (vtable `+0x14`) → per-axis destination clamp. All of this already lives in
`systems/group_action_entry.rs` and is unchanged by this lane, with one addition worth
recording: the `mov dword [0xEE1540], 0` at `0x00704B19` — the reset of the global
`Stack<PathData>` count that §7's disembark tail then fills — happens **before**
`action_begin`, not after.

## 3. The selection split (`0x00704B6F..0x00704E58`) — recovered and ported

Between the entry gates and the first order queue, retail sorts the selection into A and B
and, when both come out non-empty and the receiver is not an army, abandons the receiver
entirely.

```text
pass 1 (00704B90):  for i in 0..num:  o = list[i]
  if (!objects[who][o]->vf(+0x08)) continue                  ; is_valid_unit
  if (units[who][o]->ptype->domain == 1)      B.add(o, who, 0, 0)      ; Sea
  else if ((c = get_inside(&cw)) >= 0)        B.add(c, cw, 0, 0)       ; the transport
  else                                        A.add(o, who, 0, 0)

00704C49  if (B.num == 0) goto §4                            ; no split, and no pass 2
00704C56  if (A.num != 0 && this->army < 0) goto SPLIT
00704D31  if ((terrain[dest] & 0x30) == 0x20) goto §4        ; a water destination stops here
00704D5B  A.clear(-1); B.clear(-1)

pass 2 (00704D90):  for i in 0..num:  o = list[i]
  if (!objects[who][o]->vf(+0x08)) continue
  if (units[who][o]->ptype->unit_flags & 0x10)  A.add(o, who, 0, 0)
  else if ((c = get_inside(&cw)) >= 0)          A.add(c, cw, 0, 0)
  else                                          B.add(o, who, 0, 0)

00704E3D  if (B.num != 0 && A.num != 0 && this->army < 0) goto SPLIT

SPLIT (00704C6D):
  s = Groups::push_group(who, &A, 1)
  groups.list[s].facing = this->facing                       ; byte at +0x48
  groups.list[s].action_move_near(<the eleven arguments, unchanged>)
  s = Groups::push_group(who, &B, 1)
  groups.list[s].facing = this->facing
  groups.list[s].action_move_near(<the eleven arguments, unchanged>)
  this->Group::clear(-1)
  return
```

Notes that are load-bearing and easy to transcribe wrong:

* **The buckets swap between the passes.** Pass 1 sends sea units *and* containers to B;
  pass 2 sends transport-flagged types *and* containers to A. A is pushed first either way.
* **`domain == 1` is `Sea`** (`ObjectTypeData::domain` `+0x218`; `systems/air.rs` already
  measured `DOMAIN_SEA = 1`, `DOMAIN_AIR = 2`). Pass 1 is the land/sea split: a mixed
  naval-and-land selection never gets one formation.
* **`get_inside` is `ObjectData::get_inside` `0x00651A80`**, which returns the container's
  object index and writes the container's *owner* through the out parameter. This is the
  garrison-into-transport arm: an embarked passenger is not commanded at all; its
  transport is.
* **`unit_flags & 0x10` is dead on shipped rules.** `UnitTypeData::unit_flags` `+0x2B4`
  bit 4 is the type-level "may board a transport" bit `PathFinder`'s `calc_cost` also
  reads. All 364 `UNIT` records in `ron-data/unitrules.xml` have it clear, so pass 2
  degenerates to "containers versus everyone else" on the shipped data set. Implemented
  anyway, because a mod can set it.
* **`army < 0` forbids the split.** `Group::clear` writes `army = -1`, so transient
  selections split and real armies never do.
* **The terrain expression is the same one `Group::compute_form` `0x00707C80` uses at
  `0x00707DAF..0x00707DD4`** for its water flag: `(word[map+0x138][ div3[y>>6] * map[+0x18] +
  div3[x>>6] ] & 0x30) == 0x20`. `Fleet::formation_water_destination` already answers it.
* **Termination.** Each iteration performs at most one `Group::add`, so `|A| + |B| <= num`;
  a split requires both non-empty, so each half is strictly smaller than the receiver and
  the recursion terminates.

### `Group::add(o, who, 0, 0)` `0x00714350`, as this prelude calls it

```text
if (Group::get_num() == 0) this->who = who;            ; the param_4 == 0 arm
if (num != 0 && who != this->who) return;              ; one owner per group
if (objects[who][o]->is_unit() && !unit->is_captain())
    return this->add(unit->o_up, who, 0, 0);           ; substitute the captain
this->disband = 0;
b = objects[who][o]->is_build();
if (num != 0 && buildings != b) return;                ; one band per group
if (member(o) || num >= 0x80) return;
append; buildings = b; stamp = Game::frame; role |= type->role;
if (unit->o_down >= 0 && alive(o_down)) this->add(o_down, who, 1, 0);
if (id >= 0) compute_speed();
```

`UnitData::is_captain` is the sign bit of the `short` at `+0x8E` (`o_up`), so "is a
captain" and "who is my captain" are one field. `+0x90` (`o_down`) is the subordinate
link, and its re-add takes `param_3 = 1`, which turns `Group::add` into a `Group::kill`
for a captain — neither arm is recovered, so `group_move_near_split` refuses (answers
`Unanswered`) rather than guessing when a member reports a subordinate.

`crates/don-sim/src/systems/groups_guys.rs::GroupData::add` is missing the unconditional
`disband = 0` and the `buildings == is_build` homogeneity gate. Recorded here rather than
edited, because that file is shared.

## 4. The `buildings` refusal (`0x00704E59`) — ported

```text
00704E59  cmp byte ptr [esi + 0x49], 0
00704E5D  jne 0x706D72          ; straight to the epilogue
```

A building selection installs **nothing**. The bridge previously installed a `MOVE_TO` on
every member of one.

Stated divergence: retail's split physically precedes this gate, and for a building
selection its member loop indexes the *unit* pointer array (`[0x00C0AEC0 + who*0x1C]`) with
building object ids, which is out of that array's band. The port refuses before the split
instead. Both produce the same order queues — a building group's two halves would each hit
their own `buildings` gate — but retail additionally churns `Groups` slots and object
`group` back-pointers. Settling that needs a live probe, not more disassembly.

## 5. There is no `Group::normalize` in this body — corrected

`crates/don-sim/src/command.rs::normalize_for_action` was documented as "the
`Group::normalize` virtual call at `0x0070724D`" and was called from `action_move_near` and
`action_form`. Three independent checks say it is not there:

1. the full 9,205-byte body contains **zero** `call 0x711540`;
2. the `Group` vtable `0xB47C34` has exactly six slots — `+0x00` scalar-zero, `+0x04`
   `Group::get_num`, `+0x08` `Group::get_num_cap`, `+0x0C` `Group::add`, `+0x10`
   `Group::kill`, `+0x14` `Group::action_begin` — and `+0x18` onward is already vbtable
   data (`0xFFFFF634, 4, 8, …`), so there is no virtual route to `normalize` either;
3. `0x0070724D` is `mov eax, [ebx]`, the vptr load for the `call [eax + 0x14]` at
   `0x00707251` inside `Group::action_form` `0x00707220` — i.e. `Group::action_begin`
   `0x00714100`, whose entire body is `mov [ecx+0x28], 0` (`disband = 0`).
   `Group::action_attack` `0x00712490` likewise has no `call 0x711540`.

So the port was pruning dead / leaves-groups / not-our-unit members in the two hottest
movement commands where retail prunes none. `normalize_for_action` is removed;
`GroupData::normalize` itself stays, because `Group::normalize` is real and has 22 genuine
call sites elsewhere.

`CommandPackage::process_group` `0x0094A0C0` also runs `Group::clear(-1)` `0x00713E80`
over its stack `Group` at `0x0094A0FF` before adding anything. `GroupData::default()` alone
left `army = 0` and `form = 0` where `clear` writes `-1` to both, which made **every**
pushed selection look like an army to the split. Fixed in `process_group`.

## 6. The leader gates (`0x00705067..0x007050D9`) — ported

`0x00704E66` splits on `queued`. `QUEUE_FIRST` (0) takes the insert dance —
`Group::set_up_insert` `0x0070E520` at `0x00704EB5`, its own `find_leader` at `0x00704EA8`,
`Group::action_halt` `0x0070D0C0` at `0x00704EF4`, a recursive call with `queued =
QUEUE_NEW` at `0x00704FBF`, a restore of the leader's `unit_masks & 0x100`, then
`Group::finish_insert` `0x0070E620` at `0x00704F48` — which the bridge already models as
`with_queue_first`. Everything else jumps to `0x00704F6F`, and the main path re-gates:

```text
00705067  if (this->buildings != 0) return               ; a second, main-path-only test
00705071  if (num <= 0) return
0070507F  leader = GroupData::find_leader(0)             ; 0x0070CCB0
00705088  if (leader < 0) return
007050AD  devirt: vf(+0xC0) == UnitData::is_plane 0x0046CE40 ?
007050B7      if (ptype->domain != 2) fall through
007050C0      if (!(ptype->unit_flags & 0x20)) return    ; air and not a helicopter
007050D1  else if (vf(+0xC0)() != 0) return
```

**A leaderless selection and a plane-led selection each install nothing.** Air movement
belongs to `Group::action_flight`, not this body. Both refusals are now in the bridge;
neither was.

## 7. The rest of the body — mapped, not ported

Named here so the next lane does not re-derive the call graph. Addresses are call sites
read out of the disassembly. None of it is implemented by this lane and none of it is
guessed at.

| at | what |
|---|---|
| `0x00704F75` | the static `SimpleArray<int>` at `0x00EE1548` is resized to `num` (data `0x00EE155C`, capacity `0x00EE1554`) — the per-member index permutation the disembark tail indexes with |
| `0x007050EB` | `form` resolution: `form == 9 \|\| form == -1` → `GroupData::get_form` `0x0070B9F0`, clamped at 0, then stored to `GroupData::form` at `0x007050FA` |
| `0x0070510B` | `width == -1` → `GroupData::get_form_mod_option` `0x0070BD00` |
| `0x00705123` / `0x00705133` | origin: `queued == QUEUE_LAST` → `GroupData::get_loc_to` `0x0070C5D0`, else `GroupData::get_loc` `0x0070E030` |
| `0x0070513C` | the army arm: `!(leader_flags[who] & 4)` (`0x00E3A390 + who*0x6EEC`) and `army >= 0` arm it; `armies[who][army]->+0x2C != 0` then runs `ObjectsData::find_city(x, y, 1, who, who, 0x200, 0, 0, 0)` `0x0065BA90` at `0x0070518E` and remembers the city |
| `0x007051D0` | `queued == QUEUE_NEW` pre-pass: `Unit::clear_orders` `0x005E3860` per member, with a `LinkListBase<UnitOrder*>` tail splice for a packing unit (`ptype->unit_flags2 & 4`, `UnitData::is_packing` `0x0060AA60`, `path.length > 0`) |
| `0x007053EC` | `Group::compute_form` `0x00707C80` → the `Form*` whose `to_x`/`to_y` are `+0x514`/`+0x714` and whose leader slot index is `+0x34` |
| `0x00705410` | `GroupData::disband = 0`; for `QUEUE_LAST`/`QUEUE_NEW` also `o_angle`, `ox`, `oy` |
| `0x00705440` | the member loop — see below. `Unit::add_garrison_order` `0x005E4080` at `0x007055E4`, `Unit::add_move_order` `0x00616ED0` at `0x007056F4`, `Unit::add_group_move_order` `0x005E4710` at `0x00705F57`, `Unit::add_move_facing_order` `0x005E55C0` at `0x00705FC7` |
| `0x00706010` | `Group::update_positions` `0x00713810` |
| `0x00706060` | the disembark / path-scatter tail — see below. `PathFinder::find_wpath` `0x00688FC0` at `0x0070619A`, `0x007061FF` and `0x00706C3C` |
| `0x00706D5C` | `GroupData::order_num++`, then `memset(form + 0x30, 0, 0xE60)` at `0x00706D6A` |

**The member loop** per member: skip planes; the army arm (`UnitData::is_supply` /
`UnitTypeData::is_siege` / `UnitData::is_hero` select `UnitData::can_garrison` `0x006099A0`
→ `Unit::add_garrison_order` `0x005E4080`, else `UnitType::find_nearby_spot` `0x0061DE70`
next to the city → `Unit::add_move_order` `0x00616ED0`); otherwise write `UnitData::form`
`+0xAA` (unless the type class word `ptype+0x04` is `0x32..0x35`) and `+0xAB`; remember the
captain's slot index; clamp `Form::to_x/to_y[i]` to the map; for `i > 0` compare
`WorldData::get_tregion` `0x006B52E0` of the member's cell against the leader's and run
either the same-region `UnitData::invalid_loc` `0x00607C30` retry or the cross-region
`find_nearby_spot` relocation; then install `Unit::add_group_move_order` `0x005E4710` (the
GROUP_MOVE arm) or `Unit::add_move_facing_order` `0x005E55C0`; finally clear
`unit_masks & 0x400`.

**The disembark tail** uses a global `Stack<PathData>` — data `0x00EE1538`, count
`0x00EE1540`, capacity `0x00EE153C`, element size byte `0x00EE1544`. It seeds the stack with
the leader's formation destination, runs `PathFinder::find_wpath` `0x00688FC0` (with
`[0x00E85EB0] = 1` around it when the receiver is an army), and then either records a
"straight enough" flag when `vector_dist < 0x900`, or replays the path onto every member
with a per-member offset, a per-node region check, a second `0x600` distance cut, and a
`Stack<PathData>::invert` `0x0046D9A0` per member.

`0x00CC02F8` (scenario `ignore_orders`), `0x00C06188` (world), `0x00C0618C` (objects),
`0x00C0AEC0` (units), `0x00C09710` (armies), `0x00C09970` (builds), `0x00CAE5FC`
(`div_3_table`), `0x00E85F20` (`Groups::list`) are the globals it reads.

## 8. What is now true of the port

| behaviour | before | after |
|---|---|---|
| `Group::normalize` prune in `move_near` / `move_to` / `form` | performed | removed (retail performs none) |
| selection split into land/sea and transport halves | absent | ported, fail-open on an unanswered `get_inside` |
| embarked passenger → its transport | absent | ported |
| `buildings != 0` | installed `MOVE_TO` on every member | installs nothing |
| leaderless selection | installed | installs nothing |
| plane-led selection | installed | installs nothing |
| selection `army` / `form` after opcode 0 | `0` / `0` | `-1` / `-1`, per `Group::clear(-1)` |

`closure/group: move_near` stays **`Port::Orders`**. §7 is the reason: the army/city
garrison arm, the `invalid_loc`/`find_nearby_spot` relocation, and the whole disembark tail
are unported, and `Group::compute_form`'s Wedge/Square/Mob shapes remain refused. Flipping
the row would be tier inflation.
