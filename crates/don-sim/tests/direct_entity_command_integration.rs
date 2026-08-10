//! Atomic adapter pins for command rows 46 through 49.
//!
//! The integration module remains path-imported so this lane does not touch the shared
//! systems module map or dispatcher.

// The path-imported subject uses its eventual library paths.  Mirror only those two roots
// here so the shared module map can remain untouched in this lane.
mod objects {
    pub use don_sim::objects::*;
}

mod systems {
    pub mod economy {
        pub use don_sim::systems::economy::*;
    }
}

#[path = "../src/systems/direct_entity_command_integration.rs"]
mod subject;

use don_sim::systems::economy::{
    EconRules, LeaderEcon, MarketPriceGates, MarketState, RES_FOOD, RES_TIMBER, RES_WEALTH,
    TRADE_LOT,
};
use subject::plans::{
    DirectEntityCommandRequest, DirectEntityKind, DirectEntityTargetFacts, MarketCommandFacts,
    MarketCommandRequest, MarketSide, SOUND_MARKET_EMBARGO,
};
use subject::unit_action_come_out::{
    preflight_unit_action_come_out, InsideLookupFacts, ObjectIdentity, UnitActionComeOutFacts,
};
use subject::*;

fn market_facts(side: MarketSide) -> MarketCommandFacts {
    MarketCommandFacts {
        frame: 81,
        can_buy_sell: (side == MarketSide::Buy).then_some(true),
        has_tribe_bonus_4: (side == MarketSide::Sell).then_some(false),
        has_preq_0x2ad: (side == MarketSide::Sell).then_some(true),
        has_market: (side == MarketSide::Sell).then_some(true),
        nuke_embargo: Some(0),
        selected_leader_who: None,
        display_who: 2,
    }
}

fn market_request(side: MarketSide, good: i32, flags: i32) -> MarketCommandRequest {
    MarketCommandRequest {
        side,
        who: 2,
        good,
        flags,
    }
}

#[test]
fn buy_commits_through_existing_economy_primitive_and_recomputes() {
    let request = market_request(MarketSide::Buy, RES_FOOD as i32, 0);
    let rules = EconRules::shipped();
    let mut market = MarketState::default();
    market.base_price[RES_FOOD] = 50;
    let market_before = market;
    let mut econ = LeaderEcon::new();
    econ.stockpile[RES_WEALTH] = 500;
    let mut demand = 250;

    let receipt = execute_market_command(
        request,
        market_facts(MarketSide::Buy),
        Some(MarketEconomyBinding::complete(
            2,
            RES_FOOD as i32,
            &rules,
            &mut market,
            &mut econ,
            MarketCounterBinding::Demand(&mut demand),
            MarketPriceGates::default(),
        )),
    );

    assert_eq!(receipt.status, MarketTransactionStatus::Applied);
    assert!(receipt.validates(request));
    assert_eq!(receipt.presentation.len(), 1);
    assert_eq!(econ.stockpile[RES_FOOD], TRADE_LOT);
    assert!(econ.stockpile[RES_WEALTH] < 500);
    assert_eq!(demand, 150);
    assert_eq!(
        market.base_price[RES_FOOD],
        market_before.base_price[RES_FOOD] + rules.market_supply_demand()
    );
    assert_eq!(
        receipt.economy.unwrap().trace,
        MarketLoopTrace {
            attempts: 1,
            completed: 1,
            stopped_on_refusal: false,
        }
    );
}

#[test]
fn sell_stops_on_first_refusal_and_counts_that_attempt() {
    let request = market_request(MarketSide::Sell, RES_TIMBER as i32, 1);
    let rules = EconRules::shipped();
    let mut market = MarketState::default();
    market.base_price[RES_TIMBER] = 80;
    let mut econ = LeaderEcon::new();
    econ.stockpile[RES_TIMBER] = TRADE_LOT * 2;
    let mut supply = 350;

    let receipt = execute_market_command(
        request,
        market_facts(MarketSide::Sell),
        Some(MarketEconomyBinding::complete(
            2,
            RES_TIMBER as i32,
            &rules,
            &mut market,
            &mut econ,
            MarketCounterBinding::Supply(&mut supply),
            MarketPriceGates::default(),
        )),
    );

    assert!(receipt.validates(request));
    assert_eq!(econ.stockpile[RES_TIMBER], 0);
    assert_eq!(supply, 150);
    assert_eq!(
        receipt.economy.unwrap().trace,
        MarketLoopTrace {
            attempts: 3,
            completed: 2,
            stopped_on_refusal: true,
        }
    );
}

