// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only reconstruction of `SpecialAnimOrder` installation and execution.
//!
//! The module is registered and its fail-closed, mutation-ordered transaction is consumed by
//! the typed `systems::order_dispatch` host seam. The production `Sim::do_frame` bridge reaches
//! the host-free UNIT and object-free EXIT arms; remaining concrete world effects stay listed in
//! `SPECIAL_ANIM_OPEN_TAILS`. See `docs/assembly/special-anim-executor.md`.

pub const SPECIAL_ANIM_ORDER_INDEX: i32 = 25;
pub const SPECIAL_ANIM_ORDER_SIZE: usize = 44;
pub const SPECIAL_ANIM_WALKED_BYTES: usize = 37;
pub const SPECIAL_ANIM_WALK_PAYLOAD_BYTES: usize = 36;
pub const ORDER_GROUP: u8 = 4;

pub const SPECIAL_ANIM_WALK_DATA_VA: u32 = 0x0048_49a0;
pub const SPECIAL_ANIM_CLEAR_VA: u32 = 0x0048_4ec0;
pub const UNIT_ADD_SPECIAL_ANIM_ORDER_VA: u32 = 0x005e_4160;
pub const UNIT_DO_SPECIAL_ANIM_VA: u32 = 0x005e_5880;
pub const UNIT_DO_SPECIAL_UNIT_ANIM_VA: u32 = 0x005e_5be0;
pub const UNIT_LAND_PLANE_VA: u32 = 0x005e_9950;

pub const AIRCRAFT_CARRIER_TYPE: i32 = 0x15f;
pub const AIRBASE_TYPE: i32 = 0x1bf;
pub const ENTRY_FRAMES: i32 = 10;
pub const FIXED_EXIT_X_OFFSET: i32 = -0xc0;
pub const HELICOPTER_X_BIAS: i32 = -0xc5;
pub const HELICOPTER_Y_BIAS: i32 = -5;
pub const RANDOM_MIN: i32 = 0;
pub const RANDOM_MAX: i32 = 0xffff;
/// Largest realized result for shipped `Random::get(0, 0xffff)` scaling.
pub const RANDOM_RESULT_MAX: i32 = 0xfffe;

pub mod offsets {
    pub const FLAGS: usize = 0x04;
    pub const SPECIAL_TYPE: usize = 0x08;
    pub const STARTED: usize = 0x0c;
    pub const FRAMES: usize = 0x10;
    pub const DATA1: usize = 0x14;
    pub const DATA2: usize = 0x18;
    pub const DATA3: usize = 0x1c;
    pub const DATA4: usize = 0x20;
    pub const OX: usize = 0x24;
    pub const WHOM: usize = 0x28;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum SpecialAnimKind {
    Enter = 0,
    Exit = 1,
    Unit = 2,
}

impl SpecialAnimKind {
    pub const fn from_raw(raw: i32) -> Option<Self> {
        match raw {
            0 => Some(Self::Enter),
            1 => Some(Self::Exit),
            2 => Some(Self::Unit),
            _ => None,
        }
    }
}

/// The complete contiguous nine-word payload walked by the concrete order class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialAnimState {
    pub special_type: SpecialAnimKind,
    pub started: i32,
    pub frames: i32,
    pub data1: i32,
    pub data2: i32,
    pub data3: i32,
    pub data4: i32,
    pub ox: i32,
    pub whom: i32,
}

