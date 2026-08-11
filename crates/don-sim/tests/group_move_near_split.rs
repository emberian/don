// SPDX-License-Identifier: GPL-3.0-or-later
//! `Group::action_move_near` `0x00704990` — the selection split, the `buildings` refusal
//! and the removal of the `Group::normalize` prune, driven through the real wire path.
//!
//! Every case here goes `Bridge::process_all(build::group(..))` then
//! `Bridge::process_all(build::move_near(..))`, i.e. through `CommandPackage`'s opcode 0
//! and opcode 8 decoders and `Action::run`'s entry program, not through a planner call.

use don_sim::command::air_containment_host::AirObject;
use don_sim::command::group_move_near_split::{
    plan_move_near_split, MoveNearSplit, SplitFacts, SplitObject, SplitWorld,
    UNIT_FLAG_BOARDS_TRANSPORT,
};
use don_sim::command::{build, Bridge, ObjectTable, Package, QueuePos, Slot};
use don_sim::order::OrderIndex;

const WHO: u8 = 1;
const DEST: (i32, i32) = (4_800, 9_600);

/// A live captain at the origin whose containment column is answered as "not inside".
fn connected_unit(fleet: &mut ObjectTable, o: i16, domain: i32) {
    let mut slot = Slot::unit(100 + o as u16, 480, 960);
    slot.domain = domain;
    fleet.put(WHO, o, slot);
    fleet.set_air_object(WHO, o, AirObject::default());
}

fn select(bridge: &mut Bridge, package: &mut Package, fleet: &mut ObjectTable, list: &[i16]) {
    bridge
        .process_all(package, &build::group(WHO as i8, list), fleet)
        .expect("opcode 0 decodes");
}

fn move_near(bridge: &mut Bridge, package: &mut Package, fleet: &mut ObjectTable) {
    bridge
        .process_all(
            package,
            &build::move_near(DEST.0, DEST.1, 0, QueuePos::New, 0),
            fleet,
        )
        .expect("opcode 8 decodes");
}

fn installed(fleet: &ObjectTable, o: i16) -> Vec<(OrderIndex, i32, i32)> {
    fleet
        .get(WHO, o)
        .expect("slot")
        .orders
        .iter()
        .map(|order| (order.kind, order.x, order.y))
        .collect()
}

/// The slots this owner holds with at least one member, newest last.
fn populated_slots(bridge: &Bridge) -> Vec<(i32, Vec<i16>)> {
    (0..64 * 8)
        .filter_map(|slot| {
            let group = bridge.groups.get(slot)?;
            (group.num > 0 && group.who == WHO).then(|| {
                (
                    slot,
                    group.list[..group.num as usize].to_vec(),
                )
            })
        })
        .collect()
}

#[test]
fn a_mixed_land_and_sea_selection_is_split_and_the_receiver_is_cleared() {
    let mut fleet = ObjectTable::new(8);
    connected_unit(&mut fleet, 0, 0);
    connected_unit(&mut fleet, 1, 0);
    connected_unit(&mut fleet, 2, 1); // domain 1 == Sea

    let mut bridge = Bridge::new();
    // `Groups::get_open_slot` `0x006FA460` recycles the least-recently-stamped slot. At
    // frame 0 every slot ties with the just-made selection, so the split's second half can
    // legitimately land back on the receiver; a real frame separates them.
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1, 2]);
    let receiver = package.group;
    move_near(&mut bridge, &mut package, &mut fleet);

    // `Group::clear(-1)` on the receiver at `0x00704D19`.
    let cleared = bridge.groups.get(receiver).expect("receiver slot");
    assert_eq!(cleared.num, 0, "the receiver is abandoned by the split");
    assert_eq!(cleared.army, -1);
    assert_eq!(cleared.form, -1);

    // Two pushed halves, the land one first (`[ebp-0xa40]` is pushed at `0x00704C7B`).
    let halves: Vec<Vec<i16>> = populated_slots(&bridge)
        .into_iter()
        .map(|(_, list)| list)
        .collect();
    assert_eq!(halves, vec![vec![0, 1], vec![2]]);

    // Both re-issues installed the identical command.
    for o in [0, 1, 2] {
        assert_eq!(
            installed(&fleet, o),
            vec![(OrderIndex::MoveTo, DEST.0, DEST.1)],
            "member {o} still receives the command the split re-issued"
        );
    }
}

