//! Bounded retail port of `BuildTypeData::calc_gather` (`0x00639E40`).
//!
//! The retail function is 7,668 bytes and contains unrelated flat, forest, mine,
//! university and oil paths. This module implements one complete, reachable path: a
//! shipped, completed Farm (type 417, flat, 4x4 fine-tile footprint) with zero or one
//! active worker, as called by `LeaderData::calc_city_resources`. Any other type-data
//! shape returns an error; it is never interpreted as a Farm by resource label or radius.
//!
//! Instruction-level anchors in `riseofnations.exe`:
//!
//! * `0x00639F99`: virtual `BuildTypeData::is_flat` gate;
//! * `0x00639FBF..0x0063A181`: x-major footprint walk, ownership/alliance gate,
//!   `LandData::{make,num_make}[4]`, and `RIVER_RESOURCE_VALUE` multiplication;
//! * `0x0063A186..0x0063A330`: `PEASANT_RATE` 8.8 scaling, city enhancer, tribe bonus
//!   15 (`JAPANESE_FISHING_BOATS`), and active-worker multiplication;
//! * `0x0063A335..0x0063A3AD`: Farm property `0x1A1`, tribe bonus 7, and
//!   `EGYPTIAN_FARM_WEALTH << 4`.
//!
//! Structure is PDB/disassembly-derived Tier C evidence, not a retail differential test.

use super::economy::{NUM_RESOURCES, RES_FOOD, RES_WEALTH};
use super::gathering::{AuthoritativeGatherTerrain, GatherTile, LandGatherData};
use super::map_terrain::{tflag, TILES_PER_WCELL};

pub const SHIPPED_FARM_TYPE_INDEX: i32 = 417;
pub const SHIPPED_FARM_X_SIZE: i32 = 4;
pub const SHIPPED_FARM_Y_SIZE: i32 = 4;
pub const FARM_PROPERTY: i32 = 0x1A1;
pub const MAX_FLAT_GATHERERS: i32 = 1;

const PEASANT_RATE_OFFSET: usize = 0x280;
const EGYPTIAN_FARM_WEALTH_OFFSET: usize = 0x658;
const JAPANESE_FISHING_BOATS_OFFSET: usize = 0x7AC;
const RIVER_RESOURCE_VALUE_OFFSET: usize = 0xC40;

/// The four Rules fields read by the supported Farm arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmGatherRules {
    pub peasant_rate_8_8: i32,
    pub egyptian_farm_wealth: i32,
    pub japanese_fishing_boats_percent: i32,
    pub river_resource_value: i32,
}

impl FarmGatherRules {
    /// Values parsed from the installed `ron-data/rules.xml` by the existing rules asset.
    pub const fn shipped() -> Self {
        Self {
            peasant_rate_8_8: 2560,
            egyptian_farm_wealth: 2,
            japanese_fishing_boats_percent: 25,
            river_resource_value: 2,
        }
    }

    /// Read the engine Rules value block by byte offset. Unlike `EconRules::from_block`,
    /// this rejects a short capture because zero-extension would fabricate live rules.
    pub fn from_rules_block(block: &[i32]) -> Result<Self, FarmGatherError> {
        let required = RIVER_RESOURCE_VALUE_OFFSET / 4 + 1;
        if block.len() < required {
            return Err(FarmGatherError::RulesBlockTooShort {
                required_dwords: required,
                actual_dwords: block.len(),
            });
        }
        Ok(Self {
            peasant_rate_8_8: block[PEASANT_RATE_OFFSET / 4],
            egyptian_farm_wealth: block[EGYPTIAN_FARM_WEALTH_OFFSET / 4],
            japanese_fishing_boats_percent: block[JAPANESE_FISHING_BOATS_OFFSET / 4],
            river_resource_value: block[RIVER_RESOURCE_VALUE_OFFSET / 4],
        })
    }
}

impl Default for FarmGatherRules {
    fn default() -> Self {
        Self::shipped()
    }
}

