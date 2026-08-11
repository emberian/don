//! Source-frozen fresh starting-City constructor transaction.
//!
//! This module advances the shipped constructor chain past the old
//! `Object::add_to_world` stop far enough to own the complete checksum-visible
//! `City::init` body and `City::fix_world_vals` mutation.  It also records the
//! `Build::activate(0, 0, 0)` writes which are independent of BuildType virtuals.
//! The remaining BuildType/Leader callbacks stay explicit in the receipt, so the
//! adapter cannot accidentally promote a partial Build body to a Builds-channel
//! producer.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::objects::BANDED_SLOTS;
use don_sim::systems::borders_fog::CircleTable;
use don_sim::systems::map_terrain::{Coord, WCoord, World};
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::tech_cities::{self, CaravanLinkArray, CityRecord};

pub const SETUP_BUILD_CITIES_VA: u32 = 0x005a_b910;
pub const OBJECT_ADD_TO_WORLD_VA: u32 = 0x0064_d8c0;
pub const BUILD_INIT_VA: u32 = 0x0062_9740;
pub const WALL_ACTIVATE_VA: u32 = 0x0063_e4b0;
pub const BUILD_ACTIVATE_VA: u32 = 0x0062_3e20;
pub const CITIES_INIT_CITY_VA: u32 = 0x0073_52c0;
pub const CITY_INIT_VA: u32 = 0x0073_7050;
pub const CITY_GENERATE_NAME_VA: u32 = 0x0073_5c90;
pub const CITY_FIX_WORLD_VALS_VA: u32 = 0x0073_5aa0;
pub const CITY_FIND_BUILDINGS_VA: u32 = 0x0073_84c0;
pub const CITY_REGEN_ROADS_VA: u32 = 0x0073_8aa0;
pub const LEADER_CALC_POP_CAP_VA: u32 = 0x006d_c490;

/// `SubObject::init`'s type-derived flag which selects the City branch in
/// `Build::activate` at `0x00623f6a`.
pub const CITY_CENTER_OBJECT_FLAG: u8 = 0x20;
pub const CAPITAL_CITY_FLAGS: u16 = 0x4011;
pub const CITY_CARAVAN_INITIAL_CAPACITY: i32 = 10;
pub const REGION_CACHE_ROWS: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructorCall {
    ObjectAddToWorld,
    WallInitReturn,
    BuildInitReturn,
    WallActivate,
    RemoveUnbuiltCity,
    CitiesInitCity,
    CityInit,
    LeaderCalcPopCapInInit,
    CityGenerateName,
    CityFixWorldVals,
    CityFindBuildings,
    LeaderCalcPopCapAfterCounters,
    CityRegenRoads,
    ObjectUpdateSeen,
    ObjectUpdateSeenAlly,
}

