//! Fail-closed join from canonical setup Units to the Sim-owned Group-Move authority.
//!
//! [`crate::setup_unit_member_authority`] proves setup allocation identities and their current
//! canonical Unit images.  [`don_sim::systems::land_speed_authority`] proves the live result of
//! `UnitData::speed`.  This module requires those products to cover the same complete Sim
//! snapshot before it calls the existing canonical Group-Move authority producer.  It creates
//! no Unit, supplies no default speed, and never reads a recorded checksum.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use don_sim::systems::canonical_group_move_host::{GroupMoveAuthority, UnitImage};
use don_sim::systems::group_move_authority::{
    produce_group_move_authority, GroupMoveAuthorityError, GroupMoveContent, GroupMoveTypeFacts,
};
use don_sim::systems::land_speed_authority::{
    LandSpeedAuthorityError, LandSpeedConstants, LandSpeedContent, LandSpeedTypeFacts,
    ResolvedLandSpeedAuthority,
};
use don_sim::tick::Sim;
use don_sim::world::{Handle, OBJ_FLAG_ACTIVE};

use crate::setup_unit_member_authority::{
    CanonicalSetupMemberSource, CanonicalSetupUnitMemberReceipt,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupGroupMoveAuthoritySource {
    CompleteCanonicalSetupSnapshotAndBoundLandSpeed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupGroupMoveAuthorityReceipt {
    pub source: SetupGroupMoveAuthoritySource,
    pub setup_source: CanonicalSetupMemberSource,
    pub setup_revision: u64,
    pub setup_digest: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub frame: i32,
    pub setup_members: usize,
    pub land_speed_revision: u64,
    pub land_speed_digest: [u8; 32],
    pub land_speed_state_digest: u64,
    pub authority: GroupMoveAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupGroupMoveAuthorityError {
    EmptySetupMembers,
    MissingSetupRevision,
    MissingSetupDigest,
    MissingLandSpeedRevision,
    MissingLandSpeedDigest,
    MixedSetupAuthority { index: usize },
    WrongFrame { expected: i32, actual: i32 },
    DuplicateSetupRow { row: usize },
    SetupCoverageMismatch { active_rows: usize, receipts: usize },
    MissingCanonicalRow { row: usize },
    InactiveCanonicalRow { row: usize },
    StaleCanonicalUnit { row: usize },
    ConflictingTypeFacts { type_id: i32 },
    MissingLandSpeedType { type_id: i32 },
    LandSpeedTypeMismatch { type_id: i32 },
    LandSpeed(LandSpeedAuthorityError),
    GroupMove(GroupMoveAuthorityError),
}

impl fmt::Display for SetupGroupMoveAuthorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "canonical setup Group-Move authority refused: {self:?}")
    }
}

impl std::error::Error for SetupGroupMoveAuthorityError {}

impl From<LandSpeedAuthorityError> for SetupGroupMoveAuthorityError {
    fn from(value: LandSpeedAuthorityError) -> Self {
        Self::LandSpeed(value)
    }
}

impl From<GroupMoveAuthorityError> for SetupGroupMoveAuthorityError {
    fn from(value: GroupMoveAuthorityError) -> Self {
        Self::GroupMove(value)
    }
}

fn current_unit_image(sim: &Sim, row: usize) -> Option<UnitImage> {
    let handle = sim.world.handle_at_row(row)?;
    let who = sim.world.units.get_who(row);
    let o = sim.world.units.o()[row];
    let path = sim.paths.get(row)?.clone();
    Some(UnitImage {
        identity: don_sim::systems::canonical_group_move_host::UnitIdentity {
            handle,
            who,
            o,
            uid: sim.world.units.get_uid(row),
        },
        group: sim.world.units.group()[row],
        unit_masks: sim.world.units.get_unit_masks(row),
        form: sim.world.units.form()[row],
        form_mod: sim.world.units.form_mod()[row],
        angle: sim.world.units.angle()[row],
        x: sim.world.units.x_internal()[row],
        y: sim.world.units.y_internal()[row],
        orders_x: sim.world.units.orders_x()[row],
        orders_y: sim.world.units.orders_y()[row],
        dest_angle: sim.world.units.dest_angle()[row],
        orders: sim.world.orders(row).clone(),
        path,
    })
}

struct SetupContent<'a, C> {
    setup_revision: u64,
    types: BTreeMap<i32, GroupMoveTypeFacts>,
    land: &'a C,
}

impl<C: LandSpeedContent> GroupMoveContent for SetupContent<'_, C> {
    fn revision(&self) -> u64 {
        self.setup_revision.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ self.land.land_speed_revision()
    }

    fn type_facts(&self, type_id: i32) -> Option<GroupMoveTypeFacts> {
        self.types.get(&type_id).copied()
    }
}

impl<C: LandSpeedContent> LandSpeedContent for SetupContent<'_, C> {
    fn land_speed_revision(&self) -> u64 {
        self.land.land_speed_revision()
    }

    fn land_speed_composition_digest(&self) -> [u8; 32] {
        self.land.land_speed_composition_digest()
    }

    fn land_speed_type(&self, type_id: i32) -> Option<LandSpeedTypeFacts> {
        self.land.land_speed_type(type_id)
    }

    fn land_speed_constants(&self) -> Option<LandSpeedConstants> {
        self.land.land_speed_constants()
    }
}

