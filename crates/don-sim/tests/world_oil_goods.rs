mod container {
    pub use don_sim::container::*;
}

mod systems {
    pub mod economy {
        pub use don_sim::systems::economy::*;
    }
    pub mod map_terrain {
        pub use don_sim::systems::map_terrain::*;
    }
}

#[path = "../src/systems/world_oil_goods.rs"]
mod world_oil_goods;

use don_sim::checksum::adler32;
use std::fs;
use systems::map_terrain::{tflag, wflag, World};
use world_oil_goods::*;

fn world() -> World {
    World::init_default_rules(8, 8)
}

fn centre_tile(wx: i32, wy: i32) -> (i32, i32) {
    (wx * 4 + 2, wy * 4 + 2)
}

#[test]
fn first_oil_allocates_four_slots_and_hashes_encoded_subobject_words() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    let request = OilGoodMutation::at(3, 5, true);

    let receipt = apply_world_set_oil_at(&mut world, &mut goods, request).unwrap();

    assert_eq!(receipt.primitive_va, WORLD_SET_OIL_AT_VA);
    assert_eq!(OBJECTS_INIT_GOOD_VA, 0x0065_3f30);
    assert_eq!(GOOD_CLOSE_VA, 0x0066_d860);
    assert_eq!(GOOD_INIT_VA, 0x0066_da20);
    assert_eq!(receipt.rng_draws, 0);
    assert!(receipt.closed_slots.is_empty());
    assert_eq!(
        receipt.allocation,
        Some(GoodAllocationReceipt {
            kind: GoodAllocationKind::Appended,
            slot: 0,
            capacity_before: 0,
            capacity_after: 4,
        })
    );
    assert_eq!(goods.capacity, 4);
    assert_eq!(goods.slots.len(), 1);
    assert_eq!(goods.good_mark, 1);
    assert_ne!(world.wdata(3, 5).flags & wflag::OIL, 0);
    assert_eq!(receipt.changed_world_sections, vec![5]);
    assert_eq!(receipt.changed_wdata, vec![5 * 8 + 3]);
    assert!(receipt.changed_tdata.is_empty());

    let slot = goods.slots[0];
    assert_eq!(slot.node.flags, 1);
    assert_eq!(slot.node.who, u8::MAX);
    assert_eq!(slot.node.o, 0);
    assert_eq!(slot.node.z, SUBOBJECT_COORD_XOR);
    assert_eq!(slot.coord_z(), 0);
    assert_eq!(slot.coord_x(), request.coord_x);
    assert_eq!(slot.coord_y(), request.coord_y);
    assert_eq!(slot.node.type_index, OIL_GOOD_TYPE);
    assert_eq!(slot.node.ever_seen, 0);
    assert_eq!(slot.cur_time, 0);
    assert!(slot.ptype_present);

    let mut expected = Vec::new();
    expected.extend_from_slice(&[0, 1, 1, u8::MAX]);
    expected.extend_from_slice(&0i16.to_le_bytes());
    expected.extend_from_slice(&SUBOBJECT_COORD_XOR.to_le_bytes());
    expected.extend_from_slice(&(request.coord_x ^ SUBOBJECT_COORD_XOR).to_le_bytes());
    expected.extend_from_slice(&(request.coord_y ^ SUBOBJECT_COORD_XOR).to_le_bytes());
    expected.extend_from_slice(&OIL_GOOD_TYPE.to_le_bytes());
    assert_eq!(expected.len(), GOOD_WALKED_BYTES);
    assert_eq!(slot.node.walked_bytes().as_slice(), expected.as_slice());
    assert_eq!(goods.goods_checksum(), adler32(1, &expected));
    assert_eq!(goods.goods_checksum(), 0x2c78_0356);
    assert_eq!(receipt.after.goods_walked_bytes, 22);
    assert_eq!(
        goods.scenario_rows(),
        vec![ScenarioGoodRow {
            slot: 0,
            type_index: OIL_GOOD_TYPE,
            coord_x: request.coord_x,
            coord_y: request.coord_y,
        }]
    );
}