/// Type facts and instance inputs passed to the supported retail arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmGatherRequest {
    pub type_index: i32,
    pub is_flat: bool,
    pub x_size: i32,
    pub y_size: i32,
    pub corner: GatherTile,
    pub site_owner: i32,
    pub completed: bool,
    pub active_gatherers: i32,
    /// Exact `CityData::enhancer_amount(owner, resource, 100)` outputs.
    pub city_enhancer_percent: [i32; NUM_RESOURCES],
    /// `LeaderData::has_tribe_bonus(0x0F)`.
    pub has_japanese_fishing_bonus: bool,
    /// `LeaderData::has_tribe_bonus(7)`.
    pub has_egyptian_farm_bonus: bool,
}

impl FarmGatherRequest {
    pub const fn shipped_completed(corner: GatherTile, site_owner: i32) -> Self {
        Self {
            type_index: SHIPPED_FARM_TYPE_INDEX,
            is_flat: true,
            x_size: SHIPPED_FARM_X_SIZE,
            y_size: SHIPPED_FARM_Y_SIZE,
            corner,
            site_owner,
            completed: true,
            active_gatherers: 1,
            city_enhancer_percent: [100; NUM_RESOURCES],
            has_japanese_fishing_bonus: false,
            has_egyptian_farm_bonus: false,
        }
    }
}

/// Exact six-slot result of the bounded `BuildTypeData::calc_gather` arm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FarmGatherResult {
    /// Six local LandData totals before the Farm's Food-only `gathers` predicate.
    pub footprint_resources: [i32; NUM_RESOURCES],
    /// Per-worker value written through calc_gather's third output pointer.
    pub per_worker: i32,
    /// Six-slot amount accumulated into calc_gather's first output pointer.
    pub gross: [i32; NUM_RESOURCES],
    /// Value written through calc_gather's second output pointer.
    pub max_gatherers: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FarmGatherError {
    UnsupportedType(i32),
    NonFlatType,
    UnsupportedFootprint {
        x_size: i32,
        y_size: i32,
    },
    IncompleteBuilding,
    InvalidOwner(i32),
    ActiveGatherersOutOfRange(i32),
    InvalidWorldDimensions {
        tile_width: i32,
        tile_height: i32,
    },
    FootprintCoordinateOverflow(GatherTile),
    MissingTerritory {
        wx: i32,
        wy: i32,
    },
    MissingDiplomacy {
        owner: i32,
        territory_owner: i32,
    },
    MissingLand {
        wx: i32,
        wy: i32,
    },
    MissingTile(GatherTile),
    InvalidLandResource(i32),
    RulesBlockTooShort {
        required_dwords: usize,
        actual_dwords: usize,
    },
}

