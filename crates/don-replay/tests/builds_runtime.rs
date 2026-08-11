#[path = "../src/builds_runtime.rs"]
mod builds_runtime;

use builds_runtime::{
    build_walk_bytes, build_walk_value, check_sim_builds, BuildWalkError, BuildWalkFacts,
    BuildsRuntimeError, BuildsWalkAuthority, EMPTY_LIVE_BUILD_WALK_BYTES,
};
use don_sim::container::EngineArray;
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::systems::gathering::{GatherMiningList, GatherTile};
use don_sim::systems::production::{self, BuildData, BuildQueueEntry, GatherPoint};
use don_sim::tick::Sim;

fn live_build(object_id: u32) -> BuildData {
    let mut build = BuildData::default();
    build.flags = production::flag::VALID;
    build.founder = -7;
    build.max_age = 6;
    build.orig_type = 0x1122_3344;
    build.gather_from.mtn = -1;
    build.gather_from.cliff = -1;
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&(object_id as i16).to_le_bytes());
    build
}

fn spawn_exact(sim: &mut Sim, owner: usize, ptype: i32) -> usize {
    let slot = sim.world.objects.slot(owner).band(Band::Build).len();
    let object_id = BUILD_BAND_BASE + slot as u32;
    let row = sim.spawn_build(owner, live_build(object_id));
    sim.production_runtime.register_build(row, ptype);
    row
}

fn default_authority(rows: impl IntoIterator<Item = usize>) -> BuildsWalkAuthority {
    let mut authority = BuildsWalkAuthority::default();
    for row in rows {
        authority.install(row, BuildWalkFacts::default());
    }
    authority
}

#[test]
fn inherited_prefix_includes_every_emitted_gate_and_pointee_id() {
    let mut build = live_build(BUILD_BAND_BASE);
    build.who = 3;
    build.other[0x0b..0x18].copy_from_slice(&[
        0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad,
    ]);
    build.myhits = 0x5566_7788;
    let ptype = 0x1020_3040;
    let bytes = build_walk_bytes(&build, ptype, &BuildWalkFacts::default()).unwrap();

    assert_eq!(bytes.len() as u64, EMPTY_LIVE_BUILD_WALK_BYTES);
    assert_eq!(bytes[0], build.founder as u8);
    assert_eq!(bytes[1], build.max_age);
    assert_eq!(bytes[2], production::flag::VALID);
    assert_eq!(bytes[3], 1, "SubObject::must_walk result");
    assert_eq!(&bytes[4..19], &build.image()[0x09..0x18]);
    assert_eq!(&bytes[19..23], &ptype.to_le_bytes());
    assert_eq!(bytes[23], 1, "Object::must_walk result");
    assert_eq!(&bytes[24..58], &build.image()[0x20..0x42]);
    assert_eq!(bytes[58], 0, "authoritative null launching pointer");
    assert_eq!(bytes[59], 1, "WallData::must_walk result");
    assert_eq!(&bytes[60..90], &build.image()[0x48..0x66]);
    assert_eq!(bytes[90], 1, "BuildData::must_walk result");
    assert_eq!(&bytes[91..113], &build.image()[0x70..0x86]);
    assert_eq!(&bytes[113..117], &0i32.to_le_bytes());
    assert_eq!(&bytes[117..119], &[0xff, 0xff]);
    assert_eq!(&bytes[119..123], &0i32.to_le_bytes());
    assert_eq!(&bytes[123..127], &0i32.to_le_bytes());
    assert_eq!(&bytes[127..131], &build.orig_type.to_le_bytes());
}

