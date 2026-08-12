// SPDX-License-Identifier: GPL-3.0-or-later
//! Production Bridge/Sim mount for authority-complete opcode-38 and opcode-41 transactions.
//!
//! The whole declaration/acceptance body is prepared by [`super::canonical_diplomacy_host`]. This
//! adapter projects its detached image from the existing Sim owners and publishes the remaining
//! declaration or acceptance in one assignment-only fold. Generic alliance victory is staged
//! through the canonical Leader/Match transaction when it produces no defeated-Unit cleanup;
//! every other external authority remains unavailable before publication.

use super::canonical_diplomacy_host::{
    commit_diplomacy_transaction, prepare_diplomacy_transaction, CanonicalLeaderDiplomacyFields,
    CommitDiplomacyError, DiplomacyInstalledFacts, DiplomacyOwnerImage, ExternalDiplomacyAuthority,
    PrepareDiplomacyError, PreparedDiplomacyTransaction,
};
use super::leader_process_taunt;
use super::leader_set_diplo::SetDiploAuthority;
use super::sparse_object_bands_authority_frontier::RetailBand;
use super::victory_score::VictoryType;
use crate::command::{
    diplomacy_command_plans::{ACCEPT_OPCODE, DECLARE_OPCODE},
    Bridge, Fleet, Package, WireError,
};
use crate::systems::order_dispatch::OrderQueue;
use crate::tick::leader_match_host::{
    apply_leader_match, LeaderMatchError, LeaderMatchReceipt, LeaderMatchRequest,
};
use crate::tick::{Sim, NUM_LEADERS};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalDiplomacyRequest {
    pub frame: i32,
    pub package_stamp: u32,
    pub play: i32,
    pub wire: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalDiplomacyStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalDiplomacyRuntimeError {
    UnsupportedOpcode(Option<u8>),
    FrameMismatch { request: i32, world: i32 },
    MissingPlayerSetup,
    MissingNoRushFrames,
    ResourceMirrorMismatch { who: usize },
    LeaderFlags2MirrorMismatch { who: usize },
    NonEmptyObjectBand { who: usize, band: RetailBand },
    ArmyShape { who: usize, actual: usize },
    Prepare(PrepareDiplomacyError),
    ExternalAuthority(Vec<ExternalDiplomacyAuthority>),
    PendingTerminalCleanup,
    UnsupportedVictoryType(i32),
    VictoryOrderingNotIsolated,
    Victory(LeaderMatchError),
    VictoryNeedsDefeatedUnitCleanup { owners: u8 },
    Commit(CommitDiplomacyError),
    StaleProjection,
    BridgeDidNotReturnReceipt,
    Wire(WireError),
}

/// The receipt contains every input needed to recompute the prepared transaction. An applied
/// receipt is therefore evidence of the exact whole-body plan, not a scalar acknowledgement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalDiplomacyReceipt {
    pub request: CanonicalDiplomacyRequest,
    pub status: CanonicalDiplomacyStatus,
    pub installed_facts: Option<DiplomacyInstalledFacts>,
    pub prepared: Option<PreparedDiplomacyTransaction>,
    pub completed_authority: Vec<ExternalDiplomacyAuthority>,
    /// Exact canonical Leader/Match receipts for the completed Victory authority subset.
    pub victory_receipts: Vec<LeaderMatchReceipt>,
    pub error: Option<CanonicalDiplomacyRuntimeError>,
}

impl CanonicalDiplomacyReceipt {
    pub fn unavailable(
        request: CanonicalDiplomacyRequest,
        error: CanonicalDiplomacyRuntimeError,
    ) -> Self {
        Self {
            request,
            status: CanonicalDiplomacyStatus::Unavailable,
            installed_facts: None,
            prepared: None,
            completed_authority: Vec::new(),
            victory_receipts: Vec::new(),
            error: Some(error),
        }
    }

