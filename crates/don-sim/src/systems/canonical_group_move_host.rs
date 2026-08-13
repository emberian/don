// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical, Sim-side transaction core for one `GroupCommand` followed by `MoveToCommand`.
//!
//! This module deliberately does not use `command::Bridge` or its private `command::Groups`.
//! The request is prepared against the fixed, save/checksum-owned [`Groups`] pool and the
//! canonical [`World`].  Preparation produces complete after-images; commit first re-resolves
//! every generational identity and compares every affected before-image, then publishes only
//! assignments.  A malformed packet or stale plan therefore changes no Group, backlink, order,
//! path, cache, clock, or RNG state.
//!
//! The admitted wire cohort is intentionally narrow: the plaintext command bytes must be exactly
//! opcode 0 followed by opcode 7.  Camera/speed prefixes seen in the retail replay remain a later
//! package-dispatch shell, not something this transaction silently ignores.

use crate::order::{MoveOrderState, Order, OrderIndex, OrderList};
use crate::systems::groups_guys::{
    formation_order_coord, FormationMember, GroupData, Groups, GROUPS_PER_PLAYER,
    GROUP_MAX_MEMBERS, NUM_GROUPS, NUM_LEADERS,
};
use crate::systems::movement::{PathData, PathStack, TILE as COORD_PER_TILE};
use crate::systems::production::{BuildData, BUILDDATA_SIZE};
use crate::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use crate::world::{Handle, World, WorldObjectIdentity, OBJ_FLAG_ACTIVE};

pub const GROUP_OPCODE: u8 = 0;
pub const MOVE_TO_OPCODE: u8 = 7;
pub const MOVE_TO_WIRE_SIZE: usize = 22;
pub const NETWORK_PLAYERS: usize = 8;
pub const RECEIVED_SELECTION_CAPACITY: usize = 128;
pub const RETAIL_ALLOCATOR_SLOTS: usize = 46;
pub const ORDER_GROUP: u8 = 1;
pub const ORDER_FORM: u8 = 4;
pub const ORDER_DISEMBARK: u8 = 0x20;

/// The exact retail cache payload.  A Handle is deliberately absent: retail persists only
/// `(o,uid)`.  Prepared plans add a Handle while the entry is live, closing the Sim's current
/// repeated-UID ABA hole without changing the save image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CachedSelection {
    pub o: i16,
    pub uid: u16,
}

/// Receive state owned beside `Sim`, indexed by package `play`, not object owner `who`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPackageState {
    last_selection_by_play: [Vec<CachedSelection>; NETWORK_PLAYERS],
    revision: u64,
}

impl Default for CommandPackageState {
    fn default() -> Self {
        Self {
            last_selection_by_play: std::array::from_fn(|_| Vec::new()),
            revision: 0,
        }
    }
}

impl CommandPackageState {
    pub fn selection(&self, play: usize) -> Option<&[CachedSelection]> {
        self.last_selection_by_play.get(play).map(Vec::as_slice)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_empty(&self) -> bool {
        self.last_selection_by_play.iter().all(Vec::is_empty)
    }

    /// Install a decoded save image. Transaction revisions are process-local and restart at zero.
    pub fn from_saved_selections(
        selections: [Vec<CachedSelection>; NETWORK_PLAYERS],
    ) -> Result<Self, PackageError> {
        validate_saved_selections(&selections)?;
        Ok(Self {
            last_selection_by_play: selections,
            revision: 0,
        })
    }

    pub fn saved_selections(&self) -> [Vec<CachedSelection>; NETWORK_PLAYERS] {
        self.last_selection_by_play.clone()
    }

    /// Stage one exact receive-cache row and the enclosing package revision.
    ///
    /// Build-band Group actions share the retail `(o,uid)` cache but do not use the
    /// Unit-only selector below. Keeping this mutation primitive here preserves one owner
    /// for the persisted cache without exposing its arrays for unrelated writes.
    pub(crate) fn staged_selection(
        &self,
        play: usize,
        selection: Vec<CachedSelection>,
    ) -> Result<Self, PackageError> {
        if play >= NETWORK_PLAYERS {
            return Err(PackageError::PlayOutOfRange { play });
        }
        if selection.len() > RECEIVED_SELECTION_CAPACITY {
            return Err(PackageError::ExplicitSelectionTooLong {
                len: selection.len(),
            });
        }
        let mut after = self.clone();
        after.last_selection_by_play[play] = selection;
        after.revision = after.revision.wrapping_add(1);
        Ok(after)
    }
}

pub fn validate_saved_selections(
    selections: &[Vec<CachedSelection>; NETWORK_PLAYERS],
) -> Result<(), PackageError> {
    for (play, row) in selections.iter().enumerate() {
        if row.len() > RECEIVED_SELECTION_CAPACITY {
            return Err(PackageError::SavedCacheTooLong {
                play,
                len: row.len(),
            });
        }
        if let Some(entry) = row.iter().find(|entry| entry.o < 0) {
            return Err(PackageError::SavedCacheNegativeObject { play, o: entry.o });
        }
    }
    Ok(())
}

/// Exact object/type answers still absent from `World`'s generated columns.
///
/// The Handle makes these facts instance-bound. A product type adapter may rebuild the table from
/// immutable content, but a stale or incomplete table cannot authorize a mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveMemberAuthority {
    pub handle: Handle,
    pub role: i32,
    pub on_map: bool,
    pub is_captain: bool,
    pub can_move: bool,
    pub can_install_order: bool,
    pub is_plane: bool,
    pub domain: i32,
    pub unit_flags: u32,
    pub speed: i32,
    /// Exact result of the recovered move-near split preflight for this member set.
    pub admits_unsplit_move_near: bool,
    pub land_formation: FormationMember,
    pub water_formation: FormationMember,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupMoveAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub destination_is_water: bool,
    pub force_formation_facing_zero: bool,
    pub members: Vec<MoveMemberAuthority>,
}

