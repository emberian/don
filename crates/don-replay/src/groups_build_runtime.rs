//! Atomic replay host for opcode 25, `Group::action_build`.
//!
//! Retail does not enqueue production here.  `CommandPackage::process_build` delegates to
//! `Group::action_build`, which validates and snaps a construction site, calls
//! `Objects::init_build`, appends the new Build to both the WData object list and its City's
//! `BuildData::city_down` list, then installs QueueLast MOVE_TO/EXPLORE_TO -> BUILD_AT orders on
//! each admitted builder.  This module keeps that closure indivisible.
//!
//! Search, type, terrain, and complete `Build::init` facts are deliberately supplied by a
//! revisioned authority.  The compact replay state cannot derive those facts.  Missing or
//! mismatched authority refuses before Build allocation; this host never manufactures a Farm
//! body or treats a registry-only append as substantive construction.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::order::{MoveOrderState, Order, OrderIndex, ORDER_GROUP};
use don_sim::systems::canonical_group_move_host::{
    groups_equal, prepare_group_selection, unit_still_current, GroupSelectionUse, PackageError,
    UnitIdentity, NETWORK_PLAYERS, RECEIVED_SELECTION_CAPACITY,
};
use don_sim::systems::groups_guys::CheckSum;
use don_sim::systems::map_terrain::{Coord, WCoord};
use don_sim::systems::production::BuildData;
use don_sim::systems::tech_cities::CityRecord;
use don_sim::tick::Sim;
use don_sim::world::Handle;

use crate::build_spawn_runtime::{
    preview_canonical_build_spawn, spawn_canonical_build, CanonicalBuildSpawnError,
    CanonicalBuildSpawnReceipt, CanonicalBuildSpawnRequest,
};
use crate::groups_build_history::{
    decode_queue_up_build, GroupBuildHistoryError, QueueUpBuildWire, QUEUE_UP_BUILD_OPCODE,
};

pub const GROUP_ACTION_BUILD_VA: u32 = 0x0070_7510;
pub const OBJECTS_INIT_BUILD_VA: u32 = 0x0065_d190;
pub const BUILD_INIT_VA: u32 = 0x0062_9740;
pub const OBJECT_ADD_TO_WORLD_VA: u32 = 0x0064_d8c0;
pub const BUILD_ADD_TO_CITY_VA: u32 = 0x0062_2380;
pub const GROUP_ACTION_SWARM_AROUND_VA: u32 = 0x0070_fbe0;
pub const UNIT_ADD_MOVE_FACING_ORDER_VA: u32 = 0x005e_55c0;
pub const UNIT_ADD_BUILD_ORDER_VA: u32 = 0x005e_5210;

pub const FARM_TYPE: i32 = 0x1a1;
pub const QUEUE_LAST: i32 = 1;
pub const BUILD_AT_UNIT_MASK: u32 = 0x0000_0400;
pub const BUILD_CROSS_REGION_UNIT_MASK: u32 = 0x0080_0000;
pub const EXPLORE_MOVE_UNIT_MASK: u32 = 0x0400_0000;
pub const BUILD_BAD_PATH_MASK: u16 = 0x2000;

const OBJECT_UP: usize = 0x2a;
const OBJECT_DOWN: usize = 0x2c;
const OBJECT_DOWN_WHO: usize = 0x2e;

/// The native functions which produced one installed placement after-image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementAuthoritySource {
    BuildTypeValidateSnapAndBlockedSite,
}

/// The native initializer which produced the staged, complete Build body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildBodyAuthoritySource {
    ObjectsInitBuildPeAfterImage,
}

/// One exact `snap_center`/`blocked_site` probe in retail's candidate chronology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementProbeAuthority {
    pub raw: (i32, i32),
    pub snapped: (i32, i32),
    pub blocked_site: i32,
}

/// One exact node in the center Build's append-only `city_down` chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityBuildChainNode {
    pub row: usize,
    pub o: i16,
    pub city_down: i16,
}

/// Exact `Build::find_city` result plus the chain walked by `Build::add_to_city`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CityBuildChainAuthority {
    pub owner: u8,
    pub city_slot: i16,
    pub city_before: CityRecord,
    pub chain: Vec<CityBuildChainNode>,
}

