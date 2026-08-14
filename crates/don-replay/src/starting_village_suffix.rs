//! Source-bounded attestation for the fresh-City suffix of `Build::activate`.
//!
//! The ordinary starting Village calls `City::find_buildings`, then later
//! `City::regen_roads`.  Those names sound mutating, but the shipped one-center path is
//! deliberately empty: every starting center has Object flag `0x20`, so the global
//! Build/Wall scan rejects it before `Build::add_to_city`, and the center's
//! `BuildData::city_down` chain is empty.  `City::check_upgrade` is also inert because a
//! Village needs six distinct active building kinds before becoming a Town.
//!
//! This module does not manufacture that conclusion from a caller boolean.  It joins the
//! canonical `Sim::builds` rows, production ptypes, City identity, and center chain and
//! refuses any state in which a scanned non-center Build could take the mutating arm.  Its
//! receipt is intended to be consumed by the later atomic starting-Village constructor.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

use don_sim::checksum::adler32;
use don_sim::systems::borders_fog::CircleTable;
use don_sim::systems::gather_terrain::{GatherTerrainMaterializationError, InstalledLandCatalog};
use don_sim::systems::map_terrain::{land, tflag, wflag, Coord, WCoord, World};
use don_sim::systems::production::BuildData;
use don_sim::systems::regions::{Region, Regions, REGION_COUNT};
use don_sim::systems::tech_cities::{self, CityRecord};
use don_sim::tick::Sim;

pub const CITY_FIND_BUILDINGS_VA: u32 = 0x0073_84c0;
pub const CITY_READY_TO_UPGRADE_VA: u32 = 0x0073_6480;
pub const CITY_CAN_UPGRADE_VA: u32 = 0x0073_83c0;
pub const CITY_ENOUGH_KINDS_VA: u32 = 0x0073_6540;
pub const CITY_CHECK_UPGRADE_VA: u32 = 0x0073_8b20;
pub const CITY_REGEN_ROADS_VA: u32 = 0x0073_8aa0;
pub const BUILD_ADD_TO_CITY_VA: u32 = 0x0062_2380;
pub const LEADER_PLAN_STRATEGY_VA: u32 = 0x006b_9620;
pub const LEADER_CITY_TERRAIN_CENSUS_VA: u32 = 0x006b_b400;
pub const WORLD_CHECK_BUILDING_WCOORD_VA: u32 = 0x006b_26e0;
pub const WORLD_SPACE_AT_CORNER_VA: u32 = 0x006b_27f0;
pub const WORLD_GATHER_AT_VA: u32 = 0x006b_07f0;
pub const REGION_NUM_COASTS_VA: u32 = 0x0068_0760;
pub const BUILD_TYPE_IS_DOCK_TILE_VA: u32 = 0x0063_6700;
pub const LEADER_HAS_TRIBE_BONUS_VA: u32 = 0x006e_1370;
pub const CITY_COUNT_GATHER_SLOTS_VA: u32 = 0x0073_7dc0;

/// `SubObject::init` sets this Object bit from `BuildTypeData::is_city`.
pub const CITY_CENTER_OBJECT_FLAG: u8 = 0x20;
/// Ordinary post-`Wall::activate` center state: VALID | STARTED | ACTIVE | CITY.
pub const FRESH_CENTER_FLAGS: u8 = 0x27;
/// `rules.xml` `CITY_BUILDINGS=5`; `CityData::can_upgrade(TOWN)` passes value + 1.
pub const SHIPPED_CITY_BUILDINGS: u32 = 5;
pub const TOWN_REQUIRED_DISTINCT_KINDS: u32 = SHIPPED_CITY_BUILDINGS + 1;
pub const FRESH_CAPITAL_CITY_FLAGS: u16 = 0x4011;

/// Coordinate-keyed results for content-backed `World::gather_at(..., mode=1)` calls.
/// Product facts are materialized from [`InstalledLandCatalog`] plus the exact [`World`]
/// state and retain both identities; explicit rows remain available for isolated fixtures.
/// An absent, stale, or modified row refuses the whole staged City mutation rather than
/// inventing terrain quantities from the recorded checksum.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CityTerrainGatherFacts {
    pub by_wcoord: BTreeMap<(i32, i32), [i32; tech_cities::NUM_RES]>,
    installed_source: Option<CityTerrainGatherInstalledSource>,
}

