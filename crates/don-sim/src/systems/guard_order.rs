// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only reconstruction of the GUARD order installer and one `Unit::do_guard` frame.
//!
//! These pure planners freeze the complete retail decisions. The canonical runtime currently
//! consumes only the receipt-proven singleton Queue-New installer and nonmoving periodic-idle
//! frame; spatial, movement, casting, RNG and general formation tails remain fail-closed, so the
//! command/executor row is not globally complete.

pub const GUARD_ORDER_INDEX: i32 = 12;
pub const UNIT_ADD_GUARD_ORDER_VA: u32 = 0x005e_3e40;
pub const UNIT_UPDATE_GUARD_ORDER_VA: u32 = 0x005e_3220;
pub const UNIT_DO_GUARD_VA: u32 = 0x005e_5c70;
pub const GROUP_ACTION_GUARD_VA: u32 = 0x006f_cd30;
pub const COMMAND_PROCESS_GUARD_VA: u32 = 0x0094_78a0;

pub const GUARD_ORDER_SIZE: usize = 56;
pub const GUARD_WALKED_BYTES: usize = 35;
pub const ORDER_GROUP: u8 = 4;
pub const QUEUE_FIRST: i32 = 0;
pub const QUEUE_LAST: i32 = 1;
pub const QUEUE_NEW: i32 = 2;

pub const GUARD_BUILD_ATTACK_DISTANCE: i32 = 0x600;
pub const GUARD_PHASE_PERIOD: i32 = 16;
pub const GUARD_PHASE_MELEE_OFFSET: i32 = 8;
pub const GUARD_SPOT_MASK: u32 = 0x5555_5555;
pub const GUARD_FILTER_NOT_ME: i32 = 3;
pub const GUARD_CAST_TYPE: i32 = 0x28c;
pub const GUARD_CAST_FAST_TYPE: i32 = 0x7b;
pub const GUARD_CAST_FAST_TICKS: i32 = 30;
pub const GUARD_CAST_SLOW_TICKS: i32 = 70;

/// Compiler-emitted concrete layout from `rise.pdb`.
pub mod offsets {
    pub const OX: usize = 0x08;
    pub const WHOM: usize = 0x0c;
    pub const UID: usize = 0x10;
    pub const DX: usize = 0x14;
    pub const DY: usize = 0x18;
    pub const GUARD_X: usize = 0x1c;
    pub const GUARD_Y: usize = 0x20;
    pub const IDLE: usize = 0x24;
    pub const RETRY: usize = 0x28;
    pub const UNIT_ORDER_VBASE: usize = 0x30;
    pub const FLAGS: usize = 0x34;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardIdentity {
    pub o: i32,
    pub who: i32,
    pub uid: u16,
}

/// Complete checksum-visible concrete payload, excluding the virtual-base flag byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardOrderState {
    pub target: GuardIdentity,
    pub dx: i32,
    pub dy: i32,
    pub guard_x: i32,
    pub guard_y: i32,
    pub idle: i32,
    pub retry: i32,
}

