// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/diplomacy_command_plans.rs"]
mod diplomacy_command_plans;
mod command {
    pub mod diplomacy_command_plans {
        pub use crate::diplomacy_command_plans::*;
    }
}
#[path = "../src/systems/diplomacy_accept_host.rs"]
mod diplomacy_accept_host;
#[path = "../src/systems/diplomacy_declare_host.rs"]
mod diplomacy_declare_host;
#[path = "../src/systems/leader_set_diplo.rs"]
mod leader_set_diplo;
#[path = "../src/systems/setup_diplomacy.rs"]
mod setup_diplomacy;

use diplomacy_accept_host::*;
use diplomacy_command_plans::{DiplomacyProposal, ACCEPT_OPCODE};
use leader_set_diplo::{Relation, ARMY_SLOTS, NUM_GOODS};
use setup_diplomacy::{LeaderTeamState, PlayerSetup, DIPLO_ALLY, SETUP_SLOTS};

fn image() -> AcceptAuthorityImage {
    let mut image = AcceptAuthorityImage::default();
    image.declaration.command.frame = 400;
    image.declaration.command.setup.frame = 400;
    image.declaration.command.local_who = 0;
    image.declaration.set_diplo.local_who = Some(0);
    image.declaration.set_diplo.global_shared_vision = Some(false);
    image.declaration.war_allowed = Some(true);
    for slot in 0..SETUP_SLOTS {
        image.declaration.command.setup.players[slot] = PlayerSetup {
            flags: 1,
            who: slot as u8,
            team: slot as i8,
        };
        image.declaration.command.setup.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: [1; SETUP_SLOTS],
        };
        image.declaration.command.setup.leaders[slot].diplos[slot] = DIPLO_ALLY;
        let set = &mut image.declaration.set_diplo.leaders[slot];
        set.who = Some(slot as i32);
        set.leader_flags = Some(1);
        set.leader_flags2 = Some(0);
        set.shared_vision = Some(1 << slot);
        set.has_shared_vision_preq = Some(false);
        set.ejection_units = Some(Vec::new());
        set.valid_armies = Some([false; ARMY_SLOTS]);
        image.declaration.set_diplo.console_treaty_one[slot] = Some(false);
        for target in 0..SETUP_SLOTS {
            set.diplos[target] = Some(if slot == target {
                Relation::Ally
            } else {
                Relation::Peace
            });
        }
        image.declaration.payments[slot].authoritative_buckets = [100; NUM_GOODS];
        image.declaration.payments[slot].leader_buckets = [100; NUM_GOODS];
        image.declaration.payments[slot].type_available = [Some(true); NUM_GOODS];
        image.declaration.payments[slot].costs = [Some(2); NUM_GOODS];
        image.declaration.command.leaders[slot].buckets = [100; NUM_GOODS];
        image.resources.leaders[slot].buckets = [100; NUM_GOODS];
        image.resources.leaders[slot].type_available = [Some(true); NUM_GOODS];
        image.resources.leaders[slot].tribute_scale_percent = Some(100);
        image.leaders[slot].is_neutral = Some(false);
        image.leaders[slot].team_members_mode_one = Some(1);
        image.leaders[slot].num_allies = Some(0);
    }
    image
}

fn accept(accepter: i32, proposer: i32) -> Vec<u8> {
    let mut wire = vec![ACCEPT_OPCODE];
    wire.extend_from_slice(&accepter.to_le_bytes());
    wire.extend_from_slice(&proposer.to_le_bytes());
    wire
}

fn proposal(
    image: &mut AcceptAuthorityImage,
    first: usize,
    second: usize,
) -> &mut DiplomacyProposal {
    &mut image.declaration.command.leaders[first].proposals[second]
}

fn sync_pair(image: &mut AcceptAuthorityImage, first: usize, second: usize) {
    let p = image.declaration.command.leaders[first].proposals[second];
    image.resources.offers[first][second] = p.offers;
    image.resources.declaration_costs[first][second] = p.declaration_costs;
}

fn set_buckets(image: &mut AcceptAuthorityImage, who: usize, buckets: [i32; NUM_GOODS]) {
    image.declaration.payments[who].authoritative_buckets = buckets;
    image.declaration.payments[who].leader_buckets = buckets;
    image.declaration.command.leaders[who].buckets = buckets;
    image.resources.leaders[who].buckets = buckets;
}

fn set_escrow(image: &mut AcceptAuthorityImage, who: usize, escrow: [i32; NUM_GOODS]) {
    image.declaration.command.leaders[who].reserved_resources = escrow;
    image.resources.leaders[who].escrow = escrow;
}

