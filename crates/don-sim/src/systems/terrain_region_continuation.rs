//! Exact post-`drop_tile` continuation of `TerrainGroup::place_region_group`.
//!
//! PDB/disassembly provenance: `place_region_group` `0x006a2f60`,
//! `randomize_orthogs` `0x006a2320`, `clear_group` `0x006a28e0`, and
//! `place_oil_deposits` `0x006a2d90`.  Good-object mutations remain explicit
//! host receipts; all world/list/RNG effects around them execute locally.

use super::ammo::vector_dist;
use super::map_terrain::{tflag, wflag, World, NEIGHBOUR_DX, NEIGHBOUR_DY};
use super::regions::Regions;
use super::terrain_drop_tile::{
    has_mountain_tcoords, DropTileError, DropTileExternalRequest, DropTileExternalResolution,
    DropTileReceipt,
};
use super::terrain_groups::TerrainGroup;
use super::terrain_region_placement::{
    next_region_cursor, reject_candidate, validate_inputs, PlaceRegionGroupCall,
    PlaceRegionGroupPrefixError, PlaceRegionGroupPrefixOutcome, PlaceRegionGroupPrefixReceipt,
    RegionCandidateAttempt, RegionDropTileInvocation, RegionHelpingState,
};
use crate::rng::Random;

