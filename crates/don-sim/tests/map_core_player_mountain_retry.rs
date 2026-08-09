// SPDX-License-Identifier: GPL-3.0-or-later

//! Exact pattern-0 type-5 mountain-template retry transaction.

use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, World};
use don_sim::systems::mountains::{MountainRangeEntry, MountainRangeList, Mountains};
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllHostEvent, TerrainGroup, TerrainGroups, TerrainPlacementBoundary,
};
use don_sim::systems::terrain_player_group::{
    PlacePlayerGroupCall, PlacePlayerGroupOutcome, PlayerGroupExternalResolution,
};
use don_sim::systems::terrain_player_mountain_retry::{
    MountainTemplateRetryStage, PlayerMountainTemplateRetryReceipt,
};

fn one_candidate_world() -> World {
    let mut world = World::init_default_rules(15, 15);
    for cell in &mut world.wdata {
        cell.land = land::FERTILE;
        cell.region = 1;
        cell.blocked = 1;
    }
    world.start_x.items.push(7);
    world.start_y.items.push(7);
    // Every radius-one scan reaches exactly one Mountains host call. This
    // makes a failed placement-call boundary distinct from a template-cycle
    // boundary without weakening the native ring scan.
    world.wdata_mut(7, 8).blocked = 0;
    world
}

fn mountain_group(target_tiles: i32) -> TerrainGroup {
    TerrainGroup {
        group_type: 5,
        chance: 100,
        min_clumps: 1,
        max_clumps: 1,
        pattern: 0,
        min_size: target_tiles,
        max_size: target_tiles,
        start_min: 0,
        start_max: 1,
        ..TerrainGroup::default()
    }
}

fn mountains() -> Mountains {
    Mountains {
        small_ranges: MountainRangeList::new(vec![
            MountainRangeEntry::new(10, 0),
            MountainRangeEntry::new(11, 0),
        ]),
        medium_ranges: MountainRangeList::new(vec![
            MountainRangeEntry::new(20, 0),
            MountainRangeEntry::new(21, 0),
        ]),
        large_ranges: MountainRangeList::new(vec![
            MountainRangeEntry::new(30, 0),
            MountainRangeEntry::new(31, 0),
        ]),
    }
}

fn call(target_tiles: i32, initial_template: i32) -> PlacePlayerGroupCall {
    PlacePlayerGroupCall {
        target_tiles,
        player_index: 0,
        land_subtype: initial_template,
        oil_deposits: 0,
        group_index: 0,
        strict_type_four: false,
    }
}

fn drive_retry(
    target_tiles: i32,
    success_stage: Option<MountainTemplateRetryStage>,
) -> (PlayerMountainTemplateRetryReceipt, Mountains, Random, usize) {
    let base_world = one_candidate_world();
    let base_group = mountain_group(target_tiles);
    let mut base_mountains = mountains();
    let initial_template = base_mountains.get_range_raw(target_tiles);
    let base_random = Random::new(0x1020_3040);
    let mut externals = Vec::new();

    for _ in 0..128 {
        let mut world = base_world.clone();
        let mut group = base_group.clone();
        let mut mountain_state = base_mountains.clone();
        let mut random = base_random;
        let mut pumps = 0usize;
        let receipt = group
            .apply_player_mountain_template_retry(
                &mut world,
                &mut random,
                &mut mountain_state,
                call(target_tiles, initial_template),
                initial_template,
                &mut Vec::new(),
                &mut Vec::new(),
                &externals,
                || pumps += 1,
            )
            .unwrap();

        match receipt.outcome {
            PlacePlayerGroupOutcome::ExternalResolutionRequired { request } => {
                let stage = receipt.attempts.last().unwrap().stage;
                let liberr = i32::from(success_stage != Some(stage));
                externals.push(PlayerGroupExternalResolution::Mountains { request, liberr });
            }
            PlacePlayerGroupOutcome::Returned(_) => {
                return (receipt, mountain_state, random, pumps);
            }
            PlacePlayerGroupOutcome::GrowthKernel { .. } => {
                panic!("mountain retry entered forest/rock growth")
            }
        }
    }
    panic!("mountain retry did not converge")
}