    pub fn validates(&self, expected: &CanonicalDiplomacyRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            CanonicalDiplomacyStatus::Unavailable => {
                self.installed_facts.is_none()
                    && self.prepared.is_none()
                    && self.completed_authority.is_empty()
                    && self.victory_receipts.is_empty()
                    && self.error.is_some()
            }
            CanonicalDiplomacyStatus::Applied => {
                let (Some(facts), Some(prepared)) = (&self.installed_facts, &self.prepared) else {
                    return false;
                };
                self.error.is_none()
                    && prepared.wire == expected.wire
                    && self.completed_authority == prepared.required_external_authority
                    && victory_receipts_match_authority(
                        &self.completed_authority,
                        &self.victory_receipts,
                    )
                    && prepare_diplomacy_transaction(&prepared.before_owner, facts, &expected.wire)
                        .is_ok_and(|recomputed| recomputed == *prepared)
            }
        }
    }
}

fn set_diplo_call(call: &ExternalDiplomacyAuthority) -> SetDiploAuthority {
    match call {
        ExternalDiplomacyAuthority::Declare(call) => *call,
        ExternalDiplomacyAuthority::Accept(
            super::diplomacy_accept_host::AcceptAuthority::SetDiplo { call, .. }
            | super::diplomacy_accept_host::AcceptAuthority::RecursiveDeclare { call, .. },
        ) => *call,
        ExternalDiplomacyAuthority::Accept(
            super::diplomacy_accept_host::AcceptAuthority::ConsiderTribute { .. }
            | super::diplomacy_accept_host::AcceptAuthority::NotifyDeal { .. },
        ) => unreachable!("deal callbacks are internal before the external-authority list"),
    }
}

fn victory_request(
    call: &ExternalDiplomacyAuthority,
) -> Result<LeaderMatchRequest, CanonicalDiplomacyRuntimeError> {
    let SetDiploAuthority::Victory {
        winner,
        victory_type,
        instant,
    } = set_diplo_call(call)
    else {
        return Err(CanonicalDiplomacyRuntimeError::ExternalAuthority(vec![
            call.clone(),
        ]));
    };
    if victory_type != VictoryType::Generic as i32 {
        return Err(CanonicalDiplomacyRuntimeError::UnsupportedVictoryType(
            victory_type,
        ));
    }
    Ok(LeaderMatchRequest::Victory {
        who: winner,
        victory_type: VictoryType::Generic,
        instant,
    })
}

fn victory_receipts_match_authority(
    authority: &[ExternalDiplomacyAuthority],
    receipts: &[LeaderMatchReceipt],
) -> bool {
    if authority.len() != receipts.len() {
        return false;
    }
    authority.iter().zip(receipts).all(|(call, receipt)| {
        victory_request(call).is_ok_and(|request| receipt.request == request)
    })
}

fn set_plan_changes_relation(plan: &super::leader_set_diplo::SetDiploPlan) -> bool {
    plan.steps.iter().any(|step| {
        matches!(
            step,
            super::leader_set_diplo::SetDiploStep::Mutation(
                super::leader_set_diplo::SetDiploMutation::WriteDeclaration { .. }
            )
        )
    })
}

fn set_plan_has_victory(plan: &super::leader_set_diplo::SetDiploPlan) -> bool {
    plan.steps.iter().any(|step| {
        matches!(
            step,
            super::leader_set_diplo::SetDiploStep::Authority(SetDiploAuthority::Victory { .. })
        )
    })
}

/// The staged Leader/Match body must observe the relation image at the retail call site. The
/// current generic-victory cohort has one Victory and no later relation write, so the complete
/// diplomacy after-image is exactly that view. More complex ordering remains fail-closed.
fn victory_order_isolated(prepared: &PreparedDiplomacyTransaction) -> bool {
    if prepared.required_external_authority.len() != 1
        || victory_request(&prepared.required_external_authority[0]).is_err()
    {
        return false;
    }
    let mut saw_victory = false;
    let mut observe = |plan: &super::leader_set_diplo::SetDiploPlan| {
        if saw_victory && set_plan_changes_relation(plan) {
            return false;
        }
        saw_victory |= set_plan_has_victory(plan);
        true
    };
    let ordered = match &prepared.plan {
        super::canonical_diplomacy_host::PreparedDiplomacyPlan::Declare(plan) => {
            plan.steps.iter().all(|step| match step {
                super::diplomacy_declare_host::DeclareStep::RootSetDiplo(plan)
                | super::diplomacy_declare_host::DeclareStep::AllySetDiplo { plan, .. } => {
                    observe(plan)
                }
                _ => true,
            })
        }
        super::canonical_diplomacy_host::PreparedDiplomacyPlan::Accept(plan) => {
            plan.steps.iter().all(|step| match step {
                super::diplomacy_accept_host::AcceptStep::RootSetDiplo { plan, .. }
                | super::diplomacy_accept_host::AcceptStep::TeamFanoutSetDiplo { plan, .. } => {
                    observe(plan)
                }
                // Recursive declarations can contain multiple staged set_diplo images. They
                // remain outside this smallest child until the planner exports call-site images.
                super::diplomacy_accept_host::AcceptStep::RecursiveDeclare(_) => false,
                _ => true,
            })
        }
    };
    ordered && saw_victory
}