#[test]
fn missing_or_mismatched_market_facts_fail_before_mutation() {
    let request = market_request(MarketSide::Buy, RES_FOOD as i32, 0);
    let rules = EconRules::shipped();
    let mut market = MarketState::default();
    market.base_price[RES_FOOD] = 50;
    let mut econ = LeaderEcon::new();
    econ.stockpile[RES_WEALTH] = 500;
    let mut supply = 250;
    let market_before = market;
    let econ_before = econ;

    let absent = execute_market_command(request, market_facts(MarketSide::Buy), None);
    assert_eq!(absent.status, MarketTransactionStatus::Unavailable);
    assert!(absent.validates(request));

    // A buy bound to a supply counter is rejected only after all immutable identity facts
    // have been checked, but still before either state block is touched.
    let receipt = execute_market_command(
        request,
        market_facts(MarketSide::Buy),
        Some(MarketEconomyBinding::complete(
            2,
            RES_FOOD as i32,
            &rules,
            &mut market,
            &mut econ,
            MarketCounterBinding::Supply(&mut supply),
            MarketPriceGates::default(),
        )),
    );
    assert_eq!(receipt.status, MarketTransactionStatus::Unavailable);
    assert!(receipt.validates(request));
    assert_eq!(market, market_before);
    assert_eq!(econ, econ_before);
    assert_eq!(supply, 250);

    let invalid_resource = market_request(MarketSide::Buy, -1, 0);
    let invalid = execute_market_command(invalid_resource, market_facts(MarketSide::Buy), None);
    assert_eq!(invalid.status, MarketTransactionStatus::Unavailable);
    assert!(invalid.validates(invalid_resource));
}

#[test]
fn short_circuited_and_embargo_paths_need_no_economy_binding() {
    let request = market_request(MarketSide::Buy, RES_FOOD as i32, 5);
    let mut ineligible_facts = market_facts(MarketSide::Buy);
    ineligible_facts.can_buy_sell = Some(false);
    ineligible_facts.nuke_embargo = None;
    let ineligible = execute_market_command(request, ineligible_facts, None);
    assert_eq!(ineligible.status, MarketTransactionStatus::Applied);
    assert_eq!(ineligible.presentation.len(), 1);
    assert!(ineligible.economy.is_none());
    assert!(ineligible.validates(request));

    let mut embargo_facts = market_facts(MarketSide::Buy);
    embargo_facts.nuke_embargo = Some(-7);
    embargo_facts.selected_leader_who = Some(2);
    let embargo = execute_market_command(request, embargo_facts, None);
    assert_eq!(
        embargo.presentation,
        vec![
            DirectEntityPresentationReceipt::MarketDiagnostic {
                side: MarketSide::Buy,
                who: 2,
                good: RES_FOOD as i32,
                flags: 5,
                frame: 81,
            },
            DirectEntityPresentationReceipt::EmbargoUi { embargo: -7 },
            DirectEntityPresentationReceipt::Sound {
                category: SOUND_MARKET_EMBARGO,
            },
        ]
    );
    assert!(embargo.economy.is_none());
    assert!(embargo.validates(request));
}

