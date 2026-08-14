//! Full conditional Leader child transcript, representation, and agreement gates.

use don_replay::checksum::Channel;
use don_replay::leader_initial_prefix::{derive, InitialLeaderPrefix};
use don_replay::leaders_deferred_history_frontier::{
    bind_deferred_history_frontier, DeferredLeadersFixedAuthority,
    RuntimeLeadersDeferredHistoryFrontier, REG_BUILDING_TYPE_SLOTS,
};
use don_replay::leaders_dynamic_children_frontier::{
    bind_dynamic_children_frontier, DynamicChildRepresentation, DynamicChildrenFrontierError,
    DynamicLeadersAuthority, MakeObjectImage, RetailArray, SiteImage,
    ARRAY_MAKE_OBJECT_WALK_DATA_VA, ARRAY_SITE_WALK_DATA_VA, DEFAULT_DYNAMIC_CHILD_WALK_BYTES,
    LEADER_DATA_ENCRYPT_WALK_DATA_VA, LEADER_WALK_ENCRYPTED_CALL_VA, LEADER_WALK_MAKE_LIST_CALL_VA,
    LEADER_WALK_PERSONALITY_CALL_VA, LEADER_WALK_PROD_SCRIPT_CALL_VA, LEADER_WALK_SITES_CALL_VA,
    SIMPLE_ARRAY_INT_WALK_DATA_VA, STRING_WALK_DATA_VA,
};
use don_replay::leaders_generated_fixed_frontier::bind_generated_fixed_prefix;
use don_replay::leaders_runtime_frontier::{
    bind_live, LeadersWalkBoundary, RuntimeCoveredRange, LEADER_DIPLOMACY_BEGIN,
    LEADER_DIPLOMACY_BYTES, LEADER_FIXED_BODY_BEGIN, LEADER_FIXED_BODY_END,
};
use don_replay::leaders_runtime_tribe_frontier::{bind_live_tribes, LEADER_TRIBE_OFFSET};
use don_replay::leaders_setup_build_history_frontier::{
    bind_frame_zero_last_building_history, derive_frame_zero_last_building_history,
    FrameZeroBuildHistoryError, BUILD_ACTIVATE_LAST_BUILDING_GUARD_VA,
    BUILD_ACTIVATE_LAST_BUILDING_STORE_VA, BUILD_ACTIVATE_VA,
    FRAME_ZERO_LAST_BUILDING_HISTORY_WALKED_BYTES, LEADER_INIT_LAST_BUILDING_HISTORY_FILL_VA,
    LEADER_INIT_LAST_BUILDING_HISTORY_VA, SETUP_BUILD_ACTIVATE_VIRTUAL_CALL_VA,
};
use don_replay::leaders_setup_build_registry_frontier::{
    bind_frame_zero_build_registry_census, derive_frame_zero_build_registry_census,
    FrameZeroBuildRegistryError, BUILD_ACTIVATE_HIGH_WATER_BEGIN_VA,
    BUILD_ACTIVATE_HIGH_WATER_END_VA, BUILD_ACTIVATE_TO_LOAD_VA, BUILD_TYPE_BASIC_TYPE_VA,
    DOCK_INIT_REGISTRY_STORE_VA, FORT_INIT_REGISTRY_STORE_VA, FRAME_ZERO_BUILD_CENSUS_WALKED_BYTES,
    FRAME_ZERO_BUILD_REGISTRY_WALKED_BYTES, FRAME_ZERO_HIGH_BUILDINGS_WALKED_BYTES,
    LEADER_GET_BUILDINGS_VA,
};
use don_replay::leaders_setup_reg_buildings_frontier::{
    bind_frame_zero_regional_buildings, derive_frame_zero_regional_building_census,
    FrameZeroRegBuildingsError, FRAME_ZERO_REG_BUILDINGS_WALKED_BYTES,
    WALL_INCREMENT_STATS_REGION_STORE_VA, WALL_INCREMENT_STATS_TOTAL_STORE_VA,
    WALL_INCREMENT_STATS_VA,
};
use don_replay::leaders_setup_region_history_frontier::{
    bind_frame_zero_region_strategy_history, derive_frame_zero_region_strategy_history,
    FrameZeroRegionHistoryError, FRAME_ZERO_REG_STRATEGY_HISTORY_WALKED_BYTES,
    LEADER_INIT_REGION_HISTORY_BEGIN_VA, LEADER_INIT_REGION_HISTORY_LOOP_END_VA,
    PLAN_STRATEGY_REG_ATTACKED_FIRST_STORE_VA, PLAN_STRATEGY_RELATION_HISTORY_CLEAR_VA,
    PLAN_STRATEGY_RELATION_HISTORY_LAST_STORE_VA,
};
use don_replay::leaders_sim_owner_frontier::{
    bind_sim_owner_frontier, SimOwnerFrontierError, POP_OFFSET, SIM_OWNER_DUPLICATE_BYTES,
    SIM_OWNER_NEWLY_CANONICAL_BYTES, SIM_OWNER_SOURCE_BYTES,
};
use don_replay::leaders_sim_tech_frontier::{
    bind_sim_tech_frontier, SimTechFrontierError, ECON_DISCOVERED_INDEX,
    SIM_TECH_EXISTING_DUPLICATE_BYTES, SIM_TECH_NEWLY_CANONICAL_BYTES, SIM_TECH_SOURCE_BYTES,
};
use don_replay::leaders_type_mask_owner_frontier::{
    bind_type_mask_owner, TypeMaskOwnerError, OBS_FLAGS_WALKED_BYTES,
    TYPE_MASK_HEADER_WALKED_BYTES, TYPE_MASK_NEWLY_CANONICAL_WALKED_BYTES,
};
use don_replay::replay::{corpus, Replay};
use don_replay::{harness::WorldSim, setup_cities_builds::StartingSetupState};
use don_sim::generated::state::{leader, FieldDesc, LeaderCols, Pool};
use don_sim::systems::bhs_type_table::{
    LeaderTypeMasks, TribeRoster, TypeBackup, TypeBuiltinState, TypeRow, TypeTable, BUILD_BEGIN,
    BUILD_END, NUM_LEADERS, NUM_TRIBES, NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
};
use don_sim::systems::{leaders, victory_score};
use don_sim::tick::Sim;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn skip(reason: &str) {
    eprintln!("\n  SKIPPED — NOT A PASS. {reason}\n  Nothing was established.\n");
}

fn first_prefix() -> Option<InitialLeaderPrefix> {
    corpus(&repo_root()).into_iter().find_map(|path| {
        let replay = Replay::open(&path).ok()?;
        derive(&replay.initial).ok()
    })
}

fn current_states(prefix: &InitialLeaderPrefix) -> (victory_score::Leaders, leaders::Leaders) {
    let types = victory_score::TypeTable::with_default_kinds(Default::default());
    let mut victory = victory_score::Leaders::new(types);
    let mut step8 = leaders::Leaders::new();
    for slot in 0..NUM_LEADERS {
        let mut flags = 0i32;
        if prefix.rows[slot].active {
            flags |= victory_score::leader_flag::VALID | victory_score::leader_flag::ACTIVE;
        }
        if prefix.rows[slot].human {
            flags |= victory_score::leader_flag::HUMAN;
        }
        victory.slots[slot].leader_flags = flags;
        step8.leaders[slot].flags = flags as u32;
    }
    (victory, step8)
}