fn canonical_taunt(sim: &Sim, who: usize) -> leader_process_taunt::TauntLeaderState {
    let policy = &sim.vic_leaders.slots[who].init_diplomacy;
    let retained = &sim.diplomacy.leaders[who];
    let mut taunt = leader_process_taunt::TauntLeaderState {
        gift_stamp: policy.gift_stamp,
        last_taunt: policy.last_taunt,
        last_taunt_frame: policy.taunt_frame,
        tributes: retained.reserved_resources,
        ..leader_process_taunt::TauntLeaderState::default()
    };
    for target in 0..NUM_LEADERS {
        let proposal = retained.proposals[target];
        taunt.dip[target] = leader_process_taunt::Diplomacy {
            agree: proposal.agreement_pending,
            any_offer: proposal.proposal_open,
            treaty: proposal.treaty,
            offers: proposal.offers,
            dows: proposal.declaration_costs,
            attacks: proposal.attacks,
        };
    }
    taunt
}

/// Exact reconstructible step-8 view used by save admission and the tick synchronization edge.
pub fn step8_diplomacy_view_matches(sim: &Sim, who: usize) -> bool {
    let fresh = crate::systems::leaders::Leader::default();
    let actual = &sim.step8.leaders[who];
    (actual.diplo == fresh.diplo || actual.diplo == sim.vic_leaders.slots[who].diplos)
        && (actual.taunt == fresh.taunt || actual.taunt == canonical_taunt(sim, who))
}

fn project_owner(sim: &Sim) -> Result<DiplomacyOwnerImage, CanonicalDiplomacyRuntimeError> {
    let applied = sim
        .vic_leaders
        .setup_owner
        .applied()
        .ok_or(CanonicalDiplomacyRuntimeError::MissingPlayerSetup)?;
    let no_rush_frames = sim
        .diplomacy_authority
        .no_rush_frames
        .ok_or(CanonicalDiplomacyRuntimeError::MissingNoRushFrames)?;
    let mut owner = DiplomacyOwnerImage::default();
    owner.setup.players = std::array::from_fn(|who| {
        let player = applied.state.setup.players[who];
        crate::command::setup_diplomacy::PlayerSetup {
            flags: player.flags,
            who: player.who,
            team: player.team,
        }
    });
    owner.setup.team_style = applied.state.setup.team_style;
    owner.setup.semaphore_820 = applied.state.setup.semaphore_820;
    owner.setup.frame = sim.world.frame;
    owner.retained = sim.diplomacy.clone();
    owner.no_rush_frames = no_rush_frames;
    owner.local_who = sim
        .players
        .as_ref()
        .map_or(-1, |players| players.console_who);
    owner.victory_mask = sim.vic_match.semaphore;

    for who in 0..NUM_LEADERS {
        let leader = &sim.vic_leaders.slots[who];
        if sim.leaders[who].econ.stockpile != leader.economy.bucket {
            return Err(CanonicalDiplomacyRuntimeError::ResourceMirrorMismatch { who });
        }
        if leader.leader_flags2 as u32 != sim.army_leader_flags2[who] {
            return Err(CanonicalDiplomacyRuntimeError::LeaderFlags2MirrorMismatch { who });
        }
        for band in RetailBand::ALL {
            if sim.world.object_bands().mark(who, band) != Some(band.base()) {
                return Err(CanonicalDiplomacyRuntimeError::NonEmptyObjectBand { who, band });
            }
        }
        if sim.armies.lists[who].len() != super::leader_set_diplo::ARMY_SLOTS {
            return Err(CanonicalDiplomacyRuntimeError::ArmyShape {
                who,
                actual: sim.armies.lists[who].len(),
            });
        }

        owner.setup.leaders[who].who = leader.who;
        owner.setup.leaders[who].leader_flags = leader.leader_flags;
        owner.setup.leaders[who].diplos = leader.diplos;
        owner.resources[who] = leader.economy.bucket;
        owner.leader_flags2[who] = leader.leader_flags2 as u32;
        owner.shared_vision[who] = leader.init_diplomacy.ally_mask;
        owner.valid_armies[who] =
            std::array::from_fn(|slot| sim.armies.lists[who][slot].valid != 0);
        owner.leader_diplomacy[who] = CanonicalLeaderDiplomacyFields {
            response_314: leader.init_diplomacy.counteroffer,
            response_334: leader.init_diplomacy.tribute_demanded,
            declaration_frame: leader.init_diplomacy.broke_alliance,
            declaration_by_target: leader.init_diplomacy.dow,
            agenda_flags: leader.init_diplomacy.agendas.map(|value| value as u32),
            peace_frames: leader.init_diplomacy.made_peace,
            attack_frames: leader.init_diplomacy.hire_stamp,
            attack_peers: leader.init_diplomacy.hire_who,
            tribute_stamp: leader.init_diplomacy.tribute_stamp,
            gift_stamp: leader.init_diplomacy.gift_stamp,
        };
    }
    Ok(owner)
}

