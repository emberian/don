// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/leader_set_diplo.rs"]
mod leader_set_diplo;

use leader_set_diplo::*;

fn base_image() -> SetDiploImage {
    let mut image = SetDiploImage::default();
    image.local_who = Some(0);
    for who in 0..DIPLO_SLOTS {
        image.leaders[who].who = Some(who as i32);
        image.leaders[who].leader_flags = Some(0);
        image.leaders[who].leader_flags2 = Some(0);
    }
    image
}

#[test]
fn equal_raw_state_returns_before_tail_facts() {
    let mut image = SetDiploImage::default();
    image.leaders[0].who = Some(0);
    image.leaders[1].who = Some(1);
    image.leaders[0].diplos[1] = Some(Relation::Peace);
    let plan = plan_set_diplo(
        &image,
        SetDiploRequest {
            actor: 0,
            target: 1,
            state: Relation::Peace,
        },
    )
    .unwrap();
    assert!(plan.steps.is_empty());
    assert_eq!(plan.after, image);
}

#[test]
fn revoked_alliance_clears_vision_then_ejects_in_owner_list_order() {
    let mut image = base_image();
    image.leaders[0].diplos[1] = Some(Relation::Ally);
    image.leaders[0].shared_vision = Some(0b0000_0010);
    image.leaders[1].shared_vision = Some(0b0000_0001);
    image.leaders[0].ejection_units = Some(vec![
        EjectionUnitFact {
            object_id: 10,
            is_unit: true,
            carrier_who: Some(1),
            come_out_return: Some(1),
            domain: None,
            has_air_patrol_order: None,
        },
        EjectionUnitFact {
            object_id: 11,
            is_unit: true,
            carrier_who: Some(1),
            come_out_return: Some(0),
            domain: Some(2),
            has_air_patrol_order: Some(false),
        },
    ]);
    image.leaders[1].ejection_units = Some(Vec::new());

    let plan = plan_set_diplo(
        &image,
        SetDiploRequest {
            actor: 0,
            target: 1,
            state: Relation::Peace,
        },
    )
    .unwrap();

    assert_eq!(
        &plan.steps[..7],
        &[
            SetDiploStep::Mutation(SetDiploMutation::ClearSharedVision {
                viewer: 0,
                source: 1,
            }),
            SetDiploStep::Mutation(SetDiploMutation::ClearSharedVision {
                viewer: 1,
                source: 0,
            }),
            SetDiploStep::Authority(SetDiploAuthority::ComeOut {
                owner: 0,
                object_id: 10,
            }),
            SetDiploStep::Authority(SetDiploAuthority::KillContainedUnit {
                owner: 0,
                object_id: 10,
                reason: 0,
            }),
            SetDiploStep::Authority(SetDiploAuthority::ComeOut {
                owner: 0,
                object_id: 11,
            }),
            SetDiploStep::Authority(SetDiploAuthority::AddAirStrafeOrder {
                owner: 0,
                object_id: 11,
                queue_pos: 2,
            }),
            SetDiploStep::Mutation(SetDiploMutation::WriteDeclaration {
                from: 0,
                to: 1,
                state: Relation::Peace,
            }),
        ]
    );
    assert_eq!(plan.after.leaders[0].shared_vision, Some(0));
    assert_eq!(plan.after.leaders[1].shared_vision, Some(0));
    assert_eq!(plan.after.leaders[1].diplos[0], Some(Relation::Peace));
}

