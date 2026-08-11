//! Atomic completion of one ordinary starting-Village Build row.
//!
//! The earlier setup owner reserves a canonical `Sim::builds` row and constructs the
//! linked City, but deliberately leaves the inherited `Build::init` / `Build::activate`
//! body red.  This module closes the checksum-visible Build row without creating a second
//! object pool.  It replays the scalar initializer prefix into a staged copy, installs the
//! empty-cell `Object::add_to_world` link, applies the source-owned final blocker bits,
//! completes the Village queue and activation bytes, and normalizes the row through the
//! first active `Build::process` pass.  Only after the 491-byte walk succeeds are the
//! canonical row, World, and `BuildsWalkAuthority` committed.
//!
//! This is intentionally not a complete World producer. Terrain height changes, the
//! activation-side CITY disc, content mask's blocked counters, city roads, behind masks,
//! visibility planes, and presentation/Leader side effects remain named in
//! [`WORLD_RESIDUALS`]. The exact `Wall::mask_city` input is emitted as a typed request
//! for the separate World transaction; this owner never stamps TData CITY itself.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::objects::{Band, BANDED_SLOTS, BUILD_BAND_BASE};
use don_sim::systems::map_terrain::{tflag, Coord, WCoord};
use don_sim::systems::production::{self, BuildQueue, BuildQueueEntry, Footprint};
use don_sim::systems::tech_cities::{self, CityRules};
use don_sim::tick::Sim;

use crate::build_init_prefix::{
    apply_build_init_prefix, BuildInitPrefixError, BuildInitPrefixReceipt, BuildInitPrefixRequest,
};
use crate::builds_runtime::{
    build_walk_value, BuildWalkError, BuildWalkFacts, BuildWalkValue, BuildsWalkAuthority,
};
use crate::city_build_constructor_runtime::FreshStartingVillageReceipt;

pub const BUILD_INIT_VA: u32 = 0x0062_9740;
pub const OBJECT_ADD_TO_WORLD_VA: u32 = 0x0064_d8c0;
pub const WALL_INIT_VA: u32 = 0x0063_e9b0;
pub const BUILD_QUEUE_INIT_VA: u32 = 0x0063_07d0;
pub const WALL_START_VA: u32 = 0x0063_e810;
pub const WALL_MASK_ME_VA: u32 = 0x0064_2fc0;
pub const BUILD_TYPE_MASK_ME_VA: u32 = 0x0063_12a0;
pub const WALL_MASK_CITY_VA: u32 = 0x0063_e310;
pub const WALL_ACTIVATE_VA: u32 = 0x0063_e4b0;
pub const BUILD_ACTIVATE_VA: u32 = 0x0062_3e20;
pub const OBJECT_UPDATE_SEEN_VA: u32 = 0x0065_1b80;
pub const OBJECT_UPDATE_SEEN_ALLY_VA: u32 = 0x0065_37a0;
pub const LEADER_PROCESS_ALL_VA: u32 = 0x006e_d2a0;
pub const WALL_UPDATE_HITS_VA: u32 = 0x0063_f0d0;
pub const WALL_UPDATE_LOS_VA: u32 = 0x0063_eeb0;
pub const BUILD_PROCESS_VA: u32 = 0x0061_edf0;
pub const BUILD_DATA_WALK_VA: u32 = 0x0062_f270;

pub const STARTING_VILLAGE_TYPE: i32 = tech_cities::ty::VILLAGE;
pub const STARTING_VILLAGE_QUEUE_ROWS: usize = 20;
pub const STARTING_VILLAGE_FOOTPRINT: Footprint = Footprint {
    x_size: 7,
    y_size: 7,
};
pub const STARTING_VILLAGE_BUILD_MASK: u16 = 0x1000;
pub const STARTING_VILLAGE_NATIVE_MASK_CITY_FLAGS: u8 =
    production::flag::VALID | production::flag::STARTED | 0x20;
pub const STARTING_VILLAGE_FINAL_FLAGS: u8 =
    production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE | 0x20;
pub const STARTING_VILLAGE_WALK_BYTES: u64 = 491;
/// Set by `BuildType::mask_me` on the center WData row.  This bit is outside the currently
/// named low WData flag set, but the `or word [wdata],0x4000` writer is instruction-exact.
pub const WDATA_BUILD_MASK: u16 = 0x4000;

