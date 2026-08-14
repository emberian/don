use std::path::{Path, PathBuf};

use don_replay::replay::Replay;
use don_replay::setup_2024_frame379::{
    bind_captured_frame379_completed_init, bind_captured_frame379_setup_entry,
    discover_frame379_setup, frame379_detailed_init_receipt_sha256,
    frame379_mountain_height_receipt_sha256, frame379_setup_snapshot_sha256,
    produce_frame379_setup, publish_frame379_setup_sim, Frame379CompletedInitBindError,
    Frame379CompletedInitCapture, Frame379CompletedInitSource, Frame379LeaderSetupAuthority,
    Frame379LeaderSetupSource, Frame379SetupEntryBindError, Frame379SetupEntryCapture,
    Frame379SetupEntrySource, Frame379SetupError, Frame379WorldgenAuthority,
    Frame379WorldgenSource, PLACE_ALL_RANDOM_STATE_BEFORE, REPLAY_FILE_SHA256,
};
use don_replay::setup_2024_starting_market::{
    golden_starting_market_setup_entry_digest, GoldenStartingMarketSetupEntryAuthority,
    GOLDEN_STARTING_MARKET_SETUP_ENTRY_SCHEMA_VERSION,
};
use don_replay::setup_units_producer::{
    StartingUnitBonuses, StartingUnitPhase, StartingUnitTypeFacts, TypeResolutionFacts,
    BASE_PEASANT_TYPE, BASE_SCOUT_TYPE, DUTCH_MERCHANT_TYPE,
};
use don_replay::{
    build_spawn_runtime::{spawn_canonical_build, CanonicalBuildSpawnRequest},
    place_all_boundary::{
        ReplayPlaceAllReceipt, TERRAIN_GROUPS_PLACE_ALL_RETURN_VA, TERRAIN_GROUPS_PLACE_ALL_VA,
    },
    setup_cities_builds::CITY_CENTER_TYPE,
    setup_place_unit_deep_re::{
        produce_place_unit_probe_prefix, CenterBuildFacts, PlaceUnitExternalResidual,
        PlaceUnitInputs, PlacementMapSnapshot, PlacementTileFacts,
    },
    terrain_height_runtime::{
        TerrainHeightAuthority, TerrainHeightSource, TerrainMountainHeightReceipt,
        TERRAIN_ADJUST_FOR_MOUNTAINS_VA, TERRAIN_FILL_MOUNTAIN_DATA_VA,
    },
};
use don_sim::systems::groups_guys::{GuyData, UnitGuys};
use don_sim::systems::map_terrain::{
    land, Coord, SectionDigest, WCoord, WorldChecksum, WorldSection,
};
use don_sim::systems::mountain_template_producer::{
    MOUNTAIN_RANGE_INIT_SHA256, MOUNTAIN_RANGE_INIT_VA,
};
use don_sim::systems::objects_init_unit_authority_frontier::{
    normalize_unit_init_coordinate, BhsInitUnitRequest, CaptainFacts, CompleteBody,
    DetailedInitUnitReceipt, FindFreeDisposition, FindFreeUnitReceipt, InitUnitStep,
    ResolveCaptainReceipt, TrackUnitTypeFacts, TrainingWhere, UnitAfterInit, UnitBandStorageClass,
    UnitInitReceipt, UnitTypeAuthorityFacts,
};
use don_sim::systems::production;
use don_sim::systems::tech_cities::CityRecord;
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn installed_witness() -> Option<Replay> {
    let path = repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
    if !path.exists() {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return None;
    }
    Some(Replay::open(&path).expect("installed 2024 witness must decode"))
}

fn type_facts(base: i32, squad_size: i32, crew_size: i32) -> TypeResolutionFacts {
    TypeResolutionFacts {
        base,
        tribe_can_base: true,
        nation_variant: base,
        build_units_upgrade: base,
        place_unit_upgrade: base,
        uber_size: 1,
        squad_size,
        crew_size,
    }
}