fn fold_owner_into_leaders(
    leaders: &mut super::victory_score::Leaders,
    owner: &DiplomacyOwnerImage,
) {
    for who in 0..NUM_LEADERS {
        let leader = &mut leaders.slots[who];
        leader.economy.bucket = owner.resources[who];
        leader.diplos = owner.setup.leaders[who].diplos;
        leader.leader_flags2 = owner.leader_flags2[who] as i32;
        leader.init_diplomacy.ally_mask = owner.shared_vision[who];
        let diplomacy = &owner.leader_diplomacy[who];
        leader.init_diplomacy.counteroffer = diplomacy.response_314;
        leader.init_diplomacy.tribute_demanded = diplomacy.response_334;
        leader.init_diplomacy.broke_alliance = diplomacy.declaration_frame;
        leader.init_diplomacy.dow = diplomacy.declaration_by_target;
        leader.init_diplomacy.agendas = diplomacy.agenda_flags.map(|value| value as i32);
        leader.init_diplomacy.made_peace = diplomacy.peace_frames;
        leader.init_diplomacy.hire_stamp = diplomacy.attack_frames;
        leader.init_diplomacy.hire_who = diplomacy.attack_peers;
        leader.init_diplomacy.tribute_stamp = diplomacy.tribute_stamp;
        leader.init_diplomacy.gift_stamp = diplomacy.gift_stamp;
    }
}

fn fold_owner(sim: &mut Sim, owner: DiplomacyOwnerImage) {
    sim.diplomacy = owner.retained.clone();
    sim.vic_match.semaphore = owner.victory_mask;
    fold_owner_into_leaders(&mut sim.vic_leaders, &owner);
    for who in 0..NUM_LEADERS {
        sim.leaders[who].econ.stockpile = owner.resources[who];
        sim.step8.leaders[who].econ.stockpile = owner.resources[who];
        sim.army_leader_flags2[who] = sim.vic_leaders.slots[who].leader_flags2 as u32;
        sim.step8.leaders[who].ai.flags2 = sim.vic_leaders.slots[who].leader_flags2 as u32;
        sim.step8.leaders[who].diplo = sim.vic_leaders.slots[who].diplos;
    }
    for who in 0..NUM_LEADERS {
        sim.step8.leaders[who].taunt = canonical_taunt(sim, who);
    }
}

struct StagedVictoryAuthority {
    leaders: super::victory_score::Leaders,
    game: super::victory_score::Match,
    builds: Vec<super::production::BuildData>,
    production_runtime: super::production::runtime::LiveProductionRuntime,
    receipts: Vec<LeaderMatchReceipt>,
}

