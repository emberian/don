//! Source-bound `Unit::find_goody_box` continuation for the golden frame-one Citizen.
//!
//! The parent SetIdle transaction is still wholly detached.  This module reads the exact
//! post-SetAnim Sim image in retail instruction order: land domain, decoded WCoord, the home
//! region, 49 shipped offsets, four ordered `WorldData::was_seen` probes, the synchronized
//! terrain/item/object chain, `ItemData::is_seen`, and the stale order-target coordinates.
//! It either proves the local zero return, exposes the first still-unowned regional
//! City/Fort visibility fact, or stops before `Unit::get_goody_box` can touch Groups.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use don_sim::systems::items::{
    fcoord_of, wcoord_of, DOWN_ITEM, GOODY_SEARCH_CELLS, LAND_REJECT_A, LAND_REJECT_B, MOVE_X,
    MOVE_Y, TRIBE_BONUS_SPANISH_RUINS, WFLAG_ITEM, WFLAG_OVERRIDE_LAND,
};
use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::tick::Sim;
use don_sim::world::{Handle, WorldObjectIdentity};

use crate::replay::Replay;
use crate::setup_2024_frame1_citizen_set_idle::{
    validate_frame1_citizen_set_idle_plan, Frame1CitizenFindGoodyBoxRequest,
    Frame1CitizenSetIdleError, Frame1CitizenSetIdleOpenRequest, Frame1CitizenSetIdlePlan,
};
use crate::setup_2024_frame379::Frame379SetupEntryReceipt;
use crate::setup_2024_golden_capture::Frame1PostCommandAuthority;
use crate::world_owner_frontier::sha256;
use don_sim::systems::setup_idle_prefix::{GoldenFrame1EntryAuthority, IdleCitizenPreimage};

pub const UNIT_FIND_GOODY_BOX_VA: u32 = 0x005f_2540;
pub const UNIT_FIND_GOODY_DOMAIN_READ_VA: u32 = 0x005f_254e;
pub const UNIT_FIND_GOODY_FIRST_WAS_SEEN_CALL_VA: u32 = 0x005f_2643;
pub const UNIT_FIND_GOODY_SECOND_WAS_SEEN_CALL_VA: u32 = 0x005f_2664;
pub const UNIT_FIND_GOODY_THIRD_WAS_SEEN_CALL_VA: u32 = 0x005f_2685;
pub const UNIT_FIND_GOODY_FOURTH_WAS_SEEN_CALL_VA: u32 = 0x005f_26ac;
pub const WORLD_WAS_SEEN_VA: u32 = 0x006b_53f0;
pub const OBJECTS_FIND_GOODY_AT_CALL_VA: u32 = 0x005f_2708;
pub const OBJECTS_FIND_GOODY_AT_VA: u32 = 0x0065_c040;
pub const ITEM_IS_SEEN_CALL_VA: u32 = 0x005f_2726;
pub const ITEM_IS_SEEN_VA: u32 = 0x0067_7850;
pub const ITEM_WAS_SEEN_CALL_VA: u32 = 0x0067_790b;
pub const ITEM_WORLD_IS_SEEN_CALL_VA: u32 = 0x0067_7953;
pub const WORLD_IS_SEEN_VA: u32 = 0x006b_55c0;
pub const UNIT_GET_GOODY_BOX_CALL_VA: u32 = 0x005f_2780;
pub const UNIT_GET_GOODY_BOX_VA: u32 = 0x005f_7690;
pub const GROUP_CLEAR_VA: u32 = 0x0071_3e80;
pub const GROUP_ADD_VA: u32 = 0x0071_4350;
pub const GROUPS_PUSH_GROUP_VA: u32 = 0x0070_f9e0;
pub const GROUP_ACTION_MOVE_TO_VA: u32 = 0x0070_fba0;
pub const UNIT_SET_IDLE_AFTER_FIND_GOODY_VA: u32 = 0x005f_6039;
pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-2024-frame1-citizen-find-goody.md";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1CitizenVisibilitySource {
    FogOption,
    ExploredAll,
    SeeAll,
    RevealCounter,
    AlliedCityRegistry,
    AlliedFortRegistry,
    ExploredPlane,
    CurrentTerritory,
    CurrentPlane,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1CitizenCityWitness {
    pub owner: u8,
    pub slot: usize,
    pub city_flags: u16,
    pub city: i16,
    pub object: i16,
    pub region: i16,
}

/// Exact Leader regional-registry values consumed after the live City pool has proved
/// `reg_cities[region] == 0`.  The receipt digest joins the historical setup census to the
/// complete frame-one Build band; it is not a caller-supplied Fort count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1CitizenRegionalRegistryRead {
    pub authority_sha256: [u8; 32],
    pub territory_owner: u8,
    pub region: i16,
    pub reg_cities: u16,
    pub reg_forts: u16,
}

