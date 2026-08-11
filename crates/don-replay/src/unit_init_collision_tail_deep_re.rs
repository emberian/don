//! Exact setup continuation from the first completed `Unit::set_new_location` through the
//! common return of `Unit::init`.
//!
//! The preceding producer deliberately journals `CollCheck::move_unit`.  This continuation
//! validates that journal against the final Guy lattice, applies it to the canonical terrain
//! `World` with the instruction-derived collision implementation, then performs every
//! synchronized store in `Unit::init` from `0x00612CD9` onward.  The four large stat routines
//! are represented by address-bound scalar receipts: on a fresh Unit `o_down == -1`, so their
//! only Unit-channel outputs are respectively `myhits`, `mylos`, `myspeed`, and `myarmor`.
//! `Object::update_seen(0)` remains an explicit world/visibility request because its
//! newly-explored arm reaches the still-external `World::reveal_fog` authority.

use crate::unit_init_location_deep_re::{
    CollisionMoveUnitRequest, UnitInitLocationExternalResidual, UnitInitLocationReceipt,
    COLL_CHECK_MOVE_UNIT_VA,
};
use don_sim::systems::casters_animals::{mana_capacity, ManaCapacityInput};
use don_sim::systems::collision::move_unit;
use don_sim::systems::map_terrain::World;
use std::fmt;

pub const UNIT_INIT_POST_LOCATION_BEGIN_VA: u32 = 0x0061_2cd9;
pub const UNIT_INIT_POST_LOCATION_RETURN_VA: u32 = 0x0061_2f7f;

pub const OBJECT_DATA_CAN_CARRY_CALL_VA: u32 = 0x0061_2d6a;
pub const OBJECT_DATA_CAN_CARRY_VA: u32 = 0x0064_6c40;
pub const UNIT_UPDATE_HITS_CALL_VA: u32 = 0x0061_2d8f;
pub const UNIT_UPDATE_HITS_VA: u32 = 0x0060_e930;
pub const UNIT_UPDATE_LOS_CALL_VA: u32 = 0x0061_2d99;
pub const UNIT_UPDATE_LOS_VA: u32 = 0x0060_e4d0;
pub const UNIT_UPDATE_SPEED_CALL_VA: u32 = 0x0061_2da1;
pub const UNIT_UPDATE_SPEED_VA: u32 = 0x0060_55c0;
pub const UNIT_UPDATE_ARMOR_CALL_VA: u32 = 0x0061_2da8;
pub const UNIT_UPDATE_ARMOR_VA: u32 = 0x0060_54c0;
pub const OBJECT_UPDATE_SEEN_CALL_VA: u32 = 0x0061_2db3;
pub const OBJECT_UPDATE_SEEN_VA: u32 = 0x0065_1b80;
pub const UNIT_IS_3A_CALL_VA: u32 = 0x0061_2de0;
pub const UNIT_MANA_CALL_VA: u32 = 0x0061_2e51;
pub const UNIT_MANA_VA: u32 = 0x0060_9a50;
pub const UNIT_IS_143_CALL_VA: u32 = 0x0061_2e7d;
pub const UNIT_SET_STANCE_CALL_VA: u32 = 0x0061_2e99;
pub const UNIT_SET_STANCE_VA: u32 = 0x0060_5310;
pub const LEADER_LAKOTA_CALL_VA: u32 = 0x0061_2ea8;
pub const UNIT_LAKOTA_IS_45_CALL_VA: u32 = 0x0061_2ee4;
pub const LEADER_AMERICANS_CALL_VA: u32 = 0x0061_2f07;
pub const UNIT_AMERICANS_IS_45_CALL_VA: u32 = 0x0061_2f3f;
pub const LEADER_HAS_TRIBE_BONUS_VA: u32 = 0x006e_1370;
pub const OBJECT_DATA_IS_FORWARDER_VA: u32 = 0x0065_3790;

pub const LEADER_ACTIVE_UNIT_FLAG: u32 = 0x0080_0000;
pub const LEADER_STARTING_ECON_FLAG: u32 = 0x0200_0000;
pub const UNIT_CAN_CARRY_AIR_FLAG: u32 = 0x0020_0000;
pub const UNIT_DEFAULT_MAP_FLAG: u32 = 0x0004_0000;
pub const UNIT_STANCE_CAPABLE_FLAG: u32 = 0x0000_0004;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitTailScalarField {
    MyHits,
    MyLos,
    MySpeed,
    MyArmor,
}

