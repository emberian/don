// SPDX-License-Identifier: GPL-3.0-or-later
//! Contract tests for the unexported canonical economy/containment Group host.

// The subject is written against its eventual library paths. Forward only those paths so
// the exclusive module can compile before it is registered in `systems/mod.rs`.
mod command {
    pub mod economy_group_actions {
        pub use don_sim::command::economy_group_actions::*;
    }
}
mod systems {
    pub mod groups_guys {
        pub use don_sim::systems::groups_guys::*;
    }
}

#[path = "../src/systems/economy_containment_group_host.rs"]
mod subject;

use command::economy_group_actions::{Fact, MemberFacts, TradeTargetFacts, REPAIR_CAST_SPELL_TYPE};
use subject::*;
use systems::groups_guys::{GroupData, Groups};

const OWNER: u8 = 2;
const SLOT: u8 = 7;

fn digest(byte: u8) -> CanonicalStateDigest {
    CanonicalStateDigest([byte; 32])
}

fn canonical_group(members: &[i16]) -> Groups {
    let mut groups = Groups::default();
    let group = groups.get_mut(OWNER as usize, SLOT as usize);
    *group = GroupData {
        id: 91,
        who: OWNER,
        num: members.len() as i32,
        form: 5,
        disband: 12,
        ..GroupData::default()
    };
    for (index, &o) in members.iter().enumerate() {
        group.list[index] = o;
    }
    groups
}

fn identity(owner: u8, o: i16, uid: u16) -> CanonicalObjectIdentity {
    CanonicalObjectIdentity {
        owner,
        o,
        uid,
        generation: u32::from(uid) + 100,
    }
}

fn group_packet(owner: u8, members: &[i16]) -> Vec<u8> {
    let mut packet = vec![GROUP_OPCODE, members.len() as u8, owner];
    for member in members {
        packet.extend_from_slice(&member.to_le_bytes());
    }
    packet
}

fn action_packet(opcode: u8, fields: &[i32]) -> Vec<u8> {
    let mut packet = vec![opcode];
    for field in fields {
        packet.extend_from_slice(&field.to_le_bytes());
    }
    packet
}

fn position(group_command_index: u16, action_command_index: u16) -> CommandPackagePosition {
    CommandPackagePosition {
        game_frame: 1151,
        package_serial: 60402,
        play: 0,
        group_command_index,
        action_command_index,
    }
}

fn retail_last_group() -> [i32; 8] {
    std::array::from_fn(|owner| (owner * 64) as i32)
}

fn exact_member(o: i16) -> CanonicalMemberFacts {
    let mut retail = MemberFacts::opaque(o, true, true, 100, 200);
    retail.type_class = Fact::known(0x32);
    retail.domain = Fact::known(0);
    retail.busy = Fact::known(false);
    retail.regions_touch = Fact::known(true);
    retail.repair_spell_castable = Fact::known(true);
    retail.is_caravan = Fact::known(true);
    retail.is_sea_trade_member = Fact::known(false);
    retail.ship_can_carry = Fact::known(true);
    CanonicalMemberFacts {
        identity: identity(OWNER, o, o as u16 + 10),
        retail,
    }
}

fn caps() -> EconomyOrderCapabilities {
    EconomyOrderCapabilities {
        format_version: REQUIRED_ORDER_FORMAT_VERSION,
        typed_cast_v1: true,
        typed_trade_v1: true,
        exact_group_queue_first: true,
    }
}

fn commit(plan: &CanonicalEconomyGroupPlan) -> EconomyAtomicCommitEvidence {
    EconomyAtomicCommitEvidence {
        state_before: plan.request.state_before(),
        state_after: digest(2),
        group_after: plan.group_after.clone(),
        reloaded_state: digest(2),
        reloaded_group: plan.group_after.clone(),
        direct_next_tick: digest(3),
        reloaded_next_tick: digest(3),
    }
}

