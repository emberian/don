# The Arena's declaration command and `Leader::set_diplo`'s side-effect channel

This note records what the Arena can now execute of retail's runtime diplomacy transaction,
what it deliberately refuses, and what still stops it. It targets the registered blocker
`arena-diplomacy-model`; **the blocker is not closed by this tranche.**

Fidelity: **Tier C throughout.** Everything below comes from the shipped PDB layouts
(`schema/types.json`), the decompiled bodies named in each row (`re/decomp-all/<va>.c`), and
the already-recovered `don-sim` modules those bodies live in. Nothing here is differentially
tested against retail and nothing is verified.

## The finding that made this lane cheap

`crates/don-sim/src/systems/leader_set_diplo.rs` — 938 lines, the complete atomic
`Leader::set_diplo` (`0x006EC6A0`) transaction planner, its receipt contract, and the
`action_respond(..., 1)` resource movement opcode 41 reaches — **had no `mod` declaration
anywhere in the `don-sim` library.** It was not in `systems/mod.rs`, and unlike
`diplomacy_command_plans.rs` it was not `#[path]`-mounted from `command.rs` either. It
compiled only from `crates/don-sim/tests/leader_set_diplo_transaction.rs`, so no consumer
could reach the one authority both diplomacy blockers name. One line in `systems/mod.rs`
fixed that. `leaders_diplomacy_opening_frontier.rs` (438 lines) is still in the same state
and was left alone by this lane.

## Where the code lives

| piece | file |
|---|---|
| the `Leader::set_diplo` transaction | `crates/don-sim/src/systems/leader_set_diplo.rs` (registered, called not copied) |
| the `Leader::init` opening loop | `crates/don-sim/src/systems/leader_init_diplomacy_loop.rs` (unchanged) |
| the mutual-minimum `diplos` table | `crates/don-ai/src/arena/retail_systems.rs::DiplomacyState` (unchanged) |
| the Arena host and command | `crates/don-ai/src/arena/diplomacy_runtime.rs` |
| the live entry points | `crates/don-ai/src/arena/world.rs::{submit_player, declare, visibility_policy}` |
| tests | `crates/don-ai/tests/arena_diplomacy_runtime.rs` (18) |

## The transaction, in retail's own order

`Leader::set_diplo(int whom, int state)` reads `ECX` as the acting leader.
`re/decomp-all/006ec6a0.c` is the whole body; every step below is a
`don_sim::systems::leader_set_diplo::SetDiploStep`.

| # | retail step | evidence | Arena |
|---|---|---|---|
| 1 | equal raw declarations return before every side effect | `0x006EC6BB` | executed |
| 2 | on `old == ALLY`: clear both `LeaderData::ally_mask` (`+0x6929`) bits | `0x006EC6D2`, `0x006EC6EF` | executed |
| 3 | `eject_my_shit_from_his_ass(other)` for both parties | `0x006D0220`, called twice | **refused** — see below |
| 4 | presentation: `get_scary_console_leader()->has_treaty(who, 1)` on both parties, then a chat line and sound `0x135` | `0x005833E0`, `0x006E11E0`, `0x0097F770` | reached and provably empty — see below |
| 5 | write the actor's own `diplos[target]`, then the global target row | `0x006EC94E`, `0x006EC95B` | executed, in that order |
| 6 | on `state == ALLY`: grant shared vision to each side when `has_preq(ALLY_LOS)` or `GameInfo::reveal_map >= 1` | `0x006DB810` with `0x2B0`, `[0x00C061EC]+0x30` | executed; the prerequisite is a fail-closed bit, see residual 3 |
| 7 | on `state == ALLY`: scan every other leader with `leader_flags & 3 == 3` that is allied to neither party | `0x006EC8F0..0x006EC944` | executed |
| 8 | if that count is zero: `Leader::victory(0, 0)` and `Game::semaphore.set(22)` | `0x006EC9B0`, `0x00450360(0x16, 1)` | **refused** — the headline residual |
| 9 | `Leader::force_army_process(actor)` for every valid army | `0x006F30F0` | not reached; the Arena materializes no `Army` |
| 10 | `IFaceData+0x22A = 1` | `0x006EC9A2` | recorded; presentation |

The Arena's applied receipt is checked by `don-sim`'s own
`SetDiploReceipt::validates`, which re-plans from the same before-image and requires every
authority call the plan emitted to have been acknowledged. The Arena does not grade its own
homework.

## Two corrections this lane landed

### 1. `FogLeader::player_mask` is `ally_mask`, not a live `is_ally` recomputation