impl Default for SpecialAnimState {
    fn default() -> Self {
        Self {
            special_type: SpecialAnimKind::Unit,
            started: 0,
            frames: 0,
            data1: -1,
            data2: -1,
            data3: -1,
            data4: -1,
            ox: -1,
            whom: -1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialAnimInstallRequest {
    pub special_type: SpecialAnimKind,
    pub data1: i32,
    pub data2: i32,
    /// Present in the shipped ABI, but the 92-byte wrapper never reads it.
    pub queue_pos: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialAnimInstallStep {
    AllocateOrder(i32),
    AddOrder,
    ClearPartialPath,
    RotateInsertedOrderToHead,
    UpdateAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecialAnimInstallPlan {
    pub order: SpecialAnimState,
    pub flags: u8,
    pub steps: Vec<SpecialAnimInstallStep>,
}

/// Exact after-image of `Unit::add_spec_anim_order(type, data1, data2, QueuePos)`.
///
/// All queue values produce the same plan: the wrapper always installs at the front.
pub fn plan_special_anim_install(request: SpecialAnimInstallRequest) -> SpecialAnimInstallPlan {
    SpecialAnimInstallPlan {
        order: SpecialAnimState {
            special_type: request.special_type,
            data1: request.data1,
            data2: request.data2,
            ..SpecialAnimState::default()
        },
        flags: ORDER_GROUP,
        steps: vec![
            SpecialAnimInstallStep::AllocateOrder(SPECIAL_ANIM_ORDER_INDEX),
            SpecialAnimInstallStep::AddOrder,
            SpecialAnimInstallStep::ClearPartialPath,
            SpecialAnimInstallStep::RotateInsertedOrderToHead,
            SpecialAnimInstallStep::UpdateAction,
        ],
    }
}

/// The sole direct shipped caller of `add_spec_anim_order` is `Unit::land_plane`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LandPlaneInstallRequest {
    pub target_o: i32,
    pub target_who: i32,
    /// Target virtual zero-argument `get_gpiece()` return value.
    pub target_gpiece: i32,
    /// The actor type flag chooses mode/data2 3 when set, otherwise 1.
    pub actor_helicopter: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LandPlaneInstallStep {
    ReadTargetGpiece { result: i32 },
    AddSpecialAnimEnter { data1: i32, data2: i32 },
    PatchData3TargetO(i32),
    PatchData4TargetWho(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandPlaneInstallPlan {
    pub order: SpecialAnimState,
    pub flags: u8,
    pub steps: Vec<LandPlaneInstallStep>,
}

/// Exact SPECIAL_ANIM after-image of the successful installer portion of `Unit::land_plane`.
/// The optimized caller places a target pointer in the callee's dead QueuePos slot; it has no
/// queue semantics and is intentionally absent here.
pub fn plan_land_plane_install(request: LandPlaneInstallRequest) -> LandPlaneInstallPlan {
    let mode = if request.actor_helicopter { 3 } else { 1 };
    LandPlaneInstallPlan {
        order: SpecialAnimState {
            special_type: SpecialAnimKind::Enter,
            data1: request.target_gpiece,
            data2: mode,
            data3: request.target_o,
            data4: request.target_who,
            ..SpecialAnimState::default()
        },
        flags: ORDER_GROUP,
        steps: vec![
            LandPlaneInstallStep::ReadTargetGpiece {
                result: request.target_gpiece,
            },
            LandPlaneInstallStep::AddSpecialAnimEnter {
                data1: request.target_gpiece,
                data2: mode,
            },
            LandPlaneInstallStep::PatchData3TargetO(request.target_o),
            LandPlaneInstallStep::PatchData4TargetWho(request.target_who),
        ],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorFacts {
    pub identity: ObjectIdentity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnterTargetFacts {
    pub identity: ObjectIdentity,
    pub is_valid_build: bool,
    /// Queried only when `is_valid_build` is false.
    pub is_aircraft_carrier: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitTargetFacts {
    pub identity: ObjectIdentity,
    pub is_airbase: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialAnimExecutorRequest {
    pub order: SpecialAnimState,
    pub actor: ActorFacts,
    pub enter_target: Option<EnterTargetFacts>,
    pub exit_target: Option<ExitTargetFacts>,
    /// Exact results of the two canonical `Random::get(0, 0xffff)` calls.
    pub random_draws: Option<[i32; 2]>,
    /// Two distinct ordered reads of `UnitTypeData::unit_flags & 0x20 != 0`. The first chooses
    /// offsets/RNG; the second occurs after the first Guy Z write and chooses the +200 Z write.
    pub helicopter_samples: Option<[bool; 2]>,
    /// Result of `TerrainOut::find_data_z(data3, data4, 0)`.
    pub terrain_z: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialAnimPlanError {
    MalformedActorAddress,
    MalformedTargetAddress,
    MissingEnterTarget,
    UnexpectedEnterTarget,
    MissingExitTarget,
    UnexpectedExitTarget,
    TargetSlotMismatch,
    MissingAircraftCarrierPredicate,
    UnexpectedAircraftCarrierPredicate,
    MissingRandomDraws,
    UnexpectedRandomDraws,
    RandomDrawOutOfRange,
    MissingHelicopterSamples,
    UnexpectedHelicopterSamples,
    MissingTerrainHeight,
    UnexpectedTerrainHeight,
    ReceiptActorMismatch,
    ReceiptOrderMismatch,
    ReceiptTargetMismatch,
    ReceiptSnapshotMismatch,
    ReceiptPlanMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialAnimBranch {
    UnitNoOp,
    EnterGoInside,
    EnterDie,
    ExitWithoutAirbase,
    ExitAirbase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialAnimHostStep {
    StoreFrames(i32),
    StoreStarted(i32),
    QueryIsValidBuild {
        target_o: i32,
        target_who: i32,
        result: bool,
    },
    QueryIsAircraftCarrier {
        target_o: i32,
        target_who: i32,
        type_index: i32,
        strict: i32,
        result: bool,
    },
    QueryIsAirbase {
        target_o: i32,
        target_who: i32,
        type_index: i32,
        strict: i32,
        result: bool,
    },
    RandomGet {
        min: i32,
        max: i32,
        result: i32,
    },
    ReadActorHelicopterFlag {
        sample: i32,
        result: bool,
    },
    GoInside {
        target_o: i32,
        target_who: i32,
        arg3: i32,
    },
    KillCurrentOrder(i32),
    Die {
        arg1: i32,
        arg2: i32,
        arg3_bits: u32,
    },
    /// Shipped `set_angle` does not read its second ABI argument.
    SetAngle {
        angle: i32,
        arg3: i32,
    },
    SetNewLocation {
        x: i32,
        y: i32,
        arg3: i32,
        arg4: i32,
    },
    FindTerrainZ {
        x: i32,
        y: i32,
        arg3: i32,
        result: i32,
    },
    SetPrimaryGuyZ {
        z: i32,
        arg2: i32,
    },
    RaisePrimaryGuyZ {
        delta: i32,
        arg2: i32,
    },
    ShiftPitchToLastAndZero,
    ShiftBankToLastAndZero,
    SameTickUnitWork,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecialAnimExecutorPlan {
    pub branch: SpecialAnimBranch,
    pub after_order: SpecialAnimState,
    pub steps: Vec<SpecialAnimHostStep>,
}

fn require_actor(identity: ObjectIdentity) -> Result<(), SpecialAnimPlanError> {
    if identity.o < 0 || identity.who < 0 {
        return Err(SpecialAnimPlanError::MalformedActorAddress);
    }
    Ok(())
}

fn matches_slot(identity: ObjectIdentity, o: i32, who: i32) -> bool {
    identity.o == o && identity.who == who
}

fn reject_exit_only_facts(
    request: &SpecialAnimExecutorRequest,
) -> Result<(), SpecialAnimPlanError> {
    if request.exit_target.is_some() {
        return Err(SpecialAnimPlanError::UnexpectedExitTarget);
    }
    if request.random_draws.is_some() {
        return Err(SpecialAnimPlanError::UnexpectedRandomDraws);
    }
    if request.helicopter_samples.is_some() {
        return Err(SpecialAnimPlanError::UnexpectedHelicopterSamples);
    }
    if request.terrain_z.is_some() {
        return Err(SpecialAnimPlanError::UnexpectedTerrainHeight);
    }
    Ok(())
}

fn reject_enter_target(request: &SpecialAnimExecutorRequest) -> Result<(), SpecialAnimPlanError> {
    if request.enter_target.is_some() {
        return Err(SpecialAnimPlanError::UnexpectedEnterTarget);
    }
    Ok(())
}

fn entered_state(order: SpecialAnimState) -> SpecialAnimState {
    SpecialAnimState {
        started: 1,
        frames: ENTRY_FRAMES,
        ..order
    }
}

/// Reconstruct one complete reachable `Unit::do_spec_anim` arm.
///
/// Facts that retail would only query conditionally must be absent on all other paths. This makes
/// short-circuit order, RNG consumption, and external effects mutation-sensitive rather than
/// silently defaulting an unknown host result to false.
pub fn plan_special_anim_executor(
    request: SpecialAnimExecutorRequest,
) -> Result<SpecialAnimExecutorPlan, SpecialAnimPlanError> {
    if request.order.special_type == SpecialAnimKind::Unit {
        if request.enter_target.is_some() {
            return Err(SpecialAnimPlanError::UnexpectedEnterTarget);
        }
        reject_exit_only_facts(&request)?;
        return Ok(SpecialAnimExecutorPlan {
            branch: SpecialAnimBranch::UnitNoOp,
            after_order: request.order,
            steps: Vec::new(),
        });
    }

    require_actor(request.actor.identity)?;

    let after_order = entered_state(request.order);
    let mut steps = vec![
        SpecialAnimHostStep::StoreFrames(ENTRY_FRAMES),
        SpecialAnimHostStep::StoreStarted(1),
    ];

    match request.order.special_type {
        SpecialAnimKind::Enter => {
            reject_exit_only_facts(&request)?;
            if request.order.data3 < 0 || request.order.data4 < 0 {
                return Err(SpecialAnimPlanError::MalformedTargetAddress);
            }
            let target = request
                .enter_target
                .ok_or(SpecialAnimPlanError::MissingEnterTarget)?;
            if !matches_slot(target.identity, request.order.data3, request.order.data4) {
                return Err(SpecialAnimPlanError::TargetSlotMismatch);
            }
            steps.push(SpecialAnimHostStep::QueryIsValidBuild {
                target_o: target.identity.o,
                target_who: target.identity.who,
                result: target.is_valid_build,
            });
            let accepted = if target.is_valid_build {
                if target.is_aircraft_carrier.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedAircraftCarrierPredicate);
                }
                true
            } else {
                let is_carrier = target
                    .is_aircraft_carrier
                    .ok_or(SpecialAnimPlanError::MissingAircraftCarrierPredicate)?;
                steps.push(SpecialAnimHostStep::QueryIsAircraftCarrier {
                    target_o: target.identity.o,
                    target_who: target.identity.who,
                    type_index: AIRCRAFT_CARRIER_TYPE,
                    strict: 0,
                    result: is_carrier,
                });
                is_carrier
            };

            if accepted {
                steps.push(SpecialAnimHostStep::GoInside {
                    target_o: target.identity.o,
                    target_who: target.identity.who,
                    arg3: 0,
                });
                steps.push(SpecialAnimHostStep::KillCurrentOrder(0));
                Ok(SpecialAnimExecutorPlan {
                    branch: SpecialAnimBranch::EnterGoInside,
                    after_order,
                    steps,
                })
            } else {
                steps.push(SpecialAnimHostStep::KillCurrentOrder(0));
                steps.push(SpecialAnimHostStep::Die {
                    arg1: 0,
                    arg2: -1,
                    arg3_bits: 0.0f32.to_bits(),
                });
                Ok(SpecialAnimExecutorPlan {
                    branch: SpecialAnimBranch::EnterDie,
                    after_order,
                    steps,
                })
            }
        }
        SpecialAnimKind::Exit => {
            reject_enter_target(&request)?;
            if request.order.ox < 0 {
                if request.exit_target.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedExitTarget);
                }
                if request.random_draws.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedRandomDraws);
                }
                if request.helicopter_samples.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedHelicopterSamples);
                }
                if request.terrain_z.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedTerrainHeight);
                }
                steps.push(SpecialAnimHostStep::KillCurrentOrder(0));
                return Ok(SpecialAnimExecutorPlan {
                    branch: SpecialAnimBranch::ExitWithoutAirbase,
                    after_order,
                    steps,
                });
            }
            if request.order.whom < 0 {
                return Err(SpecialAnimPlanError::MalformedTargetAddress);
            }
            let target = request
                .exit_target
                .ok_or(SpecialAnimPlanError::MissingExitTarget)?;
            if !matches_slot(target.identity, request.order.ox, request.order.whom) {
                return Err(SpecialAnimPlanError::TargetSlotMismatch);
            }
            steps.push(SpecialAnimHostStep::QueryIsAirbase {
                target_o: target.identity.o,
                target_who: target.identity.who,
                type_index: AIRBASE_TYPE,
                strict: 0,
                result: target.is_airbase,
            });
            if !target.is_airbase {
                if request.random_draws.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedRandomDraws);
                }
                if request.helicopter_samples.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedHelicopterSamples);
                }
                if request.terrain_z.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedTerrainHeight);
                }
                steps.push(SpecialAnimHostStep::KillCurrentOrder(0));
                return Ok(SpecialAnimExecutorPlan {
                    branch: SpecialAnimBranch::ExitWithoutAirbase,
                    after_order,
                    steps,
                });
            }

            let terrain_z = request
                .terrain_z
                .ok_or(SpecialAnimPlanError::MissingTerrainHeight)?;
            let helicopter_samples = request
                .helicopter_samples
                .ok_or(SpecialAnimPlanError::MissingHelicopterSamples)?;
            steps.push(SpecialAnimHostStep::ReadActorHelicopterFlag {
                sample: 1,
                result: helicopter_samples[0],
            });
            let (x_offset, y_offset) = if helicopter_samples[0] {
                let draws = request
                    .random_draws
                    .ok_or(SpecialAnimPlanError::MissingRandomDraws)?;
                if draws
                    .iter()
                    .any(|draw| !(RANDOM_MIN..=RANDOM_RESULT_MAX).contains(draw))
                {
                    return Err(SpecialAnimPlanError::RandomDrawOutOfRange);
                }
                steps.push(SpecialAnimHostStep::RandomGet {
                    min: RANDOM_MIN,
                    max: RANDOM_MAX,
                    result: draws[0],
                });
                steps.push(SpecialAnimHostStep::RandomGet {
                    min: RANDOM_MIN,
                    max: RANDOM_MAX,
                    result: draws[1],
                });
                (
                    draws[0] % 11 + HELICOPTER_X_BIAS,
                    draws[1] % 11 + HELICOPTER_Y_BIAS,
                )
            } else {
                if request.random_draws.is_some() {
                    return Err(SpecialAnimPlanError::UnexpectedRandomDraws);
                }
                (FIXED_EXIT_X_OFFSET, 0)
            };

            steps.extend([
                SpecialAnimHostStep::SetAngle { angle: 0, arg3: 1 },
                SpecialAnimHostStep::SetNewLocation {
                    x: request.order.data3.wrapping_add(x_offset),
                    y: request.order.data4.wrapping_add(y_offset),
                    arg3: 1,
                    arg4: 1,
                },
                SpecialAnimHostStep::FindTerrainZ {
                    x: request.order.data3,
                    y: request.order.data4,
                    arg3: 0,
                    result: terrain_z,
                },
                SpecialAnimHostStep::SetPrimaryGuyZ {
                    z: terrain_z,
                    arg2: 1,
                },
            ]);
            steps.push(SpecialAnimHostStep::ReadActorHelicopterFlag {
                sample: 2,
                result: helicopter_samples[1],
            });
            if helicopter_samples[1] {
                steps.push(SpecialAnimHostStep::RaisePrimaryGuyZ {
                    delta: 200,
                    arg2: 1,
                });
            }
            steps.extend([
                SpecialAnimHostStep::ShiftPitchToLastAndZero,
                SpecialAnimHostStep::ShiftBankToLastAndZero,
                SpecialAnimHostStep::KillCurrentOrder(0),
                SpecialAnimHostStep::SameTickUnitWork,
            ]);
            Ok(SpecialAnimExecutorPlan {
                branch: SpecialAnimBranch::ExitAirbase,
                after_order,
                steps,
            })
        }
        SpecialAnimKind::Unit => unreachable!("handled before entry writes"),
    }
}