/// State/content identity for facts materialized through retail's mode-one gather path.
/// A later World mutation makes the fact table stale and refuses before City publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityTerrainGatherInstalledSource {
    pub installed_rules_sha256: [u8; 32],
    pub world_checksum: u32,
    pub world_xs: i32,
    pub world_ys: i32,
    pub world_seed: i32,
    pub cells_materialized: u32,
    pub land_histogram: [u32; 9],
    pub gathered_centers: u32,
    pub fact_table_checksum: u32,
}

impl CityTerrainGatherFacts {
    /// Materialize every in-bounds result through retail's exact
    /// `World::gather_at(..., mode=1)` source owner. No recorded checksum is consulted;
    /// the World checksum is retained only as a stale-state guard for later consumption.
    pub fn from_installed_land_catalog(
        world: &World,
        catalog: &InstalledLandCatalog,
    ) -> Result<Self, GatherTerrainMaterializationError> {
        if world.xs <= 0
            || world.ys <= 0
            || world.tile_xs != world.xs.wrapping_mul(4)
            || world.tile_ys != world.ys.wrapping_mul(4)
            || usize::try_from(world.size).ok() != Some(world.wdata.len())
            || usize::try_from(world.tile_size).ok() != Some(world.tdata.len())
        {
            return Err(GatherTerrainMaterializationError::StaleWorldShape);
        }
        let mut by_wcoord = BTreeMap::new();
        let mut land_histogram = [0u32; 9];
        let mut gathered_centers = 0u32;
        for wy in 0..world.ys {
            for wx in 0..world.xs {
                let receipt = catalog.gather_at_mode_one(world, wx, wy)?;
                let land = usize::try_from(receipt.land_index)
                    .ok()
                    .filter(|&index| index < land_histogram.len())
                    .ok_or(GatherTerrainMaterializationError::InvalidLandIndex {
                        index: receipt.land_index,
                    })?;
                land_histogram[land] = land_histogram[land].wrapping_add(1);
                gathered_centers =
                    gathered_centers.wrapping_add(u32::from(receipt.center_gathered));
                by_wcoord.insert((wx, wy), receipt.output);
            }
        }
        // Exact shape validation above binds this length to positive signed `World::size`,
        // hence it is necessarily representable in u32.
        let cells_materialized = by_wcoord.len() as u32;
        let fact_table_checksum = gather_fact_table_checksum(&by_wcoord);
        Ok(Self {
            by_wcoord,
            installed_source: Some(CityTerrainGatherInstalledSource {
                installed_rules_sha256: catalog.installed_rules_sha256(),
                world_checksum: world.checksum_sections().full,
                world_xs: world.xs,
                world_ys: world.ys,
                world_seed: world.seed,
                cells_materialized,
                land_histogram,
                gathered_centers,
                fact_table_checksum,
            }),
        })
    }

    pub fn installed_source(&self) -> Option<CityTerrainGatherInstalledSource> {
        self.installed_source
    }

    fn validate_world(&self, world: &World) -> Result<(), FreshVillageTerrainCensusError> {
        let Some(source) = self.installed_source else {
            return Ok(());
        };
        let actual_fact_table_checksum = gather_fact_table_checksum(&self.by_wcoord);
        if actual_fact_table_checksum != source.fact_table_checksum {
            return Err(FreshVillageTerrainCensusError::TamperedGatherAtFacts {
                expected_fact_table_checksum: source.fact_table_checksum,
                actual_fact_table_checksum,
            });
        }
        let actual_checksum = world.checksum_sections().full;
        if (world.xs, world.ys, world.seed, actual_checksum)
            != (
                source.world_xs,
                source.world_ys,
                source.world_seed,
                source.world_checksum,
            )
        {
            return Err(FreshVillageTerrainCensusError::StaleGatherAtFacts {
                expected_world_checksum: source.world_checksum,
                actual_world_checksum: actual_checksum,
            });
        }
        Ok(())
    }
}

