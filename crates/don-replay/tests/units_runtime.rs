#[path = "../src/units_runtime.rs"]
mod units_runtime;

use don_replay::check_all::CheckAll;
use don_replay::checksum::Channel;
use don_sim::container::EngineArray;
use don_sim::order::Order;
use don_sim::systems::movement::PathData;
use don_sim::world::World;
use units_runtime::{
    check_world_units, unit_walk_bytes, GuyArrayWalkFacts, UnitWalkError, UnitWalkFacts,
    UnitsRuntimeError, UnitsWalkAuthority, EMPTY_LIVE_UNIT_WALK_BYTES,
};

fn spawn(world: &mut World, owner: u8, ptype: i32) -> (don_sim::Handle, usize) {
    let handle = world.spawn_typed(owner, ptype).unwrap();
    let row = world.row_of(handle).unwrap();
    (handle, row)
}

fn authority(rows: impl IntoIterator<Item = (don_sim::Handle, i32)>) -> UnitsWalkAuthority {
    let mut authority = UnitsWalkAuthority::default();
    for (handle, ptype) in rows {
        authority.install(handle, UnitWalkFacts::initialized_empty(Some(ptype)));
    }
    authority
}

#[test]
fn inherited_walk_has_three_must_bytes_and_every_fixed_window() {
    let mut world = World::with_capacity(4, 1);
    let (_handle, row) = spawn(&mut world, 3, 77);
    world.units.z_internal_mut()[row] = 0x0102_0304;
    world.units.x_internal_mut()[row] = 0x1122_3344;
    world.units.y_internal_mut()[row] = 0x5566_7788;
    world.units.myhits_mut()[row] = 0x2030_4050;
    world.units.collide_frame_mut()[row] = 0x6070_1020;

    let ptype = 0x1020_3040;
    let facts = UnitWalkFacts::initialized_empty(Some(ptype));
    let bytes = unit_walk_bytes(&world.units, row, world.orders(row), &facts).unwrap();

    assert_eq!(bytes.len() as u64, EMPTY_LIVE_UNIT_WALK_BYTES);
    assert_eq!(bytes[0], 1, "SubObject flags");
    assert_eq!(bytes[1], 1, "SubObject concrete must_walk result");
    assert_eq!(bytes[2], 3, "SubObject who");
    assert_eq!(&bytes[3..5], &0i16.to_le_bytes(), "SubObject o");
    assert_eq!(&bytes[5..9], &(0x0102_0304i32 ^ 0x0006_3637).to_le_bytes());
    assert_eq!(&bytes[9..13], &(0x1122_3344i32 ^ 0x0006_3637).to_le_bytes());
    assert_eq!(
        &bytes[13..17],
        &(0x5566_7788i32 ^ 0x0006_3637).to_le_bytes()
    );
    assert_eq!(&bytes[17..21], &ptype.to_le_bytes());
    assert_eq!(bytes[21], 1, "Object concrete must_walk result");
    assert_eq!(&bytes[22..26], &0x2030_4050i32.to_le_bytes());
    assert_eq!(bytes[56], 0, "authoritative null launching pointer");
    assert_eq!(bytes[57], 1, "Unit concrete must_walk result");
    assert_eq!(&bytes[58..62], &0x6070_1020i32.to_le_bytes());
    assert_eq!(&bytes[169..173], &10i32.to_le_bytes(), "Stack size");
    assert_eq!(&bytes[173..177], &0i32.to_le_bytes(), "Stack length");
    assert_eq!(bytes[177], 0xff, "Stack byte increment");
    assert_eq!(&bytes[178..182], &0i32.to_le_bytes(), "Order count");
    assert_eq!(&bytes[182..186], &0i32.to_le_bytes(), "Guy count");
}

#[test]
fn launching_and_path_history_are_real_checksum_bytes() {
    let mut world = World::with_capacity(2, 2);
    let (_handle, row) = spawn(&mut world, 0, 7);
    let null = UnitWalkFacts::initialized_empty(Some(7));
    let null_bytes = unit_walk_bytes(&world.units, row, world.orders(row), &null).unwrap();

    let mut present_empty = null.clone();
    present_empty.launching = Some(EngineArray::new());
    let present_empty_bytes =
        unit_walk_bytes(&world.units, row, world.orders(row), &present_empty).unwrap();
    assert_eq!(present_empty_bytes.len(), null_bytes.len() + 4);
    assert_eq!(null_bytes[56], 0);
    assert_eq!(present_empty_bytes[56], 1);
    assert_eq!(&present_empty_bytes[57..61], &0i32.to_le_bytes());

    let mut dynamic = null.clone();
    let mut launching = EngineArray::new();
    launching.set_flags(0x43);
    launching.add(0x5566_7788);
    dynamic.launching = Some(launching);
    dynamic.path.push(PathData {
        to_x: 11,
        to_y: -13,
        tolerance: 17,
        flags: 0x12,
    });
    let dynamic_bytes = unit_walk_bytes(&world.units, row, world.orders(row), &dynamic).unwrap();
    assert_eq!(dynamic_bytes.len(), null_bytes.len() + 15 + 16);
    assert_ne!(dynamic_bytes, null_bytes);
    assert!(dynamic_bytes.windows(4).any(|w| w == 11i32.to_le_bytes()));
    assert!(dynamic_bytes
        .windows(4)
        .any(|w| w == (-13i32).to_le_bytes()));
    assert!(dynamic_bytes
        .windows(4)
        .any(|w| w == 0x5566_7788i32.to_le_bytes()));
    assert_eq!(
        dynamic_bytes[67], 0x03,
        "SimpleArray walk clears pointer-owned bit 0x40 before hashing flags"
    );
}

