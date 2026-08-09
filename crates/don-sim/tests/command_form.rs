use don_sim::command::{build, ActionDef, Bridge, ObjectTable, Package, Port, QueuePos, Slot};
use don_sim::order::{Order, OrderIndex};
use don_sim::systems::order_dispatch::OrderRec;

#[test]
fn form_command_resolves_writes_and_reforms_through_the_wire_bridge() {
    let mut fleet = ObjectTable::new(8);
    for (o, form, category) in [(0, 0, 0), (1, 1, 1), (2, 1, 1)] {
        let mut slot = Slot::unit(100 + o as u16, 1_000 + o * 96, 2_000);
        slot.form = form;
        slot.form_category = category;
        // These are deliberately non-group orders: action_form's QUEUE_NEW insert dance
        // does not save them and action_halt must retire them before the re-form move.
        slot.orders.push_back(OrderRec::from(Order::attack(7, 9)));
        fleet.put(1, o as i16, slot);
    }

    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 0);
    bridge
        .process_all(&mut package, &build::group(1, &[0, 1, 2]), &mut fleet)
        .unwrap();

    // The mode is form 1, so CYCLE_NEXT resolves to 2. Both QUEUE_FIRST and QUEUE_NEW
    // take action_form's special stash/halt/recursive path; with no GROUP-flagged leader
    // order, retail adds one MOVE_TO at the leader's final location.
    bridge
        .process_all(&mut package, &build::form(-1, 0, QueuePos::New), &mut fleet)
        .unwrap();

    for o in 0..3 {
        let unit = fleet.get(1, o).unwrap();
        assert_eq!(unit.form, 2);
        let orders: Vec<_> = unit.orders.iter().collect();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].kind, OrderIndex::MoveTo);
        assert_eq!((orders[0].x, orders[0].y), (1_000, 2_000));
    }
    assert_eq!(bridge.groups.get(package.group).unwrap().form, -1);
    assert_eq!(bridge.stats.orders_cleared, 3);

    // FROM_LEADER writes the leader's signed form byte to every unit, records the retail
    // -2 sentinel on GroupData, and QUEUE_LAST appends the delegated move.
    fleet.get_mut(1, 0).unwrap().form = 5;
    bridge
        .process_all(
            &mut package,
            &build::form(-2, 0x20, QueuePos::Last),
            &mut fleet,
        )
        .unwrap();
    for o in 0..3 {
        let unit = fleet.get(1, o).unwrap();
        assert_eq!(unit.form, 5);
        assert_eq!(unit.orders.iter().count(), 2);
        assert!(unit
            .orders
            .iter()
            .all(|order| order.kind == OrderIndex::MoveTo));
        let appended = unit.orders.iter().last().unwrap();
        assert_eq!(appended.facing, 1);
        assert_eq!(appended.angle, 0x20);
    }
    assert_eq!(bridge.groups.get(package.group).unwrap().form, -2);
    assert_eq!(ActionDef::find("form").unwrap().port, Port::Orders);
    assert_eq!(bridge.stats.unported, 0);
    assert_eq!(bridge.stats.acted, 2);
}