fn leader() -> Frame379LeaderSetupAuthority {
    Frame379LeaderSetupAuthority {
        revision: 1,
        composition_digest: [0xa5; 32],
        source: Frame379LeaderSetupSource::CanonicalNewGameLeaderTypeState,
        replay_file_sha256: REPLAY_FILE_SHA256,
        owner: 0,
        tribe: 22,
        bonuses: StartingUnitBonuses {
            dutch_merchants: true,
            ..StartingUnitBonuses::default()
        },
        types: StartingUnitTypeFacts {
            scout: type_facts(BASE_SCOUT_TYPE, 1, 1),
            dutch_merchant: type_facts(DUTCH_MERCHANT_TYPE, 1, 2),
            citizen: type_facts(BASE_PEASANT_TYPE, 1, 0),
        },
    }
}

fn placement_map(sim: &Sim) -> PlacementMapSnapshot {
    let world = &sim.map.world;
    let mut tiles = Vec::with_capacity((world.xs * world.ys) as usize);
    for y in 0..world.ys {
        for x in 0..world.xs {
            let cell = &world.wdata[(y * world.xs + x) as usize];
            tiles.push(PlacementTileFacts {
                continent: cell.region as u16,
                flags: cell.flags,
                land: cell.land,
                occupied_o: cell.down,
                collision: world.tmask(x * 4 + 2, y * 4 + 2),
            });
        }
    }
    PlacementMapSnapshot {
        xs: world.xs,
        ys: world.ys,
        tiles,
    }
}