fn type_state(prefix: &InitialLeaderPrefix) -> TypeBuiltinState {
    let rows: Vec<_> = (0..NUM_TYPES).map(TypeRow::empty).collect();
    let backups = rows
        .iter()
        .enumerate()
        .map(|(slot, row)| {
            ((REGULAR_UNIT_BEGIN..REGULAR_UNIT_END).contains(&slot)
                || (BUILD_BEGIN..BUILD_END).contains(&slot))
            .then(|| TypeBackup::capture_pristine(row))
        })
        .collect();
    let types = TypeTable::new(rows, backups).unwrap();
    let tribes = TribeRoster::new(
        (0..NUM_TRIBES)
            .map(|index| format!("Tribe {index}"))
            .collect(),
    )
    .unwrap();
    let leaders = std::array::from_fn(|slot| {
        let mut row = LeaderTypeMasks::default();
        row.leader_flags = i32::from(prefix.rows[slot].active);
        row.tribe = prefix.rows[slot].tribe.map_or(0, |selector| {
            if usize::from(selector) == NUM_TRIBES {
                ((slot + 5) % NUM_TRIBES) as i32
            } else {
                i32::from(selector)
            }
        });
        row
    });
    TypeBuiltinState::new(types, tribes, leaders)
}

fn zeroed_columns() -> LeaderCols {
    let mut columns = LeaderCols::with_capacity(NUM_LEADERS);
    for _ in 0..NUM_LEADERS {
        assert!(columns.push_zeroed().is_some());
    }
    columns
}

fn write_field(columns: &mut LeaderCols, row: usize, field: &FieldDesc, bytes: &[u8]) {
    assert_eq!(bytes.len(), field.size as usize);
    let count = field.count as usize;
    let plane = field.plane as usize;
    match field.pool {
        Pool::W4 => {
            let values: Vec<_> = bytes
                .chunks_exact(4)
                .map(|chunk| i32::from_le_bytes(chunk.try_into().unwrap()))
                .collect();
            if count == 1 {
                columns.w4_plane_mut(plane)[row] = values[0];
            } else {
                columns
                    .w4_arr_mut(plane, row, count)
                    .copy_from_slice(&values);
            }
        }
        Pool::W2 => {
            let values: Vec<_> = bytes
                .chunks_exact(2)
                .map(|chunk| i16::from_le_bytes(chunk.try_into().unwrap()))
                .collect();
            if count == 1 {
                columns.w2_plane_mut(plane)[row] = values[0];
            } else {
                columns
                    .w2_arr_mut(plane, row, count)
                    .copy_from_slice(&values);
            }
        }
        Pool::W1 => {
            let values: Vec<_> = bytes.iter().map(|value| *value as i8).collect();
            if count == 1 {
                columns.w1_plane_mut(plane)[row] = values[0];
            } else {
                columns
                    .w1_arr_mut(plane, row, count)
                    .copy_from_slice(&values);
            }
        }
        Pool::WF => {
            let values: Vec<_> = bytes
                .chunks_exact(4)
                .map(|chunk| f32::from_bits(u32::from_le_bytes(chunk.try_into().unwrap())))
                .collect();
            if count == 1 {
                columns.wf_slice_mut(plane)[row] = values[0];
            } else {
                columns
                    .wf_arr_mut(plane, row, count)
                    .copy_from_slice(&values);
            }
        }
        Pool::None => panic!("only materialised fields are copied"),
    }
}

fn copy_owned_payload(
    runtime: &don_replay::leaders_runtime_frontier::RuntimeLeaderRow,
    begin: usize,
    payload: &mut [u8],
) {
    if let Some(bytes) = runtime.owned_slice(RuntimeCoveredRange {
        begin,
        end: begin + payload.len(),
    }) {
        payload.copy_from_slice(bytes);
    }
}

struct Fixture {
    prefix: InitialLeaderPrefix,
    victory: victory_score::Leaders,
    step8: leaders::Leaders,
    types: TypeBuiltinState,
    columns: LeaderCols,
    previous: RuntimeLeadersDeferredHistoryFrontier,
    authority: DynamicLeadersAuthority,
    active: usize,
    active_count: usize,
}

fn deferred_frontier(
    prefix: &InitialLeaderPrefix,
    victory: &victory_score::Leaders,
    step8: &leaders::Leaders,
    types: &TypeBuiltinState,
    columns: &LeaderCols,
) -> RuntimeLeadersDeferredHistoryFrontier {
    deferred_frontier_with_authority(
        prefix,
        victory,
        step8,
        types,
        columns,
        &DeferredLeadersFixedAuthority::default(),
    )
}

fn deferred_frontier_with_authority(
    prefix: &InitialLeaderPrefix,
    victory: &victory_score::Leaders,
    step8: &leaders::Leaders,
    types: &TypeBuiltinState,
    columns: &LeaderCols,
    authority: &DeferredLeadersFixedAuthority,
) -> RuntimeLeadersDeferredHistoryFrontier {
    let base = bind_live(prefix, victory, step8).expect("runtime owners agree");
    let tribes = bind_live_tribes(prefix, base, types).expect("tribe owner agrees");
    let generated = bind_generated_fixed_prefix(tribes, columns).expect("columns agree");
    bind_deferred_history_frontier(generated, columns, authority).expect("deferred history agrees")
}

fn fixture() -> Option<Fixture> {
    let prefix = first_prefix()?;
    let (victory, step8) = current_states(&prefix);
    Some(fixture_with_states(prefix, victory, step8))
}

fn fixture_with_states(
    prefix: InitialLeaderPrefix,
    victory: victory_score::Leaders,
    step8: leaders::Leaders,
) -> Fixture {
    let active = prefix
        .rows
        .iter()
        .position(|row| row.active)
        .expect("fixture needs one active Leader");
    let active_count = prefix.rows.iter().filter(|row| row.active).count();
    let types = type_state(&prefix);

    let base = bind_live(&prefix, &victory, &step8).unwrap();
    let tribes = bind_live_tribes(&prefix, base, &types).unwrap();
    let mut columns = zeroed_columns();
    for slot in 0..NUM_LEADERS {
        let base = &tribes.base().rows[slot];
        for field in &leader::FIELDS {
            let begin = field.offset as usize;
            let end = begin + field.size as usize;
            if field.alias_of.is_some() || !field.repr.materialised() {
                continue;
            }
            if let Some(bytes) = base.owned_slice(RuntimeCoveredRange { begin, end }) {
                write_field(&mut columns, slot, field, bytes);
            }
        }
        if base.active {
            let tribe = tribes.tribe_for(slot).unwrap().to_le_bytes();
            let field = leader::FIELDS
                .iter()
                .find(|field| field.offset as usize == LEADER_TRIBE_OFFSET)
                .unwrap();
            write_field(&mut columns, slot, field, &tribe);
        }
    }

    let previous = deferred_frontier(&prefix, &victory, &step8, &types, &columns);
    let base = previous.previous().previous().base();
    let mut authority = DynamicLeadersAuthority::default();
    for slot in 0..NUM_LEADERS {
        if !base.rows[slot].active {
            continue;
        }
        let runtime = &base.rows[slot];
        if let Some(bytes) = runtime.owned_slice(RuntimeCoveredRange {
            begin: 0x6dd4 + 6 * 4,
            end: 0x6dd8 + 6 * 4,
        }) {
            authority.rows[slot].personality.raid = i32::from_le_bytes(bytes.try_into().unwrap());
        }
        let row = &mut authority.rows[slot];
        copy_owned_payload(runtime, 0x6c18, &mut row.tech.payload);
        copy_owned_payload(runtime, 0x6c8c, &mut row.tech_at_start.payload);
        copy_owned_payload(runtime, 0x6da4, &mut row.rare.payload);
        copy_owned_payload(runtime, 0x6db8, &mut row.rare_owned.payload);
        copy_owned_payload(runtime, 0x6dcc, &mut row.rare_conquest.payload);
        for (index, value) in runtime.econ_plaintext().iter().copied().enumerate() {
            if let Some(value) = value {
                authority.rows[slot].economy_plaintext[index] = value;
            }
        }
    }

    Fixture {
        prefix,
        victory,
        step8,
        types,
        columns,
        previous,
        authority,
        active,
        active_count,
    }
}