#[test]
fn real_group_action_wire_pairs_are_exact_and_package_ordered() {
    let group = group_packet(OWNER, &[4, 5]);
    let cases = [
        (
            action_packet(BOARD_SHIP_OPCODE, &[9, 2]),
            EconomyGroupAction::BoardShip {
                ship_o: 9,
                queued: 2,
            },
        ),
        (
            action_packet(REPAIR_OPCODE, &[8, 3, 1]),
            EconomyGroupAction::Repair {
                target_o: 8,
                target_owner: 3,
                queued: 1,
            },
        ),
        (
            action_packet(TRADE_OPCODE, &[8, 3, 9, 4, 0]),
            EconomyGroupAction::Trade {
                first_o: 8,
                first_owner: 3,
                second_o: 9,
                second_owner: 4,
                queued: 0,
            },
        ),
    ];
    for (action_packet, action) in cases {
        let pair = decode_economy_group_packet_pair(position(0, 1), &group, &action_packet)
            .expect("exact pair");
        assert_eq!(pair.group.owner, OWNER);
        assert_eq!(pair.group.requested, vec![4, 5]);
        assert_eq!(pair.action, action);
    }

    assert_eq!(
        decode_economy_group_packet_pair(
            position(1, 1),
            &group,
            &action_packet(BOARD_SHIP_OPCODE, &[9, 2]),
        ),
        Err(EconomyGroupWireError::ActionPrecedesGroup {
            group_command_index: 1,
            action_command_index: 1,
        })
    );
    assert!(matches!(
        decode_economy_group_action_packet(&[BOARD_SHIP_OPCODE, 0]),
        Err(EconomyGroupWireError::WrongSize {
            opcode: BOARD_SHIP_OPCODE,
            expected: BOARD_SHIP_WIRE_SIZE,
            actual: 2,
        })
    ));
}

#[test]
fn typed_v12_contracts_match_the_three_retail_walks() {
    let cases = [
        (EconomyOrderPayloadContract::GatherV1, 2, 20, 31),
        (EconomyOrderPayloadContract::CastV1, 3, 8, 27),
        (EconomyOrderPayloadContract::TradeV1, 4, 18, 29),
    ];
    for (contract, tag, extra, walked) in cases {
        assert_eq!(contract.tag(), tag);
        assert_eq!(contract.payload_version(), 1);
        assert_eq!(contract.v12_extra_bytes(), extra);
        assert_eq!(contract.retail_walk_bytes(), walked);
    }
}

#[test]
fn cached_zero_count_is_not_an_empty_group_and_stale_cache_can_drop_the_action() {
    let group = group_packet(OWNER, &[]);
    let pair = decode_economy_group_packet_pair(
        position(0, 1),
        &group,
        &action_packet(BOARD_SHIP_OPCODE, &[9, 2]),
    )
    .unwrap();
    assert!(pair.group.is_cached_reselection());

    let cache = CanonicalSelectionCacheImage {
        revision: 7,
        entries: vec![CachedSelectionIdentity { o: 4, uid: 14 }],
    };
    let last_group = retail_last_group();
    let receipt = CanonicalGroupSelectionReceipt {
        pair,
        cache_before: cache.clone(),
        cache_after: cache,
        selected: vec![],
        group_mutations: vec![],
        member_backlinks: vec![],
        last_group_before: last_group,
        last_group_after: last_group,
        state_before: digest(1),
        state_after: digest(1),
        result: CanonicalGroupSelectionResult::Dropped,
    };
    let mut staged = Groups::default();
    staged.last_group = last_group;
    assert!(receipt.validates_staged_groups(&staged));
    assert!(EconomyGroupTransactionRequest::snapshot_from_selection(
        &staged,
        receipt,
        false,
        digest(1),
    )
    .is_none());
}

