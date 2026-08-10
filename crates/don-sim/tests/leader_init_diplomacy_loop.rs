use don_sim::systems::leader_init_diplomacy_loop::{
    apply_leader_init_diplomacy_loop, init_diplomacy_loop, plan_leader_init_diplomacy_loop,
    LeaderInitDiplomacyFacts, LeaderInitDiplomacyLoopError, LeaderInitDiplomacyLoopImage,
    LeaderInitDiplomacyLoopRequest, RelationDecision, SharedVisionDecision, StartingAgePath,
    DIPLO_PEACE, DIPLO_WAR,
};
use don_sim::systems::setup_diplomacy::{
    LeaderTeamState, PlayerSetup, PLAYER_PRESENT, SETUP_SLOTS,
};

fn image(teams: [i8; SETUP_SLOTS]) -> LeaderInitDiplomacyLoopImage {
    let mut image = LeaderInitDiplomacyLoopImage::default();
    for slot in 0..SETUP_SLOTS {
        image.setup.players[slot] = PlayerSetup {
            flags: PLAYER_PRESENT,
            who: slot as u8,
            team: teams[slot],
        };
        image.setup.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: [DIPLO_WAR; SETUP_SLOTS],
        };
    }
    image.setup.team_style = 1;

    image.row.treaties = [91; SETUP_SLOTS];
    image.row.agendas = [92; SETUP_SLOTS];
    image.row.good_deeds = [93; SETUP_SLOTS];
    image.row.attack_stamp = [94; SETUP_SLOTS];
    image.row.raid_stamp = [95; SETUP_SLOTS];
    image.row.capital_stamp = [96; SETUP_SLOTS];
    image.row.ally_stamp = [97; SETUP_SLOTS];
    image.row.tribute_stamp = [98; SETUP_SLOTS];
    image.row.gift_stamp = [99; SETUP_SLOTS];
    image.row.hire_stamp = [100; SETUP_SLOTS];
    image.row.hire_who = [101; SETUP_SLOTS];
    image.row.aggression = [102; SETUP_SLOTS];
    image.row.strong = [103; SETUP_SLOTS];
    image.row.weak = [104; SETUP_SLOTS];
    image.row.dow = [105; SETUP_SLOTS];
    image.row.invaders = [106; SETUP_SLOTS];
    image.row.broke_alliance = [107; SETUP_SLOTS];
    image.row.made_peace = [108; SETUP_SLOTS];
    image.row.got_diplo_message = 109;
    image.row.last_spoke = [110; SETUP_SLOTS];
    image.row.counteroffer = [111; SETUP_SLOTS];
    image.row.tribute_demanded = [112; SETUP_SLOTS];
    image.row.last_taunt = [113; SETUP_SLOTS];
    image.row.taunt_frame = [114; SETUP_SLOTS];
    image.row.ally_mask = 0xff;
    image
}

fn ordinary() -> LeaderInitDiplomacyFacts {
    LeaderInitDiplomacyFacts {
        has_shared_vision_preq: true,
        ..LeaderInitDiplomacyFacts::default()
    }
}

fn request(tribe: i32) -> LeaderInitDiplomacyLoopRequest {
    LeaderInitDiplomacyLoopRequest {
        receiver_slot: 0,
        tribe,
    }
}

fn assert_interaction_resets(image: &LeaderInitDiplomacyLoopImage) {
    for values in [
        &image.row.agendas,
        &image.row.good_deeds,
        &image.row.attack_stamp,
        &image.row.raid_stamp,
        &image.row.capital_stamp,
        &image.row.ally_stamp,
        &image.row.tribute_stamp,
        &image.row.gift_stamp,
        &image.row.hire_stamp,
        &image.row.strong,
        &image.row.weak,
        &image.row.dow,
        &image.row.invaders,
        &image.row.broke_alliance,
        &image.row.made_peace,
        &image.row.last_spoke,
        &image.row.counteroffer,
        &image.row.tribute_demanded,
        &image.row.last_taunt,
        &image.row.taunt_frame,
    ] {
        assert_eq!(*values, [0; SETUP_SLOTS]);
    }
    assert_eq!(image.row.hire_who, [-1; SETUP_SLOTS]);
    assert_eq!(image.row.got_diplo_message, 0);
}