/// One scalar returned and stored by a large exact retail child body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitTailScalarReceipt {
    pub call_va: u32,
    pub body_va: u32,
    pub field: UnitTailScalarField,
    pub returned: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitTailStatReceipts {
    pub hits: UnitTailScalarReceipt,
    pub los: UnitTailScalarReceipt,
    pub speed: UnitTailScalarReceipt,
    pub armor: UnitTailScalarReceipt,
}

/// Exact type predicates consumed after `Unit::set_new_location` returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitTailTypeFacts {
    pub domain: i32,
    pub type_id: i32,
    /// `TypeData::where` / lineage word at `UnitTypeData +0x40`.
    pub type_line: i32,
    /// Type-owned `UnitTypeData::unit_flags2 +0x2B8`.  It is not the instance
    /// `UnitData::unit_masks2 +0x6C` word.
    pub type_unit_flags2: u32,
    /// Three strict `ObjectData::is(type, 1)` answers in `can_carry(DOMAIN_AIR)`.
    pub is_1bf_strict: bool,
    pub is_15f_strict: bool,
    pub is_208_strict: bool,
    /// Common-tail non-strict type queries.
    pub is_3a: bool,
    pub is_143: bool,
    pub is_45: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitTailLeaderFacts {
    pub leader_flags_before: u32,
    pub lakota: bool,
    pub americans: bool,
    /// `Constants +0x848` / `+0x888`; the native body tests only nonzero.
    pub lakota_food: i32,
    pub americans_barracks_gather: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnitInitCollisionTailInputs {
    pub location: UnitInitLocationReceipt,
    pub type_facts: UnitTailTypeFacts,
    pub leader: UnitTailLeaderFacts,
    pub stats: UnitTailStatReceipts,
    pub mana: ManaCapacityInput,
    /// State established earlier in `Unit::init`, before the Guy/location seam.
    pub unit_masks2_before_tail: u32,
    pub stance_before_tail: i8,
    /// `SubObjectData::flags +0x08`, needed by the deferred visibility body.
    pub object_flags: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollisionBlockDelta {
    pub block: (i32, i32),
    pub allocated: bool,
    pub flags_before: Option<i32>,
    pub flags_after: Option<i32>,
    /// Bit ordinals `0..255` whose value changed, in ascending order.
    pub changed_bits: Vec<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityUpdateRequest {
    pub call_va: u32,
    pub body_va: u32,
    pub incremental: i32,
    pub owner: u8,
    pub o: i16,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    /// The scalar established by `Unit::update_los`; `UnitData::los` may add
    /// leader/general bonuses when the deferred body runs.
    pub source_mylos: i8,
    pub type_domain: i32,
    pub type_unit_flags2: u32,
    /// The instance word as it exists at the call: after `can_carry(2)`, before
    /// the later default-map bit is installed.
    pub unit_masks: u32,
    pub object_flags: u8,
    /// Earlier `Unit::init` writes `ObjectData::infiltrated +0x3A = 0`.
    pub infiltrated: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitTailCallKind {
    CollisionMove,
    CanCarryDomainTwo,
    UpdateHits,
    UpdateLos,
    UpdateSpeed,
    UpdateArmor,
    UpdateSeen,
    IsType3a,
    Mana,
    IsType143,
    SetStance,
    LakotaBonus,
    IsType45Lakota,
    AmericansBonus,
    IsType45Americans,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitTailCallReceipt {
    pub ordinal: u16,
    pub kind: UnitTailCallKind,
    pub call_va: u32,
    pub body_va: u32,
}

/// Every synchronized UnitData store at `0x00612CD9..0x00612F7F`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitPostLocationStateReceipt {
    pub collide_frame: i32,
    pub collide: i16,
    pub collide_o: i16,
    pub collide_guy: i16,
    pub collide_who: i8,
    pub o_up: i16,
    pub o_down: i16,
    pub cavarch_o: i16,
    pub cavarch_uid: u16,
    pub cavarch_who: i8,
    pub play: i8,
    pub openlist: u32,
    pub openlistrefs: u32,
    pub closedlist: u32,
    pub validlist: u32,
    pub blocklist: u32,
    pub avoid_x: i32,
    pub avoid_y: i32,
    pub start_dist: i32,
    pub avoid_land: i32,
    pub avoid_sea: i32,
    pub announce_frame: i32,
    pub myhits: i32,
    pub mylos: i8,
    pub myspeed: i16,
    pub myarmor: i16,
    pub spell_time: i16,
    pub stance: i8,
    pub unit_masks: u32,
    pub unit_masks2: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitInitTailExternalResidual {
    /// The call is journaled because newly explored cells synchronously enter
    /// `World::reveal_fog`, whose object/item/leader effects have a separate owner.
    ObjectUpdateSeen(VisibilityUpdateRequest),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnitInitCollisionTailReceipt {
    pub identity: crate::setup_place_unit_deep_re::StableUnitIdentity,
    pub unit: UnitPostLocationStateReceipt,
    pub guys: don_sim::systems::groups_guys::UnitGuys,
    pub collision_requests: Vec<CollisionMoveUnitRequest>,
    pub collision_deltas: Vec<CollisionBlockDelta>,
    pub calls: Vec<UnitTailCallReceipt>,
    pub visibility: VisibilityUpdateRequest,
    pub leader_flags_before: u32,
    pub leader_flags_after: u32,
    pub rng_before: i32,
    pub rng_after: i32,
    pub next_external_residual: UnitInitTailExternalResidual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitInitCollisionTailError {
    WrongLocationSeam,
    UnsupportedNonLandDomain,
    InvalidIdentity,
    InvalidCollisionRequest { ordinal: usize },
    InvalidStatReceipt { field: UnitTailScalarField },
    ScalarOutOfRange { field: UnitTailScalarField },
    InvalidWorld,
}

impl fmt::Display for UnitInitCollisionTailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Unit::init collision/tail continuation refused: {self:?}"
        )
    }
}

impl std::error::Error for UnitInitCollisionTailError {}

#[inline]
fn can_carry_domain_two(f: UnitTailTypeFacts) -> bool {
    f.is_1bf_strict || f.is_15f_strict || f.is_208_strict
}

fn validate_scalar(
    got: UnitTailScalarReceipt,
    field: UnitTailScalarField,
    call_va: u32,
    body_va: u32,
) -> Result<i32, UnitInitCollisionTailError> {
    if got.field != field || got.call_va != call_va || got.body_va != body_va {
        return Err(UnitInitCollisionTailError::InvalidStatReceipt { field });
    }
    Ok(got.returned)
}

fn collision_snapshot(world: &World) -> Vec<Option<(i32, [u8; 96])>> {
    world
        .wdata
        .iter()
        .map(|w| w.block.as_deref().map(|b| (b.flags, b.ptr)))
        .collect()
}

fn collision_deltas(world: &World, before: &[Option<(i32, [u8; 96])>]) -> Vec<CollisionBlockDelta> {
    let mut out = Vec::new();
    for (index, (old, cell)) in before.iter().zip(&world.wdata).enumerate() {
        let new = cell.block.as_deref().map(|b| (b.flags, b.ptr));
        if *old == new {
            continue;
        }
        let mut changed_bits = Vec::new();
        for bit in 0..256usize {
            let a = old
                .as_ref()
                .is_some_and(|(_, p)| p[bit >> 3] & (1 << (bit & 7)) != 0);
            let b = new
                .as_ref()
                .is_some_and(|(_, p)| p[bit >> 3] & (1 << (bit & 7)) != 0);
            if a != b {
                changed_bits.push(bit as u16);
            }
        }
        out.push(CollisionBlockDelta {
            block: (index as i32 % world.xs, index as i32 / world.xs),
            allocated: old.is_none() && new.is_some(),
            flags_before: old.as_ref().map(|x| x.0),
            flags_after: new.as_ref().map(|x| x.0),
            changed_bits,
        });
    }
    out
}

fn expected_stat_receipt(field: UnitTailScalarField) -> (u32, u32) {
    match field {
        UnitTailScalarField::MyHits => (UNIT_UPDATE_HITS_CALL_VA, UNIT_UPDATE_HITS_VA),
        UnitTailScalarField::MyLos => (UNIT_UPDATE_LOS_CALL_VA, UNIT_UPDATE_LOS_VA),
        UnitTailScalarField::MySpeed => (UNIT_UPDATE_SPEED_CALL_VA, UNIT_UPDATE_SPEED_VA),
        UnitTailScalarField::MyArmor => (UNIT_UPDATE_ARMOR_CALL_VA, UNIT_UPDATE_ARMOR_VA),
    }
}

fn push_call(calls: &mut Vec<UnitTailCallReceipt>, kind: UnitTailCallKind, call: u32, body: u32) {
    calls.push(UnitTailCallReceipt {
        ordinal: calls.len() as u16,
        kind,
        call_va: call,
        body_va: body,
    });
}

/// Apply the normal-land collision transaction and finish the common `Unit::init` tail.
///
/// Validation and mutation are atomic: collision changes are made on a clone and committed
/// only after every receipt and branch input has passed validation.
pub fn produce_unit_init_collision_tail(
    world: &mut World,
    inputs: UnitInitCollisionTailInputs,
) -> Result<UnitInitCollisionTailReceipt, UnitInitCollisionTailError> {
    if inputs.location.next_external_residual
        != (UnitInitLocationExternalResidual::UnitInitPostLocationStores {
            next_va: UNIT_INIT_POST_LOCATION_BEGIN_VA,
        })
    {
        return Err(UnitInitCollisionTailError::WrongLocationSeam);
    }
    if inputs.type_facts.domain != 0 {
        return Err(UnitInitCollisionTailError::UnsupportedNonLandDomain);
    }
    let identity = inputs.location.identity;
    if !(0..crate::setup_place_unit_deep_re::PLAYABLE_OWNER_SLOTS).contains(&identity.owner)
        || !(0..crate::setup_place_unit_deep_re::UNIT_BAND_LIMIT).contains(&identity.o)
        || identity.type_index < 0
        || identity.o > i16::MAX as i32
        || inputs.location.guys.guy_mark < 0
        || inputs.location.stable_guys.len() != inputs.location.guys.guys.len()
        || inputs.location.rng_before != inputs.location.rng_after
    {
        return Err(UnitInitCollisionTailError::InvalidIdentity);
    }
    for (slot, (stable, guy)) in inputs
        .location
        .stable_guys
        .iter()
        .zip(&inputs.location.guys.guys)
        .enumerate()
    {
        let Some(guy) = guy else {
            return Err(UnitInitCollisionTailError::InvalidIdentity);
        };
        if stable.unit_id != identity.id
            || stable.unit_generation != identity.generation
            || stable.owner != identity.owner as i8
            || stable.o != identity.o as i16
            || stable.guy_num != slot as i8
            || guy.who != identity.owner as i8
            || guy.o != identity.o as i16
            || guy.guy_num != slot as i8
            || guy.ty != identity.type_index
        {
            return Err(UnitInitCollisionTailError::InvalidIdentity);
        }
    }
    if world.xs <= 0 || world.ys <= 0 || world.wdata.len() != (world.xs * world.ys) as usize {
        return Err(UnitInitCollisionTailError::InvalidWorld);
    }

    let live = inputs.location.guys.guy_mark as usize;
    if live > inputs.location.guys.guys.len()
        || inputs.location.collision_requests.len() != live
        || inputs.location.first_unapplied_shared_mutation
            != inputs.location.collision_requests.first().copied()
    {
        return Err(UnitInitCollisionTailError::InvalidCollisionRequest { ordinal: 0 });
    }
    let expected_radius = inputs
        .location
        .collision_requests
        .first()
        .map_or(0, |request| request.new_block_radius);
    for (ordinal, request) in inputs.location.collision_requests.iter().enumerate() {
        let Some(guy) = inputs
            .location
            .guys
            .guys
            .get(request.slot)
            .and_then(Option::as_ref)
        else {
            return Err(UnitInitCollisionTailError::InvalidCollisionRequest { ordinal });
        };
        let expected_new = (
            don_sim::systems::movement::ucell_of(guy.x),
            don_sim::systems::movement::ucell_of(guy.y),
        );
        if request.ordinal as usize != ordinal
            || request.slot != ordinal
            || request.call_va != crate::unit_init_location_deep_re::GUY_COLLISION_MOVE_CALL_VA
            || request.body_va != COLL_CHECK_MOVE_UNIT_VA
            || request.new_ucoord != expected_new
            || request.new_block_radius != expected_radius
            || request.new_block_radius < 0
            || request.new_block_radius > 10
        {
            return Err(UnitInitCollisionTailError::InvalidCollisionRequest { ordinal });
        }
    }

    let hits = validate_scalar(
        inputs.stats.hits,
        UnitTailScalarField::MyHits,
        UNIT_UPDATE_HITS_CALL_VA,
        UNIT_UPDATE_HITS_VA,
    )?;
    let los = validate_scalar(
        inputs.stats.los,
        UnitTailScalarField::MyLos,
        UNIT_UPDATE_LOS_CALL_VA,
        UNIT_UPDATE_LOS_VA,
    )?;
    let speed = validate_scalar(
        inputs.stats.speed,
        UnitTailScalarField::MySpeed,
        UNIT_UPDATE_SPEED_CALL_VA,
        UNIT_UPDATE_SPEED_VA,
    )?;
    let armor = validate_scalar(
        inputs.stats.armor,
        UnitTailScalarField::MyArmor,
        UNIT_UPDATE_ARMOR_CALL_VA,
        UNIT_UPDATE_ARMOR_VA,
    )?;
    if i8::try_from(los).is_err() {
        return Err(UnitInitCollisionTailError::ScalarOutOfRange {
            field: UnitTailScalarField::MyLos,
        });
    }
    for (field, value) in [
        (UnitTailScalarField::MySpeed, speed),
        (UnitTailScalarField::MyArmor, armor),
    ] {
        if i16::try_from(value).is_err() {
            return Err(UnitInitCollisionTailError::ScalarOutOfRange { field });
        }
    }

    let mut next_world = world.clone();
    let before = collision_snapshot(&next_world);
    let mut calls = Vec::new();
    for request in &inputs.location.collision_requests {
        move_unit(
            &mut next_world,
            request.old_ucoord,
            request.new_ucoord,
            request.new_block_radius,
        );
        push_call(
            &mut calls,
            UnitTailCallKind::CollisionMove,
            request.call_va,
            request.body_va,
        );
    }
    let deltas = collision_deltas(&next_world, &before);

    let mut unit_masks = inputs.location.unit.unit_masks;
    push_call(
        &mut calls,
        UnitTailCallKind::CanCarryDomainTwo,
        OBJECT_DATA_CAN_CARRY_CALL_VA,
        OBJECT_DATA_CAN_CARRY_VA,
    );
    if can_carry_domain_two(inputs.type_facts) {
        unit_masks |= UNIT_CAN_CARRY_AIR_FLAG;
    }

    let mut leader_flags = inputs.leader.leader_flags_before | LEADER_ACTIVE_UNIT_FLAG;
    for (kind, field) in [
        (UnitTailCallKind::UpdateHits, UnitTailScalarField::MyHits),
        (UnitTailCallKind::UpdateLos, UnitTailScalarField::MyLos),
        (UnitTailCallKind::UpdateSpeed, UnitTailScalarField::MySpeed),
        (UnitTailCallKind::UpdateArmor, UnitTailScalarField::MyArmor),
    ] {
        let (call, body) = expected_stat_receipt(field);
        push_call(&mut calls, kind, call, body);
    }

    let visibility = VisibilityUpdateRequest {
        call_va: OBJECT_UPDATE_SEEN_CALL_VA,
        body_va: OBJECT_UPDATE_SEEN_VA,
        incremental: 0,
        owner: identity.owner as u8,
        o: identity.o as i16,
        x: inputs.location.unit.x,
        y: inputs.location.unit.y,
        angle: inputs.location.unit.angle,
        source_mylos: los as i8,
        type_domain: inputs.type_facts.domain,
        type_unit_flags2: inputs.type_facts.type_unit_flags2,
        unit_masks,
        object_flags: inputs.object_flags,
        infiltrated: 0,
    };
    push_call(
        &mut calls,
        UnitTailCallKind::UpdateSeen,
        OBJECT_UPDATE_SEEN_CALL_VA,
        OBJECT_UPDATE_SEEN_VA,
    );

    if leader_flags & 0x0c != 0x04 {
        unit_masks |= UNIT_DEFAULT_MAP_FLAG;
    }

    push_call(
        &mut calls,
        UnitTailCallKind::IsType3a,
        UNIT_IS_3A_CALL_VA,
        OBJECT_DATA_IS_FORWARDER_VA,
    );
    let spell_time = if inputs.type_facts.is_3a {
        push_call(
            &mut calls,
            UnitTailCallKind::Mana,
            UNIT_MANA_CALL_VA,
            UNIT_MANA_VA,
        );
        let mana = mana_capacity(inputs.mana);
        let half = mana.wrapping_sub(mana >> 31) >> 1;
        half as i16
    } else {
        0
    };

    push_call(
        &mut calls,
        UnitTailCallKind::IsType143,
        UNIT_IS_143_CALL_VA,
        OBJECT_DATA_IS_FORWARDER_VA,
    );
    let mut unit_masks2 = inputs.unit_masks2_before_tail;
    let mut stance = inputs.stance_before_tail;
    if inputs.type_facts.is_143 {
        unit_masks2 |= UNIT_STANCE_CAPABLE_FLAG;
        if leader_flags & 0x04 == 0 {
            push_call(
                &mut calls,
                UnitTailCallKind::SetStance,
                UNIT_SET_STANCE_CALL_VA,
                UNIT_SET_STANCE_VA,
            );
            stance = 3;
        }
    } else {
        unit_masks2 &= !UNIT_STANCE_CAPABLE_FLAG;
    }

    push_call(
        &mut calls,
        UnitTailCallKind::LakotaBonus,
        LEADER_LAKOTA_CALL_VA,
        LEADER_HAS_TRIBE_BONUS_VA,
    );
    if inputs.leader.lakota && inputs.leader.lakota_food != 0 {
        let qualifies = if matches!(inputs.type_facts.type_id, 0x32 | 0x33) {
            true
        } else {
            push_call(
                &mut calls,
                UnitTailCallKind::IsType45Lakota,
                UNIT_LAKOTA_IS_45_CALL_VA,
                OBJECT_DATA_IS_FORWARDER_VA,
            );
            inputs.type_facts.is_45 || inputs.type_facts.type_line == 0x1ac
        };
        if qualifies {
            leader_flags |= LEADER_STARTING_ECON_FLAG;
        }
    }
    push_call(
        &mut calls,
        UnitTailCallKind::AmericansBonus,
        LEADER_AMERICANS_CALL_VA,
        LEADER_HAS_TRIBE_BONUS_VA,
    );
    if inputs.leader.americans
        && inputs.leader.americans_barracks_gather != 0
        && inputs.type_facts.type_line == 0x1ab
    {
        push_call(
            &mut calls,
            UnitTailCallKind::IsType45Americans,
            UNIT_AMERICANS_IS_45_CALL_VA,
            OBJECT_DATA_IS_FORWARDER_VA,
        );
        if !inputs.type_facts.is_45 {
            leader_flags |= LEADER_STARTING_ECON_FLAG;
        }
    }

    let unit = UnitPostLocationStateReceipt {
        collide_frame: -1,
        collide: 0,
        collide_o: -1,
        collide_guy: -1,
        collide_who: -1,
        o_up: -1,
        o_down: -1,
        cavarch_o: -1,
        cavarch_uid: 0,
        cavarch_who: 0,
        play: -1,
        openlist: 0,
        openlistrefs: 0,
        closedlist: 0,
        validlist: 0,
        blocklist: 0,
        avoid_x: -1,
        avoid_y: -1,
        start_dist: 0,
        avoid_land: 0,
        avoid_sea: 0,
        announce_frame: -1,
        myhits: hits,
        mylos: los as i8,
        myspeed: speed as i16,
        myarmor: armor as i16,
        spell_time,
        stance,
        unit_masks,
        unit_masks2,
    };

    *world = next_world;
    Ok(UnitInitCollisionTailReceipt {
        identity,
        unit,
        guys: inputs.location.guys,
        collision_requests: inputs.location.collision_requests,
        collision_deltas: deltas,
        calls,
        visibility,
        leader_flags_before: inputs.leader.leader_flags_before,
        leader_flags_after: leader_flags,
        rng_before: inputs.location.rng_after,
        rng_after: inputs.location.rng_after,
        next_external_residual: UnitInitTailExternalResidual::ObjectUpdateSeen(visibility),
    })
}
