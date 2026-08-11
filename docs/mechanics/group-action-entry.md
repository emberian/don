# The movement/attack `Group::action_*` entry prefix

Lane `op-move`, megaswarm wave 2. 2026-08-11. **Tier C** — every number here is `[measured]`
by capstone disassembly of `ron-bin/riseofnations.exe`, named from `ron-bin/sbl/rise.pdb`.
Nothing has been executed against retail.

Covers the nine wire opcodes of the movement/attack family: `FormCommand` (3),
`AttackCommand` (4), `SiegeAttackCommand` (5), `SwarmAroundCommand` (6), `MoveToCommand` (7),
`MoveNearCommand` (8), `AttackGroundCommand` (9), `PatrolCommand` (10), `LaunchPatrolCommand`
(11). `MoveToCommand` and `AttackCommand` alone are 9,178 and 5,715 of the corpus's
5,055,253 commands, so their gates are replay-agreement work, not coverage work.

## 1. Every installed formation destination was 1/48 of its intended Coord

The largest single defect found. `Group::action_move_near`'s member loop does **not** pass a
Coord to its order constructors — it passes a UCoord cell index:

```text
00705F76  mov  eax, [edx + edi*4 + 0x714]   ; Form::to_y[i]
00705F80  mov  ecx, [edx + edi*4 + 0x514]   ; Form::to_x[i]
00705F92  mov  edi, [0xCAE5FC]              ; the divide-by-three table
00705F9D  sar  eax, 4
00705FA6  sar  ecx, 4
00705FAA  push dword ptr [edi + eax*4]      ; div3[to_y >> 4]
00705FB1  push dword ptr [edi + ecx*4]      ; div3[to_x >> 4]
00705FC7  call 0x005E55C0                   ; Unit::add_move_facing_order
```

and the GROUP_MOVE arm at `0x00705F57` reaches `Unit::add_group_move_order` `0x005E4710`
with the same pair. Both constructors then store `arg * 0x30 + 0x18` — 48 Coord per cell
plus a 24-unit centre — into the order's `x`/`y` **and** `dest_x`/`dest_y`:

```c
/* 005e55c0.c:72  and  005e4710.c:26 */
iVar5 = param_1 * 0x30 + 0x18;   piVar4[1]   = iVar5;   /* +0x04 x      */
iVar3 = param_2 * 0x30 + 0x18;   piVar4[2]   = iVar3;   /* +0x08 y      */
                                 piVar4[0xb] = iVar5;   /* +0x2C dest_x */
                                 piVar4[0xc] = iVar3;   /* +0x30 dest_y */
```

`Unit::add_move_order` `0x00616ED0` reaches the same callee after converting Coord to UCoord
with the same table, so the round trip is Coord → UCoord → Coord. `orig_x`/`orig_y` are
*separate* arguments (`param_9`/`param_10` and `param_15`/`param_16`) copied verbatim, which
is why they stay the raw commanded Coord.

`crates/don-sim/src/command.rs::action_move_near` stored the bare cell index. Three
independent confirmations that the centring is the right rule:

* the two decompiled constructors above;
* the call sites, which show the `sar 4` + table lookup and nothing else;
* `order_dispatch::install_follow_move`, which already applied
  `x * UCELL + UCELL / 2` for `Unit::do_follow` — the same crate already knew the rule and
  only the `action_move_near` arm skipped it.

`group_action_entry::formation_order_destination` is the round trip;
`groups_guys::formation_order_coord` remains correct as the *argument* conversion and is
still what `follow_executor` wants when it names a call rather than a stored order.

## 2. The measured entry prefix, per action

Every body in the family opens with a short fixed gate sequence before it touches an order
queue. The order differs between actions and is load-bearing.

| action | VA | entry sequence, in emitted order |
|---|---|---|
| `move_to` | `0x0070FBA0` | none of its own — 49 bytes of argument re-push with `tolerance = 0`, then a tail call into `move_near` |
| `move_near` | `0x00704990` | on-map → prune → `num > 0` → `action_begin` → clamp |
| `attack` | `0x00712490` | on-map → `ox >= 0` → prune → `num > 0` → `action_begin` |
| `form` | `0x00707220` | on-map → `action_begin` → `buildings == 0` → `num > 0` |
| `siege_attack` | `0x00706FF0` | `buildings == 0` → `ox >= 0` → prune → `num > 0` |
| `swarm_around` | `0x0070FBE0` | on-map → prune → `action_begin` |
| `patrol` | `0x007030C0` | prune → `action_begin` → clamp → `buildings == 0` → `form = -1` → `num > 0` |
| `launch_patrol` | `0x00703580` | prune → `num > 0` |
| `attack_ground` | `0x00704520` | `action_begin` → prune → `num != 0` → clamp |

* **on-map** is `GroupData::is_on_map` `0x0070C450`: `num != 0`, then true for a building
  selection, else true for any member that is alive (`+0x08`), a captain (`+0xE8`) and on the
  map (`+0xBC`).
* **prune** is the scenario `ignore_orders` prelude — see §3.
* **`action_begin`** is the vtable `+0x14` call. The `Group` vtable is `0xB47C34`; slot
  `+0x14` is `Group::action_begin` `0x00714100`, whose whole body is
  `mov [ecx + 0x28], 0; ret`, i.e. `GroupData::disband = 0`.
* **clamp** is `if (v < 0) v = 0; if (v >= dim * 0x300) v = dim * 0x300 - 1;` per axis, with
  `dim` read from `[[0x00C06188]]` / `[[0x00C06188] + 4]` (map size in tiles).

`attack_ground`'s `action_begin` runs *before* the prune; `patrol` runs the prune first;
`form` never runs the prune at all; `siege_attack` never calls `action_begin`. A single
"shared prefix" that ignored those differences would be wrong four ways.

