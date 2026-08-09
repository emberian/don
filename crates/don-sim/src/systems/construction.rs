//! Authoritative building-construction lifecycle.
//!
//! This module is the seam between a unit executing `BUILD_AT` and the recovered
//! arithmetic in [`super::production`].  It exists because a counter named
//! `build_left` cannot represent retail construction: the site is started lazily, the
//! first contributor in a frame bumps a walked latch, subsequent contributors receive
//! harmonic diminishing returns, and completion enters the very large `Build::activate`
//! transaction before the builder's order is retired.
//!
//! The local state transitions below are instruction-derived from:
//!
//! - `Unit::add_build_order` `0x005E5210`;
//! - `Unit::do_build` `0x005EEBF0`;
//! - `Unit::check_build_order` `0x00603470`;
//! - `Unit::build_done` `0x00603BF0`;
//! - `Wall::do_construct` `0x006434D0`;
//! - `Wall::process` `0x00640450`.
//!
//! `BuildTypeData::blocked_site`, `Build::start`, `Build::activate`, `Object::disband`
//! and the post-build unit reassignment path touch stores which a compact simulation may
//! not own.  They are mandatory callbacks, not optional no-ops.  A runtime which cannot
//! provide them gets an error before the corresponding transition can masquerade as
//! retail fidelity.

use super::order_dispatch::{clear_partial_path, update_action, OrderRec, UnitWork};
use super::production::{self, BuildData, ConstructQueryGates, ProdRules, SiteVerdict};
use crate::command::QueuePos;
use crate::order::{OrderIndex, ORDER_GROUP};

/// Full runtime fidelity is deliberately closed while the mandatory effect adapter still
/// has unimplemented retail bodies.  The pure state machine is usable only with an
/// adapter which supplies every callback in [`ConstructionEffects`].
pub const RUNTIME_FIDELITY_READY: bool = false;

/// Raw `UnitData::unit_masks` bits written by `Unit::add_build_order`.
pub const UNIT_BUILDER: u32 = 0x0000_0400;
pub const UNIT_CAN_TRANSPORT: u32 = 0x0080_0000;
pub const UNIT_MULTIMOVE: u32 = 0x0400_0000;

/// One lockstep object identity.  `uid` is the reuse token copied into `TargetOrder` by
/// `Unit::add_build_order`; `(who, o)` alone is not enough after a pool slot is recycled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectKey {
    pub who: i32,
    pub o: i32,
    pub uid: u16,
}

/// Exact target payload installed by `Unit::add_build_order` `0x005E5210`.
///
/// Queue insertion, path clearing, region-crossing flags and `update_action` remain the
/// order subsystem's transaction.  This value pins the part construction owns: order
/// kind 6 plus the target identity triple.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildOrderTarget {
    pub order_index: u8,
    pub target: ObjectKey,
    pub group_flag: bool,
}

impl BuildOrderTarget {
    pub const BUILD_AT: u8 = 6;

    pub fn new(target: ObjectKey, group_flag: bool) -> Self {
        Self {
            order_index: Self::BUILD_AT,
            target,
            group_flag,
        }
    }

    /// Snapshot the UID exactly as `Unit::add_build_order` does. Negative target indices
    /// store `0xFFFF`; valid indices require the caller's coherent object-table UID.
    pub fn from_indices(
        target_o: i32,
        target_who: i32,
        live_uid: Option<u16>,
        group_flag: bool,
    ) -> Self {
        let uid = if target_o < 0 || target_who < 0 {
            0xFFFF
        } else {
            live_uid.expect("valid build-order target requires a coherent UID snapshot")
        };
        Self::new(
            ObjectKey {
                who: target_who,
                o: target_o,
                uid,
            },
            group_flag,
        )
    }
}

/// Host-resolved inputs to the cross-region transport arm of
/// `Unit::add_build_order`. No terrain/region approximation is made in this module.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildOrderRegions {
    pub builder_region: i32,
    pub target_region: i32,
    /// Result of `Unit::transport_type()`.
    pub builder_transport_type: i32,
    /// Result of `Unit::can_ever_transport()`.
    pub can_ever_transport: bool,
    /// Raw `LeaderData::flags` used by the measured threshold expression.
    pub leader_flags: u32,
}

/// Observable result of [`install_build_order`].  The installer consumes no RNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildOrderInstallReceipt {
    pub target: BuildOrderTarget,
    pub queue: QueuePos,
    pub unit_masks_after: u32,
    pub rng_draws: u32,
}

/// `Unit::add_build_order` `0x005E5210`, including all three `QueuePos` cases and the
/// cross-region transport latch.
///
/// `QUEUE_FIRST` is implemented as an equivalent front insertion: retail appends the new
/// circular-list node and rotates `head = head->next`, making that same node the front.
pub fn install_build_order(
    unit: &mut UnitWork,
    target: ObjectKey,
    queue: QueuePos,
    group: bool,
    regions: BuildOrderRegions,
) -> BuildOrderInstallReceipt {
    if queue == QueuePos::New {
        unit.unit_masks &= !UNIT_MULTIMOVE;
        unit.path.clear();
        unit.orders.clear();
        clear_partial_path(unit);
        update_action(unit);
    }

    unit.unit_masks |= UNIT_BUILDER;
    let target_order = BuildOrderTarget::new(target, group);
    let mut order = OrderRec::of_kind(OrderIndex::BuildAt);
    order.target_o = target.o;
    order.target_who = target.who;
    order.target_uid = target.uid;
    if group {
        order.flags |= ORDER_GROUP;
    } else {
        order.flags &= !ORDER_GROUP;
    }

    if regions.builder_region != regions.target_region {
        let threshold = if regions.leader_flags & 0x100 != 0 {
            3
        } else if regions.leader_flags & 0x200 != 0 {
            2
        } else {
            ((regions.leader_flags >> 10) & 1) as i32
        };
        if regions.builder_transport_type <= threshold && regions.can_ever_transport {
            unit.unit_masks |= UNIT_CAN_TRANSPORT;
        }
    }

    if queue == QueuePos::First {
        clear_partial_path(unit);
        unit.orders.push_front(order);
    } else {
        unit.orders.push_back(order);
    }
    update_action(unit);

    BuildOrderInstallReceipt {
        target: target_order,
        queue,
        unit_masks_after: unit.unit_masks,
        rng_draws: 0,
    }
}

