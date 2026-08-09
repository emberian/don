//! The AI cycle: `Leader::plan_strategy` and `Leader::production_ai`.
//!
//! # Names, at last
//!
//! Everything the earlier pass of this lane called `FUN_006Cxxxx` now has its
//! real name and source line, from `schema/symbols.json` + `ron-bin/sbl/rise.pdb`.
//! The whole production AI is one source file, `main\game\leaders.cpp`:
//!
//! | stage | function | VA | size | `leaders.cpp` |
//! |---|---|---|---|---|
//! | scheduler | `Leader::plan_strategy` | `0x006B9620` | 11,108 | 26880 |
//! | driver | `Leader::production_ai` | `0x006C1960` | 628 | 23401 |
//! | 1 | **the BHS script** | — | — | — |
//! | 2 | `Leader::production_ai_setup` | `0x006C83E0` | 1,807 | 18645 |
//! | 2 | `MakeList::clear` | `0x006C9DB0` | 210 | 17969 |
//! | 3 | `Leader::found_cities` | `0x006C7A60` | 1,708 | 19028 |
//! | 4 | `Leader::research_techs` | `0x006C6BA0` | 3,776 | 19390 |
//! | 5 | `Leader::upgrade_units` | `0x006C6430` | 1,902 | 20073 |
//! | 6, 9 | `Leader::create_units` | `0x006C40A0` | 9,104 | 20413 |
//! | 7, 10 | `Leader::create_buildings` | `0x006C1BE0` | 9,405 | 22015 |
//! | 8, 11 | `Leader::make_stuff` | `0x006C8AF0` | 1,732 | 18378 |
//!
//! and the player fields the driver touches are named too, so the earlier
//! offset-only description can be retired:
//!
//! | offset | field | role |
//! |---|---|---|
//! | `+0x000` | `LeaderData::leader_flags` | bit 2 = console/human, bit 3 |
//! | `+0x004` | `LeaderData::leader_flags2` | bit 2 = production AI disabled |
//! | `+0x008` | `LeaderData::who` | 0-based index; the script gets `who+1` |
//! | `+0x00C` | `LeaderData::tribe` | nation id |
//! | `+0x050` | `LeaderData::multi_diff` | per-leader difficulty |
//! | `+0x788` | `LeaderData::production_step` | **the 12-stage counter** |
//! | `+0x78C` | `LeaderData::prod_script_run` | "the script still has work" |
//! | `+0x790` | `LeaderData::script_step` | **the script's `ref int step`** |
//! | `+0x9E0` | `LeaderData::effective_pop` | `queued_units() + control + 1` |
//! | `+0x6DD4` | `LeaderData::pers` | `Personality`, 24 ints |
//! | `+0x6EA4` | `LeaderData::prod_script` | the script name (`"economic"`) |
//!
//! **`boom_vs_rush` is `Personality::rush`.** `Personality` is a 96-byte
//! 24-`int` struct at `LeaderData +0x6DD4` (`schema/types.json`) whose first
//! member is `rush`; `Leader::production_ai` passes `player[0x6DD4] + 2` to the
//! script. So the script's third parameter is the personality's rush axis
//! shifted into 1..3. That closes a question the earlier pass left open.
//!
//! # Scheduling `[measured, disassembly at 0x006B9620]`
//!
//! ```text
//! period = 200 / [0x00C061C0]          ; game speed
//! phase  = (player_index * 25 + Game[0x550]) % period
//! if production_step != 0 { production_ai(); return; }   ; one stage per tick
//! if tick == 0 || phase == 0 { start-of-cycle work }
//! else if phase % 30 != 0 { rest of Leader::update }     ; age/tech check every 30
//! ```
//!
//! Eight players staggered 25 ticks apart exactly fill one 200-tick period, so
//! no two AI players ever begin a cycle on the same tick.

use crate::abi::{schedule, AiStage, ScriptResult};
use crate::economic::{economic, EconomicStatics};
use crate::game::{Game, PlayerView};

