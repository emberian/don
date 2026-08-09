# Tech Race victory

## Executable tranche

`systems::tech_race::process_tech_race_gain` implements the deterministic Tech Race
tail of `Leader::gain_tech` (`0x006DE847..0x006DE997`). Tech Race is deliberately absent
from `GameDaemon::process_victory`: retail resolves it synchronously in the research
completion transaction, after the gaining leader's encrypted counters have changed.

The exact branches are:

| gate | completion predicate | retail action |
|---|---|---|
| victory = 9 and `Game::semaphore` bit 17 clear | decoded `ages == GameInfo::ending_technology` | `Leader::victory(BY_TECH_RACE, 0)` |
| victory = 9 and bit 17 set and gained type `is_epoch_type()` | decoded `epochs == 28` | `Leader::victory(BY_TECH_RACE, 0)` |

Both comparisons are equality, not saturation. The second branch's virtual call at vtable
`+0x38` is `TypeData::is_epoch_type` (`0x00470870`), confirmed from the retail
`TypeData` vtable at `0x00B43E48`. The 28-tech goal is both the literal `0x1C` in the
instruction stream and `END_EPOCHTYPES - BASE_EPOCHTYPES` (`579 - 551`).

The module calls the existing `Leaders::victory` transaction instead of restating it.
Consequently a Tech Race win immediately:

1. wins every mutual ally with `VictoryType::ByTechRace`;
2. defeats every other active player with `DefeatType::Victory`;
3. zeros each terminal leader's aggregate `num_queued` table and accumulates their owner
   bits for concrete `Build::clean_queue(0)` traversal;
4. reaches `Game::check_victory` through `Leader::defeat`, setting `GAME_OVER` and
   `VICTORY_RESOLVED` when only the winning alliance remains.

## Presentation boundary

In the all-epochs form, an announced epoch gained by a non-local player constructs a
localized opponent-progress message before the count check (`0x006DE8C8..0x006DE971`).
The port returns `TechRacePresentation::OpponentEpochGained { who, type_index }`. It does
not construct strings or require a UI, and suppressing the presentation cannot suppress
the victory mutation.

## Live step-14 integration

`production_runtime::SimFinishedHost::apply_tech_mutation` invokes the executor on
`CompleteGainBeforeAutoUnlockEffects`. That callback is after the live `TechState` counter,
bit, special-effect and resource cohorts but before the auto-unlock sweeps, matching Tech
Race's retail position at `0x006DE847` before the first generic auto-unlock at
`0x006DEBBE`. `Build::finished` supplies literal `1` for both integer tail arguments at
`0x0062852C..0x00628548`; the first of those is the announcement gate read at
`Leader::gain_tech`'s rebased `[ebp+0x68]`, and the adapter preserves that value.

The call reads `MatchOptions::ending_technology`, the gained owner/type, the production
runtime's local player, and the shared live `Leaders` / `Match`. Typed opponent notices are
retained in `LiveProductionRuntime::tech_race_presentations`.

The active producer is temporarily moved out of `Sim::builds` while its routed queue
transaction executes. A resolving Tech Race call therefore captures the terminal cleanup
owner mask immediately, blocks any saved outer parallel-slot completion, restores the
producer row, and applies `clean_terminal_build_queues` to every captured owner before
returning from `process_sim_build_queue`. The completed winning slot performs its ordinary
no-refund unqueue first; the same-transaction cleanup removes every remaining slot and
clears `REPEAT_QUEUE`. No terminal queue survives into another step-14 object visit.

Mutation-sensitive source tests are:

* `tech_race_completion_resolves_teams_and_cleans_all_build_queues_in_step14` — a deep
  parallel slot wins for an alliance, defeats an enemy, prevents the saved outer research
  from completing, and cleans current/allied/enemy concrete queues and counters;
* `all_epochs_live_completion_preserves_typed_opponent_notice` — the 28th epoch takes the
  semaphore-17 branch and retains the typed non-local progress notice;
* `step14_research_completion_reaches_tech_race_and_cleans_before_return` — the full
  `Sim::do_frame` route reaches the production adapter and returns from the object visit
  with the winner/enemy terminal state and current concrete queue already settled.

This tranche was implemented under a token-only gate. The focused Cargo test and formatter
were intentionally not executed; those are the exact pending validation actions for the
two tests above and the existing `systems::tech_race` unit-test module.

The remaining rule uncertainty is only the engine's missing symbolic name for semaphore
bit 17; its position and both behaviors are measured. Setup/lobby ingestion must populate
`MatchOptions::ending_technology` from `GameInfo+0x2A`. Network/UI delivery of the typed
progress notice and defeated-player object razing remain separate presentation/object-store
boundaries.
