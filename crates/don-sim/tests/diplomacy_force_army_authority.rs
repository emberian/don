// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::systems::armies::{Armies, LF_ARMIES_OFF, ST_MUSTERING};
use don_sim::systems::diplomacy_force_army_authority::{
    commit_force_army_process, commit_force_army_process_with_strategy,
    commit_force_army_process_with_strategy_and_difficulty, prepare_force_army_process,
    prepare_force_army_process_with_strategy,
    prepare_force_army_process_with_strategy_and_difficulty, ForceArmyMusterCityFact,
    ForceArmyMusterDifficultyFact, ForceArmyMusterStrategyFact, ForceArmyProcessError,
    ForceArmyProcessOutcome, ForceArmyProcessRequest,
};
use don_sim::systems::tech_cities::CityPool;
use don_sim::systems::victory_score::game_sem;

const WORLD_SIZE: (i32, i32) = (8, 8);

fn live_army() -> Armies {
    let mut armies = Armies::new();
    let army = &mut armies.lists[2][3];
    army.valid = 1;
    army.army = 3;
    army.who = 2;
    army.human_frame = 9;
    armies
}

#[test]
fn armies_off_force_process_decrements_only_human_frame_and_cas_commits() {
    let mut armies = live_army();
    let flags = std::array::from_fn(|who| u32::from(who == 2) * (1 | LF_ARMIES_OFF));
    let flags2 = [0; 8];
    let mut city_num = [0; 8];
    city_num[2] = 5;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    let prepared = prepare_force_army_process(
        &armies,
        &CityPool::new(),
        &flags,
        &flags2,
        &city_num,
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    assert!(prepared.validates());
    city_num[2] = 6; // This early return never reads the city count.
    let current_world_size = (9, 9); // Nor does it read World dimensions.
    assert!(prepared.is_current(
        &armies,
        &CityPool::new(),
        &flags,
        &flags2,
        &city_num,
        current_world_size,
    ));
    let receipts = commit_force_army_process(
        &mut armies,
        &CityPool::new(),
        &flags,
        &flags2,
        &city_num,
        current_world_size,
        prepared,
    )
    .unwrap();
    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].validates());
    assert_eq!(receipts[0].leader_city_num, None);
    assert_eq!(receipts[0].world_size, None);
    assert_eq!(receipts[0].before.human_frame, 9);
    assert_eq!(receipts[0].after.human_frame, 8);
    assert_eq!(armies.lists[2][3], receipts[0].after);
}

#[test]
fn active_empty_army_normalizes_and_retires_without_a_live_host() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.role = 0x55;
    army.num_units = 12;
    army.num_captains = 4;
    army.num_standard = 3;
    army.num_decoys = 2;
    army.city = -1;
    army.target_o = 77;
    army.list[0] = 123;
    let mut flags = [0; 8];
    flags[2] = 1;
    let city_num = [0; 8];
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };

    let prepared = prepare_force_army_process(
        &armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &city_num,
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    assert!(prepared.is_current(
        &armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &city_num,
        WORLD_SIZE,
    ));
    let receipts = commit_force_army_process(
        &mut armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &city_num,
        WORLD_SIZE,
        prepared,
    )
    .unwrap();
    let receipt = &receipts[0];
    assert!(receipt.validates());
    assert_eq!(receipt.outcome, ForceArmyProcessOutcome::RetiredEmpty);
    assert_eq!(receipt.leader_city_num, Some(0));
    assert_eq!(receipt.before.human_frame, 9);
    assert_eq!(receipt.after.valid, 0);
    assert_eq!(receipt.after.status, 0);
    assert_eq!(receipt.after.human_frame, 0);
    assert_eq!(receipt.after.num_groups, 0);
    assert_eq!(receipt.after.role, 0);
    assert_eq!(receipt.after.num_units, 0);
    assert_eq!(receipt.after.num_captains, 0);
    assert_eq!(receipt.after.num_standard, 0);
    assert_eq!(receipt.after.num_decoys, 0);
    assert_eq!(receipt.after.city, 0);
    assert_eq!(receipt.after.target_o, 77);
    assert_eq!(receipt.after.list[0], 123);
    assert_eq!(armies.lists[2][3], receipt.after);
}

