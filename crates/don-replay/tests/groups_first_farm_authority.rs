use std::path::{Path, PathBuf};

use don_replay::groups_first_farm_authority::{
    bind_first_farm_builder_at_frame79, bind_first_farm_first_init_unit, discover_first_2018_farm,
    produce_first_farm_first_placement, FirstFarmAuthorityBlocker, FirstFarmAuthorityError,
    FirstFarmBuilderBindingError, FirstFarmFirstInitUnitAuthority, FirstFarmFirstInitUnitError,
    FirstFarmFirstInitUnitSource, FirstFarmFirstPlacementError, FirstFarmFrame79Authority,
    FirstFarmFrame79Source, FirstFarmSetupEntryAuthority, FirstFarmSetupEntrySource, FIRST_FRAME,
    FIRST_OWNER, FIRST_PLAY, FIRST_SELECTED_O, FIRST_SERIAL, STRICT_REPLAY_SHA256,
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
    setup_cities_builds::CITY_CENTER_TYPE,
    setup_place_unit_deep_re::{PlaceUnitExternalResidual, ProbeDisposition},
};
use don_sim::rng::Random;
use don_sim::systems::{
    map_terrain::land,
    objects_init_unit_authority_frontier::{
        BhsInitUnitRequest, CaptainFacts, CompleteBody, DetailedInitUnitReceipt,
        FindFreeDisposition, FindFreeUnitReceipt, InitUnitReceiptError, InitUnitStep,
        ResolveCaptainReceipt, TrackUnitTypeFacts, TrainingWhere, UnitAfterInit,
        UnitBandStorageClass, UnitInitReceipt, UnitTypeAuthorityFacts,
    },
    production,
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
            scout: type_facts(69),
            citizen: type_facts(50),
            dutch_merchant: type_facts(DUTCH_MERCHANT_TYPE),
        },
    })
    .unwrap()
}

fn member(call: PlaceUnitCall, handle: Handle, o: i32) -> UnitMemberAuthorityReceipt {
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
            length: 1,
            capacity: 1,
            increment: 1,
            flags: 0,
        },
        guy_mark: 1,
        guy_identities: vec![GuyIdentityReceipt {
            slot: 0,
            who: FIRST_OWNER as i8,
            o: o as i16,
            guy_num: 0,
        }],
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
                1,
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
                    x: request.x,
                    y: request.y,
                    angle: after_image.angle,
                    new_block_radius: 1,
                },
            }),
        ],
        returned: 0,
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
            1,
        )
        .unwrap();
    let rng_after = placement.placement.rng_after_probes.wrapping_add(0x404);
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
