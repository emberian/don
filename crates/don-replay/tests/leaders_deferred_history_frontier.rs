//! Deferred regional-building representation, history, and child-walk gates.

mod leader_initial_prefix {
    pub use don_replay::leader_initial_prefix::*;
}

mod leaders_runtime_frontier {
    pub use don_replay::leaders_runtime_frontier::*;
}

mod leaders_generated_fixed_frontier {
    pub use don_replay::leaders_generated_fixed_frontier::*;
}

#[path = "../src/leaders_deferred_history_frontier.rs"]
mod leaders_deferred_history_frontier;

use don_replay::checksum::Channel;
use don_replay::leader_initial_prefix::{derive, InitialLeaderPrefix};
use don_replay::leaders_generated_fixed_frontier::{
    bind_generated_fixed_prefix, RuntimeLeadersGeneratedFixedFrontier,
};
use don_replay::leaders_runtime_frontier::{bind_live, LeadersWalkBoundary, RuntimeCoveredRange};
use don_replay::leaders_runtime_tribe_frontier::{
    bind_live_tribes, RuntimeLeadersTribeFrontier, LEADER_TRIBE_OFFSET,
};
use don_replay::replay::{corpus, Replay};
use don_sim::generated::state::{leader, FieldDesc, LeaderCols, Pool, Repr};
use don_sim::systems::bhs_type_table::{
    LeaderTypeMasks, TribeRoster, TypeBackup, TypeBuiltinState, TypeRow, TypeTable, BUILD_BEGIN,
    BUILD_END, NUM_LEADERS, NUM_TRIBES, NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
};
use don_sim::systems::{leaders, victory_score};
use leaders_deferred_history_frontier::{
    bind_deferred_history_frontier, DeferredHistoryFrontierError, DeferredHistoryProvenance,
    DeferredLeadersFixedAuthority, CTW_PADDING_BEGIN, CTW_PADDING_END,
    DEFERRED_FIXED_EXTENSION_BYTES, DIPLOMACY_EXTENSION_BYTES, LEADER_DIPLOMACY_END,
    LEADER_GET_REG_BUILDINGS_VA, LEADER_INIT_REG_BUILDINGS_VA, LEADER_INIT_VA,
    LEADER_WALK_DIPLOMACY_CALL_VA, LEADER_WALK_PERSONALITY_CALL_VA, NUM_QUEUED_BEGIN,
    NUM_QUEUED_END, NUM_QUEUED_VALUES, QUEUE_PADDING_BEGIN, QUEUE_PADDING_END, REG_BUILDINGS_BEGIN,
    REG_BUILDINGS_END, REG_BUILDING_REGIONS, REG_BUILDING_TYPE_SLOTS, REG_BUILDING_VALUES,
};
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
    for path in corpus(&repo_root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        if let Ok(prefix) = derive(&replay.initial) {
            return Some(prefix);
        }
    }
    None
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