/// How many times each compiled stage was entered, so a match report can say
/// what the *script* did versus what the stubs did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StageCounters {
    pub production_ai_setup: u32,
    pub make_list_clear: u32,
    pub found_cities: u32,
    pub research_techs: u32,
    pub upgrade_units: u32,
    pub create_units: u32,
    pub create_buildings: u32,
    pub make_stuff: u32,
    pub script_runs: u32,
    pub script_blocked: u32,
    pub script_done: u32,
}

/// One AI player's runtime state — the parts of `LeaderData` the production AI
/// owns, plus the BHS script's `static`s.
#[derive(Clone, Debug)]
pub struct AiRuntime {
    /// `LeaderData::production_step` `+0x788`.
    pub production_step: i32,
    /// `LeaderData::prod_script_run` `+0x78C` — **a one-way latch**.
    ///
    /// `[measured]` A whole-binary scan of every reference to `+0x78C`, done
    /// per-function over `schema/symbols.json` so no code is missed, finds
    /// exactly four sites:
    ///
    /// | site | VA | instruction |
    /// |---|---|---|
    /// | `Leader::init` | `0x006E4966` | `mov [ebx+0x78C], ecx` |
    /// | `Leader::init` | `0x006E4CC7` | `mov [ebx+0x78C], 1` |
    /// | `Leader::production_ai` | `0x006C19B1` | `cmp [edi+0x78C], 0` |
    /// | `Leader::production_ai` | `0x006C1A9F` | `mov [ebx+0x78C], 0` |
    ///
    /// plus one further *read* in `Leader::gain_tech` at `0x006DED9B`. It is
    /// set to 1 once, at `Leader::init`, and cleared exactly once, when the
    /// script returns `SCRIPT_DONE`. **Nothing re-arms it.** So the BHS
    /// production script is a **one-shot opening**: the first `SCRIPT_DONE` —
    /// whether the 36-step build order finished or its own 300-second hang
    /// watchdog fired — retires the script for the rest of the match, and the
    /// compiled stages carry the player from there.
    pub prod_script_run: i32,
    /// `LeaderData::script_step` `+0x790`. Initialised to 1: a live read of the
    /// running game found a human player's field at exactly 1 and an AI's at 35.
    pub script_step: i32,
    /// `LeaderData::pers.rush`, the value `production_ai` shifts by +2 to make
    /// the script's `boom_vs_rush`.
    pub personality_rush: i32,
    pub counters: StageCounters,
}

impl Default for AiRuntime {
    fn default() -> Self {
        AiRuntime {
            production_step: 0,
            prod_script_run: 1,
            script_step: 1,
            personality_rush: 0,
            counters: StageCounters::default(),
        }
    }
}

/// All AI players plus the script-level statics, which BHS shares across every
/// player running the same script function.
#[derive(Clone, Debug)]
pub struct AiSet {
    pub runtimes: Vec<AiRuntime>,
    /// `economic.bhs`'s statics, including the genuinely shared `needed_techs`
    /// and, in `.lib`, `aibestbuildlibrary.bhs`'s own shared statics.
    pub economic_statics: EconomicStatics,
}

impl AiSet {
    pub fn new(n: usize) -> AiSet {
        AiSet {
            runtimes: vec![AiRuntime::default(); n],
            economic_statics: EconomicStatics::new(),
        }
    }

    /// Turn on the **unverified** `city_placement` trigger model. Without it
    /// the shipped build order parks on step 10; see
    /// [`crate::library::city_placement`].
    pub fn with_trigger_model(mut self, on: bool) -> AiSet {
        self.economic_statics.lib.model_triggers = on;
        self
    }

    /// `Leader::plan_strategy` `0x006B9620`, the scheduling prologue, for every
    /// player, on one tick. Call once per [`Game::step`].
    pub fn tick(&mut self, game: &mut Game) {
        for idx in 0..game.players.len() {
            if game.players[idx].is_human {
                continue;
            }
            let tick = game.frame as i32;
            let speed = game.game_speed;
            let running = self.runtimes[idx].production_step != 0;
            // `cmp [ebx+0x788], 0; jne` — a cycle in progress advances one
            // stage per tick regardless of phase.
            if running || schedule::starts_cycle(idx as i32, tick, speed) {
                self.production_ai(game, idx);
            }
        }
    }

