# leader-production-ai-step11 — the two AI children of `Leaders::strategy_all`

Tick step 11 already executed its 93-byte dispatcher and the whole
`Leader::check_explore` `0x006BC860` body (see `docs/assembly/leaders-step11.md`). Its two
remaining children, `Leader::plan_strategy` `0x006B9620` (11,108 B) and `Leader::diplomacy`
`0x006BC950` (20,348 B), were call-counted gaps: the tick charged one `Gap` per reached
call and nothing ran.

This document records what the binary says those two calls actually *do* on entry, and what
now executes in `crates/don-sim/src/systems/leader_production_ai.rs` and
`crates/don-sim/src/systems/leaders.rs`.

Everything here is **[measured]** from a radare2 disassembly of `ron-bin/riseofnations.exe`
cross-read against `re/decomp-all/` and the PDB layouts in `schema/pdb-types.json` /
`schema/symbols.json`. **Tier C**: structure and constants are read from the binary; nothing
has been executed against retail.

---

## 1. Step 11 is the only door into retail's production AI

`radare2 -A`, cross-reference query on each target:

| callee | call sites in the whole image |
|---|---|
| `Leader::plan_strategy` `0x006B9620` | **one** — `Leaders::strategy_all` `0x006ED452` |
| `Leader::diplomacy` `0x006BC950` | **one** — `Leaders::strategy_all` `0x006ED462` |
| `Leader::production_ai` `0x006C1960` | **one** — `Leader::plan_strategy` `0x006B9662` |

`Leader::production_ai` is in turn the only caller of `production_ai_setup`,
`found_cities`, `research_techs`, `upgrade_units`, `create_units`, `create_buildings` and
`make_stuff`. So the AI's entire build/research pipeline hangs off a single edge out of tick
step 11. Until step 11 runs the machine, those 173,424 bytes are unreachable, not merely
unported — which is the sharpest available statement of why this row is the AI pillar.

## 2. `Leader::plan_strategy` `0x006B9620`, entry and dispatch

```asm
0x006b9629  mov  eax, 0xc8                  ; 200
0x006b962e  mov  ecx, [0xc061c0]            ; GameAccess::ai_speed
0x006b9636  imul esi, dword [ebx + 8], 0x19 ; who * 25
0x006b963a  idiv dword [ecx]                ; period = 200 / ai_speed
0x006b9648  mov  edi, [edi + 0x550]         ; Game::frame
0x006b964e  add  esi, edi
0x006b9653  idiv ecx                        ; edx = (who*25 + frame) % period
0x006b9655  cmp  dword [ebx + 0x788], 0     ; LeaderData::production_step
0x006b965e  je   0x6b966e
0x006b9662  call 0x6c1960                   ; Leader::production_ai  -> RETURN
0x006b966e  test edi, edi ; je 0x6b96f9     ; frame == 0    -> planning body
0x006b9676  test esi, esi ; je 0x6b96f9     ; phase == 0    -> planning body
0x006b967a..0x006b9698                      ; esi % 30, jne 0x6bc0f7 -> RETURN
0x006b969e  mov  eax, [ebx + 0x6ed8]        ; MakeList::list
0x006b96a4  mov  esi, [eax]                 ; list[0].type
0x006b96a6  lea  eax, [esi - 0x220]
0x006b96ac  cmp  eax, 0x54 ; ja 0x6bc0f7    ; not a TypeIndex in [0x220,0x274] -> RETURN
0x006b96b9  call 0x6c9b90                   ; Leader::can_pay(0)
0x006b96c9  call 0x6e0c80                   ; LeaderData::has_tech(head)
0x006b96dd  call 0x6db510                   ; LeaderData::researching(head, -1, 0, 0)
0x006b96ed  call 0x6c94f0                   ; Leader::make_this(0)
```

Four things worth writing down.

**(a) The phase is not `check_explore`'s.** `Leader::check_explore` `0x006BC890` computes
`(who*25 + frame + 12) % (200/ai_speed)`; `plan_strategy` uses the same dividend **without
the `+ 12`** (`0x006B9650` loads `esi` straight into `eax`). The two children of one
dispatcher are 12 frames out of step with each other, and the previously ported constant
`EXPLORE_PHASE_BIAS = 12` belongs to `check_explore` alone.