/// One completed retail visibility call.  Optional fields preserve its short-circuit order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenVisibilityRead {
    pub call_va: u32,
    pub body_va: u32,
    pub fog_x: i32,
    pub fog_y: i32,
    pub player: u8,
    pub fog_option: u8,
    pub see_all: bool,
    pub explored_all: bool,
    pub see_own_territory: bool,
    pub reveal_counter: i16,
    pub player_mask: u8,
    pub territory_index: Option<usize>,
    pub territory_owner: Option<i8>,
    pub territory_region: Option<i16>,
    pub allied: Option<bool>,
    pub city_witness: Option<Frame1CitizenCityWitness>,
    pub regional_registry: Option<Frame1CitizenRegionalRegistryRead>,
    pub plane_index: Option<usize>,
    pub plane_byte: Option<u8>,
    pub source: Frame1CitizenVisibilitySource,
    pub result: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1CitizenObjectIdentity {
    Unit {
        handle: Handle,
        row: usize,
        uid: u16,
    },
    Build {
        row: usize,
        uid: u16,
    },
    Wall {
        row: usize,
        uid: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1CitizenObjectLinkRead {
    pub ordinal: usize,
    pub owner: i16,
    pub object: i16,
    pub identity: Frame1CitizenObjectIdentity,
    pub next_object: i16,
    pub next_owner: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1CitizenItemRead {
    pub logical_length: usize,
    pub slot: i16,
    pub flags: u8,
    pub ever_seen: u8,
    pub has_type: bool,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenItemLookupRead {
    pub call_va: u32,
    pub body_va: u32,
    pub initial_object: i16,
    pub initial_owner: i16,
    pub object_links: Vec<Frame1CitizenObjectLinkRead>,
    pub terminal_object: i16,
    pub terminal_owner: i16,
    pub item: Option<Frame1CitizenItemRead>,
    pub item_visibility: Vec<Frame1CitizenVisibilityRead>,
    pub visible: Option<bool>,
}

/// Producer state established by the complete Sim save before the search begins. An absent
/// registry is admitted only when the whole WData plane has no item marker or item sentinel;
/// initialized-empty remains a distinct present state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1CitizenItemRegistryAuthority {
    AbsentAndMapMarkerFree,
    PresentSaveValidated {
        logical_length: usize,
        map_shape: [i32; 2],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1CitizenFindGoodyCellDecision {
    OutOfBounds,
    DifferentRegion,
    NoItemMarker,
    NotExplored,
    NeedsRegionSeen,
    HiddenItem,
    AcceptedWaterGate,
    AcceptedNoItem,
    AcceptedVisibleItem,
}

/// Reads completed for one spiral entry, in exact table order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenFindGoodyCellRead {
    pub ordinal: usize,
    pub offset_x: i32,
    pub offset_y: i32,
    pub world_x: i32,
    pub world_y: i32,
    pub world_index: Option<usize>,
    pub region: Option<i16>,
    pub flags: Option<u16>,
    pub fog_probes: Vec<Frame1CitizenVisibilityRead>,
    pub land: Option<i8>,
    pub item_lookup: Option<Frame1CitizenItemLookupRead>,
    pub decision: Frame1CitizenFindGoodyCellDecision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1CitizenFindGoodyFalseReason {
    ExhaustedSpiral,
    AlreadyOrderTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenFindGoodyFalseReceipt {
    pub receipt_sha256: [u8; 32],
    pub parent_digest: [u8; 32],
    pub body_va: u32,
    pub return_value: i32,
    pub resume_va: u32,
    pub reason: Frame1CitizenFindGoodyFalseReason,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub rng_draws: u32,
}

/// Exact first unowned arm of `WorldData::was_seen`: the regional Fort counter after a
/// mutually-allied territory owner and a proven-zero City counter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenRegionSeenRequest {
    pub request_sha256: [u8; 32],
    pub parent_digest: [u8; 32],
    pub scan_prefix_sha256: [u8; 32],
    pub body_va: u32,
    pub frame: i32,
    pub unit: Handle,
    pub player: u8,
    pub fog_x: i32,
    pub fog_y: i32,
    pub territory_owner: u8,
    pub region: i16,
    pub reg_cities: u16,
    pub needed_field_offset: usize,
    pub restore_mask2_bit8000: bool,
}

/// Source-bound answer for one regional visibility request.  This is produced only by the
/// setup-census/frame-one-Build-band join in `setup_2024_frame1_citizen_region_seen`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenRegionSeenAuthority {
    pub authority_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub post_command_sim_sha256: [u8; 32],
    pub set_anim_return_sim_sha256: [u8; 32],
    pub territory_owner: u8,
    pub region: i16,
    pub reg_cities: u16,
    pub reg_forts: u16,
    pub build_mark: i32,
    pub center_build_row: usize,
    pub center_build_o: i16,
    pub center_build_uid: u16,
    pub center_build_type: i32,
    pub center_region: i16,
    pub market_build_row: usize,
    pub market_build_o: i16,
    pub market_build_uid: u16,
    pub market_build_type: i32,
    pub restore_mask2_bit8000: bool,
}

/// Reached `Unit::get_goody_box` call.  None of these scratch Group/canonical Groups writes
/// has run; the complete parent transaction remains detached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenGetGoodyBoxRequest {
    pub request_sha256: [u8; 32],
    pub parent_digest: [u8; 32],
    pub scan_prefix_sha256: [u8; 32],
    pub call_va: u32,
    pub body_va: u32,
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
    pub world_x: i32,
    pub world_y: i32,
    pub target_fine_x: i32,
    pub target_fine_y: i32,
    pub order_target_x_before: i32,
    pub order_target_y_before: i32,
    pub group_clear_va: u32,
    pub group_add_va: u32,
    pub groups_push_group_va: u32,
    pub group_action_move_to_va: u32,
    pub queue_pos: i32,
    pub order_index: i32,
    pub after_find_goody_prefix: IdleCitizenPreimage,
    pub restore_mask2_bit8000: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenFindGoodyOpenRequest {
    ReturnedFalse(Frame1CitizenFindGoodyFalseReceipt),
    RegionSeen(Frame1CitizenRegionSeenRequest),
    GetGoodyBox(Frame1CitizenGetGoodyBoxRequest),
}

/// Detached continuation.  `after_local` is bit-identical to the SetIdle entry image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CitizenFindGoodyPlan {
    pub composition_digest: [u8; 32],
    pub set_idle: Frame1CitizenSetIdlePlan,
    pub source_sim_sha256: [u8; 32],
    pub after_local: IdleCitizenPreimage,
    pub row: usize,
    pub unit_world_x: i32,
    pub unit_world_y: i32,
    pub home_region: i16,
    pub order_target_x: i32,
    pub order_target_y: i32,
    pub order_target_world_x: i32,
    pub order_target_world_y: i32,
    pub item_registry: Frame1CitizenItemRegistryAuthority,
    pub journal: Vec<Frame1CitizenFindGoodyCellRead>,
    pub restore_mask2_bit8000: bool,
    pub open: Frame1CitizenFindGoodyOpenRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CitizenFindGoodyError {
    Parent(Frame1CitizenSetIdleError),
    Snapshot(SaveError),
    NotFindGoody,
    FindGoodyRequestMismatch,
    RestoreNotArmed,
    UnsupportedDomain { domain: i32 },
    StaleActor,
    CoordinateMismatch,
    ActorOutOfBounds { world_x: i32, world_y: i32 },
    InvalidFogPolicy { option: u8 },
    InvalidTerritoryOwner { owner: i8 },
    InvalidRegion { region: i16 },
    StaleCityRegistry { owner: u8 },
    ItemRuntimeMapMismatch,
    ObjectChainCycle { owner: i16, object: i16 },
    UnsupportedObjectAddress { owner: i16, object: i16 },
    StaleObjectRegistry { owner: i16, object: i16 },
    StaleItemRegistry { slot: i16, logical_length: usize },
    ItemProducerStateMismatch,
    StalePlan,
}

impl fmt::Display for Frame1CitizenFindGoodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-1 Citizen FindGoody refused: {self:?}")
    }
}

impl std::error::Error for Frame1CitizenFindGoodyError {}

impl From<Frame1CitizenSetIdleError> for Frame1CitizenFindGoodyError {
    fn from(value: Frame1CitizenSetIdleError) -> Self {
        Self::Parent(value)
    }
}

impl From<SaveError> for Frame1CitizenFindGoodyError {
    fn from(value: SaveError) -> Self {
        Self::Snapshot(value)
    }
}

fn request_matches(
    plan: &Frame1CitizenSetIdlePlan,
    request: &Frame1CitizenFindGoodyBoxRequest,
) -> bool {
    request.authority_revision == plan.think_suffix.think.authority.revision
        && request.authority_digest == plan.think_suffix.think.authority.composition_digest
        && request.think_suffix_digest == plan.think_suffix.composition_digest
        && request.call_va
            == crate::setup_2024_frame1_citizen_set_idle::UNIT_SET_IDLE_FIND_GOODY_CALL_VA
        && request.body_va == UNIT_FIND_GOODY_BOX_VA
        && request.frame == plan.after_local.frame
        && request.unit == plan.after_local.unit.handle
        && request.who == plan.after_local.unit.who
        && request.o == plan.after_local.unit.o
        && request.type_index == plan.after_local.unit.type_index
        && request.after_set_idle_entry == plan.after_local
        && request.staged_leader_pending == plan.think_suffix.think.staged_leader_pending
        && request.restore_mask2_bit8000
        && plan.restore_mask2_bit8000
}

fn band_for(object: i16) -> Option<RetailBand> {
    let object = i32::from(object);
    RetailBand::ALL
        .into_iter()
        .find(|band| band.contains(object))
}

fn read_i16(image: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(
        image[offset..offset + 2]
            .try_into()
            .expect("fixed ObjectData i16 window"),
    )
}

fn canonical_object_link(
    sim: &Sim,
    ordinal: usize,
    owner: i16,
    object: i16,
) -> Result<Frame1CitizenObjectLinkRead, Frame1CitizenFindGoodyError> {
    let owner_u8 = u8::try_from(owner)
        .map_err(|_| Frame1CitizenFindGoodyError::UnsupportedObjectAddress { owner, object })?;
    let band = band_for(object)
        .ok_or(Frame1CitizenFindGoodyError::UnsupportedObjectAddress { owner, object })?;
    let address = RetailObjectAddress::new(owner_u8, band, i32::from(object));
    if !address.is_well_formed() {
        return Err(Frame1CitizenFindGoodyError::UnsupportedObjectAddress { owner, object });
    }
    let identity = sim
        .world
        .object_bands()
        .live_identity(address)
        .ok_or(Frame1CitizenFindGoodyError::StaleObjectRegistry { owner, object })?;
    let (identity, next_object, next_owner) = match identity {
        WorldObjectIdentity::Unit { id, generation } if band == RetailBand::Unit => {
            let handle = Handle { id, generation };
            let row = sim
                .world
                .row_of(handle)
                .ok_or(Frame1CitizenFindGoodyError::StaleObjectRegistry { owner, object })?;
            if sim.world.units.get_who(row) != owner_u8 || sim.world.units.o()[row] != object {
                return Err(Frame1CitizenFindGoodyError::StaleObjectRegistry { owner, object });
            }
            (
                Frame1CitizenObjectIdentity::Unit {
                    handle,
                    row,
                    uid: sim.world.units.get_uid(row),
                },
                sim.world.units.down()[row],
                sim.world.units.down_who()[row],
            )
        }
        WorldObjectIdentity::BuildRow(row) if band == RetailBand::Build => {
            let row = row as usize;
            let build = sim
                .builds
                .get(row)
                .ok_or(Frame1CitizenFindGoodyError::StaleObjectRegistry { owner, object })?;
            if build.who != owner_u8 || build.object_id() != object {
                return Err(Frame1CitizenFindGoodyError::StaleObjectRegistry { owner, object });
            }
            (
                Frame1CitizenObjectIdentity::Build {
                    row,
                    uid: build.uid,
                },
                read_i16(&build.other, 0x2c),
                read_i16(&build.other, 0x2e),
            )
        }
        WorldObjectIdentity::WallRow(row) if band == RetailBand::Wall => {
            let row = row as usize;
            let wall = sim
                .walls
                .get(row)
                .ok_or(Frame1CitizenFindGoodyError::StaleObjectRegistry { owner, object })?;
            if wall.who != owner_u8 {
                return Err(Frame1CitizenFindGoodyError::StaleObjectRegistry { owner, object });
            }
            (
                Frame1CitizenObjectIdentity::Wall { row, uid: wall.uid },
                wall.down,
                wall.down_who,
            )
        }
        _ => return Err(Frame1CitizenFindGoodyError::StaleObjectRegistry { owner, object }),
    };
    Ok(Frame1CitizenObjectLinkRead {
        ordinal,
        owner,
        object,
        identity,
        next_object,
        next_owner,
    })
}

fn city_witness(
    sim: &Sim,
    owner: u8,
    region: i16,
) -> Result<Option<Frame1CitizenCityWitness>, Frame1CitizenFindGoodyError> {
    let owner_index = usize::from(owner);
    let mark = usize::try_from(sim.cities.city_mark[owner_index])
        .map_err(|_| Frame1CitizenFindGoodyError::StaleCityRegistry { owner })?;
    let slots = &sim.cities.slots[owner_index];
    if mark > slots.len() {
        return Err(Frame1CitizenFindGoodyError::StaleCityRegistry { owner });
    }
    Ok(slots[..mark]
        .iter()
        .enumerate()
        .find(|(_, city)| city.active() && city.who == owner as i8 && city.reg == region)
        .map(|(slot, city)| Frame1CitizenCityWitness {
            owner,
            slot,
            city_flags: city.city_flags,
            city: city.city,
            object: city.o,
            region: city.reg,
        }))
}

enum WasSeenResult {
    Resolved(Frame1CitizenVisibilityRead),
    NeedsRegion {
        territory_owner: u8,
        region: i16,
        fog_x: i32,
        fog_y: i32,
    },
}

fn was_seen(
    sim: &Sim,
    call_va: u32,
    fog_x: i32,
    fog_y: i32,
    player: u8,
    regional: Option<&Frame1CitizenRegionSeenAuthority>,
) -> Result<WasSeenResult, Frame1CitizenFindGoodyError> {
    let world = &sim.map.world;
    let fog = &sim.map.fog;
    if fog.option.0 > 3 {
        return Err(Frame1CitizenFindGoodyError::InvalidFogPolicy {
            option: fog.option.0,
        });
    }
    let leader = fog.leaders[usize::from(player)];
    let base = |source, result| Frame1CitizenVisibilityRead {
        call_va,
        body_va: WORLD_WAS_SEEN_VA,
        fog_x,
        fog_y,
        player,
        fog_option: fog.option.0,
        see_all: leader.see_all,
        explored_all: leader.explored_all,
        see_own_territory: leader.see_own_territory,
        reveal_counter: leader.reveal_counter,
        player_mask: leader.player_mask,
        territory_index: None,
        territory_owner: None,
        territory_region: None,
        allied: None,
        city_witness: None,
        regional_registry: None,
        plane_index: None,
        plane_byte: None,
        source,
        result,
    };
    if fog.option.0 > 1 {
        return Ok(WasSeenResult::Resolved(base(
            Frame1CitizenVisibilitySource::FogOption,
            true,
        )));
    }
    if leader.explored_all {
        return Ok(WasSeenResult::Resolved(base(
            Frame1CitizenVisibilitySource::ExploredAll,
            true,
        )));
    }
    if leader.see_all {
        return Ok(WasSeenResult::Resolved(base(
            Frame1CitizenVisibilitySource::SeeAll,
            true,
        )));
    }
    if leader.reveal_counter != 0 {
        return Ok(WasSeenResult::Resolved(base(
            Frame1CitizenVisibilitySource::RevealCounter,
            true,
        )));
    }
    let world_x = fog_x >> 1;
    let world_y = fog_y >> 1;
    if !world.valid_w(world_x, world_y) || !world.valid_f(fog_x, fog_y) {
        return Err(Frame1CitizenFindGoodyError::ActorOutOfBounds { world_x, world_y });
    }
    let territory_index = world.w_index(world_x, world_y);
    let cell = &world.wdata[territory_index];
    if cell.who >= 0 {
        if cell.who >= 8 {
            return Err(Frame1CitizenFindGoodyError::InvalidTerritoryOwner { owner: cell.who });
        }
        let owner = cell.who as u8;
        let allied = sim
            .vic_leaders
            .is_ally(usize::from(player), usize::from(owner));
        if allied {
            if !(0..64).contains(&i32::from(cell.region)) {
                return Err(Frame1CitizenFindGoodyError::InvalidRegion {
                    region: cell.region,
                });
            }
            if let Some(witness) = city_witness(sim, owner, cell.region)? {
                let mut read = base(Frame1CitizenVisibilitySource::AlliedCityRegistry, true);
                read.territory_index = Some(territory_index);
                read.territory_owner = Some(cell.who);
                read.territory_region = Some(cell.region);
                read.allied = Some(true);
                read.city_witness = Some(witness);
                return Ok(WasSeenResult::Resolved(read));
            }
            match regional {
                Some(authority)
                    if authority.territory_owner == owner
                        && authority.region == cell.region
                        && authority.reg_cities == 0 =>
                {
                    let mut read = base(
                        if authority.reg_forts != 0 {
                            Frame1CitizenVisibilitySource::AlliedFortRegistry
                        } else {
                            Frame1CitizenVisibilitySource::ExploredPlane
                        },
                        authority.reg_forts != 0,
                    );
                    read.territory_index = Some(territory_index);
                    read.territory_owner = Some(cell.who);
                    read.territory_region = Some(cell.region);
                    read.allied = Some(true);
                    read.regional_registry = Some(Frame1CitizenRegionalRegistryRead {
                        authority_sha256: authority.authority_sha256,
                        territory_owner: authority.territory_owner,
                        region: authority.region,
                        reg_cities: authority.reg_cities,
                        reg_forts: authority.reg_forts,
                    });
                    if authority.reg_forts != 0 {
                        return Ok(WasSeenResult::Resolved(read));
                    }
                    let plane_index = world.f_index(fog_x, fog_y);
                    let plane_byte = world.seen2[plane_index];
                    read.result = plane_byte & leader.player_mask != 0;
                    read.plane_index = Some(plane_index);
                    read.plane_byte = Some(plane_byte);
                    return Ok(WasSeenResult::Resolved(read));
                }
                _ => {
                    return Ok(WasSeenResult::NeedsRegion {
                        territory_owner: owner,
                        region: cell.region,
                        fog_x,
                        fog_y,
                    });
                }
            }
        }
    }
    let plane_index = world.f_index(fog_x, fog_y);
    let plane_byte = world.seen2[plane_index];
    let result = plane_byte & leader.player_mask != 0;
    let mut read = base(Frame1CitizenVisibilitySource::ExploredPlane, result);
    read.territory_index = Some(territory_index);
    read.territory_owner = Some(cell.who);
    read.territory_region = Some(cell.region);
    read.allied = (cell.who >= 0).then(|| {
        sim.vic_leaders
            .is_ally(usize::from(player), cell.who as usize)
    });
    read.plane_index = Some(plane_index);
    read.plane_byte = Some(plane_byte);
    Ok(WasSeenResult::Resolved(read))
}

fn is_seen(
    sim: &Sim,
    fog_x: i32,
    fog_y: i32,
    player: u8,
) -> Result<Frame1CitizenVisibilityRead, Frame1CitizenFindGoodyError> {
    let world = &sim.map.world;
    let fog = &sim.map.fog;
    if fog.option.0 > 3 {
        return Err(Frame1CitizenFindGoodyError::InvalidFogPolicy {
            option: fog.option.0,
        });
    }
    let leader = fog.leaders[usize::from(player)];
    let base = |source, result| Frame1CitizenVisibilityRead {
        call_va: ITEM_WORLD_IS_SEEN_CALL_VA,
        body_va: WORLD_IS_SEEN_VA,
        fog_x,
        fog_y,
        player,
        fog_option: fog.option.0,
        see_all: leader.see_all,
        explored_all: leader.explored_all,
        see_own_territory: leader.see_own_territory,
        reveal_counter: leader.reveal_counter,
        player_mask: leader.player_mask,
        territory_index: None,
        territory_owner: None,
        territory_region: None,
        allied: None,
        city_witness: None,
        regional_registry: None,
        plane_index: None,
        plane_byte: None,
        source,
        result,
    };
    if fog.option.0 == 3 {
        return Ok(base(Frame1CitizenVisibilitySource::FogOption, true));
    }
    if leader.see_all {
        return Ok(base(Frame1CitizenVisibilitySource::SeeAll, true));
    }
    if leader.reveal_counter != 0 {
        return Ok(base(Frame1CitizenVisibilitySource::RevealCounter, true));
    }
    if !world.valid_f(fog_x, fog_y) {
        return Err(Frame1CitizenFindGoodyError::ActorOutOfBounds {
            world_x: fog_x >> 1,
            world_y: fog_y >> 1,
        });
    }
    let territory_index = world.w_index(fog_x >> 1, fog_y >> 1);
    let cell = &world.wdata[territory_index];
    if leader.see_own_territory && cell.who >= 0 {
        if cell.who >= 8 {
            return Err(Frame1CitizenFindGoodyError::InvalidTerritoryOwner { owner: cell.who });
        }
        let allied = sim
            .vic_leaders
            .is_ally(usize::from(player), cell.who as usize);
        if allied {
            let mut read = base(Frame1CitizenVisibilitySource::CurrentTerritory, true);
            read.territory_index = Some(territory_index);
            read.territory_owner = Some(cell.who);
            read.territory_region = Some(cell.region);
            read.allied = Some(true);
            return Ok(read);
        }
    }
    let plane_index = world.f_index(fog_x, fog_y);
    let plane_byte = world.seen[plane_index];
    let result = plane_byte & leader.player_mask != 0;
    let mut read = base(Frame1CitizenVisibilitySource::CurrentPlane, result);
    read.territory_index = Some(territory_index);
    read.territory_owner = Some(cell.who);
    read.territory_region = Some(cell.region);
    read.allied = (leader.see_own_territory && cell.who >= 0).then(|| {
        sim.vic_leaders
            .is_ally(usize::from(player), cell.who as usize)
    });
    read.plane_index = Some(plane_index);
    read.plane_byte = Some(plane_byte);
    Ok(read)
}

fn item_lookup(
    sim: &Sim,
    world_x: i32,
    world_y: i32,
) -> Result<Frame1CitizenItemLookupRead, Frame1CitizenFindGoodyError> {
    // `plan_frame1_citizen_find_goody` first round-trips the complete Sim through save_sim.
    // Save validation permits an absent producer only when the entire WData plane has neither
    // an item flag nor an item-chain sentinel. Reaching this call from an item-marked cell
    // therefore proves that the authoritative registry is present.
    let runtime = sim
        .world
        .item_runtime
        .as_ref()
        .ok_or(Frame1CitizenFindGoodyError::ItemProducerStateMismatch)?;
    if runtime.map_shape() != (sim.map.world.xs, sim.map.world.ys) {
        return Err(Frame1CitizenFindGoodyError::ItemRuntimeMapMismatch);
    }
    let cell = sim.map.world.wdata(world_x, world_y);
    let initial_object = cell.down;
    let initial_owner = cell.down_who;
    let mut terminal_object = initial_object;
    let mut terminal_owner = initial_owner;
    let mut object_links = Vec::new();
    let mut seen = BTreeSet::new();
    while terminal_object >= 0 {
        if !seen.insert((terminal_owner, terminal_object)) {
            return Err(Frame1CitizenFindGoodyError::ObjectChainCycle {
                owner: terminal_owner,
                object: terminal_object,
            });
        }
        let link = canonical_object_link(sim, object_links.len(), terminal_owner, terminal_object)?;
        terminal_object = link.next_object;
        terminal_owner = link.next_owner;
        object_links.push(link);
    }
    let item =
        if terminal_object == DOWN_ITEM {
            let slot = usize::try_from(terminal_owner).map_err(|_| {
                Frame1CitizenFindGoodyError::StaleItemRegistry {
                    slot: terminal_owner,
                    logical_length: runtime.items().len(),
                }
            })?;
            let item = runtime.items().get(slot).ok_or(
                Frame1CitizenFindGoodyError::StaleItemRegistry {
                    slot: terminal_owner,
                    logical_length: runtime.items().len(),
                },
            )?;
            item.is_valid().then_some(Frame1CitizenItemRead {
                logical_length: runtime.items().len(),
                slot: terminal_owner,
                flags: item.flags,
                ever_seen: item.ever_seen,
                has_type: item.has_type,
                type_index: item.type_index,
                x: item.x(),
                y: item.y(),
            })
        } else {
            None
        };
    Ok(Frame1CitizenItemLookupRead {
        call_va: OBJECTS_FIND_GOODY_AT_CALL_VA,
        body_va: OBJECTS_FIND_GOODY_AT_VA,
        initial_object,
        initial_owner,
        object_links,
        terminal_object,
        terminal_owner,
        item,
        item_visibility: Vec::new(),
        visible: None,
    })
}

fn append_bool(image: &mut Vec<u8>, value: bool) {
    image.push(u8::from(value));
}

fn append_opt_i16(image: &mut Vec<u8>, value: Option<i16>) {
    match value {
        Some(value) => {
            image.push(1);
            image.extend_from_slice(&value.to_le_bytes());
        }
        None => image.push(0),
    }
}

fn append_opt_i8(image: &mut Vec<u8>, value: Option<i8>) {
    match value {
        Some(value) => {
            image.push(1);
            image.push(value as u8);
        }
        None => image.push(0),
    }
}

fn append_opt_u16(image: &mut Vec<u8>, value: Option<u16>) {
    match value {
        Some(value) => {
            image.push(1);
            image.extend_from_slice(&value.to_le_bytes());
        }
        None => image.push(0),
    }
}

fn append_opt_usize(image: &mut Vec<u8>, value: Option<usize>) {
    match value {
        Some(value) => {
            image.push(1);
            image.extend_from_slice(&(value as u64).to_le_bytes());
        }
        None => image.push(0),
    }
}

fn append_visibility(image: &mut Vec<u8>, read: &Frame1CitizenVisibilityRead) {
    image.extend_from_slice(&read.call_va.to_le_bytes());
    image.extend_from_slice(&read.body_va.to_le_bytes());
    image.extend_from_slice(&read.fog_x.to_le_bytes());
    image.extend_from_slice(&read.fog_y.to_le_bytes());
    image.push(read.player);
    image.push(read.fog_option);
    append_bool(image, read.see_all);
    append_bool(image, read.explored_all);
    append_bool(image, read.see_own_territory);
    image.extend_from_slice(&read.reveal_counter.to_le_bytes());
    image.push(read.player_mask);
    append_opt_usize(image, read.territory_index);
    append_opt_i8(image, read.territory_owner);
    append_opt_i16(image, read.territory_region);
    match read.allied {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            append_bool(image, value);
        }
    }
    match read.city_witness {
        None => image.push(0),
        Some(witness) => {
            image.push(1);
            image.push(witness.owner);
            image.extend_from_slice(&(witness.slot as u64).to_le_bytes());
            image.extend_from_slice(&witness.city_flags.to_le_bytes());
            image.extend_from_slice(&witness.city.to_le_bytes());
            image.extend_from_slice(&witness.object.to_le_bytes());
            image.extend_from_slice(&witness.region.to_le_bytes());
        }
    }
    match read.regional_registry {
        None => image.push(0),
        Some(regional) => {
            image.push(1);
            image.extend_from_slice(&regional.authority_sha256);
            image.push(regional.territory_owner);
            image.extend_from_slice(&regional.region.to_le_bytes());
            image.extend_from_slice(&regional.reg_cities.to_le_bytes());
            image.extend_from_slice(&regional.reg_forts.to_le_bytes());
        }
    }
    append_opt_usize(image, read.plane_index);
    match read.plane_byte {
        Some(value) => {
            image.push(1);
            image.push(value);
        }
        None => image.push(0),
    }
    image.push(read.source as u8);
    append_bool(image, read.result);
}

fn append_identity(image: &mut Vec<u8>, identity: Frame1CitizenObjectIdentity) {
    match identity {
        Frame1CitizenObjectIdentity::Unit { handle, row, uid } => {
            image.push(0);
            image.extend_from_slice(&handle.id.to_le_bytes());
            image.extend_from_slice(&handle.generation.to_le_bytes());
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
        Frame1CitizenObjectIdentity::Build { row, uid } => {
            image.push(1);
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
        Frame1CitizenObjectIdentity::Wall { row, uid } => {
            image.push(2);
            image.extend_from_slice(&(row as u64).to_le_bytes());
            image.extend_from_slice(&uid.to_le_bytes());
        }
    }
}

fn append_lookup(image: &mut Vec<u8>, lookup: &Frame1CitizenItemLookupRead) {
    image.extend_from_slice(&lookup.call_va.to_le_bytes());
    image.extend_from_slice(&lookup.body_va.to_le_bytes());
    image.extend_from_slice(&lookup.initial_object.to_le_bytes());
    image.extend_from_slice(&lookup.initial_owner.to_le_bytes());
    image.extend_from_slice(&(lookup.object_links.len() as u64).to_le_bytes());
    for link in &lookup.object_links {
        image.extend_from_slice(&(link.ordinal as u64).to_le_bytes());
        image.extend_from_slice(&link.owner.to_le_bytes());
        image.extend_from_slice(&link.object.to_le_bytes());
        append_identity(image, link.identity);
        image.extend_from_slice(&link.next_object.to_le_bytes());
        image.extend_from_slice(&link.next_owner.to_le_bytes());
    }
    image.extend_from_slice(&lookup.terminal_object.to_le_bytes());
    image.extend_from_slice(&lookup.terminal_owner.to_le_bytes());
    match lookup.item {
        None => image.push(0),
        Some(item) => {
            image.push(1);
            image.extend_from_slice(&(item.logical_length as u64).to_le_bytes());
            image.extend_from_slice(&item.slot.to_le_bytes());
            image.push(item.flags);
            image.push(item.ever_seen);
            append_bool(image, item.has_type);
            image.extend_from_slice(&item.type_index.to_le_bytes());
            image.extend_from_slice(&item.x.to_le_bytes());
            image.extend_from_slice(&item.y.to_le_bytes());
        }
    }
    image.extend_from_slice(&(lookup.item_visibility.len() as u64).to_le_bytes());
    for read in &lookup.item_visibility {
        append_visibility(image, read);
    }
    match lookup.visible {
        None => image.push(0),
        Some(value) => {
            image.push(1);
            append_bool(image, value);
        }
    }
}

fn append_journal(image: &mut Vec<u8>, journal: &[Frame1CitizenFindGoodyCellRead]) {
    image.extend_from_slice(&(journal.len() as u64).to_le_bytes());
    for cell in journal {
        image.extend_from_slice(&(cell.ordinal as u64).to_le_bytes());
        image.extend_from_slice(&cell.offset_x.to_le_bytes());
        image.extend_from_slice(&cell.offset_y.to_le_bytes());
        image.extend_from_slice(&cell.world_x.to_le_bytes());
        image.extend_from_slice(&cell.world_y.to_le_bytes());
        append_opt_usize(image, cell.world_index);
        append_opt_i16(image, cell.region);
        append_opt_u16(image, cell.flags);
        image.extend_from_slice(&(cell.fog_probes.len() as u64).to_le_bytes());
        for read in &cell.fog_probes {
            append_visibility(image, read);
        }
        append_opt_i8(image, cell.land);
        match &cell.item_lookup {
            Some(lookup) => {
                image.push(1);
                append_lookup(image, lookup);
            }
            None => image.push(0),
        }
        image.push(cell.decision as u8);
    }
}

fn scan_prefix_digest(journal: &[Frame1CitizenFindGoodyCellRead]) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-find-goody-scan-v1".to_vec();
    append_journal(&mut image, journal);
    sha256(&image)
}

pub fn frame1_citizen_region_seen_request_digest(
    request: &Frame1CitizenRegionSeenRequest,
) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-region-seen-v1".to_vec();
    image.extend_from_slice(&request.parent_digest);
    image.extend_from_slice(&request.scan_prefix_sha256);
    image.extend_from_slice(&request.body_va.to_le_bytes());
    image.extend_from_slice(&request.frame.to_le_bytes());
    image.extend_from_slice(&request.unit.id.to_le_bytes());
    image.extend_from_slice(&request.unit.generation.to_le_bytes());
    image.push(request.player);
    image.extend_from_slice(&request.fog_x.to_le_bytes());
    image.extend_from_slice(&request.fog_y.to_le_bytes());
    image.push(request.territory_owner);
    image.extend_from_slice(&request.region.to_le_bytes());
    image.extend_from_slice(&request.reg_cities.to_le_bytes());
    image.extend_from_slice(&(request.needed_field_offset as u64).to_le_bytes());
    append_bool(&mut image, request.restore_mask2_bit8000);
    sha256(&image)
}

pub fn frame1_citizen_get_goody_box_request_digest(
    request: &Frame1CitizenGetGoodyBoxRequest,
) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-get-goody-box-v1".to_vec();
    image.extend_from_slice(&request.parent_digest);
    image.extend_from_slice(&request.scan_prefix_sha256);
    image.extend_from_slice(&request.call_va.to_le_bytes());
    image.extend_from_slice(&request.body_va.to_le_bytes());
    image.extend_from_slice(&request.frame.to_le_bytes());
    image.extend_from_slice(&request.unit.id.to_le_bytes());
    image.extend_from_slice(&request.unit.generation.to_le_bytes());
    image.push(request.who);
    image.extend_from_slice(&request.o.to_le_bytes());
    image.extend_from_slice(&request.world_x.to_le_bytes());
    image.extend_from_slice(&request.world_y.to_le_bytes());
    image.extend_from_slice(&request.target_fine_x.to_le_bytes());
    image.extend_from_slice(&request.target_fine_y.to_le_bytes());
    image.extend_from_slice(&request.order_target_x_before.to_le_bytes());
    image.extend_from_slice(&request.order_target_y_before.to_le_bytes());
    image.extend_from_slice(&request.group_clear_va.to_le_bytes());
    image.extend_from_slice(&request.group_add_va.to_le_bytes());
    image.extend_from_slice(&request.groups_push_group_va.to_le_bytes());
    image.extend_from_slice(&request.group_action_move_to_va.to_le_bytes());
    image.extend_from_slice(&request.queue_pos.to_le_bytes());
    image.extend_from_slice(&request.order_index.to_le_bytes());
    image.extend_from_slice(&request.after_find_goody_prefix.unit_masks.to_le_bytes());
    image.extend_from_slice(&request.after_find_goody_prefix.unit_masks2.to_le_bytes());
    image.push(request.after_find_goody_prefix.idle);
    append_bool(&mut image, request.restore_mask2_bit8000);
    sha256(&image)
}