    /// `Leader::production_ai` `0x006C1960` — one stage.
    ///
    /// Transcribed from `re/decomp-all/006c1960.c`, keeping its exact control
    /// flow including the two early-outs and the fall-through on stage 11.
    pub fn production_ai(&mut self, game: &mut Game, idx: usize) {
        let who = idx as i32 + 1;

        // `(flags0 & 4 && !(flags0 & 8)) || *PTR_00C061C4 != 0 || (flags1 & 4)`
        if game.players[idx].is_human {
            self.runtimes[idx].production_step = 0;
            return;
        }

        // `player[0x9E0] = queued_units() + control + 1` — LeaderData::effective_pop.
        // Recomputed every stage in retail; recorded here for parity of reads.
        let _effective_pop = game.population(idx) + 1;

        // `if (production_step == 1 && (prod_script_run == 0 || Game[0x2D] == 8))
        //     production_step = 2;`  — an "Infinite resources" game (category 8)
        // skips the BHS script stage outright.
        if self.runtimes[idx].production_step == 1
            && (self.runtimes[idx].prod_script_run == 0 || game.starting_resources == 8)
        {
            self.runtimes[idx].production_step = 2;
        }

        let stage = self.runtimes[idx].production_step;
        match stage {
            // Stage 0 is "idle": the scheduler only reaches production_ai on a
            // cycle start, and retail's switch has no case 0, so the default
            // arm resets to 0 — which is where a new cycle then begins at 1.
            0 => {
                self.runtimes[idx].production_step = 1;
                self.production_ai(game, idx);
            }
            1 => self.run_script(game, idx, who),
            2 => {
                self.runtimes[idx].production_step += 1;
                self.production_ai_setup(idx);
                self.make_list_clear(idx);
            }
            3 => {
                self.runtimes[idx].production_step += 1;
                self.found_cities(idx);
                self.infinite_tail(game, idx);
            }
            4 => {
                self.runtimes[idx].production_step += 1;
                self.research_techs(idx);
                self.infinite_tail(game, idx);
            }
            5 => {
                self.runtimes[idx].production_step += 1;
                self.upgrade_units(idx);
                self.infinite_tail(game, idx);
            }
            6 | 9 => {
                self.runtimes[idx].production_step += 1;
                self.create_units(idx);
                self.infinite_tail(game, idx);
            }
            7 => {
                self.runtimes[idx].production_step += 1;
                self.create_buildings(idx);
                self.infinite_tail(game, idx);
            }
            8 => {
                self.runtimes[idx].production_step += 1;
                if self.make_stuff(idx) != 0 {
                    return;
                }
                if game.starting_resources == 8 {
                    return;
                }
                self.runtimes[idx].production_step = 0;
            }
            10 => {
                self.runtimes[idx].production_step += 1;
                self.create_buildings(idx);
            }
            11 => {
                self.make_stuff(idx);
                self.runtimes[idx].production_step = 0;
            }
            _ => self.runtimes[idx].production_step = 0,
        }
    }

    /// The tail every `break`ing case falls into: on an Infinite-resources game
    /// the engine runs `make_stuff` + `MakeList::clear` again.
    fn infinite_tail(&mut self, game: &Game, idx: usize) {
        if game.starting_resources == 8 {
            self.make_stuff(idx);
            self.make_list_clear(idx);
        }
    }

