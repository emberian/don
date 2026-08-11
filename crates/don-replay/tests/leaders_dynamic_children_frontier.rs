//! Full conditional Leader child transcript, representation, and agreement gates.

use don_replay::checksum::Channel;
use don_replay::leader_initial_prefix::{derive, InitialLeaderPrefix};
use don_replay::leaders_deferred_history_frontier::{
    bind_deferred_history_frontier, DeferredLeadersFixedAuthority,
    RuntimeLeadersDeferredHistoryFrontier,
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
use don_replay::replay::{corpus, Replay};
use don_sim::generated::state::{leader, FieldDesc, LeaderCols, Pool};
use don_sim::systems::bhs_type_table::{
    LeaderTypeMasks, TribeRoster, TypeBackup, TypeBuiltinState, TypeRow, TypeTable, BUILD_BEGIN,
    BUILD_END, NUM_LEADERS, NUM_TRIBES, NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
};
use don_sim::systems::{leaders, victory_score};
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
    let base = bind_live(prefix, victory, step8).expect("runtime owners agree");
    let tribes = bind_live_tribes(prefix, base, types).expect("tribe owner agrees");
    let generated = bind_generated_fixed_prefix(tribes, columns).expect("columns agree");
    bind_deferred_history_frontier(
        generated,
        columns,
        &DeferredLeadersFixedAuthority::default(),
    )
    .expect("deferred history agrees")
}

fn fixture() -> Option<Fixture> {
    let prefix = first_prefix()?;
    let active = prefix.rows.iter().position(|row| row.active)?;
    let active_count = prefix.rows.iter().filter(|row| row.active).count();
    let (victory, step8) = current_states(&prefix);
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

    Some(Fixture {
        prefix,
        victory,
        step8,
        types,
        columns,
        previous,
        authority,
        active,
        active_count,
    })
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