fn captured_first_scout(
    replay: &Replay,
) -> (
    Sim,
    Sim,
    DetailedInitUnitReceipt,
    Frame379CompletedInitCapture,
) {
    let facts = discover_frame379_setup(replay, &leader()).unwrap();
    let edge = u16::try_from(replay.initial.info.settings.map_edge_world_cells().unwrap()).unwrap();
    let mut before = Sim::new(u64::from(replay.initial.info.seed), edge);
    for cell in &mut before.map.world.wdata {
        cell.land = land::FERTILE;
        cell.region = 1;
        cell.down = -1;
        cell.flags = 0;
    }
    before.map.world.tdata.fill(0);
    let mut build = production::BuildData {
        flags: production::flag::VALID,
        gather_down: -1,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..production::BuildData::default()
    };
    build.queue.queued = 0;
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build.other[0x3e] = u8::MAX;
    let center = spawn_canonical_build(
        &mut before,
        CanonicalBuildSpawnRequest {
            owner: 0,
            type_index: CITY_CENTER_TYPE,
            snapped_x: facts.center_position.0,
            snapped_y: facts.center_position.1,
            build,
        },
    )
    .unwrap();
    assert_eq!(i32::from(center.object_id), facts.center_build_o);
    let prior = before
        .spawn_unit(1, BASE_PEASANT_TYPE, 1_000, 1_000, 1)
        .unwrap();
    before
        .install_unit_guys(
            prior,
            UnitGuys {
                guys: vec![Some(GuyData {
                    ty: BASE_PEASANT_TYPE,
                    x: 1_000,
                    y: 1_000,
                    who: 1,
                    o: 0,
                    guy_num: 0,
                    ..GuyData::default()
                })],
                size: 1,
                increment: 1,
                flags: 0,
                guy_mark: 1,
            },
        )
        .unwrap();
    before.world.random.reseed(0x12345);

    let call = facts.plan.calls[0];
    let placement = produce_place_unit_probe_prefix(
        PlaceUnitInputs {
            owner: call.owner,
            upgraded_type: call.place_unit_upgrade,
            requested_x: call.requested_x,
            requested_y: call.requested_y,
            center: Some(CenterBuildFacts {
                owner: 0,
                o: facts.center_build_o,
                x: facts.center_position.0,
                y: facts.center_position.1,
            }),
            starting_town: 1,
            leader_active: 0,
        },
        &placement_map(&before),
        before.world.random.state(),
    )
    .unwrap();
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) = placement.first_external_residual
    else {
        panic!("first Scout must reach Objects::init_unit");
    };
    let bytes = don_sim::systems::save_load::save_sim(&before).unwrap();
    let mut after = don_sim::systems::save_load::load_sim(&bytes).unwrap();
    let handle = after
        .spawn_unit(
            request.owner as usize,
            request.type_index,
            request.x,
            request.y,
            call.squad_size as i8,
        )
        .unwrap();
    let row = after.world.row_of(handle).unwrap();
    let x = normalize_unit_init_coordinate(request.x);
    let y = normalize_unit_init_coordinate(request.y);
    after.world.set_pos(row, x, y);
    let guys = (0..call.squad_size + call.crew_size)
        .map(|slot| {
            Some(GuyData {
                ty: request.type_index,
                x,
                y,
                who: request.owner as i8,
                o: 0,
                guy_num: slot as i8,
                ..GuyData::default()
            })
        })
        .collect::<Vec<_>>();
    after
        .install_unit_guys(
            handle,
            UnitGuys {
                size: guys.len() as i32,
                increment: 1,
                flags: 0,
                guy_mark: call.squad_size as i8,
                guys,
            },
        )
        .unwrap();
    after.world.random.reseed(placement.rng_after_probes);
    let after_image = UnitAfterInit {
        owner: request.owner,
        o: 0,
        type_index: request.type_index,
        x,
        y,
        angle: after.world.units.angle()[row],
        unit_masks: after.world.units.get_unit_masks(row),
    };
    let detailed = DetailedInitUnitReceipt {
        request: BhsInitUnitRequest {
            owner: request.owner,
            type_index: request.type_index,
            x: request.x,
            y: request.y,
            exact_o: request.exact_o,
            external_previous: request.external_previous,
            external_next: request.external_next,
        },
        type_facts: UnitTypeAuthorityFacts {
            uber_size: 1,
            control_cost: 0,
            is_fighter_bomber: false,
            is_government_hero: false,
            track: TrackUnitTypeFacts {
                has_attack: false,
                training_where: TrainingWhere::Other,
                domain: 0,
                is_peasant: false,
                is_scholar: false,
                role_has_scout_bit: true,
                is_type_0x42: false,
                is_type_0x4b: false,
                member_former_type_is_0x34: false,
            },
        },
        steps: vec![
            InitUnitStep::FindFree(FindFreeUnitReceipt {
                ordinal: 0,
                owner: request.owner,
                start: 0,
                limit: 2_000,
                exact_o: -1,
                cursor_before: 0,
                cursor_after: 1,
                returned: 0,
                disposition: FindFreeDisposition::ConstructedAndRegistered {
                    class: UnitBandStorageClass::Unit,
                    object_list_registered: true,
                    unit_projection_registered: true,
                },
            }),
            InitUnitStep::UnitInit(UnitInitReceipt {
                ordinal: 0,
                owner: request.owner,
                type_index: request.type_index,
                o: 0,
                x: request.x,
                y: request.y,
                returned: 0,
                extent: CompleteBody::UnitInit3732Bytes,
                after: after_image,
            }),
            InitUnitStep::SetPrevious {
                ordinal: 0,
                member_o: 0,
                previous_o: -1,
            },
            InitUnitStep::ResolveCaptain(ResolveCaptainReceipt {
                ordinal: 1,
                from_owner: request.owner,
                from_o: 0,
                returned: 0,
                captain: CaptainFacts {
                    owner: request.owner,
                    o: 0,
                    x,
                    y,
                    angle: after_image.angle,
                    new_block_radius: facts.scout.new_block_radius,
                },
            }),
        ],
        returned: 0,
    };
    let capture = Frame379CompletedInitCapture {
        revision: 1,
        source: Frame379CompletedInitSource::CompleteRetailObjectsInitUnitReceiver,
        replay_file_sha256: REPLAY_FILE_SHA256,
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        setup_ordinal: 0,
        before_sim_sha256: frame379_setup_snapshot_sha256(&before).unwrap(),
        after_sim_sha256: frame379_setup_snapshot_sha256(&after).unwrap(),
        detailed_receipt_sha256: frame379_detailed_init_receipt_sha256(&detailed),
    };
    (before, after, detailed, capture)
}

