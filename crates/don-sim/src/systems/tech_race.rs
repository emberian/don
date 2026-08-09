//! Tech Race victory at the tail of `Leader::gain_tech`.
//!
//! This condition does not belong to `GameDaemon::process_victory`. Retail evaluates it
//! synchronously after a leader's technology counters change. The two rule forms are
//! selected by `Game::semaphore` bit 17:
//!
//! * clear: the acquired-age count must equal `GameInfo::ending_technology`;
//! * set: the gained type must be an epoch/library technology and the epoch count must
//!   equal all 28 library technologies.
//!
//! The terminal mutation delegates to [`Leaders::victory`]. That is load-bearing: it
//! recursively wins mutual allies, defeats every other active leader, zeroes aggregate
//! queues, requests concrete no-refund Build-queue cleanup, and reaches
//! `Game::check_victory` through the defeated-player path.

use super::tech_cities::{ty, TechState};
use super::victory_score::{leader_flag, Leaders, Match, Victory, VictoryType};

/// `Game::semaphore` bit tested at `0x006DE853` and `0x006DE88F`.
///
/// The PDB does not contain the semaphore enum name. This behavioral name deliberately
/// describes only the measured Tech Race use site.
pub const TECH_RACE_ALL_EPOCHS_SEMAPHORE: u32 = 17;

/// `END_EPOCHTYPES - BASE_EPOCHTYPES`, compared as literal `0x1c` at `0x006DE987`.
pub const ALL_EPOCHS_GOAL: i32 = ty::END_EPOCHTYPES - ty::BASE_EPOCHTYPES;

/// The exact predicate that entered the terminal victory transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechRaceTrigger {
    /// Semaphore bit 17 was clear and `ages == ending_technology`.
    EndingTechnology { ages: i32, ending_technology: u8 },
    /// Semaphore bit 17 was set, the gained type was an epoch, and `epochs == 28`.
    AllEpochs { epochs: i32 },
}

/// Presentation-only output from the all-epochs form.
///
/// Retail builds the localized opponent/type message at `0x006DE8C8..0x006DE971`.
/// Keeping it typed prevents a headless simulation from fabricating UI strings or making
/// presentation a prerequisite for the deterministic victory mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechRacePresentation {
    OpponentEpochGained { who: usize, type_index: i32 },
}

/// Observable result of one post-gain Tech Race evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TechRaceReceipt {
    pub trigger: Option<TechRaceTrigger>,
    pub presentation: Option<TechRacePresentation>,
    /// True only when this call changed the gaining leader into a winner. A satisfied
    /// predicate can still be a retail no-op when that leader already won or was defeated.
    pub resolved: bool,
}

