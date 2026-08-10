#[path = "../src/systems/unit_action_come_out_frontier.rs"]
mod unit_action_come_out_frontier;

use unit_action_come_out_frontier::*;

const ACTOR: ObjectIdentity = ObjectIdentity::new(2, 17);

fn ordinary() -> UnitActionComeOutFacts {
    UnitActionComeOutFacts {
        actor: ACTOR,
        actor_type: 50,
        unit_masks: 0x0c00_0042,
        inside: InsideLookupFacts::default(),
        actor_first_guy: None,
        actor_inside_down: None,
        inside_chain: Vec::new(),
        leader_flags: None,
    }
}

fn guy(end_time: u32, cur_anim: i8, anim_hint_length: i32) -> GuyAnimationState {
    GuyAnimationState {
        end_time,
        cur_anim,
        anim_hint_length,
    }
}

#[test]
fn every_path_clears_launch_order_and_path_state_before_general_come_out() {
    let plan = plan_unit_action_come_out(&ordinary()).unwrap();
    assert_eq!(
        plan.steps,
        [
            UnitActionComeOutStep::SetUnitMasks {
                before: 0x0c00_0042,
                after: 0x0800_0042,
            },
            UnitActionComeOutStep::ClearPathAnchor,
            UnitActionComeOutStep::CloseOrders { argument: 0 },
            UnitActionComeOutStep::ClearPartialPath,
            UnitActionComeOutStep::UpdateAction,
            UnitActionComeOutStep::AuthorityUnitComeOut {
                unit: ACTOR,
                argument: 0,
            },
        ]
    );
    assert!(plan.downstream_required);
}

#[test]
fn branch_facts_are_lazy_until_a_same_owner_scholar_container_is_reached() {
    let mut facts = ordinary();
    facts.inside.container = Some(ObjectIdentity::new(3, 9));
    facts.actor_type = SCHOLAR_TYPE;
    assert!(plan_unit_action_come_out(&facts).is_ok());

    facts.inside.container = Some(ObjectIdentity::new(2, 9));
    assert_eq!(
        plan_unit_action_come_out(&facts),
        Err(UnitActionComeOutPlanError::Missing(
            MissingComeOutFact::ContainerIsBuild
        ))
    );
    facts.inside.container_is_build = Some(false);
    assert!(plan_unit_action_come_out(&facts).is_ok());

    facts.inside.container_is_build = Some(true);
    assert_eq!(
        plan_unit_action_come_out(&facts),
        Err(UnitActionComeOutPlanError::Missing(
            MissingComeOutFact::ContainerIsUniversity
        ))
    );
    facts.inside.container_is_university = Some(false);
    assert!(plan_unit_action_come_out(&facts).is_ok());
}

#[test]
fn university_branch_shifts_only_scholar_animation_in_chain_order() {
    let first = ObjectIdentity::new(2, 18);
    let second = ObjectIdentity::new(4, 19);
    let third = ObjectIdentity::new(2, 20);
    let mut facts = ordinary();
    facts.actor_type = KOREAN_SCHOLAR_TYPE;
    facts.inside = InsideLookupFacts {
        container: Some(ObjectIdentity::new(2, 8)),
        container_is_build: Some(true),
        container_is_university: Some(true),
    };
    facts.actor_first_guy = Some(guy(100, 7, 3));
    facts.actor_inside_down = Some(first);
    facts.inside_chain = vec![
        InsideChainNode {
            identity: first,
            type_index: 50,
            next: Some(second),
            first_guy: None,
        },
        InsideChainNode {
            identity: second,
            type_index: SCHOLAR_TYPE,
            next: Some(third),
            first_guy: Some(guy(200, 9, 4)),
        },
        InsideChainNode {
            identity: third,
            type_index: KOREAN_SCHOLAR_TYPE,
            next: None,
            first_guy: Some(guy(300, 11, 5)),
        },
    ];
    facts.leader_flags = Some(0x81);

    let plan = plan_unit_action_come_out(&facts).unwrap();
    assert_eq!(
        &plan.steps[5..],
        &[
            UnitActionComeOutStep::SetLeaderFlags {
                owner: 2,
                before: 0x81,
                after: 0x0200_0081,
            },
            UnitActionComeOutStep::ClearFirstGuyAnimHints {
                unit: ACTOR,
                before: 3,
            },
            UnitActionComeOutStep::ClearFirstGuyAnimHints {
                unit: second,
                before: 4,
            },
            UnitActionComeOutStep::ShiftScholarAnimation {
                unit: second,
                before_end_time: 200,
                before_cur_anim: 9,
                after_end_time: 100,
                after_cur_anim: 7,
            },
            UnitActionComeOutStep::ClearFirstGuyAnimHints {
                unit: third,
                before: 5,
            },
            UnitActionComeOutStep::ShiftScholarAnimation {
                unit: third,
                before_end_time: 300,
                before_cur_anim: 11,
                after_end_time: 200,
                after_cur_anim: 9,
            },
            UnitActionComeOutStep::AuthorityUnitComeOut {
                unit: ACTOR,
                argument: 0,
            },
        ]
    );
}

