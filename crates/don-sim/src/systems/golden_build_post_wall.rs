// SPDX-License-Identifier: GPL-3.0-or-later
//! Detached golden-2024 `Build::process` planner after `Wall::process` returns.
//!
//! This module is intentionally not a tick adapter.  It classifies the two owner-zero Build
//! calls at frames `2..=31` in retail order (Village `o=2000`, then Dutch Market `o=2001`),
//! and transcribes the exact common prefix beginning at `0x0061EE16`.  Any reached dynamic
//! child is returned as the first boundary; the input [`BuildData`] is never mutated.
//!
//! The closed common path includes the optional healing countdown, empty
//! `Build::do_queue(0)` return, and `BuildTypeData::is_gather_type` predicate.  From
//! `0x0061F451`, the Village skips the `538/438/436/529` type cones.  Its 256-frame slot and
//! already-owned linked-City maintenance window are both unscheduled throughout frames
//! `2..=31`, so that bounded suffix is an exact no-op.  The Market instead reaches
//! `LeaderData::has_tribe_bonus(23)` at `0x0061F5E4`; this planner stops before that call.
//! The leaf itself is already source-complete in
//! [`leader_tribe_bonus_runtime`](crate::systems::leader_tribe_bonus_runtime); joining its
//! live Leader/Tribe inputs at this call site remains separate composition work.
//! Because the Village precedes the Market, its suffix beginning at `0x0061FB53` is the
//! earliest global Build-band continuation on every Wall-returning frame. The detached
//! [`plan_village_target_gate`] transaction now binds its linked `CityData::city_flags`,
//! `GameInfo::team_style`, fixed active-Leader traversal, and complete
//! `LeaderData::get_target()` leaf at `0x0061FB9B -> 0x006DA000`. It stops before retail's
//! first effectful child, `Wall::update_local_seen` at `0x0063ED50`, and reports the
//! preceding `WallData::ever_seen` write without applying it. The Market continuation is
//! actor-local chronology, not a claim that execution reached it.
//!
//! The caller must prove that the immediately preceding `Wall::process` call returned.
//! [`golden_build_schedule`] reports the remaining post-`check_ever_seen` Wall territory
//! boundaries and prevents the detached planner from being mistaken for execution coverage.
//! It must also bind `BuildData` and [`BuildPostWallTypeFacts`] to one immediate pre-call Sim
//! snapshot; this module authenticates neither source and therefore supplies no frame-2 bytes.

#![forbid(unsafe_code)]

use crate::systems::leader_get_target_runtime::{
    self, LeaderTargetContext, LeaderTargetError, LeaderTargetReceipt,
};
use crate::systems::production::{self, BuildData};
use crate::systems::tech_cities;

pub const BUILD_PROCESS_VA: u32 = 0x0061_edf0;
pub const BUILD_POST_WALL_ENTRY_VA: u32 = 0x0061_ee16;
pub const BUILD_HEALING_STORE_VA: u32 = 0x0061_ee41;
pub const BUILD_PROCESS_EJECTION_VA: u32 = 0x0062_01e0;
pub const OBJECT_DO_LAUNCH_CALL_VA: u32 = 0x0061_ee5f;
pub const OBJECT_DO_LAUNCH_VA: u32 = 0x0064_f3b0;
pub const BUILD_DO_ATTACK_VA: u32 = 0x0062_28f0;
pub const OBJECT_IS_IN_RANGE_VA: u32 = 0x0064_8d70;
pub const BUILD_DO_QUEUE_CALL_VA: u32 = 0x0061_f3bf;
pub const BUILD_DO_QUEUE_VA: u32 = 0x0061_e410;
pub const BUILD_IS_GATHER_TYPE_CALL_VA: u32 = 0x0061_f3ca;
pub const BUILD_IS_GATHER_TYPE_VA: u32 = 0x0047_2bb0;
pub const BUILD_TYPE_TAIL_ENTRY_VA: u32 = 0x0061_f451;

pub const MARKET_PERSIAN_BONUS_CALL_VA: u32 = 0x0061_f5e4;
pub const LEADER_HAS_TRIBE_BONUS_VA: u32 = 0x006e_1370;
pub const PERSIAN_TRIBE_INDEX: i32 = 23;

pub const VILLAGE_PHASE_256_FIRST_VA: u32 = 0x0061_f73e;
pub const VILLAGE_PHASE_256_LAST_VA: u32 = 0x0061_f75d;
pub const BUILD_CITY_MAINTENANCE_GATE_VA: u32 = 0x0061_faca;
pub const BUILD_CITY_MAINTENANCE_END_VA: u32 = 0x0061_fb53;
pub const VILLAGE_GET_TARGET_CALL_VA: u32 = 0x0061_fb9b;
pub const LEADER_GET_TARGET_VA: u32 = 0x006d_a000;
pub const VILLAGE_EVER_SEEN_WRITE_VA: u32 = 0x0061_fbcd;
pub const VILLAGE_UPDATE_LOCAL_SEEN_CALL_VA: u32 = 0x0061_fbd2;
pub const WALL_UPDATE_LOCAL_SEEN_VA: u32 = 0x0063_ed50;
pub const VILLAGE_TARGET_GATE_JOIN_VA: u32 = 0x0061_fbd8;

