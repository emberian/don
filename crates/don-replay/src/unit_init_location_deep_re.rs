//! Exact source-only continuation of the new-Unit setup seam through
//! `Unit::update_gpiece` and `Unit::set_new_location`.
//!
//! This consumes the identity-bound `UnitGuyInitPrefixReceipt` produced immediately before
//! `0x00612CC1`.  It materializes the synchronized Unit/Guy state while journaling, rather
//! than performing, the shared collision writes.  Terrain results are explicit ordered
//! receipts.  No replay channel is installed here.

use crate::setup_place_unit_deep_re::{
    GuyGraphicsInitReceipt, StableGuyIdentity, UnitGuyExternalResidual, UnitGuyInitPrefixReceipt,
    PLAYABLE_OWNER_SLOTS, UNIT_SET_NEW_LOCATION_VA, UNIT_UPDATE_GPIECE_VA,
};
use don_sim::systems::groups_guys::{sinx, UnitGuys, UnitTypeStats};
use don_sim::systems::unit_inctime::{SUPPORTED_RETAIL_EXE_SHA256, SUPPORTED_UNIT_GRAPHICS_SHA256};
use std::fmt;

pub const UNIT_INIT_LOCATION_CONTINUATION_BEGIN_VA: u32 = 0x0061_2cc1;
pub const UNIT_INIT_LOCATION_CONTINUATION_END_VA: u32 = 0x0061_2cd9;
pub const UNIT_INIT_SET_ANGLE: i32 = 0x5555_5555;
pub const COORD_XOR: i32 = 0x0006_3637;

pub const UNIT_UPDATE_GPIECE_BYTES: u32 = 138;
pub const UNIT_UPDATE_SQUAD_GUY_CALL_VA: u32 = 0x005e_2939;
pub const UNIT_UPDATE_CREW_GUY_CALL_VA: u32 = 0x005e_2989;
pub const GUY_UPDATE_GPIECE_VA: u32 = 0x005d_8530;
pub const GUY_UPDATE_GPIECE_BYTES: u32 = 363;

pub const UNIT_SET_NEW_LOCATION_BYTES: u32 = 1_757;
pub const UNIT_TERRAIN_Z_CALL_VA: u32 = 0x005f_9069;
pub const TERRAIN_FIND_TCOORD_Z_VA: u32 = 0x0085_44a0;
pub const UNIT_SINGLE_GUY_SET_ANGLE_CALL_VA: u32 = 0x005f_9194;
pub const UNIT_SINGLE_GUY_SET_LOCATION_CALL_VA: u32 = 0x005f_91c2;
pub const UNIT_LATTICE_SET_ANGLE_CALL_VA: u32 = 0x005f_9316;
pub const UNIT_LATTICE_SET_LOCATION_CALL_VA: u32 = 0x005f_93b3;

pub const GUY_SET_NEW_LOCATION_VA: u32 = 0x005d_86f0;
pub const GUY_SET_NEW_LOCATION_BYTES: u32 = 899;
pub const GUY_COLLISION_MOVE_CALL_VA: u32 = 0x005d_8797;
pub const COLL_CHECK_MOVE_UNIT_VA: u32 = 0x0068_2ad0;
pub const GUY_TERRAIN_Z_CALL_VA: u32 = 0x005d_87ed;
pub const TERRAIN_FIND_DATA_Z_VA: u32 = 0x0086_6560;
pub const GUY_SET_ANGLE_VA: u32 = 0x005d_9010;
pub const GUY_SET_ANGLE_BYTES: u32 = 550;
pub const SET_ANGLE_CREW_ANGLE_CALL_VA: u32 = 0x005d_91e2;
pub const SET_ANGLE_CREW_LOCATION_CALL_VA: u32 = 0x005d_91f1;
pub const SET_LOCATION_CREW_ANGLE_CALL_VA: u32 = 0x005d_8a22;
pub const SET_LOCATION_CREW_LOCATION_CALL_VA: u32 = 0x005d_8a31;

pub const AIR_Z_TARGET_OFFSET: i32 = 1_000;
pub const AIR_Z_STEP_CLAMP: i32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainQueryKind {
    UnitTcoord,
    GuyCoord,
}