/// Evaluate the shipped completed-Farm arm of `BuildTypeData::calc_gather`.
pub fn calc_shipped_farm_gather(
    terrain: &impl AuthoritativeGatherTerrain,
    rules: &FarmGatherRules,
    request: FarmGatherRequest,
) -> Result<FarmGatherResult, FarmGatherError> {
    validate_request(request)?;
    let (tile_width, tile_height) = terrain.tile_dimensions();
    if tile_width <= 0 || tile_height <= 0 {
        return Err(FarmGatherError::InvalidWorldDimensions {
            tile_width,
            tile_height,
        });
    }
    let end_tx = request
        .corner
        .tx
        .checked_add(request.x_size)
        .ok_or(FarmGatherError::FootprintCoordinateOverflow(request.corner))?;
    let end_ty = request
        .corner
        .ty
        .checked_add(request.y_size)
        .ok_or(FarmGatherError::FootprintCoordinateOverflow(request.corner))?;

    let mut footprint_resources = [0_i32; NUM_RESOURCES];
    // Retail orders x outside y and skips invalid fine cells.
    for tx in request.corner.tx..end_tx {
        for ty in request.corner.ty..end_ty {
            if tx < 0 || ty < 0 || tx >= tile_width || ty >= tile_height {
                continue;
            }
            let wx = tx / TILES_PER_WCELL;
            let wy = ty / TILES_PER_WCELL;
            let territory_owner = terrain
                .territory_owner(wx, wy)
                .ok_or(FarmGatherError::MissingTerritory { wx, wy })?;
            if territory_owner >= 0 && territory_owner != request.site_owner {
                let allied = terrain
                    .is_allied(request.site_owner, territory_owner)
                    .ok_or(FarmGatherError::MissingDiplomacy {
                        owner: request.site_owner,
                        territory_owner,
                    })?;
                if !allied {
                    continue;
                }
            }

            // The binary obtains LandData before asking whether this fine tile is a river.
            let land = terrain
                .land_gather_data(wx, wy)
                .ok_or(FarmGatherError::MissingLand { wx, wy })?;
            let tile = GatherTile { tx, ty };
            let mask = terrain
                .tile_mask(tile)
                .ok_or(FarmGatherError::MissingTile(tile))?;
            accumulate_land(
                &mut footprint_resources,
                land,
                mask & tflag::RIVER != 0,
                rules.river_resource_value,
            )?;
        }
    }

    let mut per_worker =
        unscale_8_8(footprint_resources[RES_FOOD].wrapping_mul(rules.peasant_rate_8_8)).max(0);
    let city_percent = request.city_enhancer_percent[RES_FOOD];
    if city_percent != 100 {
        per_worker = per_worker.wrapping_mul(city_percent) / 100;
    }
    if request.has_japanese_fishing_bonus {
        per_worker = rules
            .japanese_fishing_boats_percent
            .wrapping_add(100)
            .wrapping_mul(per_worker)
            / 100;
    }

    let mut gross = [0_i32; NUM_RESOURCES];
    gross[RES_FOOD] = per_worker.wrapping_mul(request.active_gatherers);
    if request.has_egyptian_farm_bonus
        && rules.egyptian_farm_wealth > 0
        && request.active_gatherers != 0
    {
        gross[RES_WEALTH] =
            gross[RES_WEALTH].wrapping_add(rules.egyptian_farm_wealth.wrapping_shl(4));
    }

    Ok(FarmGatherResult {
        footprint_resources,
        per_worker,
        gross,
        max_gatherers: MAX_FLAT_GATHERERS,
    })
}

fn validate_request(request: FarmGatherRequest) -> Result<(), FarmGatherError> {
    if request.type_index != SHIPPED_FARM_TYPE_INDEX {
        return Err(FarmGatherError::UnsupportedType(request.type_index));
    }
    if !request.is_flat {
        return Err(FarmGatherError::NonFlatType);
    }
    if request.x_size != SHIPPED_FARM_X_SIZE || request.y_size != SHIPPED_FARM_Y_SIZE {
        return Err(FarmGatherError::UnsupportedFootprint {
            x_size: request.x_size,
            y_size: request.y_size,
        });
    }
    if !request.completed {
        return Err(FarmGatherError::IncompleteBuilding);
    }
    if !(0..8).contains(&request.site_owner) {
        return Err(FarmGatherError::InvalidOwner(request.site_owner));
    }
    if !(0..=MAX_FLAT_GATHERERS).contains(&request.active_gatherers) {
        return Err(FarmGatherError::ActiveGatherersOutOfRange(
            request.active_gatherers,
        ));
    }
    Ok(())
}

fn accumulate_land(
    out: &mut [i32; NUM_RESOURCES],
    land: LandGatherData,
    river: bool,
    river_multiplier: i32,
) -> Result<(), FarmGatherError> {
    for slot in land.slots {
        if slot.make < 0 {
            continue;
        }
        let resource = usize::try_from(slot.make)
            .ok()
            .filter(|&resource| resource < NUM_RESOURCES)
            .ok_or(FarmGatherError::InvalidLandResource(slot.make))?;
        let amount = if river {
            slot.num_make.wrapping_mul(river_multiplier)
        } else {
            slot.num_make
        };
        out[resource] = out[resource].wrapping_add(amount);
    }
    Ok(())
}

#[inline]
fn unscale_8_8(v: i32) -> i32 {
    let bias = if v < 0 { 0xFF } else { 0 };
    v.wrapping_add(bias) >> 8
}