#[test]
fn an_embarked_member_moves_its_transport_instead_of_itself() {
    let mut fleet = ObjectTable::new(8);
    connected_unit(&mut fleet, 0, 0); // walks
    connected_unit(&mut fleet, 1, 0); // riding
    connected_unit(&mut fleet, 5, 1); // the transport
    fleet.set_air_object(
        WHO,
        1,
        AirObject {
            inside: Some((5, WHO as i32)),
            ..AirObject::default()
        },
    );

    let mut bridge = Bridge::new();
    // `Groups::get_open_slot` `0x006FA460` recycles the least-recently-stamped slot. At
    // frame 0 every slot ties with the just-made selection, so the split's second half can
    // legitimately land back on the receiver; a real frame separates them.
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    move_near(&mut bridge, &mut package, &mut fleet);

    let halves: Vec<Vec<i16>> = populated_slots(&bridge)
        .into_iter()
        .map(|(_, list)| list)
        .collect();
    assert_eq!(
        halves,
        vec![vec![0], vec![5]],
        "the passenger is replaced by the object ObjectData::get_inside names"
    );
    assert_eq!(installed(&fleet, 0).len(), 1);
    assert_eq!(
        installed(&fleet, 5),
        vec![(OrderIndex::MoveTo, DEST.0, DEST.1)],
        "the transport is what actually receives an order"
    );
    assert!(
        installed(&fleet, 1).is_empty(),
        "the passenger itself is never ordered"
    );
}

#[test]
fn an_all_land_selection_is_untouched_by_the_split() {
    let mut fleet = ObjectTable::new(8);
    for o in 0..3 {
        connected_unit(&mut fleet, o, 0);
    }
    let mut bridge = Bridge::new();
    // `Groups::get_open_slot` `0x006FA460` recycles the least-recently-stamped slot. At
    // frame 0 every slot ties with the just-made selection, so the split's second half can
    // legitimately land back on the receiver; a real frame separates them.
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1, 2]);
    let receiver = package.group;
    move_near(&mut bridge, &mut package, &mut fleet);

    assert_eq!(
        populated_slots(&bridge),
        vec![(receiver, vec![0, 1, 2])],
        "no extra group slot is consumed"
    );
    for o in 0..3 {
        assert_eq!(installed(&fleet, o), vec![(OrderIndex::MoveTo, DEST.0, DEST.1)]);
    }
}

#[test]
fn a_host_without_a_containment_column_keeps_the_pre_split_body() {
    // Same selection as the split case, but no `AirObject` anywhere: `Fleet::inside_of`
    // answers `None`, the plan is `Unanswered`, and the command must still install.
    let mut fleet = ObjectTable::new(8);
    for (o, domain) in [(0, 0), (1, 0), (2, 1)] {
        let mut slot = Slot::unit(100 + o as u16, 480, 960);
        slot.domain = domain;
        fleet.put(WHO, o, slot);
    }
    let mut bridge = Bridge::new();
    // `Groups::get_open_slot` `0x006FA460` recycles the least-recently-stamped slot. At
    // frame 0 every slot ties with the just-made selection, so the split's second half can
    // legitimately land back on the receiver; a real frame separates them.
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1, 2]);
    let receiver = package.group;
    move_near(&mut bridge, &mut package, &mut fleet);

    assert_eq!(populated_slots(&bridge), vec![(receiver, vec![0, 1, 2])]);
    for o in 0..3 {
        assert_eq!(
            installed(&fleet, o),
            vec![(OrderIndex::MoveTo, DEST.0, DEST.1)],
            "an unanswered column is a stated gap, never a silent refusal"
        );
    }
}

#[test]
fn a_building_selection_installs_nothing() {
    // `0x00704E59`: `if (this->buildings != 0) return`. The bridge used to install a
    // MOVE_TO on every member of a building selection.
    let mut fleet = ObjectTable::new(8);
    for o in 0..2 {
        let mut slot = Slot::unit(200 + o as u16, 480, 960);
        slot.is_building = true;
        fleet.put(WHO, o, slot);
        fleet.set_air_object(WHO, o, AirObject::default());
    }
    let mut bridge = Bridge::new();
    // `Groups::get_open_slot` `0x006FA460` recycles the least-recently-stamped slot. At
    // frame 0 every slot ties with the just-made selection, so the split's second half can
    // legitimately land back on the receiver; a real frame separates them.
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    assert_ne!(
        bridge.groups.get(package.group).expect("slot").buildings,
        0,
        "the selection really is a building group"
    );
    move_near(&mut bridge, &mut package, &mut fleet);
    for o in 0..2 {
        assert!(
            installed(&fleet, o).is_empty(),
            "building {o} must receive nothing"
        );
    }
}

#[test]
fn a_plane_led_selection_installs_nothing() {
    // `0x007050AD`: `GroupData::find_leader(0)`'s answer is tested against the inlined
    // `UnitData::is_plane` and a plane leader returns from the whole body at `0x007050C7`.
    let mut fleet = ObjectTable::new(8);
    connected_unit(&mut fleet, 0, 2);
    fleet.get_mut(WHO, 0).expect("slot").is_plane = true;
    connected_unit(&mut fleet, 1, 2);
    fleet.get_mut(WHO, 1).expect("slot").is_plane = true;

    let mut bridge = Bridge::new();
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    move_near(&mut bridge, &mut package, &mut fleet);
    for o in 0..2 {
        assert!(installed(&fleet, o).is_empty(), "plane {o} is not commanded");
    }
}

