//! Atomic replay/BHS proof for the production research and census bridge.

use std::path::{Path, PathBuf};

use don_bhs::{ScriptTimers, VmError};
use don_replay::builds_runtime::BuildsWalkAuthority;
use don_replay::leader_produce_building_farm_frame_zero_tail::{
    FarmBuildInitAuthority, FarmFrameZeroSourceFacts,
};
use don_replay::replay::Replay;
use don_replay::replay_bhs_live_bindings::{
    bind_production_map_style, bind_production_type_counts, ProductionBuiltinImage,
    ProductionBuiltinValue, ProductionCityImage, ProductionLeaderImage, ProductionRunFailure,
    ProductionSetupImage, ReplayProductionCall,
};
use don_replay::replay_bhs_research_runtime::{
    run_production_research_call, FarmBuiltin520Authority,
    LEADER_PRODUCE_BUILDING_FRAME_PAYMENT_GATE_VA, LEADER_PRODUCE_BUILDING_PAY_COST_CALL_VA,
    TYPE_PAY_COST_VA,
};
use don_replay::replay_bhs_runtime::{
    load_replay_bhs_program, ReplayBhsBinding, LEADER_FLAG_HUMAN,
};
use don_replay::terrain_height_runtime::{TerrainHeightAuthority, TerrainHeightSource};
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::script_runtime::{ExternalGameSeconds, ScriptRuntime};
use don_sim::systems::bhs_create_unit_runtime::{
    BhsCreateUnitRuntime, CreateUnitLeaderProjection, CreateUnitProjectionWitness,
    CreateUnitRuntimeInput,
};
use don_sim::systems::bhs_place_building_runtime::{
    apply_sim_place_building_with_cost_prefix, PlaceBuildingCostAuthority, PlaceBuildingCostEntry,
    PlaceBuildingCostSource, PlaceBuildingRequest, PlaceBuildingStatus, LEADER_PRODUCE_BUILDING_VA,
};
use don_sim::systems::bhs_type_factory::{
    produce_type_builtin_state, ComposedTypeRow, RulesCompositionId, Sha256Digest,
    TypeBuiltinFactoryInput, TypeSourceRole, TypeSourceWitness, WitnessedTypeSource,
};
use don_sim::systems::bhs_type_table::{
    LeaderTypeMasks, TypeBuiltinState, TypeRow, NUM_LEADERS, NUM_TRIBES, NUM_TYPES,
};
use don_sim::systems::canonical_gather_work::Farms;
use don_sim::systems::gather_terrain::{
    GatherTerrainMaterialization, GatherTerrainSourceStamp, GatherTerrainWorldIdentity,
    SUPPORTED_RULES_XML_SHA256,
};
use don_sim::systems::leader_produce_building_blocked_site_prefix::{
    apply_sim_build_type_blocked_tcoord_land_prefix,
    apply_sim_leader_produce_building_blocked_site_farm_owned_tail,
    apply_sim_leader_produce_building_blocked_site_farm_unowned_tail,
    apply_sim_leader_produce_building_blocked_site_land_footprint,
    apply_sim_leader_produce_building_blocked_site_prefix,
    apply_sim_leader_produce_building_farm_success_preflight, BuildTypeBlockedTcoordPrefixError,
    BuildTypeBlockedTcoordPrefixStatus, LeaderProduceBuildingBlockedSiteFarmUnownedError,
    LeaderProduceBuildingBlockedSitePrefixError, BUILD_TYPE_BLOCKED_LOCATION_END_VA,
    BUILD_TYPE_BLOCKED_LOCATION_NON_FRIENDLY_CALL_VA, BUILD_TYPE_BLOCKED_LOCATION_VA,
    BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_BYTES_REMAINING,
    BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA,
    BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA, BUILD_TYPE_BLOCKED_SITE_BYTES_REMAINING,
    BUILD_TYPE_BLOCKED_SITE_PREFIX_BYTES, BUILD_TYPE_BLOCKED_TCOORD_BYTES_REMAINING,
    BUILD_TYPE_BLOCKED_TCOORD_GET_AMOUNT_CALL_VA, BUILD_TYPE_BLOCKED_TCOORD_GET_GOOD_CALL_VA,
    BUILD_TYPE_BLOCKED_TCOORD_GET_LAND_CALL_VA, BUILD_TYPE_BLOCKED_TCOORD_LAND_PREFIX_BYTES,
    BUILD_TYPE_BLOCKED_TCOORD_VA, BUILD_TYPE_GET_GOOD_VA, BUILD_TYPE_NON_FRIENDLY_TERRITORY_VA,
    GAME_SEMAPHORE_IMMEDIATE_BIT, LAND_DATA_GET_AMOUNT_VA, WORLD_DATA_GET_LAND_TCOORD_VA,
};
use don_sim::systems::leader_produce_building_candidate_prefix::{
    apply_sim_leader_produce_building_candidate_prefix, CandidatePrefixRejection,
    LeaderProduceBuildingCandidatePrefixError, LeaderProduceBuildingCandidatePrefixStatus,
    BUILD_TYPE_BLOCKED_SITE_VA, LEADER_PRODUCE_BUILDING_BLOCKED_SITE_BYTES_REMAINING,
    LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA, LEADER_PRODUCE_BUILDING_CANDIDATE_PREFIX_BYTES,
    WDATA_BUILD_CANDIDATE_EXCLUDED,
};
use don_sim::systems::leader_produce_building_prefix::{
    apply_sim_leader_produce_building_prefix, LeaderProduceBuildingPrefixRequest,
    LeaderProduceBuildingPrefixStatus, BUILD_FLAG_NO_ACTIVE_CITY_REQUIRED,
    LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA,
};
use don_sim::systems::leader_produce_building_search_setup::{
    apply_sim_leader_produce_building_search_setup, LeaderProduceBuildingSearchSetupError,
    LeaderProduceBuildingSearchSetupStatus, LEADER_PRODUCE_BUILDING_CANDIDATE_BYTES_REMAINING,
    LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA, LEADER_PRODUCE_BUILDING_SEARCH_SETUP_BYTES,
};
use don_sim::systems::map_terrain::{land, tflag};
use don_sim::systems::player_setup::ManualPlayerSetup;
use don_sim::systems::production::runtime::{
    LiveBuildVisibilityTypeFacts, LiveProductionRuntime, LiveProductionType,
    SingleLibraryResearchStatus,
};
use don_sim::systems::production::{flag, off, BuildData, BuildQueue, BuildQueueEntry, Footprint};
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::Sim;

const OWNER: usize = 0;
const LIBRARY: i32 = 435;
const WRITTEN_WORD: i32 = 551;
const CITY_STATE: i32 = 565;
const COST: [i32; 6] = [0, 12, 5, 0, 0, 0];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn digest(byte: u8) -> Sha256Digest {
    Sha256Digest([byte; 32])
}

fn witness(role: TypeSourceRole, component: u8) -> TypeSourceWitness {
    TypeSourceWitness {
        role,
        composition: RulesCompositionId(digest(1)),
        manifest_sha256: digest(2),
        component_sha256: digest(component),
    }
}

fn canonical_type_owners(owner: usize) -> (TypeBuiltinState, BhsCreateUnitRuntime) {
    let rows = (0..NUM_TYPES)
        .map(|slot| {
            let mut row = TypeRow::empty(slot);
            row.name = match slot {
                0 => "Food".into(),
                2 => "Wealth".into(),
                50 => "Citizen".into(),
                51 => "Citizens".into(),
                52 => "Upgraded Citizen".into(),
                53 => "Grafted Citizen".into(),
                417 => "Farm".into(),
                418 => "Woodcutter's Camp".into(),
                419 => "Mine".into(),
                420 => "University".into(),
                427 => "Barracks".into(),
                432 => "Dock".into(),
                435 => "Library".into(),
                436 => "Market".into(),
                439 => "Tower".into(),
                551 => "Written Word".into(),
                565 => "City State".into(),
                572 => "The Art of War".into(),
                _ => format!("Internal {slot}"),
            };
            row.type_name = format!("Family {slot}");
            if slot == WRITTEN_WORD as usize {
                row.common.costs = COST;
                row.common.job_time = 200;
                row.common.preq = [-1; 3];
                row.where_type = LIBRARY;
            }
            if slot == CITY_STATE as usize {
                row.common.costs = [12, 0, 0, 0, 0, 0];
                row.common.job_time = 200;
                row.common.preq = [-1; 3];
                row.where_type = LIBRARY;
            }
            if slot == 417 {
                // Installed Farm COST=4t; buildingrules.xml costs are scaled by ten.
                row.common.costs = [0, 40, 0, 0, 0, 0];
            }
            Some(ComposedTypeRow {
                index: row.index,
                name: row.name,
                type_name: row.type_name,
                common: row.common,
                from: row.from,
                where_type: row.where_type,
                modified: row.modified,
                grid_x: row.grid_x,
                grid_y: row.grid_y,
                is_non_strict: Some(row.is_list),
                body: row.body,
            })
        })
        .collect();
    let produced = produce_type_builtin_state(TypeBuiltinFactoryInput {
        types: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TypeRows, 3),
            value: rows,
        },
        tribes: WitnessedTypeSource {
            witness: witness(TypeSourceRole::TribeRoster, 4),
            value: (0..NUM_TRIBES)
                .map(|slot| Some(format!("Tribe {slot}")))
                .collect(),
        },
        leaders: WitnessedTypeSource {
            witness: witness(TypeSourceRole::LeaderMasks, 5),
            value: (0..NUM_LEADERS)
                .map(|slot| {
                    Some(LeaderTypeMasks {
                        leader_flags: if slot == owner { 3 } else { 0 },
                        ..LeaderTypeMasks::default()
                    })
                })
                .collect(),
        },
    })
    .unwrap();
    let (state, provenance) = produced.into_parts();
    let upgrades = BhsCreateUnitRuntime::new(
        CreateUnitRuntimeInput {
            witness: CreateUnitProjectionWitness {
                composition: RulesCompositionId(digest(1)),
                manifest_sha256: digest(2),
                component_sha256: digest(6),
            },
            types: vec![None; NUM_TYPES],
            leaders: (0..NUM_LEADERS)
                .map(|leader_slot| {
                    Some(CreateUnitLeaderProjection {
                        leader_slot,
                        current_upgrade: (0..NUM_TYPES)
                            .map(|slot| {
                                Some(if leader_slot == owner && slot == 50 {
                                    52
                                } else {
                                    slot as i32
                                })
                            })
                            .collect(),
                        graft: (0..NUM_TYPES)
                            .map(|slot| {
                                Some(if leader_slot == owner && slot == 52 {
                                    53
                                } else {
                                    slot as i32
                                })
                            })
                            .collect(),
                    })
                })
                .collect(),
            numeric_groups: Vec::new(),
        },
        &state,
        provenance,
    )
    .unwrap();
    (state, upgrades)
}

