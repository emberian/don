// SPDX-License-Identifier: GPL-3.0-or-later
//! Evidence for the executed sub-boundary inside `TerrainGroups::place_all`
//! `0x006a70d0`.
//!
//! Before this lane the recorded boundary for every checksum-bearing recording
//! was the call's entry address, which says only "somewhere in 8,916 bytes".
//! These tests pin the derived `Mountains` range lists that let the call start
//! at all, the exact primitive each dominant corpus style stops at, and the fact
//! that the survey commits nothing.

use don_replay::harness::WorldSim;
use don_replay::initial::InitialItemBoundary;
use don_replay::place_all_advance::{
    advance_place_all_boundary, resolve_mountain_ranges, OilGoodPolicy, PlaceAllAdvanceError,
    PlaceAllAdvanceFacts, PlaceAllStop, MOUNTAINS_ADD_MOUNTAIN_VA, MOUNTAIN_RANGE_SOURCE_FILE,
    WORLD_SET_OIL_AT_VA,
};
use don_replay::replay::Replay;
use don_sim::rng::Random;
use don_sim::systems::mountains::Mountains;
use std::path::{Path, PathBuf};

const PLACE_ALL_VA: u32 = 0x006a_70d0;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn open(name: &str) -> Option<Replay> {
    let path = repo_root().join("ron-data/replays/multi").join(name);
    if !path.is_file() {
        eprintln!("\n  SKIPPED — NOT A PASS. {name} is absent; nothing was exercised.\n");
        return None;
    }
    Some(Replay::open(&path).expect("retail replay must decode"))
}

fn great_lakes() -> Option<Replay> {
    open("Playback___2024.02.24_21_25_53__Sat_.rcx")
}

fn mediterranean() -> Option<Replay> {
    open("Playback___2025.02.10_21_26_50__Mon_.rcx")
}

fn shipped_mountains() -> Option<Mountains> {
    let path = repo_root()
        .join("ron-data")
        .join(MOUNTAIN_RANGE_SOURCE_FILE);
    if !path.is_file() {
        eprintln!("\n  SKIPPED — NOT A PASS. ron-data/{MOUNTAIN_RANGE_SOURCE_FILE} is absent.\n");
        return None;
    }
    Some(resolve_mountain_ranges(&path).expect("the shipped MOUNTAINS section must resolve"))
}

/// The three range lists, and the RNG cost they impose on `place_all`'s first call.
///
/// This is the load-bearing derivation of the lane: `randomize_mountains`
/// `0x0089ca70` draws one main-stream word per list of length >= 2, so the
/// shipped `1 / 8 / 7` lengths make it consume exactly two — and a wrong length
/// would shift every later draw in the 8,916-byte call.
///
/// Catches: mapping `sm`/`sml` to the wrong list, counting `<MOUNTAINS>` itself
/// as a `<MOUNTAIN>` (which would give 17), losing the head-first insertion
/// order, or a non-zero node metric.
#[test]
fn the_shipped_mountain_section_gives_one_small_eight_medium_and_seven_large_ranges() {
    let Some(mut mountains) = shipped_mountains() else {
        return;
    };
    assert_eq!(mountains.small_ranges.len(), 1);
    assert_eq!(mountains.medium_ranges.len(), 8);
    assert_eq!(mountains.large_ranges.len(), 7);

    // `LinkListBase::add` 0x004a4af0 makes each new node the head, so head-first
    // order is reverse document order and every metric is zero.
    assert_eq!(mountains.small_ranges.current_data(), 15);
    assert_eq!(mountains.medium_ranges.current_data(), 14);
    assert_eq!(mountains.large_ranges.current_data(), 6);
    assert_eq!(mountains.small_ranges.current_metric(), 0);
    assert_eq!(mountains.medium_ranges.current_metric(), 0);
    assert_eq!(mountains.large_ranges.current_metric(), 0);

    // Two draws, not three: the one-element small list takes the `xor edx, edx`
    // arm at 0x0089ca87 without touching game_random.
    let mut random = Random::new(0x1234_5678);
    let receipt = mountains.randomize_mountains(&mut random);
    assert_eq!(receipt.draws, 2);
    assert_eq!(receipt.selected_indices[0], 0);
    assert!(receipt.selected_indices[1] < 8);
    assert!(receipt.selected_indices[2] < 7);
}