#[test]
fn a_leaderless_selection_installs_nothing() {
    // `0x0070507F` then `0x00705088`: `GroupData::find_leader(0)` returning `-1` skips
    // the entire remainder of the body.
    let mut fleet = ObjectTable::new(8);
    connected_unit(&mut fleet, 0, 0);
    fleet.get_mut(WHO, 0).expect("slot").is_captain = false;

    let mut bridge = Bridge::new();
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0]);
    move_near(&mut bridge, &mut package, &mut fleet);
    assert!(installed(&fleet, 0).is_empty());
}

#[test]
fn move_near_keeps_a_member_the_old_group_normalize_prune_dropped() {
    // `Group::normalize` `0x00711540` has 22 direct call sites and none of them is in
    // `action_move_near`, `action_move_to`, `action_form` or `action_attack` — the
    // full 9,205-byte decompilation of `0x00704990` contains no call to it and the
    // `Group` vtable has no slot for it. A member whose `leaves_groups` predicate holds
    // is therefore still commanded.
    let mut fleet = ObjectTable::new(8);
    connected_unit(&mut fleet, 0, 0);
    connected_unit(&mut fleet, 1, 0);
    fleet.get_mut(WHO, 1).expect("slot").leaves_groups = true;

    let mut bridge = Bridge::new();
    // `Groups::get_open_slot` `0x006FA460` recycles the least-recently-stamped slot. At
    // frame 0 every slot ties with the just-made selection, so the split's second half can
    // legitimately land back on the receiver; a real frame separates them.
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    move_near(&mut bridge, &mut package, &mut fleet);

    assert_eq!(
        bridge.groups.get(package.group).expect("slot").num,
        2,
        "the receiver keeps both members"
    );
    for o in 0..2 {
        assert_eq!(
            installed(&fleet, o),
            vec![(OrderIndex::MoveTo, DEST.0, DEST.1)],
            "member {o} is not pruned"
        );
    }
}

#[test]
fn form_keeps_a_member_the_old_group_normalize_prune_dropped() {
    // `Group::action_form` `0x00707220`'s `0x0070724D` is the vptr load for the
    // `call [eax + 0x14]` at `0x00707251`, i.e. `Group::action_begin` `0x00714100`
    // (`disband = 0`), not a `Group::normalize` call.
    let mut fleet = ObjectTable::new(8);
    connected_unit(&mut fleet, 0, 0);
    connected_unit(&mut fleet, 1, 0);
    fleet.get_mut(WHO, 1).expect("slot").leaves_groups = true;

    let mut bridge = Bridge::new();
    // `Groups::get_open_slot` `0x006FA460` recycles the least-recently-stamped slot. At
    // frame 0 every slot ties with the just-made selection, so the split's second half can
    // legitimately land back on the receiver; a real frame separates them.
    bridge.frame = 5;
    let mut package = Package::new(WHO as i32, 0);
    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    bridge
        .process_all(&mut package, &build::form(0, 0, QueuePos::Last), &mut fleet)
        .expect("opcode 3 decodes");

    let group = bridge.groups.get(package.group).expect("slot");
    assert_eq!(group.num, 2, "FORM does not prune either");
    assert_eq!(group.disband, 0, "action_begin still ran");
    assert_eq!(
        fleet.get(WHO, 1).expect("slot").form,
        0,
        "the pruned-away member still gets the formation write"
    );
}

// ---------------------------------------------------------------------------
// Planner-level pins that the wire path cannot express with this host.
// ---------------------------------------------------------------------------

struct OneObject(SplitObject);

impl SplitWorld for OneObject {
    fn object(&self, _who: u8, o: i16) -> Option<SplitObject> {
        let mut object = self.0;
        object.captain = o;
        object.domain = if o == 9 { 1 } else { object.domain };
        object.unit_flags = if o == 9 { 0 } else { UNIT_FLAG_BOARDS_TRANSPORT };
        Some(object)
    }
}

#[test]
fn pass_two_routes_the_transport_flag_into_the_first_half() {
    // Pass 1 cannot split here (only the sea unit lands in the second half), so retail
    // falls through to pass 2, whose first arm is `unit_flags & 0x10`.
    let world = OneObject(SplitObject {
        valid: true,
        is_unit: true,
        is_build: false,
        is_captain: true,
        captain: 0,
        subordinate: -1,
        role: 0,
        domain: 1,
        unit_flags: 0,
        inside: None,
    });
    let facts = SplitFacts {
        who: WHO,
        army: -1,
        facing: 0,
        destination_is_water: false,
        frame: 0,
    };
    let MoveNearSplit::Split { first, second, pass } = plan_move_near_split(&[3, 9], &facts, &world)
    else {
        panic!("pass 2 splits the transport-flagged member out");
    };
    assert_eq!(pass, 2);
    assert_eq!((first.num, first.list[0]), (1, 3));
    assert_eq!((second.num, second.list[0]), (1, 9));

    let water = SplitFacts {
        destination_is_water: true,
        ..facts
    };
    assert_eq!(
        plan_move_near_split(&[3, 9], &water, &world),
        MoveNearSplit::Continue,
        "a water destination skips pass 2 entirely"
    );
}
