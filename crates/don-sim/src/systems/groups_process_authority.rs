//! Exact detached transaction for the fixed-pool `Groups::process` callback cone.
//!
//! Retail `Groups::process` `0x006FA210` visits one slot in each active player's 64-slot
//! band, inlines `Group::normalize`, calls `Group::find_role`, and finally writes both speed
//! fields from `Group::find_leader(0) -> UnitData::speed`.  The live scheduler still supplies
//! unconditional `Keep` and no speed.  This module closes the bounded state transaction without
//! installing a stale approximation into that scheduler.
//!
//! The object predicates are instruction-derived.  `Group::normalize` tests the active bit,
//! virtual `+0x18` (`is_unit`), the Unit backlink at `+0x80`, and virtual `+0x20`.  For the
//! supported executable, UnitData's vtable is `0x00B41B08`; slot `+0x20` is `0x0041BFF0`
//! (`xor eax,eax; ret`).  BuildData's vtable is `0x00B426DC`; slot `+0x20` is `0x0041E0E0`
//! (`mov eax,1; ret`).  Therefore a live Unit is retained according to its backlink, while a
//! Build-band member is always evicted.  Wall-band members remain a typed boundary.
//!
//! A plan borrows a current [`World`] and current [`GroupMoveAuthority`] together.  The latter
//! must have been regenerated for this exact state by an authoritative caller: its `speed` is a
//! dynamic `UnitData::speed` result, not immutable type content.  Preparation changes no owner;
//! commit compares the entire fixed-pool before-image before one assignment.

#![forbid(unsafe_code)]

use crate::objects::{BUILD_BAND_BASE, WALL_BAND_BASE};
use crate::systems::canonical_group_move_host::{
    groups_equal, recompute_group, GroupMoveAuthority, PackageError,
};
use crate::systems::groups_guys::{
    Groups, MemberState, GROUPS_PER_PLAYER, GROUP_MAX_MEMBERS, NUM_GROUPS, NUM_LEADERS,
};
use crate::world::{World, OBJ_FLAG_ACTIVE};

/// Provenance for the bounded transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupsProcessAuthoritySource {
    /// `Groups::process` `0x006FA210`, `Group::normalize` `0x00711540`,
    /// `Group::find_role` `0x007081F0`, and `Group::find_leader` `0x0070CCB0`.
    ExecutableFixedPoolAndHandleBoundUnitAuthority,
}

/// What one exact fixed-pool pass would publish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupsProcessReceipt {
    pub source: GroupsProcessAuthoritySource,
    pub slot: usize,
    pub active_players: usize,
    pub groups_processed: usize,
    pub members_before: usize,
    pub members_after: usize,
    pub members_removed: usize,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
}

/// Detached before/after images.  Fields stay private so callers cannot relabel an arbitrary
/// `Groups` value as an executable-authority plan.
#[derive(Clone, Debug)]
pub struct PreparedGroupsProcess {
    before: Groups,
    after: Groups,
    receipt: GroupsProcessReceipt,
}

impl PreparedGroupsProcess {
    pub fn before(&self) -> &Groups {
        &self.before
    }

    pub fn after(&self) -> &Groups {
        &self.after
    }

    pub fn receipt(&self) -> &GroupsProcessReceipt {
        &self.receipt
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupsProcessAuthorityError {
    InvalidPool,
    InvalidCursor { cursor: i32 },
    InvalidMemberCount { group: usize, num: i32 },
    UnsupportedWallMember { who: usize, o: i16 },
    MissingAuthorityRevision,
    MissingAuthorityDigest,
    UnitAuthority(PackageError),
    StaleGroups,
}

impl std::fmt::Display for GroupsProcessAuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "authoritative Groups::process refused: {self:?}")
    }
}

impl std::error::Error for GroupsProcessAuthorityError {}

impl From<PackageError> for GroupsProcessAuthorityError {
    fn from(value: PackageError) -> Self {
        Self::UnitAuthority(value)
    }
}

fn validate_pool(groups: &Groups) -> Result<usize, GroupsProcessAuthorityError> {
    if groups.list.len() != NUM_GROUPS
        || groups
            .list
            .iter()
            .enumerate()
            .any(|(index, group)| group.id != index as i32)
    {
        return Err(GroupsProcessAuthorityError::InvalidPool);
    }
    let slot = usize::try_from(groups.proc_group)
        .ok()
        .filter(|slot| *slot < GROUPS_PER_PLAYER)
        .ok_or(GroupsProcessAuthorityError::InvalidCursor {
            cursor: groups.proc_group,
        })?;
    Ok(slot)
}

