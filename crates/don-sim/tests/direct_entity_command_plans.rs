//! Path-import mutation pins for command rows 46 through 49.
//!
//! The source remains intentionally unwired during the swarm wave.  Importing it by path
//! lets convergence inspect these pins without editing the shared dispatcher or module map.

#[path = "../src/systems/direct_entity_command_plans.rs"]
mod subject;

use subject::*;

fn market_wire(opcode: u8, who: i32, good: i32, flags: i32) -> [u8; MARKET_WIRE_BYTES] {
    let mut wire = [0; MARKET_WIRE_BYTES];
    wire[0] = opcode;
    wire[1..5].copy_from_slice(&who.to_le_bytes());
    wire[5..9].copy_from_slice(&good.to_le_bytes());
    wire[9..13].copy_from_slice(&flags.to_le_bytes());
    wire
}

fn market_facts() -> MarketCommandFacts {
    MarketCommandFacts {
        frame: i32::MIN,
        can_buy_sell: Some(true),
        has_tribe_bonus_4: Some(false),
        has_preq_0x2ad: Some(true),
        has_market: Some(true),
        nuke_embargo: Some(0),
        selected_leader_who: None,
        display_who: 3,
    }
}

fn unqueue_wire(
    who: i32,
    object_index: i32,
    type_index: i32,
    uid: i16,
) -> [u8; UNQUEUE_WIRE_BYTES] {
    let mut wire = [0; UNQUEUE_WIRE_BYTES];
    wire[0] = UNQUEUE_OPCODE;
    wire[1..5].copy_from_slice(&who.to_le_bytes());
    wire[5..9].copy_from_slice(&object_index.to_le_bytes());
    wire[9..13].copy_from_slice(&type_index.to_le_bytes());
    wire[13..15].copy_from_slice(&uid.to_le_bytes());
    wire
}

fn come_out_wire(who: i32, object_index: i32, uid: i16) -> [u8; COME_OUT_WIRE_BYTES] {
    let mut wire = [0; COME_OUT_WIRE_BYTES];
    wire[0] = COME_OUT_OPCODE;
    wire[1..5].copy_from_slice(&who.to_le_bytes());
    wire[5..9].copy_from_slice(&object_index.to_le_bytes());
    wire[9..11].copy_from_slice(&uid.to_le_bytes());
    wire
}

#[test]
fn mutation_market_decoder_preserves_signed_dwords_and_exact_size() {
    let buy = decode_market_command(&market_wire(BUY_OPCODE, i32::MIN, -1, i32::MAX)).unwrap();
    assert_eq!(
        buy,
        MarketCommandRequest {
            side: MarketSide::Buy,
            who: i32::MIN,
            good: -1,
            flags: i32::MAX,
        }
    );

    let sell = decode_market_command(&market_wire(SELL_OPCODE, 7, 2, -8)).unwrap();
    assert_eq!(sell.side, MarketSide::Sell);
    assert_eq!(sell.flags, -8);

    assert!(decode_market_command(&market_wire(0, 0, 0, 0)).is_none());
    assert!(decode_market_command(&market_wire(BUY_OPCODE, 0, 0, 0)[..12]).is_none());
}

#[test]
fn mutation_buy_flag_four_expands_ten_or_hundred_and_bit_two_wins() {
    assert_eq!(market_attempt_limit(MarketSide::Buy, 0), 1);
    assert_eq!(market_attempt_limit(MarketSide::Buy, 1), 5);
    assert_eq!(market_attempt_limit(MarketSide::Buy, 4), 10);
    assert_eq!(market_attempt_limit(MarketSide::Buy, 5), 100);
    assert_eq!(market_attempt_limit(MarketSide::Buy, 2), 99_999);
    assert_eq!(market_attempt_limit(MarketSide::Buy, 7), 99_999);
    assert_eq!(market_attempt_limit(MarketSide::Buy, -1), 99_999);
}

#[test]
fn mutation_sell_ignores_flag_four_but_honours_one_and_two() {
    assert_eq!(market_attempt_limit(MarketSide::Sell, 0), 1);
    assert_eq!(market_attempt_limit(MarketSide::Sell, 1), 5);
    assert_eq!(market_attempt_limit(MarketSide::Sell, 4), 1);
    assert_eq!(market_attempt_limit(MarketSide::Sell, 5), 5);
    assert_eq!(market_attempt_limit(MarketSide::Sell, 6), 99_999);
    assert_eq!(market_attempt_limit(MarketSide::Sell, -1), 99_999);
}

