//! The movement/attack entry prefix and the movement-order Coord centring, driven through
//! the real wire path: `Bridge::process_all` over `build::*` packets and an `ObjectTable`.
//!
//! No test here calls `group_action_entry` directly. Each one builds a selection with
//! `GroupCommand` (0) and then sends the real opcode, so a regression in dispatch, decode,
//! gate order, or the order constructors fails it.

use don_sim::command::group_action_entry::{
    dispatch_program, formation_order_destination, EntryGate,
};
use don_sim::command::{build, Bridge, ObjectTable, Package, QueuePos, Slot};
use don_sim::order::OrderIndex;
use don_sim::systems::groups_guys::{formation_order_coord, Formation, FormationMember};

const WHO: i8 = 1;

fn captain_profile() -> FormationMember {
    FormationMember {
        category: 0,
        x_spacing: 96,
        y_spacing: 144,
        formation_size: 1,
        guy_spacing: 48,
        modern_infantry: false,
        width: 50,
        angle: 0,
    }
}

/// Three ordinary on-map captains at the same spot, with complete formation facts.
fn formation_fleet(n: i16) -> ObjectTable {
    let mut fleet = ObjectTable::new(n as usize + 1);
    for o in 0..n {
        let mut slot = Slot::unit(400 + o as u16, 4_800, 9_600);
        slot.formation_member = Some(captain_profile());
        fleet.put(WHO as u8, o, slot);
    }
    fleet
}

/// A plain selection with no formation facts, which keeps `move_near` on its flat spine.
fn plain_fleet(n: i16) -> ObjectTable {
    let mut fleet = ObjectTable::new(n as usize + 1);
    for o in 0..n {
        fleet.put(WHO as u8, o, Slot::unit(500 + o as u16, 4_800, 9_600));
    }
    fleet
}

fn select(bridge: &mut Bridge, package: &mut Package, fleet: &mut ObjectTable, list: &[i16]) {
    bridge
        .process_all(package, &build::group(WHO, list), fleet)
        .unwrap();
}

fn installed(fleet: &ObjectTable, o: i16) -> usize {
    fleet.get(WHO as u8, o).unwrap().orders.iter().count()
}

// ---------------------------------------------------------------------------
// The centring of an installed formation destination
// ---------------------------------------------------------------------------

#[test]
fn a_move_to_stores_the_centre_of_its_ucoord_cell_not_the_cell_index() {
    let mut fleet = formation_fleet(3);
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1, 2]);

    bridge
        .process_all(
            &mut package,
            &build::move_to(4_800, 9_600, QueuePos::Last, 1),
            &mut fleet,
        )
        .unwrap();

    let order = fleet
        .get(WHO as u8, 0)
        .unwrap()
        .orders
        .current()
        .cloned()
        .expect("the captain holds the installed move");
    // `Unit::add_group_move_order` `0x005E4710` stores `cell * 0x30 + 0x18` into both the
    // live and the destination coordinate pair.
    assert_eq!((order.x, order.y), (4_824, 9_624));
    assert_eq!((order.dest_x, order.dest_y), (4_824, 9_624));
    // The bare cell index — what the bridge used to store — is 1/48 of that.
    assert_ne!(
        (order.x, order.y),
        (formation_order_coord(4_800), formation_order_coord(9_600))
    );
    // `orig_x`/`orig_y` stay the raw commanded Coord: they are separate arguments
    // (`param_15`/`param_16`) that the constructor copies verbatim.
    assert_eq!((order.orig_x, order.orig_y), (4_800, 9_600));
    assert_eq!(formation_order_destination(4_800), order.x);
}

#[test]
fn every_formation_member_destination_is_centred_not_only_the_leader() {
    let mut fleet = formation_fleet(3);
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1, 2]);
    bridge
        .process_all(
            &mut package,
            &build::move_to(4_800, 9_600, QueuePos::Last, 1),
            &mut fleet,
        )
        .unwrap();

    for o in 0..3 {
        let order = fleet
            .get(WHO as u8, o)
            .unwrap()
            .orders
            .current()
            .cloned()
            .unwrap();
        assert_eq!(
            order.x.rem_euclid(48),
            24,
            "member {o} x must sit on a cell centre"
        );
        assert_eq!(
            order.y.rem_euclid(48),
            24,
            "member {o} y must sit on a cell centre"
        );
        assert_eq!((order.x, order.y), (order.dest_x, order.dest_y));
    }
}

// ---------------------------------------------------------------------------
// The measured entry gates
// ---------------------------------------------------------------------------

#[test]
fn attack_refuses_a_wholly_off_map_selection() {
    let mut fleet = plain_fleet(2);
    for o in 0..2 {
        fleet.get_mut(WHO as u8, o).unwrap().is_on_map = false;
    }
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);

    bridge
        .process_all(
            &mut package,
            &build::attack(0, 2, QueuePos::New),
            &mut fleet,
        )
        .unwrap();

    // `GroupData::is_on_map` `0x0070C450` returns zero, and `action_attack` returns before
    // any `Unit::add_attack_order`.
    assert_eq!(installed(&fleet, 0), 0);
    assert_eq!(installed(&fleet, 1), 0);
}