#[test]
fn complete_transcript_is_conditional_and_never_becomes_a_leaders_producer() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let frontier = bind_dynamic_children_frontier(fixture.previous, &fixture.authority).unwrap();
    let walk = frontier.walk_frontier();

    assert_eq!(LEADER_WALK_PERSONALITY_CALL_VA, 0x006d_67c7);
    assert_eq!(LEADER_WALK_SITES_CALL_VA, 0x006d_68f7);
    assert_eq!(LEADER_WALK_MAKE_LIST_CALL_VA, 0x006d_6904);
    assert_eq!(LEADER_WALK_PROD_SCRIPT_CALL_VA, 0x006d_6937);
    assert_eq!(LEADER_WALK_ENCRYPTED_CALL_VA, 0x006d_69d6);
    assert_eq!(ARRAY_SITE_WALK_DATA_VA, 0x0047_cee0);
    assert_eq!(ARRAY_MAKE_OBJECT_WALK_DATA_VA, 0x0047_d440);
    assert_eq!(SIMPLE_ARRAY_INT_WALK_DATA_VA, 0x0047_3120);
    assert_eq!(STRING_WALK_DATA_VA, 0x00a1_b2d0);
    assert_eq!(LEADER_DATA_ENCRYPT_WALK_DATA_VA, 0x006d_9900);
    assert_eq!(walk.boundary, LeadersWalkBoundary::Complete);

    let active_bytes = LEADER_FIXED_BODY_END
        + LEADER_DIPLOMACY_BYTES * NUM_LEADERS
        + DEFAULT_DYNAMIC_CHILD_WALK_BYTES;
    let expected = fixture.active_count * active_bytes
        + (NUM_LEADERS - fixture.active_count) * LEADER_FIXED_BODY_BEGIN;
    assert_eq!(walk.bytes_walked, expected as u64);
    assert_eq!(
        frontier.conditionally_admitted_walked_bytes(),
        fixture.active_count * 350
    );
    assert_eq!(
        frontier.duplicate_checked_walked_bytes(),
        fixture.active_count * 420
    );
    let row = &frontier.rows()[fixture.active];
    assert_eq!(
        row.claims()
            .iter()
            .map(|claim| claim.walked_bytes())
            .sum::<usize>(),
        770
    );
    assert_eq!(row.claims()[0].field, "Personality");
    assert_eq!(
        row.claims()[0].representation,
        DynamicChildRepresentation::RawObjectBytes
    );
    assert_eq!(
        row.claims().last().unwrap().field,
        "LeaderDataEncrypt plaintext"
    );
    assert_eq!(frontier.source_produced_walked_bytes(), 0);
    assert_eq!(frontier.checksum(), Err(walk));
    assert!(!frontier.installed_in_scoreboard());

    let checked = don_replay::check_all::CheckAll::of_state(&don_replay::state::SimState::new());
    let leaders = &checked.per[Channel::Leaders as usize];
    assert!(!leaders.installed);
    assert!(!leaders.exact_producer);
    assert!(!leaders.substantive());
}

#[test]
fn container_and_string_history_follow_retail_representation() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let baseline =
        bind_dynamic_children_frontier(fixture.previous.clone(), &fixture.authority).unwrap();
    let baseline_walk = baseline.walk_frontier();

    let mut populated = fixture.authority.clone();
    let row = &mut populated.rows[fixture.active];
    row.sites = RetailArray {
        length: 1,
        capacity: 3,
        increment: 2,
        flags: 0x02,
        elements: vec![SiteImage {
            wx: 11,
            wy: 12,
            value: 13,
            region: 14,
            distance: 15,
            rank: 16,
        }],
    };
    row.make_list = RetailArray {
        length: 1,
        capacity: 1,
        increment: 4,
        flags: 0x08,
        elements: vec![MakeObjectImage {
            type_id: 21,
            number: 2,
            wx: 31,
            wy: 32,
            ..Default::default()
        }],
    };
    row.military_trainers = RetailArray {
        length: 1,
        capacity: 2,
        increment: -1,
        flags: 0x10,
        elements: vec![41],
    };
    row.new_rares = RetailArray {
        length: 1,
        capacity: 1,
        increment: 1,
        flags: 0x20,
        elements: vec![42],
    };
    row.oil_patches = RetailArray {
        length: 1,
        capacity: 4,
        increment: 3,
        flags: 0x01,
        elements: vec![43],
    };
    row.production_script_utf16 = "AI!".encode_utf16().collect();

    let populated_frontier =
        bind_dynamic_children_frontier(fixture.previous.clone(), &populated).unwrap();
    assert_eq!(
        populated_frontier.walk_frontier().bytes_walked,
        baseline_walk.bytes_walked + 117
    );
    assert_ne!(
        populated_frontier.walk_frontier().checksum,
        baseline_walk.checksum
    );

    let mut transient_flag = populated.clone();
    transient_flag.rows[fixture.active].sites.flags ^= 0x40;
    let transient =
        bind_dynamic_children_frontier(fixture.previous.clone(), &transient_flag).unwrap();
    assert_eq!(
        transient.walk_frontier().checksum,
        populated_frontier.walk_frontier().checksum
    );
    let mut walked_flag = populated;
    walked_flag.rows[fixture.active].sites.flags ^= 0x02;
    let walked = bind_dynamic_children_frontier(fixture.previous, &walked_flag).unwrap();
    assert_ne!(
        walked.walk_frontier().checksum,
        populated_frontier.walk_frontier().checksum
    );
}

#[test]
fn conditional_payloads_bite_adler_and_bad_representations_refuse() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let baseline = bind_dynamic_children_frontier(fixture.previous.clone(), &fixture.authority)
        .unwrap()
        .walk_frontier();

    let mut changed = fixture.authority.clone();
    changed.rows[fixture.active].personality.target = 73;
    changed.rows[fixture.active].obs_flags.payload[79] ^= 0x80;
    let base = fixture.previous.previous().previous().base();
    let missing_econ = base.rows[fixture.active]
        .econ_plaintext()
        .iter()
        .position(Option::is_none)
        .expect("runtime economy projection is incomplete");
    changed.rows[fixture.active].economy_plaintext[missing_econ] = 99;
    let changed_walk = bind_dynamic_children_frontier(fixture.previous.clone(), &changed)
        .unwrap()
        .walk_frontier();
    assert_ne!(changed_walk.checksum, baseline.checksum);
    assert_eq!(changed_walk.bytes_walked, baseline.bytes_walked);

    let mut bad_mask = fixture.authority.clone();
    bad_mask.rows[fixture.active].obs_flags.size -= 1;
    assert!(matches!(
        bind_dynamic_children_frontier(fixture.previous.clone(), &bad_mask),
        Err(DynamicChildrenFrontierError::InvalidBitMask { slot, field: "obs_flags", .. })
            if slot == fixture.active
    ));

    let mut bad_array = fixture.authority.clone();
    bad_array.rows[fixture.active].sites.length = 1;
    assert_eq!(
        bind_dynamic_children_frontier(fixture.previous.clone(), &bad_array),
        Err(DynamicChildrenFrontierError::ArrayElementCount {
            slot: fixture.active,
            field: "Sites",
            length: 1,
            elements: 0,
        })
    );
    bad_array.rows[fixture.active].sites.length = -1;
    assert!(matches!(
        bind_dynamic_children_frontier(fixture.previous.clone(), &bad_array),
        Err(DynamicChildrenFrontierError::NegativeArrayLength { slot, field: "Sites", .. })
            if slot == fixture.active
    ));

    let mut huge_string = fixture.authority;
    huge_string.rows[fixture.active].production_script_utf16 = vec![0; 65_536];
    assert_eq!(
        bind_dynamic_children_frontier(fixture.previous, &huge_string),
        Err(DynamicChildrenFrontierError::StringTooLong {
            slot: fixture.active,
            code_units: 65_536,
        })
    );
}

