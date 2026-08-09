// SPDX-License-Identifier: GPL-3.0-or-later
//! Live opcode-35 and atomic RECALL -> RETURN transaction pins.

use don_sim::command::recall_action_frontier::{
    plan_recall, RecallBoundary, RecallFacts, RecallLeaderFacts, RecallLeaderSource, RecallRequest,
};
use don_sim::command::return_action_frontier::{
    plan_return, ReturnFacts, ReturnMemberFacts, ReturnPlanePredicate, ReturnRequest,
    ReturnRouteFacts, HELICOPTER_TYPE_FLAG,
};
use don_sim::command::{
    build, ActionDef, Bridge, Fleet, GroupCommandPrefixDisposition, ObjectTable, Package, Port,
    RecallActionReceipt, RecallActionRequest, RecallActionTransactionStatus, Slot, GROUP_ACTIONS,
};
use don_sim::systems::order_dispatch::OrderQueue;

#[derive(Clone, Copy)]
enum ReplyMode {
    Applied,
    Unavailable,
    MissingReturn,
}

struct RecallHost {
    objects: ObjectTable,
    mode: ReplyMode,
    calls: usize,
    commits: usize,
    committed_effects: usize,
}

impl RecallHost {
    fn new(mode: ReplyMode) -> Self {
        let mut objects = ObjectTable::new(1);
        let mut helicopter = Slot::unit(7, 80, 96);
        helicopter.domain = 2;
        helicopter.unit_flags = HELICOPTER_TYPE_FLAG;
        objects.put(2, 0, helicopter);
        Self {
            objects,
            mode,
            calls: 0,
            commits: 0,
            committed_effects: 0,
        }
    }

    fn applied_receipt(request: RecallActionRequest) -> RecallActionReceipt {
        let recall_request = RecallRequest {
            group: request.group.clone(),
            ignore_orders: false,
            scenario_selection: Vec::new(),
        };
        let recall_facts = RecallFacts {
            group_after_ignore_orders: request.group.clone(),
            leader: Some(RecallLeaderFacts {
                source: RecallLeaderSource::FindLeaderZero,
                o: 0,
                domain: Some(2),
            }),
            group_members: Vec::new(),
            owner_units: Vec::new(),
        };
        let recall_plan = plan_recall(&recall_request, &recall_facts).unwrap();
        assert_eq!(recall_plan.boundary, RecallBoundary::OpenActionReturn);

        let return_request = ReturnRequest {
            group: recall_plan.group.clone(),
            ignore_orders: recall_request.ignore_orders,
            scenario_selection: recall_request.scenario_selection.clone(),
        };
        let mut group_after = return_request.group.clone();
        group_after.disband = 0;
        let return_facts = ReturnFacts {
            group_after_ignore_orders: group_after,
            members: vec![ReturnMemberFacts {
                o: 0,
                valid_unit: true,
                plane: Some(ReturnPlanePredicate::Concrete {
                    domain_218: 0,
                    unit_flags_2b4: HELICOPTER_TYPE_FLAG,
                    object_masks_1e4: None,
                }),
                route: Some(ReturnRouteFacts::Helicopter { on_map: true }),
            }],
        };
        let return_plan = plan_return(&return_request, &return_facts).unwrap();
        RecallActionReceipt {
            request,
            status: RecallActionTransactionStatus::Applied,
            recall_request: Some(recall_request),
            recall_facts: Some(recall_facts),
            recall_plan: Some(recall_plan),
            return_facts: Some(return_facts),
            return_plan: Some(return_plan),
        }
    }
}