#[test]
fn market_receipt_rejects_a_mutated_deterministic_tail() {
    let request = market_request(MarketSide::Buy, RES_FOOD as i32, 0);
    let rules = EconRules::shipped();
    let mut market = MarketState::default();
    market.base_price[RES_FOOD] = 50;
    let mut econ = LeaderEcon::new();
    econ.stockpile[RES_WEALTH] = 500;
    let mut demand = 100;
    let mut receipt = execute_market_command(
        request,
        market_facts(MarketSide::Buy),
        Some(MarketEconomyBinding::complete(
            2,
            RES_FOOD as i32,
            &rules,
            &mut market,
            &mut econ,
            MarketCounterBinding::Demand(&mut demand),
            MarketPriceGates::default(),
        )),
    );
    receipt.economy.as_mut().unwrap().counter_after ^= 1;
    assert!(!receipt.validates(request));
}

fn unit_target(active: bool, uid: u16) -> DirectEntityTargetFacts {
    DirectEntityTargetFacts {
        kind: DirectEntityKind::Unit,
        active,
        uid,
    }
}

fn ordinary_come_out_facts(who: u8, object_index: i16, type_index: i32) -> UnitActionComeOutFacts {
    UnitActionComeOutFacts {
        actor: ObjectIdentity::new(who, object_index),
        actor_type: type_index,
        unit_masks: 0x0c00_0042,
        inside: InsideLookupFacts::default(),
        actor_first_guy: None,
        actor_inside_down: None,
        inside_chain: Vec::new(),
        leader_flags: None,
    }
}

#[test]
fn unsafe_or_missing_entity_resolution_fails_closed() {
    let negative_owner = DirectEntityCommandRequest::ComeOut {
        who: -1,
        object_index: 3,
        uid: 4,
    };
    let receipt = classify_direct_entity_command(
        negative_owner,
        9,
        Some(unit_target(true, 4)),
        Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Unit,
            type_index: 50,
        }),
    );
    assert_eq!(receipt.status, DirectEntityTransactionStatus::Unavailable);
    assert!(receipt.validates(negative_owner));

    let too_wide_object = DirectEntityCommandRequest::ComeOut {
        who: 0,
        object_index: i32::from(i16::MAX) + 1,
        uid: 4,
    };
    assert_eq!(
        classify_direct_entity_command(too_wide_object, 9, Some(unit_target(true, 4)), None,)
            .status,
        DirectEntityTransactionStatus::Unavailable
    );

    let missing = DirectEntityCommandRequest::ComeOut {
        who: 0,
        object_index: 3,
        uid: 4,
    };
    assert_eq!(
        classify_direct_entity_command(missing, 9, None, None).status,
        DirectEntityTransactionStatus::Unavailable
    );
}

#[test]
fn inactive_and_stale_targets_are_complete_without_type_reads() {
    let request = DirectEntityCommandRequest::Unqueue {
        who: 1,
        object_index: 8,
        type_index: -10,
        uid: 44,
    };
    let inactive = classify_direct_entity_command(
        request,
        11,
        Some(unit_target(false, 44)),
        Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Unit,
            type_index: 310,
        }),
    );
    assert_eq!(inactive.status, DirectEntityTransactionStatus::Complete);
    assert_eq!(inactive.type_facts, None);
    assert_eq!(
        inactive.disposition,
        Some(DirectEntityDisposition::CompleteNoOp)
    );
    assert!(inactive.validates(request));

    let stale = classify_direct_entity_command(request, 11, Some(unit_target(true, 45)), None);
    assert_eq!(stale.status, DirectEntityTransactionStatus::Complete);
    assert!(stale.validates(request));
}

