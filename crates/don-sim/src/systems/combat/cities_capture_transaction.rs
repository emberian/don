//! Exact tranche braid for `Cities::capture_city` `0x00733380..0x007352BE`.
//!
//! The six instruction-bounded modules were recovered independently. This module is
//! deliberately arithmetic-free: it only validates and applies their typed continuation
//! seams in retail order. Nested calls remain owned by the one host implementing every
//! tranche trait; the braid does not cache or duplicate host state.

use super::cities_capture_local_award_notification::{
    apply_cities_capture_local_award_notification, plan_cities_capture_local_award_notification,
    CitiesCaptureLocalAwardNotificationApplyError, CitiesCaptureLocalAwardNotificationPlanError,
    CitiesCaptureLocalAwardNotificationReceipt, CitiesCaptureLocalAwardNotificationWorld,
};
use super::cities_capture_plunder_award::{
    apply_cities_capture_plunder_award, plan_cities_capture_plunder_award,
    CitiesCapturePlunderAwardApplyError, CitiesCapturePlunderAwardContinuation,
    CitiesCapturePlunderAwardPlanError, CitiesCapturePlunderAwardReceipt,
    CitiesCapturePlunderAwardWorld,
};
use super::cities_capture_plunder_gate::{
    apply_cities_capture_plunder_gate, plan_cities_capture_plunder_gate,
    CitiesCapturePlunderGateApplyError, CitiesCapturePlunderGateContinuation,
    CitiesCapturePlunderGatePlanError, CitiesCapturePlunderGateReceipt,
    CitiesCapturePlunderGateWorld,
};
use super::cities_capture_prefix::{
    apply_cities_capture_prefix, plan_cities_capture_prefix, CitiesCapturePrefixApplyError,
    CitiesCapturePrefixContinuation, CitiesCapturePrefixInput, CitiesCapturePrefixPlanError,
    CitiesCapturePrefixReceipt, CitiesCapturePrefixWorld,
};
use super::cities_capture_residual::{
    apply_cities_capture_residual, plan_cities_capture_residual, CitiesCaptureResidualApplyError,
    CitiesCaptureResidualPlanError, CitiesCaptureResidualPrior, CitiesCaptureResidualReceipt,
    CitiesCaptureResidualWorld,
};
use super::cities_capture_swap_fork::{
    apply_cities_capture_swap_fork, plan_cities_capture_swap_fork, CitiesCaptureSwapForkApplyError,
    CitiesCaptureSwapForkPlanError, CitiesCaptureSwapForkReceipt, CitiesCaptureSwapForkWorld,
    SuccessfulCenterForkInput,
};

pub const CITIES_CAPTURE_TRANSACTION_START: u32 = 0x0073_3380;
pub const CITIES_CAPTURE_TRANSACTION_END: u32 = 0x0073_52BE;
pub const CITIES_CAPTURE_TRANSACTION_SIZE: u32 =
    CITIES_CAPTURE_TRANSACTION_END - CITIES_CAPTURE_TRANSACTION_START;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CitiesCaptureTransactionInput {
    pub prefix: CitiesCapturePrefixInput,
}