/// Threshold used by the compiled continuation comparison after the forced `started = 1` write.
pub const fn compiled_continuation_threshold(
    kind: SpecialAnimKind,
    ox_nonnegative: bool,
    exit_target_is_airbase: bool,
) -> Option<i32> {
    match kind {
        SpecialAnimKind::Unit => None,
        SpecialAnimKind::Exit if ox_nonnegative && exit_target_is_airbase => Some(1),
        SpecialAnimKind::Enter | SpecialAnimKind::Exit => Some(0),
    }
}

/// The body at `0x005e5ada..0x005e5bbf` is guarded by signed `started < threshold`.
/// Entry writes `started = 1`, while the only reachable thresholds are zero and one.
pub const fn compiled_continuation_reachable(
    kind: SpecialAnimKind,
    ox_nonnegative: bool,
    exit_target_is_airbase: bool,
) -> bool {
    match compiled_continuation_threshold(kind, ox_nonnegative, exit_target_is_airbase) {
        Some(threshold) => 1 < threshold,
        None => false,
    }
}

/// Machine-emitted progress tails below the impossible `started < threshold` comparison. These
/// are descriptive CFG evidence only and are never emitted by the reachable planner above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompiledDeadTailStep {
    SetLocationFromExitBase,
    FindTerrainZFromExitBase,
    ResolvePrimaryGuyUnchecked,
    SetGuyZFromTerrainIfStartedZeroElseShiftCurrentToLast,
    QueryEnterTargetIsValidBuildWithoutAirbaseFallback,
    InvalidEnterKillThenDie,
    DecodeTargetXyzWithXorKey(u32),
    SetLocationFromDecodedTarget,
    ShiftGuyZToDecodedTargetZ,
    SetAngleZeroWithDeadMiddleAbiWord,
    ShiftBankToLastAndZero,
    IncrementStarted,
}

