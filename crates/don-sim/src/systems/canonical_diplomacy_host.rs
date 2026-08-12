// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical owner projection and save payload for diplomacy opcodes 38 and 41.
//!
//! The whole-body planners operate on detached images because their retail transaction spans
//! leaders, resources, relations, vision, victory, armies, and the World object lists.  This
//! adapter is the one place that constructs that image.  Its [`DiplomacyOwnerImage`] is a
//! transaction snapshot, not another live leader table: proposals and escrow project from the
//! exact `LeaderData` fields already owned by step 8, while [`DiplomacyPersistentState`] contains
//! only the retained fields which currently have no other canonical representation. In
//! particular, response/agenda/peace/attack/declaration rows project through the existing
//! victory-leader owner and its `LEADER_MATCH` save section, rather than this new payload.
//!
//! A prepared transaction records the complete before-owner and after-image.  Publication first
//! proves the live owner is still the before-owner and that every ordered authority call was
//! completed.  It then folds into a clone and replaces the caller's owner once.  Stale state,
//! missing object/army/victory authority, or a malformed projection therefore cannot publish a
//! resource debit without the matching relation/proposal/victory state.

use super::diplomacy_accept_host::{
    plan_accept, required_accept_authority, validate_accept_projection, AcceptAuthority,
    AcceptAuthorityImage, AcceptHostError, AcceptLeaderImage, AcceptPlan,
};
use super::diplomacy_deal_callbacks::{
    plan_consider_tribute, plan_notify_deal, ConsiderTributePlan, ConsiderTributeRequest,
    DealCallbackFacts, DealCallbackImage, DealCallbackLeaderState, NotifyDealEnvelope,
    NotifyDealRequest,
};
use super::diplomacy_declare_host::{
    plan_declare, DeclarationStatistics, DeclareHostError, DeclarePlan, DeclareStep,
};
use super::leader_set_diplo::{
    EjectionUnitFact, Relation, SetDiploAuthority, SetDiploStep, ARMY_SLOTS, DIPLO_SLOTS, NUM_GOODS,
};
use crate::command::diplomacy_command_plans::{DiplomacyProposal, ACCEPT_OPCODE, DECLARE_OPCODE};
use crate::command::setup_diplomacy::SetupDiplomacy;

/// First DoNSave version which requires the top-level diplomacy section.
pub const DIPLOMACY_SAVE_FORMAT_VERSION: u32 = 14;
/// Last format whose absence of a diplomacy section means constructor state.
pub const PRE_DIPLOMACY_SAVE_FORMAT_VERSION: u32 = 13;
const DIPLOMACY_PAYLOAD_VERSION: u16 = 1;
const PAYLOAD_HEADER_LEN: usize = 4;
const PROPOSAL_I32S: usize = 3 + NUM_GOODS * 2 + DIPLO_SLOTS;
const LEADER_I32S: usize = DIPLO_SLOTS * PROPOSAL_I32S + NUM_GOODS + 3;
/// Exact size of the version-one diplomacy leaf payload.
pub const DIPLOMACY_PAYLOAD_LEN: usize = PAYLOAD_HEADER_LEN + DIPLO_SLOTS * LEADER_I32S * 4;

/// Persistent fields adjacent to one retail `LeaderData` record.
///
/// Resource buckets, directional relations, shared vision, victory state, army validity, and
/// object placement remain with their existing owners and are intentionally absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyPersistentLeader {
    /// `LeaderData+0x692c`, projected to/from step 8's `TauntLeaderState::dip`.
    pub proposals: [DiplomacyProposal; DIPLO_SLOTS],
    /// `LeaderData+0x498`, projected to/from step 8's `TauntLeaderState::tributes`.
    pub reserved_resources: [i32; NUM_GOODS],
    /// `LeaderData+0x20c`; its directional companion already lives in the victory row.
    pub repeated_targets: i32,
    /// `LeaderData+0x860`.
    pub sent_raw: i32,
    /// `LeaderData+0x864`.
    pub received_scaled: i32,
}