#[test]
fn duplicate_owner_changes_refuse_in_either_direction() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };

    let mut authority_raid = fixture.authority.clone();
    authority_raid.rows[fixture.active].personality.raid ^= 1;
    assert!(matches!(
        bind_dynamic_children_frontier(fixture.previous.clone(), &authority_raid),
        Err(DynamicChildrenFrontierError::RuntimeDisagreement {
            slot,
            field: "Personality",
            ..
        }) if slot == fixture.active
    ));

    let mut authority_tech = fixture.authority.clone();
    authority_tech.rows[fixture.active].tech.payload[7] ^= 0x20;
    assert!(matches!(
        bind_dynamic_children_frontier(fixture.previous.clone(), &authority_tech),
        Err(DynamicChildrenFrontierError::RuntimeDisagreement {
            slot,
            field: "tech",
            ..
        }) if slot == fixture.active
    ));

    let owned_econ = fixture.previous.previous().previous().base().rows[fixture.active]
        .econ_plaintext()
        .iter()
        .position(Option::is_some)
        .expect("runtime owns part of the decoded economy transcript");
    let mut authority_econ = fixture.authority.clone();
    authority_econ.rows[fixture.active].economy_plaintext[owned_econ] ^= 1;
    assert!(matches!(
        bind_dynamic_children_frontier(fixture.previous.clone(), &authority_econ),
        Err(DynamicChildrenFrontierError::EconomyDisagreement { slot, index, .. })
            if slot == fixture.active && index == owned_econ
    ));

    let mut changed_step8 = fixture.step8.clone();
    changed_step8.leaders[fixture.active].taunt.personality_raid ^= 1;
    let changed_previous = deferred_frontier(
        &fixture.prefix,
        &fixture.victory,
        &changed_step8,
        &fixture.types,
        &fixture.columns,
    );
    assert!(matches!(
        bind_dynamic_children_frontier(changed_previous, &fixture.authority),
        Err(DynamicChildrenFrontierError::RuntimeDisagreement {
            slot,
            field: "Personality",
            ..
        }) if slot == fixture.active
    ));
}

#[test]
fn transcript_program_order_is_not_layout_address_order() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let frontier = bind_dynamic_children_frontier(fixture.previous, &fixture.authority).unwrap();
    let names: Vec<_> = frontier.rows()[fixture.active]
        .claims()
        .iter()
        .map(|claim| claim.field)
        .collect();
    assert_eq!(
        names,
        [
            "Personality",
            "tech",
            "tech_at_start",
            "obs_flags",
            "conquest_wonders",
            "conquest_wonders_in_game",
            "conquest_racial_powers",
            "Sites",
            "MakeList",
            "military_trainers",
            "new_rares",
            "oil_patches",
            "production_script",
            "rare",
            "rare_owned",
            "rare_conquest",
            "LeaderDataEncrypt plaintext",
        ]
    );
    assert!(LEADER_DIPLOMACY_BEGIN < 0x6dd4);
}

fn same_frame_sim(fixture: &Fixture) -> Sim {
    let mut sim = Sim::new(0x1234_5678, 4);
    sim.step8 = fixture.step8.clone();
    sim.vic_leaders = fixture.victory.clone();
    for slot in 0..NUM_LEADERS {
        let tech = &mut sim.production_runtime.leaders[slot].tech;
        tech.tech.bytes = fixture.authority.rows[slot].tech.payload;
        tech.counters
            .epoch
            .copy_from_slice(&fixture.authority.rows[slot].economy_plaintext[55..59]);
        tech.counters.ages = fixture.authority.rows[slot].economy_plaintext[59];
        tech.counters.epochs = fixture.authority.rows[slot].economy_plaintext[60];
        tech.counters.discovered = fixture.authority.rows[slot].economy_plaintext[61];
    }
    sim
}

fn same_frame_sim_with_fixed_owner_cohort(fixture: &Fixture) -> Sim {
    let mut sim = same_frame_sim(fixture);
    for production in &mut sim.production_runtime.leaders {
        // The synthetic conditional columns are zeroed. These two runtime arrays use -1 as
        // their real default, so align this fixture explicitly before testing the join.
        production.last_unit_finished.fill(0);
        production.age_stamp.fill(0);
    }
    sim
}

#[test]
fn same_frame_owner_offsets_match_the_generated_pdb_layout() {
    for (name, offset, size, count) in [
        ("blacken", 0x020c, 4, 1),
        ("city_mark", 0x0408, 4, 1),
        ("cities_captured", 0x0824, 4, 1),
        ("cities_lost", 0x0828, 4, 1),
        ("tribute_sent", 0x0860, 4, 1),
        ("tribute_received", 0x0864, 4, 1),
        ("age_stamp", 0x08f8, 28, 7),
        ("control", 0x0940, 4, 1),
        ("pop", 0x095c, 4, 1),
        ("barracks_queued", 0x0a10, 4, 1),
        ("stable_queued", 0x0a14, 4, 1),
        ("factory_queued", 0x0a18, 4, 1),
        ("combat_queued", 0x0a1c, 4, 1),
        ("dock_queued", 0x0a20, 4, 1),
        ("air_queued", 0x0a24, 4, 1),
        ("reg_pop", 0x0e62, 128, 64),
        ("num_units", 0x5762, 704, 352),
        ("num_queued", 0x5a22, 1_612, 806),
        ("last_unit_finished", 0x6274, 1_408, 352),
        ("ages_queued", 0x67f4, 1, 1),
        ("epochs_queued", 0x67f5, 1, 1),
    ] {
        let field = leader::FIELDS
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("generated LeaderData has no {name}"));
        assert_eq!(field.offset, offset, "{name} offset");
        assert_eq!(field.size, size, "{name} size");
        assert_eq!(field.count, count, "{name} element count");
        assert_eq!(field.walked, Some(true), "{name} checksum visitation");
    }
}

#[test]
fn same_frame_sim_tech_cohort_promotes_only_six_counter_dwords() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let sim = same_frame_sim(&fixture);
    let joined = bind_sim_tech_frontier(fixture.previous, &fixture.authority, &sim).unwrap();
    let walk = joined.walk_frontier();

    assert_eq!(walk.boundary, LeadersWalkBoundary::Complete);
    assert_eq!(joined.claims().len(), fixture.active_count);
    assert_eq!(
        joined.source_produced_walked_bytes(),
        fixture.active_count * SIM_TECH_SOURCE_BYTES
    );
    assert_eq!(
        joined.sim_duplicate_checked_walked_bytes(),
        fixture.active_count * SIM_TECH_EXISTING_DUPLICATE_BYTES
    );
    assert_eq!(
        joined.newly_canonicalized_walked_bytes(),
        fixture.active_count * SIM_TECH_NEWLY_CANONICAL_BYTES
    );
    assert_eq!(
        joined.remaining_dynamic_conditionally_admitted_walked_bytes(),
        fixture.active_count * (350 - SIM_TECH_NEWLY_CANONICAL_BYTES)
    );
    assert_eq!(joined.checksum(), Err(walk));
    assert!(!joined.installed_in_scoreboard());
}