fn stage_victory_authority(
    sim: &Sim,
    before: &DiplomacyOwnerImage,
    after: &DiplomacyOwnerImage,
    prepared: &PreparedDiplomacyTransaction,
    authority: &[ExternalDiplomacyAuthority],
) -> Result<Option<StagedVictoryAuthority>, CanonicalDiplomacyRuntimeError> {
    if authority.is_empty() {
        return Ok(None);
    }
    for call in authority {
        victory_request(call)?;
    }
    if sim.vic_leaders.pending_cleanup_masks() != (0, 0) || sim.defeat_cleanup_error.is_some() {
        return Err(CanonicalDiplomacyRuntimeError::PendingTerminalCleanup);
    }
    if !victory_order_isolated(prepared) {
        return Err(CanonicalDiplomacyRuntimeError::VictoryOrderingNotIsolated);
    }

    let mut leaders = sim.vic_leaders.clone();
    let mut game = sim.vic_match.clone();
    let mut builds = sim.builds.clone();
    let mut production_runtime = sim.production_runtime.clone();
    let mut receipts = Vec::with_capacity(authority.len());

    // `set_diplo` publishes both relation rows before its Victory call. Stage those exact
    // canonical fields first, while leaving the semaphore mutation ordered after the call.
    fold_owner_into_leaders(&mut leaders, after);
    for call in authority {
        let request = victory_request(call)?;
        let receipt = apply_leader_match(&mut leaders, &mut game, request)
            .map_err(CanonicalDiplomacyRuntimeError::Victory)?;
        receipts.push(receipt);
    }

    let defeated_owners = leaders.take_defeat_unit_cleanup();
    if defeated_owners != 0 {
        return Err(
            CanonicalDiplomacyRuntimeError::VictoryNeedsDefeatedUnitCleanup {
                owners: defeated_owners,
            },
        );
    }
    let build_owners = leaders.take_terminal_queue_cleanup();
    for owner in 0..NUM_LEADERS {
        if build_owners & (1u8 << owner) != 0 {
            production_runtime.clean_terminal_build_queues(&mut builds, owner);
        }
    }

    // Only fields changed by the diplomacy planner are applied after the authority call;
    // unrelated semaphore effects produced by `Leader::victory` remain intact.
    let set_bits = after.victory_mask & !before.victory_mask;
    let cleared_bits = before.victory_mask & !after.victory_mask;
    game.semaphore |= set_bits;
    game.semaphore &= !cleared_bits;
    Ok(Some(StagedVictoryAuthority {
        leaders,
        game,
        builds,
        production_runtime,
        receipts,
    }))
}

pub struct CanonicalDiplomacyFleet<'a> {
    sim: &'a mut Sim,
    receipt: Option<CanonicalDiplomacyReceipt>,
}

impl<'a> CanonicalDiplomacyFleet<'a> {
    pub fn new(sim: &'a mut Sim) -> Self {
        Self { sim, receipt: None }
    }

    pub fn take_receipt(&mut self) -> Option<CanonicalDiplomacyReceipt> {
        self.receipt.take()
    }
}

