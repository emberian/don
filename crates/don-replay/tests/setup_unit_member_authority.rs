use std::path::{Path, PathBuf};

use don_replay::setup_unit_member_authority::{
    bind_canonical_setup_citizens, CanonicalSetupMemberError, CanonicalSetupMemberSource,
    CanonicalSetupSnapshotAuthority,
};
use don_replay::setup_units_producer::{
    build_units_plan, BuildUnitsInputs, BuildUnitsPrefixReceipt, EngineContainerShapeReceipt,
    GuyIdentityReceipt, InitUnitAuthorityReceipt, InitUnitRngSpan, PlaceUnitCall, PlaceUnitReceipt,
    PlacementOutcomeReceipt, PlacementRngEvent, StableUnitIdentityReceipt, StartingUnitBonuses,
    StartingUnitRuleFacts, StartingUnitTypeFacts, TypeResolutionFacts, UnitMemberAuthorityReceipt,
    BASE_PEASANT_TYPE, BASE_SCOUT_TYPE, DUTCH_MERCHANT_TYPE,
};
use don_replay::world_owner_frontier::sha256;
use don_replay::Replay;
use don_sim::systems::objects_init_unit_authority_frontier::{
    OBJECTS_INIT_UNIT_BYTES, OBJECTS_INIT_UNIT_VA,
};
use don_sim::tick::Sim;

const WITNESS: &str = "ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
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

fn member(
    call: PlaceUnitCall,
    handle: don_sim::world::Handle,
    o: i32,
) -> UnitMemberAuthorityReceipt {
    let guys = call.squad_size + call.crew_size;
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
            length: guys,
            capacity: guys,
            increment: 1,
            flags: 0,
        },
        guy_mark: call.squad_size as i8,
        guy_identities: (0..guys)
            .map(|slot| GuyIdentityReceipt {
                slot,
                who: call.owner as i8,
                o: o as i16,
                guy_num: slot as i8,
            })
            .collect(),
        units_authority_key: (handle.id, handle.generation),
        guys_authority_key: (handle.id, handle.generation),
    }
}

fn dutch_setup() -> Option<(
    Replay,
    don_replay::setup_units_producer::BuildUnitsPlan,
    BuildUnitsPrefixReceipt,
    Sim,
)> {
    let path = root().join(WITNESS);
    let replay = Replay::open(&path).ok()?;
    let edge = replay.initial.info.settings.map_edge_world_cells()? as u16;
    let plan = build_units_plan(BuildUnitsInputs {
        owner: 0,
        start_index: 0,
        center_city_o: 2_000,
        start_tile_x: 97,
        start_tile_y: 13,
        starting_town: 1,
        starting_resources: 2,
        reveal_map: 1,
        bonuses: StartingUnitBonuses {
            dutch_merchants: true,
            ..StartingUnitBonuses::default()
        },
        rules: StartingUnitRuleFacts::default(),
        types: StartingUnitTypeFacts {
            scout: type_facts(BASE_SCOUT_TYPE, 1, 1),
            dutch_merchant: type_facts(DUTCH_MERCHANT_TYPE, 1, 0),
            citizen: type_facts(BASE_PEASANT_TYPE, 1, 0),
        },
    })
    .unwrap();
    assert_eq!(plan.calls.len(), 7);

    let mut sim = Sim::new(u64::from(replay.initial.info.seed), edge);
    let mut placements = Vec::new();
    for (ordinal, call) in plan.calls.iter().copied().enumerate() {
        let handle = sim
            .spawn_unit(0, call.place_unit_upgrade, 74_592, 10_080, 1)
            .unwrap();
        placements.push(PlaceUnitReceipt {
            call,
            rng_before: 17,
            rng_after: 17,
            rng_events: vec![PlacementRngEvent::InitUnit(InitUnitRngSpan {
                body_va: OBJECTS_INIT_UNIT_VA,
                body_bytes: OBJECTS_INIT_UNIT_BYTES,
                state_before: 17,
                state_after: 17,
            })],
            outcome: PlacementOutcomeReceipt::Spawned(InitUnitAuthorityReceipt {
                validated_body_va: OBJECTS_INIT_UNIT_VA,
                validated_body_bytes: OBJECTS_INIT_UNIT_BYTES,
                unit_mark_before: ordinal as i32,
                unit_mark_after: ordinal as i32 + 1,
                returned_captain_o: ordinal as i32,
                members: vec![member(call, handle, ordinal as i32)],
            }),
        });
    }
    sim.world.frame = 379;
    Some((
        replay,
        plan,
        BuildUnitsPrefixReceipt {
            rng_initial: 17,
            rng_final: 17,
            placements,
        },
        sim,
    ))
}

