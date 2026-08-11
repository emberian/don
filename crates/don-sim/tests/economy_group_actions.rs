// SPDX-License-Identifier: GPL-3.0-or-later
//! Dispatcher-level pins for the three recovered economy order installers.
//!
//! Every test here drives `Bridge::process_one` with the real wire bytes and reads the
//! resulting `OrderQueue`s and `GroupData` out of the reference `ObjectTable` host — no
//! test calls a planner directly. The planner's own branch pins live in
//! `crates/don-sim/src/systems/economy_group_actions.rs`.
//!
//! Derivation: `docs/assembly/economy-group-actions.md`. Tier C.

use don_sim::command::economy_group_actions::{
    REPAIR_CAST_SPELL_TYPE, TRADE_GROUP_COUNT_INDEX, TRADE_MARKET_TYPE, TRADE_SEA_DESTINATION_TYPE,
    TRADE_SEA_MEMBER_TYPE,
};
use don_sim::command::{Bridge, Fleet, ObjectTable, Package, Slot};
use don_sim::order::OrderIndex;

const WHO: u8 = 1;

fn wire(op: u8, tail: &[u8]) -> Vec<u8> {
    let mut v = vec![op];
    v.extend_from_slice(tail);
    v
}

fn i32b(v: i32) -> [u8; 4] {
    v.to_le_bytes()
}

/// `GroupCommand` (opcode 0): `num` at +1, `who` at +2, then `num` `i16` object indices.
fn select(members: &[i16]) -> Vec<u8> {
    let mut v = vec![0u8, members.len() as u8, WHO];
    for &o in members {
        v.extend_from_slice(&o.to_le_bytes());
    }
    v
}

fn kinds(host: &ObjectTable, o: i16) -> Vec<OrderIndex> {
    host.orders(WHO, o)
        .map(|q| q.iter().map(|r| r.kind).collect())
        .unwrap_or_default()
}

fn run(host: &mut ObjectTable, cmds: &[Vec<u8>]) -> Bridge {
    let mut bridge = Bridge::new();
    let mut pkg = Package::new(i32::from(WHO), 0);
    for c in cmds {
        bridge.process_one(&mut pkg, c, host);
    }
    bridge
}

/// A worker that satisfies every `action_repair` member gate.
fn repairer(o: i16, host: &mut ObjectTable, castable: bool) {
    let slot = host.get_mut(WHO, o).unwrap();
    *slot = Slot::unit(o as u16 + 1, 100 * i32::from(o), 200);
    slot.economy_type_class = Some(0x32);
    slot.region = Some(3);
    slot.castable_spells = Some(if castable {
        vec![REPAIR_CAST_SPELL_TYPE]
    } else {
        Vec::new()
    });
}

#[test]
fn repair_installs_the_cast_companion_and_retires_the_targets_orders() {
    let mut host = ObjectTable::new(8);
    repairer(0, &mut host, true);
    // The repair target: a damaged building of owner 2 that already holds an order.
    {
        let t = host.get_mut(2, 5).unwrap();
        *t = Slot::building(9, 100, 200);
        t.region = Some(3);
        t.unit_masks = 0x0400_0000 | 0x1;
    }
    host.orders_mut(2, 5).unwrap().push_back(
        don_sim::systems::order_dispatch::OrderRec::of_kind(OrderIndex::Think),
    );

    let cmd = wire(
        16,
        &[i32b(5), i32b(2), i32b(2)].concat(), // ox=5 whom=2 queued=QUEUE_NEW
    );
    let mut bridge = run(&mut host, &[select(&[0]), cmd]);

    assert_eq!(kinds(&host, 0), vec![OrderIndex::Repair, OrderIndex::CastSpell]);
    // `action_repair` retires the repair target: mask bit 0x04000000 cleared, list emptied.
    assert_eq!(host.get(2, 5).unwrap().unit_masks, 0x1);
    assert!(host.orders(2, 5).unwrap().is_empty());

    let receipts = bridge.take_economy_action_receipts();
    assert_eq!(receipts.len(), 1);
    let plan = receipts[0].plan.as_ref().unwrap();
    assert!(
        plan.is_exact(),
        "host answered every gate, so nothing may be unresolved: {:?}",
        plan.unresolved
    );
}