#[test]
fn mutation_market_gates_read_only_the_reached_branch() {
    let buy = decode_market_command(&market_wire(BUY_OPCODE, 2, -1, 0)).unwrap();
    let mut facts = market_facts();
    facts.can_buy_sell = Some(false);
    facts.nuke_embargo = None;
    let plan = plan_market_command(buy, &facts).unwrap();
    assert_eq!(plan.effects.len(), 1);
    assert!(!plan.downstream_required);

    let sell = decode_market_command(&market_wire(SELL_OPCODE, 2, -1, 0)).unwrap();
    facts.can_buy_sell = None;
    facts.has_tribe_bonus_4 = Some(true);
    facts.has_preq_0x2ad = None;
    facts.has_market = Some(true);
    facts.nuke_embargo = Some(0);
    assert!(
        plan_market_command(sell, &facts)
            .unwrap()
            .downstream_required
    );

    facts.has_tribe_bonus_4 = Some(false);
    facts.has_preq_0x2ad = Some(false);
    facts.has_market = None;
    facts.nuke_embargo = None;
    let no_preq = plan_market_command(sell, &facts).unwrap();
    assert_eq!(no_preq.effects.len(), 1);

    facts.has_preq_0x2ad = Some(true);
    assert!(plan_market_command(sell, &facts).is_none());
}

#[test]
fn mutation_embargo_presentation_is_local_only_and_ordered() {
    let request = decode_market_command(&market_wire(BUY_OPCODE, 9, 2, 5)).unwrap();
    let mut facts = market_facts();
    facts.nuke_embargo = Some(-7);
    facts.selected_leader_who = Some(3);
    let local = plan_market_command(request, &facts).unwrap();
    assert_eq!(
        &local.effects[1..],
        &[
            MarketCommandEffect::ShowEmbargo { embargo: -7 },
            MarketCommandEffect::Audio {
                category: SOUND_MARKET_EMBARGO,
            },
        ]
    );
    assert!(!local.downstream_required);

    facts.selected_leader_who = Some(4);
    let remote = plan_market_command(request, &facts).unwrap();
    assert_eq!(remote.effects.len(), 1);
    assert!(!remote.downstream_required);

    facts.selected_leader_who = None;
    assert!(plan_market_command(request, &facts).is_none());
}

#[test]
fn mutation_market_delegate_keeps_signed_ids_and_exact_attempt_limit() {
    let request = decode_market_command(&market_wire(BUY_OPCODE, i32::MIN, -9, 5)).unwrap();
    let facts = market_facts();
    let plan = plan_market_command(request, &facts).unwrap();
    assert_eq!(
        plan.effects[1],
        MarketCommandEffect::DelegateMarketLoop {
            side: MarketSide::Buy,
            who: i32::MIN,
            good: -9,
            max_attempts: 100,
        }
    );
    assert!(plan.downstream_required);
}

#[test]
fn mutation_market_receipt_recomputes_and_rejects_changed_tail() {
    let request = decode_market_command(&market_wire(SELL_OPCODE, 4, 1, 1)).unwrap();
    let facts = market_facts();
    let plan = plan_market_command(request, &facts).unwrap();
    let receipt = MarketCommandReceipt {
        request,
        facts: Some(facts),
        status: PlanStatus::Planned,
        plan: Some(plan),
    };
    assert!(receipt.validates(request));

    let mut mutated = receipt;
    mutated.plan.as_mut().unwrap().downstream_required = false;
    assert!(!mutated.validates(request));
    assert!(MarketCommandReceipt::unavailable(request).validates(request));

    let mut missing_reached_fact = facts;
    missing_reached_fact.has_market = None;
    let false_planned = MarketCommandReceipt {
        request,
        facts: Some(missing_reached_fact),
        status: PlanStatus::Planned,
        plan: None,
    };
    assert!(!false_planned.validates(request));
}

#[test]
fn mutation_entity_decoders_preserve_signed_indexes_type_and_uid() {
    let unqueue =
        decode_direct_entity_command(&unqueue_wire(i32::MIN, -2, i32::MAX, i16::MIN)).unwrap();
    assert_eq!(
        unqueue,
        DirectEntityCommandRequest::Unqueue {
            who: i32::MIN,
            object_index: -2,
            type_index: i32::MAX,
            uid: i16::MIN,
        }
    );

    let come_out = decode_direct_entity_command(&come_out_wire(-1, i32::MAX, i16::MAX)).unwrap();
    assert_eq!(
        come_out,
        DirectEntityCommandRequest::ComeOut {
            who: -1,
            object_index: i32::MAX,
            uid: i16::MAX,
        }
    );

    assert!(decode_direct_entity_command(&unqueue_wire(0, 0, 0, 0)[..14]).is_none());
    assert!(decode_direct_entity_command(&come_out_wire(0, 0, 0)[..10]).is_none());
}