#[test]
fn target_three_exhausts_primary_then_advances_and_randomizes_after_success() {
    let (receipt, mountains_after, random_after, pumps) =
        drive_retry(3, Some(MountainTemplateRetryStage::OneStepSmaller));

    assert_eq!(receipt.outcome, PlacePlayerGroupOutcome::Returned(1));
    assert_eq!(
        receipt
            .attempts
            .iter()
            .map(|attempt| attempt.stage)
            .collect::<Vec<_>>(),
        [
            MountainTemplateRetryStage::Primary,
            MountainTemplateRetryStage::Primary,
            MountainTemplateRetryStage::OneStepSmaller,
        ]
    );
    assert_eq!(
        receipt
            .attempts
            .iter()
            .map(|attempt| attempt.template)
            .collect::<Vec<_>>(),
        [30, 31, receipt.attempts[2].template]
    );
    assert_eq!(receipt.attempts[0].next_template, Some(31));
    assert_eq!(receipt.attempts[1].next_template, Some(30));
    assert!(receipt.attempts[2].next_template.is_some());
    assert_eq!(receipt.randomizations.len(), 2);
    assert_eq!(receipt.randomizations[0].draws, 3);
    assert_eq!(receipt.randomizations[1].draws, 3);
    let selected_medium = receipt.randomizations[0].selected_indices[1];
    assert_eq!(receipt.attempts[2].template, 20 + selected_medium as i32);
    assert_eq!(pumps, receipt.attempts.len());
    assert_eq!(receipt.rng_state_after, random_after.state());
    assert_eq!(
        mountains_after.small_ranges.current_index(),
        Some(receipt.randomizations[1].selected_indices[0])
    );
    assert_eq!(
        mountains_after.medium_ranges.current_index(),
        Some(receipt.randomizations[1].selected_indices[1])
    );
    assert_eq!(
        mountains_after.large_ranges.current_index(),
        Some(receipt.randomizations[1].selected_indices[2])
    );
}

#[test]
fn target_two_smallest_cycle_is_side_effect_only_and_preserves_smaller_success() {
    let (receipt, _, _, pumps) = drive_retry(2, Some(MountainTemplateRetryStage::OneStepSmaller));

    assert_eq!(receipt.outcome, PlacePlayerGroupOutcome::Returned(1));
    assert_eq!(receipt.randomizations.len(), 2);
    assert_eq!(
        receipt
            .attempts
            .iter()
            .map(|attempt| attempt.stage)
            .collect::<Vec<_>>(),
        [
            MountainTemplateRetryStage::Primary,
            MountainTemplateRetryStage::Primary,
            MountainTemplateRetryStage::OneStepSmaller,
            MountainTemplateRetryStage::SmallestSideEffect,
            MountainTemplateRetryStage::SmallestSideEffect,
        ]
    );
    assert_eq!(
        receipt.attempts.last().unwrap().placement.outcome,
        PlacePlayerGroupOutcome::Returned(0)
    );
    assert_eq!(
        receipt.attempts.last().unwrap().next_template,
        Some(receipt.attempts[3].template)
    );
    assert_eq!(pumps, 5);
}

#[test]
fn place_all_composes_initial_failure_all_retry_pumps_and_next_player_control() {
    let base_world = one_candidate_world();
    let base_groups = TerrainGroups {
        groups: vec![mountain_group(3)],
        ..TerrainGroups::default()
    };
    let base_mountains = mountains();
    let base_random = Random::new(0x1234_5678);
    let mut externals = Vec::new();

    for _ in 0..192 {
        let mut world = base_world.clone();
        let mut groups = base_groups.clone();
        let mut mountain_state = base_mountains.clone();
        let mut random = base_random;
        let mut host_events = Vec::new();
        let error = groups
            .place_all_with_player_group_inputs(
                &mut world,
                &mut random,
                &mut mountain_state,
                0,
                1,
                &externals,
                |event| host_events.push(event),
            )
            .unwrap_err();
        let PlaceAllError::GameplayPlacementUnavailable { preview, boundary } = error else {
            panic!("unexpected place_all error: {error:?}")
        };

        if let TerrainPlacementBoundary::PlayerGroupExternalSubsystem { request, .. } = boundary {
            let success = preview
                .player_group_mountain_retries
                .last()
                .and_then(|retry| retry.attempts.last())
                .is_some_and(|attempt| attempt.stage == MountainTemplateRetryStage::OneStepSmaller);
            externals.push(PlayerGroupExternalResolution::Mountains {
                request,
                liberr: i32::from(!success),
            });
            continue;
        }

        assert_eq!(
            boundary,
            TerrainPlacementBoundary::PlayerGroupPatternComplete { group_index: 0 }
        );
        let retries = &preview.player_group_mountain_retries;
        assert_eq!(retries.len(), 1);
        assert_eq!(retries[0].outcome, PlacePlayerGroupOutcome::Returned(1));
        assert_eq!(retries[0].randomizations.len(), 2);
        assert_eq!(preview.player_group_placed_after, [1]);
        assert_eq!(
            preview.player_group_prefix.as_ref().unwrap().len(),
            1 + retries[0].attempts.len()
        );
        assert_eq!(
            preview.player_group_host_events.len(),
            1 + retries[0].attempts.len()
        );
        assert!(preview.player_group_host_events.iter().all(|event| *event
            == PlaceAllHostEvent::NetDaemonProcessAllPlayer {
                group_index: 0,
                clump_index: 0,
                player_index: 0,
            }));
        assert_eq!(host_events[1..], preview.player_group_host_events);
        assert_eq!(world.wdata, base_world.wdata);
        assert_eq!(world.tdata, base_world.tdata);
        assert_eq!(world.start_x, base_world.start_x);
        assert_eq!(world.start_y, base_world.start_y);
        assert_eq!(groups, base_groups);
        assert_eq!(mountain_state, base_mountains);
        assert_eq!(random, base_random);
        return;
    }
    panic!("place_all mountain retry did not converge")
}