const CARDINAL: [(i32, i32); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];
const TYPE_FOUR_DX: [i32; 9] = [0, -1, 0, 1, 1, 1, 0, -1, -1];
const TYPE_FOUR_DY: [i32; 9] = [0, -1, -1, -1, 0, 1, 1, 1, 0];
const OIL_GOOD_TYPE: i32 = 5;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RandomizeOrthogsReceipt {
    pub cardinal_rotation: usize,
    /// The diagonal cursor is not read by this caller, but retail still draws
    /// and advances it.
    pub diagonal_rotation: usize,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegionGrowthRejection {
    AlreadyInGroup,
    OutOfBounds,
    FixedOccupiedProbe,
    PlayerStartMinimum { player: usize, distance: i32 },
    PlayerStartMaximum { player: usize, distance: i32 },
    GroupMinimum { distance: i32 },
    GroupMaximum { distance: i32 },
    TypeFourImpassableNeighbor { offset_index: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionGrowthAttempt {
    pub base_index: usize,
    pub direction_index: usize,
    pub world_x: i32,
    pub world_y: i32,
    pub rejection: Option<RegionGrowthRejection>,
    pub drop: Option<DropTileReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionGrowthPassReceipt {
    pub selected_index: usize,
    pub base_order: Vec<usize>,
    pub orthogs: Vec<RandomizeOrthogsReceipt>,
    pub attempts: Vec<RegionGrowthAttempt>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClearRegionGroupReceipt {
    pub cleared_tiles: Vec<(i32, i32)>,
    pub external_requests: Vec<DropTileExternalRequest>,
    pub tdata_unblock_calls: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceOilDepositsReceipt {
    pub requested: i32,
    pub capped_requested: i32,
    pub placed: i32,
    pub external_requests: Vec<DropTileExternalRequest>,
    pub completed: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceRegionGroupOutcome {
    Returned(i32),
    ExternalResolutionRequired { request: DropTileExternalRequest },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceRegionGroupReceipt {
    pub prefix: PlaceRegionGroupPrefixReceipt,
    pub drops: Vec<DropTileReceipt>,
    pub helping_after: Option<RegionHelpingState>,
    pub growth_passes: Vec<RegionGrowthPassReceipt>,
    pub clear_passes: Vec<ClearRegionGroupReceipt>,
    pub oil_deposits: Option<PlaceOilDepositsReceipt>,
    pub external_resolutions_consumed: usize,
    pub outcome: PlaceRegionGroupOutcome,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceRegionGroupError {
    InvalidPrefix(PlaceRegionGroupPrefixError),
    InvalidDropTile(DropTileError),
    InvalidGrowthFixedProbe {
        region_id: usize,
        initial_x: i32,
    },
    InvalidCursorCycle {
        region_id: usize,
    },
    ExternalResolutionMismatch {
        expected: DropTileExternalRequest,
        actual: DropTileExternalRequest,
    },
    ExternalResolutionKindMismatch {
        request: DropTileExternalRequest,
    },
    UnsupportedClearTile {
        world_x: i32,
        world_y: i32,
    },
}

impl TerrainGroup {
    /// Execute the complete deterministic continuation on caller-owned preview
    /// state. Missing object-system effects are returned as ordered boundaries;
    /// callers resume transactionally by replaying on clones with the emitted
    /// resolutions appended.
    pub fn apply_place_region_group(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        call: PlaceRegionGroupCall,
        helping: Option<RegionHelpingState>,
        externals: &[DropTileExternalResolution],
    ) -> Result<PlaceRegionGroupReceipt, PlaceRegionGroupError> {
        validate_inputs(self, world, regions, call, helping)
            .map_err(PlaceRegionGroupError::InvalidPrefix)?;
        if matches!(self.group_type, 4 | 6) && call.target_tiles > 1 {
            validate_growth_probes(world, regions, call)?;
        }

        let mut prefix = self
            .plan_place_region_group_prefix(world, regions, random, call, helping)
            .map_err(PlaceRegionGroupError::InvalidPrefix)?;
        // Native clears length before its region draw. The planner records the
        // old length; moving this state-only write here preserves the same final
        // state while retaining fail-before-draw validation.
        self.tiles.items.clear();
        let mut receipt = PlaceRegionGroupReceipt {
            prefix: prefix.clone(),
            drops: Vec::new(),
            helping_after: helping,
            growth_passes: Vec::new(),
            clear_passes: Vec::new(),
            oil_deposits: None,
            external_resolutions_consumed: 0,
            outcome: PlaceRegionGroupOutcome::Returned(0),
            rng_state_after: random.state(),
        };

        let coords = &regions.list[call.region_id].coords.items;
        let initial_cursor = prefix.initial_cursor;
        let mut cursor = match prefix.outcome {
            PlaceRegionGroupPrefixOutcome::Exhausted => return Ok(receipt),
            PlaceRegionGroupPrefixOutcome::DropTile(_) => prefix.attempts.last().unwrap().cursor,
        };
        let mut pending = match prefix.outcome {
            PlaceRegionGroupPrefixOutcome::DropTile(invocation) => Some(invocation),
            PlaceRegionGroupPrefixOutcome::Exhausted => None,
        };
        // Native `local_30` holds the most recent entry-search drop result. A
        // stalled growth cleanup does not reset it before resuming the LFSR.
        let mut last_drop_result = 0;

        loop {
            let invocation = if let Some(invocation) = pending.take() {
                invocation
            } else {
                match next_accepted_candidate(
                    self,
                    world,
                    coords,
                    &mut cursor,
                    initial_cursor,
                    call,
                    receipt.helping_after,
                    &mut prefix.attempts,
                )? {
                    Some(invocation) => invocation,
                    None => {
                        receipt.outcome = PlaceRegionGroupOutcome::Returned(last_drop_result);
                        receipt.prefix = prefix;
                        receipt.rng_state_after = random.state();
                        return Ok(receipt);
                    }
                }
            };

            let Some(drop) = apply_drop_with_ordered_external(
                self,
                world,
                random,
                invocation,
                externals,
                &mut receipt.external_resolutions_consumed,
                &mut receipt.outcome,
            )?
            else {
                receipt.prefix = prefix;
                receipt.rng_state_after = random.state();
                return Ok(receipt);
            };
            let placed = drop.placed;
            last_drop_result = i32::from(placed);
            receipt.drops.push(drop);
            if !placed {
                continue;
            }

            let initial_x = invocation.world_x;
            let initial_y = invocation.world_y;
            update_helping_scores(
                world,
                self.group_type,
                initial_x,
                initial_y,
                &mut receipt.helping_after,
            );

            if matches!(self.group_type, 5 | 7 | 8) {
                receipt.outcome = PlaceRegionGroupOutcome::Returned(1);
                receipt.prefix = prefix;
                receipt.rng_state_after = random.state();
                return Ok(receipt);
            }

            loop {
                if call.target_tiles <= self.tiles.items.len() as i32 {
                    if self.group_type == 6 {
                        let Some(oil) = place_oil_deposits(
                            self,
                            world,
                            call.oil_deposits,
                            externals,
                            &mut receipt.external_resolutions_consumed,
                            &mut receipt.outcome,
                        )?
                        else {
                            receipt.prefix = prefix;
                            receipt.rng_state_after = random.state();
                            return Ok(receipt);
                        };
                        receipt.oil_deposits = Some(oil);
                    }
                    receipt.outcome = PlaceRegionGroupOutcome::Returned(1);
                    receipt.prefix = prefix;
                    receipt.rng_state_after = random.state();
                    return Ok(receipt);
                }

                let pass = grow_once(
                    self,
                    world,
                    random,
                    call,
                    initial_x,
                    initial_y,
                    externals,
                    &mut receipt.external_resolutions_consumed,
                    &mut receipt.outcome,
                )?;
                let placed = pass
                    .attempts
                    .iter()
                    .any(|attempt| attempt.drop.as_ref().is_some_and(|drop| drop.placed));
                receipt.growth_passes.push(pass);
                if matches!(
                    receipt.outcome,
                    PlaceRegionGroupOutcome::ExternalResolutionRequired { .. }
                ) {
                    receipt.prefix = prefix;
                    receipt.rng_state_after = random.state();
                    return Ok(receipt);
                }
                if placed {
                    continue;
                }

                let Some(clear) = clear_group(
                    self,
                    world,
                    externals,
                    &mut receipt.external_resolutions_consumed,
                    &mut receipt.outcome,
                )?
                else {
                    receipt.prefix = prefix;
                    receipt.rng_state_after = random.state();
                    return Ok(receipt);
                };
                receipt.clear_passes.push(clear);
                break;
            }
        }
    }
}

fn validate_growth_probes(
    world: &World,
    regions: &Regions,
    call: PlaceRegionGroupCall,
) -> Result<(), PlaceRegionGroupError> {
    let region = i32::try_from(call.region_id).ok();
    for &(initial_x, _) in &regions.list[call.region_id].coords.items {
        let stride = if call.place_players == 0 {
            world.tile_xs
        } else {
            world.xs
        };
        let index = region
            .and_then(|region| stride.checked_mul(region))
            .and_then(|base| base.checked_add(initial_x))
            .and_then(|index| usize::try_from(index).ok());
        let valid = if call.place_players == 0 {
            index.is_some_and(|index| index < world.tdata.len())
        } else {
            index.is_some_and(|index| index / 8 < world.start_city_locs.len())
        };
        if !valid {
            return Err(PlaceRegionGroupError::InvalidGrowthFixedProbe {
                region_id: call.region_id,
                initial_x,
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn next_accepted_candidate(
    group: &TerrainGroup,
    world: &World,
    coords: &[(i32, i32)],
    cursor: &mut u32,
    initial_cursor: u32,
    call: PlaceRegionGroupCall,
    helping: Option<RegionHelpingState>,
    attempts: &mut Vec<RegionCandidateAttempt>,
) -> Result<Option<RegionDropTileInvocation>, PlaceRegionGroupError> {
    loop {
        *cursor = next_region_cursor(*cursor, coords.len() as u32, initial_cursor).ok_or(
            PlaceRegionGroupError::InvalidCursorCycle {
                region_id: call.region_id,
            },
        )?;
        if *cursor == initial_cursor {
            return Ok(None);
        }
        let coord_index = *cursor as usize - 1;
        let (world_x, world_y) = coords[coord_index];
        let rejection = reject_candidate(group, world, world_x, world_y, call, helping);
        attempts.push(RegionCandidateAttempt {
            cursor: *cursor,
            coord_index,
            world_x,
            world_y,
            rejection,
        });
        if rejection.is_none() {
            return Ok(Some(RegionDropTileInvocation {
                world_x,
                world_y,
                group_type: group.group_type,
                group_radius: call.target_tiles / 4 + 1,
                land_subtype: call.land_subtype,
                target_tiles: call.target_tiles,
                oil_deposits: call.oil_deposits,
                group_index: call.group_index,
            }));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_drop_with_ordered_external(
    group: &mut TerrainGroup,
    world: &mut World,
    random: &mut Random,
    invocation: RegionDropTileInvocation,
    externals: &[DropTileExternalResolution],
    consumed: &mut usize,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<Option<DropTileReceipt>, PlaceRegionGroupError> {
    let external = if let Some(request) = group.drop_tile_external_request(invocation) {
        let Some(actual) = externals.get(*consumed).copied() else {
            *outcome = PlaceRegionGroupOutcome::ExternalResolutionRequired { request };
            return Ok(None);
        };
        ensure_external_request(request, actual)?;
        *consumed += 1;
        Some(actual)
    } else {
        None
    };
    group
        .apply_drop_tile(world, random, invocation, external)
        .map(Some)
        .map_err(PlaceRegionGroupError::InvalidDropTile)
}

fn update_helping_scores(
    world: &World,
    group_type: i32,
    x: i32,
    y: i32,
    helping: &mut Option<RegionHelpingState>,
) {
    let Some(state) = helping else { return };
    let slot = (group_type - 4) as usize;
    for player in 0..state.num_players {
        let distance = vector_dist(
            world.start_x.items[player].wrapping_sub(x),
            world.start_y.items[player].wrapping_sub(y),
        );
        state.scores[player][slot] = state.scores[player][slot].wrapping_add(distance);
    }
    let mut best_player = 0;
    let mut best_score = 0;
    for player in 0..state.num_players {
        if best_score < state.scores[player][slot] {
            best_player = player as i32;
            best_score = state.scores[player][slot];
        }
    }
    state.lowest_player[slot] = best_player;
}

#[allow(clippy::too_many_arguments)]
fn grow_once(
    group: &mut TerrainGroup,
    world: &mut World,
    random: &mut Random,
    call: PlaceRegionGroupCall,
    initial_x: i32,
    initial_y: i32,
    externals: &[DropTileExternalResolution],
    consumed: &mut usize,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<RegionGrowthPassReceipt, PlaceRegionGroupError> {
    let len = group.tiles.items.len();
    let selected_index = if len > 1 {
        (random.get(0, 0xffff) % len as i32) as usize
    } else {
        0
    };
    let mut base_order = Vec::with_capacity(len);
    let mut orthogs = Vec::with_capacity(len);
    let mut attempts = Vec::new();
    for offset in 1..=len {
        let base_index = (selected_index + offset) % len;
        base_order.push(base_index);
        let (base_x, base_y) = group.tiles.items[base_index];
        let orthog = randomize_orthogs(random);
        orthogs.push(orthog);
        for step in 0..4 {
            let direction_index = (orthog.cardinal_rotation + step) % 4;
            let (dx, dy) = CARDINAL[direction_index];
            let world_x = base_x.wrapping_add(dx);
            let world_y = base_y.wrapping_add(dy);
            let rejection =
                reject_growth_candidate(group, world, world_x, world_y, call, initial_x, initial_y);
            let mut attempt = RegionGrowthAttempt {
                base_index,
                direction_index,
                world_x,
                world_y,
                rejection,
                drop: None,
            };
            if rejection.is_none() {
                let invocation = RegionDropTileInvocation {
                    world_x,
                    world_y,
                    group_type: group.group_type,
                    group_radius: call.target_tiles / 4 + 1,
                    land_subtype: -1,
                    target_tiles: call.target_tiles,
                    oil_deposits: call.oil_deposits,
                    group_index: call.group_index,
                };
                let Some(drop) = apply_drop_with_ordered_external(
                    group, world, random, invocation, externals, consumed, outcome,
                )?
                else {
                    attempts.push(attempt);
                    return Ok(RegionGrowthPassReceipt {
                        selected_index,
                        base_order,
                        orthogs,
                        attempts,
                    });
                };
                let placed = drop.placed;
                attempt.drop = Some(drop);
                attempts.push(attempt);
                if placed {
                    return Ok(RegionGrowthPassReceipt {
                        selected_index,
                        base_order,
                        orthogs,
                        attempts,
                    });
                }
            } else {
                attempts.push(attempt);
            }
        }
    }
    Ok(RegionGrowthPassReceipt {
        selected_index,
        base_order,
        orthogs,
        attempts,
    })
}

fn randomize_orthogs(random: &mut Random) -> RandomizeOrthogsReceipt {
    let cardinal_rotation = (random.get(0, 0xffff) % 4) as usize;
    let diagonal_rotation = (random.get(0, 0xffff) % 4) as usize;
    RandomizeOrthogsReceipt {
        cardinal_rotation,
        diagonal_rotation,
        rng_state_after: random.state(),
    }
}

fn reject_growth_candidate(
    group: &TerrainGroup,
    world: &World,
    x: i32,
    y: i32,
    call: PlaceRegionGroupCall,
    initial_x: i32,
    initial_y: i32,
) -> Option<RegionGrowthRejection> {
    if group.tiles.items.contains(&(x, y)) {
        return Some(RegionGrowthRejection::AlreadyInGroup);
    }
    if !world.valid_w(x, y) {
        return Some(RegionGrowthRejection::OutOfBounds);
    }
    let fixed_index = if call.place_players == 0 {
        (world.tile_xs * call.region_id as i32 + initial_x) as usize
    } else {
        (world.xs * call.region_id as i32 + initial_x) as usize
    };
    let occupied = if call.place_players == 0 {
        let tile = world.tdata[fixed_index];
        tile & tflag::STARTED != 0 || tile & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING
    } else {
        world.start_city_locs[fixed_index >> 3] & (1 << (fixed_index & 7)) != 0
    };
    if occupied {
        return Some(RegionGrowthRejection::FixedOccupiedProbe);
    }
    for (player, (&start_x, &start_y)) in world
        .start_x
        .items
        .iter()
        .zip(&world.start_y.items)
        .enumerate()
    {
        let distance = vector_dist(x.wrapping_sub(start_x), y.wrapping_sub(start_y));
        if group.start_min > 0 && distance < group.start_min {
            return Some(RegionGrowthRejection::PlayerStartMinimum { player, distance });
        }
        if group.start_max > 0 && group.start_max < distance {
            return Some(RegionGrowthRejection::PlayerStartMaximum { player, distance });
        }
    }
    let distance = vector_dist(x.wrapping_sub(initial_x), y.wrapping_sub(initial_y));
    if group.cent_min > 0 && distance < group.cent_min {
        return Some(RegionGrowthRejection::GroupMinimum { distance });
    }
    if group.cent_max > 0 && group.cent_max < distance {
        return Some(RegionGrowthRejection::GroupMaximum { distance });
    }
    if group.group_type == 4 {
        for offset_index in 0..9 {
            let nx = x.wrapping_add(TYPE_FOUR_DX[offset_index]);
            let ny = y.wrapping_add(TYPE_FOUR_DY[offset_index]);
            if world.valid_w(nx, ny) && world.wdata(nx, ny).flags & wflag::IMPASSABLE_X != 0 {
                return Some(RegionGrowthRejection::TypeFourImpassableNeighbor { offset_index });
            }
        }
    }
    None
}

fn clear_group(
    group: &mut TerrainGroup,
    world: &mut World,
    externals: &[DropTileExternalResolution],
    consumed: &mut usize,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<Option<ClearRegionGroupReceipt>, PlaceRegionGroupError> {
    let mut receipt = ClearRegionGroupReceipt::default();
    for &(x, y) in group.tiles.items.clone().iter() {
        let cell = world.wdata(x, y);
        if cell.flags & (wflag::ROCKS | wflag::FOREST) == 0 {
            return Err(PlaceRegionGroupError::UnsupportedClearTile {
                world_x: x,
                world_y: y,
            });
        }
        world.wdata_mut(x, y).flags &= !(wflag::ROCKS | wflag::FOREST);
        let request = oil_request(x, y, false);
        let Some(()) = consume_oil_external(request, externals, consumed, outcome)? else {
            return Ok(None);
        };
        receipt.external_requests.push(request);
        world.set_oil_at(x, y, false);
        for index in 0..16 {
            let tx = x * 4 + index % 4;
            let ty = y * 4 + index / 4;
            if world.tmask(tx, ty) & tflag::BLOCKER_MASK != tflag::BLOCKER_MOUNTAIN {
                world.set_blocked_at(tx, ty, false);
                receipt.tdata_unblock_calls += 1;
            }
        }
        receipt.cleared_tiles.push((x, y));
    }
    group.tiles.items.clear();
    Ok(Some(receipt))
}

fn place_oil_deposits(
    group: &TerrainGroup,
    world: &mut World,
    requested: i32,
    externals: &[DropTileExternalResolution],
    consumed: &mut usize,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<Option<PlaceOilDepositsReceipt>, PlaceRegionGroupError> {
    let mut receipt = PlaceOilDepositsReceipt {
        requested,
        capped_requested: requested.min(group.tiles.items.len() as i32),
        ..PlaceOilDepositsReceipt::default()
    };
    if group.group_type != 6 || requested == 0 {
        return Ok(Some(receipt));
    }
    for &(x, y) in &group.tiles.items {
        if world.wdata(x, y).flags & wflag::OIL != 0 {
            continue;
        }
        let mut qualifies = false;
        for index in 0..8 {
            let nx = x + NEIGHBOUR_DX[index];
            let ny = y + NEIGHBOUR_DY[index];
            if world.valid_w(nx, ny)
                && world.wdata(nx, ny).flags & (wflag::ROCKS | wflag::FOREST) == 0
                && !has_mountain_tcoords(world, nx, ny)
            {
                qualifies = true;
                break;
            }
        }
        if qualifies {
            let request = oil_request(x, y, true);
            let Some(()) = consume_oil_external(request, externals, consumed, outcome)? else {
                return Ok(None);
            };
            receipt.external_requests.push(request);
            world.set_oil_at(x, y, true);
            receipt.placed += 1;
            if receipt.placed == receipt.capped_requested {
                break;
            }
        }
    }
    receipt.completed = receipt.placed == receipt.capped_requested;
    Ok(Some(receipt))
}

fn oil_request(x: i32, y: i32, enabled: bool) -> DropTileExternalRequest {
    DropTileExternalRequest::OilGoodMutation {
        world_x: x,
        world_y: y,
        enabled,
        good_type: OIL_GOOD_TYPE,
        coord_x: x.wrapping_mul(0x300).wrapping_add(0x180),
        coord_y: y.wrapping_mul(0x300).wrapping_add(0x180),
    }
}

fn consume_oil_external(
    request: DropTileExternalRequest,
    externals: &[DropTileExternalResolution],
    consumed: &mut usize,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<Option<()>, PlaceRegionGroupError> {
    let Some(actual) = externals.get(*consumed).copied() else {
        *outcome = PlaceRegionGroupOutcome::ExternalResolutionRequired { request };
        return Ok(None);
    };
    ensure_external_request(request, actual)?;
    *consumed += 1;
    Ok(Some(()))
}

fn ensure_external_request(
    expected: DropTileExternalRequest,
    resolution: DropTileExternalResolution,
) -> Result<(), PlaceRegionGroupError> {
    let kind_matches = matches!(
        (expected, resolution),
        (
            DropTileExternalRequest::MountainsAddMountain { .. },
            DropTileExternalResolution::Mountains { .. }
        ) | (
            DropTileExternalRequest::OilGoodMutation { .. },
            DropTileExternalResolution::OilGoodsApplied { .. }
        ) | (
            DropTileExternalRequest::CliffsPositionCliff { .. },
            DropTileExternalResolution::Cliffs { .. }
        )
    );
    let actual = match resolution {
        DropTileExternalResolution::Mountains { request, .. }
        | DropTileExternalResolution::OilGoodsApplied { request }
        | DropTileExternalResolution::Cliffs { request, .. } => request,
    };
    if !kind_matches {
        Err(PlaceRegionGroupError::ExternalResolutionKindMismatch { request: expected })
    } else if expected == actual {
        Ok(())
    } else {
        Err(PlaceRegionGroupError::ExternalResolutionMismatch { expected, actual })
    }
}