pub const GOLDEN_FIRST_FRAME: i32 = 2;
pub const GOLDEN_LAST_FRAME: i32 = 31;
pub const GOLDEN_OWNER: u8 = 0;
pub const GOLDEN_VILLAGE_O: i16 = 2_000;
pub const GOLDEN_MARKET_O: i16 = 2_001;
pub const GOLDEN_VILLAGE_TYPE: i32 = tech_cities::ty::VILLAGE;
pub const GOLDEN_MARKET_TYPE: i32 = tech_cities::ty::MARKET;
pub const GOLDEN_BUILD_STEP: u8 = 14;

pub const VILLAGE_SKIPPED_TYPE_CONES: [i32; 4] = [538, 438, 436, 529];
pub const CITY_MAINTENANCE_PERIOD: i32 = 200;
pub const CITY_CAPITAL_FLAG: u16 = 0x0010;
pub const ASSASSIN_TEAM_STYLE: u8 = 2;

const BUILD_LAUNCH_MASK: u16 = 0x0008;
const BUILD_GATHER_TYPE_MASK: u32 = 0x0040;
const BUILD_IN_CITY_FLAG: u8 = 0x20;
const NEAR_O_OFFSET: usize = 0x34;
const NEAR_WHO_OFFSET: usize = 0x36;
const HEALING_OFFSET: usize = 0x38;

/// The fixed owner-zero Build-band order in the selected replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenBuildActor {
    Village,
    Market,
}

impl GoldenBuildActor {
    pub const fn object_id(self) -> i16 {
        match self {
            Self::Village => GOLDEN_VILLAGE_O,
            Self::Market => GOLDEN_MARKET_O,
        }
    }

    pub const fn type_index(self) -> i32 {
        match self {
            Self::Village => GOLDEN_VILLAGE_TYPE,
            Self::Market => GOLDEN_MARKET_TYPE,
        }
    }

    pub const fn build_ordinal(self) -> u8 {
        match self {
            Self::Village => 0,
            Self::Market => 1,
        }
    }
}

/// The earliest still-unowned Wall outcome after the periodic `check_ever_seen(0)` child
/// has returned.  `Returned` is the only state admitted by [`plan_after_wall_return`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenWallContinuation {
    Returned,
    TerritorySlot,
}

/// One actor's place in the frame-2..31 Build chronology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenBuildSchedule {
    pub frame: i32,
    pub step: u8,
    pub owner: u8,
    pub actor: GoldenBuildActor,
    pub object_id: i16,
    pub type_index: i32,
    pub build_ordinal: u8,
    /// Owner zero's eight-frame visibility cadence.  The schedule assumes its exact child
    /// has returned before reporting [`wall_continuation`](Self::wall_continuation).
    pub periodic_check_ever_seen_due: bool,
    pub slow_slot_32_due: bool,
    pub territory_slot_16_due: bool,
    pub wall_continuation: GoldenWallContinuation,
}

/// Return the two Build calls in retail order for one supported golden frame.
///
/// This is schedule order only.  A boundary in the Village must return before the Market row
/// becomes executable in that frame.
pub fn golden_build_schedule(frame: i32) -> Option<[GoldenBuildSchedule; 2]> {
    if !(GOLDEN_FIRST_FRAME..=GOLDEN_LAST_FRAME).contains(&frame) {
        return None;
    }
    let make = |actor: GoldenBuildActor| {
        let phase = frame.wrapping_add(i32::from(actor.object_id()));
        let periodic_check_ever_seen_due = frame != 0 && (frame as u32 & 7) == 0;
        let slow_slot_32_due = phase.rem_euclid(32) == 0;
        let territory_slot_16_due = phase.rem_euclid(16) == 0;
        GoldenBuildSchedule {
            frame,
            step: GOLDEN_BUILD_STEP,
            owner: GOLDEN_OWNER,
            actor,
            object_id: actor.object_id(),
            type_index: actor.type_index(),
            build_ordinal: actor.build_ordinal(),
            periodic_check_ever_seen_due,
            slow_slot_32_due,
            territory_slot_16_due,
            wall_continuation: if territory_slot_16_due {
                GoldenWallContinuation::TerritorySlot
            } else {
                GoldenWallContinuation::Returned
            },
        }
    };
    Some([GoldenBuildActor::Village, GoldenBuildActor::Market].map(make))
}

/// The only BuildType fields consumed by the owned common prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildPostWallTypeFacts {
    pub type_index: i32,
    /// `ObjectTypeData + 0x1E8`.
    pub attack: i32,
    /// `BuildTypeData` flags; bit `0x40` is `is_gather_type`.
    pub build_flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildPostWallWriteField {
    Healing,
}

/// A write retail executes before the first returned boundary.  Planning does not apply it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildPostWallWrite {
    pub instruction_va: u32,
    pub field: BuildPostWallWriteField,
    pub before: u16,
    pub after: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackChildReason {
    CachedAttackCoordinates,
    Phase32Refresh,
}

/// Exact first continuation after the post-Wall entry.  Callable children carry their
/// recovered callee VA; state-dependent cones without a source-complete callee remain named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildPostWallContinuation {
    ProcessEjection {
        callee_va: u32,
    },
    ObjectDoLaunch {
        call_va: u32,
        callee_va: u32,
    },
    DoAttack {
        reason: AttackChildReason,
        callee_va: u32,
    },
    ObjectIsInRange {
        near_o: i16,
        near_who: i16,
        callee_va: u32,
    },
    OwnershipLatch,
    DamageRecovery {
        damage: i32,
        damage_frac: i8,
    },
    NonEmptyDoQueue {
        call_va: u32,
        callee_va: u32,
        queued: u8,
    },
    GatherTypeTail {
        call_va: u32,
        callee_va: u32,
    },
    GoldenTypeTail(GoldenBuildTypeTail),
}