#[test]
fn repair_skips_a_member_whose_region_does_not_touch_the_target() {
    let mut host = ObjectTable::new(8);
    repairer(0, &mut host, true);
    host.get_mut(WHO, 0).unwrap().region = Some(0x41); // water side, no recorded adjacency
    {
        let t = host.get_mut(2, 5).unwrap();
        *t = Slot::building(9, 100, 200);
        t.region = Some(3);
    }
    let cmd = wire(16, &[i32b(5), i32b(2), i32b(2)].concat());
    run(&mut host, &[select(&[0]), cmd]);
    assert!(kinds(&host, 0).is_empty());

    // The same member with the adjacency bit recorded does get the order — so the empty
    // queue above is the gate firing, not the command failing to arrive.
    let mut host = ObjectTable::new(8);
    repairer(0, &mut host, true);
    host.get_mut(WHO, 0).unwrap().region = Some(0x41);
    {
        let t = host.get_mut(2, 5).unwrap();
        *t = Slot::building(9, 100, 200);
        t.region = Some(3);
    }
    host.set_regions_touch(3, 0x41);
    let cmd = wire(16, &[i32b(5), i32b(2), i32b(2)].concat());
    run(&mut host, &[select(&[0]), cmd]);
    assert_eq!(kinds(&host, 0), vec![OrderIndex::Repair, OrderIndex::CastSpell]);
}

#[test]
fn repair_resets_the_group_formation_index() {
    let mut host = ObjectTable::new(8);
    repairer(0, &mut host, false);
    {
        let t = host.get_mut(2, 5).unwrap();
        *t = Slot::building(9, 100, 200);
        t.region = Some(3);
    }
    let cmd = wire(16, &[i32b(5), i32b(2), i32b(2)].concat());
    let bridge = run(&mut host, &[select(&[0]), cmd]);
    let slot = host.group_of(WHO, 0);
    assert!(slot >= 0);
    assert_eq!(bridge.groups.get(i32::from(slot)).unwrap().form, -1);
}

#[test]
fn a_host_that_answers_nothing_still_installs_exactly_the_legacy_order_set() {
    let mut host = ObjectTable::new(8);
    *host.get_mut(WHO, 0).unwrap() = Slot::unit(1, 0, 0);
    *host.get_mut(2, 5).unwrap() = Slot::building(9, 100, 200);
    let cmd = wire(16, &[i32b(5), i32b(2), i32b(2)].concat());
    let mut bridge = run(&mut host, &[select(&[0]), cmd]);
    assert_eq!(kinds(&host, 0), vec![OrderIndex::Repair]);

    let receipts = bridge.take_economy_action_receipts();
    let plan = receipts[0].plan.as_ref().unwrap();
    assert!(!plan.is_exact());
    assert!(plan.unresolved.contains(&"ObjectTypeData+0x04"));
    assert!(plan.unresolved.contains(&"Region::is_coast 0x00680F90"));
    assert!(plan
        .unresolved
        .contains(&"SpellTypeData::is_castable 0x00675BC0"));
}

#[test]
fn board_ship_gives_the_ship_await_board_and_clears_it_once() {
    let mut host = ObjectTable::new(8);
    for o in [0i16, 1] {
        let s = host.get_mut(WHO, o).unwrap();
        *s = Slot::unit(o as u16 + 1, 10, 10);
        s.domain = 0;
        s.busy = Some(false);
        s.region = Some(4);
    }
    {
        let ship = host.get_mut(WHO, 6).unwrap();
        *ship = Slot::unit(60, 20, 20);
        ship.region = Some(4);
        ship.carries = Some(vec![(WHO, 0), (WHO, 1)]);
    }
    host.orders_mut(WHO, 6).unwrap().push_back(
        don_sim::systems::order_dispatch::OrderRec::of_kind(OrderIndex::MoveTo),
    );

    let cmd = wire(15, &[i32b(6), i32b(2)].concat()); // ox=6 queued=QUEUE_NEW
    let mut bridge = run(&mut host, &[select(&[0, 1]), cmd]);

    assert_eq!(kinds(&host, 0), vec![OrderIndex::BoardShip]);
    assert_eq!(kinds(&host, 1), vec![OrderIndex::BoardShip]);
    // The ship's pre-existing MOVE_TO is gone, and it holds one AWAIT_BOARD per passenger.
    assert_eq!(
        kinds(&host, 6),
        vec![OrderIndex::AwaitBoard, OrderIndex::AwaitBoard]
    );

    let receipts = bridge.take_economy_action_receipts();
    let plan = receipts[0].plan.as_ref().unwrap();
    assert!(plan.is_exact(), "{:?}", plan.unresolved);
    assert_eq!(plan.blocked_members, 0);
}

