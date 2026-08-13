use std::path::{Path, PathBuf};

use don_replay::groups_first_farm_authority::{
    bind_first_farm_builder_at_frame79, bind_first_farm_first_citizen_init,
    bind_first_farm_first_init_unit, discover_first_2018_farm,
    produce_first_farm_first_citizen_guy_prefix, produce_first_farm_first_citizen_location,
    produce_first_farm_first_citizen_placement, produce_first_farm_first_placement,
    produce_first_farm_first_scout_collision_tail, produce_first_farm_first_scout_complete_init,
    produce_first_farm_first_scout_guy_prefix, produce_first_farm_first_scout_location,
    produce_first_farm_first_scout_visibility, FirstFarmAuthorityBlocker, FirstFarmAuthorityError,
    FirstFarmBuilderBindingError, FirstFarmFirstCitizenInitAuthority,
    FirstFarmFirstCitizenInitError, FirstFarmFirstCitizenInitSource,
    FirstFarmFirstCitizenLocationError, FirstFarmFirstCitizenLocationInputs,
    FirstFarmFirstCitizenPlacementError, FirstFarmFirstInitUnitAuthority,
    FirstFarmFirstInitUnitError, FirstFarmFirstInitUnitSource, FirstFarmFirstPlacementError,
    FirstFarmFirstScoutCollisionAuthority, FirstFarmFirstScoutCollisionError,
    FirstFarmFirstScoutCollisionInputs, FirstFarmFirstScoutCollisionSource,
    FirstFarmFirstScoutCompleteInitError, FirstFarmFirstScoutCompleteInitReceipt,
    FirstFarmFirstScoutGuyError, FirstFarmFirstScoutLocationError,
    FirstFarmFirstScoutLocationInputs, FirstFarmFirstScoutVisibilityAuthority,
    FirstFarmFirstScoutVisibilityError, FirstFarmFirstScoutVisibilitySource,
    FirstFarmFrame79Authority, FirstFarmFrame79Source, FirstFarmSetupEntryAuthority,
    FirstFarmSetupEntrySource, FIRST_FRAME, FIRST_OWNER, FIRST_PLAY, FIRST_SELECTED_O,
    FIRST_SERIAL, STRICT_REPLAY_SHA256,
};
use don_replay::groups_pre_pair_unit_authority::{
    replay_build_type_facts, PrePairUnitAuthorityError,
};
use don_replay::replay::{load_payload, Replay};
use don_replay::setup_units_producer::{
    build_units_plan, BuildUnitsInputs, BuildUnitsPlan, BuildUnitsPrefixReceipt,
    DirectRandomDrawReceipt, EngineContainerShapeReceipt, GuyIdentityReceipt,
    InitUnitAuthorityReceipt, InitUnitRngSpan, PlaceUnitCall, PlaceUnitReceipt,
    PlacementOutcomeReceipt, PlacementRngEvent, StableUnitIdentityReceipt, StartingUnitBonuses,
    StartingUnitRuleFacts, StartingUnitTypeFacts, TypeResolutionFacts, UnitMemberAuthorityReceipt,
    DUTCH_MERCHANT_TYPE, OBJECTS_INIT_UNIT_BYTES, OBJECTS_INIT_UNIT_VA,
    PLACE_UNIT_DIRECT_RANDOM_CALL_VA,
};
use don_replay::{
    build_spawn_runtime::{spawn_canonical_build, CanonicalBuildSpawnRequest},
    leaders_dynamic_children_frontier::DynamicLeadersAuthority,
    setup_cities_builds::CITY_CENTER_TYPE,
    setup_place_unit_deep_re::{
        GuyGraphicsInitReceipt, GuyInitPredicateFacts, PlaceUnitExternalResidual, ProbeDisposition,
        UnitGuyInitError,
    },
    setup_unit_visibility_deep_re::{
        RevealLeaderFacts, SetupUnitVisibilityAuthority, SetupVisibilityExternalResidual,
    },
    unit_init_collision_tail_deep_re::{
        UnitInitTailExternalResidual, UnitTailLeaderFacts, UnitTailScalarField,
        UnitTailScalarReceipt, UnitTailStatReceipts, OBJECT_UPDATE_SEEN_VA,
        UNIT_UPDATE_ARMOR_CALL_VA, UNIT_UPDATE_ARMOR_VA, UNIT_UPDATE_HITS_CALL_VA,
        UNIT_UPDATE_HITS_VA, UNIT_UPDATE_LOS_CALL_VA, UNIT_UPDATE_LOS_VA,
        UNIT_UPDATE_SPEED_CALL_VA, UNIT_UPDATE_SPEED_VA,
    },
    unit_init_location_deep_re::{
        TerrainHeightReceipt, TerrainQueryKind, UnitInitLocationError, GUY_TERRAIN_Z_CALL_VA,
        TERRAIN_FIND_DATA_Z_VA, TERRAIN_FIND_TCOORD_Z_VA, UNIT_INIT_SET_ANGLE,
        UNIT_TERRAIN_Z_CALL_VA,
    },
};
use don_sim::rng::Random;
use don_sim::systems::{
    casters_animals::ManaCapacityInput,
    graphics_turret::{ExtractedGuyGraphics, GraphicsProvenance},
    items::Items,
    map_terrain::land,
    objects_init_unit_authority_frontier::{
        BhsInitUnitRequest, CaptainFacts, CompleteBody, DetailedInitUnitReceipt,
        FindFreeDisposition, FindFreeUnitReceipt, InitUnitReceiptError, InitUnitStep,
        ResolveCaptainReceipt, TrackUnitTypeFacts, TrainingWhere, UnitAfterInit,
        UnitBandStorageClass, UnitInitReceipt, UnitTypeAuthorityFacts,
    },
    production,
    step12_visibility_producer_frontier::UnitLosFacts,
    unit_inctime::{SUPPORTED_RETAIL_EXE_SHA256, SUPPORTED_UNIT_GRAPHICS_SHA256},
    world_oil_goods::OilGoodRuntime,
};
use don_sim::tick::Sim;
use don_sim::world::Handle;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn replay_path() -> PathBuf {
    repo_root().join("ron-data/replays/multi/Playback___2018.11.17_13_21_42__Sat_.rcx")
}

fn type_facts(base: i32) -> TypeResolutionFacts {
    TypeResolutionFacts {
        base,
        tribe_can_base: true,
        nation_variant: base,
        build_units_upgrade: base,
        place_unit_upgrade: base,
        uber_size: 1,
        squad_size: 1,
        crew_size: 0,
    }
}

fn first_farm_plan() -> BuildUnitsPlan {
    let mut scout = type_facts(69);
    scout.crew_size = 1;
    build_units_plan(BuildUnitsInputs {
        owner: i32::from(FIRST_OWNER),
        start_index: 0,
        center_city_o: 2_000,
        start_tile_x: 28,
        start_tile_y: 84,
        starting_town: 1,
        starting_resources: 2,
        reveal_map: 1,
        bonuses: StartingUnitBonuses::default(),
        rules: StartingUnitRuleFacts::default(),
        types: StartingUnitTypeFacts {
            scout,
            citizen: type_facts(50),
            dutch_merchant: type_facts(DUTCH_MERCHANT_TYPE),
        },
    })
    .unwrap()
}

fn member(call: PlaceUnitCall, handle: Handle, o: i32) -> UnitMemberAuthorityReceipt {
    let guy_count = call.squad_size + call.crew_size;
    UnitMemberAuthorityReceipt {
        identity: StableUnitIdentityReceipt {
            id: handle.id,
            generation: handle.generation,
            owner: call.owner,
            o,
        },
        ptype_index: call.place_unit_upgrade,
        launching_is_null: true,
        path: EngineContainerShapeReceipt {
            length: 0,
            capacity: 10,
            increment: -1,
            flags: 0,
        },
        order_count: 0,
        guys: EngineContainerShapeReceipt {
            length: guy_count,
            capacity: guy_count,
            increment: 1,
            flags: 0,
        },
        guy_mark: call.squad_size as i8,
        guy_identities: (0..guy_count)
            .map(|slot| GuyIdentityReceipt {
                slot,
                who: FIRST_OWNER as i8,
                o: o as i16,
                guy_num: slot as i8,
            })
            .collect(),
        units_authority_key: (handle.id, handle.generation),
        guys_authority_key: (handle.id, handle.generation),
    }
}

fn synthetic_frame79_setup() -> (BuildUnitsPlan, BuildUnitsPrefixReceipt, Sim) {
    let plan = first_farm_plan();
    let mut sim = Sim::new(7, 128);
    let handles: Vec<_> = plan
        .calls
        .iter()
        .enumerate()
        .map(|(ordinal, call)| {
            sim.spawn_unit(
                FIRST_OWNER as usize,
                call.place_unit_upgrade,
                21_600 + ordinal as i32 * 32,
                64_608,
                (call.squad_size + call.crew_size) as i8,
            )
            .unwrap()
        })
        .collect();
    sim.world.frame = FIRST_FRAME;

    let rng_initial = 0x12345;
    let mut rng_state = rng_initial;
    let placements = plan
        .calls
        .iter()
        .copied()
        .enumerate()
        .map(|(ordinal, call)| {
            let rng_before = rng_state;
            let mut rng = Random::new(rng_state);
            let returned = rng.get(0, 0xffff);
            let direct_after = rng.state();
            let rng_after = direct_after.wrapping_add(0x101 + ordinal as i32);
            rng_state = rng_after;
            PlaceUnitReceipt {
                call,
                rng_before,
                rng_after,
                rng_events: vec![
                    PlacementRngEvent::DirectOffset(DirectRandomDrawReceipt {
                        call_va: PLACE_UNIT_DIRECT_RANDOM_CALL_VA,
                        state_before: rng_before,
                        returned,
                        state_after: direct_after,
                    }),
                    PlacementRngEvent::InitUnit(InitUnitRngSpan {
                        body_va: OBJECTS_INIT_UNIT_VA,
                        body_bytes: OBJECTS_INIT_UNIT_BYTES,
                        state_before: direct_after,
                        state_after: rng_after,
                    }),
                ],
                outcome: PlacementOutcomeReceipt::Spawned(InitUnitAuthorityReceipt {
                    validated_body_va: OBJECTS_INIT_UNIT_VA,
                    validated_body_bytes: OBJECTS_INIT_UNIT_BYTES,
                    unit_mark_before: ordinal as i32,
                    unit_mark_after: ordinal as i32 + 1,
                    returned_captain_o: ordinal as i32,
                    members: vec![member(call, handles[ordinal], ordinal as i32)],
                }),
            }
        })
        .collect();
    (
        plan,
        BuildUnitsPrefixReceipt {
            rng_initial,
            rng_final: rng_state,
            placements,
        },
        sim,
    )
}

