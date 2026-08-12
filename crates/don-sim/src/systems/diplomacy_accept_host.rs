// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic opcode-41 host for `Leader::action_respond(target, 1)` (`0x006D03C0`).
//!
//! The retail body is one transaction even though its persistent fields currently project
//! through the command, resource, relation, and declaration owners.  This module therefore
//! validates every overlapping projection before planning and publishes only a complete
//! after-image.  The first accept may stop after escrow reservation; a reciprocal accept may
//! continue through conflict/alliance validation, tribute transfer, two root `set_diplo`
//! calls, neutral-team fanout, recursive declarations, record clearing, and notifications.

use super::diplomacy_declare_host::{
    plan_declare, validate_projection as validate_declare_projection, DeclareAuthorityImage,
    DeclareHostError, DeclarePlan, DeclareStep,
};
use super::leader_set_diplo::{
    plan_accepted_deal_resources, plan_set_diplo, AcceptedDealResourcePlan,
    AcceptedDealResourceStep, AcceptedDealResources, DealResourcePlanError, Relation,
    SetDiploAuthority, SetDiploPlan, SetDiploPlanError, SetDiploRequest, SetDiploStep, DIPLO_SLOTS,
    NUM_GOODS,
};
use crate::command::diplomacy_command_plans::{
    plan_diplomacy_command, DiplomacyBoundary, DiplomacyCommandState, DiplomacyPlanDecision,
    DiplomacyPlanError, DiplomacyProposal, DiplomacyStep, DiplomacyWireCommand, ACCEPT_OPCODE,
    DECLARE_OPCODE,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptLeaderImage {
    /// `LeaderData+0x0B4 + other*4`; first accept ORs the reciprocal entry with four.
    pub agenda_flags: [u32; DIPLO_SLOTS],
    /// `LeaderData+0x2D0 + other*4`; accepted peace stamps both directions.
    pub peace_frames: [i32; DIPLO_SLOTS],
    /// `LeaderData+0x1B4 + other*4`.
    pub attack_frames: [i32; DIPLO_SLOTS],
    /// `LeaderData+0x1D4 + other*4`.
    pub attack_peers: [i32; DIPLO_SLOTS],
    /// Exact `LeaderData::is_neutral()` result.
    pub is_neutral: Option<bool>,
    /// Exact `LeaderData::num_team_members(1)` result.
    pub team_members_mode_one: Option<i32>,
    /// Exact `LeaderData::num_allies()` result.
    pub num_allies: Option<i32>,
}

impl Default for AcceptLeaderImage {
    fn default() -> Self {
        Self {
            agenda_flags: [0; DIPLO_SLOTS],
            peace_frames: [0; DIPLO_SLOTS],
            attack_frames: [0; DIPLO_SLOTS],
            attack_peers: [-1; DIPLO_SLOTS],
            is_neutral: None,
            team_members_mode_one: None,
            num_allies: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptAuthorityImage {
    /// Canonical declaration aggregate. Recursive attack proposals reuse this exact owner.
    pub declaration: DeclareAuthorityImage,
    /// Accepted-deal escrow, tribute statistics, availability, and scale authority.
    pub resources: AcceptedDealResources,
    /// Accept-only retained `LeaderData` fields and installed query results.
    pub leaders: [AcceptLeaderImage; DIPLO_SLOTS],
}

impl Default for AcceptAuthorityImage {
    fn default() -> Self {
        Self {
            declaration: DeclareAuthorityImage::default(),
            resources: AcceptedDealResources::default(),
            leaders: std::array::from_fn(|_| AcceptLeaderImage::default()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptProjectionField {
    AuthoritativeLeaderBucket,
    AcceptedResourceBucket,
    Escrow,
    TypeAvailability,
    Offer,
    DeclarationCost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingAcceptFact {
    Neutral { who: usize },
    TeamMembersModeOne { who: usize },
    NumAllies { who: usize },
    DeclarationCost { who: usize, good: usize },
    WarAllowed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcceptHostError {
    NotAccept,
    Command(DiplomacyPlanError),
    Declare(DeclareHostError),
    Resources(DealResourcePlanError),
    SetDiplo(SetDiploPlanError),
    UnexpectedCommandDecision,
    UnsupportedRelation(i32),
    Missing(MissingAcceptFact),
    ProjectionMismatch {
        field: AcceptProjectionField,
        first: usize,
        second: usize,
        good: usize,
    },
}

impl From<DiplomacyPlanError> for AcceptHostError {
    fn from(value: DiplomacyPlanError) -> Self {
        Self::Command(value)
    }
}

impl From<DeclareHostError> for AcceptHostError {
    fn from(value: DeclareHostError) -> Self {
        Self::Declare(value)
    }
}

impl From<DealResourcePlanError> for AcceptHostError {
    fn from(value: DealResourcePlanError) -> Self {
        Self::Resources(value)
    }
}

impl From<SetDiploPlanError> for AcceptHostError {
    fn from(value: SetDiploPlanError) -> Self {
        Self::SetDiplo(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptOutcome {
    RefusedInsufficientResources,
    ReservedAwaitingReciprocal,
    ClearedAttackConflict,
    ClearedInvalidAlliance,
    Accepted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptPresentation {
    CannotReserveAttackCost {
        accepter: usize,
        proposer: usize,
        good: usize,
    },
    AttackConflict {
        accepter: usize,
        proposer: usize,
        candidate: usize,
    },
    InvalidAlliance {
        accepter: usize,
        proposer: usize,
    },
    AwaitingReciprocal {
        accepter: usize,
        proposer: usize,
    },
    AcceptedDeal {
        accepter: usize,
        proposer: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecursiveDeclarePlan {
    pub actor: usize,
    pub target: usize,
    /// Whether retail reaches `afford_dow`/`pay_dow` and declaration statistics.
    /// `action_respond` passes the inverse as `action_declare`'s no-payment argument.
    pub pay: bool,
    /// Self-consistent opcode-38 whole-body authority used for relation/fanout child calls.
    /// For `pay == false`, its installed payment facts are unavailable so it requires no cost.
    pub plan: DeclarePlan,
    /// Exact general `action_declare(..., no_payment, ...)` after-image. The general body
    /// skips payment and declaration statistics when `pay == false`, unlike opcode 38.
    pub effective_after: DeclareAuthorityImage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcceptStep {
    Prefix(DiplomacyStep),
    ReservationPreflight {
        accepter: usize,
        proposer: usize,
        good: usize,
        offer: i32,
        /// Retail's presentation gate records that the DOW-cost path was reached,
        /// independently of whether the AI multiplier reduced the cost to zero.
        attack_cost_queried: bool,
        attack_cost: i32,
    },
    ReserveOffer {
        accepter: usize,
        proposer: usize,
        good: usize,
        amount: i32,
    },
    ReserveAttackCost {
        accepter: usize,
        proposer: usize,
        good: usize,
        amount: i32,
    },
    MarkAgreementPending {
        accepter: usize,
        proposer: usize,
    },
    MarkReciprocalAgenda {
        leader: usize,
        other: usize,
    },
    ClearAgreement {
        leader: usize,
        target: usize,
    },
    ClearProposalRecord {
        leader: usize,
        target: usize,
    },
    AcceptedResources(AcceptedDealResourcePlan),
    RootSetDiplo {
        actor: usize,
        target: usize,
        plan: SetDiploPlan,
    },
    StampPeaceFrame {
        first: usize,
        second: usize,
        frame: i32,
    },
    TeamFanoutSetDiplo {
        team_member: usize,
        other: usize,
        plan: SetDiploPlan,
    },
    StampAttackContinuation {
        accepter: usize,
        proposer: usize,
        candidate: usize,
        frame: i32,
    },
    RecursiveDeclare(RecursiveDeclarePlan),
    Presentation(AcceptPresentation),
    NotifyDeal {
        leader: usize,
        other: usize,
        treaty: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptAuthority {
    SetDiplo {
        actor: usize,
        target: usize,
        call: SetDiploAuthority,
    },
    ConsiderTribute {
        receiver: usize,
        sender: usize,
        good: usize,
        raw: i32,
    },
    RecursiveDeclare {
        actor: usize,
        target: usize,
        call: SetDiploAuthority,
    },
    NotifyDeal {
        leader: usize,
        other: usize,
        treaty: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptPlan {
    pub wire: Vec<u8>,
    pub accepter: usize,
    pub proposer: usize,
    pub before: AcceptAuthorityImage,
    pub after: AcceptAuthorityImage,
    pub outcome: AcceptOutcome,
    pub steps: Vec<AcceptStep>,
}

fn relation(raw: i32) -> Result<Relation, AcceptHostError> {
    match raw {
        0 => Ok(Relation::War),
        1 => Ok(Relation::Peace),
        2 => Ok(Relation::Ally),
        value => Err(AcceptHostError::UnsupportedRelation(value)),
    }
}

fn raw_relation(value: Relation) -> i32 {
    value as i32
}

pub fn validate_accept_projection(image: &AcceptAuthorityImage) -> Result<(), AcceptHostError> {
    validate_declare_projection(&image.declaration)?;
    for first in 0..DIPLO_SLOTS {
        let payment = &image.declaration.payments[first];
        let command = &image.declaration.command.leaders[first];
        let resources = &image.resources.leaders[first];
        for good in 0..NUM_GOODS {
            if payment.authoritative_buckets[good] != payment.leader_buckets[good] {
                return Err(AcceptHostError::ProjectionMismatch {
                    field: AcceptProjectionField::AuthoritativeLeaderBucket,
                    first,
                    second: first,
                    good,
                });
            }
            if payment.authoritative_buckets[good] != resources.buckets[good] {
                return Err(AcceptHostError::ProjectionMismatch {
                    field: AcceptProjectionField::AcceptedResourceBucket,
                    first,
                    second: first,
                    good,
                });
            }
            if command.reserved_resources[good] != resources.escrow[good] {
                return Err(AcceptHostError::ProjectionMismatch {
                    field: AcceptProjectionField::Escrow,
                    first,
                    second: first,
                    good,
                });
            }
            if payment.type_available[good] != resources.type_available[good] {
                return Err(AcceptHostError::ProjectionMismatch {
                    field: AcceptProjectionField::TypeAvailability,
                    first,
                    second: first,
                    good,
                });
            }
        }
        for second in 0..DIPLO_SLOTS {
            for good in 0..NUM_GOODS {
                let proposal = command.proposals[second];
                if proposal.offers[good] != image.resources.offers[first][second][good] {
                    return Err(AcceptHostError::ProjectionMismatch {
                        field: AcceptProjectionField::Offer,
                        first,
                        second,
                        good,
                    });
                }
                if proposal.declaration_costs[good]
                    != image.resources.declaration_costs[first][second][good]
                {
                    return Err(AcceptHostError::ProjectionMismatch {
                        field: AcceptProjectionField::DeclarationCost,
                        first,
                        second,
                        good,
                    });
                }
            }
        }
    }
    Ok(())
}

fn sync_resource_projection(image: &mut AcceptAuthorityImage) {
    for first in 0..DIPLO_SLOTS {
        image.resources.leaders[first].buckets =
            image.declaration.payments[first].authoritative_buckets;
        image.resources.leaders[first].escrow =
            image.declaration.command.leaders[first].reserved_resources;
        image.resources.leaders[first].type_available =
            image.declaration.payments[first].type_available;
        for second in 0..DIPLO_SLOTS {
            image.resources.offers[first][second] =
                image.declaration.command.leaders[first].proposals[second].offers;
            image.resources.declaration_costs[first][second] =
                image.declaration.command.leaders[first].proposals[second].declaration_costs;
        }
    }
}

fn sync_declaration_projection(image: &mut AcceptAuthorityImage) {
    for first in 0..DIPLO_SLOTS {
        let buckets = image.resources.leaders[first].buckets;
        image.declaration.payments[first].authoritative_buckets = buckets;
        image.declaration.payments[first].leader_buckets = buckets;
        image.declaration.command.leaders[first].buckets = buckets;
        image.declaration.command.leaders[first].reserved_resources =
            image.resources.leaders[first].escrow;
        for second in 0..DIPLO_SLOTS {
            image.declaration.command.leaders[first].proposals[second].offers =
                image.resources.offers[first][second];
            image.declaration.command.leaders[first].proposals[second].declaration_costs =
                image.resources.declaration_costs[first][second];
        }
    }
}

fn sync_set_diplo_projection(image: &mut AcceptAuthorityImage) {
    for first in 0..DIPLO_SLOTS {
        for second in 0..DIPLO_SLOTS {
            if let Some(value) = image.declaration.set_diplo.leaders[first].diplos[second] {
                image.declaration.command.setup.leaders[first].diplos[second] = raw_relation(value);
            }
        }
    }
}

fn debit_reservation(image: &mut AcceptAuthorityImage, who: usize, good: usize, amount: i32) {
    let payment = &mut image.declaration.payments[who];
    payment.authoritative_buckets[good] = payment.authoritative_buckets[good]
        .wrapping_sub(amount)
        .max(0);
    payment.leader_buckets[good] = payment.leader_buckets[good].wrapping_sub(amount).max(0);
    image.declaration.command.leaders[who].buckets[good] = payment.authoritative_buckets[good];
    image.declaration.command.leaders[who].reserved_resources[good] =
        image.declaration.command.leaders[who].reserved_resources[good].wrapping_add(amount);
}

fn clear_agreement(
    image: &mut AcceptAuthorityImage,
    leader: usize,
    target: usize,
    steps: &mut Vec<AcceptStep>,
) {
    let proposal = image.declaration.command.leaders[leader].proposals[target];
    if proposal.agreement_pending == 1 {
        for good in 0..NUM_GOODS {
            let declaration_cost = proposal.declaration_costs[good];
            if declaration_cost != 0 {
                let reserved = image.declaration.command.leaders[leader].reserved_resources[good];
                let returned = reserved.min(declaration_cost);
                image.declaration.command.leaders[leader].reserved_resources[good] =
                    reserved.wrapping_sub(returned);
                let payment = &mut image.declaration.payments[leader];
                payment.authoritative_buckets[good] =
                    payment.authoritative_buckets[good].wrapping_add(returned);
                payment.leader_buckets[good] = payment.leader_buckets[good].wrapping_add(returned);
                image.declaration.command.leaders[leader].buckets[good] =
                    payment.authoritative_buckets[good];
            }
            let offer = proposal.offers[good];
            if offer > 0 {
                let reserved = image.declaration.command.leaders[leader].reserved_resources[good];
                let returned = reserved.min(offer);
                image.declaration.command.leaders[leader].reserved_resources[good] =
                    reserved.wrapping_sub(returned);
                let payment = &mut image.declaration.payments[leader];
                payment.authoritative_buckets[good] =
                    payment.authoritative_buckets[good].wrapping_add(returned);
                payment.leader_buckets[good] = payment.leader_buckets[good].wrapping_add(returned);
                image.declaration.command.leaders[leader].buckets[good] =
                    payment.authoritative_buckets[good];
            }
        }
        image.declaration.command.leaders[leader].proposals[target].declaration_costs =
            [0; NUM_GOODS];
    }
    image.declaration.command.leaders[leader].proposals[target].agreement_pending = 0;
    image.declaration.command.leaders[leader].proposals[target].proposal_open = 0;
    steps.push(AcceptStep::ClearAgreement { leader, target });
}

fn clear_pair_and_records(
    image: &mut AcceptAuthorityImage,
    accepter: usize,
    proposer: usize,
    steps: &mut Vec<AcceptStep>,
) {
    clear_agreement(image, accepter, proposer, steps);
    clear_agreement(image, proposer, accepter, steps);
    image.declaration.command.leaders[accepter].proposals[proposer] = DiplomacyProposal::default();
    steps.push(AcceptStep::ClearProposalRecord {
        leader: accepter,
        target: proposer,
    });
    image.declaration.command.leaders[proposer].proposals[accepter] = DiplomacyProposal::default();
    steps.push(AcceptStep::ClearProposalRecord {
        leader: proposer,
        target: accepter,
    });
    sync_resource_projection(image);
}

fn reserve_first_accept(
    image: &mut AcceptAuthorityImage,
    accepter: usize,
    proposer: usize,
    steps: &mut Vec<AcceptStep>,
) -> Result<Option<usize>, AcceptHostError> {
    let mut attack_costs = [0i32; NUM_GOODS];
    let mut attack_cost_queried = [false; NUM_GOODS];
    for good in 0..NUM_GOODS {
        let accepter_available = image.declaration.payments[accepter].type_available[good].ok_or(
            AcceptHostError::Resources(DealResourcePlanError::MissingTypeAvailability {
                who: accepter,
                good,
            }),
        )?;
        let proposer_available = image.declaration.payments[proposer].type_available[good].ok_or(
            AcceptHostError::Resources(DealResourcePlanError::MissingTypeAvailability {
                who: proposer,
                good,
            }),
        )?;
        if !accepter_available || !proposer_available {
            continue;
        }
        for candidate in 0..DIPLO_SLOTS {
            let attacks =
                image.declaration.command.leaders[accepter].proposals[proposer].attacks[candidate];
            if !image.declaration.command.setup.leaders[candidate].is_present()
                || attacks == 0
                || image.declaration.command.is_enemy(accepter, candidate)?
            {
                continue;
            }
            attack_cost_queried[good] = true;
            let base = image.declaration.payments[accepter].costs[good].ok_or(
                AcceptHostError::Missing(MissingAcceptFact::DeclarationCost {
                    who: accepter,
                    good,
                }),
            )?;
            let ally_factor = if image.declaration.command.is_ally(accepter, candidate)? {
                2
            } else {
                1
            };
            let ai_factor =
                i32::from(image.declaration.command.setup.leaders[accepter].leader_flags & 4 != 0);
            // Retail overwrites this scratch slot for each reached candidate; it does not sum.
            attack_costs[good] = base.wrapping_mul(ally_factor).wrapping_mul(ai_factor);
        }
        let offer =
            image.declaration.command.leaders[accepter].proposals[proposer].offers[good].max(0);
        steps.push(AcceptStep::ReservationPreflight {
            accepter,
            proposer,
            good,
            offer,
            attack_cost_queried: attack_cost_queried[good],
            attack_cost: attack_costs[good],
        });
        if image.declaration.payments[accepter].authoritative_buckets[good]
            < offer.wrapping_add(attack_costs[good])
        {
            return Ok(Some(good));
        }
    }

    for good in 0..NUM_GOODS {
        let available = image.declaration.payments[accepter].type_available[good] == Some(true)
            && image.declaration.payments[proposer].type_available[good] == Some(true);
        if available {
            let offer =
                image.declaration.command.leaders[accepter].proposals[proposer].offers[good];
            if offer > 0 {
                debit_reservation(image, accepter, good, offer);
                steps.push(AcceptStep::ReserveOffer {
                    accepter,
                    proposer,
                    good,
                    amount: offer,
                });
            }
        }
    }
    for (good, amount) in attack_costs.into_iter().enumerate() {
        if amount == 0 {
            continue;
        }
        debit_reservation(image, accepter, good, amount);
        image.declaration.command.leaders[accepter].proposals[proposer].declaration_costs[good] =
            amount;
        steps.push(AcceptStep::ReserveAttackCost {
            accepter,
            proposer,
            good,
            amount,
        });
    }
    image.declaration.command.leaders[accepter].proposals[proposer].agreement_pending = 1;
    steps.push(AcceptStep::MarkAgreementPending { accepter, proposer });
    image.leaders[proposer].agenda_flags[accepter] |= 4;
    steps.push(AcceptStep::MarkReciprocalAgenda {
        leader: proposer,
        other: accepter,
    });
    sync_resource_projection(image);
    Ok(None)
}

fn set_diplo(
    image: &mut AcceptAuthorityImage,
    actor: usize,
    target: usize,
    state: Relation,
) -> Result<SetDiploPlan, AcceptHostError> {
    let plan = plan_set_diplo(
        &image.declaration.set_diplo,
        SetDiploRequest {
            actor,
            target,
            state,
        },
    )?;
    image.declaration.set_diplo = plan.after.clone();
    sync_set_diplo_projection(image);
    Ok(plan)
}

fn declare_wire(actor: usize, target: usize) -> Vec<u8> {
    let mut wire = vec![DECLARE_OPCODE];
    wire.extend_from_slice(&(actor as i32).to_le_bytes());
    wire.extend_from_slice(&(target as i32).to_le_bytes());
    wire.extend_from_slice(&0i32.to_le_bytes());
    wire
}

fn recursive_declare(
    image: &mut AcceptAuthorityImage,
    actor: usize,
    target: usize,
    pay: bool,
) -> Result<RecursiveDeclarePlan, AcceptHostError> {
    let retained_payment = image.declaration.payments[actor].clone();
    let retained_statistics = image.declaration.statistics[actor].clone();
    let mut nested_before = image.declaration.clone();
    nested_before.war_allowed = Some(true);
    if !pay {
        // A non-paying retail child skips afford/pay entirely. Marking every good
        // unavailable lets the already-landed opcode-38 planner supply the exact relation,
        // fanout, frame, and child-authority body without demanding facts retail never reads.
        nested_before.payments[actor].type_available = [Some(false); NUM_GOODS];
        nested_before.payments[actor].costs = [Some(0); NUM_GOODS];
    }
    let plan = plan_declare(&nested_before, &declare_wire(actor, target))?;
    let mut effective_after = plan.after.clone();
    if !pay {
        effective_after.payments[actor] = retained_payment;
        effective_after.command.leaders[actor].buckets =
            effective_after.payments[actor].authoritative_buckets;
        effective_after.statistics[actor] = retained_statistics;
    }
    image.declaration = effective_after.clone();
    sync_resource_projection(image);
    Ok(RecursiveDeclarePlan {
        actor,
        target,
        pay,
        plan,
        effective_after,
    })
}

fn accepted_content(command: &DiplomacyCommandState, accepter: usize, proposer: usize) -> bool {
    let forward = command.leaders[accepter].proposals[proposer];
    let reverse = command.leaders[proposer].proposals[accepter];
    forward.treaty != -1
        || forward.attacks.into_iter().any(|value| value != 0)
        || reverse.offers.into_iter().any(|value| value > 0)
}

fn alliance_admissible(image: &AcceptAuthorityImage, who: usize) -> Result<bool, AcceptHostError> {
    let team = image.leaders[who]
        .team_members_mode_one
        .ok_or(AcceptHostError::Missing(
            MissingAcceptFact::TeamMembersModeOne { who },
        ))?;
    let allies = image.leaders[who]
        .num_allies
        .ok_or(AcceptHostError::Missing(MissingAcceptFact::NumAllies {
            who,
        }))?;
    Ok(1i32.wrapping_sub(team).wrapping_add(allies) == 0)
}

pub fn plan_accept(
    before: &AcceptAuthorityImage,
    wire: &[u8],
) -> Result<AcceptPlan, AcceptHostError> {
    if wire.first().copied() != Some(ACCEPT_OPCODE) {
        return Err(AcceptHostError::NotAccept);
    }
    validate_accept_projection(before)?;
    let decision = plan_diplomacy_command(&before.declaration.command, wire)?;
    let boundary = match decision {
        DiplomacyPlanDecision::Boundary(boundary) => boundary,
        DiplomacyPlanDecision::Apply(_) => {
            return Err(AcceptHostError::UnexpectedCommandDecision);
        }
    };
    let (accepter, proposer) = match boundary.boundary {
        DiplomacyBoundary::AcceptTransferAndDiploChange { sender, target } => (sender, target),
        _ => return Err(AcceptHostError::UnexpectedCommandDecision),
    };
    if !matches!(boundary.command, DiplomacyWireCommand::Accept { .. }) {
        return Err(AcceptHostError::UnexpectedCommandDecision);
    }
    let mut after = before.clone();
    // The command prefix is itself persistent: both response counters clear before every return.
    for step in &boundary.prefix {
        match step {
            DiplomacyStep::ClearResponseCounter { field, .. } => match field {
                crate::command::diplomacy_command_plans::ResponseCounterField::TributeDemanded334 => {
                    after.declaration.command.leaders[accepter].response_334[proposer] = 0;
                }
                crate::command::diplomacy_command_plans::ResponseCounterField::Counteroffer314 => {
                    after.declaration.command.leaders[accepter].response_314[proposer] = 0;
                }
            },
            _ => return Err(AcceptHostError::UnexpectedCommandDecision),
        }
    }
    let mut steps: Vec<AcceptStep> = boundary
        .prefix
        .into_iter()
        .map(AcceptStep::Prefix)
        .collect();

    if after.declaration.command.leaders[accepter].proposals[proposer].agreement_pending != 1 {
        if let Some(good) = reserve_first_accept(&mut after, accepter, proposer, &mut steps)? {
            let attack_reached = steps.iter().any(|step| {
                matches!(step, AcceptStep::ReservationPreflight {
                    good: g,
                    attack_cost_queried: true,
                    ..
                } if *g == good)
            });
            if attack_reached && after.declaration.command.local_who == accepter as i32 {
                steps.push(AcceptStep::Presentation(
                    AcceptPresentation::CannotReserveAttackCost {
                        accepter,
                        proposer,
                        good,
                    },
                ));
            }
            validate_accept_projection(&after)?;
            return Ok(AcceptPlan {
                wire: wire.to_vec(),
                accepter,
                proposer,
                before: before.clone(),
                after,
                outcome: AcceptOutcome::RefusedInsufficientResources,
                steps,
            });
        }
    }

    let attacks = after.declaration.command.leaders[accepter].proposals[proposer].attacks;
    for candidate in 0..DIPLO_SLOTS {
        if candidate == accepter || candidate == proposer || attacks[candidate] == 0 {
            continue;
        }
        if after.declaration.command.is_ally(proposer, candidate)?
            || after.declaration.command.is_ally(accepter, candidate)?
        {
            clear_pair_and_records(&mut after, accepter, proposer, &mut steps);
            if after.declaration.command.local_who == accepter as i32
                || after.declaration.command.local_who == proposer as i32
            {
                steps.push(AcceptStep::Presentation(
                    AcceptPresentation::AttackConflict {
                        accepter,
                        proposer,
                        candidate,
                    },
                ));
            }
            validate_accept_projection(&after)?;
            return Ok(AcceptPlan {
                wire: wire.to_vec(),
                accepter,
                proposer,
                before: before.clone(),
                after,
                outcome: AcceptOutcome::ClearedAttackConflict,
                steps,
            });
        }
    }

    let reciprocal =
        after.declaration.command.leaders[proposer].proposals[accepter].agreement_pending == 1;
    let content = reciprocal || accepted_content(&after.declaration.command, accepter, proposer);
    if !reciprocal && content {
        if after.declaration.command.local_who == accepter as i32 {
            steps.push(AcceptStep::Presentation(
                AcceptPresentation::AwaitingReciprocal { accepter, proposer },
            ));
        }
        validate_accept_projection(&after)?;
        return Ok(AcceptPlan {
            wire: wire.to_vec(),
            accepter,
            proposer,
            before: before.clone(),
            after,
            outcome: AcceptOutcome::ReservedAwaitingReciprocal,
            steps,
        });
    }

    // Both root calls and the alliance gate use this object's directional proposal
    // (`0x006D0D06`, `0x006D100C`, and `0x006D105A`), not the reciprocal record.
    let accepted_treaty = after.declaration.command.leaders[accepter].proposals[proposer].treaty;
    if accepted_treaty == 2
        && (!alliance_admissible(&after, accepter)? || !alliance_admissible(&after, proposer)?)
    {
        clear_pair_and_records(&mut after, accepter, proposer, &mut steps);
        if after.declaration.command.local_who == accepter as i32
            || after.declaration.command.local_who == proposer as i32
        {
            steps.push(AcceptStep::Presentation(
                AcceptPresentation::InvalidAlliance { accepter, proposer },
            ));
        }
        validate_accept_projection(&after)?;
        return Ok(AcceptPlan {
            wire: wire.to_vec(),
            accepter,
            proposer,
            before: before.clone(),
            after,
            outcome: AcceptOutcome::ClearedInvalidAlliance,
            steps,
        });
    }

    let resources = plan_accepted_deal_resources(&after.resources, accepter, proposer)?;
    after.resources = resources.after.clone();
    sync_declaration_projection(&mut after);
    steps.push(AcceptStep::AcceptedResources(resources));

    if accepted_treaty != -1 {
        let current = after.declaration.command.setup.leaders[accepter].diplos[proposer];
        let state = relation(accepted_treaty.max(current))?;
        let plan = set_diplo(&mut after, accepter, proposer, state)?;
        steps.push(AcceptStep::RootSetDiplo {
            actor: accepter,
            target: proposer,
            plan,
        });
        let current = after.declaration.command.setup.leaders[proposer].diplos[accepter];
        let state = relation(accepted_treaty.max(current))?;
        let plan = set_diplo(&mut after, proposer, accepter, state)?;
        steps.push(AcceptStep::RootSetDiplo {
            actor: proposer,
            target: accepter,
            plan,
        });
        if accepted_treaty == 1 {
            let frame = after.declaration.command.frame;
            after.leaders[accepter].peace_frames[proposer] = frame;
            after.leaders[proposer].peace_frames[accepter] = frame;
            steps.push(AcceptStep::StampPeaceFrame {
                first: accepter,
                second: proposer,
                frame,
            });
        }

        let accepter_neutral =
            after.leaders[accepter]
                .is_neutral
                .ok_or(AcceptHostError::Missing(MissingAcceptFact::Neutral {
                    who: accepter,
                }))?;
        let proposer_neutral =
            after.leaders[proposer]
                .is_neutral
                .ok_or(AcceptHostError::Missing(MissingAcceptFact::Neutral {
                    who: proposer,
                }))?;
        if accepter_neutral || proposer_neutral {
            for candidate in 0..DIPLO_SLOTS {
                if candidate == accepter
                    || candidate == proposer
                    || !after.declaration.command.setup.leaders[candidate].is_present()
                {
                    continue;
                }
                let with_accepter = after
                    .declaration
                    .command
                    .is_runtime_team(candidate, accepter)?;
                let with_proposer = after
                    .declaration
                    .command
                    .is_runtime_team(candidate, proposer)?;
                let (other, desired) = if with_accepter && !with_proposer {
                    (
                        proposer,
                        after.declaration.command.setup.leaders[accepter].diplos[proposer],
                    )
                } else if !with_accepter && with_proposer {
                    (
                        accepter,
                        after.declaration.command.setup.leaders[proposer].diplos[accepter],
                    )
                } else {
                    continue;
                };
                if after.declaration.command.setup.leaders[candidate].diplos[other] >= desired {
                    continue;
                }
                let plan = set_diplo(&mut after, candidate, other, relation(desired)?)?;
                steps.push(AcceptStep::TeamFanoutSetDiplo {
                    team_member: candidate,
                    other,
                    plan,
                });
                steps.push(AcceptStep::NotifyDeal {
                    leader: candidate,
                    other,
                    treaty: desired,
                });
            }
        }
    }

    let war_allowed = after
        .declaration
        .war_allowed
        .ok_or(AcceptHostError::Missing(MissingAcceptFact::WarAllowed))?;
    if war_allowed {
        let attacks = after.declaration.command.leaders[accepter].proposals[proposer].attacks;
        for candidate in 0..DIPLO_SLOTS {
            if candidate == accepter || candidate == proposer || attacks[candidate] == 0 {
                continue;
            }
            let frame = after.declaration.command.frame;
            after.leaders[accepter].attack_frames[candidate] = frame;
            after.leaders[proposer].attack_frames[candidate] = frame;
            after.leaders[accepter].attack_peers[candidate] = proposer as i32;
            after.leaders[proposer].attack_peers[candidate] = accepter as i32;
            steps.push(AcceptStep::StampAttackContinuation {
                accepter,
                proposer,
                candidate,
                frame,
            });
            if !after.declaration.command.is_enemy(accepter, candidate)? {
                let pay = after.declaration.command.setup.leaders[accepter].leader_flags & 4 != 0;
                let plan = recursive_declare(&mut after, accepter, candidate, pay)?;
                steps.push(AcceptStep::RecursiveDeclare(plan));
            }
            if !after.declaration.command.is_enemy(proposer, candidate)? {
                let pay = after.declaration.command.setup.leaders[proposer].leader_flags & 4 != 0;
                let plan = recursive_declare(&mut after, proposer, candidate, pay)?;
                steps.push(AcceptStep::RecursiveDeclare(plan));
            }
        }
    }

    after.declaration.command.leaders[accepter].proposals[proposer] = DiplomacyProposal::default();
    steps.push(AcceptStep::ClearProposalRecord {
        leader: accepter,
        target: proposer,
    });
    after.declaration.command.leaders[proposer].proposals[accepter] = DiplomacyProposal::default();
    steps.push(AcceptStep::ClearProposalRecord {
        leader: proposer,
        target: accepter,
    });
    sync_resource_projection(&mut after);
    if content {
        if after.declaration.command.local_who == proposer as i32 {
            steps.push(AcceptStep::Presentation(AcceptPresentation::AcceptedDeal {
                accepter,
                proposer,
            }));
        }
        steps.push(AcceptStep::NotifyDeal {
            leader: accepter,
            other: proposer,
            treaty: accepted_treaty,
        });
        steps.push(AcceptStep::NotifyDeal {
            leader: proposer,
            other: accepter,
            treaty: accepted_treaty,
        });
    }
    validate_accept_projection(&after)?;
    Ok(AcceptPlan {
        wire: wire.to_vec(),
        accepter,
        proposer,
        before: before.clone(),
        after,
        outcome: AcceptOutcome::Accepted,
        steps,
    })
}

fn set_authority(
    actor: usize,
    target: usize,
    plan: &SetDiploPlan,
) -> impl Iterator<Item = AcceptAuthority> + '_ {
    plan.steps.iter().filter_map(move |step| match step {
        SetDiploStep::Authority(call) => Some(AcceptAuthority::SetDiplo {
            actor,
            target,
            call: *call,
        }),
        _ => None,
    })
}

fn declare_authority(plan: &RecursiveDeclarePlan) -> Vec<AcceptAuthority> {
    plan.plan
        .steps
        .iter()
        .flat_map(|step| match step {
            DeclareStep::RootSetDiplo(set) | DeclareStep::AllySetDiplo { plan: set, .. } => set
                .steps
                .iter()
                .filter_map(|step| match step {
                    SetDiploStep::Authority(call) => Some(AcceptAuthority::RecursiveDeclare {
                        actor: plan.actor,
                        target: plan.target,
                        call: *call,
                    }),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

pub fn required_accept_authority(steps: &[AcceptStep]) -> Vec<AcceptAuthority> {
    let mut out = Vec::new();
    for step in steps {
        match step {
            AcceptStep::AcceptedResources(plan) => {
                for step in &plan.steps {
                    if let AcceptedDealResourceStep::ConsiderTribute {
                        receiver,
                        sender,
                        good,
                        raw,
                    } = step
                    {
                        out.push(AcceptAuthority::ConsiderTribute {
                            receiver: *receiver,
                            sender: *sender,
                            good: *good,
                            raw: *raw,
                        });
                    }
                }
            }
            AcceptStep::RootSetDiplo {
                actor,
                target,
                plan,
            } => out.extend(set_authority(*actor, *target, plan)),
            AcceptStep::TeamFanoutSetDiplo {
                team_member,
                other,
                plan,
            } => out.extend(set_authority(*team_member, *other, plan)),
            AcceptStep::RecursiveDeclare(plan) => out.extend(declare_authority(plan)),
            AcceptStep::NotifyDeal {
                leader,
                other,
                treaty,
            } => out.push(AcceptAuthority::NotifyDeal {
                leader: *leader,
                other: *other,
                treaty: *treaty,
            }),
            _ => {}
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptReceipt {
    pub before: AcceptAuthorityImage,
    pub wire: Vec<u8>,
    pub status: AcceptTransactionStatus,
    pub plan: Option<AcceptPlan>,
    pub authority: Vec<AcceptAuthority>,
}

impl AcceptReceipt {
    pub fn validates(&self) -> bool {
        let planned = plan_accept(&self.before, &self.wire);
        match self.status {
            AcceptTransactionStatus::Unavailable => {
                planned.is_ok() && self.plan.is_none() && self.authority.is_empty()
            }
            AcceptTransactionStatus::Applied => {
                let Ok(plan) = planned else { return false };
                self.plan.as_ref() == Some(&plan)
                    && self.authority == required_accept_authority(&plan.steps)
            }
        }
    }
}

pub fn apply_accept_transaction(
    current: &mut AcceptAuthorityImage,
    before: AcceptAuthorityImage,
    wire: Vec<u8>,
    completed_authority: Vec<AcceptAuthority>,
) -> AcceptReceipt {
    if current != &before {
        return AcceptReceipt {
            before,
            wire,
            status: AcceptTransactionStatus::Unavailable,
            plan: None,
            authority: Vec::new(),
        };
    }
    let Ok(plan) = plan_accept(&before, &wire) else {
        return AcceptReceipt {
            before,
            wire,
            status: AcceptTransactionStatus::Unavailable,
            plan: None,
            authority: Vec::new(),
        };
    };
    if completed_authority != required_accept_authority(&plan.steps) {
        return AcceptReceipt {
            before,
            wire,
            status: AcceptTransactionStatus::Unavailable,
            plan: None,
            authority: Vec::new(),
        };
    }
    *current = plan.after.clone();
    AcceptReceipt {
        before,
        wire,
        status: AcceptTransactionStatus::Applied,
        plan: Some(plan),
        authority: completed_authority,
    }
}