impl Default for DiplomacyPersistentLeader {
    fn default() -> Self {
        Self {
            proposals: [DiplomacyProposal::default(); DIPLO_SLOTS],
            reserved_resources: [0; NUM_GOODS],
            repeated_targets: 0,
            sent_raw: 0,
            received_scaled: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyPersistentState {
    pub leaders: [DiplomacyPersistentLeader; DIPLO_SLOTS],
}

impl Default for DiplomacyPersistentState {
    fn default() -> Self {
        Self {
            leaders: std::array::from_fn(|_| DiplomacyPersistentLeader::default()),
        }
    }
}

/// Diplomacy fields already owned and saved by each canonical victory leader row.
///
/// The shared Sim mount maps these onto `LeaderState::init_diplomacy` and its directional
/// declaration rows. Keeping them separate from [`DiplomacyPersistentState`] prevents the v14
/// leaf from becoming a second save owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalLeaderDiplomacyFields {
    pub response_314: [i32; DIPLO_SLOTS],
    pub response_334: [i32; DIPLO_SLOTS],
    pub declaration_frame: [i32; DIPLO_SLOTS],
    pub declaration_by_target: [i32; DIPLO_SLOTS],
    pub agenda_flags: [u32; DIPLO_SLOTS],
    pub peace_frames: [i32; DIPLO_SLOTS],
    pub attack_frames: [i32; DIPLO_SLOTS],
    pub attack_peers: [i32; DIPLO_SLOTS],
    /// Existing `LeaderData+0x174/+0x194` rows; both are already in `LEADER_MATCH`.
    pub tribute_stamp: [i32; DIPLO_SLOTS],
    pub gift_stamp: [i32; DIPLO_SLOTS],
}

impl Default for CanonicalLeaderDiplomacyFields {
    fn default() -> Self {
        Self {
            response_314: [0; DIPLO_SLOTS],
            response_334: [0; DIPLO_SLOTS],
            declaration_frame: [0; DIPLO_SLOTS],
            declaration_by_target: [0; DIPLO_SLOTS],
            agenda_flags: [0; DIPLO_SLOTS],
            peace_frames: [0; DIPLO_SLOTS],
            attack_frames: [0; DIPLO_SLOTS],
            attack_peers: [-1; DIPLO_SLOTS],
            tribute_stamp: [0; DIPLO_SLOTS],
            gift_stamp: [0; DIPLO_SLOTS],
        }
    }
}

/// A detached snapshot of every canonical mutable owner touched by opcodes 38 and 41.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyOwnerImage {
    /// Identity, presence/team records, directional relations, and frame.
    pub setup: SetupDiplomacy,
    /// Canonical decoded resource buckets (normally `Sim::leaders[*].econ.stockpile`).
    pub resources: [[i32; NUM_GOODS]; DIPLO_SLOTS],
    pub retained: DiplomacyPersistentState,
    /// Existing mutable/save-owned victory-leader fields used by the action bodies.
    pub leader_diplomacy: [CanonicalLeaderDiplomacyFields; DIPLO_SLOTS],
    pub leader_flags2: [u32; DIPLO_SLOTS],
    pub shared_vision: [u8; DIPLO_SLOTS],
    pub interface_dirty: bool,
    pub victory_mask: u32,
    pub valid_armies: [[bool; ARMY_SLOTS]; DIPLO_SLOTS],
    /// Stable owner-list order, including non-Unit objects because retail queries each entry.
    pub ejection_units: [Vec<EjectionUnitFact>; DIPLO_SLOTS],
    pub no_rush_frames: i32,
    pub local_who: i32,
}

impl Default for DiplomacyOwnerImage {
    fn default() -> Self {
        Self {
            setup: SetupDiplomacy::default(),
            resources: [[0; NUM_GOODS]; DIPLO_SLOTS],
            retained: DiplomacyPersistentState::default(),
            leader_diplomacy: std::array::from_fn(|_| CanonicalLeaderDiplomacyFields::default()),
            leader_flags2: [0; DIPLO_SLOTS],
            shared_vision: [0; DIPLO_SLOTS],
            interface_dirty: false,
            victory_mask: 0,
            valid_armies: [[false; ARMY_SLOTS]; DIPLO_SLOTS],
            ejection_units: std::array::from_fn(|_| Vec::new()),
            no_rush_frames: 0,
            local_who: -1,
        }
    }
}

/// Runtime query answers which are read by the retail bodies but are not mutated by them.
/// These are reinstalled after load; they do not belong in the diplomacy save payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyInstalledFacts {
    pub type_available: [[Option<bool>; NUM_GOODS]; DIPLO_SLOTS],
    pub declaration_costs: [[Option<i32>; NUM_GOODS]; DIPLO_SLOTS],
    pub tribute_scale_percent: [Option<i32>; DIPLO_SLOTS],
    pub war_allowed: Option<bool>,
    pub has_shared_vision_preq: [Option<bool>; DIPLO_SLOTS],
    pub console_treaty_one: [Option<bool>; DIPLO_SLOTS],
    pub global_shared_vision: Option<bool>,
    pub is_neutral: [Option<bool>; DIPLO_SLOTS],
    pub team_members_mode_one: [Option<i32>; DIPLO_SLOTS],
    pub num_allies: [Option<i32>; DIPLO_SLOTS],
    /// PDB `LeaderData::econ[6]` at `+0x450`; AI bookkeeping flags, not resource buckets.
    pub tribute_econ: [[Option<i32>; NUM_GOODS]; DIPLO_SLOTS],
}

