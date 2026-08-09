//! Exact transaction plans for the two terminal `Unit::do_job` arms.
//!
//! This module owns no dispatcher integration.  It transcribes the complete bodies of:
//!
//! * `Unit::do_form_change` `0x005E8670..0x005E86C3` (`CHANGE_FORM`, arm 18); and
//! * `Unit::do_think_order` `0x005E5BF0..0x005E5C63` (`THINK`, arm 27).
//!
//! Both bodies retire their current order and then cross object-owned boundaries.  A caller
//! must therefore apply the returned effects atomically.  In particular, a caller must not
//! store `UnitData::form` and later discover that `Unit::set_angle` or order retirement is
//! unavailable.

use crate::order::OrderIndex;

/// The four exact `TypeIndex` values admitted by `Unit::do_think_order`.
///
/// Names and values are from the shipped PDB `TypeIndex` enum.  The comparisons are the
/// straight `0x32..0x35` chain at `0x005E5C44..0x005E5C56`.
pub mod think_type {
    pub const PEASANTS: i32 = 50;
    pub const PEASANTS_KOREAN: i32 = 51;
    pub const SCHOLARS: i32 = 52;
    pub const SCHOLARS_KOREAN: i32 = 53;
}

/// Stable engine identity for the actor whose order is being executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalOrderActor {
    pub who: u8,
    pub object: i16,
    pub uid: u16,
}

/// Checksum-visible fields read from `FormOrder`.
///
/// `angle` is `MoveOrder::angle` at concrete `+0x0C`; `new_form` is
/// `FormOrder::newform` at concrete `+0x50`.  `delay` at `+0x54` exists in the PDB but the
/// retail executor never reads it, so it is deliberately absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeFormOrderFacts {
    pub angle: i32,
    pub new_form: i32,
}

/// Complete immutable input needed to select either retail arm's effects.
///
/// `queue_before` is in execution order with the current order first.  Carrying the entire
/// kind sequence makes the post-retirement `THINK` read deterministic and prevents a receipt
/// for one queue from validating against another queue with a different successor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalOrderRequest {
    ChangeForm {
        actor: TerminalOrderActor,
        order: ChangeFormOrderFacts,
        queue_before: Vec<OrderIndex>,
    },
    Think {
        actor: TerminalOrderActor,
        unit_type: i32,
        queue_before: Vec<OrderIndex>,
    },
}

impl TerminalOrderRequest {
    /// Reject a stale or misclassified transaction before planning any mutation.
    pub fn has_expected_head(&self) -> bool {
        match self {
            Self::ChangeForm { queue_before, .. } => {
                queue_before.first() == Some(&OrderIndex::ChangeForm)
            }
            Self::Think { queue_before, .. } => queue_before.first() == Some(&OrderIndex::Think),
        }
    }
}

/// One mutation/callback in retail execution order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalOrderEffect {
    /// Store the low byte of `FormOrder::newform` in `UnitData::form` at `+0xAA`.
    StoreForm(i8),
    /// `Unit::set_angle(angle, ignored, 0)` at `0x005E8697..0x005E86A3`.
    ///
    /// The middle ABI argument is not read by `Unit::set_angle` `0x00605400`; the final zero
    /// is represented by `update_position: false`.
    SetAngle { angle: i32, update_position: bool },
    /// `Unit::kill_current_order(0)`.
    KillCurrentOrder { suppress_arrival: bool },
    /// Virtual `Unit::do_idle` through slot `+0x188`.
    DoIdle,
    /// `Unit::think_peasant(1)`.
    ThinkPeasant { forced: bool },
}

/// Exact ordered effect list for one arm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalOrderPlan {
    pub arm: OrderIndex,
    pub effects: Vec<TerminalOrderEffect>,
}

/// Whether the host committed the complete plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalOrderStatus {
    Unavailable,
    Applied,
}

/// Atomic host receipt.  `Applied` is meaningful only when [`Self::validates`] succeeds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalOrderReceipt {
    pub request: TerminalOrderRequest,
    pub status: TerminalOrderStatus,
    pub plan: Option<TerminalOrderPlan>,
}

impl TerminalOrderReceipt {
    pub fn unavailable(request: TerminalOrderRequest) -> Self {
        Self {
            request,
            status: TerminalOrderStatus::Unavailable,
            plan: None,
        }
    }

    pub fn applied(request: TerminalOrderRequest) -> Option<Self> {
        let plan = plan_terminal_order(&request)?;
        Some(Self {
            request,
            status: TerminalOrderStatus::Applied,
            plan: Some(plan),
        })
    }

    /// Recompute the complete plan and bind it to the expected actor, queue, and order facts.
    pub fn validates(&self, expected: &TerminalOrderRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            TerminalOrderStatus::Unavailable => self.plan.is_none(),
            TerminalOrderStatus::Applied => {
                plan_terminal_order(expected).is_some_and(|plan| self.plan.as_ref() == Some(&plan))
            }
        }
    }
}

/// Fail-closed handoff which can be copied onto the dispatcher host without adding a required
/// method to every existing test host in the same convergence step.
pub trait TerminalOrderHost {
    fn apply_terminal_order_transaction(
        &mut self,
        request: TerminalOrderRequest,
    ) -> TerminalOrderReceipt {
        TerminalOrderReceipt::unavailable(request)
    }
}

/// Whether `do_think_order` invokes `think_peasant(1)` after retiring itself.
#[inline]
pub const fn think_peasant_type(unit_type: i32) -> bool {
    matches!(
        unit_type,
        think_type::PEASANTS
            | think_type::PEASANTS_KOREAN
            | think_type::SCHOLARS
            | think_type::SCHOLARS_KOREAN
    )
}

/// Pure transcription of both executor bodies.
///
/// Returns `None` if the supplied queue does not have the expected arm at its head.  This is
/// a stale-preflight failure, not a retail branch.
pub fn plan_terminal_order(request: &TerminalOrderRequest) -> Option<TerminalOrderPlan> {
    if !request.has_expected_head() {
        return None;
    }

    match request {
        TerminalOrderRequest::ChangeForm {
            order,
            queue_before,
            ..
        } => {
            let single_order = queue_before.len() == 1;
            let mut effects = vec![TerminalOrderEffect::StoreForm(order.new_form as i8)];
            if single_order {
                effects.push(TerminalOrderEffect::SetAngle {
                    angle: order.angle,
                    update_position: false,
                });
            }
            effects.push(TerminalOrderEffect::KillCurrentOrder {
                suppress_arrival: false,
            });
            if !single_order {
                effects.push(TerminalOrderEffect::DoIdle);
            }
            Some(TerminalOrderPlan {
                arm: OrderIndex::ChangeForm,
                effects,
            })
        }
        TerminalOrderRequest::Think {
            unit_type,
            queue_before,
            ..
        } => {
            let mut effects = vec![TerminalOrderEffect::KillCurrentOrder {
                suppress_arrival: false,
            }];
            let successor_type = queue_before.get(1).copied();
            let successor_blocks_think =
                successor_type.is_some_and(|kind| kind != OrderIndex::None);
            if !successor_blocks_think && think_peasant_type(*unit_type) {
                effects.push(TerminalOrderEffect::ThinkPeasant { forced: true });
            }
            Some(TerminalOrderPlan {
                arm: OrderIndex::Think,
                effects,
            })
        }
    }
}