#[test]
fn fixed_group_allocation_receipt_stages_selection_before_the_action() {
    let mut before = canonical_group(&[]);
    let mut staged = canonical_group(&[4, 5]);
    let last_before = retail_last_group();
    let absolute_slot = (usize::from(OWNER) * 64 + usize::from(SLOT)) as i32;
    let mut last_after = last_before;
    last_after[usize::from(OWNER)] = absolute_slot;
    before.last_group = last_before;
    staged.last_group = last_after;

    let pair = decode_economy_group_packet_pair(
        position(0, 1),
        &group_packet(OWNER, &[4, 5]),
        &action_packet(BOARD_SHIP_OPCODE, &[9, 2]),
    )
    .unwrap();
    let selected = vec![identity(OWNER, 4, 14), identity(OWNER, 5, 15)];
    let receipt = CanonicalGroupSelectionReceipt {
        pair,
        cache_before: CanonicalSelectionCacheImage {
            revision: 10,
            entries: vec![],
        },
        cache_after: CanonicalSelectionCacheImage {
            revision: 11,
            entries: vec![
                CachedSelectionIdentity { o: 4, uid: 14 },
                CachedSelectionIdentity { o: 5, uid: 15 },
            ],
        },
        selected: selected.clone(),
        group_mutations: vec![CanonicalGroupMutation {
            locator: CanonicalGroupLocator {
                owner: OWNER,
                slot: SLOT,
            },
            before: before.get(OWNER as usize, SLOT as usize).clone(),
            after: staged.get(OWNER as usize, SLOT as usize).clone(),
        }],
        member_backlinks: selected
            .iter()
            .copied()
            .map(|identity| CanonicalMemberGroupBacklink {
                identity,
                before: -1,
                after: absolute_slot as i16,
            })
            .collect(),
        last_group_before: last_before,
        last_group_after: last_after,
        state_before: digest(0),
        state_after: digest(1),
        result: CanonicalGroupSelectionResult::Selected {
            locator: CanonicalGroupLocator {
                owner: OWNER,
                slot: SLOT,
            },
            retained_group_id: 91,
        },
    };
    assert!(receipt.validates_staged_groups(&staged));
    let request = EconomyGroupTransactionRequest::snapshot_from_selection(
        &staged,
        receipt.clone(),
        false,
        digest(1),
    )
    .unwrap();
    assert_eq!(request.action(), receipt.pair.action);
    assert_eq!(request.selection_receipt(), Some(&receipt));

    let mut stale = staged;
    stale.last_group[usize::from(OWNER)] = last_before[usize::from(OWNER)];
    assert!(!receipt.validates_staged_groups(&stale));
}

#[test]
fn request_can_only_be_snapshotted_from_the_fixed_canonical_pool() {
    let groups = canonical_group(&[4]);
    let request = EconomyGroupTransactionRequest::snapshot(
        &groups,
        CanonicalGroupLocator {
            owner: OWNER,
            slot: SLOT,
        },
        false,
        EconomyGroupAction::BoardShip {
            ship_o: 9,
            queued: 2,
        },
        digest(1),
    )
    .unwrap();
    assert_eq!(request.group_id(), 91);
    assert!(request.still_current(&groups, digest(1)));

    let mut changed = groups;
    changed.get_mut(OWNER as usize, SLOT as usize).form = -1;
    assert!(!request.still_current(&changed, digest(1)));
    assert!(!request.still_current(&canonical_group(&[4]), digest(9)));
}

#[test]
fn board_ship_preflight_uses_stable_member_and_ship_identities() {
    let groups = canonical_group(&[4, 5]);
    let request = EconomyGroupTransactionRequest::snapshot(
        &groups,
        CanonicalGroupLocator {
            owner: OWNER,
            slot: SLOT,
        },
        false,
        EconomyGroupAction::BoardShip {
            ship_o: 9,
            queued: 2,
        },
        digest(1),
    )
    .unwrap();
    let facts = EconomyGroupActionFacts::BoardShip {
        ship: Some(identity(OWNER, 9, 70)),
        members: vec![exact_member(4), exact_member(5)],
    };
    let plan = preflight_economy_group_transaction(&request, &facts, caps()).unwrap();
    assert!(plan.action_reached);
    assert_eq!(plan.group_after.form, -1);
    assert_eq!(plan.group_after.disband, 0);
    assert_eq!(
        plan.action_plan.as_ref().unwrap().steps.len(),
        6,
        "form + two board orders + one clear + two await orders"
    );

    let mut stale = facts;
    if let EconomyGroupActionFacts::BoardShip { members, .. } = &mut stale {
        members[1].identity.o = 6;
    }
    assert_eq!(
        preflight_economy_group_transaction(&request, &stale, caps()),
        Err(EconomyGroupPreflightError::MemberIdentity)
    );

    let wrong_ship = EconomyGroupActionFacts::BoardShip {
        ship: Some(identity(OWNER, 8, 70)),
        members: vec![exact_member(4), exact_member(5)],
    };
    assert_eq!(
        preflight_economy_group_transaction(&request, &wrong_ship, caps()),
        Err(EconomyGroupPreflightError::TargetIdentity)
    );
}