fn university() -> UnitActionComeOutFacts {
    let mut facts = ordinary();
    facts.actor_type = SCHOLAR_TYPE;
    facts.inside = InsideLookupFacts {
        container: Some(ObjectIdentity::new(2, 8)),
        container_is_build: Some(true),
        container_is_university: Some(true),
    };
    facts.actor_first_guy = Some(guy(100, 7, 3));
    facts.leader_flags = Some(9);
    facts
}

#[test]
fn university_facts_and_chain_shape_fail_closed() {
    let mut missing_guy = university();
    missing_guy.actor_first_guy = None;
    assert_eq!(
        plan_unit_action_come_out(&missing_guy),
        Err(UnitActionComeOutPlanError::Missing(
            MissingComeOutFact::ActorFirstGuy
        ))
    );

    let mut missing_flags = university();
    missing_flags.leader_flags = None;
    assert_eq!(
        plan_unit_action_come_out(&missing_flags),
        Err(UnitActionComeOutPlanError::Missing(
            MissingComeOutFact::LeaderFlags
        ))
    );

    let mut bad_head = university();
    bad_head.actor_inside_down = Some(ObjectIdentity::new(2, 1));
    assert_eq!(
        plan_unit_action_come_out(&bad_head),
        Err(UnitActionComeOutPlanError::ChainHeadMismatch)
    );

    let mut missing_scholar_guy = university();
    let child = ObjectIdentity::new(2, 21);
    missing_scholar_guy.actor_inside_down = Some(child);
    missing_scholar_guy.inside_chain.push(InsideChainNode {
        identity: child,
        type_index: SCHOLAR_TYPE,
        next: None,
        first_guy: None,
    });
    assert_eq!(
        plan_unit_action_come_out(&missing_scholar_guy),
        Err(UnitActionComeOutPlanError::MissingScholarGuy(child))
    );
}

#[test]
fn unreached_branch_payloads_are_rejected_instead_of_silently_ignored() {
    let mut facts = ordinary();
    facts.actor_first_guy = Some(guy(1, 2, 3));
    assert_eq!(
        plan_unit_action_come_out(&facts),
        Err(UnitActionComeOutPlanError::UnexpectedActorGuy)
    );

    let mut facts = ordinary();
    facts.leader_flags = Some(0);
    assert_eq!(
        plan_unit_action_come_out(&facts),
        Err(UnitActionComeOutPlanError::UnexpectedLeaderFlags)
    );
}

#[test]
fn preflight_binds_every_authoritative_epoch_and_fact() {
    let preflight = preflight_unit_action_come_out(1, 2, 3, 4, 5, ordinary()).unwrap();
    assert!(preflight_still_valid(&preflight, &preflight));

    let mut stale = preflight.clone();
    stale.containment_epoch += 1;
    assert!(!preflight_still_valid(&preflight, &stale));
    stale = preflight.clone();
    stale.facts.unit_masks ^= 1;
    assert!(!preflight_still_valid(&preflight, &stale));
}

#[test]
fn row_remains_red_until_the_general_release_and_live_commit_land() {
    assert_eq!(UNIT_ACTION_COME_OUT_VA, 0x005e_20b0);
    assert_eq!(UNIT_ACTION_COME_OUT_BYTES, 532);
    assert_eq!(UNIT_ACTION_COME_OUT_END_VA, 0x005e_22c4);
    assert_eq!(UNIT_COME_OUT_VA, 0x0061_7c10);
    assert_eq!(UNIT_COME_OUT_BYTES, 9_925);
    assert_eq!(UNIVERSITY_TYPE, 0x1a4);
    assert_eq!(COMPLETE_OPCODE_DELTA, 0);
    assert_eq!(UNIT_ACTION_COME_OUT_OPEN_TAILS.len(), 4);
    assert!(UNIT_ACTION_COME_OUT_OPEN_TAILS
        .contains(&UnitActionComeOutOpenTail::GeneralComeOutTransaction));
    assert!(
        UNIT_ACTION_COME_OUT_OPEN_TAILS.contains(&UnitActionComeOutOpenTail::AtomicOpcode49Commit)
    );
}
