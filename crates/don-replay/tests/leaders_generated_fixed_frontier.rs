//! Long fixed-prefix and duplicate-owner gates for the PDB-generated Leader columns.

mod leader_initial_prefix {
    pub use don_replay::leader_initial_prefix::*;
}

mod leaders_runtime_frontier {
    pub use don_replay::leaders_runtime_frontier::*;
}

mod leaders_runtime_tribe_frontier {
    pub use don_replay::leaders_runtime_tribe_frontier::*;
}

#[path = "../src/leaders_generated_fixed_frontier.rs"]
mod leaders_generated_fixed_frontier;

use don_replay::checksum::Channel;
use don_replay::leader_initial_prefix::{derive, InitialLeaderPrefix};
use don_replay::leaders_runtime_frontier::{
    bind_live, LeadersRuntimeError, LeadersWalkBoundary, RuntimeCoveredRange,
};
use don_replay::leaders_runtime_tribe_frontier::{
    bind_live_tribes, RuntimeLeadersTribeFrontier, LEADER_TRIBE_OFFSET, NEXT_FIXED_BODY_GAP,
};
use don_replay::replay::{corpus, Replay};
use don_sim::generated::state::{leader, FieldDesc, LeaderCols, Pool};
use don_sim::systems::bhs_type_table::{
    LeaderTypeMasks, TribeRoster, TypeBackup, TypeBuiltinState, TypeRow, TypeTable, BUILD_BEGIN,
    BUILD_END, NUM_LEADERS, NUM_TRIBES, NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
};
use don_sim::systems::{leaders, victory_score};
use leaders_generated_fixed_frontier::{
    bind_generated_fixed_prefix, GeneratedFixedFrontierError, GeneratedFixedProvenance,
    GENERATED_FIXED_PREFIX_END, LAST_TAUNT_BEGIN, LAST_TAUNT_END, LEADER_WALK_FIXED_RANGE_CALL_VA,
    NEXT_DEFERRED_FIELD, NEXT_DEFERRED_FIELD_BYTES, NEXT_DEFERRED_FIELD_NAME,
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

fn prefix_matching(
    mut predicate: impl FnMut(&InitialLeaderPrefix) -> bool,
) -> Option<InitialLeaderPrefix> {
    for path in corpus(&repo_root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        let Ok(prefix) = derive(&replay.initial) else {
            continue;
        };
        if predicate(&prefix) {
            return Some(prefix);
        }
    }
    None
}

fn first_prefix() -> Option<InitialLeaderPrefix> {
    prefix_matching(|_| true)
}

fn first_random_prefix() -> Option<InitialLeaderPrefix> {
    prefix_matching(|prefix| {
        prefix.rows.iter().any(|row| {
            row.active
                && row
                    .tribe
                    .is_some_and(|tribe| usize::from(tribe) == NUM_TRIBES)
        })
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

fn previous_frontier(
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

/// Copy established duplicate fields into an otherwise independent live-column fixture.
/// All remaining materialised fields retain their own zero values and are owned only by
/// `LeaderCols`, which is exactly what the extension must distinguish from zero-filling.
fn synchronize_duplicates(previous: &RuntimeLeadersTribeFrontier, columns: &mut LeaderCols) {
    for slot in 0..NUM_LEADERS {
        let base = &previous.base().rows[slot];
        let walk_end = if base.active {
            GENERATED_FIXED_PREFIX_END
        } else {
            8
        };
        for field in &leader::FIELDS {
            let begin = field.offset as usize;
            let end = begin + field.size as usize;
            if begin >= walk_end
                || end > walk_end
                || field.alias_of.is_some()
                || !field.repr.materialised()
            {
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

#[test]
fn one_coherent_column_owner_advances_to_the_first_deferred_matrix() {
    let Some(prefix) = first_prefix() else {
        skip("ron-data/replays contains no derivable replay");
        return;
    };
    let (victory, step8) = current_states(&prefix);
    let types = type_state(&prefix);
    let previous = previous_frontier(&prefix, &victory, &step8, &types);
    let previous_walk = previous.walk_frontier();
    let mut columns = zeroed_columns();
    synchronize_duplicates(&previous, &mut columns);
    let extended = bind_generated_fixed_prefix(previous.clone(), &columns).unwrap();

    assert_eq!(GENERATED_FIXED_PREFIX_END, 0x14de);
    assert_eq!(NEXT_DEFERRED_FIELD_NAME, "reg_buildings");
    assert_eq!(NEXT_DEFERRED_FIELD, "reg_buildings[129][64]");
    assert_eq!(NEXT_DEFERRED_FIELD_BYTES, 16_512);
    assert_eq!(LEADER_WALK_FIXED_RANGE_CALL_VA, 0x006d_6796);
    let active = prefix.rows.iter().position(|row| row.active).unwrap();
    assert_eq!(
        previous_walk.boundary,
        LeadersWalkBoundary::FixedBody {
            slot: active,
            offset: NEXT_FIXED_BODY_GAP,
        }
    );
    let frontier = extended.walk_frontier();
    assert_eq!(
        frontier.boundary,
        LeadersWalkBoundary::FixedBody {
            slot: active,
            offset: GENERATED_FIXED_PREFIX_END,
        }
    );
    assert_eq!(
        frontier.bytes_walked,
        previous_walk.bytes_walked + (GENERATED_FIXED_PREFIX_END - NEXT_FIXED_BODY_GAP) as u64
    );
    assert!(extended.conditionally_admitted_walked_bytes() > 5_000);
    assert!(extended.duplicate_checked_walked_bytes() > 0);
    assert_eq!(extended.source_produced_walked_bytes(), 0);
    assert_eq!(
        extended.previous().walk_frontier().boundary,
        previous_walk.boundary
    );
    assert_eq!(
        extended.rows()[active]
            .claims()
            .iter()
            .map(|claim| claim.bytes())
            .sum::<usize>(),
        GENERATED_FIXED_PREFIX_END
    );
    assert!(extended.rows()[active].claims().iter().any(|claim| {
        claim.begin == LAST_TAUNT_BEGIN
            && claim.end == LAST_TAUNT_END
            && claim.provenance == GeneratedFixedProvenance::ExistingRuntimeFrontierGapFill
    }));
    assert!(extended.rows()[active]
        .conditionally_admitted_slice(RuntimeCoveredRange {
            begin: 0,
            end: GENERATED_FIXED_PREFIX_END,
        })
        .is_some());
    if let Some(inactive) = prefix.rows.iter().position(|row| !row.active) {
        assert!(extended.rows()[inactive]
            .conditionally_admitted_slice(RuntimeCoveredRange { begin: 8, end: 12 })
            .is_none());
    }
    assert_eq!(extended.checksum(), Err(frontier));
    assert!(!extended.installed_in_scoreboard());

    // Mutation sensitivity reaches the final materialised field before the boundary.
    let mut tail_changed = columns.clone();
    tail_changed.reg_terr_mut(active)[63] ^= 0x55aa;
    let changed = bind_generated_fixed_prefix(previous, &tail_changed).unwrap();
    assert_ne!(
        extended.walk_frontier().checksum,
        changed.walk_frontier().checksum
    );
}

#[test]
fn duplicate_owner_mutations_refuse_in_both_directions() {
    let Some(prefix) = first_prefix() else {
        skip("ron-data/replays contains no derivable replay");
        return;
    };
    let active = prefix.rows.iter().position(|row| row.active).unwrap();
    let (mut victory, step8) = current_states(&prefix);
    let types = type_state(&prefix);
    let previous = previous_frontier(&prefix, &victory, &step8, &types);
    let mut columns = zeroed_columns();
    synchronize_duplicates(&previous, &mut columns);

    // `last_taunt` is the generated owner's sole internal aggregate. The established
    // frontier admits it only after its victory and taunt owners agree byte-for-byte.
    let mut changed_taunt = step8.clone();
    changed_taunt.leaders[active].taunt.last_taunt[0] ^= 4;
    assert!(matches!(
        bind_live(&prefix, &victory, &changed_taunt),
        Err(LeadersRuntimeError::SourceDisagreement {
            slot,
            offset: LAST_TAUNT_BEGIN,
            ..
        }) if slot == active
    ));

    let mut column_changed = columns.clone();
    column_changed.score_mut()[active] ^= 1;
    assert!(matches!(
        bind_generated_fixed_prefix(previous.clone(), &column_changed),
        Err(GeneratedFixedFrontierError::ColumnDisagreement {
            slot,
            field: "score",
            established_owner: "RuntimeLeadersFrontier",
            ..
        }) if slot == active
    ));

    victory.slots[active].score ^= 2;
    let changed_previous = previous_frontier(&prefix, &victory, &step8, &types);
    assert!(matches!(
        bind_generated_fixed_prefix(changed_previous, &columns),
        Err(GeneratedFixedFrontierError::ColumnDisagreement {
            slot,
            field: "score",
            established_owner: "RuntimeLeadersFrontier",
            ..
        }) if slot == active
    ));

    let short = LeaderCols::with_capacity(NUM_LEADERS);
    assert_eq!(
        bind_generated_fixed_prefix(previous, &short),
        Err(GeneratedFixedFrontierError::ColumnRowCount {
            expected: NUM_LEADERS,
            got: 0,
        })
    );
}

#[test]
fn random_setup_selector_still_requires_the_two_live_tribe_owners_to_agree() {
    let Some(prefix) = first_random_prefix() else {
        skip("ron-data/replays contains no derivable replay with active random tribe selector 24");
        return;
    };
    let random = prefix
        .rows
        .iter()
        .position(|row| {
            row.active
                && row
                    .tribe
                    .is_some_and(|tribe| usize::from(tribe) == NUM_TRIBES)
        })
        .unwrap();
    let (victory, step8) = current_states(&prefix);
    let types = type_state(&prefix);
    let previous = previous_frontier(&prefix, &victory, &step8, &types);
    let resolved = previous.tribe_for(random).unwrap();
    assert!((0..NUM_TRIBES as i32).contains(&resolved));
    let mut columns = zeroed_columns();
    synchronize_duplicates(&previous, &mut columns);
    assert_eq!(columns.tribe()[random], resolved);
    assert!(bind_generated_fixed_prefix(previous.clone(), &columns).is_ok());

    columns.tribe_mut()[random] = (resolved + 1) % NUM_TRIBES as i32;
    assert!(matches!(
        bind_generated_fixed_prefix(previous, &columns),
        Err(GeneratedFixedFrontierError::ColumnDisagreement {
            slot,
            field: "tribe",
            established_owner: "RuntimeLeadersTribeFrontier",
            ..
        }) if slot == random
    ));
}

#[test]
fn the_long_prefix_still_does_not_install_a_leaders_score() {
    let state = don_replay::state::SimState::new();
    let checked = don_replay::check_all::CheckAll::of_state(&state);
    let leaders = &checked.per[Channel::Leaders as usize];
    assert!(!leaders.installed);
    assert!(!leaders.exact_producer);
    assert!(!leaders.substantive());
}
