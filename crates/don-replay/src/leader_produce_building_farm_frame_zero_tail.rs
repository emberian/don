//! Atomic frame-zero Farm constructor reached by production builtin 520.
//!
//! The placement search stages two RNG draws and stops immediately before
//! `Objects::init_build`.  This module owns the reached, ordinary Farm continuation: the
//! complete Build scalar image, Object/WData intrusive link, City Build-chain link,
//! `Farms::add`, `Wall::start`, `BuildType::mask_me`, `Wall::activate(0,1,0)`, City's
//! `filled` byte, leader accounting, dense Build publication, and the third RNG draw.
//!
//! Every fallible operation is performed against clones.  Publication is one infallible
//! tail, so an error cannot expose the allocated object, a partially masked footprint, a
//! changed City/Farm owner, or any of the three staged RNG transitions.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::objects::{Band, BANDED_SLOTS, BUILD_BAND_BASE, WALL_BAND_BASE};
use don_sim::rng::Random;
use don_sim::systems::bhs_type_table::{TypeBody, TypeBuiltinState};
use don_sim::systems::canonical_gather_work::FarmStruct;
use don_sim::systems::leader_produce_building_blocked_site_prefix::LeaderProduceBuildingFarmSuccessPreflightReceipt;
use don_sim::systems::leaders::{
    self, ObjectHitInputs, StatObject, WallConstructTimeInputs, WallHitInputs,
};
use don_sim::systems::map_terrain::{tflag, Coord, WCoord};
use don_sim::systems::production::runtime::{LiveProductionRuntime, LiveTypeClass};
use don_sim::systems::production::{self, BuildData, BuildQueue, BuildQueueEntry, Footprint};
use don_sim::tick::Sim;

use crate::build_init_prefix::{
    apply_build_init_prefix, resolve_build_init_prefix_terrain, BuildInitPrefixReceipt,
    BuildInitPrefixTerrainRequest, BuildTypeInitFacts, SourceBackedBuildInitPrefixError,
    BUILD_MAX_AGE_XOR,
};
use crate::builds_runtime::{
    build_walk_value, BuildWalkError, BuildWalkFacts, BuildWalkValue, BuildsWalkAuthority,
};
use crate::terrain_height_runtime::{TerrainHeightAuthority, TerrainTcoordZReceipt};

pub const FARM_TYPE: i32 = 417;
pub const FARM_FOOTPRINT: Footprint = Footprint {
    x_size: 4,
    y_size: 4,
};
pub const FARM_QUEUE_ROWS: usize = 2;
pub const FARM_FULL_HITS: i32 = 400;
pub const FARM_JOB_TIME: i32 = 150;
pub const FARM_BEHIND_HEIGHT: i32 = 20;
pub const FARM_BUILD_FLAGS: u32 = 0x1000_0049;
pub const FARM_ACTIVE_MASK: u16 = 0x1000;
pub const WDATA_BUILD_MASK: u16 = 0x4000;
pub const LEADER_BUILD_INIT_DIRTY: u32 = 0x0200_0000;
pub const LEADER_BUILD_ACTIVE_DIRTY: u32 = 0x0800_0000;
pub const FARMS_ADD_RANDOM_HIGH: i32 = 0xffff;
pub const FARMS_ADD_VA: u32 = 0x008d_8a40;
pub const WALL_ACTIVATE_VA: u32 = 0x0063_e4b0;
pub const BUILD_TYPE_MASK_ME_VA: u32 = 0x0063_12a0;

const UP: usize = 0x2a;
const DOWN: usize = 0x2c;
const DOWN_WHO: usize = 0x2e;
const MY_LOS: usize = 0x3c;

/// Reinstalled scalar owner for retail fields not yet present on canonical `Sim` rows.
///
/// `next_uid` is `ObjectData::uid[owner]`; `buildings_built` is LeaderData `+0x814`.
/// Neither is synthesized from the current Build rows because retired objects make that
/// reconstruction lossy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FarmBuildInitAuthority {
    pub revision: u64,
    pub source_digest: [u8; 32],
    pub next_uid: [u16; BANDED_SLOTS],
    pub buildings_built: [i32; BANDED_SLOTS],
}