/// The first type-specific continuation on the closed common path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenBuildTypeTail {
    /// First external child for the Dutch Market.  No Leader result is accepted or guessed.
    MarketPersianBonus {
        call_va: u32,
        callee_va: u32,
        tribe_index: i32,
    },
    /// Bounded Village suffix through the complete linked-City maintenance window.
    VillageNoOpMaintenance(GoldenVillageNoOpMaintenance),
}

/// Exact no-write Village suffix for every supported frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenVillageNoOpMaintenance {
    pub skipped_type_cones: [i32; 4],
    pub phase_256_first_va: u32,
    pub phase_256_last_va: u32,
    pub phase_256_input: i32,
    pub phase_256_due: bool,
    pub city_gate_va: u32,
    pub in_city_flag: bool,
    pub linked_city: bool,
    pub city_slot: i16,
    pub city_maintenance_entered: bool,
    pub city_period_input: i32,
    pub city_maintenance_due: bool,
    /// First instruction after the already-owned maintenance interval.  Later Build tail
    /// instructions are not claimed by this receipt.
    pub next_boundary_va: u32,
    pub first_possible_external: GoldenVillageConditionalExternal,
}

/// Static descriptor for the first possible call after the owned Village maintenance
/// window. [`plan_village_target_gate`] binds these guards and executes the complete leaf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenVillageConditionalExternal {
    pub call_va: u32,
    pub callee_va: u32,
    pub required_city_flag: u16,
    pub required_team_style: u8,
    pub requires_active_leader_traversal: bool,
}

/// One source-complete `LeaderData::get_target` call made by the caller's active-Leader
/// loop. Array position is the actual global Leader slot, not the row's mutable `who`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenVillageTargetCall {
    pub leader_slot: u8,
    pub call_va: u32,
    pub target: i32,
    pub receipt: LeaderTargetReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenVillageTargetGateExit {
    NonCapitalCity,
    NonAssassinTeamStyle,
    ActiveLeaderScanComplete,
    UpdateLocalSeenBoundary,
}

/// The retail write immediately before the effectful virtual child. Planning never applies
/// it because `Wall::update_local_seen` is not part of this atomic transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenVillageEverSeenWrite {
    pub instruction_va: u32,
    pub leader_slot: u8,
    pub before: u8,
    pub after: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenVillageTargetGateContinuation {
    NextBuildCone {
        next_va: u32,
    },
    UpdateLocalSeen {
        call_va: u32,
        callee_va: u32,
        planned_write: GoldenVillageEverSeenWrite,
    },
}

/// Exact detached receipt for `Build::process 0x0061FB53..0x0061FBD8`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenVillageTargetGateReceipt {
    pub frame: i32,
    pub actor: GoldenBuildActor,
    pub build_ordinal: u8,
    pub object_id: i16,
    pub owner: u8,
    pub city_slot: i16,
    pub city_flags: u16,
    pub team_style: u8,
    /// Active slots reached before either scan completion or the first update boundary.
    pub reached_active_leader_mask: u8,
    pub target_calls: [Option<GoldenVillageTargetCall>; leader_get_target_runtime::LEADER_COUNT],
    pub ever_seen_before: u8,
    pub exit: GoldenVillageTargetGateExit,
    pub continuation: GoldenVillageTargetGateContinuation,
    /// The owned gate and leaf consume no RNG; the returned child is not executed.
    pub rng_draws: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenVillageTargetGateError {
    PriorContinuationDidNotReachGate,
    BuildSnapshotMismatch,
    InvalidCitySlot(i16),
    WrongCitySlot {
        expected: i16,
        actual: i16,
    },
    WrongCityOwner {
        expected: i8,
        actual: i8,
    },
    MissingLeaderTargetContext,
    LeaderTarget {
        leader_slot: u8,
        error: LeaderTargetError,
    },
}

/// Pure receipt for the common prefix and its exact first continuation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildPostWallReceipt {
    pub frame: i32,
    pub step: u8,
    pub owner: u8,
    pub actor: GoldenBuildActor,
    pub object_id: i16,
    pub uid: u16,
    pub type_index: i32,
    pub build_ordinal: u8,
    pub entry_va: u32,
    pub phase: i32,
    pub healing_before: u16,
    pub healing_after: u16,
    pub planned_healing_write: Option<BuildPostWallWrite>,
    pub build_masks: u16,
    pub attack: i32,
    pub attack_ox: i16,
    pub attack_whom: i8,
    pub attack_refresh_due: bool,
    pub near_o: i16,
    pub near_who: i16,
    pub damage: i32,
    pub damage_frac: i8,
    pub logical_queued: u8,
    pub queue_rows: usize,
    pub build_flags: u32,
    pub is_gather_type: bool,
    pub continuation: BuildPostWallContinuation,
    /// This owned interval consumes no RNG.  Reached children are not executed.
    pub rng_draws: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildPostWallPlanError {
    UnsupportedFrame(i32),
    WallDidNotReturn {
        actor: GoldenBuildActor,
        frame: i32,
        boundary: GoldenWallContinuation,
    },
    WrongOwner {
        expected: u8,
        actual: u8,
    },
    WrongObjectId {
        expected: i16,
        actual: i16,
    },
    WrongType {
        expected: i32,
        actual: i32,
    },
    InactiveGoldenBuild,
}

