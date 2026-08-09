//! Product-boundary pins for complete HALT/DISBAND command receiver transactions.

use don_sim::command::{
    build, Bridge, Fleet, GroupDisbandTransactionReceipt, GroupDisbandTransactionRequest,
    GroupDisbandTransactionStatus, GroupHaltTransactionReceipt, GroupHaltTransactionRequest,
    GroupHaltTransactionStatus, ObjectTable, Package, Slot, GROUP_ACTIONS,
};
use don_sim::order::Order;
use don_sim::systems::order_dispatch::{OrderQueue, OrderRec};

fn select(bridge: &mut Bridge, package: &mut Package, fleet: &mut dyn Fleet, list: &[i16]) {
    bridge
        .process_all(package, &build::group(package.play as i8, list), fleet)
        .unwrap();
}

#[test]
fn halt_applies_exact_masks_order_close_form_reset_and_airborne_skip() {
    for name in ["halt", "disband"] {
        assert_eq!(
            GROUP_ACTIONS
                .iter()
                .find(|action| action.name == name)
                .unwrap()
                .port,
            don_sim::command::Port::Complete
        );
    }

    let mut bridge = Bridge::new();
    let mut package = Package::new(2, 0);
    let mut fleet = ObjectTable::new(3);
    let mut ordinary = Slot::unit(10, 100, 200);
    ordinary.unit_masks = 0x0400_0102;
    ordinary
        .orders
        .push_back(OrderRec::from(Order::move_to(1, 2, 0)));
    fleet.put(2, 0, ordinary);
    let mut airborne = Slot::plane(11, 300, 400);
    airborne.domain = 2;
    airborne.unit_flags = 0;
    airborne.unit_masks = 0x0400_0104;
    airborne
        .orders
        .push_back(OrderRec::from(Order::move_to(3, 4, 0)));
    fleet.put(2, 1, airborne);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    bridge.groups.get_mut(package.group).unwrap().form = 7;

    bridge
        .process_all(&mut package, &build::halt(), &mut fleet)
        .unwrap();
    assert_eq!(fleet.get(2, 0).unwrap().unit_masks, 2);
    assert!(fleet.get(2, 0).unwrap().orders.is_empty());
    assert_eq!(
        fleet.get(2, 1).unwrap().unit_masks,
        0x0400_0104,
        "an airborne plane without flag 0x20 is skipped"
    );
    assert_eq!(fleet.get(2, 1).unwrap().orders.len(), 1);
    assert_eq!(bridge.groups.get(package.group).unwrap().form, -1);
    assert_eq!(bridge.stats.orders_cleared, 1);

    bridge
        .process_all(&mut package, &build::disband(0), &mut fleet)
        .unwrap();
    assert!(!fleet.alive(2, 1), "reverse scan disbands the last member");
    assert_eq!(
        bridge.groups.get(package.group).unwrap().num,
        2,
        "dead member identity survives until normalize"
    );
}

enum ReceiptMode {
    Unavailable,
    Malformed,
}

struct ReceiptFleet {
    objects: ObjectTable,
    halt: ReceiptMode,
    disband: ReceiptMode,
}

impl Fleet for ReceiptFleet {
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

    fn apply_group_halt_transaction(
        &mut self,
        request: GroupHaltTransactionRequest,
    ) -> GroupHaltTransactionReceipt {
        match self.halt {
            ReceiptMode::Unavailable => GroupHaltTransactionReceipt::unavailable(request),
            ReceiptMode::Malformed => GroupHaltTransactionReceipt {
                request: request.clone(),
                status: GroupHaltTransactionStatus::Applied,
                group_after_ignore_orders: Some(request.group),
                members: Vec::new(),
                plan: None,
            },
        }
    }

    fn apply_group_disband_transaction(
        &mut self,
        request: GroupDisbandTransactionRequest,
    ) -> GroupDisbandTransactionReceipt {
        match self.disband {
            ReceiptMode::Unavailable => GroupDisbandTransactionReceipt::unavailable(request),
            ReceiptMode::Malformed => GroupDisbandTransactionReceipt {
                request: request.clone(),
                status: GroupDisbandTransactionStatus::Applied,
                group_after_ignore_orders: Some(request.group),
                validate_disband: Some(false),
                owner_is_local: Some(false),
                members: Vec::new(),
                plan: None,
            },
        }
    }
}

#[test]
fn unavailable_and_malformed_lifecycle_receipts_leave_bridge_and_world_untouched() {
    let mut bridge = Bridge::new();
    let mut package = Package::new(3, 0);
    let mut objects = ObjectTable::new(2);
    let mut unit = Slot::unit(20, 10, 20);
    unit.orders
        .push_back(OrderRec::from(Order::move_to(30, 40, 0)));
    objects.put(3, 0, unit);
    let mut fleet = ReceiptFleet {
        objects,
        halt: ReceiptMode::Unavailable,
        disband: ReceiptMode::Malformed,
    };
    select(&mut bridge, &mut package, &mut fleet, &[0]);
    bridge.groups.get_mut(package.group).unwrap().form = 5;

    bridge
        .process_all(&mut package, &build::halt(), &mut fleet)
        .unwrap();
    assert_eq!(bridge.groups.get(package.group).unwrap().form, 5);
    assert_eq!(fleet.orders(3, 0).unwrap().len(), 1);

    bridge
        .process_all(&mut package, &build::disband(1), &mut fleet)
        .unwrap();
    assert!(fleet.alive(3, 0));
    assert_eq!(bridge.groups.get(package.group).unwrap().num, 1);
}