#[test]
fn canonical_gain_tech_mutation_changes_the_complete_walk_and_stale_views_refuse() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let baseline_sim = same_frame_sim(&fixture);
    let baseline =
        bind_sim_tech_frontier(fixture.previous.clone(), &fixture.authority, &baseline_sim)
            .unwrap()
            .walk_frontier();

    let type_index = 100i32;
    let mut changed_sim = same_frame_sim(&fixture);
    changed_sim.production_runtime.leaders[fixture.active]
        .tech
        .gain(type_index);

    assert!(matches!(
        bind_sim_tech_frontier(
            fixture.previous.clone(),
            &fixture.authority,
            &changed_sim,
        ),
        Err(SimTechFrontierError::TechPayloadDisagreement { slot, .. })
            if slot == fixture.active
    ));

    let mut changed_authority = fixture.authority.clone();
    changed_authority.rows[fixture.active].tech.payload = changed_sim.production_runtime.leaders
        [fixture.active]
        .tech
        .tech
        .bytes;
    changed_authority.rows[fixture.active].economy_plaintext[ECON_DISCOVERED_INDEX] = 1;
    changed_sim.vic_leaders.slots[fixture.active].has_tech[type_index as usize] = true;
    let changed_previous = deferred_frontier(
        &fixture.prefix,
        &changed_sim.vic_leaders,
        &changed_sim.step8,
        &fixture.types,
        &fixture.columns,
    );
    let changed = bind_sim_tech_frontier(changed_previous, &changed_authority, &changed_sim)
        .unwrap()
        .walk_frontier();
    assert_eq!(changed.bytes_walked, baseline.bytes_walked);
    assert_ne!(changed.checksum, baseline.checksum);

    let mut stale_authority = fixture.authority.clone();
    stale_authority.rows[fixture.active].economy_plaintext[ECON_DISCOVERED_INDEX] = 1;
    assert!(matches!(
        bind_sim_tech_frontier(fixture.previous.clone(), &stale_authority, &baseline_sim),
        Err(SimTechFrontierError::CounterDisagreement {
            slot,
            field: "discovered",
            ..
        }) if slot == fixture.active
    ));

    let mut age_split = same_frame_sim(&fixture);
    age_split.production_runtime.leaders[fixture.active]
        .tech
        .counters
        .ages = 1;
    assert_eq!(
        bind_sim_tech_frontier(fixture.previous, &fixture.authority, &age_split),
        Err(SimTechFrontierError::AgeOwnerDisagreement {
            slot: fixture.active,
            production: 1,
            step8: 0,
        })
    );
}

#[test]
fn same_frame_owner_cohort_is_large_bounded_and_stays_red() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let sim = same_frame_sim_with_fixed_owner_cohort(&fixture);
    let tech = bind_sim_tech_frontier(fixture.previous, &fixture.authority, &sim).unwrap();
    let joined = bind_sim_owner_frontier(&fixture.prefix, tech, &sim).unwrap();
    let walk = joined.walk_frontier();

    assert_eq!(walk.boundary, LeadersWalkBoundary::Complete);
    assert_eq!(joined.claims().len(), fixture.active_count * 17);
    assert_eq!(
        joined.cohort_source_produced_walked_bytes(),
        fixture.active_count * SIM_OWNER_SOURCE_BYTES
    );
    assert_eq!(
        joined.cohort_duplicate_checked_walked_bytes(),
        fixture.active_count * SIM_OWNER_DUPLICATE_BYTES
    );
    assert_eq!(
        joined.cohort_newly_canonicalized_walked_bytes(),
        fixture.active_count * SIM_OWNER_NEWLY_CANONICAL_BYTES
    );
    assert_eq!(
        joined.unique_canonical_walked_bytes(),
        fixture.active_count * 6_498 + (NUM_LEADERS - fixture.active_count) * 8
    );
    assert_eq!(
        joined.remaining_unsourced_walked_bytes(),
        walk.bytes_walked - joined.unique_canonical_walked_bytes() as u64
    );
    assert_eq!(joined.checksum(), Err(walk));
    assert!(!joined.installed_in_scoreboard());

    let checked = don_replay::check_all::CheckAll::of_state(&don_replay::state::SimState::new());
    let leaders = &checked.per[Channel::Leaders as usize];
    assert!(!leaders.installed);
    assert!(!leaders.exact_producer);
    assert!(!leaders.substantive());
}

#[test]
fn same_frame_owner_mutation_bites_walk_only_after_conditional_owner_agrees() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let baseline_sim = same_frame_sim_with_fixed_owner_cohort(&fixture);
    let baseline_tech =
        bind_sim_tech_frontier(fixture.previous.clone(), &fixture.authority, &baseline_sim)
            .unwrap();
    let baseline = bind_sim_owner_frontier(&fixture.prefix, baseline_tech, &baseline_sim)
        .unwrap()
        .walk_frontier();

    let mut changed_sim = same_frame_sim_with_fixed_owner_cohort(&fixture);
    changed_sim.production_runtime.leaders[fixture.active].population = 7;

    let stale_tech =
        bind_sim_tech_frontier(fixture.previous.clone(), &fixture.authority, &changed_sim).unwrap();
    assert!(matches!(
        bind_sim_owner_frontier(&fixture.prefix, stale_tech, &changed_sim),
        Err(SimOwnerFrontierError::ConditionalDisagreement {
            slot,
            field: "pop",
            ..
        }) if slot == fixture.active
    ));

    let mut changed_columns = fixture.columns.clone();
    let pop = leader::FIELDS
        .iter()
        .find(|field| field.offset as usize == POP_OFFSET && field.name == "pop")
        .unwrap();
    write_field(
        &mut changed_columns,
        fixture.active,
        pop,
        &7i32.to_le_bytes(),
    );
    let changed_previous = deferred_frontier(
        &fixture.prefix,
        &fixture.victory,
        &fixture.step8,
        &fixture.types,
        &changed_columns,
    );
    let changed_tech =
        bind_sim_tech_frontier(changed_previous, &fixture.authority, &changed_sim).unwrap();
    let changed = bind_sim_owner_frontier(&fixture.prefix, changed_tech, &changed_sim)
        .unwrap()
        .walk_frontier();
    assert_eq!(changed.bytes_walked, baseline.bytes_walked);
    assert_ne!(changed.checksum, baseline.checksum);
}

#[test]
fn last_finished_tail_mutation_bites_only_after_the_independent_column_agrees() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let baseline_sim = same_frame_sim_with_fixed_owner_cohort(&fixture);
    let baseline_tech =
        bind_sim_tech_frontier(fixture.previous.clone(), &fixture.authority, &baseline_sim)
            .unwrap();
    let baseline = bind_sim_owner_frontier(&fixture.prefix, baseline_tech, &baseline_sim)
        .unwrap()
        .walk_frontier();

    let tail = REGULAR_UNIT_END - REGULAR_UNIT_BEGIN - 1;
    let value = 0x1234_5678i32;
    let mut changed_sim = same_frame_sim_with_fixed_owner_cohort(&fixture);
    changed_sim.production_runtime.leaders[fixture.active].last_unit_finished[tail] = value;

    let stale_tech =
        bind_sim_tech_frontier(fixture.previous.clone(), &fixture.authority, &changed_sim).unwrap();
    assert!(matches!(
        bind_sim_owner_frontier(&fixture.prefix, stale_tech, &changed_sim),
        Err(SimOwnerFrontierError::ConditionalDisagreement {
            slot,
            field: "last_unit_finished",
            ..
        }) if slot == fixture.active
    ));

    let mut changed_columns = fixture.columns.clone();
    let field = leader::FIELDS
        .iter()
        .find(|field| field.name == "last_unit_finished")
        .unwrap();
    let mut field_bytes = vec![0; field.size as usize];
    field_bytes[tail * 4..tail * 4 + 4].copy_from_slice(&value.to_le_bytes());
    write_field(&mut changed_columns, fixture.active, field, &field_bytes);
    let changed_previous = deferred_frontier(
        &fixture.prefix,
        &fixture.victory,
        &fixture.step8,
        &fixture.types,
        &changed_columns,
    );
    let changed_tech =
        bind_sim_tech_frontier(changed_previous, &fixture.authority, &changed_sim).unwrap();
    let changed = bind_sim_owner_frontier(&fixture.prefix, changed_tech, &changed_sim)
        .unwrap()
        .walk_frontier();
    assert_eq!(changed.bytes_walked, baseline.bytes_walked);
    assert_ne!(changed.checksum, baseline.checksum);
}