fn authority() -> FirstFarmFrame79Authority {
    FirstFarmFrame79Authority {
        revision: 1,
        composition_digest: [0x79; 32],
        source: FirstFarmFrame79Source::ValidatedSetupAndCanonicalReplayExecution,
    }
}

fn synthetic_setup_entry(replay: &Replay) -> (Sim, FirstFarmSetupEntryAuthority) {
    let edge = replay.initial.info.settings.map_edge_world_cells().unwrap() as u16;
    let mut sim = Sim::new(u64::from(replay.initial.info.seed), edge);
    sim.activate(FIRST_OWNER as usize);
    for cell in &mut sim.map.world.wdata {
        cell.land = land::FERTILE;
        cell.region = 1;
        cell.down = -1;
        cell.flags = 0;
    }
    sim.map.world.tdata.fill(0);

    let mut build = production::BuildData {
        flags: production::flag::VALID,
        city: -1,
        city_down: -1,
        ..production::BuildData::default()
    };
    build.queue.queued = 0;
    let spawned = spawn_canonical_build(
        &mut sim,
        CanonicalBuildSpawnRequest {
            owner: FIRST_OWNER,
            type_index: CITY_CENTER_TYPE,
            snapped_x: 21_600,
            snapped_y: 64_608,
            build,
        },
    )
    .unwrap();
    assert_eq!(spawned.object_id, 2_000);
    let post_worldgen_rng = 0x12345;
    sim.world.random.reseed(post_worldgen_rng);
    let authority = FirstFarmSetupEntryAuthority {
        revision: 1,
        composition_digest: [0x5a; 32],
        source: FirstFarmSetupEntrySource::CompletedCanonicalWorldgenAndStartingCitySetup,
        world_checksum: sim.map.world.checksum_sections(),
        post_worldgen_rng,
    };
    (sim, authority)
}

fn first_scout_detailed_receipt(
    request: don_replay::setup_place_unit_deep_re::ObjectsInitUnitRequest,
    after: &Sim,
) -> DetailedInitUnitReceipt {
    let row = after.world.unit_row_at(FIRST_OWNER.into(), 0).unwrap();
    let after_image = UnitAfterInit {
        owner: request.owner,
        o: 0,
        type_index: request.type_index,
        x: after.world.units.x_internal()[row],
        y: after.world.units.y_internal()[row],
        angle: after.world.units.angle()[row],
        unit_masks: after.world.units.get_unit_masks(row),
    };
    DetailedInitUnitReceipt {
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
                    x: after_image.x,
                    y: after_image.y,
                    angle: after_image.angle,
                    new_block_radius: 1,
                },
            }),
        ],
        returned: 0,
    }
}

fn first_citizen_detailed_receipt(
    request: don_replay::setup_place_unit_deep_re::ObjectsInitUnitRequest,
    after: &Sim,
    new_block_radius: i32,
) -> DetailedInitUnitReceipt {
    let row = after.world.unit_row_at(i32::from(FIRST_OWNER), 1).unwrap();
    let after_image = UnitAfterInit {
        owner: request.owner,
        o: 1,
        type_index: request.type_index,
        x: after.world.units.x_internal()[row],
        y: after.world.units.y_internal()[row],
        angle: after.world.units.angle()[row],
        unit_masks: after.world.units.get_unit_masks(row),
    };
    DetailedInitUnitReceipt {
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
                is_peasant: true,
                is_scholar: false,
                role_has_scout_bit: false,
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
                cursor_before: 1,
                cursor_after: 2,
                returned: 1,
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
                o: 1,
                x: request.x,
                y: request.y,
                returned: 1,
                extent: CompleteBody::UnitInit3732Bytes,
                after: after_image,
            }),
            InitUnitStep::SetPrevious {
                ordinal: 0,
                member_o: 1,
                previous_o: -1,
            },
            InitUnitStep::ResolveCaptain(ResolveCaptainReceipt {
                ordinal: 1,
                from_owner: request.owner,
                from_o: 1,
                returned: 1,
                captain: CaptainFacts {
                    owner: request.owner,
                    o: 1,
                    x: after_image.x,
                    y: after_image.y,
                    angle: after_image.angle,
                    new_block_radius,
                },
            }),
        ],
        returned: 1,
    }
}

fn synthetic_first_scout_init(
    replay: &Replay,
    plan: &BuildUnitsPlan,
) -> (
    Sim,
    FirstFarmSetupEntryAuthority,
    DetailedInitUnitReceipt,
    Sim,
    FirstFarmFirstInitUnitAuthority,
) {
    let (before, placement_authority) = synthetic_setup_entry(replay);
    let placement =
        produce_first_farm_first_placement(replay, plan, &before, &placement_authority).unwrap();
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) =
        placement.placement.first_external_residual
    else {
        unreachable!()
    };
    let (mut after, _) = synthetic_setup_entry(replay);
    after
        .spawn_unit(
            request.owner as usize,
            request.type_index,
            request.x,
            request.y,
            2,
        )
        .unwrap();
    let row = after.world.unit_row_at(request.owner, 0).unwrap();
    let normalize = |value: i32| {
        let shifted = value >> 4;
        shifted.div_euclid(3) * 48 + 24
    };
    after
        .world
        .set_pos(row, normalize(request.x), normalize(request.y));
    after.world.units.angle_mut()[row] = UNIT_INIT_SET_ANGLE;
    after.world.units.form_mut()[row] = 0;
    let mut init_rng = Random::new(placement.placement.rng_after_probes);
    init_rng.get(0, 0xffff);
    init_rng.get(0, 0xffff);
    let rng_after = init_rng.state();
    after.world.random.reseed(rng_after);
    let detailed = first_scout_detailed_receipt(request, &after);
    let authority = FirstFarmFirstInitUnitAuthority {
        revision: 1,
        composition_digest: [0x65; 32],
        source: FirstFarmFirstInitUnitSource::CompleteRetailBodyAndCanonicalAfterImage,
        map_checksum_after: after.map.world.checksum_sections(),
        rng_after,
    };
    (before, placement_authority, detailed, after, authority)
}

fn synthetic_first_scout_graphics(slot: i8) -> GuyGraphicsInitReceipt {
    GuyGraphicsInitReceipt {
        provenance: GraphicsProvenance {
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            installed_unit_graphics_sha256: SUPPORTED_UNIT_GRAPHICS_SHA256,
            coherent_capture: true,
        },
        extracted: ExtractedGuyGraphics {
            guy_num: slot,
            gpiece: 100 + i32::from(slot),
            pivot_graph_name: None,
            track_dx: if slot == 0 { 0 } else { 300 },
            track_dy: if slot == 0 { 0 } else { -400 },
            turret_angles: [0; 4],
            des_turret_angles: [0; 4],
            node_flags: 0,
            des_node_flags: 0,
        },
        restriction_count: 0,
    }
}

fn first_scout_terrain(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    before: &Sim,
    authority: &FirstFarmSetupEntryAuthority,
    crew_track: (i32, i32),
) -> Vec<TerrainHeightReceipt> {
    let placement = produce_first_farm_first_placement(replay, plan, before, authority).unwrap();
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) =
        placement.placement.first_external_residual
    else {
        unreachable!()
    };
    let normalize = |value: i32| {
        let shifted = value >> 4;
        shifted.div_euclid(3) * 48 + 24
    };
    let anchor = (normalize(request.x), normalize(request.y));
    let world_max = (before.map.world.xs * 0x300, before.map.world.ys * 0x300);
    let angle = UNIT_INIT_SET_ANGLE;
    let crew_at = |base: (i32, i32)| {
        let (dx, dy) = crew_track;
        let mut x = base.0;
        let mut y = base.1;
        if dx != 0 {
            x = x.wrapping_add(don_sim::systems::groups_guys::sinx(
                angle.wrapping_add(0x4000_0000),
                dx,
            ));
            y = y.wrapping_add(don_sim::systems::groups_guys::sinx(angle, dx));
        }
        if dy != 0 {
            x = x.wrapping_add(don_sim::systems::groups_guys::sinx(
                angle.wrapping_add(i32::MIN),
                dy,
            ));
            y = y.wrapping_add(don_sim::systems::groups_guys::sinx(
                angle.wrapping_add(0x4000_0000),
                dy,
            ));
        }
        if dx != 0 || dy != 0 {
            x = x.clamp(0, world_max.0 - 1);
            y = y.clamp(0, world_max.1 - 1);
        }
        (x, y)
    };
    let pre_crew = crew_at((-1_536, -1_536));
    let final_crew = crew_at(anchor);
    let unit = |ordinal, x, y, returned_z| TerrainHeightReceipt {
        ordinal,
        call_va: UNIT_TERRAIN_Z_CALL_VA,
        body_va: TERRAIN_FIND_TCOORD_Z_VA,
        kind: TerrainQueryKind::UnitTcoord,
        x,
        y,
        final_arg: 1,
        returned_z,
    };
    let guy = |ordinal, x, y, returned_z| TerrainHeightReceipt {
        ordinal,
        call_va: GUY_TERRAIN_Z_CALL_VA,
        body_va: TERRAIN_FIND_DATA_Z_VA,
        kind: TerrainQueryKind::GuyCoord,
        x,
        y,
        final_arg: 0,
        returned_z,
    };
    vec![
        unit(0, anchor.0 / 192, anchor.1 / 192, 10),
        guy(1, pre_crew.0, pre_crew.1, 11),
        guy(2, anchor.0, anchor.1, 12),
        guy(3, final_crew.0, final_crew.1, 13),
    ]
}