#[test]
fn ordinary_loop_initializes_relations_treaties_shared_vision_and_resets() {
    let mut state = image([0, 1, 0, 1, 2, 3, 8, 8]);
    // The shared-vision branch calls mutual `is_ally` after the actor's write. Bind one
    // reciprocal declaration so target 2 is already mutually allied at that instant.
    state.setup.leaders[2].diplos[0] = 2;

    let receipt = init_diplomacy_loop(&mut state, request(7), ordinary()).unwrap();

    assert_eq!(state.setup.leaders[0].diplos, [2, 0, 2, 0, 0, 0, 0, 0]);
    assert_eq!(state.row.treaties, [1, 0, 1, 0, 0, 0, 0, 0]);
    assert_eq!(state.row.aggression, [0, 1, 0, 1, 1, 1, 1, 1]);
    assert_eq!(state.row.ally_mask, 0b0000_0101);
    assert_eq!(receipt.ally_mask, state.row.ally_mask);
    assert_eq!(receipt.targets[2].initialization_team, Some(true));
    assert_eq!(
        receipt.targets[2].shared_vision,
        SharedVisionDecision::Prerequisite
    );
    assert!(matches!(
        receipt.targets[1].relation_decision,
        RelationDecision::NonTeam {
            rush_rules: 0,
            starting_age: None,
            teams_locked: true,
        }
    ));
    assert_interaction_resets(&state);
}

#[test]
fn nonteam_relation_uses_exact_rush_starting_age_and_team_lock_order() {
    let mut unlocked = image([0, 1, 2, 3, 8, 8, 8, 8]);
    unlocked.setup.team_style = 0;
    init_diplomacy_loop(&mut unlocked, request(2), ordinary()).unwrap();
    assert_eq!(unlocked.setup.leaders[0].diplos[1], DIPLO_PEACE);

    let mut before_lock = image([0, 1, 2, 3, 8, 8, 8, 8]);
    let facts = LeaderInitDiplomacyFacts {
        game_rules: 8,
        rush_rules: 7,
        starting_technology: 5,
        starting_technology2: 4,
        ending_technology: 6,
        ..ordinary()
    };
    let receipt = init_diplomacy_loop(&mut before_lock, request(2), facts).unwrap();
    assert_eq!(before_lock.setup.leaders[0].diplos[1], DIPLO_PEACE);
    match receipt.targets[1].relation_decision {
        RelationDecision::NonTeam {
            starting_age: Some(age),
            teams_locked: true,
            ..
        } => {
            assert_eq!(age.value, 6);
            assert_eq!(
                age.path,
                StartingAgePath::TeamZeroCombined {
                    primary: 5,
                    secondary: 4,
                    ending: 6,
                }
            );
        }
        other => panic!("unexpected decision: {other:?}"),
    }

    let mut locked = image([0, 1, 2, 3, 8, 8, 8, 8]);
    let facts = LeaderInitDiplomacyFacts {
        rush_rules: 6,
        ..facts
    };
    init_diplomacy_loop(&mut locked, request(2), facts).unwrap();
    assert_eq!(locked.setup.leaders[0].diplos[1], DIPLO_WAR);
}

