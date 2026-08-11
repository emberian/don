use don_replay::world_owner_frontier;
use don_sim::checksum::adler32;
use don_sim::systems::map_terrain::{wflag, WCoord, World, WorldSection};
use world_owner_frontier::{
    sha256, ExactPortTransitionProof, InitialWorldPrefixEvidence, ReplaySpan,
    RetailDifferenceLocation, RetailWorldCheckpoint, RetailWorldWalkCapture, RulesWorldEvidence,
    WorldByteSource, WorldOwnerError, WorldOwnerLedger, WorldSectionMask, RETAIL_AFTER_CONSTANTS,
    SHIPPED_RULES_CHANNEL, SHIPPED_RULES_SERIALIZED_BYTES,
};

#[test]
fn dependency_free_sha256_matches_the_standard_vector() {
    assert_eq!(
        sha256(b"abc"),
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ]
    );
}

fn digest(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn world(edge: i32, seed: u32) -> World {
    let mut world = World::init_default_rules(edge, edge);
    assert_eq!(world.seed_map_generation(seed as i32), Some(seed as i32));
    world
}

fn evidence(map_size: u8, seed: u32) -> InitialWorldPrefixEvidence {
    InitialWorldPrefixEvidence {
        replay_sha256: digest(0x11),
        map_size,
        map_size_span: ReplaySpan::new(0x25, 1),
        seed,
        seed_span: ReplaySpan::new(0x08, 4),
        rules: Some(RulesWorldEvidence {
            serialized_span: ReplaySpan::new(0x360, SHIPPED_RULES_SERIALIZED_BYTES),
            serialized_sha256: digest(0x22),
            checksum: SHIPPED_RULES_CHANNEL,
            after_constants: RETAIL_AFTER_CONSTANTS,
            player_base: 44,
            player_civic: 4,
            player_city: 4,
        }),
    }
}

fn transition(
    input_checksum: u32,
    output_checksum: u32,
    allowed_sections: WorldSectionMask,
) -> ExactPortTransitionProof {
    ExactPortTransitionProof {
        entry_va: 0x0068_e8f0,
        resume_va: 0x0068_c052,
        implementation_sha256: digest(0x33),
        receipt_sha256: digest(0x44),
        proof_document: "docs/assembly/replay-check-player-forest.md",
        input_checksum,
        output_checksum,
        allowed_sections,
    }
}

#[test]
fn prefix_owns_the_exact_76_bytes_and_names_every_source() {
    let world = world(70, 0x0b3f_69d3);
    let ledger = WorldOwnerLedger::from_initial_prefix(&world, evidence(3, 0x0b3f_69d3))
        .expect("admit exact replay prefix");
    let coverage = ledger.coverage();
    assert_eq!(coverage.owned_bytes, 76);
    assert_eq!(coverage.unknown_bytes, coverage.walked_bytes - 76);
    assert_eq!(coverage.walked_bytes, 382_368);

    assert!(matches!(
        ledger.owner_at(WorldSection::Dims, 0),
        Some(WorldByteSource::DerivedMapSize {
            selector: 3,
            edge: 70,
            ..
        })
    ));
    assert!(matches!(
        ledger.owner_at(WorldSection::Scalars, 48),
        Some(WorldByteSource::ReplayRulesProjection { .. })
    ));
    assert!(matches!(
        ledger.owner_at(WorldSection::Scalars, 116),
        Some(WorldByteSource::ReplayScalar {
            field: "GameInfo::seed -> World::seed",
            ..
        })
    ));
    assert_eq!(ledger.owner_at(WorldSection::Scalars, 40), None);
    assert_eq!(ledger.owner_at(WorldSection::WData, 0), None);
}

#[test]
fn wrong_selector_rules_or_replay_spans_fail_before_ownership_exists() {
    let world = world(70, 17);
    let mut wrong_selector = evidence(2, 17);
    assert!(matches!(
        WorldOwnerLedger::from_initial_prefix(&world, wrong_selector),
        Err(WorldOwnerError::ClaimValueMismatch {
            section: WorldSection::Dims,
            ..
        })
    ));

    wrong_selector = evidence(3, 17);
    wrong_selector.seed_span.bytes = 1;
    assert!(matches!(
        WorldOwnerLedger::from_initial_prefix(&world, wrong_selector),
        Err(WorldOwnerError::InvalidReplaySpan { field: "seed", .. })
    ));

    let mut wrong_rules = evidence(3, 17);
    wrong_rules.rules.as_mut().unwrap().after_constants ^= 1;
    assert!(matches!(
        WorldOwnerLedger::from_initial_prefix(&world, wrong_rules),
        Err(WorldOwnerError::RulesCheckpointMismatch { .. })
    ));

    let mut invented_rules_value = evidence(3, 17);
    invented_rules_value.rules.as_mut().unwrap().player_base = 45;
    assert!(matches!(
        WorldOwnerLedger::from_initial_prefix(&world, invented_rules_value),
        Err(WorldOwnerError::RulesTerritoryMismatch { .. })
    ));
}

#[test]
fn exact_port_transition_owns_only_changed_bytes_and_preserves_unknown_zeroes() {
    let mut before = world(70, 17);
    let mut ledger = WorldOwnerLedger::from_initial_prefix(&before, evidence(3, 17)).unwrap();
    let input = before.checksum_sections().full;
    before.wdata_mut(8, 9).flags |= wflag::FOREST;
    let output = before.checksum_sections().full;
    let receipt = ledger
        .advance_exact_port(
            &before,
            transition(input, output, WorldSectionMask::only(WorldSection::WData)),
        )
        .unwrap();

    assert!(receipt.changed_bytes > 0);
    assert!(receipt
        .changed_ranges
        .iter()
        .all(|range| range.section == WorldSection::WData));
    let changed = receipt.changed_ranges[0];
    assert!(matches!(
        ledger.owner_at(changed.section, changed.offset),
        Some(WorldByteSource::ExactPortTransition {
            entry_va: 0x0068_e8f0,
            ..
        })
    ));
    assert_eq!(
        ledger.owner_at(WorldSection::WData, changed.offset.saturating_sub(1)),
        None,
        "an unchanged neighbouring byte must remain unknown"
    );
    assert_eq!(ledger.coverage().owned_bytes, 76 + receipt.changed_bytes);
}

#[test]
fn transition_rejects_stale_checksums_and_out_of_scope_sections_transactionally() {
    let mut after = world(70, 17);
    let mut ledger = WorldOwnerLedger::from_initial_prefix(&after, evidence(3, 17)).unwrap();
    let baseline = ledger.clone();
    let input = after.checksum_sections().full;
    after.tdata[0] = 9;
    let output = after.checksum_sections().full;

    assert!(matches!(
        ledger.advance_exact_port(
            &after,
            transition(input, output, WorldSectionMask::only(WorldSection::WData),),
        ),
        Err(WorldOwnerError::ForbiddenSectionMutation {
            section: WorldSection::TDataAndFog,
            ..
        })
    ));
    assert_eq!(ledger, baseline);

    let stale = transition(
        input ^ 1,
        output,
        WorldSectionMask::only(WorldSection::TDataAndFog),
    );
    assert!(matches!(
        ledger.advance_exact_port(&after, stale),
        Err(WorldOwnerError::TransitionInputMismatch { .. })
    ));
    assert_eq!(ledger, baseline);
}

#[test]
fn exact_port_transition_rebases_an_allowed_unowned_section_growth() {
    let mut after = world(70, 17);
    let mut ledger = WorldOwnerLedger::from_initial_prefix(&after, evidence(3, 17)).unwrap();
    let input = after.checksum_sections().full;
    let before_walked = ledger.coverage().walked_bytes;
    after.add_starting_location(WCoord(8), WCoord(9));
    let output = after.checksum_sections().full;

    let receipt = ledger
        .advance_exact_port(
            &after,
            transition(
                input,
                output,
                WorldSectionMask::only(WorldSection::StartArrays),
            ),
        )
        .unwrap();

    assert!(receipt.changed_bytes > 0);
    assert!(receipt
        .changed_ranges
        .iter()
        .all(|range| range.section == WorldSection::StartArrays));
    assert!(ledger.coverage().walked_bytes > before_walked);
    assert_eq!(ledger.snapshot().checksum, after.checksum_sections());
    assert_eq!(ledger.coverage().owned_bytes, 76 + receipt.changed_bytes);
}

#[test]
fn section_growth_rejects_forbidden_or_previously_owned_shapes_transactionally() {
    let mut after = world(70, 17);
    let mut ledger = WorldOwnerLedger::from_initial_prefix(&after, evidence(3, 17)).unwrap();
    let input = after.checksum_sections().full;
    after.add_starting_location(WCoord(8), WCoord(9));
    let output = after.checksum_sections().full;
    let baseline = ledger.clone();
    assert!(matches!(
        ledger.advance_exact_port(
            &after,
            transition(input, output, WorldSectionMask::only(WorldSection::WData)),
        ),
        Err(WorldOwnerError::ForbiddenSectionShapeMutation {
            section: WorldSection::StartArrays,
            ..
        })
    ));
    assert_eq!(ledger, baseline);

    ledger
        .advance_exact_port(
            &after,
            transition(
                input,
                output,
                WorldSectionMask::only(WorldSection::StartArrays),
            ),
        )
        .unwrap();
    let owned_shape = ledger.clone();
    let second_input = after.checksum_sections().full;
    after.add_starting_location(WCoord(10), WCoord(11));
    let second_output = after.checksum_sections().full;
    assert!(matches!(
        ledger.advance_exact_port(
            &after,
            transition(
                second_input,
                second_output,
                WorldSectionMask::only(WorldSection::StartArrays),
            ),
        ),
        Err(WorldOwnerError::OwnedSectionShapeMutation {
            section: WorldSection::StartArrays,
        })
    ));
    assert_eq!(ledger, owned_shape);
}

#[test]
fn checksum_only_evidence_never_claims_a_first_byte_but_a_bound_walk_does() {
    let world = world(60, 0x0015_3b65);
    let ledger =
        WorldOwnerLedger::from_initial_prefix(&world, evidence(2, 0x0015_3b65)).expect("prefix");
    let mut retail_image = ledger.snapshot().image.clone();
    let section = ledger.snapshot().section(WorldSection::WData);
    let global = section.start + 7;
    retail_image[global] ^= 0x80;
    let expected = adler32(1, &retail_image);
    let checkpoint = RetailWorldCheckpoint {
        replay_sha256: digest(0x55),
        turn: 2,
        peer_checksums: vec![expected, expected],
    };

    let checksum_only = ledger.compare_checkpoint(&checkpoint).unwrap();
    assert!(!checksum_only.matches);
    assert_eq!(checksum_only.first_difference, None);
    assert_eq!(ledger.coverage().owned_bytes, 76);

    let capture = RetailWorldWalkCapture {
        checkpoint,
        executable_sha256: digest(0x66),
        capture_sha256: digest(0x77),
        image: retail_image,
    };
    let compared = ledger.compare_retail_walk(&capture).unwrap();
    assert_eq!(
        compared.first_difference.unwrap().location,
        RetailDifferenceLocation::Section {
            section: WorldSection::WData,
            offset: 7,
        }
    );
    assert_eq!(ledger.coverage().owned_bytes, 76);
}

/// Frozen from `schema/replay-validation.json` (61 files, 21 checksummed).  This table is
/// intentionally checksum evidence, not state input: every row disagrees on the first
/// checksummed turn, and none of these values is accepted as a byte owner above.
#[test]
fn sixty_one_file_corpus_fixes_the_first_substantive_world_mismatch() {
    const ROWS: [(&str, u32, u32, u64, u64, usize); 21] = [
        (
            "2018.11.17",
            0xd63a_3a53,
            0x1389_cbbb,
            780_168,
            780_092,
            18_069,
        ),
        (
            "2018.12.01",
            0xfabe_984e,
            0x46e3_cc37,
            780_168,
            780_092,
            9_511,
        ),
        (
            "2019.03.24",
            0xf173_a4e9,
            0xa47c_cba0,
            780_168,
            780_092,
            12_728,
        ),
        (
            "2020.02.08",
            0x849d_1967,
            0xabbc_40fa,
            382_368,
            382_292,
            9_789,
        ),
        (
            "2020.02.21",
            0x9c7d_03de,
            0x4758_b5f1,
            280_968,
            280_892,
            9_097,
        ),
        (
            "2020.07.25a",
            0xfd01_741b,
            0xe1bc_41d1,
            382_368,
            382_292,
            171,
        ),
        (
            "2020.07.25b",
            0x0ea5_ec23,
            0x61ad_41ce,
            382_368,
            382_292,
            1_245,
        ),
        (
            "2020.07.25c",
            0x9df4_f7b7,
            0xab06_40f4,
            382_368,
            382_292,
            8_014,
        ),
        (
            "2024.02.23a",
            0x102e_2a1e,
            0xda1c_cc46,
            780_168,
            780_092,
            1_111,
        ),
        (
            "2024.02.23b",
            0x33c8_d45c,
            0x8df5_cbf5,
            780_168,
            780_092,
            25_442,
        ),
        (
            "2024.02.24",
            0xe81f_af2a,
            0x744f_cb8d,
            780_168,
            780_092,
            16_845,
        ),
        (
            "2024.03.10a",
            0x9f8f_9f77,
            0xb01e_cb95,
            780_168,
            780_092,
            78,
        ),
        (
            "2024.03.10b",
            0x9ae9_8649,
            0x7816_cc35,
            780_168,
            780_092,
            16_719,
        ),
        (
            "2024.03.17",
            0x2ea7_e1ad,
            0x4351_cba4,
            780_168,
            780_092,
            11_331,
        ),
        (
            "2024.03.18",
            0xe69c_aa99,
            0x791b_ca9b,
            780_168,
            780_092,
            17_583,
        ),
        (
            "2024.03.20",
            0xa372_c4f0,
            0x542a_1e3d,
            499_368,
            499_292,
            12_006,
        ),
        (
            "2024.03.23",
            0xd665_9d06,
            0x5df2_4180,
            382_368,
            382_292,
            14_846,
        ),
        (
            "2024.03.29a",
            0x2dac_a6f5,
            0xdcb7_b62b,
            280_968,
            280_892,
            37,
        ),
        (
            "2024.03.29b",
            0x1dc1_c8aa,
            0xee3e_cb9d,
            780_168,
            780_092,
            11_952,
        ),
        (
            "2024.04.10",
            0x91c0_52d8,
            0x11c1_cb67,
            780_168,
            780_092,
            15_042,
        ),
        (
            "2025.02.10",
            0x4f65_1430,
            0x08cd_cbdb,
            780_168,
            780_092,
            11_322,
        ),
    ];
    assert_eq!(ROWS.len(), 21);
    for (name, retail, model, walked, unknown, _) in ROWS {
        assert_ne!(
            retail, model,
            "{name}: turn 2 must remain an honest mismatch"
        );
        assert_eq!(walked - unknown, 76, "{name}: exact prefix ownership");
    }
    assert_eq!(
        ROWS.iter().map(|row| row.5).sum::<usize>(),
        222_938,
        "channel-12 corpus comparisons"
    );
}