fn gather_fact_table_checksum(
    by_wcoord: &BTreeMap<(i32, i32), [i32; tech_cities::NUM_RES]>,
) -> u32 {
    let mut bytes = Vec::with_capacity(by_wcoord.len().saturating_mul(32));
    for (&(wx, wy), output) in by_wcoord {
        bytes.extend_from_slice(&wx.to_le_bytes());
        bytes.extend_from_slice(&wy.to_le_bytes());
        for value in output {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    adler32(1, &bytes)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CityTerrainCensusBytes {
    pub ocean: u8,
    pub land: u8,
    pub filled: u8,
    pub ocean_filled: u8,
    pub dock_tile: u8,
    pub space: [u8; 3],
    pub ter: [u8; tech_cities::NUM_RES],
}

impl CityTerrainCensusBytes {
    fn read(city: &CityRecord) -> Self {
        Self {
            ocean: city.ocean,
            land: city.land,
            filled: city.filled,
            ocean_filled: city.ocean_filled,
            dock_tile: city.dock_tile,
            space: city.space,
            ter: city.ter,
        }
    }
}

/// Leader writes adjacent to the City-byte recomputation.  The transaction reports these
/// exact joins without pretending to own the large `LeaderData` object itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CityTerrainLeaderDeltas {
    /// Candidate for `LeaderData +0x84c = max(old, City::pop)`.
    pub max_city_pop_candidate: u8,
    /// `City::count_gather_slots(0, 0)` added to the City region's short accumulator.
    /// The source-bounded one-center chain contains no gathering Build, hence zero.
    pub gather_slots_for_region: i16,
    /// `city_flags & 2` is clear in the fresh `0x4011` image.
    pub flag_2_city_count: u32,
    /// Increment at `LeaderData +0x9d0` when `land - filled < 2`.
    pub low_open_land_city_count: u32,
    /// Short added to the region-indexed accumulator at `LeaderData +0x13de`.
    pub open_land_for_region: i16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CityTerrainCensusReceipt {
    /// The already-landed `find_buildings/check_upgrade/regen_roads` proof joined before
    /// this later strategy mutation begins.
    pub constructor_suffix: FreshVillageFindBuildingsReceipt,
    pub center_wcoord: (i32, i32),
    pub radius_tiles: i32,
    pub circle_index: usize,
    /// Exact cumulative table endpoints read from `circle_radius[index]` and `[index+1]`.
    pub inner_table_end: usize,
    pub outer_table_end: usize,
    /// Retail reads `circle_x/y[i+1]`: table entry zero (the center) is not scanned.
    pub first_table_index: usize,
    pub last_table_index: usize,
    pub table_entries_scanned: u32,
    pub in_bounds_entries: u32,
    pub foreign_owner_rejections: u32,
    pub admitted_entries: u32,
    pub inner_entries: u32,
    pub inner_region_matches: u32,
    pub water_entries: u32,
    pub waterhalf_entries: u32,
    pub placement_calls: u32,
    /// Return histogram for exact grades 0 through 4 from `check_building_wcoord`.
    pub placement_grades: [u32; 5],
    pub gather_at_calls: u32,
    /// Installed content/world identity when gather rows came from the exact source owner;
    /// `None` preserves the explicit fact-fixture boundary.
    pub gather_installed_source: Option<CityTerrainGatherInstalledSource>,
    pub dock_tile_calls: u32,
    pub dock_tile_successes: u32,
    pub bytes_before: CityTerrainCensusBytes,
    pub bytes_after: CityTerrainCensusBytes,
    pub bordering_before: u8,
    pub bordering_after: u8,
    pub was_capital_flags_before: u8,
    pub was_capital_flags_after: u8,
    pub leader: CityTerrainLeaderDeltas,
    pub city_pod_bytes_changed: u32,
    pub city_terrain_census_complete: bool,
    pub world_writes: u32,
    pub build_or_registry_writes: u32,
    pub main_rng_draws: u32,
    /// Other, earlier parts of `Leader::plan_strategy` can still rewrite `peasant_dist`,
    /// `free`, and `busy` from Units.  This remains false until those producers and the
    /// complete pre-checkpoint schedule are joined too.
    pub first_checksum_city_image_ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshVillageTerrainCensusFacts<'a> {
    pub upgrade: FreshVillageUpgradeFacts,
    /// Exact result of `LeaderData::has_tribe_bonus(0x15)`.
    pub indian_radius_bonus: bool,
    pub gather: &'a CityTerrainGatherFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshVillageUpgradeFacts {
    /// Exact result of `LeaderData::type_avail(TOWN, 1)`.  It controls whether retail
    /// enters `CityData::can_upgrade`; either arm is inert for the one-kind chain.
    pub town_type_available: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshVillageFindBuildingsReceipt {
    pub owner: u8,
    pub city_slot: i16,
    pub center_object_id: i16,
    pub scanned_build_rows: u32,
    pub rejected_city_center_rows: u32,
    pub add_to_city_calls: u32,
    pub city_pod_writes: u32,
    pub build_city_link_writes: u32,
    pub ready_to_upgrade_calls: u32,
    pub type_avail_calls: u32,
    pub can_upgrade_calls: u32,
    pub distinct_active_kinds: u32,
    pub required_distinct_kinds: u32,
    pub check_upgrade_body_entered: bool,
    pub regen_members_walked: u32,
    pub regen_build_mask_writes: u32,
    pub leader_writes: u32,
    pub world_writes: u32,
    pub main_rng_draws: u32,
    pub city_suffix_complete: bool,
    /// `Leader::plan_strategy` later clears and recomputes fourteen terrain-census bytes
    /// within City object offsets `+0x62..=+0x71` before the first known replay checkpoint.
    /// This constructor-suffix receipt does not claim that downstream image.
    pub first_checksum_city_image_ready: bool,
}

impl FreshVillageFindBuildingsReceipt {
    /// This receipt settles the City scan/road suffix only.  It cannot by itself certify
    /// the earlier Object/Wall/Build initializer or publish either checksum channel.
    #[inline]
    pub const fn constructor_transaction_ready(self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FreshVillageSuffixError {
    CityNotFreshCapital {
        city_flags: u16,
    },
    CityOwnerOutOfRange {
        owner: i8,
    },
    CitySlotNegative {
        city_slot: i16,
    },
    CenterObjectIdNegative {
        object_id: i16,
    },
    MissingCenterBuild {
        owner: u8,
        object_id: i16,
    },
    DuplicateCenterBuild {
        owner: u8,
        object_id: i16,
    },
    CenterFlagsNotFresh {
        flags: u8,
    },
    CenterCityMismatch {
        expected: i16,
        actual: i16,
    },
    CenterChainNotEmpty {
        city_down: i16,
    },
    CenterOwnerMismatch {
        expected: u8,
        actual: u8,
    },
    CenterPositionMismatch {
        city: (i32, i32),
        build: (i32, i32),
    },
    MissingCenterType {
        row: usize,
    },
    CenterTypeNotVillage {
        current_type: i32,
    },
    /// A row without Object flag `0x20` reaches the distance/ownership/AddToCity fork in
    /// retail.  Its exact effects require the general transaction and are not fresh setup.
    ScannedNonCenterBuild {
        row: usize,
        owner: u8,
        object_id: i16,
        flags: u8,
    },
}

impl fmt::Display for FreshVillageSuffixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fresh starting-Village suffix refused: {self:?}")
    }
}

impl std::error::Error for FreshVillageSuffixError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FreshVillageTerrainCensusError {
    ConstructorSuffix(FreshVillageSuffixError),
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
    CenterOutsideWorld {
        wx: i32,
        wy: i32,
    },
    RegionOutsideTable {
        wx: i32,
        wy: i32,
        region: i16,
    },
    PlacementGradeOutsideRetailRange {
        wx: i32,
        wy: i32,
        grade: i32,
    },
    MissingGatherAtFact {
        wx: i32,
        wy: i32,
    },
    StaleGatherAtFacts {
        expected_world_checksum: u32,
        actual_world_checksum: u32,
    },
    TamperedGatherAtFacts {
        expected_fact_table_checksum: u32,
        actual_fact_table_checksum: u32,
    },
}

impl fmt::Display for FreshVillageTerrainCensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fresh starting-Village terrain census refused: {self:?}")
    }
}

impl std::error::Error for FreshVillageTerrainCensusError {}

impl From<FreshVillageSuffixError> for FreshVillageTerrainCensusError {
    fn from(value: FreshVillageSuffixError) -> Self {
        Self::ConstructorSuffix(value)
    }
}

fn matching_center(build: &BuildData, city: &CityRecord) -> bool {
    build.who == city.who as u8 && build.object_id() == city.o
}

/// Attest the exact `City::find_buildings -> check_upgrade` and `City::regen_roads`
/// result for an ordinary starting-town-1 center.
///
/// This is read-only because the source-proven result is no mutation.  Every Build row is
/// nevertheless inspected: `City::find_buildings` scans the Build and Wall object bands
/// for all eight Leaders, and certifying an empty result from the center alone would miss
/// a non-center row.  DoN stores starting buildings in the canonical Build owner; a future
/// distinct Wall owner must extend this preflight before this receipt is promoted there.
pub fn attest_fresh_starting_village_suffix(
    sim: &Sim,
    city: &CityRecord,
    upgrade: FreshVillageUpgradeFacts,
) -> Result<FreshVillageFindBuildingsReceipt, FreshVillageSuffixError> {
    if city.city_flags != FRESH_CAPITAL_CITY_FLAGS {
        return Err(FreshVillageSuffixError::CityNotFreshCapital {
            city_flags: city.city_flags,
        });
    }
    let owner = u8::try_from(city.who)
        .ok()
        .filter(|owner| usize::from(*owner) < tech_cities::NUM_PLAYERS)
        .ok_or(FreshVillageSuffixError::CityOwnerOutOfRange { owner: city.who })?;
    if city.city < 0 {
        return Err(FreshVillageSuffixError::CitySlotNegative {
            city_slot: city.city,
        });
    }
    if city.o < 0 {
        return Err(FreshVillageSuffixError::CenterObjectIdNegative { object_id: city.o });
    }

    let mut center_row = None;
    let mut rejected_city_center_rows = 0u32;
    for (row, build) in sim.builds.iter().enumerate() {
        if build.flags & CITY_CENTER_OBJECT_FLAG == 0 {
            return Err(FreshVillageSuffixError::ScannedNonCenterBuild {
                row,
                owner: build.who,
                object_id: build.object_id(),
                flags: build.flags,
            });
        }
        rejected_city_center_rows = rejected_city_center_rows.saturating_add(1);
        if matching_center(build, city) && center_row.replace(row).is_some() {
            return Err(FreshVillageSuffixError::DuplicateCenterBuild {
                owner,
                object_id: city.o,
            });
        }
    }

    let row = center_row.ok_or(FreshVillageSuffixError::MissingCenterBuild {
        owner,
        object_id: city.o,
    })?;
    let center = &sim.builds[row];
    if center.flags != FRESH_CENTER_FLAGS {
        return Err(FreshVillageSuffixError::CenterFlagsNotFresh {
            flags: center.flags,
        });
    }
    if center.who != owner {
        return Err(FreshVillageSuffixError::CenterOwnerMismatch {
            expected: owner,
            actual: center.who,
        });
    }
    if center.city != city.city {
        return Err(FreshVillageSuffixError::CenterCityMismatch {
            expected: city.city,
            actual: center.city,
        });
    }
    if center.city_down != -1 {
        return Err(FreshVillageSuffixError::CenterChainNotEmpty {
            city_down: center.city_down,
        });
    }
    if center.position() != (city.x, city.y) {
        return Err(FreshVillageSuffixError::CenterPositionMismatch {
            city: (city.x, city.y),
            build: center.position(),
        });
    }
    let current_type = sim
        .production_runtime
        .build_types
        .get(row)
        .copied()
        .flatten()
        .ok_or(FreshVillageSuffixError::MissingCenterType { row })?;
    if current_type != tech_cities::ty::VILLAGE || center.orig_type != tech_cities::ty::VILLAGE {
        return Err(FreshVillageSuffixError::CenterTypeNotVillage { current_type });
    }

    // `CityData::enough_kinds` starts at the center and follows `city_down`.  The fresh
    // chain contains exactly the active Village center, hence one distinct kind.  The
    // shipped Town threshold is CITY_BUILDINGS + 1 == 6, so `ready_to_upgrade` is false
    // even when type availability admits the second half of its short-circuit.
    let can_upgrade_calls = u32::from(upgrade.town_type_available);
    Ok(FreshVillageFindBuildingsReceipt {
        owner,
        city_slot: city.city,
        center_object_id: city.o,
        scanned_build_rows: u32::try_from(sim.builds.len()).unwrap_or(u32::MAX),
        rejected_city_center_rows,
        add_to_city_calls: 0,
        city_pod_writes: 0,
        build_city_link_writes: 0,
        ready_to_upgrade_calls: 1,
        type_avail_calls: 1,
        can_upgrade_calls,
        distinct_active_kinds: 1,
        required_distinct_kinds: TOWN_REQUIRED_DISTINCT_KINDS,
        check_upgrade_body_entered: false,
        regen_members_walked: 0,
        regen_build_mask_writes: 0,
        leader_writes: 0,
        world_writes: 0,
        main_rng_draws: 0,
        city_suffix_complete: true,
        first_checksum_city_image_ready: false,
    })
}

/// Exact `Region::num_coasts(int)` result for the PDB-shaped Region record.
///
/// The stack argument is unused in the shipped body.  It examines sea identities 65 through
/// 126 in that order (not 64 or 127).  A land Region tests its `coast` bit mask; a sea Region
/// counts only its own identity.
pub fn retail_region_num_coasts(region: &Region) -> i32 {
    let mut count = 0i32;
    for sea in 65usize..127usize {
        let touches = if region.region == sea as i32 {
            true
        } else if region.region < 64 {
            let bit = sea - 64;
            region.coast.bytes[bit >> 3] & (1 << (bit & 7)) != 0
        } else {
            false
        };
        count += i32::from(touches);
    }
    count
}

#[inline]
fn exact_world_shape(world: &World) -> bool {
    world.xs > 0
        && world.ys > 0
        && world.tile_xs == world.xs.wrapping_mul(4)
        && world.tile_ys == world.ys.wrapping_mul(4)
        && world.size == world.xs.wrapping_mul(world.ys)
        && world.tile_size == world.tile_xs.wrapping_mul(world.tile_ys)
        && usize::try_from(world.size).ok() == Some(world.wdata.len())
        && usize::try_from(world.tile_size).ok() == Some(world.tdata.len())
}

#[inline]
fn wcell_is_open_water(world: &World, wx: i32, wy: i32) -> bool {
    let w = world.wdata(wx, wy);
    w.flags & wflag::WATERHALF == 0 && matches!(w.land, land::COASTAL | land::OCEAN)
}

/// Direct `World`-owner transcription of `BuildTypeData::is_dock_tile` `0x00636700`.
/// The already-existing naval port uses a reduced `WaterWorld`; keeping this local adapter
/// avoids copying the full map merely to make one source-owned shore query.
fn is_dock_tile(world: &World, wx: i32, wy: i32) -> bool {
    if wx < 0 || wy < 0 || wx >= world.xs || wy >= world.ys {
        return false;
    }
    if !wcell_is_open_water(world, wx, wy) {
        return false;
    }

    const ORTHOG_X: [i32; 5] = [0, 0, 1, 0, -1];
    const ORTHOG_Y: [i32; 5] = [0, -1, 0, 1, 0];
    let tx = wx * 4;
    let ty = wy * 4;
    for direction in 1..=4usize {
        let mut dx = ORTHOG_X[direction];
        let mut dy = ORTHOG_Y[direction];
        if dx > 0 {
            dx <<= 2;
        }
        if dy > 0 {
            dy <<= 2;
        }
        let mut nx = tx + dx;
        let mut ny = ty + dy;
        if !world.valid_t(nx, ny) || wcell_is_open_water(world, nx >> 2, ny >> 2) {
            continue;
        }
        let shore = world.tmask(nx, ny);
        if shore & tflag::BLOCKER_MASK == tflag::BLOCKER_MOUNTAIN
            || shore & tflag::SURFACE_MASK == tflag::SURFACE_TREES
        {
            continue;
        }

        let mut clear = true;
        if dx == 0 {
            for row in 0..2 {
                if row != 0 {
                    ny += dy.signum();
                }
                for column in 0..4 {
                    let px = tx + column;
                    if !world.valid_t(px, ny) {
                        clear = false;
                        break;
                    }
                    let mask = world.tmask(px, ny);
                    if mask & tflag::STARTED != 0
                        || mask & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING
                    {
                        clear = false;
                        break;
                    }
                }
                if !clear {
                    break;
                }
            }
        } else {
            for column in 0..2 {
                if column != 0 {
                    nx += dx.signum();
                }
                for row in 0..4 {
                    let py = ty + row;
                    if !world.valid_t(nx, py) {
                        clear = false;
                        break;
                    }
                    let mask = world.tmask(nx, py);
                    if mask & tflag::STARTED != 0
                        || mask & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING
                    {
                        clear = false;
                        break;
                    }
                }
                if !clear {
                    break;
                }
            }
        }
        if clear {
            return true;
        }
    }
    false
}

/// Join the source-frozen starting-Village constructor suffix to the later City terrain
/// census inside `Leader::plan_strategy`.
///
/// The mutation is atomic: all WData/TData reads, Region rows, and content-backed
/// `World::gather_at` facts are preflighted against a cloned City.  The caller's record is
/// assigned only after the complete retail circle walk succeeds.  World, Build, registry,
/// Leader, and main-RNG state are not mutated; exact Leader deltas are returned separately.
pub fn apply_fresh_starting_village_terrain_census(
    sim: &Sim,
    city: &mut CityRecord,
    world: &World,
    regions: &Regions,
    facts: FreshVillageTerrainCensusFacts<'_>,
) -> Result<CityTerrainCensusReceipt, FreshVillageTerrainCensusError> {
    let constructor_suffix = attest_fresh_starting_village_suffix(sim, city, facts.upgrade)?;
    if !exact_world_shape(world) {
        return Err(FreshVillageTerrainCensusError::InvalidWorldShape {
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
    facts.gather.validate_world(world)?;

    let center_wcoord = (
        WCoord::from_coord(Coord(city.x)).0,
        WCoord::from_coord(Coord(city.y)).0,
    );
    if center_wcoord.0 < 0
        || center_wcoord.1 < 0
        || center_wcoord.0 >= world.xs
        || center_wcoord.1 >= world.ys
    {
        return Err(FreshVillageTerrainCensusError::CenterOutsideWorld {
            wx: center_wcoord.0,
            wy: center_wcoord.1,
        });
    }

    let radius_tiles = tech_cities::city_radius(
        &tech_cities::CityRules::RETAIL,
        tech_cities::ty::VILLAGE,
        facts.indian_radius_bonus,
    );
    // `add eax,2; cdq; and edx,3; add eax,edx; sar eax,2` is signed division by four.
    // The admitted retail radii are positive, so normal integer division is identical.
    let circle_index = ((radius_tiles + 2) / 4) as usize;
    let table = CircleTable::build();
    let inner_table_end = table.radius[circle_index] as usize;
    let outer_table_end = table.radius[circle_index + 1] as usize;
    let bytes_before = CityTerrainCensusBytes::read(city);
    let pod_before = city.pod_bytes();
    let bordering_before = city.bordering;
    let was_capital_flags_before = city.was_capital_flags;
    let owner = i32::from(constructor_suffix.owner);
    let mut staged = city.clone();

    // Exact clear order at 0x006bb51e..0x006bb674. `bordering` +0x65 and
    // `was_capital_flags` +0x68 are deliberately untouched.
    staged.ocean = 0;
    staged.ocean_filled = 0;
    staged.land = 0;
    staged.filled = 0;
    staged.dock_tile = 0;
    staged.ter.fill(0);
    staged.space.fill(0);

    let mut table_entries_scanned = 0u32;
    let mut in_bounds_entries = 0u32;
    let mut foreign_owner_rejections = 0u32;
    let mut admitted_entries = 0u32;
    let mut inner_entries = 0u32;
    let mut inner_region_matches = 0u32;
    let mut water_entries = 0u32;
    let mut waterhalf_entries = 0u32;
    let mut placement_calls = 0u32;
    let mut placement_grades = [0u32; 5];
    let mut gather_at_calls = 0u32;
    let mut dock_tile_calls = 0u32;
    let mut dock_tile_successes = 0u32;

    // The machine loop stores i=0 but reads circle_x/y[i+1].  Its bottom test is
    // `(i+1) < circle_radius[index+1]`, hence the exact table range is 1..outer_end.
    for table_index in 1..outer_table_end {
        table_entries_scanned = table_entries_scanned.wrapping_add(1);
        let wx = center_wcoord.0 + i32::from(table.x[table_index]);
        let wy = center_wcoord.1 + i32::from(table.y[table_index]);
        if wx < 0 || wy < 0 || wx >= world.xs || wy >= world.ys {
            continue;
        }
        in_bounds_entries = in_bounds_entries.wrapping_add(1);
        let w = world.wdata(wx, wy);
        let territory_owner = i32::from(w.who);
        if territory_owner >= 0 && territory_owner != owner {
            foreign_owner_rejections = foreign_owner_rejections.wrapping_add(1);
            continue;
        }
        admitted_entries = admitted_entries.wrapping_add(1);
        if w.flags & wflag::WATERHALF != 0 {
            waterhalf_entries = waterhalf_entries.wrapping_add(1);
        }

        // An open-water WData row takes the outer water/dock arm and then jumps directly
        // to the loop bottom; it never executes the inner land/fill/gather arm.
        if w.flags & wflag::WATERHALF == 0 && matches!(w.land, land::COASTAL | land::OCEAN) {
            water_entries = water_entries.wrapping_add(1);
            staged.ocean = staged.ocean.wrapping_add(1);
            let region_index = usize::try_from(w.region).ok().filter(|&r| r < REGION_COUNT);
            let Some(region_index) = region_index else {
                return Err(FreshVillageTerrainCensusError::RegionOutsideTable {
                    wx,
                    wy,
                    region: w.region,
                });
            };
            let region = &regions.list[region_index];
            if retail_region_num_coasts(region) > 1 || region.size >= world.size / 10 {
                if staged.dock_tile == 0 {
                    dock_tile_calls = dock_tile_calls.wrapping_add(1);
                    if is_dock_tile(world, wx, wy) {
                        dock_tile_successes = dock_tile_successes.wrapping_add(1);
                        staged.dock_tile = staged.dock_tile.wrapping_add(1);
                    }
                }
            }
            continue;
        }

        if table_index >= inner_table_end {
            continue;
        }
        inner_entries = inner_entries.wrapping_add(1);
        if w.region != staged.reg {
            continue;
        }
        inner_region_matches = inner_region_matches.wrapping_add(1);

        let mut needs_gather = w.flags & wflag::IMPASSABLE_MASK != 0;
        if !needs_gather {
            staged.land = staged.land.wrapping_add(1);
            placement_calls = placement_calls.wrapping_add(1);
            let grade = world.check_building_wcoord(wx, wy, owner, 0, 0, 1, true);
            let grade_index = usize::try_from(grade)
                .ok()
                .filter(|&grade| grade < placement_grades.len())
                .ok_or(
                    FreshVillageTerrainCensusError::PlacementGradeOutsideRetailRange {
                        wx,
                        wy,
                        grade,
                    },
                )?;
            placement_grades[grade_index] = placement_grades[grade_index].wrapping_add(1);
            if grade >= 2 {
                for ordinal in 2..=grade.min(4) {
                    let space = usize::try_from(ordinal - 2).unwrap_or_default();
                    staged.space[space] = staged.space[space].wrapping_add(1);
                }
            }
            if grade < 4 {
                // Both grade zero and the otherwise-unproduced grade one take this arm.
                staged.filled = staged.filled.wrapping_add(1);
            } else {
                needs_gather = true;
            }
        }

        if needs_gather {
            let output = facts
                .gather
                .by_wcoord
                .get(&(wx, wy))
                .ok_or(FreshVillageTerrainCensusError::MissingGatherAtFact { wx, wy })?;
            gather_at_calls = gather_at_calls.wrapping_add(1);
            for (slot, &value) in output.iter().enumerate() {
                let current = i32::from(staged.ter[slot]);
                let selected = if current > value { current } else { value };
                staged.ter[slot] = selected as u8;
            }
        }
    }

    let bytes_after = CityTerrainCensusBytes::read(&staged);
    let pod_after = staged.pod_bytes();
    let city_pod_bytes_changed = pod_before
        .iter()
        .zip(pod_after.iter())
        .filter(|(before, after)| before != after)
        .count() as u32;
    let open_land_for_region = i16::from(staged.land).wrapping_sub(i16::from(staged.filled));
    let leader = CityTerrainLeaderDeltas {
        max_city_pop_candidate: staged.pop,
        gather_slots_for_region: 0,
        flag_2_city_count: u32::from(staged.city_flags & 2 != 0),
        low_open_land_city_count: u32::from(open_land_for_region < 2),
        open_land_for_region,
    };

    let receipt = CityTerrainCensusReceipt {
        constructor_suffix,
        center_wcoord,
        radius_tiles,
        circle_index,
        inner_table_end,
        outer_table_end,
        first_table_index: 1,
        last_table_index: outer_table_end.saturating_sub(1),
        table_entries_scanned,
        in_bounds_entries,
        foreign_owner_rejections,
        admitted_entries,
        inner_entries,
        inner_region_matches,
        water_entries,
        waterhalf_entries,
        placement_calls,
        placement_grades,
        gather_at_calls,
        gather_installed_source: facts.gather.installed_source(),
        dock_tile_calls,
        dock_tile_successes,
        bytes_before,
        bytes_after,
        bordering_before,
        bordering_after: staged.bordering,
        was_capital_flags_before,
        was_capital_flags_after: staged.was_capital_flags,
        leader,
        city_pod_bytes_changed,
        city_terrain_census_complete: true,
        world_writes: 0,
        build_or_registry_writes: 0,
        main_rng_draws: 0,
        first_checksum_city_image_ready: false,
    };
    *city = staged;
    Ok(receipt)
}