fn tribe_frontier(
    prefix: &InitialLeaderPrefix,
    victory: &victory_score::Leaders,
    step8: &leaders::Leaders,
    types: &TypeBuiltinState,
) -> RuntimeLeadersTribeFrontier {
    let base = bind_live(prefix, victory, step8).expect("runtime owners agree");
    bind_live_tribes(prefix, base, types).expect("live tribe owner agrees")
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

fn synchronize_runtime_duplicates(
    previous: &RuntimeLeadersTribeFrontier,
    columns: &mut LeaderCols,
) {
    for slot in 0..NUM_LEADERS {
        let base = &previous.base().rows[slot];
        for field in &leader::FIELDS {
            let begin = field.offset as usize;
            let end = begin + field.size as usize;
            if field.alias_of.is_some() || !field.repr.materialised() {
                continue;
            }
            if let Some(bytes) = base.owned_slice(RuntimeCoveredRange { begin, end }) {
                write_field(columns, slot, field, bytes);
            }
        }
        if base.active {
            let tribe = previous.tribe_for(slot).unwrap().to_le_bytes();
            let field = leader::FIELDS
                .iter()
                .find(|field| field.offset as usize == LEADER_TRIBE_OFFSET)
                .unwrap();
            write_field(columns, slot, field, &tribe);
        }
    }
}

fn generated_frontier(
    prefix: &InitialLeaderPrefix,
    victory: &victory_score::Leaders,
    step8: &leaders::Leaders,
    types: &TypeBuiltinState,
    columns: &LeaderCols,
) -> RuntimeLeadersGeneratedFixedFrontier {
    let tribes = tribe_frontier(prefix, victory, step8, types);
    bind_generated_fixed_prefix(tribes, columns).expect("generated prefix agrees")
}

struct Fixture {
    prefix: InitialLeaderPrefix,
    victory: victory_score::Leaders,
    step8: leaders::Leaders,
    types: TypeBuiltinState,
    columns: LeaderCols,
    previous: RuntimeLeadersGeneratedFixedFrontier,
    authority: DeferredLeadersFixedAuthority,
    active: usize,
}

fn fixture() -> Option<Fixture> {
    let prefix = first_prefix()?;
    let active = prefix.rows.iter().position(|row| row.active)?;
    let (victory, step8) = current_states(&prefix);
    let types = type_state(&prefix);
    let tribes = tribe_frontier(&prefix, &victory, &step8, &types);
    let mut columns = zeroed_columns();
    synchronize_runtime_duplicates(&tribes, &mut columns);
    let previous = bind_generated_fixed_prefix(tribes, &columns).unwrap();
    Some(Fixture {
        prefix,
        victory,
        step8,
        types,
        columns,
        previous,
        authority: DeferredLeadersFixedAuthority::default(),
        active,
    })
}

#[test]
fn deferred_matrix_crosses_the_fixed_body_and_all_eight_diplomacy_children() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let previous_walk = fixture.previous.walk_frontier();
    let extended = bind_deferred_history_frontier(
        fixture.previous.clone(),
        &fixture.columns,
        &fixture.authority,
    )
    .unwrap();
    let frontier = extended.walk_frontier();

    assert_eq!(REG_BUILDING_REGIONS, 64);
    assert_eq!(REG_BUILDING_TYPE_SLOTS, 129);
    assert_eq!(REG_BUILDING_VALUES, 8_256);
    assert_eq!(REG_BUILDINGS_BEGIN, 0x14de);
    assert_eq!(REG_BUILDINGS_END, 0x555e);
    assert_eq!(NUM_QUEUED_BEGIN, 0x5a22);
    assert_eq!(NUM_QUEUED_END, 0x606e);
    assert_eq!(NUM_QUEUED_VALUES, 806);
    assert_eq!((QUEUE_PADDING_BEGIN, QUEUE_PADDING_END), (0x606e, 0x6070));
    assert_eq!((CTW_PADDING_BEGIN, CTW_PADDING_END), (0x6922, 0x6924));
    assert_eq!(LEADER_DIPLOMACY_END, 0x6c0c);
    assert_eq!(LEADER_INIT_VA, 0x006e_3930);
    assert_eq!(LEADER_INIT_REG_BUILDINGS_VA, 0x006e_4acd);
    assert_eq!(LEADER_GET_REG_BUILDINGS_VA, 0x006d_5470);
    assert_eq!(LEADER_WALK_DIPLOMACY_CALL_VA, 0x006d_67ac);
    assert_eq!(LEADER_WALK_PERSONALITY_CALL_VA, 0x006d_67c7);

    let reg = leader::FIELDS
        .iter()
        .find(|field| field.name == "reg_buildings")
        .unwrap();
    assert_eq!(reg.ctype, "unsigned short[129][64]");
    assert_eq!(reg.count as usize, REG_BUILDING_VALUES);
    assert_eq!(reg.repr, Repr::Deferred);
    assert_eq!(reg.pool, Pool::None);
    let queued = leader::FIELDS
        .iter()
        .find(|field| field.name == "num_queued")
        .unwrap();
    assert_eq!(queued.count as usize, NUM_QUEUED_VALUES);
    assert_eq!(queued.repr, Repr::Deferred);
    assert_eq!(queued.pool, Pool::None);

    assert_eq!(
        frontier.boundary,
        LeadersWalkBoundary::DynamicChildren {
            slot: fixture.active,
            child: "Personality at +0x6dd4",
        }
    );
    assert_eq!(
        frontier.bytes_walked,
        previous_walk.bytes_walked
            + DEFERRED_FIXED_EXTENSION_BYTES as u64
            + DIPLOMACY_EXTENSION_BYTES as u64
    );
    assert_eq!(
        extended.rows()[fixture.active].conditionally_admitted_walked_bytes(),
        19_004
    );
    assert!(extended.rows()[fixture.active].duplicate_checked_walked_bytes() > 6_000);
    assert!(extended.rows()[fixture.active]
        .claims()
        .iter()
        .any(|claim| {
            claim.field == "reg_buildings"
                && claim.provenance == DeferredHistoryProvenance::DeferredRegionalBuildingAuthority
        }));
    assert!(extended.rows()[fixture.active]
        .claims()
        .iter()
        .any(|claim| {
            claim.field == "num_queued"
                && claim.provenance == DeferredHistoryProvenance::ExistingRuntimeNumQueued
        }));
    assert!(extended.rows()[fixture.active]
        .claims()
        .iter()
        .any(|claim| {
            claim.field == "Diplomacy[8]"
                && claim.provenance == DeferredHistoryProvenance::ExistingRuntimeDiplomacy
        }));
    assert!(extended.rows()[fixture.active]
        .conditionally_admitted_slice(RuntimeCoveredRange {
            begin: 0x692a,
            end: 0x692c,
        })
        .is_none());
    assert_eq!(extended.source_produced_walked_bytes(), 0);
    assert_eq!(extended.checksum(), Err(frontier));
    assert!(!extended.installed_in_scoreboard());

    let mut short = fixture.authority;
    short.rows[fixture.active]
        .reg_buildings_by_region_then_type
        .pop();
    assert_eq!(
        bind_deferred_history_frontier(fixture.previous, &fixture.columns, &short),
        Err(DeferredHistoryFrontierError::RegionalBuildingLength {
            slot: fixture.active,
            expected: REG_BUILDING_VALUES,
            got: REG_BUILDING_VALUES - 1,
        })
    );
}