#[test]
fn dutch_citizen_cohort_binds_ordinals_three_through_six_to_canonical_units() {
    let Some((replay, plan, setup, sim)) = dutch_setup() else {
        eprintln!("SKIPPED -- NOT A PASS: {WITNESS} is absent");
        return;
    };
    let authority = CanonicalSetupSnapshotAuthority {
        revision: 1,
        composition_digest: [0x5a; 32],
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: sha256(&std::fs::read(&replay.path).unwrap()),
        frame: sim.world.frame,
        world_checksum: sim.map.world.checksum_sections(),
        random_state: sim.world.random.state(),
    };
    let citizens = bind_canonical_setup_citizens(&replay, &plan, &setup, &sim, &authority).unwrap();
    assert_eq!(citizens.len(), 4);
    assert_eq!(
        citizens
            .iter()
            .map(|receipt| (
                receipt.setup_ordinal,
                receipt.unit.identity.o,
                receipt.current_type,
                receipt.type_facts.type_index,
            ))
            .collect::<Vec<_>>(),
        [
            (3, 3, 50, 50),
            (4, 4, 50, 50),
            (5, 5, 50, 50),
            (6, 6, 50, 50)
        ]
    );
    assert!(citizens.iter().all(|receipt| {
        let group = receipt.group_move_type_facts();
        receipt.unit.identity.handle.id == receipt.allocation.identity.id
            && receipt.unit.identity.handle.generation == receipt.allocation.identity.generation
            && receipt.type_facts.role == 262_912
            && receipt.type_facts.unit_flags == 6_273
            && receipt.type_facts.domain == 0
            && group.type_id == 50
            && group.role == receipt.type_facts.role
            && group.unit_flags == receipt.type_facts.unit_flags
            && receipt.bind_resolved_land_speed(25).handle == receipt.unit.identity.handle
    }));
}

#[test]
fn stale_setup_identity_and_snapshot_mutations_fail_closed() {
    let Some((replay, plan, mut setup, sim)) = dutch_setup() else {
        eprintln!("SKIPPED -- NOT A PASS: {WITNESS} is absent");
        return;
    };
    let authority = CanonicalSetupSnapshotAuthority {
        revision: 2,
        composition_digest: [0xa5; 32],
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: sha256(&std::fs::read(&replay.path).unwrap()),
        frame: sim.world.frame,
        world_checksum: sim.map.world.checksum_sections(),
        random_state: sim.world.random.state(),
    };
    let PlacementOutcomeReceipt::Spawned(init) = &mut setup.placements[3].outcome else {
        unreachable!()
    };
    init.members[0].identity.id = init.members[0].identity.id.wrapping_add(100);
    init.members[0].units_authority_key.0 = init.members[0].identity.id;
    init.members[0].guys_authority_key.0 = init.members[0].identity.id;
    assert!(matches!(
        bind_canonical_setup_citizens(&replay, &plan, &setup, &sim, &authority),
        Err(CanonicalSetupMemberError::StaleCanonicalHandle { owner: 0, o: 3 })
    ));

    let mut wrong_snapshot = authority;
    wrong_snapshot.random_state = wrong_snapshot.random_state.wrapping_add(1);
    assert_eq!(
        bind_canonical_setup_citizens(
            &replay,
            &plan,
            &dutch_setup().unwrap().2,
            &sim,
            &wrong_snapshot,
        ),
        Err(CanonicalSetupMemberError::RandomStateMismatch)
    );
}