impl Fleet for RecallHost {
    fn alive(&self, who: u8, o: i16) -> bool {
        Fleet::alive(&self.objects, who, o)
    }
    fn is_unit(&self, who: u8, o: i16) -> bool {
        Fleet::is_unit(&self.objects, who, o)
    }
    fn is_building(&self, who: u8, o: i16) -> bool {
        Fleet::is_building(&self.objects, who, o)
    }
    fn group_of(&self, who: u8, o: i16) -> i16 {
        Fleet::group_of(&self.objects, who, o)
    }
    fn set_group_of(&mut self, who: u8, o: i16, slot: i16) {
        Fleet::set_group_of(&mut self.objects, who, o, slot);
    }
    fn uid(&self, who: u8, o: i16) -> u16 {
        Fleet::uid(&self.objects, who, o)
    }
    fn pos(&self, who: u8, o: i16) -> (i32, i32) {
        Fleet::pos(&self.objects, who, o)
    }
    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue> {
        Fleet::orders(&self.objects, who, o)
    }
    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue> {
        Fleet::orders_mut(&mut self.objects, who, o)
    }
    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        Fleet::set_stance(&mut self.objects, who, o, stance);
    }
    fn disband(&mut self, who: u8, o: i16) {
        Fleet::disband(&mut self.objects, who, o);
    }

    fn apply_recall_action_transaction(
        &mut self,
        request: RecallActionRequest,
    ) -> RecallActionReceipt {
        self.calls += 1;
        match self.mode {
            ReplyMode::Unavailable => RecallActionReceipt::unavailable(request),
            ReplyMode::MissingReturn => {
                let mut receipt = Self::applied_receipt(request);
                receipt.return_facts = None;
                receipt.return_plan = None;
                receipt
            }
            ReplyMode::Applied => {
                let receipt = Self::applied_receipt(request);
                assert!(receipt.validates(&receipt.request));
                self.commits += 1;
                self.committed_effects = receipt.recall_plan.as_ref().unwrap().effects.len()
                    + receipt.return_plan.as_ref().unwrap().effects.len();
                receipt
            }
        }
    }
}

fn select_one(bridge: &mut Bridge, package: &mut Package, host: &mut RecallHost) {
    bridge
        .process_all(package, &build::group(2, &[0]), host)
        .unwrap();
    bridge.groups.get_mut(package.group).unwrap().disband = 9;
}

#[test]
fn opcode_35_executes_recall_and_return_under_one_host_commit() {
    let mut bridge = Bridge::new();
    let mut package = Package::new(2, 0);
    let mut host = RecallHost::new(ReplyMode::Applied);
    select_one(&mut bridge, &mut package, &mut host);

    bridge.process_one(&mut package, &[35], &mut host);

    assert_eq!(host.calls, 1);
    assert_eq!(host.commits, 1);
    assert_eq!(host.committed_effects, 8);
    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 0);
    assert_eq!(bridge.stats.acted, 1);
    assert_eq!(bridge.stats.unported, 0);
    assert_eq!(bridge.stats.open_group_action_tails, 0);
    assert_eq!(bridge.stats.orders_cleared, 1);
    assert_eq!(bridge.stats.orders_installed, 1);
    let recall = ActionDef::find("recall").unwrap();
    assert_eq!(recall.port, Port::StateWired);
    let recall_index = GROUP_ACTIONS
        .iter()
        .position(|action| action.name == "recall")
        .unwrap();
    assert_eq!(bridge.stats.by_action[recall_index], 1);
    assert!(matches!(
        bridge.take_group_command_prefix_receipts()[0].disposition,
        GroupCommandPrefixDisposition::OpenActionTail(_)
    ));
}

#[test]
fn unavailable_or_missing_return_tail_publishes_nothing() {
    for mode in [ReplyMode::Unavailable, ReplyMode::MissingReturn] {
        let mut bridge = Bridge::new();
        let mut package = Package::new(2, 0);
        let mut host = RecallHost::new(mode);
        select_one(&mut bridge, &mut package, &mut host);

        bridge.process_one(&mut package, &[35], &mut host);

        assert_eq!(host.calls, 1);
        assert_eq!(host.commits, 0);
        assert_eq!(bridge.groups.get(package.group).unwrap().disband, 9);
        assert_eq!(bridge.stats.acted, 0);
        assert_eq!(bridge.stats.unported, 1);
        assert_eq!(bridge.stats.open_group_action_tails, 1);
        assert_eq!(bridge.stats.orders_cleared, 0);
        assert_eq!(bridge.stats.orders_installed, 0);
    }
}

#[test]
fn combined_receipt_rejects_spliced_return_plan_and_nonzero_rng() {
    let mut group = don_sim::systems::groups_guys::GroupData::default();
    group.who = 2;
    group.num = 1;
    group.list[0] = 0;
    let request = RecallActionRequest { group };
    let mut receipt = RecallHost::applied_receipt(request.clone());
    assert!(receipt.validates(&request));

    receipt.return_plan.as_mut().unwrap().effects.swap(0, 1);
    assert!(!receipt.validates(&request));
    receipt = RecallHost::applied_receipt(request.clone());
    receipt.return_plan.as_mut().unwrap().direct_rng_draws = 1;
    assert!(!receipt.validates(&request));
}