fn read_i16(build: &BuildData, offset: usize) -> i16 {
    i16::from_le_bytes(
        build.other[offset..offset + 2]
            .try_into()
            .expect("fixed BuildData i16 window"),
    )
}

fn read_u16(build: &BuildData, offset: usize) -> u16 {
    u16::from_le_bytes(
        build.other[offset..offset + 2]
            .try_into()
            .expect("fixed BuildData u16 window"),
    )
}

fn village_noop_maintenance(build: &BuildData, frame: i32) -> GoldenVillageNoOpMaintenance {
    let phase_256_input = frame.wrapping_add(i32::from(GOLDEN_VILLAGE_O));
    let phase_256_due = phase_256_input.rem_euclid(256) == 0;
    let in_city_flag = build.flags & BUILD_IN_CITY_FLAG != 0;
    let linked_city = build.city >= 0;
    let city_maintenance_entered = in_city_flag && linked_city;
    let city_period_input = phase_256_input;
    let city_maintenance_due =
        city_maintenance_entered && city_period_input.rem_euclid(CITY_MAINTENANCE_PERIOD) == 0;

    // Village o2000 is due at frames 48 mod 256 and 0 mod 200. Neither occurs in 2..=31.
    debug_assert!(!phase_256_due);
    debug_assert!(!city_maintenance_due);
    GoldenVillageNoOpMaintenance {
        skipped_type_cones: VILLAGE_SKIPPED_TYPE_CONES,
        phase_256_first_va: VILLAGE_PHASE_256_FIRST_VA,
        phase_256_last_va: VILLAGE_PHASE_256_LAST_VA,
        phase_256_input,
        phase_256_due,
        city_gate_va: BUILD_CITY_MAINTENANCE_GATE_VA,
        in_city_flag,
        linked_city,
        city_slot: build.city,
        city_maintenance_entered,
        city_period_input,
        city_maintenance_due,
        next_boundary_va: BUILD_CITY_MAINTENANCE_END_VA,
        first_possible_external: GoldenVillageConditionalExternal {
            call_va: VILLAGE_GET_TARGET_CALL_VA,
            callee_va: LEADER_GET_TARGET_VA,
            required_city_flag: CITY_CAPITAL_FLAG,
            required_team_style: ASSASSIN_TEAM_STYLE,
            requires_active_leader_traversal: true,
        },
    }
}

