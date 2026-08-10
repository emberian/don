//! Atomic adapters for command rows 46 through 49.
//!
//! The sibling planner is nested here so this file can be path-imported without changing
//! the shared systems module map.  Rows 46/47 have complete deterministic economy tails:
//! after every required fact is preflighted, this module calls the existing
//! [`economy::do_buy`] / [`economy::do_sell`] primitives and returns a recomputable
//! before/after receipt. Opcode 48's Unit receiver can compose the complete Carrier
//! implicit-queue transaction into the same receipt. Opcode 49 can additionally bind the
//! complete `Unit::action_come_out` wrapper preflight into the receipt without publishing its
//! state-writing prefix; the mandatory general `Unit::come_out` transaction remains an explicit
//! open-tail handoff.

#[path = "carrier_implicit_unqueue_frontier.rs"]
pub mod carrier_implicit_unqueue;
#[path = "direct_entity_command_plans.rs"]
pub mod plans;
#[path = "unit_action_come_out_frontier.rs"]
pub mod unit_action_come_out;

use self::carrier_implicit_unqueue::{
    CarrierImplicitUnqueueReceipt, CarrierImplicitUnqueueRequest,
};
use self::plans::{
    plan_direct_entity_command, plan_market_command, DirectEntityCommandEffect,
    DirectEntityCommandPlan, DirectEntityCommandRequest, DirectEntityKind, DirectEntityTargetFacts,
    MarketCommandEffect, MarketCommandFacts, MarketCommandPlan, MarketCommandRequest, MarketSide,
};
use self::unit_action_come_out::{
    preflight_still_valid, preflight_unit_action_come_out, ObjectIdentity,
    UnitActionComeOutPreflight,
};
use crate::objects::{BANDED_SLOTS, OWNER_SLOTS};
use crate::systems::economy::{
    self, EconRules, LeaderEcon, MarketPriceGates, MarketState, TradeResult, NUM_RESOURCES,
};

// ---------------------------------------------------------------------------
// Shared presentation boundary
// ---------------------------------------------------------------------------

/// Ordered, headless receipts for presentation calls reached by rows 46 through 49.
///
/// These values are deliberately not callbacks.  Emitting them consumes no simulation or
/// sound RNG and lets a product presentation adapter deliver them after the atomic state
/// transaction has succeeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntityPresentationReceipt {
    MarketDiagnostic {
        side: MarketSide,
        who: i32,
        good: i32,
        flags: i32,
        frame: i32,
    },
    EmbargoUi {
        embargo: i32,
    },
    Sound {
        category: i32,
    },
    EntityDiagnostic {
        request: DirectEntityCommandRequest,
        frame: i32,
    },
}