/// Great Lakes reaches `World::set_oil_at` `0x006b2a10`, inside `place_all`.
///
/// Catches a regression to the coarse `terrain_groups_place_all` boundary, to
/// the earlier `place_all_mountain_range_lists` boundary, or to a production
/// oil policy that silently crosses the unmodelled `goods` channel.
#[test]
fn great_lakes_place_all_boundary_is_world_set_oil_at_inside_the_call() {
    let Some(rep) = great_lakes() else {
        return;
    };
    let sim = WorldSim::from_replay(&rep);
    let plan = sim.initial_items.as_ref().expect("plan");
    let InitialItemBoundary::MapTerrainGroupsPlaceAllPrimitiveUnavailable {
        boundary,
        place_all_va,
        primitive_va,
        group_index,
        ..
    } = plan.boundary
    else {
        panic!(
            "expected an executed place_all boundary, got {:?} (error {:?})",
            plan.boundary, sim.initial_item_error
        );
    };
    assert_eq!(boundary, "place_all_world_set_oil_at");
    assert_eq!(place_all_va, PLACE_ALL_VA);
    assert_eq!(primitive_va, WORLD_SET_OIL_AT_VA);
    assert_eq!(group_index, Some(0));
    assert_eq!(plan.mountain_range_error, None);

    let advance = plan.place_all_advance.as_ref().expect("survey receipt");
    assert_eq!(advance.oil_good_policy, OilGoodPolicy::Stop);
    assert!(advance.crossed_oil_good_effects.is_empty());
    assert_eq!(advance.mountain_range_lengths, Some([1, 8, 7]));
    assert_eq!(advance.mountain_randomize_draws, Some(2));
    // greatlakes.xml carries ten GROUPENTRY rows, all chance="100".
    assert_eq!(advance.catalog_groups, 10);
    assert_eq!(advance.selected_groups.len(), 10);
}

/// Mediterranean's first group is `pattern="nonplayer"`, so it enters
/// `place_region_group` `0x006a2f60` and stops at `Mountains::add_mountain`.
///
/// Catches the helping globals regressing back to an "absent producer": before
/// this lane the region path failed with `MissingHelpingState` instead of
/// reaching a retail primitive.
#[test]
fn mediterranean_place_all_boundary_is_mountains_add_mountain() {
    let Some(rep) = mediterranean() else {
        return;
    };
    let sim = WorldSim::from_replay(&rep);
    let plan = sim.initial_items.as_ref().expect("plan");
    let InitialItemBoundary::MapTerrainGroupsPlaceAllPrimitiveUnavailable {
        boundary,
        primitive_va,
        group_index,
        completed_groups,
        ..
    } = plan.boundary
    else {
        panic!(
            "expected an executed place_all boundary, got {:?} (error {:?})",
            plan.boundary, sim.initial_item_error
        );
    };
    assert_eq!(boundary, "place_all_mountains_add_mountain");
    assert_eq!(primitive_va, MOUNTAINS_ADD_MOUNTAIN_VA);
    assert_eq!(group_index, Some(0));
    assert_eq!(completed_groups, 0);
    let advance = plan.place_all_advance.as_ref().expect("survey receipt");
    assert_eq!(advance.mountain_range_lengths, Some([1, 8, 7]));
    assert_eq!(advance.catalog_groups, 15);
}

/// The survey must not move a single generated byte into the `world` channel.
///
/// `advance_place_all_boundary` takes `&InitialWorld`, so the compiler already
/// forbids a commit; this asserts the observable consequence so that changing
/// the signature to `&mut` cannot quietly start hashing unsourced terrain.
#[test]
fn place_all_survey_commits_no_world_or_checksum_bytes() {
    let (Some(rep), Some(mountains)) = (great_lakes(), shipped_mountains()) else {
        return;
    };
    let sim = WorldSim::from_replay(&rep);
    let plan = sim.initial_items.as_ref().expect("plan");
    let map = sim.initial_world.as_ref().expect("prefix world");
    let continent = sim.initial_continent.as_ref().expect("continent receipt");

    let before_world = map.world.checksum_sections();
    let before_checksum = map.checksum.clone();
    let before_sourced = map.exact_sourced_walked_bytes();

    let facts = PlaceAllAdvanceFacts {
        mountains: Some(mountains),
        helping: Some(don_replay::place_all_advance::initial_region_helping_state(
            &map.world,
        )),
        oil_good_policy: OilGoodPolicy::ContinueRecordingGoodEffects,
        ..PlaceAllAdvanceFacts::default()
    };
    let advance = advance_place_all_boundary(plan, map, continent, &facts)
        .expect("the survey must run from the fill_fertile boundary");
    assert!(!advance.completed_groups.is_empty());

    assert_eq!(
        map.world.checksum_sections(),
        before_world,
        "the survey mutated the World"
    );
    assert_eq!(map.checksum, before_checksum);
    assert_eq!(map.exact_sourced_walked_bytes(), before_sourced);
}