#[test]
fn empty_mustering_army_obeys_human_rally_without_a_group_host() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.role = 0x55;
    army.num_units = 12;
    army.num_captains = 4;
    army.num_standard = 3;
    army.num_decoys = 2;
    let mut flags = [0; 8];
    flags[2] = 1;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };

    let prepared = prepare_force_army_process(
        &armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    let receipts = commit_force_army_process(
        &mut armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        prepared,
    )
    .unwrap();
    let receipt = &receipts[0];
    assert!(receipt.validates());
    assert_eq!(
        receipt.outcome,
        ForceArmyProcessOutcome::MovedEmptyHumanOrder
    );
    assert_eq!(receipt.leader_city_num, None);
    assert_eq!(receipt.world_size, Some(WORLD_SIZE));
    assert_eq!(receipt.after.valid, 1);
    assert_eq!(receipt.after.status, ST_MUSTERING);
    assert_eq!(receipt.after.human_frame, 8);
    assert_eq!(receipt.after.role, 0);
    assert_eq!(receipt.after.num_units, 0);
    assert_eq!(receipt.after.num_captains, 0);
    assert_eq!(receipt.after.num_standard, 0);
    assert_eq!(receipt.after.num_decoys, 0);
    assert_eq!((receipt.after.x, receipt.after.y), (0, 0));
    assert_eq!((receipt.after.muster_x, receipt.after.muster_y), (0, 0));
    assert_eq!(armies.lists[2][3], receipt.after);
}

