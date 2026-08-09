//! Exact entry/search transaction of `TerrainGroup::place_player_group`.
//!
//! The shipped PDB places the 7,202-byte function at `0x006a4190`.  This module
//! closes its deterministic start-ring scan through each first type-specific
//! effect: mountain and cliff calls are typed host receipts, while the common
//! type-4/type-6 `drop_tile` call is executed locally.  A successful common
//! drop reaches the still-downstream orthogonal growth kernel at `0x006a4cf0`.

use super::ammo::vector_dist;
use super::borders_fog::{CircleTable, CIRCLE_MAX_R};
use super::map_terrain::{wflag, World};
use super::terrain_drop_tile::{DropTileError, DropTileReceipt};
use super::terrain_groups::TerrainGroup;
use super::terrain_player_growth::{
    PlayerGroupGrowthError, PlayerGroupGrowthOutcome, PlayerGroupGrowthReceipt,
};
use super::terrain_region_placement::RegionDropTileInvocation;
use crate::rng::Random;

const TYPE_FOUR_STRICT_RADIUS: i32 = 4;
const TYPE_FOUR_IMPASSABLE_RADIUS: i32 = 1;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlacePlayerGroupCall {
    pub target_tiles: i32,
    pub player_index: usize,
    pub land_subtype: i32,
    pub oil_deposits: i32,
    pub group_index: usize,
    /// Pattern-0 type 4 makes one strict forest-free attempt, then retries with
    /// this flag clear if that call returns zero.
    pub strict_type_four: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerGroupExternalRequest {
    MountainsAddMountain {
        template: i32,
        world_x: i32,
        world_y: i32,
        pattern: i32,
        mountain_space: i32,
        forest_space: i32,
        rock_space: i32,
        coast_space: i32,
        start_min: i32,
    },
    CliffsVerifyDefensivePosition {
        world_x: i32,
        world_y: i32,
        start_x: i32,
        start_y: i32,
        cliff_face: i32,
    },
    CliffsPositionCliff {
        world_x: i32,
        world_y: i32,
        cliff_type: i32,
        start_x: i32,
        start_y: i32,
        defensive: bool,
        cliff_face: i32,
        anchor_y: i32,
    },
    /// `clear_group` and the type-6 oil tail call `World::set_oil_at`, whose
    /// Good-object close/create effects are owned by the object system.
    OilGoodMutation {
        world_x: i32,
        world_y: i32,
        enabled: bool,
        good_type: i32,
        coord_x: i32,
        coord_y: i32,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerGroupExternalResolution {
    Mountains {
        request: PlayerGroupExternalRequest,
        liberr: i32,
    },
    CliffsVerify {
        request: PlayerGroupExternalRequest,
        return_value: i32,
    },
    CliffsPosition {
        request: PlayerGroupExternalRequest,
        return_value: i32,
    },
    OilGoodsApplied {
        request: PlayerGroupExternalRequest,
    },
}

impl PlayerGroupExternalResolution {
    fn request(self) -> PlayerGroupExternalRequest {
        match self {
            Self::Mountains { request, .. }
            | Self::CliffsVerify { request, .. }
            | Self::CliffsPosition { request, .. }
            | Self::OilGoodsApplied { request } => request,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayerCandidateRejection {
    Region,
    CloserToOtherPlayer { player: usize, distance: i32 },
    ForbiddenWorldFlags { flags: u16 },
    BlockedWorldCell,
    CenterMinimum { distance: i32 },
    CenterMaximum { distance: i32 },
    TypeFourForestRing { offset_index: usize },
    EdgeMinimum { distance: i32 },
    EdgeMaximum { distance: i32 },
    HardBoundary,
    CornerMinimum { distance: i32 },
    CornerMaximum { distance: i32 },
    TypeFourImpassableRing { offset_index: usize },
    DropTileRejected,
    MountainRejected { liberr: i32 },
    CliffVerificationRejected,
    CliffPositionRejected,
    TypeFourFormationRejected { offset_index: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerCandidateAttempt {
    pub circle_index: usize,
    pub world_x: i32,
    pub world_y: i32,
    pub rejection: Option<PlayerCandidateRejection>,
    pub first_drop: Option<DropTileReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlacePlayerGroupOutcome {
    Returned(i32),
    ExternalResolutionRequired {
        request: PlayerGroupExternalRequest,
    },
    /// The first type-4/type-6 drop succeeded. Retail next enters the
    /// randomized orthogonal growth block at `0x006a4cf0`.
    GrowthKernel {
        invocation: RegionDropTileInvocation,
        first_drop: DropTileReceipt,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacePlayerGroupReceipt {
    pub call: PlacePlayerGroupCall,
    pub start_min: i32,
    pub start_max: i32,
    pub selected_circle_index: Option<usize>,
    pub first_circle_index: Option<usize>,
    pub circle_selection_draws: u32,
    pub attempts: Vec<PlayerCandidateAttempt>,
    pub formation_x_after: Vec<i32>,
    pub formation_y_after: Vec<i32>,
    pub external_resolutions_consumed: usize,
    /// Filled by the composed caller after a first local type-4/type-6 drop
    /// enters the exact `0x006a4cf0` continuation.
    pub growth: Option<PlayerGroupGrowthReceipt>,
    pub outcome: PlacePlayerGroupOutcome,
    pub rng_state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlacePlayerGroupError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
        tile_xs: i32,
        tile_ys: i32,
        tile_size: i32,
        tdata_len: usize,
    },
    StartArrayLengthMismatch {
        x: usize,
        y: usize,
    },
    InvalidPlayer {
        player_index: usize,
        players: usize,
    },
    InvalidStartRange {
        start_min: i32,
        start_max: i32,
    },
    UnsupportedGroupType {
        group_type: i32,
    },
    FormationLengthMismatch {
        x: usize,
        y: usize,
    },
    InvalidDropTile(DropTileError),
    InvalidGrowth(PlayerGroupGrowthError),
    ExternalResolutionMismatch {
        expected: PlayerGroupExternalRequest,
        actual: PlayerGroupExternalRequest,
    },
    ExternalResolutionKindMismatch {
        expected: PlayerGroupExternalRequest,
    },
}

impl TerrainGroup {
    /// Execute the locally closed player-group path, including the exact
    /// orthogonal growth continuation after a successful first forest/rock
    /// drop. Object-system effects remain ordered typed receipts.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_place_player_group(
        &mut self,
        world: &mut World,
        random: &mut Random,
        call: PlacePlayerGroupCall,
        formation_x: &mut Vec<i32>,
        formation_y: &mut Vec<i32>,
        externals: &[PlayerGroupExternalResolution],
    ) -> Result<PlacePlayerGroupReceipt, PlacePlayerGroupError> {
        let mut receipt = self.apply_place_player_group_prefix(
            world,
            random,
            call,
            formation_x,
            formation_y,
            externals,
        )?;
        if matches!(
            receipt.outcome,
            PlacePlayerGroupOutcome::GrowthKernel { .. }
        ) {
            let consumed = receipt.external_resolutions_consumed;
            let growth = self
                .apply_player_group_growth(
                    world,
                    random,
                    call,
                    formation_x,
                    formation_y,
                    &externals[consumed..],
                )
                .map_err(PlacePlayerGroupError::InvalidGrowth)?;
            receipt.external_resolutions_consumed += growth.external_resolutions_consumed;
            receipt.outcome = match growth.outcome {
                PlayerGroupGrowthOutcome::Returned(value) => {
                    PlacePlayerGroupOutcome::Returned(value)
                }
                PlayerGroupGrowthOutcome::ExternalResolutionRequired { request } => {
                    PlacePlayerGroupOutcome::ExternalResolutionRequired { request }
                }
            };
            receipt.growth = Some(growth);
            finish_receipt(&mut receipt, formation_x, formation_y, random);
        }
        Ok(receipt)
    }

    /// Execute `place_player_group` from entry through its first unresolved
    /// post-drop growth dependency. `formation_x/y` are the two group-local
    /// arrays passed by pattern 0 and retained across player calls.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_place_player_group_prefix(
        &mut self,
        world: &mut World,
        random: &mut Random,
        call: PlacePlayerGroupCall,
        formation_x: &mut Vec<i32>,
        formation_y: &mut Vec<i32>,
        externals: &[PlayerGroupExternalResolution],
    ) -> Result<PlacePlayerGroupReceipt, PlacePlayerGroupError> {
        validate_inputs(self, world, call, formation_x, formation_y)?;

        // Native clears TerrainGroup::tiles.length before clamping start_max.
        self.tiles.items.clear();
        if self.start_max == 0 {
            self.start_max = CIRCLE_MAX_R as i32;
        }
        self.start_max = self.start_max.min(CIRCLE_MAX_R as i32);

        let mut receipt = PlacePlayerGroupReceipt {
            call,
            start_min: self.start_min,
            start_max: self.start_max,
            selected_circle_index: None,
            first_circle_index: None,
            circle_selection_draws: 0,
            attempts: Vec::new(),
            formation_x_after: formation_x.clone(),
            formation_y_after: formation_y.clone(),
            external_resolutions_consumed: 0,
            growth: None,
            outcome: PlacePlayerGroupOutcome::Returned(0),
            rng_state_after: random.state(),
        };
        if self.start_max < self.start_min {
            return Ok(receipt);
        }

        let circles = CircleTable::build();
        let ring_start = circles.radius[self.start_min as usize] as usize;
        let ring_end = circles.radius[self.start_max as usize] as usize;
        // Equal radii leave no annulus. Shipped terrain data does not exercise
        // native's out-of-range equal-bound bug, so keep the reconstruction
        // fail-closed on that unsupported domain.
        if ring_end <= ring_start {
            return Err(PlacePlayerGroupError::InvalidStartRange {
                start_min: self.start_min,
                start_max: self.start_max,
            });
        }

        let selected = if ring_start < ring_end - 1 {
            receipt.circle_selection_draws = 1;
            let draw = random.get(0, 0xffff) as usize;
            ring_start + draw % (ring_end - ring_start)
        } else {
            ring_start
        };
        receipt.selected_circle_index = Some(selected);
        let initial_cursor = selected + 1;
        receipt.first_circle_index = Some(if initial_cursor == ring_end {
            ring_start
        } else {
            initial_cursor
        });

        let start_x = world.start_x.items[call.player_index];
        let start_y = world.start_y.items[call.player_index];
        let start_region = world.wdata(start_x, start_y).region;
        let land_subtype = if self.group_type == 8 {
            call.land_subtype.wrapping_sub(1)
        } else {
            call.land_subtype
        };
        let mut cursor = initial_cursor;

        loop {
            if cursor == ring_end {
                cursor = ring_start;
            }
            let world_x = start_x.wrapping_add(i32::from(circles.x[cursor]));
            let world_y = start_y.wrapping_add(i32::from(circles.y[cursor]));
            let mut attempt = PlayerCandidateAttempt {
                circle_index: cursor,
                world_x,
                world_y,
                rejection: None,
                first_drop: None,
            };

            if !world.valid_w(world_x, world_y) {
                attempt.rejection = Some(PlayerCandidateRejection::HardBoundary);
            } else {
                attempt.rejection =
                    reject_player_candidate(self, world, call, world_x, world_y, start_region);
            }

            if attempt.rejection.is_none() {
                match self.group_type {
                    5 => {
                        let request = PlayerGroupExternalRequest::MountainsAddMountain {
                            template: land_subtype,
                            world_x,
                            world_y,
                            pattern: 5,
                            mountain_space: self.mount_space,
                            forest_space: self.forest_space,
                            rock_space: self.rock_space,
                            coast_space: self.coast_space,
                            start_min: self.start_min,
                        };
                        let Some(external) = externals.get(receipt.external_resolutions_consumed)
                        else {
                            receipt.attempts.push(attempt);
                            receipt.outcome =
                                PlacePlayerGroupOutcome::ExternalResolutionRequired { request };
                            finish_receipt(&mut receipt, formation_x, formation_y, random);
                            return Ok(receipt);
                        };
                        ensure_request(request, external.request())?;
                        let PlayerGroupExternalResolution::Mountains { liberr, .. } = *external
                        else {
                            return Err(PlacePlayerGroupError::ExternalResolutionKindMismatch {
                                expected: request,
                            });
                        };
                        receipt.external_resolutions_consumed += 1;
                        if liberr == 0 {
                            receipt.attempts.push(attempt);
                            receipt.outcome = PlacePlayerGroupOutcome::Returned(1);
                            finish_receipt(&mut receipt, formation_x, formation_y, random);
                            return Ok(receipt);
                        }
                        attempt.rejection =
                            Some(PlayerCandidateRejection::MountainRejected { liberr });
                    }
                    8 => {
                        if land_subtype != 4 {
                            let request =
                                PlayerGroupExternalRequest::CliffsVerifyDefensivePosition {
                                    world_x,
                                    world_y,
                                    start_x,
                                    start_y,
                                    cliff_face: self.cliff_face,
                                };
                            let Some(external) =
                                externals.get(receipt.external_resolutions_consumed)
                            else {
                                receipt.attempts.push(attempt);
                                receipt.outcome =
                                    PlacePlayerGroupOutcome::ExternalResolutionRequired { request };
                                finish_receipt(&mut receipt, formation_x, formation_y, random);
                                return Ok(receipt);
                            };
                            ensure_request(request, external.request())?;
                            let PlayerGroupExternalResolution::CliffsVerify {
                                return_value, ..
                            } = *external
                            else {
                                return Err(
                                    PlacePlayerGroupError::ExternalResolutionKindMismatch {
                                        expected: request,
                                    },
                                );
                            };
                            receipt.external_resolutions_consumed += 1;
                            if return_value == 0 {
                                attempt.rejection =
                                    Some(PlayerCandidateRejection::CliffVerificationRejected);
                            }
                        }
                        if attempt.rejection.is_none() {
                            let request = PlayerGroupExternalRequest::CliffsPositionCliff {
                                world_x,
                                world_y,
                                cliff_type: land_subtype,
                                start_x,
                                start_y,
                                defensive: land_subtype == 4,
                                cliff_face: self.cliff_face,
                                anchor_y: start_y,
                            };
                            let Some(external) =
                                externals.get(receipt.external_resolutions_consumed)
                            else {
                                receipt.attempts.push(attempt);
                                receipt.outcome =
                                    PlacePlayerGroupOutcome::ExternalResolutionRequired { request };
                                finish_receipt(&mut receipt, formation_x, formation_y, random);
                                return Ok(receipt);
                            };
                            ensure_request(request, external.request())?;
                            let PlayerGroupExternalResolution::CliffsPosition {
                                return_value, ..
                            } = *external
                            else {
                                return Err(
                                    PlacePlayerGroupError::ExternalResolutionKindMismatch {
                                        expected: request,
                                    },
                                );
                            };
                            receipt.external_resolutions_consumed += 1;
                            if return_value != 0 {
                                receipt.attempts.push(attempt);
                                receipt.outcome = PlacePlayerGroupOutcome::Returned(1);
                                finish_receipt(&mut receipt, formation_x, formation_y, random);
                                return Ok(receipt);
                            }
                            attempt.rejection =
                                Some(PlayerCandidateRejection::CliffPositionRejected);
                        }
                    }
                    4 if !formation_x.is_empty() => {
                        let rotation = (random.get(0, 0xffff) & 3) as usize;
                        const SIGN_X: [i32; 4] = [-1, 1, 1, -1];
                        const SIGN_Y: [i32; 4] = [-1, -1, 1, 1];
                        let mut transformed = Vec::with_capacity(formation_x.len());
                        for (offset_index, (&offset_x, &offset_y)) in
                            formation_x.iter().zip(formation_y.iter()).enumerate()
                        {
                            let x = world_x.wrapping_add(SIGN_X[rotation] * offset_x);
                            let y = world_y.wrapping_add(SIGN_Y[rotation] * offset_y);
                            let available = self
                                .available_rough_tile(world, x, y, wflag::FOREST)
                                .map_err(PlacePlayerGroupError::InvalidDropTile)?;
                            if !available.accepted {
                                attempt.rejection =
                                    Some(PlayerCandidateRejection::TypeFourFormationRejected {
                                        offset_index,
                                    });
                                break;
                            }
                            transformed.push((x, y));
                        }
                        if attempt.rejection.is_none() {
                            for (x, y) in transformed {
                                let invocation = RegionDropTileInvocation {
                                    world_x: x,
                                    world_y: y,
                                    group_type: 4,
                                    group_radius: call.target_tiles,
                                    land_subtype,
                                    target_tiles: call.target_tiles,
                                    oil_deposits: call.oil_deposits,
                                    group_index: call.group_index,
                                };
                                self.apply_drop_tile(world, random, invocation, None)
                                    .map_err(PlacePlayerGroupError::InvalidDropTile)?;
                            }
                            receipt.attempts.push(attempt);
                            receipt.outcome = PlacePlayerGroupOutcome::Returned(1);
                            finish_receipt(&mut receipt, formation_x, formation_y, random);
                            return Ok(receipt);
                        }
                    }
                    4 | 6 => {
                        let invocation = RegionDropTileInvocation {
                            world_x,
                            world_y,
                            group_type: self.group_type,
                            group_radius: call
                                .target_tiles
                                .wrapping_add((call.target_tiles >> 31) & 3)
                                .wrapping_shr(2)
                                .wrapping_add(1),
                            land_subtype,
                            target_tiles: call.target_tiles,
                            oil_deposits: call.oil_deposits,
                            group_index: call.group_index,
                        };
                        let drop = self
                            .apply_drop_tile(world, random, invocation, None)
                            .map_err(PlacePlayerGroupError::InvalidDropTile)?;
                        attempt.first_drop = Some(drop.clone());
                        if drop.placed {
                            if self.group_type == 4 {
                                formation_x.push(0);
                                formation_y.push(0);
                            }
                            receipt.attempts.push(attempt);
                            receipt.outcome = PlacePlayerGroupOutcome::GrowthKernel {
                                invocation,
                                first_drop: drop,
                            };
                            finish_receipt(&mut receipt, formation_x, formation_y, random);
                            return Ok(receipt);
                        }
                        attempt.rejection = Some(PlayerCandidateRejection::DropTileRejected);
                    }
                    _ => unreachable!("validated group type"),
                }
            }

            receipt.attempts.push(attempt);
            cursor += 1;
            if cursor == initial_cursor {
                break;
            }
        }

        finish_receipt(&mut receipt, formation_x, formation_y, random);
        Ok(receipt)
    }
}

fn finish_receipt(
    receipt: &mut PlacePlayerGroupReceipt,
    formation_x: &[i32],
    formation_y: &[i32],
    random: &Random,
) {
    receipt.formation_x_after = formation_x.to_vec();
    receipt.formation_y_after = formation_y.to_vec();
    receipt.rng_state_after = random.state();
}

fn ensure_request(
    expected: PlayerGroupExternalRequest,
    actual: PlayerGroupExternalRequest,
) -> Result<(), PlacePlayerGroupError> {
    if expected == actual {
        Ok(())
    } else {
        Err(PlacePlayerGroupError::ExternalResolutionMismatch { expected, actual })
    }
}

fn validate_inputs(
    group: &TerrainGroup,
    world: &World,
    call: PlacePlayerGroupCall,
    formation_x: &[i32],
    formation_y: &[i32],
) -> Result<(), PlacePlayerGroupError> {
    let world_size = i64::from(world.xs) * i64::from(world.ys);
    let tile_size = i64::from(world.tile_xs) * i64::from(world.tile_ys);
    if world.xs <= 0
        || world.ys <= 0
        || world.size != world.xs.wrapping_mul(world.ys)
        || world_size != world.wdata.len() as i64
        || world.tile_xs != world.xs.wrapping_mul(4)
        || world.tile_ys != world.ys.wrapping_mul(4)
        || world.tile_size != world.tile_xs.wrapping_mul(world.tile_ys)
        || tile_size != world.tdata.len() as i64
    {
        return Err(PlacePlayerGroupError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
            tile_size: world.tile_size,
            tdata_len: world.tdata.len(),
        });
    }
    if world.start_x.items.len() != world.start_y.items.len() {
        return Err(PlacePlayerGroupError::StartArrayLengthMismatch {
            x: world.start_x.items.len(),
            y: world.start_y.items.len(),
        });
    }
    if call.player_index >= world.start_x.items.len() {
        return Err(PlacePlayerGroupError::InvalidPlayer {
            player_index: call.player_index,
            players: world.start_x.items.len(),
        });
    }
    for player in 0..world.start_x.items.len() {
        let x = world.start_x.items[player];
        let y = world.start_y.items[player];
        if !world.valid_w(x, y) {
            return Err(PlacePlayerGroupError::InvalidPlayer {
                player_index: player,
                players: world.start_x.items.len(),
            });
        }
    }
    let start_max = if group.start_max == 0 {
        CIRCLE_MAX_R as i32
    } else {
        group.start_max.min(CIRCLE_MAX_R as i32)
    };
    if !(0..=CIRCLE_MAX_R as i32).contains(&group.start_min) {
        return Err(PlacePlayerGroupError::InvalidStartRange {
            start_min: group.start_min,
            start_max,
        });
    }
    if !matches!(group.group_type, 4 | 5 | 6 | 8) {
        return Err(PlacePlayerGroupError::UnsupportedGroupType {
            group_type: group.group_type,
        });
    }
    if formation_x.len() != formation_y.len() {
        return Err(PlacePlayerGroupError::FormationLengthMismatch {
            x: formation_x.len(),
            y: formation_y.len(),
        });
    }
    Ok(())
}

fn reject_player_candidate(
    group: &TerrainGroup,
    world: &World,
    call: PlacePlayerGroupCall,
    world_x: i32,
    world_y: i32,
    start_region: i16,
) -> Option<PlayerCandidateRejection> {
    let cell = world.wdata(world_x, world_y);
    if cell.region < 64 && cell.region != start_region {
        return Some(PlayerCandidateRejection::Region);
    }

    let own_distance = vector_dist(
        world_x.wrapping_sub(world.start_x.items[call.player_index]),
        world_y.wrapping_sub(world.start_y.items[call.player_index]),
    );
    for player in 0..world.start_x.items.len() {
        if player == call.player_index {
            continue;
        }
        let distance = vector_dist(
            world_x.wrapping_sub(world.start_x.items[player]),
            world_y.wrapping_sub(world.start_y.items[player]),
        );
        if distance < own_distance {
            return Some(PlayerCandidateRejection::CloserToOtherPlayer { player, distance });
        }
    }

    let forbidden = wflag::NO_BUILD_MASK | wflag::OIL | wflag::HAS_RIVER;
    if cell.flags & forbidden != 0 {
        return Some(PlayerCandidateRejection::ForbiddenWorldFlags { flags: cell.flags });
    }
    if cell.blocked != 0 {
        return Some(PlayerCandidateRejection::BlockedWorldCell);
    }

    let centre_x = if world.xs / 2 < world_x {
        world.xs / 2 + 1
    } else {
        world.xs / 2
    };
    let centre_y = if world.ys / 2 < world_y {
        world.ys / 2 + 1
    } else {
        world.ys / 2
    };
    let centre_distance = vector_dist(world_x - centre_x, world_y - centre_y);
    if group.cent_min > 0 && centre_distance < group.cent_min {
        return Some(PlayerCandidateRejection::CenterMinimum {
            distance: centre_distance,
        });
    }
    if group.cent_max > 0 && centre_distance > group.cent_max {
        return Some(PlayerCandidateRejection::CenterMaximum {
            distance: centre_distance,
        });
    }

    if group.group_type == 4 && call.strict_type_four {
        for (offset_index, (dx, dy)) in square_spiral(TYPE_FOUR_STRICT_RADIUS)
            .into_iter()
            .enumerate()
        {
            let x = world_x + dx;
            let y = world_y + dy;
            if world.valid_w(x, y) && world.wdata(x, y).flags & wflag::FOREST != 0 {
                return Some(PlayerCandidateRejection::TypeFourForestRing { offset_index });
            }
        }
    }

    let edge_distance = world_x
        .min(world_y)
        .min(world.xs - world_x)
        .min(world.ys - world_y);
    if group.edge_min > 0 && edge_distance < group.edge_min {
        return Some(PlayerCandidateRejection::EdgeMinimum {
            distance: edge_distance,
        });
    }
    if group.edge_max > 0 && edge_distance > group.edge_max {
        return Some(PlayerCandidateRejection::EdgeMaximum {
            distance: edge_distance,
        });
    }
    // Retail's final comparison uses xs for Y too.
    if world_x == 0 || world_y == 0 || world_x == world.xs - 1 || world_y == world.xs - 1 {
        return Some(PlayerCandidateRejection::HardBoundary);
    }

    let mut corner_distance = 99_999;
    for (corner_x, corner_y) in [(0, 0), (0, world.ys), (world.xs, 0), (world.xs, world.ys)] {
        corner_distance = corner_distance.min(vector_dist(world_x - corner_x, world_y - corner_y));
    }
    if group.corner_min > 0 && corner_distance < group.corner_min {
        return Some(PlayerCandidateRejection::CornerMinimum {
            distance: corner_distance,
        });
    }
    if group.corner_max > 0 && corner_distance > group.corner_max {
        return Some(PlayerCandidateRejection::CornerMaximum {
            distance: corner_distance,
        });
    }

    if group.group_type == 4 {
        for (offset_index, (dx, dy)) in square_spiral(TYPE_FOUR_IMPASSABLE_RADIUS)
            .into_iter()
            .enumerate()
        {
            let x = world_x + dx;
            let y = world_y + dy;
            if world.valid_w(x, y) && world.wdata(x, y).flags & wflag::IMPASSABLE_X != 0 {
                return Some(PlayerCandidateRejection::TypeFourImpassableRing { offset_index });
            }
        }
    }
    None
}

/// `move_x/move_y` `0x00adcaf0/0x00adc400`: centre, then each square
/// perimeter clockwise from its north-west corner. The first 81 shipped pairs
/// are exactly radii 0 through 4.
fn square_spiral(max_radius: i32) -> Vec<(i32, i32)> {
    let mut offsets = vec![(0, 0)];
    for radius in 1..=max_radius {
        for x in -radius..=radius {
            offsets.push((x, -radius));
        }
        for y in (-radius + 1)..=radius {
            offsets.push((radius, y));
        }
        for x in (-radius..=(radius - 1)).rev() {
            offsets.push((x, radius));
        }
        for y in ((-radius + 1)..=(radius - 1)).rev() {
            offsets.push((-radius, y));
        }
    }
    offsets
}