/// Exact World cell reached by `Object::add_to_world` for the snapped Build center.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldObjectLinkAuthority {
    pub wcoord: (i32, i32),
    pub head_before: (i16, i16),
}

/// Exact search result and after-image for one QueueLast builder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuilderSwarmAuthority {
    pub actor: UnitIdentity,
    /// First `UnitType::find_nearby_spot` result.
    pub first_nearby_spot: (i32, i32),
    /// BUILD_AT's optional second, footprint-offset search result.
    pub second_nearby_spot: Option<(i32, i32)>,
    /// Centered UCoord destination passed to `Unit::add_move_facing_order`.
    pub final_destination: (i32, i32),
    pub facing_angle: i32,
    pub move_order: Order,
    pub build_order: Order,
    pub unit_masks_after: u32,
    pub orders_x_after: i32,
    pub orders_y_after: i32,
    pub dest_angle_after: i32,
}

/// One exact action-specific authority entry.  Entries are selected by every opcode-25 wire
/// field, so a source for one of the eight 2018 packages cannot authorize another.
#[derive(Clone, Debug)]
pub struct GroupBuildPlacementAuthority {
    pub owner: u8,
    pub requested: QueueUpBuildWire,
    pub source: PlacementAuthoritySource,
    pub probes: Vec<PlacementProbeAuthority>,
    pub chosen_probe: usize,
    pub world_link: WorldObjectLinkAuthority,
    pub city: CityBuildChainAuthority,
    pub build_source: BuildBodyAuthoritySource,
    /// Complete post-`Build::init`, pre-City-link Build body. Identity and position are stamped
    /// by `spawn_canonical_build`; `city` and `city_down` must still be `-1`.
    pub initialized_build: BuildData,
    pub builders: Vec<BuilderSwarmAuthority>,
}

/// Reinstalled content/search adapter.  Gameplay state remains in Sim/World/Groups/Builds.
#[derive(Clone, Debug, Default)]
pub struct GroupBuildRuntimeAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub scenario_ignore_orders: bool,
    pub placements: Vec<GroupBuildPlacementAuthority>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupBuildWire {
    pub who: u8,
    pub objects: Vec<i16>,
    pub action: QueueUpBuildWire,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupBuildPackageReceipt {
    pub play: usize,
    pub lockstep_serial: i32,
    pub frame: i32,
    pub who: u8,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub build: CanonicalBuildSpawnReceipt,
    pub city_slot: i16,
    pub city_tail_row: usize,
    pub world_cell: usize,
    pub installed_move_orders: usize,
    pub installed_build_orders: usize,
    pub command_state_revision: u64,
    pub groups_checksum: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub resources_mutated: bool,
    pub production_queue_mutated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupBuildPackageError {
    Truncated,
    WrongFirstOpcode {
        got: u8,
    },
    WrongActionOpcode {
        got: u8,
    },
    TrailingBytes {
        expected: usize,
        got: usize,
    },
    NegativeObject {
        o: i16,
    },
    Wire(GroupBuildHistoryError),
    Selection(PackageError),
    MissingPlayerMap {
        play: usize,
    },
    PlayerOwnerMismatch {
        play: usize,
        expected: u8,
        got: u8,
    },
    MissingAuthority,
    MissingAuthorityRevision,
    MissingCompositionDigest,
    ScenarioIgnoreOrders,
    UnsupportedType {
        got: i32,
    },
    UnsupportedQueue {
        got: i32,
    },
    InvalidProbeChronology,
    ChosenSiteBlocked {
        code: i32,
    },
    InvalidWorldShape,
    WorldCoordinateMismatch,
    WorldCellOutsideMap {
        wx: i32,
        wy: i32,
    },
    WorldHeadMismatch {
        expected: (i16, i16),
        actual: (i16, i16),
    },
    OccupiedWorldHeadUnsupported {
        head: (i16, i16),
    },
    CityOwnerMismatch,
    CitySlotOutsidePool,
    CityImageMismatch,
    InvalidCityChain,
    MissingCityBuild {
        owner: u8,
        o: i16,
    },
    CityBuildImageMismatch {
        row: usize,
    },
    IncompleteBuildBody,
    Spawn(CanonicalBuildSpawnError),
    BuildAllocationMismatch,
    BuilderAuthorityMismatch,
    InvalidMoveOrder {
        handle: Handle,
    },
    InvalidBuildOrder {
        handle: Handle,
    },
    InvalidUnitMaskAfter {
        handle: Handle,
    },
    StaleUnitDestination {
        handle: Handle,
    },
}

impl fmt::Display for GroupBuildPackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "opcode-25 construction transaction refused: {self:?}")
    }
}