impl GroupMoveAuthority {
    fn member(&self, handle: Handle) -> Option<&MoveMemberAuthority> {
        self.members.iter().find(|entry| entry.handle == handle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveToWire {
    pub x: i32,
    pub y: i32,
    pub set_angle: i32,
    pub angle: i32,
    pub orders: i8,
    pub queued: i8,
    pub form: i8,
    pub width: i8,
    pub disembark: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupMoveWire {
    pub who: u8,
    pub objects: Vec<i16>,
    pub movement: MoveToWire,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageError {
    Truncated,
    WrongFirstOpcode { got: u8 },
    WrongSecondOpcode { got: u8 },
    TrailingBytes { expected: usize, got: usize },
    PlayOutOfRange { play: usize },
    MissingPlayerMap { play: usize },
    PlayerOwnerMismatch { play: usize, expected: u8, got: u8 },
    OwnerOutOfRange { who: i8 },
    ExplicitSelectionTooLong { len: usize },
    NegativeObject { o: i16 },
    EmptyEffectiveSelection,
    MissingUnit { who: u8, o: i16 },
    InactiveUnit { who: u8, o: i16 },
    MissingHandle { who: u8, o: i16 },
    MissingAuthority { handle: Handle },
    MissingBuild { who: u8, o: i16 },
    InactiveBuild { who: u8, o: i16 },
    MissingBuildAuthority { who: u8, o: i16, row: u32 },
    DuplicateBuildAuthority { who: u8, o: i16, row: u32 },
    StaleBuildIdentity { who: u8, o: i16, row: u32 },
    IncompleteSelectionAuthority { handle: Handle },
    IncompleteMoveAuthority { handle: Handle },
    SubordinateChainUnavailable { who: u8, o: i16 },
    InvalidGroupPool,
    BuildingAllocatorBoundary { slot: usize },
    AllocatorCaptainLruBoundary,
    GroupNotOnMap,
    PlaneLedSelection,
    QueueFirstBoundary,
    QueueLastTargetBoundary { handle: Handle },
    AttackReplacementBoundary { handle: Handle },
    UnsupportedFormation { form: i32 },
    FormationComputationFailed,
    MissingPath { row: usize },
    SavedCacheTooLong { play: usize, len: usize },
    SavedCacheNegativeObject { play: usize, o: i16 },
    StaleCommandState,
    StaleGroups,
    StaleAuthority,
    StaleUnit { handle: Handle },
}

#[inline]
fn read_i32(bytes: &[u8], at: usize) -> Option<i32> {
    let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(i32::from_le_bytes(raw))
}

#[inline]
fn read_i16(bytes: &[u8], at: usize) -> Option<i16> {
    let raw: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
    Some(i16::from_le_bytes(raw))
}

/// Decode the exact two-command plaintext cohort. Every one of MoveTo's nine fields is retained.
pub fn decode_group_move_package(bytes: &[u8]) -> Result<GroupMoveWire, PackageError> {
    let Some(&first) = bytes.first() else {
        return Err(PackageError::Truncated);
    };
    if first != GROUP_OPCODE {
        return Err(PackageError::WrongFirstOpcode { got: first });
    }
    let Some(&num) = bytes.get(1) else {
        return Err(PackageError::Truncated);
    };
    if usize::from(num) > RECEIVED_SELECTION_CAPACITY {
        return Err(PackageError::ExplicitSelectionTooLong {
            len: usize::from(num),
        });
    }
    let Some(&who_raw) = bytes.get(2) else {
        return Err(PackageError::Truncated);
    };
    let who_signed = who_raw as i8;
    if who_signed < 0 || usize::from(who_raw) >= NUM_LEADERS {
        return Err(PackageError::OwnerOutOfRange { who: who_signed });
    }
    let group_len = 3usize + usize::from(num) * 2;
    let expected = group_len + MOVE_TO_WIRE_SIZE;
    if bytes.len() < expected {
        return Err(PackageError::Truncated);
    }
    if bytes.len() != expected {
        return Err(PackageError::TrailingBytes {
            expected,
            got: bytes.len(),
        });
    }
    if bytes[group_len] != MOVE_TO_OPCODE {
        return Err(PackageError::WrongSecondOpcode {
            got: bytes[group_len],
        });
    }
    let mut objects = Vec::with_capacity(usize::from(num));
    for index in 0..usize::from(num) {
        let o = read_i16(bytes, 3 + index * 2).ok_or(PackageError::Truncated)?;
        if o < 0 {
            return Err(PackageError::NegativeObject { o });
        }
        objects.push(o);
    }
    let m = &bytes[group_len..];
    Ok(GroupMoveWire {
        who: who_raw,
        objects,
        movement: MoveToWire {
            x: read_i32(m, 1).ok_or(PackageError::Truncated)?,
            y: read_i32(m, 5).ok_or(PackageError::Truncated)?,
            set_angle: read_i32(m, 9).ok_or(PackageError::Truncated)?,
            angle: read_i32(m, 13).ok_or(PackageError::Truncated)?,
            orders: m[17] as i8,
            queued: m[18] as i8,
            form: m[19] as i8,
            width: m[20] as i8,
            disembark: m[21] as i8,
        },
    })
}

/// Retail's post-`Groups::clear` initial image.  The current `Groups::default` predates the
/// structural SVX proof and is not an admissible fresh authority for command processing.
pub fn retail_fresh_groups() -> Groups {
    Groups {
        list: (0..NUM_GROUPS)
            .map(|id| GroupData {
                id: id as i32,
                army: -1,
                form: -1,
                ..GroupData::default()
            })
            .collect(),
        last_group: std::array::from_fn(|who| (who * GROUPS_PER_PLAYER) as i32),
        proc_group: 0,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitIdentity {
    pub handle: Handle,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitImage {
    pub identity: UnitIdentity,
    pub group: i16,
    pub unit_masks: u32,
    pub form: i8,
    pub form_mod: i8,
    pub angle: i32,
    pub x: i32,
    pub y: i32,
    pub orders_x: i32,
    pub orders_y: i32,
    pub dest_angle: i32,
    pub orders: OrderList,
    pub path: PathStack,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitMutation {
    pub before: UnitImage,
    pub after: UnitImage,
}

/// Which recovered consumer is asking the canonical opcode-0 selector to form a Group.
///
/// Simple Unit-state actions need only a live generational Unit with a bound authority member.
/// Economy and construction-placement actions additionally need an order-installable Unit, while
/// Group→Move also requires the exact split admission bit. Construction placement still selects
/// builder Units here; its separately allocated Build target never enters this selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSelectionUse {
    SimpleUnitState,
    EconomyOrderInstall,
    ConstructionPlacement,
    AirOrderInstall,
    MoveNear,
}

#[derive(Clone, Debug)]
pub struct PreparedSelectionMember {
    pub row: usize,
    pub identity: UnitIdentity,
    pub authority: MoveMemberAuthority,
}

/// Stable Build-band identity used by the AIR selector. Unlike a Unit, a Build has no
/// `UnitData::group` backlink and its current dense row is itself the strongest identity the
/// canonical object registry can provide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildSelectionIdentity {
    pub row: u32,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
}

/// Type-derived facts required to reproduce `Group::add` for a Build member. The identity binds
/// the answer to one sparse-registry slot and BuildData reuse token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildSelectionAuthority {
    pub identity: BuildSelectionIdentity,
    pub role: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSelectionImage {
    pub identity: BuildSelectionIdentity,
    pub bytes: [u8; BUILDDATA_SIZE],
    pub position: (i32, i32),
    pub inside_down: i16,
    pub inside_down_who: i8,
}

#[derive(Clone, Debug)]
pub struct PreparedBuildSelectionMember {
    pub image: BuildSelectionImage,
    pub authority: BuildSelectionAuthority,
}

/// Ordered all-band selection image. Existing Unit-only consumers keep using `members`; AIR uses
/// this exact list so a mixed or Build-only retail Group never gets projected through UnitData.
#[derive(Clone, Debug)]
pub enum PreparedSelectionObject {
    Unit(PreparedSelectionMember),
    Build(PreparedBuildSelectionMember),
}

/// Detached after-images for the canonical opcode-0 selection/cache/allocation stage.
///
/// This is deliberately reusable by every same-package Group action. The after-images are
/// not independently committable: the following action must finish planning first, then one
/// outer transaction revalidates and publishes selection plus action together.
#[derive(Clone, Debug)]
pub struct PreparedGroupSelection {
    pub play: usize,
    pub frame: i32,
    pub who: u8,
    pub group_slot: usize,
    pub members: Vec<PreparedSelectionMember>,
    pub selected_objects: Vec<PreparedSelectionObject>,
    pub command_state_before: CommandPackageState,
    pub command_state_after: CommandPackageState,
    pub groups_before: Groups,
    pub groups_after: Groups,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub authority_members: Vec<MoveMemberAuthority>,
    pub units: Vec<UnitMutation>,
}

#[derive(Clone, Debug)]
pub struct PreparedGroupMovePackage {
    pub play: usize,
    pub lockstep_serial: i32,
    pub frame: i32,
    pub random_state: i32,
    pub wire: GroupMoveWire,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub command_state_before: CommandPackageState,
    pub command_state_after: CommandPackageState,
    pub groups_before: Groups,
    pub groups_after: Groups,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub authority_members: Vec<MoveMemberAuthority>,
    pub units: Vec<UnitMutation>,
}

/// Stable product receipt for one applied package. Group checksum is the retail checksum-channel
/// traversal of the complete post-commit pool; RNG equality proves this command consumed no draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupMovePackageReceipt {
    pub play: usize,
    pub lockstep_serial: i32,
    pub frame: i32,
    pub who: u8,
    pub group_slot: usize,
    pub selected: Vec<UnitIdentity>,
    pub command_state_revision: u64,
    pub groups_checksum: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
}

/// Exact equality predicate used by outer canonical Group transactions before publication.
pub fn groups_equal(a: &Groups, b: &Groups) -> bool {
    a.last_group == b.last_group && a.proc_group == b.proc_group && a.list == b.list
}

pub(crate) fn capture_unit(
    world: &World,
    paths: &[PathStack],
    who: u8,
    o: i16,
) -> Result<(usize, UnitImage), PackageError> {
    let row = world
        .unit_row_at(i32::from(who), i32::from(o))
        .ok_or(PackageError::MissingUnit { who, o })?;
    if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(PackageError::InactiveUnit { who, o });
    }
    let handle = world
        .handle_at_row(row)
        .ok_or(PackageError::MissingHandle { who, o })?;
    let path = paths
        .get(row)
        .cloned()
        .ok_or(PackageError::MissingPath { row })?;
    Ok((
        row,
        UnitImage {
            identity: UnitIdentity {
                handle,
                who,
                o,
                uid: world.units.get_uid(row),
            },
            group: world.units.group()[row],
            unit_masks: world.units.get_unit_masks(row),
            form: world.units.form()[row],
            form_mod: world.units.form_mod()[row],
            angle: world.units.angle()[row],
            x: world.units.x_internal()[row],
            y: world.units.y_internal()[row],
            orders_x: world.units.orders_x()[row],
            orders_y: world.units.orders_y()[row],
            dest_angle: world.units.dest_angle()[row],
            orders: world.orders(row).clone(),
            path,
        },
    ))
}

fn remove_group_member(group: &mut GroupData, o: i16) {
    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    let Some(index) = group.list[..n].iter().position(|&candidate| candidate == o) else {
        return;
    };
    for at in index..n.saturating_sub(1) {
        group.list[at] = group.list[at + 1];
        group.angles[at] = group.angles[at + 1];
        group.off_x[at] = group.off_x[at + 1];
        group.off_y[at] = group.off_y[at + 1];
        group.curr_x[at] = group.curr_x[at + 1];
        group.curr_y[at] = group.curr_y[at + 1];
    }
    group.num -= 1;
}

fn unit_authority<'a>(
    world: &World,
    authority: &'a GroupMoveAuthority,
    who: u8,
    o: i16,
) -> Result<(usize, Handle, &'a MoveMemberAuthority), PackageError> {
    let row = world
        .unit_row_at(i32::from(who), i32::from(o))
        .ok_or(PackageError::MissingUnit { who, o })?;
    if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(PackageError::InactiveUnit { who, o });
    }
    let handle = world
        .handle_at_row(row)
        .ok_or(PackageError::MissingHandle { who, o })?;
    let facts = authority
        .member(handle)
        .ok_or(PackageError::MissingAuthority { handle })?;
    Ok((row, handle, facts))
}

fn group_leader<'a>(
    group: &GroupData,
    world: &World,
    authority: &'a GroupMoveAuthority,
) -> Result<Option<(i16, usize, Handle, &'a MoveMemberAuthority)>, PackageError> {
    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    for require_on_map in [true, false] {
        let mut best: Option<(i16, usize, Handle, &MoveMemberAuthority)> = None;
        let mut best_category = 18;
        for &o in &group.list[..n] {
            let (row, handle, facts) = unit_authority(world, authority, group.who, o)?;
            if !facts.is_captain || (require_on_map && !facts.on_map) {
                continue;
            }
            let category = if authority.destination_is_water {
                facts.water_formation.category
            } else {
                facts.land_formation.category
            };
            if best.is_none() || category < best_category {
                best = Some((o, row, handle, facts));
                best_category = category;
            }
        }
        if best.is_some() {
            return Ok(best);
        }
    }
    Ok(None)
}

/// Exact dynamic `Group::find_leader(0) -> UnitData::speed()` projection reused by
/// same-package action preludes after they remove scenario-ignored members.
pub(crate) fn group_leader_speed(
    group: &GroupData,
    world: &World,
    authority: &GroupMoveAuthority,
) -> Result<Option<i32>, PackageError> {
    group_leader(group, world, authority).map(|leader| leader.map(|(_, _, _, facts)| facts.speed))
}

fn recompute_group(
    group: &mut GroupData,
    world: &World,
    authority: &GroupMoveAuthority,
) -> Result<(), PackageError> {
    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    let mut role = 0;
    for &o in &group.list[..n] {
        let (_, _, facts) = unit_authority(world, authority, group.who, o)?;
        role |= facts.role;
    }
    group.role = role;
    let speed = group_leader(group, world, authority)?.map(|(_, _, _, facts)| facts.speed);
    group.compute_speed(speed);
    Ok(())
}

fn normalize_small_group(
    group: &mut GroupData,
    world: &World,
    authority: &GroupMoveAuthority,
) -> Result<(), PackageError> {
    if group.buildings != 0 && group.num > 0 {
        return Err(PackageError::BuildingAllocatorBoundary {
            slot: group.id.max(0) as usize,
        });
    }
    let mut at = group.num - 1;
    while at >= 0 {
        let index = at as usize;
        let o = group.list[index];
        let drop = if o < 0 {
            true
        } else {
            match unit_authority(world, authority, group.who, o) {
                Ok((row, _, _)) => {
                    group.priority == 0
                        && group.id >= 0
                        && world.units.group()[row] != group.id as i16
                }
                Err(PackageError::MissingUnit { .. } | PackageError::InactiveUnit { .. }) => true,
                Err(error) => return Err(error),
            }
        };
        if drop {
            remove_group_member(group, o);
        }
        at -= 1;
    }
    recompute_group(group, world, authority)
}

fn get_num_for_allocator(
    group: &mut GroupData,
    world: &World,
    authority: &GroupMoveAuthority,
) -> Result<i32, PackageError> {
    if group.num < 1 {
        group.num = 0;
        group.ox = 0;
        group.oy = 0;
        return Ok(0);
    }
    if group.num < 4 && group.id >= 0 {
        normalize_small_group(group, world, authority)?;
        return Ok(group.num);
    }
    let mut at = group.num - 1;
    while at >= 0 {
        let index = at as usize;
        let o = group.list[index];
        let dead = o < 0
            || world
                .unit_row_at(i32::from(group.who), i32::from(o))
                .is_none();
        if dead {
            remove_group_member(group, o);
        }
        at -= 1;
    }
    Ok(group.num)
}

fn validate_group_pool(groups: &Groups) -> Result<(), PackageError> {
    if groups.list.len() != NUM_GROUPS
        || groups
            .list
            .iter()
            .enumerate()
            .any(|(index, group)| group.id != index as i32)
    {
        return Err(PackageError::InvalidGroupPool);
    }
    Ok(())
}

pub(crate) fn exact_immediate_allocator_slot(
    groups: &mut Groups,
    who: u8,
    world: &World,
    authority: &GroupMoveAuthority,
) -> Result<usize, PackageError> {
    let base = usize::from(who) * GROUPS_PER_PLAYER;
    let current = groups.last_group[usize::from(who)];
    for slot in base..base + RETAIL_ALLOCATOR_SLOTS {
        let count = get_num_for_allocator(&mut groups.list[slot], world, authority)?;
        if (count == 0 || groups.list[slot].buildings != 0) && slot as i32 != current {
            return Ok(slot);
        }
    }
    // The exact captain-count LRU/fallback decision is recovered, but publishing it also needs
    // exact normalization facts for every nonempty object class. Keep the uncommon full-pool arm
    // unavailable until that all-band authority is mounted.
    Err(PackageError::AllocatorCaptainLruBoundary)
}

fn movement_order_kind(orders: i8) -> OrderIndex {
    match orders {
        2 => OrderIndex::AttackTo,
        3 => OrderIndex::ExploreTo,
        4 => OrderIndex::FleeTo,
        _ => OrderIndex::MoveTo,
    }
}

fn final_position(image: &UnitImage, map_tiles: (i32, i32)) -> Result<(i32, i32), PackageError> {
    for order in image.orders.iter() {
        if matches!(
            order.kind,
            OrderIndex::MoveTo
                | OrderIndex::AttackTo
                | OrderIndex::ExploreTo
                | OrderIndex::FleeTo
                | OrderIndex::ChangeForm
                | OrderIndex::GroupMove
                | OrderIndex::GroupAttackTo
        ) {
            let max_x = map_tiles.0.wrapping_mul(COORD_PER_TILE);
            let max_y = map_tiles.1.wrapping_mul(COORD_PER_TILE);
            return Ok(
                if order.x >= 0 && order.y >= 0 && order.x < max_x && order.y < max_y {
                    (order.x, order.y)
                } else {
                    (image.x, image.y)
                },
            );
        }
        if order.target_who >= 0 && order.target_o >= 0 {
            return Err(PackageError::QueueLastTargetBoundary {
                handle: image.identity.handle,
            });
        }
    }
    Ok((image.x, image.y))
}

fn order_remainder(value: i32) -> i16 {
    value.wrapping_sub((value / 0x300).wrapping_mul(0x300)) as i16
}

/// Produce the complete direct-path after-image paired with a newly-current locomotion order.
///
/// `PathStack` is checksum-owned Unit state, so a canonical package cannot publish a MoveTo
/// order and leave the old path behind (or clear it to an unusable empty stack). The browser
/// lifecycle currently mounts a flat, obstacle-free terrain source; its authoritative repath is
/// therefore the one direct destination record consumed by `Unit::do_move`. A queued order does
/// not replace the current path until it becomes current.
pub fn initial_direct_move_path(destination: (i32, i32), tolerance: i32) -> PathStack {
    let mut path = PathStack::new();
    path.push(PathData {
        to_x: destination.0,
        to_y: destination.1,
        tolerance,
        flags: PathData::FLAG_MORE,
    });
    path
}

#[allow(clippy::too_many_arguments)]
fn build_move_order(
    kind: OrderIndex,
    destination: (i32, i32),
    member_angle: i32,
    reverse: bool,
    raw_destination: (i32, i32),
    disembark: bool,
    group_fields: Option<(i16, u8, i32, usize)>,
) -> Order {
    let mut state = MoveOrderState::fresh(destination.0, destination.1);
    state.angle = member_angle;
    state.facing = i32::from(reverse);
    state.orig_x = raw_destination.0;
    state.orig_y = raw_destination.1;
    state.off_x = order_remainder(destination.0);
    state.off_y = order_remainder(destination.1);
    if let Some((leader_o, who, group_id, member_index)) = group_fields {
        state.group_oxx = i32::from(leader_o);
        state.group_whose = i32::from(who);
        state.group_id = group_id;
        // Measured constructor argument at GroupOrder+0x60 is the member loop index.
        state.group_form_id = member_index as i32;
        state.group_angle = member_angle;
    }
    Order {
        kind,
        flags: ORDER_GROUP | ORDER_FORM | if disembark { ORDER_DISEMBARK } else { 0 },
        x: destination.0,
        y: destination.1,
        tolerance: 0,
        move_state: Some(state),
        ..Order::default()
    }
}

pub(crate) fn ensure_mutation_for_row(
    mutations: &mut Vec<UnitMutation>,
    world: &World,
    paths: &[PathStack],
    row: usize,
) -> Result<usize, PackageError> {
    if let Some(index) = mutations
        .iter()
        .position(|entry| world.row_of(entry.before.identity.handle) == Some(row))
    {
        return Ok(index);
    }
    let who = world.units.get_who(row);
    let o = world.units.o()[row];
    let (_, image) = capture_unit(world, paths, who, o)?;
    mutations.push(UnitMutation {
        before: image.clone(),
        after: image,
    });
    Ok(mutations.len() - 1)
}

/// Prepare the one canonical opcode-0 selection stage shared by Group→Move and the
/// economy/containment action cohort.
///
/// Explicit selections refresh the play-keyed retail `(o,uid)` cache before stale entries
/// are filtered. An empty selection reuses that cache. Group allocation, old-group removal,
/// and every `UnitData::group` backlink are staged together; callers must append their action
/// mutations and publish the resulting outer transaction atomically.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn prepare_group_selection(
    world: &World,
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    authority: &GroupMoveAuthority,
    frame: i32,
    play: usize,
    who: u8,
    objects: &[i16],
    selection_use: GroupSelectionUse,
) -> Result<PreparedGroupSelection, PackageError> {
    if play >= NETWORK_PLAYERS {
        return Err(PackageError::PlayOutOfRange { play });
    }
    if usize::from(who) >= NUM_LEADERS {
        return Err(PackageError::OwnerOutOfRange { who: who as i8 });
    }
    if objects.len() > RECEIVED_SELECTION_CAPACITY {
        return Err(PackageError::ExplicitSelectionTooLong { len: objects.len() });
    }
    if let Some(&o) = objects.iter().find(|&&o| o < 0) {
        return Err(PackageError::NegativeObject { o });
    }
    validate_group_pool(groups)?;

    let mut command_state_after = command_state.clone();
    let received = if objects.is_empty() {
        command_state.last_selection_by_play[play].clone()
    } else {
        let mut cache = Vec::with_capacity(objects.len());
        for &o in objects {
            if let Some(row) = world.unit_row_at(i32::from(who), i32::from(o)) {
                if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                    continue;
                }
                cache.push(CachedSelection {
                    o,
                    uid: world.units.get_uid(row),
                });
            }
        }
        command_state_after.last_selection_by_play[play] = cache.clone();
        cache
    };