fn false_receipt_digest(receipt: &Frame1CitizenFindGoodyFalseReceipt) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-find-goody-false-v1".to_vec();
    image.extend_from_slice(&receipt.parent_digest);
    image.extend_from_slice(&receipt.body_va.to_le_bytes());
    image.extend_from_slice(&receipt.return_value.to_le_bytes());
    image.extend_from_slice(&receipt.resume_va.to_le_bytes());
    image.push(receipt.reason as u8);
    image.extend_from_slice(&receipt.random_state_before.to_le_bytes());
    image.extend_from_slice(&receipt.random_state_after.to_le_bytes());
    image.extend_from_slice(&receipt.rng_draws.to_le_bytes());
    sha256(&image)
}

fn plan_digest(plan: &Frame1CitizenFindGoodyPlan) -> [u8; 32] {
    let mut image = b"don-frame1-citizen-find-goody-v2".to_vec();
    image.extend_from_slice(&plan.set_idle.composition_digest);
    image.extend_from_slice(&plan.source_sim_sha256);
    image.extend_from_slice(&(plan.row as u64).to_le_bytes());
    image.extend_from_slice(&plan.unit_world_x.to_le_bytes());
    image.extend_from_slice(&plan.unit_world_y.to_le_bytes());
    image.extend_from_slice(&plan.home_region.to_le_bytes());
    image.extend_from_slice(&plan.order_target_x.to_le_bytes());
    image.extend_from_slice(&plan.order_target_y.to_le_bytes());
    image.extend_from_slice(&plan.order_target_world_x.to_le_bytes());
    image.extend_from_slice(&plan.order_target_world_y.to_le_bytes());
    match plan.item_registry {
        Frame1CitizenItemRegistryAuthority::AbsentAndMapMarkerFree => image.push(0),
        Frame1CitizenItemRegistryAuthority::PresentSaveValidated {
            logical_length,
            map_shape,
        } => {
            image.push(1);
            image.extend_from_slice(&(logical_length as u64).to_le_bytes());
            image.extend_from_slice(&map_shape[0].to_le_bytes());
            image.extend_from_slice(&map_shape[1].to_le_bytes());
        }
    }
    append_journal(&mut image, &plan.journal);
    append_bool(&mut image, plan.restore_mask2_bit8000);
    match &plan.open {
        Frame1CitizenFindGoodyOpenRequest::ReturnedFalse(receipt) => {
            image.push(0);
            image.extend_from_slice(&receipt.receipt_sha256);
        }
        Frame1CitizenFindGoodyOpenRequest::RegionSeen(request) => {
            image.push(1);
            image.extend_from_slice(&request.request_sha256);
        }
        Frame1CitizenFindGoodyOpenRequest::GetGoodyBox(request) => {
            image.push(2);
            image.extend_from_slice(&request.request_sha256);
        }
    }
    sha256(&image)
}