const UP: usize = 0x2a;
const DOWN: usize = 0x2c;
const DOWN_WHO: usize = 0x2e;
const MY_LOS: usize = 0x3c;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingBuildStage {
    CanonicalOwnerValidated,
    BeforeObjectAddToWorld,
    ObjectAddedToEmptyWorldCell,
    WallInitComplete,
    BuildInitComplete,
    WallStartComplete,
    WallMaskCityNativeCallAttested,
    WallActivateComplete,
    BuildActivateCityJoinComplete,
    SetupVisibilityComplete,
    FrameZeroWallStatsComplete,
    FirstActiveBuildProcessComplete,
    BuildWalkComplete,
}

pub const STARTING_BUILD_STAGE_ORDER: [StartingBuildStage; 13] = [
    StartingBuildStage::CanonicalOwnerValidated,
    StartingBuildStage::BeforeObjectAddToWorld,
    StartingBuildStage::ObjectAddedToEmptyWorldCell,
    StartingBuildStage::WallInitComplete,
    StartingBuildStage::BuildInitComplete,
    StartingBuildStage::WallStartComplete,
    StartingBuildStage::WallMaskCityNativeCallAttested,
    StartingBuildStage::WallActivateComplete,
    StartingBuildStage::BuildActivateCityJoinComplete,
    StartingBuildStage::SetupVisibilityComplete,
    StartingBuildStage::FrameZeroWallStatsComplete,
    StartingBuildStage::FirstActiveBuildProcessComplete,
    StartingBuildStage::BuildWalkComplete,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingBuildWorldResidual {
    TerrainTerraformAndHeightRefresh,
    ActivationCityMaskTransaction,
    ContentFootprintBlockedCounters,
    CityPerimeterRoads,
    BehindMask,
    VisibilityPlanesAndRevealCallbacks,
    CollisionAndGraphicsCallbacks,
    LeaderAndGlobalStatistics,
}

pub const WORLD_RESIDUALS: [StartingBuildWorldResidual; 8] = [
    StartingBuildWorldResidual::TerrainTerraformAndHeightRefresh,
    StartingBuildWorldResidual::ActivationCityMaskTransaction,
    StartingBuildWorldResidual::ContentFootprintBlockedCounters,
    StartingBuildWorldResidual::CityPerimeterRoads,
    StartingBuildWorldResidual::BehindMask,
    StartingBuildWorldResidual::VisibilityPlanesAndRevealCallbacks,
    StartingBuildWorldResidual::CollisionAndGraphicsCallbacks,
    StartingBuildWorldResidual::LeaderAndGlobalStatistics,
];

/// Values read from shipped type/rule/Leader/visibility owners.  They are final scalar
/// results, not replay checksum words.  Keeping the Wall-init and first-frame values apart
/// preserves the actual temporal boundary: frame-zero `Leader::process_all` can change
/// hits and LOS after setup, and `Build::process` then copies the active full-hit value to
/// `construct_hits`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingVillageBuildSourceFacts {
    pub wall_init_full_hits: i32,
    pub first_checkpoint_full_hits: i32,
    pub construct_time: u32,
    pub first_checkpoint_los: u8,
    pub stance: i8,
    pub wall_init_ever_seen: u8,
    pub wall_init_ever_seen_completed: u8,
    pub first_checkpoint_ever_seen: u8,
    pub first_checkpoint_ever_seen_completed: u8,
    /// `Game::frame` stored by `Wall::start`; ordinary setup is frame zero.
    pub setup_game_frame: i32,
    /// At least one active Build pass must precede the first checkpoint.
    pub active_build_process_frames: u32,
    /// `BuildTypeData::can_carry(DOMAIN_AIR)` at the tail of `Build::init`.
    pub can_carry_air: bool,
    /// Both gather predicates which guard `Build::find_gather_tiles` are false for Village.
    pub gathers_from_terrain: bool,
    /// `LeaderData::has_tribe_bonus(0x15)`, which extends the CITY mask by four tiles.
    pub indian_radius_bonus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingBuildObjectLinkReceipt {
    pub center_wcoord: (i32, i32),
    pub wdata_head_before: (i16, i16),
    pub wdata_head_after: (i16, i16),
    pub object_up: i16,
    pub object_down: i16,
    pub object_down_who: i16,
}

/// Source-attested native arguments plus the post-activation normalized Build evidence
/// handed to the separate World transaction.
///
/// The World owner must validate this request against the canonical Build identity before
/// executing the ordered even-circle TData transaction. This Build owner deliberately
/// does not mutate TData CITY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallMaskCityRequest {
    pub center_tcoord: (i32, i32),
    pub radius_tiles: i32,
    /// Exact retail call argument. Starting-city activation passes one.
    pub on: i32,
    pub owner: u8,
    pub object_id: i16,
    /// Exact native `Wall::mask_city` call-boundary flags inside `Wall::start`.
    pub native_call_flags: u8,
    /// Exact native City link at that call boundary; City activation has not linked it yet.
    pub native_call_city: i16,
    /// Normalized flags after `Wall::activate`, carried as later join evidence.
    pub post_activation_flags: u8,
}

