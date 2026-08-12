// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only owner for the exact Unit/detector portion of step-12 fog production.
//!
//! This module deliberately stops before the shared [`crate::tick::Sim`] bridge. It freezes
//! the retail cadence, the instance/type transition which creates `OBJECT_DETECTOR`, the
//! Unit-band admission gates, and the post-producer plane sample needed by
//! `external_entity_visibility_frontier`. No caller may turn a missing detector or resolved
//! `UnitData::los()` fact into `false`/zero.

#![allow(dead_code)]

pub const GAME_DAEMON_PROCESS_ALL_VA: u32 = 0x0073_2700;
pub const GAME_DAEMON_UPDATE_ALL_SEEN_VA: u32 = 0x0073_2840;
pub const OBJECT_INIT_VA: u32 = 0x0064_7750;
pub const OBJECT_UPDATE_SEEN_VA: u32 = 0x0065_1b80;
pub const OBJECT_HAS_GENERAL_VA: u32 = 0x0064_6b00;
pub const UNIT_DATA_LOS_VA: u32 = 0x0061_00c0;
pub const UNIT_DATA_IS_ON_MAP_VA: u32 = 0x0046_ce30;
pub const UNIT_DATA_IS_VALID_UNIT_VA: u32 = 0x0046_cda0;
pub const WORLD_CLEAR_SEEN_VA: u32 = 0x006b_2250;
pub const WORLD_SET_SEEN_VA: u32 = 0x006b_3c60;
pub const PROJECT_VA: u32 = 0x0092_cf40;

/// Non-scheduler call sites which force the same complete producer immediately. They are
/// `Game::run`, `Build::close`, Scenario `add_visibility`, `remove_visibility`, and
/// `set_explored_show_buildings`, respectively.
pub const UPDATE_ALL_SEEN_DIRECT_CALL_SITES: [u32; 5] = [
    0x0058_525d,
    0x0062_95f7,
    0x009f_c78d,
    0x009f_c82d,
    0x00a0_33f5,
];

pub const LEADER_SLOTS: usize = 8;
pub const FULL_REFRESH_PERIOD: i32 = 100;
pub const FULL_REFRESH_PHASE: i32 = 33;
pub const UPDATE_ALL_SEEN_BUSY_VALUE: i32 = 4;
pub const FINE_PER_TILE: i32 = 0xc0;
pub const FINE_PER_FOG_CELL: i32 = 0x180;
pub const MAX_FOG_RADIUS: i32 = 0x40;
pub const UNIT_ANGLE_OFFSET: usize = 0x50;
pub const SMALL_LOS_PROJECT_DISTANCE: i32 = 0x180;

/// `SubObjectData::flags` (`+0x08`) values from the shipped PDB enum.
pub const OBJECT_VALID: u8 = 0x01;
pub const OBJECT_STARTED: u8 = 0x02;
pub const OBJECT_ACTIVE: u8 = 0x04;
pub const OBJECT_IDLING: u8 = 0x08;
pub const OBJECT_NEW_THINK: u8 = 0x10;
pub const OBJECT_CITY: u8 = 0x20;
pub const OBJECT_DETECTOR: u8 = 0x40;
pub const OBJECT_ATTACKING: u8 = 0x80;

/// `ObjMaskType::OBJMASK_DETECT`, uppercase `Z` in the shipped object-mask alphabet.
pub const OBJMASK_DETECT: u32 = 0x0200_0000;

pub const PTOLEMY_TYPE_INDEX: i32 = 0x16a;
pub const THE_CEO_TYPE_INDEX: i32 = 0x165;
/// `LeaderData::num_units[0x138]` and `[0x133]`; adding the unit-table base `0x32`
/// recovers the two TypeIndexes above.
pub const PTOLEMY_NUM_UNITS_INDEX: usize = 0x138;
pub const THE_CEO_NUM_UNITS_INDEX: usize = 0x133;
pub const PTOLEMY_ROLE_MASK: u32 = 0x0000_0420;
pub const SMALL_LOS_STANDARD_TYPE_FLAGS2: u32 = 0x0000_0004;
pub const SMALL_LOS_STANDARD_UNIT_MASKS: u32 = 0x0000_0001;
pub const CONSTANTS_PTOLEMY_LOS_BONUS_OFFSET: usize = 0x0b64;
pub const CONSTANTS_THE_CEO_UNIT_LOS_OFFSET: usize = 0x0ca8;

/// CheckSums channel and `World::walk_data` section containing `seen`, `seen2`, and `seen3`.
pub const WORLD_CHECKSUM_CHANNEL: u8 = 12;
pub const WORLD_FOG_WALK_SECTION: u8 = 6;

/// Stages in `GameDaemon::update_all_seen`, in retail order after its fog-option early
/// return. The final two stages are conditional. The Build/Wall and scenario/alliance bodies
/// remain separate integration frontiers; naming them here prevents a Unit-only port from
/// claiming the whole producer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullRefreshStage {
    SetDaemonBusyFour,
    ClearSeenAndWcoordSeen,
    ClearDetectedSeen3,
    WalkBuildAndWallBands,
    WalkUnitBands,
    ConsiderScenarioRevealPoints,
    ConsiderFrameZeroExploredSharing,
}