#[test]
fn empty_human_rally_world_size_is_part_of_the_stale_cas() {
    let mut armies = live_army();
    armies.lists[2][3].status = ST_MUSTERING;
    let mut flags = [0; 8];
    flags[2] = 1;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    let prepared = prepare_force_army_process(
        &armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    let before = armies.clone();
    assert_eq!(
        commit_force_army_process(
            &mut armies,
            &CityPool::new(),
            &flags,
            &[0; 8],
            &[0; 8],
            (9, 8),
            prepared,
        ),
        Err(ForceArmyProcessError::StaleWorld)
    );
    assert_eq!(armies.lists, before.lists);
}

#[test]
fn empty_retirement_city_count_is_part_of_the_stale_cas() {
    let mut armies = live_army();
    let mut flags = [0; 8];
    flags[2] = 1;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    let prepared = prepare_force_army_process(
        &armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    let before = armies.clone();
    let mut city_num = [0; 8];
    city_num[2] = 1;
    assert_eq!(
        commit_force_army_process(
            &mut armies,
            &CityPool::new(),
            &flags,
            &[0; 8],
            &city_num,
            WORLD_SIZE,
            prepared,
        ),
        Err(ForceArmyProcessError::StaleLeader { owner: 2 })
    );
    assert_eq!(armies.lists, before.lists);
}

#[test]
fn stale_army_and_unresolved_general_body_never_publish() {
    let armies = live_army();
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    let mut flags = [0; 8];
    flags[2] = 1 | LF_ARMIES_OFF;
    let prepared = prepare_force_army_process(
        &armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    let mut stale = armies.clone();
    stale.lists[2][3].target_o = 99;
    let stale_before = stale.lists[2][3].clone();
    assert_eq!(
        commit_force_army_process(
            &mut stale,
            &CityPool::new(),
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            prepared,
        ),
        Err(ForceArmyProcessError::StaleArmy {
            owner: 2,
            army_slot: 3,
        })
    );
    assert_eq!(stale.lists[2][3], stale_before);

    flags[2] = 1;
    assert!(matches!(
        prepare_force_army_process(
            &armies,
            &CityPool::new(),
            &flags,
            &[0; 8],
            &[1; 8],
            WORLD_SIZE,
            &[request],
        ),
        Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
            owner: 2,
            army_slot: 3,
        })
    ));
}

#[test]
fn owner_gate_is_part_of_the_same_stale_cas() {
    let mut armies = live_army();
    let mut flags = [0; 8];
    flags[2] = 1 | LF_ARMIES_OFF;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    let prepared = prepare_force_army_process(
        &armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    let before = armies.clone();
    flags[2] &= !LF_ARMIES_OFF;
    assert_eq!(
        commit_force_army_process(
            &mut armies,
            &CityPool::new(),
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            prepared,
        ),
        Err(ForceArmyProcessError::StaleLeader { owner: 2 })
    );
    assert_eq!(armies.lists, before.lists);
}

#[test]
fn multiple_slots_keep_retail_order_and_zero_countdowns_stable() {
    let mut armies = live_army();
    let second = &mut armies.lists[2][7];
    second.valid = 1;
    second.army = 7;
    second.who = 2;
    second.human_frame = 0;
    let mut flags = [0; 8];
    flags[2] = 1 | LF_ARMIES_OFF;
    let requests = [
        ForceArmyProcessRequest {
            owner: 2,
            army_slot: 3,
            forced: 1,
        },
        ForceArmyProcessRequest {
            owner: 2,
            army_slot: 7,
            forced: 1,
        },
    ];
    let prepared = prepare_force_army_process(
        &armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &requests,
    )
    .unwrap();
    let receipts = commit_force_army_process(
        &mut armies,
        &CityPool::new(),
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        prepared,
    )
    .unwrap();
    assert_eq!(
        receipts.iter().map(|r| r.request).collect::<Vec<_>>(),
        requests
    );
    assert_eq!(armies.lists[2][3].human_frame, 8);
    assert_eq!(armies.lists[2][7].human_frame, 0);
}

#[test]
fn expired_empty_naval_muster_releases_from_an_inactive_canonical_city() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.human_frame = 1;
    army.navy = 1;
    army.city = 4;
    army.role = 0x55;
    army.num_units = 12;
    army.num_captains = 4;
    army.num_standard = 3;
    army.num_decoys = 2;
    army.muster_x = 2;
    army.muster_y = 3;
    army.muster_angle = 0x1234_5678;
    let mut cities = CityPool::new();
    cities.slots[2][4].who = 2;
    cities.slots[2][4].city_flags = 0x200;
    let mut flags = [0; 8];
    flags[2] = 1;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };

    let prepared = prepare_force_army_process(
        &armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    assert!(prepared.is_current(&armies, &cities, &flags, &[0; 8], &[0; 8], WORLD_SIZE,));
    let receipts = commit_force_army_process(
        &mut armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        prepared,
    )
    .unwrap();
    let receipt = &receipts[0];
    assert!(receipt.validates());
    assert_eq!(
        receipt.outcome,
        ForceArmyProcessOutcome::ClosedEmptyNavalMuster
    );
    assert_eq!(
        receipt.muster_city,
        Some(ForceArmyMusterCityFact::InactiveOwner {
            who: 2,
            flags_low: 0,
        })
    );
    assert_eq!(receipt.after.human_frame, 0);
    assert_eq!(receipt.after.valid, 0);
    assert_eq!(receipt.after.status, 0);
    assert_eq!(receipt.after.city, -1);
    assert_eq!((receipt.after.x, receipt.after.y), (0x780, 0xa80));
    assert_eq!(receipt.after.angle, 0x1234_5678);
    assert_eq!(receipt.after.role, 0);
    assert_eq!(receipt.after.num_units, 0);
    assert_eq!(receipt.after.num_captains, 0);
    assert_eq!(receipt.after.num_standard, 0);
    assert_eq!(receipt.after.num_decoys, 0);
    assert_eq!(armies.lists[2][3], receipt.after);
    let mut forged = receipt.clone();
    forged.after.valid = 1;
    forged.after.status = 2;
    assert!(
        !forged.validates(),
        "retail re-reads marching status and closes before returning"
    );
}

#[test]
fn naval_muster_city_witness_is_lazy_and_part_of_the_atomic_cas() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.human_frame = 0;
    army.navy = 1;
    army.city = 4;
    let mut cities = CityPool::new();
    cities.slots[2][4].who = 7;
    cities.slots[2][4].city_flags = 1;
    let mut flags = [0; 8];
    flags[2] = 1;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    let prepared = prepare_force_army_process(
        &armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &[request],
    )
    .unwrap();
    assert_eq!(
        prepared.receipts()[0].muster_city,
        Some(ForceArmyMusterCityFact::ForeignOwner { who: 7 })
    );

    // Foreign ownership returns before retail reads city_flags, so this does not stale.
    cities.slots[2][4].city_flags = 0;
    assert!(prepared.is_current(&armies, &cities, &flags, &[0; 8], &[0; 8], WORLD_SIZE,));
    // The owner byte was read and changing it invalidates the whole staged publish.
    cities.slots[2][4].who = 6;
    let before = armies.clone();
    assert_eq!(
        commit_force_army_process(
            &mut armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            prepared,
        ),
        Err(ForceArmyProcessError::StaleCity { owner: 2, city: 4 })
    );
    assert_eq!(armies.lists, before.lists);
}