    /// Stage 1 — compile-and-run the production script.
    ///
    /// `ScriptRunByName(&env, &player.prod_script, 4, who+1, &script_step,
    /// pers.rush + 2, 5)`, then the three-value protocol:
    /// `BLOCK_ON_THIS` resets the stage counter (skipping the remaining ten
    /// compiled stages this cycle), `SCRIPT_DONE` clears `prod_script_run` so
    /// the stage is never entered again, anything else advances.
    fn run_script(&mut self, game: &mut Game, idx: usize, who: i32) {
        let boom_vs_rush = self.runtimes[idx].personality_rush + 2;
        let num_loops = schedule::SCRIPT_NUM_LOOPS;

        let mut step = self.runtimes[idx].script_step;
        let rv = {
            let mut view = PlayerView { game, who };
            economic(
                &mut view,
                who,
                &mut step,
                boom_vs_rush,
                num_loops,
                &mut self.economic_statics,
            )
        };
        // `player[0x790] = step_cell->value` — the write-back happens whether
        // or not the script returned SCRIPT_DONE.
        self.runtimes[idx].script_step = step;
        self.runtimes[idx].counters.script_runs += 1;

        if rv == ScriptResult::BlockOnThis.as_i32() {
            self.runtimes[idx].counters.script_blocked += 1;
            self.runtimes[idx].production_step = 0;
            return;
        }
        if rv == ScriptResult::ScriptDone.as_i32() {
            self.runtimes[idx].counters.script_done += 1;
            self.runtimes[idx].prod_script_run = 0;
        }
        self.runtimes[idx].production_step += 1;
    }

    // -- the ten compiled stages ------------------------------------------
    //
    // NOT TRANSCRIBED. 173,424 bytes of `leaders.cpp` across 306 functions sit
    // behind these eight names; this lane recovered the names and the call
    // order, not the bodies. They are counted so a match report can state
    // plainly that the compiled side did nothing.

    fn production_ai_setup(&mut self, idx: usize) {
        self.runtimes[idx].counters.production_ai_setup += 1;
    }
    fn make_list_clear(&mut self, idx: usize) {
        self.runtimes[idx].counters.make_list_clear += 1;
    }
    fn found_cities(&mut self, idx: usize) {
        self.runtimes[idx].counters.found_cities += 1;
    }
    fn research_techs(&mut self, idx: usize) {
        self.runtimes[idx].counters.research_techs += 1;
    }
    fn upgrade_units(&mut self, idx: usize) {
        self.runtimes[idx].counters.upgrade_units += 1;
    }
    fn create_units(&mut self, idx: usize) {
        self.runtimes[idx].counters.create_units += 1;
    }
    fn create_buildings(&mut self, idx: usize) {
        self.runtimes[idx].counters.create_buildings += 1;
    }
    /// `Leader::make_stuff` returns nonzero to hold the cycle open.
    fn make_stuff(&mut self, idx: usize) -> i32 {
        self.runtimes[idx].counters.make_stuff += 1;
        0
    }

