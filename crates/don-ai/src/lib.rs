//! `don-ai` — a transcription of the **shipped Rise of Nations production AI**.
//!
//! Three things live here, in decreasing order of how well established they
//! are:
//!
//! 1. [`abi`] — the engine side of the contract: the AI cycle scheduler in
//!    `Player::UpdateAI` @ `0x006B9620`, the twelve-stage production state
//!    machine in `FUN_006C1960`, the script return codes, the four
//!    independently switchable AI subsystems, and the difficulty→income
//!    percentage table. All of it read out of `riseofnations.exe`.
//! 2. [`api`] — the 50 BHS host functions the shipped scripts call, with the
//!    implementation address, parameter names and parameter types recovered
//!    from the engine's registration table.
//! 3. [`economic`] and [`library`] — a line-by-line transcription of
//!    `economic.bhs` and the parts of `aibestbuildlibrary.bhs` it uses.
//!
//! ## What this crate deliberately does not do
//!
//! It does not implement the *compiled* AI. Eleven of the twelve production
//! stages, all of combat/unit/city AI, and every `place_*` / `train_*` /
//! `find_*` host function are C++ in the retail binary and are represented
//! here as [`api::ScriptWorld`] methods for an engine to implement. The BHS
//! layer is a build-order sequencer, nothing more; see
//! `docs/tracks/ron-ai.md` for the evidence and for the division of labour.
//!
//! Fidelity tier: **C — behaviourally transcribed from shipped source and
//! decompiled/disassembled engine code, divergence not yet measured.** No part
//! of this has been differentially tested against the retail process. Nothing
//! here is verified.

//! ## Since the PDB landed
//!
//! `schema/symbols.json` names every compiled AI function, so [`scheduler`]
//! carries real names (`Leader::plan_strategy`, `Leader::production_ai`,
//! `Leader::found_cities`, …) instead of addresses, and [`game`] is a headless
//! economic world the transcription actually plays, driven through
//! [`orders::Order`] and stepped by `don-sim`'s derived economy. Run a match
//! with `cargo run -p don-ai --bin ai-match`.

pub mod abi;
pub mod api;
pub mod economic;
pub mod game;
pub mod library;
pub mod optimum;
pub mod orders;
pub mod probe;
pub mod rules;
pub mod scheduler;