**(b) A running production cycle bypasses the phase entirely.** The phase is computed
first, but `0x006B9655` tests `production_step` before anything consults it. So once the
cycle is started it advances **one stage per frame**, not once per period.

**(c) 193 calls in 200 return having touched nothing.** With `production_step == 0` and
`ai_speed == 1`, `phase = (who*25 + frame) % 200`. Only `phase == 0` enters the
11,108-byte body, and only `phase in {30, 60, 90, 120, 150, 180}` opens the fast lane;
every other frame returns at `0x006B9698`. Charging a coverage gap on those 193 frames was
over-reporting what retail ran, and `tick_step11_production_ai.rs::
the_planner_gap_is_charged_seven_frames_in_two_hundred` measures the corrected number
through `Sim::do_frame`.

**(d) The fast lane is an unsigned window test.** `lea`/`cmp 0x54`/`ja` accepts exactly
`0x220 ..= 0x274`; a negative head is far above the window, not below it. `Leader::can_pay`
`0x006C9B90` indexes the same array — `[this + 0x6ED8] + index * 40` — so `MakeObject` is 40
bytes and `can_pay(0)` / `make_this(0)` operate on the head the range test just accepted.
`MakeObject + 0x18` is the value the virtual cost query at `[type_vtable + 0x84]` is
compared against.

## 3. `Leader::production_ai` `0x006C1960`, the whole 628-byte step machine

Gate, in order, each of which falls through to `0x006C1B96` and **resets
`production_step` to 0** on the way out:

| test | VA | meaning |
|---|---|---|
| `leader_flags & 4` set **and** `& 8` clear | `0x006C1970` | human leader without the companion bit |
| `*GameAccess::ai_off != 0` | `0x006C197C` | the global AI kill switch |
| `leader_flags2 & 4` | `0x006C198A` | `disable_production_ai` |
| `(unsigned)(production_step - 1) > 10` | `0x006C19D8` | step outside the jump table |

Then, unconditionally, `effective_pop` (`+0x9E0`) `= Leader::queued_units() + control + 1`
(`0x006C1994`..`0x006C19A9`), and a pre-check: while `production_step == 1`, if
`prod_script_run == 0` **or** `byte [game + 0x2D] == 8`, the step is promoted to 2 —
i.e. the BHS script is skipped outright.

The dispatch is `jmp dword [ecx*4 + 0x6C1BA8]`, an 11-entry table indexed
`production_step - 1`:

| step | arm | writes | calls |
|---:|---|---|---|
| 1 | `0x006C19E8` | `script_step`, then 0 / `prod_script_run = 0` / `+1` | `RunTimeEnv::run_script` `0x0043D0E0` |
| 2 | `0x006C1AB6` | `= 3` | `production_ai_setup` `0x006C83E0`, `MakeList::clear` `0x006C9DB0` |
| 3 | `0x006C1AD6` | `= 4` | `found_cities` `0x006C7A60`, then the tail |
| 4 | `0x006C1B0C` | `= 5` | `research_techs` `0x006C6BA0`, then the tail |
| 5 | `0x006C1B1C` | `= 6` | `upgrade_units` `0x006C6430`, then the tail |
| 6 | `0x006C1B2C` | `= 7` | `create_units` `0x006C40A0`, then the tail |
| 7 | `0x006C1B3C` | `= 8` | `create_buildings` `0x006C1BE0`, then the tail |
| 8 | `0x006C1B4C` | `= 9`, then maybe `= 0` | `make_stuff` `0x006C8AF0` |
| 9 | `0x006C1B2C` | `= 10` | **the same arm as step 6** |
| 10 | `0x006C1B7A` | `= 11` | `create_buildings`; **no tail** |
| 11 | `0x006C1B8F` | `= 0` | `make_stuff`; **no pre-increment** |

The shared tail at `0x006C1AE4` is `if (byte [game + 0x2D] == 8) { make_stuff();
MakeList::clear(&make_list); }`. Step 8's arm does not use it; it branches on
`make_stuff`'s `int` return instead: non-zero keeps the cycle alive into steps 9..11, zero
ends it unless `byte [game + 0x2D] == 8`.