#[test]
fn foreign_presentation_precedes_both_relation_writes() {
    let mut image = base_image();
    image.local_who = Some(7);
    image.console_treaty_one[0] = Some(true);
    image.console_treaty_one[1] = Some(true);
    image.leaders[0].diplos[1] = Some(Relation::Peace);
    image.leaders[0].leader_flags = Some(1);
    let mut armies = [false; ARMY_SLOTS];
    armies[2] = true;
    image.leaders[0].valid_armies = Some(armies);

    let plan = plan_set_diplo(
        &image,
        SetDiploRequest {
            actor: 0,
            target: 1,
            state: Relation::War,
        },
    )
    .unwrap();
    assert_eq!(
        plan.steps,
        vec![
            SetDiploStep::Presentation(SetDiploPresentation::ForeignRelation {
                actor: 0,
                target: 1,
                kind: ForeignRelationKind::War,
            }),
            SetDiploStep::Presentation(SetDiploPresentation::Sound { id: 0x135 }),
            SetDiploStep::Mutation(SetDiploMutation::WriteDeclaration {
                from: 0,
                to: 1,
                state: Relation::War,
            }),
            SetDiploStep::Mutation(SetDiploMutation::WriteDeclaration {
                from: 1,
                to: 0,
                state: Relation::War,
            }),
            SetDiploStep::Authority(SetDiploAuthority::ForceArmyProcess {
                owner: 0,
                army_slot: 2,
                forced: 1,
            }),
            SetDiploStep::Mutation(SetDiploMutation::MarkInterfaceDirty),
        ]
    );
}

#[test]
fn new_alliance_grants_directional_vision_then_checks_last_alliance_victory() {
    let mut image = base_image();
    image.leaders[0].diplos[1] = Some(Relation::Peace);
    image.leaders[0].shared_vision = Some(0);
    image.leaders[1].shared_vision = Some(0);
    image.leaders[0].has_shared_vision_preq = Some(true);
    image.leaders[1].has_shared_vision_preq = Some(false);
    image.global_shared_vision = Some(true);
    image.leaders[0].leader_flags = Some(1);
    image.leaders[0].valid_armies = Some([false; ARMY_SLOTS]);

    let plan = plan_set_diplo(
        &image,
        SetDiploRequest {
            actor: 0,
            target: 1,
            state: Relation::Ally,
        },
    )
    .unwrap();
    let authority: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            SetDiploStep::Authority(call) => Some(*call),
            _ => None,
        })
        .collect();
    assert_eq!(
        authority,
        vec![SetDiploAuthority::Victory {
            winner: 0,
            victory_type: 0,
            instant: 0,
        }]
    );
    assert_eq!(plan.after.leaders[0].shared_vision, Some(0b10));
    assert_eq!(plan.after.leaders[1].shared_vision, Some(0b01));
    assert_ne!(plan.after.victory_mask & (1 << 22), 0);
}

#[test]
fn missing_ejection_outcome_fails_the_whole_plan() {
    let mut image = base_image();
    image.leaders[0].diplos[1] = Some(Relation::Ally);
    image.leaders[0].shared_vision = Some(0);
    image.leaders[1].shared_vision = Some(0);
    image.leaders[0].ejection_units = Some(vec![EjectionUnitFact {
        object_id: 99,
        is_unit: true,
        carrier_who: Some(1),
        come_out_return: None,
        domain: None,
        has_air_patrol_order: None,
    }]);
    image.leaders[1].ejection_units = Some(Vec::new());
    assert_eq!(
        plan_set_diplo(
            &image,
            SetDiploRequest {
                actor: 0,
                target: 1,
                state: Relation::Peace,
            },
        ),
        Err(SetDiploPlanError::MissingEjectionFact {
            owner: 0,
            object_id: 99,
            fact: MissingEjectionFact::ComeOutReturn,
        })
    );
}

#[test]
fn applied_receipt_requires_every_authority_call_in_order() {
    let mut image = base_image();
    image.leaders[0].diplos[1] = Some(Relation::Peace);
    image.leaders[0].leader_flags = Some(1);
    let mut armies = [false; ARMY_SLOTS];
    armies[4] = true;
    image.leaders[0].valid_armies = Some(armies);
    let request = SetDiploTransactionRequest {
        before: image,
        change: SetDiploRequest {
            actor: 0,
            target: 1,
            state: Relation::War,
        },
    };
    let plan = plan_set_diplo(&request.before, request.change).unwrap();
    let mut receipt = SetDiploReceipt {
        request: request.clone(),
        status: SetDiploTransactionStatus::Applied,
        plan: Some(plan),
        authority: Vec::new(),
    };
    assert!(!receipt.validates(&request));
    receipt.authority.push(SetDiploAuthority::ForceArmyProcess {
        owner: 0,
        army_slot: 4,
        forced: 1,
    });
    assert!(receipt.validates(&request));
}