    let mut effective = Vec::new();
    for entry in received {
        let (row, handle, facts) = match unit_authority(world, authority, who, entry.o) {
            Ok(found) => found,
            Err(PackageError::MissingUnit { .. } | PackageError::InactiveUnit { .. }) => continue,
            Err(error) => return Err(error),
        };
        if world.units.get_uid(row) != entry.uid {
            continue;
        }
        if effective
            .iter()
            .any(|member: &PreparedSelectionMember| member.identity.o == entry.o)
        {
            continue;
        }
        if world.units.o_down()[row] >= 0 {
            return Err(PackageError::SubordinateChainUnavailable { who, o: entry.o });
        }
        if selection_use != GroupSelectionUse::SimpleUnitState && !facts.can_install_order {
            return Err(match selection_use {
                GroupSelectionUse::SimpleUnitState => unreachable!("guard excludes this case"),
                GroupSelectionUse::EconomyOrderInstall => {
                    PackageError::IncompleteSelectionAuthority { handle }
                }
                GroupSelectionUse::ConstructionPlacement => {
                    PackageError::IncompleteSelectionAuthority { handle }
                }
                GroupSelectionUse::AirOrderInstall => {
                    PackageError::IncompleteSelectionAuthority { handle }
                }
                GroupSelectionUse::MoveNear => PackageError::IncompleteMoveAuthority { handle },
            });
        }
        if selection_use == GroupSelectionUse::MoveNear && !facts.admits_unsplit_move_near {
            return Err(PackageError::IncompleteMoveAuthority { handle });
        }
        effective.push(PreparedSelectionMember {
            row,
            identity: UnitIdentity {
                handle,
                who,
                o: entry.o,
                uid: entry.uid,
            },
            authority: facts.clone(),
        });
    }
    if effective.is_empty() {
        return Err(PackageError::EmptyEffectiveSelection);
    }