fn first_scout_location_inputs(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    before: &Sim,
    authority: &FirstFarmSetupEntryAuthority,
) -> FirstFarmFirstScoutLocationInputs {
    let graphics = vec![
        synthetic_first_scout_graphics(0),
        synthetic_first_scout_graphics(1),
    ];
    let terrain = first_scout_terrain(
        replay,
        plan,
        before,
        authority,
        (
            graphics[1].extracted.track_dx,
            graphics[1].extracted.track_dy,
        ),
    );
    FirstFarmFirstScoutLocationInputs {
        graphics,
        predicates: vec![
            synthetic_first_scout_predicates(),
            synthetic_first_scout_predicates(),
        ],
        terrain,
    }
}

fn tail_scalar(field: UnitTailScalarField, returned: i32) -> UnitTailScalarReceipt {
    let (call_va, body_va) = match field {
        UnitTailScalarField::MyHits => (UNIT_UPDATE_HITS_CALL_VA, UNIT_UPDATE_HITS_VA),
        UnitTailScalarField::MyLos => (UNIT_UPDATE_LOS_CALL_VA, UNIT_UPDATE_LOS_VA),
        UnitTailScalarField::MySpeed => (UNIT_UPDATE_SPEED_CALL_VA, UNIT_UPDATE_SPEED_VA),
        UnitTailScalarField::MyArmor => (UNIT_UPDATE_ARMOR_CALL_VA, UNIT_UPDATE_ARMOR_VA),
    };
    UnitTailScalarReceipt {
        call_va,
        body_va,
        field,
        returned,
    }
}

fn first_scout_collision_inputs(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    before: &Sim,
    authority: &FirstFarmSetupEntryAuthority,
) -> FirstFarmFirstScoutCollisionInputs {
    FirstFarmFirstScoutCollisionInputs {
        location: first_scout_location_inputs(replay, plan, before, authority),
        type_line: 0,
        is_1bf_strict: false,
        is_15f_strict: false,
        is_208_strict: false,
        is_3a: false,
        is_143: false,
        is_45: true,
        leader: UnitTailLeaderFacts::default(),
        stats: UnitTailStatReceipts {
            hits: tail_scalar(UnitTailScalarField::MyHits, 100),
            los: tail_scalar(UnitTailScalarField::MyLos, 5),
            speed: tail_scalar(UnitTailScalarField::MySpeed, 25),
            armor: tail_scalar(UnitTailScalarField::MyArmor, 0),
        },
        mana: ManaCapacityInput {
            base_mana: 0,
            is_air: false,
            has_space_program: false,
            space_air_percent: 0,
            is_supply: false,
            supply_upgrade: 0,
            has_special_craft_bonus: false,
            is_general: false,
            special_craft_percent: 0,
        },
        unit_masks2_before_tail: 0,
        stance_before_tail: 0,
        object_flags: 1,
    }
}

fn first_scout_visibility_authority(
    canonical_world: &don_sim::systems::map_terrain::World,
    goods: &OilGoodRuntime,
    items: &Items,
    dynamic: &DynamicLeadersAuthority,
) -> FirstFarmFirstScoutVisibilityAuthority {
    let mut leaders = [RevealLeaderFacts::default(); 8];
    leaders[FIRST_OWNER as usize].leader_flags = 0x0080_0000;
    FirstFarmFirstScoutVisibilityAuthority {
        revision: 1,
        composition_digest: [0x6b; 32],
        source: FirstFarmFirstScoutVisibilitySource::CanonicalFreshUnitVisibilitySeam,
        canonical_world_checksum: canonical_world.checksum_sections(),
        goods_before: goods.clone(),
        items_before: items.clone(),
        dynamic_before: dynamic.clone(),
        visibility: SetupUnitVisibilityAuthority {
            los: UnitLosFacts {
                mylos: 5,
                ptolemy_count: 0,
                unit_role: 0,
                has_ptolemy_general: None,
                ptolemy_los_bonus: 0,
                the_ceo_count: 0,
                unit_is_siege: None,
                has_the_ceo_general: None,
                the_ceo_unit_los: 0,
            },
            leaders,
            object_links: Vec::new(),
            rare_goods: Vec::new(),
            type_avail_calls: Vec::new(),
        },
    }
}

fn install_first_scout_tail_after_image(
    after: &mut Sim,
    tail: &don_replay::unit_init_collision_tail_deep_re::UnitPostLocationStateReceipt,
) {
    let row = after.world.unit_row_at(i32::from(FIRST_OWNER), 0).unwrap();
    let units = &mut after.world.units;
    units.collide_frame_mut()[row] = tail.collide_frame;
    units.collide_mut()[row] = tail.collide;
    units.collide_o_mut()[row] = tail.collide_o;
    units.collide_guy_mut()[row] = tail.collide_guy;
    units.collide_who_mut()[row] = tail.collide_who;
    units.o_up_mut()[row] = tail.o_up;
    units.o_down_mut()[row] = tail.o_down;
    units.cavarch_o_mut()[row] = tail.cavarch_o;
    units.cavarch_uid_mut()[row] = tail.cavarch_uid as i16;
    units.cavarch_who_mut()[row] = tail.cavarch_who;
    units.play_mut()[row] = tail.play;
    units.avoid_x_mut()[row] = tail.avoid_x;
    units.avoid_y_mut()[row] = tail.avoid_y;
    units.start_dist_mut()[row] = tail.start_dist;
    units.avoid_land_mut()[row] = tail.avoid_land;
    units.avoid_sea_mut()[row] = tail.avoid_sea;
    units.announce_frame_mut()[row] = tail.announce_frame;
    units.myhits_mut()[row] = tail.myhits;
    units.mylos_mut()[row] = tail.mylos;
    units.myspeed_mut()[row] = tail.myspeed;
    units.myarmor_mut()[row] = tail.myarmor;
    units.spell_time_mut()[row] = tail.spell_time;
    units.stance_mut()[row] = tail.stance;
    units.set_unit_masks(row, tail.unit_masks);
    units.set_unit_masks2(row, tail.unit_masks2);
}

fn prepare_complete_first_scout_after_image(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    before: &Sim,
    placement_authority: &FirstFarmSetupEntryAuthority,
    detailed: &mut DetailedInitUnitReceipt,
    after: &mut Sim,
    init_authority: &mut FirstFarmFirstInitUnitAuthority,
) -> (
    don_sim::systems::map_terrain::World,
    don_sim::systems::map_terrain::World,
    FirstFarmFirstScoutCollisionAuthority,
) {
    let seam_world = after.map.world.clone();
    let canonical_world = seam_world.clone();
    let collision_authority = FirstFarmFirstScoutCollisionAuthority {
        revision: 1,
        composition_digest: [0x68; 32],
        source: FirstFarmFirstScoutCollisionSource::CanonicalUnitInitLocationSeam,
        world_checksum_before: seam_world.checksum_sections(),
    };
    let mut final_world = seam_world.clone();
    let mut goods = OilGoodRuntime::default();
    let mut items = Items::new();
    let mut dynamic = DynamicLeadersAuthority::default();
    let visibility_authority =
        first_scout_visibility_authority(&canonical_world, &goods, &items, &dynamic);
    let completed = produce_first_farm_first_scout_visibility(
        replay,
        plan,
        before,
        placement_authority,
        detailed,
        after,
        init_authority,
        &mut final_world,
        &canonical_world,
        &mut goods,
        &mut items,
        &mut dynamic,
        &collision_authority,
        first_scout_collision_inputs(replay, plan, before, placement_authority),
        &visibility_authority,
    )
    .unwrap();

    after.map.world = final_world;
    install_first_scout_tail_after_image(after, &completed.collision.tail.unit);
    let InitUnitStep::UnitInit(unit) = &mut detailed.steps[1] else {
        unreachable!()
    };
    unit.after.unit_masks = completed.collision.tail.unit.unit_masks;
    init_authority.map_checksum_after = after.map.world.checksum_sections();
    (seam_world, canonical_world, collision_authority)
}

fn synthetic_complete_first_scout(
    replay: &Replay,
    plan: &BuildUnitsPlan,
) -> (FirstFarmFirstScoutCompleteInitReceipt, Sim) {
    let (before, placement_authority, mut detailed, mut after, mut init_authority) =
        synthetic_first_scout_init(replay, plan);
    let (mut world, canonical_world, collision_authority) =
        prepare_complete_first_scout_after_image(
            replay,
            plan,
            &before,
            &placement_authority,
            &mut detailed,
            &mut after,
            &mut init_authority,
        );
    let mut goods = OilGoodRuntime::default();
    let mut items = Items::new();
    let mut dynamic = DynamicLeadersAuthority::default();
    let visibility_authority =
        first_scout_visibility_authority(&canonical_world, &goods, &items, &dynamic);
    let receipt = produce_first_farm_first_scout_complete_init(
        replay,
        plan,
        &before,
        &placement_authority,
        &detailed,
        &after,
        &init_authority,
        &mut world,
        &canonical_world,
        &mut goods,
        &mut items,
        &mut dynamic,
        &collision_authority,
        first_scout_collision_inputs(replay, plan, &before, &placement_authority),
        &visibility_authority,
    )
    .unwrap();
    assert_eq!(
        world.checksum_sections(),
        after.map.world.checksum_sections()
    );
    (receipt, after)
}