#[test]
fn every_dynamic_container_changes_the_exact_stream_length() {
    let mut build = live_build(BUILD_BAND_BASE);
    let base = build_walk_bytes(&build, 7, &BuildWalkFacts::default()).unwrap();
    assert_eq!(base.len(), EMPTY_LIVE_BUILD_WALK_BYTES as usize);

    let mut launching = EngineArray::new();
    launching.set_flags(0x43);
    launching.add(0x5566_7788);
    let launching_facts = BuildWalkFacts {
        launching: Some(launching),
        ..BuildWalkFacts::default()
    };
    let with_launching = build_walk_bytes(&build, 7, &launching_facts).unwrap();
    assert_eq!(with_launching.len(), base.len() + 15);
    assert_ne!(with_launching, base);

    let mut mining = GatherMiningList::default();
    mining.add(GatherTile { tx: 19, ty: -23 });
    let mining_facts = BuildWalkFacts {
        launching: None,
        mining,
    };
    let with_mining = build_walk_bytes(&build, 7, &mining_facts).unwrap();
    assert_eq!(with_mining.len(), base.len() + 15);
    assert_ne!(with_mining, base);

    build.queue.entries.push(BuildQueueEntry {
        elapsed: 17,
        type_index: 81,
        tail: 0x7f7f,
        ..BuildQueueEntry::default()
    });
    let with_queue = build_walk_bytes(&build, 7, &BuildWalkFacts::default()).unwrap();
    assert_eq!(with_queue.len(), base.len() + 18);
    assert!(
        !with_queue
            .windows(2)
            .any(|window| window == 0x7f7fi16.to_le_bytes()),
        "BuildQueue's trailing short is not walked"
    );

    build.queue.entries.clear();
    build.gather.push(GatherPoint {
        x: 31,
        y: -37,
        action: 9,
        node_tag: 0x5a,
    });
    let with_gather = build_walk_bytes(&build, 7, &BuildWalkFacts::default()).unwrap();
    assert_eq!(with_gather.len(), base.len() + 14);
    let count_at = 123;
    assert_eq!(&with_gather[count_at..count_at + 4], &1i32.to_le_bytes());
    assert_eq!(&with_gather[count_at + 4..count_at + 8], &[0, 0, 0, 0]);
    assert_eq!(with_gather[count_at + 8], 0x5a);
}

#[test]
fn present_empty_launching_is_not_a_null_pointer() {
    let build = live_build(BUILD_BAND_BASE);
    let null = build_walk_bytes(&build, 3, &BuildWalkFacts::default()).unwrap();
    let present = build_walk_bytes(
        &build,
        3,
        &BuildWalkFacts {
            launching: Some(EngineArray::new()),
            ..BuildWalkFacts::default()
        },
    )
    .unwrap();
    assert_eq!(present.len(), null.len() + 4);
    assert_eq!(null[58], 0);
    assert_eq!(present[58], 1);
    assert_eq!(&present[59..63], &0i32.to_le_bytes());
}

#[test]
fn channel_uses_owner_then_band_order_not_dense_row_order() {
    let mut sim = Sim::new(1, 8);
    let owner1_row = spawn_exact(&mut sim, 1, 101);
    let owner0_row = spawn_exact(&mut sim, 0, 202);
    let authority = default_authority([owner1_row, owner0_row]);

    let channel = check_sim_builds(&sim, &authority).unwrap();
    let owner0 = build_walk_bytes(
        &sim.builds[owner0_row],
        202,
        authority.get(owner0_row).unwrap(),
    )
    .unwrap();
    let owner1 = build_walk_bytes(
        &sim.builds[owner1_row],
        101,
        authority.get(owner1_row).unwrap(),
    )
    .unwrap();
    let expected = don_sim::checksum::adler32(don_sim::checksum::adler32(1, &owner0), &owner1);
    let row_order = don_sim::checksum::adler32(don_sim::checksum::adler32(1, &owner1), &owner0);

    assert_eq!(channel.checksum, expected);
    assert_ne!(channel.checksum, row_order);
    assert_eq!(channel.builds_walked, 2);
    assert_eq!(channel.registry_entries, 2);
    assert_eq!(channel.bytes_walked, 2 * EMPTY_LIVE_BUILD_WALK_BYTES);
}

#[test]
fn inactive_owner_and_invalid_builds_contribute_no_bytes() {
    let mut inactive = Sim::new(2, 8);
    spawn_exact(&mut inactive, 0, 7);
    assert!(inactive.world.set_object_owner_active(0, false));
    let result = check_sim_builds(&inactive, &BuildsWalkAuthority::default()).unwrap();
    assert_eq!(result.checksum, 1);
    assert_eq!(result.bytes_walked, 0);
    assert_eq!(result.registry_entries, 1);

    let mut invalid = Sim::new(3, 8);
    let row = spawn_exact(&mut invalid, 0, 7);
    invalid.builds[row].flags = 0;
    let result = check_sim_builds(&invalid, &BuildsWalkAuthority::default()).unwrap();
    assert_eq!(result.checksum, 1);
    assert_eq!(result.bytes_walked, 0);
    assert_eq!(result.registry_entries, 1);
}