#[test]
fn scenario_preserves_raw_cells_before_shared_vision_and_treaty_queries() {
    let mut state = image([0, 1, 0, 1, 2, 3, 8, 8]);
    state.setup.leaders[0].diplos[1] = DIPLO_PEACE;
    state.setup.leaders[0].diplos[2] = 2;
    state.setup.leaders[2].diplos[0] = 2;
    let facts = LeaderInitDiplomacyFacts {
        reveal_map: 1,
        scenario_rules: true,
        has_shared_vision_preq: false,
        ..LeaderInitDiplomacyFacts::default()
    };

    let receipt = init_diplomacy_loop(&mut state, request(4), facts).unwrap();

    assert_eq!(state.setup.leaders[0].diplos[1], DIPLO_PEACE);
    assert_eq!(state.setup.leaders[0].diplos[2], 2);
    assert_eq!(state.row.ally_mask, 0b0000_0101);
    assert_eq!(receipt.targets[1].initialization_team, None);
    assert_eq!(
        receipt.targets[1].relation_decision,
        RelationDecision::ScenarioPreserved
    );
    assert_eq!(
        receipt.targets[2].shared_vision,
        SharedVisionDecision::RevealMap
    );
    assert_eq!(receipt.targets[2].treaty_team, Some(true));
}

#[test]
fn check_victory_mode_forces_nonself_war_before_shared_vision_query() {
    let mut state = image([0, 1, 0, 1, 2, 3, 8, 8]);
    state.setup.leaders[0].diplos[2] = 2;
    state.setup.leaders[2].diplos[0] = 2;
    let facts = LeaderInitDiplomacyFacts {
        reveal_map: 1,
        scenario_rules: true,
        check_victory_mode: true,
        ..LeaderInitDiplomacyFacts::default()
    };

    let receipt = init_diplomacy_loop(&mut state, request(4), facts).unwrap();

    assert_eq!(state.setup.leaders[0].diplos[0], DIPLO_WAR);
    assert_eq!(state.setup.leaders[0].diplos[2], DIPLO_WAR);
    assert_eq!(state.row.ally_mask, 1);
    assert!(!receipt.targets[0].forced_war);
    assert!(receipt.targets[2].forced_war);
    assert_eq!(
        receipt.targets[2].shared_vision,
        SharedVisionDecision::NotAllied
    );
}

#[test]
fn negative_tribe_arm_skips_team_and_shared_vision_calls() {
    let mut state = image([0, 0, 0, 0, 0, 0, 0, 0]);
    let facts = LeaderInitDiplomacyFacts {
        reveal_map: 3,
        scenario_rules: true,
        check_victory_mode: true,
        has_shared_vision_preq: true,
        ..LeaderInitDiplomacyFacts::default()
    };

    let receipt = init_diplomacy_loop(&mut state, request(-1), facts).unwrap();

    assert_eq!(state.setup.leaders[0].diplos, [2, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(state.row.treaties, [1; SETUP_SLOTS]);
    assert_eq!(state.row.ally_mask, 1);
    assert!(receipt.targets.iter().all(|target| {
        target.initialization_team.is_none()
            && target.treaty_team.is_none()
            && target.shared_vision == SharedVisionDecision::NegativeTribeSkipped
    }));
    assert_interaction_resets(&state);
}

#[test]
fn invalid_identity_and_stale_plan_refuse_without_mutation() {
    let state = image([0, 1, 2, 3, 8, 8, 8, 8]);
    assert_eq!(
        plan_leader_init_diplomacy_loop(
            &state,
            LeaderInitDiplomacyLoopRequest {
                receiver_slot: SETUP_SLOTS,
                tribe: 0,
            },
            ordinary(),
        ),
        Err(LeaderInitDiplomacyLoopError::ReceiverSlotOutOfRange { slot: SETUP_SLOTS })
    );

    let mut invalid_who = state.clone();
    invalid_who.setup.leaders[0].who = -1;
    assert_eq!(
        plan_leader_init_diplomacy_loop(&invalid_who, request(0), ordinary()),
        Err(LeaderInitDiplomacyLoopError::ReceiverWhoOutOfRange { slot: 0, who: -1 })
    );

    let plan = plan_leader_init_diplomacy_loop(&state, request(0), ordinary()).unwrap();
    let mut changed = state.clone();
    changed.row.treaties[7] ^= 1;
    let snapshot = changed.clone();
    assert_eq!(
        apply_leader_init_diplomacy_loop(&mut changed, plan),
        Err(LeaderInitDiplomacyLoopError::StaleState)
    );
    assert_eq!(changed, snapshot);
}