#[test]
fn reached_unqueue_requires_type_and_preserves_unit_build_routing() {
    let unit_request = DirectEntityCommandRequest::Unqueue {
        who: 1,
        object_index: 8,
        type_index: i32::MIN,
        uid: 44,
    };
    let missing_type =
        classify_direct_entity_command(unit_request, 11, Some(unit_target(true, 44)), None);
    assert_eq!(
        missing_type.status,
        DirectEntityTransactionStatus::Unavailable
    );

    let unit = classify_direct_entity_command(
        unit_request,
        11,
        Some(unit_target(true, 44)),
        Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Unit,
            type_index: 351,
        }),
    );
    assert_eq!(unit.status, DirectEntityTransactionStatus::OpenTail);
    assert_eq!(
        unit.disposition,
        Some(DirectEntityDisposition::OpenTail(
            DirectEntityOpenTail::ProductionUnitActionUnqueue {
                target: DirectEntityIdentity {
                    who: 1,
                    object_index: 8,
                    uid: 44,
                    type_index: 351,
                },
                argument: 1,
            }
        ))
    );
    assert!(unit.validates(unit_request));

    let build_target = DirectEntityTargetFacts {
        kind: DirectEntityKind::Build,
        active: true,
        uid: 44,
    };
    let build = classify_direct_entity_command(
        unit_request,
        11,
        Some(build_target),
        Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Build,
            type_index: 420,
        }),
    );
    assert_eq!(
        build.disposition,
        Some(DirectEntityDisposition::OpenTail(
            DirectEntityOpenTail::ProductionBuildActionUnqueue {
                target: DirectEntityIdentity {
                    who: 1,
                    object_index: 8,
                    uid: 44,
                    type_index: 420,
                },
                selector: i32::MIN,
            }
        ))
    );
    assert!(build.validates(unit_request));
}

#[test]
fn come_out_classifies_supported_scholar_prefix_without_claiming_completion() {
    let request = DirectEntityCommandRequest::ComeOut {
        who: 3,
        object_index: 19,
        uid: 70,
    };
    let scholar = classify_direct_entity_command(
        request,
        -2,
        Some(unit_target(true, 70)),
        Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Unit,
            type_index: 0x34,
        }),
    );
    assert_eq!(
        scholar.disposition,
        Some(DirectEntityDisposition::OpenTail(
            DirectEntityOpenTail::ContainmentScholarActionComeOut {
                target: DirectEntityIdentity {
                    who: 3,
                    object_index: 19,
                    uid: 70,
                    type_index: 0x34,
                },
            }
        ))
    );
    assert_eq!(scholar.status, DirectEntityTransactionStatus::OpenTail);
    assert!(scholar.validates(request));

    let general = classify_direct_entity_command(
        request,
        -2,
        Some(unit_target(true, 70)),
        Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Unit,
            type_index: 50,
        }),
    );
    assert!(matches!(
        general.disposition,
        Some(DirectEntityDisposition::OpenTail(
            DirectEntityOpenTail::ContainmentGeneralActionComeOut { .. }
        ))
    ));

    let wrong_abi = classify_direct_entity_command(
        request,
        -2,
        Some(DirectEntityTargetFacts {
            kind: DirectEntityKind::Build,
            active: true,
            uid: 70,
        }),
        Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Build,
            type_index: 420,
        }),
    );
    assert_eq!(wrong_abi.status, DirectEntityTransactionStatus::Unavailable);
}

#[test]
fn come_out_wrapper_preflight_advances_both_type_routes_to_the_general_tail() {
    let request = DirectEntityCommandRequest::ComeOut {
        who: 3,
        object_index: 19,
        uid: 70,
    };
    let target = Some(unit_target(true, 70));

    for type_index in [0x34, 50] {
        let type_facts = Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Unit,
            type_index,
        });
        let preflight = preflight_unit_action_come_out(
            101,
            102,
            103,
            104,
            105,
            ordinary_come_out_facts(3, 19, type_index),
        )
        .unwrap();
        let receipt = preflight_opcode49_unit_action_come_out_command(
            request,
            -2,
            target,
            type_facts,
            preflight.clone(),
        );

        assert_eq!(receipt.status, DirectEntityTransactionStatus::OpenTail);
        assert_eq!(receipt.unit_action_come_out, Some(preflight));
        assert_eq!(receipt.unit_unqueue, None);
        assert_eq!(
            receipt.disposition,
            Some(DirectEntityDisposition::OpenTail(
                DirectEntityOpenTail::GeneralUnitComeOutTransaction {
                    target: DirectEntityIdentity {
                        who: 3,
                        object_index: 19,
                        uid: 70,
                        type_index,
                    },
                    argument: 0,
                }
            ))
        );
        assert!(receipt.validates(request));
    }
}