fn captured_setup_entry(
    replay: &Replay,
) -> (
    Sim,
    ReplayPlaceAllReceipt,
    TerrainHeightAuthority,
    TerrainMountainHeightReceipt,
    Frame379SetupEntryCapture,
) {
    let facts = discover_frame379_setup(replay, &leader()).unwrap();
    let edge = u16::try_from(replay.initial.info.settings.map_edge_world_cells().unwrap()).unwrap();
    let mut entry = Sim::new(u64::from(replay.initial.info.seed), edge);
    entry.map.world.seed = replay.initial.info.seed as i32;
    for cell in &mut entry.map.world.wdata {
        cell.land = land::FERTILE;
        cell.region = 1;
        cell.down = -1;
        cell.down_who = -1;
        cell.flags = 0;
    }
    entry.map.world.tdata.fill(0);
    let mut build = production::BuildData {
        flags: production::flag::VALID
            | production::flag::STARTED
            | production::flag::ACTIVE
            | 0x20,
        city: -1,
        gather_down: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..production::BuildData::default()
    };
    // The zero height plane below returns logical Z zero; SubObject stores it XOR-obfuscated.
    build.other[production::off::Z_INTERNAL..production::off::Z_INTERNAL + 4]
        .copy_from_slice(&0x0006_3637_i32.to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build.other[0x3e] = u8::MAX;
    let center = spawn_canonical_build(
        &mut entry,
        CanonicalBuildSpawnRequest {
            owner: 0,
            type_index: CITY_CENTER_TYPE,
            snapped_x: facts.center_position.0,
            snapped_y: facts.center_position.1,
            build,
        },
    )
    .unwrap();
    entry.builds[center.row].city = 0;
    let mut market_build = production::BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        city: -1,
        gather_down: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..production::BuildData::default()
    };
    market_build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    market_build.other[0x3e] = u8::MAX;
    let market = spawn_canonical_build(
        &mut entry,
        CanonicalBuildSpawnRequest {
            owner: 0,
            type_index: don_replay::setup_2024_frame379::DUTCH_STARTING_MARKET_TYPE,
            snapped_x: facts.center_position.0 + 0x300,
            snapped_y: facts.center_position.1,
            build: market_build,
        },
    )
    .unwrap();
    assert_eq!(i32::from(market.object_id), 2_001);
    entry.builds[center.row].city_down = market.object_id;
    entry.builds[market.row].city = 0;
    let wx = WCoord::from_coord(Coord(facts.center_position.0)).0;
    let wy = WCoord::from_coord(Coord(facts.center_position.1)).0;
    let wrow = (wy * entry.map.world.xs + wx) as usize;
    entry.map.world.wdata[wrow].down = center.object_id;
    entry.map.world.wdata[wrow].down_who = 0;
    entry.cities.slots[0][0] = CityRecord {
        city_flags: 0x4011,
        city: 0,
        o: center.object_id,
        reg: 1,
        x: facts.center_position.0,
        y: facts.center_position.1,
        who: 0,
        ..CityRecord::default()
    };
    entry.cities.city_mark[0] = 1;
    entry.world.random.reseed(0x1234_5678);
    let post_place_all_random_state = entry.world.random.state();
    // The captured BuildUnits entry is later than place_all. Model one of Market's one-to-four
    // successful fine probes so this contract test cannot regress to equating the two boundaries.
    let _ = entry.world.random.get(0, 0xffff);

    let post_place_all_checksum = entry.map.world.checksum_sections();
    // Village/Market setup may mutate World after place_all. Keep the later entry checksum
    // observably independent just as the RNG state is independent below.
    entry.map.world.wdata[0].val ^= 1;
    let place_all = ReplayPlaceAllReceipt {
        entry_va: TERRAIN_GROUPS_PLACE_ALL_VA,
        return_va: TERRAIN_GROUPS_PLACE_ALL_RETURN_VA,
        return_value: 1,
        map_style: 14,
        generated_starts: replay.initial.active_players().count(),
        tdata_cells: entry.map.world.tdata.len(),
        random_state_before: PLACE_ALL_RANDOM_STATE_BEFORE,
        random_state_after: post_place_all_random_state,
        host_events: Vec::new(),
        checksum_before: post_place_all_checksum.clone(),
        checksum_after: post_place_all_checksum,
        sourced_walked_bytes: 0,
    };
    let vertices = (entry.map.world.tile_xs as usize + 1) * (entry.map.world.tile_ys as usize + 1);
    let terrain = TerrainHeightAuthority {
        master_land_height_bits: vec![0; vertices],
        land_height_bits: 0,
        source: TerrainHeightSource::CompletedWorldgen,
        source_digest: [0x33; 32],
    };
    let mountain_height = TerrainMountainHeightReceipt {
        adjust_for_mountains_va: TERRAIN_ADJUST_FOR_MOUNTAINS_VA,
        fill_mountain_data_va: TERRAIN_FILL_MOUNTAIN_DATA_VA,
        mountain_range_init_va: MOUNTAIN_RANGE_INIT_VA,
        mountain_range_init_sha256: MOUNTAIN_RANGE_INIT_SHA256,
        pre_mountain_digest: [0x11; 32],
        catalog_digest: [0x22; 32],
        placement_digest: [0x44; 32],
        final_plane_digest: terrain.source_digest,
        placements: 0,
        source_vertices: 0,
        matched_vertices: 0,
        unmatched_vertices: 0,
        load_rebuild_mode: false,
        final_query_authority: true,
    };
    let capture = Frame379SetupEntryCapture {
        revision: 1,
        source: Frame379SetupEntrySource::CompleteRetailBuildUnitsEntry,
        replay_file_sha256: REPLAY_FILE_SHA256,
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        entry_sim_sha256: frame379_setup_snapshot_sha256(&entry).unwrap(),
        post_place_all_world_checksum: place_all.checksum_after.clone(),
        post_place_all_random_state: place_all.random_state_after,
        entry_random_state: entry.world.random.state(),
        terrain_source_digest: terrain.source_digest,
        mountain_height_receipt_sha256: frame379_mountain_height_receipt_sha256(&mountain_height),
    };
    (entry, place_all, terrain, mountain_height, capture)
}