/// Content facts behind the reached Farm virtual calls.
///
/// `block_points` is the exact 4x4 byte array behind `BuildTypeData +0x30c`; it is not
/// the unrelated scalar named `block_points` in the flattened TSV row.  The digest binds
/// this pointer payload to the same installed content snapshot as the scalar facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarmFrameZeroSourceFacts {
    pub source_digest: [u8; 32],
    pub block_points: [u8; 16],
    pub full_hits: i32,
    pub stance: i8,
    pub behind_height: i32,
    pub gathers_from_terrain: bool,
    pub can_carry_air: bool,
    /// Type vtable `+0x108` at the post-footprint `mask_me` halo branch.
    pub sets_mountain_bad_path_halo: bool,
    pub sets_flat_flag: bool,
    pub sets_detector_flag: bool,
}

impl FarmFrameZeroSourceFacts {
    pub fn supported_flat_farm(source_digest: [u8; 32], block_points: [u8; 16]) -> Self {
        Self {
            source_digest,
            block_points,
            full_hits: FARM_FULL_HITS,
            stance: 0,
            behind_height: FARM_BEHIND_HEIGHT,
            gathers_from_terrain: false,
            can_carry_air: false,
            sets_mountain_bad_path_halo: false,
            sets_flat_flag: false,
            sets_detector_flag: false,
        }
    }