/// The measured gates in `Unit::do_build`, resolved by the host against its real unit,
/// object, order, movement and map stores.
///
/// These variants intentionally name control-flow sites rather than inventing high-level
/// meanings for the large `Unit::check_build_order` body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuilderGate {
    /// Target indices were negative or the addressed object failed `is_valid_wall()`.
    /// `Unit::do_build` deliberately does **not** compare the stored order UID here.
    InvalidTarget,
    /// Target virtual `+0x4C` reported active before `do_construct` was entered.
    TargetAlreadyActive,
    /// Builder is not adjacent, or stands inside a non-Farm footprint. Retail kills the
    /// current order, constructs a one-member temporary group, then `action_swarm_around`
    /// with `QUEUE_FIRST`, `BUILD_AT`, preserving the order's group flag.
    Reswarmed,
    /// Raw `unit_masks & 1` (`UNIT_DECOY`) remained set after animation/facing setup.
    /// The order stays installed but this activation contributes no work.
    UnitDecoy,
    /// The builder reached the `Wall::do_construct` call.
    Ready,
}

/// Builder-gate result plus all movement/animation/group mutations and transitive RNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuilderGateReceipt {
    pub gate: BuilderGate,
    pub effect: EffectReceipt,
}

/// Why the order leaves or is externally interrupted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuilderFinish {
    InvalidTarget,
    TargetAlreadyActive,
    SiteCompleted,
    BuilderDied,
    OrderCancelled,
}

/// Checksum stores touched by a transitive callback.
///
/// Local construction progress always adds `builds`; callbacks union the stores they
/// actually changed.  The booleans are explicit so an adapter cannot hide broad
/// activation or disband effects behind a single vague "dirty" bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChecksumEffects {
    pub builds: bool,
    pub units: bool,
    pub guys: bool,
    pub leaders: bool,
    pub cities: bool,
    pub groups: bool,
    pub world: bool,
    pub objects_other: bool,
}

impl ChecksumEffects {
    pub const NONE: Self = Self {
        builds: false,
        units: false,
        guys: false,
        leaders: false,
        cities: false,
        groups: false,
        world: false,
        objects_other: false,
    };

    pub const BUILDS: Self = Self {
        builds: true,
        ..Self::NONE
    };

    pub fn union(self, rhs: Self) -> Self {
        Self {
            builds: self.builds || rhs.builds,
            units: self.units || rhs.units,
            guys: self.guys || rhs.guys,
            leaders: self.leaders || rhs.leaders,
            cities: self.cities || rhs.cities,
            groups: self.groups || rhs.groups,
            world: self.world || rhs.world,
            objects_other: self.objects_other || rhs.objects_other,
        }
    }
}

/// Mandatory accounting returned by every transitive effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectReceipt {
    /// Complete number of draws consumed from the simulation RNG by the callback and all
    /// of its callees.  The local wrapper itself consumes none.
    pub rng_draws: u32,
    pub checksums: ChecksumEffects,
}

/// Channel/RNG snapshot required from a retail oracle capture around one construction
/// transaction. Values are the isolated channel accumulators, not a synthetic combined
/// digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstructionOracleSnapshot {
    pub rng_state: i32,
    pub site_image: [u8; production::BUILDDATA_SIZE],
    pub builder: ObjectKey,
    pub order_target: ObjectKey,
    pub builder_unit_masks: u32,
    pub builder_angle: i32,
    pub builder_group: i32,
    pub builds: u32,
    pub units: u32,
    pub guys: u32,
    pub leaders: u32,
    pub cities: u32,
    pub groups: u32,
    pub world: u32,
    pub objects_other: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleReceiptError {
    RngState {
        expected: i32,
        observed: i32,
    },
    ChecksumEffects {
        expected: ChecksumEffects,
        observed: ChecksumEffects,
    },
}

/// Verify one effect receipt against coherent before/after retail snapshots.
///
/// This is intentionally strict. A capture which cannot expose each channel separately or
/// cannot bracket the shared RNG seed is not a construction oracle and must not promote
/// the fidelity tier.
pub fn verify_oracle_receipt(
    before: ConstructionOracleSnapshot,
    after: ConstructionOracleSnapshot,
    receipt: EffectReceipt,
) -> Result<(), OracleReceiptError> {
    let mut expected_rng = before.rng_state;
    for _ in 0..receipt.rng_draws {
        expected_rng = expected_rng
            .wrapping_mul(crate::rng::Random::MUL)
            .wrapping_add(crate::rng::Random::ADD);
    }
    if after.rng_state != expected_rng {
        return Err(OracleReceiptError::RngState {
            expected: expected_rng,
            observed: after.rng_state,
        });
    }
    let observed = ChecksumEffects {
        builds: before.builds != after.builds,
        units: before.units != after.units,
        guys: before.guys != after.guys,
        leaders: before.leaders != after.leaders,
        cities: before.cities != after.cities,
        groups: before.groups != after.groups,
        world: before.world != after.world,
        objects_other: before.objects_other != after.objects_other,
    };
    if observed != receipt.checksums {
        return Err(OracleReceiptError::ChecksumEffects {
            expected: receipt.checksums,
            observed,
        });
    }
    Ok(())
}

