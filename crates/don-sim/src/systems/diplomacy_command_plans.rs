// SPDX-License-Identifier: GPL-3.0-or-later
//! Transaction plans for retail diplomacy command opcodes 37 through 45.
//!
//! The small proposal actions are reproduced here without depending on the command bridge's
//! shared dispatcher.  The two large branches (`DECLARE` and `ACCEPT`) stop at an explicit
//! boundary: retail continues through declaration costs, resource transfer,
//! `Leader::set_diplo`, unit retargeting, vision, victory, and event callbacks.
//! Mode-2 `REJECT` is complete: its apparent recursive declaration is unreachable under
//! retail's required `leader slot == LeaderData::who` invariant.
//! A host cannot turn one of those boundary descriptions into an applied receipt.
//!
//! [`SetupDiplomacy`] remains the sole owner of retail team lookup.  This module delegates
//! runtime team questions to it and owns only the command-action records at
//! `LeaderData+0x692c`.

use super::setup_diplomacy::{IsTeamArg, SetupDiplomacy, TeamQueryError, SETUP_SLOTS};

pub const DIPLOMACY_SLOTS: usize = SETUP_SLOTS;
pub const NUM_GOODS: usize = 6;

pub const TREATY_OPCODE: u8 = 37;
pub const DECLARE_OPCODE: u8 = 38;
pub const CLEAR_TRIBUTES_OPCODE: u8 = 39;
pub const CLEAR_ALL_OPCODE: u8 = 40;
pub const ACCEPT_OPCODE: u8 = 41;
pub const REJECT_OPCODE: u8 = 42;
pub const TRIBUTE_OPCODE: u8 = 43;
pub const DEMAND_TRIBUTE_OPCODE: u8 = 44;
pub const PROPOSE_ATTACK_OPCODE: u8 = 45;

/// One 92-byte `Diplomacy` record (`LeaderData+0x692c + target*0x5c`).
///
/// Names which survive in the PDB are used where possible.  The first two dwords have no
/// recovered field symbols, so their names describe only how the action cohort uses them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiplomacyProposal {
    /// Dword `+0x00`, tested by `Leader::clear_agree` and `action_respond`.
    pub agreement_pending: i32,
    /// Dword `+0x04`, set before proposal edits and cleared by `clear_agree`.
    pub proposal_open: i32,
    /// Dword `+0x08`; `Diplomacy::clear_all` restores the sentinel `-1`.
    pub treaty: i32,
    /// Dwords `+0x0c..+0x20`, cleared by `Diplomacy::clear_offers`.
    pub offers: [i32; NUM_GOODS],
    /// Dwords `+0x24..+0x38`, cleared by `Diplomacy::clear_dows`.
    pub declaration_costs: [i32; NUM_GOODS],
    /// Dwords `+0x3c..+0x58`, queried by `Diplomacy::num_attacks`.
    pub attacks: [i32; DIPLOMACY_SLOTS],
}

impl Default for DiplomacyProposal {
    fn default() -> Self {
        Self {
            agreement_pending: 0,
            proposal_open: 0,
            treaty: -1,
            offers: [0; NUM_GOODS],
            declaration_costs: [0; NUM_GOODS],
            attacks: [0; DIPLOMACY_SLOTS],
        }
    }
}

impl DiplomacyProposal {
    #[inline]
    pub fn has_any_proposal(self) -> bool {
        self.treaty != -1
            || self.offers.into_iter().any(|value| value != 0)
            || self.attacks.into_iter().any(|value| value != 0)
    }

    #[inline]
    fn clear_offers(&mut self) {
        self.offers = [0; NUM_GOODS];
    }

    #[inline]
    fn clear_all(&mut self) {
        *self = Self::default();
    }
}