fn market_presentation(plan: &MarketCommandPlan) -> Vec<DirectEntityPresentationReceipt> {
    plan.effects
        .iter()
        .filter_map(|effect| match *effect {
            MarketCommandEffect::Diagnostic {
                side,
                who,
                good,
                flags,
                frame,
            } => Some(DirectEntityPresentationReceipt::MarketDiagnostic {
                side,
                who,
                good,
                flags,
                frame,
            }),
            MarketCommandEffect::ShowEmbargo { embargo } => {
                Some(DirectEntityPresentationReceipt::EmbargoUi { embargo })
            }
            MarketCommandEffect::Audio { category } => {
                Some(DirectEntityPresentationReceipt::Sound { category })
            }
            MarketCommandEffect::DelegateMarketLoop { .. } => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Opcodes 46/47: complete atomic economy adapter
// ---------------------------------------------------------------------------

/// Identifies the exact counter reference supplied with one market binding.
///
/// `do_buy` receives the selected resource's demand counter; `do_sell` receives its supply
/// counter.  Keeping the side in the type prevents an adapter from silently passing the
/// wrong live column.
pub enum MarketCounterBinding<'a> {
    Demand(&'a mut i32),
    Supply(&'a mut i32),
}

/// All mutable economy state required by a reached market loop.
///
/// Every field which points at host state is optional so an incomplete host can fail closed
/// without manufacturing a default.  [`execute_market_command`] validates the owner,
/// resource, counter side, and every option before the first economy mutation.
pub struct MarketEconomyBinding<'a> {
    pub selected_who: i32,
    pub resource: i32,
    pub rules: Option<&'a EconRules>,
    pub market: Option<&'a mut MarketState>,
    pub econ: Option<&'a mut LeaderEcon>,
    pub counter: Option<MarketCounterBinding<'a>>,
    pub price_gates: Option<MarketPriceGates>,
}

impl<'a> MarketEconomyBinding<'a> {
    pub fn complete(
        selected_who: i32,
        resource: i32,
        rules: &'a EconRules,
        market: &'a mut MarketState,
        econ: &'a mut LeaderEcon,
        counter: MarketCounterBinding<'a>,
        price_gates: MarketPriceGates,
    ) -> Self {
        Self {
            selected_who,
            resource,
            rules: Some(rules),
            market: Some(market),
            econ: Some(econ),
            counter: Some(counter),
            price_gates: Some(price_gates),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketLoopTrace {
    /// Total `do_buy` / `do_sell` calls, including the final refused call.
    pub attempts: u32,
    pub completed: u32,
    pub stopped_on_refusal: bool,
}

fn run_market_loop(
    side: MarketSide,
    max_attempts: u32,
    rules: &EconRules,
    market: &mut MarketState,
    econ: &mut LeaderEcon,
    counter: &mut i32,
    resource: usize,
    price_gates: &MarketPriceGates,
) -> MarketLoopTrace {
    let mut trace = MarketLoopTrace {
        attempts: 0,
        completed: 0,
        stopped_on_refusal: false,
    };
    for _ in 0..max_attempts {
        trace.attempts += 1;
        let result = match side {
            MarketSide::Buy => economy::do_buy(rules, market, econ, counter, resource, price_gates),
            MarketSide::Sell => {
                economy::do_sell(rules, market, econ, counter, resource, price_gates)
            }
        };
        match result {
            TradeResult::Done => trace.completed += 1,
            TradeResult::Refused => {
                trace.stopped_on_refusal = true;
                break;
            }
        }
    }
    trace
}

/// Recomputable proof that the complete deterministic market loop ran against the selected
/// leader and the shared market.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketEconomyReceipt {
    pub selected_who: i32,
    pub resource: i32,
    pub rules: EconRules,
    pub price_gates: MarketPriceGates,
    pub max_attempts: u32,
    pub econ_before: LeaderEcon,
    pub econ_after: LeaderEcon,
    pub econ_checksum_before: u32,
    pub econ_checksum_after: u32,
    pub market_before: MarketState,
    pub market_after: MarketState,
    pub market_checksum_before: u32,
    pub market_checksum_after: u32,
    pub counter_before: i32,
    pub counter_after: i32,
    pub trace: MarketLoopTrace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketCommandTransactionReceipt {
    pub request: MarketCommandRequest,
    pub status: MarketTransactionStatus,
    pub facts: Option<MarketCommandFacts>,
    pub plan: Option<MarketCommandPlan>,
    pub presentation: Vec<DirectEntityPresentationReceipt>,
    pub economy: Option<MarketEconomyReceipt>,
}

impl MarketCommandTransactionReceipt {
    pub fn unavailable(request: MarketCommandRequest) -> Self {
        Self {
            request,
            status: MarketTransactionStatus::Unavailable,
            facts: None,
            plan: None,
            presentation: Vec::new(),
            economy: None,
        }
    }

    pub fn validates(&self, expected: MarketCommandRequest) -> bool {
        if self.request != expected {
            return false;
        }
        match self.status {
            MarketTransactionStatus::Unavailable => {
                self.facts.is_none()
                    && self.plan.is_none()
                    && self.presentation.is_empty()
                    && self.economy.is_none()
            }
            MarketTransactionStatus::Applied => {
                if !safe_market_leader(expected.who) {
                    return false;
                }
                let (Some(facts), Some(observed_plan)) = (self.facts, self.plan.as_ref()) else {
                    return false;
                };
                let Some(recomputed_plan) = plan_market_command(expected, &facts) else {
                    return false;
                };
                if observed_plan != &recomputed_plan
                    || self.presentation != market_presentation(&recomputed_plan)
                {
                    return false;
                }

                let delegate = recomputed_plan
                    .effects
                    .iter()
                    .find_map(|effect| match *effect {
                        MarketCommandEffect::DelegateMarketLoop {
                            side,
                            who,
                            good,
                            max_attempts,
                        } => Some((side, who, good, max_attempts)),
                        _ => None,
                    });
                match (delegate, self.economy) {
                    (None, None) => !recomputed_plan.downstream_required,
                    (Some((side, who, good, max_attempts)), Some(receipt)) => {
                        if !recomputed_plan.downstream_required
                            || receipt.selected_who != who
                            || receipt.resource != good
                            || receipt.max_attempts != max_attempts
                        {
                            return false;
                        }
                        let Ok(resource) = usize::try_from(receipt.resource) else {
                            return false;
                        };
                        let Ok(leader) = usize::try_from(receipt.selected_who) else {
                            return false;
                        };
                        if leader >= BANDED_SLOTS || resource >= NUM_RESOURCES {
                            return false;
                        }
                        let mut econ = receipt.econ_before;
                        let mut market = receipt.market_before;
                        let mut counter = receipt.counter_before;
                        let trace = run_market_loop(
                            side,
                            receipt.max_attempts,
                            &receipt.rules,
                            &mut market,
                            &mut econ,
                            &mut counter,
                            resource,
                            &receipt.price_gates,
                        );
                        econ == receipt.econ_after
                            && market == receipt.market_after
                            && counter == receipt.counter_after
                            && trace == receipt.trace
                            && receipt.econ_checksum_before == receipt.econ_before.adler32()
                            && receipt.econ_checksum_after == receipt.econ_after.adler32()
                            && receipt.market_checksum_before
                                == economy::market_adler32(&receipt.market_before)
                            && receipt.market_checksum_after
                                == economy::market_adler32(&receipt.market_after)
                    }
                    _ => false,
                }
            }
        }
    }
}

fn safe_market_leader(who: i32) -> bool {
    usize::try_from(who).is_ok_and(|who| who < BANDED_SLOTS)
}

/// Execute one decoded buy/sell command with a single fail-closed preflight.
///
/// Ineligible and embargoed paths do not read the economy binding.  A reached market loop
/// requires an exact binding, and any missing/mismatched owner, resource, rule, state,
/// counter-side, or price fact returns `Unavailable` before mutation.
pub fn execute_market_command(
    request: MarketCommandRequest,
    facts: MarketCommandFacts,
    mut economy: Option<MarketEconomyBinding<'_>>,
) -> MarketCommandTransactionReceipt {
    if !safe_market_leader(request.who) {
        return MarketCommandTransactionReceipt::unavailable(request);
    }
    let Some(plan) = plan_market_command(request, &facts) else {
        return MarketCommandTransactionReceipt::unavailable(request);
    };
    let presentation = market_presentation(&plan);
    if !plan.downstream_required {
        return MarketCommandTransactionReceipt {
            request,
            status: MarketTransactionStatus::Applied,
            facts: Some(facts),
            plan: Some(plan),
            presentation,
            economy: None,
        };
    }

    let Some((side, who, good, max_attempts)) =
        plan.effects.iter().find_map(|effect| match *effect {
            MarketCommandEffect::DelegateMarketLoop {
                side,
                who,
                good,
                max_attempts,
            } => Some((side, who, good, max_attempts)),
            _ => None,
        })
    else {
        return MarketCommandTransactionReceipt::unavailable(request);
    };
    let Ok(resource) = usize::try_from(good) else {
        return MarketCommandTransactionReceipt::unavailable(request);
    };
    if resource >= NUM_RESOURCES {
        return MarketCommandTransactionReceipt::unavailable(request);
    }
    let Some(mut binding) = economy.take() else {
        return MarketCommandTransactionReceipt::unavailable(request);
    };
    if binding.selected_who != who || binding.resource != good {
        return MarketCommandTransactionReceipt::unavailable(request);
    }

    // No mutation is allowed above this line.  Extract and validate every fallible host
    // fact before entering the infallible primitive loop.
    let (Some(rules), Some(market), Some(econ), Some(price_gates), Some(counter_binding)) = (
        binding.rules.take(),
        binding.market.take(),
        binding.econ.take(),
        binding.price_gates.take(),
        binding.counter.take(),
    ) else {
        return MarketCommandTransactionReceipt::unavailable(request);
    };
    let counter = match (side, counter_binding) {
        (MarketSide::Buy, MarketCounterBinding::Demand(counter))
        | (MarketSide::Sell, MarketCounterBinding::Supply(counter)) => counter,
        _ => return MarketCommandTransactionReceipt::unavailable(request),
    };

    let econ_before = *econ;
    let market_before = *market;
    let counter_before = *counter;
    let trace = run_market_loop(
        side,
        max_attempts,
        rules,
        market,
        econ,
        counter,
        resource,
        &price_gates,
    );
    let receipt = MarketEconomyReceipt {
        selected_who: who,
        resource: good,
        rules: *rules,
        price_gates,
        max_attempts,
        econ_before,
        econ_after: *econ,
        econ_checksum_before: econ_before.adler32(),
        econ_checksum_after: econ.adler32(),
        market_before,
        market_after: *market,
        market_checksum_before: economy::market_adler32(&market_before),
        market_checksum_after: economy::market_adler32(market),
        counter_before,
        counter_after: *counter,
        trace,
    };
    MarketCommandTransactionReceipt {
        request,
        status: MarketTransactionStatus::Applied,
        facts: Some(facts),
        plan: Some(plan),
        presentation,
        economy: Some(receipt),
    }
}

// ---------------------------------------------------------------------------
// Opcodes 48/49: exact classification plus the complete opcode-48 Unit receiver
// ---------------------------------------------------------------------------

/// Type-table result for the concrete object selected by the entity prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectEntityTypeFacts {
    pub kind: DirectEntityKind,
    pub type_index: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectEntityIdentity {
    pub who: u8,
    pub object_index: i16,
    pub uid: u16,
    pub type_index: i32,
}

/// Exact open tail selected after the active/UID guard.
///
/// The names record the current subsystem boundary.  They do not claim that the narrower
/// queue or containment primitive is the complete action body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntityOpenTail {
    ProductionUnitActionUnqueue {
        target: DirectEntityIdentity,
        argument: i32,
    },
    ProductionBuildActionUnqueue {
        target: DirectEntityIdentity,
        selector: i32,
    },
    ContainmentScholarActionComeOut {
        target: DirectEntityIdentity,
    },
    ContainmentGeneralActionComeOut {
        target: DirectEntityIdentity,
    },
    /// The complete 532-byte action wrapper has been preflighted, but none of its ordered
    /// writes may be published until this mandatory general release succeeds atomically.
    GeneralUnitComeOutTransaction {
        target: DirectEntityIdentity,
        argument: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntityDisposition {
    /// The resolved object was inactive or its UID was stale.  Retail stops after the
    /// diagnostic and no type-table read is reached.
    CompleteNoOp,
    /// The Unit receiver reached the complete Carrier implicit-queue transaction.  The
    /// recomputable before/facts/after proof is retained separately on the command receipt.
    CompleteUnitActionUnqueue {
        target: DirectEntityIdentity,
        argument: i32,
    },
    OpenTail(DirectEntityOpenTail),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntityTransactionStatus {
    Complete,
    OpenTail,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectEntityCommandTransactionReceipt {
    pub request: DirectEntityCommandRequest,
    pub status: DirectEntityTransactionStatus,
    pub frame: Option<i32>,
    pub target: Option<DirectEntityTargetFacts>,
    /// Absent on the inactive/stale path because retail does not need a type fact there.
    pub type_facts: Option<DirectEntityTypeFacts>,
    pub plan: Option<DirectEntityCommandPlan>,
    pub presentation: Vec<DirectEntityPresentationReceipt>,
    pub disposition: Option<DirectEntityDisposition>,
    /// Present only when opcode 48's reached Unit receiver completed the exact Carrier
    /// implicit-queue transaction.  Build unqueue and both come-out receivers remain open.
    pub unit_unqueue: Option<CarrierImplicitUnqueueReceipt>,
    /// Present only after opcode 49's complete `Unit::action_come_out` wrapper has been
    /// recomputed and bound to the addressed Unit identity. This is a preflight receipt, not
    /// permission to publish its writes before the general `Unit::come_out` tail succeeds.
    pub unit_action_come_out: Option<UnitActionComeOutPreflight>,
}

impl DirectEntityCommandTransactionReceipt {
    pub fn unavailable(request: DirectEntityCommandRequest) -> Self {
        Self {
            request,
            status: DirectEntityTransactionStatus::Unavailable,
            frame: None,
            target: None,
            type_facts: None,
            plan: None,
            presentation: Vec::new(),
            disposition: None,
            unit_unqueue: None,
            unit_action_come_out: None,
        }
    }

    pub fn validates(&self, expected: DirectEntityCommandRequest) -> bool {
        if self.request != expected {
            return false;
        }
        match self.status {
            DirectEntityTransactionStatus::Unavailable => {
                self.frame.is_none()
                    && self.target.is_none()
                    && self.type_facts.is_none()
                    && self.plan.is_none()
                    && self.presentation.is_empty()
                    && self.disposition.is_none()
                    && self.unit_unqueue.is_none()
                    && self.unit_action_come_out.is_none()
            }
            DirectEntityTransactionStatus::Complete | DirectEntityTransactionStatus::OpenTail => {
                let (Some(frame), Some(target)) = (self.frame, self.target) else {
                    return false;
                };
                let recomputed = match (
                    self.unit_unqueue.as_ref(),
                    self.unit_action_come_out.as_ref(),
                ) {
                    (Some(receiver), None) => complete_carrier_unit_unqueue_command(
                        expected,
                        frame,
                        Some(target),
                        self.type_facts,
                        receiver.clone(),
                    ),
                    (None, Some(preflight)) => preflight_opcode49_unit_action_come_out_command(
                        expected,
                        frame,
                        Some(target),
                        self.type_facts,
                        preflight.clone(),
                    ),
                    (None, None) => classify_direct_entity_command(
                        expected,
                        frame,
                        Some(target),
                        self.type_facts,
                    ),
                    (Some(_), Some(_)) => return false,
                };
                &recomputed == self
            }
        }
    }
}

fn safe_entity_address(request: DirectEntityCommandRequest) -> Option<(u8, i16)> {
    let (who, object_index) = match request {
        DirectEntityCommandRequest::Unqueue {
            who, object_index, ..
        }
        | DirectEntityCommandRequest::ComeOut {
            who, object_index, ..
        } => (who, object_index),
    };
    let who = usize::try_from(who).ok()?;
    if who >= OWNER_SLOTS {
        return None;
    }
    let object_index = i16::try_from(object_index).ok()?;
    if object_index < 0 {
        return None;
    }
    Some((who as u8, object_index))
}

/// Classify one decoded unqueue/come-out command without claiming its broad action tail.
///
/// A missing/unsafe object resolution is unavailable.  Inactive and stale targets are
/// complete no-ops and do not consume type facts.  A reached action requires an exact,
/// non-negative type-table fact matching the resolved concrete class; it is returned as an
/// `OpenTail` for the production/containment owner.
pub fn classify_direct_entity_command(
    request: DirectEntityCommandRequest,
    frame: i32,
    target: Option<DirectEntityTargetFacts>,
    type_facts: Option<DirectEntityTypeFacts>,
) -> DirectEntityCommandTransactionReceipt {
    let Some((who, object_index)) = safe_entity_address(request) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(target) = target else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let Some(plan) = plan_direct_entity_command(request, frame, &target) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let presentation = vec![DirectEntityPresentationReceipt::EntityDiagnostic { request, frame }];
    if !plan.downstream_required {
        return DirectEntityCommandTransactionReceipt {
            request,
            status: DirectEntityTransactionStatus::Complete,
            frame: Some(frame),
            target: Some(target),
            type_facts: None,
            plan: Some(plan),
            presentation,
            disposition: Some(DirectEntityDisposition::CompleteNoOp),
            unit_unqueue: None,
            unit_action_come_out: None,
        };
    }

    let Some(type_facts) = type_facts else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if type_facts.kind != target.kind || type_facts.type_index < 0 {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }
    let identity = DirectEntityIdentity {
        who,
        object_index,
        uid: target.uid,
        type_index: type_facts.type_index,
    };
    let Some(tail) = plan.effects.iter().find_map(|effect| match *effect {
        DirectEntityCommandEffect::Diagnostic { .. } => None,
        DirectEntityCommandEffect::DelegateUnitActionUnqueue { argument, .. } => {
            Some(DirectEntityOpenTail::ProductionUnitActionUnqueue {
                target: identity,
                argument,
            })
        }
        DirectEntityCommandEffect::DelegateBuildActionUnqueue { type_index, .. } => {
            Some(DirectEntityOpenTail::ProductionBuildActionUnqueue {
                target: identity,
                selector: type_index,
            })
        }
        DirectEntityCommandEffect::DelegateUnitActionComeOut { .. } => {
            Some(if matches!(type_facts.type_index, 0x34 | 0x35) {
                DirectEntityOpenTail::ContainmentScholarActionComeOut { target: identity }
            } else {
                DirectEntityOpenTail::ContainmentGeneralActionComeOut { target: identity }
            })
        }
    }) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };

    DirectEntityCommandTransactionReceipt {
        request,
        status: DirectEntityTransactionStatus::OpenTail,
        frame: Some(frame),
        target: Some(target),
        type_facts: Some(type_facts),
        plan: Some(plan),
        presentation,
        disposition: Some(DirectEntityDisposition::OpenTail(tail)),
        unit_unqueue: None,
        unit_action_come_out: None,
    }
}

/// Close opcode 48's reached Unit receiver with the complete Carrier implicit-queue proof.
///
/// The caller still owns the live state commit.  This adapter validates the command prefix,
/// concrete Unit identity, exact retail argument (`1`), receiver owner, and the receiver's
/// fully recomputable before/facts/after transaction before changing the command status to
/// `Complete`.  Any Build or come-out tail, mismatched identity, or malformed receiver proof
/// fails closed without manufacturing a partially complete receipt.
pub fn complete_carrier_unit_unqueue_command(
    request: DirectEntityCommandRequest,
    frame: i32,
    target: Option<DirectEntityTargetFacts>,
    type_facts: Option<DirectEntityTypeFacts>,
    receiver: CarrierImplicitUnqueueReceipt,
) -> DirectEntityCommandTransactionReceipt {
    let prefix = classify_direct_entity_command(request, frame, target, type_facts);
    let Some(DirectEntityDisposition::OpenTail(
        DirectEntityOpenTail::ProductionUnitActionUnqueue { target, argument },
    )) = prefix.disposition
    else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let receiver_request = CarrierImplicitUnqueueRequest { refund_cost: true };
    if argument != 1
        || receiver.request != receiver_request
        || !receiver.validates(receiver_request)
        || receiver
            .before
            .as_ref()
            .is_none_or(|before| before.owner != target.who)
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }

    DirectEntityCommandTransactionReceipt {
        status: DirectEntityTransactionStatus::Complete,
        disposition: Some(DirectEntityDisposition::CompleteUnitActionUnqueue { target, argument }),
        unit_unqueue: Some(receiver),
        ..prefix
    }
}

/// Bind opcode 49's complete wrapper preflight to its decoded, active Unit identity.
///
/// This advances the typed action-wrapper handoff to the exact mandatory
/// `Unit::come_out(0)` tail, but deliberately leaves the transaction `OpenTail`. The wrapper
/// clears masks, orders, and path state before reaching that call, so a host must eventually
/// revalidate this snapshot and commit both parts as one transaction; applying only the
/// returned wrapper plan would be retail-incompatible. Any malformed plan, stale identity,
/// wrong type, non-come-out command, or non-wrapper prefix fails closed.
pub fn preflight_opcode49_unit_action_come_out_command(
    request: DirectEntityCommandRequest,
    frame: i32,
    target: Option<DirectEntityTargetFacts>,
    type_facts: Option<DirectEntityTypeFacts>,
    preflight: UnitActionComeOutPreflight,
) -> DirectEntityCommandTransactionReceipt {
    let prefix = classify_direct_entity_command(request, frame, target, type_facts);
    let Some(DirectEntityDisposition::OpenTail(wrapper_tail)) = prefix.disposition else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    let identity = match wrapper_tail {
        DirectEntityOpenTail::ContainmentScholarActionComeOut { target }
        | DirectEntityOpenTail::ContainmentGeneralActionComeOut { target } => target,
        _ => return DirectEntityCommandTransactionReceipt::unavailable(request),
    };
    let expected_actor = ObjectIdentity::new(identity.who, identity.object_index);
    if preflight.facts.actor != expected_actor
        || preflight.facts.actor_type != identity.type_index
        || !preflight.plan.downstream_required
    {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }
    let Ok(recomputed) = preflight_unit_action_come_out(
        preflight.object_epoch,
        preflight.order_epoch,
        preflight.containment_epoch,
        preflight.guy_epoch,
        preflight.leader_epoch,
        preflight.facts.clone(),
    ) else {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    };
    if !preflight_still_valid(&preflight, &recomputed) {
        return DirectEntityCommandTransactionReceipt::unavailable(request);
    }

    DirectEntityCommandTransactionReceipt {
        status: DirectEntityTransactionStatus::OpenTail,
        disposition: Some(DirectEntityDisposition::OpenTail(
            DirectEntityOpenTail::GeneralUnitComeOutTransaction {
                target: identity,
                argument: 0,
            },
        )),
        unit_action_come_out: Some(preflight),
        ..prefix
    }
}

// ---------------------------------------------------------------------------
// Frozen one-call Fleet boundary for the dispatcher owner
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntityFleetRequest {
    Market {
        request: MarketCommandRequest,
        frame: i32,
    },
    Entity {
        request: DirectEntityCommandRequest,
        frame: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectEntityFleetReceipt {
    Market(MarketCommandTransactionReceipt),
    Entity(DirectEntityCommandTransactionReceipt),
}

/// Compile-checked shape of the one callback the dispatcher adds to `command::Fleet`.
///
/// This remains a standalone protocol trait in this exclusive module.  The dispatcher
/// should copy the method onto `Fleet`, not add this as a supertrait, so existing external
/// Fleet implementations retain the fail-closed default automatically.
pub trait DirectEntityFleetHandoff {
    fn apply_direct_entity_command_transaction(
        &mut self,
        request: DirectEntityFleetRequest,
    ) -> DirectEntityFleetReceipt {
        DirectEntityFleetReceipt::unavailable(request)
    }
}

impl DirectEntityFleetReceipt {
    /// Fail-closed default for the method added to `command::Fleet` by the dispatcher lane.
    pub fn unavailable(request: DirectEntityFleetRequest) -> Self {
        match request {
            DirectEntityFleetRequest::Market { request, .. } => {
                Self::Market(MarketCommandTransactionReceipt::unavailable(request))
            }
            DirectEntityFleetRequest::Entity { request, .. } => {
                Self::Entity(DirectEntityCommandTransactionReceipt::unavailable(request))
            }
        }
    }

    pub fn validates(&self, expected: DirectEntityFleetRequest) -> bool {
        match (self, expected) {
            (Self::Market(receipt), DirectEntityFleetRequest::Market { request, frame }) => {
                receipt.validates(request) && receipt.facts.is_none_or(|facts| facts.frame == frame)
            }
            (Self::Entity(receipt), DirectEntityFleetRequest::Entity { request, frame }) => {
                receipt.validates(request) && receipt.frame.is_none_or(|observed| observed == frame)
            }
            _ => false,
        }
    }
}
