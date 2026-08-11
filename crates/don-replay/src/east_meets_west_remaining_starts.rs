//! Exact remaining East Meets West active-player start loop.

use super::east_meets_west_add_start::{
    execute_first_add_start, EastMeetsWestAddStartError, EastMeetsWestAddStartInput,
    EastMeetsWestAddStartReceipt, EAST_MEETS_WEST_ADD_START_RETURN_VA,
};
use crate::continent::TeamContinentPartitionReceipt;
use crate::east_meets_west_place_start::{
    execute_first_place_start_selector, PlaceStartSelectorError, PlaceStartSelectorNext,
    PlaceStartSelectorReceipt,
};
use crate::east_meets_west_start_boundary::{
    execute_first_place_start_boundary, EastMeetsWestPlaceStartBoundary,
    EastMeetsWestStartBoundaryError,
};
use crate::initial::InitialWorldgenInputs;
use crate::region_centroid::EastMeetsWestCentroidReceipt;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{World, WorldSection};
use don_sim::systems::regions::Regions;

pub const EAST_MEETS_WEST_REMAINING_STARTS_ENTRY_VA: u32 = 0x0069_7466;
pub const EAST_MEETS_WEST_RECORD_START_X_VA: u32 = 0x0069_746c;
pub const EAST_MEETS_WEST_RECORD_START_Y_VA: u32 = 0x0069_7473;
pub const EAST_MEETS_WEST_START_COUNT_INCREMENT_VA: u32 = 0x0069_747a;
pub const EAST_MEETS_WEST_PLAYER_INCREMENT_VA: u32 = 0x0069_747e;
pub const EAST_MEETS_WEST_START_LOOP_JUMP_VA: u32 = 0x0069_747f;
pub const EAST_MEETS_WEST_START_LOOP_HEAD_VA: u32 = 0x0069_6d28;
pub const EAST_MEETS_WEST_ACTIVE_TEST_VA: u32 = 0x0069_6d3a;
pub const EAST_MEETS_WEST_INACTIVE_SKIP_VA: u32 = 0x0069_747e;
pub const EAST_MEETS_WEST_START_LOOP_EXIT_VA: u32 = 0x0069_7484;
pub const EAST_MEETS_WEST_CHECK_PLAYER_LAND_CALL_VA: u32 = 0x0069_7492;
pub const MAP_CHECK_PLAYER_LAND_VA: u32 = 0x0068_ef00;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestRemainingStartIteration {
    pub player_slot: u8,
    pub skipped_inactive_slots_before: Vec<u8>,
    pub start_index_before: usize,
    pub call: EastMeetsWestPlaceStartBoundary,
    pub selector: PlaceStartSelectorReceipt,
    pub mutation: Option<EastMeetsWestAddStartReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestRemainingStartsNext {
    /// All remaining active leaders appended a start. The native caller is
    /// parked before its post-loop `Map::check_player_land` call.
    CheckPlayerLand { caller_va: u32, primitive_va: u32 },
    /// A later selector returned zero. The unported caller fallback begins at
    /// the exact selector failure edge; no later active leader was visited.
    CallerFallback { player_slot: u8, next_va: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestRemainingStartsReceipt {
    pub entry_va: u32,
    pub record_x_va: u32,
    pub record_y_va: u32,
    pub start_count_increment_va: u32,
    pub player_increment_va: u32,
    pub loop_jump_va: u32,
    pub loop_head_va: u32,
    pub active_test_va: u32,
    pub inactive_skip_va: u32,
    pub loop_exit_va: u32,
    pub active_slots: Vec<u8>,
    pub first_recorded_start: (i32, i32),
    pub initial_start_count: usize,
    pub recorded_starts: Vec<(i32, i32)>,
    pub iterations: Vec<EastMeetsWestRemainingStartIteration>,
    pub inactive_tail_slots: Vec<u8>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub full_checksum_before: u32,
    pub full_checksum_after: u32,
    pub start_arrays_checksum_before: u32,
    pub start_arrays_checksum_after: u32,
    pub next: EastMeetsWestRemainingStartsNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestRemainingStartsError {
    PlayerLayoutMismatch,
    ActiveSlotOutOfRange {
        slot: u8,
    },
    ActiveSlotsNotStrictlyIncreasing {
        previous: u8,
        current: u8,
    },
    FirstAppendMismatch {
        field: &'static str,
    },
    WorldStartShapeMismatch {
        start_x: usize,
        start_y: usize,
        start_city_x: usize,
        start_city_y: usize,
    },
    StartBoundary(EastMeetsWestStartBoundaryError),
    Selector(PlaceStartSelectorError),
    AddStart(EastMeetsWestAddStartError),
    UnexpectedWalkedMutation {
        section: WorldSection,
    },
}

/// Continue from the first append's `0x00697466` return through every remaining
/// active leader whose identity is present in the exact partition receipt.
///
/// Successful selectors append through the same shipped call sites. A zero
/// selector return stops at its caller fallback. Only when every remaining
/// active leader succeeds does the receipt name, but not execute,
/// `Map::check_player_land`.
#[allow(clippy::too_many_arguments)]
pub fn execute_remaining_active_starts(
    inputs: &InitialWorldgenInputs,
    world: &mut World,
    regions: &Regions,
    partition: &TeamContinentPartitionReceipt,
    centroids: &EastMeetsWestCentroidReceipt,
    first_call: &EastMeetsWestPlaceStartBoundary,
    first_selector: &PlaceStartSelectorReceipt,
    first_mutation: &EastMeetsWestAddStartReceipt,
    rng: &mut Random,
) -> Result<EastMeetsWestRemainingStartsReceipt, EastMeetsWestRemainingStartsError> {
    if inputs.active_slots != partition.active_slots
        || inputs.active_slots.len() != inputs.active_teams.len()
    {
        return Err(EastMeetsWestRemainingStartsError::PlayerLayoutMismatch);
    }
    for pair in partition.active_slots.windows(2) {
        if pair[0] >= pair[1] {
            return Err(
                EastMeetsWestRemainingStartsError::ActiveSlotsNotStrictlyIncreasing {
                    previous: pair[0],
                    current: pair[1],
                },
            );
        }
    }
    if let Some(&slot) = partition.active_slots.iter().find(|&&slot| slot >= 8) {
        return Err(EastMeetsWestRemainingStartsError::ActiveSlotOutOfRange { slot });
    }
    let Some(&first_slot) = partition.active_slots.first() else {
        return Err(EastMeetsWestRemainingStartsError::PlayerLayoutMismatch);
    };
    if first_call.player_slot != first_slot {
        return Err(EastMeetsWestRemainingStartsError::FirstAppendMismatch {
            field: "player_slot",
        });
    }
    if first_selector.output != Some(first_mutation.input)
        || first_mutation.returned_start_index != 0
        || first_mutation.caller_return_va != EAST_MEETS_WEST_REMAINING_STARTS_ENTRY_VA
    {
        return Err(EastMeetsWestRemainingStartsError::FirstAppendMismatch {
            field: "selector/mutation edge",
        });
    }
    validate_world_start_shape(world, 1)?;
    if world.start_x.items[0] != first_mutation.input.0
        || world.start_y.items[0] != first_mutation.input.1
    {
        return Err(EastMeetsWestRemainingStartsError::FirstAppendMismatch {
            field: "World first start",
        });
    }

    let before = world.checksum_sections();
    let random_state_before = rng.state();
    let mut direct_rng_sites = Vec::new();
    let mut iterations = Vec::new();
    let mut recorded_starts = vec![first_mutation.input];
    let mut previous_slot = first_slot;
    let mut next = None;

    for &slot in partition.active_slots.iter().skip(1) {
        let skipped_inactive_slots_before = ((previous_slot + 1)..slot).collect::<Vec<_>>();
        let start_index_before = world.start_x.items.len();
        let mut partition_view = partition.clone();
        partition_view.active_slots.clear();
        partition_view.active_slots.push(slot);
        let call =
            execute_first_place_start_boundary(inputs, world, &partition_view, centroids, rng)
                .map_err(EastMeetsWestRemainingStartsError::StartBoundary)?;
        direct_rng_sites.extend_from_slice(&call.direct_rng_sites);
        let selector = execute_first_place_start_selector(
            world,
            regions,
            rng,
            call.region,
            call.min_start_distance,
            call.unread_argument,
        )
        .map_err(EastMeetsWestRemainingStartsError::Selector)?;
        direct_rng_sites.extend(selector.draws.iter().map(|draw| draw.call_va));

        let selector_next = selector.next.clone();
        match selector_next {
            PlaceStartSelectorNext::AddStartingLocation {
                caller_va,
                primitive_va,
            } => {
                let mutation = execute_first_add_start(
                    world,
                    EastMeetsWestAddStartInput {
                        selector_return_va: selector.return_va,
                        caller_va,
                        primitive_va,
                        output: selector.output,
                    },
                )
                .map_err(EastMeetsWestRemainingStartsError::AddStart)?;
                debug_assert_eq!(
                    mutation.caller_return_va,
                    EAST_MEETS_WEST_ADD_START_RETURN_VA
                );
                debug_assert_eq!(mutation.returned_start_index, start_index_before as i32);
                validate_world_start_shape(world, start_index_before + 1)?;
                recorded_starts.push(mutation.input);
                iterations.push(EastMeetsWestRemainingStartIteration {
                    player_slot: slot,
                    skipped_inactive_slots_before,
                    start_index_before,
                    call,
                    selector,
                    mutation: Some(mutation),
                });
            }
            PlaceStartSelectorNext::CallerFallback { next_va } => {
                iterations.push(EastMeetsWestRemainingStartIteration {
                    player_slot: slot,
                    skipped_inactive_slots_before,
                    start_index_before,
                    call,
                    selector,
                    mutation: None,
                });
                next = Some(EastMeetsWestRemainingStartsNext::CallerFallback {
                    player_slot: slot,
                    next_va,
                });
                break;
            }
        }
        previous_slot = slot;
    }

    let completed = next.is_none();
    let inactive_tail_slots = if completed {
        ((previous_slot + 1)..8).collect()
    } else {
        Vec::new()
    };
    let next = next.unwrap_or(EastMeetsWestRemainingStartsNext::CheckPlayerLand {
        caller_va: EAST_MEETS_WEST_CHECK_PLAYER_LAND_CALL_VA,
        primitive_va: MAP_CHECK_PLAYER_LAND_VA,
    });
    let after = world.checksum_sections();
    for section in WorldSection::all() {
        if section != WorldSection::StartArrays && before.section(section) != after.section(section)
        {
            return Err(EastMeetsWestRemainingStartsError::UnexpectedWalkedMutation { section });
        }
    }

    Ok(EastMeetsWestRemainingStartsReceipt {
        entry_va: EAST_MEETS_WEST_REMAINING_STARTS_ENTRY_VA,
        record_x_va: EAST_MEETS_WEST_RECORD_START_X_VA,
        record_y_va: EAST_MEETS_WEST_RECORD_START_Y_VA,
        start_count_increment_va: EAST_MEETS_WEST_START_COUNT_INCREMENT_VA,
        player_increment_va: EAST_MEETS_WEST_PLAYER_INCREMENT_VA,
        loop_jump_va: EAST_MEETS_WEST_START_LOOP_JUMP_VA,
        loop_head_va: EAST_MEETS_WEST_START_LOOP_HEAD_VA,
        active_test_va: EAST_MEETS_WEST_ACTIVE_TEST_VA,
        inactive_skip_va: EAST_MEETS_WEST_INACTIVE_SKIP_VA,
        loop_exit_va: EAST_MEETS_WEST_START_LOOP_EXIT_VA,
        active_slots: partition.active_slots.clone(),
        first_recorded_start: first_mutation.input,
        initial_start_count: 1,
        recorded_starts,
        iterations,
        inactive_tail_slots,
        random_state_before,
        random_state_after: rng.state(),
        direct_rng_sites,
        full_checksum_before: before.full,
        full_checksum_after: after.full,
        start_arrays_checksum_before: before.section(WorldSection::StartArrays).adler,
        start_arrays_checksum_after: after.section(WorldSection::StartArrays).adler,
        next,
    })
}

fn validate_world_start_shape(
    world: &World,
    starts: usize,
) -> Result<(), EastMeetsWestRemainingStartsError> {
    if world.start_x.items.len() != starts
        || world.start_y.items.len() != starts
        || world.start_city_x.items.len() != starts.saturating_mul(4)
        || world.start_city_y.items.len() != starts.saturating_mul(4)
    {
        return Err(EastMeetsWestRemainingStartsError::WorldStartShapeMismatch {
            start_x: world.start_x.items.len(),
            start_y: world.start_y.items.len(),
            start_city_x: world.start_city_x.items.len(),
            start_city_y: world.start_city_y.items.len(),
        });
    }
    Ok(())
}