/// Action-owned state adjacent to one retail `LeaderData` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyLeaderState {
    pub proposals: [DiplomacyProposal; DIPLOMACY_SLOTS],
    /// Decoded resource buckets reached through `LeaderData+0x6eb8`.
    pub buckets: [i32; NUM_GOODS],
    /// Six dwords at `LeaderData+0x498`, debited before `bucket_add` by `clear_agree`.
    pub reserved_resources: [i32; NUM_GOODS],
    /// `LeaderData+0x314 + target*4`, cleared at entry to `action_respond`.
    pub response_314: [i32; DIPLOMACY_SLOTS],
    /// `LeaderData+0x334 + target*4`, cleared at entry to `action_respond`.
    pub response_334: [i32; DIPLOMACY_SLOTS],
    /// `LeaderData+0x2b0 + target*4`, used by the no-rush declaration gate.
    pub declaration_frame: [i32; DIPLOMACY_SLOTS],
}

impl Default for DiplomacyLeaderState {
    fn default() -> Self {
        Self {
            proposals: [DiplomacyProposal::default(); DIPLOMACY_SLOTS],
            buckets: [0; NUM_GOODS],
            reserved_resources: [0; NUM_GOODS],
            response_314: [0; DIPLOMACY_SLOTS],
            response_334: [0; DIPLOMACY_SLOTS],
            declaration_frame: [0; DIPLOMACY_SLOTS],
        }
    }
}

/// Bounded image required by the recovered command prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyCommandState {
    /// Team, leader identity, presence, and directional declaration fields.
    pub setup: SetupDiplomacy,
    pub leaders: [DiplomacyLeaderState; DIPLOMACY_SLOTS],
    pub frame: i32,
    /// `Constants+0xcf8`, compared with `frame - max(last-declaration stamps)`.
    pub no_rush_frames: i32,
    /// `Console+0x298`.  It affects only presentation receipts, never proposal state.
    pub local_who: i32,
}

impl Default for DiplomacyCommandState {
    fn default() -> Self {
        Self {
            setup: SetupDiplomacy::default(),
            leaders: std::array::from_fn(|_| DiplomacyLeaderState::default()),
            frame: 0,
            no_rush_frames: 0,
            local_who: -1,
        }
    }
}

/// Exact fixed-wire body decoded from one opcode in the cohort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiplomacyWireCommand {
    Treaty {
        sender: i32,
        target: i32,
        treaty: i32,
    },
    Declare {
        sender: i32,
        target: i32,
        treaty: i32,
    },
    ClearTributes {
        sender: i32,
        target: i32,
    },
    ClearAll {
        sender: i32,
        target: i32,
    },
    Accept {
        sender: i32,
        target: i32,
    },
    Reject {
        sender: i32,
        target: i32,
    },
    Offer {
        sender: i32,
        target: i32,
        good: i32,
        /// Signed delta passed to `Leader::action_offer`; demand packets negate their
        /// raw amount with x86 wrapping semantics.
        amount: i32,
        demand: bool,
    },
    ProposeAttack {
        sender: i32,
        target: i32,
        whose: i32,
        onoff: i32,
    },
}

impl DiplomacyWireCommand {
    #[inline]
    pub const fn sender(self) -> i32 {
        match self {
            Self::Treaty { sender, .. }
            | Self::Declare { sender, .. }
            | Self::ClearTributes { sender, .. }
            | Self::ClearAll { sender, .. }
            | Self::Accept { sender, .. }
            | Self::Reject { sender, .. }
            | Self::Offer { sender, .. }
            | Self::ProposeAttack { sender, .. } => sender,
        }
    }

