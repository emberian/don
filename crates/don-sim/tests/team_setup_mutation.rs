#[path = "../src/systems/setup_diplomacy.rs"]
mod setup_diplomacy;
#[path = "../src/systems/team_setup_mutation.rs"]
mod team_setup_mutation;

use setup_diplomacy::{LeaderTeamState, PlayerSetup, PLAYER_PRESENT, SETUP_SLOTS};
use team_setup_mutation::*;

fn state_with_live_slots(slots: &[usize]) -> TeamSetupState {
    let mut state = TeamSetupState::default();
    state.chat_status = [[77; SETUP_SLOTS]; SETUP_SLOTS];
    for &slot in slots {
        state.setup.players[slot] = PlayerSetup {
            flags: PLAYER_PRESENT,
            who: slot as u8,
            team: slot as i8,
        };
        state.setup.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: [0; SETUP_SLOTS],
        };
    }
    state
}

#[test]
fn configured_teams_update_victory_semaphore_and_chat_as_one_state() {
    let mut state = state_with_live_slots(&[0, 1, 2]);
    state.setup.players[0].team = 2;
    state.setup.players[1].team = 2;
    state.setup.players[2].team = 3;
    state.semaphore_flags = 9;

    let receipt = init_teams_atomic(&mut state, InitTeamsRequest::default()).unwrap();

    assert_eq!(state.on_team, [0, 0, 2, 1, 0, 0, 0, 0]);
    assert_eq!((state.num_teams, state.num_sides), (2, 2));
    assert_eq!(state.setup.semaphore_820 & 0x80, 0x80);
    assert_eq!(state.semaphore_flags, 0);
    assert!(receipt.team_mode_enabled);
    assert_eq!(&state.chat_status[0][..4], &[0, 0, 1, 1]);
    assert_eq!(&state.chat_status[1][..4], &[0, 0, 1, 1]);
    assert_eq!(&state.chat_status[2][..4], &[1, 1, 0, 1]);
    assert_eq!(state.chat_status[3], [77; SETUP_SLOTS]);
}

#[test]
fn unteamed_noncooperative_players_are_independent_sides_and_close_live_chat() {
    let mut state = state_with_live_slots(&[0, 1, 4]);
    for slot in [0, 1, 4] {
        state.setup.players[slot].team = 8;
    }
    state.setup.semaphore_820 = 0xA5;

    let receipt = init_teams_atomic(&mut state, InitTeamsRequest::default()).unwrap();

    assert_eq!(state.on_team, [0; SETUP_SLOTS]);
    assert_eq!((state.num_teams, state.num_sides), (0, 3));
    assert_eq!(state.setup.semaphore_820, 0x25, "only bit 0x80 is cleared");
    assert_eq!(state.semaphore_flags, 2);
    assert!(!receipt.team_mode_enabled);
    for &leader in &[0, 1, 4] {
        for target in 0..SETUP_SLOTS {
            let expected = if [0, 1, 4].contains(&target) { 0 } else { 1 };
            assert_eq!(state.chat_status[leader][target], expected);
        }
    }
}

#[test]
fn cooperative_style_keeps_nonteam_chat_open_but_still_closes_self() {
    let mut state = state_with_live_slots(&[0, 1]);
    state.setup.team_style = 8;
    state.setup.players[0].team = 8;
    state.setup.players[1].team = 8;

    init_teams_atomic(&mut state, InitTeamsRequest::default()).unwrap();

    assert_eq!(&state.chat_status[0][..3], &[0, 1, 1]);
    assert_eq!(&state.chat_status[1][..3], &[1, 0, 1]);
}

#[test]
fn style_three_forces_local_zero_and_every_other_resolved_player_one() {
    let mut state = state_with_live_slots(&[0, 2, 6]);
    state.setup.team_style = 3;
    for slot in [0, 2, 6] {
        state.setup.players[slot].team = RANDOM_TEAM;
    }

    let receipt = init_teams_atomic(
        &mut state,
        InitTeamsRequest {
            local_player_setup_slot: 2,
            ranked: false,
        },
    )
    .unwrap();

    assert_eq!(state.setup.players[0].team, 1);
    assert_eq!(state.setup.players[2].team, 0);
    assert_eq!(state.setup.players[6].team, 1);
    assert_eq!(state.on_team[..4], [1, 2, 0, 0]);
    assert_eq!((state.num_teams, state.num_sides), (2, 2));

    let before: Vec<_> = receipt
        .script_calls
        .iter()
        .filter_map(|call| match call {
            InitTeamsScriptCall::PlayerTeam {
                phase: PlayerTeamPhase::BeforeAssignment,
                player_slot,
                team,
                ..
            } => Some((*player_slot, *team)),
            _ => None,
        })
        .collect();
    let after: Vec<_> = receipt
        .script_calls
        .iter()
        .filter_map(|call| match call {
            InitTeamsScriptCall::PlayerTeam {
                phase: PlayerTeamPhase::AfterAssignment,
                player_slot,
                team,
                ..
            } => Some((*player_slot, *team)),
            _ => None,
        })
        .collect();
    assert_eq!(before, vec![(0, 5), (2, 5), (6, 5)]);
    assert_eq!(after, vec![(0, 1), (2, 0), (6, 1)]);
}