pub const FULL_REFRESH_STAGES: [FullRefreshStage; 7] = [
    FullRefreshStage::SetDaemonBusyFour,
    FullRefreshStage::ClearSeenAndWcoordSeen,
    FullRefreshStage::ClearDetectedSeen3,
    FullRefreshStage::WalkBuildAndWallBands,
    FullRefreshStage::WalkUnitBands,
    FullRefreshStage::ConsiderScenarioRevealPoints,
    FullRefreshStage::ConsiderFrameZeroExploredSharing,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step12VisibilityCadence {
    /// `GameDaemon::process_all` does not call the producer on this signed frame remainder.
    NotScheduled,
    /// The call is scheduled, but `Game +0x30 == 3` returns before even writing `busy = 4`.
    SuppressedByFogOptionThree,
    /// Enter the producer body and visit every stage in [`FULL_REFRESH_STAGES`].
    FullRefresh,
}

/// Every known way the complete producer is entered. Only [`ScheduledStep12`](Self::ScheduledStep12)
/// is subject to the phase-33 gate; the five other variants are the immediate call sites in
/// [`UPDATE_ALL_SEEN_DIRECT_CALL_SITES`]. The fog-option-three early return remains inside the
/// producer and therefore suppresses every trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step12VisibilityTrigger {
    ScheduledStep12,
    GameRun,
    BuildClose,
    ScenarioAddVisibility,
    ScenarioRemoveVisibility,
    ScenarioSetExploredShowBuildings,
}

#[inline]
pub const fn visibility_entry(
    frame: i32,
    fog_option: u8,
    trigger: Step12VisibilityTrigger,
) -> Step12VisibilityCadence {
    if matches!(trigger, Step12VisibilityTrigger::ScheduledStep12)
        && frame % FULL_REFRESH_PERIOD != FULL_REFRESH_PHASE
    {
        Step12VisibilityCadence::NotScheduled
    } else if fog_option == 3 {
        Step12VisibilityCadence::SuppressedByFogOptionThree
    } else {
        Step12VisibilityCadence::FullRefresh
    }
}

/// Freeze the signed `idiv 100; cmp edx,33` gate in `GameDaemon::process_all` and the
/// independent `Game +0x30 == 3` early return at the producer entry.
#[inline]
pub const fn visibility_cadence(frame: i32, fog_option: u8) -> Step12VisibilityCadence {
    visibility_entry(frame, fog_option, Step12VisibilityTrigger::ScheduledStep12)
}

/// `Game +0x821 & 0x10 || Game +0x822 & 0x02`, the gate on scenario reveal-point stamps.
/// Those stamps always call `World::set_seen(..., detect=0)` and therefore never create
/// detector coverage.
#[inline]
pub const fn scenario_reveal_points_enabled(game_821: u8, game_822: u8) -> bool {
    game_821 & 0x10 != 0 || game_822 & 0x02 != 0
}