#[test]
fn source_backed_witness_has_exact_seven_call_schedule_and_guy_shape() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let facts = discover_frame379_setup(&replay, &leader()).unwrap();
    assert_eq!(facts.center_position, (74_592, 10_080));
    assert_eq!(facts.start_tile, (97, 13));
    assert_eq!(facts.center_build_o, 2_000);
    assert_eq!(
        facts
            .plan
            .calls
            .iter()
            .map(|call| call.place_unit_upgrade)
            .collect::<Vec<_>>(),
        [69, 62, 62, 50, 50, 50, 50]
    );
    assert_eq!(
        facts
            .plan
            .calls
            .iter()
            .map(|call| call.squad_size + call.crew_size)
            .collect::<Vec<_>>(),
        [2, 3, 3, 1, 1, 1, 1]
    );
    assert!(matches!(
        facts.plan.calls[0].phase,
        StartingUnitPhase::BaseScout
    ));
    assert!(matches!(
        facts.plan.calls[1].phase,
        StartingUnitPhase::DutchMerchant { index: 0 }
    ));
    assert!(matches!(
        facts.plan.calls[2].phase,
        StartingUnitPhase::DutchMerchant { index: 1 }
    ));
    assert_eq!(
        facts.plan.calls[3..]
            .iter()
            .map(|call| call.phase)
            .collect::<Vec<_>>(),
        (0..4)
            .map(|index| StartingUnitPhase::Citizen { index })
            .collect::<Vec<_>>()
    );
}