#[test]
fn executable_region_major_history_and_independent_high_water_marks_bite_adler() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let base = bind_deferred_history_frontier(
        fixture.previous.clone(),
        &fixture.columns,
        &fixture.authority,
    )
    .unwrap();

    let region = 3;
    let building_slot = 17;
    let mut matrix_changed = fixture.authority.clone();
    assert!(matrix_changed.rows[fixture.active].set_reg_building(region, building_slot, 0x1234));
    assert_eq!(
        matrix_changed.rows[fixture.active].reg_building(region, building_slot),
        Some(0x1234)
    );
    let matrix_bound =
        bind_deferred_history_frontier(fixture.previous.clone(), &fixture.columns, &matrix_changed)
            .unwrap();
    let index = region * REG_BUILDING_TYPE_SLOTS + building_slot;
    let begin = REG_BUILDINGS_BEGIN + index * 2;
    assert_eq!(
        matrix_bound.rows()[fixture.active]
            .conditionally_admitted_slice(RuntimeCoveredRange {
                begin,
                end: begin + 2,
            })
            .unwrap(),
        &0x1234u16.to_le_bytes()
    );
    assert_ne!(
        base.walk_frontier().checksum,
        matrix_bound.walk_frontier().checksum
    );

    // `high_buildings` is historical high-water state, not derivable from either the
    // current regional matrix or current aggregate `num_buildings`.
    let mut high_changed = fixture.columns.clone();
    high_changed.high_buildings_mut(fixture.active)[building_slot] = 9;
    let high_bound =
        bind_deferred_history_frontier(fixture.previous.clone(), &high_changed, &fixture.authority)
            .unwrap();
    assert_ne!(
        base.walk_frontier().checksum,
        high_bound.walk_frontier().checksum
    );

    let mut padding_changed = fixture.authority.clone();
    padding_changed.rows[fixture.active].queue_padding = [0xa5, 0x5a];
    padding_changed.rows[fixture.active].ctw_padding = [0x33, 0xcc];
    let padding_bound =
        bind_deferred_history_frontier(fixture.previous, &fixture.columns, &padding_changed)
            .unwrap();
    assert_ne!(
        base.walk_frontier().checksum,
        padding_bound.walk_frontier().checksum
    );
}