pub const COMPILED_EXIT_PROGRESS_TAIL: &[CompiledDeadTailStep] = &[
    CompiledDeadTailStep::SetLocationFromExitBase,
    CompiledDeadTailStep::FindTerrainZFromExitBase,
    CompiledDeadTailStep::ResolvePrimaryGuyUnchecked,
    CompiledDeadTailStep::SetGuyZFromTerrainIfStartedZeroElseShiftCurrentToLast,
    CompiledDeadTailStep::SetAngleZeroWithDeadMiddleAbiWord,
    CompiledDeadTailStep::ShiftBankToLastAndZero,
    CompiledDeadTailStep::IncrementStarted,
];

pub const COMPILED_NON_EXIT_PROGRESS_TAIL: &[CompiledDeadTailStep] = &[
    CompiledDeadTailStep::QueryEnterTargetIsValidBuildWithoutAirbaseFallback,
    CompiledDeadTailStep::InvalidEnterKillThenDie,
    CompiledDeadTailStep::DecodeTargetXyzWithXorKey(0x0006_3637),
    CompiledDeadTailStep::SetLocationFromDecodedTarget,
    CompiledDeadTailStep::ResolvePrimaryGuyUnchecked,
    CompiledDeadTailStep::ShiftGuyZToDecodedTargetZ,
    CompiledDeadTailStep::SetAngleZeroWithDeadMiddleAbiWord,
    CompiledDeadTailStep::ShiftBankToLastAndZero,
    CompiledDeadTailStep::IncrementStarted,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectSnapshot {
    pub identity: ObjectIdentity,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialAnimHostSnapshot {
    pub actor: ObjectSnapshot,
    pub target: Option<ObjectSnapshot>,
    pub current_order: SpecialAnimState,
    pub current_order_digest: u64,
    pub queue_digest: u64,
    pub path_digest: u64,
    pub primary_guy_digest: u64,
    pub object_epoch: u64,
    pub terrain_epoch: u64,
    pub external_epoch: u64,
    pub rng_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecialAnimExecutorReceipt {
    pub snapshot: SpecialAnimHostSnapshot,
    pub request: SpecialAnimExecutorRequest,
    pub plan: SpecialAnimExecutorPlan,
}

fn requested_target(request: &SpecialAnimExecutorRequest) -> Option<ObjectIdentity> {
    match request.order.special_type {
        SpecialAnimKind::Enter => request.enter_target.map(|target| target.identity),
        SpecialAnimKind::Exit => request.exit_target.map(|target| target.identity),
        SpecialAnimKind::Unit => None,
    }
}

/// Bind every queried/mutated host surface and the RNG epoch before publication.
pub fn preflight_special_anim_executor(
    snapshot: SpecialAnimHostSnapshot,
    request: SpecialAnimExecutorRequest,
) -> Result<SpecialAnimExecutorReceipt, SpecialAnimPlanError> {
    if snapshot.actor.identity != request.actor.identity {
        return Err(SpecialAnimPlanError::ReceiptActorMismatch);
    }
    if snapshot.current_order != request.order {
        return Err(SpecialAnimPlanError::ReceiptOrderMismatch);
    }
    if snapshot.target.map(|target| target.identity) != requested_target(&request) {
        return Err(SpecialAnimPlanError::ReceiptTargetMismatch);
    }
    let plan = plan_special_anim_executor(request)?;
    Ok(SpecialAnimExecutorReceipt {
        snapshot,
        request,
        plan,
    })
}

/// Revalidate and recompute before atomically publishing the plan. Any failure authorizes no
/// actor/order/queue/path/guy/terrain/external mutation and consumes no RNG value.
pub fn validate_special_anim_receipt(
    receipt: &SpecialAnimExecutorReceipt,
    current: SpecialAnimHostSnapshot,
) -> Result<&SpecialAnimExecutorPlan, SpecialAnimPlanError> {
    if receipt.snapshot != current {
        return Err(SpecialAnimPlanError::ReceiptSnapshotMismatch);
    }
    let recomputed = plan_special_anim_executor(receipt.request)?;
    if recomputed != receipt.plan {
        return Err(SpecialAnimPlanError::ReceiptPlanMismatch);
    }
    Ok(&receipt.plan)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialAnimOpenTail {
    InternalLandPlaneInstaller,
    ObjectLookupAndVirtualPredicates,
    CanonicalGameRandom,
    AngleAndLocationMutation,
    TerrainAndPrimaryGuyMutation,
    ContainmentAndDeath,
    QueueRetirementAndSameTickWork,
    DispatcherAndLiveTickAdapter,
}

pub const SPECIAL_ANIM_OPEN_TAILS: &[SpecialAnimOpenTail] = &[
    SpecialAnimOpenTail::InternalLandPlaneInstaller,
    SpecialAnimOpenTail::ObjectLookupAndVirtualPredicates,
    SpecialAnimOpenTail::CanonicalGameRandom,
    SpecialAnimOpenTail::AngleAndLocationMutation,
    SpecialAnimOpenTail::TerrainAndPrimaryGuyMutation,
    SpecialAnimOpenTail::ContainmentAndDeath,
    SpecialAnimOpenTail::QueueRetirementAndSameTickWork,
];