    let mut transient = GroupData {
        id: -1,
        army: -1,
        form: -1,
        stamp: frame,
        ..GroupData::default()
    };
    for member in &effective {
        transient.add(member.identity.o, who, false, member.authority.role, frame);
    }

    let mut groups_after = groups.clone();
    let current = groups_after.last_group[usize::from(who)];
    let n = transient.num as usize;
    let reuse = usize::try_from(current).ok().is_some_and(|slot| {
        groups_after.list.get(slot).is_some_and(|candidate| {
            candidate.num == transient.num && candidate.list[..n] == transient.list[..n]
        })
    });
    let group_slot = if reuse {
        current as usize
    } else {
        let slot = exact_immediate_allocator_slot(&mut groups_after, who, world, authority)?;
        let id = groups_after.list[slot].id;
        groups_after.list[slot] = transient;
        groups_after.list[slot].id = id;
        groups_after.list[slot].stamp = frame;
        groups_after.last_group[usize::from(who)] = slot as i32;
        slot
    };

    let mut mutations = Vec::new();
    if !reuse && groups.list[group_slot].buildings == 0 {
        for row in 0..world.live_count() as usize {
            if world.units.get_who(row) == who && world.units.group()[row] == group_slot as i16 {
                let index = ensure_mutation_for_row(&mut mutations, world, paths, row)?;
                mutations[index].after.group = -1;
            }
        }
    }

