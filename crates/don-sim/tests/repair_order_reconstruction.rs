#[path = "../src/systems/repair_order.rs"]
mod repair_order;

use repair_order::*;

fn facts() -> RepairFacts {
    RepairFacts {
        repairer: ObjectId { o: 10, who: 1 },
        target: ObjectId { o: 20, who: 1 },
        more_work: true,
        repairer_unit_masks: 0,
        target_damage: 50,
        target_is_repairer_team: true,
        repairer_relation_to_target_is_two: false,
        team_relation_to_target_is_two: false,
        target_has_object_interface: true,
        target_build_active: true,
        target_under_attack: false,
        territory_owner: None,
        target_owner_allied_with_territory: false,
        repairer_in_range: true,
        target_has_repair_interface_after_range: false,
        repair_numerator: 1,
        target_hit_capacity: 100,
        repair_scale_constant: 1,
        target_helpers: 7,
        target_build_flags: 0,
        city_repair_state_mismatch: false,
        leader_has_korean_repair_bonus: false,
        korean_repair_percent: 0,
        korean_skips_damage_penalties: false,
        target_build_masks: 0,
        frame: 1,
        target_repair_state: 0,
        resources: [RepairResourceFacts::default(); RESOURCE_COUNT],
        leader_repair_stamp: 0,
        repairer_owner_is_local_player: false,
        lost_target_fallback: LostTargetFallbackFacts {
            repairer_order_type: 1,
            repairer_state_f8: 2,
            target_has_object_interface: false,
            target_type_allows_gather: false,
            target_is_university: false,
        },
    }
}

#[test]
fn animation_precedes_lost_target_kill_and_repair_spot_fallback() {
    let mut f = facts();
    f.target_damage = 0;
    f.repairer_unit_masks = 0x40000;
    let plan = plan_repair(&f).unwrap();
    assert_eq!(
        plan.effects,
        [
            RepairEffect::SetAnimation {
                animation: 0x22,
                arg0: 0,
                arg1: 1
            },
            RepairEffect::KillCurrentOrder { arg: 0 },
            RepairEffect::FindRepairSpot,
        ]
    );
}

#[test]
fn lost_friendly_target_can_fall_through_to_gather_but_university_cannot() {
    let mut f = facts();
    f.target_damage = 0;
    f.lost_target_fallback = LostTargetFallbackFacts {
        repairer_order_type: 0,
        repairer_state_f8: 1,
        target_has_object_interface: true,
        target_type_allows_gather: true,
        target_is_university: false,
    };
    let plan = plan_repair(&f).unwrap();
    assert!(matches!(
        plan.effects.last(),
        Some(RepairEffect::AddGatherOrder { queue_pos: 2, .. })
    ));
    f.lost_target_fallback.target_is_university = true;
    assert!(!plan_repair(&f)
        .unwrap()
        .effects
        .iter()
        .any(|effect| matches!(effect, RepairEffect::AddGatherOrder { .. })));
}

#[test]
fn out_of_range_requeues_repair_with_the_order_flag() {
    let mut f = facts();
    f.repairer_in_range = false;
    let plan = plan_repair(&f).unwrap();
    assert_eq!(plan.outcome, RepairOutcome::RequeuedAroundTarget);
    assert!(matches!(
        plan.effects.last(),
        Some(RepairEffect::SwarmAround {
            order_index: 13,
            more_work: 4,
            queue_pos: 0,
            ..
        })
    ));
}

#[test]
fn admitted_unit_mask_one_only_keeps_the_animation() {
    let mut f = facts();
    f.repairer_unit_masks = 1;
    let plan = plan_repair(&f).unwrap();
    assert_eq!(plan.outcome, RepairOutcome::WaitingOnUnitMask);
    assert_eq!(plan.effects.len(), 1);
}

#[test]
fn helpers_increment_even_when_the_frame_has_no_quantum() {
    let mut f = facts();
    f.target_has_repair_interface_after_range = true;
    f.repair_numerator = 100;
    f.target_hit_capacity = 100;
    f.repair_scale_constant = 1;
    f.target_helpers = 0;
    f.frame = 1;
    let plan = plan_repair(&f).unwrap();
    assert_eq!(plan.divisor, Some(512));
    assert_eq!(plan.outcome, RepairOutcome::NoRepairQuantum);
    assert_eq!(
        plan.effects.last(),
        Some(&RepairEffect::SetTargetHelpers { value: 1 })
    );
}

