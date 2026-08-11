//! Exact World-side CITY installation for a freshly activated starting Village.
//!
//! This is deliberately a seam rather than another Build activator.  The activation
//! owner resolves the center/radius and proves the Build identity; this module consumes
//! that receipt and performs the late `BuildType::mask_me -> Wall::mask_city` TData
//! mutation.  The split keeps one owner for Build state and one owner for the canonical
//! World image observed by the first `Leader::plan_strategy` pass.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::map_terrain::{tflag, World};

pub const SETUP_BUILD_CITIES_VA: u32 = 0x005a_b910;
pub const SETUP_BUILD_INIT_CALL_VA: u32 = 0x005a_ba15;
pub const SETUP_BUILD_ACTIVATE_CALL_VA: u32 = 0x005a_babf;
pub const BUILD_ACTIVATE_VA: u32 = 0x0062_3e20;
pub const WALL_ACTIVATE_CALL_VA: u32 = 0x0062_3f65;
pub const WALL_START_MASK_CALL_VA: u32 = 0x0063_e86e;
pub const WALL_MASK_ME_VA: u32 = 0x0064_2fc0;
pub const BUILD_TYPE_MASK_ME_VA: u32 = 0x0063_12a0;
pub const BUILD_TYPE_MASK_CITY_CALL_VA: u32 = 0x0063_17c9;
pub const WALL_MASK_CITY_VA: u32 = 0x0063_e310;
pub const WALL_MASK_CITY_OR_VA: u32 = 0x0063_e36a;
pub const EVEN_CIRCLE_INIT_VA: u32 = 0x0068_16d0;
pub const OBJECT_UPDATE_SEEN_SETUP_CALL_VA: u32 = 0x005a_bae8;
pub const OBJECT_UPDATE_SEEN_ALLY_SETUP_CALL_VA: u32 = 0x005a_bb04;
pub const PER_CITY_COMPUTE_ALL_TERRITORY_CALL_VA: u32 = 0x005a_bb25;
pub const WORLD_ANALYZE_MAP_SETUP_CALL_VA: u32 = 0x005a_d5f4;
pub const FINAL_COMPUTE_ALL_TERRITORY_SETUP_CALL_VA: u32 = 0x005a_d603;
pub const WORLD_COMPUTE_ALL_TERRITORY_VA: u32 = 0x006b_5700;
pub const WORLD_COMPUTE_REG_TERRITORY_VA: u32 = 0x006b_0bb0;
pub const GAME_DO_FRAME_VA: u32 = 0x0059_1ef0;
pub const LEADERS_STRATEGY_ALL_CALL_VA: u32 = 0x0059_2448;
pub const GAME_DAEMON_PROCESS_ALL_CALL_VA: u32 = 0x0059_244d;
pub const LEADER_PLAN_STRATEGY_CALL_VA: u32 = 0x006e_d452;

pub const EVEN_CIRCLE_MAX_RADIUS: usize = 64;
pub const EVEN_CIRCLE_CAP: usize = 0x3249;
pub const FRESH_NATIVE_MASK_CITY_FLAGS: u8 = 0x23;
pub const FRESH_STARTING_CENTER_FLAGS: u8 = 0x27;
pub const CITY_OBJECT_FLAG: u8 = 0x20;

/// `even_circle_x/y/radius`, regenerated from `even_circle_init` `0x006816d0`.
///
/// Unlike the ordinary `circle_init` table, this table is centered between four tiles.
/// Positive source coordinates are decremented before storage, so radius one is the
/// ordered 2x2 set `(-1,-1), (-1,0), (0,-1), (0,0)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvenCircleTable {
    pub x: Vec<i8>,
    pub y: Vec<i8>,
    /// Cumulative endpoint: offsets for radius `r` occupy `0..radius[r]`.
    pub radius: [i32; EVEN_CIRCLE_MAX_RADIUS + 1],
}