`World::visibility_policy` built each viewer's fog `player_mask` by folding
`DiplomacyState::is_ally` over the other players every fog frame. `borders_fog.rs` names
that field's source exactly: **`LeaderData +0x6929`**, the `ally_mask` byte. Retail writes it
in `Leader::init`'s eight-target loop and then only in `Leader::set_diplo`, and there only
behind `has_preq(ALLY_LOS)` or `reveal_map >= 1`. Deriving it from the relation instead
grants allied shared vision *unconditionally* — invisible while a match stays at war (the
mask is `1 << who` either way, which is why this change regresses nothing today) and wrong
the moment a declaration lands. `World` now owns the byte and the fog reads it.

### 2. The opening `treaties` row is not all-zero

`ArenaLeaderDiplomacyRow::opening` first shipped with `treaties = [0; 8]`. The test that
pins the opening against the registered `Leader::init` loop caught it: the loop's
`treaties[t] |= 1` gate is `is_team(who, t, 0)`, and `LeaderData::is_team` `0x006EBD39`
returns true for `t == LeaderData::who` **before it reads any team byte**. So the self cell
opens at 1 and every other cell at `base_treaty = (reveal_map == 3)`, which is 0 here.

That is also what makes step 4 above provably empty rather than assumed empty: the presentation
branch is gated on `console_leader.treaties[actor] & 1`, and the only cell that is ever set is
the console leader's own — which is reachable only when `actor == console_who`, exactly the
case the branch's own guard already excludes.

## What the Arena declares rather than derives

Two values are *settings*, not derivations, and are named as such:

- **`Console::who` = slot 0.** Retail always has a display client, and
  `get_scary_console_leader` `0x005833E0` indexes `Leaders` with `Console::who` unguarded —
  a headless `-1` is an out-of-bounds read, not a neutral value. The Arena names a slot.
- **`GameInfo::reveal_map` = 0.** `World::visibility_policy` runs `Fog::option`'s default,
  which is `FogOption(0)`, so `Game+0x30` is 0 and shared vision has no fallback.

## What still stops it

The blocker stays open on six residuals. None is a missing number; each is a missing host.

1. **`Leader::victory` `0x006EC9B0` has no Arena host.** In a two-player match an alliance
   leaves no independent active leader, so retail calls `Leader::victory(0, 0)` — meaning
   **every alliance in a 1v1 is an immediate shared victory**, not a diplomatic state.
   `don_sim::systems::victory_score::Leaders::victory` implements the state part completely,
   but it needs a `Leaders`/`Match` owner and the Arena has neither; `World::check_defeat` is
   a separate elimination model. The Arena refuses at plan time, so the world is left
   byte-identical. With a third independent active leader the same alliance commits — that
   asymmetry is tested both ways.
2. **`Leader::diplomacy` `0x006BC950` (20,348 B) is not ported** and is recorded in
   `tick.rs` as deliberately replaced by a self-play agent. No Arena bot issues a
   declaration, so the channel exists with no policy above it.
3. **`LeaderData::has_preq` `0x006DB810` is not hosted.** `ally_los` is an explicit
   per-leader bit that nothing in the Arena grants, because the Arena materializes no
   BonusType/effect runtime. `PlayerState::techs` holds `TechType` indices only — the same
   gap `arena-attrition-model` recorded for its two leader scalars.
4. **`GameInfo::team_style` is undeclared.** The Arena's all-war opening matches
   `Leader::init` under any locked team style (`teams_locked` is
   `!matches!(team_style, 0 | 8 | 11)`) with `rush_rules == 0`, or under
   `check_victory_mode`; it does **not** match team style 0, which opens at peace. The Arena
   states no team style, so its opening is consistent with retail rather than derived from
   it, and the pin test says which options it assumed.
5. **`leader_flags2` is reported as 0.** `Leader::set_diplo` reads `& 0x0A`, and retail sets
   `UNIT_AI_OFF` in `Leader::defeat` `0x006ECB00` — which the Arena's defeat model does not
   run. The difference reaches only the army gate, whose `valid_armies` are all clear, so it
   is not observable today. It is still a difference.
6. **The rest of the diplomacy command family is unhosted**: opcode 37 `TreatyCommand`, 41
   `AcceptCommand`, 43 `TributeCommand`, 44 `DemandTributeCommand`, 45
   `ProposeAttackCommand`. `treaties` therefore never changes after `Leader::init`, and
   `leader_set_diplo`'s own `action_respond(..., 1)` resource-movement half — now reachable
   for the first time — has no caller.

`eject_my_shit_from_his_ass` `0x006D0220` is listed above as refused rather than as a
residual, because the Arena never reaches it and reaches it for a derived reason: nothing in
the Arena sets `ObjectData::get_inside` (`can_transport` and `can_board_transport` are false
for every type), so the sweep walks the owner's whole object list and selects nothing. A test
plants a contained object anyway and asserts the refusal names `0x006D0220`, so a future
transport runtime cannot silently inherit "no ejection happens".