impl Default for DiplomacyInstalledFacts {
    fn default() -> Self {
        Self {
            type_available: [[None; NUM_GOODS]; DIPLO_SLOTS],
            declaration_costs: [[None; NUM_GOODS]; DIPLO_SLOTS],
            tribute_scale_percent: [None; DIPLO_SLOTS],
            war_allowed: None,
            has_shared_vision_preq: [None; DIPLO_SLOTS],
            console_treaty_one: [None; DIPLO_SLOTS],
            global_shared_vision: None,
            is_neutral: [None; DIPLO_SLOTS],
            team_members_mode_one: [None; DIPLO_SLOTS],
            num_allies: [None; DIPLO_SLOTS],
            tribute_econ: [[None; NUM_GOODS]; DIPLO_SLOTS],
        }
    }
}

/// Construct the single full-body planner projection from canonical owners.
pub fn project_authority(
    owner: &DiplomacyOwnerImage,
    facts: &DiplomacyInstalledFacts,
) -> Result<AcceptAuthorityImage, AcceptHostError> {
    let mut image = AcceptAuthorityImage::default();
    image.declaration.command.setup = owner.setup.clone();
    image.declaration.command.frame = owner.setup.frame;
    image.declaration.command.no_rush_frames = owner.no_rush_frames;
    image.declaration.command.local_who = owner.local_who;
    image.declaration.set_diplo.local_who = Some(owner.local_who);
    image.declaration.set_diplo.console_treaty_one = facts.console_treaty_one;
    image.declaration.set_diplo.global_shared_vision = facts.global_shared_vision;
    image.declaration.set_diplo.interface_dirty = owner.interface_dirty;
    image.declaration.set_diplo.victory_mask = owner.victory_mask;
    image.declaration.war_allowed = facts.war_allowed;

    for who in 0..DIPLO_SLOTS {
        let retained = &owner.retained.leaders[who];
        let command = &mut image.declaration.command.leaders[who];
        command.proposals = retained.proposals;
        command.buckets = owner.resources[who];
        command.reserved_resources = retained.reserved_resources;
        command.response_314 = owner.leader_diplomacy[who].response_314;
        command.response_334 = owner.leader_diplomacy[who].response_334;
        command.declaration_frame = owner.leader_diplomacy[who].declaration_frame;

        let payment = &mut image.declaration.payments[who];
        payment.authoritative_buckets = owner.resources[who];
        payment.leader_buckets = owner.resources[who];
        payment.type_available = facts.type_available[who];
        payment.costs = facts.declaration_costs[who];
        image.declaration.statistics[who] = DeclarationStatistics {
            repeated_targets: retained.repeated_targets,
            by_target: owner.leader_diplomacy[who].declaration_by_target,
        };

        let setup = owner.setup.leaders[who];
        let set = &mut image.declaration.set_diplo.leaders[who];
        set.who = Some(setup.who);
        set.leader_flags = Some(setup.leader_flags as u32);
        set.leader_flags2 = Some(owner.leader_flags2[who]);
        for other in 0..DIPLO_SLOTS {
            set.diplos[other] = Some(match setup.diplos[other] {
                0 => Relation::War,
                1 => Relation::Peace,
                2 => Relation::Ally,
                raw => return Err(AcceptHostError::UnsupportedRelation(raw)),
            });
        }
        set.shared_vision = Some(owner.shared_vision[who]);
        set.has_shared_vision_preq = facts.has_shared_vision_preq[who];
        set.ejection_units = Some(owner.ejection_units[who].clone());
        set.valid_armies = Some(owner.valid_armies[who]);

        let resources = &mut image.resources.leaders[who];
        resources.buckets = owner.resources[who];
        resources.escrow = retained.reserved_resources;
        resources.sent_raw = retained.sent_raw;
        resources.received_scaled = retained.received_scaled;
        resources.tribute_scale_percent = facts.tribute_scale_percent[who];
        resources.type_available = facts.type_available[who];
        for other in 0..DIPLO_SLOTS {
            image.resources.offers[who][other] = retained.proposals[other].offers;
            image.resources.declaration_costs[who][other] =
                retained.proposals[other].declaration_costs;
        }

        image.leaders[who] = AcceptLeaderImage {
            agenda_flags: owner.leader_diplomacy[who].agenda_flags,
            peace_frames: owner.leader_diplomacy[who].peace_frames,
            attack_frames: owner.leader_diplomacy[who].attack_frames,
            attack_peers: owner.leader_diplomacy[who].attack_peers,
            is_neutral: facts.is_neutral[who],
            team_members_mode_one: facts.team_members_mode_one[who],
            num_allies: facts.num_allies[who],
        };
    }
    validate_accept_projection(&image)?;
    Ok(image)
}