impl EvenCircleTable {
    /// Instruction-order port of `even_circle_init`.
    pub fn build() -> Self {
        let mut x = Vec::with_capacity(EVEN_CIRCLE_CAP);
        let mut y = Vec::with_capacity(EVEN_CIRCLE_CAP);
        let mut radius = [0i32; EVEN_CIRCLE_MAX_RADIUS + 1];

        for r in 1..=EVEN_CIRCLE_MAX_RADIUS as i32 {
            let low = !r; // `not eax`: -r-1
            let high = r + 1;
            for sx in low..=high {
                for sy in low..=high {
                    if sx == 0 || sy == 0 {
                        continue;
                    }
                    let square = sx * sx + sy * sy;
                    let root = (square as f32).sqrt();
                    let mut rounded = root.trunc() as i32;
                    if root - rounded as f32 > 0.5 {
                        rounded += 1;
                    }
                    if rounded != r {
                        continue;
                    }

                    x.push((if sx > 0 { sx - 1 } else { sx }) as i8);
                    y.push((if sy > 0 { sy - 1 } else { sy }) as i8);
                    if x.len() >= EVEN_CIRCLE_CAP {
                        for endpoint in radius.iter_mut().skip(r as usize) {
                            *endpoint = x.len() as i32;
                        }
                        return Self { x, y, radius };
                    }
                }
            }
            radius[r as usize] = x.len() as i32;
        }
        radius[0] = 0;
        Self { x, y, radius }
    }
}

/// Source-proven output expected from the separate starting-Build activation owner.
///
/// This intentionally matches the activation lane's handoff shape byte-for-byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallMaskCityRequest {
    pub center_tcoord: (i32, i32),
    pub radius_tiles: i32,
    /// Exact retail mask argument, represented as its native `int`.
    pub on: i32,
    pub owner: u8,
    pub object_id: i16,
    pub native_call_flags: u8,
    pub native_call_city: i16,
    pub post_activation_flags: u8,
}

