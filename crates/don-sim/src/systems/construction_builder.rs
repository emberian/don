//! Exact unit-side construction policy around [`super::construction`].
//!
//! The site lifecycle owns counters and placement.  This module owns the deterministic
//! decisions `Unit::do_build`, `Unit::check_build_order` and `Unit::build_done` make around
//! that lifecycle.  It returns plans so the order/group/world host can apply mutations in
//! retail order without replacing the policy with an Arena heuristic.

use crate::order::{OrderIndex, ORDER_GROUP};
use crate::systems::order_dispatch::OrderRec;

use super::construction::{BuilderGate, ChecksumEffects, EffectReceipt, ObjectKey};

/// These policy bodies make no direct simulation-RNG calls.
pub const DIRECT_RNG_DRAWS: u32 = 0;

pub const CHAR_BUILD: i32 = 0x21;
pub const CHAR_SOW: i32 = 0x23;

/// Worker-stance values consumed by `Unit::do_build` / `Unit::build_done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerStance {
    Gather,
    BuildAndGather,
    Build,
    Other(i32),
}

impl WorkerStance {
    pub fn from_retail(v: i32) -> Self {
        match v {
            0 => Self::Gather,
            1 => Self::BuildAndGather,
            2 => Self::Build,
            x => Self::Other(x),
        }
    }

    fn wants_build(self) -> bool {
        matches!(self, Self::BuildAndGather | Self::Build)
    }

    fn wants_gather(self) -> bool {
        matches!(self, Self::Gather | Self::BuildAndGather)
    }
}

/// Exact pre-contribution facts resolved from the addressed wall/build and builder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreflightInput {
    pub builder: ObjectKey,
    pub target_order: ObjectKey,
    /// The `(who,o)` slot currently holds a valid Wall/Build. Deliberately not UID-aware.
    pub target_is_valid_wall: bool,
    pub target_is_active: bool,
    pub has_next_action_after_retire: bool,
    pub adjacent: bool,
    pub builder_tile_is_covered: bool,
    pub target_is_farm: bool,
    pub order_flags: u8,
    pub builder_x: i32,
    pub builder_y: i32,
    pub target_x: i32,
    pub target_y: i32,
    pub builder_angle: i32,
    /// Raw `UnitData::unit_masks & 1`, named `UNIT_DECOY` by the recovered lane.
    pub unit_decoy: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterInvalidTarget {
    KeepNextAction,
    BuildDone,
}

/// Ordered `Unit::do_build` head plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreflightPlan {
    /// Bare `kill_current_order(0)`, then either keep the next action or call build_done.
    RetireInvalid { then: AfterInvalidTarget },
    /// Bare kill; the active-target tail is selected separately by [`active_target_tail`].
    RetireActive,
    /// Bare kill, temporary one-member Group, then `action_swarm_around` with FIRST.
    Reswarm { preserve_group_flag: bool },
    /// Set Build/Sow animation, optionally face the target, then either contribute or
    /// stop at the measured UNIT_DECOY gate.
    AnimateFace {
        animation: i32,
        set_angle: Option<i32>,
        contribute: bool,
    },
}

/// `Unit::do_build` through the call (or non-call) of `Wall::do_construct`.
///
/// The UID in `target_order` is intentionally ignored. `check_build_order` validates it;
/// direct `do_build` only indexes `(who,o)` and calls `is_valid_wall()`.
pub fn preflight(i: PreflightInput) -> PreflightPlan {
    if i.target_order.o < 0 || i.target_order.who < 0 || !i.target_is_valid_wall {
        return PreflightPlan::RetireInvalid {
            then: if i.has_next_action_after_retire {
                AfterInvalidTarget::KeepNextAction
            } else {
                AfterInvalidTarget::BuildDone
            },
        };
    }
    if i.target_is_active {
        return PreflightPlan::RetireActive;
    }
    if !i.adjacent || (i.builder_tile_is_covered && !i.target_is_farm) {
        return PreflightPlan::Reswarm {
            preserve_group_flag: i.order_flags & ORDER_GROUP != 0,
        };
    }

    let angle = crate::trig::find_angle(
        i.target_x.wrapping_sub(i.builder_x),
        i.target_y.wrapping_sub(i.builder_y),
    );
    PreflightPlan::AnimateFace {
        animation: if i.target_is_farm {
            CHAR_SOW
        } else {
            CHAR_BUILD
        },
        set_angle: (angle != i.builder_angle).then_some(angle),
        contribute: !i.unit_decoy,
    }
}

