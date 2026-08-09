//! Complete `TerrainGroup::drop_tile` branch dispatcher (`0x006a2ab0`).
//!
//! Forest and rock branches are executed locally, including
//! `TerrainGroup::available_rough_tile` (`0x006a24d0`), the exact WData class
//! write, and the forest branch's sixteen `World::set_blocked_at` calls. The
//! Mountains, Cliffs, and Good-object owners are represented by exact typed
//! requests plus resolved receipts; no behavior from those subsystems is
//! invented here.

use super::ammo::vector_dist;
use super::borders_fog::CircleTable;
use super::map_terrain::{land, tflag, wflag, WCoord, World, NEIGHBOUR_DX, NEIGHBOUR_DY};
use super::terrain_groups::TerrainGroup;
use super::terrain_region_placement::RegionDropTileInvocation;
use crate::rng::Random;

const OIL_GOOD_TYPE: i32 = 5;
const MAX_CIRCLE_RADIUS: i32 = 64;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DropTileExternalRequest {
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
    /// `World::set_oil_at` first closes every live oil Good at this WCoord.  An
    /// enabled mutation then sets WData::OIL and adds Good type 5 at the cell
    /// center; a disabled mutation clears the flag and does not use the center.
    OilGoodMutation {
        world_x: i32,
        world_y: i32,
        enabled: bool,
        good_type: i32,
        coord_x: i32,
        coord_y: i32,
    },
    CliffsPositionCliff {
        world_x: i32,
        world_y: i32,
        cliff_type: i32,
        target_x: i32,
        target_y: i32,
        mode: i32,
        facing: i32,
        anchor_y: i32,
    },
}

