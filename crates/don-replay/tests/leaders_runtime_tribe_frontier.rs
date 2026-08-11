//! Exact-source and refusal gates for the live Leader tribe frontier extension.

mod initial {
    pub use don_replay::initial::*;
}

mod leader_initial_prefix {
    pub use don_replay::leader_initial_prefix::*;
}

mod leaders_runtime_frontier {
    pub use don_replay::leaders_runtime_frontier::*;
}

#[path = "../src/leaders_runtime_tribe_frontier.rs"]
mod leaders_runtime_tribe_frontier;

use don_replay::checksum::Channel;
use don_replay::leader_initial_prefix::{derive, InitialLeaderPrefix};
use don_replay::leaders_runtime_frontier::{bind_live, LeadersWalkBoundary};
use don_replay::replay::{corpus, load_payload, Replay};
use don_sim::systems::bhs_type_table::{
    LeaderTypeMasks, TribeRoster, TypeBackup, TypeBuiltinState, TypeRow, TypeTable, BUILD_BEGIN,
    BUILD_END, NUM_LEADERS, NUM_TRIBES, NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
};
use don_sim::systems::{leaders, victory_score};
use leaders_runtime_tribe_frontier::{
    bind_live_tribes, LeadersTribeFrontierError, LiveLeaderTribeProvenance, LEADER_TRIBE_BYTES,
    LEADER_TRIBE_OFFSET, LEADER_TRIBE_WRITER_VA, NEXT_FIXED_BODY_GAP,
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

fn first_prefix() -> Option<(PathBuf, Vec<u8>, InitialLeaderPrefix)> {
    for path in corpus(&repo_root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        let Ok(prefix) = derive(&replay.initial) else {
            continue;
        };
        let Ok(payload) = load_payload(&path) else {
            continue;
        };
        return Some((path, payload, prefix));
    }
    None
}

fn first_prefix_with_concrete_active_tribe() -> Option<(PathBuf, Vec<u8>, InitialLeaderPrefix)> {
    for path in corpus(&repo_root()) {
        let Ok(replay) = Replay::open(&path) else {
            continue;
        };
        let Ok(prefix) = derive(&replay.initial) else {
            continue;
        };
        if !prefix.rows.iter().any(|row| {
            row.active
                && row
                    .tribe
                    .is_some_and(|tribe| usize::from(tribe) < NUM_TRIBES)
        }) {
            continue;
        }
        let Ok(payload) = load_payload(&path) else {
            continue;
        };
        return Some((path, payload, prefix));
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
                ((slot + 3) % NUM_TRIBES) as i32
            } else {
                i32::from(selector)
            }
        });
        row
    });
    TypeBuiltinState::new(types, tribes, leaders)
}