impl WallMaskCityRequest {
    #[inline]
    pub const fn city_flag_set(self) -> bool {
        self.native_call_flags & CITY_OBJECT_FLAG != 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PreStrategyWorldBoundary {
    /// The exact call order is sourced, including both setup territory passes and the
    /// fact that strategy precedes GameDaemon on frame zero.
    pub schedule_attested: bool,
    /// This transaction owns only `TData::CITY`; activation's blocker/road mutations are
    /// carried by the activation receipt and full setup territory remains a separate join.
    pub activation_mask_inputs_joined: bool,
    pub final_territory_image_joined: bool,
    /// Earlier `plan_strategy` Unit census inputs (`free`, `busy`, `peasant_dist`) are not
    /// produced by this World transaction.
    pub unit_derived_city_fields_joined: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingVillageWorldReceipt {
    pub request: WallMaskCityRequest,
    pub table_endpoint: usize,
    pub offsets_considered: u32,
    pub in_bounds_offsets: u32,
    pub out_of_bounds_offsets: u32,
    /// Native performs an OR for every in-bounds entry, even if CITY was already set.
    pub tdata_city_write_instructions: u32,
    pub tdata_cells_changed: u32,
    pub tdata_cells_already_set: u32,
    pub main_rng_draws: u32,
    pub build_writes: u32,
    pub leader_writes: u32,
    pub territory_writes: u32,
    pub city_mask_complete: bool,
    pub pre_strategy: PreStrategyWorldBoundary,
    pub first_checksum_world_image_ready: bool,
    pub first_checksum_city_image_ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingVillageWorldError {
    OwnerOutsideStrategySlots {
        owner: u8,
    },
    ObjectIdNegative {
        object_id: i16,
    },
    NotCityObject {
        native_call_flags: u8,
    },
    NativeCallFlagsNotFresh {
        native_call_flags: u8,
    },
    NativeCityLinkNotSentinel {
        native_call_city: i16,
    },
    PostActivationFlagsNotFresh {
        post_activation_flags: u8,
    },
    MaskOperationNotInstall {
        on: i32,
    },
    RadiusOutsideEvenCircle {
        radius: i32,
    },
    InvalidWorldShape {
        tile_xs: i32,
        tile_ys: i32,
        tile_size: i32,
        tdata_len: usize,
    },
}

impl fmt::Display for StartingVillageWorldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting-Village World transaction refused: {self:?}")
    }
}

impl std::error::Error for StartingVillageWorldError {}

fn validate_world(world: &World) -> Result<(), StartingVillageWorldError> {
    let expected = world.tile_xs.checked_mul(world.tile_ys);
    if world.tile_xs <= 0
        || world.tile_ys <= 0
        || expected != Some(world.tile_size)
        || usize::try_from(world.tile_size).ok() != Some(world.tdata.len())
    {
        return Err(StartingVillageWorldError::InvalidWorldShape {
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
            tile_size: world.tile_size,
            tdata_len: world.tdata.len(),
        });
    }
    Ok(())
}

/// Install the exact post-activation CITY disc consumed by the first strategy census.
///
/// Validation and index staging happen before the first write, so malformed cross-lane
/// receipts fail atomically.  Map-edge clipping is not an error: `Wall::mask_city` skips
/// each out-of-bounds offset independently in table order.
pub fn apply_starting_village_city_mask(
    world: &mut World,
    request: WallMaskCityRequest,
) -> Result<StartingVillageWorldReceipt, StartingVillageWorldError> {
    if request.owner >= 8 {
        return Err(StartingVillageWorldError::OwnerOutsideStrategySlots {
            owner: request.owner,
        });
    }
    if request.object_id < 0 {
        return Err(StartingVillageWorldError::ObjectIdNegative {
            object_id: request.object_id,
        });
    }
    if !request.city_flag_set() {
        return Err(StartingVillageWorldError::NotCityObject {
            native_call_flags: request.native_call_flags,
        });
    }
    if request.native_call_flags != FRESH_NATIVE_MASK_CITY_FLAGS {
        return Err(StartingVillageWorldError::NativeCallFlagsNotFresh {
            native_call_flags: request.native_call_flags,
        });
    }
    if request.native_call_city != -1 {
        return Err(StartingVillageWorldError::NativeCityLinkNotSentinel {
            native_call_city: request.native_call_city,
        });
    }
    if request.post_activation_flags != FRESH_STARTING_CENTER_FLAGS {
        return Err(StartingVillageWorldError::PostActivationFlagsNotFresh {
            post_activation_flags: request.post_activation_flags,
        });
    }
    if request.on != 1 {
        return Err(StartingVillageWorldError::MaskOperationNotInstall { on: request.on });
    }
    if !(1..=EVEN_CIRCLE_MAX_RADIUS as i32).contains(&request.radius_tiles) {
        return Err(StartingVillageWorldError::RadiusOutsideEvenCircle {
            radius: request.radius_tiles,
        });
    }
    validate_world(world)?;

    let table = EvenCircleTable::build();
    let endpoint = table.radius[request.radius_tiles as usize] as usize;
    let mut staged = Vec::with_capacity(endpoint);
    let mut out_of_bounds = 0u32;
    for index in 0..endpoint {
        let tx = request.center_tcoord.0 + i32::from(table.x[index]);
        let ty = request.center_tcoord.1 + i32::from(table.y[index]);
        if world.valid_t(tx, ty) {
            staged.push(world.t_index(tx, ty));
        } else {
            out_of_bounds += 1;
        }
    }

    let mut changed = 0u32;
    let mut already_set = 0u32;
    for &index in &staged {
        if world.tdata[index] & tflag::CITY == 0 {
            changed += 1;
        } else {
            already_set += 1;
        }
        world.tdata[index] |= tflag::CITY;
    }

    Ok(StartingVillageWorldReceipt {
        request,
        table_endpoint: endpoint,
        offsets_considered: endpoint as u32,
        in_bounds_offsets: staged.len() as u32,
        out_of_bounds_offsets: out_of_bounds,
        tdata_city_write_instructions: staged.len() as u32,
        tdata_cells_changed: changed,
        tdata_cells_already_set: already_set,
        main_rng_draws: 0,
        build_writes: 0,
        leader_writes: 0,
        territory_writes: 0,
        city_mask_complete: true,
        pre_strategy: PreStrategyWorldBoundary {
            schedule_attested: true,
            activation_mask_inputs_joined: false,
            final_territory_image_joined: false,
            unit_derived_city_fields_joined: false,
        },
        first_checksum_world_image_ready: false,
        first_checksum_city_image_ready: false,
    })
}