/// Result of the exact `blocked_site` query and the `0x2A` linked-city wonder-capacity
/// dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiteCheckReceipt {
    pub raw_code: i32,
    /// `CityData::num_wonders(1) <= 1 + LeaderData::has_tribe_bonus(7)`. Consulted only
    /// for raw code `0x2A`, and only when `BuildData::city >= 0`.
    pub linked_city_wonder_capacity_allows: bool,
    pub effect: EffectReceipt,
}

/// A lifecycle callback must report the site's post-call flags.  This lets the core
/// reject an adapter which claims to have started/activated a building but did not apply
/// the corresponding retail state bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiteLifecycleReceipt {
    pub flags_after: u8,
    pub effect: EffectReceipt,
}

/// Inputs which are global/player/type queries in retail rather than site fields.
#[derive(Clone, Copy, Debug, Default)]
pub struct BuilderContribution {
    pub ai_speed: i32,
    pub korean_build_under_fire_bonus: bool,
    pub construct_query: ConstructQueryGates,
}

/// Inputs to the frame-head building process.  This must run before any builders in that
/// frame, just as building-band processing precedes the rotated unit band.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstructionFrame {
    pub construct_query: ConstructQueryGates,
    pub is_wonder: bool,
}

/// Observable result of one builder's `Unit::do_build` arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildOutcome {
    OrderRetired(BuilderFinish),
    NoContribution(BuilderGate),
    /// The unstarted site failed admission and entered `Object::disband(1)`.
    SiteRejected,
    Progressed {
        credited: u32,
        started_this_call: bool,
    },
    Completed {
        credited: u32,
        started_this_call: bool,
    },
}

/// Full accounting for [`execute_builder`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildReceipt {
    pub outcome: BuildOutcome,
    pub rng_draws: u32,
    pub checksums: ChecksumEffects,
}

/// Fail-loud structural errors.  Callback failures retain their concrete host error.
#[derive(Debug, PartialEq, Eq)]
pub enum ConstructionError<E> {
    Effect(E),
    SiteIdentityMismatch {
        expected_who: i32,
        actual_who: u8,
        expected_uid: u16,
        actual_uid: u16,
    },
    TargetAddressMismatch {
        order_who: i32,
        order_o: i32,
        actual_who: i32,
        actual_o: i32,
    },
    ReadySiteIsNotValid,
    LifecycleFlagsMismatch {
        flags_after: u8,
        site_flags: u8,
    },
    StartDidNotSetStarted {
        flags_after: u8,
    },
    ActivateDidNotSetActive {
        flags_after: u8,
    },
    ActivateDidNotApplyLocalCore {
        job_counter: u32,
        job_counter_2: u32,
        recharging: i16,
        build_masks: u16,
    },
}

/// Host transaction boundary for the large object/world-dependent retail bodies.
///
/// None of these methods has a default.  In particular, a headless host may not answer
/// `blocked_site` with `0`, or `activate_site` with a flags-only shortcut, and call that
/// fidelity.  An extractor-backed or fully ported implementation must perform the real
/// transaction and report its transitive RNG/checksum effects.
pub trait ConstructionEffects {
    type Error;

    fn builder_gate(
        &mut self,
        builder: ObjectKey,
        target: ObjectKey,
    ) -> Result<BuilderGateReceipt, Self::Error>;

    fn blocked_site(
        &mut self,
        site_key: ObjectKey,
        site: &BuildData,
    ) -> Result<SiteCheckReceipt, Self::Error>;

    fn start_site(
        &mut self,
        site_key: ObjectKey,
        site: &mut BuildData,
        notify: i32,
    ) -> Result<SiteLifecycleReceipt, Self::Error>;

    fn disband_site(
        &mut self,
        site_key: ObjectKey,
        site: &mut BuildData,
        mode: i32,
    ) -> Result<SiteLifecycleReceipt, Self::Error>;

    fn activate_site(
        &mut self,
        site_key: ObjectKey,
        site: &mut BuildData,
        arg0: i32,
        arg1: i32,
        arg2: i32,
    ) -> Result<SiteLifecycleReceipt, Self::Error>;

    /// Retire the current build order and run the exact `build_done` / reassignment tail.
    fn finish_builder(
        &mut self,
        builder: ObjectKey,
        target: ObjectKey,
        reason: BuilderFinish,
    ) -> Result<EffectReceipt, Self::Error>;

    /// Builder death/explicit cancellation transaction. Target death is intentionally
    /// absent: retail does not proactively visit builders; their next `do_build` sees an
    /// invalid/stale target and takes [`BuilderGate::InvalidTarget`].
    fn interrupt_builder(
        &mut self,
        builder: ObjectKey,
        target: ObjectKey,
        reason: BuilderFinish,
    ) -> Result<EffectReceipt, Self::Error>;
}

/// `Wall::process` construction head plus `Wall::update_hits`' progress ramp.
///
/// Call once for every live building before walking builders.  The helper counter is a
/// per-frame accumulator, not persistent membership.
pub fn begin_site_frame(
    site: &mut BuildData,
    frame: ConstructionFrame,
    rules: &ProdRules,
) -> EffectReceipt {
    site.begin_frame_construction();
    let total = production::construct_time(site.constr_time, false, &frame.construct_query, rules);
    site.construct_hits = production::construct_hits(
        site.myhits,
        site.is_active(),
        frame.is_wonder,
        site.job_counter,
        total,
    );
    EffectReceipt {
        rng_draws: 0,
        checksums: ChecksumEffects::BUILDS,
    }
}

