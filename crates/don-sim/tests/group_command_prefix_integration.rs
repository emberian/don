// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::command::unimplemented_group_command_plans::{
    plan_unimplemented_group_command, GroupActionCall, PlanStatus, UnimplementedGroupCommandFacts,
    UnimplementedGroupCommandReceipt, UnimplementedGroupCommandRequest,
};
use don_sim::command::{
    ActionDef, Bridge, Fleet, GroupCommandPrefixDisposition, ObjectTable, Package, Port, Slot,
    GROUP_ACTIONS,
};
use don_sim::systems::order_dispatch::OrderQueue;

struct NoPrefixReadHost {
    inner: ObjectTable,
    reply: Option<UnimplementedGroupCommandReceipt>,
}

impl NoPrefixReadHost {
    fn new(per_owner: usize) -> Self {
        Self {
            inner: ObjectTable::new(per_owner),
            reply: None,
        }
    }

    fn with_reply(reply: UnimplementedGroupCommandReceipt) -> Self {
        Self {
            inner: ObjectTable::new(0),
            reply: Some(reply),
        }
    }
}

impl Fleet for NoPrefixReadHost {
    fn alive(&self, who: u8, o: i16) -> bool {
        self.inner.get(who, o).is_some_and(|slot| slot.alive)
    }

    fn is_unit(&self, who: u8, o: i16) -> bool {
        self.inner.get(who, o).is_some_and(|slot| slot.is_unit)
    }

    fn is_building(&self, who: u8, o: i16) -> bool {
        self.inner.get(who, o).is_some_and(|slot| slot.is_building)
    }

    fn group_of(&self, who: u8, o: i16) -> i16 {
        self.inner.get(who, o).map_or(-1, |slot| slot.group)
    }

    fn set_group_of(&mut self, who: u8, o: i16, group: i16) {
        if let Some(slot) = self.inner.get_mut(who, o) {
            slot.group = group;
        }
    }

    fn uid(&self, who: u8, o: i16) -> u16 {
        self.inner.get(who, o).map_or(u16::MAX, |slot| slot.uid)
    }

    fn pos(&self, who: u8, o: i16) -> (i32, i32) {
        self.inner
            .get(who, o)
            .map_or((0, 0), |slot| (slot.x, slot.y))
    }

    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue> {
        self.inner.get(who, o).map(|slot| &slot.orders)
    }

    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue> {
        self.inner.get_mut(who, o).map(|slot| &mut slot.orders)
    }

    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        if let Some(slot) = self.inner.get_mut(who, o) {
            slot.stance = stance;
        }
    }

    fn disband(&mut self, who: u8, o: i16) {
        if let Some(slot) = self.inner.get_mut(who, o) {
            slot.alive = false;
        }
    }

    fn group_command_prefix_receipt(
        &self,
        _request: UnimplementedGroupCommandRequest,
        _group_index: i32,
    ) -> UnimplementedGroupCommandReceipt {
        self.reply.unwrap_or_else(|| {
            panic!("an exact no-object-read branch crossed the host fact boundary")
        })
    }
}

fn wire(opcode: u8, fields: &[i32]) -> Vec<u8> {
    let mut bytes = vec![opcode];
    for field in fields {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes
}

#[test]
fn all_seven_live_dispatch_rows_emit_the_exact_open_action_call() {
    let cases = [
        (
            wire(5, &[0, 1, 2]),
            GroupActionCall::SiegeAttack {
                ox: 0,
                whom: 1,
                queued: 2,
            },
        ),
        (
            wire(6, &[0, 1, 2, 3]),
            GroupActionCall::SwarmAround {
                ox: 0,
                whom: 1,
                queued: 2,
                orders: 3,
                retail_mode: 1,
            },
        ),
        (
            wire(23, &[0, 1, 629, 40, 50]),
            GroupActionCall::Spell {
                type_index: 629,
                ox: 0,
                whom: 1,
                x: 40,
                y: 50,
            },
        ),
        (
            wire(24, &[17, 4]),
            GroupActionCall::QueueUp {
                type_index: 17,
                num: 4,
            },
        ),
        (
            wire(25, &[10, 20, 30, 40, 17, 2]),
            GroupActionCall::Build {
                x: 10,
                y: 20,
                x2: 30,
                y2: 40,
                type_index: 17,
                queued: 2,
            },
        ),
        (
            wire(28, &[0, 1, 2, 3, 4, 5]),
            GroupActionCall::Flight {
                ox: 0,
                whom: 1,
                orders: 5,
                shift: 2,
                ctrl: 3,
                alt: 4,
            },
        ),
        (wire(35, &[]), GroupActionCall::Recall),
    ];

    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 0);
    package.group = 7;
    let mut host = ObjectTable::new(1);
    *host.get_mut(1, 0).unwrap() = Slot::unit(1, 0, 0);

    for (bytes, _) in &cases {
        bridge.process_one(&mut package, bytes, &mut host);
    }

    let receipts = bridge.take_group_command_prefix_receipts();
    assert_eq!(receipts.len(), cases.len());
    for (record, (_, expected_call)) in receipts.iter().zip(cases) {
        assert_eq!(
            record.disposition,
            GroupCommandPrefixDisposition::OpenActionTail(
                don_sim::command::unimplemented_group_command_plans::DelegatedGroupAction {
                    group_index: 7,
                    call: expected_call,
                },
            )
        );
    }
    assert_eq!(bridge.stats.acted, 0);
    assert_eq!(bridge.stats.open_group_action_tails, 7);
    assert_eq!(bridge.stats.group_prefix_noops, 0);
    assert_eq!(bridge.stats.unported, 7);
    for name in [
        "siege_attack",
        "swarm_around",
        "spell",
        "queue_up",
        "build",
        "flight",
        "recall",
    ] {
        let index = GROUP_ACTIONS
            .iter()
            .position(|action| action.name == name)
            .unwrap();
        assert_eq!(bridge.stats.by_action[index], 1, "{name}");
    }
}