#[test]
fn missing_command_target_is_an_exact_noop_without_payload_dependencies() {
    let groups = canonical_group(&[4]);
    let request = EconomyGroupTransactionRequest::snapshot(
        &groups,
        CanonicalGroupLocator {
            owner: OWNER,
            slot: SLOT,
        },
        false,
        EconomyGroupAction::Trade {
            first_o: 8,
            first_owner: 3,
            second_o: 9,
            second_owner: 4,
            queued: 0,
        },
        digest(1),
    )
    .unwrap();
    let facts = EconomyGroupActionFacts::Trade {
        first: None,
        second: None,
        target: TradeTargetFacts::opaque(),
        members: vec![],
    };
    let plan =
        preflight_economy_group_transaction(&request, &facts, EconomyOrderCapabilities::default())
            .unwrap();
    assert!(!plan.action_reached);
    assert_eq!(plan.group_after, *request.group_before());
    assert!(plan.action_plan.is_none());
}

#[test]
fn repair_requires_real_cast_payload_when_the_retail_plan_installs_one() {
    let groups = canonical_group(&[4]);
    let request = EconomyGroupTransactionRequest::snapshot(
        &groups,
        CanonicalGroupLocator {
            owner: OWNER,
            slot: SLOT,
        },
        false,
        EconomyGroupAction::Repair {
            target_o: 8,
            target_owner: 3,
            queued: 2,
        },
        digest(1),
    )
    .unwrap();
    let facts = EconomyGroupActionFacts::Repair {
        target: Some(identity(3, 8, 55)),
        members: vec![exact_member(4)],
    };
    let mut no_cast = caps();
    no_cast.typed_cast_v1 = false;
    assert_eq!(
        preflight_economy_group_transaction(&request, &facts, no_cast),
        Err(EconomyGroupPreflightError::TypedCastPayload)
    );
    let plan = preflight_economy_group_transaction(&request, &facts, caps()).unwrap();
    assert!(plan
        .action_plan
        .as_ref()
        .unwrap()
        .steps
        .iter()
        .any(|step| matches!(
            step,
            command::economy_group_actions::EconomyStep::AddCastOrder { spell, .. }
                if *spell == REPAIR_CAST_SPELL_TYPE
        )));
}

#[test]
fn trade_requires_two_stable_targets_typed_payload_and_exact_queue_first() {
    let groups = canonical_group(&[4]);
    let request = EconomyGroupTransactionRequest::snapshot(
        &groups,
        CanonicalGroupLocator {
            owner: OWNER,
            slot: SLOT,
        },
        false,
        EconomyGroupAction::Trade {
            first_o: 8,
            first_owner: 3,
            second_o: 9,
            second_owner: 4,
            queued: 0,
        },
        digest(1),
    )
    .unwrap();
    let target = TradeTargetFacts {
        live_building: Fact::known(true),
        active: Fact::known(true),
        build_is_trade: Fact::known(true),
        is_trade: Fact::known(true),
        is_sea_trade: Fact::known(false),
        group_has_trader: Fact::known(true),
    };
    let facts = EconomyGroupActionFacts::Trade {
        first: Some(identity(3, 8, 80)),
        second: Some(identity(4, 9, 90)),
        target,
        members: vec![exact_member(4)],
    };
    let mut capabilities = caps();
    capabilities.exact_group_queue_first = false;
    assert_eq!(
        preflight_economy_group_transaction(&request, &facts, capabilities),
        Err(EconomyGroupPreflightError::ExactGroupQueueFirst)
    );
    capabilities.exact_group_queue_first = true;
    capabilities.typed_trade_v1 = false;
    assert_eq!(
        preflight_economy_group_transaction(&request, &facts, capabilities),
        Err(EconomyGroupPreflightError::TypedTradePayload)
    );
}