#[test]
fn channel_is_owner_major_even_when_dense_rows_are_reversed() {
    let mut world = World::with_capacity(4, 3);
    let (owner1, owner1_row) = spawn(&mut world, 1, 101);
    let (owner0, owner0_row) = spawn(&mut world, 0, 202);
    let authority = authority([(owner1, 101), (owner0, 202)]);

    let result = check_world_units(&world, &authority).unwrap();
    let owner0_bytes = unit_walk_bytes(
        &world.units,
        owner0_row,
        world.orders(owner0_row),
        authority.get(owner0).unwrap(),
    )
    .unwrap();
    let owner1_bytes = unit_walk_bytes(
        &world.units,
        owner1_row,
        world.orders(owner1_row),
        authority.get(owner1).unwrap(),
    )
    .unwrap();
    let expected =
        don_sim::checksum::adler32(don_sim::checksum::adler32(1, &owner0_bytes), &owner1_bytes);
    let dense_row_order =
        don_sim::checksum::adler32(don_sim::checksum::adler32(1, &owner1_bytes), &owner0_bytes);

    assert_eq!(result.checksum, expected);
    assert_ne!(result.checksum, dense_row_order);
    assert_eq!(result.units_walked, 2);
    assert_eq!(result.registry_entries, 2);
    assert_eq!(result.bytes_walked, 2 * EMPTY_LIVE_UNIT_WALK_BYTES);
}

#[test]
fn stable_authority_survives_dense_compaction() {
    let mut world = World::with_capacity(4, 4);
    let (removed, _) = spawn(&mut world, 0, 17);
    let (survivor, old_row) = spawn(&mut world, 1, 29);
    assert_eq!(old_row, 1);
    let mut authority = authority([(removed, 17), (survivor, 29)]);

    assert!(world.despawn(removed));
    authority.remove(removed);
    assert_eq!(world.row_of(survivor), Some(0));
    let result = check_world_units(&world, &authority).unwrap();
    assert_eq!(result.units_walked, 1);
    assert_eq!(result.registry_entries, 1);
    assert_eq!(result.bytes_walked, EMPTY_LIVE_UNIT_WALK_BYTES);
}

#[test]
fn inactive_owner_and_tenth_owner_do_not_demand_walk_authority() {
    let mut inactive = World::with_capacity(2, 5);
    spawn(&mut inactive, 0, 7);
    assert!(inactive.set_object_owner_active(0, false));
    let result = check_world_units(&inactive, &UnitsWalkAuthority::default()).unwrap();
    assert_eq!(result.checksum, 1);
    assert_eq!(result.bytes_walked, 0);
    assert_eq!(result.registry_entries, 1);

    let mut tenth = World::with_capacity(2, 6);
    spawn(&mut tenth, 9, 8);
    let result = check_world_units(&tenth, &UnitsWalkAuthority::default()).unwrap();
    assert_eq!(result.checksum, 1);
    assert_eq!(result.bytes_walked, 0);
    assert_eq!(result.registry_entries, 1);
    assert_eq!(result.skipped_outside_walk, 1);
}

#[test]
fn missing_dynamic_authority_and_flattened_order_nodes_fail_closed() {
    let mut world = World::with_capacity(2, 7);
    let (handle, row) = spawn(&mut world, 0, 33);
    assert_eq!(
        check_world_units(&world, &UnitsWalkAuthority::default()),
        Err(UnitsRuntimeError::MissingWalkAuthority {
            row,
            owner: 0,
            o: 0,
            handle,
        })
    );

    let mut authority = authority([(handle, 33)]);
    assert!(world.issue(handle, Order::move_to(10, 20, 30)));
    assert_eq!(
        check_world_units(&world, &authority),
        Err(UnitsRuntimeError::UnitWalk {
            row,
            owner: 0,
            o: 0,
            source: UnitWalkError::NonEmptyOrders { row, length: 1 },
        })
    );

    world.orders_mut(row).clear();
    let mut nonempty_guys = UnitWalkFacts::initialized_empty(Some(33));
    nonempty_guys.guys = GuyArrayWalkFacts::NonEmptyUnsupported { length: 3 };
    authority.install(handle, nonempty_guys);
    assert_eq!(
        check_world_units(&world, &authority),
        Err(UnitsRuntimeError::UnitWalk {
            row,
            owner: 0,
            o: 0,
            source: UnitWalkError::NonEmptyGuyArrayUnsupported { row, length: 3 },
        })
    );
}

#[test]
fn current_generic_bridge_is_an_audit_probe_not_an_exact_unit_walk() {
    let mut world = World::with_capacity(2, 8);
    let (_handle, _row) = spawn(&mut world, 0, 44);
    let generic = CheckAll::of_world(&world);
    let units = generic.per[Channel::Units as usize];

    assert_eq!(units.elements, 1);
    assert_eq!(units.bytes, 178);
    assert_eq!(units.outcome.ops_missed(), 17);
    assert!(!units.complete());

    let row = 0;
    let exact = unit_walk_bytes(
        &world.units,
        row,
        world.orders(row),
        &UnitWalkFacts::initialized_empty(Some(44)),
    )
    .unwrap();
    assert_eq!(exact.len() as u64, EMPTY_LIVE_UNIT_WALK_BYTES);
    assert_ne!(units.bytes, exact.len() as u64);
}
