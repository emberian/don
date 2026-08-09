//! `Cities::capture_city` capital-plunder award prefix, `0x00733FAC..0x00734152`.
//!
//! This module owns the old-owner capture flag, capital-plunder sizing and split,
//! and the first ordered six-good `type_avail`/`bucket_add` pass.  It stops before
//! localized notification construction and leaves both forward refund forks typed.

use super::cities_capture_plunder_gate::{
    CitiesCapturePlunderGateContinuation, CitiesCapturePlunderGateReceipt,
};
use super::cities_capture_prefix::CITIES_CAPTURE_CITY_END;

pub const CITIES_CAPTURE_PLUNDER_AWARD_START: u32 = 0x0073_3FAC;
pub const CITIES_CAPTURE_PLUNDER_AWARD_END: u32 = 0x0073_4152;
pub const CITIES_CAPTURE_PLUNDER_AWARD_SIZE: u32 =
    CITIES_CAPTURE_PLUNDER_AWARD_END - CITIES_CAPTURE_PLUNDER_AWARD_START;
pub const CITIES_CAPTURE_PLUNDER_AWARD_RESIDUAL_SIZE: u32 =
    CITIES_CAPTURE_CITY_END - CITIES_CAPTURE_PLUNDER_AWARD_END;

pub const OLD_OWNER_CAPITAL_CAPTURED_FLAG: u32 = 0x0040_0000;
pub const CAPITAL_CITY_FLAG: u16 = 0x0010;
pub const BARBARIANS_AT_THE_GATES_TEAM_STYLE: u8 = 3;
pub const ELIMINATION_PLUNDER_TEAM_STYLES: [u8; 3] = [1, 2, 9];
pub const RESOURCE_TYPE_FIRST: i32 = 0;
pub const RESOURCE_TYPE_END: i32 = 6;
pub const RESOURCE_TYPE_3_EXCLUDED: i32 = 3;
pub const TYPE_AVAIL_MODE: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OldOwnerLeaderFlagsRequest {
    pub old_owner: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OldOwnerLeaderFlagsReceipt {
    pub request: OldOwnerLeaderFlagsRequest,
    pub leader_flags: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamStyleReadPhase {
    Eligibility0x00733fd2,
    Sizing0x00733fe2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamStyleReadRequest {
    pub phase: TeamStyleReadPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamStyleReadReceipt {
    pub request: TeamStyleReadRequest,
    pub team_style: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrOldOwnerLeaderFlagRequest {
    pub old_owner: u8,
    pub mask: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrOldOwnerLeaderFlagReceipt {
    pub request: OrOldOwnerLeaderFlagRequest,
    pub flags_before: u32,
    pub flags_after: u32,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapitalPlunderRuleReceipt {
    pub capital_plunder: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EliminationPlunderSizingReceipt {
    /// Attests `Game::num_nations`, leader flags 0 through 7, then
    /// `Constants::capital_plunder_assassin` read order.
    pub exact_read_order_attested: bool,
    pub num_nations: i32,
    pub leader_flags: [u32; 8],
    pub capital_plunder_assassin: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TheDespotPlunderRuleReceipt {
    pub thedespot_plunder_percent: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeAvailabilityRequest {
    pub owner: u8,
    pub type_index: i32,
    pub mode: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeAvailabilityReceipt {
    pub request: TypeAvailabilityRequest,
    /// Raw `LeaderData::type_avail` result: retail consumes only zero/nonzero.
    pub raw_availability: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketAddRequest {
    pub owner: u8,
    pub bucket: i32,
    pub amount: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketAddReceipt {
    pub request: BucketAddRequest,
    pub balance_before: i32,
    pub balance_after: i32,
    pub mutation_applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleWhoReceipt {
    pub who: i32,
}

pub trait CitiesCapturePlunderAwardWorld {
    fn read_old_owner_leader_flags(
        &mut self,
        request: OldOwnerLeaderFlagsRequest,
    ) -> Option<OldOwnerLeaderFlagsReceipt>;
    fn read_team_style(&mut self, request: TeamStyleReadRequest) -> Option<TeamStyleReadReceipt>;
    fn or_old_owner_leader_flag(
        &mut self,
        request: OrOldOwnerLeaderFlagRequest,
    ) -> Option<OrOldOwnerLeaderFlagReceipt>;
    fn read_capital_plunder_rule(&mut self) -> Option<CapitalPlunderRuleReceipt>;
    fn read_elimination_plunder_sizing(&mut self) -> Option<EliminationPlunderSizingReceipt>;
    fn read_thedespot_plunder_rule(&mut self) -> Option<TheDespotPlunderRuleReceipt>;
    fn read_type_availability(
        &mut self,
        request: TypeAvailabilityRequest,
    ) -> Option<TypeAvailabilityReceipt>;
    fn bucket_add(&mut self, request: BucketAddRequest) -> Option<BucketAddReceipt>;
    fn read_console_who(&mut self) -> Option<ConsoleWhoReceipt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCapturePlunderAwardPlan {
    pub prior: CitiesCapturePlunderGateReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePlunderAwardPlanError {
    PriorContinuationMismatch,
    OldCityNotCapital,
}

pub fn plan_cities_capture_plunder_award(
    prior: CitiesCapturePlunderGateReceipt,
) -> Result<CitiesCapturePlunderAwardPlan, CitiesCapturePlunderAwardPlanError> {
    if prior.continuation != CitiesCapturePlunderGateContinuation::EnterPlunder0x00733fac {
        return Err(CitiesCapturePlunderAwardPlanError::PriorContinuationMismatch);
    }
    // A non-capital can enter only with nonzero ordinary plunder.  Retain both
    // possibilities; this check merely freezes that the CityData snapshot is present.
    if prior.city_flags & CAPITAL_CITY_FLAG == 0 && prior.prior.plunder_accumulator == 0 {
        return Err(CitiesCapturePlunderAwardPlanError::OldCityNotCapital);
    }
    Ok(CitiesCapturePlunderAwardPlan { prior })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitiesCapturePlunderAwardEvent {
    ReadOldOwnerLeaderFlags(OldOwnerLeaderFlagsReceipt),
    ReadTeamStyle(TeamStyleReadReceipt),
    OrOldOwnerLeaderFlag(OrOldOwnerLeaderFlagReceipt),
    ReadCapitalPlunderRule(CapitalPlunderRuleReceipt),
    ReadEliminationSizing(EliminationPlunderSizingReceipt),
    WriteSizedCapitalPlunder(i32),
    InitializeOldOwnerRefund(i32),
    ReadTheDespotPlunderRule(TheDespotPlunderRuleReceipt),
    SetOldOwnerRefund(i32),
    SetNewOwnerAward(i32),
    ReadTypeAvailability(TypeAvailabilityReceipt),
    BucketAdd(BucketAddReceipt),
    ReadConsoleWho(ConsoleWhoReceipt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePlunderAwardContinuation {
    LocalAwardNotification0x00734152,
    OldOwnerRefund0x0073432d,
    AlternatePlunder0x00734547,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CitiesCapturePlunderAwardReceipt {
    pub prior: CitiesCapturePlunderGateReceipt,
    /// `None` only on the untouched forward edge to `0x00734547`.
    pub sized_capital_plunder: Option<i32>,
    pub new_owner_award: Option<i32>,
    pub old_owner_refund: Option<i32>,
    pub continuation: CitiesCapturePlunderAwardContinuation,
    pub events: Vec<CitiesCapturePlunderAwardEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitiesCapturePlunderAwardApplyError {
    MissingOldOwnerFlagsReceipt,
    OldOwnerFlagsReceiptMismatch,
    MissingTeamStyleReceipt,
    TeamStyleReceiptMismatch,
    MissingFlagMutationReceipt,
    FlagMutationReceiptMismatch,
    FlagMutationEffectsIncomplete,
    MissingCapitalPlunderRuleReceipt,
    MissingEliminationSizingReceipt,
    EliminationSizingOrderNotAttested,
    MissingTheDespotRuleReceipt,
    MissingAvailabilityReceipt,
    AvailabilityReceiptMismatch,
    AvailabilityValueInvalid,
    MissingBucketAddReceipt,
    BucketAddReceiptMismatch,
    BucketAddEffectsIncomplete,
    MissingConsoleWhoReceipt,
}

#[inline]
pub const fn is_elimination_plunder_team_style(team_style: u8) -> bool {
    team_style == ELIMINATION_PLUNDER_TEAM_STYLES[0]
        || team_style == ELIMINATION_PLUNDER_TEAM_STYLES[1]
        || team_style == ELIMINATION_PLUNDER_TEAM_STYLES[2]
}

#[inline]
pub fn elimination_plunder_amount(receipt: EliminationPlunderSizingReceipt) -> i32 {
    let flagged = receipt
        .leader_flags
        .iter()
        .filter(|flags| **flags & 3 == 1)
        .count() as i32;
    let remaining = receipt.num_nations.wrapping_sub(flagged);
    let multiplier = receipt.num_nations.wrapping_sub(remaining).max(1);
    receipt.capital_plunder_assassin.wrapping_mul(multiplier)
}

#[inline]
pub const fn thedespot_scaled_plunder(plunder: i32, percent: i32) -> i32 {
    plunder.wrapping_mul(percent) / 100
}

fn finish(
    prior: CitiesCapturePlunderGateReceipt,
    sized_capital_plunder: Option<i32>,
    new_owner_award: Option<i32>,
    old_owner_refund: Option<i32>,
    continuation: CitiesCapturePlunderAwardContinuation,
    events: Vec<CitiesCapturePlunderAwardEvent>,
) -> CitiesCapturePlunderAwardReceipt {
    CitiesCapturePlunderAwardReceipt {
        prior,
        sized_capital_plunder,
        new_owner_award,
        old_owner_refund,
        continuation,
        events,
    }
}

/// Execute `0x00733FAC..0x00734152` in retail read/mutation order.
pub fn apply_cities_capture_plunder_award<W: CitiesCapturePlunderAwardWorld + ?Sized>(
    plan: CitiesCapturePlunderAwardPlan,
    world: &mut W,
) -> Result<CitiesCapturePlunderAwardReceipt, CitiesCapturePlunderAwardApplyError> {
    let old_owner = plan.prior.old_city.who;
    let new_owner = plan.prior.new_owner;
    let mut events = Vec::new();

    let flags_request = OldOwnerLeaderFlagsRequest { old_owner };
    let flags = world
        .read_old_owner_leader_flags(flags_request)
        .ok_or(CitiesCapturePlunderAwardApplyError::MissingOldOwnerFlagsReceipt)?;
    if flags.request != flags_request {
        return Err(CitiesCapturePlunderAwardApplyError::OldOwnerFlagsReceiptMismatch);
    }
    events.push(CitiesCapturePlunderAwardEvent::ReadOldOwnerLeaderFlags(
        flags,
    ));

    if flags.leader_flags & OLD_OWNER_CAPITAL_CAPTURED_FLAG != 0
        || plan.prior.city_flags & CAPITAL_CITY_FLAG == 0
        || plan.prior.captured_own_capital
    {
        return Ok(finish(
            plan.prior,
            None,
            None,
            None,
            CitiesCapturePlunderAwardContinuation::AlternatePlunder0x00734547,
            events,
        ));
    }

    let eligibility_request = TeamStyleReadRequest {
        phase: TeamStyleReadPhase::Eligibility0x00733fd2,
    };
    let eligibility_style = world
        .read_team_style(eligibility_request)
        .ok_or(CitiesCapturePlunderAwardApplyError::MissingTeamStyleReceipt)?;
    if eligibility_style.request != eligibility_request {
        return Err(CitiesCapturePlunderAwardApplyError::TeamStyleReceiptMismatch);
    }
    events.push(CitiesCapturePlunderAwardEvent::ReadTeamStyle(
        eligibility_style,
    ));
    if eligibility_style.team_style == BARBARIANS_AT_THE_GATES_TEAM_STYLE {
        return Ok(finish(
            plan.prior,
            None,
            None,
            None,
            CitiesCapturePlunderAwardContinuation::AlternatePlunder0x00734547,
            events,
        ));
    }

    let flag_request = OrOldOwnerLeaderFlagRequest {
        old_owner,
        mask: OLD_OWNER_CAPITAL_CAPTURED_FLAG,
    };
    let flag_mutation = world
        .or_old_owner_leader_flag(flag_request)
        .ok_or(CitiesCapturePlunderAwardApplyError::MissingFlagMutationReceipt)?;
    if flag_mutation.request != flag_request || flag_mutation.flags_before != flags.leader_flags {
        return Err(CitiesCapturePlunderAwardApplyError::FlagMutationReceiptMismatch);
    }
    if !flag_mutation.mutation_applied
        || flag_mutation.flags_after != flag_mutation.flags_before | OLD_OWNER_CAPITAL_CAPTURED_FLAG
    {
        return Err(CitiesCapturePlunderAwardApplyError::FlagMutationEffectsIncomplete);
    }
    events.push(CitiesCapturePlunderAwardEvent::OrOldOwnerLeaderFlag(
        flag_mutation,
    ));

    let sizing_request = TeamStyleReadRequest {
        phase: TeamStyleReadPhase::Sizing0x00733fe2,
    };
    let sizing_style = world
        .read_team_style(sizing_request)
        .ok_or(CitiesCapturePlunderAwardApplyError::MissingTeamStyleReceipt)?;
    if sizing_style.request != sizing_request {
        return Err(CitiesCapturePlunderAwardApplyError::TeamStyleReceiptMismatch);
    }
    events.push(CitiesCapturePlunderAwardEvent::ReadTeamStyle(sizing_style));

    let initial_plunder = plan.prior.prior.plunder_accumulator;
    let mut sized_plunder = initial_plunder;
    if is_elimination_plunder_team_style(sizing_style.team_style) {
        let sizing = world
            .read_elimination_plunder_sizing()
            .ok_or(CitiesCapturePlunderAwardApplyError::MissingEliminationSizingReceipt)?;
        if !sizing.exact_read_order_attested {
            return Err(CitiesCapturePlunderAwardApplyError::EliminationSizingOrderNotAttested);
        }
        sized_plunder = elimination_plunder_amount(sizing);
        events.push(CitiesCapturePlunderAwardEvent::ReadEliminationSizing(
            sizing,
        ));
        events.push(CitiesCapturePlunderAwardEvent::WriteSizedCapitalPlunder(
            sized_plunder,
        ));
    } else {
        let rule = world
            .read_capital_plunder_rule()
            .ok_or(CitiesCapturePlunderAwardApplyError::MissingCapitalPlunderRuleReceipt)?;
        sized_plunder = sized_plunder.max(rule.capital_plunder);
        events.push(CitiesCapturePlunderAwardEvent::ReadCapitalPlunderRule(rule));
        if initial_plunder <= rule.capital_plunder {
            events.push(CitiesCapturePlunderAwardEvent::WriteSizedCapitalPlunder(
                sized_plunder,
            ));
        }
    }

    let mut old_owner_refund = 0;
    events.push(CitiesCapturePlunderAwardEvent::InitializeOldOwnerRefund(0));
    let mut new_owner_award = sized_plunder;
    if plan.prior.russian_plunder_steal {
        old_owner_refund = sized_plunder;
        events.push(CitiesCapturePlunderAwardEvent::SetOldOwnerRefund(
            old_owner_refund,
        ));
        new_owner_award = if plan.prior.qualifying_general {
            sized_plunder
        } else {
            0
        };
        events.push(CitiesCapturePlunderAwardEvent::SetNewOwnerAward(
            new_owner_award,
        ));
    } else if plan.prior.qualifying_general {
        let rule = world
            .read_thedespot_plunder_rule()
            .ok_or(CitiesCapturePlunderAwardApplyError::MissingTheDespotRuleReceipt)?;
        events.push(CitiesCapturePlunderAwardEvent::ReadTheDespotPlunderRule(
            rule,
        ));
        new_owner_award = thedespot_scaled_plunder(sized_plunder, rule.thedespot_plunder_percent);
        events.push(CitiesCapturePlunderAwardEvent::SetNewOwnerAward(
            new_owner_award,
        ));
    }

    if new_owner_award == 0 {
        return Ok(finish(
            plan.prior,
            Some(sized_plunder),
            Some(new_owner_award),
            Some(old_owner_refund),
            CitiesCapturePlunderAwardContinuation::OldOwnerRefund0x0073432d,
            events,
        ));
    }

    let mut type_index = RESOURCE_TYPE_FIRST;
    while type_index < RESOURCE_TYPE_END {
        let new_request = TypeAvailabilityRequest {
            owner: new_owner,
            type_index,
            mode: TYPE_AVAIL_MODE,
        };
        let new_availability = world
            .read_type_availability(new_request)
            .ok_or(CitiesCapturePlunderAwardApplyError::MissingAvailabilityReceipt)?;
        if new_availability.request != new_request {
            return Err(CitiesCapturePlunderAwardApplyError::AvailabilityReceiptMismatch);
        }
        if !matches!(new_availability.raw_availability, 0 | 2 | 4) {
            return Err(CitiesCapturePlunderAwardApplyError::AvailabilityValueInvalid);
        }
        events.push(CitiesCapturePlunderAwardEvent::ReadTypeAvailability(
            new_availability,
        ));
        if new_availability.raw_availability != 0 {
            let old_request = TypeAvailabilityRequest {
                owner: old_owner,
                type_index,
                mode: TYPE_AVAIL_MODE,
            };
            let old_availability = world
                .read_type_availability(old_request)
                .ok_or(CitiesCapturePlunderAwardApplyError::MissingAvailabilityReceipt)?;
            if old_availability.request != old_request {
                return Err(CitiesCapturePlunderAwardApplyError::AvailabilityReceiptMismatch);
            }
            if !matches!(old_availability.raw_availability, 0 | 2 | 4) {
                return Err(CitiesCapturePlunderAwardApplyError::AvailabilityValueInvalid);
            }
            events.push(CitiesCapturePlunderAwardEvent::ReadTypeAvailability(
                old_availability,
            ));
            if old_availability.raw_availability != 0 && type_index != RESOURCE_TYPE_3_EXCLUDED {
                let bucket_request = BucketAddRequest {
                    owner: new_owner,
                    bucket: type_index,
                    amount: new_owner_award,
                };
                let bucket = world
                    .bucket_add(bucket_request)
                    .ok_or(CitiesCapturePlunderAwardApplyError::MissingBucketAddReceipt)?;
                if bucket.request != bucket_request {
                    return Err(CitiesCapturePlunderAwardApplyError::BucketAddReceiptMismatch);
                }
                if !bucket.mutation_applied
                    || bucket.balance_after != bucket.balance_before.wrapping_add(new_owner_award)
                {
                    return Err(CitiesCapturePlunderAwardApplyError::BucketAddEffectsIncomplete);
                }
                events.push(CitiesCapturePlunderAwardEvent::BucketAdd(bucket));
            }
        }
        type_index += 1;
    }

    let console = world
        .read_console_who()
        .ok_or(CitiesCapturePlunderAwardApplyError::MissingConsoleWhoReceipt)?;
    events.push(CitiesCapturePlunderAwardEvent::ReadConsoleWho(console));
    let continuation = if console.who == i32::from(new_owner) {
        CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152
    } else {
        CitiesCapturePlunderAwardContinuation::OldOwnerRefund0x0073432d
    };
    Ok(finish(
        plan.prior,
        Some(sized_plunder),
        Some(new_owner_award),
        Some(old_owner_refund),
        continuation,
        events,
    ))
}