#[test]
fn existing_helpers_multiply_the_divisor_before_the_helper_increment() {
    let mut f = facts();
    f.target_has_repair_interface_after_range = true;
    f.repair_numerator = 100;
    f.target_hit_capacity = 100;
    f.repair_scale_constant = 1;
    f.target_helpers = 7;
    let plan = plan_repair(&f).unwrap();
    assert_eq!(plan.divisor, Some(512 * 8));
    assert!(plan
        .effects
        .contains(&RepairEffect::SetTargetHelpers { value: 8 }));
}

#[test]
fn city_and_damage_penalties_compose_before_quantization() {
    let mut f = facts();
    f.target_has_repair_interface_after_range = true;
    f.repair_numerator = 100;
    f.target_hit_capacity = 100;
    f.repair_scale_constant = 1;
    // Isolate the city/attack/mask multipliers from retail's `(helpers + 1)` term.
    f.target_helpers = 0;
    f.target_build_flags = 0x20;
    f.city_repair_state_mismatch = true;
    f.target_under_attack = true;
    f.target_build_masks = 0x10;
    f.frame = 8192;
    let plan = plan_repair(&f).unwrap();
    assert_eq!(plan.divisor, Some(512 * 4 * 4 * 4));
}

#[test]
fn first_unaffordable_bucket_kills_and_throttles_local_feedback() {
    let mut f = facts();
    f.frame = 200;
    f.leader_repair_stamp = 0;
    f.repairer_owner_is_local_player = true;
    f.target_repair_state = 49;
    f.target_hit_capacity = 100;
    f.resources[2] = RepairResourceFacts {
        type_available: true,
        cost_basis: 2,
        stock: 0,
        secondary_stock: 4,
    };
    let plan = plan_repair(&f).unwrap();
    assert_eq!(
        plan.outcome,
        RepairOutcome::InsufficientResource { resource: 2 }
    );
    assert!(plan.effects.ends_with(&[
        RepairEffect::KillCurrentOrder { arg: 0 },
        RepairEffect::SetLeaderRepairStamp { frame: 200 },
        RepairEffect::LocalInsufficientResourceFeedback,
    ]));
}

#[test]
fn success_debits_available_resources_then_repairs() {
    let mut f = facts();
    f.target_repair_state = 49;
    f.target_hit_capacity = 100;
    f.resources[0] = RepairResourceFacts {
        type_available: true,
        cost_basis: 2,
        stock: 3,
        secondary_stock: 1,
    };
    let plan = plan_repair(&f).unwrap();
    assert_eq!(plan.outcome, RepairOutcome::Repaired { amount: 1 });
    assert_eq!(plan.resource_costs[0], 1);
    assert!(plan.effects.ends_with(&[
        RepairEffect::DebitResource {
            resource: 0,
            cost: 1,
            stock_after: 2,
            secondary_after: 0,
        },
        RepairEffect::RepairDamage {
            target: f.target,
            amount: 1,
            arg0: 0,
            arg1: 1,
        },
    ]));
}

#[test]
fn korean_mode_can_skip_both_late_x4_penalties() {
    let mut f = facts();
    f.target_has_repair_interface_after_range = true;
    f.repair_numerator = 100;
    f.target_hit_capacity = 100;
    f.repair_scale_constant = 1;
    // Isolate the Korean percentage/skip gates from retail's `(helpers + 1)` term.
    f.target_helpers = 0;
    f.target_under_attack = true;
    f.target_build_masks = 0x10;
    f.leader_has_korean_repair_bonus = true;
    f.korean_repair_percent = 25;
    f.korean_skips_damage_penalties = true;
    let plan = plan_repair(&f).unwrap();
    assert_eq!(plan.divisor, Some(384));
}

#[test]
fn zero_wrapped_unsigned_denominator_is_an_explicit_retail_trap() {
    let mut f = facts();
    f.target_has_repair_interface_after_range = true;
    f.target_hit_capacity = 0;
    assert_eq!(
        plan_repair(&f),
        Err(RepairPlanError::RetailUnsignedDivideByZero)
    );
}
