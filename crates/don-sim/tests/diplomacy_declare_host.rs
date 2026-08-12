// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/diplomacy_command_plans.rs"]
mod diplomacy_command_plans;
mod command {
    pub mod diplomacy_command_plans {
        pub use crate::diplomacy_command_plans::*;
    }
}
#[path = "../src/systems/diplomacy_declare_host.rs"]
mod diplomacy_declare_host;
#[path = "../src/systems/leader_set_diplo.rs"]
mod leader_set_diplo;
#[path = "../src/systems/setup_diplomacy.rs"]
mod setup_diplomacy;

use diplomacy_declare_host::*;
use leader_set_diplo::*;
use setup_diplomacy::{LeaderTeamState, PlayerSetup, DIPLO_ALLY, SETUP_SLOTS};

fn image() -> DeclareAuthorityImage {
    let mut image = DeclareAuthorityImage::default();
    image.command.local_who = 0;
    image.command.frame = 400;
    image.command.setup.frame = 400;
    image.set_diplo.local_who = Some(0);
    image.set_diplo.global_shared_vision = Some(false);
    for slot in 0..SETUP_SLOTS {
        image.command.setup.players[slot] = PlayerSetup {
            flags: 1,
            who: slot as u8,
            team: slot as i8,
        };
        image.command.setup.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: [1; SETUP_SLOTS],
        };
        image.command.setup.leaders[slot].diplos[slot] = DIPLO_ALLY;
        image.set_diplo.leaders[slot].who = Some(slot as i32);
        image.set_diplo.leaders[slot].leader_flags = Some(1);
        image.set_diplo.leaders[slot].leader_flags2 = Some(0);
        image.set_diplo.leaders[slot].shared_vision = Some(1 << slot);
        image.set_diplo.leaders[slot].has_shared_vision_preq = Some(false);
        image.set_diplo.leaders[slot].ejection_units = Some(Vec::new());
        let mut armies = [false; ARMY_SLOTS];
        armies[0] = true;
        image.set_diplo.leaders[slot].valid_armies = Some(armies);
        image.set_diplo.console_treaty_one[slot] = Some(false);
        for target in 0..SETUP_SLOTS {
            image.set_diplo.leaders[slot].diplos[target] = Some(if slot == target {
                Relation::Ally
            } else {
                Relation::Peace
            });
        }
        image.payments[slot].authoritative_buckets = [100; NUM_GOODS];
        image.payments[slot].leader_buckets = [100; NUM_GOODS];
        image.payments[slot].type_available = [Some(true); NUM_GOODS];
        image.payments[slot].costs = [Some(3); NUM_GOODS];
        image.command.leaders[slot].buckets = [100; NUM_GOODS];
    }
    image.war_allowed = Some(true);
    image
}

fn declare(sender: i32, target: i32, relation: i32) -> Vec<u8> {
    let mut wire = vec![diplomacy_command_plans::DECLARE_OPCODE];
    wire.extend_from_slice(&sender.to_le_bytes());
    wire.extend_from_slice(&target.to_le_bytes());
    wire.extend_from_slice(&relation.to_le_bytes());
    wire
}