/// Execute one builder's construction arm after the scheduler reaches that unit.
///
/// Builder traversal order is load-bearing because [`production::do_construct`] divides
/// by `helpers + 1`.  Call this once per live builder in the same owner/slot order as the
/// object scheduler; do not collect builders into a set or multiply one rate by a count.
pub fn execute_builder<E: ConstructionEffects>(
    site: &mut BuildData,
    site_key: ObjectKey,
    builder: ObjectKey,
    target: BuildOrderTarget,
    contribution: BuilderContribution,
    rules: &ProdRules,
    effects: &mut E,
) -> Result<BuildReceipt, ConstructionError<E::Error>> {
    let gate_receipt = effects
        .builder_gate(builder, target.target)
        .map_err(ConstructionError::Effect)?;
    let gate = gate_receipt.gate;
    let mut rng_draws = gate_receipt.effect.rng_draws;
    let mut checksums = gate_receipt.effect.checksums;

    match gate {
        BuilderGate::InvalidTarget => {
            return finish_without_work(
                effects,
                builder,
                target.target,
                BuilderFinish::InvalidTarget,
                gate_receipt.effect,
            );
        }
        BuilderGate::TargetAlreadyActive => {
            return finish_without_work(
                effects,
                builder,
                target.target,
                BuilderFinish::TargetAlreadyActive,
                gate_receipt.effect,
            );
        }
        BuilderGate::Reswarmed | BuilderGate::UnitDecoy => {
            return Ok(BuildReceipt {
                outcome: BuildOutcome::NoContribution(gate),
                rng_draws,
                checksums,
            });
        }
        BuilderGate::Ready => {}
    }

    if i32::from(site.who) != site_key.who || site.uid != site_key.uid {
        return Err(ConstructionError::SiteIdentityMismatch {
            expected_who: site_key.who,
            actual_who: site.who,
            expected_uid: site_key.uid,
            actual_uid: site.uid,
        });
    }
    if target.target.who != site_key.who || target.target.o != site_key.o {
        return Err(ConstructionError::TargetAddressMismatch {
            order_who: target.target.who,
            order_o: target.target.o,
            actual_who: site_key.who,
            actual_o: site_key.o,
        });
    }
    if !site.is_valid() {
        return Err(ConstructionError::ReadySiteIsNotValid);
    }

    let mut started_this_call = false;
    if !site.is_started() {
        let check = effects
            .blocked_site(site_key, site)
            .map_err(ConstructionError::Effect)?;
        add_effect(&mut rng_draws, &mut checksums, check.effect);
        let accepted = match SiteVerdict::from_code(check.raw_code) {
            SiteVerdict::Ok => true,
            SiteVerdict::OkIfLinkedCityHasWonderCapacity => {
                site.city >= 0 && check.linked_city_wonder_capacity_allows
            }
            SiteVerdict::Blocked => false,
        };
        if accepted {
            let receipt = effects
                .start_site(site_key, site, 1)
                .map_err(ConstructionError::Effect)?;
            add_effect(&mut rng_draws, &mut checksums, receipt.effect);
            validate_lifecycle_flags(site, receipt.flags_after)?;
            if receipt.flags_after & production::flag::STARTED == 0 {
                return Err(ConstructionError::StartDidNotSetStarted {
                    flags_after: receipt.flags_after,
                });
            }
            checksums = checksums.union(ChecksumEffects::BUILDS);
            // Build::start(1) at 0x006435D1 falls through 0x006435DD into the normal
            // inactive progress body in this same do_construct activation.
            started_this_call = true;
        } else {
            let receipt = effects
                .disband_site(site_key, site, 1)
                .map_err(ConstructionError::Effect)?;
            add_effect(&mut rng_draws, &mut checksums, receipt.effect);
            validate_lifecycle_flags(site, receipt.flags_after)?;
            checksums = checksums.union(ChecksumEffects::BUILDS);
            return Ok(BuildReceipt {
                outcome: BuildOutcome::SiteRejected,
                rng_draws,
                checksums,
            });
        }
    }

    // The first builder in a frame increments BuildData::recharging and latches 0x800.
    // `production::do_construct` deliberately owns only the arithmetic, so the lifecycle
    // wrapper applies these two walked writes around it.
    if site.build_masks & production::mask::HELPER_COUNTED == 0 {
        site.recharging = site.recharging.wrapping_add(1);
        site.build_masks |= production::mask::HELPER_COUNTED;
    }

    let total = production::construct_time(
        site.constr_time,
        false,
        &contribution.construct_query,
        rules,
    );
    let rate = production::construct_rate(
        site.is_under_attack(),
        contribution.korean_build_under_fire_bonus,
        rules,
    );
    let step = production::do_construct(
        rate,
        contribution.ai_speed,
        site.is_active(),
        site.job_counter,
        site.job_counter_2,
        site.helpers,
        total,
    );
    site.job_counter = step.job_counter;
    site.job_counter_2 = step.job_counter_2;
    site.helpers = step.helpers;
    checksums = checksums.union(ChecksumEffects::BUILDS);

    if !step.completed {
        return Ok(BuildReceipt {
            outcome: BuildOutcome::Progressed {
                credited: step.applied as u32,
                started_this_call,
            },
            rng_draws,
            checksums,
        });
    }

    let activation = effects
        .activate_site(site_key, site, 0, 1, 1)
        .map_err(ConstructionError::Effect)?;
    add_effect(&mut rng_draws, &mut checksums, activation.effect);
    validate_lifecycle_flags(site, activation.flags_after)?;
    if activation.flags_after & production::flag::ACTIVE == 0 {
        return Err(ConstructionError::ActivateDidNotSetActive {
            flags_after: activation.flags_after,
        });
    }
    if site.job_counter_2 != 0
        || !matches!(site.job_counter, 0 | 0x4000_0000)
        || site.recharging != 0
        || site.build_masks & 0x1000 == 0
    {
        return Err(ConstructionError::ActivateDidNotApplyLocalCore {
            job_counter: site.job_counter,
            job_counter_2: site.job_counter_2,
            recharging: site.recharging,
            build_masks: site.build_masks,
        });
    }
    checksums = checksums.union(ChecksumEffects::BUILDS);

    let finish = effects
        .finish_builder(builder, target.target, BuilderFinish::SiteCompleted)
        .map_err(ConstructionError::Effect)?;
    add_effect(&mut rng_draws, &mut checksums, finish);
    Ok(BuildReceipt {
        outcome: BuildOutcome::Completed {
            credited: step.applied as u32,
            started_this_call,
        },
        rng_draws,
        checksums,
    })
}

