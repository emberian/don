#[path = "../src/build_init_prefix.rs"]
mod subject;

use don_sim::systems::production::BuildData;
use subject::{
    apply_build_init_prefix, BuildInitPrefixError, BuildInitPrefixRequest, BuildLifecycleStage,
    BuildTypeInitFacts, BUILD_DATA_CTOR_VA, BUILD_INIT_VA, BUILD_MAX_AGE_XOR,
    BUILD_MINING_INCREMENT, BUILD_MINING_INITIAL_CAPACITY, MINING_LIST_CTOR_VA,
    OBJECT_ADD_TO_WORLD_VA, OBJECT_DETECTOR_FLAG, OBJECT_INIT_VA, SUBOBJECT_COORD_XOR,
    SUBOBJECT_FLAT_FLAG, SUBOBJECT_INIT_VA, WALL_INIT_VA,
};

const VILLAGE: i32 = 0x19e;

fn request() -> BuildInitPrefixRequest {
    BuildInitPrefixRequest {
        owner: 2,
        object_id: 2000,
        type_index: VILLAGE,
        type_rows: 806,
        snapped_x: 0x1357,
        snapped_y: 0x2468,
        terrain_z: -0x314,
        owner_uid_before: 37,
        max_age_source_byte: 0xa5,
        type_facts: BuildTypeInitFacts::default(),
    }
}

fn i16_at(image: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(image[offset..offset + 2].try_into().unwrap())
}

fn u16_at(image: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(image[offset..offset + 2].try_into().unwrap())
}

fn i32_at(image: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(image[offset..offset + 4].try_into().unwrap())
}

#[test]
fn shipped_prefix_chain_and_literals_are_pinned() {
    assert_eq!(BUILD_DATA_CTOR_VA, 0x0062_f370);
    assert_eq!(MINING_LIST_CTOR_VA, 0x0047_2260);
    assert_eq!(BUILD_INIT_VA, 0x0062_9740);
    assert_eq!(WALL_INIT_VA, 0x0063_e9b0);
    assert_eq!(OBJECT_INIT_VA, 0x0064_7750);
    assert_eq!(OBJECT_ADD_TO_WORLD_VA, 0x0064_d8c0);
    assert_eq!(SUBOBJECT_INIT_VA, 0x0066_2300);
    assert_eq!(SUBOBJECT_COORD_XOR, 0x63637);
    assert_eq!(BUILD_MAX_AGE_XOR, 0x66);
}

#[test]
fn starting_village_prefix_writes_identity_terrain_type_and_object_body() {
    let mut build = BuildData::default();
    let request = request();
    let receipt = apply_build_init_prefix(&mut build, request).unwrap();
    let image = build.image();

    assert_eq!(receipt.stage, BuildLifecycleStage::BeforeObjectAddToWorld);
    assert!(!receipt.init_complete());
    assert!(!receipt.activation_complete());
    assert_eq!(receipt.owner, 2);
    assert_eq!(receipt.object_id, 2000);
    assert_eq!(receipt.current_type, VILLAGE);
    assert_eq!(receipt.snapped_position, (0x1357, 0x2468));
    assert_eq!(receipt.terrain_z, -0x314);
    assert_eq!(
        receipt.encoded_position,
        (0x1357 ^ 0x63637, 0x2468 ^ 0x63637)
    );
    assert_eq!(receipt.encoded_terrain_z, -0x314 ^ 0x63637);
    assert_eq!(receipt.uid, 37);
    assert_eq!(receipt.max_age, 0xa5 ^ 0x66);
    assert!(receipt.pointer_facts.launching_is_null);
    assert_eq!(receipt.pointer_facts.mining_length, 0);
    assert_eq!(receipt.pointer_facts.mining_capacity, 5);
    assert_eq!(receipt.pointer_facts.mining_increment, -1);
    assert_eq!(receipt.pointer_facts.mining_flags, 0);
    assert_eq!(
        (
            receipt.pointer_facts.mining_mtn,
            receipt.pointer_facts.mining_cliff
        ),
        (-1, -1)
    );

    assert_eq!(build.who, 2);
    assert_eq!(build.flags, 1);
    assert_eq!(build.orig_type, VILLAGE);
    assert_eq!(build.founder, 2);
    assert_eq!(build.city, -1);
    assert_eq!(i16_at(&image, 0x0a), 2000);
    assert_eq!(i32_at(&image, 0x0c), -0x314 ^ 0x63637);
    assert_eq!(i32_at(&image, 0x10), 0x1357 ^ 0x63637);
    assert_eq!(i32_at(&image, 0x14), 0x2468 ^ 0x63637);
    assert_eq!(
        i32_at(&image, 0x18),
        0,
        "ptype address is held by the TypeIndex receipt"
    );
    assert_eq!(i16_at(&image, 0x28), -1);
    assert_eq!(i16_at(&image, 0x2a), -1);
    assert_eq!(i16_at(&image, 0x2c), -1);
    assert_eq!(i16_at(&image, 0x2e), -1);
    assert_eq!(u16_at(&image, 0x30), 37);
    assert_eq!(u16_at(&image, 0x32), 0);
    assert_eq!(i16_at(&image, 0x34), -1);
    assert_eq!(i16_at(&image, 0x36), -1);
    assert_eq!(i32_at(&image, 0x38), 0);
    assert_eq!(image[0x3e], 2);
    assert_eq!(image[0x3f], 0xff);
    assert_eq!(u16_at(&image, 0x40), 0);
    assert_eq!(i32_at(&image, 0x44), 0);
}