fn set_relation(image: &mut AcceptAuthorityImage, first: usize, second: usize, relation: Relation) {
    image.declaration.command.setup.leaders[first].diplos[second] = relation as i32;
    image.declaration.command.setup.leaders[second].diplos[first] = relation as i32;
    image.declaration.set_diplo.leaders[first].diplos[second] = Some(relation);
    image.declaration.set_diplo.leaders[second].diplos[first] = Some(relation);
}

fn reciprocal_peace() -> AcceptAuthorityImage {
    let mut before = image();
    let (a, p) = (2, 5);
    before.declaration.command.local_who = p as i32;
    before.declaration.set_diplo.local_who = Some(p as i32);
    set_relation(&mut before, a, p, Relation::War);
    {
        let forward = proposal(&mut before, a, p);
        forward.agreement_pending = 1;
        forward.proposal_open = 1;
        forward.treaty = 1;
        forward.offers[0] = 10;
    }
    {
        let reverse = proposal(&mut before, p, a);
        reverse.agreement_pending = 1;
        reverse.proposal_open = 1;
        reverse.treaty = 1;
        reverse.offers[0] = 20;
    }
    sync_pair(&mut before, a, p);
    sync_pair(&mut before, p, a);
    set_buckets(&mut before, a, [90, 100, 100, 100, 100, 100]);
    set_buckets(&mut before, p, [80, 100, 100, 100, 100, 100]);
    set_escrow(&mut before, a, [10, 0, 0, 0, 0, 0]);
    set_escrow(&mut before, p, [20, 0, 0, 0, 0, 0]);
    before
}

#[test]
fn first_accept_reserves_forward_offer_and_stops_for_reciprocal_response() {
    let mut before = image();
    let (a, p) = (2, 5);
    before.declaration.command.local_who = a as i32;
    before.declaration.set_diplo.local_who = Some(a as i32);
    before.declaration.command.leaders[a].response_334[p] = 7;
    before.declaration.command.leaders[a].response_314[p] = 8;
    let forward = proposal(&mut before, a, p);
    forward.proposal_open = 1;
    forward.treaty = 1;
    forward.offers[0] = 10;
    sync_pair(&mut before, a, p);

    let plan = plan_accept(&before, &accept(a as i32, p as i32)).unwrap();
    assert_eq!(plan.outcome, AcceptOutcome::ReservedAwaitingReciprocal);
    assert_eq!(
        plan.after.declaration.payments[a].authoritative_buckets[0],
        90
    );
    assert_eq!(plan.after.declaration.payments[a].leader_buckets[0], 90);
    assert_eq!(plan.after.resources.leaders[a].escrow[0], 10);
    assert_eq!(
        plan.after.declaration.command.leaders[a].proposals[p].agreement_pending,
        1
    );
    assert_eq!(plan.after.leaders[p].agenda_flags[a] & 4, 4);
    assert_eq!(plan.after.declaration.command.leaders[a].response_334[p], 0);
    assert_eq!(plan.after.declaration.command.leaders[a].response_314[p], 0);
    assert!(required_accept_authority(&plan.steps).is_empty());
}

#[test]
fn queried_zero_attack_cost_still_uses_retail_insufficient_cost_presentation_gate() {
    let mut before = image();
    let (a, p, candidate) = (2, 5, 3);
    before.declaration.command.local_who = a as i32;
    before.declaration.set_diplo.local_who = Some(a as i32);
    // This leader lacks flags bit 2, so the queried retail DOW cost is multiplied by zero.
    assert_eq!(
        before.declaration.command.setup.leaders[a].leader_flags & 4,
        0
    );
    {
        let forward = proposal(&mut before, a, p);
        forward.proposal_open = 1;
        forward.offers[0] = 10;
        forward.attacks[candidate] = 1;
    }
    sync_pair(&mut before, a, p);
    set_buckets(&mut before, a, [9, 100, 100, 100, 100, 100]);

    let plan = plan_accept(&before, &accept(a as i32, p as i32)).unwrap();
    assert_eq!(plan.outcome, AcceptOutcome::RefusedInsufficientResources);
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        AcceptStep::ReservationPreflight {
            good: 0,
            attack_cost_queried: true,
            attack_cost: 0,
            ..
        }
    )));
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        AcceptStep::Presentation(AcceptPresentation::CannotReserveAttackCost { good: 0, .. })
    )));
}