#[test]
fn same_frame_owner_duplicate_and_shape_disagreements_refuse() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };

    let mut stale_counts = same_frame_sim_with_fixed_owner_cohort(&fixture);
    stale_counts.production_runtime.leaders[fixture.active].unit_counts[REGULAR_UNIT_BEGIN] = 1;
    let stale_counts_tech =
        bind_sim_tech_frontier(fixture.previous.clone(), &fixture.authority, &stale_counts)
            .unwrap();
    assert!(matches!(
        bind_sim_owner_frontier(&fixture.prefix, stale_counts_tech, &stale_counts),
        Err(SimOwnerFrontierError::RuntimeDuplicateDisagreement {
            slot,
            field: "num_units",
            ..
        }) if slot == fixture.active
    ));

    let mut short_history = same_frame_sim_with_fixed_owner_cohort(&fixture);
    short_history.production_runtime.leaders[fixture.active]
        .last_unit_finished
        .pop();
    let short_history_tech =
        bind_sim_tech_frontier(fixture.previous.clone(), &fixture.authority, &short_history)
            .unwrap();
    assert!(matches!(
        bind_sim_owner_frontier(&fixture.prefix, short_history_tech, &short_history),
        Err(SimOwnerFrontierError::SourceLength {
            slot,
            source: "production_runtime.last_unit_finished",
            ..
        }) if slot == fixture.active
    ));

    let mut negative_count = same_frame_sim_with_fixed_owner_cohort(&fixture);
    negative_count.production_runtime.leaders[fixture.active].unit_counts[REGULAR_UNIT_BEGIN] = -1;
    let negative_count_tech =
        bind_sim_tech_frontier(fixture.previous, &fixture.authority, &negative_count).unwrap();
    assert_eq!(
        bind_sim_owner_frontier(&fixture.prefix, negative_count_tech, &negative_count),
        Err(SimOwnerFrontierError::SourceValueOutOfRange {
            slot: fixture.active,
            field: "num_units",
            index: 0,
            value: -1,
        })
    );
}

fn first_frame_zero_setup() -> Option<(InitialLeaderPrefix, StartingSetupState, Replay)> {
    for path in corpus(&repo_root()) {
        let replay = match Replay::open(&path) {
            Ok(replay) => replay,
            Err(_) => continue,
        };
        let prefix = match derive(&replay.initial) {
            Ok(prefix) => prefix,
            Err(_) => continue,
        };
        let mut world = WorldSim::from_replay(&replay);
        if let Some(setup) = world.initial_setup.take() {
            return Some((prefix, setup, replay));
        }
    }
    None
}

fn synchronize_build_registry_columns(
    columns: &mut LeaderCols,
    census: &don_replay::leaders_setup_build_registry_frontier::FrameZeroBuildRegistryCensus,
    slot: usize,
) {
    let mut write_u16_field = |name: &str, values: &[u16]| {
        let field = leader::FIELDS
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("generated LeaderData has no {name}"));
        let bytes: Vec<_> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        write_field(columns, slot, field, &bytes);
    };
    let registries = census.regional_registries(slot).unwrap();
    write_u16_field("reg_cities", &registries[..64]);
    write_u16_field("reg_forts", &registries[64..128]);
    write_u16_field("reg_docks", &registries[128..]);
    write_u16_field("high_buildings", census.high_buildings(slot).unwrap());
}

fn synchronize_setup_only_owner_columns(columns: &mut LeaderCols, sim: &Sim, slot: usize) {
    let mut write_named = |name: &str, bytes: &[u8]| {
        let field = leader::FIELDS
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("generated LeaderData has no {name}"));
        write_field(columns, slot, field, bytes);
    };

    write_named("city_mark", &sim.cities.city_mark[slot].to_le_bytes());
    let mut age_stamp = Vec::with_capacity(7 * 4);
    for value in sim.production_runtime.leaders[slot].age_stamp {
        age_stamp.extend_from_slice(&value.to_le_bytes());
    }
    write_named("age_stamp", &age_stamp);
    let mut last_finished = Vec::with_capacity((REGULAR_UNIT_END - REGULAR_UNIT_BEGIN) * 4);
    for value in &sim.production_runtime.leaders[slot].last_unit_finished {
        last_finished.extend_from_slice(&value.to_le_bytes());
    }
    write_named("last_unit_finished", &last_finished);
    let mut last_building_finished = Vec::with_capacity(129 * 4);
    for _ in 0..129 {
        last_building_finished.extend_from_slice(&(-1i32).to_le_bytes());
    }
    write_named("last_building_finished", &last_building_finished);
}