impl Fleet for CanonicalDiplomacyFleet<'_> {
    fn alive(&self, _who: u8, _o: i16) -> bool {
        false
    }
    fn is_unit(&self, _who: u8, _o: i16) -> bool {
        false
    }
    fn is_building(&self, _who: u8, _o: i16) -> bool {
        false
    }
    fn group_of(&self, _who: u8, _o: i16) -> i16 {
        -1
    }
    fn set_group_of(&mut self, _who: u8, _o: i16, _slot: i16) {}
    fn uid(&self, _who: u8, _o: i16) -> u16 {
        0
    }
    fn pos(&self, _who: u8, _o: i16) -> (i32, i32) {
        (0, 0)
    }
    fn orders(&self, _who: u8, _o: i16) -> Option<&OrderQueue> {
        None
    }
    fn orders_mut(&mut self, _who: u8, _o: i16) -> Option<&mut OrderQueue> {
        None
    }
    fn set_stance(&mut self, _who: u8, _o: i16, _stance: i8) {}
    fn disband(&mut self, _who: u8, _o: i16) {}

    fn apply_canonical_diplomacy_transaction(
        &mut self,
        request: CanonicalDiplomacyRequest,
    ) -> CanonicalDiplomacyReceipt {
        let result = (|| {
            if !matches!(
                request.wire.first().copied(),
                Some(DECLARE_OPCODE | ACCEPT_OPCODE)
            ) {
                return Err(CanonicalDiplomacyRuntimeError::UnsupportedOpcode(
                    request.wire.first().copied(),
                ));
            }
            if request.frame != self.sim.world.frame {
                return Err(CanonicalDiplomacyRuntimeError::FrameMismatch {
                    request: request.frame,
                    world: self.sim.world.frame,
                });
            }
            let before = project_owner(self.sim)?;
            let prepared = prepare_diplomacy_transaction(
                &before,
                &self.sim.diplomacy_authority,
                &request.wire,
            )
            .map_err(CanonicalDiplomacyRuntimeError::Prepare)?;
            let completed_authority = prepared.required_external_authority.clone();
            let mut committed = before.clone();
            commit_diplomacy_transaction(&mut committed, &prepared, &completed_authority)
                .map_err(CanonicalDiplomacyRuntimeError::Commit)?;
            let staged_victory = stage_victory_authority(
                self.sim,
                &before,
                &committed,
                &prepared,
                &completed_authority,
            )?;
            if project_owner(self.sim)? != before {
                return Err(CanonicalDiplomacyRuntimeError::StaleProjection);
            }
            fold_owner(self.sim, committed);
            let victory_receipts = if let Some(staged) = staged_victory {
                self.sim.vic_leaders = staged.leaders;
                self.sim.vic_match = staged.game;
                self.sim.builds = staged.builds;
                self.sim.production_runtime = staged.production_runtime;
                for who in 0..NUM_LEADERS {
                    self.sim.army_leader_flags2[who] =
                        self.sim.vic_leaders.slots[who].leader_flags2 as u32;
                    self.sim.step8.leaders[who].ai.flags2 =
                        self.sim.vic_leaders.slots[who].leader_flags2 as u32;
                }
                staged.receipts
            } else {
                Vec::new()
            };
            Ok(CanonicalDiplomacyReceipt {
                request: request.clone(),
                status: CanonicalDiplomacyStatus::Applied,
                installed_facts: Some(self.sim.diplomacy_authority.clone()),
                prepared: Some(prepared),
                completed_authority,
                victory_receipts,
                error: None,
            })
        })();
        let receipt = result
            .unwrap_or_else(|error| CanonicalDiplomacyReceipt::unavailable(request.clone(), error));
        self.receipt = Some(receipt.clone());
        receipt
    }
}

impl Sim {
    pub fn replace_diplomacy_authority(&mut self, authority: DiplomacyInstalledFacts) {
        self.diplomacy_authority = authority;
    }

    /// Execute one real opcode-38 or opcode-41 command through `Bridge::process_all` and the
    /// canonical Sim owner. Reached object/army/victory authority remains fail-closed.
    pub fn process_diplomacy_package(
        &mut self,
        play: i32,
        package_stamp: u32,
        bytes: &[u8],
    ) -> Result<CanonicalDiplomacyReceipt, CanonicalDiplomacyRuntimeError> {
        if !matches!(bytes.first().copied(), Some(DECLARE_OPCODE | ACCEPT_OPCODE)) {
            return Err(CanonicalDiplomacyRuntimeError::UnsupportedOpcode(
                bytes.first().copied(),
            ));
        }
        let mut bridge = Bridge::new();
        bridge.frame = self.world.frame;
        let mut package = Package::new(play, package_stamp);
        let mut fleet = CanonicalDiplomacyFleet::new(self);
        bridge
            .process_all(&mut package, bytes, &mut fleet)
            .map_err(CanonicalDiplomacyRuntimeError::Wire)?;
        fleet
            .take_receipt()
            .ok_or(CanonicalDiplomacyRuntimeError::BridgeDidNotReturnReceipt)
    }
}