/// Produce the complete canonical Group-Move authority for one setup-derived Sim snapshot.
///
/// `members` must cover every active Unit row, not merely the selected command cohort. Retail's
/// fixed Group allocator can normalize earlier groups while choosing a slot, so a partial table
/// would make that allocator depend on absent object facts. The supplied land-speed authority is
/// re-evaluated and bound to the same immutable `sim` borrow before Group authority is produced.
pub fn produce_setup_group_move_authority<C: LandSpeedContent>(
    sim: &Sim,
    members: &[CanonicalSetupUnitMemberReceipt],
    land_content: &C,
    land_speeds: &ResolvedLandSpeedAuthority,
    destination: (i32, i32),
    force_formation_facing_zero: bool,
) -> Result<SetupGroupMoveAuthorityReceipt, SetupGroupMoveAuthorityError> {
    let Some(first) = members.first() else {
        return Err(SetupGroupMoveAuthorityError::EmptySetupMembers);
    };
    if first.authority_revision == 0 {
        return Err(SetupGroupMoveAuthorityError::MissingSetupRevision);
    }
    if first.authority_digest == [0; 32] {
        return Err(SetupGroupMoveAuthorityError::MissingSetupDigest);
    }
    if land_content.land_speed_revision() == 0 {
        return Err(SetupGroupMoveAuthorityError::MissingLandSpeedRevision);
    }
    if land_content.land_speed_composition_digest() == [0; 32] {
        return Err(SetupGroupMoveAuthorityError::MissingLandSpeedDigest);
    }
    if sim.world.frame != first.frame {
        return Err(SetupGroupMoveAuthorityError::WrongFrame {
            expected: first.frame,
            actual: sim.world.frame,
        });
    }

    let mut rows = BTreeSet::new();
    let mut types = BTreeMap::new();
    for (index, member) in members.iter().enumerate() {
        if member.authority_revision != first.authority_revision
            || member.authority_digest != first.authority_digest
            || member.source != first.source
            || member.replay_file_sha256 != first.replay_file_sha256
            || member.frame != first.frame
        {
            return Err(SetupGroupMoveAuthorityError::MixedSetupAuthority { index });
        }
        if !rows.insert(member.row) {
            return Err(SetupGroupMoveAuthorityError::DuplicateSetupRow { row: member.row });
        }
        if member.row >= sim.world.live_count() as usize {
            return Err(SetupGroupMoveAuthorityError::MissingCanonicalRow { row: member.row });
        }
        if sim.world.units.get_flags(member.row) & OBJ_FLAG_ACTIVE == 0 {
            return Err(SetupGroupMoveAuthorityError::InactiveCanonicalRow { row: member.row });
        }
        let current_type = sim.unit_type.get(member.row).copied();
        if current_type != Some(member.current_type)
            || sim.world.unit_type_id(member.row) != Some(member.current_type)
            || current_unit_image(sim, member.row).as_ref() != Some(&member.unit)
        {
            return Err(SetupGroupMoveAuthorityError::StaleCanonicalUnit { row: member.row });
        }
        let facts = member.group_move_type_facts();
        if let Some(previous) = types.insert(facts.type_id, facts) {
            if previous != facts {
                return Err(SetupGroupMoveAuthorityError::ConflictingTypeFacts {
                    type_id: facts.type_id,
                });
            }
        }
        let land = land_content.land_speed_type(facts.type_id).ok_or(
            SetupGroupMoveAuthorityError::MissingLandSpeedType {
                type_id: facts.type_id,
            },
        )?;
        if land.type_id != facts.type_id
            || land.graft != member.type_facts.graft
            || land.domain != facts.domain
            || land.unit_flags != facts.unit_flags
            || land.unit_flags2 != facts.unit_flags2
        {
            return Err(SetupGroupMoveAuthorityError::LandSpeedTypeMismatch {
                type_id: facts.type_id,
            });
        }
    }

    let active_rows = (0..sim.world.live_count() as usize)
        .filter(|&row| sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0)
        .count();
    if rows.len() != active_rows
        || (0..sim.world.live_count() as usize)
            .filter(|&row| sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE != 0)
            .any(|row| !rows.contains(&row))
    {
        return Err(SetupGroupMoveAuthorityError::SetupCoverageMismatch {
            active_rows,
            receipts: rows.len(),
        });
    }

    let content = SetupContent {
        setup_revision: first.authority_revision,
        types,
        land: land_content,
    };
    let bound = land_speeds.bind(sim, &content)?;
    let authority =
        produce_group_move_authority(sim, &bound, destination, force_formation_facing_zero)?;
    Ok(SetupGroupMoveAuthorityReceipt {
        source: SetupGroupMoveAuthoritySource::CompleteCanonicalSetupSnapshotAndBoundLandSpeed,
        setup_source: first.source,
        setup_revision: first.authority_revision,
        setup_digest: first.authority_digest,
        replay_file_sha256: first.replay_file_sha256,
        frame: first.frame,
        setup_members: members.len(),
        land_speed_revision: land_speeds.content_revision,
        land_speed_digest: land_speeds.composition_digest,
        land_speed_state_digest: land_speeds.state_digest,
        authority,
    })
}

/// Stable identity helper for callers building exact speed evidence maps.
pub const fn setup_member_handle(member: &CanonicalSetupUnitMemberReceipt) -> Handle {
    member.unit.identity.handle
}
