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

## Frozen live seam

The executor is exported but is not yet called by `production_runtime`. The exact future
adapter belongs at successful `Leader::gain_tech` completion, after the live `TechState`
counter mutation and at the retail position represented by `0x006DE847`. It must supply:

* `GameInfo::ending_technology` (`GameInfo+0x2A`, `Game+0x36`);
* the gaining owner and gained `TypeIndex`;
* `Game::my_player` and the final `gain_tech` announcement argument;
* the shared live `Leaders` / `Match` terminal state.

Production completion currently runs during object processing (step 14), after the tick's
step-11 and step-12 terminal-cleanup drains. Therefore the adapter must drain and apply the
new owner cleanup mask to concrete Build rows in the same step-14 completion transaction;
waiting for the existing next-frame drain would leave terminal queues live for the rest of
the current frame, unlike retail.

No tick/world edit is part of this tranche. The remaining rule uncertainty is only the
engine's missing symbolic name for semaphore bit 17; its position and both behaviors are
measured. Network/UI delivery of the typed progress notice and defeated-player object
razing remain separate presentation/object-store boundaries.
