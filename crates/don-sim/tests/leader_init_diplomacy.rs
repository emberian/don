use don_sim::systems::leader_init_diplomacy::{
    apply_active_team_alliances, init_active_team_alliances, plan_active_team_alliances,
    LeaderInitTeamAllianceError,
};
use don_sim::systems::setup_diplomacy::{
    LeaderTeamState, PlayerSetup, SetupDiplomacy, DIPLO_ALLY, PLAYER_PRESENT, SETUP_SLOTS,
    TEAM_AUTO,
};

fn setup(mask: u8, teams: [i8; SETUP_SLOTS]) -> SetupDiplomacy {
    let mut state = SetupDiplomacy::default();
    for slot in 0..SETUP_SLOTS {
        if mask & (1u8 << slot) == 0 {
            state.players[slot] = PlayerSetup {
                flags: 0,
                who: slot as u8,
                team: TEAM_AUTO,
            };
            continue;
        }
        state.players[slot] = PlayerSetup {
            flags: PLAYER_PRESENT,
            who: slot as u8,
            team: teams[slot],
        };
        state.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: [0; SETUP_SLOTS],
        };
    }
    state
}

#[test]
fn alternating_teams_install_only_the_exact_directional_ally_cells() {
    let mut state = setup(0x0f, [0, 1, 0, 1, 8, 8, 8, 8]);
    let receipt = init_active_team_alliances(&mut state, 0x0f).unwrap();

    assert_eq!(receipt.ally_masks[..4], [0b0101, 0b1010, 0b0101, 0b1010]);
    assert_eq!(receipt.writes, 8);
    assert_eq!(state.leaders[0].diplos[..4], [2, 0, 2, 0]);
    assert_eq!(state.leaders[1].diplos[..4], [0, 2, 0, 2]);
}

#[test]
fn free_for_all_writes_self_only_and_preserves_every_nonteam_declaration() {
    let mut state = setup(0x0f, [0, 1, 2, 3, 8, 8, 8, 8]);
    state.leaders[0].diplos[1] = 1;
    state.leaders[1].diplos[0] = 1;

    let receipt = init_active_team_alliances(&mut state, 0x0f).unwrap();

    assert_eq!(receipt.ally_masks[..4], [1, 2, 4, 8]);
    assert_eq!(receipt.writes, 4);
    assert_eq!(state.leaders[0].diplos[0], DIPLO_ALLY);
    assert_eq!(state.leaders[0].diplos[1], 1);
    assert_eq!(state.leaders[1].diplos[0], 1);
}

#[test]
fn inactive_rows_and_columns_are_outside_the_bounded_prefix() {
    let mut state = setup(0x05, [0, 8, 0, 8, 8, 8, 8, 8]);
    state.leaders[0].diplos[1] = 17;
    state.leaders[1].diplos[0] = 19;

    let receipt = init_active_team_alliances(&mut state, 0x05).unwrap();

    assert_eq!(receipt.ally_masks[0], 0x05);
    assert_eq!(receipt.ally_masks[2], 0x05);
    assert_eq!(receipt.writes, 4);
    assert_eq!(state.leaders[0].diplos[1], 17);
    assert_eq!(state.leaders[1].diplos[0], 19);
}

#[test]
fn nonzero_frame_and_missing_active_leader_refuse_without_mutation() {
    let mut nonzero = setup(0x03, [0, 0, 8, 8, 8, 8, 8, 8]);
    nonzero.frame = 1;
    let before = nonzero.clone();
    assert_eq!(
        init_active_team_alliances(&mut nonzero, 0x03),
        Err(LeaderInitTeamAllianceError::SetupFrameIsNotZero { frame: 1 })
    );
    assert_eq!(nonzero, before);

    let mut missing = setup(0x01, [0, 8, 8, 8, 8, 8, 8, 8]);
    let before = missing.clone();
    assert_eq!(
        init_active_team_alliances(&mut missing, 0x03),
        Err(LeaderInitTeamAllianceError::ActiveLeaderMissing { slot: 1 })
    );
    assert_eq!(missing, before);
}

#[test]
fn stale_plan_cannot_publish_after_any_setup_change() {
    let before = setup(0x03, [0, 0, 8, 8, 8, 8, 8, 8]);
    let plan = plan_active_team_alliances(&before, 0x03).unwrap();
    let mut changed = before.clone();
    changed.team_style = 1;
    let snapshot = changed.clone();

    assert_eq!(
        apply_active_team_alliances(&mut changed, plan),
        Err(LeaderInitTeamAllianceError::StaleState)
    );
    assert_eq!(changed, snapshot);
}