    /// The current stage as a typed value, for reporting.
    pub fn stage(&self, idx: usize) -> Option<AiStage> {
        AiStage::from_i32(self.runtimes[idx].production_step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::Difficulty;
    use crate::rules::{default_data_dir, Rules};

    fn match_of(n: usize) -> Option<(Game, AiSet)> {
        let rules = Rules::load(&default_data_dir()).ok()?;
        let nations: Vec<&str> = ["Romans", "Greeks", "Bantu", "Koreans"][..n].to_vec();
        let g = Game::new(rules, &nations, Difficulty::Tough);
        let a = AiSet::new(n);
        Some((g, a))
    }

    /// Eight players staggered 25 ticks apart in a 200-tick period must each
    /// start on a distinct tick — this is the property that makes the schedule
    /// a schedule and not a stampede.
    #[test]
    fn player_cycle_starts_never_collide() {
        let mut starts = std::collections::HashMap::new();
        for tick in 1..=200 {
            for idx in 0..8 {
                if schedule::starts_cycle(idx, tick, 1) {
                    starts.entry(tick).or_insert_with(Vec::new).push(idx);
                }
            }
        }
        for (tick, who) in &starts {
            assert_eq!(who.len(), 1, "tick {tick} started {who:?} together");
        }
        assert_eq!(
            starts.len(),
            8,
            "each of 8 players should start exactly once"
        );
    }

    #[test]
    fn a_cycle_walks_the_stages_one_per_tick() {
        let Some((mut g, mut a)) = match_of(1) else {
            return;
        };
        // Force a cycle open at stage 2 so the script stage is out of the way.
        a.runtimes[0].production_step = 2;
        for _ in 0..12 {
            a.tick(&mut g);
            g.step();
        }
        let c = &a.runtimes[0].counters;
        assert!(c.production_ai_setup >= 1);
        assert!(c.found_cities >= 1);
        assert!(c.research_techs >= 1);
        assert!(c.create_units >= 1);
        assert!(c.create_buildings >= 1);
    }

    #[test]
    fn infinite_resources_skips_the_script_stage_entirely() {
        let Some((mut g, mut a)) = match_of(1) else {
            return;
        };
        g.starting_resources = 8; // "Infinite", rules.xml <CATEGORIES id="startingresources">
        for _ in 0..600 {
            a.tick(&mut g);
            g.step();
        }
        assert_eq!(a.runtimes[0].counters.script_runs, 0);
        assert!(a.runtimes[0].counters.found_cities > 0);
    }

    /// `prod_script_run` is a one-way latch (see [`AiRuntime::prod_script_run`]):
    /// once the script returns SCRIPT_DONE the stage is never entered again, for
    /// the rest of the match. Nothing in the binary re-arms it.
    #[test]
    fn script_done_retires_the_script_permanently() {
        let Some((mut g, mut a)) = match_of(1) else {
            return;
        };
        a.runtimes[0].production_step = 1;
        a.runtimes[0].prod_script_run = 0; // as if SCRIPT_DONE had fired
        for _ in 0..2000 {
            a.tick(&mut g);
            g.step();
        }
        assert_eq!(a.runtimes[0].counters.script_runs, 0);
        assert!(
            a.runtimes[0].counters.create_buildings > 0,
            "compiled stages still cycle"
        );
    }

    /// Without the (unverified) `city_placement` trigger model the shipped
    /// build order cannot get past step 10, and its own 300-second hang
    /// watchdog then retires it. With the model it walks on. Both are recorded
    /// because the gap between them is the size of one unresolved interpreter
    /// feature.
    #[test]
    fn city_placement_is_the_gate_on_the_whole_opening() {
        let Some(rules) = Rules::load(&default_data_dir()).ok() else {
            return;
        };
        let mut reached = Vec::new();
        for model in [false, true] {
            let mut g = Game::new(rules.clone(), &["Romans"], Difficulty::Tough);
            g.params.gather_period_shift = 0;
            g.logging = false;
            let mut a = AiSet::new(1).with_trigger_model(model);
            for _ in 0..18_000 {
                a.tick(&mut g);
                g.step();
            }
            reached.push(a.runtimes[0].script_step);
        }
        assert_eq!(
            reached[0], 10,
            "without the model the opening parks on step 10"
        );
        assert!(reached[1] > 10, "with the model it advances past step 10");
    }

    /// Two identical matches must produce identical state. The AI has no RNG of
    /// its own, so this is a check on the game model, not on a seed.
    #[test]
    fn a_match_is_deterministic() {
        let Some(rules) = Rules::load(&default_data_dir()).ok() else {
            return;
        };
        let mut out = Vec::new();
        for _ in 0..2 {
            let mut g = Game::new(rules.clone(), &["Romans", "Greeks"], Difficulty::Tough);
            g.params.gather_period_shift = 0;
            g.logging = false;
            let mut a = AiSet::new(2).with_trigger_model(true);
            for _ in 0..9_000 {
                a.tick(&mut g);
                g.step();
            }
            out.push(
                g.players
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        (
                            p.stock,
                            p.buildings.len(),
                            p.techs.len(),
                            a.runtimes[i].script_step,
                        )
                    })
                    .collect::<Vec<_>>(),
            );
        }
        assert_eq!(out[0], out[1]);
    }

    #[test]
    fn the_script_actually_runs_and_issues_orders_in_a_normal_game() {
        let Some((mut g, mut a)) = match_of(1) else {
            return;
        };
        for _ in 0..3000 {
            a.tick(&mut g);
            g.step();
        }
        assert!(a.runtimes[0].counters.script_runs > 0, "script never ran");
        assert!(
            g.players[0].orders_ok > 0,
            "script issued no accepted order"
        );
    }
}