#[test]
fn frame_zero_starting_build_census_promotes_the_dominant_residual_and_stays_red() {
    std::thread::Builder::new()
        .name("leaders-frame-zero-census".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(frame_zero_starting_build_census_body)
        .expect("spawn large frame-zero proof stack")
        .join()
        .expect("frame-zero proof thread panicked");
}

fn frame_zero_starting_build_census_body() {
    let Some((prefix, mut setup, replay)) = first_frame_zero_setup() else {
        skip("no replay admits the canonical all-land starting-town-one setup");
        return;
    };
    let census = derive_frame_zero_regional_building_census(&setup).unwrap();
    let history = derive_frame_zero_last_building_history(&setup).unwrap();
    let region_history = derive_frame_zero_region_strategy_history(&setup).unwrap();
    let build_registry = derive_frame_zero_build_registry_census(&setup, &replay).unwrap();
    let active_count = prefix.rows.iter().filter(|row| row.active).count();

    assert_eq!(WALL_INCREMENT_STATS_VA, 0x0064_3270);
    assert_eq!(WALL_INCREMENT_STATS_TOTAL_STORE_VA, 0x0064_32e2);
    assert_eq!(WALL_INCREMENT_STATS_REGION_STORE_VA, 0x0064_3307);
    assert_eq!(FRAME_ZERO_REG_BUILDINGS_WALKED_BYTES, 16_512);
    assert_eq!(LEADER_INIT_LAST_BUILDING_HISTORY_VA, 0x006e_4bef);
    assert_eq!(LEADER_INIT_LAST_BUILDING_HISTORY_FILL_VA, 0x006e_4bfa);
    assert_eq!(SETUP_BUILD_ACTIVATE_VIRTUAL_CALL_VA, 0x005a_babf);
    assert_eq!(BUILD_ACTIVATE_VA, 0x0062_3e20);
    assert_eq!(BUILD_ACTIVATE_LAST_BUILDING_GUARD_VA, 0x0062_3f2d);
    assert_eq!(BUILD_ACTIVATE_LAST_BUILDING_STORE_VA, 0x0062_3f47);
    assert_eq!(FRAME_ZERO_LAST_BUILDING_HISTORY_WALKED_BYTES, 516);
    assert_eq!(LEADER_INIT_REGION_HISTORY_BEGIN_VA, 0x006e_4b18);
    assert_eq!(LEADER_INIT_REGION_HISTORY_LOOP_END_VA, 0x006e_4ba2);
    assert_eq!(PLAN_STRATEGY_REG_ATTACKED_FIRST_STORE_VA, 0x006b_9bf8);
    assert_eq!(PLAN_STRATEGY_RELATION_HISTORY_CLEAR_VA, 0x006b_bb82);
    assert_eq!(PLAN_STRATEGY_RELATION_HISTORY_LAST_STORE_VA, 0x006b_bcc2);
    assert_eq!(FRAME_ZERO_REG_STRATEGY_HISTORY_WALKED_BYTES, 256);
    assert_eq!(BUILD_ACTIVATE_HIGH_WATER_BEGIN_VA, 0x0062_4ba4);
    assert_eq!(BUILD_ACTIVATE_TO_LOAD_VA, 0x0062_4baa);
    assert_eq!(BUILD_ACTIVATE_HIGH_WATER_END_VA, 0x0062_4c16);
    assert_eq!(LEADER_GET_BUILDINGS_VA, 0x006e_0680);
    assert_eq!(BUILD_TYPE_BASIC_TYPE_VA, 0x0063_9970);
    assert_eq!(FORT_INIT_REGISTRY_STORE_VA, 0x0073_eb5f);
    assert_eq!(DOCK_INIT_REGISTRY_STORE_VA, 0x0074_0b17);
    assert_eq!(FRAME_ZERO_HIGH_BUILDINGS_WALKED_BYTES, 258);
    assert_eq!(FRAME_ZERO_BUILD_REGISTRY_WALKED_BYTES, 384);
    assert_eq!(FRAME_ZERO_BUILD_CENSUS_WALKED_BYTES, 642);
    assert_eq!(census.claims().len(), active_count);
    assert_eq!(history.claims().len(), active_count);
    assert_eq!(region_history.claims().len(), active_count);
    assert_eq!(build_registry.claims().len(), active_count);
    for claim in build_registry.claims() {
        let slot = usize::from(claim.slot);
        assert_eq!(claim.basic_type_chain.first(), Some(&414));
        assert_eq!(claim.basic_type, 414);
        assert_eq!(claim.upgrade_chain.first(), Some(&414));
        assert_eq!(claim.newly_canonical_walked_bytes, 642);
        let high = build_registry.high_buildings(slot).unwrap();
        assert_eq!(high[0], 1);
        assert_eq!(
            high.iter().map(|value| usize::from(*value)).sum::<usize>(),
            1
        );
        let registries = build_registry.regional_registries(slot).unwrap();
        assert_eq!(registries[1], 1);
        assert_eq!(
            registries
                .iter()
                .map(|value| usize::from(*value))
                .sum::<usize>(),
            1
        );
    }
    for claim in region_history.claims() {
        assert_eq!(claim.regions_per_history, 64);
        assert_eq!(claim.histories, 4);
        assert_eq!(claim.newly_canonical_walked_bytes, 256);
        assert_eq!(
            region_history.row(usize::from(claim.slot)).unwrap(),
            &[0; 256]
        );
    }
    for claim in history.claims() {
        assert_eq!(claim.entries, 129);
        assert_eq!(claim.newly_canonical_walked_bytes, 516);
        assert_eq!(history.row(usize::from(claim.slot)).unwrap(), &[-1; 129]);
    }
    for claim in census.claims() {
        assert_eq!(claim.builds_censused, 1);
        assert_eq!(claim.newly_canonical_walked_bytes, 16_512);
        let row = census.row(usize::from(claim.slot)).unwrap();
        assert_eq!(row.len(), 64 * 129);
        let village = 1 * REG_BUILDING_TYPE_SLOTS;
        assert_eq!(row[village], 1);
        assert_eq!(
            row.iter().map(|value| usize::from(*value)).sum::<usize>(),
            1
        );
    }

    let victory = setup.sim.vic_leaders.clone();
    let step8 = setup.sim.step8.clone();
    let mut fixture = fixture_with_states(prefix, victory, step8);
    let mut mask_types = fixture.types.clone();
    for slot in 0..NUM_LEADERS {
        mask_types.leaders[slot].tech.bytes = fixture.authority.rows[slot].tech.payload;
        mask_types.leaders[slot].obs_flags.bytes = fixture.authority.rows[slot].obs_flags.payload;
    }
    for slot in 0..NUM_LEADERS {
        synchronize_setup_only_owner_columns(&mut fixture.columns, &setup.sim, slot);
        synchronize_build_registry_columns(&mut fixture.columns, &build_registry, slot);
    }
    let mut fixed = DeferredLeadersFixedAuthority::default();
    for slot in 0..NUM_LEADERS {
        fixed.rows[slot]
            .reg_buildings_by_region_then_type
            .copy_from_slice(census.row(slot).unwrap());
    }
    let previous = deferred_frontier_with_authority(
        &fixture.prefix,
        &fixture.victory,
        &fixture.step8,
        &fixture.types,
        &fixture.columns,
        &fixed,
    );
    let tech = bind_sim_tech_frontier(previous, &fixture.authority, &setup.sim).unwrap();
    let owners = bind_sim_owner_frontier(&fixture.prefix, tech, &setup.sim).unwrap();
    let regional = bind_frame_zero_regional_buildings(owners, census.clone()).unwrap();
    let build_history = bind_frame_zero_last_building_history(regional, history.clone()).unwrap();
    let region_joined =
        bind_frame_zero_region_strategy_history(build_history, region_history.clone()).unwrap();
    let registry_joined =
        bind_frame_zero_build_registry_census(region_joined, build_registry.clone()).unwrap();
    let joined = bind_type_mask_owner(registry_joined, &mask_types).unwrap();
    let walk = joined.walk_frontier();

    assert_eq!(
        joined.newly_canonicalized_walked_bytes(),
        active_count * 117
    );
    assert_eq!(
        joined.unique_canonical_walked_bytes(),
        active_count * 24_541 + (NUM_LEADERS - active_count) * 8
    );
    assert_eq!(
        joined.remaining_unsourced_walked_bytes(),
        (active_count * 3_887) as u64
    );
    assert_eq!(joined.checksum(), Err(walk));
    assert!(!joined.installed_in_scoreboard());

    let active = fixture.active;
    assert_eq!(TYPE_MASK_HEADER_WALKED_BYTES, 8);
    assert_eq!(OBS_FLAGS_WALKED_BYTES, 109);
    assert_eq!(TYPE_MASK_NEWLY_CANONICAL_WALKED_BYTES, 117);
    assert_eq!(joined.claims().len(), active_count);
    assert_eq!(joined.claims()[0].tech_duplicate_payload_bytes, 101);

    let mut stale_masks = mask_types.clone();
    stale_masks.leaders[active].obs_flags.bytes[100] ^= 1;
    let stale_mask_previous = deferred_frontier_with_authority(
        &fixture.prefix,
        &fixture.victory,
        &fixture.step8,
        &fixture.types,
        &fixture.columns,
        &fixed,
    );
    let stale_mask_tech =
        bind_sim_tech_frontier(stale_mask_previous, &fixture.authority, &setup.sim).unwrap();
    let stale_mask_owners =
        bind_sim_owner_frontier(&fixture.prefix, stale_mask_tech, &setup.sim).unwrap();
    let stale_mask_regional =
        bind_frame_zero_regional_buildings(stale_mask_owners, census.clone()).unwrap();
    let stale_mask_history =
        bind_frame_zero_last_building_history(stale_mask_regional, history.clone()).unwrap();
    let stale_mask_regions =
        bind_frame_zero_region_strategy_history(stale_mask_history, region_history.clone())
            .unwrap();
    let stale_mask_registry =
        bind_frame_zero_build_registry_census(stale_mask_regions, build_registry.clone()).unwrap();
    assert!(matches!(
        bind_type_mask_owner(stale_mask_registry, &stale_masks),
        Err(TypeMaskOwnerError::TranscriptDisagreement {
            slot,
            field: "obs_flags",
            byte: 108,
            ..
        }) if slot == active
    ));

    let high_field = leader::FIELDS
        .iter()
        .find(|field| field.name == "high_buildings")
        .unwrap();
    let mut stale_high_columns = fixture.columns.clone();
    let mut stale_high_bytes: Vec<_> = build_registry
        .high_buildings(active)
        .unwrap()
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    *stale_high_bytes.last_mut().unwrap() = 1;
    write_field(
        &mut stale_high_columns,
        active,
        high_field,
        &stale_high_bytes,
    );
    let stale_high_previous = deferred_frontier_with_authority(
        &fixture.prefix,
        &fixture.victory,
        &fixture.step8,
        &fixture.types,
        &stale_high_columns,
        &fixed,
    );
    let stale_high_tech =
        bind_sim_tech_frontier(stale_high_previous, &fixture.authority, &setup.sim).unwrap();
    let stale_high_owners =
        bind_sim_owner_frontier(&fixture.prefix, stale_high_tech, &setup.sim).unwrap();
    let stale_high_regional =
        bind_frame_zero_regional_buildings(stale_high_owners, census.clone()).unwrap();
    let stale_high_history =
        bind_frame_zero_last_building_history(stale_high_regional, history.clone()).unwrap();
    let stale_high_regions =
        bind_frame_zero_region_strategy_history(stale_high_history, region_history.clone())
            .unwrap();
    assert!(matches!(
        bind_frame_zero_build_registry_census(stale_high_regions, build_registry.clone()),
        Err(FrameZeroBuildRegistryError::ConditionalDisagreement {
            slot,
            field: "high_buildings",
            ..
        }) if slot == active
    ));

    let region_history_field = leader::FIELDS
        .iter()
        .find(|field| field.name == "reg_allies")
        .unwrap();
    let mut stale_region_columns = fixture.columns.clone();
    let mut stale_region_bytes = vec![0; region_history_field.size as usize];
    stale_region_bytes[region_history_field.size as usize - 1] = 1;
    write_field(
        &mut stale_region_columns,
        active,
        region_history_field,
        &stale_region_bytes,
    );
    let stale_region_previous = deferred_frontier_with_authority(
        &fixture.prefix,
        &fixture.victory,
        &fixture.step8,
        &fixture.types,
        &stale_region_columns,
        &fixed,
    );
    let stale_region_tech =
        bind_sim_tech_frontier(stale_region_previous, &fixture.authority, &setup.sim).unwrap();
    let stale_region_owners =
        bind_sim_owner_frontier(&fixture.prefix, stale_region_tech, &setup.sim).unwrap();
    let stale_region_regional =
        bind_frame_zero_regional_buildings(stale_region_owners, census.clone()).unwrap();
    let stale_region_build_history =
        bind_frame_zero_last_building_history(stale_region_regional, history.clone()).unwrap();
    assert!(matches!(
        bind_frame_zero_region_strategy_history(stale_region_build_history, region_history),
        Err(FrameZeroRegionHistoryError::ConditionalDisagreement { slot, .. })
            if slot == active
    ));
    let history_field = leader::FIELDS
        .iter()
        .find(|field| field.name == "last_building_finished")
        .unwrap();
    let mut stale_history_columns = fixture.columns.clone();
    let mut stale_history_bytes = vec![0xff; history_field.size as usize];
    stale_history_bytes[history_field.size as usize - 1] = 0xfe;
    write_field(
        &mut stale_history_columns,
        active,
        history_field,
        &stale_history_bytes,
    );
    let stale_history_previous = deferred_frontier_with_authority(
        &fixture.prefix,
        &fixture.victory,
        &fixture.step8,
        &fixture.types,
        &stale_history_columns,
        &fixed,
    );
    let stale_history_tech =
        bind_sim_tech_frontier(stale_history_previous, &fixture.authority, &setup.sim).unwrap();
    let stale_history_owners =
        bind_sim_owner_frontier(&fixture.prefix, stale_history_tech, &setup.sim).unwrap();
    let stale_history_regional =
        bind_frame_zero_regional_buildings(stale_history_owners, census.clone()).unwrap();
    assert!(matches!(
        bind_frame_zero_last_building_history(stale_history_regional, history),
        Err(FrameZeroBuildHistoryError::ConditionalDisagreement { slot, .. })
            if slot == active
    ));

    let mut stale_fixed = fixed;
    stale_fixed.rows[active].reg_buildings_by_region_then_type[REG_BUILDING_TYPE_SLOTS] = 0;
    let stale_previous = deferred_frontier_with_authority(
        &fixture.prefix,
        &fixture.victory,
        &fixture.step8,
        &fixture.types,
        &fixture.columns,
        &stale_fixed,
    );
    let stale_tech =
        bind_sim_tech_frontier(stale_previous, &fixture.authority, &setup.sim).unwrap();
    let stale_owners = bind_sim_owner_frontier(&fixture.prefix, stale_tech, &setup.sim).unwrap();
    assert!(matches!(
        bind_frame_zero_regional_buildings(stale_owners, census),
        Err(FrameZeroRegBuildingsError::ConditionalDisagreement { slot, .. })
            if slot == active
    ));

    setup.sim.builds.push(Default::default());
    assert_eq!(
        derive_frame_zero_regional_building_census(&setup),
        Err(FrameZeroRegBuildingsError::SetupCountDisagreement {
            receipt_active: active_count,
            city_receipts: active_count,
            sim_builds: active_count + 1,
        })
    );
    setup.sim.builds.pop();

    setup.receipt.cities[0].constructor.build_init_complete = true;
    assert_eq!(
        derive_frame_zero_last_building_history(&setup),
        Err(FrameZeroBuildHistoryError::SetupActivationChronologyDisagreement { receipt: 0 })
    );
    setup.receipt.cities[0].constructor.build_init_complete = false;

    setup.receipt.replay_payload_sha256[0] ^= 1;
    assert_eq!(
        derive_frame_zero_build_registry_census(&setup, &replay),
        Err(FrameZeroBuildRegistryError::ReplaySourceDisagreement)
    );
    setup.receipt.replay_payload_sha256[0] ^= 1;

    setup.receipt.cities[0]
        .constructor
        .effects
        .leader_region_cities_delta = 0;
    assert!(matches!(
        derive_frame_zero_build_registry_census(&setup, &replay),
        Err(FrameZeroBuildRegistryError::RegionalCityAgreement {
            constructor_delta: 0,
            ..
        })
    ));
    setup.receipt.cities[0]
        .constructor
        .effects
        .leader_region_cities_delta = 1;

    let row = setup.receipt.cities[0].build.row;
    setup.sim.builds[row].orig_type += 1;
    assert_eq!(
        derive_frame_zero_regional_building_census(&setup),
        Err(FrameZeroRegBuildingsError::BuildIdentityDisagreement { receipt: 0 })
    );

    let checked = don_replay::check_all::CheckAll::of_state(&don_replay::state::SimState::new());
    let leaders = &checked.per[Channel::Leaders as usize];
    assert!(!leaders.installed);
    assert!(!leaders.exact_producer);
    assert!(!leaders.substantive());
}