fn synthetic_first_citizen_init(
    replay: &Replay,
    plan: &BuildUnitsPlan,
) -> (
    FirstFarmFirstScoutCompleteInitReceipt,
    Sim,
    DetailedInitUnitReceipt,
    Sim,
    FirstFarmFirstCitizenInitAuthority,
) {
    let (scout, after_scout) = synthetic_complete_first_scout(replay, plan);
    let placement =
        produce_first_farm_first_citizen_placement(replay, plan, &scout, &after_scout).unwrap();
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) =
        placement.placement.first_external_residual
    else {
        unreachable!()
    };
    let (same_scout, mut after_citizen) = synthetic_complete_first_scout(replay, plan);
    assert_eq!(same_scout, scout);
    after_citizen
        .spawn_unit(
            request.owner as usize,
            request.type_index,
            request.x,
            request.y,
            1,
        )
        .unwrap();
    let row = after_citizen.world.unit_row_at(request.owner, 1).unwrap();
    after_citizen.world.set_pos(
        row,
        don_sim::systems::objects_init_unit_authority_frontier::normalize_unit_init_coordinate(
            request.x,
        ),
        don_sim::systems::objects_init_unit_authority_frontier::normalize_unit_init_coordinate(
            request.y,
        ),
    );
    after_citizen.world.units.angle_mut()[row] = UNIT_INIT_SET_ANGLE;
    let discovery = discover_first_2018_farm(replay).unwrap();
    after_citizen.world.units.form_mut()[row] = discovery.citizen.base_form as i8;
    after_citizen
        .world
        .units
        .set_unit_masks(row, discovery.citizen.obj_masks);
    let mut rng = Random::new(placement.placement.rng_after_probes);
    rng.get(0, 0xffff);
    let rng_after = rng.state();
    after_citizen.world.random.reseed(rng_after);
    let detailed =
        first_citizen_detailed_receipt(request, &after_citizen, discovery.citizen.new_block_radius);
    let authority = FirstFarmFirstCitizenInitAuthority {
        revision: 1,
        composition_digest: [0x71; 32],
        source: FirstFarmFirstCitizenInitSource::CompleteRetailBodyAndCanonicalAfterImage,
        map_checksum_after: after_citizen.map.world.checksum_sections(),
        rng_after,
    };
    (scout, after_scout, detailed, after_citizen, authority)
}

fn first_citizen_location_inputs(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    scout: &FirstFarmFirstScoutCompleteInitReceipt,
    after_scout: &Sim,
) -> FirstFarmFirstCitizenLocationInputs {
    let placement =
        produce_first_farm_first_citizen_placement(replay, plan, scout, after_scout).unwrap();
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) =
        placement.placement.first_external_residual
    else {
        unreachable!()
    };
    let anchor = (
        don_sim::systems::objects_init_unit_authority_frontier::normalize_unit_init_coordinate(
            request.x,
        ),
        don_sim::systems::objects_init_unit_authority_frontier::normalize_unit_init_coordinate(
            request.y,
        ),
    );
    FirstFarmFirstCitizenLocationInputs {
        graphics: vec![synthetic_first_scout_graphics(0)],
        predicates: vec![synthetic_first_scout_predicates()],
        terrain: vec![
            TerrainHeightReceipt {
                ordinal: 0,
                call_va: UNIT_TERRAIN_Z_CALL_VA,
                body_va: TERRAIN_FIND_TCOORD_Z_VA,
                kind: TerrainQueryKind::UnitTcoord,
                x: anchor.0 / 192,
                y: anchor.1 / 192,
                final_arg: 1,
                returned_z: 20,
            },
            TerrainHeightReceipt {
                ordinal: 1,
                call_va: GUY_TERRAIN_Z_CALL_VA,
                body_va: TERRAIN_FIND_DATA_Z_VA,
                kind: TerrainQueryKind::GuyCoord,
                x: anchor.0,
                y: anchor.1,
                final_arg: 0,
                returned_z: 21,
            },
        ],
    }
}

fn synthetic_first_scout_predicates() -> GuyInitPredicateFacts {
    GuyInitPredicateFacts {
        valid_animation: [true, false, false, true],
        has_pivot_restrictions: false,
        animation_22_loaded: false,
        unit_flags2_bit_4: false,
        type_is_0x20: false,
        type_is_0x1000: false,
        unit_flags_bit_0x10: false,
        unit_flags_bit_0x2: false,
        air_predicate: false,
        base_type_is_52_or_53: false,
    }
}

#[test]
fn first_real_farm_advances_to_the_exact_setup_and_runtime_boundary() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let discovered = discover_first_2018_farm(&replay).unwrap();

    assert_eq!(discovered.replay_file_sha256, STRICT_REPLAY_SHA256);
    assert_eq!(
        (
            discovered.package.lockstep_serial,
            discovered.package.package_frame,
            discovered.package.play,
            discovered.package.who,
            discovered.package.objects.as_slice(),
        ),
        (
            FIRST_SERIAL,
            FIRST_FRAME,
            FIRST_PLAY,
            FIRST_OWNER,
            &[FIRST_SELECTED_O][..],
        )
    );
    assert_eq!(discovered.package.command_index, 0);
    assert_eq!(discovered.package.shell_prefix, []);
    assert_eq!(discovered.package.shell_suffix, [0x4a, 0x48]);
    assert_eq!(
        (
            discovered.player_slot,
            discovered.tribe_index,
            discovered.tribe.tribe_id,
            discovered.scout.type_index,
            discovered.citizen.type_index,
        ),
        (1, 14, 14, 69, 50)
    );
    assert_eq!(
        (
            discovered.scout.squad_size,
            discovered.scout.crew_size,
            discovered.scout.uber_size,
            discovered.citizen.squad_size,
            discovered.citizen.crew_size,
            discovered.citizen.uber_size,
        ),
        (1, 1, 1, 1, 0, 1)
    );

    assert_eq!(
        (
            discovered.center_build_o,
            discovered.center_position,
            discovered.center_world_cell,
            discovered.reconstructed_center_region,
            discovered.expected_center_region,
            discovered.city_slot
        ),
        (2_000, (21_600, 64_608), (28, 84), 64, 1, 0)
    );
    assert_eq!(
        (
            discovered.builder_schedule.base_scout_calls,
            discovered.builder_schedule.citizen_calls,
            discovered.builder_schedule.selected_o,
            discovered.builder_schedule.selected_call_ordinal,
            discovered.builder_schedule.selected_citizen_index,
        ),
        (1, 4, 4, 4, 3)
    );
    assert!(!discovered.builder_schedule.allocation_receipts_bound);
    assert!(!discovered.builder_schedule.current_upgrade_bound);

    assert_eq!((discovered.farm.x_size, discovered.farm.y_size), (4, 4));
    assert_eq!(discovered.geometry.requested, (22_286, 66_184));
    assert_eq!(discovered.geometry.requested_second, (22_286, 66_184));
    assert_eq!(discovered.geometry.corner_tcoord, (114, 342));
    assert_eq!(discovered.geometry.snapped, (22_272, 66_048));
    assert_eq!(discovered.geometry.world_cell, (29, 86));

    assert!(discovered.recorded_groups_matches_pre_issue_state());
    assert_eq!(discovered.recorded_groups_checksum, 0x1c78_f3f5);
    assert_eq!(discovered.recorded_units_checksum, 0x2bc4_5014);
    assert!(!discovered.runtime_authority_ready());
    assert_eq!(
        discovered.blockers,
        [
            FirstFarmAuthorityBlocker::PostWorldgenRandomState,
            FirstFarmAuthorityBlocker::SetupPlacementWorldSnapshot,
            FirstFarmAuthorityBlocker::ObjectsInitUnitReceipts,
            FirstFarmAuthorityBlocker::Frame79UnitHandleAndState,
            FirstFarmAuthorityBlocker::InterveningFrameChronology,
            FirstFarmAuthorityBlocker::Frame79CityAfterImage,
            FirstFarmAuthorityBlocker::Frame79WorldObjectHead,
            FirstFarmAuthorityBlocker::ValidateBuildProbeChronology,
            FirstFarmAuthorityBlocker::ObjectsInitBuildAfterImage,
            FirstFarmAuthorityBlocker::BuilderSwarmSearchAfterImage,
        ]
    );

    eprintln!(
        "first Farm facts: Farm={:#?} Units={:08x} blockers={:?}",
        discovered.farm, discovered.recorded_units_checksum, discovered.blockers
    );
}