    for member in &effective {
        let mutation_index = ensure_mutation_for_row(&mut mutations, world, paths, member.row)?;
        let previous = mutations[mutation_index].after.group;
        if previous >= 0 && previous as usize != group_slot {
            let previous_index = previous as usize;
            if previous_index >= groups_after.list.len() {
                return Err(PackageError::InvalidGroupPool);
            }
            remove_group_member(&mut groups_after.list[previous_index], member.identity.o);
            recompute_group(&mut groups_after.list[previous_index], world, authority)?;
        }
        mutations[mutation_index].after.group = group_slot as i16;
    }

    // One accepted Group+action package advances the process-local transaction revision once.
    // The outer action may still refuse; these detached after-images are published only after
    // that action passes, so a failed package leaves the revision untouched.
    command_state_after.revision = command_state_after.revision.wrapping_add(1);

    let selected_objects = effective
        .iter()
        .cloned()
        .map(PreparedSelectionObject::Unit)
        .collect();
    Ok(PreparedGroupSelection {
        play,
        frame,
        who,
        group_slot,
        members: effective,
        selected_objects,
        command_state_before: command_state.clone(),
        command_state_after,
        groups_before: groups.clone(),
        groups_after,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        authority_members: authority.members.clone(),
        units: mutations,
    })
}

fn capture_build(
    world: &World,
    builds: &[BuildData],
    who: u8,
    o: i16,
) -> Result<BuildSelectionImage, PackageError> {
    let address = RetailObjectAddress::new(who, RetailBand::Build, i32::from(o));
    let WorldObjectIdentity::BuildRow(row) = world
        .object_bands()
        .live_identity(address)
        .ok_or(PackageError::MissingBuild { who, o })?
    else {
        return Err(PackageError::MissingBuild { who, o });
    };
    let build = builds
        .get(row as usize)
        .ok_or(PackageError::MissingBuild { who, o })?;
    if build.who != who || build.object_id() != o {
        return Err(PackageError::StaleBuildIdentity { who, o, row });
    }
    if !build.is_valid() {
        return Err(PackageError::InactiveBuild { who, o });
    }
    Ok(BuildSelectionImage {
        identity: BuildSelectionIdentity {
            row,
            who,
            o,
            uid: build.uid,
        },
        bytes: build.image(),
        position: build.position(),
        inside_down: i16::from_le_bytes([build.other[0x28], build.other[0x29]]),
        inside_down_who: build.other[0x3e] as i8,
    })
}