The step-1 arm builds four `ScriptInt`s (`ScriptInt::pop` `0x009D75D0`) — `{who + 1}`,
`{script_step, kind 3}`, `{pers + 2}`, `{5}` — hands them plus `prod_script` (`+0x6EA4`) to
`RunTimeEnv::run_script`, and then reads `RunTimeEnv::get_ret_int` `0x0047D9E0`:

* **1** → `production_step = 0` at `0x006C1A89`. This is `BLOCK_ON_THIS`, and it is why a
  player whose script never finishes executes stages 2..11 zero times.
* **3** → `prod_script_run = 0` at `0x006C1A9F`. This is `SCRIPT_DONE`, the one-way latch
  `docs/tracks/ron-ai-impl.md` §1(a) established. A non-zero `run_script` return lands on
  the *same* instruction, so a script that fails to run also retires itself.
* otherwise → `production_step += 1` at `0x006C1AA9`.

## 4. Two flag words, named from their writers

`docs/tracks/ron-ai-impl.md` described `+0x000` as "human bit / AI-subsystem disable bits"
and `+0x004` as `leader_flags2`. The shipped scenario-script API settles both, and it is a
*writer*-side derivation rather than a PDB name:

| function | VA | instruction |
|---|---|---|
| `ScenarioFuncSet::change_to_ai` | `0x009E5D20` | requires `leader_flags & 4`, then `and dword [leader_flags], 0xFFFFFFFB` |
| `ScenarioFuncSet::disable_production_ai` | `0x009FF7A0` | `or  dword [leader_flags2], 4` |
| `ScenarioFuncSet::enable_production_ai` | `0x009FF5E0` | `and dword [leader_flags2], ~4` |
| `ScenarioFuncSet::enable_combat_ai` | `0x009FF820` | `and dword [leader_flags2], ~8` |
| `ScenarioFuncSet::enable_all_unit_ai` | `0x009FF8A0` | `and dword [leader_flags2], ~2` |
| `ScenarioFuncSet::enable_city_ai` | `0x009FFBA0` | `and dword [leader_flags2], ~0x10`, and it refuses a leader with `leader_flags & 4` |

So `leader_flags & 4` is the **human** bit — the thing `change_to_ai` removes — and
`leader_flags2`'s low four bits are the four AI-subsystem kill switches, in the order
`unit / production / combat / city`. `Leader::production_ai`'s third gate is exactly
`disable_production_ai`'s bit.

## 5. `Leader::diplomacy` `0x006BC950`, the entry gate

```asm
0x006bc99a  test byte [eax + 0xe3a390], 4   ; leaders[this->who].leader_flags & 4
0x006bc9a1  jne  0x6be99a                   ; -> plain return
0x006bc9ac  test byte [eax + 0x821], 2      ; Game::semaphore bit 9
0x006bc9b3  jne  0x6be99a
0x006bc9b9  mov  eax, [0xc061c4] ; cmp dword [eax], 0   ; GameAccess::ai_off
0x006bc9c1  jne  0x6be99a
```

Three facts, in that order, all of which `don-sim` already has:

1. the human bit, read off `leaders[this->who]` — the *indexed* record, not `this`;
2. **game semaphore bit 9**, which is the same `CHECK_VICTORY_MODE` bit that arms step 11's
   own `Game::check_victory` tail at `0x006ED47A`. A tick that runs the victory check
   therefore runs **no** diplomacy at all — the two are complementary halves of one bit;
3. the global `GameAccess::ai_off`, the cheat toggle `Game::action_cheat_ai_toggle`
   `0x005930C0` flips (itself gated on semaphore bit 2, `game[0x820] & 4`).

Everything past the gate — the eight-record ally and score survey, the target eligibility
tests, the power-of-two cadence and the first agenda mutation — was already recovered by an
earlier lane in `crates/don-sim/src/systems/leaders_diplomacy_opening_frontier.rs` and
written up in `docs/assembly/leaders-diplomacy-opening-frontier.md`. That
module had **no `mod` declaration anywhere in the library**; it compiled only from its own
test file, so step 11 could not reach the one derivation of its own child. It is now
declared, and `strategy_all` executes its gate through
`leaders::diplomacy_entry_gate`.