#[test]
fn in_memory_wire_or_rules_mutation_cannot_be_promoted_to_first_packet_authority() {
    let path = replay_path();
    let mut replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let action = replay
        .turns
        .iter_mut()
        .find(|turn| turn.turn == FIRST_SERIAL)
        .unwrap()
        .players
        .iter_mut()
        .find(|player| player.play == FIRST_PLAY as i32)
        .unwrap()
        .commands
        .iter_mut()
        .find(|command| command.opcode == 25)
        .unwrap();
    action.bytes[1] ^= 1;
    assert_eq!(
        discover_first_2018_farm(&replay).unwrap_err(),
        FirstFarmAuthorityError::WrongFirstPackage
    );

    let replay = Replay::open(&path).unwrap();
    let rules = replay.initial.rules.unwrap();
    let mut payload = load_payload(&path).unwrap();
    let farm = replay_build_type_facts(&payload, &rules, 0x1a1).unwrap();
    payload[farm.spans.object.offset + (0x234 - 0x1e4)] ^= 1;
    assert_eq!(
        replay_build_type_facts(&payload, &rules, 0x1a1),
        Err(PrePairUnitAuthorityError::RulesSha256Mismatch)
    );
}

#[test]
fn canonical_setup_entry_produces_the_first_real_placement_probe_boundary() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (sim, authority) = synthetic_setup_entry(&replay);

    let receipt = produce_first_farm_first_placement(&replay, &plan, &sim, &authority).unwrap();
    assert_eq!(receipt.setup_ordinal, 0);
    assert_eq!(receipt.center_row, 0);
    assert_eq!(receipt.post_worldgen_rng, 0x12345);
    assert_eq!(receipt.map_checksum, authority.world_checksum);
    assert_ne!(receipt.placement_snapshot_sha256, [0; 32]);
    assert_eq!(receipt.placement.inputs.upgraded_type, 69);
    assert_eq!(receipt.placement.anchor_coord, (21_600, 64_608));
    assert_eq!(receipt.placement.radius, 2);
    assert_eq!(receipt.placement.probes.len(), 1);
    assert_eq!(
        receipt.placement.probes[0].disposition,
        ProbeDisposition::Accepted
    );
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) =
        receipt.placement.first_external_residual
    else {
        panic!("all-land fixture must reach Objects::init_unit")
    };
    assert_eq!((request.owner, request.type_index), (0, 69));
    assert_eq!(
        (
            request.exact_o,
            request.external_previous,
            request.external_next
        ),
        (-1, -1, -1)
    );
}

#[test]
fn first_placement_refuses_unbound_map_rng_center_and_allocation_chronology() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (mut sim, authority) = synthetic_setup_entry(&replay);

    let mut missing_revision = authority.clone();
    missing_revision.revision = 0;
    assert_eq!(
        produce_first_farm_first_placement(&replay, &plan, &sim, &missing_revision),
        Err(FirstFarmFirstPlacementError::MissingAuthorityRevision)
    );

    let mut wrong_checksum = authority.clone();
    wrong_checksum.world_checksum.full ^= 1;
    assert_eq!(
        produce_first_farm_first_placement(&replay, &plan, &sim, &wrong_checksum),
        Err(FirstFarmFirstPlacementError::WorldChecksumMismatch)
    );

    let mut wrong_rng = authority.clone();
    wrong_rng.post_worldgen_rng ^= 1;
    assert_eq!(
        produce_first_farm_first_placement(&replay, &plan, &sim, &wrong_rng),
        Err(FirstFarmFirstPlacementError::PostWorldgenRngMismatch)
    );

    sim.world.frame = 1;
    assert_eq!(
        produce_first_farm_first_placement(&replay, &plan, &sim, &authority),
        Err(FirstFarmFirstPlacementError::WrongFrame {
            expected: 0,
            actual: 1,
        })
    );
    sim.world.frame = 0;

    sim.production_runtime.build_types[0] = Some(CITY_CENTER_TYPE + 1);
    assert_eq!(
        produce_first_farm_first_placement(&replay, &plan, &sim, &authority),
        Err(FirstFarmFirstPlacementError::CenterBuildMismatch)
    );
    sim.production_runtime.build_types[0] = Some(CITY_CENTER_TYPE);

    sim.spawn_unit(FIRST_OWNER as usize, 69, 100, 100, 1)
        .unwrap();
    sim.world.random.reseed(authority.post_worldgen_rng);
    assert_eq!(
        produce_first_farm_first_placement(&replay, &plan, &sim, &authority),
        Err(FirstFarmFirstPlacementError::ExistingOwnerUnits { mark: 1 })
    );
}

#[test]
fn complete_first_scout_init_receipt_binds_native_o0_to_the_canonical_handle() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, after, authority) =
        synthetic_first_scout_init(&replay, &plan);

    let receipt = bind_first_farm_first_init_unit(
        &replay,
        &plan,
        &before,
        &placement_authority,
        &detailed,
        &after,
        &authority,
    )
    .unwrap();
    assert_eq!(receipt.effects.initialized_members, [0]);
    assert_eq!(
        (
            receipt.effects.unit_mark_before,
            receipt.effects.unit_mark_after
        ),
        (0, 1)
    );
    assert_eq!((receipt.allocation.owner, receipt.allocation.o), (0, 0));
    assert_eq!(receipt.allocation.id, receipt.unit.identity.handle.id);
    assert_eq!(
        receipt.allocation.generation,
        receipt.unit.identity.handle.generation
    );
    assert_eq!(
        (
            receipt.row,
            receipt.unit.identity.who,
            receipt.unit.identity.o
        ),
        (0, 0, 0)
    );
    assert_eq!((receipt.request.owner, receipt.request.type_index), (0, 69));
    assert_eq!(receipt.rng_after, authority.rng_after);
    assert!(receipt.unit.orders.is_empty());
    assert!(receipt.unit.path.is_empty());
}

#[test]
fn first_scout_init_join_rejects_extent_request_rng_type_and_mark_mutations() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, mut after, authority) =
        synthetic_first_scout_init(&replay, &plan);

    let mut missing_revision = authority.clone();
    missing_revision.revision = 0;
    assert_eq!(
        bind_first_farm_first_init_unit(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &missing_revision,
        ),
        Err(FirstFarmFirstInitUnitError::MissingAuthorityRevision)
    );

    let mut wrong_request = detailed.clone();
    wrong_request.request.x += 1;
    assert_eq!(
        bind_first_farm_first_init_unit(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &wrong_request,
            &after,
            &authority,
        ),
        Err(FirstFarmFirstInitUnitError::InitRequestMismatch)
    );

    let mut wrong_extent = detailed.clone();
    let InitUnitStep::UnitInit(init) = &mut wrong_extent.steps[1] else {
        unreachable!()
    };
    init.extent = CompleteBody::SetNewLocation1757Bytes;
    assert_eq!(
        bind_first_farm_first_init_unit(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &wrong_extent,
            &after,
            &authority,
        ),
        Err(FirstFarmFirstInitUnitError::DetailedReceipt(
            InitUnitReceiptError::InvalidUnitInit
        ))
    );

    let mut wrong_captain = detailed.clone();
    let InitUnitStep::ResolveCaptain(captain) = wrong_captain.steps.last_mut().unwrap() else {
        unreachable!()
    };
    captain.captain.x += 1;
    assert_eq!(
        bind_first_farm_first_init_unit(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &wrong_captain,
            &after,
            &authority,
        ),
        Err(FirstFarmFirstInitUnitError::CanonicalAfterImageMismatch)
    );

    after.world.random.reseed(authority.rng_after ^ 1);
    assert_eq!(
        bind_first_farm_first_init_unit(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &authority,
        ),
        Err(FirstFarmFirstInitUnitError::AfterRngMismatch)
    );
    after.world.random.reseed(authority.rng_after);

    after.unit_type[0] = 70;
    assert_eq!(
        bind_first_farm_first_init_unit(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &authority,
        ),
        Err(FirstFarmFirstInitUnitError::CanonicalAfterImageMismatch)
    );
    after.unit_type[0] = 69;

    after.spawn_unit(0, 50, 100, 100, 1).unwrap();
    after.world.random.reseed(authority.rng_after);
    assert_eq!(
        bind_first_farm_first_init_unit(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &authority,
        ),
        Err(FirstFarmFirstInitUnitError::WrongAfterUnitMark {
            expected: 1,
            actual: 2,
        })
    );
}

#[test]
fn first_scout_guy_prefix_binds_exact_rules_identity_graphics_and_rng() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, after, authority) =
        synthetic_first_scout_init(&replay, &plan);
    let receipt = produce_first_farm_first_scout_guy_prefix(
        &replay,
        &plan,
        &before,
        &placement_authority,
        &detailed,
        &after,
        &authority,
        vec![
            synthetic_first_scout_graphics(0),
            synthetic_first_scout_graphics(1),
        ],
        vec![
            synthetic_first_scout_predicates(),
            synthetic_first_scout_predicates(),
        ],
    )
    .unwrap();

    assert_eq!((receipt.squad_size, receipt.crew_size), (1, 1));
    assert_eq!(receipt.prefix.array_length, 2);
    assert_eq!(receipt.prefix.guy_mark, 1);
    assert_eq!(receipt.prefix.rng_initial, receipt.init.rng_before);
    assert_eq!(receipt.prefix.rng_after_guys, receipt.init.rng_after);
    assert_eq!(receipt.prefix.stable_guys.len(), 2);
    assert_eq!(
        (
            receipt.prefix.identity.id,
            receipt.prefix.identity.generation,
            receipt.prefix.identity.owner,
            receipt.prefix.identity.o,
            receipt.prefix.identity.type_index,
        ),
        (
            receipt.init.allocation.id,
            receipt.init.allocation.generation,
            0,
            0,
            69,
        )
    );
    let guy = receipt.prefix.guys.guys[0].as_ref().unwrap();
    assert_eq!((guy.ty, guy.who, guy.o, guy.guy_num), (69, 0, 0, 0));
    assert_eq!(guy.gpiece, 100);
    let crew = receipt.prefix.guys.guys[1].as_ref().unwrap();
    assert_eq!((crew.ty, crew.who, crew.o, crew.guy_num), (69, 0, 0, 1));
    assert_eq!(crew.gpiece, 101);
}