/// One externally measured height answer in exact call order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainHeightReceipt {
    pub ordinal: u16,
    pub call_va: u32,
    pub body_va: u32,
    pub kind: TerrainQueryKind,
    pub x: i32,
    pub y: i32,
    pub final_arg: i32,
    pub returned_z: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuyLocationPhase {
    /// First recursion from `Guy::set_angle`, before Guy 0 itself is moved from `-1536`.
    CrewFromSetAngle,
    /// The Unit-owned live-squad placement.
    SquadFinal,
    /// Second recursion from Guy 0 after its final position has been installed.
    CrewFromSetLocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuyCallKind {
    UpdateGpiece,
    SetAngle,
    SetNewLocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuyCallReceipt {
    pub ordinal: u16,
    pub caller_va: u32,
    pub body_va: u32,
    pub slot: usize,
    pub phase: Option<GuyLocationPhase>,
    pub kind: GuyCallKind,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub snap: bool,
    /// `Unit::update_gpiece` clears `GuyOut +0xD0` after every Guy call. This byte is not
    /// in the synchronized 155-byte image but is retained in the chronology receipt.
    pub presentation_hint_reset: bool,
}

/// Shared `CollCheck::move_unit` mutation that the pure producer journals but does not apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollisionMoveUnitRequest {
    pub ordinal: u16,
    pub call_va: u32,
    pub body_va: u32,
    pub slot: usize,
    pub old_ucoord: (i32, i32),
    pub new_ucoord: (i32, i32),
    pub new_block_radius: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnitInitLocationInputs {
    pub prefix: UnitGuyInitPrefixReceipt,
    /// Fresh outputs of the same coherent graphics hierarchy at `Unit::update_gpiece`.
    pub graphics: Vec<GuyGraphicsInitReceipt>,
    pub unit_type: UnitTypeStats,
    pub formation: i8,
    pub unit_masks: u32,
    /// `UnitTypeData +0x2B4 & 0x20`, used only when `domain == 2`.
    pub domain_two_tracks_ground: bool,
    /// Already normalized 48-unit cell center passed by `Unit::init` to `SubObject::init`
    /// and then unchanged to `Unit::set_new_location`.
    pub anchor_x: i32,
    pub anchor_y: i32,
    /// Exclusive Coord bounds, `WorldData::xs/ys * 0x300`.
    pub world_max_x: i32,
    pub world_max_y: i32,
    pub terrain: Vec<TerrainHeightReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitLocationStateReceipt {
    pub x: i32,
    pub y: i32,
    pub encoded_x: i32,
    pub encoded_y: i32,
    pub z: i32,
    pub angle: i32,
    pub formation: i8,
    pub unit_masks: u32,
    pub moved_wcoord: bool,
    pub moved_tcoord: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitInitLocationExternalResidual {
    /// The next instruction after the call returns is `or ecx,-1` at `0x00612CD9`.
    UnitInitPostLocationStores { next_va: u32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnitInitLocationReceipt {
    pub identity: crate::setup_place_unit_deep_re::StableUnitIdentity,
    pub unit: UnitLocationStateReceipt,
    pub guys: UnitGuys,
    pub stable_guys: Vec<StableGuyIdentity>,
    pub graphics_calls: Vec<GuyCallReceipt>,
    pub location_calls: Vec<GuyCallReceipt>,
    pub collision_requests: Vec<CollisionMoveUnitRequest>,
    pub terrain: Vec<TerrainHeightReceipt>,
    pub rng_before: i32,
    pub rng_after: i32,
    pub first_unapplied_shared_mutation: Option<CollisionMoveUnitRequest>,
    pub next_external_residual: UnitInitLocationExternalResidual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitInitLocationError {
    WrongPrefixSeam,
    WrongIdentity,
    InvalidCounts,
    InvalidArrayShape,
    InvalidGuyIdentity { slot: usize },
    GraphicsCountMismatch,
    UnsupportedGraphics { slot: usize },
    WrongGraphicsSlot { slot: usize },
    MissingGpiece { slot: usize },
    GuyZeroHasTrackOffset,
    UnsupportedCrewOnlyGuyZero,
    InvalidWorldBounds,
    UnnormalizedAnchor,
    AnchorOutsideWorld,
    TerrainReceiptMissing { ordinal: usize },
    TerrainReceiptMismatch { ordinal: usize },
    TerrainReceiptSurplus { first_surplus: usize },
}

impl fmt::Display for UnitInitLocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unit::init location continuation refused: {self:?}")
    }
}

impl std::error::Error for UnitInitLocationError {}

#[inline]
fn div_3_table(v: i32) -> i32 {
    let q = v / 3;
    let r = v % 3;
    if v < 0 && r != 0 {
        q - 1
    } else {
        q
    }
}

#[inline]
fn ucoord(v: i32) -> i32 {
    div_3_table(v >> 4)
}

#[inline]
fn tcoord(v: i32) -> i32 {
    div_3_table(v >> 6)
}

#[inline]
fn normalize_unit_init_coord(v: i32) -> i32 {
    ucoord(v).wrapping_mul(48).wrapping_add(24)
}

#[inline]
fn clamp_coord(v: i32, max: i32) -> i32 {
    if v < 0 {
        0
    } else if v >= max {
        max.wrapping_sub(1)
    } else {
        v
    }
}

fn crew_destination(
    base_x: i32,
    base_y: i32,
    angle: i32,
    track_dx: i32,
    track_dy: i32,
    world_max_x: i32,
    world_max_y: i32,
) -> (i32, i32) {
    let mut x = base_x;
    let mut y = base_y;
    if track_dx != 0 {
        x = x.wrapping_add(sinx(angle.wrapping_add(0x4000_0000), track_dx));
        y = y.wrapping_add(sinx(angle, track_dx));
    }
    if track_dy != 0 {
        x = x.wrapping_add(sinx(angle.wrapping_add(i32::MIN), track_dy));
        y = y.wrapping_add(sinx(angle.wrapping_add(0x4000_0000), track_dy));
    }
    if track_dx != 0 || track_dy != 0 {
        x = clamp_coord(x, world_max_x);
        y = clamp_coord(y, world_max_y);
    }
    (x, y)
}

struct TerrainCursor<'a> {
    receipts: &'a [TerrainHeightReceipt],
    at: usize,
    consumed: Vec<TerrainHeightReceipt>,
}

impl<'a> TerrainCursor<'a> {
    fn new(receipts: &'a [TerrainHeightReceipt]) -> Self {
        Self {
            receipts,
            at: 0,
            consumed: Vec::with_capacity(receipts.len()),
        }
    }

    fn take(
        &mut self,
        call_va: u32,
        body_va: u32,
        kind: TerrainQueryKind,
        x: i32,
        y: i32,
        final_arg: i32,
    ) -> Result<i32, UnitInitLocationError> {
        let ordinal = self.at;
        let receipt = *self
            .receipts
            .get(ordinal)
            .ok_or(UnitInitLocationError::TerrainReceiptMissing { ordinal })?;
        if receipt.ordinal as usize != ordinal
            || receipt.call_va != call_va
            || receipt.body_va != body_va
            || receipt.kind != kind
            || receipt.x != x
            || receipt.y != y
            || receipt.final_arg != final_arg
        {
            return Err(UnitInitLocationError::TerrainReceiptMismatch { ordinal });
        }
        self.at += 1;
        self.consumed.push(receipt);
        Ok(receipt.returned_z)
    }

    fn finish(self) -> Result<Vec<TerrainHeightReceipt>, UnitInitLocationError> {
        if self.at != self.receipts.len() {
            return Err(UnitInitLocationError::TerrainReceiptSurplus {
                first_surplus: self.at,
            });
        }
        Ok(self.consumed)
    }
}

struct LocationJournal {
    calls: Vec<GuyCallReceipt>,
    collisions: Vec<CollisionMoveUnitRequest>,
}

impl LocationJournal {
    fn new() -> Self {
        Self {
            calls: Vec::new(),
            collisions: Vec::new(),
        }
    }

    fn call(
        &mut self,
        caller_va: u32,
        body_va: u32,
        slot: usize,
        phase: GuyLocationPhase,
        kind: GuyCallKind,
        x: i32,
        y: i32,
        angle: i32,
        snap: bool,
    ) {
        self.calls.push(GuyCallReceipt {
            ordinal: self.calls.len() as u16,
            caller_va,
            body_va,
            slot,
            phase: Some(phase),
            kind,
            x,
            y,
            angle,
            snap,
            presentation_hint_reset: false,
        });
    }
}

fn apply_one_location(
    guys: &mut UnitGuys,
    slot: usize,
    x: i32,
    y: i32,
    snap: bool,
    phase: GuyLocationPhase,
    unit_type: UnitTypeStats,
    domain_two_tracks_ground: bool,
    terrain: &mut TerrainCursor<'_>,
    journal: &mut LocationJournal,
) -> Result<(), UnitInitLocationError> {
    let guy = guys.guys[slot]
        .as_mut()
        .ok_or(UnitInitLocationError::InvalidGuyIdentity { slot })?;
    if unit_type.domain != 2 && i32::from(guy.guy_num) < unit_type.squad_size {
        let old = (ucoord(guy.x), ucoord(guy.y));
        let new = (ucoord(x), ucoord(y));
        if old != new {
            journal.collisions.push(CollisionMoveUnitRequest {
                ordinal: journal.collisions.len() as u16,
                call_va: GUY_COLLISION_MOVE_CALL_VA,
                body_va: COLL_CHECK_MOVE_UNIT_VA,
                slot,
                old_ucoord: old,
                new_ucoord: new,
                new_block_radius: unit_type.new_block_radius,
            });
        }
    }

    guy.x = x;
    guy.y = y;
    match unit_type.domain {
        2 if domain_two_tracks_ground => {
            let ground = terrain.take(
                GUY_TERRAIN_Z_CALL_VA,
                TERRAIN_FIND_DATA_Z_VA,
                TerrainQueryKind::GuyCoord,
                x,
                y,
                0,
            )?;
            let mut delta = ground.wrapping_sub(guy.z).wrapping_add(AIR_Z_TARGET_OFFSET);
            if delta < -AIR_Z_STEP_CLAMP {
                delta = -AIR_Z_STEP_CLAMP;
            } else if delta > AIR_Z_STEP_CLAMP {
                delta = AIR_Z_STEP_CLAMP;
            }
            guy.z = guy.z.wrapping_add(delta);
        }
        2 => {}
        1 => guy.z = 0,
        _ => {
            guy.z = terrain.take(
                GUY_TERRAIN_Z_CALL_VA,
                TERRAIN_FIND_DATA_Z_VA,
                TerrainQueryKind::GuyCoord,
                x,
                y,
                0,
            )?;
        }
    }
    if snap {
        guy.last_x = x;
        guy.last_y = y;
        guy.last_z = guy.z;
    }
    let angle = guy.angle;
    journal.call(
        match phase {
            GuyLocationPhase::CrewFromSetAngle => SET_ANGLE_CREW_LOCATION_CALL_VA,
            GuyLocationPhase::CrewFromSetLocation => SET_LOCATION_CREW_LOCATION_CALL_VA,
            GuyLocationPhase::SquadFinal => 0,
        },
        GUY_SET_NEW_LOCATION_VA,
        slot,
        phase,
        GuyCallKind::SetNewLocation,
        x,
        y,
        angle,
        snap,
    );
    Ok(())
}

fn set_crew_angle(
    guys: &mut UnitGuys,
    slot: usize,
    angle: i32,
    caller_va: u32,
    phase: GuyLocationPhase,
    journal: &mut LocationJournal,
) -> Result<(), UnitInitLocationError> {
    let guy = guys.guys[slot]
        .as_mut()
        .ok_or(UnitInitLocationError::InvalidGuyIdentity { slot })?;
    guy.des_angle = angle;
    guy.angle = angle;
    guy.last_angle = angle;
    journal.call(
        caller_va,
        GUY_SET_ANGLE_VA,
        slot,
        phase,
        GuyCallKind::SetAngle,
        guy.x,
        guy.y,
        angle,
        true,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn propagate_crew(
    guys: &mut UnitGuys,
    base_x: i32,
    base_y: i32,
    angle: i32,
    unit_type: UnitTypeStats,
    domain_two_tracks_ground: bool,
    world_max_x: i32,
    world_max_y: i32,
    phase: GuyLocationPhase,
    terrain: &mut TerrainCursor<'_>,
    journal: &mut LocationJournal,
) -> Result<(), UnitInitLocationError> {
    let total = guys.guys.len();
    for slot in unit_type.squad_size as usize..total {
        let guy = guys.guys[slot]
            .as_ref()
            .ok_or(UnitInitLocationError::InvalidGuyIdentity { slot })?;
        let (x, y) = crew_destination(
            base_x,
            base_y,
            angle,
            guy.track_dx,
            guy.track_dy,
            world_max_x,
            world_max_y,
        );
        {
            let guy = guys.guys[slot].as_mut().expect("validated above");
            guy.des_angle = angle;
            guy.des_x = x;
            guy.des_y = y;
        }
        let angle_call = match phase {
            GuyLocationPhase::CrewFromSetAngle => SET_ANGLE_CREW_ANGLE_CALL_VA,
            GuyLocationPhase::CrewFromSetLocation => SET_LOCATION_CREW_ANGLE_CALL_VA,
            GuyLocationPhase::SquadFinal => unreachable!("squad phase never propagates crew"),
        };
        set_crew_angle(guys, slot, angle, angle_call, phase, journal)?;
        apply_one_location(
            guys,
            slot,
            x,
            y,
            true,
            phase,
            unit_type,
            domain_two_tracks_ground,
            terrain,
            journal,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn set_squad_angle(
    guys: &mut UnitGuys,
    slot: usize,
    angle: i32,
    caller_va: u32,
    unit_type: UnitTypeStats,
    domain_two_tracks_ground: bool,
    world_max_x: i32,
    world_max_y: i32,
    terrain: &mut TerrainCursor<'_>,
    journal: &mut LocationJournal,
) -> Result<(), UnitInitLocationError> {
    let (x, y, guy_num) = {
        let guy = guys.guys[slot]
            .as_mut()
            .ok_or(UnitInitLocationError::InvalidGuyIdentity { slot })?;
        guy.des_angle = angle;
        guy.angle = angle;
        guy.last_angle = angle;
        (guy.x, guy.y, guy.guy_num)
    };
    journal.call(
        caller_va,
        GUY_SET_ANGLE_VA,
        slot,
        GuyLocationPhase::SquadFinal,
        GuyCallKind::SetAngle,
        x,
        y,
        angle,
        true,
    );
    if guy_num == 0 {
        propagate_crew(
            guys,
            x,
            y,
            angle,
            unit_type,
            domain_two_tracks_ground,
            world_max_x,
            world_max_y,
            GuyLocationPhase::CrewFromSetAngle,
            terrain,
            journal,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn set_squad_location(
    guys: &mut UnitGuys,
    slot: usize,
    x: i32,
    y: i32,
    caller_va: u32,
    unit_type: UnitTypeStats,
    domain_two_tracks_ground: bool,
    world_max_x: i32,
    world_max_y: i32,
    terrain: &mut TerrainCursor<'_>,
    journal: &mut LocationJournal,
) -> Result<(), UnitInitLocationError> {
    apply_one_location(
        guys,
        slot,
        x,
        y,
        true,
        GuyLocationPhase::SquadFinal,
        unit_type,
        domain_two_tracks_ground,
        terrain,
        journal,
    )?;
    let call = journal
        .calls
        .last_mut()
        .expect("location call was appended");
    call.caller_va = caller_va;
    let (angle, guy_num) = {
        let guy = guys.guys[slot].as_ref().expect("slot validated");
        (guy.angle, guy.guy_num)
    };
    if guy_num == 0 {
        propagate_crew(
            guys,
            x,
            y,
            angle,
            unit_type,
            domain_two_tracks_ground,
            world_max_x,
            world_max_y,
            GuyLocationPhase::CrewFromSetLocation,
            terrain,
            journal,
        )?;
    }
    Ok(())
}

/// Continue the exact new-Unit path from `0x00612CC1` through the return at `0x00612CD9`.
///
/// `SubObject::init` has already installed the same normalized anchor, so the generic
/// `Unit::set_new_location` movement arms are provably not selected.  Shared collision
/// writes are emitted in native call order.  Terrain answers must match every native query.
/// The four covered bodies make no game-RNG call, so the state word is preserved.
pub fn produce_unit_init_location_continuation(
    inputs: UnitInitLocationInputs,
) -> Result<UnitInitLocationReceipt, UnitInitLocationError> {
    if inputs.prefix.first_external_residual
        != (UnitGuyExternalResidual::UpdateGpieceThenSetNewLocation {
            update_gpiece_va: UNIT_UPDATE_GPIECE_VA,
            set_new_location_va: UNIT_SET_NEW_LOCATION_VA,
        })
    {
        return Err(UnitInitLocationError::WrongPrefixSeam);
    }
    let identity = inputs.prefix.identity;
    let rng_state = inputs.prefix.rng_after_guys;
    if !(0..PLAYABLE_OWNER_SLOTS).contains(&identity.owner)
        || inputs.prefix.stable_guys.len() != inputs.prefix.guys.guys.len()
    {
        return Err(UnitInitLocationError::WrongIdentity);
    }
    let total = inputs
        .unit_type
        .squad_size
        .checked_add(inputs.unit_type.crew_size)
        .ok_or(UnitInitLocationError::InvalidCounts)?;
    if inputs.unit_type.squad_size < 0
        || inputs.unit_type.crew_size < 0
        || total < 0
        || total as usize != inputs.prefix.guys.guys.len()
        || inputs.prefix.guy_mark as i32 != inputs.unit_type.squad_size
    {
        return Err(UnitInitLocationError::InvalidCounts);
    }
    if inputs.unit_type.squad_size == 0 && inputs.unit_type.crew_size > 0 {
        return Err(UnitInitLocationError::UnsupportedCrewOnlyGuyZero);
    }
    if inputs.prefix.array_length != total
        || inputs.prefix.array_capacity != total
        || inputs.prefix.array_increment != 1
        || inputs.prefix.array_flags != 0
        || inputs.prefix.guys.size != total
        || inputs.prefix.guys.increment != 1
        || inputs.prefix.guys.flags != 0
    {
        return Err(UnitInitLocationError::InvalidArrayShape);
    }
    for (slot, (stable, guy)) in inputs
        .prefix
        .stable_guys
        .iter()
        .zip(inputs.prefix.guys.guys.iter())
        .enumerate()
    {
        let Some(guy) = guy else {
            return Err(UnitInitLocationError::InvalidGuyIdentity { slot });
        };
        if *stable
            != (StableGuyIdentity {
                unit_id: identity.id,
                unit_generation: identity.generation,
                owner: identity.owner as i8,
                o: identity.o as i16,
                guy_num: slot as i8,
            })
            || guy.who != identity.owner as i8
            || guy.o != identity.o as i16
            || guy.guy_num != slot as i8
            || guy.ty != identity.type_index
        {
            return Err(UnitInitLocationError::InvalidGuyIdentity { slot });
        }
    }
    if inputs.graphics.len() != total as usize {
        return Err(UnitInitLocationError::GraphicsCountMismatch);
    }
    for (slot, graphics) in inputs.graphics.iter().enumerate() {
        if graphics.provenance.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
            || graphics.provenance.installed_unit_graphics_sha256 != SUPPORTED_UNIT_GRAPHICS_SHA256
            || !graphics.provenance.coherent_capture
        {
            return Err(UnitInitLocationError::UnsupportedGraphics { slot });
        }
        if graphics.extracted.guy_num != slot as i8 {
            return Err(UnitInitLocationError::WrongGraphicsSlot { slot });
        }
        if graphics.extracted.gpiece < 0 {
            return Err(UnitInitLocationError::MissingGpiece { slot });
        }
    }
    if inputs
        .graphics
        .first()
        .is_some_and(|g| g.extracted.track_dx != 0 || g.extracted.track_dy != 0)
    {
        return Err(UnitInitLocationError::GuyZeroHasTrackOffset);
    }
    if inputs.world_max_x <= 0 || inputs.world_max_y <= 0 {
        return Err(UnitInitLocationError::InvalidWorldBounds);
    }
    if normalize_unit_init_coord(inputs.anchor_x) != inputs.anchor_x
        || normalize_unit_init_coord(inputs.anchor_y) != inputs.anchor_y
    {
        return Err(UnitInitLocationError::UnnormalizedAnchor);
    }
    if inputs.anchor_x < 0
        || inputs.anchor_y < 0
        || inputs.anchor_x >= inputs.world_max_x
        || inputs.anchor_y >= inputs.world_max_y
    {
        return Err(UnitInitLocationError::AnchorOutsideWorld);
    }

    let mut guys = inputs.prefix.guys.clone();
    let mut graphics_calls = Vec::with_capacity(total as usize);
    let squad = inputs.unit_type.squad_size as usize;
    let live = inputs.prefix.guy_mark.max(0) as usize;
    for slot in 0..live {
        let guy = guys.guys[slot]
            .as_mut()
            .ok_or(UnitInitLocationError::InvalidGuyIdentity { slot })?;
        let graphics = &inputs.graphics[slot].extracted;
        guy.gpiece = graphics.gpiece;
        if slot == 0 {
            guy.track_dx = 0;
            guy.track_dy = 0;
        } else {
            guy.track_dx = graphics.track_dx;
            guy.track_dy = graphics.track_dy;
        }
        graphics_calls.push(GuyCallReceipt {
            ordinal: graphics_calls.len() as u16,
            caller_va: UNIT_UPDATE_SQUAD_GUY_CALL_VA,
            body_va: GUY_UPDATE_GPIECE_VA,
            slot,
            phase: None,
            kind: GuyCallKind::UpdateGpiece,
            x: guy.x,
            y: guy.y,
            angle: guy.angle,
            snap: false,
            presentation_hint_reset: true,
        });
    }
    for slot in squad..total as usize {
        let guy = guys.guys[slot]
            .as_mut()
            .ok_or(UnitInitLocationError::InvalidGuyIdentity { slot })?;
        let graphics = &inputs.graphics[slot].extracted;
        guy.gpiece = graphics.gpiece;
        if slot == 0 {
            guy.track_dx = 0;
            guy.track_dy = 0;
        } else {
            guy.track_dx = graphics.track_dx;
            guy.track_dy = graphics.track_dy;
        }
        graphics_calls.push(GuyCallReceipt {
            ordinal: graphics_calls.len() as u16,
            caller_va: UNIT_UPDATE_CREW_GUY_CALL_VA,
            body_va: GUY_UPDATE_GPIECE_VA,
            slot,
            phase: None,
            kind: GuyCallKind::UpdateGpiece,
            x: guy.x,
            y: guy.y,
            angle: guy.angle,
            snap: false,
            presentation_hint_reset: true,
        });
    }

    let mut terrain = TerrainCursor::new(&inputs.terrain);
    let unit_z = terrain.take(
        UNIT_TERRAIN_Z_CALL_VA,
        TERRAIN_FIND_TCOORD_Z_VA,
        TerrainQueryKind::UnitTcoord,
        tcoord(inputs.anchor_x),
        tcoord(inputs.anchor_y),
        1,
    )?;
    let mut journal = LocationJournal::new();
    let locations = UnitGuys::initial_squad_locations(
        live,
        inputs.anchor_x,
        inputs.anchor_y,
        UNIT_INIT_SET_ANGLE,
        inputs.formation,
        inputs.unit_masks,
        inputs.world_max_x,
        inputs.world_max_y,
        &inputs.unit_type,
    );
    for (slot, (x, y)) in locations.into_iter().enumerate() {
        let (angle_call, location_call) = if live == 1 {
            (
                UNIT_SINGLE_GUY_SET_ANGLE_CALL_VA,
                UNIT_SINGLE_GUY_SET_LOCATION_CALL_VA,
            )
        } else {
            (
                UNIT_LATTICE_SET_ANGLE_CALL_VA,
                UNIT_LATTICE_SET_LOCATION_CALL_VA,
            )
        };
        set_squad_angle(
            &mut guys,
            slot,
            UNIT_INIT_SET_ANGLE,
            angle_call,
            inputs.unit_type,
            inputs.domain_two_tracks_ground,
            inputs.world_max_x,
            inputs.world_max_y,
            &mut terrain,
            &mut journal,
        )?;
        {
            let guy = guys.guys[slot].as_mut().expect("validated squad slot");
            guy.des_x = x;
            guy.des_y = y;
        }
        set_squad_location(
            &mut guys,
            slot,
            x,
            y,
            location_call,
            inputs.unit_type,
            inputs.domain_two_tracks_ground,
            inputs.world_max_x,
            inputs.world_max_y,
            &mut terrain,
            &mut journal,
        )?;
    }
    let terrain = terrain.finish()?;
    let first_unapplied_shared_mutation = journal.collisions.first().copied();

    Ok(UnitInitLocationReceipt {
        identity,
        unit: UnitLocationStateReceipt {
            x: inputs.anchor_x,
            y: inputs.anchor_y,
            encoded_x: inputs.anchor_x ^ COORD_XOR,
            encoded_y: inputs.anchor_y ^ COORD_XOR,
            z: unit_z,
            angle: UNIT_INIT_SET_ANGLE,
            formation: inputs.formation,
            unit_masks: inputs.unit_masks,
            moved_wcoord: false,
            moved_tcoord: false,
        },
        guys,
        stable_guys: inputs.prefix.stable_guys,
        graphics_calls,
        location_calls: journal.calls,
        collision_requests: journal.collisions,
        terrain,
        rng_before: rng_state,
        rng_after: rng_state,
        first_unapplied_shared_mutation,
        next_external_residual: UnitInitLocationExternalResidual::UnitInitPostLocationStores {
            next_va: UNIT_INIT_LOCATION_CONTINUATION_END_VA,
        },
    })
}