/// Plan the complete immediate Village gate after a prior common-prefix receipt reached
/// `0x0061FB53`.
///
/// The linked City record is admitted only when its slot and current owner match the same
/// Build snapshot. `leader_target` may be `None` when either retail short-circuit gate is
/// false; once a capital in team style 2 reaches the active-Leader loop, the complete Game
/// arrays and Leader rows are mandatory. The first missing `ever_seen` bit is reported as a
/// planned write plus `Wall::update_local_seen` boundary. Neither is published.
pub fn plan_village_target_gate(
    prior: &BuildPostWallReceipt,
    build: &BuildData,
    city: &tech_cities::CityRecord,
    team_style: u8,
    leader_target: Option<&LeaderTargetContext>,
) -> Result<GoldenVillageTargetGateReceipt, GoldenVillageTargetGateError> {
    let schedule_matches = golden_build_schedule(prior.frame)
        .map(|rows| rows[GoldenBuildActor::Village.build_ordinal() as usize])
        .is_some_and(|row| {
            row.wall_continuation == GoldenWallContinuation::Returned
                && row.actor == GoldenBuildActor::Village
                && row.step == prior.step
                && row.owner == prior.owner
                && row.object_id == prior.object_id
                && row.type_index == prior.type_index
                && row.build_ordinal == prior.build_ordinal
        });
    let reached_gate = matches!(
        prior.continuation,
        BuildPostWallContinuation::GoldenTypeTail(GoldenBuildTypeTail::VillageNoOpMaintenance(
            GoldenVillageNoOpMaintenance {
                next_boundary_va: BUILD_CITY_MAINTENANCE_END_VA,
                ..
            }
        ))
    );
    if !schedule_matches
        || !reached_gate
        || prior.actor != GoldenBuildActor::Village
        || prior.build_ordinal != GoldenBuildActor::Village.build_ordinal()
        || prior.owner != GOLDEN_OWNER
        || prior.object_id != GOLDEN_VILLAGE_O
        || prior.type_index != GOLDEN_VILLAGE_TYPE
    {
        return Err(GoldenVillageTargetGateError::PriorContinuationDidNotReachGate);
    }
    if prior.uid != build.uid
        || build.who != prior.owner
        || build.object_id() != prior.object_id
        || !build.is_active()
    {
        return Err(GoldenVillageTargetGateError::BuildSnapshotMismatch);
    }
    if build.city < 0 {
        return Err(GoldenVillageTargetGateError::InvalidCitySlot(build.city));
    }
    if city.city != build.city {
        return Err(GoldenVillageTargetGateError::WrongCitySlot {
            expected: build.city,
            actual: city.city,
        });
    }
    let expected_city_owner = prior.owner as i8;
    if city.who != expected_city_owner {
        return Err(GoldenVillageTargetGateError::WrongCityOwner {
            expected: expected_city_owner,
            actual: city.who,
        });
    }

    let base_receipt = |exit, continuation, reached_active_leader_mask, target_calls| {
        GoldenVillageTargetGateReceipt {
            frame: prior.frame,
            actor: prior.actor,
            build_ordinal: prior.build_ordinal,
            object_id: prior.object_id,
            owner: prior.owner,
            city_slot: build.city,
            city_flags: city.city_flags,
            team_style,
            reached_active_leader_mask,
            target_calls,
            ever_seen_before: build.ever_seen,
            exit,
            continuation,
            rng_draws: 0,
        }
    };

    let empty_calls = [None; leader_get_target_runtime::LEADER_COUNT];
    if city.city_flags & CITY_CAPITAL_FLAG == 0 {
        return Ok(base_receipt(
            GoldenVillageTargetGateExit::NonCapitalCity,
            GoldenVillageTargetGateContinuation::NextBuildCone {
                next_va: VILLAGE_TARGET_GATE_JOIN_VA,
            },
            0,
            empty_calls,
        ));
    }
    if team_style != ASSASSIN_TEAM_STYLE {
        return Ok(base_receipt(
            GoldenVillageTargetGateExit::NonAssassinTeamStyle,
            GoldenVillageTargetGateContinuation::NextBuildCone {
                next_va: VILLAGE_TARGET_GATE_JOIN_VA,
            },
            0,
            empty_calls,
        ));
    }

    let context = leader_target.ok_or(GoldenVillageTargetGateError::MissingLeaderTargetContext)?;
    let mut reached_active_leader_mask = 0u8;
    let mut target_calls = empty_calls;
    for leader_slot in 0..leader_get_target_runtime::LEADER_COUNT {
        let leader_flags = context.leaders[leader_slot].leader_flags;
        if leader_flags & leader_get_target_runtime::ACTIVE_LEADER_MASK
            != leader_get_target_runtime::ACTIVE_LEADER_MASK
        {
            continue;
        }
        let leader_bit = 1u8 << leader_slot;
        reached_active_leader_mask |= leader_bit;
        let target_receipt =
            leader_get_target_runtime::get_target(context, leader_slot).map_err(|error| {
                GoldenVillageTargetGateError::LeaderTarget {
                    leader_slot: leader_slot as u8,
                    error,
                }
            })?;
        target_calls[leader_slot] = Some(GoldenVillageTargetCall {
            leader_slot: leader_slot as u8,
            call_va: VILLAGE_GET_TARGET_CALL_VA,
            target: target_receipt.target,
            receipt: target_receipt,
        });
        if target_receipt.target != i32::from(build.who) || build.ever_seen & leader_bit != 0 {
            continue;
        }

        let planned_write = GoldenVillageEverSeenWrite {
            instruction_va: VILLAGE_EVER_SEEN_WRITE_VA,
            leader_slot: leader_slot as u8,
            before: build.ever_seen,
            after: build.ever_seen | leader_bit,
        };
        return Ok(base_receipt(
            GoldenVillageTargetGateExit::UpdateLocalSeenBoundary,
            GoldenVillageTargetGateContinuation::UpdateLocalSeen {
                call_va: VILLAGE_UPDATE_LOCAL_SEEN_CALL_VA,
                callee_va: WALL_UPDATE_LOCAL_SEEN_VA,
                planned_write,
            },
            reached_active_leader_mask,
            target_calls,
        ));
    }

    Ok(base_receipt(
        GoldenVillageTargetGateExit::ActiveLeaderScanComplete,
        GoldenVillageTargetGateContinuation::NextBuildCone {
            next_va: VILLAGE_TARGET_GATE_JOIN_VA,
        },
        reached_active_leader_mask,
        target_calls,
    ))
}