fn false_open(
    parent_digest: [u8; 32],
    reason: Frame1CitizenFindGoodyFalseReason,
    random_state: i32,
) -> Frame1CitizenFindGoodyOpenRequest {
    let mut receipt = Frame1CitizenFindGoodyFalseReceipt {
        receipt_sha256: [0; 32],
        parent_digest,
        body_va: UNIT_FIND_GOODY_BOX_VA,
        return_value: 0,
        resume_va: UNIT_SET_IDLE_AFTER_FIND_GOODY_VA,
        reason,
        random_state_before: random_state,
        random_state_after: random_state,
        rng_draws: 0,
    };
    receipt.receipt_sha256 = false_receipt_digest(&receipt);
    Frame1CitizenFindGoodyOpenRequest::ReturnedFalse(receipt)
}

#[allow(clippy::too_many_arguments)]
fn finish_plan(
    set_idle: &Frame1CitizenSetIdlePlan,
    source_sim_sha256: [u8; 32],
    row: usize,
    unit_world_x: i32,
    unit_world_y: i32,
    home_region: i16,
    order_target_x: i32,
    order_target_y: i32,
    item_registry: Frame1CitizenItemRegistryAuthority,
    journal: Vec<Frame1CitizenFindGoodyCellRead>,
    open: Frame1CitizenFindGoodyOpenRequest,
) -> Frame1CitizenFindGoodyPlan {
    let mut plan = Frame1CitizenFindGoodyPlan {
        composition_digest: [0; 32],
        set_idle: set_idle.clone(),
        source_sim_sha256,
        after_local: set_idle.after_local.clone(),
        row,
        unit_world_x,
        unit_world_y,
        home_region,
        order_target_x,
        order_target_y,
        order_target_world_x: wcoord_of(order_target_x),
        order_target_world_y: wcoord_of(order_target_y),
        item_registry,
        journal,
        restore_mask2_bit8000: true,
        open,
    };
    plan.composition_digest = plan_digest(&plan);
    plan
}

