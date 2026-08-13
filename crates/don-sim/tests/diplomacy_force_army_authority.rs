// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::systems::armies::{Armies, LF_ARMIES_OFF};
use don_sim::systems::diplomacy_force_army_authority::{
    commit_force_army_process, prepare_force_army_process, ForceArmyProcessError,
    ForceArmyProcessRequest,
};

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
    let request = ForceArmyProcessRequest {
        owner: 2,
        army_slot: 3,
        forced: 1,
    };
    let prepared = prepare_force_army_process(&armies, &flags, &flags2, &[request]).unwrap();
    assert!(prepared.validates());
    assert!(prepared.is_current(&armies, &flags, &flags2));
    let receipts = commit_force_army_process(&mut armies, &flags, &flags2, prepared).unwrap();
    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].validates());
    assert_eq!(receipts[0].before.human_frame, 9);
    assert_eq!(receipts[0].after.human_frame, 8);
    assert_eq!(armies.lists[2][3], receipts[0].after);
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
    let prepared = prepare_force_army_process(&armies, &flags, &[0; 8], &[request]).unwrap();
    let mut stale = armies.clone();
    stale.lists[2][3].target_o = 99;
    let stale_before = stale.lists[2][3].clone();
    assert_eq!(
        commit_force_army_process(&mut stale, &flags, &[0; 8], prepared),
        Err(ForceArmyProcessError::StaleArmy {
            owner: 2,
            army_slot: 3,
        })
    );
    assert_eq!(stale.lists[2][3], stale_before);

    flags[2] = 1;
    assert!(matches!(
        prepare_force_army_process(&armies, &flags, &[0; 8], &[request]),
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
    let prepared = prepare_force_army_process(&armies, &flags, &[0; 8], &[request]).unwrap();
    let before = armies.clone();
    flags[2] &= !LF_ARMIES_OFF;
    assert_eq!(
        commit_force_army_process(&mut armies, &flags, &[0; 8], prepared),
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
    let prepared = prepare_force_army_process(&armies, &flags, &[0; 8], &requests).unwrap();
    let receipts = commit_force_army_process(&mut armies, &flags, &[0; 8], prepared).unwrap();
    assert_eq!(
        receipts.iter().map(|r| r.request).collect::<Vec<_>>(),
        requests
    );
    assert_eq!(armies.lists[2][3].human_frame, 8);
    assert_eq!(armies.lists[2][7].human_frame, 0);
}