#[test]
fn first_scout_guy_prefix_rejects_unbound_graphics_and_full_body_rng_disagreement() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, mut after, mut authority) =
        synthetic_first_scout_init(&replay, &plan);

    let mut graphics = synthetic_first_scout_graphics(0);
    graphics.provenance.coherent_capture = false;
    assert_eq!(
        produce_first_farm_first_scout_guy_prefix(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &authority,
            vec![graphics, synthetic_first_scout_graphics(1)],
            vec![
                synthetic_first_scout_predicates(),
                synthetic_first_scout_predicates(),
            ],
        ),
        Err(FirstFarmFirstScoutGuyError::Guy(
            UnitGuyInitError::IncoherentGraphicsCapture { slot: 0 }
        ))
    );

    let prefix_after = authority.rng_after;
    authority.rng_after ^= 1;
    after.world.random.reseed(authority.rng_after);
    let error = produce_first_farm_first_scout_guy_prefix(
        &replay,
        &plan,
        &before,
        &placement_authority,
        &detailed,
        &after,
        &authority,
        vec![
            synthetic_first_scout_graphics(0),
            synthetic_first_scout_graphics(1),
        ],
        vec![
            synthetic_first_scout_predicates(),
            synthetic_first_scout_predicates(),
        ],
    )
    .unwrap_err();
    assert_eq!(
        error,
        FirstFarmFirstScoutGuyError::InitializerRngMismatch {
            prefix_after,
            init_after: authority.rng_after,
        }
    );
}

#[test]
fn first_scout_location_composes_rules_graphics_terrain_and_canonical_after_image() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, after, authority) =
        synthetic_first_scout_init(&replay, &plan);
    let graphics = vec![
        synthetic_first_scout_graphics(0),
        synthetic_first_scout_graphics(1),
    ];
    let terrain = first_scout_terrain(
        &replay,
        &plan,
        &before,
        &placement_authority,
        (
            graphics[1].extracted.track_dx,
            graphics[1].extracted.track_dy,
        ),
    );
    let receipt = produce_first_farm_first_scout_location(
        &replay,
        &plan,
        &before,
        &placement_authority,
        &detailed,
        &after,
        &authority,
        FirstFarmFirstScoutLocationInputs {
            graphics,
            predicates: vec![
                synthetic_first_scout_predicates(),
                synthetic_first_scout_predicates(),
            ],
            terrain,
        },
    )
    .unwrap();

    assert_ne!(receipt.raw_request, receipt.normalized_anchor);
    assert_eq!(
        receipt.normalized_anchor,
        (receipt.guy.init.unit.x, receipt.guy.init.unit.y)
    );
    assert_eq!(receipt.location.unit.angle, UNIT_INIT_SET_ANGLE);
    assert_eq!(
        receipt.location.unit.formation,
        receipt.unit_type.base_form as i8
    );
    assert_eq!(receipt.unit_type.squad_size, 1);
    assert_eq!(receipt.unit_type.crew_size, 1);
    assert_eq!(receipt.location.graphics_calls.len(), 2);
    assert_eq!(receipt.location.collision_requests.len(), 1);
    assert_eq!(
        receipt.location.first_unapplied_shared_mutation,
        receipt.location.collision_requests.first().copied()
    );
    assert_eq!(receipt.location.rng_before, receipt.location.rng_after);
    assert_eq!(
        receipt.location.rng_before,
        receipt.guy.prefix.rng_after_guys
    );
}

#[test]
fn first_scout_location_rejects_terrain_and_canonical_after_image_mutations() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, mut after, authority) =
        synthetic_first_scout_init(&replay, &plan);
    let graphics = vec![
        synthetic_first_scout_graphics(0),
        synthetic_first_scout_graphics(1),
    ];
    let mut terrain = first_scout_terrain(
        &replay,
        &plan,
        &before,
        &placement_authority,
        (
            graphics[1].extracted.track_dx,
            graphics[1].extracted.track_dy,
        ),
    );
    terrain[1].x += 1;
    assert_eq!(
        produce_first_farm_first_scout_location(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &authority,
            FirstFarmFirstScoutLocationInputs {
                graphics: graphics.clone(),
                predicates: vec![
                    synthetic_first_scout_predicates(),
                    synthetic_first_scout_predicates(),
                ],
                terrain,
            },
        ),
        Err(FirstFarmFirstScoutLocationError::Location(
            UnitInitLocationError::TerrainReceiptMismatch { ordinal: 1 }
        ))
    );

    let row = after.world.unit_row_at(FIRST_OWNER.into(), 0).unwrap();
    after.world.units.angle_mut()[row] ^= 1;
    let terrain = first_scout_terrain(
        &replay,
        &plan,
        &before,
        &placement_authority,
        (
            graphics[1].extracted.track_dx,
            graphics[1].extracted.track_dy,
        ),
    );
    let mut mismatched = detailed.clone();
    let InitUnitStep::UnitInit(unit) = &mut mismatched.steps[1] else {
        unreachable!()
    };
    unit.after.angle ^= 1;
    let InitUnitStep::ResolveCaptain(captain) = mismatched.steps.last_mut().unwrap() else {
        unreachable!()
    };
    captain.captain.angle ^= 1;
    assert_eq!(
        produce_first_farm_first_scout_location(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &mismatched,
            &after,
            &authority,
            FirstFarmFirstScoutLocationInputs {
                graphics,
                predicates: vec![
                    synthetic_first_scout_predicates(),
                    synthetic_first_scout_predicates(),
                ],
                terrain,
            },
        ),
        Err(FirstFarmFirstScoutLocationError::CanonicalLocationMismatch)
    );
}

#[test]
fn first_scout_collision_tail_commits_canonical_blocks_and_stops_at_visibility() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, after, init_authority) =
        synthetic_first_scout_init(&replay, &plan);
    let mut collision_world = after.map.world.clone();
    let collision_authority = FirstFarmFirstScoutCollisionAuthority {
        revision: 1,
        composition_digest: [0x68; 32],
        source: FirstFarmFirstScoutCollisionSource::CanonicalUnitInitLocationSeam,
        world_checksum_before: collision_world.checksum_sections(),
    };
    let receipt = produce_first_farm_first_scout_collision_tail(
        &replay,
        &plan,
        &before,
        &placement_authority,
        &detailed,
        &after,
        &init_authority,
        &mut collision_world,
        &collision_authority,
        first_scout_collision_inputs(&replay, &plan, &before, &placement_authority),
    )
    .unwrap();

    assert_eq!(receipt.authority_digest, [0x68; 32]);
    assert_eq!(receipt.tail.collision_requests.len(), 1);
    assert!(!receipt.tail.collision_deltas.is_empty());
    assert_eq!(
        receipt.world_checksum_after_collision,
        collision_world.checksum_sections()
    );
    assert_ne!(
        receipt.world_checksum_before.full,
        receipt.world_checksum_after_collision.full
    );
    assert_eq!(receipt.tail.unit.myhits, 100);
    assert_eq!(receipt.tail.unit.mylos, 5);
    assert_eq!(receipt.tail.unit.myspeed, 25);
    assert_eq!(receipt.tail.unit.myarmor, 0);
    let UnitInitTailExternalResidual::ObjectUpdateSeen(visibility) =
        receipt.tail.next_external_residual;
    assert_eq!(visibility.body_va, OBJECT_UPDATE_SEEN_VA);
    assert_eq!((visibility.owner, visibility.o), (FIRST_OWNER, 0));
    assert_eq!(
        (visibility.x, visibility.y),
        receipt.location.normalized_anchor
    );
    assert_eq!(visibility.source_mylos, 5);
}

#[test]
fn first_scout_collision_tail_is_atomic_on_world_and_scalar_authority_failures() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, after, init_authority) =
        synthetic_first_scout_init(&replay, &plan);
    let mut collision_world = after.map.world.clone();
    let pristine = collision_world.checksum_sections();
    let authority = FirstFarmFirstScoutCollisionAuthority {
        revision: 1,
        composition_digest: [0x68; 32],
        source: FirstFarmFirstScoutCollisionSource::CanonicalUnitInitLocationSeam,
        world_checksum_before: pristine.clone(),
    };

    let mut wrong_world = authority.clone();
    wrong_world.world_checksum_before.full ^= 1;
    assert_eq!(
        produce_first_farm_first_scout_collision_tail(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &init_authority,
            &mut collision_world,
            &wrong_world,
            first_scout_collision_inputs(&replay, &plan, &before, &placement_authority),
        ),
        Err(FirstFarmFirstScoutCollisionError::WorldChecksumMismatch)
    );
    assert_eq!(collision_world.checksum_sections(), pristine);

    let mut bad = first_scout_collision_inputs(&replay, &plan, &before, &placement_authority);
    bad.stats.los.body_va ^= 1;
    assert!(matches!(
        produce_first_farm_first_scout_collision_tail(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &init_authority,
            &mut collision_world,
            &authority,
            bad,
        ),
        Err(FirstFarmFirstScoutCollisionError::Tail(_))
    ));
    assert_eq!(collision_world.checksum_sections(), pristine);
}