/// Execute the read-only `Unit::find_goody_box` body up to its first unowned child.
///
/// Retail's shipped control flow is deliberately preserved: an item-marked cell first needs
/// one successful `was_seen` probe.  Only then does the water gate run; on ordinary land,
/// `find_goody_at < 0` accepts the cell, while a found item must pass `ItemData::is_seen`.
/// This differs from the older generic helper's inferred "item visibility fallback" shape.
#[allow(clippy::too_many_arguments)]
pub fn plan_frame1_citizen_find_goody(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    set_idle: &Frame1CitizenSetIdlePlan,
) -> Result<Frame1CitizenFindGoodyPlan, Frame1CitizenFindGoodyError> {
    plan_frame1_citizen_find_goody_with_regional_authority(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        set_idle,
        None,
    )
}

/// Re-run the exact read-only body with one independently bound regional-registry answer.
/// The public continuation validates the answer and its predecessor before entering here.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_frame1_citizen_find_goody_with_regional_authority(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    set_idle: &Frame1CitizenSetIdlePlan,
    regional: Option<&Frame1CitizenRegionSeenAuthority>,
) -> Result<Frame1CitizenFindGoodyPlan, Frame1CitizenFindGoodyError> {
    validate_frame1_citizen_set_idle_plan(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        set_idle,
    )?;
    let Frame1CitizenSetIdleOpenRequest::FindGoodyBox(request) = &set_idle.open;
    if !request_matches(set_idle, request) {
        return Err(Frame1CitizenFindGoodyError::FindGoodyRequestMismatch);
    }
    if !set_idle.restore_mask2_bit8000 || set_idle.after_local.unit_masks2 & 0x8000 != 0 {
        return Err(Frame1CitizenFindGoodyError::RestoreNotArmed);
    }
    if set_idle.after_local.type_facts.domain != 0 {
        return Err(Frame1CitizenFindGoodyError::UnsupportedDomain {
            domain: set_idle.after_local.type_facts.domain,
        });
    }

    let source_sim_sha256 = sha256(&save_sim(set_anim_return)?);
    let item_registry = match set_anim_return.world.item_runtime.as_ref() {
        None => Frame1CitizenItemRegistryAuthority::AbsentAndMapMarkerFree,
        Some(runtime) => Frame1CitizenItemRegistryAuthority::PresentSaveValidated {
            logical_length: runtime.items().len(),
            map_shape: [set_anim_return.map.world.xs, set_anim_return.map.world.ys],
        },
    };
    if source_sim_sha256
        != set_idle
            .think_suffix
            .think
            .authority
            .set_anim_return_sim_sha256
    {
        return Err(Frame1CitizenFindGoodyError::StaleActor);
    }
    let row = set_anim_return
        .world
        .row_of(request.unit)
        .ok_or(Frame1CitizenFindGoodyError::StaleActor)?;
    let units = &set_anim_return.world.units;
    if units.get_who(row) != request.who
        || units.o()[row] != request.o
        || set_anim_return.world.unit_type_id(row) != Some(request.type_index)
        || units.x_internal()[row] != request.stored_x
        || units.y_internal()[row] != request.stored_y
        || (units.x_internal()[row] ^ crate::setup_2024_frame1_citizen_think::COORD_XOR)
            != request.decoded_x
        || (units.y_internal()[row] ^ crate::setup_2024_frame1_citizen_think::COORD_XOR)
            != request.decoded_y
    {
        return Err(Frame1CitizenFindGoodyError::CoordinateMismatch);
    }

    let unit_world_x = wcoord_of(request.decoded_x);
    let unit_world_y = wcoord_of(request.decoded_y);
    let world = &set_anim_return.map.world;
    if !world.valid_w(unit_world_x, unit_world_y) {
        return Err(Frame1CitizenFindGoodyError::ActorOutOfBounds {
            world_x: unit_world_x,
            world_y: unit_world_y,
        });
    }
    let home_region = world.wdata(unit_world_x, unit_world_y).region;
    let order_target_x = units.orders_x()[row];
    let order_target_y = units.orders_y()[row];
    let order_world_x = wcoord_of(order_target_x);
    let order_world_y = wcoord_of(order_target_y);
    let random_state = set_anim_return.world.random.state();
    let mut journal = Vec::with_capacity(GOODY_SEARCH_CELLS);

    for ordinal in 0..GOODY_SEARCH_CELLS {
        let world_x = unit_world_x + MOVE_X[ordinal];
        let world_y = unit_world_y + MOVE_Y[ordinal];
        let mut read = Frame1CitizenFindGoodyCellRead {
            ordinal,
            offset_x: MOVE_X[ordinal],
            offset_y: MOVE_Y[ordinal],
            world_x,
            world_y,
            world_index: None,
            region: None,
            flags: None,
            fog_probes: Vec::new(),
            land: None,
            item_lookup: None,
            decision: Frame1CitizenFindGoodyCellDecision::OutOfBounds,
        };
        if !world.valid_w(world_x, world_y) {
            journal.push(read);
            continue;
        }
        let world_index = world.w_index(world_x, world_y);
        let cell = &world.wdata[world_index];
        read.world_index = Some(world_index);
        read.region = Some(cell.region);
        if cell.region != home_region {
            read.decision = Frame1CitizenFindGoodyCellDecision::DifferentRegion;
            journal.push(read);
            continue;
        }
        read.flags = Some(cell.flags);
        if cell.flags & WFLAG_ITEM == 0 {
            read.decision = Frame1CitizenFindGoodyCellDecision::NoItemMarker;
            journal.push(read);
            continue;
        }

        let probes = [
            (
                UNIT_FIND_GOODY_FIRST_WAS_SEEN_CALL_VA,
                world_x * 2 + 1,
                world_y * 2 + 1,
            ),
            (
                UNIT_FIND_GOODY_SECOND_WAS_SEEN_CALL_VA,
                world_x * 2,
                world_y * 2 + 1,
            ),
            (
                UNIT_FIND_GOODY_THIRD_WAS_SEEN_CALL_VA,
                world_x * 2 + 1,
                world_y * 2,
            ),
            (
                UNIT_FIND_GOODY_FOURTH_WAS_SEEN_CALL_VA,
                world_x * 2,
                world_y * 2,
            ),
        ];
        let mut discovered = false;
        for (call_va, fog_x, fog_y) in probes {
            match was_seen(
                set_anim_return,
                call_va,
                fog_x,
                fog_y,
                request.who,
                regional,
            )? {
                WasSeenResult::Resolved(probe) => {
                    discovered = probe.result;
                    read.fog_probes.push(probe);
                    if discovered {
                        break;
                    }
                }
                WasSeenResult::NeedsRegion {
                    territory_owner,
                    region,
                    fog_x,
                    fog_y,
                } => {
                    read.decision = Frame1CitizenFindGoodyCellDecision::NeedsRegionSeen;
                    journal.push(read);
                    let mut request_open = Frame1CitizenRegionSeenRequest {
                        request_sha256: [0; 32],
                        parent_digest: set_idle.composition_digest,
                        scan_prefix_sha256: scan_prefix_digest(&journal),
                        body_va: WORLD_WAS_SEEN_VA,
                        frame: request.frame,
                        unit: request.unit,
                        player: request.who,
                        fog_x,
                        fog_y,
                        territory_owner,
                        region,
                        reg_cities: 0,
                        needed_field_offset: 0x12de,
                        restore_mask2_bit8000: true,
                    };
                    request_open.request_sha256 =
                        frame1_citizen_region_seen_request_digest(&request_open);
                    return Ok(finish_plan(
                        set_idle,
                        source_sim_sha256,
                        row,
                        unit_world_x,
                        unit_world_y,
                        home_region,
                        order_target_x,
                        order_target_y,
                        item_registry,
                        journal,
                        Frame1CitizenFindGoodyOpenRequest::RegionSeen(request_open),
                    ));
                }
            }
        }
        if !discovered {
            read.decision = Frame1CitizenFindGoodyCellDecision::NotExplored;
            journal.push(read);
            continue;
        }

        read.land = Some(cell.land);
        let accepted = if cell.flags & WFLAG_OVERRIDE_LAND == 0
            && (cell.land == LAND_REJECT_A || cell.land == LAND_REJECT_B)
        {
            read.decision = Frame1CitizenFindGoodyCellDecision::AcceptedWaterGate;
            true
        } else {
            let mut lookup = item_lookup(set_anim_return, world_x, world_y)?;
            match lookup.item {
                None => {
                    lookup.visible = None;
                    read.decision = Frame1CitizenFindGoodyCellDecision::AcceptedNoItem;
                    read.item_lookup = Some(lookup);
                    true
                }
                Some(item_read) => {
                    let vision_mask =
                        set_anim_return.map.fog.leaders[usize::from(request.who)].player_mask;
                    let mut visible = item_read.ever_seen & vision_mask != 0;
                    if !visible {
                        let spanish = set_anim_return.step8.leaders[usize::from(request.who)]
                            .unit_stats
                            .has_tribe_bonus(TRIBE_BONUS_SPANISH_RUINS as u32);
                        let item_world_x = wcoord_of(item_read.x);
                        let item_world_y = wcoord_of(item_read.y);
                        if spanish
                            && world.valid_w(item_world_x, item_world_y)
                            && world.wdata(item_world_x, item_world_y).who == request.who as i8
                        {
                            match was_seen(
                                set_anim_return,
                                ITEM_WAS_SEEN_CALL_VA,
                                fcoord_of(item_read.x),
                                fcoord_of(item_read.y),
                                request.who,
                                regional,
                            )? {
                                WasSeenResult::Resolved(item_seen) => {
                                    visible = item_seen.result;
                                    lookup.item_visibility.push(item_seen);
                                }
                                WasSeenResult::NeedsRegion {
                                    territory_owner,
                                    region,
                                    fog_x,
                                    fog_y,
                                } => {
                                    read.item_lookup = Some(lookup);
                                    read.decision =
                                        Frame1CitizenFindGoodyCellDecision::NeedsRegionSeen;
                                    journal.push(read);
                                    let mut request_open = Frame1CitizenRegionSeenRequest {
                                        request_sha256: [0; 32],
                                        parent_digest: set_idle.composition_digest,
                                        scan_prefix_sha256: scan_prefix_digest(&journal),
                                        body_va: WORLD_WAS_SEEN_VA,
                                        frame: request.frame,
                                        unit: request.unit,
                                        player: request.who,
                                        fog_x,
                                        fog_y,
                                        territory_owner,
                                        region,
                                        reg_cities: 0,
                                        needed_field_offset: 0x12de,
                                        restore_mask2_bit8000: true,
                                    };
                                    request_open.request_sha256 =
                                        frame1_citizen_region_seen_request_digest(&request_open);
                                    return Ok(finish_plan(
                                        set_idle,
                                        source_sim_sha256,
                                        row,
                                        unit_world_x,
                                        unit_world_y,
                                        home_region,
                                        order_target_x,
                                        order_target_y,
                                        item_registry,
                                        journal,
                                        Frame1CitizenFindGoodyOpenRequest::RegionSeen(request_open),
                                    ));
                                }
                            }
                        } else {
                            let item_seen = is_seen(
                                set_anim_return,
                                fcoord_of(item_read.x),
                                fcoord_of(item_read.y),
                                request.who,
                            )?;
                            visible = item_seen.result;
                            lookup.item_visibility.push(item_seen);
                        }
                    }
                    lookup.visible = Some(visible);
                    read.decision = if visible {
                        Frame1CitizenFindGoodyCellDecision::AcceptedVisibleItem
                    } else {
                        Frame1CitizenFindGoodyCellDecision::HiddenItem
                    };
                    read.item_lookup = Some(lookup);
                    visible
                }
            }
        };
        journal.push(read);
        if !accepted {
            continue;
        }
        if world_x == order_world_x && world_y == order_world_y {
            return Ok(finish_plan(
                set_idle,
                source_sim_sha256,
                row,
                unit_world_x,
                unit_world_y,
                home_region,
                order_target_x,
                order_target_y,
                item_registry,
                journal,
                false_open(
                    set_idle.composition_digest,
                    Frame1CitizenFindGoodyFalseReason::AlreadyOrderTarget,
                    random_state,
                ),
            ));
        }
        let mut request_open = Frame1CitizenGetGoodyBoxRequest {
            request_sha256: [0; 32],
            parent_digest: set_idle.composition_digest,
            scan_prefix_sha256: scan_prefix_digest(&journal),
            call_va: UNIT_GET_GOODY_BOX_CALL_VA,
            body_va: UNIT_GET_GOODY_BOX_VA,
            frame: request.frame,
            unit: request.unit,
            who: request.who,
            o: request.o,
            world_x,
            world_y,
            target_fine_x: world_x * 0x300 + 0x180,
            target_fine_y: world_y * 0x300 + 0x180,
            order_target_x_before: order_target_x,
            order_target_y_before: order_target_y,
            group_clear_va: GROUP_CLEAR_VA,
            group_add_va: GROUP_ADD_VA,
            groups_push_group_va: GROUPS_PUSH_GROUP_VA,
            group_action_move_to_va: GROUP_ACTION_MOVE_TO_VA,
            queue_pos: 0,
            order_index: 3,
            after_find_goody_prefix: set_idle.after_local.clone(),
            restore_mask2_bit8000: true,
        };
        request_open.request_sha256 = frame1_citizen_get_goody_box_request_digest(&request_open);
        return Ok(finish_plan(
            set_idle,
            source_sim_sha256,
            row,
            unit_world_x,
            unit_world_y,
            home_region,
            order_target_x,
            order_target_y,
            item_registry,
            journal,
            Frame1CitizenFindGoodyOpenRequest::GetGoodyBox(request_open),
        ));
    }

    Ok(finish_plan(
        set_idle,
        source_sim_sha256,
        row,
        unit_world_x,
        unit_world_y,
        home_region,
        order_target_x,
        order_target_y,
        item_registry,
        journal,
        false_open(
            set_idle.composition_digest,
            Frame1CitizenFindGoodyFalseReason::ExhaustedSpiral,
            random_state,
        ),
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn validate_frame1_citizen_find_goody_plan(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    post_authority: &Frame1PostCommandAuthority,
    post_command: &Sim,
    set_anim_return: &Sim,
    entry_authority: &GoldenFrame1EntryAuthority,
    plan: &Frame1CitizenFindGoodyPlan,
) -> Result<(), Frame1CitizenFindGoodyError> {
    let expected = plan_frame1_citizen_find_goody(
        replay,
        setup_entry,
        post_authority,
        post_command,
        set_anim_return,
        entry_authority,
        &plan.set_idle,
    )?;
    if &expected != plan {
        return Err(Frame1CitizenFindGoodyError::StalePlan);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_2024_frame1_citizen_find_goody_exact_addresses_and_order() {
        assert_eq!(UNIT_FIND_GOODY_BOX_VA, 0x005f_2540);
        assert_eq!(WORLD_WAS_SEEN_VA, 0x006b_53f0);
        assert_eq!(OBJECTS_FIND_GOODY_AT_VA, 0x0065_c040);
        assert_eq!(ITEM_IS_SEEN_VA, 0x0067_7850);
        assert_eq!(UNIT_GET_GOODY_BOX_VA, 0x005f_7690);
        assert_eq!(GOODY_SEARCH_CELLS, 49);
        assert_eq!((MOVE_X[0], MOVE_Y[0]), (0, 0));
        assert_eq!((MOVE_X[48], MOVE_Y[48]), (-3, -2));
        assert_eq!(
            [(11, 15), (10, 15), (11, 14), (10, 14)],
            [
                (5 * 2 + 1, 7 * 2 + 1),
                (5 * 2, 7 * 2 + 1),
                (5 * 2 + 1, 7 * 2),
                (5 * 2, 7 * 2),
            ]
        );
    }

    #[test]
    fn setup_2024_frame1_citizen_find_goody_children_stop_before_groups() {
        assert_eq!(UNIT_GET_GOODY_BOX_CALL_VA, 0x005f_2780);
        assert_eq!(GROUP_CLEAR_VA, 0x0071_3e80);
        assert_eq!(GROUP_ADD_VA, 0x0071_4350);
        assert_eq!(GROUPS_PUSH_GROUP_VA, 0x0070_f9e0);
        assert_eq!(GROUP_ACTION_MOVE_TO_VA, 0x0070_fba0);
        assert_eq!(UNIT_SET_IDLE_AFTER_FIND_GOODY_VA, 0x005f_6039);
    }

    #[test]
    fn setup_2024_frame1_citizen_find_goody_region_request_binds_scan_prefix() {
        let mut request = Frame1CitizenRegionSeenRequest {
            request_sha256: [0; 32],
            parent_digest: [0x11; 32],
            scan_prefix_sha256: [0x22; 32],
            body_va: WORLD_WAS_SEEN_VA,
            frame: 1,
            unit: Handle {
                id: 7,
                generation: 3,
            },
            player: 0,
            fog_x: 19,
            fog_y: 23,
            territory_owner: 0,
            region: 4,
            reg_cities: 0,
            needed_field_offset: 0x12de,
            restore_mask2_bit8000: true,
        };
        request.request_sha256 = frame1_citizen_region_seen_request_digest(&request);
        assert_ne!(request.request_sha256, [0; 32]);
        let digest = request.request_sha256;
        request.scan_prefix_sha256[0] ^= 1;
        assert_ne!(frame1_citizen_region_seen_request_digest(&request), digest);
    }
}