#[test]
fn constructor_sentinels_and_empty_container_shapes_are_owned() {
    let mut build = BuildData::default();
    build.queue.queued = 4;
    build.queue.entries.resize(4, Default::default());
    build.gather_from.tiles.push(0x1122_3344);
    build.gather.push(Default::default());
    build.gather_down = 6;
    build.city = 7;
    build.city_down = 8;
    build.wonder = 9;
    build.dock = 10;
    build.recharging = 11;
    build.attack_ox = 12;
    build.attack_whom = 13;
    build.gather_max = 14;

    apply_build_init_prefix(&mut build, request()).unwrap();

    assert_eq!(build.gpiece, -1);
    assert_eq!(build.frame_started, -1);
    assert_eq!(build.gather_down, -1);
    assert_eq!(build.city, -1);
    assert_eq!(build.city_down, -1);
    assert_eq!(build.wonder, -1);
    assert_eq!(build.dock, -1);
    assert_eq!(build.recharging, 0);
    assert_eq!(build.attack_ox, -1);
    assert_eq!(build.attack_whom, -1);
    assert_eq!(build.gather_max, 0);
    assert_eq!(build.queue.queued, 0);
    assert!(build.queue.entries.is_empty());
    assert_eq!(build.queue.entries.capacity(), 0);
    assert!(build.gather_from.tiles.is_empty());
    assert!(build.gather_from.tiles.capacity() >= BUILD_MINING_INITIAL_CAPACITY as usize);
    assert_eq!((build.gather_from.mtn, build.gather_from.cliff), (-1, -1));
    assert!(build.gather.is_empty());
    assert_eq!(BUILD_MINING_INCREMENT, -1);
}

#[test]
fn type_virtual_results_are_explicit_flags_not_inferred_from_type_number() {
    let mut build = BuildData::default();
    let mut request = request();
    request.type_facts = BuildTypeInitFacts {
        sets_flat_flag: true,
        sets_detector_flag: true,
    };

    let receipt = apply_build_init_prefix(&mut build, request).unwrap();
    assert_eq!(build.flags, 1 | SUBOBJECT_FLAT_FLAG | OBJECT_DETECTOR_FLAG);
    assert_eq!(receipt.flags, build.flags);
}

#[test]
fn fields_after_the_world_join_barrier_are_preserved_and_not_claimed() {
    let mut build = BuildData::default();
    build.construct_hits = 0x1234_5678;
    build.ever_seen = 0xa1;
    build.ever_seen_completed = 0xb2;
    build.stance = 0x33;
    build.infiltrate = 0xc4;
    build.infiltrate2 = 0xd5;

    let receipt = apply_build_init_prefix(&mut build, request()).unwrap();

    assert_eq!(build.construct_hits, 0x1234_5678);
    assert_eq!(build.ever_seen, 0xa1);
    assert_eq!(build.ever_seen_completed, 0xb2);
    assert_eq!(build.stance, 0x33);
    assert_eq!(build.infiltrate, 0xc4);
    assert_eq!(build.infiltrate2, 0xd5);
    assert!(!receipt.init_complete());
    assert!(!receipt.activation_complete());
}

#[test]
fn invalid_owner_and_type_are_refused_before_mutation() {
    let cases = [
        (
            BuildInitPrefixRequest {
                owner: 8,
                ..request()
            },
            BuildInitPrefixError::OwnerOutOfRange { owner: 8 },
        ),
        (
            BuildInitPrefixRequest {
                type_index: -1,
                ..request()
            },
            BuildInitPrefixError::TypeIndexOutOfRange {
                type_index: -1,
                type_rows: 806,
            },
        ),
        (
            BuildInitPrefixRequest {
                type_index: 806,
                ..request()
            },
            BuildInitPrefixError::TypeIndexOutOfRange {
                type_index: 806,
                type_rows: 806,
            },
        ),
    ];

    for (request, expected) in cases {
        let mut build = BuildData::default();
        build.construct_hits = 99;
        build.other.fill(0x5a);
        let before = build.image();
        assert_eq!(apply_build_init_prefix(&mut build, request), Err(expected));
        assert_eq!(build.image(), before);
        assert_eq!(build.construct_hits, 99);
    }
}