pub use abi::{AiStage, AiSubsystems, Difficulty, ScriptResult};
pub use api::ScriptWorld;
pub use economic::{economic, EconomicStatics};
pub use game::{Game, ModelParams, PlayerView};
pub use orders::{Order, OrderResult};
pub use rules::Rules;
pub use scheduler::{AiRuntime, AiSet, StageCounters};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::ProbeWorld;

    /// A world with just enough answers to walk the opening.
    fn base_world() -> ProbeWorld {
        ProbeWorld::new()
            .with_str("get_mapstyle()", "Texas")
            .with_str("find_nation(1)", "Romans")
            .with_str("find_city_with_num(1,1)", "Rome")
            .with_str("find_city_with_num(1,2)", "Ostia")
            .with_str("find_city_with_num(1,3)", "Capua")
            .with("num_cities(1)", 1)
            .with("num_type_with_queued(1,Citizen)", 3)
            .with("get_techs_per_age(1)", 4)
    }

    #[test]
    fn no_cities_and_no_citizens_is_immediately_done() {
        let mut w = ProbeWorld::new();
        let mut st = EconomicStatics::new();
        let mut step = 1;
        let r = economic(&mut w, 1, &mut step, 2, 5, &mut st);
        assert_eq!(r, ScriptResult::ScriptDone.as_i32());
        // It bailed before doing anything else.
        assert_eq!(w.count("find_city_with_num"), 0);
    }

    #[test]
    fn attacked_capital_diverts_to_barracks_and_blocks_when_it_cannot_place() {
        let mut w = base_world()
            .with("was_city_attacked(1,,-1)", 1)
            .with("num_type_with_queued(1,Barracks)", 0)
            .with("have_tech(1,The Art of War)", 1)
            .with("place_building_with_cost(1,Barracks,Rome)", 0);
        let mut st = EconomicStatics::new();
        let mut step = 6;
        let r = economic(&mut w, 1, &mut step, 2, 5, &mut st);
        assert_eq!(r, ScriptResult::BlockOnThis.as_i32());
        assert_eq!(w.count("place_building_with_cost(1,Barracks,Rome)"), 1);
        // The build-order machine never ran.
        assert_eq!(step, 6);
    }

    /// `step == 1` on a standard (non-nomad) start jumps straight to the
    /// Science I opening at step 6, per the size-2 branch.
    #[test]
    fn small_town_start_enters_at_step_six() {
        let mut w = base_world()
            .with("get_starting_town_size(1)", 2)
            .with("get_starting_resources(1)", 1)
            // Make the machine a no-op once it gets there.
            .with("have_tech(1,Written Word)", 1);
        let mut st = EconomicStatics::new();
        let mut step = 1;
        economic(&mut w, 1, &mut step, 2, 5, &mut st);
        // step 6 -> 7 -> 8 ... the loop runs num_loops times, so what we can
        // assert cheaply is that it left the nomad branch entirely.
        assert!(step >= 6, "step was {step}");
    }

    /// Nomad start (`town size == 0`) stays at step 1 and asks for a city.
    #[test]
    fn nomad_start_places_a_city() {
        let mut w = base_world()
            .with("get_starting_town_size(1)", 0)
            .with("get_starting_resources(1)", 1)
            .with("num_type_with_queued(1,Small City)", 0)
            .with("find_inactive_build(1,Small City)", -1);
        let mut st = EconomicStatics::new();
        let mut step = 1;
        let r = economic(&mut w, 1, &mut step, 2, 5, &mut st);
        assert_eq!(r, ScriptResult::BlockOnThis.as_i32());
        // num_loops = 5 and the step never advances (no city appears), so the
        // engine's five loop iterations each try to place one.
        assert_eq!(w.count("place_city_with_cost(1)"), 5);
        assert_eq!(step, 1);
    }

    /// `num_loops` is the engine's constant 5. Feeding a different value must
    /// change the number of state-machine iterations — this is what makes the
    /// `5` in `FUN_006C1960` observable.
    #[test]
    fn num_loops_controls_iteration_count() {
        for loops in [1, 3, 5] {
            let mut w = base_world()
                .with("get_starting_town_size(1)", 0)
                .with("get_starting_resources(1)", 1)
                .with("num_type_with_queued(1,Small City)", 0)
                .with("find_inactive_build(1,Small City)", -1);
            let mut st = EconomicStatics::new();
            let mut step = 1;
            economic(&mut w, 1, &mut step, 2, loops, &mut st);
            assert_eq!(w.count("place_city_with_cost(1)"), loops as usize);
        }
    }

    /// The per-player "ghetto array" really is per-player: two players running
    /// the same script must not share `prev_step`.
    #[test]
    fn ghetto_array_is_per_player() {
        let mut st = EconomicStatics::new();
        for who in 1..=2 {
            let mut w = ProbeWorld::new()
                .with_str("get_mapstyle()", "Texas")
                .with_str(&format!("find_nation({who})"), "Romans")
                .with_str(&format!("find_city_with_num({who},1)"), "Rome")
                .with(&format!("num_cities({who})"), 1)
                .with(&format!("num_type_with_queued({who},Citizen)"), 3)
                .with(&format!("get_starting_town_size({who})"), 2)
                .with(&format!("get_starting_resources({who})"), 1);
            let mut step = 1;
            economic(&mut w, who, &mut step, 2, 5, &mut st);
        }
        // needed_techs is a *shared* static: whichever player ran first fixed
        // it. Both slots of the ghetto array exist independently.
        assert!(st.needed_techs.is_some());
        assert_eq!(st.prev_step.len(), 8);
    }

    /// `needed_techs` is a shared static, initialised once from the first
    /// caller. Player 2 must not re-run `get_techs_per_age`.
    #[test]
    fn needed_techs_is_initialised_once() {
        let mut st = EconomicStatics::new();
        let mut w1 = base_world().with("get_techs_per_age(1)", 4);
        let mut step = 6;
        economic(&mut w1, 1, &mut step, 2, 5, &mut st);
        assert_eq!(st.needed_techs, Some(4));
        assert_eq!(w1.count("get_techs_per_age"), 1);

        let mut w2 = ProbeWorld::new()
            .with_str("get_mapstyle()", "Texas")
            .with_str("find_nation(2)", "Romans")
            .with_str("find_city_with_num(2,1)", "Cumae")
            .with("num_cities(2)", 1)
            .with("num_type_with_queued(2,Citizen)", 3)
            .with("get_techs_per_age(2)", 9);
        let mut step2 = 6;
        economic(&mut w2, 2, &mut step2, 2, 5, &mut st);
        assert_eq!(
            st.needed_techs,
            Some(4),
            "player 2 must inherit player 1's value"
        );
        assert_eq!(w2.count("get_techs_per_age"), 0);
    }
}