#[test]
fn accepted_deal_refunds_then_moves_proposer_to_accepter_before_reverse_direction() {
    let mut state = AcceptedDealResources::default();
    for who in [0, 1] {
        state.leaders[who].type_available = [Some(false); NUM_GOODS];
    }
    state.leaders[0].type_available[0] = Some(true);
    state.leaders[1].type_available[0] = Some(true);
    state.leaders[0].tribute_scale_percent = Some(50);
    state.leaders[1].tribute_scale_percent = Some(80);
    state.leaders[0].escrow[0] = 40;
    state.leaders[1].escrow[0] = 10;
    state.offers[0][1][0] = 40;
    state.offers[1][0][0] = 10;
    state.declaration_costs[0][1][0] = 3;
    state.declaration_costs[1][0][0] = 4;

    let plan = plan_accepted_deal_resources(&state, 0, 1).unwrap();
    assert_eq!(
        &plan.steps[..7],
        &[
            AcceptedDealResourceStep::RefundDeclarationCost {
                who: 0,
                good: 0,
                amount: 3,
            },
            AcceptedDealResourceStep::RefundDeclarationCost {
                who: 1,
                good: 0,
                amount: 4,
            },
            AcceptedDealResourceStep::ClearDeclarationCost {
                from: 0,
                to: 1,
                good: 0,
            },
            AcceptedDealResourceStep::ClearDeclarationCost {
                from: 1,
                to: 0,
                good: 0,
            },
            AcceptedDealResourceStep::CreditScaledTribute {
                from: 1,
                to: 0,
                good: 0,
                raw: 10,
                scaled: 8,
            },
            AcceptedDealResourceStep::DebitEscrow {
                who: 1,
                good: 0,
                raw: 10,
            },
            AcceptedDealResourceStep::RecordSentRaw { who: 1, raw: 10 },
        ]
    );
    assert_eq!(plan.after.leaders[0].buckets[0], 11);
    assert_eq!(plan.after.leaders[1].buckets[0], 24);
    assert_eq!(plan.after.leaders[0].escrow[0], 0);
    assert_eq!(plan.after.leaders[1].escrow[0], 0);
    assert_eq!(plan.after.leaders[0].received_scaled, 8);
    assert_eq!(plan.after.leaders[1].received_scaled, 20);
}

#[test]
fn declaration_payment_debits_both_resource_images_before_the_next_good() {
    let mut state = DeclarationPaymentImage::default();
    state.authoritative_buckets[0] = 30;
    state.leader_buckets[0] = 25;
    state.type_available = [Some(false); NUM_GOODS];
    state.type_available[0] = Some(true);
    state.costs[0] = Some(20);
    let plan = plan_declaration_payments(&state, 2).unwrap();
    assert_eq!(
        plan.steps,
        vec![
            DeclarationPaymentStep::DebitAuthoritativeBucket {
                payment: 0,
                good: 0,
                cost: 20,
            },
            DeclarationPaymentStep::DebitLeaderBucket {
                payment: 0,
                good: 0,
                cost: 20,
            },
            DeclarationPaymentStep::DebitAuthoritativeBucket {
                payment: 1,
                good: 0,
                cost: 20,
            },
            DeclarationPaymentStep::DebitLeaderBucket {
                payment: 1,
                good: 0,
                cost: 20,
            },
        ]
    );
    assert_eq!(plan.after.authoritative_buckets[0], 0);
    assert_eq!(plan.after.leader_buckets[0], 0);
}