#[test]
fn first_scout_visibility_atomically_composes_collision_fog_and_reveal() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, after, init_authority) =
        synthetic_first_scout_init(&replay, &plan);
    let mut world = after.map.world.clone();
    let canonical_world = world.clone();
    let mut goods = OilGoodRuntime::default();
    let mut items = Items::new();
    let mut dynamic = DynamicLeadersAuthority::default();
    let collision_authority = FirstFarmFirstScoutCollisionAuthority {
        revision: 1,
        composition_digest: [0x68; 32],
        source: FirstFarmFirstScoutCollisionSource::CanonicalUnitInitLocationSeam,
        world_checksum_before: world.checksum_sections(),
    };
    let visibility_authority =
        first_scout_visibility_authority(&canonical_world, &goods, &items, &dynamic);
    let receipt = produce_first_farm_first_scout_visibility(
        &replay,
        &plan,
        &before,
        &placement_authority,
        &detailed,
        &after,
        &init_authority,
        &mut world,
        &canonical_world,
        &mut goods,
        &mut items,
        &mut dynamic,
        &collision_authority,
        first_scout_collision_inputs(&replay, &plan, &before, &placement_authority),
        &visibility_authority,
    )
    .unwrap();

    assert_eq!(receipt.authority_digest, [0x6b; 32]);
    assert!(!receipt.collision.tail.collision_deltas.is_empty());
    assert!(!receipt.visibility.newly_explored.is_empty());
    assert_eq!(receipt.visibility.rng_before, receipt.visibility.rng_after);
    assert_eq!(
        receipt.visibility.next_external_residual,
        SetupVisibilityExternalResidual::None
    );
    assert_eq!(
        receipt.world_checksum_after_visibility,
        world.checksum_sections()
    );
    assert!(world.seen.iter().any(|&value| value != 0));
    assert_eq!(goods, visibility_authority.goods_before);
    assert_eq!(items, visibility_authority.items_before);
    assert_eq!(dynamic, visibility_authority.dynamic_before);
}

#[test]
fn first_scout_visibility_failure_rolls_back_the_prior_collision_stage() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, detailed, after, init_authority) =
        synthetic_first_scout_init(&replay, &plan);
    let mut world = after.map.world.clone();
    let canonical_world = world.clone();
    let world_before = world.clone();
    let mut goods = OilGoodRuntime::default();
    let mut items = Items::new();
    let mut dynamic = DynamicLeadersAuthority::default();
    let collision_authority = FirstFarmFirstScoutCollisionAuthority {
        revision: 1,
        composition_digest: [0x68; 32],
        source: FirstFarmFirstScoutCollisionSource::CanonicalUnitInitLocationSeam,
        world_checksum_before: world.checksum_sections(),
    };
    let mut visibility_authority =
        first_scout_visibility_authority(&canonical_world, &goods, &items, &dynamic);
    visibility_authority.visibility.leaders[FIRST_OWNER as usize].leader_flags ^= 1;

    assert!(matches!(
        produce_first_farm_first_scout_visibility(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &init_authority,
            &mut world,
            &canonical_world,
            &mut goods,
            &mut items,
            &mut dynamic,
            &collision_authority,
            first_scout_collision_inputs(&replay, &plan, &before, &placement_authority),
            &visibility_authority,
        ),
        Err(FirstFarmFirstScoutVisibilityError::Visibility(_))
    ));
    assert_eq!(world.checksum_sections(), world_before.checksum_sections());
    assert_eq!(world.seen, world_before.seen);
    assert_eq!(goods, visibility_authority.goods_before);
    assert_eq!(items, visibility_authority.items_before);
    assert_eq!(dynamic, visibility_authority.dynamic_before);
}

#[test]
fn first_scout_complete_init_emits_the_exact_first_setup_member_receipt() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, mut detailed, mut after, mut init_authority) =
        synthetic_first_scout_init(&replay, &plan);
    let (mut world, canonical_world, collision_authority) =
        prepare_complete_first_scout_after_image(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &mut detailed,
            &mut after,
            &mut init_authority,
        );
    let mut goods = OilGoodRuntime::default();
    let mut items = Items::new();
    let mut dynamic = DynamicLeadersAuthority::default();
    let visibility_authority =
        first_scout_visibility_authority(&canonical_world, &goods, &items, &dynamic);
    let receipt = produce_first_farm_first_scout_complete_init(
        &replay,
        &plan,
        &before,
        &placement_authority,
        &detailed,
        &after,
        &init_authority,
        &mut world,
        &canonical_world,
        &mut goods,
        &mut items,
        &mut dynamic,
        &collision_authority,
        first_scout_collision_inputs(&replay, &plan, &before, &placement_authority),
        &visibility_authority,
    )
    .unwrap();

    assert_eq!(receipt.effects.initialized_members, [0]);
    assert_eq!(receipt.effects.unit_mark_before, 0);
    assert_eq!(receipt.effects.unit_mark_after, 1);
    assert_eq!(receipt.canonical_after.unit_masks, 0x0206_8024);
    assert_eq!(receipt.unit_init.extent, CompleteBody::UnitInit3732Bytes);
    assert_eq!(receipt.setup_init.validated_body_va, OBJECTS_INIT_UNIT_VA);
    assert_eq!(
        receipt.setup_init.validated_body_bytes,
        OBJECTS_INIT_UNIT_BYTES
    );
    assert_eq!(receipt.setup_init.returned_captain_o, 0);
    assert_eq!(receipt.setup_init.members.len(), 1);
    let member = &receipt.setup_init.members[0];
    assert_eq!((member.identity.owner, member.identity.o), (0, 0));
    assert_eq!(member.ptype_index, 69);
    assert_eq!((member.path.length, member.path.capacity), (0, 10));
    assert_eq!((member.guys.length, member.guys.capacity), (2, 2));
    assert_eq!(member.guy_mark, 1);
    assert_eq!(member.guy_identities.len(), 2);
    assert_eq!(
        receipt.world_checksum_after,
        after.map.world.checksum_sections()
    );
    assert_eq!(
        world.checksum_sections(),
        after.map.world.checksum_sections()
    );
}

#[test]
fn complete_init_after_image_failure_rolls_back_collision_and_visibility() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (before, placement_authority, mut detailed, mut after, mut init_authority) =
        synthetic_first_scout_init(&replay, &plan);
    let (mut world, canonical_world, collision_authority) =
        prepare_complete_first_scout_after_image(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &mut detailed,
            &mut after,
            &mut init_authority,
        );
    let row = after.world.unit_row_at(i32::from(FIRST_OWNER), 0).unwrap();
    after.world.units.mylos_mut()[row] ^= 1;
    let world_before = world.clone();
    let mut goods = OilGoodRuntime::default();
    let mut items = Items::new();
    let mut dynamic = DynamicLeadersAuthority::default();
    let visibility_authority =
        first_scout_visibility_authority(&canonical_world, &goods, &items, &dynamic);

    assert_eq!(
        produce_first_farm_first_scout_complete_init(
            &replay,
            &plan,
            &before,
            &placement_authority,
            &detailed,
            &after,
            &init_authority,
            &mut world,
            &canonical_world,
            &mut goods,
            &mut items,
            &mut dynamic,
            &collision_authority,
            first_scout_collision_inputs(&replay, &plan, &before, &placement_authority),
            &visibility_authority,
        ),
        Err(FirstFarmFirstScoutCompleteInitError::CanonicalUnitTailMismatch)
    );
    assert_eq!(world.checksum_sections(), world_before.checksum_sections());
    assert_eq!(world.seen, world_before.seen);
    assert_eq!(goods, visibility_authority.goods_before);
    assert_eq!(items, visibility_authority.items_before);
    assert_eq!(dynamic, visibility_authority.dynamic_before);
}

#[test]
fn first_citizen_placement_chains_from_the_complete_scout_world_and_rng() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (scout, mut after_scout) = synthetic_complete_first_scout(&replay, &plan);
    let receipt =
        produce_first_farm_first_citizen_placement(&replay, &plan, &scout, &after_scout).unwrap();

    assert_eq!(receipt.setup_ordinal, 1);
    assert_eq!((receipt.prior_scout.owner, receipt.prior_scout.o), (0, 0));
    assert_eq!(receipt.rng_before, scout.rng_after);
    assert_eq!(receipt.map_checksum, scout.world_checksum_after);
    assert_eq!(receipt.placement.inputs.upgraded_type, 50);
    assert!(!receipt.placement.probes.is_empty());
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) =
        receipt.placement.first_external_residual
    else {
        panic!("first Citizen synthetic map must reach Objects::init_unit");
    };
    assert_eq!((request.owner, request.type_index), (0, 50));

    after_scout.world.random.reseed(scout.rng_after ^ 1);
    assert_eq!(
        produce_first_farm_first_citizen_placement(&replay, &plan, &scout, &after_scout),
        Err(FirstFarmFirstCitizenPlacementError::ScoutRngMismatch)
    );
}