/// Direct calls on the ordinary first-Village path, in machine-code order.  Virtual/type
/// queries inside the large Build bodies are intentionally not flattened into this list.
pub const FRESH_STARTING_CITY_CALL_ORDER: [ConstructorCall; 15] = [
    ConstructorCall::ObjectAddToWorld,
    ConstructorCall::WallInitReturn,
    ConstructorCall::BuildInitReturn,
    ConstructorCall::WallActivate,
    ConstructorCall::RemoveUnbuiltCity,
    ConstructorCall::CitiesInitCity,
    ConstructorCall::CityInit,
    ConstructorCall::LeaderCalcPopCapInInit,
    ConstructorCall::CityGenerateName,
    ConstructorCall::CityFixWorldVals,
    ConstructorCall::CityFindBuildings,
    ConstructorCall::LeaderCalcPopCapAfterCounters,
    ConstructorCall::CityRegenRoads,
    ConstructorCall::ObjectUpdateSeen,
    ConstructorCall::ObjectUpdateSeenAlly,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CityNameSelectionBranch {
    /// First name for this owner/tribe cohort: one-based ordinal equals the count of
    /// earlier Leaders with the same tribe plus one.
    FirstCohortOrdinal,
    /// Later names use the shipped local LFSR over `(tribe + World::seed)`.
    DeterministicLfsr,
    /// Retail falls back to the tribe's default name because the XML list is exhausted.
    DefaultFallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityNamePlanRequest {
    pub owner: u8,
    pub tribes: [i32; BANDED_SLOTS],
    pub city_name_counters: [i32; BANDED_SLOTS],
    pub world_seed: i32,
    /// Number of child nodes in the selected shipped `CITIES` XML list.
    pub list_len: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityNamePlanReceipt {
    pub branch: CityNameSelectionBranch,
    /// The Leader row whose shared-tribe counter was advanced.
    pub counter_owner: u8,
    pub counters_after: [i32; BANDED_SLOTS],
    /// One-based XML child ordinal. `None` means the default-name fallback.
    pub name_ordinal: Option<i32>,
    /// `City::generate_name` does not call `Random::rand`; its later-name shuffle is a
    /// private arithmetic LFSR based on tribe and `World::seed`.
    pub main_rng_draws: u32,
}

/// Reproduce the stateful selection spine in `City::generate_name`.
///
/// String/XML resolution remains a content-owner concern; this function owns the Leader
/// counter selection, one-based child ordinal, and the proof that the main simulation RNG
/// is not advanced.
pub fn plan_city_name(
    request: CityNamePlanRequest,
) -> Result<CityNamePlanReceipt, StartingCityConstructorError> {
    let owner = usize::from(request.owner);
    if owner >= BANDED_SLOTS {
        return Err(StartingCityConstructorError::OwnerOutOfRange {
            owner: request.owner,
        });
    }
    if request
        .city_name_counters
        .iter()
        .any(|&counter| counter < 0)
    {
        return Err(StartingCityConstructorError::NegativeCityNameCounter);
    }

    let tribe = request.tribes[owner];
    let mut counter_owner = owner;
    let mut greatest = -1;
    for (slot, (&candidate_tribe, &candidate_counter)) in request
        .tribes
        .iter()
        .zip(request.city_name_counters.iter())
        .enumerate()
    {
        if candidate_tribe == tribe && candidate_counter > greatest {
            counter_owner = slot;
            greatest = candidate_counter;
        }
    }
    if greatest <= 0 {
        counter_owner = owner;
    }

    let owner_counter_before = request.city_name_counters[owner];
    let mut counters_after = request.city_name_counters;
    counters_after[counter_owner] = counters_after[counter_owner].wrapping_add(1);
    let sequence = counters_after[counter_owner];
    if request.list_len <= sequence {
        return Ok(CityNamePlanReceipt {
            branch: CityNameSelectionBranch::DefaultFallback,
            counter_owner: counter_owner as u8,
            counters_after,
            name_ordinal: None,
            main_rng_draws: 0,
        });
    }

    if sequence < 2 || owner_counter_before == 0 {
        let ordinal = request.tribes[..owner]
            .iter()
            .filter(|&&candidate| candidate == tribe)
            .count() as i32
            + 1;
        if counters_after[owner] == 0 {
            counters_after[owner] = 1;
        }
        return Ok(CityNamePlanReceipt {
            branch: if ordinal <= request.list_len {
                CityNameSelectionBranch::FirstCohortOrdinal
            } else {
                CityNameSelectionBranch::DefaultFallback
            },
            counter_owner: counter_owner as u8,
            counters_after,
            name_ordinal: (ordinal <= request.list_len).then_some(ordinal),
            main_rng_draws: 0,
        });
    }

    let reserved = request
        .tribes
        .iter()
        .filter(|&&candidate| candidate == tribe)
        .count() as i32;
    let selectable = request.list_len - reserved + 1;
    if selectable <= 0 {
        return Err(StartingCityConstructorError::CityNameListTooShort {
            list_len: request.list_len,
            reserved,
        });
    }

    // `cdq; idiv ebx` at 0x00736255 is signed division: retain C/Rust's
    // truncation-toward-zero remainder rather than normalizing it with `rem_euclid`.
    let mut candidate = tribe
        .wrapping_add(request.world_seed)
        .wrapping_rem(selectable)
        .wrapping_add(1);
    let mut accepted = 1i32;
    loop {
        if candidate & 1 != 0 {
            candidate ^= 0x170;
        }
        candidate >>= 1;
        if candidate < selectable {
            accepted += 1;
            if accepted == sequence {
                break;
            }
        }
    }
    let ordinal = candidate + reserved;
    Ok(CityNamePlanReceipt {
        branch: CityNameSelectionBranch::DeterministicLfsr,
        counter_owner: counter_owner as u8,
        counters_after,
        name_ordinal: (ordinal <= request.list_len).then_some(ordinal),
        main_rng_draws: 0,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityFixWorldValsReceipt {
    pub radius_tiles: i32,
    pub circle_index: usize,
    pub inner_template_entries: u32,
    pub total_template_entries: u32,
    pub in_bounds_entries: u32,
    pub changed_entries: u32,
}

/// Exact `City::fix_world_vals` `WData::val` shifts.
pub fn apply_city_fix_world_vals(
    world: &mut World,
    position: (i32, i32),
    radius_tiles: i32,
) -> Result<CityFixWorldValsReceipt, StartingCityConstructorError> {
    if world.xs <= 0
        || world.ys <= 0
        || usize::try_from(world.xs).ok().and_then(|xs| {
            usize::try_from(world.ys)
                .ok()
                .and_then(|ys| xs.checked_mul(ys))
        }) != Some(world.wdata.len())
    {
        return Err(StartingCityConstructorError::InvalidWorldShape {
            xs: world.xs,
            ys: world.ys,
            cells: world.wdata.len(),
        });
    }
    if !(0..=64).contains(&radius_tiles) {
        return Err(StartingCityConstructorError::CityRadiusOutOfRange { radius_tiles });
    }

    let circle_index = usize::try_from((radius_tiles + 3) / 4)
        .unwrap_or_default()
        .min(61);
    let table = CircleTable::build();
    let inner = usize::try_from(table.radius[circle_index]).unwrap_or_default();
    let total = usize::try_from(table.radius[circle_index + 3]).unwrap_or_default();
    let center_x = WCoord::from_coord(Coord(position.0)).0;
    let center_y = WCoord::from_coord(Coord(position.1)).0;
    let xs = world.xs as usize;
    let mut in_bounds = 0u32;
    let mut changed = 0u32;

    for index in 0..total {
        let wx = center_x + i32::from(table.x[index]);
        let wy = center_y + i32::from(table.y[index]);
        if wx < 0 || wy < 0 || wx >= world.xs || wy >= world.ys {
            continue;
        }
        in_bounds += 1;
        let row = wy as usize * xs + wx as usize;
        let old = world.wdata[row].val;
        let new = if index < inner { old >> 2 } else { old >> 1 };
        world.wdata[row].val = new;
        changed += u32::from(new != old);
    }

    Ok(CityFixWorldValsReceipt {
        radius_tiles,
        circle_index,
        inner_template_entries: inner as u32,
        total_template_entries: total as u32,
        in_bounds_entries: in_bounds,
        changed_entries: changed,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreshStartingVillageRequest {
    pub owner: u8,
    pub city_slot: i16,
    /// Stable current ptype supplied by the Build initializer owner.
    pub current_type: i32,
    /// Exact display/id strings selected by the shipped XML/content branch. They are save
    /// state, but are absent from the sync checksum.
    pub city_name: String,
    pub city_id: String,
    pub indian_radius_bonus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingCitySideEffects {
    pub leader_pop_delta: i32,
    pub game_world_pop_delta: i32,
    pub leader_region_pop_delta: i16,
    pub leader_region_cities_delta: i16,
    pub leader_city_num_delta: i32,
    pub leader_city_mine_delta: i32,
    pub game_world_cities_delta: i32,
    pub setup_cities_built_delta: i32,
    pub region_border_caches_cleared: u32,
    pub leader_calc_pop_cap_calls: u32,
    pub find_buildings_calls: u32,
    pub regen_roads_calls: u32,
    pub main_rng_draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FreshStartingVillageReceipt {
    pub city: CityRecord,
    pub region: i16,
    pub object_id: i16,
    pub current_type: i32,
    pub position: (i32, i32),
    pub flags_before_activation: u8,
    pub flags_after_activation: u8,
    pub city_link_before: i16,
    pub city_link_after: i16,
    pub world_fix: CityFixWorldValsReceipt,
    pub effects: StartingCitySideEffects,
    /// The City body, caravan allocation history, and world-value shifts are complete.
    pub city_init_complete: bool,
    /// BuildType virtuals in the rest of `Build::init`/`activate` remain external.
    pub build_init_complete: bool,
    pub build_activation_complete: bool,
    pub builds_channel_ready: bool,
}

/// Advance the source-proven, checksum-relevant projection of the ordinary first Village.
///
/// All refusal paths are atomic. The center region is read from the already-produced WData
/// cell exactly as `City::init` does; callers cannot inject an unrelated region id.
pub fn apply_fresh_starting_village_projection(
    build: &mut BuildData,
    world: &mut World,
    request: FreshStartingVillageRequest,
) -> Result<FreshStartingVillageReceipt, StartingCityConstructorError> {
    let owner = usize::from(request.owner);
    if owner >= BANDED_SLOTS {
        return Err(StartingCityConstructorError::OwnerOutOfRange {
            owner: request.owner,
        });
    }
    if request.city_slot < 0 {
        return Err(StartingCityConstructorError::NegativeCitySlot {
            city_slot: request.city_slot,
        });
    }
    if request.current_type != tech_cities::ty::VILLAGE
        || build.orig_type != tech_cities::ty::VILLAGE
    {
        return Err(StartingCityConstructorError::NotStartingVillage {
            current_type: request.current_type,
            original_type: build.orig_type,
        });
    }
    let required_flags = production::flag::VALID | CITY_CENTER_OBJECT_FLAG;
    if build.flags & required_flags != required_flags {
        return Err(StartingCityConstructorError::MissingCityCenterFlags { flags: build.flags });
    }
    if build.who != request.owner {
        return Err(StartingCityConstructorError::BuildOwnerMismatch {
            expected: request.owner,
            actual: build.who,
        });
    }
    if build.city != -1 {
        return Err(StartingCityConstructorError::BuildAlreadyLinked { city: build.city });
    }
    let object_id = build.object_id();
    if object_id < 0 {
        return Err(StartingCityConstructorError::InvalidObjectId { object_id });
    }
    let position = build.position();
    let wx = WCoord::from_coord(Coord(position.0)).0;
    let wy = WCoord::from_coord(Coord(position.1)).0;
    if wx < 0 || wy < 0 || wx >= world.xs || wy >= world.ys {
        return Err(StartingCityConstructorError::CenterOutsideWorld { wx, wy });
    }
    let world_row = wy as usize * world.xs as usize + wx as usize;
    let region = world
        .wdata
        .get(world_row)
        .ok_or(StartingCityConstructorError::CenterOutsideWorld { wx, wy })?
        .region;

    let mut staged_build = build.clone();
    let mut staged_world = world.clone();
    let flags_before = staged_build.flags;
    let city_before = staged_build.city;

    // Source-proven common writes from Wall::activate and the City branch in
    // Build::activate. Unknown BuildType virtual results are deliberately preserved.
    staged_build.flags |= production::flag::STARTED | production::flag::ACTIVE;
    staged_build.job_counter = 0;
    staged_build.job_counter_2 = 0;
    staged_build.build_masks |= 0x1000;
    staged_build.city = request.city_slot;

    let radius = tech_cities::city_radius(
        &tech_cities::CityRules::RETAIL,
        tech_cities::ty::VILLAGE,
        request.indian_radius_bonus,
    );
    let world_fix = apply_city_fix_world_vals(&mut staged_world, position, radius)?;
    let city = CityRecord {
        city_flags: CAPITAL_CITY_FLAGS,
        city: request.city_slot,
        o: object_id,
        reg: region,
        x: position.0,
        y: position.1,
        conquest_node: -1,
        pop: 1,
        who: request.owner as i8,
        // City::init first writes 0/-1 here; Build::activate's fresh-city suffix then
        // writes the owner to both bytes before returning to Setup::build_cities.
        race: request.owner as i8,
        founder: request.owner as i8,
        land: 9,
        filled: 1,
        vans: CaravanLinkArray {
            items: Vec::new(),
            capacity: CITY_CARAVAN_INITIAL_CAPACITY,
            grow: -1,
            flags: 0,
        },
        name: request.city_name,
        id: request.city_id,
        ..CityRecord::default()
    };

    *build = staged_build;
    *world = staged_world;
    Ok(FreshStartingVillageReceipt {
        region,
        object_id,
        current_type: request.current_type,
        position,
        flags_before_activation: flags_before,
        flags_after_activation: build.flags,
        city_link_before: city_before,
        city_link_after: build.city,
        world_fix,
        effects: StartingCitySideEffects {
            leader_pop_delta: 1,
            game_world_pop_delta: 1,
            leader_region_pop_delta: 1,
            leader_region_cities_delta: 1,
            leader_city_num_delta: 1,
            leader_city_mine_delta: 1,
            game_world_cities_delta: 1,
            setup_cities_built_delta: 1,
            region_border_caches_cleared: REGION_CACHE_ROWS,
            leader_calc_pop_cap_calls: 2,
            find_buildings_calls: 1,
            regen_roads_calls: 1,
            main_rng_draws: 0,
        },
        city,
        city_init_complete: true,
        build_init_complete: false,
        build_activation_complete: false,
        builds_channel_ready: false,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmallCityBuildingsRequest {
    pub bonus_20: bool,
    pub bonus_20_farms: i32,
    pub bonus_19: bool,
}

/// Exact `Setup::small_city_buildings` TypeIndex call sequence for starting-town 2.
pub fn small_city_free_build_plan(request: SmallCityBuildingsRequest) -> Vec<i32> {
    let mut types = vec![tech_cities::ty::WOODCUTTER];
    let farms = if request.bonus_20 && request.bonus_20_farms != 0 {
        request.bonus_20_farms.max(0)
    } else if request.bonus_19 {
        0
    } else {
        3
    };
    types.extend(std::iter::repeat_n(tech_cities::ty::FARM, farms as usize));
    types.push(tech_cities::ty::LIBRARY);
    types
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CivSpecificBuildingsRequest {
    pub bonus_4: bool,
    pub bonus_22: bool,
    pub bonus_5: bool,
    pub bonus_5_rule: i32,
    pub bonus_16: bool,
    pub bonus_16_rule: i32,
    pub bonus_7: bool,
    pub bonus_7_rule: i32,
    pub bonus_10: bool,
    pub bonus_10_rule: i32,
    pub bonus_18: bool,
    pub bonus_18_rule: i32,
}

/// Exact `Setup::build_civ_specific` call order. Each element is one later
/// `Leader::free_build(type, center_object_id, 0)` transaction; those Builds' city-list
/// joins remain outside this constructor owner.
pub fn civ_specific_free_build_plan(request: CivSpecificBuildingsRequest) -> Vec<i32> {
    let mut types = Vec::new();
    if request.bonus_4 || request.bonus_22 {
        types.push(tech_cities::ty::MARKET);
    }
    if request.bonus_5 && request.bonus_5_rule > 1 {
        types.push(tech_cities::ty::UNIVERSITY);
    }
    if request.bonus_16 && request.bonus_16_rule > 1 {
        types.push(tech_cities::ty::TEMPLE);
    }
    if request.bonus_7 && request.bonus_7_rule > 1 {
        types.push(tech_cities::ty::SMELTER_AGE0);
    }
    if request.bonus_10 && request.bonus_10_rule > 1 {
        types.push(tech_cities::ty::SMELTER_AGE1);
    }
    if request.bonus_18 && request.bonus_18_rule != 0 {
        types.push(tech_cities::ty::CAPITOL);
    }
    types
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingCityConstructorError {
    OwnerOutOfRange {
        owner: u8,
    },
    NegativeCityNameCounter,
    CityNameListTooShort {
        list_len: i32,
        reserved: i32,
    },
    InvalidWorldShape {
        xs: i32,
        ys: i32,
        cells: usize,
    },
    CityRadiusOutOfRange {
        radius_tiles: i32,
    },
    NegativeCitySlot {
        city_slot: i16,
    },
    NotStartingVillage {
        current_type: i32,
        original_type: i32,
    },
    MissingCityCenterFlags {
        flags: u8,
    },
    BuildOwnerMismatch {
        expected: u8,
        actual: u8,
    },
    BuildAlreadyLinked {
        city: i16,
    },
    InvalidObjectId {
        object_id: i16,
    },
    CenterOutsideWorld {
        wx: i32,
        wy: i32,
    },
}

impl fmt::Display for StartingCityConstructorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting City constructor refused: {self:?}")
    }
}

impl std::error::Error for StartingCityConstructorError {}