pub fn build_still_current(
    world: &World,
    builds: &[BuildData],
    before: &BuildSelectionImage,
) -> bool {
    capture_build(world, builds, before.identity.who, before.identity.o)
        .is_ok_and(|current| current == *before)
}

fn exact_air_allocator_slot(
    groups: &mut Groups,
    who: u8,
    world: &World,
    authority: &GroupMoveAuthority,
) -> Result<usize, PackageError> {
    let base = usize::from(who) * GROUPS_PER_PLAYER;
    let current = groups.last_group[usize::from(who)];
    for slot in base..base + RETAIL_ALLOCATOR_SLOTS {
        // Building groups have no Unit backlinks to normalize. Retail admits them as immediate
        // reusable slots; Unit groups retain the exact existing normalization path.
        let count = if groups.list[slot].buildings != 0 {
            groups.list[slot].num.max(0)
        } else {
            get_num_for_allocator(&mut groups.list[slot], world, authority)?
        };
        if (count == 0 || groups.list[slot].buildings != 0) && slot as i32 != current {
            return Ok(slot);
        }
    }
    Err(PackageError::AllocatorCaptainLruBoundary)
}

/// AIR-only extension of the canonical opcode-0 selector. It preserves the established Unit
/// selection/cache/backlink path while additionally resolving Build-band airbases through the
/// sparse object registry and `BuildData::uid`. Build members form a building Group but never
/// receive a fabricated UnitData backlink.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn prepare_air_group_selection(
    world: &World,
    builds: &[BuildData],
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    authority: &GroupMoveAuthority,
    build_authority: &[BuildSelectionAuthority],
    frame: i32,
    play: usize,
    who: u8,
    objects: &[i16],
) -> Result<PreparedGroupSelection, PackageError> {
    if play >= NETWORK_PLAYERS {
        return Err(PackageError::PlayOutOfRange { play });
    }
    if usize::from(who) >= NUM_LEADERS {
        return Err(PackageError::OwnerOutOfRange { who: who as i8 });
    }
    if objects.len() > RECEIVED_SELECTION_CAPACITY {
        return Err(PackageError::ExplicitSelectionTooLong { len: objects.len() });
    }
    if let Some(&o) = objects.iter().find(|&&o| o < 0) {
        return Err(PackageError::NegativeObject { o });
    }
    if let Some(duplicate) = build_authority
        .iter()
        .enumerate()
        .find_map(|(index, entry)| {
            build_authority[..index]
                .iter()
                .any(|old| old.identity == entry.identity)
                .then_some(entry.identity)
        })
    {
        return Err(PackageError::DuplicateBuildAuthority {
            who: duplicate.who,
            o: duplicate.o,
            row: duplicate.row,
        });
    }
    validate_group_pool(groups)?;

    let mut command_state_after = command_state.clone();
    let received = if objects.is_empty() {
        command_state.last_selection_by_play[play].clone()
    } else {
        let mut cache = Vec::with_capacity(objects.len());
        for &o in objects {
            let uid = if RetailBand::Unit.contains(i32::from(o)) {
                world
                    .unit_row_at(i32::from(who), i32::from(o))
                    .filter(|&row| world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0)
                    .map(|row| world.units.get_uid(row))
            } else if RetailBand::Build.contains(i32::from(o)) {
                capture_build(world, builds, who, o)
                    .ok()
                    .map(|image| image.identity.uid)
            } else {
                None
            };
            if let Some(uid) = uid {
                cache.push(CachedSelection { o, uid });
            }
        }
        command_state_after.last_selection_by_play[play] = cache.clone();
        cache
    };

    let mut selected_objects = Vec::new();
    let mut unit_members = Vec::new();
    for entry in received {
        if selected_objects.iter().any(|selected| match selected {
            PreparedSelectionObject::Unit(member) => member.identity.o == entry.o,
            PreparedSelectionObject::Build(member) => member.image.identity.o == entry.o,
        }) {
            continue;
        }
        if RetailBand::Unit.contains(i32::from(entry.o)) {
            let (row, handle, facts) = match unit_authority(world, authority, who, entry.o) {
                Ok(found) => found,
                Err(PackageError::MissingUnit { .. } | PackageError::InactiveUnit { .. }) => {
                    continue;
                }
                Err(error) => return Err(error),
            };
            if world.units.get_uid(row) != entry.uid {
                continue;
            }
            if !facts.can_install_order {
                return Err(PackageError::IncompleteSelectionAuthority { handle });
            }
            let member = PreparedSelectionMember {
                row,
                identity: UnitIdentity {
                    handle,
                    who,
                    o: entry.o,
                    uid: entry.uid,
                },
                authority: facts.clone(),
            };
            unit_members.push(member.clone());
            selected_objects.push(PreparedSelectionObject::Unit(member));
        } else if RetailBand::Build.contains(i32::from(entry.o)) {
            let image = match capture_build(world, builds, who, entry.o) {
                Ok(image) => image,
                Err(PackageError::MissingBuild { .. } | PackageError::InactiveBuild { .. }) => {
                    continue;
                }
                Err(error) => return Err(error),
            };
            if image.identity.uid != entry.uid {
                continue;
            }
            let build_facts = build_authority
                .iter()
                .copied()
                .find(|facts| facts.identity == image.identity)
                .ok_or(PackageError::MissingBuildAuthority {
                    who,
                    o: entry.o,
                    row: image.identity.row,
                })?;
            selected_objects.push(PreparedSelectionObject::Build(
                PreparedBuildSelectionMember {
                    image,
                    authority: build_facts,
                },
            ));
        }
    }
    if selected_objects.is_empty() {
        return Err(PackageError::EmptyEffectiveSelection);
    }

    let mut transient = GroupData {
        id: -1,
        army: -1,
        form: -1,
        stamp: frame,
        ..GroupData::default()
    };
    for member in &selected_objects {
        match member {
            PreparedSelectionObject::Unit(member) => {
                transient.add(member.identity.o, who, false, member.authority.role, frame);
            }
            PreparedSelectionObject::Build(member) => {
                transient.add(
                    member.image.identity.o,
                    who,
                    true,
                    member.authority.role,
                    frame,
                );
            }
        }
    }

    let mut groups_after = groups.clone();
    let current = groups_after.last_group[usize::from(who)];
    let n = transient.num as usize;
    let reuse = usize::try_from(current).ok().is_some_and(|slot| {
        groups_after.list.get(slot).is_some_and(|candidate| {
            candidate.num == transient.num
                && candidate.buildings == transient.buildings
                && candidate.list[..n] == transient.list[..n]
        })
    });
    let group_slot = if reuse {
        current as usize
    } else {
        let slot = exact_air_allocator_slot(&mut groups_after, who, world, authority)?;
        let id = groups_after.list[slot].id;
        groups_after.list[slot] = transient;
        groups_after.list[slot].id = id;
        groups_after.list[slot].stamp = frame;
        groups_after.last_group[usize::from(who)] = slot as i32;
        slot
    };

    let mut mutations = Vec::new();
    if !reuse && groups.list[group_slot].buildings == 0 {
        for row in 0..world.live_count() as usize {
            if world.units.get_who(row) == who && world.units.group()[row] == group_slot as i16 {
                let index = ensure_mutation_for_row(&mut mutations, world, paths, row)?;
                mutations[index].after.group = -1;
            }
        }
    }
    for member in &unit_members {
        let mutation_index = ensure_mutation_for_row(&mut mutations, world, paths, member.row)?;
        let previous = mutations[mutation_index].after.group;
        if previous >= 0 && previous as usize != group_slot {
            let previous_index = previous as usize;
            if previous_index >= groups_after.list.len() {
                return Err(PackageError::InvalidGroupPool);
            }
            remove_group_member(&mut groups_after.list[previous_index], member.identity.o);
            recompute_group(&mut groups_after.list[previous_index], world, authority)?;
        }
        mutations[mutation_index].after.group = group_slot as i16;
    }

    command_state_after.revision = command_state_after.revision.wrapping_add(1);
    Ok(PreparedGroupSelection {
        play,
        frame,
        who,
        group_slot,
        members: unit_members,
        selected_objects,
        command_state_before: command_state.clone(),
        command_state_after,
        groups_before: groups.clone(),
        groups_after,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        authority_members: authority.members.clone(),
        units: mutations,
    })
}

