// SPDX-License-Identifier: GPL-3.0-or-later
//! Production Bridge/Sim mount for authority-complete opcode-38 and opcode-41 transactions.
//!
//! The whole declaration/acceptance body is prepared by [`super::canonical_diplomacy_host`]. This
//! adapter projects its detached image from the existing Sim owners, refuses any transaction
//! which reaches an external object/army/victory authority, and publishes the remaining
//! declaration or acceptance in one assignment-only fold. Any transaction which reaches an
//! authority outside this aggregate remains unavailable before publication.

use super::canonical_diplomacy_host::{
    commit_diplomacy_transaction, prepare_diplomacy_transaction, CanonicalLeaderDiplomacyFields,
    CommitDiplomacyError, DiplomacyInstalledFacts, DiplomacyOwnerImage, ExternalDiplomacyAuthority,
    PrepareDiplomacyError, PreparedDiplomacyTransaction,
};
use super::leader_process_taunt;
use super::sparse_object_bands_authority_frontier::RetailBand;
use crate::command::{
    diplomacy_command_plans::{ACCEPT_OPCODE, DECLARE_OPCODE},
    Bridge, Fleet, Package, WireError,
};
use crate::systems::order_dispatch::OrderQueue;
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
                    && self.error.is_some()
            }
            CanonicalDiplomacyStatus::Applied => {
                let (Some(facts), Some(prepared)) = (&self.installed_facts, &self.prepared) else {
                    return false;
                };
                self.error.is_none()
                    && prepared.wire == expected.wire
                    && self.completed_authority == prepared.required_external_authority
                    && prepare_diplomacy_transaction(&prepared.before_owner, facts, &expected.wire)
                        .is_ok_and(|recomputed| recomputed == *prepared)
            }
        }
    }
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

fn fold_owner(sim: &mut Sim, owner: DiplomacyOwnerImage) {
    sim.diplomacy = owner.retained;
    sim.vic_match.semaphore = owner.victory_mask;
    for who in 0..NUM_LEADERS {
        sim.leaders[who].econ.stockpile = owner.resources[who];
        sim.step8.leaders[who].econ.stockpile = owner.resources[who];
        let leader = &mut sim.vic_leaders.slots[who];
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
        sim.army_leader_flags2[who] = owner.leader_flags2[who];
        sim.step8.leaders[who].diplo = leader.diplos;
    }
    for who in 0..NUM_LEADERS {
        sim.step8.leaders[who].taunt = canonical_taunt(sim, who);
    }
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
            if !prepared.required_external_authority.is_empty() {
                return Err(CanonicalDiplomacyRuntimeError::ExternalAuthority(
                    prepared.required_external_authority.clone(),
                ));
            }
            let mut committed = before.clone();
            commit_diplomacy_transaction(&mut committed, &prepared, &[])
                .map_err(CanonicalDiplomacyRuntimeError::Commit)?;
            if project_owner(self.sim)? != before {
                return Err(CanonicalDiplomacyRuntimeError::StaleProjection);
            }
            fold_owner(self.sim, committed);
            Ok(CanonicalDiplomacyReceipt {
                request: request.clone(),
                status: CanonicalDiplomacyStatus::Applied,
                installed_facts: Some(self.sim.diplomacy_authority.clone()),
                prepared: Some(prepared),
                completed_authority: Vec::new(),
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