impl WallMaskCityRequest {
    pub fn city_flag_set(&self) -> bool {
        self.native_call_flags & 0x20 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingBuildFootprintReceipt {
    pub corner_tcoord: (i32, i32),
    pub footprint_tiles: u32,
    pub center_wdata_build_mask_before: bool,
    pub center_wdata_build_mask_after: bool,
    /// The exact final blocker kind is owned. CITY is represented only by
    /// [`WallMaskCityRequest`] and remains a World residual.
    pub blocker_kind_complete: bool,
    pub world_channel_ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingBuildQueueReceipt {
    pub rows: usize,
    pub logical_queued: u8,
    pub first_type: i16,
    pub remaining_rows_zeroed: usize,
    pub walked_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingBuildMiningReceipt {
    pub length: i32,
    pub constructor_capacity: i32,
    pub increment: i16,
    pub flags: u8,
    pub mtn: i8,
    pub cliff: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartingVillageBuildActivationReceipt {
    pub row: usize,
    pub owner: u8,
    pub object_id: i16,
    pub current_type: i32,
    pub city_slot: i16,
    pub prefix: BuildInitPrefixReceipt,
    pub stages: [StartingBuildStage; 13],
    pub object_link: StartingBuildObjectLinkReceipt,
    pub footprint: StartingBuildFootprintReceipt,
    pub wall_mask_city: WallMaskCityRequest,
    pub queue: StartingBuildQueueReceipt,
    pub mining: StartingBuildMiningReceipt,
    pub wall_init_construct_hits: i32,
    pub first_checkpoint_construct_hits: i32,
    pub final_flags: u8,
    pub final_build_mask: u16,
    pub final_seen: (u8, u8),
    pub walk_facts: BuildWalkFacts,
    pub walk: BuildWalkValue,
    pub build_init_complete: bool,
    pub build_activation_complete: bool,
    pub first_checkpoint_build_image_ready: bool,
    pub build_walk_authority_installed: bool,
    pub world_channel_ready: bool,
    pub world_residuals: [StartingBuildWorldResidual; 8],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingVillageBuildActivationError {
    RowOutsideBuildPool {
        row: usize,
        rows: usize,
    },
    RegistryNotDenseEquivalent,
    RegistryIdentityMissing {
        row: usize,
    },
    RegistryIdentityDuplicate {
        row: usize,
    },
    OwnerOutsidePlayableRange {
        owner: u8,
    },
    BuildOwnerMismatch {
        expected: u8,
        actual: u8,
    },
    BuildObjectMismatch {
        expected: i16,
        actual: i16,
    },
    MissingCurrentType {
        row: usize,
    },
    WrongCurrentType {
        expected: i32,
        actual: i32,
    },
    ConstructorReceiptIncomplete,
    ConstructorJoinMismatch,
    WrongConstructorState {
        flags: u8,
        city: i16,
    },
    PrefixJoinMismatch,
    Prefix(BuildInitPrefixError),
    InvalidSourceFacts,
    MissingOwnerVisibility {
        owner_bit: u8,
        ever_seen: u8,
        ever_seen_completed: u8,
    },
    InvalidWorldShape,
    CenterOutsideWorld {
        wx: i32,
        wy: i32,
    },
    CenterWorldCellAlreadyLinked {
        down: i16,
        down_who: i16,
    },
    FootprintOutsideWorld {
        tx: i32,
        ty: i32,
    },
    Walk(BuildWalkError),
    WalkLengthMismatch {
        expected: u64,
        actual: u64,
    },
}

impl fmt::Display for StartingVillageBuildActivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting Village Build activation refused: {self:?}")
    }
}

impl std::error::Error for StartingVillageBuildActivationError {}

/// Complete and authorize one already-reserved, already-City-linked starting Village.
///
/// All fallible work targets staged clones.  No `Err` path changes the `Sim`, World, or
/// prior walk authority.  The World commit includes only the instruction-owned subset
/// described by [`StartingBuildFootprintReceipt`]; the residual ledger remains red.
pub fn complete_starting_village_build(
    sim: &mut Sim,
    authority: &mut BuildsWalkAuthority,
    row: usize,
    constructor: &FreshStartingVillageReceipt,
    prefix_request: BuildInitPrefixRequest,
    facts: StartingVillageBuildSourceFacts,
) -> Result<StartingVillageBuildActivationReceipt, StartingVillageBuildActivationError> {
    let current =
        sim.builds
            .get(row)
            .ok_or(StartingVillageBuildActivationError::RowOutsideBuildPool {
                row,
                rows: sim.builds.len(),
            })?;
    if !sim.world.object_bands_are_dense_equivalent() {
        return Err(StartingVillageBuildActivationError::RegistryNotDenseEquivalent);
    }

    let mut identity = None;
    for owner in 0..BANDED_SLOTS {
        for (slot, &candidate_row) in sim
            .world
            .objects
            .slot(owner)
            .band(Band::Build)
            .iter()
            .enumerate()
        {
            if candidate_row as usize != row {
                continue;
            }
            let candidate = (owner as u8, (BUILD_BAND_BASE + slot as u32) as i16);
            if identity.replace(candidate).is_some() {
                return Err(StartingVillageBuildActivationError::RegistryIdentityDuplicate { row });
            }
        }
    }
    let (owner, object_id) =
        identity.ok_or(StartingVillageBuildActivationError::RegistryIdentityMissing { row })?;
    if owner as usize >= BANDED_SLOTS {
        return Err(StartingVillageBuildActivationError::OwnerOutsidePlayableRange { owner });
    }
    if current.who != owner {
        return Err(StartingVillageBuildActivationError::BuildOwnerMismatch {
            expected: owner,
            actual: current.who,
        });
    }
    if current.object_id() != object_id {
        return Err(StartingVillageBuildActivationError::BuildObjectMismatch {
            expected: object_id,
            actual: current.object_id(),
        });
    }
    let current_type = sim
        .production_runtime
        .build_types
        .get(row)
        .and_then(|value| *value)
        .ok_or(StartingVillageBuildActivationError::MissingCurrentType { row })?;
    if current_type != STARTING_VILLAGE_TYPE {
        return Err(StartingVillageBuildActivationError::WrongCurrentType {
            expected: STARTING_VILLAGE_TYPE,
            actual: current_type,
        });
    }
    if !constructor.city_init_complete
        || constructor.build_init_complete
        || constructor.build_activation_complete
        || constructor.builds_channel_ready
    {
        return Err(StartingVillageBuildActivationError::ConstructorReceiptIncomplete);
    }
    if constructor.object_id != object_id
        || constructor.current_type != current_type
        || constructor.position != current.position()
        || constructor.city_link_after != current.city
        || constructor.city.o != object_id
        || constructor.city.city != current.city
        || constructor.city.who != owner as i8
    {
        return Err(StartingVillageBuildActivationError::ConstructorJoinMismatch);
    }
    if current.flags != STARTING_VILLAGE_FINAL_FLAGS || current.city < 0 {
        return Err(StartingVillageBuildActivationError::WrongConstructorState {
            flags: current.flags,
            city: current.city,
        });
    }
    if prefix_request.owner != owner
        || prefix_request.object_id != object_id
        || prefix_request.type_index != current_type
        || prefix_request.type_rows != sim.production_runtime.types.len()
        || prefix_request.snapped_x != current.position().0
        || prefix_request.snapped_y != current.position().1
        || !prefix_request.type_facts.sets_flat_flag
        || prefix_request.type_facts.sets_detector_flag
    {
        return Err(StartingVillageBuildActivationError::PrefixJoinMismatch);
    }
    if facts.wall_init_full_hits <= 0
        || facts.first_checkpoint_full_hits <= 0
        || facts.construct_time == 0
        || !(0..=3).contains(&facts.stance)
        || facts.setup_game_frame != 0
        || facts.active_build_process_frames == 0
        || facts.can_carry_air
        || facts.gathers_from_terrain
    {
        return Err(StartingVillageBuildActivationError::InvalidSourceFacts);
    }
    let owner_bit = 1u8 << owner;
    let activation_completed = facts.wall_init_ever_seen_completed | owner_bit;
    if facts.first_checkpoint_ever_seen & owner_bit == 0
        || facts.first_checkpoint_ever_seen_completed & owner_bit == 0
        || facts.first_checkpoint_ever_seen & facts.wall_init_ever_seen != facts.wall_init_ever_seen
        || facts.first_checkpoint_ever_seen_completed & activation_completed != activation_completed
    {
        return Err(
            StartingVillageBuildActivationError::MissingOwnerVisibility {
                owner_bit,
                ever_seen: facts.first_checkpoint_ever_seen,
                ever_seen_completed: facts.first_checkpoint_ever_seen_completed,
            },
        );
    }

    let world = &sim.map.world;
    if world.xs <= 0
        || world.ys <= 0
        || world.wdata.len() != (world.xs as usize).saturating_mul(world.ys as usize)
        || world.tdata.len() != (world.tile_xs as usize).saturating_mul(world.tile_ys as usize)
    {
        return Err(StartingVillageBuildActivationError::InvalidWorldShape);
    }
    let position = current.position();
    let wx = WCoord::from_coord(Coord(position.0)).0;
    let wy = WCoord::from_coord(Coord(position.1)).0;
    if !world.valid_w(wx, wy) {
        return Err(StartingVillageBuildActivationError::CenterOutsideWorld { wx, wy });
    }
    let center_row = world.w_index(wx, wy);
    let center = &world.wdata[center_row];
    if center.down != -1 || center.down_who != 0 {
        return Err(
            StartingVillageBuildActivationError::CenterWorldCellAlreadyLinked {
                down: center.down,
                down_who: center.down_who,
            },
        );
    }
    let corner = STARTING_VILLAGE_FOOTPRINT.tile_corner(position.0, position.1);
    for (tx, ty) in STARTING_VILLAGE_FOOTPRINT.tiles(corner.0, corner.1) {
        if !world.valid_t(tx, ty) {
            return Err(StartingVillageBuildActivationError::FootprintOutsideWorld { tx, ty });
        }
    }
    let city_radius = tech_cities::city_radius(
        &CityRules::RETAIL,
        STARTING_VILLAGE_TYPE,
        facts.indian_radius_bonus,
    );
    if constructor.world_fix.radius_tiles != city_radius {
        return Err(StartingVillageBuildActivationError::ConstructorJoinMismatch);
    }

    let city_slot = current.city;
    let mut staged_build = current.clone();
    let prefix = apply_build_init_prefix(&mut staged_build, prefix_request)
        .map_err(StartingVillageBuildActivationError::Prefix)?;
    let mut staged_world = sim.map.world.clone();

    // Object::add_to_world, admitted only for the source-proven empty setup cell.
    write_i16(&mut staged_build.other, UP, -1);
    write_i16(&mut staged_build.other, DOWN, center.down);
    write_i16(&mut staged_build.other, DOWN_WHO, center.down_who);
    let wdata_mask_before = staged_world.wdata[center_row].flags & WDATA_BUILD_MASK != 0;
    staged_world.wdata[center_row].down = object_id;
    staged_world.wdata[center_row].down_who = i16::from(owner);

    // Wall::init and the Build-specific initializer tail.  The initial construction-hit
    // value is retained in the receipt even though the first active Build pass later
    // replaces it with the frame-zero full-hit value.
    staged_build.flags &= 0xf9;
    staged_build.build_masks = 0;
    staged_build.job_counter = 0;
    staged_build.job_counter_2 = 0;
    staged_build.helpers = 0;
    staged_build.demolition = 0;
    staged_build.gpiece = -1;
    staged_build.frame_started = -1;
    staged_build.constr_time = facts.construct_time;
    staged_build.myhits = facts.wall_init_full_hits;
    staged_build.construct_hits = production::construct_hits(
        facts.wall_init_full_hits,
        false,
        false,
        0,
        facts.construct_time,
    );
    let wall_init_construct_hits = staged_build.construct_hits;
    staged_build.other[MY_LOS] = 0;
    staged_build.ever_seen = facts.wall_init_ever_seen;
    staged_build.ever_seen_completed = facts.wall_init_ever_seen_completed;
    staged_build.infiltrate = 0;
    staged_build.infiltrate2 = 0;
    staged_build.recharging = 0;
    staged_build.gather_max = 0;
    staged_build.attack_whom = -1;
    staged_build.gather_down = -1;
    staged_build.city = -1;
    staged_build.city_down = -1;
    staged_build.attack_ox = -1;
    staged_build.stance = facts.stance;

    let mut queue_entries = vec![BuildQueueEntry::default(); STARTING_VILLAGE_QUEUE_ROWS];
    queue_entries[0].type_index = -1;
    staged_build.queue = BuildQueue {
        queued: 0,
        entries: queue_entries,
    };
    staged_build.gather_from.tiles.clear();
    staged_build.gather_from.mtn = -1;
    staged_build.gather_from.cliff = -1;
    staged_build.gather.clear();

    // Final source-owned part of Wall::start/BuildType::mask_me.  The transient STARTED
    // bits from Wall::start_me are cleared by mask_me before return, so only the final
    // blocker kind is committed here.
    staged_world.wdata[center_row].flags |= WDATA_BUILD_MASK;
    for (tx, ty) in STARTING_VILLAGE_FOOTPRINT.tiles(corner.0, corner.1) {
        let ti = staged_world.t_index(tx, ty);
        staged_world.tdata[ti] &= !(tflag::STARTED | tflag::STARTED2);
        staged_world.tdata[ti] =
            (staged_world.tdata[ti] & !tflag::BLOCKER_MASK) | tflag::BLOCKER_BUILDING;
    }
    let center_tx = production::tile_of(position.0);
    let center_ty = production::tile_of(position.1);

    // Wall::start reaches Wall::mask_city with STARTED but not ACTIVE and before the City
    // link. Wall::activate then supplies the normalized evidence carried in the handoff.
    // The separate World owner performs the ordered even-circle CITY writes; this Build
    // transaction does not stamp those TData bits.
    staged_build.ever_seen_completed = activation_completed;
    staged_build.flags |= production::flag::STARTED;
    staged_build.frame_started = facts.setup_game_frame;
    staged_build.flags |= production::flag::ACTIVE;
    staged_build.build_masks |= STARTING_VILLAGE_BUILD_MASK;
    staged_build.job_counter = 0;
    staged_build.job_counter_2 = 0;
    let wall_mask_city = WallMaskCityRequest {
        center_tcoord: (center_tx, center_ty),
        radius_tiles: city_radius,
        on: 1,
        owner,
        object_id,
        native_call_flags: staged_build.flags & !production::flag::ACTIVE,
        native_call_city: staged_build.city,
        post_activation_flags: staged_build.flags,
    };
    if !wall_mask_city.city_flag_set()
        || wall_mask_city.native_call_flags != STARTING_VILLAGE_NATIVE_MASK_CITY_FLAGS
        || wall_mask_city.native_call_city != -1
        || wall_mask_city.post_activation_flags != STARTING_VILLAGE_FINAL_FLAGS
    {
        return Err(StartingVillageBuildActivationError::InvalidSourceFacts);
    }

    // Build::activate's already-proven City join, Setup visibility, then the frame-zero
    // stat/update and first active Build::process normalization.
    staged_build.city = city_slot;
    staged_build.ever_seen = facts.first_checkpoint_ever_seen;
    staged_build.ever_seen_completed = facts.first_checkpoint_ever_seen_completed;
    staged_build.myhits = facts.first_checkpoint_full_hits;
    staged_build.other[MY_LOS] = facts.first_checkpoint_los;
    staged_build.begin_frame_construction();
    staged_build.construct_hits = production::construct_hits(
        staged_build.myhits,
        true,
        false,
        staged_build.job_counter,
        staged_build.constr_time,
    );

    if staged_build.flags != STARTING_VILLAGE_FINAL_FLAGS
        || staged_build.build_masks != STARTING_VILLAGE_BUILD_MASK
        || staged_build.city != city_slot
    {
        return Err(StartingVillageBuildActivationError::InvalidSourceFacts);
    }

    let walk_facts = BuildWalkFacts::default();
    let walk = build_walk_value(&staged_build, current_type, &walk_facts)
        .map_err(StartingVillageBuildActivationError::Walk)?;
    if walk.bytes_walked != STARTING_VILLAGE_WALK_BYTES {
        return Err(StartingVillageBuildActivationError::WalkLengthMismatch {
            expected: STARTING_VILLAGE_WALK_BYTES,
            actual: walk.bytes_walked,
        });
    }

    let object_link = StartingBuildObjectLinkReceipt {
        center_wcoord: (wx, wy),
        wdata_head_before: (center.down, center.down_who),
        wdata_head_after: (object_id, i16::from(owner)),
        object_up: read_i16(&staged_build.other, UP),
        object_down: read_i16(&staged_build.other, DOWN),
        object_down_who: read_i16(&staged_build.other, DOWN_WHO),
    };
    let footprint = StartingBuildFootprintReceipt {
        corner_tcoord: corner,
        footprint_tiles: STARTING_VILLAGE_FOOTPRINT.area() as u32,
        center_wdata_build_mask_before: wdata_mask_before,
        center_wdata_build_mask_after: true,
        blocker_kind_complete: true,
        world_channel_ready: false,
    };
    let queue = StartingBuildQueueReceipt {
        rows: staged_build.queue.entries.len(),
        logical_queued: staged_build.queue.queued,
        first_type: staged_build.queue.entries[0].type_index,
        remaining_rows_zeroed: staged_build.queue.entries[1..]
            .iter()
            .filter(|entry| **entry == BuildQueueEntry::default())
            .count(),
        walked_bytes: (STARTING_VILLAGE_QUEUE_ROWS * BuildQueueEntry::WALKED_BYTES) as u64,
    };
    let mining = StartingBuildMiningReceipt {
        length: 0,
        constructor_capacity: prefix.pointer_facts.mining_capacity,
        increment: prefix.pointer_facts.mining_increment,
        flags: prefix.pointer_facts.mining_flags,
        mtn: staged_build.gather_from.mtn,
        cliff: staged_build.gather_from.cliff,
    };

    // The only publication point.  Walk authority is installed last and is infallible.
    sim.builds[row] = staged_build;
    sim.map.world = staged_world;
    authority.install(row, walk_facts.clone());

    Ok(StartingVillageBuildActivationReceipt {
        row,
        owner,
        object_id,
        current_type,
        city_slot,
        prefix,
        stages: STARTING_BUILD_STAGE_ORDER,
        object_link,
        footprint,
        wall_mask_city,
        queue,
        mining,
        wall_init_construct_hits,
        first_checkpoint_construct_hits: sim.builds[row].construct_hits,
        final_flags: sim.builds[row].flags,
        final_build_mask: sim.builds[row].build_masks,
        final_seen: (
            sim.builds[row].ever_seen,
            sim.builds[row].ever_seen_completed,
        ),
        walk_facts,
        walk,
        build_init_complete: true,
        build_activation_complete: true,
        first_checkpoint_build_image_ready: true,
        build_walk_authority_installed: true,
        world_channel_ready: false,
        world_residuals: WORLD_RESIDUALS,
    })
}

fn write_i16(image: &mut [u8], offset: usize, value: i16) {
    image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn read_i16(image: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(
        image[offset..offset + 2]
            .try_into()
            .expect("fixed BuildData short window"),
    )
}