/// Plan the source-complete active-Build prefix after `Wall::process` returned.
///
/// The schedule guard rejects the three actor/frames which still stop in Wall's territory
/// cone after periodic `check_ever_seen` has been composed: Market frames 15 and 31, and
/// Village frame 16.  The receipt plans, but never publishes, the earlier healing write when
/// a later child is reached.
pub fn plan_after_wall_return(
    actor: GoldenBuildActor,
    frame: i32,
    build: &BuildData,
    facts: BuildPostWallTypeFacts,
) -> Result<BuildPostWallReceipt, BuildPostWallPlanError> {
    let schedule = golden_build_schedule(frame)
        .ok_or(BuildPostWallPlanError::UnsupportedFrame(frame))?[actor.build_ordinal() as usize];
    if schedule.wall_continuation != GoldenWallContinuation::Returned {
        return Err(BuildPostWallPlanError::WallDidNotReturn {
            actor,
            frame,
            boundary: schedule.wall_continuation,
        });
    }
    if build.who != GOLDEN_OWNER {
        return Err(BuildPostWallPlanError::WrongOwner {
            expected: GOLDEN_OWNER,
            actual: build.who,
        });
    }
    if build.object_id() != actor.object_id() {
        return Err(BuildPostWallPlanError::WrongObjectId {
            expected: actor.object_id(),
            actual: build.object_id(),
        });
    }
    if facts.type_index != actor.type_index() {
        return Err(BuildPostWallPlanError::WrongType {
            expected: actor.type_index(),
            actual: facts.type_index,
        });
    }
    if !build.is_active() {
        return Err(BuildPostWallPlanError::InactiveGoldenBuild);
    }

    let phase = frame.wrapping_add(i32::from(build.object_id()));
    let healing_before = read_u16(build, HEALING_OFFSET);
    let healing_after = healing_before.saturating_sub(1);
    let planned_healing_write = (healing_before != 0).then_some(BuildPostWallWrite {
        instruction_va: BUILD_HEALING_STORE_VA,
        field: BuildPostWallWriteField::Healing,
        before: healing_before,
        after: healing_after,
    });
    let near_o = read_i16(build, NEAR_O_OFFSET);
    let near_who = read_i16(build, NEAR_WHO_OFFSET);
    let attack_refresh_due = phase.rem_euclid(32) == 0;
    let is_gather_type = facts.build_flags & BUILD_GATHER_TYPE_MASK != 0;

    let continuation = if build.build_masks & production::mask::EJECTING != 0 {
        BuildPostWallContinuation::ProcessEjection {
            callee_va: BUILD_PROCESS_EJECTION_VA,
        }
    } else if build.build_masks & BUILD_LAUNCH_MASK != 0 {
        BuildPostWallContinuation::ObjectDoLaunch {
            call_va: OBJECT_DO_LAUNCH_CALL_VA,
            callee_va: OBJECT_DO_LAUNCH_VA,
        }
    } else if facts.attack != 0 && build.attack_ox >= 0 && build.attack_whom >= 0 {
        BuildPostWallContinuation::DoAttack {
            reason: AttackChildReason::CachedAttackCoordinates,
            callee_va: BUILD_DO_ATTACK_VA,
        }
    } else if facts.attack != 0 && attack_refresh_due {
        BuildPostWallContinuation::DoAttack {
            reason: AttackChildReason::Phase32Refresh,
            callee_va: BUILD_DO_ATTACK_VA,
        }
    } else if facts.attack != 0 && near_o >= 0 {
        BuildPostWallContinuation::ObjectIsInRange {
            near_o,
            near_who,
            callee_va: OBJECT_IS_IN_RANGE_VA,
        }
    } else if build.build_masks & production::mask::OWNERSHIP_LATCH != 0 {
        BuildPostWallContinuation::OwnershipLatch
    } else if build.damage != 0 || build.damage_frac != 0 {
        BuildPostWallContinuation::DamageRecovery {
            damage: build.damage,
            damage_frac: build.damage_frac,
        }
    } else if build.queue.queued != 0 {
        BuildPostWallContinuation::NonEmptyDoQueue {
            call_va: BUILD_DO_QUEUE_CALL_VA,
            callee_va: BUILD_DO_QUEUE_VA,
            queued: build.queue.queued,
        }
    } else if is_gather_type {
        BuildPostWallContinuation::GatherTypeTail {
            call_va: BUILD_IS_GATHER_TYPE_CALL_VA,
            callee_va: BUILD_IS_GATHER_TYPE_VA,
        }
    } else {
        let tail = match actor {
            GoldenBuildActor::Village => {
                GoldenBuildTypeTail::VillageNoOpMaintenance(village_noop_maintenance(build, frame))
            }
            GoldenBuildActor::Market => GoldenBuildTypeTail::MarketPersianBonus {
                call_va: MARKET_PERSIAN_BONUS_CALL_VA,
                callee_va: LEADER_HAS_TRIBE_BONUS_VA,
                tribe_index: PERSIAN_TRIBE_INDEX,
            },
        };
        BuildPostWallContinuation::GoldenTypeTail(tail)
    };

    Ok(BuildPostWallReceipt {
        frame,
        step: GOLDEN_BUILD_STEP,
        owner: GOLDEN_OWNER,
        actor,
        object_id: actor.object_id(),
        uid: build.uid,
        type_index: actor.type_index(),
        build_ordinal: actor.build_ordinal(),
        entry_va: BUILD_POST_WALL_ENTRY_VA,
        phase,
        healing_before,
        healing_after,
        planned_healing_write,
        build_masks: build.build_masks,
        attack: facts.attack,
        attack_ox: build.attack_ox,
        attack_whom: build.attack_whom,
        attack_refresh_due,
        near_o,
        near_who,
        damage: build.damage,
        damage_frac: build.damage_frac,
        logical_queued: build.queue.queued,
        queue_rows: build.queue.num(),
        build_flags: facts.build_flags,
        is_gather_type,
        continuation,
        rng_draws: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(actor: GoldenBuildActor) -> BuildData {
        let mut build = BuildData::default();
        build.flags = production::flag::VALID
            | production::flag::STARTED
            | production::flag::ACTIVE
            | BUILD_IN_CITY_FLAG;
        build.who = GOLDEN_OWNER;
        build.set_object_id(actor.object_id());
        build.city = 0;
        build.attack_ox = -1;
        build.attack_whom = -1;
        build.other[NEAR_O_OFFSET..NEAR_O_OFFSET + 2].copy_from_slice(&(-1_i16).to_le_bytes());
        build.other[NEAR_WHO_OFFSET..NEAR_WHO_OFFSET + 2].copy_from_slice(&(-1_i16).to_le_bytes());
        build
    }

    fn facts(actor: GoldenBuildActor) -> BuildPostWallTypeFacts {
        BuildPostWallTypeFacts {
            type_index: actor.type_index(),
            attack: if actor == GoldenBuildActor::Village {
                80
            } else {
                0
            },
            build_flags: 0,
        }
    }

    fn city(flags: u16) -> tech_cities::CityRecord {
        tech_cities::CityRecord {
            city_flags: flags,
            city: 0,
            who: GOLDEN_OWNER as i8,
            ..tech_cities::CityRecord::default()
        }
    }

    fn target_context() -> LeaderTargetContext {
        LeaderTargetContext {
            leaders: std::array::from_fn(|slot| leader_get_target_runtime::LeaderTargetRow {
                leader_flags: if slot == 0 {
                    leader_get_target_runtime::ACTIVE_LEADER_MASK
                } else {
                    0
                },
                who: slot as i32,
            }),
            game: leader_get_target_runtime::LeaderTargetGame {
                start_list: [0, 1, 2, 3, 4, 5, 6, 7],
                start_index: [7; leader_get_target_runtime::LEADER_COUNT],
            },
        }
    }

    #[test]
    fn fixed_order_and_post_periodic_wall_boundaries_cover_every_frame() {
        for frame in GOLDEN_FIRST_FRAME..=GOLDEN_LAST_FRAME {
            let [village, market] = golden_build_schedule(frame).unwrap();
            assert_eq!((village.build_ordinal, village.object_id), (0, 2000));
            assert_eq!((market.build_ordinal, market.object_id), (1, 2001));
            assert_eq!(village.periodic_check_ever_seen_due, frame % 8 == 0);
            assert_eq!(market.periodic_check_ever_seen_due, frame % 8 == 0);
            assert_eq!(
                village.wall_continuation,
                if frame == 16 {
                    GoldenWallContinuation::TerritorySlot
                } else {
                    GoldenWallContinuation::Returned
                }
            );
            assert_eq!(
                market.wall_continuation,
                if frame == 15 || frame == 31 {
                    GoldenWallContinuation::TerritorySlot
                } else {
                    GoldenWallContinuation::Returned
                }
            );
        }
    }

    #[test]
    fn earliest_frame_village_suffix_is_a_pure_noop_through_city_maintenance() {
        let build = build(GoldenBuildActor::Village);
        let image_before = build.image();
        let receipt = plan_after_wall_return(
            GoldenBuildActor::Village,
            GOLDEN_FIRST_FRAME,
            &build,
            facts(GoldenBuildActor::Village),
        )
        .unwrap();
        assert_eq!(
            build.image(),
            image_before,
            "planner must not publish writes"
        );
        assert_eq!(
            (receipt.owner, receipt.object_id, receipt.build_ordinal),
            (0, 2000, 0)
        );
        assert_eq!(receipt.rng_draws, 0);
        let BuildPostWallContinuation::GoldenTypeTail(GoldenBuildTypeTail::VillageNoOpMaintenance(
            tail,
        )) = receipt.continuation
        else {
            panic!("unexpected continuation: {:?}", receipt.continuation);
        };
        assert_eq!(tail.skipped_type_cones, [538, 438, 436, 529]);
        assert!(!tail.phase_256_due);
        assert!(tail.city_maintenance_entered);
        assert!(!tail.city_maintenance_due);
        assert_eq!(tail.next_boundary_va, BUILD_CITY_MAINTENANCE_END_VA);
        assert_eq!(
            tail.first_possible_external,
            GoldenVillageConditionalExternal {
                call_va: 0x0061_fb9b,
                callee_va: 0x006d_a000,
                required_city_flag: 0x10,
                required_team_style: 2,
                requires_active_leader_traversal: true,
            }
        );
    }

    #[test]
    fn village_target_gate_short_circuits_without_unreached_leader_inputs() {
        let build = build(GoldenBuildActor::Village);
        let prior = plan_after_wall_return(
            GoldenBuildActor::Village,
            GOLDEN_FIRST_FRAME,
            &build,
            facts(GoldenBuildActor::Village),
        )
        .unwrap();

        let noncapital =
            plan_village_target_gate(&prior, &build, &city(1), ASSASSIN_TEAM_STYLE, None).unwrap();
        assert_eq!(noncapital.exit, GoldenVillageTargetGateExit::NonCapitalCity);
        assert_eq!(noncapital.reached_active_leader_mask, 0);
        assert_eq!(
            noncapital.continuation,
            GoldenVillageTargetGateContinuation::NextBuildCone {
                next_va: 0x0061_fbd8
            }
        );

        let ordinary_teams =
            plan_village_target_gate(&prior, &build, &city(1 | CITY_CAPITAL_FLAG), 0, None)
                .unwrap();
        assert_eq!(
            ordinary_teams.exit,
            GoldenVillageTargetGateExit::NonAssassinTeamStyle
        );
        assert!(ordinary_teams.target_calls.iter().all(Option::is_none));
    }

    #[test]
    fn first_unseen_targeter_plans_write_then_stops_before_update_local_seen() {
        let build = build(GoldenBuildActor::Village);
        let image_before = build.image();
        let prior = plan_after_wall_return(
            GoldenBuildActor::Village,
            GOLDEN_FIRST_FRAME,
            &build,
            facts(GoldenBuildActor::Village),
        )
        .unwrap();
        let receipt = plan_village_target_gate(
            &prior,
            &build,
            &city(1 | CITY_CAPITAL_FLAG),
            ASSASSIN_TEAM_STYLE,
            Some(&target_context()),
        )
        .unwrap();

        assert_eq!(
            build.image(),
            image_before,
            "planner may not publish the bit"
        );
        assert_eq!(
            (receipt.actor, receipt.build_ordinal),
            (GoldenBuildActor::Village, 0)
        );
        assert_eq!(receipt.reached_active_leader_mask, 1);
        let target_call = receipt.target_calls[0].unwrap();
        assert_eq!((target_call.leader_slot, target_call.target), (0, 0));
        assert_eq!(target_call.receipt.function_va, 0x006d_a000);
        assert_eq!(target_call.receipt.rng_draws, 0);
        assert_eq!(
            receipt.continuation,
            GoldenVillageTargetGateContinuation::UpdateLocalSeen {
                call_va: 0x0061_fbd2,
                callee_va: 0x0063_ed50,
                planned_write: GoldenVillageEverSeenWrite {
                    instruction_va: 0x0061_fbcd,
                    leader_slot: 0,
                    before: 0,
                    after: 1,
                },
            }
        );
        assert_eq!(receipt.rng_draws, 0);
    }

    #[test]
    fn already_seen_targeter_finishes_active_leader_scan_locally() {
        let mut build = build(GoldenBuildActor::Village);
        build.ever_seen = 1;
        let prior = plan_after_wall_return(
            GoldenBuildActor::Village,
            GOLDEN_FIRST_FRAME,
            &build,
            facts(GoldenBuildActor::Village),
        )
        .unwrap();
        let receipt = plan_village_target_gate(
            &prior,
            &build,
            &city(1 | CITY_CAPITAL_FLAG),
            ASSASSIN_TEAM_STYLE,
            Some(&target_context()),
        )
        .unwrap();
        assert_eq!(
            receipt.exit,
            GoldenVillageTargetGateExit::ActiveLeaderScanComplete
        );
        assert_eq!(
            receipt.continuation,
            GoldenVillageTargetGateContinuation::NextBuildCone {
                next_va: 0x0061_fbd8
            }
        );
    }

    #[test]
    fn every_reachable_village_frame_has_unscheduled_maintenance() {
        for frame in GOLDEN_FIRST_FRAME..=GOLDEN_LAST_FRAME {
            if frame == 16 {
                continue;
            }
            let build = build(GoldenBuildActor::Village);
            let receipt = plan_after_wall_return(
                GoldenBuildActor::Village,
                frame,
                &build,
                facts(GoldenBuildActor::Village),
            )
            .unwrap();
            let BuildPostWallContinuation::GoldenTypeTail(
                GoldenBuildTypeTail::VillageNoOpMaintenance(tail),
            ) = receipt.continuation
            else {
                panic!("unexpected continuation: {:?}", receipt.continuation);
            };
            assert!(!tail.phase_256_due);
            assert!(!tail.city_maintenance_due);
        }
    }

    #[test]
    fn market_stops_at_the_correctly_named_persian_bonus_child() {
        let build = build(GoldenBuildActor::Market);
        let receipt = plan_after_wall_return(
            GoldenBuildActor::Market,
            GOLDEN_FIRST_FRAME,
            &build,
            facts(GoldenBuildActor::Market),
        )
        .unwrap();
        assert_eq!(
            receipt.continuation,
            BuildPostWallContinuation::GoldenTypeTail(GoldenBuildTypeTail::MarketPersianBonus {
                call_va: 0x0061_f5e4,
                callee_va: 0x006e_1370,
                tribe_index: 23,
            })
        );
    }

    #[test]
    fn healing_is_planned_before_a_later_child_but_never_applied() {
        let mut build = build(GoldenBuildActor::Village);
        build.other[HEALING_OFFSET..HEALING_OFFSET + 2].copy_from_slice(&2_u16.to_le_bytes());
        build.build_masks |= production::mask::EJECTING;
        let receipt = plan_after_wall_return(
            GoldenBuildActor::Village,
            GOLDEN_FIRST_FRAME,
            &build,
            facts(GoldenBuildActor::Village),
        )
        .unwrap();
        assert_eq!(read_u16(&build, HEALING_OFFSET), 2);
        assert_eq!(
            receipt.planned_healing_write,
            Some(BuildPostWallWrite {
                instruction_va: BUILD_HEALING_STORE_VA,
                field: BuildPostWallWriteField::Healing,
                before: 2,
                after: 1,
            })
        );
        assert_eq!(
            receipt.continuation,
            BuildPostWallContinuation::ProcessEjection {
                callee_va: BUILD_PROCESS_EJECTION_VA
            }
        );
    }

    #[test]
    fn launch_mask_reaches_object_do_launch_not_build_missile_launch() {
        let mut build = build(GoldenBuildActor::Village);
        build.build_masks |= BUILD_LAUNCH_MASK;
        let receipt = plan_after_wall_return(
            GoldenBuildActor::Village,
            GOLDEN_FIRST_FRAME,
            &build,
            facts(GoldenBuildActor::Village),
        )
        .unwrap();
        assert_eq!(
            receipt.continuation,
            BuildPostWallContinuation::ObjectDoLaunch {
                call_va: 0x0061_ee5f,
                callee_va: 0x0064_f3b0,
            }
        );
    }

    #[test]
    fn territory_frames_cannot_be_misreported_as_post_wall_execution() {
        assert_eq!(
            plan_after_wall_return(
                GoldenBuildActor::Village,
                16,
                &build(GoldenBuildActor::Village),
                facts(GoldenBuildActor::Village),
            ),
            Err(BuildPostWallPlanError::WallDidNotReturn {
                actor: GoldenBuildActor::Village,
                frame: 16,
                boundary: GoldenWallContinuation::TerritorySlot,
            })
        );
        assert_eq!(
            plan_after_wall_return(
                GoldenBuildActor::Market,
                31,
                &build(GoldenBuildActor::Market),
                facts(GoldenBuildActor::Market),
            ),
            Err(BuildPostWallPlanError::WallDidNotReturn {
                actor: GoldenBuildActor::Market,
                frame: 31,
                boundary: GoldenWallContinuation::TerritorySlot,
            })
        );
    }
}
