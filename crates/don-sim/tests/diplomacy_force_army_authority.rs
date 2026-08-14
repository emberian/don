// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::systems::armies::{Armies, LF_ARMIES_OFF, ST_MUSTERING};
use don_sim::systems::diplomacy_force_army_authority::{
    commit_force_army_process, prepare_force_army_process, ForceArmyProcessError,
    ForceArmyProcessOutcome, ForceArmyProcessRequest,
};

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
    let prepared =
        prepare_force_army_process(&armies, &flags, &flags2, &city_num, WORLD_SIZE, &[request])
            .unwrap();
    assert!(prepared.validates());
    city_num[2] = 6; // This early return never reads the city count.
    let current_world_size = (9, 9); // Nor does it read World dimensions.
    assert!(prepared.is_current(&armies, &flags, &flags2, &city_num, current_world_size,));
    let receipts = commit_force_army_process(
        &mut armies,
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

    let prepared =
        prepare_force_army_process(&armies, &flags, &[0; 8], &city_num, WORLD_SIZE, &[request])
            .unwrap();
    assert!(prepared.is_current(&armies, &flags, &[0; 8], &city_num, WORLD_SIZE));
    let receipts = commit_force_army_process(
        &mut armies,
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

    let prepared =
        prepare_force_army_process(&armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, &[request])
            .unwrap();
    let receipts =
        commit_force_army_process(&mut armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, prepared)
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
    let prepared =
        prepare_force_army_process(&armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, &[request])
            .unwrap();
    let before = armies.clone();
    assert_eq!(
        commit_force_army_process(&mut armies, &flags, &[0; 8], &[0; 8], (9, 8), prepared,),
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
    let prepared =
        prepare_force_army_process(&armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, &[request])
            .unwrap();
    let before = armies.clone();
    let mut city_num = [0; 8];
    city_num[2] = 1;
    assert_eq!(
        commit_force_army_process(
            &mut armies,
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
    let prepared =
        prepare_force_army_process(&armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, &[request])
            .unwrap();
    let mut stale = armies.clone();
    stale.lists[2][3].target_o = 99;
    let stale_before = stale.lists[2][3].clone();
    assert_eq!(
        commit_force_army_process(&mut stale, &flags, &[0; 8], &[0; 8], WORLD_SIZE, prepared,),
        Err(ForceArmyProcessError::StaleArmy {
            owner: 2,
            army_slot: 3,
        })
    );
    assert_eq!(stale.lists[2][3], stale_before);

    flags[2] = 1;
    assert!(matches!(
        prepare_force_army_process(&armies, &flags, &[0; 8], &[1; 8], WORLD_SIZE, &[request],),
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
    let prepared =
        prepare_force_army_process(&armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, &[request])
            .unwrap();
    let before = armies.clone();
    flags[2] &= !LF_ARMIES_OFF;
    assert_eq!(
        commit_force_army_process(&mut armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, prepared,),
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
    let prepared =
        prepare_force_army_process(&armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, &requests)
            .unwrap();
    let receipts =
        commit_force_army_process(&mut armies, &flags, &[0; 8], &[0; 8], WORLD_SIZE, prepared)
            .unwrap();
    assert_eq!(
        receipts.iter().map(|r| r.request).collect::<Vec<_>>(),
        requests
    );
    assert_eq!(armies.lists[2][3].human_frame, 8);
    assert_eq!(armies.lists[2][7].human_frame, 0);
}