/// The explored-plane alliance closure at the tail of `update_all_seen` is guarded by
/// `Game::frame +0x550 == 0`. A normally scheduled phase-33 step-12 call can never take it;
/// only a direct call made while the frame is zero can take this tail.
#[inline]
pub const fn frame_zero_explored_sharing(frame: i32) -> bool {
    frame == 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitLosFacts {
    /// Signed `ObjectData::mylos +0x3C`, the accumulator seed.
    pub mylos: i8,
    /// `LeaderData::num_units[0x138]`, the optimized Ptolemy presence check.
    pub ptolemy_count: u16,
    /// `UnitTypeData::role +0x2C8`; either bit in `0x420` admits the Ptolemy arm.
    pub unit_role: u32,
    /// Exact `ObjectData::has_general(0, 0x16A) >= 0` result. It is required only when
    /// `ptolemy_count != 0 && unit_role & 0x420 != 0` reaches that retail call.
    pub has_ptolemy_general: Option<bool>,
    /// `Constants::ptolemy_los_bonus +0xB64`.
    pub ptolemy_los_bonus: i32,
    /// `LeaderData::num_units[0x133]`, the optimized The CEO presence check.
    pub the_ceo_count: u16,
    /// Virtual `UnitTypeData::is_siege()` at ObjectType vtable `+0x10C`. Required only
    /// when `the_ceo_count != 0`.
    pub unit_is_siege: Option<bool>,
    /// Exact `ObjectData::has_general(0, 0x165) >= 0` result. Required only for the
    /// non-siege CEO branch.
    pub has_the_ceo_general: Option<bool>,
    /// `Constants::theceo_unit_los +0xCA8`.
    pub the_ceo_unit_los: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitLosFault {
    MissingPtolemyGeneral,
    MissingTheCeoSiegeClass,
    MissingTheCeoGeneral,
}

/// Exact state-bearing arithmetic of `UnitData::los` `0x006100C0`.
///
/// The two `num_units` reads are short-circuit guards. Their corresponding virtual/general
/// facts are optional in the input but become mandatory when retail reaches them. Additions
/// use x86 wrapping semantics; callers pass the result to [`plan_unit_stamp`], which refuses
/// negative or corrupt wrapped radii before any plane clear.
pub fn resolve_unit_los(facts: UnitLosFacts) -> Result<i32, UnitLosFault> {
    let mut los = i32::from(facts.mylos);
    if facts.ptolemy_count != 0 && facts.unit_role & PTOLEMY_ROLE_MASK != 0 {
        let has_general = facts
            .has_ptolemy_general
            .ok_or(UnitLosFault::MissingPtolemyGeneral)?;
        if has_general {
            los = los.wrapping_add(facts.ptolemy_los_bonus);
        }
    }
    if facts.the_ceo_count != 0 {
        let is_siege = facts
            .unit_is_siege
            .ok_or(UnitLosFault::MissingTheCeoSiegeClass)?;
        if !is_siege {
            let has_general = facts
                .has_the_ceo_general
                .ok_or(UnitLosFault::MissingTheCeoGeneral)?;
            if has_general {
                los = los.wrapping_add(facts.the_ceo_unit_los);
            }
        }
    }
    Ok(los)
}

/// Exact detector-bit suffix of `Object::init` `0x006477FC..0x00647813`.
///
/// `SubObject::init` has already replaced the complete flags byte with `OBJECT_VALID`.
/// `Object::init` then invokes virtual `has_objmask(OBJMASK_DETECT)` and ORs
/// `OBJECT_DETECTOR` when it succeeds. Step 12 reads this *instance byte*; it does not
/// consult `ObjectTypeData::obj_masks` again.
#[inline]
pub const fn object_init_flags(object_type_masks: u32) -> u8 {
    OBJECT_VALID
        | if object_type_masks & OBJMASK_DETECT != 0 {
            OBJECT_DETECTOR
        } else {
            0
        }
}

/// Exact `UnitData::is_on_map` leaf: the high bit of signed `inside_up +0x82` is set.
#[inline]
pub const fn unit_is_on_map(inside_up: i16) -> bool {
    (inside_up as u16) >> 15 != 0
}

/// Exact `UnitData::is_valid_unit` leaf for a canonical Unit vtable.
#[inline]
pub const fn unit_is_valid(object_flags: u8) -> bool {
    object_flags & OBJECT_VALID != 0
}

/// Retail floor division supplied by `div_3_table`, including negative off-map values.
#[inline]
pub const fn div3_floor(value: i32) -> i32 {
    let quotient = value / 3;
    let remainder = value % 3;
    if value < 0 && remainder != 0 {
        quotient - 1
    } else {
        quotient
    }
}

/// Deobfuscated fine coordinate to the exact FCoord sampled by `UnitData::is_seen` and
/// `UnitData::is_detected`.
#[inline]
pub const fn fine_to_fog(fine: i32) -> i32 {
    div3_floor(fine >> 7)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step12UnitFacts {
    /// `LeaderData::leader_flags & 1` for this owner slot.
    pub leader_active: bool,
    /// The authoritative instance `SubObjectData::flags +0x08` byte.
    pub object_flags: u8,
    /// `UnitData::inside_up +0x82`; negative means on-map.
    pub inside_up: i16,
    /// `SubObjectData::who +0x09`.
    pub who: u8,
    /// `SubObjectData::o +0x0A`, retained for registry parity/error reporting.
    pub object_o: i16,
    /// Deobfuscated `x_internal +0x10` / `y_internal +0x14`, in fine units.
    pub fine_x: i32,
    pub fine_y: i32,
    /// Result of virtual `UnitData::los()` `0x006100C0`, after leader/general/type bonuses.
    /// The stored `ObjectData::mylos +0x3C` byte alone is not authoritative here.
    pub resolved_los_tiles: i32,
    /// `ObjectTypeData::domain +0x218`. Domain zero is the small-LOS land-unit arm.
    pub unit_domain: i32,
    /// `UnitTypeData::unit_flags2 +0x2B8`; bit 4 forces the ordinary object centre.
    pub type_unit_flags2: u32,
    /// Instance `UnitData::unit_masks +0x68`; bit 1 forces the ordinary object centre.
    pub unit_masks: u32,
    /// Exact `project(x, y, angle, 0x180)` output, where `angle` is the live signed dword
    /// `UnitData::angle +0x50`. Required only when the resolved fog radius is at most three,
    /// domain is zero, and neither standard-centre mask is set.
    /// Keeping this reached fact explicit avoids substituting floating-point trigonometry for
    /// retail `sin_table` `0x00A46A00`.
    pub projected_small_los_center: Option<(i32, i32)>,
    /// `ObjectData::infiltrated +0x3A`, used as the extra explored-only recipient by
    /// `Object::update_seen`; zero means no extra recipient.
    pub grant_seen2_to: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitStampSkip {
    InactiveLeader,
    InvalidUnit,
    ContainedOrLaunched,
    ZeroLos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitStampFault {
    InvalidOwner(u8),
    InvalidObjectSlot(i16),
    NegativeResolvedLos(i32),
    MissingSmallLosProjectedCenter,
    CorruptWrappedRadius { los_tiles: i32, wrapped_radius: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitStampCenter {
    ObjectPosition,
    ProjectedSmallLandUnit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step12UnitStamp {
    pub who: u8,
    pub object_o: i16,
    pub object_fine_x: i32,
    pub object_fine_y: i32,
    pub stamp_fine_x: i32,
    pub stamp_fine_y: i32,
    pub fog_x: i32,
    pub fog_y: i32,
    /// Exact positive result of `UnitData::los()`.  Retaining it lets the commit call the
    /// already exact full-disc `Object::update_seen(0)` loop without reverse-engineering an
    /// odd LOS value from its truncated fog radius.
    pub resolved_los_tiles: i32,
    pub radius_fog_cells: i32,
    pub center: UnitStampCenter,
    /// Comes only from authoritative `object_flags & OBJECT_DETECTOR` at this pass.
    pub detector: bool,
    /// Call-time `UnitData::unit_masks +0x218`. `World::reveal_fog` tests bit `0x100`
    /// on the source Unit after its rare/oil/item clauses.
    pub unit_masks: u32,
    pub grant_seen2_to: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitStampDecision {
    Skip(UnitStampSkip),
    Stamp(Step12UnitStamp),
}

fn checked_resolved_los_radius(resolved_los_tiles: i32) -> Result<Option<i32>, UnitStampFault> {
    if resolved_los_tiles == 0 {
        return Ok(None);
    }
    if resolved_los_tiles < 0 {
        return Err(UnitStampFault::NegativeResolvedLos(resolved_los_tiles));
    }

    // Retail uses signed `imul` followed by signed `idiv`. Real resolved LOS values are
    // small; retain the wrapping machine operation, but fail before a wrapped negative value
    // could index the retail circle table backwards in a corrupt state.
    let wrapped_product = resolved_los_tiles.wrapping_mul(FINE_PER_TILE);
    let wrapped_radius = wrapped_product / FINE_PER_FOG_CELL;
    if wrapped_product < 0 || wrapped_radius < 0 {
        return Err(UnitStampFault::CorruptWrappedRadius {
            los_tiles: resolved_los_tiles,
            wrapped_radius,
        });
    }
    Ok(Some(if wrapped_radius > MAX_FOG_RADIUS {
        MAX_FOG_RADIUS
    } else {
        wrapped_radius
    }))
}

/// Prepare the exact ordinary Unit-band call to `Object::update_seen(0)`.
///
/// The caller must enumerate the canonical owner-local Unit band in ascending object index,
/// as retail does. All fallible facts are explicit so a Sim integration can preflight every
/// row before `World::clear_seen` destroys the previous checksum-visible planes.
pub fn plan_unit_stamp(facts: Step12UnitFacts) -> Result<UnitStampDecision, UnitStampFault> {
    if usize::from(facts.who) >= LEADER_SLOTS {
        return Err(UnitStampFault::InvalidOwner(facts.who));
    }
    if facts.object_o < 0 {
        return Err(UnitStampFault::InvalidObjectSlot(facts.object_o));
    }
    if !facts.leader_active {
        return Ok(UnitStampDecision::Skip(UnitStampSkip::InactiveLeader));
    }
    if !unit_is_valid(facts.object_flags) {
        return Ok(UnitStampDecision::Skip(UnitStampSkip::InvalidUnit));
    }
    if !unit_is_on_map(facts.inside_up) {
        return Ok(UnitStampDecision::Skip(UnitStampSkip::ContainedOrLaunched));
    }
    let Some(radius_fog_cells) = checked_resolved_los_radius(facts.resolved_los_tiles)? else {
        return Ok(UnitStampDecision::Skip(UnitStampSkip::ZeroLos));
    };
    let projected_small_land_unit = radius_fog_cells <= 3
        && facts.unit_domain == 0
        && facts.type_unit_flags2 & SMALL_LOS_STANDARD_TYPE_FLAGS2 == 0
        && facts.unit_masks & SMALL_LOS_STANDARD_UNIT_MASKS == 0;
    let (stamp_fine_x, stamp_fine_y, center) = if projected_small_land_unit {
        let (x, y) = facts
            .projected_small_los_center
            .ok_or(UnitStampFault::MissingSmallLosProjectedCenter)?;
        (x, y, UnitStampCenter::ProjectedSmallLandUnit)
    } else {
        (facts.fine_x, facts.fine_y, UnitStampCenter::ObjectPosition)
    };

    Ok(UnitStampDecision::Stamp(Step12UnitStamp {
        who: facts.who,
        object_o: facts.object_o,
        object_fine_x: facts.fine_x,
        object_fine_y: facts.fine_y,
        stamp_fine_x,
        stamp_fine_y,
        fog_x: fine_to_fog(stamp_fine_x),
        fog_y: fine_to_fog(stamp_fine_y),
        resolved_los_tiles: facts.resolved_los_tiles,
        radius_fog_cells,
        center,
        detector: facts.object_flags & OBJECT_DETECTOR != 0,
        unit_masks: facts.unit_masks,
        grant_seen2_to: facts.grant_seen2_to,
    }))
}

// ---------------------------------------------------------------------------
// Live integration receipt boundary
// ---------------------------------------------------------------------------

/// Stable identity of one row in the owner-local Unit bands traversed by step 12.
///
/// `row` is the port's dense-row identity. The other fields are retail state and prevent a
/// receipt from following a recycled row or a compacted object into a later refresh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveStep12UnitIdentity {
    pub row: usize,
    pub who: u8,
    pub object_o: i16,
    pub uid: u16,
    pub type_index: i32,
}

/// The complete subset already owned by the live Sim Unit columns and object registry.
///
/// This intentionally contains no type-derived detector/LOS answer. In particular, a clear
/// `OBJECT_DETECTOR` bit is only an observed byte until [`DetectorInstanceProvenance`] proves
/// why that byte is authoritative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveStep12UnitState {
    pub identity: LiveStep12UnitIdentity,
    pub object_flags: u8,
    pub inside_up: i16,
    pub fine_x: i32,
    pub fine_y: i32,
    pub unit_angle: i32,
    pub mylos: i8,
    /// `ObjectData::visible +0x40`.  A nonzero byte makes full `Object::update_seen(0)`
    /// call virtual `Unit::update_local_seen` after the LOS calculation.
    pub visible: i8,
    pub unit_masks: u32,
    pub infiltrated: i8,
}

/// One coherent view of all eight retail owner-local Unit bands.
///
/// The caller must obtain `state_revision` from the owner which increments whenever any field,
/// registry membership, leader admission, or general-radius dependency used here mutates.
/// `owner_band_lengths` binds the flattened rows to the complete traversal of each *active*
/// owner's Unit band. Retail does not read an inactive owner's band, so its declared length must
/// be zero even if the registry retains dormant objects there.
#[derive(Clone, Copy, Debug)]
pub struct LiveStep12UnitBandSnapshot<'a> {
    pub frame: i32,
    pub state_revision: u64,
    pub type_revision: u64,
    pub leader_active: [bool; LEADER_SLOTS],
    pub owner_band_lengths: [usize; LEADER_SLOTS],
    pub rows: &'a [LiveStep12UnitState],
}

/// Why the current instance detector bit is known rather than inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectorInstanceProvenance {
    /// The allocation retained the masks read by `Object::init`. Only the detector bit is
    /// compared: unrelated instance flag bits may legitimately change after initialization.
    ObjectInit { object_masks_at_init: u32 },
    /// Save/load or an explicit later mutation supplied the complete instance byte at the same
    /// revision as the dynamic authority receipt.
    AuthoritativeInstance {
        instance_flags: u8,
        state_revision: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectorProvenanceFault {
    ObjectInitDetectorMismatch {
        object_masks_at_init: u32,
        current_flags: u8,
    },
    InstanceRevisionMismatch {
        receipt_revision: u64,
        instance_revision: u64,
    },
    InstanceFlagsMismatch {
        current_flags: u8,
        authoritative_flags: u8,
    },
}

fn validate_detector_provenance(
    provenance: DetectorInstanceProvenance,
    current_flags: u8,
    receipt_revision: u64,
) -> Result<(), DetectorProvenanceFault> {
    match provenance {
        DetectorInstanceProvenance::ObjectInit {
            object_masks_at_init,
        } => {
            let expected = object_init_flags(object_masks_at_init) & OBJECT_DETECTOR;
            if current_flags & OBJECT_DETECTOR != expected {
                return Err(DetectorProvenanceFault::ObjectInitDetectorMismatch {
                    object_masks_at_init,
                    current_flags,
                });
            }
        }
        DetectorInstanceProvenance::AuthoritativeInstance {
            instance_flags,
            state_revision,
        } => {
            if state_revision != receipt_revision {
                return Err(DetectorProvenanceFault::InstanceRevisionMismatch {
                    receipt_revision,
                    instance_revision: state_revision,
                });
            }
            if instance_flags != current_flags {
                return Err(DetectorProvenanceFault::InstanceFlagsMismatch {
                    current_flags,
                    authoritative_flags: instance_flags,
                });
            }
        }
    }
    Ok(())
}

/// Exact reached call and result of `project(x,y,UnitData::angle,0x180)`.
///
/// The result is still supplied by the eventual projection owner; this receipt prevents a
/// cached output for another position, facing, or distance from being accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmallLosProjectionReceipt {
    pub source_fine_x: i32,
    pub source_fine_y: i32,
    pub unit_angle: i32,
    pub distance: i32,
    pub projected_fine_x: i32,
    pub projected_fine_y: i32,
}

/// Dynamic/type authority for a Unit row which reaches `Object::update_seen(0)`.
///
/// The identity duplicates the live snapshot deliberately: equality is the anti-staleness
/// boundary. `los_facts` owns all reached Ptolemy/CEO count, role, siege, general-radius, and
/// Constants answers; an absent optional answer is accepted only when retail short-circuits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step12UnitAuthorityReceipt {
    pub identity: LiveStep12UnitIdentity,
    /// Required only after a positive LOS has produced a disc stamp.
    pub detector: Option<DetectorInstanceProvenance>,
    pub los_facts: UnitLosFacts,
    /// Required only for a positive resolved radius of at most three fog cells.
    pub unit_domain: Option<i32>,
    /// Required only after the small-radius arm observes domain zero.
    pub type_unit_flags2: Option<u32>,
    pub small_los_projection: Option<SmallLosProjectionReceipt>,
}

/// Frame/revision/cardinality-bound authority facts aligned with the flattened Unit bands.
/// Skipped rows carry `None`; admitted rows must carry `Some`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step12VisibilityAuthorityReceipt {
    pub frame: i32,
    pub state_revision: u64,
    pub type_revision: u64,
    pub leader_active: [bool; LEADER_SLOTS],
    pub owner_band_lengths: [usize; LEADER_SLOTS],
    pub rows: Vec<Option<Step12UnitAuthorityReceipt>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmallLosProjectionFault {
    OriginMismatch {
        expected_x: i32,
        expected_y: i32,
        receipt_x: i32,
        receipt_y: i32,
    },
    AngleMismatch {
        expected: i32,
        receipt: i32,
    },
    DistanceMismatch {
        expected: i32,
        receipt: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveStep12PrepareFault {
    UnitBandCardinality {
        declared: usize,
        rows: usize,
    },
    InvalidOwner {
        row: usize,
        who: u8,
    },
    InvalidObjectSlot {
        row: usize,
        object_o: i16,
    },
    InactiveLeaderBandEnumerated {
        who: u8,
        rows: usize,
    },
    OwnerBandCardinality {
        who: u8,
        declared: usize,
        observed: usize,
    },
    OutOfRetailOrder {
        previous: LiveStep12UnitIdentity,
        current: LiveStep12UnitIdentity,
    },
    MissingAuthorityReceipt,
    AuthorityFrameMismatch {
        snapshot: i32,
        receipt: i32,
    },
    AuthorityStateRevisionMismatch {
        snapshot: u64,
        receipt: u64,
    },
    AuthorityTypeRevisionMismatch {
        snapshot: u64,
        receipt: u64,
    },
    AuthorityBandLengthsMismatch,
    AuthorityLeaderActivityMismatch,
    AuthorityRowCardinality {
        snapshot: usize,
        receipt: usize,
    },
    MissingRowAuthority {
        row: usize,
    },
    AuthorityIdentityMismatch {
        row: usize,
        snapshot: LiveStep12UnitIdentity,
        receipt: LiveStep12UnitIdentity,
    },
    AuthorityMylosMismatch {
        row: usize,
        snapshot: i8,
        receipt: i8,
    },
    DetectorProvenance {
        row: usize,
        fault: DetectorProvenanceFault,
    },
    Los {
        row: usize,
        fault: UnitLosFault,
    },
    MissingSmallLosDomain {
        row: usize,
    },
    MissingSmallLosTypeFlags2 {
        row: usize,
    },
    MissingDetectorProvenance {
        row: usize,
    },
    Projection {
        row: usize,
        fault: SmallLosProjectionFault,
    },
    Stamp {
        row: usize,
        fault: UnitStampFault,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedStep12UnitRow {
    identity: LiveStep12UnitIdentity,
    visible: i8,
    decision: UnitStampDecision,
}

impl PreparedStep12UnitRow {
    pub const fn identity(&self) -> LiveStep12UnitIdentity {
        self.identity
    }

    pub const fn visible(&self) -> i8 {
        self.visible
    }

    pub const fn decision(&self) -> UnitStampDecision {
        self.decision
    }
}

/// Opaque, preflight-complete Unit subpass. It is deliberately not a complete refresh token:
/// Build/Wall stamps, reveal-fog effects, and the other producer tails must join before any
/// caller clears checksum-visible planes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedStep12UnitPass {
    frame: i32,
    state_revision: u64,
    type_revision: u64,
    authority_bound: bool,
    rows: Vec<PreparedStep12UnitRow>,
    stamps: usize,
}

impl PreparedStep12UnitPass {
    pub const fn frame(&self) -> i32 {
        self.frame
    }

    pub const fn state_revision(&self) -> u64 {
        self.state_revision
    }

    pub const fn type_revision(&self) -> u64 {
        self.type_revision
    }

    pub const fn authority_bound(&self) -> bool {
        self.authority_bound
    }

    pub fn rows(&self) -> &[PreparedStep12UnitRow] {
        &self.rows
    }

    pub const fn stamps(&self) -> usize {
        self.stamps
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveStep12Preparation {
    /// Retail reaches no visibility mutation: either the scheduler did not call the producer,
    /// or fog option three returned at its entry. No authority receipt is read.
    NoMutation(Step12VisibilityCadence),
    /// The Unit-band subpass is fully preflighted. This is not permission to clear planes.
    UnitPass(PreparedStep12UnitPass),
}

fn validate_live_band(
    snapshot: LiveStep12UnitBandSnapshot<'_>,
) -> Result<(), LiveStep12PrepareFault> {
    let Some(declared) = snapshot
        .owner_band_lengths
        .iter()
        .try_fold(0usize, |sum, &length| sum.checked_add(length))
    else {
        return Err(LiveStep12PrepareFault::UnitBandCardinality {
            declared: usize::MAX,
            rows: snapshot.rows.len(),
        });
    };
    if declared != snapshot.rows.len() {
        return Err(LiveStep12PrepareFault::UnitBandCardinality {
            declared,
            rows: snapshot.rows.len(),
        });
    }

    let mut observed = [0usize; LEADER_SLOTS];
    let mut previous: Option<LiveStep12UnitIdentity> = None;
    for state in snapshot.rows {
        let identity = state.identity;
        let who = usize::from(identity.who);
        if who >= LEADER_SLOTS {
            return Err(LiveStep12PrepareFault::InvalidOwner {
                row: identity.row,
                who: identity.who,
            });
        }
        if identity.object_o < 0 {
            return Err(LiveStep12PrepareFault::InvalidObjectSlot {
                row: identity.row,
                object_o: identity.object_o,
            });
        }
        if !snapshot.leader_active[who] {
            return Err(LiveStep12PrepareFault::InactiveLeaderBandEnumerated {
                who: identity.who,
                rows: snapshot.owner_band_lengths[who],
            });
        }
        if let Some(prior) = previous {
            if identity.who < prior.who
                || (identity.who == prior.who && identity.object_o <= prior.object_o)
            {
                return Err(LiveStep12PrepareFault::OutOfRetailOrder {
                    previous: prior,
                    current: identity,
                });
            }
        }
        previous = Some(identity);
        observed[who] += 1;
    }
    for who in 0..LEADER_SLOTS {
        if !snapshot.leader_active[who] && snapshot.owner_band_lengths[who] != 0 {
            return Err(LiveStep12PrepareFault::InactiveLeaderBandEnumerated {
                who: who as u8,
                rows: snapshot.owner_band_lengths[who],
            });
        }
        if observed[who] != snapshot.owner_band_lengths[who] {
            return Err(LiveStep12PrepareFault::OwnerBandCardinality {
                who: who as u8,
                declared: snapshot.owner_band_lengths[who],
                observed: observed[who],
            });
        }
    }
    Ok(())
}

fn validate_projection_receipt(
    live: LiveStep12UnitState,
    receipt: SmallLosProjectionReceipt,
) -> Result<(), SmallLosProjectionFault> {
    if (receipt.source_fine_x, receipt.source_fine_y) != (live.fine_x, live.fine_y) {
        return Err(SmallLosProjectionFault::OriginMismatch {
            expected_x: live.fine_x,
            expected_y: live.fine_y,
            receipt_x: receipt.source_fine_x,
            receipt_y: receipt.source_fine_y,
        });
    }
    if receipt.unit_angle != live.unit_angle {
        return Err(SmallLosProjectionFault::AngleMismatch {
            expected: live.unit_angle,
            receipt: receipt.unit_angle,
        });
    }
    if receipt.distance != SMALL_LOS_PROJECT_DISTANCE {
        return Err(SmallLosProjectionFault::DistanceMismatch {
            expected: SMALL_LOS_PROJECT_DISTANCE,
            receipt: receipt.distance,
        });
    }
    Ok(())
}

/// Preflight the maximal live Unit-band subset without mutating a fog plane.
///
/// Retail's lazy gates are retained: an unscheduled/suppressed producer reads no snapshot, and
/// an inactive, invalid, or contained Unit row needs no authority record. Every row which reaches
/// `Object::update_seen(0)` requires exact dynamic/type authority. The output intentionally has no
/// commit method; installing one before the Build/Wall and reveal-fog owners exist would turn an
/// exact Unit subpass into an inexact complete refresh.
pub fn prepare_live_unit_pass(
    fog_option: u8,
    trigger: Step12VisibilityTrigger,
    snapshot: LiveStep12UnitBandSnapshot<'_>,
    authority: Option<&Step12VisibilityAuthorityReceipt>,
) -> Result<LiveStep12Preparation, LiveStep12PrepareFault> {
    let cadence = visibility_entry(snapshot.frame, fog_option, trigger);
    if cadence != Step12VisibilityCadence::FullRefresh {
        return Ok(LiveStep12Preparation::NoMutation(cadence));
    }
    validate_live_band(snapshot)?;

    let authority_required = snapshot
        .rows
        .iter()
        .any(|live| unit_is_valid(live.object_flags) && unit_is_on_map(live.inside_up));
    let receipt = if authority_required {
        let receipt = authority.ok_or(LiveStep12PrepareFault::MissingAuthorityReceipt)?;
        if receipt.frame != snapshot.frame {
            return Err(LiveStep12PrepareFault::AuthorityFrameMismatch {
                snapshot: snapshot.frame,
                receipt: receipt.frame,
            });
        }
        if receipt.state_revision != snapshot.state_revision {
            return Err(LiveStep12PrepareFault::AuthorityStateRevisionMismatch {
                snapshot: snapshot.state_revision,
                receipt: receipt.state_revision,
            });
        }
        if receipt.type_revision != snapshot.type_revision {
            return Err(LiveStep12PrepareFault::AuthorityTypeRevisionMismatch {
                snapshot: snapshot.type_revision,
                receipt: receipt.type_revision,
            });
        }
        if receipt.owner_band_lengths != snapshot.owner_band_lengths {
            return Err(LiveStep12PrepareFault::AuthorityBandLengthsMismatch);
        }
        if receipt.leader_active != snapshot.leader_active {
            return Err(LiveStep12PrepareFault::AuthorityLeaderActivityMismatch);
        }
        if receipt.rows.len() != snapshot.rows.len() {
            return Err(LiveStep12PrepareFault::AuthorityRowCardinality {
                snapshot: snapshot.rows.len(),
                receipt: receipt.rows.len(),
            });
        }
        Some(receipt)
    } else {
        None
    };

    let mut rows = Vec::with_capacity(snapshot.rows.len());
    let mut stamps = 0usize;
    for (position, &live) in snapshot.rows.iter().enumerate() {
        let early = if !unit_is_valid(live.object_flags) {
            Some(UnitStampSkip::InvalidUnit)
        } else if !unit_is_on_map(live.inside_up) {
            Some(UnitStampSkip::ContainedOrLaunched)
        } else {
            None
        };
        if let Some(skip) = early {
            rows.push(PreparedStep12UnitRow {
                identity: live.identity,
                visible: live.visible,
                decision: UnitStampDecision::Skip(skip),
            });
            continue;
        }

        let authority = receipt.and_then(|receipt| receipt.rows[position]).ok_or(
            LiveStep12PrepareFault::MissingRowAuthority {
                row: live.identity.row,
            },
        )?;
        if authority.identity != live.identity {
            return Err(LiveStep12PrepareFault::AuthorityIdentityMismatch {
                row: live.identity.row,
                snapshot: live.identity,
                receipt: authority.identity,
            });
        }
        if authority.los_facts.mylos != live.mylos {
            return Err(LiveStep12PrepareFault::AuthorityMylosMismatch {
                row: live.identity.row,
                snapshot: live.mylos,
                receipt: authority.los_facts.mylos,
            });
        }
        let resolved_los_tiles =
            resolve_unit_los(authority.los_facts).map_err(|fault| LiveStep12PrepareFault::Los {
                row: live.identity.row,
                fault,
            })?;
        let resolved_radius = checked_resolved_los_radius(resolved_los_tiles).map_err(|fault| {
            LiveStep12PrepareFault::Stamp {
                row: live.identity.row,
                fault,
            }
        })?;
        let (unit_domain, type_unit_flags2) = match resolved_radius {
            Some(radius) if radius <= 3 => {
                let domain =
                    authority
                        .unit_domain
                        .ok_or(LiveStep12PrepareFault::MissingSmallLosDomain {
                            row: live.identity.row,
                        })?;
                let flags2 = if domain == 0 {
                    authority.type_unit_flags2.ok_or(
                        LiveStep12PrepareFault::MissingSmallLosTypeFlags2 {
                            row: live.identity.row,
                        },
                    )?
                } else {
                    0
                };
                (domain, flags2)
            }
            _ => (0, 0),
        };
        let projected_small_los_center = authority
            .small_los_projection
            .map(|projection| (projection.projected_fine_x, projection.projected_fine_y));
        let decision = plan_unit_stamp(Step12UnitFacts {
            leader_active: true,
            object_flags: live.object_flags,
            inside_up: live.inside_up,
            who: live.identity.who,
            object_o: live.identity.object_o,
            fine_x: live.fine_x,
            fine_y: live.fine_y,
            resolved_los_tiles,
            unit_domain,
            type_unit_flags2,
            unit_masks: live.unit_masks,
            projected_small_los_center,
            grant_seen2_to: live.infiltrated,
        })
        .map_err(|fault| LiveStep12PrepareFault::Stamp {
            row: live.identity.row,
            fault,
        })?;

        if matches!(
            decision,
            UnitStampDecision::Stamp(Step12UnitStamp {
                center: UnitStampCenter::ProjectedSmallLandUnit,
                ..
            })
        ) {
            let projection =
                authority
                    .small_los_projection
                    .ok_or(LiveStep12PrepareFault::Stamp {
                        row: live.identity.row,
                        fault: UnitStampFault::MissingSmallLosProjectedCenter,
                    })?;
            validate_projection_receipt(live, projection).map_err(|fault| {
                LiveStep12PrepareFault::Projection {
                    row: live.identity.row,
                    fault,
                }
            })?;
        }
        if matches!(decision, UnitStampDecision::Stamp(_)) {
            let detector =
                authority
                    .detector
                    .ok_or(LiveStep12PrepareFault::MissingDetectorProvenance {
                        row: live.identity.row,
                    })?;
            validate_detector_provenance(detector, live.object_flags, snapshot.state_revision)
                .map_err(|fault| LiveStep12PrepareFault::DetectorProvenance {
                    row: live.identity.row,
                    fault,
                })?;
        }
        if matches!(decision, UnitStampDecision::Stamp(_)) {
            stamps += 1;
        }
        rows.push(PreparedStep12UnitRow {
            identity: live.identity,
            visible: live.visible,
            decision,
        });
    }

    Ok(LiveStep12Preparation::UnitPass(PreparedStep12UnitPass {
        frame: snapshot.frame,
        state_revision: snapshot.state_revision,
        type_revision: snapshot.type_revision,
        authority_bound: authority_required,
        rows,
        stamps,
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityCellSample {
    pub fog_x: i32,
    pub fog_y: i32,
    pub index: usize,
    pub cell_seen_mask: u8,
    pub cell_detected_mask: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityCellSampleFault {
    InvalidDimensions {
        width: i32,
        height: i32,
    },
    PlaneCardinality {
        expected: usize,
        seen: usize,
        seen3: usize,
    },
    OffMap {
        fog_x: i32,
        fog_y: i32,
    },
}

/// Sample the co-registered current-visibility and detector planes for an external Unit row.
///
/// This is the handoff to `external_entity_visibility_frontier`: call it only after the
/// producer and all later same-frame incremental visibility mutations have committed. The
/// returned bytes map directly to `RetailUnitVisibilityFacts::{cell_seen_mask,
/// cell_detected_mask}`; neither byte may be synthesized from type masks or viewer cheats.
pub fn sample_visibility_cell(
    fine_x: i32,
    fine_y: i32,
    fog_width: i32,
    fog_height: i32,
    seen: &[u8],
    seen3: &[u8],
) -> Result<VisibilityCellSample, VisibilityCellSampleFault> {
    let Ok(width) = usize::try_from(fog_width) else {
        return Err(VisibilityCellSampleFault::InvalidDimensions {
            width: fog_width,
            height: fog_height,
        });
    };
    let Ok(height) = usize::try_from(fog_height) else {
        return Err(VisibilityCellSampleFault::InvalidDimensions {
            width: fog_width,
            height: fog_height,
        });
    };
    let Some(expected) = width.checked_mul(height) else {
        return Err(VisibilityCellSampleFault::InvalidDimensions {
            width: fog_width,
            height: fog_height,
        });
    };
    if width == 0 || height == 0 {
        return Err(VisibilityCellSampleFault::InvalidDimensions {
            width: fog_width,
            height: fog_height,
        });
    }
    if seen.len() != expected || seen3.len() != expected {
        return Err(VisibilityCellSampleFault::PlaneCardinality {
            expected,
            seen: seen.len(),
            seen3: seen3.len(),
        });
    }

    let fog_x = fine_to_fog(fine_x);
    let fog_y = fine_to_fog(fine_y);
    let (Ok(x), Ok(y)) = (usize::try_from(fog_x), usize::try_from(fog_y)) else {
        return Err(VisibilityCellSampleFault::OffMap { fog_x, fog_y });
    };
    if x >= width || y >= height {
        return Err(VisibilityCellSampleFault::OffMap { fog_x, fog_y });
    }
    let index = y * width + x;
    Ok(VisibilityCellSample {
        fog_x,
        fog_y,
        index,
        cell_seen_mask: seen[index],
        cell_detected_mask: seen3[index],
    })
}
