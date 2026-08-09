//! Exact type-4/type-6 growth continuation of `place_player_group`.
//!
//! Retail enters this block at `0x006a4cf0` after the first successful
//! `drop_tile`. Direction data is read from the shipped arrays
//! `0x00add254/0x00add214` (cardinals) and `0x00adc3e4/0x00adc3c4`
//! (diagonals). The unusual type-4 base cursor is intentional: the instruction
//! stream at `0x006a4eab` and `0x006a5545` keeps it at tile zero while the
//! enclosing termination cursor traverses the list.

use super::ammo::vector_dist;
use super::map_terrain::{tflag, wflag, World, NEIGHBOUR_DX, NEIGHBOUR_DY};
use super::terrain_drop_tile::{has_mountain_tcoords, DropTileError, DropTileReceipt};
use super::terrain_groups::TerrainGroup;
use super::terrain_player_group::{
    PlacePlayerGroupCall, PlayerGroupExternalRequest, PlayerGroupExternalResolution,
};
use super::terrain_region_placement::RegionDropTileInvocation;
use crate::rng::Random;

const CARDINAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
const DIAGONAL: [(i32, i32); 4] = [(-1, -1), (1, -1), (1, 1), (-1, 1)];
const OIL_GOOD_TYPE: i32 = 5;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlayerOrthogRotation {
    pub cardinal_rotation: usize,
    pub diagonal_rotation: usize,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerGrowthDirectionKind {
    Cardinal,
    Diagonal,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerGrowthRejection {
    AlreadyInGroup,
    OutOfBounds,
    StartCity,
    StartMinimum { distance: i32 },
    StartMaximum { distance: i32 },
    CenterMinimum { distance: i32 },
    CenterMaximum { distance: i32 },
    EdgeMinimum { distance: i32 },
    EdgeMaximum { distance: i32 },
    CornerMinimum { distance: i32 },
    CornerMaximum { distance: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerGrowthAttempt {
    pub base_index: usize,
    pub direction_step: usize,
    pub direction_kind: PlayerGrowthDirectionKind,
    pub direction_index: usize,
    pub world_x: i32,
    pub world_y: i32,
    pub rejection: Option<PlayerGrowthRejection>,
    pub drop: Option<DropTileReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerGrowthPassReceipt {
    pub selected_index: usize,
    pub selection_draws: u32,
    pub base_order: Vec<usize>,
    pub rotations: Vec<PlayerOrthogRotation>,
    pub attempts: Vec<PlayerGrowthAttempt>,
    pub placed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClearPlayerGroupReceipt {
    pub cleared_tiles: Vec<(i32, i32)>,
    pub external_requests: Vec<PlayerGroupExternalRequest>,
    pub tdata_unblock_calls: u32,
    pub completed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerOilDepositsReceipt {
    pub requested: i32,
    pub capped_requested: i32,
    pub placed: i32,
    pub external_requests: Vec<PlayerGroupExternalRequest>,
    pub completed: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerGroupGrowthOutcome {
    Returned(i32),
    ExternalResolutionRequired { request: PlayerGroupExternalRequest },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerGroupGrowthReceipt {
    pub passes: Vec<PlayerGrowthPassReceipt>,
    pub clear: Option<ClearPlayerGroupReceipt>,
    pub oil_deposits: Option<PlayerOilDepositsReceipt>,
    pub external_resolutions_consumed: usize,
    pub outcome: PlayerGroupGrowthOutcome,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerGroupGrowthError {
    UnsupportedGroupType {
        group_type: i32,
    },
    InvalidPlayer {
        player_index: usize,
        players: usize,
    },
    InvalidDropTile(DropTileError),
    ExternalResolutionMismatch {
        expected: PlayerGroupExternalRequest,
        actual: PlayerGroupExternalRequest,
    },
    ExternalResolutionKindMismatch {
        request: PlayerGroupExternalRequest,
    },
    UnsupportedClearTile {
        world_x: i32,
        world_y: i32,
    },
}

impl TerrainGroup {
    /// Continue a successful first player-group drop through randomized growth,
    /// cleanup, and the type-6 oil tail. Missing Good-object effects are surfaced
    /// in native order and must be supplied by replaying on caller-owned preview
    /// state with the emitted resolutions appended.
    pub fn apply_player_group_growth(
        &mut self,
        world: &mut World,
        random: &mut Random,
        call: PlacePlayerGroupCall,
        formation_x: &mut Vec<i32>,
        formation_y: &mut Vec<i32>,
        externals: &[PlayerGroupExternalResolution],
    ) -> Result<PlayerGroupGrowthReceipt, PlayerGroupGrowthError> {
        if !matches!(self.group_type, 4 | 6) {
            return Err(PlayerGroupGrowthError::UnsupportedGroupType {
                group_type: self.group_type,
            });
        }
        if call.player_index >= world.start_x.items.len() {
            return Err(PlayerGroupGrowthError::InvalidPlayer {
                player_index: call.player_index,
                players: world.start_x.items.len(),
            });
        }

        let mut receipt = PlayerGroupGrowthReceipt {
            passes: Vec::new(),
            clear: None,
            oil_deposits: None,
            external_resolutions_consumed: 0,
            outcome: PlayerGroupGrowthOutcome::Returned(0),
            rng_state_after: random.state(),
        };

        // This is native's `local_20 == 0` arm. It is normally reached after a
        // successful growth drop jumps over the exhausted-search assignment.
        if self.tiles.items.is_empty() {
            return finish_success(self, world, random, call, externals, receipt);
        }

        loop {
            let pass =
                self.grow_player_group_once(world, random, call, formation_x, formation_y)?;
            let placed = pass.placed;
            receipt.passes.push(pass);
            if placed {
                if call.target_tiles <= self.tiles.items.len() as i32 {
                    return finish_success(self, world, random, call, externals, receipt);
                }
                continue;
            }

            // Exhausting the circular base scan sets local_20=1. Native clears
            // only undersized groups; a group already at target returns zero but
            // retains its first tile(s).
            if (self.tiles.items.len() as i32) < call.target_tiles {
                let Some(clear) = clear_player_group(
                    self,
                    world,
                    externals,
                    &mut receipt.external_resolutions_consumed,
                    &mut receipt.outcome,
                )?
                else {
                    receipt.rng_state_after = random.state();
                    return Ok(receipt);
                };
                receipt.clear = Some(clear);
            }
            receipt.rng_state_after = random.state();
            return Ok(receipt);
        }
    }

    fn grow_player_group_once(
        &mut self,
        world: &mut World,
        random: &mut Random,
        call: PlacePlayerGroupCall,
        formation_x: &mut Vec<i32>,
        formation_y: &mut Vec<i32>,
    ) -> Result<PlayerGrowthPassReceipt, PlayerGroupGrowthError> {
        let len = self.tiles.items.len();
        let (selected_index, selection_draws) = if len > 1 {
            ((random.get(0, 0xffff) % len as i32) as usize, 1)
        } else {
            (0, 0)
        };
        let marker = selected_index + 1;
        let mut termination_cursor = marker;
        let mut base_cursor = if self.group_type == 4 { 0 } else { marker };
        let mut pass = PlayerGrowthPassReceipt {
            selected_index,
            selection_draws,
            base_order: Vec::with_capacity(len),
            rotations: Vec::with_capacity(len),
            attempts: Vec::new(),
            placed: false,
        };

        loop {
            let not_at_end = termination_cursor != len;
            let next_termination = if not_at_end { termination_cursor } else { 0 };
            let base_index = if not_at_end { base_cursor } else { 0 };
            pass.base_order.push(base_index);
            let rotation = randomize_orthogs(random);
            pass.rotations.push(rotation);

            for direction_step in 0..4 {
                let (direction_kind, direction_index, (dx, dy)) =
                    if len < 3 || random.get(0, 0xffff) & 1 == 0 {
                        let index = (rotation.cardinal_rotation + direction_step) & 3;
                        (PlayerGrowthDirectionKind::Cardinal, index, CARDINAL[index])
                    } else {
                        let index = (rotation.diagonal_rotation + direction_step) & 3;
                        (PlayerGrowthDirectionKind::Diagonal, index, DIAGONAL[index])
                    };
                let (base_x, base_y) = self.tiles.items[base_index];
                let world_x = base_x.wrapping_add(dx);
                let world_y = base_y.wrapping_add(dy);
                let rejection = reject_growth_candidate(self, world, call, world_x, world_y);
                let mut attempt = PlayerGrowthAttempt {
                    base_index,
                    direction_step,
                    direction_kind,
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
                        group_type: self.group_type,
                        group_radius: call
                            .target_tiles
                            .wrapping_add((call.target_tiles >> 31) & 3)
                            .wrapping_shr(2)
                            .wrapping_add(1),
                        land_subtype: -1,
                        target_tiles: call.target_tiles,
                        oil_deposits: call.oil_deposits,
                        group_index: call.group_index,
                    };
                    let drop = self
                        .apply_drop_tile(world, random, invocation, None)
                        .map_err(PlayerGroupGrowthError::InvalidDropTile)?;
                    let placed = drop.placed;
                    attempt.drop = Some(drop);
                    pass.attempts.push(attempt);
                    if placed {
                        if self.group_type == 4 {
                            let (first_x, first_y) = self.tiles.items[0];
                            formation_x.push(world_x.wrapping_sub(first_x));
                            formation_y.push(world_y.wrapping_sub(first_y));
                        }
                        pass.placed = true;
                        return Ok(pass);
                    }
                } else {
                    pass.attempts.push(attempt);
                }
            }

            termination_cursor = next_termination + 1;
            base_cursor = if self.group_type == 4 {
                base_index
            } else {
                base_index + 1
            };
            if termination_cursor == marker {
                return Ok(pass);
            }
        }
    }
}

fn randomize_orthogs(random: &mut Random) -> PlayerOrthogRotation {
    let cardinal_rotation = (random.get(0, 0xffff) % 4) as usize;
    let diagonal_rotation = (random.get(0, 0xffff) % 4) as usize;
    PlayerOrthogRotation {
        cardinal_rotation,
        diagonal_rotation,
        rng_state_after: random.state(),
    }
}

fn reject_growth_candidate(
    group: &TerrainGroup,
    world: &World,
    call: PlacePlayerGroupCall,
    x: i32,
    y: i32,
) -> Option<PlayerGrowthRejection> {
    if group.tiles.items.contains(&(x, y)) {
        return Some(PlayerGrowthRejection::AlreadyInGroup);
    }
    if !world.valid_w(x, y) {
        return Some(PlayerGrowthRejection::OutOfBounds);
    }
    let fixed_index = (world.xs * y + x) as usize;
    if world.start_city_locs[fixed_index >> 3] & (1 << (fixed_index & 7)) != 0 {
        return Some(PlayerGrowthRejection::StartCity);
    }

    let distance = vector_dist(
        x.wrapping_sub(world.start_x.items[call.player_index]),
        y.wrapping_sub(world.start_y.items[call.player_index]),
    );
    if distance < group.start_min {
        return Some(PlayerGrowthRejection::StartMinimum { distance });
    }
    if group.start_max < distance {
        return Some(PlayerGrowthRejection::StartMaximum { distance });
    }

    let centre_x = if world.xs / 2 < x {
        world.xs / 2 + 1
    } else {
        world.xs / 2
    };
    let centre_y = if world.ys / 2 < y {
        world.ys / 2 + 1
    } else {
        world.ys / 2
    };
    let centre_distance = vector_dist(x - centre_x, y - centre_y);
    if group.cent_min > 0 && centre_distance < group.cent_min {
        return Some(PlayerGrowthRejection::CenterMinimum {
            distance: centre_distance,
        });
    }
    if group.cent_max > 0 && group.cent_max < centre_distance {
        return Some(PlayerGrowthRejection::CenterMaximum {
            distance: centre_distance,
        });
    }

    let edge_distance = x.min(y).min(world.xs - x).min(world.ys - y);
    if group.edge_min > 0 && edge_distance < group.edge_min {
        return Some(PlayerGrowthRejection::EdgeMinimum {
            distance: edge_distance,
        });
    }
    if group.edge_max > 0 && group.edge_max < edge_distance {
        return Some(PlayerGrowthRejection::EdgeMaximum {
            distance: edge_distance,
        });
    }

    let mut corner_distance = 99_999;
    for (corner_x, corner_y) in [(0, 0), (0, world.ys), (world.xs, 0), (world.xs, world.ys)] {
        corner_distance = corner_distance.min(vector_dist(x - corner_x, y - corner_y));
    }
    if group.corner_min > 0 && corner_distance < group.corner_min {
        return Some(PlayerGrowthRejection::CornerMinimum {
            distance: corner_distance,
        });
    }
    if group.corner_max > 0 && group.corner_max < corner_distance {
        return Some(PlayerGrowthRejection::CornerMaximum {
            distance: corner_distance,
        });
    }
    None
}

fn finish_success(
    group: &TerrainGroup,
    world: &mut World,
    random: &Random,
    call: PlacePlayerGroupCall,
    externals: &[PlayerGroupExternalResolution],
    mut receipt: PlayerGroupGrowthReceipt,
) -> Result<PlayerGroupGrowthReceipt, PlayerGroupGrowthError> {
    if group.group_type == 6 {
        let Some(oil) = place_oil_deposits(
            group,
            world,
            call.oil_deposits,
            externals,
            &mut receipt.external_resolutions_consumed,
            &mut receipt.outcome,
        )?
        else {
            receipt.rng_state_after = random.state();
            return Ok(receipt);
        };
        receipt.oil_deposits = Some(oil);
    }
    receipt.outcome = PlayerGroupGrowthOutcome::Returned(1);
    receipt.rng_state_after = random.state();
    Ok(receipt)
}

fn clear_player_group(
    group: &mut TerrainGroup,
    world: &mut World,
    externals: &[PlayerGroupExternalResolution],
    consumed: &mut usize,
    outcome: &mut PlayerGroupGrowthOutcome,
) -> Result<Option<ClearPlayerGroupReceipt>, PlayerGroupGrowthError> {
    let mut receipt = ClearPlayerGroupReceipt::default();
    for &(x, y) in group.tiles.items.clone().iter() {
        if world.wdata(x, y).flags & (wflag::ROCKS | wflag::FOREST) == 0 {
            return Err(PlayerGroupGrowthError::UnsupportedClearTile {
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
    receipt.completed = true;
    Ok(Some(receipt))
}

fn place_oil_deposits(
    group: &TerrainGroup,
    world: &mut World,
    requested: i32,
    externals: &[PlayerGroupExternalResolution],
    consumed: &mut usize,
    outcome: &mut PlayerGroupGrowthOutcome,
) -> Result<Option<PlayerOilDepositsReceipt>, PlayerGroupGrowthError> {
    let mut receipt = PlayerOilDepositsReceipt {
        requested,
        capped_requested: requested.min(group.tiles.items.len() as i32),
        ..PlayerOilDepositsReceipt::default()
    };
    if requested == 0 {
        receipt.completed = true;
        return Ok(Some(receipt));
    }
    for &(x, y) in &group.tiles.items {
        if world.wdata(x, y).flags & wflag::OIL != 0 {
            continue;
        }
        let qualifies = (0..8).any(|index| {
            let nx = x + NEIGHBOUR_DX[index];
            let ny = y + NEIGHBOUR_DY[index];
            world.valid_w(nx, ny)
                && world.wdata(nx, ny).flags & (wflag::ROCKS | wflag::FOREST) == 0
                && !has_mountain_tcoords(world, nx, ny)
        });
        if !qualifies {
            continue;
        }
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
    receipt.completed = receipt.placed == receipt.capped_requested;
    Ok(Some(receipt))
}

fn oil_request(x: i32, y: i32, enabled: bool) -> PlayerGroupExternalRequest {
    PlayerGroupExternalRequest::OilGoodMutation {
        world_x: x,
        world_y: y,
        enabled,
        good_type: OIL_GOOD_TYPE,
        coord_x: x.wrapping_mul(0x300).wrapping_add(0x180),
        coord_y: y.wrapping_mul(0x300).wrapping_add(0x180),
    }
}

fn consume_oil_external(
    request: PlayerGroupExternalRequest,
    externals: &[PlayerGroupExternalResolution],
    consumed: &mut usize,
    outcome: &mut PlayerGroupGrowthOutcome,
) -> Result<Option<()>, PlayerGroupGrowthError> {
    let Some(actual) = externals.get(*consumed).copied() else {
        *outcome = PlayerGroupGrowthOutcome::ExternalResolutionRequired { request };
        return Ok(None);
    };
    let actual_request = match actual {
        PlayerGroupExternalResolution::OilGoodsApplied { request } => request,
        _ => {
            return Err(PlayerGroupGrowthError::ExternalResolutionKindMismatch { request });
        }
    };
    if actual_request != request {
        return Err(PlayerGroupGrowthError::ExternalResolutionMismatch {
            expected: request,
            actual: actual_request,
        });
    }
    *consumed += 1;
    Ok(Some(()))
}