#[test]
fn captured_setup_entry_derives_worldgen_authority_from_map_height_and_village() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let (entry, place_all, terrain, mountain_height, capture) = captured_setup_entry(&replay);
    let bound = bind_captured_frame379_setup_entry(
        &replay,
        &entry,
        &place_all,
        &terrain,
        &mountain_height,
        &capture,
    )
    .unwrap();
    assert_ne!(bound.worldgen.composition_digest, [0; 32]);
    assert_eq!(
        bound.worldgen.world_checksum,
        entry.map.world.checksum_sections()
    );
    assert_ne!(
        bound.worldgen.world_checksum,
        bound.post_place_all_world_checksum
    );
    assert_eq!(bound.worldgen.random_state, entry.world.random.state());
    assert_eq!(bound.entry_random_state, entry.world.random.state());
    assert_ne!(bound.entry_random_state, bound.post_place_all_random_state);
    assert_eq!(bound.center_build_o, 2_000);
    assert_eq!(bound.center_city_slot, 0);
    assert_eq!(bound.center_region, 1);
    assert_eq!(bound.market_build_o, 2_001);
    assert_eq!(bound.market_city_slot, 0);
    assert_eq!(bound.terrain_query.returned_z, 0);
}

#[test]
fn captured_setup_entry_rejects_cross_wired_map_height_and_receiver_entry() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let (entry, place_all, terrain, mountain_height, capture) = captured_setup_entry(&replay);

    let mut wrong = capture.clone();
    wrong.post_place_all_world_checksum.full ^= 1;
    assert_eq!(
        bind_captured_frame379_setup_entry(
            &replay,
            &entry,
            &place_all,
            &terrain,
            &mountain_height,
            &wrong,
        ),
        Err(Frame379SetupEntryBindError::PlaceAllChecksumMismatch)
    );

    let mut wrong = capture.clone();
    wrong.entry_random_state ^= 1;
    assert_eq!(
        bind_captured_frame379_setup_entry(
            &replay,
            &entry,
            &place_all,
            &terrain,
            &mountain_height,
            &wrong,
        ),
        Err(Frame379SetupEntryBindError::EntryRandomStateMismatch)
    );

    let mut wrong = capture.clone();
    wrong.terrain_source_digest[0] ^= 1;
    assert_eq!(
        bind_captured_frame379_setup_entry(
            &replay,
            &entry,
            &place_all,
            &terrain,
            &mountain_height,
            &wrong,
        ),
        Err(Frame379SetupEntryBindError::TerrainMountainReceiptMismatch)
    );

    let mut wrong = capture;
    wrong.entry_sim_sha256[0] ^= 1;
    assert_eq!(
        bind_captured_frame379_setup_entry(
            &replay,
            &entry,
            &place_all,
            &terrain,
            &mountain_height,
            &wrong,
        ),
        Err(Frame379SetupEntryBindError::EntrySnapshotMismatch)
    );
}

#[test]
fn dynamic_leader_selectors_fail_closed() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let mut dynamic = leader();
    dynamic.types.dutch_merchant.place_unit_upgrade = 63;
    assert!(matches!(
        discover_frame379_setup(&replay, &dynamic),
        Err(Frame379SetupError::LeaderAuthorityMismatch)
            | Err(Frame379SetupError::WrongResolvedType)
            | Err(Frame379SetupError::TypeFacts(_))
    ));

    let mut dynamic = leader();
    dynamic.bonuses.extra_scouts = true;
    assert_eq!(
        discover_frame379_setup(&replay, &dynamic),
        Err(Frame379SetupError::LeaderAuthorityMismatch)
    );

    let mut dynamic = leader();
    dynamic.composition_digest = [0; 32];
    assert_eq!(
        discover_frame379_setup(&replay, &dynamic),
        Err(Frame379SetupError::MissingLeaderCompositionDigest)
    );
}