#[test]
fn mutation_uid_compare_does_not_alias_negative_wire_uid_to_unsigned_object_uid() {
    assert!(entity_uid_matches(0, 0));
    assert!(entity_uid_matches(i16::MAX as u16, i16::MAX));
    assert!(!entity_uid_matches(u16::MAX, -1));
    assert!(!entity_uid_matches(0x8000, i16::MIN));
}

#[test]
fn mutation_unqueue_class_controls_argument_and_unit_ignores_wire_type() {
    let request = decode_direct_entity_command(&unqueue_wire(2, 17, i32::MIN, 300)).unwrap();
    let mut target = DirectEntityTargetFacts {
        kind: DirectEntityKind::Unit,
        active: true,
        uid: 300,
    };
    let unit = plan_direct_entity_command(request, -8, &target).unwrap();
    assert_eq!(
        unit.effects[1],
        DirectEntityCommandEffect::DelegateUnitActionUnqueue {
            who: 2,
            object_index: 17,
            argument: 1,
        }
    );

    target.kind = DirectEntityKind::Build;
    let build = plan_direct_entity_command(request, -8, &target).unwrap();
    assert_eq!(
        build.effects[1],
        DirectEntityCommandEffect::DelegateBuildActionUnqueue {
            who: 2,
            object_index: 17,
            type_index: i32::MIN,
        }
    );
}

#[test]
fn mutation_inactive_or_stale_entity_is_exact_noop_after_diagnostic() {
    let request = decode_direct_entity_command(&unqueue_wire(2, 17, 9, 300)).unwrap();
    let mut target = DirectEntityTargetFacts {
        kind: DirectEntityKind::Build,
        active: false,
        uid: 300,
    };
    let inactive = plan_direct_entity_command(request, 7, &target).unwrap();
    assert_eq!(inactive.effects.len(), 1);
    assert!(!inactive.downstream_required);

    target.active = true;
    target.uid = 301;
    let stale = plan_direct_entity_command(request, 7, &target).unwrap();
    assert_eq!(stale.effects.len(), 1);
}

#[test]
fn mutation_come_out_requires_unit_abi_and_preserves_target() {
    let request = decode_direct_entity_command(&come_out_wire(6, -3, 22)).unwrap();
    let mut target = DirectEntityTargetFacts {
        kind: DirectEntityKind::Unit,
        active: true,
        uid: 22,
    };
    let plan = plan_direct_entity_command(request, i32::MAX, &target).unwrap();
    assert_eq!(
        plan.effects,
        vec![
            DirectEntityCommandEffect::Diagnostic {
                request,
                frame: i32::MAX,
            },
            DirectEntityCommandEffect::DelegateUnitActionComeOut {
                who: 6,
                object_index: -3,
            },
        ]
    );

    target.kind = DirectEntityKind::Build;
    assert!(plan_direct_entity_command(request, i32::MAX, &target).is_none());
}

#[test]
fn mutation_entity_receipt_recomputes_and_unavailable_carries_no_host_fact() {
    let request = decode_direct_entity_command(&come_out_wire(1, 2, 3)).unwrap();
    let target = DirectEntityTargetFacts {
        kind: DirectEntityKind::Unit,
        active: true,
        uid: 3,
    };
    let plan = plan_direct_entity_command(request, 4, &target).unwrap();
    let receipt = DirectEntityCommandReceipt {
        request,
        frame: Some(4),
        target: Some(target),
        status: PlanStatus::Planned,
        plan: Some(plan),
    };
    assert!(receipt.validates(request));

    let mut mutated = receipt;
    mutated.target.as_mut().unwrap().uid ^= 1;
    assert!(!mutated.validates(request));
    assert!(DirectEntityCommandReceipt::unavailable(request).validates(request));

    let false_planned = DirectEntityCommandReceipt {
        request,
        frame: Some(4),
        target: Some(DirectEntityTargetFacts {
            kind: DirectEntityKind::Build,
            active: true,
            uid: 3,
        }),
        status: PlanStatus::Planned,
        plan: None,
    };
    assert!(!false_planned.validates(request));
}