#[test]
fn board_ship_counts_a_refused_passenger_and_installs_nothing_for_it() {
    let mut host = ObjectTable::new(8);
    for o in [0i16, 1] {
        let s = host.get_mut(WHO, o).unwrap();
        *s = Slot::unit(o as u16 + 1, 10, 10);
        s.domain = 0;
        s.busy = Some(false);
        s.region = Some(4);
    }
    {
        let ship = host.get_mut(WHO, 6).unwrap();
        *ship = Slot::unit(60, 20, 20);
        ship.region = Some(4);
        ship.carries = Some(vec![(WHO, 0)]); // object 1 does not fit
    }
    let cmd = wire(15, &[i32b(6), i32b(2)].concat());
    let mut bridge = run(&mut host, &[select(&[0, 1]), cmd]);

    assert_eq!(kinds(&host, 0), vec![OrderIndex::BoardShip]);
    assert!(kinds(&host, 1).is_empty());
    assert_eq!(kinds(&host, 6), vec![OrderIndex::AwaitBoard]);
    let receipts = bridge.take_economy_action_receipts();
    assert_eq!(receipts[0].plan.as_ref().unwrap().blocked_members, 1);
}

fn trade_market(host: &mut ObjectTable, whom: u8, ox: i16) {
    let t = host.get_mut(whom, ox).unwrap();
    *t = Slot::building(70, 500, 500);
    t.build_active_known = Some(true);
    t.type_is = Some(vec![TRADE_MARKET_TYPE]);
    t.region = Some(2);
}

#[test]
fn trade_installs_on_caravans_only_and_records_both_endpoints() {
    let mut host = ObjectTable::new(8);
    trade_market(&mut host, 2, 5);
    for (o, caravan) in [(0i16, true), (1i16, false)] {
        let s = host.get_mut(WHO, o).unwrap();
        *s = Slot::unit(o as u16 + 1, 0, 0);
        s.is_caravan = Some(caravan);
        s.region = Some(2);
    }
    host.set_group_count(TRADE_GROUP_COUNT_INDEX, 0x3b, 1);
    host.set_group_count(TRADE_GROUP_COUNT_INDEX, TRADE_SEA_MEMBER_TYPE, 0);

    // TradeCommand: ox@1 whom@5 oxx@9 whose@13 queued@17.
    let cmd = wire(
        17,
        &[i32b(5), i32b(2), i32b(4), i32b(3), i32b(2)].concat(),
    );
    let mut bridge = run(&mut host, &[select(&[0, 1]), cmd]);

    assert_eq!(kinds(&host, 0), vec![OrderIndex::TradeRoute]);
    assert!(kinds(&host, 1).is_empty());

    let receipts = bridge.take_economy_action_receipts();
    let plan = receipts[0].plan.as_ref().unwrap();
    assert!(plan.is_exact(), "{:?}", plan.unresolved);
    // The second endpoint survives in the plan even though `Order` cannot carry it.
    let carried: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|s| match *s {
            don_sim::command::economy_group_actions::EconomyStep::AddTradeOrder {
                ox,
                whom,
                oxx,
                whose,
                ..
            } => Some((ox, whom, oxx, whose)),
            _ => None,
        })
        .collect();
    assert_eq!(carried, vec![(5, 2, 4, 3)]);
}