/// Convert the exact unit-side plan to the construction core's gate after the host has
/// applied the plan's ordered unit/group effects and produced its [`EffectReceipt`](super::construction::EffectReceipt).
pub fn construction_gate(plan: PreflightPlan) -> BuilderGate {
    match plan {
        PreflightPlan::RetireInvalid { .. } => BuilderGate::InvalidTarget,
        PreflightPlan::RetireActive => BuilderGate::TargetAlreadyActive,
        PreflightPlan::Reswarm { .. } => BuilderGate::Reswarmed,
        PreflightPlan::AnimateFace {
            contribute: false, ..
        } => BuilderGate::UnitDecoy,
        PreflightPlan::AnimateFace {
            contribute: true, ..
        } => BuilderGate::Ready,
    }
}

/// Inputs shared by active-target and post-completion auto-gather decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoGatherInput {
    pub builder_who: i32,
    pub target_who: i32,
    pub ai_controlled: bool,
    pub stance: WorkerStance,
    pub target_is_oil_platform: bool,
    pub target_is_gather_type: bool,
    pub target_is_university: bool,
}

impl AutoGatherInput {
    fn fast_path(self) -> bool {
        (self.target_is_oil_platform || (!self.ai_controlled && self.stance.wants_gather()))
            && self.builder_who == self.target_who
            && self.target_is_gather_type
            && !self.target_is_university
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveTargetTail {
    CheckBuildOrder,
    AddGatherNewDetachGroup,
    BuildDone,
}

/// Tail after `do_build` observes an already-active target and kills BUILD_AT.
pub fn active_target_tail(has_next_action: bool, gather: AutoGatherInput) -> ActiveTargetTail {
    if has_next_action {
        return ActiveTargetTail::CheckBuildOrder;
    }
    if gather.fast_path() {
        ActiveTargetTail::AddGatherNewDetachGroup
    } else {
        ActiveTargetTail::BuildDone
    }
}

/// Facts read after `Wall::do_construct` has activated the site and returned 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletionInput {
    pub has_next_action: bool,
    pub builder_group: i32,
    /// `Group::normalize` then `Group::count(COUNT=1,0,0)` when the special arm reaches it.
    pub normalized_group_count: i32,
    pub gather: AutoGatherInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionTail {
    CheckBuildOrder,
    AddGatherNewDetachGroup,
    BuildDone,
}

/// Completion policy after activation has returned and BUILD_AT has been bare-killed.
pub fn completion_tail(i: CompletionInput) -> CompletionTail {
    if i.has_next_action
        && (!i.gather.target_is_gather_type
            || i.gather.target_is_university
            || i.builder_group < 0
            || i.normalized_group_count <= 1)
    {
        return CompletionTail::CheckBuildOrder;
    }
    if i.gather.fast_path() {
        return CompletionTail::AddGatherNewDetachGroup;
    }
    if i.has_next_action {
        CompletionTail::CheckBuildOrder
    } else {
        CompletionTail::BuildDone
    }
}

/// Results of the exact find_* calls, supplied in call order by the host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildDoneSearches {
    pub find_build: bool,
    pub find_repair: bool,
    pub find_gather: bool,
}

/// Final target fallback in `build_done`. Retail does no flags/UID validity check here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildDoneFallback {
    pub ox: i32,
    pub whom: i32,
    pub builder_who: i32,
    pub target_is_gather_type: bool,
    /// Result of `isnt(UNIVERSITY, slow=1) != 0`.
    pub target_isnt_university: bool,
    /// Result of `isnt(OIL_PLATFORM, slow=1) != 0`.
    pub target_isnt_oil_platform: bool,
    pub num_gatherers: i32,
    /// Sign-extended `BuildData::gather_max`.
    pub gather_max: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildDonePlan {
    KeepQueuedAction,
    AssignedBuild,
    AssignedRepair,
    AssignedGather { detach_selection_group: bool },
    AddGatherNewFallback,
    Idle,
}

/// `Unit::build_done` `0x00603BF0` policy over exact find_* results.
pub fn build_done_plan(
    real_action_remains: bool,
    ai_controlled: bool,
    wonderwin_is_8: bool,
    stance: WorkerStance,
    searches: BuildDoneSearches,
    fallback: BuildDoneFallback,
) -> BuildDonePlan {
    if real_action_remains {
        return BuildDonePlan::KeepQueuedAction;
    }
    if ai_controlled {
        if searches.find_build {
            return BuildDonePlan::AssignedBuild;
        }
        if searches.find_repair {
            return BuildDonePlan::AssignedRepair;
        }
        if wonderwin_is_8 {
            return BuildDonePlan::Idle;
        }
        return if searches.find_gather {
            BuildDonePlan::AssignedGather {
                detach_selection_group: false,
            }
        } else {
            BuildDonePlan::Idle
        };
    }

    if stance.wants_build() && searches.find_build {
        return BuildDonePlan::AssignedBuild;
    }
    if stance.wants_gather() && searches.find_gather {
        return BuildDonePlan::AssignedGather {
            detach_selection_group: true,
        };
    }
    if stance.wants_build() && searches.find_repair {
        return BuildDonePlan::AssignedRepair;
    }
    if fallback.ox >= 0
        && fallback.whom == fallback.builder_who
        && fallback.target_is_gather_type
        && fallback.target_isnt_university
        && fallback.target_isnt_oil_platform
        && fallback.num_gatherers < fallback.gather_max
    {
        BuildDonePlan::AddGatherNewFallback
    } else {
        BuildDonePlan::Idle
    }
}

/// One mutating `find_*` call made by retail `Unit::build_done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchReceipt {
    pub assigned: bool,
    pub effect: EffectReceipt,
}