/// One authoritative host must own every nested call reached by the body. A blanket
/// implementation keeps adapters honest: composing six unrelated fake stores does not
/// satisfy this boundary unless one object explicitly implements all six contracts.
pub trait CitiesCaptureTransactionWorld:
    CitiesCapturePrefixWorld
    + CitiesCaptureSwapForkWorld
    + CitiesCapturePlunderGateWorld
    + CitiesCapturePlunderAwardWorld
    + CitiesCaptureLocalAwardNotificationWorld
    + CitiesCaptureResidualWorld
{
    /// Acquire success-only facts after the prefix receipt fixes the center-swap branch.
    /// This prevents adapters from snapshotting later city/leader state before the prefix
    /// mutations which retail performs first.
    fn read_successful_center_fork_input(
        &mut self,
        prefix: &CitiesCapturePrefixReceipt,
    ) -> Option<SuccessfulCenterForkInput>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCaptureTransactionReceipt {
    pub prefix: CitiesCapturePrefixReceipt,
    pub swap: CitiesCaptureSwapForkReceipt,
    pub plunder_gate: CitiesCapturePlunderGateReceipt,
    pub plunder_award: Option<CitiesCapturePlunderAwardReceipt>,
    pub local_award_notification: Option<CitiesCaptureLocalAwardNotificationReceipt>,
    pub residual: CitiesCaptureResidualReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCaptureTransactionError {
    PrefixPlan(CitiesCapturePrefixPlanError),
    PrefixApply(CitiesCapturePrefixApplyError),
    SwapPlan(CitiesCaptureSwapForkPlanError),
    SwapApply(CitiesCaptureSwapForkApplyError),
    PlunderGatePlan(CitiesCapturePlunderGatePlanError),
    PlunderGateApply(CitiesCapturePlunderGateApplyError),
    PlunderAwardPlan(CitiesCapturePlunderAwardPlanError),
    PlunderAwardApply(CitiesCapturePlunderAwardApplyError),
    LocalAwardPlan(CitiesCaptureLocalAwardNotificationPlanError),
    LocalAwardApply(CitiesCaptureLocalAwardNotificationApplyError),
    ResidualPlan(CitiesCaptureResidualPlanError),
    ResidualApply(CitiesCaptureResidualApplyError),
}

/// Execute all 7,998 bytes from the entry receipt through the signed city-index return.
pub fn apply_cities_capture_transaction<W: CitiesCaptureTransactionWorld + ?Sized>(
    input: CitiesCaptureTransactionInput,
    world: &mut W,
) -> Result<CitiesCaptureTransactionReceipt, CitiesCaptureTransactionError> {
    let request = input.prefix.request;
    let prefix_plan = plan_cities_capture_prefix(input.prefix)
        .map_err(CitiesCaptureTransactionError::PrefixPlan)?;
    let prefix = apply_cities_capture_prefix(prefix_plan, world)
        .map_err(CitiesCaptureTransactionError::PrefixApply)?;

    let successful = match prefix.continuation {
        CitiesCapturePrefixContinuation::CenterSwapFailed0x00733ce2 => None,
        CitiesCapturePrefixContinuation::CenterSwapSucceeded0x00733755 => {
            world.read_successful_center_fork_input(&prefix)
        }
    };
    let swap_plan = plan_cities_capture_swap_fork(prefix.clone(), successful)
        .map_err(CitiesCaptureTransactionError::SwapPlan)?;
    let swap = apply_cities_capture_swap_fork(swap_plan, world)
        .map_err(CitiesCaptureTransactionError::SwapApply)?;

    let gate_plan = plan_cities_capture_plunder_gate(
        swap.clone(),
        request.old_city,
        request.new_owner,
        prefix.captured_own_capital,
    )
    .map_err(CitiesCaptureTransactionError::PlunderGatePlan)?;
    let plunder_gate = apply_cities_capture_plunder_gate(gate_plan, world)
        .map_err(CitiesCaptureTransactionError::PlunderGateApply)?;

    let (plunder_award, local_award_notification, residual_prior) = match plunder_gate.continuation
    {
        CitiesCapturePlunderGateContinuation::SkipPlunder0x00734a3c => (
            None,
            None,
            CitiesCaptureResidualPrior::SkipPlunder(plunder_gate.clone()),
        ),
        CitiesCapturePlunderGateContinuation::EnterPlunder0x00733fac => {
            let award_plan = plan_cities_capture_plunder_award(plunder_gate.clone())
                .map_err(CitiesCaptureTransactionError::PlunderAwardPlan)?;
            let award = apply_cities_capture_plunder_award(award_plan, world)
                .map_err(CitiesCaptureTransactionError::PlunderAwardApply)?;
            if award.continuation
                == CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152
            {
                let notification_plan = plan_cities_capture_local_award_notification(award.clone())
                    .map_err(CitiesCaptureTransactionError::LocalAwardPlan)?;
                let notification =
                    apply_cities_capture_local_award_notification(notification_plan, world)
                        .map_err(CitiesCaptureTransactionError::LocalAwardApply)?;
                (
                    Some(award),
                    Some(notification.clone()),
                    CitiesCaptureResidualPrior::LocalAward(notification),
                )
            } else {
                (
                    Some(award.clone()),
                    None,
                    CitiesCaptureResidualPrior::PlunderAward(award),
                )
            }
        }
    };

    let residual_plan = plan_cities_capture_residual(residual_prior)
        .map_err(CitiesCaptureTransactionError::ResidualPlan)?;
    let residual = apply_cities_capture_residual(residual_plan, world)
        .map_err(CitiesCaptureTransactionError::ResidualApply)?;

    Ok(CitiesCaptureTransactionReceipt {
        prefix,
        swap,
        plunder_gate,
        plunder_award,
        local_award_notification,
        residual,
    })
}