#[test]
fn exact_gate_noops_are_distinct_from_missing_host_facts() {
    let mut bridge = Bridge::new();
    let mut no_read_host = NoPrefixReadHost::new(0);

    let mut no_group = Package::new(1, 0);
    bridge.process_one(&mut no_group, &wire(24, &[17, 4]), &mut no_read_host);

    let mut inactive_target = Package::new(1, 0);
    inactive_target.group = 3;
    let mut host = ObjectTable::new(1);
    bridge.process_one(&mut inactive_target, &wire(5, &[0, 1, 2]), &mut host);

    let receipts = bridge.take_group_command_prefix_receipts();
    assert_eq!(receipts.len(), 2);
    assert!(receipts
        .iter()
        .all(|record| { record.disposition == GroupCommandPrefixDisposition::ExactNoAction }));
    assert_eq!(bridge.stats.no_group, 1);
    assert_eq!(bridge.stats.group_prefix_noops, 2);
    assert_eq!(bridge.stats.open_group_action_tails, 0);
    assert_eq!(bridge.stats.unported, 0);

    let mut missing = Bridge::new();
    let mut package = Package::new(1, 0);
    package.group = 3;
    let mut empty_host = ObjectTable::new(0);
    missing.process_one(
        &mut package,
        &wire(23, &[0, 1, 629, 40, 50]),
        &mut empty_host,
    );
    assert_eq!(
        missing.take_group_command_prefix_receipts()[0].disposition,
        GroupCommandPrefixDisposition::Unavailable
    );
    assert_eq!(missing.stats.unported, 1);
}

#[test]
fn closure_table_marks_prefixes_state_wired_and_no_action_complete() {
    for (opcode, name) in [
        (5, "siege_attack"),
        (6, "swarm_around"),
        (23, "spell"),
        (24, "queue_up"),
        (25, "build"),
        (28, "flight"),
        (35, "recall"),
    ] {
        let action = ActionDef::find(name).unwrap();
        assert_eq!(action.port, Port::StateWired, "opcode {opcode}");
        assert_ne!(action.port, Port::Complete, "opcode {opcode}");
    }
}

#[test]
fn forged_or_malformed_host_receipts_fail_closed() {
    let request = UnimplementedGroupCommandRequest::SiegeAttack {
        ox: 0,
        whom: 1,
        queued: 2,
    };
    let wrong_group_facts = UnimplementedGroupCommandFacts {
        group_index: 99,
        addressed_object_flag_1: Some(true),
    };
    let wrong_group_receipt = UnimplementedGroupCommandReceipt {
        request,
        facts: Some(wrong_group_facts),
        status: PlanStatus::Planned,
        plan: plan_unimplemented_group_command(request, &wrong_group_facts),
    };
    let malformed_receipt = UnimplementedGroupCommandReceipt {
        request,
        facts: Some(UnimplementedGroupCommandFacts {
            group_index: 3,
            addressed_object_flag_1: Some(true),
        }),
        status: PlanStatus::Planned,
        plan: None,
    };

    for receipt in [wrong_group_receipt, malformed_receipt] {
        let mut bridge = Bridge::new();
        let mut package = Package::new(1, 0);
        package.group = 3;
        let mut host = NoPrefixReadHost::with_reply(receipt);
        bridge.process_one(&mut package, &wire(5, &[0, 1, 2]), &mut host);

        assert_eq!(
            bridge.take_group_command_prefix_receipts()[0].disposition,
            GroupCommandPrefixDisposition::Unavailable
        );
        assert_eq!(bridge.stats.open_group_action_tails, 0);
        assert_eq!(bridge.stats.unported, 1);
    }
}

#[test]
fn negative_targets_bypass_the_addressed_object_flag_read() {
    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 0);
    package.group = 2;
    let mut empty_host = NoPrefixReadHost::new(0);
    bridge.process_one(&mut package, &wire(5, &[-1, -1, 2]), &mut empty_host);

    let record = bridge.take_group_command_prefix_receipts()[0];
    assert!(matches!(
        record.receipt.request,
        UnimplementedGroupCommandRequest::SiegeAttack {
            ox: -1,
            whom: -1,
            queued: 2,
        }
    ));
    assert!(matches!(
        record.disposition,
        GroupCommandPrefixDisposition::OpenActionTail(_)
    ));
}
