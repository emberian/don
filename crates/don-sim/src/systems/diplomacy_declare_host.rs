// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic composition of opcode 38 with the recovered payment and `set_diplo` owners.
//!
//! This module owns no shadow relation or resource store.  [`DeclareAuthorityImage`]
//! deliberately carries the representations which still have separate owners in `Sim`, and
//! [`validate_projection`] refuses them unless every overlapping retail field agrees.  A later
//! integration may replace the projections with borrows into one canonical leader object; it
//! must not weaken the invariant.

use super::leader_set_diplo::{
    plan_declaration_payments, plan_set_diplo, DeclarationPaymentError, DeclarationPaymentImage,
    DeclarationPaymentPlan, Relation, SetDiploAuthority, SetDiploImage, SetDiploPlan,
    SetDiploPlanError, SetDiploRequest, SetDiploStep, DIPLO_SLOTS,
};
use crate::command::diplomacy_command_plans::{
    plan_diplomacy_command, DiplomacyBoundary, DiplomacyCommandState, DiplomacyPlanDecision,
    DiplomacyPlanError, DiplomacyStep, DiplomacyWireCommand, DECLARE_OPCODE,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeclarationStatistics {
    /// `LeaderData+0x20c`.
    pub repeated_targets: i32,
    /// `LeaderData+0x270 + target*4`.
    pub by_target: [i32; DIPLO_SLOTS],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclareAuthorityImage {
    /// Command/proposal view, including setup-team facts and decoded authoritative buckets.
    pub command: DiplomacyCommandState,
    /// Full relation/vision/ejection/victory/army owner for `Leader::set_diplo`.
    pub set_diplo: SetDiploImage,
    /// Per-leader authoritative and `LeaderData+0x468` resource images plus DOW cost facts.
    pub payments: [DeclarationPaymentImage; DIPLO_SLOTS],
    pub statistics: [DeclarationStatistics; DIPLO_SLOTS],
    /// Exact `Game::war_allowed` result.  Read only for an effective war declaration.
    pub war_allowed: Option<bool>,
}

impl Default for DeclareAuthorityImage {
    fn default() -> Self {
        Self {
            command: DiplomacyCommandState::default(),
            set_diplo: SetDiploImage::default(),
            payments: std::array::from_fn(|_| DeclarationPaymentImage::default()),
            statistics: [DeclarationStatistics::default(); DIPLO_SLOTS],
            war_allowed: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionField {
    Identity,
    LeaderFlags,
    Relation,
    LocalWho,
    AuthoritativeBucket,
    Frame,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclareHostError {
    Command(DiplomacyPlanError),
    NotDeclare,
    UnexpectedCommandDecision,
    ProjectionMismatch {
        field: ProjectionField,
        first: usize,
        second: usize,
    },
    UnsupportedRelation(i32),
    MissingWarAllowed,
    Payment(DeclarationPaymentError),
    SetDiplo(SetDiploPlanError),
}

impl From<DiplomacyPlanError> for DeclareHostError {
    fn from(value: DiplomacyPlanError) -> Self {
        Self::Command(value)
    }
}

impl From<DeclarationPaymentError> for DeclareHostError {
    fn from(value: DeclarationPaymentError) -> Self {
        Self::Payment(value)
    }
}

impl From<SetDiploPlanError> for DeclareHostError {
    fn from(value: SetDiploPlanError) -> Self {
        Self::SetDiplo(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarePresentation {
    WarNotAllowed {
        sender: usize,
        target: usize,
    },
    CannotAfford {
        sender: usize,
        target: usize,
        /// Retail reports the good only for exactly one shortage; multiple shortages use -1.
        short_good: Option<usize>,
    },
    LocalDeclaration {
        sender: usize,
        target: usize,
        relation: Relation,
    },
    LocalAllyFanout {
        sender: usize,
        target: usize,
        relation: Relation,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclareStep {
    Prefix(DiplomacyStep),
    Payment(DeclarationPaymentPlan),
    IncrementTargetStatistic {
        sender: usize,
        target: usize,
    },
    IncrementRepeatedTargetStatistic {
        sender: usize,
    },
    StampDeclarationFrame {
        sender: usize,
        target: usize,
        frame: i32,
    },
    RootSetDiplo(SetDiploPlan),
    AllySetDiplo {
        team_member: usize,
        other: usize,
        plan: SetDiploPlan,
    },
    Presentation(DeclarePresentation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclarePlan {
    pub wire: Vec<u8>,
    pub before: DeclareAuthorityImage,
    pub after: DeclareAuthorityImage,
    pub steps: Vec<DeclareStep>,
}

fn relation(raw: i32) -> Result<Relation, DeclareHostError> {
    match raw {
        0 => Ok(Relation::War),
        1 => Ok(Relation::Peace),
        2 => Ok(Relation::Ally),
        value => Err(DeclareHostError::UnsupportedRelation(value)),
    }
}

fn raw_relation(value: Relation) -> i32 {
    value as i32
}

pub fn validate_projection(image: &DeclareAuthorityImage) -> Result<(), DeclareHostError> {
    if image.command.local_who != image.set_diplo.local_who.unwrap_or(i32::MIN) {
        return Err(DeclareHostError::ProjectionMismatch {
            field: ProjectionField::LocalWho,
            first: 0,
            second: 0,
        });
    }
    if image.command.frame != image.command.setup.frame {
        return Err(DeclareHostError::ProjectionMismatch {
            field: ProjectionField::Frame,
            first: 0,
            second: 0,
        });
    }
    for first in 0..DIPLO_SLOTS {
        let command = &image.command.setup.leaders[first];
        let set = &image.set_diplo.leaders[first];
        if set.who != Some(command.who) {
            return Err(DeclareHostError::ProjectionMismatch {
                field: ProjectionField::Identity,
                first,
                second: first,
            });
        }
        if set.leader_flags != Some(command.leader_flags as u32) {
            return Err(DeclareHostError::ProjectionMismatch {
                field: ProjectionField::LeaderFlags,
                first,
                second: first,
            });
        }
        if image.payments[first].authoritative_buckets != image.command.leaders[first].buckets {
            return Err(DeclareHostError::ProjectionMismatch {
                field: ProjectionField::AuthoritativeBucket,
                first,
                second: first,
            });
        }
        for second in 0..DIPLO_SLOTS {
            let expected = relation(command.diplos[second])?;
            if set.diplos[second] != Some(expected) {
                return Err(DeclareHostError::ProjectionMismatch {
                    field: ProjectionField::Relation,
                    first,
                    second,
                });
            }
        }
    }
    Ok(())
}

fn sync_set_diplo_projection(image: &mut DeclareAuthorityImage) {
    for first in 0..DIPLO_SLOTS {
        for second in 0..DIPLO_SLOTS {
            if let Some(value) = image.set_diplo.leaders[first].diplos[second] {
                image.command.setup.leaders[first].diplos[second] = raw_relation(value);
            }
        }
    }
}

fn increment_statistic(
    image: &mut DeclareAuthorityImage,
    sender: usize,
    target: usize,
    steps: &mut Vec<DeclareStep>,
) {
    let row = &mut image.statistics[sender];
    if row.by_target[target] != 0 {
        row.repeated_targets = row.repeated_targets.wrapping_add(1);
        steps.push(DeclareStep::IncrementRepeatedTargetStatistic { sender });
    }
    row.by_target[target] = row.by_target[target].wrapping_add(1);
    steps.push(DeclareStep::IncrementTargetStatistic { sender, target });
}

fn required_authority(steps: &[DeclareStep]) -> Vec<SetDiploAuthority> {
    steps
        .iter()
        .flat_map(|step| match step {
            DeclareStep::RootSetDiplo(plan) | DeclareStep::AllySetDiplo { plan, .. } => plan
                .steps
                .iter()
                .filter_map(|step| match step {
                    SetDiploStep::Authority(call) => Some(*call),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect()
}

pub fn plan_declare(
    before: &DeclareAuthorityImage,
    wire: &[u8],
) -> Result<DeclarePlan, DeclareHostError> {
    if wire.first().copied() != Some(DECLARE_OPCODE) {
        return Err(DeclareHostError::NotDeclare);
    }
    validate_projection(before)?;
    let decision = plan_diplomacy_command(&before.command, wire)?;
    let mut after = before.clone();
    let mut steps = Vec::new();
    let boundary = match decision {
        DiplomacyPlanDecision::Apply(prefix) => {
            steps.extend(prefix.steps.into_iter().map(DeclareStep::Prefix));
            after.command = prefix.state;
            return Ok(DeclarePlan {
                wire: wire.to_vec(),
                before: before.clone(),
                after,
                steps,
            });
        }
        DiplomacyPlanDecision::Boundary(boundary) => boundary,
    };
    steps.extend(boundary.prefix.into_iter().map(DeclareStep::Prefix));
    let (sender, target, effective_raw) = match boundary.boundary {
        DiplomacyBoundary::DeclarationResourceAndDiploChange {
            sender,
            target,
            effective_treaty,
            ..
        } => (sender, target, effective_treaty),
        _ => return Err(DeclareHostError::UnexpectedCommandDecision),
    };
    if !matches!(boundary.command, DiplomacyWireCommand::Declare { .. }) {
        return Err(DeclareHostError::UnexpectedCommandDecision);
    }
    let effective = relation(effective_raw)?;
    let old_ally = before.command.is_ally(sender, target)?;

    // `afford_dow`: non-war requests between parties which are not already mutual allies
    // return true before querying any bucket. `pay_dow` still runs and clamps the debit.
    // Otherwise remember the unique shortage, or the -1/multiple sentinel.
    let payment = &before.payments[sender];
    let mut shortages = Vec::new();
    if effective == Relation::War || old_ally {
        for good in 0..super::leader_set_diplo::NUM_GOODS {
            let available = payment.type_available[good]
                .ok_or(DeclarationPaymentError::MissingTypeAvailability { good })?;
            if !available {
                continue;
            }
            let cost = payment.costs[good].ok_or(DeclarationPaymentError::MissingCost { good })?;
            if payment.authoritative_buckets[good] < cost {
                shortages.push(good);
            }
        }
    }
    if !shortages.is_empty() {
        if before.command.local_who == sender as i32 {
            steps.push(DeclareStep::Presentation(
                DeclarePresentation::CannotAfford {
                    sender,
                    target,
                    short_good: (shortages.len() == 1).then_some(shortages[0]),
                },
            ));
        }
        return Ok(DeclarePlan {
            wire: wire.to_vec(),
            before: before.clone(),
            after,
            steps,
        });
    }

    // Retail reaches `Game::war_allowed` only after `afford_dow` succeeds. This order is
    // visible when both gates would refuse: the local player sees the shortage, not the
    // no-war notice.
    if effective == Relation::War {
        let allowed = before
            .war_allowed
            .ok_or(DeclareHostError::MissingWarAllowed)?;
        if !allowed {
            if before.command.local_who == sender as i32 {
                steps.push(DeclareStep::Presentation(
                    DeclarePresentation::WarNotAllowed { sender, target },
                ));
            }
            return Ok(DeclarePlan {
                wire: wire.to_vec(),
                before: before.clone(),
                after,
                steps,
            });
        }
    }

    let payment_count = 1 + usize::from(effective == Relation::War && old_ally);
    let payment_plan = plan_declaration_payments(&before.payments[sender], payment_count)?;
    after.payments[sender] = payment_plan.after.clone();
    after.command.leaders[sender].buckets = payment_plan.after.authoritative_buckets;
    steps.push(DeclareStep::Payment(payment_plan));

    // Exact `0x006DAD03..0x006DAD63` counter order. The second old-ally increment is
    // reachable in the general action body; opcode 38 rewrites allied war to peace first.
    if effective == Relation::War {
        if old_ally || after.statistics[sender].by_target[target] < 5 {
            increment_statistic(&mut after, sender, target, &mut steps);
        }
    }
    if old_ally {
        increment_statistic(&mut after, sender, target, &mut steps);
        after.command.leaders[sender].declaration_frame[target] = after.command.frame;
        steps.push(DeclareStep::StampDeclarationFrame {
            sender,
            target,
            frame: after.command.frame,
        });
    }

    let root = plan_set_diplo(
        &after.set_diplo,
        SetDiploRequest {
            actor: sender,
            target,
            state: effective,
        },
    )?;
    after.set_diplo = root.after.clone();
    sync_set_diplo_projection(&mut after);
    steps.push(DeclareStep::RootSetDiplo(root));

    // `action_declare`'s slot-order third-party team fanout. Each ally call uses the
    // after-image of the previous call, so vision/ejection/army consequences cannot split.
    for candidate in 0..DIPLO_SLOTS {
        if candidate == sender || candidate == target {
            continue;
        }
        if after.command.setup.leaders[candidate].leader_flags & 1 == 0 {
            continue;
        }
        let with_sender = after.command.is_runtime_team(candidate, sender)?;
        let with_target = after.command.is_runtime_team(candidate, target)?;
        if with_sender == with_target {
            continue;
        }
        let other = if with_sender { target } else { sender };
        if effective_raw >= after.command.setup.leaders[candidate].diplos[other] {
            continue;
        }
        let plan = plan_set_diplo(
            &after.set_diplo,
            SetDiploRequest {
                actor: candidate,
                target: other,
                state: effective,
            },
        )?;
        after.set_diplo = plan.after.clone();
        sync_set_diplo_projection(&mut after);
        steps.push(DeclareStep::AllySetDiplo {
            team_member: candidate,
            other,
            plan,
        });
        if after.command.local_who == candidate as i32 {
            steps.push(DeclareStep::Presentation(
                DeclarePresentation::LocalAllyFanout {
                    sender: candidate,
                    target: other,
                    relation: effective,
                },
            ));
        }
    }
    if after.command.local_who == sender as i32 {
        steps.push(DeclareStep::Presentation(
            DeclarePresentation::LocalDeclaration {
                sender,
                target,
                relation: effective,
            },
        ));
    }
    validate_projection(&after)?;
    Ok(DeclarePlan {
        wire: wire.to_vec(),
        before: before.clone(),
        after,
        steps,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclareTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclareReceipt {
    pub before: DeclareAuthorityImage,
    pub wire: Vec<u8>,
    pub status: DeclareTransactionStatus,
    pub plan: Option<DeclarePlan>,
    pub authority: Vec<SetDiploAuthority>,
}

impl DeclareReceipt {
    pub fn validates(&self) -> bool {
        let planned = plan_declare(&self.before, &self.wire);
        match self.status {
            DeclareTransactionStatus::Unavailable => {
                planned.is_ok() && self.plan.is_none() && self.authority.is_empty()
            }
            DeclareTransactionStatus::Applied => {
                let Ok(plan) = planned else { return false };
                self.plan.as_ref() == Some(&plan)
                    && self.authority == required_authority(&plan.steps)
            }
        }
    }
}

/// In-memory reference host used by the real Bridge mounting test.  The caller supplies only
/// authority calls it actually completed.  Stale images, missing calls, and planner errors leave
/// the caller-visible state byte-for-byte unchanged.
pub fn apply_declare_transaction(
    current: &mut DeclareAuthorityImage,
    before: DeclareAuthorityImage,
    wire: Vec<u8>,
    completed_authority: Vec<SetDiploAuthority>,
) -> DeclareReceipt {
    if current != &before {
        return DeclareReceipt {
            before,
            wire,
            status: DeclareTransactionStatus::Unavailable,
            plan: None,
            authority: Vec::new(),
        };
    }
    let Ok(plan) = plan_declare(&before, &wire) else {
        return DeclareReceipt {
            before,
            wire,
            status: DeclareTransactionStatus::Unavailable,
            plan: None,
            authority: Vec::new(),
        };
    };
    if completed_authority != required_authority(&plan.steps) {
        return DeclareReceipt {
            before,
            wire,
            status: DeclareTransactionStatus::Unavailable,
            plan: None,
            authority: Vec::new(),
        };
    }
    *current = plan.after.clone();
    DeclareReceipt {
        before,
        wire,
        status: DeclareTransactionStatus::Applied,
        plan: Some(plan),
        authority: completed_authority,
    }
}