/// `place_all`'s first argument gates a progress-display host event only.
///
/// It reaches neither World nor the main RNG, so every value must produce the
/// same stop and the same completed-group set. Catches a future change that
/// lets `progress` select a branch, which would make the argument a real
/// unavailable input rather than presentation.
#[test]
fn progress_argument_cannot_change_the_stop() {
    let (Some(rep), Some(mountains)) = (great_lakes(), shipped_mountains()) else {
        return;
    };
    let sim = WorldSim::from_replay(&rep);
    let plan = sim.initial_items.as_ref().expect("plan");
    let map = sim.initial_world.as_ref().expect("prefix world");
    let continent = sim.initial_continent.as_ref().expect("continent receipt");

    let run = |progress: i32| {
        let facts = PlaceAllAdvanceFacts {
            mountains: Some(mountains.clone()),
            helping: Some(don_replay::place_all_advance::initial_region_helping_state(
                &map.world,
            )),
            progress,
            ..PlaceAllAdvanceFacts::default()
        };
        advance_place_all_boundary(plan, map, continent, &facts).expect("survey must run")
    };
    // Zero versus nonzero is the whole branch; `i32::MIN` also catches a model
    // that treats the argument as a count or an index rather than a flag.
    let zero = run(0);
    let extreme = run(i32::MIN);
    assert_eq!(zero.stop, extreme.stop);
    assert_eq!(zero.completed_groups, extreme.completed_groups);
    assert_eq!(zero.selected_groups, extreme.selected_groups);
    assert_eq!(zero.progress, 0);
    assert_eq!(extreme.progress, i32::MIN);
}

/// What the next lane meets once the `Good` object system exists.
///
/// `World::set_oil_at`'s Good create/close is void with respect to the `world`
/// channel and the main RNG (`crates/don-sim/src/systems/terrain_drop_tile.rs`),
/// so crossing it costs no derived value — but it is still an unmodelled channel
/// and the production reconstruction refuses it. With it crossed, Great Lakes'
/// two leading `type="trees" pattern="player"` groups complete and group 2,
/// `type="mountains"`, stops at `Mountains::add_mountain` `0x0089c2e0`.
#[test]
fn crossing_the_void_oil_good_effect_reaches_add_mountain_at_group_two() {
    let (Some(rep), Some(mountains)) = (great_lakes(), shipped_mountains()) else {
        return;
    };
    let sim = WorldSim::from_replay(&rep);
    let plan = sim.initial_items.as_ref().expect("plan");
    let map = sim.initial_world.as_ref().expect("prefix world");
    let continent = sim.initial_continent.as_ref().expect("continent receipt");

    let facts = PlaceAllAdvanceFacts {
        mountains: Some(mountains),
        helping: Some(don_replay::place_all_advance::initial_region_helping_state(
            &map.world,
        )),
        oil_good_policy: OilGoodPolicy::ContinueRecordingGoodEffects,
        ..PlaceAllAdvanceFacts::default()
    };
    let advance = advance_place_all_boundary(plan, map, continent, &facts).expect("survey");

    assert_eq!(advance.selected_groups[0].group_type, 4);
    assert_eq!(advance.selected_groups[1].group_type, 4);
    assert_eq!(advance.selected_groups[2].group_type, 5);
    assert_eq!(advance.completed_groups, vec![0, 1]);
    let PlaceAllStop::MountainsAddMountain { group_index, .. } = advance.stop else {
        panic!("expected the add_mountain boundary, got {:?}", advance.stop);
    };
    assert_eq!(group_index, 2);
    assert_eq!(advance.stop.primitive_va(), MOUNTAINS_ADD_MOUNTAIN_VA);
    assert!(!advance.crossed_oil_good_effects.is_empty());
}

/// Without the shipped `MOUNTAINS` section the survey must stop at
/// `Mountains::randomize_mountains` and not guess a draw count.
#[test]
fn an_absent_mountain_section_stops_before_the_first_draw() {
    let Some(rep) = great_lakes() else {
        return;
    };
    let sim = WorldSim::from_replay(&rep);
    let plan = sim.initial_items.as_ref().expect("plan");
    let map = sim.initial_world.as_ref().expect("prefix world");
    let continent = sim.initial_continent.as_ref().expect("continent receipt");
    let advance = advance_place_all_boundary(plan, map, continent, &PlaceAllAdvanceFacts::default())
        .expect("survey");
    assert_eq!(advance.stop, PlaceAllStop::MountainRangeLists);
    assert_eq!(advance.stop.name(), "place_all_mountain_range_lists");
    assert_eq!(advance.mountain_range_lengths, None);
    assert_eq!(advance.mountain_randomize_draws, None);
    assert!(advance.selected_groups.is_empty());
}

/// A plan that has not reached `fill_fertile` must not be surveyed.
#[test]
fn the_survey_refuses_a_plan_at_an_earlier_boundary() {
    let Some(rep) = great_lakes() else {
        return;
    };
    let sim = WorldSim::from_replay(&rep);
    let map = sim.initial_world.as_ref().expect("prefix world");
    let continent = sim.initial_continent.as_ref().expect("continent receipt");
    let mut plan = sim.initial_items.as_ref().expect("plan").clone();
    plan.boundary = InitialItemBoundary::MapTerrainGroupsUnavailable {
        next_va: don_replay::TERRAIN_GROUPS_FILL_FERTILE_VA,
    };
    let error = advance_place_all_boundary(&plan, map, continent, &PlaceAllAdvanceFacts::default())
        .expect_err("an earlier boundary must fail closed");
    assert!(matches!(error, PlaceAllAdvanceError::Blocked { .. }));
}