#[test]
fn reciprocal_accept_transfers_both_directions_sets_peace_and_clears_records() {
    let before = reciprocal_peace();
    let plan = plan_accept(&before, &accept(2, 5)).unwrap();
    assert_eq!(plan.outcome, AcceptOutcome::Accepted);
    assert_eq!(plan.after.resources.leaders[2].buckets[0], 110);
    assert_eq!(plan.after.resources.leaders[5].buckets[0], 90);
    assert_eq!(plan.after.resources.leaders[2].escrow[0], 0);
    assert_eq!(plan.after.resources.leaders[5].escrow[0], 0);
    assert_eq!(plan.after.declaration.command.setup.leaders[2].diplos[5], 1);
    assert_eq!(plan.after.declaration.command.setup.leaders[5].diplos[2], 1);
    assert_eq!(plan.after.leaders[2].peace_frames[5], 400);
    assert_eq!(plan.after.leaders[5].peace_frames[2], 400);
    assert_eq!(
        plan.after.declaration.command.leaders[2].proposals[5],
        DiplomacyProposal::default()
    );
    assert_eq!(
        plan.after.declaration.command.leaders[5].proposals[2],
        DiplomacyProposal::default()
    );
    let authority = required_accept_authority(&plan.steps);
    assert_eq!(
        authority
            .iter()
            .filter(|call| matches!(call, AcceptAuthority::ConsiderTribute { .. }))
            .count(),
        12
    );
    assert_eq!(
        authority
            .iter()
            .filter(|call| matches!(call, AcceptAuthority::NotifyDeal { .. }))
            .count(),
        2
    );
}

#[test]
fn asymmetric_records_use_the_accepters_directional_treaty_for_both_root_calls() {
    let mut before = reciprocal_peace();
    proposal(&mut before, 5, 2).treaty = 2;

    let plan = plan_accept(&before, &accept(2, 5)).unwrap();
    assert_eq!(plan.outcome, AcceptOutcome::Accepted);
    assert_eq!(plan.after.declaration.command.setup.leaders[2].diplos[5], 1);
    assert_eq!(plan.after.declaration.command.setup.leaders[5].diplos[2], 1);
    assert_eq!(plan.after.leaders[2].peace_frames[5], 400);
    assert_eq!(plan.after.leaders[5].peace_frames[2], 400);
    assert!(required_accept_authority(&plan.steps)
        .iter()
        .any(|call| matches!(
            call,
            AcceptAuthority::NotifyDeal {
                leader: 2,
                other: 5,
                treaty: 1
            }
        )));
}

#[test]
fn attack_conflict_refunds_reserved_state_and_clears_both_records() {
    let mut before = image();
    let (a, p, candidate) = (2, 5, 3);
    {
        let forward = proposal(&mut before, a, p);
        forward.agreement_pending = 1;
        forward.proposal_open = 1;
        forward.offers[0] = 7;
        forward.attacks[candidate] = 1;
    }
    proposal(&mut before, p, a).agreement_pending = 1;
    sync_pair(&mut before, a, p);
    sync_pair(&mut before, p, a);
    set_buckets(&mut before, a, [93, 100, 100, 100, 100, 100]);
    set_escrow(&mut before, a, [7, 0, 0, 0, 0, 0]);
    set_relation(&mut before, a, candidate, Relation::Ally);

    let plan = plan_accept(&before, &accept(a as i32, p as i32)).unwrap();
    assert_eq!(plan.outcome, AcceptOutcome::ClearedAttackConflict);
    assert_eq!(plan.after.resources.leaders[a].buckets[0], 100);
    assert_eq!(plan.after.resources.leaders[a].escrow[0], 0);
    assert_eq!(
        plan.after.declaration.command.leaders[a].proposals[p],
        DiplomacyProposal::default()
    );
    assert_eq!(
        plan.after.declaration.command.leaders[p].proposals[a],
        DiplomacyProposal::default()
    );
}

#[test]
fn inadmissible_alliance_rolls_back_both_pending_records_before_transfer() {
    let mut before = image();
    let (a, p) = (2, 5);
    {
        let forward = proposal(&mut before, a, p);
        forward.agreement_pending = 1;
        forward.treaty = 2;
    }
    {
        let reverse = proposal(&mut before, p, a);
        reverse.agreement_pending = 1;
    }
    before.leaders[a].team_members_mode_one = Some(2);
    before.leaders[a].num_allies = Some(0);
    sync_pair(&mut before, a, p);
    sync_pair(&mut before, p, a);

    let plan = plan_accept(&before, &accept(a as i32, p as i32)).unwrap();
    assert_eq!(plan.outcome, AcceptOutcome::ClearedInvalidAlliance);
    assert!(required_accept_authority(&plan.steps).is_empty());
    assert_eq!(
        plan.after.declaration.command.leaders[a].proposals[p],
        DiplomacyProposal::default()
    );
}