#[test]
fn semaphore_bit_two_preserves_random_team_as_an_explicit_red_gate() {
    let mut state = state_with_live_slots(&[0, 1]);
    state.setup.team_style = 3;
    state.setup.semaphore_820 = 0x04;
    state.setup.players[1].team = RANDOM_TEAM;
    let before = state.clone();

    assert_eq!(
        init_teams_atomic(&mut state, InitTeamsRequest::default()),
        Err(InitTeamsError::UnresolvedRandomTeam { player_slot: 1 })
    );
    assert_eq!(state, before);
}

#[test]
fn missing_player_setup_fails_closed_without_mutating_any_owner() {
    let mut state = TeamSetupState::default();
    state.on_team = [9; SETUP_SLOTS];
    state.chat_status = [[6; SETUP_SLOTS]; SETUP_SLOTS];
    state.setup.leaders[3] = LeaderTeamState {
        leader_flags: 1,
        who: 3,
        diplos: [0; SETUP_SLOTS],
    };
    let before = state.clone();

    assert_eq!(
        init_teams_atomic(&mut state, InitTeamsRequest::default()),
        Err(InitTeamsError::AbsentPlayerSetup {
            leader_slot: 3,
            who: 3,
        })
    );
    assert_eq!(state, before);
}

#[test]
fn fallback_player_zero_is_not_accepted_as_a_missing_leaders_setup() {
    let mut state = state_with_live_slots(&[0]);
    state.setup.leaders[3] = LeaderTeamState {
        leader_flags: 1,
        who: 3,
        diplos: [0; SETUP_SLOTS],
    };
    let before = state.clone();

    assert_eq!(
        init_teams_atomic(&mut state, InitTeamsRequest::default()),
        Err(InitTeamsError::AbsentPlayerSetup {
            leader_slot: 3,
            who: 3,
        })
    );
    assert_eq!(state, before);
}

#[test]
fn unsafe_signed_and_ranked_paths_are_atomic_red_gates() {
    let mut signed = state_with_live_slots(&[0]);
    signed.setup.players[0].team = -1;
    let before_signed = signed.clone();
    assert_eq!(
        init_teams_atomic(&mut signed, InitTeamsRequest::default()),
        Err(InitTeamsError::UnsafeSignedTeam {
            player_slot: 0,
            team: -1,
        })
    );
    assert_eq!(signed, before_signed);

    let mut ranked = state_with_live_slots(&[0]);
    let before_ranked = ranked.clone();
    assert_eq!(
        init_teams_atomic(
            &mut ranked,
            InitTeamsRequest {
                local_player_setup_slot: 0,
                ranked: true,
            },
        ),
        Err(InitTeamsError::RankedPathUnsupported)
    );
    assert_eq!(ranked, before_ranked);
}

#[test]
fn stale_plan_cannot_overwrite_a_concurrent_setup_change() {
    let state = state_with_live_slots(&[0, 1]);
    let plan = plan_init_teams(&state, InitTeamsRequest::default()).unwrap();
    let mut changed = state;
    changed.setup.players[1].team = 3;
    let before = changed.clone();

    assert_eq!(
        apply_init_teams_plan(&mut changed, plan),
        Err(InitTeamsError::StaleState)
    );
    assert_eq!(changed, before);
}

#[test]
fn callback_plan_retains_retail_order_and_breaks_after_first_team_proof() {
    let mut state = state_with_live_slots(&[0, 1, 2]);
    state.setup.players[0].team = 0;
    state.setup.players[1].team = 0;
    state.setup.players[2].team = 1;

    let plan = plan_init_teams(&state, InitTeamsRequest::default()).unwrap();
    let calls = &plan.receipt().script_calls;
    assert!(matches!(
        calls[0],
        InitTeamsScriptCall::PlayerTeam {
            phase: PlayerTeamPhase::BeforeAssignment,
            player_slot: 0,
            ..
        }
    ));
    assert!(matches!(
        calls[3],
        InitTeamsScriptCall::TeamOccupancy { team_slot: 0, .. }
    ));

    let member_counts: Vec<_> = calls
        .iter()
        .filter_map(|call| match call {
            InitTeamsScriptCall::TeamMemberCount {
                leader_slot, count, ..
            } => Some((*leader_slot, *count)),
            _ => None,
        })
        .collect();
    assert_eq!(member_counts, vec![(0, 2)]);
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, InitTeamsScriptCall::TeamModeEnabled { .. }))
            .count(),
        1
    );
}