/// The final fallback is read only after all applicable mutating searches fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FallbackReceipt {
    pub value: BuildDoneFallback,
    pub effect: EffectReceipt,
}

/// Runtime inputs which are already live when `Unit::build_done` is entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildDoneContext {
    pub builder: ObjectKey,
    pub target_who: i32,
    pub target_o: i32,
    pub real_action_remains: bool,
    pub ai_controlled: bool,
    pub wonderwin_is_8: bool,
    pub stance: WorkerStance,
    /// The caller's third `build_done` argument. The measured do_build paths pass
    /// `Constants[0x28] * 192`.
    pub gather_radius: i32,
}

/// Mandatory mutating half of the exact `build_done` policy.
///
/// `find_build_spot`, `find_repair_spot`, and `find_gather_spot` install orders before
/// returning true. Consequently they cannot be precomputed without changing behavior;
/// [`execute_build_done`] invokes them only when retail reaches that branch.
pub trait BuildDoneHost {
    type Error;

    fn find_build_spot(&mut self, builder: ObjectKey) -> Result<SearchReceipt, Self::Error>;

    fn find_repair_spot(&mut self, builder: ObjectKey) -> Result<SearchReceipt, Self::Error>;

    fn find_gather_spot(
        &mut self,
        builder: ObjectKey,
        radius: i32,
        arg1: i32,
        arg2: i32,
    ) -> Result<SearchReceipt, Self::Error>;

    /// Resolve the addressed Build without adding a flags or UID gate. This callback is
    /// reached only after `target_o >= 0 && target_who == builder.who`.
    fn final_gather_fallback(
        &mut self,
        builder: ObjectKey,
        target_who: i32,
        target_o: i32,
    ) -> Result<FallbackReceipt, Self::Error>;

    fn add_gather_order_new(
        &mut self,
        builder: ObjectKey,
        target_who: i32,
        target_o: i32,
    ) -> Result<EffectReceipt, Self::Error>;