fn authority(plan: &DeclarePlan) -> Vec<SetDiploAuthority> {
    plan.steps
        .iter()
        .flat_map(|step| match step {
            DeclareStep::RootSetDiplo(plan) | DeclareStep::AllySetDiplo { plan, .. } => plan
                .steps
                .iter()
                .filter_map(|step| match step {
                    SetDiploStep::Authority(call) => Some(*call),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn exact_retail_war_packet_debits_both_images_then_commits_full_set_diplo() {
    let before = image();
    let wire = declare(2, 5, 0);
    assert_eq!(wire, hex("26020000000500000000000000"));
    let plan = plan_declare(&before, &wire).unwrap();
    assert_eq!(
        plan.after.payments[2].authoritative_buckets,
        [97; NUM_GOODS]
    );
    assert_eq!(plan.after.payments[2].leader_buckets, [97; NUM_GOODS]);
    assert_eq!(plan.after.command.setup.leaders[2].diplos[5], 0);
    assert_eq!(plan.after.command.setup.leaders[5].diplos[2], 0);
    assert!(matches!(plan.steps[0], DeclareStep::Payment(_)));
    assert!(plan
        .steps
        .iter()
        .any(|step| matches!(step, DeclareStep::RootSetDiplo(_))));
}

#[test]
fn allied_war_packet_rewrites_to_peace_and_stamps_before_set_diplo() {
    let mut before = image();
    for (from, to) in [(1, 6), (6, 1)] {
        before.command.setup.leaders[from].diplos[to] = 2;
        before.set_diplo.leaders[from].diplos[to] = Some(Relation::Ally);
    }
    let plan = plan_declare(&before, &declare(1, 6, 0)).unwrap();
    assert_eq!(plan.after.command.setup.leaders[1].diplos[6], 1);
    assert_eq!(plan.after.command.setup.leaders[6].diplos[1], 1);
    assert_eq!(plan.after.command.leaders[1].declaration_frame[6], 400);
    assert_eq!(plan.after.statistics[1].by_target[6], 1);
    assert!(plan.steps.iter().any(|step| matches!(
        step,
        DeclareStep::Prefix(
            diplomacy_command_plans::DiplomacyStep::RewriteAlliedWarRequestToNeutral { .. }
        )
    )));
}

#[test]
fn no_war_and_unaffordable_paths_are_exact_noops() {
    let mut blocked = image();
    blocked.command.local_who = 2;
    blocked.set_diplo.local_who = Some(2);
    blocked.war_allowed = Some(false);
    let plan = plan_declare(&blocked, &declare(2, 5, 0)).unwrap();
    assert_eq!(plan.after, blocked);
    assert!(plan.steps.contains(&DeclareStep::Presentation(
        DeclarePresentation::WarNotAllowed {
            sender: 2,
            target: 5
        }
    )));

    let mut poor = image();
    poor.command.local_who = 2;
    poor.set_diplo.local_who = Some(2);
    poor.payments[2].authoritative_buckets[3] = 2;
    poor.command.leaders[2].buckets[3] = 2;
    let plan = plan_declare(&poor, &declare(2, 5, 0)).unwrap();
    assert_eq!(plan.after, poor);
    assert!(plan.steps.contains(&DeclareStep::Presentation(
        DeclarePresentation::CannotAfford {
            sender: 2,
            target: 5,
            short_good: Some(3),
        }
    )));

    poor.war_allowed = Some(false);
    let plan = plan_declare(&poor, &declare(2, 5, 0)).unwrap();
    assert!(plan.steps.contains(&DeclareStep::Presentation(
        DeclarePresentation::CannotAfford {
            sender: 2,
            target: 5,
            short_good: Some(3),
        }
    )));
    assert!(!plan.steps.contains(&DeclareStep::Presentation(
        DeclarePresentation::WarNotAllowed {
            sender: 2,
            target: 5,
        }
    )));
}

#[test]
fn neutral_peace_skips_affordability_but_pay_dow_still_clamps() {
    let mut before = image();
    before.payments[2].authoritative_buckets[3] = 2;
    before.payments[2].leader_buckets[3] = 2;
    before.command.leaders[2].buckets[3] = 2;
    let plan = plan_declare(&before, &declare(2, 5, 1)).unwrap();
    assert_eq!(plan.after.payments[2].authoritative_buckets[3], 0);
    assert_eq!(plan.after.payments[2].leader_buckets[3], 0);
    assert!(matches!(plan.steps[0], DeclareStep::Payment(_)));
}

#[test]
fn stale_snapshot_and_missing_authority_roll_back_every_projection() {
    let before = image();
    let wire = declare(2, 5, 0);
    let plan = plan_declare(&before, &wire).unwrap();
    let calls = authority(&plan);
    assert!(matches!(
        calls.as_slice(),
        [SetDiploAuthority::ForceArmyProcess {
            owner: 2,
            army_slot: 0,
            forced: 1,
        }]
    ));

    let mut stale = before.clone();
    stale.statistics[0].repeated_targets = 1;
    let unchanged = stale.clone();
    let receipt =
        apply_declare_transaction(&mut stale, before.clone(), wire.clone(), calls.clone());
    assert_eq!(receipt.status, DeclareTransactionStatus::Unavailable);
    assert_eq!(stale, unchanged);

    let mut current = before.clone();
    let receipt = apply_declare_transaction(&mut current, before.clone(), wire.clone(), Vec::new());
    assert_eq!(receipt.status, DeclareTransactionStatus::Unavailable);
    assert_eq!(current, before);

    let mut committed = before.clone();
    let receipt = apply_declare_transaction(&mut committed, before, wire, calls);
    assert_eq!(receipt.status, DeclareTransactionStatus::Applied);
    assert!(receipt.validates());
}

#[test]
fn mismatched_relation_or_bucket_projection_refuses_before_planning() {
    let mut bad = image();
    bad.set_diplo.leaders[2].diplos[5] = Some(Relation::Ally);
    assert!(matches!(
        plan_declare(&bad, &declare(2, 5, 0)),
        Err(DeclareHostError::ProjectionMismatch {
            field: ProjectionField::Relation,
            first: 2,
            second: 5,
        })
    ));
    let mut bad = image();
    bad.command.leaders[2].buckets[0] = 99;
    assert!(matches!(
        plan_declare(&bad, &declare(2, 5, 0)),
        Err(DeclareHostError::ProjectionMismatch {
            field: ProjectionField::AuthoritativeBucket,
            first: 2,
            ..
        })
    ));
}

fn hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
