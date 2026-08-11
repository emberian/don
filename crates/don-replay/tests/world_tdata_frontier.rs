#[path = "../src/world_tdata_frontier.rs"]
mod world_tdata_frontier;

use don_sim::checksum::adler32;
use don_sim::systems::map_terrain::{World, WorldSection};
use world_tdata_frontier::{
    execute_tdata_and_fog_wipe, tdata_and_fog_image, wcoord_seen_image, ByteRange, TDataFogPlane,
    TDataFogWipeError, CLEAR_SEEN2_CALL_VA, CLEAR_SEEN3_MEMSET_VA, CLEAR_SEEN_CALL_VA,
    SHIPPED_EXE_SHA256, TDATA_ZERO_LOOP_WRITE_VA, WORLD_CLEAR_SEEN2_VA, WORLD_CLEAR_SEEN_VA,
    WORLD_CLEAR_WCOORD_SEEN_MEMSET_CALL_VA, WORLD_WIPE_BODY_BYTES, WORLD_WIPE_BODY_SHA256,
    WORLD_WIPE_RETURN_VA, WORLD_WIPE_VA,
};

#[test]
fn wipe_receipts_every_section_six_store_not_only_changed_bytes() {
    let mut world = World::init_default_rules(40, 40);
    world.tdata.fill(0x1234);
    world.seen.fill(0x11);
    world.seen2.fill(0x22);
    world.seen3.fill(0x44);
    world.wcoord_seen.fill(0x80);

    let receipt = execute_tdata_and_fog_wipe(&mut world).expect("valid wipe");
    assert_eq!(receipt.entry_va, WORLD_WIPE_VA);
    assert_eq!(receipt.resume_va, WORLD_WIPE_RETURN_VA);
    assert_eq!(
        WORLD_WIPE_RETURN_VA - WORLD_WIPE_VA,
        WORLD_WIPE_BODY_BYTES as u32
    );
    assert_eq!(receipt.tdata_cells_written, 25_600);
    assert_eq!(receipt.fog_cells_written_per_plane, 6_400);
    assert_eq!(receipt.layout.tdata.start, 0);
    assert_eq!(receipt.layout.tdata.end, 51_200);
    assert_eq!(receipt.layout.seen.start, 51_200);
    assert_eq!(receipt.layout.seen3.end, 70_400);
    assert_eq!(receipt.section_bytes_written, 70_400);
    assert_eq!(receipt.wcoord_seen_bytes_written, 1_600);
    assert_eq!(receipt.total_bytes_written, 72_000);
    assert_eq!(receipt.section_bytes_changed, 70_400);
    assert_eq!(receipt.wcoord_seen_bytes_changed, 1_600);
    assert_eq!(receipt.total_bytes_changed, 72_000);
    assert_eq!(
        receipt.changed_ranges,
        vec![ByteRange {
            start: 0,
            end: 70_400,
        }]
    );
    assert_eq!(receipt.written_ranges[0].plane, TDataFogPlane::TData);
    assert_eq!(receipt.written_ranges[0].range, receipt.layout.tdata);
    assert_eq!(
        receipt.written_ranges[0].producer_va,
        TDATA_ZERO_LOOP_WRITE_VA
    );
    assert_eq!(receipt.written_ranges[1].range, receipt.layout.seen);
    assert_eq!(receipt.written_ranges[1].producer_va, CLEAR_SEEN_CALL_VA);
    assert_eq!(receipt.written_ranges[2].range, receipt.layout.seen2);
    assert_eq!(receipt.written_ranges[2].producer_va, CLEAR_SEEN2_CALL_VA);
    assert_eq!(receipt.written_ranges[3].range, receipt.layout.seen3);
    assert_eq!(receipt.written_ranges[3].producer_va, CLEAR_SEEN3_MEMSET_VA);
    assert_eq!(receipt.written_ranges[4].plane, TDataFogPlane::WCoordSeen);
    assert_eq!(
        receipt.written_ranges[4].range,
        ByteRange {
            start: 0,
            end: 1_600
        }
    );
    assert_eq!(
        receipt.written_ranges[4].producer_va,
        WORLD_CLEAR_WCOORD_SEEN_MEMSET_CALL_VA
    );
    assert_eq!(receipt.rewritten_zero_bytes, 0);
    assert_eq!(receipt.nonzero_tdata_words_before, 25_600);
    assert_eq!(receipt.nonzero_seen_bytes_before, 6_400);
    assert_eq!(receipt.nonzero_seen2_bytes_before, 6_400);
    assert_eq!(receipt.nonzero_seen3_bytes_before, 6_400);
    assert_eq!(receipt.nonzero_wcoord_seen_bytes_before, 1_600);
    assert_eq!(receipt.rng_draws, 0);
    assert_eq!(receipt.section_adler_after, adler32(1, &vec![0; 70_400]));
    assert_eq!(receipt.wcoord_seen_adler_after, adler32(1, &vec![0; 1_600]));
    assert!(tdata_and_fog_image(&world).iter().all(|&byte| byte == 0));
    assert!(wcoord_seen_image(&world).iter().all(|&byte| byte == 0));
}