#[test]
fn current_count_duplicates_refuse_but_deferred_queue_uses_the_runtime_owner() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let building_slot = 7;
    let mut column_changed = fixture.columns.clone();
    column_changed.num_buildings_mut(fixture.active)[building_slot] ^= 1;
    assert!(matches!(
        bind_deferred_history_frontier(
            fixture.previous.clone(),
            &column_changed,
            &fixture.authority,
        ),
        Err(DeferredHistoryFrontierError::RuntimeDisagreement {
            slot,
            field: "num_buildings",
            ..
        }) if slot == fixture.active
    ));

    let mut victory_changed = fixture.victory.clone();
    victory_changed.slots[fixture.active].num_buildings[building_slot] ^= 2;
    let changed_previous = generated_frontier(
        &fixture.prefix,
        &victory_changed,
        &fixture.step8,
        &fixture.types,
        &fixture.columns,
    );
    assert!(matches!(
        bind_deferred_history_frontier(
            changed_previous,
            &fixture.columns,
            &fixture.authority,
        ),
        Err(DeferredHistoryFrontierError::RuntimeDisagreement {
            slot,
            field: "num_buildings",
            ..
        }) if slot == fixture.active
    ));

    let baseline =
        bind_deferred_history_frontier(fixture.previous, &fixture.columns, &fixture.authority)
            .unwrap();
    let mut queue_changed = fixture.victory.clone();
    queue_changed.slots[fixture.active].num_queued[123] = 5;
    let queue_previous = generated_frontier(
        &fixture.prefix,
        &queue_changed,
        &fixture.step8,
        &fixture.types,
        &fixture.columns,
    );
    let queue_bound =
        bind_deferred_history_frontier(queue_previous, &fixture.columns, &fixture.authority)
            .unwrap();
    assert_ne!(
        baseline.walk_frontier().checksum,
        queue_bound.walk_frontier().checksum
    );

    let mut diplomacy_changed = fixture.step8.clone();
    diplomacy_changed.leaders[fixture.active].taunt.dip[2].attacks[4] ^= 7;
    let diplomacy_previous = generated_frontier(
        &fixture.prefix,
        &fixture.victory,
        &diplomacy_changed,
        &fixture.types,
        &fixture.columns,
    );
    let diplomacy_bound =
        bind_deferred_history_frontier(diplomacy_previous, &fixture.columns, &fixture.authority)
            .unwrap();
    assert_ne!(
        baseline.walk_frontier().checksum,
        diplomacy_bound.walk_frontier().checksum
    );
}

#[test]
fn a_different_column_snapshot_refuses() {
    let Some(fixture) = fixture() else {
        skip("ron-data/replays contains no derivable replay with an active Leader");
        return;
    };
    let mut changed = fixture.columns.clone();
    changed.score_mut()[fixture.active] ^= 1;
    assert!(matches!(
        bind_deferred_history_frontier(fixture.previous, &changed, &fixture.authority),
        Err(DeferredHistoryFrontierError::ColumnSnapshotDisagreement {
            slot,
            field: "score",
            ..
        }) if slot == fixture.active
    ));
}

#[test]
fn leaders_score_stays_zero() {
    let state = don_replay::state::SimState::new();
    let checked = don_replay::check_all::CheckAll::of_state(&state);
    let leaders = &checked.per[Channel::Leaders as usize];
    assert!(!leaders.installed);
    assert!(!leaders.exact_producer);
    assert!(!leaders.substantive());
}
