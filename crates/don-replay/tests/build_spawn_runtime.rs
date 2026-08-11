#[path = "../src/build_spawn_runtime.rs"]
mod subject;

use don_replay::builds_runtime::{
    check_sim_builds, BuildWalkFacts, BuildsWalkAuthority, EMPTY_LIVE_BUILD_WALK_BYTES,
};
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::tick::Sim;
use don_sim::world::WorldObjectIdentity;
use subject::{
    spawn_canonical_build, CanonicalBuildSpawnError, CanonicalBuildSpawnRequest, BUILD_INIT_VA,
    OBJECTS_FIND_FREE_VA, OBJECTS_INIT_BUILD_VA, OBJECT_INIT_VA, SUBOBJECT_COORD_XOR,
    SUBOBJECT_INIT_VA, WALL_INIT_VA,
};

const VILLAGE: i32 = 0x19e;

fn staged_build() -> BuildData {
    let mut build = BuildData::default();
    build.flags = production::flag::VALID;
    build.city = -1;
    build.gather_from.mtn = -1;
    build.gather_from.cliff = -1;
    build.who = 9;
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(-77i16).to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&0x1122_3344i32.to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&0x5566_7788i32.to_le_bytes());
    build
}

fn request(owner: u8, x: i32, y: i32) -> CanonicalBuildSpawnRequest {
    CanonicalBuildSpawnRequest {
        owner,
        type_index: VILLAGE,
        snapped_x: x,
        snapped_y: y,
        build: staged_build(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct MutationSnapshot {
    builds: usize,
    build_types: Vec<Option<i32>>,
    dense_builds: [Vec<u32>; 10],
    active: [bool; 10],
    sparse: don_sim::systems::sparse_object_bands_authority_frontier::SparseRegistrySnapshot<
        WorldObjectIdentity,
    >,
}

fn snapshot(sim: &Sim) -> MutationSnapshot {
    MutationSnapshot {
        builds: sim.builds.len(),
        build_types: sim.production_runtime.build_types.clone(),
        dense_builds: std::array::from_fn(|owner| {
            sim.world.objects.slot(owner).band(Band::Build).to_vec()
        }),
        active: std::array::from_fn(|owner| sim.world.objects.is_active(owner)),
        sparse: sim.world.object_bands().snapshot().unwrap(),
    }
}

#[test]
fn shipped_identity_call_chain_and_constants_are_pinned() {
    assert_eq!(OBJECTS_INIT_BUILD_VA, 0x0065_d190);
    assert_eq!(OBJECTS_FIND_FREE_VA, 0x0065_ad60);
    assert_eq!(BUILD_INIT_VA, 0x0062_9740);
    assert_eq!(WALL_INIT_VA, 0x0063_e9b0);
    assert_eq!(OBJECT_INIT_VA, 0x0064_7750);
    assert_eq!(SUBOBJECT_INIT_VA, 0x0066_2300);
    assert_eq!(SUBOBJECT_COORD_XOR, 0x63637);
}

#[test]
fn spawn_receipts_body_ptype_and_both_registry_owners() {
    let mut sim = Sim::new(0x51de, 8);
    let x = 17 * 0x300 + 0x180;
    let y = 23 * 0x300 + 0x180;
    let receipt = spawn_canonical_build(&mut sim, request(2, x, y)).unwrap();

    assert_eq!(receipt.row, 0);
    assert_eq!(receipt.owner, 2);
    assert_eq!(receipt.type_index, VILLAGE);
    assert_eq!(receipt.object_id, BUILD_BAND_BASE as i16);
    assert_eq!(receipt.snapped_position, (x, y));
    assert_eq!(receipt.encoded_position, (x ^ 0x63637, y ^ 0x63637));
    assert_eq!(receipt.build_mark_before, BUILD_BAND_BASE);
    assert_eq!(receipt.build_mark_after, BUILD_BAND_BASE + 1);
    assert!(!receipt.owner_active_before);
    assert!(receipt.owner_active_after);
    assert_eq!(receipt.dense_registry_row, 0);
    assert_eq!(receipt.sparse_identity, WorldObjectIdentity::BuildRow(0));
    assert_eq!(receipt.body_owner, 2);
    assert_eq!(receipt.body_object_id, BUILD_BAND_BASE as i16);
    assert_eq!(receipt.body_position, (x, y));
    assert_eq!(receipt.registered_ptype, VILLAGE);

    assert_eq!(sim.builds[0].who, 2);
    assert_eq!(sim.builds[0].object_id(), BUILD_BAND_BASE as i16);
    assert_eq!(sim.builds[0].position(), (x, y));
    assert_eq!(sim.production_runtime.build_types[0], Some(VILLAGE));
    assert_eq!(
        sim.world.objects.slot(2).band(Band::Build),
        &[0],
        "legacy dense registry"
    );
    assert_eq!(
        sim.world
            .object_bands()
            .live_identity(RetailObjectAddress::new(2, RetailBand::Build, 2000)),
        Some(WorldObjectIdentity::BuildRow(0)),
        "sparse canonical registry"
    );
    assert!(sim.world.object_bands_are_dense_equivalent());
}

#[test]
fn exact_spawn_is_immediately_admissible_to_the_builds_runtime() {
    let mut sim = Sim::new(0x61de, 8);
    let receipt = spawn_canonical_build(&mut sim, request(0, 0x1234, 0x5678)).unwrap();
    let mut authority = BuildsWalkAuthority::default();
    authority.install(receipt.row, BuildWalkFacts::default());

    let channel = check_sim_builds(&sim, &authority).unwrap();
    assert_eq!(channel.registry_entries, 1);
    assert_eq!(channel.builds_walked, 1);
    assert_eq!(channel.bytes_walked, EMPTY_LIVE_BUILD_WALK_BYTES);
    assert_ne!(channel.checksum, 1);
}

#[test]
fn consecutive_spawns_own_find_free_order_and_row_identity() {
    let mut sim = Sim::new(0x71de, 8);
    let first = spawn_canonical_build(&mut sim, request(1, 100, 200)).unwrap();
    let second = spawn_canonical_build(&mut sim, request(1, 300, 400)).unwrap();
    let other = spawn_canonical_build(&mut sim, request(0, 500, 600)).unwrap();

    assert_eq!((first.row, first.object_id), (0, 2000));
    assert_eq!((second.row, second.object_id), (1, 2001));
    assert_eq!((other.row, other.object_id), (2, 2000));
    assert_eq!(sim.world.objects.slot(1).band(Band::Build), &[0, 1]);
    assert_eq!(sim.world.objects.slot(0).band(Band::Build), &[2]);
    assert_eq!(sim.builds[0].position(), (100, 200));
    assert_eq!(sim.builds[1].position(), (300, 400));
    assert_eq!(sim.builds[2].position(), (500, 600));
    assert!(sim.world.object_bands_are_dense_equivalent());
}

#[test]
fn request_refusals_are_preflighted_without_mutation() {
    let cases = [
        (
            request(8, 1, 2),
            CanonicalBuildSpawnError::OwnerOutOfRange { owner: 8 },
        ),
        (
            CanonicalBuildSpawnRequest {
                type_index: -1,
                ..request(0, 1, 2)
            },
            CanonicalBuildSpawnError::TypeIndexOutOfRange {
                type_index: -1,
                type_rows: 806,
            },
        ),
        (
            CanonicalBuildSpawnRequest {
                type_index: 806,
                ..request(0, 1, 2)
            },
            CanonicalBuildSpawnError::TypeIndexOutOfRange {
                type_index: 806,
                type_rows: 806,
            },
        ),
        (
            CanonicalBuildSpawnRequest {
                build: BuildData {
                    flags: 0,
                    city: -1,
                    ..BuildData::default()
                },
                ..request(0, 1, 2)
            },
            CanonicalBuildSpawnError::InvalidBuildFlags { flags: 0 },
        ),
        (
            CanonicalBuildSpawnRequest {
                build: BuildData {
                    city: 4,
                    ..staged_build()
                },
                ..request(0, 1, 2)
            },
            CanonicalBuildSpawnError::IncomingCityAlreadyLinked { city: 4 },
        ),
    ];

    for (request, expected) in cases {
        let mut sim = Sim::new(0x81de, 8);
        let before = snapshot(&sim);
        assert_eq!(spawn_canonical_build(&mut sim, request), Err(expected));
        assert_eq!(snapshot(&sim), before);
    }
}

#[test]
fn preexisting_raw_spawn_identity_gap_is_refused_without_a_second_mutation() {
    let mut sim = Sim::new(0x91de, 8);
    let mut raw = staged_build();
    raw.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&0i16.to_le_bytes());
    sim.spawn_build(0, raw);
    let before = snapshot(&sim);

    assert_eq!(
        spawn_canonical_build(&mut sim, request(0, 5, 7)),
        Err(CanonicalBuildSpawnError::ExistingBuildObjectIdMismatch {
            row: 0,
            owner: 0,
            registry_object_id: BUILD_BAND_BASE,
            build_object_id: 0,
        })
    );
    assert_eq!(snapshot(&sim), before);
}

#[test]
fn split_dense_sparse_owner_and_premature_ptype_are_refused_without_mutation() {
    let mut split = Sim::new(0xa1de, 8);
    split.world.objects.insert(0, Band::Build, 0);
    let before = snapshot(&split);
    assert_eq!(
        spawn_canonical_build(&mut split, request(0, 5, 7)),
        Err(CanonicalBuildSpawnError::RegistryNotDenseEquivalent)
    );
    assert_eq!(snapshot(&split), before);

    let mut ptype = Sim::new(0xb1de, 8);
    ptype.production_runtime.register_build(0, 99);
    let before = snapshot(&ptype);
    assert_eq!(
        spawn_canonical_build(&mut ptype, request(0, 5, 7)),
        Err(CanonicalBuildSpawnError::PtypeRowAlreadyOwned {
            row: 0,
            type_index: 99,
        })
    );
    assert_eq!(snapshot(&ptype), before);
}