#[test]
fn attack_installs_when_one_member_is_on_the_map() {
    let mut fleet = plain_fleet(2);
    fleet.get_mut(WHO as u8, 1).unwrap().is_on_map = false;
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);

    bridge
        .process_all(
            &mut package,
            &build::attack(0, 2, QueuePos::New),
            &mut fleet,
        )
        .unwrap();

    assert_eq!(installed(&fleet, 0), 1);
    assert_eq!(
        fleet
            .get(WHO as u8, 0)
            .unwrap()
            .orders
            .current()
            .unwrap()
            .kind,
        OrderIndex::Attack
    );
}

#[test]
fn attack_refuses_a_negative_addressed_object() {
    let mut fleet = plain_fleet(2);
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);

    bridge
        .process_all(
            &mut package,
            &build::attack(-1, 2, QueuePos::New),
            &mut fleet,
        )
        .unwrap();

    assert_eq!(installed(&fleet, 0), 0);
    assert_eq!(installed(&fleet, 1), 0);
}

#[test]
fn attack_runs_action_begin_and_clears_disband() {
    let mut fleet = plain_fleet(2);
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    bridge.groups.get_mut(package.group).unwrap().disband = 1;

    bridge
        .process_all(
            &mut package,
            &build::attack(0, 2, QueuePos::New),
            &mut fleet,
        )
        .unwrap();

    // The vtable `+0x14` call is `Group::action_begin` `0x00714100`: `disband = 0`.
    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 0);
}

#[test]
fn patrol_runs_action_begin_and_clears_form_before_installing() {
    let mut fleet = plain_fleet(2);
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    {
        let group = bridge.groups.get_mut(package.group).unwrap();
        group.disband = 1;
        group.form = Formation::Line as i32;
    }

    bridge
        .process_all(
            &mut package,
            &build::patrol(6_000, 7_000, QueuePos::New),
            &mut fleet,
        )
        .unwrap();

    let group = bridge.groups.get(package.group).unwrap();
    assert_eq!(group.disband, 0);
    assert_eq!(group.form, -1, "`mov [this + 0x10], -1` at 0x007030F3");
    assert_eq!(installed(&fleet, 0), 1);
}

#[test]
fn a_destination_past_the_map_edge_is_clamped_to_the_last_coord() {
    let mut fleet = plain_fleet(1);
    fleet.set_map_tiles(Some((10, 10)));
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0]);

    bridge
        .process_all(
            &mut package,
            &build::move_to(999_999, -50, QueuePos::New, 1),
            &mut fleet,
        )
        .unwrap();

    let order = fleet
        .get(WHO as u8, 0)
        .unwrap()
        .orders
        .current()
        .cloned()
        .unwrap();
    // 10 tiles * 0x300 Coord - 1. The lower bound is a plain zero clamp.
    assert_eq!((order.x, order.y), (10 * 0x300 - 1, 0));
}

#[test]
fn a_host_without_map_bounds_passes_the_destination_through_unclamped() {
    let mut fleet = plain_fleet(1);
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0]);

    bridge
        .process_all(
            &mut package,
            &build::move_to(999_999, -50, QueuePos::New, 1),
            &mut fleet,
        )
        .unwrap();

    let order = fleet
        .get(WHO as u8, 0)
        .unwrap()
        .orders
        .current()
        .cloned()
        .unwrap();
    assert_eq!((order.x, order.y), (999_999, -50));
}

#[test]
fn an_armed_scenario_prune_without_a_committed_host_installs_nothing() {
    let mut fleet = plain_fleet(2);
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    fleet.set_scenario_ignore_orders(true, false);

    bridge
        .process_all(
            &mut package,
            &build::attack(0, 2, QueuePos::New),
            &mut fleet,
        )
        .unwrap();
    assert_eq!(installed(&fleet, 0), 0, "the prune has no committed host");

    // Declaring the prune committed releases exactly the same command.
    fleet.set_scenario_ignore_orders(true, true);
    bridge
        .process_all(
            &mut package,
            &build::attack(0, 2, QueuePos::New),
            &mut fleet,
        )
        .unwrap();
    assert_eq!(installed(&fleet, 0), 1);
}

#[test]
fn form_never_consults_the_scenario_prune() {
    // `Group::action_form` `0x00707220` has no `0x00CC02F8` test at all, so an armed
    // uncommitted prune must not stop it the way it stops `attack`.
    let mut fleet = formation_fleet(2);
    let mut bridge = Bridge::new();
    let mut package = Package::new(WHO.into(), 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    fleet.set_scenario_ignore_orders(true, false);

    bridge
        .process_all(
            &mut package,
            &build::form(Formation::Line as i32, 0, QueuePos::Last),
            &mut fleet,
        )
        .unwrap();

    assert_eq!(fleet.get(WHO as u8, 0).unwrap().form, Formation::Line as i8);
    assert!(!dispatch_program("form")
        .unwrap()
        .gates
        .contains(&EntryGate::IgnoreOrdersPrune));
}

#[test]
fn move_to_runs_move_near_s_program_because_its_own_body_is_a_forwarder() {
    let own = dispatch_program("move_to").unwrap();
    assert_eq!(own.action, "move_near");
    assert_eq!(own.va, 0x0070_4990);
}