#[test]
fn dynamic_and_ptype_authority_are_mandatory_for_a_live_row() {
    let mut sim = Sim::new(4, 8);
    let row = spawn_exact(&mut sim, 0, 7);
    assert_eq!(
        check_sim_builds(&sim, &BuildsWalkAuthority::default()),
        Err(BuildsRuntimeError::MissingWalkAuthority {
            row,
            owner: 0,
            object_id: BUILD_BAND_BASE,
        })
    );

    sim.production_runtime.build_types[row] = None;
    let authority = default_authority([row]);
    assert_eq!(
        check_sim_builds(&sim, &authority),
        Err(BuildsRuntimeError::MissingBuildType {
            row,
            owner: 0,
            object_id: BUILD_BAND_BASE,
        })
    );
}

#[test]
fn spawn_builds_current_identity_gap_is_refused_instead_of_hashed() {
    let mut sim = Sim::new(5, 8);
    let mut build = live_build(0);
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&0i16.to_le_bytes());
    let row = sim.spawn_build(0, build);
    sim.production_runtime.register_build(row, 7);
    let authority = default_authority([row]);
    assert_eq!(
        check_sim_builds(&sim, &authority),
        Err(BuildsRuntimeError::BuildObjectIdMismatch {
            row,
            owner: 0,
            registry_object_id: BUILD_BAND_BASE,
            build_object_id: 0,
        })
    );
}

#[test]
fn registry_aliases_out_of_range_rows_and_unregistered_rows_are_refused() {
    let mut duplicate = Sim::new(6, 8);
    let row0 = spawn_exact(&mut duplicate, 0, 3);
    let _row1 = spawn_exact(&mut duplicate, 0, 4);
    duplicate
        .world
        .objects
        .repoint(0, Band::Build, BUILD_BAND_BASE + 1, row0 as u32);
    assert!(matches!(
        check_sim_builds(&duplicate, &BuildsWalkAuthority::default()),
        Err(BuildsRuntimeError::DuplicateRegistryRow { row, .. }) if row == row0
    ));

    let mut out_of_range = Sim::new(7, 8);
    spawn_exact(&mut out_of_range, 0, 3);
    out_of_range
        .world
        .objects
        .repoint(0, Band::Build, BUILD_BAND_BASE, 99);
    assert!(matches!(
        check_sim_builds(&out_of_range, &BuildsWalkAuthority::default()),
        Err(BuildsRuntimeError::RegistryRowOutOfRange { row: 99, .. })
    ));

    let mut unregistered = Sim::new(8, 8);
    unregistered.builds.push(live_build(BUILD_BAND_BASE));
    assert_eq!(
        check_sim_builds(&unregistered, &BuildsWalkAuthority::default()),
        Err(BuildsRuntimeError::UnregisteredBuildRow { row: 0 })
    );
}

#[test]
fn nonretail_build_owner_and_sidecar_tail_disagreement_are_refused() {
    let mut nature = Sim::new(9, 8);
    nature.spawn_build(8, live_build(BUILD_BAND_BASE));
    assert_eq!(
        check_sim_builds(&nature, &BuildsWalkAuthority::default()),
        Err(BuildsRuntimeError::UnsupportedBuildOwner {
            owner: 8,
            entries: 1,
        })
    );

    let build = live_build(BUILD_BAND_BASE);
    let mut mining = GatherMiningList::default();
    mining.mtn = 4;
    assert_eq!(
        build_walk_value(
            &build,
            7,
            &BuildWalkFacts {
                launching: None,
                mining,
            },
        ),
        Err(BuildWalkError::MiningTailMismatch {
            build_mtn: -1,
            build_cliff: -1,
            authority_mtn: 4,
            authority_cliff: -1,
        })
    );

    let mut split_owner = live_build(BUILD_BAND_BASE);
    split_owner.gather_from.tiles.push(0x0013_0025);
    assert_eq!(
        build_walk_value(&split_owner, 7, &BuildWalkFacts::default()),
        Err(BuildWalkError::LegacyMiningPayloadPresent { length: 1 })
    );
}