/// Receipt supplied by the subsystem which owns the external mutation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DropTileExternalResolution {
    /// `Liberr == 0` is success. `Mountains::add_mountain` consumes no RNG.
    Mountains {
        request: DropTileExternalRequest,
        liberr: i32,
    },
    /// `World::set_oil_at` is void and consumes no RNG.
    OilGoodsApplied { request: DropTileExternalRequest },
    /// `Cliffs::position_cliff` returns zero/nonzero. It consumes either zero or
    /// one map-RNG draw depending on the number of eligible cliff templates.
    Cliffs {
        request: DropTileExternalRequest,
        return_value: i32,
        rng_draws: u32,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AvailableRoughTileRejection {
    OutOfBounds,
    FullWater,
    ExistingRocksOrForest,
    Edge,
    Mountain,
    StartCity,
    CoastOrImpassable,
    MountainSpacing { offset_index: usize },
    ForestSpacing { offset_index: usize },
    RockSpacing { offset_index: usize },
    CoastSpacing { offset_index: usize },
    RockTouchesMountainTiles { neighbor_index: usize },
    RockTouchesOtherGroupRocks { neighbor_index: usize },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AvailableRoughTileReceipt {
    pub accepted: bool,
    pub rejection: Option<AvailableRoughTileRejection>,
    pub circle_queries: u32,
    pub mountain_tcoord_queries: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DropTileBranch {
    Forest,
    Mountain {
        request: DropTileExternalRequest,
        liberr: i32,
    },
    Rocks,
    Oil {
        request: DropTileExternalRequest,
    },
    Cliff {
        request: DropTileExternalRequest,
        return_value: i32,
    },
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropTileReceipt {
    pub invocation: RegionDropTileInvocation,
    pub branch: DropTileBranch,
    pub available_rough_tile: Option<AvailableRoughTileReceipt>,
    pub placed: bool,
    pub tiles_len_before: usize,
    pub tiles_len_after: usize,
    pub tiles_capacity_before: i32,
    pub tiles_capacity_after: i32,
    pub changed_wdata_indices: Vec<usize>,
    pub changed_tdata_indices: Vec<usize>,
    pub rng_draws: u32,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DropTileError {
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        size: i32,
        wdata_len: usize,
        start_city_locs_len: usize,
        tile_xs: i32,
        tile_ys: i32,
        tile_size: i32,
        tdata_len: usize,
    },
    InvocationTypeMismatch {
        group_type: i32,
        invocation_type: i32,
    },
    CoordinateOutOfBounds {
        world_x: i32,
        world_y: i32,
    },
    InvalidSpacingRadius {
        value: i32,
    },
    InvalidTileListCapacity {
        length: usize,
        capacity: i32,
        increment: i16,
    },
    ExternalResolutionRequired {
        request: DropTileExternalRequest,
    },
    ExternalResolutionMismatch {
        expected: DropTileExternalRequest,
        actual: DropTileExternalRequest,
    },
    InvalidExternalRngDraws {
        group_type: i32,
        draws: u32,
    },
}

impl TerrainGroup {
    /// Exact external request selected by `drop_tile`, if this branch belongs to
    /// another gameplay subsystem.
    pub fn drop_tile_external_request(
        &self,
        invocation: RegionDropTileInvocation,
    ) -> Option<DropTileExternalRequest> {
        match invocation.group_type {
            5 => Some(DropTileExternalRequest::MountainsAddMountain {
                template: invocation.land_subtype,
                world_x: invocation.world_x,
                world_y: invocation.world_y,
                pattern: 4,
                mountain_space: self.mount_space,
                forest_space: self.forest_space,
                rock_space: self.rock_space,
                coast_space: self.coast_space,
                start_min: self.start_min,
            }),
            7 => Some(DropTileExternalRequest::OilGoodMutation {
                world_x: invocation.world_x,
                world_y: invocation.world_y,
                enabled: true,
                good_type: OIL_GOOD_TYPE,
                coord_x: invocation.world_x.wrapping_mul(0x300).wrapping_add(0x180),
                coord_y: invocation.world_y.wrapping_mul(0x300).wrapping_add(0x180),
            }),
            8 => Some(DropTileExternalRequest::CliffsPositionCliff {
                world_x: invocation.world_x,
                world_y: invocation.world_y,
                cliff_type: invocation.land_subtype.wrapping_sub(1),
                target_x: -1,
                target_y: -1,
                mode: 0,
                facing: 0,
                anchor_y: invocation.world_y,
            }),
            _ => None,
        }
    }

    /// Complete dispatcher/body of `TerrainGroup::drop_tile` (`0x006a2ab0`).
    ///
    /// The method mutates the supplied preview state. Callers which have not yet
    /// recovered the enclosing continuation should invoke it on clones, as
    /// `TerrainGroups::place_all` does.
    pub fn apply_drop_tile(
        &mut self,
        world: &mut World,
        random: &mut Random,
        invocation: RegionDropTileInvocation,
        external: Option<DropTileExternalResolution>,
    ) -> Result<DropTileReceipt, DropTileError> {
        validate_drop_inputs(self, world, invocation)?;
        validate_tiles_push(self)?;

        let world_before = world.clone();
        let tiles_len_before = self.tiles.items.len();
        let tiles_capacity_before = self.tiles.capacity;
        let mut available_rough_tile = None;
        let mut rng_draws = 0;

        let (branch, placed) = match invocation.group_type {
            4 => {
                let available = self.available_rough_tile(
                    world,
                    invocation.world_x,
                    invocation.world_y,
                    wflag::FOREST,
                )?;
                available_rough_tile = Some(available);
                let mut placed = available.accepted;
                if placed && !self.tiles.items.is_empty() {
                    let (first_x, first_y) = self.tiles.items[0];
                    let distance = vector_dist(
                        invocation.world_x.wrapping_sub(first_x),
                        invocation.world_y.wrapping_sub(first_y),
                    );
                    if invocation.group_radius < distance {
                        placed = false;
                    }
                }
                if placed {
                    set_rough_class(world, invocation.world_x, invocation.world_y, wflag::FOREST);
                    for index in 0..16 {
                        let tile_x = invocation.world_x * 4 + index % 4;
                        let tile_y = invocation.world_y * 4 + index / 4;
                        world.set_blocked_at(tile_x, tile_y, true);
                    }
                }
                (DropTileBranch::Forest, placed)
            }
            5 => {
                let request = self.drop_tile_external_request(invocation).unwrap();
                let Some(DropTileExternalResolution::Mountains {
                    request: actual,
                    liberr,
                }) = external
                else {
                    return Err(external_error(request, external));
                };
                ensure_request(request, actual)?;
                (DropTileBranch::Mountain { request, liberr }, liberr == 0)
            }
            6 => {
                let available = self.available_rough_tile(
                    world,
                    invocation.world_x,
                    invocation.world_y,
                    wflag::ROCKS,
                )?;
                available_rough_tile = Some(available);
                if available.accepted {
                    set_rough_class(world, invocation.world_x, invocation.world_y, wflag::ROCKS);
                }
                (DropTileBranch::Rocks, available.accepted)
            }
            7 => {
                let request = self.drop_tile_external_request(invocation).unwrap();
                let Some(DropTileExternalResolution::OilGoodsApplied { request: actual }) =
                    external
                else {
                    return Err(external_error(request, external));
                };
                ensure_request(request, actual)?;
                world.set_oil_at(invocation.world_x, invocation.world_y, true);
                (DropTileBranch::Oil { request }, true)
            }
            8 => {
                let request = self.drop_tile_external_request(invocation).unwrap();
                let Some(DropTileExternalResolution::Cliffs {
                    request: actual,
                    return_value,
                    rng_draws: draws,
                }) = external
                else {
                    return Err(external_error(request, external));
                };
                ensure_request(request, actual)?;
                if draws > 1 {
                    return Err(DropTileError::InvalidExternalRngDraws {
                        group_type: 8,
                        draws,
                    });
                }
                for _ in 0..draws {
                    random.advance();
                }
                rng_draws = draws;
                (
                    DropTileBranch::Cliff {
                        request,
                        return_value,
                    },
                    return_value != 0,
                )
            }
            _ => (DropTileBranch::Unsupported, false),
        };

        if placed {
            push_tile_retail(self, (invocation.world_x, invocation.world_y));
        }

        let changed_wdata_indices = world_before
            .wdata
            .iter()
            .zip(&world.wdata)
            .enumerate()
            .filter_map(|(index, (before, after))| (before != after).then_some(index))
            .collect();
        let changed_tdata_indices = world_before
            .tdata
            .iter()
            .zip(&world.tdata)
            .enumerate()
            .filter_map(|(index, (before, after))| (before != after).then_some(index))
            .collect();

        Ok(DropTileReceipt {
            invocation,
            branch,
            available_rough_tile,
            placed,
            tiles_len_before,
            tiles_len_after: self.tiles.items.len(),
            tiles_capacity_before,
            tiles_capacity_after: self.tiles.capacity,
            changed_wdata_indices,
            changed_tdata_indices,
            rng_draws,
            rng_state_after: random.state(),
        })
    }

    /// Complete `TerrainGroup::available_rough_tile` (`0x006a24d0`).
    pub fn available_rough_tile(
        &self,
        world: &World,
        world_x: i32,
        world_y: i32,
        requested_class: u16,
    ) -> Result<AvailableRoughTileReceipt, DropTileError> {
        validate_world_storage(world)?;
        for value in [
            self.mount_space,
            self.forest_space,
            self.rock_space,
            self.coast_space,
        ] {
            if !(0..=MAX_CIRCLE_RADIUS).contains(&value) {
                return Err(DropTileError::InvalidSpacingRadius { value });
            }
        }

        let mut receipt = AvailableRoughTileReceipt {
            accepted: false,
            rejection: None,
            circle_queries: 0,
            mountain_tcoord_queries: 0,
        };
        let reject = |receipt: &mut AvailableRoughTileReceipt, rejection| {
            receipt.rejection = Some(rejection);
            *receipt
        };
        if !world.valid_w(world_x, world_y) {
            return Ok(reject(
                &mut receipt,
                AvailableRoughTileRejection::OutOfBounds,
            ));
        }
        let cell = world.wdata(world_x, world_y);
        if cell.flags & wflag::WATERHALF == 0 && matches!(cell.land, land::COASTAL | land::OCEAN) {
            return Ok(reject(&mut receipt, AvailableRoughTileRejection::FullWater));
        }
        if cell.flags & (wflag::ROCKS | wflag::FOREST) != 0 {
            return Ok(reject(
                &mut receipt,
                AvailableRoughTileRejection::ExistingRocksOrForest,
            ));
        }
        if world.is_edge(world_x, world_y) {
            return Ok(reject(&mut receipt, AvailableRoughTileRejection::Edge));
        }
        if cell.flags & wflag::MOUNTAINS != 0 {
            return Ok(reject(&mut receipt, AvailableRoughTileRejection::Mountain));
        }
        if world.start_city_wcoord(WCoord(world_x), WCoord(world_y)) {
            return Ok(reject(&mut receipt, AvailableRoughTileRejection::StartCity));
        }
        if cell.flags & (wflag::COAST | wflag::IMPASSABLE_X) != 0 {
            return Ok(reject(
                &mut receipt,
                AvailableRoughTileRejection::CoastOrImpassable,
            ));
        }

        let circle = CircleTable::build();
        let begin = circle.radius[0] as usize;
        for (class, radius, kind) in [
            (wflag::MOUNTAINS, self.mount_space, 0u8),
            (wflag::FOREST, self.forest_space, 1u8),
            (wflag::ROCKS, self.rock_space, 2u8),
        ] {
            for offset_index in begin..circle.radius[radius as usize] as usize {
                receipt.circle_queries += 1;
                let nx = world_x + i32::from(circle.x[offset_index]);
                let ny = world_y + i32::from(circle.y[offset_index]);
                if world.valid_w(nx, ny)
                    && world.wdata(nx, ny).flags & class != 0
                    && !self.tiles.items.contains(&(nx, ny))
                {
                    let rejection = match kind {
                        0 => AvailableRoughTileRejection::MountainSpacing { offset_index },
                        1 => AvailableRoughTileRejection::ForestSpacing { offset_index },
                        _ => AvailableRoughTileRejection::RockSpacing { offset_index },
                    };
                    return Ok(reject(&mut receipt, rejection));
                }
            }
        }
        for offset_index in begin..circle.radius[self.coast_space as usize] as usize {
            receipt.circle_queries += 1;
            let nx = world_x + i32::from(circle.x[offset_index]);
            let ny = world_y + i32::from(circle.y[offset_index]);
            if world.valid_w(nx, ny) {
                let neighbor = world.wdata(nx, ny);
                if neighbor.flags & wflag::WATERHALF == 0
                    && matches!(neighbor.land, land::COASTAL | land::OCEAN)
                {
                    return Ok(reject(
                        &mut receipt,
                        AvailableRoughTileRejection::CoastSpacing { offset_index },
                    ));
                }
            }
        }

        if requested_class == wflag::ROCKS {
            for neighbor_index in 0..8 {
                let nx = world_x + NEIGHBOUR_DX[neighbor_index];
                let ny = world_y + NEIGHBOUR_DY[neighbor_index];
                if !world.valid_w(nx, ny) {
                    continue;
                }
                receipt.mountain_tcoord_queries += 1;
                if has_mountain_tcoords(world, nx, ny) {
                    return Ok(reject(
                        &mut receipt,
                        AvailableRoughTileRejection::RockTouchesMountainTiles { neighbor_index },
                    ));
                }
                if world.wdata(nx, ny).flags & wflag::ROCKS != 0
                    && !self.tiles.items.contains(&(nx, ny))
                {
                    return Ok(reject(
                        &mut receipt,
                        AvailableRoughTileRejection::RockTouchesOtherGroupRocks { neighbor_index },
                    ));
                }
            }
        }

        receipt.accepted = true;
        Ok(receipt)
    }
}

fn validate_drop_inputs(
    group: &TerrainGroup,
    world: &World,
    invocation: RegionDropTileInvocation,
) -> Result<(), DropTileError> {
    validate_world_storage(world)?;
    if group.group_type != invocation.group_type {
        return Err(DropTileError::InvocationTypeMismatch {
            group_type: group.group_type,
            invocation_type: invocation.group_type,
        });
    }
    if !world.valid_w(invocation.world_x, invocation.world_y) {
        return Err(DropTileError::CoordinateOutOfBounds {
            world_x: invocation.world_x,
            world_y: invocation.world_y,
        });
    }
    Ok(())
}

fn validate_world_storage(world: &World) -> Result<(), DropTileError> {
    let world_size = world.xs.checked_mul(world.ys);
    let start_city_locs_len = world_size
        .and_then(|size| size.checked_add(7))
        .and_then(|bits| usize::try_from(bits / 8).ok());
    let tile_xs = world.xs.checked_mul(4);
    let tile_ys = world.ys.checked_mul(4);
    let tile_size = world.tile_xs.checked_mul(world.tile_ys);
    if world.xs <= 0
        || world.ys <= 0
        || world_size != Some(world.size)
        || usize::try_from(world.size).ok() != Some(world.wdata.len())
        || start_city_locs_len.is_none_or(|len| world.start_city_locs.len() < len)
        || tile_xs != Some(world.tile_xs)
        || tile_ys != Some(world.tile_ys)
        || tile_size != Some(world.tile_size)
        || usize::try_from(world.tile_size).ok() != Some(world.tdata.len())
    {
        return Err(DropTileError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata_len: world.wdata.len(),
            start_city_locs_len: world.start_city_locs.len(),
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
            tile_size: world.tile_size,
            tdata_len: world.tdata.len(),
        });
    }
    Ok(())
}

fn validate_tiles_push(group: &TerrainGroup) -> Result<(), DropTileError> {
    let length = group.tiles.items.len();
    let capacity = group.tiles.capacity;
    let increase = if group.tiles.increment < 0 {
        capacity.max(4)
    } else {
        i32::from(group.tiles.increment)
    };
    let growing = capacity >= 0 && length >= capacity as usize;
    if capacity < 0
        || length > capacity as usize
        || growing && (increase <= 0 || capacity.checked_add(increase).is_none())
    {
        return Err(DropTileError::InvalidTileListCapacity {
            length,
            capacity,
            increment: group.tiles.increment,
        });
    }
    Ok(())
}

fn push_tile_retail(group: &mut TerrainGroup, coord: (i32, i32)) {
    if group.tiles.items.len() >= group.tiles.capacity as usize {
        let increase = if group.tiles.increment < 0 {
            group.tiles.capacity.max(4)
        } else {
            i32::from(group.tiles.increment)
        };
        // `validate_tiles_push` proved the native signed capacity addition is
        // representable before any branch mutation can occur.
        group.tiles.capacity += increase;
    }
    group.tiles.items.push(coord);
}

fn set_rough_class(world: &mut World, world_x: i32, world_y: i32, class: u16) {
    let cell = world.wdata(world_x, world_y);
    let classified_land = if cell.flags & wflag::COAST != 0 {
        land::OCEAN
    } else {
        cell.land
    };
    world.set_land(
        world_x,
        world_y,
        i32::from(classified_land),
        -1,
        i32::from(class),
        false,
    );
}

pub(crate) fn has_mountain_tcoords(world: &World, world_x: i32, world_y: i32) -> bool {
    let tile_x = world_x * 4;
    let tile_y = world_y * 4;
    for local_y in 0..4 {
        for local_x in 0..4 {
            if world.tmask(tile_x + local_x, tile_y + local_y) & tflag::BLOCKER_MASK
                == tflag::BLOCKER_MOUNTAIN
            {
                return true;
            }
        }
    }
    false
}

fn ensure_request(
    expected: DropTileExternalRequest,
    actual: DropTileExternalRequest,
) -> Result<(), DropTileError> {
    if expected == actual {
        Ok(())
    } else {
        Err(DropTileError::ExternalResolutionMismatch { expected, actual })
    }
}

fn external_error(
    expected: DropTileExternalRequest,
    actual: Option<DropTileExternalResolution>,
) -> DropTileError {
    let actual = match actual {
        Some(DropTileExternalResolution::Mountains { request, .. })
        | Some(DropTileExternalResolution::OilGoodsApplied { request })
        | Some(DropTileExternalResolution::Cliffs { request, .. }) => Some(request),
        None => None,
    };
    if let Some(actual) = actual {
        DropTileError::ExternalResolutionMismatch { expected, actual }
    } else {
        DropTileError::ExternalResolutionRequired { request: expected }
    }
}