#[test]
fn active_owned_muster_city_keeps_find_muster_spot_fail_closed() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.human_frame = 1;
    army.navy = 1;
    army.city = 4;
    let mut cities = CityPool::new();
    cities.slots[2][4].who = 2;
    cities.slots[2][4].city_flags = 1;
    let mut flags = [0; 8];
    flags[2] = 1;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    assert!(matches!(
        prepare_force_army_process(
            &armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            &[request],
        ),
        Err(ForceArmyProcessError::RequiresUnresolvedArmyBody {
            owner: 2,
            army_slot: 3,
        })
    ));
}

#[test]
fn released_empty_land_muster_reads_saved_strategy_then_closes_after_status_reread() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.human_frame = 1;
    army.navy = 0;
    army.city = 4;
    army.reg = 5;
    army.muster_x = 2;
    army.muster_y = 3;
    army.muster_angle = 0x1234_5678;
    let mut cities = CityPool::new();
    cities.slots[2][4].who = 2;
    cities.slots[2][4].city_flags = 0;
    let mut flags = [0; 8];
    flags[2] = 1;
    let mut strategy = [[0u16; 64]; 8];
    // Bit 8 performs the exact Leader flags read; flags & 0x300 is clear, so retail falls
    // through to marching instead of transporting.
    strategy[2][5] = 8;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };

    let prepared = prepare_force_army_process_with_strategy(
        &armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &strategy,
        &[request],
    )
    .unwrap();
    assert!(prepared.is_current_with_strategy(
        &armies, &cities, &flags, &[0; 8], &[0; 8], WORLD_SIZE, &strategy,
    ));
    let receipts = commit_force_army_process_with_strategy(
        &mut armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &strategy,
        prepared,
    )
    .unwrap();
    let receipt = &receipts[0];
    assert!(receipt.validates());
    assert_eq!(
        receipt.outcome,
        ForceArmyProcessOutcome::ClosedEmptyLandMuster
    );
    assert_eq!(
        receipt.muster_strategy,
        Some(ForceArmyMusterStrategyFact {
            region: 5,
            value: 8
        })
    );
    assert_eq!(receipt.after.valid, 0);
    assert_eq!(receipt.after.status, 0);
    assert_eq!(receipt.after.human_frame, 0);
    assert_eq!(receipt.after.city, -1);
    assert_eq!((receipt.after.x, receipt.after.y), (0x780, 0xa80));
    assert_eq!(receipt.after.angle, 0x1234_5678);
}

#[test]
fn land_muster_strategy_is_required_and_part_of_the_atomic_cas() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.human_frame = 0;
    army.navy = 0;
    army.city = 4;
    army.reg = 5;
    let mut cities = CityPool::new();
    cities.slots[2][4].who = 7;
    let mut flags = [0; 8];
    flags[2] = 1;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };

    assert!(matches!(
        prepare_force_army_process(
            &armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            &[request],
        ),
        Err(ForceArmyProcessError::RequiresUnresolvedArmyBody { .. })
    ));

    let mut strategy = [[0u16; 64]; 8];
    let prepared = prepare_force_army_process_with_strategy(
        &armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &strategy,
        &[request],
    )
    .unwrap();
    strategy[2][5] = 4;
    let before = armies.clone();
    assert_eq!(
        commit_force_army_process_with_strategy(
            &mut armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            &strategy,
            prepared,
        ),
        Err(ForceArmyProcessError::StaleStrategy {
            owner: 2,
            region: 5,
        })
    );
    assert_eq!(armies.lists, before.lists);
}