    fn validates(self) -> bool {
        self.source_digest != [0; 32]
            && self.block_points.iter().all(|&value| value <= 1)
            && self.full_hits == FARM_FULL_HITS
            && self.stance == 0
            && self.behind_height == FARM_BEHIND_HEIGHT
            && !self.gathers_from_terrain
            && !self.can_carry_air
            && !self.sets_mountain_bad_path_halo
            && !self.sets_flat_flag
            && !self.sets_detector_flag
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FarmFrameZeroTailReceipt {
    pub owner: u8,
    pub row: usize,
    pub object_id: i16,
    pub city_slot: usize,
    pub farm_index: usize,
    pub prefix: BuildInitPrefixReceipt,
    pub terrain_z: TerrainTcoordZReceipt,
    pub footprint_corner: [i32; 2],
    pub blocked_tiles: u32,
    pub behind_tiles: u32,
    pub farm_random_value: i32,
    pub farm_type: u8,
    pub random_state_before: i32,
    pub random_state_after_search: i32,
    pub random_state_after_farm: i32,
    pub construct_time: u32,
    pub final_flags: u8,
    pub final_build_mask: u16,
    pub final_seen: (u8, u8),
    pub city_filled_before: u8,
    pub city_filled_after: u8,
    pub authority_revision_before: u64,
    pub authority_revision_after: u64,
    pub walk: BuildWalkValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FarmFrameZeroTailError {
    InvalidPreflight,
    InvalidSourceFacts,
    StaleAuthority { expected: u64, actual: u64 },
    SourceDigestMismatch,
    InvalidCanonicalOwner(&'static str),
    UnsupportedFarmProfile,
    UnsupportedTerrainProfile,
    FarmCapacity,
    Terrain(SourceBackedBuildInitPrefixError),
    Prefix(crate::build_init_prefix::BuildInitPrefixError),
    Walk(BuildWalkError),
}

impl fmt::Display for FarmFrameZeroTailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame-zero Farm tail refused: {self:?}")
    }
}

impl std::error::Error for FarmFrameZeroTailError {}

fn write_i16(bytes: &mut [u8], at: usize, value: i16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn validate_dense_builds(sim: &Sim, production: &LiveProductionRuntime) -> bool {
    if !sim.world.object_bands_are_dense_equivalent() {
        return false;
    }
    let mut seen = vec![false; sim.builds.len()];
    for owner in 0..BANDED_SLOTS {
        for (slot, &row_u32) in sim
            .world
            .objects
            .slot(owner)
            .band(Band::Build)
            .iter()
            .enumerate()
        {
            let row = row_u32 as usize;
            let Some(build) = sim.builds.get(row) else {
                return false;
            };
            if std::mem::replace(&mut seen[row], true)
                || build.who as usize != owner
                || i32::from(build.object_id()) != (BUILD_BAND_BASE + slot as u32) as i32
                || production
                    .build_types
                    .get(row)
                    .and_then(|value| *value)
                    .is_none()
            {
                return false;
            }
        }
    }
    seen.into_iter().all(|value| value)
}

fn build_row(sim: &Sim, owner: usize, object_id: i16) -> Option<usize> {
    let slot = i32::from(object_id).checked_sub(BUILD_BAND_BASE as i32)? as usize;
    sim.world
        .objects
        .slot(owner)
        .band(Band::Build)
        .get(slot)
        .copied()
        .map(|row| row as usize)
}

fn flat_zero_height_plane(
    world: &don_sim::systems::map_terrain::World,
    terrain: &TerrainHeightAuthority,
) -> bool {
    let Some(width) = usize::try_from(world.tile_xs)
        .ok()
        .and_then(|value| value.checked_add(1))
    else {
        return false;
    };
    let Some(height) = usize::try_from(world.tile_ys)
        .ok()
        .and_then(|value| value.checked_add(1))
    else {
        return false;
    };
    width.checked_mul(height).is_some_and(|length| {
        terrain.master_land_height_bits.len() == length
            && terrain
                .master_land_height_bits
                .iter()
                .all(|&bits| bits == 0)
            && terrain.land_height_bits == 0
    })
}

fn mark_behind(
    world: &mut don_sim::systems::map_terrain::World,
    corner: [i32; 2],
    behind_height: i32,
) -> u32 {
    let mut changed = 0u32;
    for layer in 1..=behind_height {
        let y = corner[1].wrapping_sub(layer);
        for x in corner[0].wrapping_sub(layer)
            ..corner[0]
                .wrapping_add(FARM_FOOTPRINT.x_size)
                .wrapping_sub(layer)
        {
            if world.valid_t(x, y) {
                let index = world.t_index(x, y);
                let before = world.tdata[index];
                world.tdata[index] |= tflag::BEHIND_A;
                changed += u32::from(before != world.tdata[index]);
            }
        }
        let x = corner[0].wrapping_sub(layer);
        for y in corner[1].wrapping_sub(layer).wrapping_add(1)
            ..corner[1]
                .wrapping_add(FARM_FOOTPRINT.y_size)
                .wrapping_sub(layer)
        {
            if world.valid_t(x, y) {
                let index = world.t_index(x, y);
                let before = world.tdata[index];
                world.tdata[index] |= tflag::BEHIND_A;
                changed += u32::from(before != world.tdata[index]);
            }
        }
    }
    changed
}

/// Complete the exact Farm tail reached by one successful production-520 preflight.
///
/// This is deliberately not wired into the VM builtin yet: the VM call remains wholly
/// rollback-red until its preceding cost transaction and this receipt can share one outer
/// publication boundary.
#[allow(clippy::too_many_arguments)]
pub fn complete_leader_produce_building_farm_frame_zero_tail(
    sim: &mut Sim,
    production: &mut LiveProductionRuntime,
    types: &TypeBuiltinState,
    terrain: &TerrainHeightAuthority,
    build_authority: &mut FarmBuildInitAuthority,
    builds_walk: &mut BuildsWalkAuthority,
    expected_authority_revision: u64,
    source: FarmFrameZeroSourceFacts,
    preflight: &LeaderProduceBuildingFarmSuccessPreflightReceipt,
) -> Result<FarmFrameZeroTailReceipt, FarmFrameZeroTailError> {
    if !preflight.validates()
        || preflight.continuation.type_index != FARM_TYPE
        || preflight.continuation.fifth_argument != 0
        || preflight.continuation.sixth_argument != -1
    {
        return Err(FarmFrameZeroTailError::InvalidPreflight);
    }
    if !source.validates() {
        return Err(FarmFrameZeroTailError::InvalidSourceFacts);
    }
    if build_authority.revision != expected_authority_revision {
        return Err(FarmFrameZeroTailError::StaleAuthority {
            expected: expected_authority_revision,
            actual: build_authority.revision,
        });
    }
    if build_authority.source_digest != source.source_digest {
        return Err(FarmFrameZeroTailError::SourceDigestMismatch);
    }

    let owner = preflight.continuation.owner as usize;
    if owner >= BANDED_SLOTS
        || !validate_dense_builds(sim, production)
        || preflight.continuation.expected_build_row != sim.builds.len()
        || preflight.continuation.expected_object_id as u32
            != sim.world.objects.slot(owner).mark(Band::Build)
        || sim.world.objects.slot(owner).mark(Band::Build) >= WALL_BAND_BASE
        || production
            .build_types
            .get(sim.builds.len())
            .is_some_and(Option::is_some)
    {
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "dense Build allocation boundary",
        ));
    }
    if sim.world.random.state() != preflight.coarse_random.state_before {
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "stale staged RNG input",
        ));
    }

    let farm_live = production
        .types
        .get(FARM_TYPE as usize)
        .and_then(Option::as_ref)
        .ok_or(FarmFrameZeroTailError::UnsupportedFarmProfile)?;
    let visibility = farm_live
        .build_visibility
        .ok_or(FarmFrameZeroTailError::UnsupportedFarmProfile)?;
    let farm_row = types
        .types
        .rows()
        .get(FARM_TYPE as usize)
        .ok_or(FarmFrameZeroTailError::UnsupportedFarmProfile)?;
    let canonical_farm_row = farm_row.index == FARM_TYPE
        && farm_row.is_list.as_slice() == [FARM_TYPE as u16]
        && matches!(&farm_row.body, TypeBody::Build { object, .. } if object.attack == 0);
    if farm_live.type_index != FARM_TYPE
        || farm_live.class != LiveTypeClass::Building
        || farm_live.train_time != FARM_JOB_TIME
        || farm_live.object_masks != 0
        || farm_live.stance_type != 0
        || farm_live.build_flags != FARM_BUILD_FLAGS
        || visibility.domain != Some(0)
        || visibility.footprint != Some(FARM_FOOTPRINT)
        || !canonical_farm_row
    {
        return Err(FarmFrameZeroTailError::UnsupportedFarmProfile);
    }

    let city_slot = preflight.input.town.city_slot;
    let city = sim
        .cities
        .slots
        .get(owner)
        .and_then(|rows| rows.get(city_slot))
        .ok_or(FarmFrameZeroTailError::InvalidCanonicalOwner("City slot"))?;
    let origin_row = build_row(sim, owner, preflight.input.town.city_object).ok_or(
        FarmFrameZeroTailError::InvalidCanonicalOwner("City center registry"),
    )?;
    let origin = &sim.builds[origin_row];
    if !city.active()
        || city.who != owner as i8
        || city.city as usize != city_slot
        || city.o != preflight.input.town.city_object
        || origin.object_id() != city.o
        || origin.position() != (city.x, city.y)
        || origin.who as usize != owner
        || !origin.is_active()
        || origin.flags & production::flag::CAPTURED == 0
        || origin.city as usize != city_slot
        || origin.city_down != -1
        || production
            .build_types
            .get(origin_row)
            .and_then(|value| *value)
            != Some(preflight.input.town.center_type)
        || sim.cities.count(owner) != 1
        || sim.step8.leaders[owner].city_num != 1
        || sim.step8.leaders[owner].econ.age_alt != sim.leaders[owner].econ.age_alt
    {
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "live City/center Build join",
        ));
    }
    if sim.farms.records().len() as i32 != preflight.input.town.counted_farms
        || !sim.farms.records().is_empty()
    {
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "first reached Farm record",
        ));
    }
    if sim.world.live_count() != 0 {
        // Wall::init's nearby-Unit Z normalization is a separate mutation cone. The
        // installed production witness has no live Unit rows, so it is exactly a no-op.
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "frame-zero Farm requires the witnessed empty Unit band",
        ));
    }

    if !flat_zero_height_plane(&sim.map.world, terrain) {
        return Err(FarmFrameZeroTailError::UnsupportedTerrainProfile);
    }
    let position = preflight.selected_placement_coord;
    let corner_pair = FARM_FOOTPRINT.tile_corner(position[0], position[1]);
    let corner = [corner_pair.0, corner_pair.1];
    if corner != preflight.input.input.entry.footprint_corner {
        return Err(FarmFrameZeroTailError::InvalidPreflight);
    }
    let wx = WCoord::from_coord(Coord(position[0])).0;
    let wy = WCoord::from_coord(Coord(position[1])).0;
    if !sim.map.world.valid_w(wx, wy) {
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "Farm WData center",
        ));
    }
    let center_index = sim.map.world.w_index(wx, wy);
    let center = &sim.map.world.wdata[center_index];
    if center.who != owner as i8 || center.down != -1 || center.region < 64 {
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "Farm WData head/territory",
        ));
    }
    for dx in 0..FARM_FOOTPRINT.x_size {
        for dy in 0..FARM_FOOTPRINT.y_size {
            let tx = corner[0] + dx;
            let ty = corner[1] + dy;
            if !sim.map.world.valid_t(tx, ty) {
                return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
                    "Farm footprint bounds",
                ));
            }
            let mask = sim.map.world.tdata[sim.map.world.t_index(tx, ty)];
            if mask & (tflag::BLOCKER_MASK | tflag::BLOCKED | tflag::STARTED | tflag::STARTED2) != 0
            {
                return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
                    "Farm footprint changed after placement preflight",
                ));
            }
        }
    }
    if sim.vic_leaders.slots[owner].leader_flags as u32 != sim.step8.leaders[owner].flags
        || sim.vic_leaders.slots[owner].num_buildings.len() <= (FARM_TYPE - 414) as usize
        || sim.vic_leaders.slots[owner].num_queued.len() <= FARM_TYPE as usize
        || sim.vic_leaders.slots[owner].num_queued[FARM_TYPE as usize] != 0
    {
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "Leader activation mirrors/counters",
        ));
    }

    // From here onward every state owner is cloned. No canonical write occurs until all
    // constructor, Farm capacity, terrain and Build-walk gates have succeeded.
    let object_id = preflight.continuation.expected_object_id;
    let row = preflight.continuation.expected_build_row;
    let mut staged_build = BuildData::default();
    let terrain_request = BuildInitPrefixTerrainRequest {
        owner: owner as u8,
        object_id,
        type_index: FARM_TYPE,
        type_rows: types.types.rows().len(),
        snapped_x: position[0],
        snapped_y: position[1],
        owner_uid_before: build_authority.next_uid[owner],
        max_age_source_byte: (sim.step8.leaders[owner].econ.age_alt as u8) ^ BUILD_MAX_AGE_XOR,
        type_facts: BuildTypeInitFacts {
            sets_flat_flag: source.sets_flat_flag,
            sets_detector_flag: source.sets_detector_flag,
        },
    };
    let (prefix_request, terrain_z) =
        resolve_build_init_prefix_terrain(terrain_request, &sim.map.world, terrain)
            .map_err(FarmFrameZeroTailError::Terrain)?;
    let prefix = apply_build_init_prefix(&mut staged_build, prefix_request)
        .map_err(FarmFrameZeroTailError::Prefix)?;

    let mut construct = StatObject {
        wall_construct_time_inputs: Some(WallConstructTimeInputs {
            type_time: (FARM_JOB_TIME as u32).wrapping_mul(100),
            is_wonder: false,
            is_airdefense: false,
            is_fort: false,
        }),
        ..StatObject::default()
    };
    let construct_time = leaders::wall_update_construct_time(
        &mut construct,
        &sim.step8.leaders[owner],
        &sim.step8_rules,
    )
    .ok_or(FarmFrameZeroTailError::UnsupportedFarmProfile)?;
    if construct_time != 15_000 {
        return Err(FarmFrameZeroTailError::UnsupportedFarmProfile);
    }

    let mut wall_hits = StatObject {
        hit_inputs: Some(ObjectHitInputs {
            base_hits: source.full_hits,
            ..ObjectHitInputs::default()
        }),
        wall_hit_inputs: Some(WallHitInputs::default()),
        wall_active: false,
        wall_city_flag: false,
        job_counter: 0,
        constr_time: construct_time,
        inside_down: -1,
        ..StatObject::default()
    };
    let wall_hit_outcome = leaders::wall_update_hits(&mut wall_hits, &sim.step8_rules, false)
        .ok_or(FarmFrameZeroTailError::UnsupportedFarmProfile)?;
    if wall_hits.myhits != source.full_hits
        || wall_hits.construct_hits != 1
        || wall_hit_outcome.returned_hits != 1
        || wall_hit_outcome.eject_contents
    {
        return Err(FarmFrameZeroTailError::UnsupportedFarmProfile);
    }

    let mut staged_world = sim.map.world.clone();
    let center_before = staged_world.wdata[center_index].clone();
    write_i16(&mut staged_build.other, UP, -1);
    write_i16(&mut staged_build.other, DOWN, center_before.down);
    write_i16(&mut staged_build.other, DOWN_WHO, center_before.down_who);
    staged_world.wdata[center_index].down = object_id;
    staged_world.wdata[center_index].down_who = owner as i16;

    // Build::init + Wall::init scalar tail.
    staged_build.flags &= 0xf9;
    staged_build.recharging = 0;
    staged_build.infiltrate = 0;
    staged_build.infiltrate2 = 0;
    staged_build.attack_whom = -1;
    staged_build.gather_down = -1;
    staged_build.city = city_slot as i16;
    staged_build.city_down = -1;
    staged_build.attack_ox = -1;
    staged_build.gather_max = 0;
    staged_build.stance = source.stance;
    staged_build.queue = BuildQueue {
        queued: 0,
        entries: vec![
            BuildQueueEntry {
                type_index: -1,
                ..BuildQueueEntry::default()
            },
            BuildQueueEntry::default(),
        ],
    };
    staged_build.gather_from.tiles.clear();
    staged_build.gather_from.mtn = -1;
    staged_build.gather_from.cliff = -1;
    staged_build.gather.clear();
    staged_build.build_masks = 0;
    staged_build.job_counter = 0;
    staged_build.job_counter_2 = 0;
    staged_build.helpers = 0;
    staged_build.demolition = 0;
    staged_build.gpiece = -1;
    staged_build.frame_started = -1;
    staged_build.constr_time = construct_time;
    staged_build.myhits = wall_hits.myhits;
    staged_build.construct_hits = wall_hits.construct_hits;
    staged_build.other[MY_LOS] = 0;
    staged_build.ever_seen = 0;
    staged_build.ever_seen_completed = 0;

    let mut staged_origin = origin.clone();
    staged_origin.city_down = object_id;

    let mut staged_random = Random::new(preflight.staged_random_state_after);
    let farm_random_value = staged_random.get(0, FARMS_ADD_RANDOM_HIGH);
    let farm_type = u8::from(farm_random_value % 4 > 2);
    let random_state_after_farm = staged_random.state();
    let mut staged_farms = sim.farms.clone();
    let farm_index = staged_farms
        .push(FarmStruct {
            who: owner as i32,
            o: i32::from(object_id),
            terrain_height: [0; 25],
            valid: 1,
            farm_type,
            ..FarmStruct::default()
        })
        .map_err(|_| FarmFrameZeroTailError::FarmCapacity)?;
    staged_build.dock = farm_index as i16;

    // Wall::activate writes completion visibility before its defensive Wall::start.
    let owner_bit = 1u8 << owner;
    let completion_seen_mask = sim.vic_leaders.slots[owner].init_diplomacy.ally_mask;
    if completion_seen_mask & owner_bit == 0 {
        return Err(FarmFrameZeroTailError::InvalidCanonicalOwner(
            "completion visibility ally mask",
        ));
    }
    staged_build.ever_seen_completed |= completion_seen_mask;
    staged_build.flags |= production::flag::STARTED;
    staged_build.frame_started = sim.world.frame;
    staged_world.wdata[center_index].flags |= WDATA_BUILD_MASK;
    let mut blocked_tiles = 0u32;
    for dx in 0..FARM_FOOTPRINT.x_size {
        for dy in 0..FARM_FOOTPRINT.y_size {
            let tx = corner[0] + dx;
            let ty = corner[1] + dy;
            let ti = staged_world.t_index(tx, ty);
            staged_world.tdata[ti] &= !(tflag::STARTED | tflag::STARTED2);
            staged_world.tdata[ti] =
                (staged_world.tdata[ti] & !tflag::BLOCKER_MASK) | tflag::BLOCKER_BUILDING;
            // Farm's build_flags & 0x40/type-relation arm clears the road before the
            // separate block-point byte decides whether collision is installed.
            staged_world.set_road_at(tx, ty, false, 0, false);
            let source_index = (dx * FARM_FOOTPRINT.y_size + dy) as usize;
            let blocked = source.block_points[source_index] == 1;
            staged_world.set_blocked_at(tx, ty, blocked);
            blocked_tiles += u32::from(blocked);

            // Wall::start coalesces these calls by half cell. OR is idempotent, so the
            // canonical after-image is identical when written directly per fine tile.
            let wi = staged_world.w_index(tx >> 2, ty >> 2);
            staged_world.wdata[wi].was_seen |= owner_bit;
            let fi = staged_world.f_index(tx >> 1, ty >> 1);
            staged_world.seen2[fi] |= owner_bit;
        }
    }
    staged_build.ever_seen = owner_bit;
    let behind_tiles = mark_behind(&mut staged_world, corner, source.behind_height);

    staged_build.flags |= production::flag::ACTIVE;
    staged_build.build_masks |= FARM_ACTIVE_MASK;
    staged_build.job_counter = 0;
    staged_build.job_counter_2 = 0;

    let mut staged_cities = sim.cities.clone();
    let city_filled_before = staged_cities.slots[owner][city_slot].filled;
    staged_cities.slots[owner][city_slot].filled = city_filled_before.wrapping_add(1);
    let city_filled_after = staged_cities.slots[owner][city_slot].filled;

    let mut staged_build_authority = build_authority.clone();
    staged_build_authority.next_uid[owner] = staged_build_authority.next_uid[owner].wrapping_add(1);
    staged_build_authority.buildings_built[owner] =
        staged_build_authority.buildings_built[owner].wrapping_add(1);
    staged_build_authority.revision = staged_build_authority.revision.wrapping_add(1);

    let leader_flags_after =
        sim.step8.leaders[owner].flags | LEADER_BUILD_INIT_DIRTY | LEADER_BUILD_ACTIVE_DIRTY;
    let building_slot = (FARM_TYPE - 414) as usize;
    // Wall::increment_stats also has a region-local branch, but it is guarded by
    // WData::region < 64. The reached production witness is the sentinel region 64,
    // validated above, so the aggregate building counter is the sole stats mutation.
    let building_count_after =
        sim.vic_leaders.slots[owner].num_buildings[building_slot].wrapping_add(1);

    let walk_facts = BuildWalkFacts::default();
    let walk = build_walk_value(&staged_build, FARM_TYPE, &walk_facts)
        .map_err(FarmFrameZeroTailError::Walk)?;
    let mut staged_walk = builds_walk.clone();
    staged_walk.install(row, walk_facts);

    // Single publication tail. Every operation below is an infallible assignment or the
    // preflighted dense append.
    sim.builds[origin_row] = staged_origin;
    sim.map.world = staged_world;
    sim.farms = staged_farms;
    sim.cities = staged_cities;
    sim.step8.leaders[owner].flags = leader_flags_after;
    sim.vic_leaders.slots[owner].leader_flags = leader_flags_after as i32;
    sim.vic_leaders.slots[owner].num_buildings[building_slot] = building_count_after;
    let committed_row = sim.spawn_build(owner, staged_build);
    assert_eq!(committed_row, row, "preflighted Farm row changed");
    production.register_build(row, FARM_TYPE);
    sim.world.random.reseed(random_state_after_farm);
    *build_authority = staged_build_authority;
    *builds_walk = staged_walk;

    Ok(FarmFrameZeroTailReceipt {
        owner: owner as u8,
        row,
        object_id,
        city_slot,
        farm_index,
        prefix,
        terrain_z,
        footprint_corner: corner,
        blocked_tiles,
        behind_tiles,
        farm_random_value,
        farm_type,
        random_state_before: preflight.coarse_random.state_before,
        random_state_after_search: preflight.staged_random_state_after,
        random_state_after_farm,
        construct_time,
        final_flags: sim.builds[row].flags,
        final_build_mask: sim.builds[row].build_masks,
        final_seen: (
            sim.builds[row].ever_seen,
            sim.builds[row].ever_seen_completed,
        ),
        city_filled_before,
        city_filled_after,
        authority_revision_before: expected_authority_revision,
        authority_revision_after: build_authority.revision,
        walk,
    })
}