/// Explicit death/cancel boundary.  Retail keeps no persistent site-side builder count:
/// a builder which stops executing `do_build` simply contributes nothing, and the site's
/// `helpers` accumulator is reset by [`begin_site_frame`] on the next frame.
pub fn interrupt_builder<E: ConstructionEffects>(
    effects: &mut E,
    builder: ObjectKey,
    target: ObjectKey,
    reason: BuilderFinish,
) -> Result<BuildReceipt, ConstructionError<E::Error>> {
    assert!(
        matches!(
            reason,
            BuilderFinish::BuilderDied | BuilderFinish::OrderCancelled
        ),
        "interrupt_builder requires an external interruption reason"
    );
    let effect = effects
        .interrupt_builder(builder, target, reason)
        .map_err(ConstructionError::Effect)?;
    Ok(BuildReceipt {
        outcome: BuildOutcome::OrderRetired(reason),
        rng_draws: effect.rng_draws,
        checksums: effect.checksums,
    })
}

fn finish_without_work<E: ConstructionEffects>(
    effects: &mut E,
    builder: ObjectKey,
    target: ObjectKey,
    reason: BuilderFinish,
    prior: EffectReceipt,
) -> Result<BuildReceipt, ConstructionError<E::Error>> {
    let effect = effects
        .finish_builder(builder, target, reason)
        .map_err(ConstructionError::Effect)?;
    Ok(BuildReceipt {
        outcome: BuildOutcome::OrderRetired(reason),
        rng_draws: prior.rng_draws.wrapping_add(effect.rng_draws),
        checksums: prior.checksums.union(effect.checksums),
    })
}

fn add_effect(draws: &mut u32, checksums: &mut ChecksumEffects, effect: EffectReceipt) {
    *draws = draws.wrapping_add(effect.rng_draws);
    *checksums = checksums.union(effect.checksums);
}