#[test]
fn already_zero_plane_is_still_fully_produced() {
    let mut world = World::init_default_rules(70, 70);
    let before = tdata_and_fog_image(&world);
    assert!(before.iter().all(|&byte| byte == 0));

    let receipt = execute_tdata_and_fog_wipe(&mut world).expect("valid zero rewrite");
    assert_eq!(receipt.section_bytes_written, 215_600);
    assert_eq!(receipt.wcoord_seen_bytes_written, 4_900);
    assert_eq!(receipt.total_bytes_written, 220_500);
    assert_eq!(receipt.section_bytes_changed, 0);
    assert_eq!(receipt.wcoord_seen_bytes_changed, 0);
    assert!(receipt.changed_ranges.is_empty());
    assert!(receipt.wcoord_seen_changed_ranges.is_empty());
    assert_eq!(
        receipt
            .written_ranges
            .iter()
            .map(|written| written.range.len())
            .sum::<usize>(),
        220_500
    );
    assert_eq!(receipt.rewritten_zero_bytes, 220_500);
    assert_eq!(receipt.section_adler_before, receipt.section_adler_after);
}

#[test]
fn malformed_plane_shape_refuses_without_mutation() {
    let mut world = World::init_default_rules(50, 50);
    world.tdata[9] = 0xabcd;
    world.seen3.pop();
    let before = world.clone();

    let error = execute_tdata_and_fog_wipe(&mut world).unwrap_err();
    assert_eq!(
        error,
        TDataFogWipeError::PlaneLengthMismatch {
            plane: TDataFogPlane::Seen3,
            expected: 10_000,
            actual: 9_999,
        }
    );
    assert_eq!(world.tdata, before.tdata);
    assert_eq!(world.seen, before.seen);
    assert_eq!(world.seen2, before.seen2);
    assert_eq!(world.seen3, before.seen3);
    assert_eq!(world.wcoord_seen, before.wcoord_seen);
}

#[test]
fn malformed_wcoord_seen_shape_refuses_the_whole_wipe() {
    let mut world = World::init_default_rules(50, 50);
    world.tdata[9] = 0xabcd;
    world.wcoord_seen.pop();
    let before = world.clone();

    assert_eq!(
        execute_tdata_and_fog_wipe(&mut world),
        Err(TDataFogWipeError::PlaneLengthMismatch {
            plane: TDataFogPlane::WCoordSeen,
            expected: 2_500,
            actual: 2_499,
        })
    );
    assert_eq!(world.tdata, before.tdata);
    assert_eq!(world.seen, before.seen);
    assert_eq!(world.wcoord_seen, before.wcoord_seen);
}

#[test]
fn derived_dimensions_are_part_of_the_admission_gate() {
    let mut world = World::init_default_rules(60, 60);
    world.fog_xs += 1;
    let before = tdata_and_fog_image(&world);
    assert_eq!(
        execute_tdata_and_fog_wipe(&mut world),
        Err(TDataFogWipeError::DerivedDimensionMismatch {
            field: "fog_xs",
            expected: 120,
            actual: 121,
        })
    );
    assert_eq!(tdata_and_fog_image(&world), before);
}

#[test]
fn binary_anchor_chain_is_frozen() {
    assert_eq!(
        world_tdata_frontier::PROOF_DOCUMENT,
        "docs/assembly/replay-world-tdata-frontier.md"
    );
    assert_eq!(
        SHIPPED_EXE_SHA256,
        "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079"
    );
    assert_eq!(
        WORLD_WIPE_BODY_SHA256,
        "12f0886dfc5bff4dafb834a8e9d0f3743c4ba0ec8ba6024003712d81b01138f2"
    );
    assert_eq!(WORLD_WIPE_VA, 0x006b_2c00);
    assert_eq!(TDATA_ZERO_LOOP_WRITE_VA, 0x006b_2d43);
    assert_eq!(CLEAR_SEEN_CALL_VA, 0x006b_2d5f);
    assert_eq!(WORLD_CLEAR_SEEN_VA, 0x006b_2250);
    assert_eq!(WORLD_CLEAR_WCOORD_SEEN_MEMSET_CALL_VA, 0x006b_22be);
    assert_eq!(CLEAR_SEEN2_CALL_VA, 0x006b_2d66);
    assert_eq!(WORLD_CLEAR_SEEN2_VA, 0x006b_2160);
    assert_eq!(CLEAR_SEEN3_MEMSET_VA, 0x006b_2d75);
    assert_eq!(WORLD_WIPE_RETURN_VA, 0x006b_2dd7);
    assert_eq!(TDataFogPlane::TData.section(), WorldSection::TDataAndFog);
    assert_eq!(
        TDataFogPlane::WCoordSeen.section(),
        WorldSection::WCoordSeen
    );
}

#[test]
fn shipped_map_edges_have_the_exact_section_six_formula() {
    for edge in [40i32, 50, 60, 70, 80, 90, 100] {
        let mut world = World::init_default_rules(edge, edge);
        let receipt = execute_tdata_and_fog_wipe(&mut world).unwrap();
        assert_eq!(receipt.section_bytes_written, (44 * edge * edge) as usize);
        assert_eq!(receipt.layout.tdata.len(), (32 * edge * edge) as usize);
        assert_eq!(receipt.layout.seen.len(), (4 * edge * edge) as usize);
        assert_eq!(receipt.layout.seen2.len(), (4 * edge * edge) as usize);
        assert_eq!(receipt.layout.seen3.len(), (4 * edge * edge) as usize);
        assert_eq!(receipt.wcoord_seen_bytes_written, (edge * edge) as usize);
        assert_eq!(receipt.total_bytes_written, (45 * edge * edge) as usize);
    }
}