impl Default for GuardOrderState {
    fn default() -> Self {
        Self {
            target: GuardIdentity {
                o: -1,
                who: -1,
                uid: u16::MAX,
            },
            dx: 0,
            dy: 0,
            guard_x: 0,
            guard_y: 0,
            idle: 0,
            retry: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardInstallRequest {
    pub actor: GuardIdentity,
    pub actor_x: i32,
    pub actor_y: i32,
    pub target_o: i32,
    pub target_who: i32,
    pub dx: i32,
    pub dy: i32,
    pub queue_pos: i32,
    /// The sixth retail argument is fetched by the ABI but never read by the body.
    pub unused_arg6: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardInstallTarget {
    pub identity: GuardIdentity,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardInstallStep {
    ClearActorMask(u32),
    StoreActorC0(i32),
    CloseOrders(i32),
    ClearPartialPath,
    UpdateAction,
    AllocateOrder(i32),
    AddOrder,
    RotateFirstOrderToHead,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardInstallPlan {
    pub order: GuardOrderState,
    pub flags: u8,
    pub steps: Vec<GuardInstallStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardPlanError {
    MissingInstallTarget,
    InstallTargetSlotMismatch,
    MissingTarget,
    TargetSlotMismatch,
    MissingInitialOnMap,
    UnexpectedInitialOnMap,
    MissingSecondOnMap,
    UnexpectedSecondOnMap,
    MissingSpatialFacts,
    MissingBuildingSpot,
    UnexpectedBuildingSpot,
    MissingPrimarySpot,
    UnexpectedPrimarySpot,
    MissingSecondarySpot,
    UnexpectedSecondarySpot,
    MissingFindAngle,
    UnexpectedFindAngle,
    MissingPostMoveFacts,
    MissingRandomDraw,
    UnexpectedRandomDraw,
    RandomDrawOutOfRange,
}

/// Exact `Unit::add_guard_order` after-image and call ordering.
pub fn plan_guard_install(
    request: GuardInstallRequest,
    target: Option<GuardInstallTarget>,
) -> Result<GuardInstallPlan, GuardPlanError> {
    let has_address = request.target_o >= 0 && request.target_who >= 0;
    let (uid, guard_x, guard_y) = if has_address {
        let target = target.ok_or(GuardPlanError::MissingInstallTarget)?;
        if target.identity.o != request.target_o || target.identity.who != request.target_who {
            return Err(GuardPlanError::InstallTargetSlotMismatch);
        }
        (target.identity.uid, target.x, target.y)
    } else {
        (u16::MAX, request.actor_x, request.actor_y)
    };

    let mut steps = Vec::new();
    if request.queue_pos == QUEUE_NEW {
        steps.extend([
            GuardInstallStep::ClearActorMask(0x0400_0000),
            GuardInstallStep::StoreActorC0(0),
            GuardInstallStep::CloseOrders(0),
            GuardInstallStep::ClearPartialPath,
            GuardInstallStep::UpdateAction,
        ]);
    }
    steps.push(GuardInstallStep::AllocateOrder(GUARD_ORDER_INDEX));
    steps.push(GuardInstallStep::AddOrder);
    if request.queue_pos == QUEUE_FIRST {
        steps.push(GuardInstallStep::ClearPartialPath);
        steps.push(GuardInstallStep::RotateFirstOrderToHead);
    }
    steps.push(GuardInstallStep::UpdateAction);

    Ok(GuardInstallPlan {
        order: GuardOrderState {
            target: GuardIdentity {
                o: request.target_o,
                who: request.target_who,
                uid,
            },
            dx: request.dx,
            dy: request.dy,
            guard_x,
            guard_y,
            idle: 0,
            retry: 0,
        },
        flags: ORDER_GROUP,
        steps,
    })
}

/// After `Unit::update_guard_order(ox, whom)` finds an existing order anywhere in the
/// circular queue, `Group::action_guard` updates only these two offsets.  Retail preserves
/// UID, snapped destination, idle age, and retry delay.
pub fn update_existing_guard_offsets(
    mut order: GuardOrderState,
    dx: i32,
    dy: i32,
) -> GuardOrderState {
    order.dx = dx;
    order.dy = dy;
    order
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardActorFacts {
    pub identity: GuardIdentity,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub frame: i32,
    pub unit_masks: u32,
    pub type_flags_2b4: u32,
    pub type_flags_2b8: u32,
    pub type_slot_10c: bool,
    pub actor_slot_cc: bool,
    pub actor_slot_c4: bool,
    pub actor_slot_100: bool,
    pub is_unpacking: bool,
    pub is_type_7b: bool,
    pub is_captain: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardTargetFacts {
    pub identity: GuardIdentity,
    pub active: bool,
    /// First vslot `+0x08` result, queried before the retry gate.
    pub initial_valid_unit: bool,
    /// First `is_on_map` result. Retail calls it only when `initial_valid_unit` is true and
    /// discards the value.
    pub initial_on_map: Option<bool>,
    /// The second vslot `+0x08` result, after a zero retry.
    pub valid_unit: bool,
    pub on_map: Option<bool>,
    /// Target-unit virtual `+0xd8`; all three predicates form retail's `moving` value.
    pub is_moving: bool,
    /// Target vslot `+0x1c`, named `is_wallbuild` by the shipped PDB.
    pub is_wallbuild: bool,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub unit_masks: u32,
    pub attack_dist: i32,
    pub x_size: i32,
    pub y_size: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardSpotOutcome {
    /// `UnitType::find_nearby_spot` returned zero and wrote these coordinates.
    Found { x: i32, y: i32 },
    /// The call returned nonzero. Retail uses the branch-specific fallback coordinate.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardNearbyRequest {
    pub centre_x: i32,
    pub centre_y: i32,
    pub min_radius: i32,
    pub max_radius: i32,
    pub step: i32,
    pub angle_mask: u32,
    pub filter: i32,
    pub actor_o: i32,
    pub actor_who: i32,
    pub tail_12: i32,
    pub tail_13: i32,
    pub tail_14: i32,
    pub tail_15: i32,
    pub tail_16: i32,
}

/// Host-produced spatial results.  The field names encode exactly where the retail value is
/// sampled; none is a permissive default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardSpatialFacts {
    /// Exact `sinx(target.angle, order.dy)` result.
    pub sin_dy: i32,
    /// Exact `cosx(target.angle, effective_dx)` result.
    pub cos_dx: i32,
    /// Exact `cosx(target.angle, order.dy)` result.
    pub cos_dy: i32,
    /// Exact `sinx(target.angle, effective_dx)` result.
    pub sin_dx: i32,
    /// `div_3_table[projected >> 4]`, before retail's `>>2` invalid-loc arguments.
    pub projected_div3_x: i32,
    pub projected_div3_y: i32,
    pub invalid_loc: bool,
    pub terrain_flags: u16,
    pub building_spot: Option<GuardSpotOutcome>,
    pub primary_spot: Option<GuardSpotOutcome>,
    pub secondary_spot: Option<GuardSpotOutcome>,
    /// `div_3_table[chosen >> 4]`; the stored destination is this value * 48 + 24.
    pub chosen_div3_x: i32,
    pub chosen_div3_y: i32,
    /// Results of the final snapped-cell comparison.
    pub actor_cell_x: i32,
    pub actor_cell_y: i32,
    pub guard_cell_x: i32,
    pub guard_cell_y: i32,
    /// Exact `find_angle(guard_x-target.x, guard_y-target.y)` result when requested.
    pub find_angle: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardPostMoveFacts {
    /// The exact Manhattan expression used for `MoveOrder.pause`, before `max(1, value)`.
    pub coarse_manhattan: i32,
    /// `UnitData::order_type()` immediately after the same-tick `do_move` call.
    pub head_is_guard: bool,
    /// Canonical `game_random.get(0, 0xffff)` result, reached only when GUARD is exposed.
    pub random_draw: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuardExecutorRequest {
    pub actor: GuardActorFacts,
    pub order: GuardOrderState,
    pub target: Option<GuardTargetFacts>,
    pub spatial: Option<GuardSpatialFacts>,
    pub post_move: Option<GuardPostMoveFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupMoveNearRequest {
    pub x: i32,
    pub y: i32,
    pub tail: [i32; 9],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddMoveFacingRequest {
    pub x: i32,
    pub y: i32,
    pub angle: i32,
    pub tail: [i32; 8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardHostStep {
    SetAnimation {
        animation: i32,
        arg0: i32,
        arg1: i32,
    },
    KillCurrentOrder {
        arg: i32,
    },
    FindMeleeTarget {
        args: [i32; 5],
    },
    FindNearbySpot {
        request: GuardNearbyRequest,
        outcome: GuardSpotOutcome,
    },
    GroupActionMoveNear(GroupMoveNearRequest),
    SetAngle {
        angle: i32,
        update_position: i32,
    },
    AddMoveFacingOrder(AddMoveFacingRequest),
    SetInsertedMovePause(i32),
    UpdateOrderThenDoMove,
    DrawGameRandom {
        low: i32,
        high: i32,
        result: i32,
    },
    AddCastOrder {
        target_o: i32,
        target_who: i32,
        x: i32,
        y: i32,
        cast_type: i32,
        queue_pos: i32,
        arg7: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardExecutorBranch {
    TerminalInvalidTarget,
    RetryDelay,
    PeriodicMeleeScan,
    PeriodicIdlePulse,
    HoldNearWallBuild,
    CannotApproachWallBuild,
    ReplaceWithWallBuildMove,
    MoveStillCurrent,
    RetryAfterMove,
    AutoCast,
    IdleAtGuardPoint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardExecutorPlan {
    pub order_after: GuardOrderState,
    pub branch: GuardExecutorBranch,
    pub steps: Vec<GuardHostStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardExecutorTransactionStatus {
    Unavailable,
    Applied,
}

/// Receipt shape for the future atomic host adapter.  Revalidation recomputes the entire
/// branch, including all conditionally consumed host facts and ordered open-tail calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardExecutorReceipt {
    pub request: GuardExecutorRequest,
    pub status: GuardExecutorTransactionStatus,
    pub plan: Option<GuardExecutorPlan>,
}

impl GuardExecutorReceipt {
    pub fn unavailable(request: GuardExecutorRequest) -> Self {
        Self {
            request,
            status: GuardExecutorTransactionStatus::Unavailable,
            plan: None,
        }
    }

    pub fn applied(request: GuardExecutorRequest) -> Result<Self, GuardPlanError> {
        let plan = plan_guard_executor(request)?;
        Ok(Self {
            request,
            status: GuardExecutorTransactionStatus::Applied,
            plan: Some(plan),
        })
    }

    pub fn validates(&self, expected: GuardExecutorRequest) -> bool {
        if self.request != expected {
            return false;
        }
        match self.status {
            GuardExecutorTransactionStatus::Unavailable => self.plan.is_none(),
            GuardExecutorTransactionStatus::Applied => {
                plan_guard_executor(expected).is_ok_and(|plan| self.plan.as_ref() == Some(&plan))
            }
        }
    }
}

fn validate_on_map(
    valid_unit: bool,
    on_map: Option<bool>,
    missing: GuardPlanError,
    unexpected: GuardPlanError,
) -> Result<bool, GuardPlanError> {
    if valid_unit {
        on_map.ok_or(missing)
    } else if on_map.is_some() {
        Err(unexpected)
    } else {
        Ok(false)
    }
}

fn nearby_request(
    centre_x: i32,
    centre_y: i32,
    min_radius: i32,
    max_radius: i32,
    step: i32,
    actor: GuardIdentity,
    tail: [i32; 5],
) -> GuardNearbyRequest {
    GuardNearbyRequest {
        centre_x,
        centre_y,
        min_radius,
        max_radius,
        step,
        angle_mask: GUARD_SPOT_MASK,
        filter: GUARD_FILTER_NOT_ME,
        actor_o: actor.o,
        actor_who: actor.who,
        tail_12: tail[0],
        tail_13: tail[1],
        tail_14: tail[2],
        tail_15: tail[3],
        tail_16: tail[4],
    }
}

/// Exact integer approximation at `vector_dist` `0x0046cff0`.
pub fn guard_vector_dist(a: i32, b: i32) -> i32 {
    let ai = a.wrapping_abs();
    let bi = b.wrapping_abs();
    let (big, small) = if ai > bi { (ai, bi) } else { (bi, ai) };
    if big == 0 {
        return 0;
    }
    if small >= 60_000 {
        return (((small as u32).wrapping_add((big as u32) << 1)) >> 1) as i32;
    }
    let square = (small as u32).wrapping_mul(small as u32);
    (square / ((big as u32) << 1)) as i32 + big
}

fn terminal(order: GuardOrderState) -> GuardExecutorPlan {
    GuardExecutorPlan {
        order_after: order,
        branch: GuardExecutorBranch::TerminalInvalidTarget,
        steps: vec![
            GuardHostStep::SetAnimation {
                animation: 0,
                arg0: 0,
                arg1: 1,
            },
            GuardHostStep::KillCurrentOrder { arg: 0 },
        ],
    }
}

/// Plan one complete retail `Unit::do_guard` activation.
///
/// This is a decision transcription, not an integration receipt.  Object virtuals, trig,
/// `div_3_table`, path admission/search, temporary groups, movement, casting, and RNG remain
/// explicit host facts or steps.  See [`GUARD_OPEN_TAILS`].
pub fn plan_guard_executor(
    request: GuardExecutorRequest,
) -> Result<GuardExecutorPlan, GuardPlanError> {
    let mut order = request.order;
    if order.target.o < 0 {
        return Ok(terminal(order));
    }

    let target = request.target.ok_or(GuardPlanError::MissingTarget)?;
    if target.identity.o != order.target.o || target.identity.who != order.target.who {
        return Err(GuardPlanError::TargetSlotMismatch);
    }
    if !target.active {
        return Ok(terminal(order));
    }

    let _initial_on_map = validate_on_map(
        target.initial_valid_unit,
        target.initial_on_map,
        GuardPlanError::MissingInitialOnMap,
        GuardPlanError::UnexpectedInitialOnMap,
    )?;

    // Retail tests the full word, not `> 0`; malformed negative values decrement forever.
    if order.retry != 0 {
        order.retry = order.retry.wrapping_sub(1);
        return Ok(GuardExecutorPlan {
            order_after: order,
            branch: GuardExecutorBranch::RetryDelay,
            steps: Vec::new(),
        });
    }

    let on_map = validate_on_map(
        target.valid_unit,
        target.on_map,
        GuardPlanError::MissingSecondOnMap,
        GuardPlanError::UnexpectedSecondOnMap,
    )?;
    let moving = target.valid_unit && on_map && target.is_moving;

    if !moving {
        let phase = i32::from(request.actor.identity.o as i16).wrapping_add(request.actor.frame);
        if phase.wrapping_add(GUARD_PHASE_MELEE_OFFSET) % GUARD_PHASE_PERIOD == 0 {
            return Ok(GuardExecutorPlan {
                order_after: order,
                branch: GuardExecutorBranch::PeriodicMeleeScan,
                steps: vec![GuardHostStep::FindMeleeTarget {
                    args: [-1, 0, 0, 1, 0],
                }],
            });
        }
        if phase % GUARD_PHASE_PERIOD == 0 {
            order.idle = order.idle.wrapping_add(1);
            return Ok(GuardExecutorPlan {
                order_after: order,
                branch: GuardExecutorBranch::PeriodicIdlePulse,
                steps: Vec::new(),
            });
        }
    }

    if target.is_wallbuild {
        if target.attack_dist <= GUARD_BUILD_ATTACK_DISTANCE {
            return Ok(GuardExecutorPlan {
                order_after: order,
                branch: GuardExecutorBranch::HoldNearWallBuild,
                steps: Vec::new(),
            });
        }
        if !request.actor.is_captain {
            return Ok(GuardExecutorPlan {
                order_after: order,
                branch: GuardExecutorBranch::CannotApproachWallBuild,
                steps: Vec::new(),
            });
        }
        let spatial = request.spatial.ok_or(GuardPlanError::MissingSpatialFacts)?;
        let outcome = spatial
            .building_spot
            .ok_or(GuardPlanError::MissingBuildingSpot)?;
        if spatial.primary_spot.is_some() || spatial.secondary_spot.is_some() {
            return Err(GuardPlanError::UnexpectedBuildingSpot);
        }
        let extent = target.x_size.wrapping_add(target.y_size);
        let find = nearby_request(
            target.x,
            target.y,
            extent.wrapping_mul(0x30),
            extent.wrapping_add(0x20).wrapping_mul(0x30),
            0,
            request.actor.identity,
            [0, 1, -1, 0, -1],
        );
        let (x, y) = match outcome {
            GuardSpotOutcome::Found { x, y } => (x, y),
            GuardSpotOutcome::Failed => (target.x, target.y),
        };
        return Ok(GuardExecutorPlan {
            order_after: order,
            branch: GuardExecutorBranch::ReplaceWithWallBuildMove,
            steps: vec![
                GuardHostStep::FindNearbySpot {
                    request: find,
                    outcome,
                },
                GuardHostStep::GroupActionMoveNear(GroupMoveNearRequest {
                    x,
                    y,
                    tail: [0, QUEUE_NEW, 0, 0, 1, 0, -1, -1, 0],
                }),
            ],
        });
    }

    let spatial = request.spatial.ok_or(GuardPlanError::MissingSpatialFacts)?;
    if spatial.building_spot.is_some() {
        return Err(GuardPlanError::UnexpectedBuildingSpot);
    }
    let effective_dx = if target.unit_masks & 2 != 0 {
        order.dx.wrapping_neg()
    } else {
        order.dx
    };
    let projected_x = target
        .x
        .wrapping_add(spatial.sin_dy)
        .wrapping_add(spatial.cos_dx);
    let projected_y = target
        .y
        .wrapping_sub(spatial.cos_dy)
        .wrapping_add(spatial.sin_dx);
    let _projection_contract = (effective_dx, projected_x, projected_y);

    let mut steps = Vec::new();
    let mut chosen = (projected_x, projected_y);
    if spatial.invalid_loc {
        let direct_cell =
            request.actor.type_flags_2b4 & 0x10 != 0 && spatial.terrain_flags & 0x30 != 0x20;
        if direct_cell {
            if spatial.primary_spot.is_some() || spatial.secondary_spot.is_some() {
                return Err(GuardPlanError::UnexpectedPrimarySpot);
            }
            chosen = (
                spatial
                    .projected_div3_x
                    .wrapping_mul(0x30)
                    .wrapping_add(0x18),
                spatial
                    .projected_div3_y
                    .wrapping_mul(0x30)
                    .wrapping_add(0x18),
            );
        } else {
            let primary = spatial
                .primary_spot
                .ok_or(GuardPlanError::MissingPrimarySpot)?;
            let projected_cell_x = spatial
                .projected_div3_x
                .wrapping_mul(0x30)
                .wrapping_add(0x18);
            let projected_cell_y = spatial
                .projected_div3_y
                .wrapping_mul(0x30)
                .wrapping_add(0x18);
            let primary_request = nearby_request(
                projected_cell_x,
                projected_cell_y,
                0xc0,
                0x180,
                0x60,
                request.actor.identity,
                [0, 0, -1, 0, -1],
            );
            steps.push(GuardHostStep::FindNearbySpot {
                request: primary_request,
                outcome: primary,
            });
            match primary {
                GuardSpotOutcome::Found { x, y } => {
                    if spatial.secondary_spot.is_some() {
                        return Err(GuardPlanError::UnexpectedSecondarySpot);
                    }
                    chosen = (x, y);
                }
                GuardSpotOutcome::Failed => {
                    let secondary = spatial
                        .secondary_spot
                        .ok_or(GuardPlanError::MissingSecondarySpot)?;
                    let radius = guard_vector_dist(order.dx, order.dy);
                    let secondary_request = nearby_request(
                        target.x,
                        target.y,
                        radius,
                        radius.wrapping_add(0x180),
                        0xc0,
                        request.actor.identity,
                        [0, 0, -1, 0, -1],
                    );
                    steps.push(GuardHostStep::FindNearbySpot {
                        request: secondary_request,
                        outcome: secondary,
                    });
                    chosen = match secondary {
                        GuardSpotOutcome::Found { x, y } => (x, y),
                        GuardSpotOutcome::Failed => (request.actor.x, request.actor.y),
                    };
                }
            }
        }
    } else if spatial.primary_spot.is_some() || spatial.secondary_spot.is_some() {
        return Err(GuardPlanError::UnexpectedPrimarySpot);
    }
    let _chosen_contract = chosen;

    order.guard_x = spatial.chosen_div3_x.wrapping_mul(0x30).wrapping_add(0x18);
    order.guard_y = spatial.chosen_div3_y.wrapping_mul(0x30).wrapping_add(0x18);

    let desired_angle = if moving {
        if spatial.find_angle.is_some() {
            return Err(GuardPlanError::UnexpectedFindAngle);
        }
        target.angle
    } else {
        spatial.find_angle.ok_or(GuardPlanError::MissingFindAngle)?
    };
    let force_target_angle = request.actor.unit_masks & 0x0004_0000 != 0
        && (request.actor.type_slot_10c
            || request.actor.actor_slot_cc
            || request.actor.actor_slot_c4);
    let desired_angle = if force_target_angle {
        target.angle
    } else {
        desired_angle
    };

    let same_cell = spatial.actor_cell_x == spatial.guard_cell_x
        && spatial.actor_cell_y == spatial.guard_cell_y;
    if same_cell {
        if request.post_move.is_some() {
            return Err(GuardPlanError::UnexpectedRandomDraw);
        }
        if !moving && request.actor.angle != desired_angle {
            steps.push(GuardHostStep::SetAngle {
                angle: desired_angle,
                update_position: 0,
            });
        }
        order.idle = order.idle.wrapping_add(1);
        let auto_cast = request.actor.type_flags_2b8 & 4 != 0
            && !request.actor.actor_slot_100
            && request.actor.unit_masks & 0x0008_0000 != 0
            && !request.actor.is_unpacking;
        let threshold = if request.actor.is_type_7b {
            GUARD_CAST_FAST_TICKS
        } else {
            GUARD_CAST_SLOW_TICKS
        };
        if auto_cast && order.idle >= threshold {
            steps.push(GuardHostStep::AddCastOrder {
                target_o: -1,
                target_who: -1,
                x: -1,
                y: -1,
                cast_type: GUARD_CAST_TYPE,
                queue_pos: QUEUE_FIRST,
                arg7: 0,
            });
            return Ok(GuardExecutorPlan {
                order_after: order,
                branch: GuardExecutorBranch::AutoCast,
                steps,
            });
        }
        steps.push(GuardHostStep::SetAnimation {
            animation: 0,
            arg0: 0,
            arg1: 1,
        });
        return Ok(GuardExecutorPlan {
            order_after: order,
            branch: GuardExecutorBranch::IdleAtGuardPoint,
            steps,
        });
    }

    order.idle = 0;
    steps.push(GuardHostStep::AddMoveFacingOrder(AddMoveFacingRequest {
        x: order.guard_x,
        y: order.guard_y,
        angle: desired_angle,
        tail: [2, 0, QUEUE_FIRST, 0, -1, -1, -1, 0],
    }));
    let post = request
        .post_move
        .ok_or(GuardPlanError::MissingPostMoveFacts)?;
    let distance = if post.coarse_manhattan > 1 {
        post.coarse_manhattan
    } else {
        1
    };
    steps.push(GuardHostStep::SetInsertedMovePause(
        distance.wrapping_mul(30),
    ));
    steps.push(GuardHostStep::UpdateOrderThenDoMove);
    if !post.head_is_guard {
        if post.random_draw.is_some() {
            return Err(GuardPlanError::UnexpectedRandomDraw);
        }
        return Ok(GuardExecutorPlan {
            order_after: order,
            branch: GuardExecutorBranch::MoveStillCurrent,
            steps,
        });
    }

    let draw = post.random_draw.ok_or(GuardPlanError::MissingRandomDraw)?;
    if !(0..=0xffff).contains(&draw) {
        return Err(GuardPlanError::RandomDrawOutOfRange);
    }
    steps.push(GuardHostStep::SetAnimation {
        animation: 0,
        arg0: 0,
        arg1: 1,
    });
    steps.push(GuardHostStep::DrawGameRandom {
        low: 0,
        high: 0xffff,
        result: draw,
    });
    order.retry = draw % 3 + 6;
    Ok(GuardExecutorPlan {
        order_after: order,
        branch: GuardExecutorBranch::RetryAfterMove,
        steps,
    })
}

/// Runtime capabilities the pure plan does not own.  Every one must be backed by an atomic,
/// versioned host receipt before GUARD can move from strict red to Complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardOpenTail {
    GroupActionGuardFormationAndUpdate,
    ObjectVirtualPredicates,
    TrigDiv3TerrainAndInvalidLoc,
    FindMeleeTarget,
    FindNearbySpot,
    TemporaryGroupActionMoveNear,
    AddMoveFacingAndSameTickDoMove,
    AutoCastInsertion,
    CanonicalGameRandom,
    ConcretePayloadSaveAndLiveTickAdapter,
}

pub const GUARD_OPEN_TAILS: [GuardOpenTail; 10] = [
    GuardOpenTail::GroupActionGuardFormationAndUpdate,
    GuardOpenTail::ObjectVirtualPredicates,
    GuardOpenTail::TrigDiv3TerrainAndInvalidLoc,
    GuardOpenTail::FindMeleeTarget,
    GuardOpenTail::FindNearbySpot,
    GuardOpenTail::TemporaryGroupActionMoveNear,
    GuardOpenTail::AddMoveFacingAndSameTickDoMove,
    GuardOpenTail::AutoCastInsertion,
    GuardOpenTail::CanonicalGameRandom,
    GuardOpenTail::ConcretePayloadSaveAndLiveTickAdapter,
];