#[test]
fn released_empty_bit4_muster_uses_forced_leader_difficulty_then_defends_and_closes() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.human_frame = 1;
    army.navy = 0;
    army.city = 4;
    army.reg = 5;
    army.role = 0x55;
    army.num_units = 12;
    army.num_captains = 4;
    army.num_standard = 3;
    army.num_decoys = 2;
    army.muster_x = 2;
    army.muster_y = 3;
    army.muster_angle = 0x1234_5678;
    let mut cities = CityPool::new();
    cities.slots[2][4].who = 7;
    let mut flags = [0; 8];
    flags[2] = 1;
    let mut strategy = [[0u16; 64]; 8];
    strategy[2][5] = 4;
    let match_semaphore = 1u32 << game_sem::NET_OR_RECORDING;
    let mut multi_diff = [0; 8];
    multi_diff[2] = 2;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };

    let prepared = prepare_force_army_process_with_strategy_and_difficulty(
        &armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &strategy,
        match_semaphore,
        &multi_diff,
        &[request],
    )
    .unwrap();
    assert!(prepared.is_current_with_strategy_and_difficulty(
        &armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &strategy,
        match_semaphore,
        &multi_diff,
    ));
    let receipts = commit_force_army_process_with_strategy_and_difficulty(
        &mut armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &strategy,
        match_semaphore,
        &multi_diff,
        prepared,
    )
    .unwrap();

    let receipt = &receipts[0];
    assert!(receipt.validates());
    assert_eq!(
        receipt.outcome,
        ForceArmyProcessOutcome::ClosedEmptyDefendingMuster
    );
    assert_eq!(
        receipt.muster_strategy,
        Some(ForceArmyMusterStrategyFact {
            region: 5,
            value: 4,
        })
    );
    assert_eq!(
        receipt.muster_difficulty,
        Some(ForceArmyMusterDifficultyFact {
            match_flags_820: 4,
            multi_diff: 2,
        })
    );
    assert_eq!(receipt.after.human_frame, 0);
    assert_eq!(receipt.after.valid, 0);
    assert_eq!(receipt.after.status, 0);
    assert_eq!(receipt.after.city, -1);
    assert_eq!((receipt.after.x, receipt.after.y), (0x780, 0xa80));
    assert_eq!(receipt.after.angle, 0x1234_5678);
}

#[test]
fn bit4_muster_difficulty_inputs_are_atomic_and_global_arm_stays_fail_closed() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.human_frame = 0;
    army.navy = 0;
    army.city = 4;
    army.reg = 5;
    let mut cities = CityPool::new();
    cities.slots[2][4].who = 7;
    let mut flags = [0; 8];
    flags[2] = 1;
    let mut strategy = [[0u16; 64]; 8];
    strategy[2][5] = 4;
    let match_semaphore = 1u32 << game_sem::NET_OR_RECORDING;
    let mut multi_diff = [0; 8];
    multi_diff[2] = 2;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };

    assert!(matches!(
        prepare_force_army_process_with_strategy_and_difficulty(
            &armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            &strategy,
            0,
            &multi_diff,
            &[request],
        ),
        Err(ForceArmyProcessError::RequiresUnresolvedArmyBody { .. })
    ));

    let prepared = prepare_force_army_process_with_strategy_and_difficulty(
        &armies,
        &cities,
        &flags,
        &[0; 8],
        &[0; 8],
        WORLD_SIZE,
        &strategy,
        match_semaphore,
        &multi_diff,
        &[request],
    )
    .unwrap();
    let before = armies.clone();
    assert_eq!(
        commit_force_army_process_with_strategy_and_difficulty(
            &mut armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            &strategy,
            0,
            &multi_diff,
            prepared.clone(),
        ),
        Err(ForceArmyProcessError::StaleDifficulty { owner: 2 })
    );
    assert_eq!(armies.lists, before.lists);

    multi_diff[2] = 3;
    assert_eq!(
        commit_force_army_process_with_strategy_and_difficulty(
            &mut armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            &strategy,
            match_semaphore,
            &multi_diff,
            prepared,
        ),
        Err(ForceArmyProcessError::StaleDifficulty { owner: 2 })
    );
    assert_eq!(armies.lists, before.lists);
}

#[test]
fn land_muster_without_difficulty_and_transporting_dispatches_remain_fail_closed() {
    let mut armies = live_army();
    let army = &mut armies.lists[2][3];
    army.status = ST_MUSTERING;
    army.human_frame = 0;
    army.navy = 0;
    army.city = 4;
    army.reg = 5;
    let mut cities = CityPool::new();
    cities.slots[2][4].who = 7;
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    let mut flags = [0; 8];
    flags[2] = 1;
    let mut strategy = [[0u16; 64]; 8];
    strategy[2][5] = 4;
    assert!(matches!(
        prepare_force_army_process_with_strategy(
            &armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            &strategy,
            &[request],
        ),
        Err(ForceArmyProcessError::RequiresUnresolvedArmyBody { .. })
    ));

    strategy[2][5] = 8;
    flags[2] |= 0x100;
    assert!(matches!(
        prepare_force_army_process_with_strategy(
            &armies,
            &cities,
            &flags,
            &[0; 8],
            &[0; 8],
            WORLD_SIZE,
            &strategy,
            &[request],
        ),
        Err(ForceArmyProcessError::RequiresUnresolvedArmyBody { .. })
    ));
}