fn unit_member_state(world: &World, group_id: i32, who: usize, o: i16) -> MemberState {
    let Some(row) = world.unit_row_at(who as i32, i32::from(o)) else {
        return MemberState::Dead;
    };
    if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return MemberState::Dead;
    }
    if world.units.group()[row] != group_id as i16 {
        MemberState::NotOurUnit
    } else {
        // UnitData vtable +0x20 is the constant-false function in the supported executable.
        MemberState::Keep
    }
}

fn member_state(
    world: &World,
    group_id: i32,
    who: usize,
    o: i16,
) -> Result<MemberState, GroupsProcessAuthorityError> {
    let object = i32::from(o);
    if object < BUILD_BAND_BASE as i32 {
        return Ok(unit_member_state(world, group_id, who, o));
    }
    if object < WALL_BAND_BASE as i32 {
        // BuildData vtable +0x20 is constant true. Active and inactive Builds both take a
        // removal arm, so no Build after-image is needed to determine the Group transaction.
        return Ok(MemberState::LeavesGroups);
    }
    Err(GroupsProcessAuthorityError::UnsupportedWallMember { who, o })
}

/// Prepare one exact `Groups::process` fixed-pool pass.
///
/// This function deliberately does not read a recorded checksum and does not mutate `groups`.
/// The supplied `authority` must contain the current Handle-bound facts for every retained Unit
/// in a processed group. Empty groups and members which are provably removed need no authority.
pub fn prepare_groups_process(
    world: &World,
    groups: &Groups,
    leader_active: &[bool; NUM_LEADERS],
    authority: &GroupMoveAuthority,
) -> Result<PreparedGroupsProcess, GroupsProcessAuthorityError> {
    let slot = validate_pool(groups)?;
    let mut after = groups.clone();
    let mut members_before = 0usize;
    let mut members_after = 0usize;
    let mut groups_processed = 0usize;

    for who in 0..NUM_LEADERS {
        if !leader_active[who] {
            continue;
        }
        groups_processed += 1;
        let group_index = Groups::index(who, slot);
        let group = &mut after.list[group_index];
        if !(0..=GROUP_MAX_MEMBERS as i32).contains(&group.num) {
            return Err(GroupsProcessAuthorityError::InvalidMemberCount {
                group: group_index,
                num: group.num,
            });
        }
        let before_count = group.num as usize;
        members_before += before_count;

        let mut states = Vec::with_capacity(before_count);
        for &o in &group.list[..before_count] {
            let state = if o < 0 {
                MemberState::Dead
            } else {
                member_state(world, group.id, who, o)?
            };
            states.push((o, state));
        }
        let keep = |o: i16| {
            states
                .iter()
                .find_map(|&(candidate, state)| (candidate == o).then_some(state))
                .unwrap_or(MemberState::Dead)
        };
        group.normalize(&keep);

        if group.num > 0 && group.buildings == 0 {
            if authority.revision == 0 {
                return Err(GroupsProcessAuthorityError::MissingAuthorityRevision);
            }
            if authority.composition_digest == [0; 32] {
                return Err(GroupsProcessAuthorityError::MissingAuthorityDigest);
            }
        }
        // This is the exact find_role + find_leader(0)/speed tail. It also rejects a retained
        // Unit whose current Handle is absent from the same authority snapshot.
        recompute_group(group, world, authority)?;
        members_after += group.num as usize;
    }

    after.proc_group += 1;
    if after.proc_group >= GROUPS_PER_PLAYER as i32 {
        after.proc_group = 0;
    }
    let receipt = GroupsProcessReceipt {
        source: GroupsProcessAuthoritySource::ExecutableFixedPoolAndHandleBoundUnitAuthority,
        slot,
        active_players: leader_active.iter().filter(|&&active| active).count(),
        groups_processed,
        members_before,
        members_after,
        members_removed: members_before - members_after,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
    };
    Ok(PreparedGroupsProcess {
        before: groups.clone(),
        after,
        receipt,
    })
}

/// Publish a prepared pass if the complete fixed-pool before-image is still current.
pub fn commit_groups_process(
    groups: &mut Groups,
    prepared: PreparedGroupsProcess,
) -> Result<GroupsProcessReceipt, GroupsProcessAuthorityError> {
    if !groups_equal(groups, &prepared.before) {
        return Err(GroupsProcessAuthorityError::StaleGroups);
    }
    *groups = prepared.after;
    Ok(prepared.receipt)
}