#[test]
fn trade_refuses_an_inactive_destination_without_installing_or_resetting_form() {
    let mut host = ObjectTable::new(8);
    trade_market(&mut host, 2, 5);
    host.get_mut(2, 5).unwrap().build_active_known = Some(false);
    {
        let s = host.get_mut(WHO, 0).unwrap();
        *s = Slot::unit(1, 0, 0);
        s.is_caravan = Some(true);
        s.region = Some(2);
    }
    host.set_group_count(TRADE_GROUP_COUNT_INDEX, 0x3b, 1);
    host.set_group_count(TRADE_GROUP_COUNT_INDEX, TRADE_SEA_MEMBER_TYPE, 0);

    let cmd = wire(
        17,
        &[i32b(5), i32b(2), i32b(4), i32b(3), i32b(2)].concat(),
    );
    // `CommandPackage::process_group` `0x0094A0C0` runs `Group::clear(-1)` `0x00713E80` at
    // `0x0094A0FF`, and `clear` writes `form = -1` (`+0x10`). A fresh selection therefore
    // already carries `-1`, so plant a distinct value between the two commands and let the
    // absence of the store be what the assertion sees.
    let mut bridge = run(&mut host, &[select(&[0])]);
    let mut pkg = Package::new(i32::from(WHO), 0);
    pkg.group = i32::from(host.group_of(WHO, 0));
    bridge.groups.get_mut(pkg.group).unwrap().form = 7;
    bridge.process_one(&mut pkg, &cmd, &mut host);
    assert!(kinds(&host, 0).is_empty());
    let slot = host.group_of(WHO, 0);
    assert_ne!(
        bridge.groups.get(i32::from(slot)).unwrap().form,
        -1,
        "a refused TRADE returns before `group.form = -1`"
    );
    assert!(bridge.take_economy_action_receipts()[0].plan.is_none());
}

#[test]
fn trade_takes_the_sea_branch_when_the_destination_is_a_sea_trade_dock() {
    let mut host = ObjectTable::new(8);
    {
        let t = host.get_mut(2, 5).unwrap();
        *t = Slot::building(70, 500, 500);
        t.build_active_known = Some(true);
        // The entry gate reads `+0xB0`'s `+0x24`; this bridge answers both `+0x24` reads
        // from the same host column, so a dock is modelled as answering both.
        t.type_is = Some(vec![TRADE_MARKET_TYPE, TRADE_SEA_DESTINATION_TYPE]);
        t.region = Some(0x41);
    }
    for (o, sea) in [(0i16, true), (1i16, false)] {
        let s = host.get_mut(WHO, o).unwrap();
        *s = Slot::unit(o as u16 + 1, 0, 0);
        s.is_caravan = Some(false);
        s.type_is = Some(if sea {
            vec![TRADE_SEA_MEMBER_TYPE]
        } else {
            Vec::new()
        });
        s.region = Some(2);
    }
    host.set_regions_touch(2, 0x41);
    host.set_group_count(TRADE_GROUP_COUNT_INDEX, 0x3b, 0);
    host.set_group_count(TRADE_GROUP_COUNT_INDEX, TRADE_SEA_MEMBER_TYPE, 1);

    let cmd = wire(
        17,
        &[i32b(5), i32b(2), i32b(4), i32b(3), i32b(2)].concat(),
    );
    run(&mut host, &[select(&[0, 1]), cmd]);
    // `is_trade` answered true, so retail takes the caravan branch and neither member,
    // both of which answered `is_caravan = false`, is accepted.
    assert!(kinds(&host, 0).is_empty());
    assert!(kinds(&host, 1).is_empty());

    // Now a destination that answers only `is_sea_trade` on the member-loop read.
    let mut host = ObjectTable::new(8);
    {
        let t = host.get_mut(2, 5).unwrap();
        *t = Slot::building(70, 500, 500);
        t.build_active_known = Some(true);
        t.type_is = Some(vec![TRADE_SEA_DESTINATION_TYPE]);
        t.region = Some(0x41);
    }
    for (o, sea) in [(0i16, true), (1i16, false)] {
        let s = host.get_mut(WHO, o).unwrap();
        *s = Slot::unit(o as u16 + 1, 0, 0);
        s.is_caravan = Some(false);
        s.type_is = Some(if sea {
            vec![TRADE_SEA_MEMBER_TYPE]
        } else {
            Vec::new()
        });
        s.region = Some(2);
    }
    host.set_regions_touch(2, 0x41);
    host.set_group_count(TRADE_GROUP_COUNT_INDEX, 0x3b, 0);
    host.set_group_count(TRADE_GROUP_COUNT_INDEX, TRADE_SEA_MEMBER_TYPE, 1);
    let cmd = wire(
        17,
        &[i32b(5), i32b(2), i32b(4), i32b(3), i32b(2)].concat(),
    );
    run(&mut host, &[select(&[0, 1]), cmd]);
    // The entry gate reads the same column, so it refuses before the member loop: the
    // recovered split between the two `+0x24` receivers is not observable through a host
    // that cannot distinguish them, and this pin records that boundary rather than hiding
    // it behind a plan-level call.
    assert!(kinds(&host, 0).is_empty());
    assert!(kinds(&host, 1).is_empty());
}