fn pe_offset(image: &[u8], va: u32) -> usize {
    let u16_at = |at| u16::from_le_bytes(image[at..at + 2].try_into().unwrap());
    let u32_at = |at| u32::from_le_bytes(image[at..at + 4].try_into().unwrap());
    let pe = u32_at(0x3c) as usize;
    let sections = u16_at(pe + 6) as usize;
    let optional_size = u16_at(pe + 20) as usize;
    let optional = pe + 24;
    let rva = va - u32_at(optional + 28);
    let table = optional + optional_size;
    for index in 0..sections {
        let section = table + index * 40;
        let virtual_size = u32_at(section + 8);
        let virtual_address = u32_at(section + 12);
        let raw_size = u32_at(section + 16);
        let raw = u32_at(section + 20);
        if (virtual_address..virtual_address + virtual_size.max(raw_size)).contains(&rva) {
            return (raw + rva - virtual_address) as usize;
        }
    }
    panic!("VA {va:#x} is outside the image")
}

#[test]
fn native_must_walk_byte_is_pe_backed_and_checksum_mutation_sensitive() {
    let node = don_sim::systems::economy::GoodNode {
        flags: 1,
        who: 2,
        o: 3,
        z: 4,
        x: 5,
        y: 6,
        type_index: 7,
        ever_seen: 8,
    };
    let retail = node.walked_bytes();
    assert_eq!(retail.len(), GOOD_WALKED_BYTES);
    assert_eq!(retail[2], 1);
    let mut mutated = retail;
    mutated[2] ^= 1;
    assert_ne!(adler32(1, &retail), adler32(1, &mutated));

    let old_omitting_must_walk = retail[..2]
        .iter()
        .chain(&retail[3..])
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(old_omitting_must_walk.len(), 21);
    assert_ne!(adler32(1, &retail), adler32(1, &old_omitting_must_walk));

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let exe_path = root.join("ron-bin/riseofnations.exe");
    if !exe_path.exists() {
        return;
    }
    let image = fs::read(exe_path).unwrap();
    let exact = [
        // check_goods: active-bit filter, base-Good scalar walk, SubObject call.
        (0x0093_7739, "f646080174348b063de847b400753ba17863c0008bcb8b138b4010054415010050ff52048b138d4621508d46208bcb50ff12538bcee85daad2ff"),
        // SubObject::walk_data: virtual must_walk call and conditional tail walk.
        (0x0066_21fe, "8b078bcf53ff507885c00f84930000008b038d7718568d4f09518bcbff10"),
        // SubObject::must_walk: derives the byte, walks it, and returns it.
        (0x0066_23a0, "558bec8b5508837a0400751483791800750af6410801c6450b007404c6450b018b028d4d0c518d4d0b518bcaff100fb6450b5dc20400"),
        // CheckSum constructor: DataWalk::input remains zero on the output path.
        (0x0045_dee9, "c7410400000000c7410cffffffffc745fc00000000c70120f9b300c7410801000000c7411000000000"),
    ];
    for (va, expected) in exact {
        let expected = expected
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect::<Vec<_>>();
        let offset = pe_offset(&image, va);
        assert_eq!(&image[offset..offset + expected.len()], expected);
    }
}

#[test]
fn disable_runs_oil_close_footprint_and_negative_clear_down_arm() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(2, 3, true)).unwrap();

    let (tx, ty) = centre_tile(2, 3);
    *world.tmask_mut(tx, ty) |= tflag::RESOURCE;
    let w = world.wdata_mut(2, 3);
    w.flags |= WDATA_GOOD_FOOTPRINT;
    w.down = -2;
    w.down_who = 17;

    let receipt =
        apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(2, 3, false)).unwrap();

    assert_eq!(receipt.closed_slots, vec![0]);
    assert_eq!(receipt.allocation, None);
    assert_eq!(receipt.changed_world_sections, vec![5, 6]);
    assert_eq!(
        receipt.changed_tdata,
        vec![(ty * world.tile_xs + tx) as usize]
    );
    assert_eq!(world.tmask(tx, ty) & tflag::RESOURCE, 0);
    let w = world.wdata(2, 3);
    assert_eq!(w.flags & (wflag::OIL | WDATA_GOOD_FOOTPRINT), 0);
    assert_eq!((w.down, w.down_who), (-1, -1));

    let slot = goods.slots[0];
    assert_eq!(slot.node.flags, 0);
    assert_eq!(slot.node.who, u8::MAX, "close preserves who");
    assert_eq!(slot.node.o, 0, "close preserves object index");
    assert_eq!(slot.node.ever_seen, 0, "close preserves ever_seen");
    assert_eq!(slot.cur_time, 0, "close preserves cur_time");
    assert_eq!(slot.node.z, CLOSED_COORD_INTERNAL);
    assert_eq!(slot.node.x, CLOSED_COORD_INTERNAL);
    assert_eq!(slot.node.y, CLOSED_COORD_INTERNAL);
    assert!(!slot.ptype_present);
    assert_eq!(
        goods.good_mark, 1,
        "close never lowers ObjectsData::good_mark"
    );
    assert_eq!(goods.capacity, 4, "close never shrinks storage");
    assert_eq!(goods.goods_checksum(), 1);
    assert!(goods.scenario_rows().is_empty());
}

