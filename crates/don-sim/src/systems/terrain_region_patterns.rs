//! Exact pattern 1--3 region/clump control in `TerrainGroups::place_all`.
//!
//! This is the enclosing retail control around `TerrainGroup::place_region_group`
//! at `0x006a79af..0x006a866f`: eligible-region scans, pattern-specific closed
//! list weighting, mountain-template cursor transactions, clump retries, and
//! `TerrainGroup::placed` writes.

use super::map_terrain::World;
use super::mountains::Mountains;
use super::regions::{Regions, LAND_REGION_COUNT, SEA_REGION_END, SEA_REGION_FIRST};
use super::terrain_drop_tile::{DropTileExternalRequest, DropTileExternalResolution};
use super::terrain_groups::TerrainGroup;
use super::terrain_region_continuation::{
    PlaceRegionGroupError, PlaceRegionGroupOutcome, PlaceRegionGroupReceipt,
};
use super::terrain_region_placement::{PlaceRegionGroupCall, RegionHelpingState};
use crate::rng::Random;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionPatternCallReceipt {
    /// Native `TerrainGroup::placed.length` before this call. Mountain-template
    /// retries share a slot because only the enclosing attempt is appended.
    pub placed_length_before: usize,
    pub region_id: usize,
    pub target_tiles: i32,
    pub land_subtype: i32,
    pub oil_deposits: i32,
    pub is_helping: bool,
    pub placement: PlaceRegionGroupReceipt,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegionPatternOutcome {
    Complete,
    ExternalResolutionRequired { request: DropTileExternalRequest },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionPatternReceipt {
    pub pattern: i32,
    pub eligible_regions: Vec<usize>,
    /// Pattern 2/3 closed-list order after proportional duplication.
    pub region_cycle: Vec<usize>,
    pub initial_cycle_index: Option<usize>,
    pub region_selection_draws: u32,
    pub calls: Vec<RegionPatternCallReceipt>,
    pub failed_clumps: Vec<usize>,
    pub helping_after: Option<RegionHelpingState>,
    pub external_resolutions_consumed: usize,
    pub outcome: RegionPatternOutcome,
    pub rng_state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegionPatternError {
    UnsupportedPattern {
        pattern: i32,
    },
    UnsupportedGroupType {
        group_type: i32,
    },
    ClumpSizeLengthMismatch {
        clumps: usize,
        primary: usize,
        secondary: usize,
    },
    MissingHelpingState {
        players: usize,
    },
    StartCoordinateOutOfBounds {
        player: usize,
        x: i32,
        y: i32,
    },
    InvalidRegionPlacement(PlaceRegionGroupError),
}

impl TerrainGroup {
    /// Execute one selected pattern-1/2/3 group on caller-owned preview state.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_region_pattern(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        mountains: &mut Mountains,
        pattern: i32,
        primary_sizes: &[i32],
        secondary_sizes: &[i32],
        normalized_clumps_for_type: i32,
        place_players: i32,
        group_index: usize,
        helping: Option<RegionHelpingState>,
        externals: &[DropTileExternalResolution],
    ) -> Result<RegionPatternReceipt, RegionPatternError> {
        if !(1..=3).contains(&pattern) {
            return Err(RegionPatternError::UnsupportedPattern { pattern });
        }
        if !(4..=8).contains(&self.group_type) {
            return Err(RegionPatternError::UnsupportedGroupType {
                group_type: self.group_type,
            });
        }
        if primary_sizes.len() != secondary_sizes.len() {
            return Err(RegionPatternError::ClumpSizeLengthMismatch {
                clumps: primary_sizes.len(),
                primary: primary_sizes.len(),
                secondary: secondary_sizes.len(),
            });
        }
        if !world.start_x.items.is_empty() && helping.is_none() {
            return Err(RegionPatternError::MissingHelpingState {
                players: world.start_x.items.len(),
            });
        }
        if pattern == 3 {
            for (player, (&x, &y)) in world
                .start_x
                .items
                .iter()
                .zip(&world.start_y.items)
                .enumerate()
            {
                if !world.valid_w(x, y) {
                    return Err(RegionPatternError::StartCoordinateOutOfBounds { player, x, y });
                }
            }
        }

        let eligible_regions = eligible_regions(self.group_type, pattern, world, regions);
        let mut receipt = RegionPatternReceipt {
            pattern,
            eligible_regions: eligible_regions.clone(),
            region_cycle: Vec::new(),
            initial_cycle_index: None,
            region_selection_draws: 0,
            calls: Vec::new(),
            failed_clumps: Vec::new(),
            helping_after: helping,
            external_resolutions_consumed: 0,
            outcome: RegionPatternOutcome::Complete,
            rng_state_after: random.state(),
        };

        if pattern == 1 {
            apply_pattern_one(
                self,
                world,
                regions,
                random,
                mountains,
                primary_sizes,
                secondary_sizes,
                normalized_clumps_for_type,
                place_players,
                group_index,
                externals,
                &mut receipt,
            )?;
        } else {
            apply_pattern_two_or_three(
                self,
                world,
                regions,
                random,
                mountains,
                primary_sizes,
                secondary_sizes,
                normalized_clumps_for_type,
                place_players,
                group_index,
                externals,
                &mut receipt,
            )?;
        }
        receipt.rng_state_after = random.state();
        Ok(receipt)
    }
}

fn eligible_regions(group_type: i32, pattern: i32, world: &World, regions: &Regions) -> Vec<usize> {
    let range = if group_type == 7 {
        SEA_REGION_FIRST..SEA_REGION_END
    } else {
        1..LAND_REGION_COUNT
    };
    range
        .filter(|&region_id| regions.list[region_id].size > 5)
        .filter(|&region_id| {
            pattern != 3
                || !world
                    .start_x
                    .items
                    .iter()
                    .zip(&world.start_y.items)
                    .any(|(&x, &y)| world.wdata(x, y).region == region_id as i16)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn apply_pattern_one(
    group: &mut TerrainGroup,
    world: &mut World,
    regions: &Regions,
    random: &mut Random,
    mountains: &mut Mountains,
    primary: &[i32],
    secondary: &[i32],
    helping_threshold: i32,
    place_players: i32,
    group_index: usize,
    externals: &[DropTileExternalResolution],
    receipt: &mut RegionPatternReceipt,
) -> Result<(), RegionPatternError> {
    let eligible = receipt.eligible_regions.clone();
    for &region_id in &eligible {
        for clump_index in 0..primary.len() {
            let is_helping = helping_threshold <= clump_index as i32;
            let subtype = if group.group_type == 5 {
                mountains.get_range_raw(primary[clump_index])
            } else {
                primary[clump_index]
            };
            let Some(result) = invoke_region_group(
                group,
                world,
                regions,
                random,
                primary[clump_index],
                secondary[clump_index],
                region_id,
                subtype,
                is_helping,
                place_players,
                group_index,
                externals,
                receipt,
            )?
            else {
                return Ok(());
            };
            if result == 0 {
                receipt.failed_clumps.push(clump_index);
            }
            group.placed.push(result);
        }
    }

    let failures = receipt.failed_clumps.clone();
    for clump_index in failures {
        for &region_id in &eligible {
            let subtype = if group.group_type == 5 {
                mountains.get_range_raw(primary[clump_index])
            } else {
                primary[clump_index]
            };
            let Some(result) = invoke_region_group(
                group,
                world,
                regions,
                random,
                primary[clump_index],
                secondary[clump_index],
                region_id,
                subtype,
                true,
                place_players,
                group_index,
                externals,
                receipt,
            )?
            else {
                return Ok(());
            };
            if clump_index < group.placed.len() {
                group.placed[clump_index] = result;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_pattern_two_or_three(
    group: &mut TerrainGroup,
    world: &mut World,
    regions: &Regions,
    random: &mut Random,
    mountains: &mut Mountains,
    primary: &[i32],
    secondary: &[i32],
    helping_threshold: i32,
    place_players: i32,
    group_index: usize,
    externals: &[DropTileExternalResolution],
    receipt: &mut RegionPatternReceipt,
) -> Result<(), RegionPatternError> {
    let mut cycle = receipt.eligible_regions.clone();
    if primary.len() > 6 && cycle.len() < 6 && !cycle.is_empty() {
        let original = cycle.clone();
        let total = original.iter().fold(0.0f32, |sum, &region| {
            sum + regions.list[region].size as f32
        });
        let mut max_region = original[0];
        let mut max_size = 0;
        for &region in &original {
            let size = regions.list[region].size;
            if max_size < size {
                max_size = size;
                max_region = region;
            }
            let allocation = (((size as f32 / total) * primary.len() as f32) as i32).max(0);
            for _ in 0..allocation.saturating_sub(1) {
                cycle.push(region);
            }
        }
        cycle.push(max_region);
    }
    receipt.region_cycle = cycle.clone();
    if cycle.is_empty() {
        return Ok(());
    }
    let selected = if cycle.len() > 1 {
        receipt.region_selection_draws = 1;
        (random.get(0, 0xffff) % cycle.len() as i32) as usize
    } else {
        0
    };
    receipt.initial_cycle_index = Some(selected);
    let mut cycle_index = selected;
    let mut marker_region = cycle[selected];
    let mut clump_index = 0usize;
    let mut used_mountain_regions = [false; LAND_REGION_COUNT];

    while clump_index < primary.len() {
        let region_id = cycle[cycle_index];
        let is_helping = helping_threshold <= clump_index as i32;
        let target = primary[clump_index];
        let subtype = match group.group_type {
            5 => mountains.get_range_raw(target),
            8 => (target - 1).max(1),
            _ => -1,
        };
        let result = if group.group_type == 5 {
            let Some(result) = invoke_type_five_pattern_retry(
                group,
                world,
                regions,
                random,
                mountains,
                target,
                secondary[clump_index],
                region_id,
                subtype,
                is_helping,
                place_players,
                group_index,
                &mut used_mountain_regions,
                externals,
                receipt,
            )?
            else {
                return Ok(());
            };
            result
        } else {
            let Some(result) = invoke_region_group(
                group,
                world,
                regions,
                random,
                target,
                secondary[clump_index],
                region_id,
                subtype,
                is_helping,
                place_players,
                group_index,
                externals,
                receipt,
            )?
            else {
                return Ok(());
            };
            result
        };
        group.placed.push(result);

        let old_clump = clump_index;
        clump_index += 1;
        let current_region = region_id;
        cycle_index = (cycle_index + 1) % cycle.len();
        let next_region = cycle[cycle_index];
        if result == 0 {
            if next_region != marker_region {
                clump_index = old_clump;
            }
        } else {
            marker_region = current_region;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn invoke_type_five_pattern_retry(
    group: &mut TerrainGroup,
    world: &mut World,
    regions: &Regions,
    random: &mut Random,
    mountains: &mut Mountains,
    target: i32,
    oil: i32,
    region_id: usize,
    initial_subtype: i32,
    is_helping: bool,
    place_players: i32,
    group_index: usize,
    used_regions: &mut [bool; LAND_REGION_COUNT],
    externals: &[DropTileExternalResolution],
    receipt: &mut RegionPatternReceipt,
) -> Result<Option<i32>, RegionPatternError> {
    let Some(mut result) = invoke_region_group(
        group,
        world,
        regions,
        random,
        target,
        oil,
        region_id,
        initial_subtype,
        is_helping,
        place_players,
        group_index,
        externals,
        receipt,
    )?
    else {
        return Ok(None);
    };
    if result == 0 && !used_regions[region_id] {
        let saved_helping = receipt.helping_after.map(|state| state.is_helping);
        let ignored = invoke_region_group(
            group,
            world,
            regions,
            random,
            target,
            oil,
            region_id,
            initial_subtype,
            false,
            place_players,
            group_index,
            externals,
            receipt,
        )?;
        if ignored.is_none() {
            restore_helping_flag(&mut receipt.helping_after, saved_helping);
            return Ok(None);
        }

        let first = mountains.get_range_raw(target);
        result = invoke_mountain_range_cycle(
            group,
            world,
            regions,
            random,
            mountains,
            target,
            oil,
            region_id,
            first,
            false,
            place_players,
            group_index,
            externals,
            receipt,
        )?;
        if matches!(
            receipt.outcome,
            RegionPatternOutcome::ExternalResolutionRequired { .. }
        ) {
            restore_helping_flag(&mut receipt.helping_after, saved_helping);
            return Ok(None);
        }

        if result == 0 {
            let first = mountains.get_range_raw(target);
            let saved_coast_space = group.coast_space;
            if group.coast_space != 0 {
                group.coast_space -= 1;
            }
            result = invoke_mountain_range_cycle(
                group,
                world,
                regions,
                random,
                mountains,
                target,
                oil,
                region_id,
                first,
                false,
                place_players,
                group_index,
                externals,
                receipt,
            )?;
            group.coast_space = saved_coast_space;
            if matches!(
                receipt.outcome,
                RegionPatternOutcome::ExternalResolutionRequired { .. }
            ) {
                restore_helping_flag(&mut receipt.helping_after, saved_helping);
                return Ok(None);
            }
        }

        if result == 0 {
            mountains.randomize_mountains(random);
            if target > 1 {
                let smaller = target - 1;
                let first = mountains.get_range_raw(smaller);
                result = invoke_mountain_range_cycle(
                    group,
                    world,
                    regions,
                    random,
                    mountains,
                    smaller,
                    oil,
                    region_id,
                    first,
                    false,
                    place_players,
                    group_index,
                    externals,
                    receipt,
                )?;
                if matches!(
                    receipt.outcome,
                    RegionPatternOutcome::ExternalResolutionRequired { .. }
                ) {
                    restore_helping_flag(&mut receipt.helping_after, saved_helping);
                    return Ok(None);
                }
                mountains.randomize_mountains(random);
                if result != 0 && target == 2 {
                    let first = mountains.get_range_raw(1);
                    let _ = invoke_mountain_range_cycle(
                        group,
                        world,
                        regions,
                        random,
                        mountains,
                        1,
                        oil,
                        region_id,
                        first,
                        false,
                        place_players,
                        group_index,
                        externals,
                        receipt,
                    )?;
                    if matches!(
                        receipt.outcome,
                        RegionPatternOutcome::ExternalResolutionRequired { .. }
                    ) {
                        restore_helping_flag(&mut receipt.helping_after, saved_helping);
                        return Ok(None);
                    }
                }
            }
        }
        restore_helping_flag(&mut receipt.helping_after, saved_helping);
    }
    if result != 0 {
        used_regions[region_id] = true;
    }
    Ok(Some(result))
}

#[allow(clippy::too_many_arguments)]
fn invoke_mountain_range_cycle(
    group: &mut TerrainGroup,
    world: &mut World,
    regions: &Regions,
    random: &mut Random,
    mountains: &mut Mountains,
    target: i32,
    oil: i32,
    region_id: usize,
    mut subtype: i32,
    is_helping: bool,
    place_players: i32,
    group_index: usize,
    externals: &[DropTileExternalResolution],
    receipt: &mut RegionPatternReceipt,
) -> Result<i32, RegionPatternError> {
    let first = subtype;
    loop {
        let Some(result) = invoke_region_group(
            group,
            world,
            regions,
            random,
            target,
            oil,
            region_id,
            subtype,
            is_helping,
            place_players,
            group_index,
            externals,
            receipt,
        )?
        else {
            return Ok(0);
        };
        subtype = mountains.get_range_raw(target);
        if result != 0 || subtype == first {
            return Ok(result);
        }
    }
}

fn restore_helping_flag(helping: &mut Option<RegionHelpingState>, flag: Option<bool>) {
    if let (Some(helping), Some(flag)) = (helping, flag) {
        helping.is_helping = flag;
    }
}

#[allow(clippy::too_many_arguments)]
fn invoke_region_group(
    group: &mut TerrainGroup,
    world: &mut World,
    regions: &Regions,
    random: &mut Random,
    target: i32,
    oil: i32,
    region_id: usize,
    subtype: i32,
    is_helping: bool,
    place_players: i32,
    group_index: usize,
    externals: &[DropTileExternalResolution],
    receipt: &mut RegionPatternReceipt,
) -> Result<Option<i32>, RegionPatternError> {
    let helping = receipt.helping_after.map(|mut state| {
        state.is_helping = is_helping;
        state
    });
    let call = PlaceRegionGroupCall {
        target_tiles: target,
        region_id,
        land_subtype: subtype,
        oil_deposits: oil,
        place_players,
        group_index,
    };
    let placement = group
        .apply_place_region_group(
            world,
            regions,
            random,
            call,
            helping,
            &externals[receipt.external_resolutions_consumed..],
        )
        .map_err(RegionPatternError::InvalidRegionPlacement)?;
    receipt.external_resolutions_consumed += placement.external_resolutions_consumed;
    receipt.helping_after = placement.helping_after;
    let outcome = placement.outcome;
    receipt.calls.push(RegionPatternCallReceipt {
        placed_length_before: group.placed.len(),
        region_id,
        target_tiles: target,
        land_subtype: subtype,
        oil_deposits: oil,
        is_helping,
        placement,
    });
    match outcome {
        PlaceRegionGroupOutcome::Returned(result) => Ok(Some(result)),
        PlaceRegionGroupOutcome::ExternalResolutionRequired { request } => {
            receipt.outcome = RegionPatternOutcome::ExternalResolutionRequired { request };
            Ok(None)
        }
    }
}
