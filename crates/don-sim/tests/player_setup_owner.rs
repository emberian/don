use don_sim::systems::player_setup::{ManualPlayerSetup, ManualPlayerSetupError, MAX_TEAM_STYLE};
use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::setup_diplomacy::{SETUP_SLOTS, TEAM_AUTO};
use don_sim::systems::team_setup_mutation::{InitTeamsError, RANDOM_TEAM};
use don_sim::systems::victory_score::{game_sem, Diplo};
use don_sim::tick::Sim;

fn request(mask: u8, teams: &[(usize, i8)], style: u8, local: usize) -> ManualPlayerSetup {
    let mut request = ManualPlayerSetup {
        active_mask: mask,
        team_style: style,
        local_player_setup_slot: local,
        ..ManualPlayerSetup::default()
    };
    for &(slot, team) in teams {
        request.teams[slot] = team;
    }
    request
}

#[test]
fn alternating_teams_apply_once_to_the_authoritative_sim() {
    let mut sim = Sim::new(0x51a7_2026, 8);
    let rng = sim.world.random.state();
    let applied = sim
        .start_manual_player_setup(request(0x0f, &[(0, 0), (1, 1), (2, 0), (3, 1)], 1, 0))
        .unwrap();

    assert_eq!(applied.active_mask, 0x0f);
    assert_eq!(applied.state.on_team, [2, 2, 0, 0, 0, 0, 0, 0]);
    assert_eq!((applied.state.num_teams, applied.state.num_sides), (2, 2));
    assert!(applied.receipt.team_mode_enabled);
    assert_eq!(sim.vic_match.options.team_style, 1);
    assert_eq!(sim.vic_match.on_team, [2, 2, 0, 0, 0, 0, 0, 0]);
    assert_eq!(sim.vic_match.num_sides, 2);
    assert!(sim.vic_match.sem(game_sem::TEAM_SCORING));
    assert_eq!(sim.vic_leaders.setup_owner.configured_mask(), 0x0f);
    assert_eq!(
        (0..4)
            .map(|who| sim.vic_leaders.team_of(who))
            .collect::<Vec<_>>(),
        vec![0, 1, 0, 1]
    );
    assert!((0..4).all(|who| sim.vic_leaders.slots[who].is_active()));
    assert_eq!(sim.world.frame, 0);
    assert_eq!(sim.world.random.state(), rng);

    // `Game::init_teams` does not initialize diplomacy and this owner does not invent it.
    assert_eq!(sim.vic_leaders.get_diplo(0, 2), Diplo::War);
    assert_eq!(
        save_sim(&sim),
        Err(SaveError::Unsupported("player setup owner"))
    );
}

#[test]
fn style_three_forces_local_and_remote_teams_without_rng() {
    let mut sim = Sim::new(7, 8);
    let rng = sim.world.random.state();
    sim.start_manual_player_setup(request(0x45, &[(0, 3), (2, 2), (6, 1)], 3, 2))
        .unwrap();

    assert_eq!(sim.vic_leaders.team_of(0), 1);
    assert_eq!(sim.vic_leaders.team_of(2), 0);
    assert_eq!(sim.vic_leaders.team_of(6), 1);
    assert_eq!(sim.vic_match.on_team[..4], [1, 2, 0, 0]);
    assert_eq!(sim.world.random.state(), rng);
}

#[test]
fn random_ranked_and_malformed_requests_leave_every_channel_unchanged() {
    let cases = [
        request(0x03, &[(0, RANDOM_TEAM), (1, 1)], 1, 0),
        ManualPlayerSetup {
            ranked: true,
            ..request(0x03, &[(0, 0), (1, 1)], 1, 0)
        },
        request(0x03, &[(0, 0), (1, 1)], MAX_TEAM_STYLE + 1, 0),
        request(0x01, &[(0, 0), (1, 1)], 1, 0),
        request(0x03, &[(0, -1), (1, 1)], 1, 0),
    ];

    for (index, request) in cases.into_iter().enumerate() {
        let mut sim = Sim::new(0x100 + index as u64, 8);
        let digest = sim.channel_digest();
        let rng = sim.world.random.state();
        let error = sim.start_manual_player_setup(request).unwrap_err();
        match index {
            0 | 4 => assert!(matches!(
                error,
                ManualPlayerSetupError::UnsupportedManualTeam { .. }
            )),
            1 => assert_eq!(
                error,
                ManualPlayerSetupError::InitTeams(InitTeamsError::RankedPathUnsupported)
            ),
            2 => assert!(matches!(
                error,
                ManualPlayerSetupError::TeamStyleOutOfRange { .. }
            )),
            3 => assert!(matches!(
                error,
                ManualPlayerSetupError::InactiveSlotHasTeam { .. }
            )),
            _ => unreachable!(),
        }
        assert_eq!(sim.channel_digest(), digest);
        assert_eq!(sim.world.random.state(), rng);
        assert_eq!(sim.vic_leaders.setup_owner.configured_mask(), 0);
        assert!((0..SETUP_SLOTS).all(|who| !sim.vic_leaders.slots[who].is_active()));
    }
}

#[test]
fn empty_inactive_and_nonzero_frame_requests_fail_closed() {
    let mut empty = Sim::new(1, 8);
    assert!(matches!(
        empty.start_manual_player_setup(ManualPlayerSetup::default()),
        Err(ManualPlayerSetupError::LocalPlayerInactive { .. })
    ));

    let mut inactive_local = Sim::new(2, 8);
    assert!(matches!(
        inactive_local.start_manual_player_setup(request(0x02, &[(1, 1)], 0, 0)),
        Err(ManualPlayerSetupError::LocalPlayerInactive { slot: 0 })
    ));

    let mut advanced = Sim::new(3, 8);
    advanced.world.frame = 1;
    advanced.vic_match.frame = 1;
    assert_eq!(
        advanced.start_manual_player_setup(request(0x01, &[(0, 0)], 0, 0)),
        Err(ManualPlayerSetupError::WorldFrameIsNotZero { frame: 1 })
    );
}

#[test]
fn applied_owner_is_one_shot_and_inactive_slots_keep_the_exact_sentinel() {
    let mut sim = Sim::new(4, 8);
    sim.start_manual_player_setup(request(0x03, &[(0, 0), (1, 1)], 0, 0))
        .unwrap();
    let digest = sim.channel_digest();
    assert_eq!(
        sim.vic_leaders
            .setup_owner
            .applied()
            .unwrap()
            .state
            .setup
            .players[7]
            .team,
        TEAM_AUTO
    );
    assert_eq!(
        sim.start_manual_player_setup(request(0x03, &[(0, 1), (1, 1)], 1, 0)),
        Err(ManualPlayerSetupError::MatchAlreadyStarted)
    );
    assert_eq!(sim.channel_digest(), digest);
}