fn production_owners(owner: usize) -> (Sim, LiveProductionRuntime, usize) {
    let mut sim = Sim::new(0x357, 8);
    // Coherent frame-zero Athens placement terrain: dry W cells and the City tile mask
    // consumed by `WorldData::check_building_wcoord(..., need_city=1)`.
    for cell in &mut sim.map.world.wdata {
        cell.land = land::FERTILE;
    }
    for mask in &mut sim.map.world.tdata {
        *mask |= tflag::CITY;
    }
    let mut build = BuildData {
        flags: flag::VALID | flag::ACTIVE,
        who: owner as u8,
        city: 0,
        city_down: -1,
        gather_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        queue: BuildQueue {
            queued: 0,
            entries: vec![BuildQueueEntry::default(); 2],
        },
        ..BuildData::default()
    };
    build.other[off::OBJECT_ID..off::OBJECT_ID + 2]
        .copy_from_slice(&(BUILD_BAND_BASE as i16).to_le_bytes());
    // The native search setup decodes the Build's Coord into the canonical WData plane.
    let center: i32 = 3 * 768 + 384;
    build.other[off::X_INTERNAL..off::X_INTERNAL + 4]
        .copy_from_slice(&(center ^ 0x63637).to_le_bytes());
    build.other[off::Y_INTERNAL..off::Y_INTERNAL + 4]
        .copy_from_slice(&(center ^ 0x63637).to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    let row = sim.spawn_build(owner, build);
    sim.cities.city_mark[owner] = 1;
    let city = &mut sim.cities.slots[owner][0];
    city.city_flags = 1;
    city.city = 0;
    city.o = BUILD_BAND_BASE as i16;
    city.reg = 64;
    city.x = center;
    city.y = center;
    city.who = owner as i8;
    city.name = "Athens".into();
    city.id = "capital_0".into();

    let mut production = std::mem::take(&mut sim.production_runtime);
    production.register_build(row, LIBRARY);
    let mut library = LiveProductionType::in_place_building(LIBRARY, 1);
    library.is_library = true;
    production.install_type(library);
    let mut written_word = LiveProductionType::research(WRITTEN_WORD, 200);
    written_word.repeat_cost = Some(COST);
    production.install_type(written_word);
    let mut city_state = LiveProductionType::research(CITY_STATE, 200);
    city_state.repeat_cost = Some([12, 0, 0, 0, 0, 0]);
    production.install_type(city_state);
    let mut farm = LiveProductionType::in_place_building(417, 150);
    // Installed FARM BuildTypeData::build_flags, schema/live/live-tables-building.tsv.
    farm.build_flags = 0x1000_0049;
    farm.build_visibility = Some(LiveBuildVisibilityTypeFacts {
        // Installed FARM ObjectTypeData::domain, schema/live/live-tables-building.tsv.
        domain: Some(0),
        footprint: Some(Footprint {
            x_size: 4,
            y_size: 4,
        }),
        is_fort: None,
    });
    production.install_type(farm);
    production.leaders[owner].resources = [100; 6];
    sim.leaders[owner].econ.stockpile = [100; 6];
    sim.step8.leaders[owner].econ.stockpile = [100; 6];
    sim.vic_leaders.slots[owner].leader_flags = 3;
    sim.vic_leaders.slots[owner].economy.bucket = [100; 6];
    sim.step8.leaders[owner].flags = 3;
    sim.vic_leaders.slots[owner].num_units[0] = 3;
    sim.vic_leaders.slots[owner].num_units[53 - 50] = 7;
    sim.vic_leaders.slots[owner].num_buildings[427 - 414] = 4;
    sim.vic_leaders.slots[owner].num_queued[53] = 5;
    sim.vic_leaders.slots[owner].num_queued[427] = 6;
    sim.vic_leaders.slots[owner].num_queued[0] = 9;
    sim.vic_leaders.slots[owner].num_queued[402] = 11;
    (sim, production, row)
}

fn place_building_costs(owner: usize) -> PlaceBuildingCostAuthority {
    PlaceBuildingCostAuthority {
        revision: 7,
        composition_digest: [0x52; 32],
        entries: [-1, 0]
            .into_iter()
            .map(|city_constraint| PlaceBuildingCostEntry {
                owner: owner as u8,
                type_index: 417,
                origin_build_object: BUILD_BAND_BASE as i16,
                city_constraint,
                source: PlaceBuildingCostSource::TypeDataCanPayCostPeAfterImage,
                possible_goods: [true; 6],
                resolved_costs: [0, 40, 0, 0, 0, 0],
            })
            .collect(),
    }
}

fn game_seconds(seconds: i32) -> ExternalGameSeconds {
    ExternalGameSeconds::admit(Some(seconds), Some(seconds)).unwrap()
}

fn installed_gather_terrain(sim: &Sim) -> GatherTerrainMaterialization {
    let rules_path = repo_root().join("ron-data/rules.xml");
    let rules_xml = std::fs::read(&rules_path)
        .unwrap_or_else(|error| panic!("read installed {}: {error}", rules_path.display()));
    GatherTerrainMaterialization::from_supported_sources(
        &rules_xml,
        GatherTerrainSourceStamp {
            installed_rules_sha256: SUPPORTED_RULES_XML_SHA256,
            world_seed: sim.map.world.seed,
            coherent_generation: true,
        },
        GatherTerrainWorldIdentity::from_world(&sim.map.world),
        Vec::new(),
        Vec::new(),
    )
    .expect("materialize exact installed LandData rows for the replay world")
}

#[test]
fn place_building_prefix_is_receipt_bearing_read_only_and_save_stable() {
    let (types, _) = canonical_type_owners(OWNER);
    let (mut sim, production, row) = production_owners(OWNER);
    let authority = place_building_costs(OWNER);
    let request = PlaceBuildingRequest {
        who: 1,
        type_name: "fArM".into(),
        city_name: "aThEnS".into(),
    };
    let builds_before: Vec<_> = sim.builds.iter().map(BuildData::image).collect();
    let cities_before = sim.cities.clone();
    let groups_before = sim.groups.clone();
    let resources_before = production.leaders[OWNER].resources;

    let receipt = apply_sim_place_building_with_cost_prefix(
        &sim,
        &production,
        &types,
        &authority,
        request.clone(),
    )
    .unwrap();
    assert_eq!(
        receipt.status,
        PlaceBuildingStatus::ReadyForLeaderProduceBuilding
    );
    assert_eq!(receipt.origin_build_row, Some(row));
    assert_eq!(receipt.city_constraint, Some(-1));
    assert_eq!(receipt.resolved_costs, Some([0, 40, 0, 0, 0, 0]));
    assert_eq!(receipt.can_pay_result, Some(2));
    assert_eq!(receipt.returned, None);
    assert_eq!(receipt.continuation.unwrap().va, LEADER_PRODUCE_BUILDING_VA);
    assert_eq!(
        sim.builds.iter().map(BuildData::image).collect::<Vec<_>>(),
        builds_before
    );
    assert_eq!(sim.cities.slots, cities_before.slots);
    assert_eq!(sim.cities.city_mark, cities_before.city_mark);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(production.leaders[OWNER].resources, resources_before);

    let rejected_produce = apply_sim_leader_produce_building_prefix(
        &sim,
        &production,
        LeaderProduceBuildingPrefixRequest {
            owner: OWNER as u8,
            type_index: 417,
            origin_build_object: BUILD_BAND_BASE as i16,
            mode: 0,
        },
    )
    .unwrap();
    assert_eq!(
        rejected_produce.status,
        LeaderProduceBuildingPrefixStatus::RejectedBeforePlacementSearch
    );
    assert_eq!(rejected_produce.native_returned, Some(1));
    assert_eq!(rejected_produce.scenario_returned, Some(0));

    let mut standalone_production = production.clone();
    standalone_production.types[417]
        .as_mut()
        .unwrap()
        .build_flags |= BUILD_FLAG_NO_ACTIVE_CITY_REQUIRED;
    let standalone_produce = apply_sim_leader_produce_building_prefix(
        &sim,
        &standalone_production,
        LeaderProduceBuildingPrefixRequest {
            owner: OWNER as u8,
            type_index: 417,
            origin_build_object: BUILD_BAND_BASE as i16,
            mode: 0,
        },
    )
    .unwrap();
    assert_eq!(
        standalone_produce.status,
        LeaderProduceBuildingPrefixStatus::ReadyForPlacementSearch
    );
    assert!(!standalone_produce.active_city_origin);
    assert!(standalone_produce.used_target_fallback);
    assert!(standalone_produce.target_allows_without_active_city);

    let (mut city_sim, city_production, city_row) = production_owners(OWNER);
    city_sim.builds[city_row].flags |= flag::CAPTURED;
    let admitted_produce = apply_sim_leader_produce_building_prefix(
        &city_sim,
        &city_production,
        LeaderProduceBuildingPrefixRequest {
            owner: OWNER as u8,
            type_index: 417,
            origin_build_object: BUILD_BAND_BASE as i16,
            mode: 0,
        },
    )
    .unwrap();
    assert_eq!(
        admitted_produce.status,
        LeaderProduceBuildingPrefixStatus::ReadyForPlacementSearch
    );
    assert!(admitted_produce.active_city_origin);
    assert!(!admitted_produce.used_target_fallback);
    assert_eq!(
        admitted_produce.continuation.unwrap().va,
        LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA
    );
    let search_input = admitted_produce.continuation.unwrap();
    let search_build_before = city_sim.builds[city_row].image();
    let search_groups_before = city_sim.groups.clone();
    let search_setup = apply_sim_leader_produce_building_search_setup(
        &city_sim,
        &city_production,
        &types,
        search_input,
    )
    .unwrap();
    assert_eq!(LEADER_PRODUCE_BUILDING_SEARCH_SETUP_BYTES, 0x5d4);
    assert_eq!(
        search_setup.status,
        LeaderProduceBuildingSearchSetupStatus::ReadyForCandidateLoop
    );
    assert_eq!(search_setup.origin_type, LIBRARY);
    assert_eq!(search_setup.origin_world_cell, [3, 3]);
    assert_eq!(search_setup.origin_region, 64);
    assert_eq!(search_setup.leader_radius, 20);
    assert_eq!(search_setup.circle_radius_index, 5);
    assert_eq!(search_setup.initial_circle_offset, 1);
    assert_eq!(
        search_setup.target_footprint,
        Footprint {
            x_size: 4,
            y_size: 4
        }
    );
    assert_eq!(search_setup.footprint_search, [1, 1, 2]);
    assert!(!search_setup.target_is_dock);
    assert!(
        !search_setup.resource_sensitive_search,
        "Farm forces retail's build_flags & 0x40 search local to zero"
    );
    let candidate = search_setup.continuation.unwrap();
    assert_eq!(candidate.va, LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA);
    assert_eq!(
        candidate.bytes_remaining,
        LEADER_PRODUCE_BUILDING_CANDIDATE_BYTES_REMAINING
    );
    assert_eq!(city_sim.builds[city_row].image(), search_build_before);
    assert_eq!(city_sim.groups.list, search_groups_before.list);
    assert_eq!(city_production.leaders[OWNER].resources, resources_before);

    let candidate_prefix = apply_sim_leader_produce_building_candidate_prefix(
        &city_sim,
        &city_production,
        &types,
        candidate,
    )
    .unwrap();
    assert_eq!(LEADER_PRODUCE_BUILDING_CANDIDATE_PREFIX_BYTES, 0x25c);
    assert_eq!(
        candidate_prefix.status,
        LeaderProduceBuildingCandidatePrefixStatus::ReadyForBlockedSite
    );
    assert!(candidate_prefix.probes.is_empty());
    assert_eq!(candidate_prefix.target_domain, 0);
    let blocked_site = candidate_prefix.continuation.unwrap();
    assert_eq!(
        blocked_site.va,
        LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA
    );
    assert_eq!(blocked_site.callee_va, BUILD_TYPE_BLOCKED_SITE_VA);
    assert_eq!(
        blocked_site.bytes_remaining,
        LEADER_PRODUCE_BUILDING_BLOCKED_SITE_BYTES_REMAINING
    );
    assert_eq!(blocked_site.circle_offset, 1);
    assert_eq!(blocked_site.candidate_world_cell, [2, 2]);
    assert_eq!(blocked_site.space_grade, 4);
    assert_eq!(blocked_site.placement_coord, [2 * 768 + 384, 2 * 768 + 384]);
    assert_eq!(blocked_site.city_constraint, -1);
    assert_eq!(blocked_site.blocked_detail_initial, 0);
    assert_eq!(city_sim.builds[city_row].image(), search_build_before);
    assert_eq!(city_sim.groups.list, search_groups_before.list);
    assert_eq!(city_production.leaders[OWNER].resources, resources_before);

    let blocked_site_prefix = apply_sim_leader_produce_building_blocked_site_prefix(
        &city_production,
        &types,
        blocked_site,
    )
    .unwrap();
    assert_eq!(BUILD_TYPE_BLOCKED_SITE_PREFIX_BYTES, 0x17c);
    assert!(!blocked_site_prefix.target_is_city);
    assert_eq!(blocked_site_prefix.placement_tcoord, [10, 10]);
    assert_eq!(blocked_site_prefix.footprint_corner, [8, 8]);
    assert_eq!(blocked_site_prefix.native_returned, None);
    let blocked_tcoord = blocked_site_prefix.continuation;
    assert_eq!(
        blocked_tcoord.va,
        BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA
    );
    assert_eq!(blocked_tcoord.callee_va, BUILD_TYPE_BLOCKED_TCOORD_VA);
    assert_eq!(
        blocked_tcoord.blocked_site_bytes_remaining,
        BUILD_TYPE_BLOCKED_SITE_BYTES_REMAINING
    );
    assert_eq!(blocked_tcoord.tile, [8, 8]);
    assert_eq!(blocked_tcoord.owner, OWNER as u8);
    assert_eq!(blocked_tcoord.city_constraint, -1);
    assert_eq!(blocked_tcoord.blocked_detail_initial, 0);
    assert_eq!(city_sim.builds[city_row].image(), search_build_before);
    assert_eq!(city_sim.groups.list, search_groups_before.list);
    assert_eq!(city_production.leaders[OWNER].resources, resources_before);

    let blocked_tcoord_prefix = apply_sim_build_type_blocked_tcoord_land_prefix(
        &city_sim,
        &city_production,
        &types,
        blocked_tcoord,
    )
    .unwrap();
    assert_eq!(BUILD_TYPE_BLOCKED_TCOORD_LAND_PREFIX_BYTES, 0x795);
    assert_eq!(
        blocked_tcoord_prefix.status,
        BuildTypeBlockedTcoordPrefixStatus::ReadyForLandDataGetAmount
    );
    assert_eq!(blocked_tcoord_prefix.was_seen, Some(false));
    assert_eq!(blocked_tcoord_prefix.world_region, Some(64));
    assert_eq!(blocked_tcoord_prefix.terrain_mask, Some(tflag::CITY));
    assert_eq!(blocked_tcoord_prefix.raw_returned_to_blocked_site, None);
    assert_eq!(BUILD_TYPE_BLOCKED_TCOORD_GET_GOOD_CALL_VA, 0x0063_751c);
    assert_eq!(BUILD_TYPE_GET_GOOD_VA, 0x0063_bd50);
    assert_eq!(BUILD_TYPE_BLOCKED_TCOORD_GET_LAND_CALL_VA, 0x0063_7532);
    assert_eq!(WORLD_DATA_GET_LAND_TCOORD_VA, 0x006b_4c70);
    let get_amount = blocked_tcoord_prefix.continuation.unwrap();
    assert_eq!(get_amount.va, BUILD_TYPE_BLOCKED_TCOORD_GET_AMOUNT_CALL_VA);
    assert_eq!(get_amount.callee_va, LAND_DATA_GET_AMOUNT_VA);
    assert_eq!(
        get_amount.blocked_tcoord_bytes_remaining,
        BUILD_TYPE_BLOCKED_TCOORD_BYTES_REMAINING
    );
    assert_eq!(BUILD_TYPE_BLOCKED_TCOORD_BYTES_REMAINING, 0x67);
    assert_eq!(get_amount.tile, [8, 8]);
    assert!(!get_amount.was_seen);
    assert!(!get_amount.seen_or_immediate);
    assert_eq!(get_amount.build_flags, 0x1000_0049);
    assert_eq!(get_amount.good, 0);
    assert_eq!(get_amount.land_index, 0);
    assert_eq!(
        get_amount.tile_linear_index,
        8 * city_sim.map.world.tile_xs + 8
    );
    assert_eq!(city_sim.builds[city_row].image(), search_build_before);
    assert_eq!(city_sim.groups.list, search_groups_before.list);
    assert_eq!(city_production.leaders[OWNER].resources, resources_before);

    let mut occupied_sim = production_owners(OWNER).0;
    *occupied_sim.map.world.tmask_mut(8, 8) |= tflag::BLOCKER_BUILDING;
    let occupied = apply_sim_build_type_blocked_tcoord_land_prefix(
        &occupied_sim,
        &city_production,
        &types,
        blocked_tcoord,
    )
    .unwrap();
    assert_eq!(
        occupied.status,
        BuildTypeBlockedTcoordPrefixStatus::ReturnedToBlockedSite
    );
    assert_eq!(occupied.raw_returned_to_blocked_site, Some(0x24));
    assert!(occupied.continuation.is_none());

    let mut immediate = production_owners(OWNER).0;
    immediate.vic_match.semaphore |= 1 << GAME_SEMAPHORE_IMMEDIATE_BIT;
    let immediate_prefix = apply_sim_build_type_blocked_tcoord_land_prefix(
        &immediate,
        &city_production,
        &types,
        blocked_tcoord,
    )
    .unwrap();
    let immediate_amount = immediate_prefix.continuation.unwrap();
    assert!(!immediate_amount.was_seen);
    assert!(immediate_amount.seen_or_immediate);

    let mut owned_territory = production_owners(OWNER).0;
    owned_territory.map.world.wdata_mut(2, 2).who = OWNER as i8;
    let owned_prefix = apply_sim_build_type_blocked_tcoord_land_prefix(
        &owned_territory,
        &city_production,
        &types,
        blocked_tcoord,
    )
    .expect("self-owned active-City region is an exact was_seen shortcut");
    assert_eq!(owned_prefix.was_seen, Some(true));
    assert!(owned_prefix.continuation.unwrap().seen_or_immediate);
    owned_territory.map.world.wdata_mut(2, 2).who = 1;
    assert_eq!(
        apply_sim_build_type_blocked_tcoord_land_prefix(
            &owned_territory,
            &city_production,
            &types,
            blocked_tcoord,
        ),
        Err(
            BuildTypeBlockedTcoordPrefixError::UnsupportedOwnedTerritorySeenShortcut {
                territory_owner: 1,
                region: 64,
            }
        )
    );

    let mut unsupported_city = blocked_site;
    unsupported_city.city_constraint = 0;
    assert_eq!(
        apply_sim_leader_produce_building_blocked_site_prefix(
            &city_production,
            &types,
            unsupported_city,
        ),
        Err(
            LeaderProduceBuildingBlockedSitePrefixError::UnsupportedCityConstraint {
                city_constraint: 0
            }
        )
    );

    let mut missing_domain = city_production.clone();
    missing_domain.types[417]
        .as_mut()
        .unwrap()
        .build_visibility
        .as_mut()
        .unwrap()
        .domain = None;
    assert_eq!(
        apply_sim_leader_produce_building_candidate_prefix(
            &city_sim,
            &missing_domain,
            &types,
            candidate,
        ),
        Err(LeaderProduceBuildingCandidatePrefixError::MissingTargetDomain)
    );

    let (mut skipped_sim, skipped_production, skipped_row) = production_owners(OWNER);
    skipped_sim.builds[skipped_row].flags |= flag::CAPTURED;
    let first = blocked_site.candidate_world_cell;
    skipped_sim.map.world.wdata_mut(first[0], first[1]).flags |= WDATA_BUILD_CANDIDATE_EXCLUDED;
    let skipped = apply_sim_leader_produce_building_candidate_prefix(
        &skipped_sim,
        &skipped_production,
        &types,
        candidate,
    )
    .unwrap();
    assert_eq!(skipped.probes.len(), 1);
    assert_eq!(skipped.probes[0].circle_offset, 1);
    assert!(matches!(
        skipped.probes[0].rejection,
        CandidatePrefixRejection::WDataExcluded { .. }
    ));
    assert_eq!(skipped.continuation.unwrap().circle_offset, 2);

    city_sim.step8.leaders[OWNER].flags = 0;
    city_sim.vic_leaders.slots[OWNER].leader_flags = 0;
    let saved_search_sim = save_sim(&city_sim).unwrap();
    city_sim.step8.leaders[OWNER].flags = 3;
    city_sim.vic_leaders.slots[OWNER].leader_flags = 3;
    let mut resumed_search_sim = load_sim(&saved_search_sim).unwrap();
    resumed_search_sim.step8.leaders[OWNER].flags = 3;
    resumed_search_sim.vic_leaders.slots[OWNER].leader_flags = 3;
    let resumed_search = apply_sim_leader_produce_building_search_setup(
        &resumed_search_sim,
        &city_production,
        &types,
        search_input,
    )
    .unwrap();
    assert_eq!(resumed_search, search_setup);
    let resumed_candidate = apply_sim_leader_produce_building_candidate_prefix(
        &resumed_search_sim,
        &city_production,
        &types,
        resumed_search.continuation.unwrap(),
    )
    .unwrap();
    assert_eq!(resumed_candidate, candidate_prefix);
    let resumed_blocked_site = apply_sim_leader_produce_building_blocked_site_prefix(
        &city_production,
        &types,
        resumed_candidate.continuation.unwrap(),
    )
    .unwrap();
    assert_eq!(resumed_blocked_site, blocked_site_prefix);
    let resumed_blocked_tcoord = apply_sim_build_type_blocked_tcoord_land_prefix(
        &resumed_search_sim,
        &city_production,
        &types,
        resumed_blocked_site.continuation,
    )
    .unwrap();
    assert_eq!(resumed_blocked_tcoord, blocked_tcoord_prefix);

    city_sim.world.frame = 1;
    assert_eq!(
        apply_sim_leader_produce_building_search_setup(
            &city_sim,
            &city_production,
            &types,
            search_input,
        ),
        Err(LeaderProduceBuildingSearchSetupError::UnsupportedNonzeroFrame { frame: 1 })
    );
    city_sim.world.frame = 0;
    let mut missing_footprint = city_production.clone();
    missing_footprint.types[417]
        .as_mut()
        .unwrap()
        .build_visibility = None;
    assert_eq!(
        apply_sim_leader_produce_building_search_setup(
            &city_sim,
            &missing_footprint,
            &types,
            search_input,
        ),
        Err(LeaderProduceBuildingSearchSetupError::MissingTargetFootprint)
    );

    let mut inactive_city_sim = city_sim;
    inactive_city_sim.cities.slots[OWNER][0].city_flags = 0;
    let mut inactive_city_production = city_production;
    inactive_city_production.types[417]
        .as_mut()
        .unwrap()
        .build_flags |= BUILD_FLAG_NO_ACTIVE_CITY_REQUIRED;
    let inactive_city = apply_sim_leader_produce_building_prefix(
        &inactive_city_sim,
        &inactive_city_production,
        LeaderProduceBuildingPrefixRequest {
            owner: OWNER as u8,
            type_index: 417,
            origin_build_object: BUILD_BAND_BASE as i16,
            mode: 0,
        },
    )
    .unwrap();
    assert_eq!(
        inactive_city.status,
        LeaderProduceBuildingPrefixStatus::RejectedBeforePlacementSearch,
        "a selected inactive City does not fall through to the target-Type bit"
    );
    assert!(!inactive_city.used_target_fallback);

    // Activation/type/cost projections are external runtime inputs. Preserve the canonical
    // City/Build/resource owners, save with those mirrors uninstalled, then reinstall them.
    sim.step8.leaders[OWNER].flags = 0;
    sim.vic_leaders.slots[OWNER].leader_flags = 0;
    let sim_before = save_sim(&sim).unwrap();
    let mut loaded = load_sim(&sim_before).unwrap();
    loaded.step8.leaders[OWNER].flags = 3;
    loaded.vic_leaders.slots[OWNER].leader_flags = 3;
    loaded.step8.leaders[OWNER].econ.stockpile = resources_before;
    loaded.vic_leaders.slots[OWNER].economy.bucket = resources_before;
    let resumed = apply_sim_place_building_with_cost_prefix(
        &loaded,
        &production,
        &types,
        &authority,
        request,
    )
    .unwrap();
    assert_eq!(resumed, receipt);
    let resumed_produce = apply_sim_leader_produce_building_prefix(
        &loaded,
        &production,
        LeaderProduceBuildingPrefixRequest {
            owner: OWNER as u8,
            type_index: 417,
            origin_build_object: BUILD_BAND_BASE as i16,
            mode: 0,
        },
    )
    .unwrap();
    assert_eq!(resumed_produce, rejected_produce);

    let (mut poor_sim, mut poor_production, _) = production_owners(OWNER);
    poor_production.leaders[OWNER].resources = [0; 6];
    poor_sim.leaders[OWNER].econ.stockpile = [0; 6];
    poor_sim.step8.leaders[OWNER].econ.stockpile = [0; 6];
    poor_sim.vic_leaders.slots[OWNER].economy.bucket = [0; 6];
    let poor = apply_sim_place_building_with_cost_prefix(
        &poor_sim,
        &poor_production,
        &types,
        &authority,
        PlaceBuildingRequest {
            who: 1,
            type_name: "Farm".into(),
            city_name: "capital_0".into(),
        },
    )
    .unwrap();
    assert_eq!(poor.status, PlaceBuildingStatus::Unaffordable);
    assert_eq!(poor.returned, Some(0));
    assert!(poor.continuation.is_none());
}

#[test]
fn produce_building_city_gate_terminal_is_mounted_and_returns_scenario_zero() {
    let fixture = repo_root().join("crates/don-replay/tests/fixtures/bhs_research_rollback.bhs");
    let inc = don_bhs_cc::load::install_include_path(repo_root());
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let mut script_runtime = ScriptRuntime::new(loaded.program, None, None).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: "place_prefix_only".into(),
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 99,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let image = ProductionBuiltinImage {
        leaders: std::array::from_fn(|who| {
            if who == OWNER {
                ProductionLeaderImage {
                    flags: 3,
                    ..ProductionLeaderImage::default()
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        ..ProductionBuiltinImage::default()
    };
    let (types, upgrades) = canonical_type_owners(OWNER);
    let (mut sim, mut production, row) = production_owners(OWNER);
    let groups_before = sim.groups.clone();
    let build_before = sim.builds[row].image();
    let resources_before = production.leaders[OWNER].resources;

    let receipt = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &place_building_costs(OWNER),
        None,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .unwrap();

    assert_eq!(call.step, 0);
    assert_eq!(receipt.production.returned, 0);
    assert_eq!(receipt.production.trace.len(), 1);
    assert_eq!(receipt.production.trace[0].index, 520);
    assert_eq!(
        receipt.production.trace[0].returned,
        ProductionBuiltinValue::Int(0)
    );
    assert_eq!(receipt.place_buildings.len(), 1);
    assert_eq!(receipt.produce_buildings.len(), 1);
    assert_eq!(
        receipt.produce_buildings[0].status,
        LeaderProduceBuildingPrefixStatus::RejectedBeforePlacementSearch
    );
    assert_eq!(receipt.produce_buildings[0].native_returned, Some(1));
    assert_eq!(receipt.produce_buildings[0].scenario_returned, Some(0));
    assert_eq!(sim.builds[row].image(), build_before);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(production.leaders[OWNER].resources, resources_before);
}

#[test]
fn produce_building_candidate_exhaustion_is_mounted_and_returns_scenario_zero() {
    let fixture = repo_root().join("crates/don-replay/tests/fixtures/bhs_research_rollback.bhs");
    let inc = don_bhs_cc::load::install_include_path(repo_root());
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let mut script_runtime = ScriptRuntime::new(loaded.program, None, None).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: "place_prefix_only".into(),
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 99,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let image = ProductionBuiltinImage {
        leaders: std::array::from_fn(|who| {
            if who == OWNER {
                ProductionLeaderImage {
                    flags: 3,
                    ..ProductionLeaderImage::default()
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        ..ProductionBuiltinImage::default()
    };
    let (types, upgrades) = canonical_type_owners(OWNER);
    let (mut sim, mut production, row) = production_owners(OWNER);
    sim.builds[row].flags |= flag::CAPTURED;
    for cell in &mut sim.map.world.wdata {
        cell.flags |= WDATA_BUILD_CANDIDATE_EXCLUDED;
    }
    let groups_before = sim.groups.clone();
    let build_before = sim.builds[row].image();
    let resources_before = production.leaders[OWNER].resources;

    let receipt = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &place_building_costs(OWNER),
        None,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .unwrap();

    assert_eq!(call.step, 0);
    assert_eq!(receipt.production.returned, 0);
    assert_eq!(receipt.production.trace.len(), 1);
    assert_eq!(receipt.production.trace[0].index, 520);
    assert_eq!(
        receipt.production.trace[0].returned,
        ProductionBuiltinValue::Int(0)
    );
    assert_eq!(receipt.place_buildings.len(), 1);
    assert_eq!(receipt.produce_buildings.len(), 1);
    assert_eq!(receipt.produce_building_search_setups.len(), 1);
    assert_eq!(receipt.produce_building_candidate_prefixes.len(), 1);
    let candidate = &receipt.produce_building_candidate_prefixes[0];
    assert_eq!(
        candidate.status,
        LeaderProduceBuildingCandidatePrefixStatus::ExhaustedBeforeBlockedSite
    );
    assert_eq!(candidate.native_returned, Some(1));
    assert_eq!(candidate.scenario_returned, Some(0));
    assert!(candidate.continuation.is_none());
    assert!(!candidate.probes.is_empty());
    assert!(candidate.probes.iter().any(|probe| matches!(
        probe.rejection,
        CandidatePrefixRejection::WDataExcluded { .. }
    )));
    assert!(candidate.probes.iter().all(|probe| matches!(
        probe.rejection,
        CandidatePrefixRejection::OutOfBounds | CandidatePrefixRejection::WDataExcluded { .. }
    )));
    assert_eq!(sim.builds[row].image(), build_before);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(production.leaders[OWNER].resources, resources_before);
}

#[test]
fn insufficient_resources_return_zero_before_cursor_group_or_queue_commit() {
    let fixture = repo_root().join("crates/don-replay/tests/fixtures/bhs_research_rollback.bhs");
    let inc = don_bhs_cc::load::install_include_path(repo_root());
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let mut script_runtime = ScriptRuntime::new(loaded.program, None, None).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: "research_only".into(),
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 99,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let image = ProductionBuiltinImage {
        leaders: std::array::from_fn(|who| {
            if who == OWNER {
                ProductionLeaderImage {
                    flags: 3,
                    ..ProductionLeaderImage::default()
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        ..ProductionBuiltinImage::default()
    };
    let (types, upgrades) = canonical_type_owners(OWNER);
    let (mut sim, mut production, row) = production_owners(OWNER);
    production.leaders[OWNER].resources = [0; 6];
    sim.leaders[OWNER].econ.stockpile = [0; 6];
    sim.step8.leaders[OWNER].econ.stockpile = [0; 6];
    sim.vic_leaders.slots[OWNER].economy.bucket = [0; 6];
    let groups_before = sim.groups.clone();
    let queue_before = sim.builds[row].queue.clone();

    let receipt = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &PlaceBuildingCostAuthority::default(),
        None,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .unwrap();

    assert_eq!(call.step, 0);
    assert_eq!(receipt.production.returned, 0);
    assert_eq!(receipt.research.len(), 1);
    assert_eq!(
        receipt.research[0].status,
        SingleLibraryResearchStatus::Unavailable
    );
    assert_eq!(sim.scenario_data.find_counters[30], BUILD_BAND_BASE as i32);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.groups.last_group, groups_before.last_group);
    assert_eq!(sim.builds[row].queue.queued, queue_before.queued);
    assert_eq!(
        sim.builds[row].queue.entries[0].image(),
        queue_before.entries[0].image()
    );
    assert_eq!(production.leaders[OWNER].resources, [0; 6]);
    assert_eq!(production.leaders[OWNER].epochs_queued, 0);
}

#[test]
fn owned_game_info_gates_commit_the_complete_research_transaction() {
    let fixture = repo_root().join("crates/don-replay/tests/fixtures/bhs_research_rollback.bhs");
    let inc = don_bhs_cc::load::install_include_path(repo_root());
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let mut timers = ScriptTimers::default();
    timers.add_timer("1", 300).unwrap();
    let mut script_runtime =
        ScriptRuntime::new_with_timers(loaded.program, None, None, timers).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: "research_then_commit".into(),
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 99,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let image = ProductionBuiltinImage {
        setup: Some(ProductionSetupImage {
            game_info_flags: 0b100,
            game_rules: 0,
            difficulty: 0,
            rush_rules: 14,
            victory: don_bhs::scenario::victory::ECONOMIC,
            starting_town: 2,
            starting_resources: 1,
            starting_resources2: 1,
            semaphore: [0; 32],
        }),
        leaders: std::array::from_fn(|who| {
            if who == OWNER {
                ProductionLeaderImage {
                    flags: 3,
                    ..ProductionLeaderImage::default()
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        ..ProductionBuiltinImage::default()
    };
    let (types, upgrades) = canonical_type_owners(OWNER);
    let (mut sim, mut production, row) = production_owners(OWNER);
    sim.step8.leaders[OWNER].flags = 0;
    sim.vic_leaders.slots[OWNER].leader_flags = 0;
    let mut setup = ManualPlayerSetup {
        active_mask: 1 << OWNER,
        local_player_setup_slot: OWNER,
        ..ManualPlayerSetup::default()
    };
    setup.teams[OWNER] = 0;
    sim.start_manual_player_setup(setup)
        .expect("install the canonical active Leader setup owner");
    sim.vic_leaders.slots[OWNER].init_diplomacy.ally_mask = 1;
    sim.builds[row].flags |= flag::CAPTURED;
    sim.spawn_unit(OWNER, 50, 100, 200, 4)
        .expect("install one live idle Citizen captain");

    let receipt = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &PlaceBuildingCostAuthority::default(),
        None,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .expect("all reached setup gates are replay-owned");

    assert_eq!(
        receipt
            .production
            .trace
            .iter()
            .map(|call| call.index)
            .collect::<Vec<_>>(),
        [
            78, 357, 455, 386, 436, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 108,
            109,
        ]
    );
    assert_eq!(
        receipt.production.trace[5..]
            .iter()
            .map(|call| call.returned.clone())
            .collect::<Vec<_>>(),
        [
            ProductionBuiltinValue::Int(1),
            ProductionBuiltinValue::Int(14),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(1),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(1),
            ProductionBuiltinValue::Int(1),
            ProductionBuiltinValue::Int(6),
        ]
    );
    assert_eq!(receipt.research.len(), 1);
    assert_eq!(
        receipt.research[0].status,
        SingleLibraryResearchStatus::Applied
    );
    assert_eq!(receipt.idle_units[0].returned, 1);
    assert_eq!(call.step, BUILD_BAND_BASE as i32 + 25);
    assert_eq!(receipt.production.returned, BUILD_BAND_BASE as i32 + 31);
    assert!(script_runtime.script_timers().is_empty());
    assert_eq!(sim.scenario_data.find_counters[30], BUILD_BAND_BASE as i32);
    assert_eq!(sim.builds[row].queue.queued, 1);
    assert_eq!(
        production.leaders[OWNER].resources,
        [100, 88, 95, 100, 100, 100]
    );
    assert_eq!(
        sim.leaders[OWNER].econ.stockpile,
        production.leaders[OWNER].resources
    );
    assert_eq!(
        sim.step8.leaders[OWNER].econ.stockpile,
        production.leaders[OWNER].resources
    );
    assert_eq!(
        sim.vic_leaders.slots[OWNER].economy.bucket,
        production.leaders[OWNER].resources
    );
    assert_eq!(
        production.leaders[OWNER].queued_counts[WRITTEN_WORD as usize],
        1
    );
    assert_eq!(
        sim.vic_leaders.slots[OWNER].num_queued[WRITTEN_WORD as usize],
        1
    );
    assert_eq!(sim.vic_match.options.difficulty, 5);
    assert_eq!(sim.vic_leaders.slots[OWNER].multi_diff, 6);
    let saved = save_sim(&sim).expect("save committed difficulty after-image");
    let resumed = load_sim(&saved).expect("resume committed difficulty after-image");
    assert_eq!(save_sim(&resumed).unwrap(), saved);
    assert_eq!(resumed.vic_match.options.difficulty, 5);
    assert_eq!(resumed.vic_leaders.slots[OWNER].multi_diff, 6);
}

#[test]
fn difficulty_refusal_branches_return_minus_one_without_binding_or_mutating_owners() {
    let fixture = repo_root().join("crates/don-replay/tests/fixtures/bhs_research_rollback.bhs");
    let inc = don_bhs_cc::load::install_include_path(repo_root());
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let mut script_runtime = ScriptRuntime::new(loaded.program, None, None).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: "difficulty_refusals".into(),
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 99,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let image = ProductionBuiltinImage::default();
    let (types, upgrades) = canonical_type_owners(OWNER);
    let (mut sim, mut production, _) = production_owners(OWNER);
    sim.vic_match.options.difficulty = 4;
    sim.vic_leaders.slots[OWNER].multi_diff = 3;
    let match_before = sim.vic_match.options;
    let leader_difficulty_before = sim.vic_leaders.slots[OWNER].multi_diff;

    let receipt = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &PlaceBuildingCostAuthority::default(),
        None,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .expect("all retail refusal arms complete before authority binding");

    assert_eq!(call.step, -5);
    assert_eq!(receipt.production.returned, -5);
    assert_eq!(
        receipt
            .production
            .trace
            .iter()
            .map(|call| (call.index, call.returned.clone()))
            .collect::<Vec<_>>(),
        [106, 106, 108, 108, 109]
            .into_iter()
            .map(|index| (index, ProductionBuiltinValue::Int(-1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(sim.vic_match.options, match_before);
    assert_eq!(
        sim.vic_leaders.slots[OWNER].multi_diff,
        leader_difficulty_before
    );
}

#[test]
fn later_vm_failure_rolls_back_program_ref_timer_cursor_group_queue_and_all_leader_mirrors() {
    let fixture = repo_root().join("crates/don-replay/tests/fixtures/bhs_research_rollback.bhs");
    let inc = don_bhs_cc::load::install_include_path(repo_root());
    let loaded = don_bhs_cc::load::load_script_file(&inc, &fixture).unwrap();
    let pristine_program = don_replay::script_channel::checksum_program(&loaded.program).unwrap();
    let mut timers = ScriptTimers::default();
    timers.add_timer("1", 300).unwrap();
    let pristine_timers = timers.clone();
    let mut script_runtime =
        ScriptRuntime::new_with_timers(loaded.program, None, None, timers).unwrap();
    let binding = ReplayBhsBinding {
        file: 0,
        name: "research_then_fail".into(),
    };
    let mut call = ReplayProductionCall {
        who: 1,
        step: 99,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let call_before = call;
    let image = ProductionBuiltinImage {
        setup: Some(ProductionSetupImage {
            game_info_flags: 0b100,
            game_rules: 0,
            difficulty: 0,
            rush_rules: 14,
            victory: don_bhs::scenario::victory::ECONOMIC,
            starting_town: 2,
            starting_resources: 1,
            starting_resources2: 1,
            semaphore: [0; 32],
        }),
        leaders: std::array::from_fn(|who| {
            if who == OWNER {
                ProductionLeaderImage {
                    flags: 3,
                    ..ProductionLeaderImage::default()
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        ..ProductionBuiltinImage::default()
    };
    let (types, upgrades) = canonical_type_owners(OWNER);
    let (mut sim, mut production, row) = production_owners(OWNER);
    let idle_unit = sim
        .spawn_unit(OWNER, 50, 100, 200, 4)
        .expect("install one live idle Citizen captain");
    let idle_row = sim.world.row_of(idle_unit).unwrap();
    let idle_flags_before = sim.world.units.get_flags(idle_row);
    let idle_inside_before = sim.world.units.inside_up()[idle_row];
    let idle_captain_before = sim.world.units.o_up()[idle_row];
    let idle_orders_before = sim.world.orders(idle_row).clone();
    let cities_before = sim.cities.clone();
    let scenario_before = sim.scenario_data.clone();
    let groups_before = sim.groups.clone();
    let queue_before = sim.builds[row].queue.clone();
    let resources_before = production.leaders[OWNER].resources;
    let difficulty_before = sim.vic_match.options.difficulty;
    let leader_difficulty_before = sim.vic_leaders.slots[OWNER].multi_diff;

    let error = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &PlaceBuildingCostAuthority::default(),
        None,
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .unwrap_err();

    assert!(
        matches!(
            &error.failure,
            ProductionRunFailure::Vm(VmError::UnimplementedBuiltin {
                index: 107,
                name: "get_difficulty"
            })
        ),
        "unexpected failure: {:?}",
        error.failure
    );
    assert_eq!(
        error
            .trace
            .iter()
            .map(|call| call.index)
            .collect::<Vec<_>>(),
        [
            78, 357, 455, 386, 436, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 108,
            109,
        ]
    );
    assert_eq!(call, call_before);
    assert_eq!(
        don_replay::script_channel::checksum_program(script_runtime.program()).unwrap(),
        pristine_program
    );
    assert_eq!(script_runtime.script_timers(), &pristine_timers);
    assert_eq!(
        error.trace.last().map(|call| (&call.index, &call.returned)),
        Some((&109, &ProductionBuiltinValue::Int(6)))
    );
    assert_eq!(
        error.trace[5..]
            .iter()
            .map(|call| call.returned.clone())
            .collect::<Vec<_>>(),
        [
            ProductionBuiltinValue::Int(1),
            ProductionBuiltinValue::Int(14),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(1),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(0),
            ProductionBuiltinValue::Int(1),
            ProductionBuiltinValue::Int(1),
            ProductionBuiltinValue::Int(6),
        ]
    );
    assert_eq!(sim.scenario_data, scenario_before);
    assert_eq!(sim.cities.slots, cities_before.slots);
    assert_eq!(sim.cities.city_mark, cities_before.city_mark);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.groups.last_group, groups_before.last_group);
    assert_eq!(sim.groups.proc_group, groups_before.proc_group);
    assert_eq!(sim.builds[row].queue.queued, queue_before.queued);
    assert_eq!(sim.world.units.get_flags(idle_row), idle_flags_before);
    assert_eq!(sim.world.units.inside_up()[idle_row], idle_inside_before);
    assert_eq!(sim.world.units.o_up()[idle_row], idle_captain_before);
    assert_eq!(sim.world.orders(idle_row), &idle_orders_before);
    assert_eq!(sim.unit_type[idle_row], 50);
    assert_eq!(
        sim.builds[row].queue.entries[0].image(),
        queue_before.entries[0].image()
    );
    assert_eq!(production.leaders[OWNER].resources, resources_before);
    assert_eq!(
        production.leaders[OWNER].queued_counts[WRITTEN_WORD as usize],
        0
    );
    assert_eq!(production.leaders[OWNER].ages_queued, 0);
    assert_eq!(production.leaders[OWNER].epochs_queued, 0);
    assert_eq!(sim.leaders[OWNER].econ.stockpile, resources_before);
    assert_eq!(sim.step8.leaders[OWNER].econ.stockpile, resources_before);
    assert_eq!(
        sim.vic_leaders.slots[OWNER].economy.bucket,
        resources_before
    );
    assert_eq!(
        sim.vic_leaders.slots[OWNER].num_queued[WRITTEN_WORD as usize],
        0
    );
    assert_eq!(sim.vic_match.options.difficulty, difficulty_before);
    assert_eq!(
        sim.vic_leaders.slots[OWNER].multi_diff,
        leader_difficulty_before
    );
}

#[test]
fn shipped_economic_program_reaches_the_canonical_type_queue_then_the_next_missing_call() {
    let content_root = repo_root().join("ron-data/bhs-corpus");
    if !content_root.is_dir() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. Installed ron-data/bhs-corpus is absent; the shipped trace was not established.\n"
        );
        return;
    }
    let replay_path =
        repo_root().join("ron-data/replays/multi/Playback___2020.07.25_19_30_12__Sat_.rcx");
    if !replay_path.is_file() {
        eprintln!(
            "\n  SKIPPED — NOT A PASS. The installed AI replay witness is absent; the canonical replay-selected BHS path was not established.\n"
        );
        return;
    }
    let replay = Replay::open(&replay_path).expect("decode installed AI replay witness");
    let content_owner = 2usize;
    assert!(replay.initial.active_players().any(|player| {
        player.slot as usize == content_owner && player.flags & LEADER_FLAG_HUMAN == 0
    }));
    let loaded = load_replay_bhs_program(&replay.initial, &content_root)
        .expect("select installed economic.bhs from the replay's AI roster");
    let pristine_program = loaded.checksum;
    let mut call = ReplayProductionCall {
        who: content_owner as i32 + 1,
        step: 1,
        boom_vs_rush: 1,
        num_loops: 5,
    };
    let call_before = call;
    let (types, upgrades) = canonical_type_owners(content_owner);
    let (mut sim, mut production, row) = production_owners(content_owner);
    // The replay-selected player is a real configured participant. Install that canonical
    // frame-zero owner so the post-Farm victory Leader flags can be reconstructed on load.
    sim.step8.leaders[content_owner].flags = 0;
    sim.vic_leaders.slots[content_owner].leader_flags = 0;
    let mut setup = ManualPlayerSetup {
        active_mask: 1 << content_owner,
        local_player_setup_slot: content_owner,
        ..ManualPlayerSetup::default()
    };
    setup.teams[content_owner] = 0;
    sim.start_manual_player_setup(setup)
        .expect("install replay-selected player setup owner");
    // The installed production image below owns one live Athens City. Keep the canonical
    // construct-time view coherent with that image for the reached Farm initializer.
    sim.step8.leaders[content_owner].city_num = 1;
    let completion_seen_mask = (1 << content_owner) | 1;
    sim.vic_leaders.slots[content_owner]
        .init_diplomacy
        .ally_mask = completion_seen_mask;
    // The replay's Athens center is an active City Build (native Object CITY bit 0x20).
    sim.builds[row].flags |= flag::CAPTURED;
    let installed_style = don_replay::map_style::MapStyleStaticData::load_from_ron_data(
        &repo_root().join("ron-data"),
        12,
    )
    .expect("load installed Mediterranean map-style owner");
    let image = ProductionBuiltinImage {
        map_style: Some(bind_production_map_style(12, &installed_style).unwrap()),
        setup: Some(ProductionSetupImage {
            game_info_flags: 0,
            game_rules: 0,
            difficulty: 0,
            rush_rules: 0,
            victory: 0,
            starting_town: 2,
            starting_resources: 1,
            starting_resources2: 1,
            semaphore: [0; 32],
        }),
        leaders: std::array::from_fn(|who| {
            if who == content_owner {
                ProductionLeaderImage {
                    flags: 3,
                    flags2: 0,
                    is_major_power: Some(true),
                    city_num: 1,
                    nation: "Romans".into(),
                    age: 0,
                    cities: vec![ProductionCityImage {
                        active: true,
                        object_id: BUILD_BAND_BASE as i16,
                        name: "Athens".into(),
                        identity: "capital_0".into(),
                        last_attacked: 0,
                        last_raided: 0,
                    }],
                }
            } else {
                ProductionLeaderImage::default()
            }
        }),
        type_counts: Some(
            bind_production_type_counts(&types, &upgrades, &sim.vic_leaders.slots).unwrap(),
        ),
        population: None,
        techs_per_age: [Some(4), None, None, None, None, None, None],
    };
    let mut timers = ScriptTimers::default();
    timers.add_timer("1", 300).unwrap();
    let pristine_timers = timers.clone();

    let (mut failure_sim, mut failure_production, failure_row) = production_owners(content_owner);
    failure_production.leaders[content_owner].resources = [0; 6];
    failure_sim.leaders[content_owner].econ.stockpile = [0; 6];
    failure_sim.step8.leaders[content_owner].econ.stockpile = [0; 6];
    failure_sim.vic_leaders.slots[content_owner].economy.bucket = [0; 6];
    let failure_scenario_before = failure_sim.scenario_data.clone();
    let failure_groups_before = failure_sim.groups.clone();
    let failure_queue_before = failure_sim.builds[failure_row].queue.clone();
    let mut failure_call = call;
    let failure_loaded = load_replay_bhs_program(&replay.initial, &content_root)
        .expect("reload the installed replay-selected program for the refusal arm");
    let (mut failure_runtime, failure_binding) = failure_loaded
        .into_script_runtime_with_timers(timers.clone())
        .expect("bind the installed refusal runtime");
    let failure_binding = failure_binding.expect("AI replay carries an economic binding");
    let refusal = run_production_research_call(
        &mut failure_runtime,
        &failure_binding,
        &mut failure_call,
        &image,
        &types,
        &upgrades,
        &place_building_costs(content_owner),
        None,
        &mut failure_sim,
        &mut failure_production,
        game_seconds(0),
    )
    .expect("the installed insufficient-resource path returns from economic.bhs");
    let refused = refusal
        .production
        .trace
        .iter()
        .position(|entry| {
            entry.index == 357
                && entry.args.get(1) == Some(&ProductionBuiltinValue::Str("Written Word".into()))
        })
        .expect("installed stock execution reaches the refused Written Word builtin 357");
    assert_eq!(
        refusal.production.trace[refused].returned,
        ProductionBuiltinValue::Int(0)
    );
    assert_eq!(
        refusal.production.trace[refused + 1].index,
        332,
        "the installed insufficient-resource continuation is at_least_type"
    );
    assert_eq!(
        refusal.production.trace[refused + 1].args,
        [
            ProductionBuiltinValue::Int(content_owner as i32 + 1),
            ProductionBuiltinValue::Int(75),
            ProductionBuiltinValue::Str("Wealth".into()),
        ]
    );
    assert_eq!(
        refusal.production.trace[refused + 1].returned,
        ProductionBuiltinValue::Int(0)
    );
    assert_eq!(refusal.production.returned, 1);
    assert_eq!(failure_call.step, 7);
    assert_eq!(failure_call.who, call_before.who);
    assert_eq!(failure_call.boom_vs_rush, call_before.boom_vs_rush);
    assert_eq!(failure_call.num_loops, call_before.num_loops);
    let mut expected_failure_scenario = failure_scenario_before;
    expected_failure_scenario.find_counters[30] = BUILD_BAND_BASE as i32;
    assert_eq!(failure_sim.scenario_data, expected_failure_scenario);
    assert_eq!(failure_sim.groups.list, failure_groups_before.list);
    assert_eq!(
        failure_sim.builds[failure_row].queue.queued,
        failure_queue_before.queued
    );
    assert_eq!(
        failure_sim.builds[failure_row].queue.entries,
        failure_queue_before.entries
    );
    assert_eq!(failure_production.leaders[content_owner].resources, [0; 6]);
    assert!(refusal
        .research
        .iter()
        .all(|receipt| receipt.status == SingleLibraryResearchStatus::Unavailable));

    let (mut script_runtime, binding) = loaded
        .into_script_runtime_with_timers(timers)
        .expect("bind the installed success runtime");
    let binding = binding.expect("AI replay carries an economic binding");
    let groups_before = sim.groups.clone();
    let queue_before = sim.builds[row].queue.clone();
    let city_build_before = sim.builds[row].image();
    let installed_place = apply_sim_place_building_with_cost_prefix(
        &sim,
        &production,
        &types,
        &place_building_costs(content_owner),
        PlaceBuildingRequest {
            who: content_owner as i32 + 1,
            type_name: "Farm".into(),
            city_name: "Athens".into(),
        },
    )
    .expect("admit the exact first installed Farm/Athens placement prefix");
    assert_eq!(
        installed_place.status,
        PlaceBuildingStatus::ReadyForLeaderProduceBuilding
    );
    assert_eq!(installed_place.can_pay_result, Some(2));
    assert_eq!(
        installed_place.continuation.unwrap().va,
        LEADER_PRODUCE_BUILDING_VA
    );
    let installed_produce = apply_sim_leader_produce_building_prefix(
        &sim,
        &production,
        LeaderProduceBuildingPrefixRequest {
            owner: content_owner as u8,
            type_index: 417,
            origin_build_object: BUILD_BAND_BASE as i16,
            mode: 0,
        },
    )
    .expect("admit the installed active-City origin into native placement search");
    assert_eq!(
        installed_produce.status,
        LeaderProduceBuildingPrefixStatus::ReadyForPlacementSearch
    );
    assert!(installed_produce.active_city_origin);
    assert_eq!(installed_produce.city_slot, Some(0));
    assert_eq!(
        installed_produce.continuation.unwrap().va,
        LEADER_PRODUCE_BUILDING_CITY_GATE_CONTINUATION_VA
    );
    let installed_search = apply_sim_leader_produce_building_search_setup(
        &sim,
        &production,
        &types,
        installed_produce.continuation.unwrap(),
    )
    .expect("advance the installed Farm through exact frame-zero search setup");
    assert_eq!(
        installed_search.status,
        LeaderProduceBuildingSearchSetupStatus::ReadyForCandidateLoop
    );
    assert_eq!(installed_search.origin_world_cell, [3, 3]);
    assert_eq!(installed_search.leader_radius, 20);
    assert_eq!(
        installed_search.target_footprint,
        Footprint {
            x_size: 4,
            y_size: 4
        }
    );
    assert_eq!(installed_search.footprint_search, [1, 1, 2]);
    assert_eq!(
        installed_search.continuation.unwrap().va,
        LEADER_PRODUCE_BUILDING_CANDIDATE_LOOP_VA
    );
    let installed_candidate = apply_sim_leader_produce_building_candidate_prefix(
        &sim,
        &production,
        &types,
        installed_search.continuation.unwrap(),
    )
    .expect("advance installed Farm to the first blocked_site virtual");
    assert_eq!(
        installed_candidate.status,
        LeaderProduceBuildingCandidatePrefixStatus::ReadyForBlockedSite
    );
    let installed_blocked_site = installed_candidate.continuation.unwrap();
    assert_eq!(installed_blocked_site.circle_offset, 1);
    assert_eq!(installed_blocked_site.candidate_world_cell, [2, 2]);
    assert_eq!(installed_blocked_site.space_grade, 4);
    assert_eq!(
        installed_blocked_site.va,
        LEADER_PRODUCE_BUILDING_BLOCKED_SITE_CALL_VA
    );
    let installed_blocked_prefix = apply_sim_leader_produce_building_blocked_site_prefix(
        &production,
        &types,
        installed_blocked_site,
    )
    .expect("advance the installed Farm through the exact blocked_site entry prefix");
    assert_eq!(installed_blocked_prefix.placement_tcoord, [10, 10]);
    assert_eq!(installed_blocked_prefix.footprint_corner, [8, 8]);
    assert_eq!(
        installed_blocked_prefix.continuation.va,
        BUILD_TYPE_BLOCKED_SITE_BLOCKED_TCOORD_CALL_VA
    );
    assert_eq!(
        installed_blocked_prefix.continuation.callee_va,
        BUILD_TYPE_BLOCKED_TCOORD_VA
    );
    let installed_blocked_tcoord = apply_sim_build_type_blocked_tcoord_land_prefix(
        &sim,
        &production,
        &types,
        installed_blocked_prefix.continuation,
    )
    .expect("advance installed Farm through the ordinary land blocked_tcoord prefix");
    assert_eq!(
        installed_blocked_tcoord.status,
        BuildTypeBlockedTcoordPrefixStatus::ReadyForLandDataGetAmount
    );
    assert_eq!(installed_blocked_tcoord.was_seen, Some(false));
    assert_eq!(
        installed_blocked_tcoord.continuation.unwrap().va,
        BUILD_TYPE_BLOCKED_TCOORD_GET_AMOUNT_CALL_VA
    );
    assert_eq!(
        installed_blocked_tcoord.continuation.unwrap().callee_va,
        LAND_DATA_GET_AMOUNT_VA
    );

    let terrain = installed_gather_terrain(&sim);
    let installed_footprint = apply_sim_leader_produce_building_blocked_site_land_footprint(
        &sim,
        &production,
        &types,
        &terrain,
        installed_blocked_prefix,
    )
    .expect("execute all sixteen installed Farm LandData::get_amount calls");
    assert_eq!(installed_footprint.tiles.len(), 16);
    assert_eq!(
        installed_footprint
            .tiles
            .iter()
            .map(|tile| tile.prefix.input.tile)
            .collect::<Vec<_>>(),
        (8..12)
            .flat_map(|tx| (8..12).map(move |ty| [tx, ty]))
            .collect::<Vec<_>>(),
        "retail walks the 4x4 footprint in x-outer/y-inner order"
    );
    assert!(installed_footprint.tiles.iter().all(|tile| {
        tile.amount.input.good == 0
            && tile.amount.input.land_index == 0
            && tile.amount.land_name == "Land"
            && tile.amount.returned == 1
            && tile.raw_returned_to_blocked_site == 0
    }));
    assert_eq!(installed_footprint.blocked_detail, 0);
    assert_eq!(installed_footprint.locally_seen_tiles, 0);
    assert_eq!(
        installed_footprint.continuation.va,
        BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_CALL_VA
    );
    assert_eq!(
        installed_footprint.continuation.callee_va,
        BUILD_TYPE_BLOCKED_LOCATION_VA
    );
    assert_eq!(
        installed_footprint
            .continuation
            .blocked_site_bytes_remaining,
        BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_BYTES_REMAINING
    );
    assert_eq!(
        BUILD_TYPE_BLOCKED_SITE_BLOCKED_LOCATION_BYTES_REMAINING,
        0x5f
    );
    assert_eq!(installed_footprint.continuation.owner, content_owner as u8);
    assert_eq!(installed_footprint.continuation.type_index, 417);
    assert_eq!(
        installed_footprint.continuation.placement_coord,
        [1920, 1920]
    );
    assert_eq!(installed_footprint.continuation.footprint_corner, [8, 8]);
    assert_eq!(installed_footprint.continuation.city_constraint, -1);
    assert_eq!(installed_footprint.continuation.blocked_detail, 0);

    let installed_site_tail = apply_sim_leader_produce_building_blocked_site_farm_unowned_tail(
        &sim,
        &production,
        &types,
        installed_footprint,
    )
    .expect("return the exact installed Farm dry/unowned blocked_location verdict");
    assert!(installed_site_tail.validates());
    assert_eq!(
        BUILD_TYPE_BLOCKED_LOCATION_END_VA - BUILD_TYPE_BLOCKED_LOCATION_VA,
        0x1400
    );
    assert_eq!(
        BUILD_TYPE_BLOCKED_LOCATION_NON_FRIENDLY_CALL_VA,
        0x0063_768b
    );
    assert_eq!(BUILD_TYPE_NON_FRIENDLY_TERRITORY_VA, 0x0063_89c0);
    assert_eq!(installed_site_tail.territory_reads.len(), 16);
    assert_eq!(
        installed_site_tail
            .territory_reads
            .iter()
            .map(|read| (read.tile, read.territory_owner))
            .collect::<Vec<_>>(),
        (8..12)
            .flat_map(|tx| (8..12).map(move |ty| ([tx, ty], -1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(installed_site_tail.non_friendly_territory, 1);
    assert!(!installed_site_tail.lakota_bonus);
    assert_eq!(installed_site_tail.blocked_location_returned, 0x1a);
    assert!(!installed_site_tail.immediate);
    assert_eq!(installed_site_tail.native_returned, 0x1a);
    let prior_territory_owner = sim.map.world.wdata(2, 2).who;
    sim.map.world.wdata_mut(2, 2).who = content_owner as i8;
    let unsupported_owned = apply_sim_leader_produce_building_blocked_site_farm_unowned_tail(
        &sim,
        &production,
        &types,
        installed_site_tail.input.clone(),
    );
    sim.map.world.wdata_mut(2, 2).who = prior_territory_owner;
    assert_eq!(
        unsupported_owned,
        Err(
            LeaderProduceBuildingBlockedSiteFarmUnownedError::UnsupportedTerritoryOwner {
                tile: [8, 8],
                territory_owner: content_owner as i8,
            }
        )
    );

    // The same first candidate is viable when its WData cell belongs to the caller. The
    // self-owned City-region witness makes every was_seen call exact; get_town selects Athens,
    // its canonical Build chain contains zero Farms, and retail's minimum limit is five.
    sim.map.world.wdata_mut(2, 2).who = content_owner as i8;
    let owned_footprint = apply_sim_leader_produce_building_blocked_site_land_footprint(
        &sim,
        &production,
        &types,
        &terrain,
        installed_site_tail.input.entry.clone(),
    )
    .expect("execute the self-owned Farm footprint with the exact active-City seen shortcut");
    assert!(owned_footprint.tiles.iter().all(|tile| {
        tile.prefix.was_seen == Some(true)
            && tile.amount.input.was_seen
            && tile.raw_returned_to_blocked_site == 0
    }));
    let owned_site = apply_sim_leader_produce_building_blocked_site_farm_owned_tail(
        &sim,
        &production,
        &types,
        owned_footprint,
    )
    .expect("accept the exact dry/self-owned Town/Farm-capacity branch");
    assert!(owned_site.validates());
    assert_eq!(owned_site.non_friendly_territory, 0);
    assert!(!owned_site.lakota_bonus);
    assert_eq!(owned_site.town.city_slot, 0);
    assert_eq!(owned_site.town.city_object, BUILD_BAND_BASE as i16);
    assert_eq!(owned_site.town.center_type, LIBRARY);
    assert_eq!(owned_site.town.distance, 6);
    assert_eq!(owned_site.town.radius, 20);
    assert_eq!(owned_site.town.counted_farms, 0);
    assert_eq!(owned_site.town.farm_limit_lower_bound, 5);
    assert_eq!(owned_site.water_tiles, 0);
    assert_eq!(owned_site.blocked_location_returned, 0);
    assert_eq!(owned_site.native_returned, 0);
    assert_eq!(owned_site.continuation.va, 0x006e_1e82);
    assert_eq!(owned_site.continuation.bytes_remaining, 0x126c);
    assert_eq!(owned_site.continuation.circle_offset, 1);
    assert_eq!(owned_site.continuation.candidate_world_cell, [2, 2]);
    assert_eq!(owned_site.continuation.placement_coord, [1920, 1920]);

    // The remaining success cone is staged without publishing either canonical RNG draw. Farm
    // sees no TData building/started bit at any ring-one probe, so every nested
    // `find_building_placed_at` returns -1 before scanning WData and the distance score consumes
    // one draw. All 34 later W candidates fail in territory before scoring. The fine 2x2
    // re-probe admits only corner (8,8); the other three cross unowned WData, so exactly one
    // more draw selects the same placement before `Objects::init_build`.
    let random_state_before = sim.world.random.state();
    let build_rows_before = sim.builds.len();
    let build_mark_before = sim.world.objects.slot(content_owner).mark(Band::Build);
    let success = apply_sim_leader_produce_building_farm_success_preflight(
        &sim,
        &production,
        &types,
        &terrain,
        installed_candidate.clone(),
        owned_site.clone(),
    )
    .expect("stage exact Farm scoring/RNG/fine-site/allocation cone");
    assert!(success.validates());
    assert_eq!(success.find_friends.len(), 8);
    assert!(success
        .find_friends
        .iter()
        .all(|probe| probe.found_build_object.is_none()));
    assert!(success
        .find_friends
        .iter()
        .all(|probe| probe.terrain_mask == tflag::CITY));
    assert_eq!(success.find_friends[7].circle_offset, 8);
    assert_eq!(success.find_friends[7].world_cell, [3, 3]);
    assert_eq!(success.find_friends[7].tile, [14, 14]);
    assert_eq!(success.find_friends_returned, 0);
    assert_eq!(success.origin_city_filter, 0);
    assert_eq!(success.distance_to_origin, 6);
    assert_eq!(success.distance_score, 666);
    assert_eq!(success.coarse_random.state_before, 0x357);
    assert_eq!(success.coarse_random.raw, 51_401);
    assert_eq!(success.coarse_random.remainder, 401);
    assert_eq!(success.coarse_random.state_after as u32, 0x9142_c8ca);
    assert_eq!(success.wdata_value, 0);
    assert_eq!(success.terrain_value_bonus, 255);
    assert_eq!(success.coarse_score, 1_322);
    assert_eq!(success.later_territory_rejected_sites, 34);
    assert_eq!(
        success
            .fine_sites
            .iter()
            .map(|site| (
                site.footprint_corner,
                site.placement_coord,
                site.blocked_site_returned,
                site.random.map(|draw| draw.remainder),
            ))
            .collect::<Vec<_>>(),
        vec![
            ([8, 8], [1920, 1920], 0, Some(76)),
            ([8, 9], [1920, 2112], 0x1a, None),
            ([9, 8], [2112, 1920], 0x1a, None),
            ([9, 9], [2112, 2112], 0x1a, None),
        ]
    );
    assert_eq!(success.selected_placement_coord, [1920, 1920]);
    assert_eq!(success.staged_random_state_after as u32, 0xd48d_a1a1);
    assert_eq!(success.continuation.va, 0x006e_2ca3);
    assert_eq!(success.continuation.callee_va, 0x0065_d190);
    assert_eq!(success.continuation.bytes_remaining, 0x44b);
    assert_eq!(success.continuation.fifth_argument, 0);
    assert_eq!(success.continuation.sixth_argument, -1);
    assert_eq!(success.continuation.expected_build_row, 1);
    assert_eq!(success.continuation.expected_object_id, 2001);
    assert_eq!(sim.world.random.state(), random_state_before);
    assert_eq!(sim.builds.len(), build_rows_before);
    assert_eq!(
        sim.world.objects.slot(content_owner).mark(Band::Build),
        build_mark_before
    );
    sim.map.world.wdata_mut(2, 2).who = prior_territory_owner;

    // Resume retail's bounded circle immediately after the first rejected site. Every later
    // surviving candidate must independently execute the exact sixteen-tile read cone and
    // preserve `0x1a`; only the common native-one/scenario-zero exhaustion epilogue is terminal.
    let mut resumed_boundary = installed_candidate.input;
    resumed_boundary.circle_offset = installed_blocked_site.circle_offset + 1;
    let mut territory_rejected_sites = 1usize;
    let (search_native_returned, search_scenario_returned) = loop {
        if resumed_boundary.circle_offset >= installed_candidate.circle_radius_end {
            break (1, 0);
        }
        let resumed_candidate = apply_sim_leader_produce_building_candidate_prefix(
            &sim,
            &production,
            &types,
            resumed_boundary,
        )
        .expect("resume the exact candidate ring after an unowned-territory refusal");
        if let Some(native) = resumed_candidate.native_returned {
            break (
                native,
                resumed_candidate
                    .scenario_returned
                    .expect("candidate exhaustion carries the scenario scalar"),
            );
        }
        let resumed_site_boundary = resumed_candidate
            .continuation
            .expect("a nonterminal candidate reaches blocked_site");
        let resumed_site_prefix = apply_sim_leader_produce_building_blocked_site_prefix(
            &production,
            &types,
            resumed_site_boundary,
        )
        .expect("execute the resumed candidate's blocked_site entry");
        let resumed_footprint = apply_sim_leader_produce_building_blocked_site_land_footprint(
            &sim,
            &production,
            &types,
            &terrain,
            resumed_site_prefix,
        )
        .expect("execute the resumed candidate's sixteen exact Farm tile reads");
        let resumed_site = apply_sim_leader_produce_building_blocked_site_farm_unowned_tail(
            &sim,
            &production,
            &types,
            resumed_footprint,
        )
        .expect("preserve the resumed candidate's exact unowned-territory verdict");
        assert_eq!(resumed_site.native_returned, 0x1a);
        territory_rejected_sites += 1;
        resumed_boundary = resumed_candidate.input;
        resumed_boundary.circle_offset = resumed_site_boundary.circle_offset + 1;
    };
    assert_eq!(territory_rejected_sites, 35);
    assert_eq!(search_native_returned, 1);
    assert_eq!(search_scenario_returned, 0);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.builds[row].queue.entries, queue_before.entries);
    assert_eq!(sim.builds[row].image(), city_build_before);
    assert_eq!(production.leaders[content_owner].resources, [100; 6]);

    // Bind every source needed by the reached success arm before entering the real builtin.
    // A zero-capacity Farms owner forces the latest fallible constructor refusal, proving that
    // the surrounding Program/ref/timer/research/resource/placement transaction rolls it all
    // back rather than exposing the earlier successful research calls or staged RNG draws.
    sim.map.world.wdata_mut(2, 2).who = content_owner as i8;
    assert_eq!(sim.map.world.wdata(2, 2).region, 64);
    let source_digest = [0x91; 32];
    let height_len = (sim.map.world.tile_xs as usize + 1) * (sim.map.world.tile_ys as usize + 1);
    let terrain_height = TerrainHeightAuthority {
        master_land_height_bits: vec![0; height_len],
        land_height_bits: 0,
        source: TerrainHeightSource::CompletedWorldgen,
        source_digest,
    };
    let mut next_uid = [0; 8];
    next_uid[content_owner] = 1;
    let mut buildings_built = [0; 8];
    buildings_built[content_owner] = 1;
    let mut build_authority = FarmBuildInitAuthority {
        revision: 7,
        source_digest,
        next_uid,
        buildings_built,
    };
    let mut builds_walk = BuildsWalkAuthority::default();
    let source = FarmFrameZeroSourceFacts::supported_flat_farm(source_digest, [1; 16]);
    let costs = place_building_costs(content_owner);
    let saved_farms = std::mem::replace(&mut sim.farms, Farms::with_header(0, -1, 0));
    let outer_random_before = sim.world.random.state();
    let outer_builds_before: Vec<_> = sim.builds.iter().map(BuildData::image).collect();
    let outer_build_mark_before = sim.world.objects.slot(content_owner).mark(Band::Build);
    let outer_wdata_before = sim.map.world.wdata.clone();
    let outer_tdata_before = sim.map.world.tdata.clone();
    let outer_seen2_before = sim.map.world.seen2.clone();
    let outer_city_before = sim.cities.slots[content_owner][0].clone();
    let outer_build_types_before = production.build_types.clone();
    let outer_resources_before = production.leaders[content_owner].resources;
    let outer_victory_before = format!("{:?}", sim.vic_leaders);
    let outer_build_authority_before = build_authority.clone();
    let outer_walks_before = builds_walk.clone();

    let error = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &costs,
        Some(FarmBuiltin520Authority {
            expected_cost_revision: costs.revision,
            expected_cost_composition_digest: costs.composition_digest,
            gather_terrain: &terrain,
            terrain_height: &terrain_height,
            source,
            build_authority: &mut build_authority,
            builds_walk: &mut builds_walk,
            expected_build_authority_revision: 7,
        }),
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .expect_err("the next unowned production builtin must keep the whole call red");
    assert!(
        matches!(
            &error.failure,
            ProductionRunFailure::Vm(VmError::UnimplementedBuiltin {
                index: 520,
                name: "place_building_with_cost"
            })
        ),
        "unexpected installed continuation: {:?}",
        error.failure
    );

    let ww = error
        .trace
        .iter()
        .position(|entry| {
            entry.index == 357
                && entry.args.get(1) == Some(&ProductionBuiltinValue::Str("Written Word".into()))
        })
        .expect("installed stock execution reaches Written Word builtin 357");
    assert_eq!(
        error.trace[ww].returned,
        ProductionBuiltinValue::Int(BUILD_BAND_BASE as i32)
    );
    assert_eq!(
        error.trace[ww + 1..ww + 4]
            .iter()
            .map(|entry| entry.index)
            .collect::<Vec<_>>(),
        [258, 362, 357],
        "the immediate measured success continuation is num_cities/have_tech/City State research"
    );
    assert_eq!(
        error.trace[ww + 3].args.get(1),
        Some(&ProductionBuiltinValue::Str("City State".into()))
    );
    assert_eq!(
        error.trace[ww + 3].returned,
        ProductionBuiltinValue::Int(BUILD_BAND_BASE as i32)
    );
    let idle = error
        .trace
        .iter()
        .position(|entry| entry.index == 455)
        .expect("installed success continuation reaches the live idle-Unit census");
    assert_eq!(
        error.trace[idle].args,
        [
            ProductionBuiltinValue::Int(content_owner as i32 + 1),
            ProductionBuiltinValue::Str("Citizens".into()),
        ]
    );
    assert_eq!(
        error.trace[idle].returned,
        ProductionBuiltinValue::Int(0),
        "the installed Sim has an empty canonical Unit band"
    );
    let city_counts: Vec<_> = error
        .trace
        .iter()
        .skip(idle + 1)
        .take_while(|entry| entry.index == 386)
        .collect();
    assert_eq!(city_counts.len(), 3);
    assert_eq!(
        city_counts
            .iter()
            .map(|entry| entry.args.get(2))
            .collect::<Vec<_>>(),
        [
            Some(&ProductionBuiltinValue::Str("Farm".into())),
            Some(&ProductionBuiltinValue::Str("Woodcutter's Camp".into())),
            Some(&ProductionBuiltinValue::Str("Mine".into())),
        ]
    );
    assert!(city_counts
        .iter()
        .all(|entry| entry.returned == ProductionBuiltinValue::Int(0)));
    let queue_counts: Vec<_> = error
        .trace
        .iter()
        .filter(|entry| entry.index == 436)
        .collect();
    assert_eq!(queue_counts.len(), 1);
    assert_eq!(
        queue_counts[0].args,
        [
            ProductionBuiltinValue::Int(content_owner as i32 + 1),
            ProductionBuiltinValue::Int(BUILD_BAND_BASE as i32),
            ProductionBuiltinValue::Str("Citizens".into()),
        ]
    );
    assert_eq!(
        queue_counts[0].returned,
        ProductionBuiltinValue::Int(0),
        "the canonical Library queue contains research, not Citizen production"
    );

    assert_eq!(call, call_before);
    assert_eq!(
        don_replay::script_channel::checksum_program(script_runtime.program()).unwrap(),
        pristine_program
    );
    assert_eq!(script_runtime.script_timers(), &pristine_timers);
    assert_eq!(sim.scenario_data.find_counters[30], -1);
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.groups.last_group, groups_before.last_group);
    assert_eq!(sim.builds[row].queue.queued, queue_before.queued);
    assert_eq!(sim.builds[row].image(), city_build_before);
    assert_eq!(
        production.leaders[content_owner].queued_counts[WRITTEN_WORD as usize],
        0
    );
    assert_eq!(
        production.leaders[content_owner].queued_counts[CITY_STATE as usize],
        0
    );
    assert_eq!(production.leaders[content_owner].ages_queued, 0);
    assert_eq!(production.leaders[content_owner].epochs_queued, 0);
    assert_eq!(sim.world.random.state(), outer_random_before);
    assert_eq!(
        sim.builds.iter().map(BuildData::image).collect::<Vec<_>>(),
        outer_builds_before
    );
    assert_eq!(
        sim.world.objects.slot(content_owner).mark(Band::Build),
        outer_build_mark_before
    );
    assert_eq!(sim.map.world.wdata, outer_wdata_before);
    assert_eq!(sim.map.world.tdata, outer_tdata_before);
    assert_eq!(sim.map.world.seen2, outer_seen2_before);
    assert_eq!(sim.cities.slots[content_owner][0], outer_city_before);
    assert_eq!(production.build_types, outer_build_types_before);
    assert_eq!(
        production.leaders[content_owner].resources,
        outer_resources_before
    );
    assert_eq!(format!("{:?}", sim.vic_leaders), outer_victory_before);
    assert_eq!(build_authority, outer_build_authority_before);
    assert_eq!(builds_walk, outer_walks_before);
    sim.farms = saved_farms;

    let receipt = run_production_research_call(
        &mut script_runtime,
        &binding,
        &mut call,
        &image,
        &types,
        &upgrades,
        &costs,
        Some(FarmBuiltin520Authority {
            expected_cost_revision: costs.revision,
            expected_cost_composition_digest: costs.composition_digest,
            gather_terrain: &terrain,
            terrain_height: &terrain_height,
            source,
            build_authority: &mut build_authority,
            builds_walk: &mut builds_walk,
            expected_build_authority_revision: 7,
        }),
        &mut sim,
        &mut production,
        game_seconds(0),
    )
    .expect("the fully owned frame-zero Farm builtin commits the economic call");
    let placed = receipt
        .production
        .trace
        .iter()
        .find(|entry| entry.index == 520)
        .expect("the successful call records builtin 520");
    assert_eq!(placed.returned, ProductionBuiltinValue::Int(1));
    assert_eq!(receipt.farm_builtin_520_costs.len(), 1);
    assert_eq!(receipt.produce_building_land_footprints.len(), 1);
    assert_eq!(receipt.produce_building_owned_sites.len(), 1);
    assert_eq!(receipt.produce_building_success_preflights.len(), 1);
    let paid = &receipt.farm_builtin_520_costs[0];
    assert_eq!(LEADER_PRODUCE_BUILDING_FRAME_PAYMENT_GATE_VA, 0x006e_2c66);
    assert_eq!(LEADER_PRODUCE_BUILDING_PAY_COST_CALL_VA, 0x006e_2c89);
    assert_eq!(TYPE_PAY_COST_VA, 0x0066_81f0);
    assert!(paid.payment_skipped_at_frame_zero);
    assert_eq!(paid.resolved_costs, [0, 40, 0, 0, 0, 0]);
    assert_eq!(paid.resources_before, [88, 88, 95, 100, 100, 100]);
    assert_eq!(paid.resources_after, paid.resources_before);
    assert_eq!(receipt.farm_frame_zero_tails.len(), 1);
    let farm = &receipt.farm_frame_zero_tails[0];
    assert_eq!(farm.row, 1);
    assert_eq!(farm.object_id, 2001);
    assert_eq!(farm.city_slot, 0);
    assert_eq!(farm.farm_index, 0);
    assert_eq!(farm.footprint_corner, [8, 8]);
    assert_eq!(farm.blocked_tiles, 16);
    assert_eq!(farm.behind_tiles, 56);
    assert_eq!(farm.farm_random_value, 20_107);
    assert_eq!(farm.farm_type, 1);
    assert_eq!(farm.random_state_before, 0x357);
    assert_eq!(farm.random_state_after_search as u32, 0xd48d_a1a1);
    assert_eq!(farm.random_state_after_farm as u32, 0x3ebf_4e8c);
    assert_eq!(farm.construct_time, 15_000);
    assert_eq!(farm.final_flags, flag::VALID | flag::STARTED | flag::ACTIVE);
    assert_eq!(farm.final_build_mask, 0x1000);
    assert_eq!(farm.final_seen, (1 << content_owner, completion_seen_mask));
    assert_eq!((farm.city_filled_before, farm.city_filled_after), (0, 1));
    assert_eq!(
        (
            farm.authority_revision_before,
            farm.authority_revision_after
        ),
        (7, 8)
    );
    assert_eq!(farm.walk.bytes_walked, 167);
    assert_eq!(sim.world.random.state() as u32, 0x3ebf_4e8c);
    assert_eq!(
        sim.world.objects.slot(content_owner).mark(Band::Build),
        2002
    );
    assert_eq!(production.build_types[1], Some(417));
    assert_eq!(sim.builds[row].city_down, 2001);
    assert_eq!(sim.builds[1].city, 0);
    assert_eq!(sim.builds[1].city_down, -1);
    assert_eq!(sim.builds[1].dock, 0);
    assert_eq!(sim.builds[1].uid, 1);
    assert_eq!(sim.builds[1].myhits, 400);
    assert_eq!(sim.builds[1].construct_hits, 1);
    assert_eq!(sim.builds[1].queue.entries.len(), 2);
    assert_eq!(sim.builds[1].queue.entries[0].type_index, -1);
    assert_eq!(sim.map.world.wdata(2, 2).down, 2001);
    assert_eq!(sim.map.world.wdata(2, 2).down_who, content_owner as i16);
    assert_eq!(sim.map.world.wdata(2, 2).was_seen, 1 << content_owner);
    assert_eq!(sim.cities.slots[content_owner][0].filled, 1);
    assert_eq!(sim.farms.records()[0].who, content_owner as i32);
    assert_eq!(sim.farms.records()[0].o, 2001);
    assert_eq!(sim.farms.records()[0].farm_type, 1);
    assert_eq!(build_authority.next_uid[content_owner], 2);
    assert_eq!(build_authority.buildings_built[content_owner], 2);
    assert_eq!(
        sim.step8.leaders[content_owner].flags,
        3 | 0x0200_0000 | 0x0800_0000
    );
    assert_eq!(
        sim.vic_leaders.slots[content_owner].leader_flags as u32,
        sim.step8.leaders[content_owner].flags
    );
    assert_eq!(
        sim.vic_leaders.slots[content_owner].num_buildings[417 - 414],
        1
    );
    assert!(builds_walk.get(1).is_some());

    // The canonical victory Leader and CityPool own the two non-pristine step-8 values left
    // by Build::init/Wall::activate. Loading rehydrates that exact adapter after-image and an
    // immediate resave is byte-identical.
    let saved = save_sim(&sim).expect("save exact frame-zero Farm after-image");
    let resumed = load_sim(&saved).expect("resume exact frame-zero Farm after-image");
    assert_eq!(save_sim(&resumed).unwrap(), saved);
    assert_eq!(resumed.world.random.state(), sim.world.random.state());
    assert_eq!(resumed.builds[row].flags, sim.builds[row].flags);
    assert_eq!(resumed.builds[row].who, sim.builds[row].who);
    assert_eq!(resumed.builds[row].city, sim.builds[row].city);
    assert_eq!(resumed.builds[row].city_down, sim.builds[row].city_down);
    assert_eq!(resumed.builds[row].other, sim.builds[row].other);
    assert_eq!(
        resumed.builds[row].queue.entries,
        sim.builds[row].queue.entries
    );
    assert_eq!(resumed.farms.records(), sim.farms.records());
    assert_eq!(
        resumed.cities.slots[content_owner][0],
        sim.cities.slots[content_owner][0]
    );
    assert_eq!(
        resumed.step8.leaders[content_owner].flags,
        sim.step8.leaders[content_owner].flags
    );
    assert_eq!(
        resumed.step8.leaders[content_owner].city_num,
        sim.step8.leaders[content_owner].city_num
    );
    assert_eq!(
        resumed.step8.leaders[content_owner].econ,
        sim.step8.leaders[content_owner].econ
    );
    assert_eq!(
        resumed.vic_leaders.slots[content_owner].leader_flags,
        sim.vic_leaders.slots[content_owner].leader_flags
    );
    assert_eq!(
        resumed.vic_leaders.slots[content_owner].num_buildings,
        sim.vic_leaders.slots[content_owner].num_buildings
    );
    assert_eq!(resumed.channel_digest(), sim.channel_digest());
}