#[test]
fn enable_replacement_closes_then_reuses_the_same_slot() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    let request = OilGoodMutation::at(4, 4, true);
    apply_world_set_oil_at(&mut world, &mut goods, request).unwrap();
    let before_checksum = goods.goods_checksum();

    let receipt = apply_world_set_oil_at(&mut world, &mut goods, request).unwrap();

    assert_eq!(receipt.closed_slots, vec![0]);
    assert_eq!(
        receipt.allocation,
        Some(GoodAllocationReceipt {
            kind: GoodAllocationKind::ReusedInactive,
            slot: 0,
            capacity_before: 4,
            capacity_after: 4,
        })
    );
    assert_eq!(goods.slots.len(), 1);
    assert_eq!(goods.good_mark, 1);
    assert!(goods.slots[0].active());
    assert_eq!(goods.goods_checksum(), before_checksum);
    assert_eq!(receipt.rng_draws, 0);
}

#[test]
fn every_duplicate_is_closed_before_the_earliest_hole_is_reinitialized() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    let target = OilGoodMutation::at(1, 1, true);
    apply_world_set_oil_at(&mut world, &mut goods, target).unwrap();
    apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(2, 2, true)).unwrap();

    goods.slots[1].node.x = target.coord_x ^ SUBOBJECT_COORD_XOR;
    goods.slots[1].node.y = target.coord_y ^ SUBOBJECT_COORD_XOR;

    let receipt = apply_world_set_oil_at(&mut world, &mut goods, target).unwrap();

    assert_eq!(receipt.closed_slots, vec![0, 1]);
    assert_eq!(receipt.allocation.unwrap().slot, 0);
    assert!(goods.slots[0].active());
    assert!(!goods.slots[1].active());
    assert_eq!(goods.good_mark, 2);
    assert_eq!(goods.scenario_rows().len(), 1);
    assert_eq!(goods.scenario_rows()[0].slot, 0);
}

#[test]
fn append_growth_is_zero_to_four_then_doubling() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    let mut receipts = Vec::new();
    for wx in 1..=5 {
        receipts.push(
            apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(wx, 1, true))
                .unwrap(),
        );
    }

    assert_eq!(receipts[0].allocation.unwrap().capacity_after, 4);
    assert_eq!(receipts[3].allocation.unwrap().capacity_after, 4);
    assert_eq!(
        receipts[4].allocation,
        Some(GoodAllocationReceipt {
            kind: GoodAllocationKind::Appended,
            slot: 4,
            capacity_before: 4,
            capacity_after: 8,
        })
    );
    assert_eq!(goods.slots.len(), 5);
    assert_eq!(goods.capacity, 8);
    assert_eq!(goods.good_mark, 5);
}

#[test]
fn allocation_scans_full_length_and_preserves_save_slot_order() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    for wx in 1..=3 {
        apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(wx, 2, true)).unwrap();
    }
    apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(2, 2, false)).unwrap();

    let receipt =
        apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(6, 2, true)).unwrap();

    assert_eq!(receipt.allocation.unwrap().slot, 1);
    assert_eq!(goods.slots.len(), 3);
    assert_eq!(goods.good_mark, 3);
    assert_eq!(
        goods
            .scenario_rows()
            .iter()
            .map(|row| (row.slot, row.coord_x))
            .collect::<Vec<_>>(),
        vec![
            (0, OilGoodMutation::at(1, 2, true).coord_x),
            (1, OilGoodMutation::at(6, 2, true).coord_x),
            (2, OilGoodMutation::at(3, 2, true).coord_x)
        ]
    );
}

