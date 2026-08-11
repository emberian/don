use don_sim::systems::leader_init_diplomacy_loop::{RelationDecision, SharedVisionDecision};
use don_sim::systems::player_setup::{ManualPlayerSetup, ManualPlayerSetupError, MAX_TEAM_STYLE};
use don_sim::systems::save_load::{load_sim, save_sim};
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
    // Without prerequisite/reveal-map authority the shipped loop retains self only,
    // even though the raw teammate relation is Ally.
    assert_eq!(applied.diplomacy.ally_masks[..4], [0x01, 0x02, 0x04, 0x08]);
    assert_eq!(applied.diplomacy.writes, 8);
    assert_eq!(applied.diplomacy.receipts.len(), SETUP_SLOTS);
    assert!(applied
        .diplomacy
        .receipts
        .iter()
        .enumerate()
        .all(|(slot, receipt)| receipt.receiver_slot == slot && receipt.who == slot));
    assert_eq!(applied.diplomacy.rows[0].treaties[2], 1);
    assert_eq!(applied.diplomacy.rows[0].aggression[1], 1);
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

    assert_eq!(sim.vic_leaders.get_diplo(0, 2), Diplo::Ally);
    assert_eq!(sim.vic_leaders.get_diplo(0, 1), Diplo::War);
    let bytes = save_sim(&sim).unwrap();
    let loaded = load_sim(&bytes).unwrap();
    assert_eq!(loaded.channel_digest(), sim.channel_digest());
    assert_eq!(
        loaded.vic_leaders.setup_owner.applied(),
        sim.vic_leaders.setup_owner.applied()
    );
    assert_eq!(save_sim(&loaded).unwrap(), bytes);
}

#[test]
fn shared_vision_is_computed_in_sequential_eight_leader_order() {
    let mut sim = Sim::new(0x51a7_2027, 8);
    let mut setup = request(0x0f, &[(0, 0), (1, 1), (2, 0), (3, 1)], 1, 0);
    setup.shared_vision_preq_mask = 0x0f;
    let applied = sim.start_manual_player_setup(setup).unwrap();

    // Leader 0 cannot see a reciprocal Ally declaration from not-yet-initialized 2,
    // while Leader 2 observes the declaration already published by Leader 0. The same
    // read-after-write ordering applies to 1/3.
    assert_eq!(applied.diplomacy.ally_masks[..4], [0x01, 0x02, 0x05, 0x0a]);
    assert_eq!(
        applied.diplomacy.receipts[0].targets[2].shared_vision,
        SharedVisionDecision::NotAllied
    );
    assert_eq!(
        applied.diplomacy.receipts[2].targets[0].shared_vision,
        SharedVisionDecision::Prerequisite
    );
    assert_eq!(sim.vic_leaders.slots[2].init_diplomacy.ally_mask, 0x05);
    assert!(sim.vic_leaders.slots[2].has_preq_2b0);
}

#[test]
fn setup_supplies_option_and_semaphore_facts_to_every_row() {
    let mut sim = Sim::new(0x51a7_2028, 8);
    sim.vic_match.options.rush_rules = 4;
    sim.vic_match.options.starting_technology = 2;
    sim.vic_match.options.reveal_map = 3;
    sim.vic_match.set_sem(game_sem::CHECK_VICTORY_MODE);

    let applied = sim
        .start_manual_player_setup(request(0x03, &[(0, 0), (1, 1)], 0, 0))
        .unwrap();
    let target = applied.diplomacy.receipts[0].targets[1];

    assert!(matches!(
        target.relation_decision,
        RelationDecision::NonTeam {
            rush_rules: 4,
            teams_locked: false,
            ..
        }
    ));
    assert!(target.forced_war);
    assert_eq!(target.relation_after, Diplo::War as i32);
    assert_eq!(target.treaty_after, 1);
    assert_eq!(target.aggression_after, 1);
    assert_eq!(applied.diplomacy.facts[7].reveal_map, 3);
    assert!(applied.diplomacy.facts[7].check_victory_mode);
}

#[test]
fn diplomacy_rows_and_shared_vision_are_checksum_owned() {
    let setup = request(0x05, &[(0, 0), (2, 0)], 1, 0);
    let mut no_vision = Sim::new(0x51a7_2029, 8);
    no_vision.start_manual_player_setup(setup).unwrap();

    let mut with_vision = Sim::new(0x51a7_2029, 8);
    with_vision.vic_match.options.reveal_map = 1;
    with_vision.start_manual_player_setup(setup).unwrap();

    assert_ne!(
        no_vision.vic_leaders.slots[2].init_diplomacy.ally_mask,
        with_vision.vic_leaders.slots[2].init_diplomacy.ally_mask
    );
    assert_ne!(no_vision.channel_digest(), with_vision.channel_digest());
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
    assert_eq!(sim.vic_leaders.get_diplo(0, 6), Diplo::Ally);
    assert_eq!(sim.vic_leaders.get_diplo(2, 0), Diplo::War);
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

#[test]
fn mutable_leader_rows_and_post_setup_clocks_are_owned_by_v11() {
    let mut sim = Sim::new(5, 8);
    sim.start_manual_player_setup(request(0x03, &[(0, 0), (1, 1)], 1, 0))
        .unwrap();
    sim.vic_leaders.slots[0].init_diplomacy.treaties[1] ^= 1;
    let bytes = save_sim(&sim).expect("runtime diplomacy is no longer setup receipt state");
    let loaded = load_sim(&bytes).unwrap();
    assert_eq!(loaded.vic_leaders.slots[0].init_diplomacy.treaties[1], 1);
    assert_eq!(save_sim(&loaded).unwrap(), bytes);

    let mut advanced = Sim::new(6, 8);
    advanced
        .start_manual_player_setup(request(0x03, &[(0, 0), (1, 1)], 1, 0))
        .unwrap();
    advanced.world.frame = 1;
    advanced.vic_match.frame = 1;
    let bytes = save_sim(&advanced).expect("mutable match state crosses frame zero");
    assert_eq!(load_sim(&bytes).unwrap().world.frame, 1);
}