impl std::error::Error for GroupBuildPackageError {}

impl From<PackageError> for GroupBuildPackageError {
    fn from(value: PackageError) -> Self {
        Self::Selection(value)
    }
}

impl From<CanonicalBuildSpawnError> for GroupBuildPackageError {
    fn from(value: CanonicalBuildSpawnError) -> Self {
        Self::Spawn(value)
    }
}

fn i16_at(bytes: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

/// Decode exactly `[GroupCommand][QueueUpBuildCommand]`, with no package shell or sibling action.
pub fn decode_group_build_package(bytes: &[u8]) -> Result<GroupBuildWire, GroupBuildPackageError> {
    let Some(&first) = bytes.first() else {
        return Err(GroupBuildPackageError::Truncated);
    };
    if first != 0 {
        return Err(GroupBuildPackageError::WrongFirstOpcode { got: first });
    }
    let Some(&count) = bytes.get(1) else {
        return Err(GroupBuildPackageError::Truncated);
    };
    if usize::from(count) > RECEIVED_SELECTION_CAPACITY {
        return Err(PackageError::ExplicitSelectionTooLong {
            len: usize::from(count),
        }
        .into());
    }
    let Some(&who) = bytes.get(2) else {
        return Err(GroupBuildPackageError::Truncated);
    };
    let group_len = 3usize + usize::from(count) * 2;
    let Some(&opcode) = bytes.get(group_len) else {
        return Err(GroupBuildPackageError::Truncated);
    };
    if opcode != QUEUE_UP_BUILD_OPCODE {
        return Err(GroupBuildPackageError::WrongActionOpcode { got: opcode });
    }
    let expected = group_len + crate::groups_build_history::QUEUE_UP_BUILD_WIRE_SIZE;
    if bytes.len() < expected {
        return Err(GroupBuildPackageError::Truncated);
    }
    if bytes.len() != expected {
        return Err(GroupBuildPackageError::TrailingBytes {
            expected,
            got: bytes.len(),
        });
    }
    let mut objects = Vec::with_capacity(usize::from(count));
    for index in 0..usize::from(count) {
        let o = i16_at(bytes, 3 + index * 2).ok_or(GroupBuildPackageError::Truncated)?;
        if o < 0 {
            return Err(GroupBuildPackageError::NegativeObject { o });
        }
        objects.push(o);
    }
    let action =
        decode_queue_up_build(&bytes[group_len..]).map_err(GroupBuildPackageError::Wire)?;
    Ok(GroupBuildWire {
        who,
        objects,
        action,
    })
}

fn build_row(sim: &Sim, owner: u8, o: i16) -> Option<usize> {
    let slot = i32::from(o).checked_sub(BUILD_BAND_BASE as i32)?;
    let slot = usize::try_from(slot).ok()?;
    sim.world
        .objects
        .slot(usize::from(owner))
        .band(Band::Build)
        .get(slot)
        .copied()
        .map(|row| row as usize)
}

fn write_i16(image: &mut [u8], at: usize, value: i16) {
    image[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn validate_city_chain(
    sim: &Sim,
    placement: &GroupBuildPlacementAuthority,
) -> Result<usize, GroupBuildPackageError> {
    let city = &placement.city;
    if city.owner != placement.owner || city.city_before.who != placement.owner as i8 {
        return Err(GroupBuildPackageError::CityOwnerMismatch);
    }
    let city_slot = usize::try_from(city.city_slot)
        .ok()
        .filter(|slot| *slot < sim.cities.slots[usize::from(city.owner)].len())
        .ok_or(GroupBuildPackageError::CitySlotOutsidePool)?;
    let current = &sim.cities.slots[usize::from(city.owner)][city_slot];
    if current != &city.city_before
        || !current.active()
        || current.city != city.city_slot
        || current.o < 0
    {
        return Err(GroupBuildPackageError::CityImageMismatch);
    }
    if city.chain.is_empty() || city.chain[0].o != current.o {
        return Err(GroupBuildPackageError::InvalidCityChain);
    }
    for (index, node) in city.chain.iter().enumerate() {
        let row =
            build_row(sim, city.owner, node.o).ok_or(GroupBuildPackageError::MissingCityBuild {
                owner: city.owner,
                o: node.o,
            })?;
        let build = sim
            .builds
            .get(row)
            .ok_or(GroupBuildPackageError::MissingCityBuild {
                owner: city.owner,
                o: node.o,
            })?;
        let expected_next = city.chain.get(index + 1).map_or(-1, |next| next.o);
        if row != node.row
            || build.who != city.owner
            || build.object_id() != node.o
            || build.city != city.city_slot
            || build.city_down != node.city_down
            || node.city_down != expected_next
        {
            return Err(GroupBuildPackageError::CityBuildImageMismatch { row });
        }
    }
    Ok(city.chain.last().expect("nonempty chain").row)
}

fn validate_move_order(builder: &BuilderSwarmAuthority) -> Result<(), GroupBuildPackageError> {
    let order = &builder.move_order;
    let state = order
        .move_state
        .ok_or(GroupBuildPackageError::InvalidMoveOrder {
            handle: builder.actor.handle,
        })?;
    let final_spot = builder
        .second_nearby_spot
        .unwrap_or(builder.first_nearby_spot);
    if !matches!(order.kind, OrderIndex::MoveTo | OrderIndex::ExploreTo)
        || order.flags != 0
        || (order.x, order.y) != builder.final_destination
        || final_spot == (i32::MIN, i32::MIN)
        || state.angle != builder.facing_angle
        || (state.dest_x, state.dest_y) != builder.final_destination
        || state.last_x != -1
        || state.last_y != -1
        || state.orig_x != -1
        || state.orig_y != -1
        || state.timer != -1
    {
        return Err(GroupBuildPackageError::InvalidMoveOrder {
            handle: builder.actor.handle,
        });
    }
    Ok(())
}

fn validate_build_order(
    builder: &BuilderSwarmAuthority,
    owner: u8,
    object_id: i16,
    uid: u16,
) -> Result<(), GroupBuildPackageError> {
    let order = &builder.build_order;
    if order.kind != OrderIndex::BuildAt
        || order.flags != ORDER_GROUP
        || order.target_who != owner as i8
        || order.target_o != object_id
        || order.target_uid != uid
        || order.target_handle.is_some()
        || order.move_state.is_some()
    {
        return Err(GroupBuildPackageError::InvalidBuildOrder {
            handle: builder.actor.handle,
        });
    }
    Ok(())
}

fn placement_for<'a>(
    authority: &'a GroupBuildRuntimeAuthority,
    wire: &GroupBuildWire,
) -> Result<&'a GroupBuildPlacementAuthority, GroupBuildPackageError> {
    let mut matches = authority
        .placements
        .iter()
        .filter(|placement| placement.owner == wire.who && placement.requested == wire.action);
    let placement = matches
        .next()
        .ok_or(GroupBuildPackageError::MissingAuthority)?;
    if matches.next().is_some() {
        return Err(GroupBuildPackageError::MissingAuthority);
    }
    Ok(placement)
}

/// Execute one exact opcode-25 package against canonical Sim owners.
///
/// All parsing, selection, search/body authority, City chain, WData occupancy, Build allocation,
/// and builder after-images are checked before `spawn_canonical_build`.  That call has no mutating
/// error arm after its own preflight; every remaining operation is an infallible assignment.
#[allow(clippy::too_many_lines)]
pub fn process_group_build_package(
    sim: &mut Sim,
    authority: &GroupBuildRuntimeAuthority,
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<GroupBuildPackageReceipt, GroupBuildPackageError> {
    let wire = decode_group_build_package(bytes)?;
    if authority.revision == 0 {
        return Err(GroupBuildPackageError::MissingAuthorityRevision);
    }
    if authority.composition_digest == [0; 32] {
        return Err(GroupBuildPackageError::MissingCompositionDigest);
    }
    if authority.scenario_ignore_orders {
        return Err(GroupBuildPackageError::ScenarioIgnoreOrders);
    }
    if wire.action.type_index != FARM_TYPE {
        return Err(GroupBuildPackageError::UnsupportedType {
            got: wire.action.type_index,
        });
    }
    if wire.action.queued != QUEUE_LAST {
        return Err(GroupBuildPackageError::UnsupportedQueue {
            got: wire.action.queued,
        });
    }
    let player_who: [Option<u8>; NETWORK_PLAYERS] = std::array::from_fn(|slot| {
        sim.players.as_ref().and_then(|players| {
            let row = players.players[slot];
            (usize::from(row.play) == slot
                && row.flags & don_sim::systems::player_lifecycle_tails::PLAYER_PRESENT != 0)
                .then_some(row.who)
        })
    });
    let expected = *player_who
        .get(play)
        .ok_or(GroupBuildPackageError::MissingPlayerMap { play })?
        .as_ref()
        .ok_or(GroupBuildPackageError::MissingPlayerMap { play })?;
    if expected != wire.who {
        return Err(GroupBuildPackageError::PlayerOwnerMismatch {
            play,
            expected,
            got: wire.who,
        });
    }

    let placement = placement_for(authority, &wire)?;
    let chosen = placement
        .probes
        .get(placement.chosen_probe)
        .ok_or(GroupBuildPackageError::InvalidProbeChronology)?;
    if placement.probes.is_empty()
        || placement.source != PlacementAuthoritySource::BuildTypeValidateSnapAndBlockedSite
        || placement.probes[0].raw != (wire.action.x, wire.action.y)
        || placement.probes[..placement.chosen_probe]
            .iter()
            .any(|probe| probe.blocked_site == 0)
    {
        return Err(GroupBuildPackageError::InvalidProbeChronology);
    }
    if chosen.blocked_site != 0 {
        return Err(GroupBuildPackageError::ChosenSiteBlocked {
            code: chosen.blocked_site,
        });
    }
    if placement.build_source != BuildBodyAuthoritySource::ObjectsInitBuildPeAfterImage
        || placement.initialized_build.city != -1
        || placement.initialized_build.city_down != -1
        || placement.initialized_build.orig_type != FARM_TYPE
        || !placement.initialized_build.is_valid()
    {
        return Err(GroupBuildPackageError::IncompleteBuildBody);
    }

    let selection = prepare_group_selection(
        &sim.world,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        sim.world.frame,
        play,
        wire.who,
        &wire.objects,
        GroupSelectionUse::ConstructionPlacement,
    )?;
    if selection.members.iter().any(|member| {
        !member.authority.on_map
            || !member.authority.is_captain
            || !member.authority.can_move
            || !member.authority.can_install_order
    }) {
        return Err(GroupBuildPackageError::BuilderAuthorityMismatch);
    }
    if placement.builders.len() != selection.members.len() {
        return Err(GroupBuildPackageError::BuilderAuthorityMismatch);
    }

    let request = CanonicalBuildSpawnRequest {
        owner: wire.who,
        type_index: wire.action.type_index,
        snapped_x: chosen.snapped.0,
        snapped_y: chosen.snapped.1,
        build: placement.initialized_build.clone(),
    };
    let preview = preview_canonical_build_spawn(sim, &request)?;
    let tail_row = validate_city_chain(sim, placement)?;

    let world = &sim.map.world;
    if world.xs <= 0
        || world.ys <= 0
        || world.wdata.len() != (world.xs as usize).saturating_mul(world.ys as usize)
    {
        return Err(GroupBuildPackageError::InvalidWorldShape);
    }
    let derived_wcoord = (
        WCoord::from_coord(Coord(chosen.snapped.0)).0,
        WCoord::from_coord(Coord(chosen.snapped.1)).0,
    );
    if placement.world_link.wcoord != derived_wcoord {
        return Err(GroupBuildPackageError::WorldCoordinateMismatch);
    }
    let (wx, wy) = derived_wcoord;
    if !world.valid_w(wx, wy) {
        return Err(GroupBuildPackageError::WorldCellOutsideMap { wx, wy });
    }
    let world_cell = world.w_index(wx, wy);
    let actual_head = (
        world.wdata[world_cell].down,
        world.wdata[world_cell].down_who,
    );
    if actual_head != placement.world_link.head_before {
        return Err(GroupBuildPackageError::WorldHeadMismatch {
            expected: placement.world_link.head_before,
            actual: actual_head,
        });
    }
    // A chosen blocked_site result should never land on another object. Supporting a nonempty
    // head would require all-band `ObjectData::up/up_who` mutation; refuse instead of dropping it.
    if actual_head.0 != -1 {
        return Err(GroupBuildPackageError::OccupiedWorldHeadUnsupported { head: actual_head });
    }

    let target_uid = placement.initialized_build.uid;
    let mut selection = selection;
    let mut destinations = Vec::with_capacity(placement.builders.len());
    for (member, builder) in selection.members.iter().zip(&placement.builders) {
        if builder.actor != member.identity {
            return Err(GroupBuildPackageError::BuilderAuthorityMismatch);
        }
        validate_move_order(builder)?;
        validate_build_order(builder, wire.who, preview.object_id, target_uid)?;
        let mutation = selection
            .units
            .iter_mut()
            .find(|mutation| mutation.after.identity == builder.actor)
            .ok_or(GroupBuildPackageError::BuilderAuthorityMismatch)?;
        let allowed_additions =
            BUILD_AT_UNIT_MASK | BUILD_CROSS_REGION_UNIT_MASK | EXPLORE_MOVE_UNIT_MASK;
        if builder.unit_masks_after & BUILD_AT_UNIT_MASK == 0
            || builder.unit_masks_after & mutation.after.unit_masks != mutation.after.unit_masks
            || builder.unit_masks_after & !mutation.after.unit_masks & !allowed_additions != 0
        {
            return Err(GroupBuildPackageError::InvalidUnitMaskAfter {
                handle: builder.actor.handle,
            });
        }
        mutation.after.orders.push(builder.move_order.clone());
        mutation.after.orders.push(builder.build_order.clone());
        mutation.after.unit_masks = builder.unit_masks_after;
        mutation.after.orders_x = builder.orders_x_after;
        mutation.after.orders_y = builder.orders_y_after;
        destinations.push((
            builder.actor.handle,
            sim.world.units.dest_angle()[member.row],
            builder.dest_angle_after,
        ));
    }
    selection.groups_after.list[selection.group_slot].disband = 0;
    selection.groups_after.list[selection.group_slot].form = -1;

    // Final revalidation before the sole publication point.
    if sim.command_package_state != selection.command_state_before {
        return Err(PackageError::StaleCommandState.into());
    }
    if !groups_equal(&sim.groups, &selection.groups_before) {
        return Err(PackageError::StaleGroups.into());
    }
    if sim.group_move_authority.revision != selection.authority_revision
        || sim.group_move_authority.composition_digest != selection.authority_digest
        || sim.group_move_authority.members != selection.authority_members
    {
        return Err(PackageError::StaleAuthority.into());
    }
    for mutation in &selection.units {
        if !unit_still_current(&sim.world, &sim.paths, &mutation.before) {
            return Err(PackageError::StaleUnit {
                handle: mutation.before.identity.handle,
            }
            .into());
        }
    }
    for (handle, before, _) in &destinations {
        let row = sim
            .world
            .row_of(*handle)
            .ok_or(GroupBuildPackageError::BuilderAuthorityMismatch)?;
        if sim.world.units.dest_angle()[row] != *before {
            return Err(GroupBuildPackageError::StaleUnitDestination { handle: *handle });
        }
    }

    let random_state = sim.world.random.state();
    let mut groups_checksum = CheckSum::default();
    selection.groups_after.check_groups(&mut groups_checksum);

    // `spawn_canonical_build` reruns the same preflight and has no mutating Err tail. Holding
    // `&mut Sim` across this function makes the preview stable.
    let build = spawn_canonical_build(sim, request)?;
    debug_assert_eq!(build.row, preview.row);
    debug_assert_eq!(build.object_id, preview.object_id);

    // Everything below is assignment-only and cannot refuse.
    sim.groups = selection.groups_after;
    sim.command_package_state = selection.command_state_after;
    for mutation in selection.units {
        let row = sim
            .world
            .row_of(mutation.before.identity.handle)
            .expect("all Unit identities were revalidated before Build publication");
        sim.world.units.group_mut()[row] = mutation.after.group;
        sim.world
            .units
            .set_unit_masks(row, mutation.after.unit_masks);
        sim.world.units.orders_x_mut()[row] = mutation.after.orders_x;
        sim.world.units.orders_y_mut()[row] = mutation.after.orders_y;
        *sim.world.orders_mut(row) = mutation.after.orders;
        sim.paths[row] = mutation.after.path;
    }
    for (handle, _, after) in destinations {
        let row = sim
            .world
            .row_of(handle)
            .expect("all destination identities were revalidated before publication");
        sim.world.units.dest_angle_mut()[row] = after;
    }

    let new_build = &mut sim.builds[build.row];
    write_i16(&mut new_build.other, OBJECT_UP, -1);
    write_i16(&mut new_build.other, OBJECT_DOWN, actual_head.0);
    write_i16(&mut new_build.other, OBJECT_DOWN_WHO, actual_head.1);
    new_build.city = placement.city.city_slot;
    new_build.city_down = -1;
    new_build.build_masks &= !BUILD_BAD_PATH_MASK;
    sim.builds[tail_row].city_down = build.object_id;
    sim.map.world.wdata[world_cell].down = build.object_id;
    sim.map.world.wdata[world_cell].down_who = i16::from(wire.who);

    Ok(GroupBuildPackageReceipt {
        play,
        lockstep_serial,
        frame: sim.world.frame,
        who: wire.who,
        group_slot: selection.group_slot,
        selected: selection
            .members
            .iter()
            .map(|member| member.identity.clone())
            .collect(),
        build,
        city_slot: placement.city.city_slot,
        city_tail_row: tail_row,
        world_cell,
        installed_move_orders: placement.builders.len(),
        installed_build_orders: placement.builders.len(),
        command_state_revision: sim.command_package_state.revision(),
        groups_checksum: groups_checksum.value,
        random_state_before: random_state,
        random_state_after: sim.world.random.state(),
        resources_mutated: false,
        production_queue_mutated: false,
    })
}

/// Construct the exact scalar portion of retail's QueueLast movement order for tests and
/// content adapters. Search still remains mandatory authority.
pub fn queue_last_swarm_move_order(
    kind: OrderIndex,
    destination: (i32, i32),
    facing_angle: i32,
) -> Result<Order, GroupBuildPackageError> {
    if !matches!(kind, OrderIndex::MoveTo | OrderIndex::ExploreTo) {
        return Err(GroupBuildPackageError::InvalidProbeChronology);
    }
    let mut state = MoveOrderState::fresh(destination.0, destination.1);
    state.angle = facing_angle;
    state.timer = -1;
    state.orig_x = -1;
    state.orig_y = -1;
    state.off_x = destination
        .0
        .wrapping_sub((destination.0 / 0x300).wrapping_mul(0x300)) as i16;
    state.off_y = destination
        .1
        .wrapping_sub((destination.1 / 0x300).wrapping_mul(0x300)) as i16;
    Ok(Order {
        kind,
        x: destination.0,
        y: destination.1,
        move_state: Some(state),
        ..Order::default()
    })
}

/// Construct retail's grouped BUILD_AT target node. Build targets use the retail UID but do not
/// have a Unit-generation Handle in the current sparse Build owner.
pub fn queue_last_build_order(owner: u8, object_id: i16, uid: u16) -> Order {
    Order {
        kind: OrderIndex::BuildAt,
        flags: ORDER_GROUP,
        target_who: owner as i8,
        target_o: object_id,
        target_uid: uid,
        ..Order::default()
    }
}