#[test]
fn checksum_scans_logical_length_while_mark_bounded_save_skips_a_tail_good() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    for wx in 1..=3 {
        apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(wx, 3, true)).unwrap();
    }
    apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(1, 3, false)).unwrap();
    apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(2, 3, false)).unwrap();

    // Distinguish the two native scan sources with holes before an active tail
    // row. This intentionally models an imported snapshot, not a state that
    // `Objects::init_good` itself would produce.
    goods.good_mark = 1;
    let tail = goods.slots[2].node;

    assert_eq!(goods.active_nodes(), vec![tail]);
    assert_eq!(goods.goods_checksum(), adler32(1, &tail.walked_bytes()));
    assert!(goods.scenario_rows().is_empty());
    let summary = goods.summary();
    assert_eq!(summary.active_count, 1);
    assert_eq!(summary.goods_walked_bytes, GOOD_WALKED_BYTES);
    assert!(summary.scenario_rows.is_empty());
}

#[test]
fn nonnegative_clear_down_head_refuses_both_owners_transactionally() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    let request = OilGoodMutation::at(2, 5, true);
    apply_world_set_oil_at(&mut world, &mut goods, request).unwrap();
    world.wdata_mut(2, 5).down = 7;
    world.wdata_mut(2, 5).down_who = 3;
    let world_before = world.clone();
    let goods_before = goods.clone();

    let error = apply_world_set_oil_at(&mut world, &mut goods, request).unwrap_err();

    assert_eq!(
        error,
        OilGoodMutationError::DownChainNeedsObjectBand {
            slot: 0,
            world_x: 2,
            world_y: 5,
            down: 7,
            down_who: 3,
        }
    );
    assert_eq!(world.wdata, world_before.wdata);
    assert_eq!(world.tdata, world_before.tdata);
    assert_eq!(world.checksum(), world_before.checksum());
    assert_eq!(goods, goods_before);
}

#[test]
fn alien_request_and_ungrowable_array_refuse_without_partial_oil_bit() {
    let mut world = world();
    let mut goods = OilGoodRuntime::default();
    goods.increment = 0;
    let world_before = world.clone();
    let goods_before = goods.clone();

    let error = apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(3, 3, true))
        .unwrap_err();
    assert_eq!(
        error,
        OilGoodMutationError::ArrayCannotGrow {
            capacity: 0,
            increment: 0,
        }
    );
    assert_eq!(world.wdata, world_before.wdata);
    assert_eq!(world.tdata, world_before.tdata);
    assert_eq!(goods, goods_before);

    let mut alien = OilGoodMutation::at(3, 3, true);
    alien.coord_x += 1;
    assert!(matches!(
        apply_world_set_oil_at(&mut world, &mut goods, alien),
        Err(OilGoodMutationError::WrongCentre { .. })
    ));
    assert_eq!(world.wdata, world_before.wdata);
    assert_eq!(goods, goods_before);
}

#[test]
fn invalid_sparse_mark_and_array_flags_are_fail_closed() {
    let mut world = world();
    let mut goods = OilGoodRuntime {
        slots: vec![OilGoodSlot::default()],
        capacity: 1,
        increment: -1,
        array_flags: 0,
        cur_index: 0,
        good_mark: 2,
    };
    let world_before = world.clone();
    let goods_before = goods.clone();
    assert!(matches!(
        apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(1, 1, true)),
        Err(OilGoodMutationError::InvalidGoodMark { .. })
    ));
    assert_eq!(world.wdata, world_before.wdata);
    assert_eq!(goods, goods_before);

    goods.good_mark = 0;
    goods.array_flags = 0x80;
    assert_eq!(
        apply_world_set_oil_at(&mut world, &mut goods, OilGoodMutation::at(1, 1, true)),
        Err(OilGoodMutationError::UnsupportedArrayFlags { flags: 0x80 })
    );
    assert_eq!(world.wdata, world_before.wdata);
}