    #[inline]
    pub const fn target(self) -> i32 {
        match self {
            Self::Treaty { target, .. }
            | Self::Declare { target, .. }
            | Self::ClearTributes { target, .. }
            | Self::ClearAll { target, .. }
            | Self::Accept { target, .. }
            | Self::Reject { target, .. }
            | Self::Offer { target, .. }
            | Self::ProposeAttack { target, .. } => target,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiplomacyDecodeError {
    UnsupportedOpcode(u8),
    WrongWireLength {
        opcode: u8,
        expected: usize,
        actual: usize,
    },
}

#[inline]
fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("length checked"),
    )
}

pub fn decode_diplomacy_command(
    bytes: &[u8],
) -> Result<DiplomacyWireCommand, DiplomacyDecodeError> {
    let opcode = bytes.first().copied().unwrap_or(u8::MAX);
    let expected = match opcode {
        TREATY_OPCODE | DECLARE_OPCODE => 13,
        CLEAR_TRIBUTES_OPCODE | CLEAR_ALL_OPCODE | ACCEPT_OPCODE | REJECT_OPCODE => 9,
        TRIBUTE_OPCODE | DEMAND_TRIBUTE_OPCODE | PROPOSE_ATTACK_OPCODE => 17,
        _ => return Err(DiplomacyDecodeError::UnsupportedOpcode(opcode)),
    };
    if bytes.len() != expected {
        return Err(DiplomacyDecodeError::WrongWireLength {
            opcode,
            expected,
            actual: bytes.len(),
        });
    }
    let sender = i32_at(bytes, 1);
    let target = i32_at(bytes, 5);
    Ok(match opcode {
        TREATY_OPCODE => DiplomacyWireCommand::Treaty {
            sender,
            target,
            treaty: i32_at(bytes, 9),
        },
        DECLARE_OPCODE => DiplomacyWireCommand::Declare {
            sender,
            target,
            treaty: i32_at(bytes, 9),
        },
        CLEAR_TRIBUTES_OPCODE => DiplomacyWireCommand::ClearTributes { sender, target },
        CLEAR_ALL_OPCODE => DiplomacyWireCommand::ClearAll { sender, target },
        ACCEPT_OPCODE => DiplomacyWireCommand::Accept { sender, target },
        REJECT_OPCODE => DiplomacyWireCommand::Reject { sender, target },
        TRIBUTE_OPCODE | DEMAND_TRIBUTE_OPCODE => {
            let raw = i32_at(bytes, 13);
            let demand = opcode == DEMAND_TRIBUTE_OPCODE;
            DiplomacyWireCommand::Offer {
                sender,
                target,
                good: i32_at(bytes, 9),
                amount: if demand { raw.wrapping_neg() } else { raw },
                demand,
            }
        }
        PROPOSE_ATTACK_OPCODE => DiplomacyWireCommand::ProposeAttack {
            sender,
            target,
            whose: i32_at(bytes, 9),
            onoff: i32_at(bytes, 13),
        },
        _ => unreachable!(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiplomacyPlanError {
    Decode(DiplomacyDecodeError),
    LeaderOutOfRange {
        value: i32,
    },
    GoodOutOfRange {
        value: i32,
    },
    AttackLeaderOutOfRange {
        value: i32,
    },
    /// Retail assumes the setup invariant `LeaderData::who == leader slot`.  A product
    /// image violating it is rejected rather than translating writes to another row.
    LeaderIdentityMismatch {
        slot: usize,
        who: i32,
    },
    TeamQuery(TeamQueryError),
}

impl From<DiplomacyDecodeError> for DiplomacyPlanError {
    fn from(value: DiplomacyDecodeError) -> Self {
        Self::Decode(value)
    }
}

impl From<TeamQueryError> for DiplomacyPlanError {
    fn from(value: TeamQueryError) -> Self {
        Self::TeamQuery(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalNotice {
    TributesCleared,
    ProposalsCleared,
    Rejected,
    CounterproposalRejected,
    NoRushDeclaration,
    InsufficientTribute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseCounterField {
    /// Retail `LeaderData+0x334`, cleared first by `action_respond`.
    TributeDemanded334,
    /// Retail `LeaderData+0x314`, cleared second by `action_respond`.
    Counteroffer314,
}

/// Instruction-ordered evidence from the action prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiplomacyStep {
    ClickStamp {
        sender: usize,
        target: usize,
    },
    MarkProposalOpen {
        sender: usize,
        target: usize,
    },
    ClearAgreement {
        leader: usize,
        target: usize,
    },
    ClearOffers {
        sender: usize,
        target: usize,
    },
    ClearAll {
        sender: usize,
        target: usize,
    },
    /// One directional 92-byte record clear. Mode-2 reject uses two of these so its
    /// retail reverse-pair-first order remains observable in the receipt.
    ClearProposalRecord {
        leader: usize,
        target: usize,
    },
    WriteTreaty {
        sender: usize,
        target: usize,
        treaty: i32,
    },
    WriteOffer {
        sender: usize,
        target: usize,
        good: usize,
        amount: i32,
    },
    WriteAttack {
        sender: usize,
        target: usize,
        whose: usize,
        onoff: i32,
    },
    ClearResponseCounter {
        sender: usize,
        target: usize,
        field: ResponseCounterField,
    },
    /// Mode-2 `action_respond` examined one nonzero attack counterproposal, but retail's
    /// next self-enemy query was false under the validated `slot == who` invariant.  The
    /// otherwise recursive `action_declare` call is therefore unreachable.
    RejectCounterproposalSelfGate {
        sender: usize,
        target: usize,
        candidate: usize,
    },
    DeclarationAlreadyEnemy {
        sender: usize,
        target: usize,
    },
    RewriteAlliedWarRequestToNeutral {
        sender: usize,
        target: usize,
    },
    RefuseInsufficientTribute {
        sender: usize,
        target: usize,
        good: usize,
        amount: i32,
    },
    LocalNotice {
        who: usize,
        notice: LocalNotice,
    },
    /// Ordered `SoundGlobal::play` presentation request.  Planning consumes no sound RNG;
    /// the presentation host owns that stream when it delivers the retained receipt.
    Sound {
        category: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiplomacyBoundary {
    /// `LeaderData::afford_dow` / `pay_dow`, followed by `Leader::set_diplo` and its
    /// retarget, vision, victory, army, and event tail.
    DeclarationResourceAndDiploChange {
        sender: usize,
        target: usize,
        requested_treaty: i32,
        effective_treaty: i32,
    },
    /// `action_respond(..., 1)`: tribute movement, attack proposals, treaty agreement,
    /// `set_diplo`, and product events.
    AcceptTransferAndDiploChange { sender: usize, target: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyCommandPlan {
    pub command: DiplomacyWireCommand,
    pub state: DiplomacyCommandState,
    pub steps: Vec<DiplomacyStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyBoundaryPlan {
    pub command: DiplomacyWireCommand,
    /// Prefix observations only.  They are not authorized mutations.
    pub prefix: Vec<DiplomacyStep>,
    pub boundary: DiplomacyBoundary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiplomacyPlanDecision {
    Apply(DiplomacyCommandPlan),
    Boundary(DiplomacyBoundaryPlan),
}

impl DiplomacyCommandState {
    fn slot(&self, value: i32) -> Result<usize, DiplomacyPlanError> {
        usize::try_from(value)
            .ok()
            .filter(|&slot| slot < DIPLOMACY_SLOTS)
            .ok_or(DiplomacyPlanError::LeaderOutOfRange { value })
    }

    fn pair(&self, sender: i32, target: i32) -> Result<(usize, usize), DiplomacyPlanError> {
        let sender = self.slot(sender)?;
        let target = self.slot(target)?;
        for slot in [sender, target] {
            let who = self.setup.leaders[slot].who;
            if who != slot as i32 {
                return Err(DiplomacyPlanError::LeaderIdentityMismatch { slot, who });
            }
        }
        Ok((sender, target))
    }

    /// `LeaderData::get_diplo` `0x006EBA50`.
    pub fn get_diplo(&self, sender: usize, target: usize) -> Result<i32, DiplomacyPlanError> {
        let (sender, target) = self.pair(sender as i32, target as i32)?;
        if sender == target {
            return Ok(2);
        }
        let forward = self.setup.leaders[sender].diplos[target];
        let reverse = self.setup.leaders[target].diplos[sender];
        Ok(if forward == 0 || reverse == 0 {
            0
        } else if forward == 2 && reverse == 2 {
            2
        } else {
            1
        })
    }

    #[inline]
    pub fn is_enemy(&self, sender: usize, target: usize) -> Result<bool, DiplomacyPlanError> {
        Ok(self.get_diplo(sender, target)? == 0)
    }

    #[inline]
    pub fn is_ally(&self, sender: usize, target: usize) -> Result<bool, DiplomacyPlanError> {
        Ok(self.get_diplo(sender, target)? == 2)
    }

    /// Delegate the runtime team question to the setup-diplomacy seam.  Command adapters
    /// use zero mode, exactly as the third-party fan-out in `action_declare`.
    pub fn is_runtime_team(
        &self,
        sender: usize,
        target: usize,
    ) -> Result<bool, DiplomacyPlanError> {
        Ok(self.setup.is_team(sender, target, IsTeamArg::Zero)?)
    }
}

fn clear_agreement(
    state: &mut DiplomacyCommandState,
    leader: usize,
    target: usize,
    steps: &mut Vec<DiplomacyStep>,
) {
    let DiplomacyLeaderState {
        proposals,
        buckets,
        reserved_resources,
        ..
    } = &mut state.leaders[leader];
    let proposal = &mut proposals[target];
    if proposal.agreement_pending == 1 {
        for good in 0..NUM_GOODS {
            let declaration_cost = proposal.declaration_costs[good];
            if declaration_cost != 0 {
                let returned = reserved_resources[good].min(declaration_cost);
                reserved_resources[good] = reserved_resources[good].wrapping_sub(returned);
                buckets[good] = buckets[good].wrapping_add(returned);
            }
            let offer = proposal.offers[good];
            if offer > 0 {
                let returned = reserved_resources[good].min(offer);
                reserved_resources[good] = reserved_resources[good].wrapping_sub(returned);
                buckets[good] = buckets[good].wrapping_add(returned);
            }
        }
        proposal.declaration_costs = [0; NUM_GOODS];
    }
    proposal.agreement_pending = 0;
    proposal.proposal_open = 0;
    steps.push(DiplomacyStep::ClearAgreement { leader, target });
}

fn clear_pair_agreements(
    state: &mut DiplomacyCommandState,
    sender: usize,
    target: usize,
    steps: &mut Vec<DiplomacyStep>,
) {
    clear_agreement(state, sender, target, steps);
    clear_agreement(state, target, sender, steps);
}

fn clear_response_counters(
    state: &mut DiplomacyCommandState,
    sender: usize,
    target: usize,
    steps: &mut Vec<DiplomacyStep>,
) {
    state.leaders[sender].response_334[target] = 0;
    steps.push(DiplomacyStep::ClearResponseCounter {
        sender,
        target,
        field: ResponseCounterField::TributeDemanded334,
    });
    state.leaders[sender].response_314[target] = 0;
    steps.push(DiplomacyStep::ClearResponseCounter {
        sender,
        target,
        field: ResponseCounterField::Counteroffer314,
    });
}

pub fn plan_diplomacy_command(
    before: &DiplomacyCommandState,
    wire: &[u8],
) -> Result<DiplomacyPlanDecision, DiplomacyPlanError> {
    let command = decode_diplomacy_command(wire)?;
    let (sender, target) = before.pair(command.sender(), command.target())?;
    let mut state = before.clone();
    let mut steps = Vec::new();

    match command {
        DiplomacyWireCommand::Treaty { treaty, .. } => {
            steps.push(DiplomacyStep::ClickStamp { sender, target });
            state.leaders[sender].proposals[target].proposal_open = 1;
            steps.push(DiplomacyStep::MarkProposalOpen { sender, target });
            clear_pair_agreements(&mut state, sender, target, &mut steps);
            state.leaders[sender].proposals[target].treaty = treaty;
            state.leaders[target].proposals[sender].treaty = treaty;
            steps.push(DiplomacyStep::WriteTreaty {
                sender,
                target,
                treaty,
            });
        }
        DiplomacyWireCommand::Declare { treaty, .. } => {
            if before.is_enemy(sender, target)? {
                steps.push(DiplomacyStep::DeclarationAlreadyEnemy { sender, target });
                return Ok(DiplomacyPlanDecision::Apply(DiplomacyCommandPlan {
                    command,
                    state,
                    steps,
                }));
            }
            let mut effective_treaty = treaty;
            if treaty == 0 && before.is_ally(sender, target)? {
                effective_treaty = 1;
                steps.push(DiplomacyStep::RewriteAlliedWarRequestToNeutral { sender, target });
            } else if treaty == 0 {
                let last = before.leaders[sender].declaration_frame[target]
                    .max(before.leaders[target].declaration_frame[sender]);
                if last != 0 && before.no_rush_frames > before.frame.wrapping_sub(last) {
                    if before.local_who == sender as i32 {
                        steps.push(DiplomacyStep::LocalNotice {
                            who: sender,
                            notice: LocalNotice::NoRushDeclaration,
                        });
                    }
                    return Ok(DiplomacyPlanDecision::Apply(DiplomacyCommandPlan {
                        command,
                        state,
                        steps,
                    }));
                }
            }
            return Ok(DiplomacyPlanDecision::Boundary(DiplomacyBoundaryPlan {
                command,
                prefix: steps,
                boundary: DiplomacyBoundary::DeclarationResourceAndDiploChange {
                    sender,
                    target,
                    requested_treaty: treaty,
                    effective_treaty,
                },
            }));
        }
        DiplomacyWireCommand::ClearTributes { .. } => {
            clear_pair_agreements(&mut state, sender, target, &mut steps);
            if before.local_who == target as i32
                && state.leaders[sender].proposals[target]
                    .offers
                    .into_iter()
                    .any(|amount| amount != 0)
            {
                steps.push(DiplomacyStep::LocalNotice {
                    who: target,
                    notice: LocalNotice::TributesCleared,
                });
            }
            state.leaders[sender].proposals[target].clear_offers();
            state.leaders[target].proposals[sender].clear_offers();
            steps.push(DiplomacyStep::ClearOffers { sender, target });
        }
        DiplomacyWireCommand::ClearAll { .. } => {
            steps.push(DiplomacyStep::ClickStamp { sender, target });
            clear_pair_agreements(&mut state, sender, target, &mut steps);
            if before.local_who == target as i32
                && state.leaders[sender].proposals[target].has_any_proposal()
            {
                steps.push(DiplomacyStep::LocalNotice {
                    who: target,
                    notice: LocalNotice::ProposalsCleared,
                });
            }
            state.leaders[sender].proposals[target].clear_all();
            state.leaders[target].proposals[sender].clear_all();
            steps.push(DiplomacyStep::ClearAll { sender, target });
        }
        DiplomacyWireCommand::Accept { .. } => {
            clear_response_counters(&mut state, sender, target, &mut steps);
            return Ok(DiplomacyPlanDecision::Boundary(DiplomacyBoundaryPlan {
                command,
                prefix: steps,
                boundary: DiplomacyBoundary::AcceptTransferAndDiploChange { sender, target },
            }));
        }
        DiplomacyWireCommand::Reject { .. } => {
            // `process_reject` chooses mode zero from the receiver's pair-record dword
            // `+0x00`, not from `LeaderData::get_diplo` or either raw declaration.
            if before.leaders[sender].proposals[target].agreement_pending != 1 {
                clear_response_counters(&mut state, sender, target, &mut steps);

                // `action_respond(target, 2)` appears to contain a recursive declaration,
                // but its gate at 0x006D04A9 calls `leaders[target].is_enemy(target)` and
                // requires a nonzero result. `pair` has already proved the retail setup
                // invariant `leaders[target].who == target`, so this self query is always
                // false. Preserve the candidate scan as evidence without inventing the
                // unreachable DOW/set_diplo transaction.
                if before.setup.leaders[sender].leader_flags & 4 != 0
                    && before.setup.leaders[target].leader_flags & 4 == 0
                    && before.is_ally(sender, target)?
                {
                    for candidate in 0..DIPLOMACY_SLOTS {
                        if before.setup.leaders[candidate].is_present()
                            && before.leaders[sender].proposals[target].attacks[candidate] != 0
                        {
                            debug_assert!(!before.is_enemy(target, target)?);
                            steps.push(DiplomacyStep::RejectCounterproposalSelfGate {
                                sender,
                                target,
                                candidate,
                            });
                        }
                    }
                }

                // Retail then clears both agreements before clearing the reciprocal record
                // first and the sender record second.
                clear_pair_agreements(&mut state, sender, target, &mut steps);
                state.leaders[target].proposals[sender].clear_all();
                steps.push(DiplomacyStep::ClearProposalRecord {
                    leader: target,
                    target: sender,
                });
                state.leaders[sender].proposals[target].clear_all();
                steps.push(DiplomacyStep::ClearProposalRecord {
                    leader: sender,
                    target,
                });
                if before.local_who == target as i32 {
                    steps.push(DiplomacyStep::LocalNotice {
                        who: target,
                        notice: LocalNotice::CounterproposalRejected,
                    });
                    steps.push(DiplomacyStep::Sound { category: 0x18 });
                }
            } else {
                clear_response_counters(&mut state, sender, target, &mut steps);
                clear_agreement(&mut state, sender, target, &mut steps);
                if before.local_who == target as i32 {
                    steps.push(DiplomacyStep::LocalNotice {
                        who: target,
                        notice: LocalNotice::Rejected,
                    });
                }
            }
        }
        DiplomacyWireCommand::Offer { good, amount, .. } => {
            let good = usize::try_from(good)
                .ok()
                .filter(|&good| good < NUM_GOODS)
                .ok_or(DiplomacyPlanError::GoodOutOfRange { value: good })?;
            steps.push(DiplomacyStep::ClickStamp { sender, target });
            let current = before.leaders[sender].proposals[target].offers[good];
            if amount > 0 && current.wrapping_add(amount) > before.leaders[sender].buckets[good] {
                steps.push(DiplomacyStep::RefuseInsufficientTribute {
                    sender,
                    target,
                    good,
                    amount,
                });
                if before.local_who == sender as i32 {
                    steps.push(DiplomacyStep::LocalNotice {
                        who: sender,
                        notice: LocalNotice::InsufficientTribute,
                    });
                }
                return Ok(DiplomacyPlanDecision::Apply(DiplomacyCommandPlan {
                    command,
                    state,
                    steps,
                }));
            }
            state.leaders[sender].proposals[target].proposal_open = 1;
            steps.push(DiplomacyStep::MarkProposalOpen { sender, target });
            clear_pair_agreements(&mut state, sender, target, &mut steps);
            state.leaders[sender].proposals[target].offers[good] =
                state.leaders[sender].proposals[target].offers[good].wrapping_add(amount);
            state.leaders[target].proposals[sender].offers[good] =
                state.leaders[target].proposals[sender].offers[good].wrapping_sub(amount);
            steps.push(DiplomacyStep::WriteOffer {
                sender,
                target,
                good,
                amount,
            });
        }
        DiplomacyWireCommand::ProposeAttack { whose, onoff, .. } => {
            steps.push(DiplomacyStep::ClickStamp { sender, target });
            state.leaders[sender].proposals[target].proposal_open = 1;
            steps.push(DiplomacyStep::MarkProposalOpen { sender, target });
            if whose >= 0 {
                let whose = usize::try_from(whose)
                    .ok()
                    .filter(|&whose| whose < DIPLOMACY_SLOTS)
                    .ok_or(DiplomacyPlanError::AttackLeaderOutOfRange { value: whose })?;
                clear_pair_agreements(&mut state, sender, target, &mut steps);
                state.leaders[sender].proposals[target].attacks[whose] = onoff;
                state.leaders[target].proposals[sender].attacks[whose] = onoff;
                steps.push(DiplomacyStep::WriteAttack {
                    sender,
                    target,
                    whose,
                    onoff,
                });
            }
        }
    }

    Ok(DiplomacyPlanDecision::Apply(DiplomacyCommandPlan {
        command,
        state,
        steps,
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyCommandRequest {
    pub before: DiplomacyCommandState,
    pub wire: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiplomacyTransactionStatus {
    Applied,
    Unavailable,
}

/// Echoed host receipt.  Only a recomputable `Apply` decision can validate as applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyCommandReceipt {
    pub request: DiplomacyCommandRequest,
    pub status: DiplomacyTransactionStatus,
    pub plan: Option<DiplomacyCommandPlan>,
}

impl DiplomacyCommandReceipt {
    pub fn unavailable(request: DiplomacyCommandRequest) -> Self {
        Self {
            request,
            status: DiplomacyTransactionStatus::Unavailable,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &DiplomacyCommandRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            DiplomacyTransactionStatus::Unavailable => {
                self.plan.is_none()
                    && plan_diplomacy_command(&expected.before, &expected.wire).is_ok()
            }
            DiplomacyTransactionStatus::Applied => {
                let Some(observed) = &self.plan else {
                    return false;
                };
                matches!(
                    plan_diplomacy_command(&expected.before, &expected.wire),
                    Ok(DiplomacyPlanDecision::Apply(expected_plan)) if expected_plan == *observed
                )
            }
        }
    }
}