#[test]
fn come_out_wrapper_preflight_fails_closed_on_identity_type_or_plan_drift() {
    let request = DirectEntityCommandRequest::ComeOut {
        who: 3,
        object_index: 19,
        uid: 70,
    };
    let target = Some(unit_target(true, 70));
    let type_facts = Some(DirectEntityTypeFacts {
        kind: DirectEntityKind::Unit,
        type_index: 50,
    });
    let make_preflight = |who, object_index, type_index| {
        preflight_unit_action_come_out(
            101,
            102,
            103,
            104,
            105,
            ordinary_come_out_facts(who, object_index, type_index),
        )
        .unwrap()
    };

    for mismatched in [
        make_preflight(2, 19, 50),
        make_preflight(3, 20, 50),
        make_preflight(3, 19, 51),
    ] {
        let receipt = preflight_opcode49_unit_action_come_out_command(
            request, 9, target, type_facts, mismatched,
        );
        assert_eq!(receipt.status, DirectEntityTransactionStatus::Unavailable);
        assert!(receipt.validates(request));
    }

    let mut malformed = make_preflight(3, 19, 50);
    malformed.plan.steps.pop();
    let receipt =
        preflight_opcode49_unit_action_come_out_command(request, 9, target, type_facts, malformed);
    assert_eq!(receipt.status, DirectEntityTransactionStatus::Unavailable);
    assert!(receipt.validates(request));

    let valid = preflight_opcode49_unit_action_come_out_command(
        request,
        9,
        target,
        type_facts,
        make_preflight(3, 19, 50),
    );
    let mut changed_tail = valid.clone();
    changed_tail.disposition = Some(DirectEntityDisposition::OpenTail(
        DirectEntityOpenTail::GeneralUnitComeOutTransaction {
            target: DirectEntityIdentity {
                who: 3,
                object_index: 19,
                uid: 70,
                type_index: 50,
            },
            argument: 1,
        },
    ));
    assert!(!changed_tail.validates(request));

    let mut changed_plan = valid;
    changed_plan
        .unit_action_come_out
        .as_mut()
        .unwrap()
        .plan
        .steps
        .pop();
    assert!(!changed_plan.validates(request));
}

#[test]
fn entity_receipt_rejects_changed_type_or_open_tail() {
    let request = DirectEntityCommandRequest::ComeOut {
        who: 3,
        object_index: 19,
        uid: 70,
    };
    let mut receipt = classify_direct_entity_command(
        request,
        2,
        Some(unit_target(true, 70)),
        Some(DirectEntityTypeFacts {
            kind: DirectEntityKind::Unit,
            type_index: 50,
        }),
    );
    receipt.type_facts.as_mut().unwrap().type_index = 0x34;
    assert!(!receipt.validates(request));
}

#[test]
fn frozen_fleet_envelope_is_variant_exact_and_fail_closed_by_default() {
    struct UnavailableFleet;
    impl DirectEntityFleetHandoff for UnavailableFleet {}

    let mut fleet = UnavailableFleet;
    let market = DirectEntityFleetRequest::Market {
        request: market_request(MarketSide::Buy, RES_FOOD as i32, 0),
        frame: 81,
    };
    let market_receipt = fleet.apply_direct_entity_command_transaction(market);
    assert!(market_receipt.validates(market));

    let entity_request = DirectEntityCommandRequest::ComeOut {
        who: 0,
        object_index: 1,
        uid: 2,
    };
    let entity = DirectEntityFleetRequest::Entity {
        request: entity_request,
        frame: 81,
    };
    let entity_receipt = fleet.apply_direct_entity_command_transaction(entity);
    assert!(entity_receipt.validates(entity));
    assert!(!entity_receipt.validates(market));

    let complete = DirectEntityFleetReceipt::Entity(classify_direct_entity_command(
        entity_request,
        81,
        Some(unit_target(false, 2)),
        None,
    ));
    assert!(complete.validates(entity));
    assert!(!complete.validates(DirectEntityFleetRequest::Entity {
        request: entity_request,
        frame: 82,
    }));
}