#[test]
fn attack_continuations_reuse_opcode_38_and_retain_nested_authority() {
    let mut before = image();
    let (a, p, candidate) = (2, 5, 3);
    {
        let forward = proposal(&mut before, a, p);
        forward.agreement_pending = 1;
        forward.attacks[candidate] = 1;
    }
    proposal(&mut before, p, a).agreement_pending = 1;
    sync_pair(&mut before, a, p);
    sync_pair(&mut before, p, a);
    before.declaration.command.setup.leaders[a].leader_flags = 13;
    before.declaration.set_diplo.leaders[a].leader_flags = Some(13);
    before.declaration.payments[p].costs = [None; NUM_GOODS];
    before.declaration.set_diplo.leaders[a]
        .valid_armies
        .as_mut()
        .unwrap()[0] = true;
    before.declaration.set_diplo.leaders[p]
        .valid_armies
        .as_mut()
        .unwrap()[0] = true;

    let plan = plan_accept(&before, &accept(a as i32, p as i32)).unwrap();
    assert_eq!(plan.outcome, AcceptOutcome::Accepted);
    assert_eq!(plan.after.leaders[a].attack_frames[candidate], 400);
    assert_eq!(plan.after.leaders[p].attack_frames[candidate], 400);
    assert_eq!(plan.after.leaders[a].attack_peers[candidate], p as i32);
    assert_eq!(plan.after.leaders[p].attack_peers[candidate], a as i32);
    assert_eq!(
        plan.after.declaration.command.setup.leaders[a].diplos[candidate],
        0
    );
    assert_eq!(
        plan.after.declaration.command.setup.leaders[p].diplos[candidate],
        0
    );
    let recursive: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            AcceptStep::RecursiveDeclare(plan) => Some((plan.actor, plan.target, plan.pay)),
            _ => None,
        })
        .collect();
    assert_eq!(recursive, vec![(a, candidate, true), (p, candidate, false)]);
    assert_eq!(
        plan.after.declaration.payments[a].authoritative_buckets[0],
        98
    );
    assert_eq!(
        plan.after.declaration.payments[p].authoritative_buckets[0],
        100
    );
    assert_eq!(plan.after.declaration.statistics[a].by_target[candidate], 1);
    assert_eq!(plan.after.declaration.statistics[p].by_target[candidate], 0);
    assert_eq!(plan.after.declaration.payments[p].costs, [None; NUM_GOODS]);
    for nested in plan.steps.iter().filter_map(|step| match step {
        AcceptStep::RecursiveDeclare(nested) => Some(nested),
        _ => None,
    }) {
        assert_eq!(
            diplomacy_declare_host::plan_declare(&nested.plan.before, &nested.plan.wire).unwrap(),
            nested.plan
        );
        assert_eq!(
            nested.effective_after,
            if nested.pay {
                nested.plan.after.clone()
            } else {
                let mut exact = nested.plan.after.clone();
                exact.payments[nested.actor] = before.declaration.payments[nested.actor].clone();
                exact.command.leaders[nested.actor].buckets =
                    before.declaration.payments[nested.actor].authoritative_buckets;
                exact.statistics[nested.actor] =
                    before.declaration.statistics[nested.actor].clone();
                exact
            }
        );
    }
    assert_eq!(
        required_accept_authority(&plan.steps)
            .iter()
            .filter(|call| matches!(call, AcceptAuthority::RecursiveDeclare { .. }))
            .count(),
        2
    );
}

#[test]
fn stale_state_missing_authority_and_projection_disagreement_never_commit() {
    let before = reciprocal_peace();
    let wire = accept(2, 5);
    let plan = plan_accept(&before, &wire).unwrap();
    let authority = required_accept_authority(&plan.steps);
    assert!(!authority.is_empty());

    let mut stale = before.clone();
    stale.leaders[0].agenda_flags[0] = 1;
    let unchanged = stale.clone();
    let receipt =
        apply_accept_transaction(&mut stale, before.clone(), wire.clone(), authority.clone());
    assert_eq!(receipt.status, AcceptTransactionStatus::Unavailable);
    assert_eq!(stale, unchanged);

    let mut missing = before.clone();
    let receipt = apply_accept_transaction(&mut missing, before.clone(), wire.clone(), Vec::new());
    assert_eq!(receipt.status, AcceptTransactionStatus::Unavailable);
    assert_eq!(missing, before);

    let mut committed = before.clone();
    let receipt = apply_accept_transaction(&mut committed, before, wire, authority);
    assert_eq!(receipt.status, AcceptTransactionStatus::Applied);
    assert!(receipt.validates());

    let mut bad = image();
    bad.resources.leaders[2].buckets[0] = 99;
    assert!(matches!(
        plan_accept(&bad, &accept(2, 5)),
        Err(AcceptHostError::ProjectionMismatch {
            field: AcceptProjectionField::AcceptedResourceBucket,
            first: 2,
            good: 0,
            ..
        })
    ));
}