/// Execute the Tech Race tail of `Leader::gain_tech`.
///
/// [measured, `0x006DE847..0x006DE997`; vtable `TypeData +0x38` resolves to
/// `TypeData::is_epoch_type` `0x00470870`]
///
/// `ending_technology` is `GameInfo+0x2A` (`Game+0x36`). `announce` is the final integer
/// argument of `Leader::gain_tech`; it gates only the opponent-progress presentation arm.
/// The counter checks intentionally use equality, not `>=`.
pub fn process_tech_race_gain(
    state: &TechState,
    gained_type: i32,
    ending_technology: u8,
    who: usize,
    local_player: usize,
    announce: bool,
    leaders: &mut Leaders,
    game: &mut Match,
) -> TechRaceReceipt {
    let mut receipt = TechRaceReceipt::default();
    if game.options.victory != Victory::TechRace as u8 {
        return receipt;
    }

    let all_epochs = game.sem(TECH_RACE_ALL_EPOCHS_SEMAPHORE);
    if !all_epochs {
        if state.counters.ages == ending_technology as i32 {
            receipt.trigger = Some(TechRaceTrigger::EndingTechnology {
                ages: state.counters.ages,
                ending_technology,
            });
        }
    } else {
        let gained_epoch = (ty::BASE_EPOCHTYPES..ty::END_EPOCHTYPES).contains(&gained_type);
        if !gained_epoch {
            return receipt;
        }
        if who != local_player && announce {
            receipt.presentation = Some(TechRacePresentation::OpponentEpochGained {
                who,
                type_index: gained_type,
            });
        }
        if state.counters.epochs == ALL_EPOCHS_GOAL {
            receipt.trigger = Some(TechRaceTrigger::AllEpochs {
                epochs: state.counters.epochs,
            });
        }
    }

    if receipt.trigger.is_some() {
        let terminal_before =
            leaders.slots[who].leader_flags & (leader_flag::WON | leader_flag::DEFEATED) != 0;
        leaders.victory(game, who, VictoryType::ByTechRace, 0);
        receipt.resolved = !terminal_before && leaders.slots[who].flag(leader_flag::WON);
    }
    receipt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::victory_score::{game_sem, DefeatType, Diplo, ScoreConstants, TypeTable};

    fn game_and_leaders(active: usize) -> (Match, Leaders) {
        let mut game = Match::default();
        game.options.victory = Victory::TechRace as u8;
        let mut leaders = Leaders::new(TypeTable::with_default_kinds(ScoreConstants::default()));
        for who in 0..active {
            leaders.slots[who].leader_flags = leader_flag::VALID | leader_flag::ACTIVE;
        }
        (game, leaders)
    }

    #[test]
    fn ending_technology_uses_exact_equality_and_executes_terminal_transaction() {
        let (mut game, mut leaders) = game_and_leaders(3);
        leaders.set_diplo(0, 1, Diplo::Ally);
        leaders.set_diplo(1, 0, Diplo::Ally);
        for who in 0..3 {
            leaders.slots[who].num_queued[0x227 + who] = (who + 1) as u16;
        }
        let mut state = TechState::default();
        state.counters.ages = 7;

        let receipt = process_tech_race_gain(
            &state,
            ty::INFORMATION_AGE,
            7,
            0,
            0,
            true,
            &mut leaders,
            &mut game,
        );

        assert_eq!(
            receipt,
            TechRaceReceipt {
                trigger: Some(TechRaceTrigger::EndingTechnology {
                    ages: 7,
                    ending_technology: 7,
                }),
                presentation: None,
                resolved: true,
            }
        );
        assert!(leaders.slots[0].flag(leader_flag::WON));
        assert!(leaders.slots[1].flag(leader_flag::WON));
        assert_eq!(
            leaders.slots[0].victory_type,
            VictoryType::ByTechRace as i32
        );
        assert_eq!(
            leaders.slots[1].victory_type,
            VictoryType::ByTechRace as i32
        );
        assert!(leaders.slots[2].flag(leader_flag::DEFEATED));
        assert_eq!(leaders.slots[2].defeat_type, DefeatType::Victory as i32);
        assert!(leaders.slots[..3]
            .iter()
            .all(|leader| leader.num_queued.iter().all(|queued| *queued == 0)));
        assert_eq!(leaders.take_terminal_queue_cleanup(), 0b0000_0111);
        assert!(game.sem(game_sem::GAME_OVER));
        assert!(game.sem(game_sem::VICTORY_RESOLVED));
    }

    #[test]
    fn ending_technology_rejects_wrong_mode_and_both_sides_of_equality() {
        for ages in [6, 8] {
            let (mut game, mut leaders) = game_and_leaders(2);
            let mut state = TechState::default();
            state.counters.ages = ages;
            let receipt = process_tech_race_gain(
                &state,
                ty::INFORMATION_AGE,
                7,
                0,
                0,
                true,
                &mut leaders,
                &mut game,
            );
            assert_eq!(receipt, TechRaceReceipt::default());
            assert_eq!(leaders.take_terminal_queue_cleanup(), 0);
        }

        let (mut game, mut leaders) = game_and_leaders(2);
        game.options.victory = Victory::Standard as u8;
        let mut state = TechState::default();
        state.counters.ages = 7;
        let receipt = process_tech_race_gain(
            &state,
            ty::INFORMATION_AGE,
            7,
            0,
            0,
            true,
            &mut leaders,
            &mut game,
        );
        assert_eq!(receipt, TechRaceReceipt::default());
    }

    #[test]
    fn all_epochs_requires_epoch_type_and_exactly_twenty_eight() {
        for epochs in [27, 29] {
            let (mut game, mut leaders) = game_and_leaders(2);
            game.set_sem(TECH_RACE_ALL_EPOCHS_SEMAPHORE);
            let mut state = TechState::default();
            state.counters.epochs = epochs;
            let receipt = process_tech_race_gain(
                &state,
                ty::BASE_EPOCHTYPES,
                7,
                0,
                0,
                true,
                &mut leaders,
                &mut game,
            );
            assert_eq!(receipt.trigger, None);
            assert!(!receipt.resolved);
        }

        let (mut game, mut leaders) = game_and_leaders(2);
        game.set_sem(TECH_RACE_ALL_EPOCHS_SEMAPHORE);
        let mut state = TechState::default();
        state.counters.epochs = ALL_EPOCHS_GOAL;
        let non_epoch = process_tech_race_gain(
            &state,
            ty::INFORMATION_AGE,
            7,
            0,
            0,
            true,
            &mut leaders,
            &mut game,
        );
        assert_eq!(non_epoch, TechRaceReceipt::default());

        let completed = process_tech_race_gain(
            &state,
            ty::END_EPOCHTYPES - 1,
            7,
            0,
            0,
            true,
            &mut leaders,
            &mut game,
        );
        assert_eq!(
            completed.trigger,
            Some(TechRaceTrigger::AllEpochs { epochs: 28 })
        );
        assert!(completed.resolved);
    }

    #[test]
    fn all_epochs_warning_is_typed_and_never_gates_resolution() {
        let (mut game, mut leaders) = game_and_leaders(2);
        game.set_sem(TECH_RACE_ALL_EPOCHS_SEMAPHORE);
        let mut state = TechState::default();
        state.counters.epochs = 27;

        let opponent = process_tech_race_gain(
            &state,
            ty::BASE_EPOCHTYPES,
            7,
            1,
            0,
            true,
            &mut leaders,
            &mut game,
        );
        assert_eq!(
            opponent.presentation,
            Some(TechRacePresentation::OpponentEpochGained {
                who: 1,
                type_index: ty::BASE_EPOCHTYPES,
            })
        );
        assert_eq!(opponent.trigger, None);

        let local = process_tech_race_gain(
            &state,
            ty::BASE_EPOCHTYPES,
            7,
            0,
            0,
            true,
            &mut leaders,
            &mut game,
        );
        assert_eq!(local.presentation, None);

        state.counters.epochs = ALL_EPOCHS_GOAL;
        let silent_completion = process_tech_race_gain(
            &state,
            ty::BASE_EPOCHTYPES,
            7,
            1,
            0,
            false,
            &mut leaders,
            &mut game,
        );
        assert_eq!(silent_completion.presentation, None);
        assert!(silent_completion.resolved);
    }
}