#[test]
fn chronology_refuses_to_substitute_an_empty_receipt_set_for_real_units() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let worldgen = Frame379WorldgenAuthority {
        revision: 1,
        composition_digest: [0x5a; 32],
        source: Frame379WorldgenSource::CompletedGreatLakesWorldgenAndStartingVillage,
        replay_file_sha256: REPLAY_FILE_SHA256,
        world_checksum: WorldChecksum {
            per_section: [SectionDigest::default(); WorldSection::COUNT],
            full: 0,
            bytes: 0,
        },
        random_state: 0,
    };
    assert_eq!(
        produce_frame379_setup(&replay, &[], &[], &worldgen, &leader()),
        Err(Frame379SetupError::WrongReceiptCount)
    );

    let mut missing = worldgen;
    missing.composition_digest = [0; 32];
    assert_eq!(
        produce_frame379_setup(&replay, &[], &[], &missing, &leader()),
        Err(Frame379SetupError::MissingCompositionDigest)
    );
}

#[test]
fn strict_market_lifecycle_authority_is_digest_and_entry_bound() {
    let entry = Sim::new(0x7171, 64);
    let worldgen = Frame379WorldgenAuthority {
        revision: 1,
        composition_digest: [0x5a; 32],
        source: Frame379WorldgenSource::CompletedGreatLakesWorldgenAndStartingVillage,
        replay_file_sha256: REPLAY_FILE_SHA256,
        world_checksum: entry.map.world.checksum_sections(),
        random_state: entry.world.random.state(),
    };
    let mut authority = GoldenStartingMarketSetupEntryAuthority {
        revision: 1,
        composition_digest: [0; 32],
        schema_version: GOLDEN_STARTING_MARKET_SETUP_ENTRY_SCHEMA_VERSION,
        replay_file_sha256: REPLAY_FILE_SHA256,
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        before_sim_sha256: [0x31; 32],
        entry_sim_sha256: frame379_setup_snapshot_sha256(&entry).unwrap(),
        native_trace_sha256: [0x32; 32],
        footprint_receipt_sha256: [0x33; 32],
        center_build_row: 0,
        market_build_row: 1,
        market_build_o: 2_001,
        market_city_slot: 0,
        space_grade: 4,
        random_state_after: entry.world.random.state(),
        source_produced_city_bytes: 114,
    };
    authority.composition_digest = golden_starting_market_setup_entry_digest(&authority);
    don_replay::setup_2024_frame379::validate_golden_starting_market_setup_entry_authority(
        &worldgen, &entry, &authority,
    )
    .unwrap();

    authority.entry_sim_sha256[0] ^= 1;
    assert_eq!(
        don_replay::setup_2024_frame379::validate_golden_starting_market_setup_entry_authority(
            &worldgen, &entry, &authority,
        ),
        Err(Frame379SetupError::StartingMarketAuthorityMismatch)
    );
}

#[test]
fn atomic_publication_preserves_existing_owner_on_refusal() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let edge = u16::try_from(replay.initial.info.settings.map_edge_world_cells().unwrap()).unwrap();
    let candidate = Sim::new(u64::from(replay.initial.info.seed), edge);
    let mut published = Some(Sim::new(0x5151, edge));
    published.as_mut().unwrap().world.frame = 123;
    let worldgen = Frame379WorldgenAuthority {
        revision: 1,
        composition_digest: [0x5a; 32],
        source: Frame379WorldgenSource::CompletedGreatLakesWorldgenAndStartingVillage,
        replay_file_sha256: REPLAY_FILE_SHA256,
        world_checksum: candidate.map.world.checksum_sections(),
        random_state: candidate.world.random.state(),
    };
    let (error, returned) = publish_frame379_setup_sim(
        &replay,
        &[],
        candidate,
        &[],
        &worldgen,
        &leader(),
        &mut published,
    )
    .unwrap_err();
    assert_eq!(error, Frame379SetupError::WrongReceiptCount);
    assert_eq!(returned.world.frame, 0);
    assert_eq!(published.as_ref().unwrap().world.frame, 123);
}