## 6. What runs now, and what is charged

`leaders::strategy_all` emits a `StrategyCall::PlanStrategy` / `StrategyCall::Diplomacy`
**only when retail entered code this port does not execute**. `StrategyTrace::plan` and
`StrategyTrace::diplomacy` record every reached call either way, and `Leaders::last_strategy`
keeps the last frame's trace so a consumer can read *why* a call was or was not charged.

Not charged, because retail provably touched nothing:

* `PlanArm::NotDue` — the `0x006B9698` sub-phase return;
* `PlanArm::TechQueueHeadRejected` — the `0x006B96AF` window return;
* `ProductionGate::{Human, AiOff, ProductionAiDisabled, StepOutsideTable}` — all four
  refusals return before the first call, and all four reset `production_step` to 0;
* `DiplomacyGate::{HumanOwner, CheckVictoryMode, AiOff}`.

Charged, with the boundary named:

* `PlanArm::PlanningBody` — `0x006B96F9`, the 11,108-byte body;
* `PlanArm::TechQueueReached` — `Leader::can_pay` `0x006C9B90` and the three calls behind it;
* every `Stage` the step machine reached (eight stage functions plus the BHS entry);
* every `Stall` — a fact the host did not answer, so a branch was not chosen;
* `DiplomacyGate::Entered` — `0x006BC96E` onward.

## 7. HOOK NEEDED — three tick-side wires this lane does not own

`crates/don-sim/src/tick.rs` is another lane's file. Three mirrors would make the machine
run on real match state instead of on state a test installs:

1. **`step8.ai_off` ← `command::InlineState::ai_off`.** `don-sim` already has the producer
   (inline op 64 `cheat_ai_toggle`, `command.rs:3837`); nothing mirrors it into `Leaders`.
   The field defaults to that producer's own default of `0`, so the current value is right
   until somebody uses the cheat.
2. **`step8.starting_resources` ← `vic_match.options.starting_resources`.** Same shipped
   option byte `victory_score::MatchOptions` already carries. Without it the step machine
   refuses every branch that compares `byte [game + 0x2D]` against 8 rather than choosing
   one, which is correct but inert.
3. **`Leader::diplomacy`'s opening cone should execute at the `StrategyCall::Diplomacy`
   boundary, not inside `strategy_all`.** `leaders_diplomacy_opening_frontier::
   execute_diplomacy_opening` needs `LeaderData::score` `+0x18`, which
   `victory_score::Leaders::compute_score` owns, and retail's order is
   `compute_score(0)` *then* `diplomacy` **per slot**. The tick currently builds the whole
   trace and only then replays `ComputeScore`, so running the cone inside `strategy_all`
   would feed it scores that are a full frame stale for every slot instead of fresh for
   slots `<= k`. The cone's own mutation (`agendas[target] &= ~4`) and its control flow do
   not read the survey, so the *state* would be right; the recorded survey would not be,
   and this lane refused to land a receipt it knows is misordered.

## 8. Deliberately not derived

* **No stage body.** All eight are between 210 and 9,405 bytes of AI policy.
* **No `MakeList`.** `[this + 0x6ED8]` is `ArrayBase<MakeObject>::list` and retail reads
  `list[0].type` **without consulting `length`**; with `length == 0` that is a stale or
  uninitialised read whose value nothing in the image establishes. It stays
  `Leader::ai.make_list_head: Option<i32>`.
* **No `Leader::queued_units` `0x006CE000`** (394 B). Without its answer `effective_pop` is
  left alone rather than written wrong.
* **No `Leader::can_pay` / `has_tech` / `researching` / `make_this`.** The fast lane's four
  calls are one boundary; the range test in front of them is executed.
* **No meaning for `leader_flags & 8`.** It forms half of `production_ai`'s first gate and
  no writer for it was located in this lane, so it is named
  `flags::PRODUCTION_DESPITE_HUMAN` for the gate it forms and nothing more.
* **`ai_speed == 0` is not modelled as a period.** Retail takes an `idiv` fault there; a
  fault is not a behaviour to reproduce, so it is `PlanArm::MissingPeriod`.