    /// The successful non-AI find-gather and fast auto-gather arms remove the worker
    /// from a multi-member selection group, then write `group=-1`.
    fn detach_selection_group(&mut self, builder: ObjectKey) -> Result<EffectReceipt, Self::Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildDoneReceipt {
    pub plan: BuildDonePlan,
    pub effect: EffectReceipt,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BuildDoneError<E> {
    Host(E),
    UnexpectedRng { operation: &'static str, draws: u32 },
}

/// Execute `Unit::build_done` with retail short-circuiting and call order.
pub fn execute_build_done<H: BuildDoneHost>(
    host: &mut H,
    ctx: BuildDoneContext,
) -> Result<BuildDoneReceipt, BuildDoneError<H::Error>> {
    let mut effect = EffectReceipt {
        rng_draws: 0,
        checksums: ChecksumEffects::NONE,
    };
    if ctx.real_action_remains {
        return Ok(done(BuildDonePlan::KeepQueuedAction, effect));
    }

    if ctx.ai_controlled {
        if call_search(
            &mut effect,
            "find_build_spot",
            host.find_build_spot(ctx.builder)
                .map_err(BuildDoneError::Host)?,
        )? {
            return Ok(done(BuildDonePlan::AssignedBuild, effect));
        }
        if call_search(
            &mut effect,
            "find_repair_spot",
            host.find_repair_spot(ctx.builder)
                .map_err(BuildDoneError::Host)?,
        )? {
            return Ok(done(BuildDonePlan::AssignedRepair, effect));
        }
        if ctx.wonderwin_is_8 {
            return Ok(done(BuildDonePlan::Idle, effect));
        }
        let assigned = call_search(
            &mut effect,
            "find_gather_spot",
            host.find_gather_spot(ctx.builder, ctx.gather_radius, 0, 0)
                .map_err(BuildDoneError::Host)?,
        )?;
        return Ok(done(
            if assigned {
                BuildDonePlan::AssignedGather {
                    detach_selection_group: false,
                }
            } else {
                BuildDonePlan::Idle
            },
            effect,
        ));
    }

    if ctx.stance.wants_build()
        && call_search(
            &mut effect,
            "find_build_spot",
            host.find_build_spot(ctx.builder)
                .map_err(BuildDoneError::Host)?,
        )?
    {
        return Ok(done(BuildDonePlan::AssignedBuild, effect));
    }
    if ctx.stance.wants_gather()
        && call_search(
            &mut effect,
            "find_gather_spot",
            host.find_gather_spot(ctx.builder, ctx.gather_radius, 0, 0)
                .map_err(BuildDoneError::Host)?,
        )?
    {
        add_builder_effect(
            &mut effect,
            "detach_selection_group",
            host.detach_selection_group(ctx.builder)
                .map_err(BuildDoneError::Host)?,
        )?;
        return Ok(done(
            BuildDonePlan::AssignedGather {
                detach_selection_group: true,
            },
            effect,
        ));
    }
    if ctx.stance.wants_build()
        && call_search(
            &mut effect,
            "find_repair_spot",
            host.find_repair_spot(ctx.builder)
                .map_err(BuildDoneError::Host)?,
        )?
    {
        return Ok(done(BuildDonePlan::AssignedRepair, effect));
    }

    if ctx.target_o >= 0 && ctx.target_who == ctx.builder.who {
        let fallback = host
            .final_gather_fallback(ctx.builder, ctx.target_who, ctx.target_o)
            .map_err(BuildDoneError::Host)?;
        add_builder_effect(&mut effect, "final_gather_fallback", fallback.effect)?;
        let f = fallback.value;
        if f.target_is_gather_type
            && f.target_isnt_university
            && f.target_isnt_oil_platform
            && f.num_gatherers < f.gather_max
        {
            add_builder_effect(
                &mut effect,
                "add_gather_order_new",
                host.add_gather_order_new(ctx.builder, ctx.target_who, ctx.target_o)
                    .map_err(BuildDoneError::Host)?,
            )?;
            return Ok(done(BuildDonePlan::AddGatherNewFallback, effect));
        }
    }
    Ok(done(BuildDonePlan::Idle, effect))
}

fn done(plan: BuildDonePlan, effect: EffectReceipt) -> BuildDoneReceipt {
    BuildDoneReceipt { plan, effect }
}

fn call_search<E>(
    total: &mut EffectReceipt,
    operation: &'static str,
    receipt: SearchReceipt,
) -> Result<bool, BuildDoneError<E>> {
    add_builder_effect(total, operation, receipt.effect)?;
    Ok(receipt.assigned)
}

fn add_builder_effect<E>(
    total: &mut EffectReceipt,
    operation: &'static str,
    effect: EffectReceipt,
) -> Result<(), BuildDoneError<E>> {
    if effect.rng_draws != 0 {
        return Err(BuildDoneError::UnexpectedRng {
            operation,
            draws: effect.rng_draws,
        });
    }
    total.checksums = total.checksums.union(effect.checksums);
    Ok(())
}

/// One nearby unit's current non-move action for `check_build_order` balancing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbyBuildAction {
    pub unit: ObjectKey,
    pub action_kind: OrderIndex,
    pub target_who: i32,
    pub target_o: i32,
}

/// Deterministic candidate ordering used by `Unit::check_build_order`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateInstallPlan {
    /// Candidate selected by strict first-minimum contention.
    pub winner: i32,
    /// Actual call order for repeated `add_build_order(..., FIRST, group=1)`.
    pub first_install_calls: Vec<i32>,
    /// Observable BUILD_AT execution order after those front insertions.
    pub resulting_execution_order: Vec<i32>,
    pub contender_counts: Vec<i32>,
}

/// Balance candidate sites and produce the reverse-FIRST reconstruction.
///
/// Counts compare only `(builder.who, candidate_o)` and intentionally ignore target UID.
/// The scanning builder itself is excluded. Ties retain the earliest candidate.
pub fn balance_candidates(
    builder: ObjectKey,
    candidates: &[i32],
    nearby: &[NearbyBuildAction],
) -> Option<CandidateInstallPlan> {
    if candidates.is_empty() {
        return None;
    }
    let counts: Vec<i32> = candidates
        .iter()
        .map(|&candidate| {
            nearby
                .iter()
                .filter(|n| {
                    n.unit.who == builder.who
                        && n.unit.o != builder.o
                        && n.action_kind == OrderIndex::BuildAt
                        && n.target_who == builder.who
                        && n.target_o == candidate
                })
                .count() as i32
        })
        .collect();
    let mut winner_index = 0usize;
    let mut minimum = counts[0];
    for (i, &count) in counts.iter().enumerate().skip(1) {
        if count < minimum {
            winner_index = i;
            minimum = count;
        }
    }

    let mut execution = candidates.to_vec();
    execution.swap(0, winner_index);
    let mut calls = execution.clone();
    calls.reverse();
    Some(CandidateInstallPlan {
        winner: execution[0],
        first_install_calls: calls,
        resulting_execution_order: execution,
        contender_counts: counts,
    })
}

/// UID-aware target gate used by `check_build_order`, separate from direct do_build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckBuildTarget {
    pub valid: bool,
    pub actual_uid: u16,
    pub active: bool,
    pub flags_0x20: bool,
    pub worker_stance_nonzero: bool,
    pub caster_stance_nonzero: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckBuildTargetPlan {
    RetireStaleOrActive,
    CandidateAndStop,
    CandidateAndContinue,
}

pub fn check_build_target(order: &OrderRec, target: CheckBuildTarget) -> CheckBuildTargetPlan {
    debug_assert_eq!(order.kind, OrderIndex::BuildAt);
    if order.target_o < 0
        || order.target_who < 0
        || !target.valid
        || target.actual_uid != order.target_uid
        || target.active
    {
        return CheckBuildTargetPlan::RetireStaleOrActive;
    }
    if target.flags_0x20 || target.worker_stance_nonzero || target.caster_stance_nonzero {
        CheckBuildTargetPlan::CandidateAndStop
    } else {
        CheckBuildTargetPlan::CandidateAndContinue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUILDER: ObjectKey = ObjectKey {
        who: 2,
        o: 4,
        uid: 10,
    };
    const TARGET: ObjectKey = ObjectKey {
        who: 2,
        o: 2001,
        uid: 20,
    };

    fn pre() -> PreflightInput {
        PreflightInput {
            builder: BUILDER,
            target_order: TARGET,
            target_is_valid_wall: true,
            target_is_active: false,
            has_next_action_after_retire: false,
            adjacent: true,
            builder_tile_is_covered: false,
            target_is_farm: false,
            order_flags: 0,
            builder_x: 100,
            builder_y: 100,
            target_x: 200,
            target_y: 100,
            builder_angle: 0,
            unit_decoy: false,
        }
    }

    #[test]
    fn preflight_ignores_uid_but_rejects_address_or_wall_failure() {
        let mut i = pre();
        i.target_order.uid ^= 1;
        assert!(matches!(
            preflight(i),
            PreflightPlan::AnimateFace {
                contribute: true,
                ..
            }
        ));
        i.target_order.o = -1;
        assert_eq!(
            preflight(i),
            PreflightPlan::RetireInvalid {
                then: AfterInvalidTarget::BuildDone
            }
        );
    }

    #[test]
    fn reswarm_preserves_group_and_farm_is_the_footprint_exception() {
        let mut i = pre();
        i.builder_tile_is_covered = true;
        i.order_flags = ORDER_GROUP;
        assert_eq!(
            preflight(i),
            PreflightPlan::Reswarm {
                preserve_group_flag: true
            }
        );
        i.target_is_farm = true;
        assert!(matches!(
            preflight(i),
            PreflightPlan::AnimateFace {
                animation: CHAR_SOW,
                ..
            }
        ));
    }

    #[test]
    fn animation_and_facing_happen_before_decoy_suppresses_work() {
        let mut i = pre();
        i.unit_decoy = true;
        let plan = preflight(i);
        assert_eq!(
            plan,
            PreflightPlan::AnimateFace {
                animation: CHAR_BUILD,
                set_angle: Some(crate::trig::find_angle(100, 0)),
                contribute: false,
            }
        );
        assert_eq!(construction_gate(plan), BuilderGate::UnitDecoy);
    }

    fn gather() -> AutoGatherInput {
        AutoGatherInput {
            builder_who: 2,
            target_who: 2,
            ai_controlled: false,
            stance: WorkerStance::Gather,
            target_is_oil_platform: false,
            target_is_gather_type: true,
            target_is_university: false,
        }
    }

    #[test]
    fn active_target_with_next_action_never_auto_gathers() {
        assert_eq!(
            active_target_tail(true, gather()),
            ActiveTargetTail::CheckBuildOrder
        );
        assert_eq!(
            active_target_tail(false, gather()),
            ActiveTargetTail::AddGatherNewDetachGroup
        );
    }

    #[test]
    fn completion_group_special_requires_more_than_one_member() {
        for count in [0, 1] {
            assert_eq!(
                completion_tail(CompletionInput {
                    has_next_action: true,
                    builder_group: 3,
                    normalized_group_count: count,
                    gather: gather(),
                }),
                CompletionTail::CheckBuildOrder
            );
        }
        assert_eq!(
            completion_tail(CompletionInput {
                has_next_action: true,
                builder_group: 3,
                normalized_group_count: 2,
                gather: gather(),
            }),
            CompletionTail::AddGatherNewDetachGroup
        );
    }

    fn fallback() -> BuildDoneFallback {
        BuildDoneFallback {
            ox: 2001,
            whom: 2,
            builder_who: 2,
            target_is_gather_type: true,
            target_isnt_university: true,
            target_isnt_oil_platform: true,
            num_gatherers: 3,
            gather_max: 4,
        }
    }

    #[test]
    fn build_done_non_ai_order_is_build_gather_repair_then_fallback() {
        let all = BuildDoneSearches {
            find_build: true,
            find_repair: true,
            find_gather: true,
        };
        assert_eq!(
            build_done_plan(
                false,
                false,
                false,
                WorkerStance::BuildAndGather,
                all,
                fallback(),
            ),
            BuildDonePlan::AssignedBuild
        );
        assert_eq!(
            build_done_plan(false, false, false, WorkerStance::Gather, all, fallback(),),
            BuildDonePlan::AssignedGather {
                detach_selection_group: true
            }
        );
    }

    #[test]
    fn build_done_ai_obeys_wonderwin_gather_suppression() {
        let searches = BuildDoneSearches {
            find_build: false,
            find_repair: false,
            find_gather: true,
        };
        assert_eq!(
            build_done_plan(
                false,
                true,
                true,
                WorkerStance::Gather,
                searches,
                fallback(),
            ),
            BuildDonePlan::Idle
        );
        assert_eq!(
            build_done_plan(
                false,
                true,
                false,
                WorkerStance::Gather,
                searches,
                fallback(),
            ),
            BuildDonePlan::AssignedGather {
                detach_selection_group: false
            }
        );
    }

    #[test]
    fn final_gather_fallback_is_strict_and_excludes_university_and_oil() {
        let none = BuildDoneSearches::default();
        let mut f = fallback();
        assert_eq!(
            build_done_plan(false, false, false, WorkerStance::Other(9), none, f),
            BuildDonePlan::AddGatherNewFallback
        );
        f.num_gatherers = f.gather_max;
        assert_eq!(
            build_done_plan(false, false, false, WorkerStance::Other(9), none, f),
            BuildDonePlan::Idle
        );
        f = fallback();
        f.target_isnt_oil_platform = false;
        assert_eq!(
            build_done_plan(false, false, false, WorkerStance::Other(9), none, f),
            BuildDonePlan::Idle
        );
    }

    #[test]
    fn candidate_balancing_uses_strict_first_tie_and_reverse_first_calls() {
        let nearby = [
            NearbyBuildAction {
                unit: ObjectKey {
                    who: 2,
                    o: 10,
                    uid: 1,
                },
                action_kind: OrderIndex::BuildAt,
                target_who: 2,
                target_o: 101,
            },
            NearbyBuildAction {
                unit: ObjectKey {
                    who: 2,
                    o: 11,
                    uid: 2,
                },
                action_kind: OrderIndex::BuildAt,
                target_who: 2,
                target_o: 102,
            },
            NearbyBuildAction {
                unit: ObjectKey {
                    who: 2,
                    o: 12,
                    uid: 3,
                },
                action_kind: OrderIndex::BuildAt,
                target_who: 2,
                target_o: 102,
            },
        ];
        let p = balance_candidates(BUILDER, &[101, 102, 103], &nearby).unwrap();
        assert_eq!(p.contender_counts, vec![1, 2, 0]);
        assert_eq!(p.winner, 103);
        assert_eq!(p.resulting_execution_order, vec![103, 102, 101]);
        assert_eq!(p.first_install_calls, vec![101, 102, 103]);

        let tie = balance_candidates(BUILDER, &[8, 9], &[]).unwrap();
        assert_eq!(tie.winner, 8);
    }

    #[test]
    fn check_build_order_is_uid_aware_and_stance_stops_scan() {
        let mut o = OrderRec::of_kind(OrderIndex::BuildAt);
        o.target_who = 2;
        o.target_o = 2001;
        o.target_uid = 44;
        let mut t = CheckBuildTarget {
            valid: true,
            actual_uid: 45,
            active: false,
            flags_0x20: false,
            worker_stance_nonzero: false,
            caster_stance_nonzero: false,
        };
        assert_eq!(
            check_build_target(&o, t),
            CheckBuildTargetPlan::RetireStaleOrActive
        );
        t.actual_uid = 44;
        assert_eq!(
            check_build_target(&o, t),
            CheckBuildTargetPlan::CandidateAndContinue
        );
        t.worker_stance_nonzero = true;
        assert_eq!(
            check_build_target(&o, t),
            CheckBuildTargetPlan::CandidateAndStop
        );
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum DoneCall {
        Build,
        Repair,
        Gather(i32),
        Fallback,
        AddGather,
        Detach,
    }

    struct DoneHost {
        build: bool,
        repair: bool,
        gather: bool,
        fallback: BuildDoneFallback,
        calls: Vec<DoneCall>,
    }

    fn no_effect() -> EffectReceipt {
        EffectReceipt {
            rng_draws: 0,
            checksums: ChecksumEffects::NONE,
        }
    }

    impl BuildDoneHost for DoneHost {
        type Error = &'static str;

        fn find_build_spot(&mut self, _builder: ObjectKey) -> Result<SearchReceipt, Self::Error> {
            self.calls.push(DoneCall::Build);
            Ok(SearchReceipt {
                assigned: self.build,
                effect: no_effect(),
            })
        }

        fn find_repair_spot(&mut self, _builder: ObjectKey) -> Result<SearchReceipt, Self::Error> {
            self.calls.push(DoneCall::Repair);
            Ok(SearchReceipt {
                assigned: self.repair,
                effect: no_effect(),
            })
        }

        fn find_gather_spot(
            &mut self,
            _builder: ObjectKey,
            radius: i32,
            arg1: i32,
            arg2: i32,
        ) -> Result<SearchReceipt, Self::Error> {
            assert_eq!((arg1, arg2), (0, 0));
            self.calls.push(DoneCall::Gather(radius));
            Ok(SearchReceipt {
                assigned: self.gather,
                effect: no_effect(),
            })
        }

        fn final_gather_fallback(
            &mut self,
            _builder: ObjectKey,
            target_who: i32,
            target_o: i32,
        ) -> Result<FallbackReceipt, Self::Error> {
            assert_eq!((target_who, target_o), (TARGET.who, TARGET.o));
            self.calls.push(DoneCall::Fallback);
            Ok(FallbackReceipt {
                value: self.fallback,
                effect: no_effect(),
            })
        }

        fn add_gather_order_new(
            &mut self,
            _builder: ObjectKey,
            target_who: i32,
            target_o: i32,
        ) -> Result<EffectReceipt, Self::Error> {
            assert_eq!((target_who, target_o), (TARGET.who, TARGET.o));
            self.calls.push(DoneCall::AddGather);
            Ok(no_effect())
        }

        fn detach_selection_group(
            &mut self,
            _builder: ObjectKey,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(DoneCall::Detach);
            Ok(no_effect())
        }
    }

    fn done_host() -> DoneHost {
        DoneHost {
            build: false,
            repair: false,
            gather: false,
            fallback: fallback(),
            calls: Vec::new(),
        }
    }

    fn done_context(ai_controlled: bool, stance: WorkerStance) -> BuildDoneContext {
        BuildDoneContext {
            builder: BUILDER,
            target_who: TARGET.who,
            target_o: TARGET.o,
            real_action_remains: false,
            ai_controlled,
            wonderwin_is_8: false,
            stance,
            gather_radius: 0x480,
        }
    }

    #[test]
    fn executable_build_done_short_circuits_mutating_searches() {
        let mut host = done_host();
        host.build = true;
        let receipt =
            execute_build_done(&mut host, done_context(true, WorkerStance::BuildAndGather))
                .unwrap();
        assert_eq!(receipt.plan, BuildDonePlan::AssignedBuild);
        assert_eq!(host.calls, [DoneCall::Build]);

        let mut host = done_host();
        host.gather = true;
        let receipt =
            execute_build_done(&mut host, done_context(false, WorkerStance::BuildAndGather))
                .unwrap();
        assert_eq!(
            receipt.plan,
            BuildDonePlan::AssignedGather {
                detach_selection_group: true
            }
        );
        assert_eq!(
            host.calls,
            [DoneCall::Build, DoneCall::Gather(0x480), DoneCall::Detach]
        );
    }

    #[test]
    fn executable_build_done_reaches_fallback_only_after_searches_fail() {
        let mut host = done_host();
        let receipt =
            execute_build_done(&mut host, done_context(false, WorkerStance::Other(9))).unwrap();
        assert_eq!(receipt.plan, BuildDonePlan::AddGatherNewFallback);
        assert_eq!(host.calls, [DoneCall::Fallback, DoneCall::AddGather]);

        let mut host = done_host();
        let mut ctx = done_context(true, WorkerStance::Gather);
        ctx.wonderwin_is_8 = true;
        let receipt = execute_build_done(&mut host, ctx).unwrap();
        assert_eq!(receipt.plan, BuildDonePlan::Idle);
        assert_eq!(host.calls, [DoneCall::Build, DoneCall::Repair]);
    }
}