#[test]
fn unknown_reached_trade_gate_cannot_be_accepted_as_a_complete_noop() {
    let groups = canonical_group(&[4]);
    let request = EconomyGroupTransactionRequest::snapshot(
        &groups,
        CanonicalGroupLocator {
            owner: OWNER,
            slot: SLOT,
        },
        false,
        EconomyGroupAction::Trade {
            first_o: 8,
            first_owner: 3,
            second_o: 9,
            second_owner: 4,
            queued: 2,
        },
        digest(1),
    )
    .unwrap();
    let mut target = TradeTargetFacts::opaque();
    target.live_building = Fact::known(true);
    target.active = Fact::known(false);
    let facts = EconomyGroupActionFacts::Trade {
        first: Some(identity(3, 8, 80)),
        second: Some(identity(4, 9, 90)),
        target,
        members: vec![exact_member(4)],
    };
    // The active=false branch is a known early return; later unknown facts are not read.
    let plan = preflight_economy_group_transaction(&request, &facts, caps()).unwrap();
    assert!(plan.action_reached);
    assert!(plan.action_plan.is_none());

    target.active = Fact::unknown("WallData::is_active");
    let EconomyGroupActionFacts::Trade {
        first,
        second,
        members,
        ..
    } = facts
    else {
        unreachable!()
    };
    let unknown = EconomyGroupActionFacts::Trade {
        first,
        second,
        target,
        members,
    };
    assert_eq!(
        preflight_economy_group_transaction(&request, &unknown, caps()),
        Err(EconomyGroupPreflightError::UnknownRetailFact)
    );
}

#[test]
fn applied_receipt_recomputes_plan_and_pins_save_reload_and_resume() {
    let groups = canonical_group(&[4]);
    let request = EconomyGroupTransactionRequest::snapshot(
        &groups,
        CanonicalGroupLocator {
            owner: OWNER,
            slot: SLOT,
        },
        false,
        EconomyGroupAction::BoardShip {
            ship_o: 9,
            queued: 2,
        },
        digest(1),
    )
    .unwrap();
    let facts = EconomyGroupActionFacts::BoardShip {
        ship: Some(identity(OWNER, 9, 70)),
        members: vec![exact_member(4)],
    };
    let plan = preflight_economy_group_transaction(&request, &facts, caps()).unwrap();
    let receipt = EconomyGroupTransactionReceipt {
        request: request.clone(),
        facts,
        capabilities: caps(),
        status: EconomyGroupTransactionStatus::Applied,
        commit: Some(commit(&plan)),
        plan: Some(plan),
    };
    assert!(receipt.validates(&request));

    let mut bad_reload = receipt.clone();
    bad_reload.commit.as_mut().unwrap().reloaded_state = digest(99);
    assert!(!bad_reload.validates(&request));
    let mut bad_tick = receipt.clone();
    bad_tick.commit.as_mut().unwrap().reloaded_next_tick = digest(88);
    assert!(!bad_tick.validates(&request));

    let mut bad_identity = receipt;
    if let EconomyGroupActionFacts::BoardShip { members, .. } = &mut bad_identity.facts {
        members[0].identity.generation ^= 1;
    }
    assert!(!bad_identity.validates(&request));
}

#[test]
fn ignore_orders_and_wrong_format_remain_explicit_prerequisites() {
    let groups = canonical_group(&[4]);
    let make = |ignore_orders| {
        EconomyGroupTransactionRequest::snapshot(
            &groups,
            CanonicalGroupLocator {
                owner: OWNER,
                slot: SLOT,
            },
            ignore_orders,
            EconomyGroupAction::BoardShip {
                ship_o: 9,
                queued: 2,
            },
            digest(1),
        )
        .unwrap()
    };
    let facts = EconomyGroupActionFacts::BoardShip {
        ship: Some(identity(OWNER, 9, 70)),
        members: vec![exact_member(4)],
    };
    assert_eq!(
        preflight_economy_group_transaction(&make(true), &facts, caps()),
        Err(EconomyGroupPreflightError::ScenarioIgnoreOrdersPrelude)
    );
    let mut old = caps();
    old.format_version = 11;
    assert_eq!(
        preflight_economy_group_transaction(&make(false), &facts, old),
        Err(EconomyGroupPreflightError::OrderEnvelopeV12)
    );
}