#[test]
fn captured_receiver_binds_complete_scout_and_derives_authority_digest() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let (before, after, detailed, capture) = captured_first_scout(&replay);
    let bound = bind_captured_frame379_completed_init(
        &replay,
        &leader(),
        0,
        &before,
        &after,
        &detailed,
        &capture,
    )
    .unwrap();
    assert_ne!(bound.composition_digest, [0; 32]);
    assert_eq!(bound.setup_ordinal, 0);
    assert_eq!(bound.projected.members.len(), 1);
    assert_eq!(bound.projected.members[0].identity.o, 0);
    assert_eq!(bound.projected.members[0].ptype_index, BASE_SCOUT_TYPE);
    assert_eq!(bound.projected.members[0].guy_identities.len(), 2);
    assert_eq!(bound.rng_before, bound.rng_after);
}

#[test]
fn captured_receiver_rejects_cross_wired_source_images_and_provenance() {
    let Some(replay) = installed_witness() else {
        return;
    };
    let (before, after, detailed, capture) = captured_first_scout(&replay);

    let mut wrong = capture.clone();
    wrong.after_sim_sha256[0] ^= 1;
    assert_eq!(
        bind_captured_frame379_completed_init(
            &replay,
            &leader(),
            0,
            &before,
            &after,
            &detailed,
            &wrong,
        ),
        Err(Frame379CompletedInitBindError::AfterSnapshotMismatch)
    );

    let mut wrong = capture.clone();
    wrong.detailed_receipt_sha256[0] ^= 1;
    assert_eq!(
        bind_captured_frame379_completed_init(
            &replay,
            &leader(),
            0,
            &before,
            &after,
            &detailed,
            &wrong,
        ),
        Err(Frame379CompletedInitBindError::DetailedReceiptMismatch)
    );

    let mut wrong = capture;
    wrong.executable_sha256[0] ^= 1;
    assert_eq!(
        bind_captured_frame379_completed_init(
            &replay,
            &leader(),
            0,
            &before,
            &after,
            &detailed,
            &wrong,
        ),
        Err(Frame379CompletedInitBindError::UnsupportedExecutable)
    );

    let bytes = don_sim::systems::save_load::save_sim(&after).unwrap();
    let mut changed_prior = don_sim::systems::save_load::load_sim(&bytes).unwrap();
    let prior_row = changed_prior.world.unit_row_at(1, 0).unwrap();
    changed_prior.world.units.avoid_x_mut()[prior_row] ^= 1;
    let rewired = Frame379CompletedInitCapture {
        revision: 1,
        source: Frame379CompletedInitSource::CompleteRetailObjectsInitUnitReceiver,
        replay_file_sha256: REPLAY_FILE_SHA256,
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        setup_ordinal: 0,
        before_sim_sha256: frame379_setup_snapshot_sha256(&before).unwrap(),
        after_sim_sha256: frame379_setup_snapshot_sha256(&changed_prior).unwrap(),
        detailed_receipt_sha256: frame379_detailed_init_receipt_sha256(&detailed),
    };
    assert_eq!(
        bind_captured_frame379_completed_init(
            &replay,
            &leader(),
            0,
            &before,
            &changed_prior,
            &detailed,
            &rewired,
        ),
        Err(Frame379CompletedInitBindError::PriorUnitChanged { row: 0 })
    );
}