#[test]
fn first_citizen_initializer_binds_o1_and_rejects_changed_scout_or_rng() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (scout, after_scout) = synthetic_complete_first_scout(&replay, &plan);
    let placement =
        produce_first_farm_first_citizen_placement(&replay, &plan, &scout, &after_scout).unwrap();
    let PlaceUnitExternalResidual::ObjectsInitUnit(request) =
        placement.placement.first_external_residual
    else {
        unreachable!()
    };
    let (same_scout, mut after_citizen) = synthetic_complete_first_scout(&replay, &plan);
    assert_eq!(same_scout, scout);
    let handle = after_citizen
        .spawn_unit(
            request.owner as usize,
            request.type_index,
            request.x,
            request.y,
            1,
        )
        .unwrap();
    let row = after_citizen.world.unit_row_at(request.owner, 1).unwrap();
    after_citizen.world.set_pos(
        row,
        don_sim::systems::objects_init_unit_authority_frontier::normalize_unit_init_coordinate(
            request.x,
        ),
        don_sim::systems::objects_init_unit_authority_frontier::normalize_unit_init_coordinate(
            request.y,
        ),
    );
    after_citizen.world.units.angle_mut()[row] = UNIT_INIT_SET_ANGLE;
    after_citizen.world.units.form_mut()[row] =
        discover_first_2018_farm(&replay).unwrap().citizen.base_form as i8;
    let mut rng = Random::new(placement.placement.rng_after_probes);
    rng.get(0, 0xffff);
    let rng_after = rng.state();
    after_citizen.world.random.reseed(rng_after);
    let discovery = discover_first_2018_farm(&replay).unwrap();
    let detailed =
        first_citizen_detailed_receipt(request, &after_citizen, discovery.citizen.new_block_radius);
    let authority = FirstFarmFirstCitizenInitAuthority {
        revision: 1,
        composition_digest: [0x71; 32],
        source: FirstFarmFirstCitizenInitSource::CompleteRetailBodyAndCanonicalAfterImage,
        map_checksum_after: after_citizen.map.world.checksum_sections(),
        rng_after,
    };
    let receipt = bind_first_farm_first_citizen_init(
        &replay,
        &plan,
        &scout,
        &after_scout,
        &detailed,
        &after_citizen,
        &authority,
    )
    .unwrap();
    assert_eq!(receipt.effects.initialized_members, [1]);
    assert_eq!(
        (
            receipt.effects.unit_mark_before,
            receipt.effects.unit_mark_after
        ),
        (1, 2)
    );
    assert_eq!(receipt.allocation.o, 1);
    assert_eq!(receipt.allocation.id, handle.id);
    assert_eq!(receipt.unit.identity.handle, handle);
    assert_eq!(receipt.rng_before, placement.placement.rng_after_probes);
    assert_eq!(receipt.rng_after, rng_after);

    after_citizen.world.random.reseed(rng_after ^ 1);
    assert_eq!(
        bind_first_farm_first_citizen_init(
            &replay,
            &plan,
            &scout,
            &after_scout,
            &detailed,
            &after_citizen,
            &authority,
        ),
        Err(FirstFarmFirstCitizenInitError::AfterRngMismatch)
    );
    after_citizen.world.random.reseed(rng_after);

    let scout_row = after_citizen
        .world
        .unit_row_at(i32::from(FIRST_OWNER), 0)
        .unwrap();
    after_citizen.world.units.mylos_mut()[scout_row] ^= 1;
    assert_eq!(
        bind_first_farm_first_citizen_init(
            &replay,
            &plan,
            &scout,
            &after_scout,
            &detailed,
            &after_citizen,
            &authority,
        ),
        Err(FirstFarmFirstCitizenInitError::PriorScoutChanged)
    );
}

#[test]
fn first_citizen_guy_and_location_bind_one_draw_and_two_terrain_queries() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (scout, after_scout, detailed, after_citizen, authority) =
        synthetic_first_citizen_init(&replay, &plan);
    let inputs = first_citizen_location_inputs(&replay, &plan, &scout, &after_scout);
    let receipt = produce_first_farm_first_citizen_location(
        &replay,
        &plan,
        &scout,
        &after_scout,
        &detailed,
        &after_citizen,
        &authority,
        inputs,
    )
    .unwrap();

    assert_eq!((receipt.guy.squad_size, receipt.guy.crew_size), (1, 0));
    assert_eq!(receipt.guy.prefix.stable_guys.len(), 1);
    assert_eq!(receipt.guy.prefix.random.len(), 1);
    assert_eq!(receipt.guy.prefix.rng_initial, receipt.guy.init.rng_before);
    assert_eq!(
        receipt.guy.prefix.rng_after_guys,
        receipt.guy.init.rng_after
    );
    assert_eq!(receipt.location.terrain.len(), 2);
    assert_eq!(receipt.location.graphics_calls.len(), 1);
    assert_eq!(receipt.location.collision_requests.len(), 1);
    assert_eq!(
        (receipt.location.unit.x, receipt.location.unit.y),
        receipt.normalized_anchor
    );
    assert_eq!(
        receipt.location.unit.formation,
        receipt.unit_type.base_form as i8
    );
    assert_eq!(
        receipt.location.unit.unit_masks,
        receipt.guy.init.unit.unit_masks
    );
}

#[test]
fn first_citizen_graphics_terrain_and_initializer_rng_are_fail_closed() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let plan = first_farm_plan();
    let (scout, after_scout, detailed, after_citizen, authority) =
        synthetic_first_citizen_init(&replay, &plan);

    let mut bad_graphics = first_citizen_location_inputs(&replay, &plan, &scout, &after_scout);
    bad_graphics.graphics[0].provenance.coherent_capture = false;
    assert_eq!(
        produce_first_farm_first_citizen_location(
            &replay,
            &plan,
            &scout,
            &after_scout,
            &detailed,
            &after_citizen,
            &authority,
            bad_graphics,
        ),
        Err(FirstFarmFirstCitizenLocationError::Guy(
            don_replay::groups_first_farm_authority::FirstFarmFirstCitizenGuyError::Guy(
                UnitGuyInitError::IncoherentGraphicsCapture { slot: 0 }
            )
        ))
    );

    let mut bad_terrain = first_citizen_location_inputs(&replay, &plan, &scout, &after_scout);
    bad_terrain.terrain[1].x ^= 1;
    assert_eq!(
        produce_first_farm_first_citizen_location(
            &replay,
            &plan,
            &scout,
            &after_scout,
            &detailed,
            &after_citizen,
            &authority,
            bad_terrain,
        ),
        Err(FirstFarmFirstCitizenLocationError::Location(
            UnitInitLocationError::TerrainReceiptMismatch { ordinal: 1 }
        ))
    );

    let mut bad_authority = authority.clone();
    bad_authority.rng_after ^= 1;
    assert!(matches!(
        produce_first_farm_first_citizen_guy_prefix(
            &replay,
            &plan,
            &scout,
            &after_scout,
            &detailed,
            &after_citizen,
            &bad_authority,
            vec![synthetic_first_scout_graphics(0)],
            vec![synthetic_first_scout_predicates()],
        ),
        Err(
            don_replay::groups_first_farm_authority::FirstFarmFirstCitizenGuyError::Init(
                FirstFarmFirstCitizenInitError::AfterRngMismatch
            )
        )
    ));
}

#[test]
fn fifth_setup_allocation_binds_generationally_to_the_frame79_builder() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let (plan, setup, sim) = synthetic_frame79_setup();

    let bound =
        bind_first_farm_builder_at_frame79(&replay, &plan, &setup, &sim, authority()).unwrap();
    assert_eq!(bound.setup_ordinal, 4);
    assert_eq!((bound.frame, bound.row, bound.current_type), (79, 4, 50));
    assert_eq!(
        (
            bound.allocation.owner,
            bound.allocation.o,
            bound.unit.identity.who,
            bound.unit.identity.o,
            bound.unit.identity.handle.id,
            bound.unit.identity.handle.generation,
        ),
        (0, 4, 0, 4, bound.allocation.id, bound.allocation.generation,)
    );
    assert_eq!(bound.authority_digest, [0x79; 32]);
}

#[test]
fn frame79_builder_join_rejects_unversioned_stale_or_malformed_authority() {
    let path = replay_path();
    let replay = Replay::open(&path)
        .unwrap_or_else(|error| panic!("required strict replay {}: {error}", path.display()));
    let (plan, setup, mut sim) = synthetic_frame79_setup();

    let mut missing_revision = authority();
    missing_revision.revision = 0;
    assert_eq!(
        bind_first_farm_builder_at_frame79(&replay, &plan, &setup, &sim, missing_revision),
        Err(FirstFarmBuilderBindingError::MissingAuthorityRevision)
    );

    sim.world.frame -= 1;
    assert_eq!(
        bind_first_farm_builder_at_frame79(&replay, &plan, &setup, &sim, authority()),
        Err(FirstFarmBuilderBindingError::WrongFrame {
            expected: 79,
            actual: 78,
        })
    );
    sim.world.frame += 1;

    let mut no_direct_draw = setup.clone();
    let last = no_direct_draw.placements.last_mut().unwrap();
    last.rng_events.remove(0);
    let PlacementRngEvent::InitUnit(span) = &mut last.rng_events[0] else {
        unreachable!()
    };
    span.state_before = last.rng_before;
    assert_eq!(
        bind_first_farm_builder_at_frame79(&replay, &plan, &no_direct_draw, &sim, authority()),
        Err(FirstFarmBuilderBindingError::MissingDirectPlacementDraw { ordinal: 4 })
    );

    let mut wrong_sequence = setup.clone();
    let PlacementOutcomeReceipt::Spawned(last) =
        &mut wrong_sequence.placements.last_mut().unwrap().outcome
    else {
        unreachable!()
    };
    last.returned_captain_o = 3;
    last.members[0].identity.o = 3;
    last.members[0].guy_identities[0].o = 3;
    assert_eq!(
        bind_first_farm_builder_at_frame79(&replay, &plan, &wrong_sequence, &sim, authority()),
        Err(FirstFarmBuilderBindingError::WrongAllocationSequence { ordinal: 4 })
    );

    let mut stale = setup.clone();
    let PlacementOutcomeReceipt::Spawned(last) = &mut stale.placements.last_mut().unwrap().outcome
    else {
        unreachable!()
    };
    last.members[0].identity.generation += 1;
    last.members[0].units_authority_key.1 += 1;
    last.members[0].guys_authority_key.1 += 1;
    assert_eq!(
        bind_first_farm_builder_at_frame79(&replay, &plan, &stale, &sim, authority()),
        Err(FirstFarmBuilderBindingError::StaleCanonicalHandle)
    );

    sim.unit_type[4] = 51;
    assert_eq!(
        bind_first_farm_builder_at_frame79(&replay, &plan, &setup, &sim, authority()),
        Err(FirstFarmBuilderBindingError::CanonicalTypeMismatch)
    );
}