fn declare_authority(plan: &DeclarePlan) -> Vec<SetDiploAuthority> {
    plan.steps
        .iter()
        .flat_map(|step| match step {
            DeclareStep::RootSetDiplo(set) | DeclareStep::AllySetDiplo { plan: set, .. } => set
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

fn embed_declaration_after(
    before: &AcceptAuthorityImage,
    plan: &DeclarePlan,
) -> AcceptAuthorityImage {
    let mut after = before.clone();
    after.declaration = plan.after.clone();
    for who in 0..DIPLO_SLOTS {
        after.resources.leaders[who].buckets =
            after.declaration.payments[who].authoritative_buckets;
        after.resources.leaders[who].escrow =
            after.declaration.command.leaders[who].reserved_resources;
        after.resources.leaders[who].type_available =
            after.declaration.payments[who].type_available;
        for other in 0..DIPLO_SLOTS {
            after.resources.offers[who][other] =
                after.declaration.command.leaders[who].proposals[other].offers;
            after.resources.declaration_costs[who][other] =
                after.declaration.command.leaders[who].proposals[other].declaration_costs;
        }
    }
    after
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalDiplomacyAuthority {
    Declare(SetDiploAuthority),
    Accept(AcceptAuthority),
}

/// Unowned simulation authorities that must still be completed by the Sim host.  The two deal
/// callbacks are deliberately absent: this adapter now executes `consider_tribute` against its
/// retained frame-stamp rows and lowers `notify_deal` into presentation envelopes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalDiplomacyAuthority {
    Declare(SetDiploAuthority),
    Accept(AcceptAuthority),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreparedDiplomacyPlan {
    Declare(DeclarePlan),
    Accept(AcceptPlan),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedDiplomacyTransaction {
    pub wire: Vec<u8>,
    pub before_owner: DiplomacyOwnerImage,
    pub before_authority: AcceptAuthorityImage,
    pub after_authority: AcceptAuthorityImage,
    /// Fully folded owner after internal `consider_tribute` callbacks. This is computed during
    /// prepare so commit performs only stale-CAS validation and infallible replacement.
    pub after_owner: DiplomacyOwnerImage,
    pub plan: PreparedDiplomacyPlan,
    /// Complete planner call sequence, retained for source-order validation and diagnostics.
    pub planned_authority: Vec<CanonicalDiplomacyAuthority>,
    /// Only calls which still require a separate simulation owner.
    pub required_external_authority: Vec<ExternalDiplomacyAuthority>,
    /// Complete plans for each reached `consider_tribute`, including zero-valued early returns.
    pub consider_tribute: Vec<ConsiderTributePlan>,
    /// Local-only presentation selected by reached `notify_deal` calls.
    pub notify_deal: Vec<NotifyDealEnvelope>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrepareDiplomacyError {
    UnsupportedOpcode(Option<u8>),
    Projection(AcceptHostError),
    Declare(DeclareHostError),
    Accept(AcceptHostError),
    DealCallback(super::diplomacy_deal_callbacks::DealCallbackError),
}

fn callback_image(owner: &DiplomacyOwnerImage) -> DealCallbackImage {
    DealCallbackImage {
        leaders: std::array::from_fn(|who| DealCallbackLeaderState {
            tribute_stamp: owner.leader_diplomacy[who].tribute_stamp,
            gift_stamp: owner.leader_diplomacy[who].gift_stamp,
        }),
        frame: owner.setup.frame,
        local_who: owner.local_who,
    }
}

fn callback_facts(
    owner: &DiplomacyOwnerImage,
    after_authority: &AcceptAuthorityImage,
    facts: &DiplomacyInstalledFacts,
) -> DealCallbackFacts {
    DealCallbackFacts {
        // Both fields already belong to the canonical Leader projection. `+0x20c` is the same
        // PDB `blacken` scalar opcode 38 increments; treating either as an installed answer would
        // permit the callback decision to diverge from the transaction's own snapshot.
        receiver_flag_four: std::array::from_fn(|who| {
            Some(owner.setup.leaders[who].leader_flags & 4 != 0)
        }),
        sender_blacken: std::array::from_fn(|who| {
            Some(owner.retained.leaders[who].repeated_targets)
        }),
        // `consider_tribute` runs after each receiver credit in the accepted-resource loop. The
        // completed resource planner's bucket is the exact value visible at callback time for
        // the same good because later goods cannot modify it.
        receiver_resources: std::array::from_fn(|who| {
            std::array::from_fn(|good| Some(after_authority.resources.leaders[who].buckets[good]))
        }),
        receiver_econ: facts.tribute_econ,
    }
}

fn execute_deal_callbacks(
    owner: &DiplomacyOwnerImage,
    facts: &DiplomacyInstalledFacts,
    after_authority: &AcceptAuthorityImage,
    authority: &[CanonicalDiplomacyAuthority],
) -> Result<
    (
        DealCallbackImage,
        Vec<ConsiderTributePlan>,
        Vec<NotifyDealEnvelope>,
        Vec<ExternalDiplomacyAuthority>,
    ),
    PrepareDiplomacyError,
> {
    let mut image = callback_image(owner);
    let callback_facts = callback_facts(owner, after_authority, facts);
    let mut consider = Vec::new();
    let mut notify = Vec::new();
    let mut external = Vec::new();
    for call in authority {
        match call {
            CanonicalDiplomacyAuthority::Accept(AcceptAuthority::ConsiderTribute {
                receiver,
                sender,
                good,
                raw,
            }) => {
                let plan = plan_consider_tribute(
                    &image,
                    &callback_facts,
                    ConsiderTributeRequest {
                        receiver: *receiver,
                        sender: *sender,
                        raw: *raw,
                        good: *good,
                    },
                )
                .map_err(PrepareDiplomacyError::DealCallback)?;
                image = plan.after.clone();
                consider.push(plan);
            }
            CanonicalDiplomacyAuthority::Accept(AcceptAuthority::NotifyDeal {
                leader,
                other,
                treaty,
            }) => {
                if let Some(envelope) = plan_notify_deal(
                    &image,
                    NotifyDealRequest {
                        leader: *leader,
                        other: *other,
                        treaty: *treaty,
                    },
                )
                .map_err(PrepareDiplomacyError::DealCallback)?
                {
                    notify.push(envelope);
                }
            }
            CanonicalDiplomacyAuthority::Declare(call) => {
                external.push(ExternalDiplomacyAuthority::Declare(*call));
            }
            CanonicalDiplomacyAuthority::Accept(call) => {
                external.push(ExternalDiplomacyAuthority::Accept(*call));
            }
        }
    }
    Ok((image, consider, notify, external))
}

/// Prepare either complete transaction from the real fixed-wire command body.
pub fn prepare_diplomacy_transaction(
    owner: &DiplomacyOwnerImage,
    facts: &DiplomacyInstalledFacts,
    wire: &[u8],
) -> Result<PreparedDiplomacyTransaction, PrepareDiplomacyError> {
    let before = project_authority(owner, facts).map_err(PrepareDiplomacyError::Projection)?;
    let (plan, after, authority) = match wire.first().copied() {
        Some(DECLARE_OPCODE) => {
            let plan =
                plan_declare(&before.declaration, wire).map_err(PrepareDiplomacyError::Declare)?;
            let authority: Vec<CanonicalDiplomacyAuthority> = declare_authority(&plan)
                .into_iter()
                .map(CanonicalDiplomacyAuthority::Declare)
                .collect();
            let after = embed_declaration_after(&before, &plan);
            (PreparedDiplomacyPlan::Declare(plan), after, authority)
        }
        Some(ACCEPT_OPCODE) => {
            let plan = plan_accept(&before, wire).map_err(PrepareDiplomacyError::Accept)?;
            let authority: Vec<CanonicalDiplomacyAuthority> =
                required_accept_authority(&plan.steps)
                    .into_iter()
                    .map(CanonicalDiplomacyAuthority::Accept)
                    .collect();
            let after = plan.after.clone();
            (PreparedDiplomacyPlan::Accept(plan), after, authority)
        }
        opcode => return Err(PrepareDiplomacyError::UnsupportedOpcode(opcode)),
    };
    validate_accept_projection(&after).map_err(PrepareDiplomacyError::Projection)?;
    let (callback_after, consider_tribute, notify_deal, required_external_authority) =
        execute_deal_callbacks(owner, facts, &after, &authority)?;
    let mut after_owner = owner.clone();
    fold_after(&mut after_owner, &after);
    for who in 0..DIPLO_SLOTS {
        after_owner.leader_diplomacy[who].tribute_stamp = callback_after.leaders[who].tribute_stamp;
        after_owner.leader_diplomacy[who].gift_stamp = callback_after.leaders[who].gift_stamp;
    }
    Ok(PreparedDiplomacyTransaction {
        wire: wire.to_vec(),
        before_owner: owner.clone(),
        before_authority: before,
        after_authority: after,
        after_owner,
        plan,
        planned_authority: authority,
        required_external_authority,
        consider_tribute,
        notify_deal,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitDiplomacyError {
    StaleOwner,
    AuthorityMismatch,
    InvalidAfter(AcceptHostError),
}

fn fold_after(owner: &mut DiplomacyOwnerImage, image: &AcceptAuthorityImage) {
    owner.setup = image.declaration.command.setup.clone();
    owner.interface_dirty = image.declaration.set_diplo.interface_dirty;
    owner.victory_mask = image.declaration.set_diplo.victory_mask;
    for who in 0..DIPLO_SLOTS {
        owner.resources[who] = image.declaration.payments[who].authoritative_buckets;
        owner.shared_vision[who] = image.declaration.set_diplo.leaders[who]
            .shared_vision
            .expect("validated canonical projection");
        let retained = &mut owner.retained.leaders[who];
        let command = &image.declaration.command.leaders[who];
        retained.proposals = command.proposals;
        retained.reserved_resources = command.reserved_resources;
        retained.repeated_targets = image.declaration.statistics[who].repeated_targets;
        retained.sent_raw = image.resources.leaders[who].sent_raw;
        retained.received_scaled = image.resources.leaders[who].received_scaled;
        owner.leader_diplomacy[who] = CanonicalLeaderDiplomacyFields {
            response_314: command.response_314,
            response_334: command.response_334,
            declaration_frame: command.declaration_frame,
            declaration_by_target: image.declaration.statistics[who].by_target,
            agenda_flags: image.leaders[who].agenda_flags,
            peace_frames: image.leaders[who].peace_frames,
            attack_frames: image.leaders[who].attack_frames,
            attack_peers: image.leaders[who].attack_peers,
            tribute_stamp: owner.leader_diplomacy[who].tribute_stamp,
            gift_stamp: owner.leader_diplomacy[who].gift_stamp,
        };
    }
}

/// Atomically publish a prepared transaction after the host completed its ordered authority.
pub fn commit_diplomacy_transaction(
    current: &mut DiplomacyOwnerImage,
    prepared: &PreparedDiplomacyTransaction,
    completed_authority: &[ExternalDiplomacyAuthority],
) -> Result<(), CommitDiplomacyError> {
    if current != &prepared.before_owner {
        return Err(CommitDiplomacyError::StaleOwner);
    }
    if completed_authority != prepared.required_external_authority {
        return Err(CommitDiplomacyError::AuthorityMismatch);
    }
    validate_accept_projection(&prepared.after_authority)
        .map_err(CommitDiplomacyError::InvalidAfter)?;
    *current = prepared.after_owner.clone();
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiplomacyCodecError {
    UnsupportedPayloadVersion(u16),
    WrongSlotCount(u8),
    Truncated,
    TrailingBytes,
    UnsupportedSaveFormat(u32),
    MissingForCurrentFormat,
    UnexpectedForLegacyFormat,
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn get_i32(bytes: &[u8], cursor: &mut usize) -> Result<i32, DiplomacyCodecError> {
    let end = cursor
        .checked_add(4)
        .ok_or(DiplomacyCodecError::Truncated)?;
    let raw = bytes
        .get(*cursor..end)
        .ok_or(DiplomacyCodecError::Truncated)?;
    *cursor = end;
    Ok(i32::from_le_bytes(raw.try_into().expect("four bytes")))
}

/// Encode only diplomacy-owned retained state for the v14 top-level leaf.
pub fn encode_diplomacy_payload(state: &DiplomacyPersistentState) -> Vec<u8> {
    let mut out = Vec::with_capacity(DIPLOMACY_PAYLOAD_LEN);
    out.extend_from_slice(&DIPLOMACY_PAYLOAD_VERSION.to_le_bytes());
    out.push(DIPLO_SLOTS as u8);
    out.push(0);
    for leader in &state.leaders {
        for proposal in &leader.proposals {
            put_i32(&mut out, proposal.agreement_pending);
            put_i32(&mut out, proposal.proposal_open);
            put_i32(&mut out, proposal.treaty);
            for value in proposal.offers {
                put_i32(&mut out, value);
            }
            for value in proposal.declaration_costs {
                put_i32(&mut out, value);
            }
            for value in proposal.attacks {
                put_i32(&mut out, value);
            }
        }
        for value in leader.reserved_resources {
            put_i32(&mut out, value);
        }
        put_i32(&mut out, leader.repeated_targets);
        put_i32(&mut out, leader.sent_raw);
        put_i32(&mut out, leader.received_scaled);
    }
    debug_assert_eq!(out.len(), DIPLOMACY_PAYLOAD_LEN);
    out
}

/// Decode the bounded v14 diplomacy leaf. No prefix or trailing data is accepted.
pub fn decode_diplomacy_payload(
    bytes: &[u8],
) -> Result<DiplomacyPersistentState, DiplomacyCodecError> {
    if bytes.len() < PAYLOAD_HEADER_LEN {
        return Err(DiplomacyCodecError::Truncated);
    }
    let version = u16::from_le_bytes([bytes[0], bytes[1]]);
    if version != DIPLOMACY_PAYLOAD_VERSION {
        return Err(DiplomacyCodecError::UnsupportedPayloadVersion(version));
    }
    if bytes[2] != DIPLO_SLOTS as u8 {
        return Err(DiplomacyCodecError::WrongSlotCount(bytes[2]));
    }
    if bytes.len() < DIPLOMACY_PAYLOAD_LEN {
        return Err(DiplomacyCodecError::Truncated);
    }
    if bytes.len() > DIPLOMACY_PAYLOAD_LEN {
        return Err(DiplomacyCodecError::TrailingBytes);
    }
    let mut cursor = PAYLOAD_HEADER_LEN;
    let mut state = DiplomacyPersistentState::default();
    for leader in &mut state.leaders {
        for proposal in &mut leader.proposals {
            proposal.agreement_pending = get_i32(bytes, &mut cursor)?;
            proposal.proposal_open = get_i32(bytes, &mut cursor)?;
            proposal.treaty = get_i32(bytes, &mut cursor)?;
            for value in &mut proposal.offers {
                *value = get_i32(bytes, &mut cursor)?;
            }
            for value in &mut proposal.declaration_costs {
                *value = get_i32(bytes, &mut cursor)?;
            }
            for value in &mut proposal.attacks {
                *value = get_i32(bytes, &mut cursor)?;
            }
        }
        for value in &mut leader.reserved_resources {
            *value = get_i32(bytes, &mut cursor)?;
        }
        leader.repeated_targets = get_i32(bytes, &mut cursor)?;
        leader.sent_raw = get_i32(bytes, &mut cursor)?;
        leader.received_scaled = get_i32(bytes, &mut cursor)?;
    }
    debug_assert_eq!(cursor, bytes.len());
    Ok(state)
}

/// Version-aware leaf gate used by the shared save loader mount.
pub fn decode_diplomacy_for_save(
    save_format: u32,
    payload: Option<&[u8]>,
) -> Result<DiplomacyPersistentState, DiplomacyCodecError> {
    match (save_format, payload) {
        (version, None) if version <= PRE_DIPLOMACY_SAVE_FORMAT_VERSION => {
            Ok(DiplomacyPersistentState::default())
        }
        (version, Some(_)) if version <= PRE_DIPLOMACY_SAVE_FORMAT_VERSION => {
            Err(DiplomacyCodecError::UnexpectedForLegacyFormat)
        }
        (DIPLOMACY_SAVE_FORMAT_VERSION, Some(bytes)) => decode_diplomacy_payload(bytes),
        (DIPLOMACY_SAVE_FORMAT_VERSION, None) => Err(DiplomacyCodecError::MissingForCurrentFormat),
        (version, _) => Err(DiplomacyCodecError::UnsupportedSaveFormat(version)),
    }
}