/// Prepare the canonical package against detached after-images.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn prepare_group_move_package(
    world: &World,
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    authority: &GroupMoveAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    map_tiles: (i32, i32),
    frame: i32,
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<PreparedGroupMovePackage, PackageError> {
    let wire = decode_group_move_package(bytes)?;
    if play >= NETWORK_PLAYERS {
        return Err(PackageError::PlayOutOfRange { play });
    }
    let expected_who = player_who[play].ok_or(PackageError::MissingPlayerMap { play })?;
    if expected_who != wire.who {
        return Err(PackageError::PlayerOwnerMismatch {
            play,
            expected: expected_who,
            got: wire.who,
        });
    }
    let selection = prepare_group_selection(
        world,
        groups,
        paths,
        command_state,
        authority,
        frame,
        play,
        wire.who,
        &wire.objects,
        GroupSelectionUse::MoveNear,
    )?;
    let effective = &selection.members;
    let group_slot = selection.group_slot;
    let command_state_after = selection.command_state_after.clone();
    let mut groups_after = selection.groups_after.clone();
    let mut mutations = selection.units.clone();

    let max_x = map_tiles.0.wrapping_mul(COORD_PER_TILE).wrapping_sub(1);
    let max_y = map_tiles.1.wrapping_mul(COORD_PER_TILE).wrapping_sub(1);
    let x = wire.movement.x.clamp(0, max_x.max(0));
    let y = wire.movement.y.clamp(0, max_y.max(0));
    let group = &mut groups_after.list[group_slot];
    group.disband = 0;
    let Some((_leader_o, leader_row, _, leader_facts)) = group_leader(group, world, authority)?
    else {
        return Err(PackageError::GroupNotOnMap);
    };
    if !leader_facts.on_map {
        return Err(PackageError::GroupNotOnMap);
    }
    if leader_facts.is_plane && leader_facts.domain == 2 && leader_facts.unit_flags & 0x20 == 0 {
        return Err(PackageError::PlaneLedSelection);
    }
    if effective.iter().any(|member| {
        !member.authority.can_move || !member.authority.is_captain || !member.authority.on_map
    }) {
        return Err(PackageError::IncompleteMoveAuthority {
            handle: effective
                .iter()
                .find(|member| {
                    !member.authority.can_move
                        || !member.authority.is_captain
                        || !member.authority.on_map
                })
                .expect("predicate just matched")
                .identity
                .handle,
        });
    }

    let queue = match wire.movement.queued {
        1 => 1,
        2 => 2,
        _ => return Err(PackageError::QueueFirstBoundary),
    };
    let resolved_form = if matches!(wire.movement.form, -1 | 9) {
        let mut common = -1i32;
        for member in effective {
            let member_form = world.units.form()[member.row] as i32;
            if member_form != common {
                let had_form = common >= 0;
                common = member_form;
                if had_form {
                    common = -1;
                    break;
                }
            }
        }
        common.max(0)
    } else {
        i32::from(wire.movement.form)
    };
    if !matches!(resolved_form, 0..=5 | 8) {
        return Err(PackageError::UnsupportedFormation {
            form: resolved_form,
        });
    }
    let profiles: Vec<FormationMember> = effective
        .iter()
        .map(|member| {
            let mut profile = if authority.destination_is_water {
                member.authority.water_formation
            } else {
                member.authority.land_formation
            };
            profile.angle = world.units.angle()[member.row];
            profile.width = i32::from(world.units.form_mod()[member.row]);
            profile
        })
        .collect();
    let resolved_width = if wire.movement.width == -1 {
        let (sum, count) = profiles
            .iter()
            .filter(|profile| profile.width != -1)
            .fold((0i32, 0i32), |(sum, count), profile| {
                (sum.wrapping_add(profile.width), count + 1)
            });
        if count == 0 {
            50
        } else {
            sum / count
        }
    } else {
        i32::from(wire.movement.width)
    };

    let leader_mutation = ensure_mutation_for_row(&mut mutations, world, paths, leader_row)?;
    let mut old = if queue == 1 {
        final_position(&mutations[leader_mutation].after, map_tiles)?
    } else {
        (
            mutations[leader_mutation].after.x,
            mutations[leader_mutation].after.y,
        )
    };
    if group.ox >= 0
        && group.oy >= 0
        && crate::systems::groups_guys::vector_dist(
            group.ox.wrapping_sub(old.0),
            group.oy.wrapping_sub(old.1),
        ) < 0x181
    {
        old = (group.ox, group.oy);
    }
    let (layout, actual_angle) = group
        .compute_form(
            &profiles,
            x,
            y,
            resolved_form,
            resolved_width,
            wire.movement.set_angle != 0,
            wire.movement.angle,
            old.0,
            old.1,
            authority.force_formation_facing_zero,
        )
        .ok_or(PackageError::FormationComputationFailed)?;
    group.o_angle = actual_angle;
    group.ox = x;
    group.oy = y;
    group.disband = 0;

    let group_order_id = frame
        .wrapping_mul(10)
        .wrapping_add(group.id)
        .wrapping_mul(100)
        .wrapping_add(group.order_num);
    let group_leader = group.list[layout.leader_index];
    let ordinary_kind = movement_order_kind(wire.movement.orders);
    for (member_index, member) in effective.iter().enumerate() {
        let row = member.row;
        let identity = &member.identity;
        let facts = &member.authority;
        let angle_offset = (group.angles[member_index] as i32).wrapping_mul(0x0100_0000);
        let member_angle = actual_angle.wrapping_add(angle_offset);
        let destination = (
            formation_order_coord(layout.to_x[member_index])
                .wrapping_mul(0x30)
                .wrapping_add(0x18),
            formation_order_coord(layout.to_y[member_index])
                .wrapping_mul(0x30)
                .wrapping_add(0x18),
        );
        let masks = mutations
            .iter()
            .find(|entry| entry.before.identity.handle == identity.handle)
            .map_or_else(
                || world.units.get_unit_masks(row),
                |entry| entry.after.unit_masks,
            );
        let promote_group = matches!(wire.movement.orders, 1 | 2)
            && resolved_form != 6
            && effective.len() > 1
            && !facts.land_formation.modern_infantry
            && (facts.role & 0x10 == 0 || masks & 0x0004_0000 != 0)
            && masks & 4 == 0
            && facts.domain != 1;
        let kind = if promote_group {
            if wire.movement.orders == 2 {
                OrderIndex::GroupAttackTo
            } else {
                OrderIndex::GroupMove
            }
        } else {
            ordinary_kind
        };
        let order = build_move_order(
            kind,
            destination,
            member_angle,
            layout.reverse,
            (x, y),
            wire.movement.disembark != 0,
            promote_group.then_some((group_leader, wire.who, group_order_id, member_index)),
        );
        let mutation_index = ensure_mutation_for_row(&mut mutations, world, paths, row)?;
        let mutation = &mut mutations[mutation_index];
        let becomes_current = queue == 2 || mutation.before.orders.is_empty();
        if queue == 2 {
            if wire.movement.orders == 2
                && mutation
                    .after
                    .orders
                    .current()
                    .is_some_and(|order| order.kind == OrderIndex::Attack)
            {
                return Err(PackageError::AttackReplacementBoundary {
                    handle: identity.handle,
                });
            }
            mutation.after.orders.replace(order);
            mutation.after.unit_masks &= !0x0400_0000;
        } else {
            mutation.after.orders.push(order);
        }
        if becomes_current {
            mutation.after.path = initial_direct_move_path(destination, 0);
        }
        mutation.after.unit_masks &= !0x400;
        if becomes_current {
            mutation.after.orders_x = destination.0;
            mutation.after.orders_y = destination.1;
        }
    }
    group.update_positions(world.units.angle()[leader_row]);
    group.order_num = group.order_num.wrapping_add(1);

    Ok(PreparedGroupMovePackage {
        play,
        lockstep_serial,
        frame,
        random_state: world.random.state(),
        wire,
        group_slot,
        selected: effective
            .iter()
            .map(|member| member.identity.clone())
            .collect(),
        command_state_before: selection.command_state_before,
        command_state_after,
        groups_before: selection.groups_before,
        groups_after,
        authority_revision: selection.authority_revision,
        authority_digest: selection.authority_digest,
        authority_members: selection.authority_members,
        units: mutations,
    })
}