Effects from gates that ran before a refusing gate still stand — `form` clears `disband`
and *then* refuses a building selection.

## 3. The scenario `ignore_orders` prelude

```text
if (ignore_orders /* 0x00CC02F8 */ != 0 && this->who < 8)
    for (i = 0; i < ignoring[who].count /* 0x00ED6574 + who*0x1C */; i++) {
        int obj = ignoring[who].list /* 0x00ED6580 + who*0x1C */ [i];
        if (obj >= 0) this->vtable[0x10](obj, who, 0, 0);   /* Group::kill 0x00714110 */
    }
```

Its complete recovered planner already exists as
`systems::groups_guys::plan_ignore_order_kills`; `action_halt`, `action_disband` and RECALL
already cross it. The nine actions here did not. `ScenarioFuncSet::init` zeroes the scalar
at `0x00A04084` and only a scenario trigger sets it, so in every non-scenario match — which
is the whole replay corpus — the prelude is a **measured no-op**. The bridge therefore runs
the family unchanged when the scalar is clear, and fails closed (installs nothing, mutates
nothing) when it is set and the host has not committed the prune.

## 4. What the bridge did not do before, and does now

`Group::action_attack` had **no** entry gates in the port: a command addressed at a wholly
off-map selection, at a negative object id, or at an empty group still installed `ATTACK` on
every member. `attack_ground`, `patrol` and `launch_patrol` were likewise ungated and
unclamped, and none of the four ran `action_begin`.

## 5. Corrections to existing artifacts

* **`crates/don-sim/src/command.rs::normalize_for_action`** is documented as "the
  `Group::normalize` virtual call at `0x0070724D`". `0x0070724D` is the `mov eax, [ebx]` that
  loads the vptr for the `call [eax + 0x14]` at `0x00707251`, i.e. `Group::action_begin`
  `0x00714100` — eight bytes whose entire body is `disband = 0`. `Group::normalize`
  `0x00711540` is a different procedure; a scan of every direct `call`/`jmp` in `.text`
  gives it 22 call sites and **none** of them is in `action_form`, `action_move_near` or
  `action_attack`. So the port prunes members (dead / leaves-groups / not-our-unit) where
  retail prunes none, in the two hottest movement commands. Not changed here: removing the
  prune changes what those two actions do to a group and wants its own claimed row. This
  lane supplies the `disband = 0` that retail actually performs at that call site.

* **`docs/assembly/command-bridge.md`** "the ~44 other `Group::action_*`" correction stands,
  and its `move_to` row is confirmed exactly: the 49-byte body pushes `[ebp+0x2C]` down to
  `[ebp+0x10]`, then a literal `0` for `tolerance`, then `[ebp+0xC]` and `[ebp+8]`, and tail
  calls `0x00704990`.

## 6. What this lane deliberately did not write

The nine rows stay red, and the reason is one function. `Port::Complete` requires the whole
simulation-side effect, and the dependency graph makes `move_near` the root:
`move_to → move_near`; `form`, `attack` and `swarm_around` → `halt`, `move_to`;
`siege_attack` → `attack`, `guard`; `attack_ground` → `air_attack_ground`;
`patrol` → `air_patrol`.

* **`Group::action_move_near` `0x00704990` (9,205 B) has no decompilation** in
  `re/decomp-all/` — it is one of the largest uncited bodies in the command layer. Wedge /
  Square / Mob layout, subordinate (non-captain) sorting, the garrison-into-transport branch
  and the disembark executor all live inside it.
* **`action_attack`'s reach split.** `0x00712490` calls `Unit::find_attack_pos` `0x00601280`
  once at `0x007128A5`, `Group::action_move_to` at `0x007129FA`, `Unit::add_cast_order`
  `0x005E4A60` at `0x00712F1B` and `0x007130A8`, and `Unit::add_move_order` `0x00616ED0` at
  `0x00712F75`, `0x007131B8` and `0x0071322E`: reachable members get `ATTACK`, casters get
  `CAST_SPELL`, the rest are sent at the target. The port still installs `ATTACK` on
  everyone. `systems/attack_position.rs` has the positioning arms; the *split* is the gap.
* **`action_attack_ground`'s territory gate.** `0x00704520` reads the terrain owner byte at
  `[map+0x134] + 0xF + cell*0x1C` for the clamped destination and, when that owner is valid,
  not the commander, and `LeaderData::is_peace` `0x006E1200` holds, **abandons the command**
  after a local message. That is a simulation-visible refusal and it needs a terrain-owner
  host this bridge does not have.
* **`action_patrol`'s and `action_attack_ground`'s per-member type predicates**
  (`UnitTypeData::can_attack_ground` `0x0061DB20`, `UnitData::is_busy` `0x0060A370`,
  `ObjectData::can_carry` `0x00646C40`, the `+0x2B4 & 0x20` helicopter bit) and the
  contained-unit walk that `attack_ground` and `launch_patrol` both perform over
  `ObjectData::inside_down` `+0x28`.
* **`Group::action_siege_attack` `0x00706FF0` is fully read** — `buildings`/`ox` gates,
  prune, a stack-local `Group` cleared by `Group::clear(-1)` `0x00713E80`, a reverse member
  scan moving every member passing `alive && +0xBC && type->+0x10C` out of the receiver
  (`Group::kill`) and into the temp (`Group::add` `0x00714350`), then
  `temp.action_attack(ox, whom, 1, queued, 0)` and `this.action_guard(temp_leader, who,
  queued, 0)`, with a plain `this.action_attack(...)` when the temp is empty or leaderless.
  It is not written because it is a *delegating* body: it cannot be more complete than
  `attack` and `guard`, which are both `Port::Orders`.