fn validate_lifecycle_flags<E>(
    site: &BuildData,
    flags_after: u8,
) -> Result<(), ConstructionError<E>> {
    if site.flags != flags_after {
        return Err(ConstructionError::LifecycleFlagsMismatch {
            flags_after,
            site_flags: site.flags,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: ObjectKey = ObjectKey {
        who: 2,
        o: 2007,
        uid: 91,
    };
    const BUILDER: ObjectKey = ObjectKey {
        who: 2,
        o: 11,
        uid: 17,
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Call {
        Gate,
        Blocked,
        Start(i32),
        Disband(i32),
        Activate(i32, i32, i32),
        Finish(BuilderFinish),
        Interrupt(BuilderFinish),
    }

    struct Fx {
        gate: BuilderGate,
        raw_code: i32,
        linked_city_wonder_capacity_allows: bool,
        start_flags: u8,
        disband_flags: u8,
        active_flags: u8,
        calls: Vec<Call>,
    }

    impl Fx {
        fn new(gate: BuilderGate) -> Self {
            Self {
                gate,
                raw_code: 0,
                linked_city_wonder_capacity_allows: false,
                start_flags: production::flag::VALID | production::flag::STARTED,
                disband_flags: 0,
                active_flags: production::flag::VALID
                    | production::flag::STARTED
                    | production::flag::ACTIVE,
                calls: Vec::new(),
            }
        }
    }

    impl ConstructionEffects for Fx {
        type Error = &'static str;

        fn builder_gate(
            &mut self,
            _builder: ObjectKey,
            _target: ObjectKey,
        ) -> Result<BuilderGateReceipt, Self::Error> {
            self.calls.push(Call::Gate);
            Ok(BuilderGateReceipt {
                gate: self.gate,
                effect: EffectReceipt {
                    rng_draws: 0,
                    checksums: ChecksumEffects::NONE,
                },
            })
        }

        fn blocked_site(
            &mut self,
            _site_key: ObjectKey,
            _site: &BuildData,
        ) -> Result<SiteCheckReceipt, Self::Error> {
            self.calls.push(Call::Blocked);
            Ok(SiteCheckReceipt {
                raw_code: self.raw_code,
                linked_city_wonder_capacity_allows: self.linked_city_wonder_capacity_allows,
                effect: EffectReceipt {
                    rng_draws: 2,
                    checksums: ChecksumEffects::WORLD,
                },
            })
        }

        fn start_site(
            &mut self,
            _site_key: ObjectKey,
            site: &mut BuildData,
            notify: i32,
        ) -> Result<SiteLifecycleReceipt, Self::Error> {
            self.calls.push(Call::Start(notify));
            site.flags = self.start_flags;
            Ok(SiteLifecycleReceipt {
                flags_after: self.start_flags,
                effect: EffectReceipt {
                    rng_draws: 3,
                    checksums: ChecksumEffects::UNITS,
                },
            })
        }

        fn disband_site(
            &mut self,
            _site_key: ObjectKey,
            site: &mut BuildData,
            mode: i32,
        ) -> Result<SiteLifecycleReceipt, Self::Error> {
            self.calls.push(Call::Disband(mode));
            site.flags = self.disband_flags;
            Ok(SiteLifecycleReceipt {
                flags_after: self.disband_flags,
                effect: EffectReceipt {
                    rng_draws: 5,
                    checksums: ChecksumEffects::LEADERS,
                },
            })
        }

        fn activate_site(
            &mut self,
            _site_key: ObjectKey,
            site: &mut BuildData,
            a: i32,
            b: i32,
            c: i32,
        ) -> Result<SiteLifecycleReceipt, Self::Error> {
            self.calls.push(Call::Activate(a, b, c));
            site.flags = self.active_flags;
            site.job_counter = 0;
            site.job_counter_2 = 0;
            site.recharging = 0;
            site.build_masks |= 0x1000;
            Ok(SiteLifecycleReceipt {
                flags_after: self.active_flags,
                effect: EffectReceipt {
                    rng_draws: 7,
                    checksums: ChecksumEffects::CITIES,
                },
            })
        }

        fn finish_builder(
            &mut self,
            _builder: ObjectKey,
            _target: ObjectKey,
            reason: BuilderFinish,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Finish(reason));
            Ok(EffectReceipt {
                rng_draws: 11,
                checksums: ChecksumEffects::GROUPS,
            })
        }

        fn interrupt_builder(
            &mut self,
            _builder: ObjectKey,
            _target: ObjectKey,
            reason: BuilderFinish,
        ) -> Result<EffectReceipt, Self::Error> {
            self.calls.push(Call::Interrupt(reason));
            Ok(EffectReceipt {
                rng_draws: 13,
                checksums: ChecksumEffects::UNITS,
            })
        }
    }

    impl ChecksumEffects {
        const WORLD: Self = Self {
            world: true,
            ..Self::NONE
        };
        const UNITS: Self = Self {
            units: true,
            ..Self::NONE
        };
        const LEADERS: Self = Self {
            leaders: true,
            ..Self::NONE
        };
        const CITIES: Self = Self {
            cities: true,
            ..Self::NONE
        };
        const GROUPS: Self = Self {
            groups: true,
            ..Self::NONE
        };
    }

    fn site(started: bool) -> BuildData {
        let mut b = BuildData {
            who: SITE.who as u8,
            uid: SITE.uid,
            flags: production::flag::VALID,
            myhits: 1_000,
            construct_hits: 1,
            constr_time: 1_000,
            city: 4,
            ..BuildData::default()
        };
        if started {
            b.flags |= production::flag::STARTED;
        }
        b
    }

    fn run(
        b: &mut BuildData,
        fx: &mut Fx,
    ) -> Result<BuildReceipt, ConstructionError<&'static str>> {
        execute_builder(
            b,
            SITE,
            BUILDER,
            BuildOrderTarget::new(SITE, false),
            BuilderContribution {
                ai_speed: 1,
                ..BuilderContribution::default()
            },
            &ProdRules::shipped(),
            fx,
        )
    }

    #[test]
    fn build_order_target_keeps_uid_and_kind_six() {
        let o = BuildOrderTarget::new(SITE, true);
        assert_eq!(o.order_index, 6);
        assert_eq!(o.target, SITE);
        assert!(o.group_flag);

        let invalid = BuildOrderTarget::from_indices(-1, 2, None, false);
        assert_eq!(invalid.target.uid, 0xFFFF);
        let invalid = BuildOrderTarget::from_indices(7, -1, None, false);
        assert_eq!(invalid.target.uid, 0xFFFF);
        let valid = BuildOrderTarget::from_indices(7, 2, Some(0x1234), false);
        assert_eq!(valid.target.uid, 0x1234);
    }

    #[test]
    fn oracle_boundary_verifies_exact_rng_steps_and_channel_deltas() {
        let before = ConstructionOracleSnapshot {
            rng_state: 7,
            site_image: [0; production::BUILDDATA_SIZE],
            builder: BUILDER,
            order_target: SITE,
            builder_unit_masks: UNIT_BUILDER,
            builder_angle: 0,
            builder_group: -1,
            builds: 10,
            units: 20,
            guys: 30,
            leaders: 40,
            cities: 50,
            groups: 60,
            world: 70,
            objects_other: 80,
        };
        let mut expected_rng = before.rng_state;
        for _ in 0..15 {
            expected_rng = expected_rng
                .wrapping_mul(crate::rng::Random::MUL)
                .wrapping_add(crate::rng::Random::ADD);
        }
        let after = ConstructionOracleSnapshot {
            rng_state: expected_rng,
            site_image: {
                let mut image = before.site_image;
                image[production::off::JOB_COUNTER] = 1;
                image
            },
            builder_unit_masks: UNIT_BUILDER | UNIT_CAN_TRANSPORT,
            builds: 11,
            units: 21,
            ..before
        };
        let effect = EffectReceipt {
            rng_draws: 15,
            checksums: ChecksumEffects::BUILDS.union(ChecksumEffects::UNITS),
        };
        assert_eq!(verify_oracle_receipt(before, after, effect), Ok(()));

        let wrong = ConstructionOracleSnapshot {
            rng_state: before.rng_state,
            ..after
        };
        assert!(matches!(
            verify_oracle_receipt(before, wrong, effect),
            Err(OracleReceiptError::RngState { .. })
        ));
    }

    #[test]
    fn build_order_new_replaces_queue_and_clears_multimove_path_state() {
        let mut unit = UnitWork::at(BUILDER.who as u8, BUILDER.o as i16, 10, 20);
        unit.unit_masks = UNIT_MULTIMOVE;
        unit.parked_search = true;
        unit.orders.push_back(OrderRec::of_kind(OrderIndex::Attack));
        let r = install_build_order(
            &mut unit,
            SITE,
            QueuePos::New,
            false,
            BuildOrderRegions::default(),
        );
        assert_eq!(r.rng_draws, 0);
        assert!(unit.unit_masks & UNIT_MULTIMOVE == 0);
        assert!(unit.unit_masks & UNIT_BUILDER != 0);
        assert!(!unit.parked_search);
        assert_eq!(unit.orders.len(), 1);
        let o = unit.orders.front().unwrap();
        assert_eq!(o.kind, OrderIndex::BuildAt);
        assert_eq!(
            (o.target_who, o.target_o, o.target_uid),
            (SITE.who, SITE.o, SITE.uid)
        );
        assert!(!o.is_group());
    }

    #[test]
    fn build_order_first_and_last_preserve_retail_queue_order() {
        let old = OrderRec::of_kind(OrderIndex::Guard);
        let mut first = UnitWork::at(BUILDER.who as u8, BUILDER.o as i16, 0, 0);
        first.orders.push_back(old.clone());
        install_build_order(
            &mut first,
            SITE,
            QueuePos::First,
            true,
            BuildOrderRegions::default(),
        );
        assert_eq!(
            first.orders.iter().map(|o| o.kind).collect::<Vec<_>>(),
            vec![OrderIndex::BuildAt, OrderIndex::Guard]
        );
        assert!(first.orders.front().unwrap().is_group());

        let mut last = UnitWork::at(BUILDER.who as u8, BUILDER.o as i16, 0, 0);
        last.orders.push_back(old);
        install_build_order(
            &mut last,
            SITE,
            QueuePos::Last,
            false,
            BuildOrderRegions::default(),
        );
        assert_eq!(
            last.orders.iter().map(|o| o.kind).collect::<Vec<_>>(),
            vec![OrderIndex::Guard, OrderIndex::BuildAt]
        );
    }

    #[test]
    fn cross_region_transport_latch_uses_exact_leader_threshold() {
        for (leader_flags, threshold) in [(0, 0), (0x400, 1), (0x200, 2), (0x100, 3)] {
            for transport_type in 0..=4 {
                let mut unit = UnitWork::at(BUILDER.who as u8, BUILDER.o as i16, 0, 0);
                install_build_order(
                    &mut unit,
                    SITE,
                    QueuePos::Last,
                    false,
                    BuildOrderRegions {
                        builder_region: 1,
                        target_region: 2,
                        builder_transport_type: transport_type,
                        can_ever_transport: true,
                        leader_flags,
                    },
                );
                assert_eq!(
                    unit.unit_masks & UNIT_CAN_TRANSPORT != 0,
                    transport_type <= threshold
                );
            }
        }

        let mut same_region = UnitWork::at(BUILDER.who as u8, BUILDER.o as i16, 0, 0);
        install_build_order(
            &mut same_region,
            SITE,
            QueuePos::Last,
            false,
            BuildOrderRegions {
                builder_region: 7,
                target_region: 7,
                builder_transport_type: 0,
                can_ever_transport: true,
                leader_flags: 0x100,
            },
        );
        assert!(same_region.unit_masks & UNIT_CAN_TRANSPORT == 0);
    }

    #[test]
    fn invalid_target_retires_without_touching_site() {
        let mut b = site(true);
        let before = b.image();
        let mut fx = Fx::new(BuilderGate::InvalidTarget);
        let r = run(&mut b, &mut fx).unwrap();
        assert_eq!(
            r.outcome,
            BuildOutcome::OrderRetired(BuilderFinish::InvalidTarget)
        );
        assert_eq!(b.image(), before);
        assert_eq!(
            fx.calls,
            vec![Call::Gate, Call::Finish(BuilderFinish::InvalidTarget)]
        );
    }

    #[test]
    fn reswarm_and_decoy_gates_credit_no_site_work() {
        for gate in [BuilderGate::Reswarmed, BuilderGate::UnitDecoy] {
            let mut b = site(true);
            let before = b.image();
            let mut fx = Fx::new(gate);
            let r = run(&mut b, &mut fx).unwrap();
            assert_eq!(r.outcome, BuildOutcome::NoContribution(gate));
            assert_eq!(r.rng_draws, 0);
            assert_eq!(b.image(), before);
            assert_eq!(fx.calls, vec![Call::Gate]);
        }
    }

    #[test]
    fn first_legal_touch_starts_and_credits_work() {
        let mut b = site(false);
        let mut fx = Fx::new(BuilderGate::Ready);
        let r = run(&mut b, &mut fx).unwrap();
        assert_eq!(
            r.outcome,
            BuildOutcome::Progressed {
                credited: 100,
                started_this_call: true,
            }
        );
        assert!(b.is_started());
        assert_eq!(b.job_counter, 100);
        assert_eq!(b.helpers, 1);
        assert_eq!(r.rng_draws, 5);
        assert!(r.checksums.builds && r.checksums.world && r.checksums.units);
        assert_eq!(fx.calls, vec![Call::Gate, Call::Blocked, Call::Start(1)]);
    }

    #[test]
    fn conditional_site_code_requires_both_city_and_cap() {
        for (city, cap, starts) in [(4, true, true), (-1, true, false), (4, false, false)] {
            let mut b = site(false);
            b.city = city;
            let mut fx = Fx::new(BuilderGate::Ready);
            fx.raw_code = 0x2A;
            fx.linked_city_wonder_capacity_allows = cap;
            let r = run(&mut b, &mut fx).unwrap();
            assert_eq!(
                matches!(
                    r.outcome,
                    BuildOutcome::Progressed {
                        started_this_call: true,
                        ..
                    }
                ),
                starts
            );
            assert_eq!(
                fx.calls.last(),
                Some(&if starts {
                    Call::Start(1)
                } else {
                    Call::Disband(1)
                })
            );
        }
    }

    #[test]
    fn blocked_site_disbands_and_reports_transitive_accounting() {
        let mut b = site(false);
        let mut fx = Fx::new(BuilderGate::Ready);
        fx.raw_code = 0x99;
        let r = run(&mut b, &mut fx).unwrap();
        assert_eq!(r.outcome, BuildOutcome::SiteRejected);
        assert!(!b.is_valid());
        assert_eq!(r.rng_draws, 7);
        assert!(r.checksums.builds && r.checksums.world && r.checksums.leaders);
    }

    #[test]
    fn contributors_are_harmonic_and_first_one_bumps_recharging_once() {
        let mut b = site(true);
        b.constr_time = 100_000;
        let mut fx = Fx::new(BuilderGate::Ready);
        let a = run(&mut b, &mut fx).unwrap();
        let c = run(&mut b, &mut fx).unwrap();
        assert_eq!(
            a.outcome,
            BuildOutcome::Progressed {
                credited: 100,
                started_this_call: false,
            }
        );
        assert_eq!(
            c.outcome,
            BuildOutcome::Progressed {
                credited: 50,
                started_this_call: false,
            }
        );
        assert_eq!(b.recharging, 1);
        assert_eq!(b.helpers, 2);
        assert_eq!(b.job_counter, 150);
        assert_eq!(b.job_counter_2, 150);
        assert!(b.build_masks & production::mask::HELPER_COUNTED != 0);
    }

    #[test]
    fn begin_frame_restarts_divisor_and_updates_construct_hits() {
        let mut b = site(true);
        b.helpers = 2;
        b.build_masks |= production::mask::HELPER_COUNTED;
        b.job_counter = 500;
        let r = begin_site_frame(&mut b, ConstructionFrame::default(), &ProdRules::shipped());
        assert_eq!(r.rng_draws, 0);
        assert!(r.checksums.builds);
        assert_eq!(b.helpers, 0);
        assert!(b.build_masks & production::mask::HELPER_COUNTED == 0);
        assert!(b.build_masks & production::mask::WORKED_LAST_FRAME != 0);
        // Retail prescales both progress and duration by >>5: 1000*(500>>5)/(1000>>5).
        assert_eq!(b.construct_hits, 483);
    }

    #[test]
    fn completion_activates_before_builder_finish_and_accounts_all_effects() {
        let mut b = site(true);
        b.constr_time = 100;
        let mut fx = Fx::new(BuilderGate::Ready);
        let r = run(&mut b, &mut fx).unwrap();
        assert_eq!(
            r.outcome,
            BuildOutcome::Completed {
                credited: 100,
                started_this_call: false,
            }
        );
        assert!(b.is_active());
        assert_eq!((b.job_counter, b.job_counter_2), (0, 0));
        assert_eq!(r.rng_draws, 18);
        assert!(r.checksums.builds && r.checksums.cities && r.checksums.groups);
        assert_eq!(
            fx.calls,
            vec![
                Call::Gate,
                Call::Activate(0, 1, 1),
                Call::Finish(BuilderFinish::SiteCompleted),
            ]
        );
    }

    #[test]
    fn lifecycle_callbacks_cannot_lie_about_start_or_activation() {
        let mut b = site(false);
        let mut fx = Fx::new(BuilderGate::Ready);
        fx.start_flags = production::flag::VALID;
        assert_eq!(
            run(&mut b, &mut fx),
            Err(ConstructionError::StartDidNotSetStarted {
                flags_after: production::flag::VALID,
            })
        );

        let mut b = site(true);
        b.constr_time = 100;
        let mut fx = Fx::new(BuilderGate::Ready);
        fx.active_flags = production::flag::VALID | production::flag::STARTED;
        assert_eq!(
            run(&mut b, &mut fx),
            Err(ConstructionError::ActivateDidNotSetActive {
                flags_after: production::flag::VALID | production::flag::STARTED,
            })
        );
    }

    #[test]
    fn stale_site_identity_fails_before_any_progress() {
        let mut b = site(true);
        b.uid = SITE.uid + 1;
        let mut fx = Fx::new(BuilderGate::Ready);
        assert!(matches!(
            run(&mut b, &mut fx),
            Err(ConstructionError::SiteIdentityMismatch { .. })
        ));
        assert_eq!(b.job_counter, 0);
    }

    #[test]
    fn direct_do_build_deliberately_ignores_stored_order_uid() {
        let mut b = site(true);
        b.constr_time = 100_000;
        let mut fx = Fx::new(BuilderGate::Ready);
        let mut order = BuildOrderTarget::new(SITE, false);
        order.target.uid = SITE.uid.wrapping_add(1);
        let r = execute_builder(
            &mut b,
            SITE,
            BUILDER,
            order,
            BuilderContribution {
                ai_speed: 1,
                ..BuilderContribution::default()
            },
            &ProdRules::shipped(),
            &mut fx,
        )
        .unwrap();
        assert_eq!(
            r.outcome,
            BuildOutcome::Progressed {
                credited: 100,
                started_this_call: false,
            }
        );
    }

    #[test]
    fn interruption_has_no_site_membership_to_decrement() {
        let mut fx = Fx::new(BuilderGate::Ready);
        let r = interrupt_builder(&mut fx, BUILDER, SITE, BuilderFinish::BuilderDied).unwrap();
        assert_eq!(
            r.outcome,
            BuildOutcome::OrderRetired(BuilderFinish::BuilderDied)
        );
        assert_eq!(r.rng_draws, 13);
        assert_eq!(fx.calls, vec![Call::Interrupt(BuilderFinish::BuilderDied)]);
    }
}