/// Revalidate one detached Unit/order/path before an outer canonical transaction publishes.
pub fn unit_still_current(world: &World, paths: &[PathStack], image: &UnitImage) -> bool {
    let Some(address_row) =
        world.unit_row_at(i32::from(image.identity.who), i32::from(image.identity.o))
    else {
        return false;
    };
    if world.row_of(image.identity.handle) != Some(address_row)
        || world.handle_at_row(address_row) != Some(image.identity.handle)
        || world.units.get_flags(address_row) & OBJ_FLAG_ACTIVE == 0
        || world.units.get_who(address_row) != image.identity.who
        || world.units.o()[address_row] != image.identity.o
        || world.units.get_uid(address_row) != image.identity.uid
        || world.units.group()[address_row] != image.group
        || world.units.get_unit_masks(address_row) != image.unit_masks
        || world.units.form()[address_row] != image.form
        || world.units.form_mod()[address_row] != image.form_mod
        || world.units.angle()[address_row] != image.angle
        || world.units.x_internal()[address_row] != image.x
        || world.units.y_internal()[address_row] != image.y
        || world.units.orders_x()[address_row] != image.orders_x
        || world.units.orders_y()[address_row] != image.orders_y
        || world.units.dest_angle()[address_row] != image.dest_angle
        || world.orders(address_row) != &image.orders
        || paths.get(address_row) != Some(&image.path)
    {
        return false;
    }
    true
}

/// Revalidate every owner, then publish the transaction without a fallible tail.
pub fn commit_group_move_package(
    world: &mut World,
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    authority: &GroupMoveAuthority,
    prepared: PreparedGroupMovePackage,
) -> Result<GroupMovePackageReceipt, PackageError> {
    if command_state != &prepared.command_state_before {
        return Err(PackageError::StaleCommandState);
    }
    if !groups_equal(groups, &prepared.groups_before) {
        return Err(PackageError::StaleGroups);
    }
    if authority.revision != prepared.authority_revision
        || authority.composition_digest != prepared.authority_digest
        || authority.members != prepared.authority_members
    {
        return Err(PackageError::StaleAuthority);
    }
    for mutation in &prepared.units {
        if !unit_still_current(world, paths, &mutation.before) {
            return Err(PackageError::StaleUnit {
                handle: mutation.before.identity.handle,
            });
        }
    }

    let mut checksum = crate::systems::groups_guys::CheckSum::default();
    prepared.groups_after.check_groups(&mut checksum);
    let receipt = GroupMovePackageReceipt {
        play: prepared.play,
        lockstep_serial: prepared.lockstep_serial,
        frame: prepared.frame,
        who: prepared.wire.who,
        group_slot: prepared.group_slot,
        selected: prepared.selected.clone(),
        command_state_revision: prepared.command_state_after.revision(),
        groups_checksum: checksum.value,
        random_state_before: prepared.random_state,
        random_state_after: prepared.random_state,
    };
    *groups = prepared.groups_after;
    *command_state = prepared.command_state_after;
    for mutation in prepared.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("all identities were revalidated before publication");
        world.units.group_mut()[row] = mutation.after.group;
        world.units.set_unit_masks(row, mutation.after.unit_masks);
        world.units.orders_x_mut()[row] = mutation.after.orders_x;
        world.units.orders_y_mut()[row] = mutation.after.orders_y;
        world.units.dest_angle_mut()[row] = mutation.after.dest_angle;
        *world.orders_mut(row) = mutation.after.orders;
        paths[row] = mutation.after.path;
    }
    Ok(receipt)
}