#[test]
fn the_live_type_owner_advances_the_prefix_from_tribe_to_gov() {
    let Some((_path, payload, prefix)) = first_prefix() else {
        skip("ron-data/replays contains no derivable replay");
        return;
    };
    let (victory, step8) = current_states(&prefix);
    let base = bind_live(&prefix, &victory, &step8).expect("current owners bind");
    let base_frontier = base.walk_frontier();
    let types = type_state(&prefix);
    let extended = bind_live_tribes(&prefix, base, &types).expect("live tribes agree");

    assert_eq!(LEADER_TRIBE_OFFSET, 0x0c);
    assert_eq!(LEADER_TRIBE_BYTES, 4);
    assert_eq!(LEADER_TRIBE_WRITER_VA, 0x006e_3aff);
    assert_eq!(NEXT_FIXED_BODY_GAP, 0x14);
    assert_eq!(
        extended.claims().len(),
        prefix.active_mask.count_ones() as usize
    );
    assert_eq!(
        extended.newly_owned_walked_bytes(),
        extended.claims().len() * 4
    );
    assert_eq!(
        extended.runtime_claimed_walked_bytes(),
        extended.base().runtime_claimed_walked_bytes() + extended.claims().len() * 4
    );

    for claim in extended.claims() {
        assert_eq!(claim.writer_va, LEADER_TRIBE_WRITER_VA);
        let replay_selector = payload[claim.replay_player_body.offset + 0x32];
        assert_eq!(replay_selector, claim.setup_selector);
        match claim.provenance {
            LiveLeaderTribeProvenance::ReplaySetupAndTypeBuiltinState => {
                assert_eq!(replay_selector, claim.tribe as u8);
                assert!(usize::from(replay_selector) < NUM_TRIBES);
            }
            LiveLeaderTribeProvenance::ReplayRandomSelectorResolvedByTypeBuiltinState => {
                assert_eq!(usize::from(replay_selector), NUM_TRIBES);
                assert!((0..NUM_TRIBES as i32).contains(&claim.tribe));
            }
        }
        assert_eq!(claim.replay_payload_sha256, prefix.sources.payload_sha256);
        assert_eq!(
            extended.tribe_for(usize::from(claim.slot)),
            Some(claim.tribe)
        );
    }

    let first_active = prefix
        .rows
        .iter()
        .position(|row| row.active)
        .expect("recorded setup has an active Leader");
    assert_eq!(
        base_frontier.boundary,
        LeadersWalkBoundary::FixedBody {
            slot: first_active,
            offset: LEADER_TRIBE_OFFSET,
        }
    );
    let frontier = extended.walk_frontier();
    assert_eq!(
        frontier.boundary,
        LeadersWalkBoundary::FixedBody {
            slot: first_active,
            offset: NEXT_FIXED_BODY_GAP,
        }
    );
    assert_eq!(frontier.bytes_walked, base_frontier.bytes_walked + 8);
    assert_eq!(extended.checksum(), Err(frontier));
    assert!(!extended.installed_in_scoreboard());
}

#[test]
fn a_stale_or_invalid_live_tribe_refuses_before_any_extension_exists() {
    let Some((_path, _payload, prefix)) = first_prefix_with_concrete_active_tribe() else {
        skip("ron-data/replays contains no derivable replay with a concrete active tribe");
        return;
    };
    let active = prefix
        .rows
        .iter()
        .position(|row| {
            row.active
                && row
                    .tribe
                    .is_some_and(|tribe| usize::from(tribe) < NUM_TRIBES)
        })
        .expect("fixture selection established a concrete active tribe");
    let (victory, step8) = current_states(&prefix);

    let mut stale = type_state(&prefix);
    stale.leaders[active].tribe = (stale.leaders[active].tribe + 1) % NUM_TRIBES as i32;
    let base = bind_live(&prefix, &victory, &step8).unwrap();
    assert!(matches!(
        bind_live_tribes(&prefix, base, &stale),
        Err(LeadersTribeFrontierError::TribeDrift { slot, .. }) if slot == active
    ));

    let mut invalid = type_state(&prefix);
    invalid.leaders[active].tribe = NUM_TRIBES as i32;
    let base = bind_live(&prefix, &victory, &step8).unwrap();
    assert_eq!(
        bind_live_tribes(&prefix, base, &invalid),
        Err(LeadersTribeFrontierError::LiveTribeOutOfRange {
            slot: active,
            tribe: NUM_TRIBES as i32,
        })
    );

    let mut wrong_roster = type_state(&prefix);
    wrong_roster.leaders[active].leader_flags &= !1;
    let base = bind_live(&prefix, &victory, &step8).unwrap();
    assert_eq!(
        bind_live_tribes(&prefix, base, &wrong_roster),
        Err(LeadersTribeFrontierError::TypeRuntimeRosterMismatch {
            slot: active,
            type_active: false,
            runtime_active: true,
        })
    );
}

#[test]
fn registering_the_extension_does_not_install_leaders_in_the_scoreboard() {
    let state = don_replay::state::SimState::new();
    let checked = don_replay::check_all::CheckAll::of_state(&state);
    let leaders = &checked.per[Channel::Leaders as usize];
    assert!(!leaders.installed);
    assert!(!leaders.exact_producer);
    assert!(!leaders.substantive());
}
