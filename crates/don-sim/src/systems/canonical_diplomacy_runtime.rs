// SPDX-License-Identifier: GPL-3.0-or-later
//! Production Bridge/Sim mount for authority-complete opcode-38 and opcode-41 transactions.
//!
//! The whole declaration/acceptance body is prepared by [`super::canonical_diplomacy_host`]. This
//! adapter projects its detached image from the existing Sim owners and publishes the remaining
//! declaration or acceptance in one assignment-only fold. Generic alliance victory is staged
//! through the canonical Leader/Match transaction; its defeated-player cleanup admits exact
//! empty Army rosters and on-map ground Unit bands. It also owns the exact forced-Army arm where
//! the entry countdown decrements and the leader's armies-off bit returns before normalization.
//! The exact instruction-ordered no-op Victory plus armies-off cohort publishes atomically too;
//! an active winner's non-mustering Army also normalizes and retires atomically when it has no
//! Groups or its live prefix consists only of already-empty persistent Groups; the latter are
//! unlinked in the same publication.
//! An empty mustering Army with a live human countdown normalizes and clamps its rally atomically.
//! An expired empty naval muster with a foreign/inactive canonical City releases, enters the
//! zero-mobile `do_marching` close arm, and publishes in the same Victory-plus-Army transaction,
//! with the actually-read City bytes in its stale CAS.
//! An empty released land muster mounts the same close arm when its saved Leader strategy word
//! cannot route through unresolved difficulty/defending or transporting authority.
//! Every broader external authority remains unavailable before publication.

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
use crate::objects::Band;
use crate::order::Order;
use crate::systems::defeat_cleanup::DefeatCleanupReceipt;
use crate::systems::diplomacy_force_army_authority::{
    commit_force_army_process_with_groups_strategy_and_difficulty,
    prepare_force_army_process_with_groups_strategy_and_difficulty, ForceArmyProcessReceipt,
    ForceArmyProcessRequest,
};
use crate::systems::order_dispatch::OrderQueue;
use crate::tick::leader_match_host::{
    apply_leader_match, LeaderMatchError, LeaderMatchReceipt, LeaderMatchRequest,
};
use crate::tick::{Sim, NUM_LEADERS};
use crate::world::OBJ_FLAG_ACTIVE;

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
    FrameMismatch {
        request: i32,
        world: i32,
    },
    MissingPlayerSetup,
    MissingNoRushFrames,
    ResourceMirrorMismatch {
        who: usize,
    },
    LeaderFlags2MirrorMismatch {
        who: usize,
    },
    NonEmptyObjectBand {
        who: usize,
        band: RetailBand,
    },
    ContainedEjectionAuthority(
        super::diplomacy_ejection_authority::ContainedEjectionAuthorityError,
    ),
    ArmyShape {
        who: usize,
        actual: usize,
    },
    Prepare(PrepareDiplomacyError),
    ExternalAuthority(Vec<ExternalDiplomacyAuthority>),
    PendingTerminalCleanup,
    UnsupportedVictoryType(i32),
    VictoryOrderingNotIsolated,
    Victory(LeaderMatchError),
    VictoryNeedsNonVacuousDefeatCleanup {
        owners: u8,
    },
    VictoryMalformedArmyLink {
        owner: usize,
        army_slot: usize,
        group_id: i32,
    },
    VictoryNeedsPlaneDefeatCleanup {
        owner: usize,
        object_id: usize,
    },
    VictoryDefeatCleanup(super::defeat_cleanup::DefeatCleanupError),
    ForceArmy(super::diplomacy_force_army_authority::ForceArmyProcessError),
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
    /// Typed evidence for every staged defeated-player Army/Unit sweep.
    pub defeat_cleanup: Option<DiplomacyDefeatCleanupReceipt>,
    /// Exact v17-owned Army entry mutations completed before the diplomacy publish.
    pub army_process_receipts: Vec<ForceArmyProcessReceipt>,
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
            defeat_cleanup: None,
            army_process_receipts: Vec::new(),
            error: Some(error),
        }
    }

    fn unavailable_prepared(
        request: CanonicalDiplomacyRequest,
        installed_facts: DiplomacyInstalledFacts,
        prepared: PreparedDiplomacyTransaction,
        authority: Vec<ExternalDiplomacyAuthority>,
    ) -> Self {
        Self {
            request,
            status: CanonicalDiplomacyStatus::Unavailable,
            installed_facts: Some(installed_facts),
            prepared: Some(prepared),
            completed_authority: Vec::new(),
            victory_receipts: Vec::new(),
            defeat_cleanup: None,
            army_process_receipts: Vec::new(),
            error: Some(CanonicalDiplomacyRuntimeError::ExternalAuthority(authority)),
        }
    }

    pub fn validates(&self, expected: &CanonicalDiplomacyRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            CanonicalDiplomacyStatus::Unavailable => {
                let common = self.completed_authority.is_empty()
                    && self.victory_receipts.is_empty()
                    && self.defeat_cleanup.is_none()
                    && self.army_process_receipts.is_empty();
                match (&self.installed_facts, &self.prepared, &self.error) {
                    (None, None, Some(_)) => common,
                    (
                        Some(facts),
                        Some(prepared),
                        Some(CanonicalDiplomacyRuntimeError::ExternalAuthority(authority)),
                    ) => {
                        common
                            && prepared.wire == expected.wire
                            && authority == &prepared.required_external_authority
                            && prepare_diplomacy_transaction(
                                &prepared.before_owner,
                                facts,
                                &expected.wire,
                            )
                            .is_ok_and(|recomputed| recomputed == *prepared)
                    }
                    _ => false,
                }
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
                        &self.army_process_receipts,
                    )
                    && defeat_cleanup_matches_victory(
                        &self.victory_receipts,
                        self.defeat_cleanup.as_ref(),
                    )
                    && prepare_diplomacy_transaction(&prepared.before_owner, facts, &expected.wire)
                        .is_ok_and(|recomputed| recomputed == *prepared)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiplomacyDefeatCleanupReceipt {
    pub owners: u8,
    pub per_owner: Vec<DefeatCleanupReceipt>,
}

impl DiplomacyDefeatCleanupReceipt {
    fn validates(&self) -> bool {
        self.per_owner.len() == self.owners.count_ones() as usize
            && self.per_owner.iter().all(|receipt| {
                receipt.owner < NUM_LEADERS
                    && self.owners & (1u8 << receipt.owner) != 0
                    && receipt.armies_stopped <= super::leader_set_diplo::ARMY_SLOTS
                    && receipt.groups_stopped
                        <= receipt.armies_stopped * super::armies::ARMY_MAX_GROUPS
                    && receipt.army_members_halted
                        <= receipt.groups_stopped * super::groups_guys::GROUP_MAX_MEMBERS
                    && receipt.planes_killed == 0
                    && receipt.slots_visited == receipt.invalid_skipped + receipt.orders_closed
                    && receipt.unit_masks_cleared == receipt.orders_closed
            })
            && self
                .per_owner
                .windows(2)
                .all(|pair| pair[0].owner < pair[1].owner)
    }
}

fn defeat_cleanup_matches_victory(
    victories: &[LeaderMatchReceipt],
    cleanup: Option<&DiplomacyDefeatCleanupReceipt>,
) -> bool {
    if victories.is_empty() {
        return cleanup.is_none();
    }
    let Some(cleanup) = cleanup else { return false };
    cleanup.validates()
        && cleanup.owners
            == victories
                .iter()
                .fold(0u8, |owners, receipt| owners | receipt.defeat_unit_cleanup)
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
    army_receipts: &[ForceArmyProcessReceipt],
) -> bool {
    if authority.len() != receipts.len() + army_receipts.len() {
        return false;
    }
    let mut victory = receipts.iter();
    let mut army = army_receipts.iter();
    authority.iter().all(|call| match set_diplo_call(call) {
        SetDiploAuthority::Victory { .. } => victory.next().is_some_and(|receipt| {
            victory_request(call).is_ok_and(|request| receipt.request == request)
        }),
        SetDiploAuthority::ForceArmyProcess {
            owner,
            army_slot,
            forced,
        } => army.next().is_some_and(|receipt| {
            receipt.request
                == (ForceArmyProcessRequest {
                    owner,
                    army_slot,
                    forced,
                })
                && receipt.validates()
        }),
        _ => false,
    }) && victory.next().is_none()
        && army.next().is_none()
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
/// supported generic-victory cohort has one Victory and no later relation write. It may then run
/// only that winner's exact bounded forced-Army calls, matching `Leader::set_diplo`'s instruction
/// order.
/// More complex ordering remains fail-closed.
fn victory_order_isolated(prepared: &PreparedDiplomacyTransaction) -> bool {
    let mut winner = None;
    for call in &prepared.required_external_authority {
        match set_diplo_call(call) {
            SetDiploAuthority::Victory { .. } => {
                let Ok(LeaderMatchRequest::Victory { who, .. }) = victory_request(call) else {
                    return false;
                };
                if winner.replace(who).is_some() {
                    return false;
                }
            }
            SetDiploAuthority::ForceArmyProcess { owner, .. }
                if winner.is_some_and(|winner| owner == winner) => {}
            _ => return false,
        }
    }
    if winner.is_none() {
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
    let ejection_units = sim
        .diplomacy_authority
        .contained_ejection
        .project(&sim.world, sim.channel_digest())
        .map_err(CanonicalDiplomacyRuntimeError::ContainedEjectionAuthority)?;

    for who in 0..NUM_LEADERS {
        let leader = &sim.vic_leaders.slots[who];
        if sim.leaders[who].econ.stockpile != leader.economy.bucket {
            return Err(CanonicalDiplomacyRuntimeError::ResourceMirrorMismatch { who });
        }
        if leader.leader_flags2 as u32 != sim.army_leader_flags2[who] {
            return Err(CanonicalDiplomacyRuntimeError::LeaderFlags2MirrorMismatch { who });
        }
        for band in [RetailBand::Build, RetailBand::Wall] {
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
        owner.ejection_units[who] = ejection_units[who].clone();
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
    world: crate::world::World,
    groups: super::groups_guys::Groups,
    paths: Vec<super::movement::PathStack>,
    builds: Vec<super::production::BuildData>,
    production_runtime: super::production::runtime::LiveProductionRuntime,
    receipts: Vec<LeaderMatchReceipt>,
    defeat_cleanup: DiplomacyDefeatCleanupReceipt,
}

struct StagedForceArmyAuthority {
    armies: Box<super::armies::Armies>,
    receipts: Vec<ForceArmyProcessReceipt>,
}

impl StagedForceArmyAuthority {
    fn current_error(
        &self,
        armies: &super::armies::Armies,
        cities: &super::tech_cities::CityPool,
        groups: &super::groups_guys::Groups,
        leader_city_num: &[i32; NUM_LEADERS],
        world_size: (i32, i32),
        leader_strategy: &[[u16; super::army_do_mustering::MUSTER_STRATEGY_REGIONS]; NUM_LEADERS],
        match_semaphore: u32,
        leader_multi_diff: &[i32; NUM_LEADERS],
    ) -> Option<super::diplomacy_force_army_authority::ForceArmyProcessError> {
        self.receipts.iter().find_map(|receipt| {
            if receipt
                .leader_city_num
                .is_some_and(|city_num| leader_city_num[receipt.request.owner] != city_num)
            {
                return Some(
                    super::diplomacy_force_army_authority::ForceArmyProcessError::StaleLeader {
                        owner: receipt.request.owner,
                    },
                );
            }
            if receipt
                .world_size
                .is_some_and(|expected| expected != world_size)
            {
                return Some(
                    super::diplomacy_force_army_authority::ForceArmyProcessError::StaleWorld,
                );
            }
            if !receipt.retirement_groups_are_current(groups) {
                let gid = receipt
                    .retirement_groups
                    .as_ref()
                    .and_then(|facts| {
                        facts.iter().find(|fact| {
                            !fact.is_current(groups)
                        })
                    })
                    .map_or(0, |fact| fact.gid);
                return Some(
                    super::diplomacy_force_army_authority::ForceArmyProcessError::StaleRetirementGroup {
                        owner: receipt.request.owner,
                        gid,
                    },
                );
            }
            if !receipt.retirement_cities_are_current(cities) {
                return Some(
                    super::diplomacy_force_army_authority::ForceArmyProcessError::StaleRetirementCity {
                        owner: receipt.request.owner,
                    },
                );
            }
            if !receipt.muster_city_is_current(cities) {
                return Some(
                    super::diplomacy_force_army_authority::ForceArmyProcessError::StaleCity {
                        owner: receipt.request.owner,
                        city: receipt.before.city,
                    },
                );
            }
            if !receipt.muster_strategy_is_current(leader_strategy) {
                return Some(
                    super::diplomacy_force_army_authority::ForceArmyProcessError::StaleStrategy {
                        owner: receipt.request.owner,
                        region: receipt
                            .muster_strategy
                            .expect("stale strategy requires a strategy receipt")
                            .region,
                    },
                );
            }
            if !receipt.muster_difficulty_is_current(match_semaphore, leader_multi_diff) {
                return Some(
                    super::diplomacy_force_army_authority::ForceArmyProcessError::StaleDifficulty {
                        owner: receipt.request.owner,
                    },
                );
            }
            (armies
                .lists
                .get(receipt.request.owner)
                .and_then(|list| list.get(receipt.request.army_slot))
                != Some(&receipt.before))
            .then_some(
                super::diplomacy_force_army_authority::ForceArmyProcessError::StaleArmy {
                    owner: receipt.request.owner,
                    army_slot: receipt.request.army_slot,
                },
            )
        })
    }
}

fn stage_ground_defeat_cleanup(
    sim: &Sim,
    owners: u8,
) -> Result<
    (
        crate::world::World,
        super::groups_guys::Groups,
        Vec<super::movement::PathStack>,
        DiplomacyDefeatCleanupReceipt,
    ),
    CanonicalDiplomacyRuntimeError,
> {
    use super::defeat_cleanup::{DefeatCleanupError as Error, DEFEAT_UNIT_MASK};

    let mut world = sim.world.clone();
    let mut groups = sim.groups.clone();
    let mut paths = sim.paths.clone();
    let mut army_group_predecessor = vec![None; groups.list.len()];
    let mut per_owner = Vec::with_capacity(owners.count_ones() as usize);
    for owner in 0..NUM_LEADERS {
        if owners & (1u8 << owner) == 0 {
            continue;
        }
        let armies = sim.armies.leader_defeated_targets(owner);
        let mut army_plans = Vec::with_capacity(armies.groups.len());
        for target in armies.groups.iter().copied() {
            let group_id = target.group_id as usize;
            let Some(group) = groups.list.get(group_id).cloned() else {
                return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                    Error::MissingArmyGroup {
                        owner,
                        army_slot: target.army_slot,
                        group_id: target.group_id,
                    },
                ));
            };
            if army_group_predecessor[group_id]
                .replace((owner, target.army_slot))
                .is_some()
                || (group.num != 0 && group.army != target.army_slot as i32)
            {
                return Err(CanonicalDiplomacyRuntimeError::VictoryMalformedArmyLink {
                    owner,
                    army_slot: target.army_slot,
                    group_id: target.group_id,
                });
            }
            if group.num == 0 {
                continue;
            }
            if group.who as usize != owner {
                return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                    Error::ArmyGroupOwnerMismatch {
                        owner,
                        army_slot: target.army_slot,
                        group_id: target.group_id,
                        group_owner: group.who,
                    },
                ));
            }
            if group.buildings != 0 {
                let plan =
                    super::groups_guys::plan_action_halt(&group, 0, &[]).map_err(|error| {
                        CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(Error::ArmyGroupPlan {
                            owner,
                            army_slot: target.army_slot,
                            group_id: target.group_id,
                            error,
                        })
                    })?;
                army_plans.push((group_id, plan));
                continue;
            }

            let n = group
                .num
                .clamp(0, super::groups_guys::GROUP_MAX_MEMBERS as i32)
                as usize;
            let mut members = Vec::with_capacity(n);
            for &member_o in &group.list[..n] {
                let mut facts = super::groups_guys::HaltMemberFacts {
                    o: member_o,
                    ..Default::default()
                };
                let Ok(object_id) = usize::try_from(member_o) else {
                    members.push(facts);
                    continue;
                };
                let Some(row) = world
                    .objects
                    .slot(owner)
                    .band(Band::Unit)
                    .get(object_id)
                    .copied()
                    .map(|row| row as usize)
                else {
                    members.push(facts);
                    continue;
                };
                if row >= world.units.len() || world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                    members.push(facts);
                    continue;
                }

                facts.valid_unit = true;
                facts.on_map = crate::systems::air::is_on_map(world.units.inside_up()[row]);
                if !facts.on_map {
                    members.push(facts);
                    continue;
                }
                let Some(&type_index) = sim.unit_type.get(row) else {
                    return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                        Error::MissingUnitType { owner, object_id },
                    ));
                };
                let Some(is_plane) = sim.production_runtime.installed_unit_is_plane(type_index)
                else {
                    return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                        Error::UnsupportedUnitType {
                            owner,
                            object_id,
                            type_index,
                        },
                    ));
                };
                facts.is_plane = is_plane;
                if is_plane {
                    facts.domain = 2;
                } else {
                    let Some(entering_or_exiting) = world
                        .orders(row)
                        .current()
                        .map_or(Some(false), Order::is_entering_or_exiting)
                    else {
                        return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                            Error::MissingSpecialAnimPayload { owner, object_id },
                        ));
                    };
                    facts.entering_or_exiting = entering_or_exiting;
                    if !entering_or_exiting && paths.get(row).is_none() {
                        return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                            Error::MissingPathState { owner, object_id },
                        ));
                    }
                }
                members.push(facts);
            }
            let plan =
                super::groups_guys::plan_action_halt(&group, 0, &members).map_err(|error| {
                    CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(Error::ArmyGroupPlan {
                        owner,
                        army_slot: target.army_slot,
                        group_id: target.group_id,
                        error,
                    })
                })?;
            army_plans.push((group_id, plan));
        }

        let object_rows = sim.world.objects.slot(owner).band(Band::Unit).to_vec();
        let mut plan = Vec::with_capacity(object_rows.len());
        let mut invalid_skipped = 0usize;
        for (object_id, row) in object_rows.iter().copied().enumerate() {
            let row = row as usize;
            if row >= world.units.len() {
                return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                    Error::MissingUnitType { owner, object_id },
                ));
            }
            if world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                invalid_skipped += 1;
                continue;
            }
            let Some(&type_index) = sim.unit_type.get(row) else {
                return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                    Error::MissingUnitType { owner, object_id },
                ));
            };
            let Some(is_plane) = sim.production_runtime.installed_unit_is_plane(type_index) else {
                return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                    Error::UnsupportedUnitType {
                        owner,
                        object_id,
                        type_index,
                    },
                ));
            };
            if is_plane {
                return Err(
                    CanonicalDiplomacyRuntimeError::VictoryNeedsPlaneDefeatCleanup {
                        owner,
                        object_id,
                    },
                );
            }
            if paths.get(row).is_none() {
                return Err(CanonicalDiplomacyRuntimeError::VictoryDefeatCleanup(
                    Error::MissingPathState { owner, object_id },
                ));
            }
            plan.push(row);
        }

        let mut receipt = DefeatCleanupReceipt {
            owner,
            armies_stopped: armies.valid_armies,
            groups_stopped: army_plans.len(),
            slots_visited: object_rows.len(),
            invalid_skipped,
            ..DefeatCleanupReceipt::default()
        };
        for (group_id, halt) in army_plans {
            groups.list[group_id] = halt.group;
            for step in halt.steps {
                let (who, object_id) = match step {
                    super::groups_guys::HaltStep::ClearUnitMask { who, o, .. }
                    | super::groups_guys::HaltStep::ClearPathAnchor { who, o }
                    | super::groups_guys::HaltStep::CloseOrders { who, o, .. }
                    | super::groups_guys::HaltStep::ClearPartialPath { who, o }
                    | super::groups_guys::HaltStep::UpdateAction { who, o } => {
                        (who as usize, o as usize)
                    }
                };
                let row = world.objects.slot(who).band(Band::Unit)[object_id] as usize;
                match step {
                    super::groups_guys::HaltStep::ClearUnitMask { mask, .. } => {
                        let masks = world.units.get_unit_masks(row) & !mask;
                        world.units.set_unit_masks(row, masks);
                    }
                    super::groups_guys::HaltStep::ClearPathAnchor { .. }
                    | super::groups_guys::HaltStep::ClearPartialPath { .. } => {
                        paths[row].clear();
                    }
                    super::groups_guys::HaltStep::CloseOrders { .. } => {
                        world.orders_mut(row).clear();
                    }
                    super::groups_guys::HaltStep::UpdateAction { .. } => {
                        let x = world.units.x_internal()[row];
                        let y = world.units.y_internal()[row];
                        let angle = world.units.angle()[row];
                        world.units.orders_x_mut()[row] = x;
                        world.units.orders_y_mut()[row] = y;
                        world.units.dest_angle_mut()[row] = angle;
                        receipt.army_members_halted += 1;
                    }
                }
            }
        }
        for row in plan {
            world.orders_mut(row).clear();
            paths[row].clear();
            let masks = world.units.get_unit_masks(row) & !(0x0400_0000 | DEFEAT_UNIT_MASK);
            world.units.set_unit_masks(row, masks);
            let x = world.units.x_internal()[row];
            let y = world.units.y_internal()[row];
            let angle = world.units.angle()[row];
            world.units.orders_x_mut()[row] = x;
            world.units.orders_y_mut()[row] = y;
            world.units.dest_angle_mut()[row] = angle;
            receipt.orders_closed += 1;
            receipt.unit_masks_cleared += 1;
        }
        per_owner.push(receipt);
    }
    let receipt = DiplomacyDefeatCleanupReceipt { owners, per_owner };
    debug_assert!(receipt.validates());
    Ok((world, groups, paths, receipt))
}

fn stage_force_army_authority(
    sim: &Sim,
    leader_flags: &[u32; NUM_LEADERS],
    leader_flags2: &[u32; NUM_LEADERS],
    leader_strategy: &[[u16; super::army_do_mustering::MUSTER_STRATEGY_REGIONS]; NUM_LEADERS],
    match_semaphore: u32,
    leader_multi_diff: &[i32; NUM_LEADERS],
    authority: &[ExternalDiplomacyAuthority],
) -> Result<Option<StagedForceArmyAuthority>, CanonicalDiplomacyRuntimeError> {
    if authority.is_empty() {
        return Ok(None);
    }
    let mut requests = Vec::with_capacity(authority.len());
    for call in authority {
        match set_diplo_call(call) {
            SetDiploAuthority::Victory { .. } => continue,
            SetDiploAuthority::ForceArmyProcess {
                owner,
                army_slot,
                forced,
            } => {
                requests.push(ForceArmyProcessRequest {
                    owner,
                    army_slot,
                    forced,
                });
            }
            _ => {
                return Err(CanonicalDiplomacyRuntimeError::ExternalAuthority(
                    authority.to_vec(),
                ));
            }
        }
    }
    if requests.is_empty() {
        return Ok(None);
    }
    // `LeaderData::city_num` is the maintained live-City count. CityPool is the canonical
    // saved owner here, so reconstruct the field from it instead of depending on step 8's
    // transient LeaderData projection.
    let leader_city_num = std::array::from_fn(|who| sim.cities.count(who));
    let world_size = (sim.map.world.tile_xs, sim.map.world.tile_ys);
    let prepared = match prepare_force_army_process_with_groups_strategy_and_difficulty(
        &sim.armies,
        &sim.cities,
        &sim.groups,
        leader_flags,
        leader_flags2,
        &leader_city_num,
        world_size,
        leader_strategy,
        match_semaphore,
        leader_multi_diff,
        &requests,
    ) {
        Ok(prepared) => prepared,
        Err(
            super::diplomacy_force_army_authority::ForceArmyProcessError::RequiresUnresolvedArmyBody {
                ..
            },
        ) => {
            return Err(CanonicalDiplomacyRuntimeError::ExternalAuthority(
                authority.to_vec(),
            ));
        }
        Err(error) => return Err(CanonicalDiplomacyRuntimeError::ForceArmy(error)),
    };
    let mut armies = Box::new(sim.armies.clone());
    let mut groups = sim.groups.clone();
    let receipts = commit_force_army_process_with_groups_strategy_and_difficulty(
        &mut armies,
        &sim.cities,
        &mut groups,
        leader_flags,
        leader_flags2,
        &leader_city_num,
        world_size,
        leader_strategy,
        match_semaphore,
        leader_multi_diff,
        prepared,
    )
    .map_err(CanonicalDiplomacyRuntimeError::ForceArmy)?;
    Ok(Some(StagedForceArmyAuthority { armies, receipts }))
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
    let victory_calls = authority
        .iter()
        .filter(|call| matches!(set_diplo_call(call), SetDiploAuthority::Victory { .. }))
        .collect::<Vec<_>>();
    if authority.iter().any(|call| {
        !matches!(
            set_diplo_call(call),
            SetDiploAuthority::Victory { .. } | SetDiploAuthority::ForceArmyProcess { .. }
        )
    }) {
        // Preserve the whole instruction-ordered mutation cone for the next host. Returning
        // only the first call would make a typed producer look more complete than it is.
        return Err(CanonicalDiplomacyRuntimeError::ExternalAuthority(
            authority.to_vec(),
        ));
    }
    if victory_calls.is_empty() {
        return Ok(None);
    }
    for call in &victory_calls {
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
    let mut receipts = Vec::with_capacity(victory_calls.len());

    // `set_diplo` publishes both relation rows before its Victory call. Stage those exact
    // canonical fields first, while leaving the semaphore mutation ordered after the call.
    fold_owner_into_leaders(&mut leaders, after);
    for call in victory_calls {
        let request = victory_request(call)?;
        let receipt = apply_leader_match(&mut leaders, &mut game, request)
            .map_err(CanonicalDiplomacyRuntimeError::Victory)?;
        receipts.push(receipt);
    }

    let defeated_owners = leaders.take_defeat_unit_cleanup();
    let (world, groups, paths, defeat_cleanup) = stage_ground_defeat_cleanup(sim, defeated_owners)?;
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
        world,
        groups,
        paths,
        builds,
        production_runtime,
        receipts,
        defeat_cleanup,
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
            let staged_victory = match stage_victory_authority(
                self.sim,
                &before,
                &committed,
                &prepared,
                &completed_authority,
            ) {
                Ok(staged) => staged,
                Err(CanonicalDiplomacyRuntimeError::ExternalAuthority(authority)) => {
                    return Ok(CanonicalDiplomacyReceipt::unavailable_prepared(
                        request.clone(),
                        self.sim.diplomacy_authority.clone(),
                        prepared,
                        authority,
                    ));
                }
                Err(error) => return Err(error),
            };
            // A mixed cohort reaches Armies only after `Leader::victory`, so the exact owner
            // gates observe the staged post-Victory Leader words rather than the old live copy.
            let leader_flags = std::array::from_fn(|who| {
                staged_victory
                    .as_ref()
                    .map_or(self.sim.vic_leaders.slots[who].leader_flags, |staged| {
                        staged.leaders.slots[who].leader_flags
                    }) as u32
            });
            let leader_flags2 = std::array::from_fn(|who| {
                staged_victory
                    .as_ref()
                    .map_or(self.sim.vic_leaders.slots[who].leader_flags2, |staged| {
                        staged.leaders.slots[who].leader_flags2
                    }) as u32
            });
            let leader_strategy = std::array::from_fn(|who| {
                staged_victory
                    .as_ref()
                    .map_or(self.sim.vic_leaders.slots[who].strategy, |staged| {
                        staged.leaders.slots[who].strategy
                    })
            });
            let leader_multi_diff = std::array::from_fn(|who| {
                staged_victory
                    .as_ref()
                    .map_or(self.sim.vic_leaders.slots[who].multi_diff, |staged| {
                        staged.leaders.slots[who].multi_diff
                    })
            });
            let match_semaphore = staged_victory
                .as_ref()
                .map_or(self.sim.vic_match.semaphore, |staged| staged.game.semaphore);
            let staged_army = match stage_force_army_authority(
                self.sim,
                &leader_flags,
                &leader_flags2,
                &leader_strategy,
                match_semaphore,
                &leader_multi_diff,
                &completed_authority,
            ) {
                Ok(staged) => staged,
                Err(CanonicalDiplomacyRuntimeError::ExternalAuthority(authority)) => {
                    return Ok(CanonicalDiplomacyReceipt::unavailable_prepared(
                        request.clone(),
                        self.sim.diplomacy_authority.clone(),
                        prepared,
                        authority,
                    ));
                }
                Err(error) => return Err(error),
            };
            if project_owner(self.sim)? != before {
                return Err(CanonicalDiplomacyRuntimeError::StaleProjection);
            }
            let leader_city_num = std::array::from_fn(|who| self.sim.cities.count(who));
            let leader_strategy =
                std::array::from_fn(|who| self.sim.vic_leaders.slots[who].strategy);
            let leader_multi_diff =
                std::array::from_fn(|who| self.sim.vic_leaders.slots[who].multi_diff);
            let world_size = (self.sim.map.world.tile_xs, self.sim.map.world.tile_ys);
            if let Some(error) = staged_army.as_ref().and_then(|staged| {
                staged.current_error(
                    &self.sim.armies,
                    &self.sim.cities,
                    &self.sim.groups,
                    &leader_city_num,
                    world_size,
                    &leader_strategy,
                    self.sim.vic_match.semaphore,
                    &leader_multi_diff,
                )
            }) {
                return Err(CanonicalDiplomacyRuntimeError::ForceArmy(error));
            }
            fold_owner(self.sim, committed);
            let victory_receipts = if let Some(staged) = staged_victory {
                self.sim.vic_leaders = staged.leaders;
                self.sim.vic_match = staged.game;
                self.sim.world = staged.world;
                self.sim.groups = staged.groups;
                self.sim.paths = staged.paths;
                self.sim.builds = staged.builds;
                self.sim.production_runtime = staged.production_runtime;
                for who in 0..NUM_LEADERS {
                    self.sim.army_leader_flags2[who] =
                        self.sim.vic_leaders.slots[who].leader_flags2 as u32;
                    self.sim.step8.leaders[who].ai.flags2 =
                        self.sim.vic_leaders.slots[who].leader_flags2 as u32;
                }
                let receipts = staged.receipts;
                let defeat_cleanup = Some(staged.defeat_cleanup);
                (receipts, defeat_cleanup)
            } else {
                (Vec::new(), None)
            };
            let army_process_receipts = if let Some(staged) = staged_army {
                for fact in staged
                    .receipts
                    .iter()
                    .flat_map(|receipt| receipt.retirement_groups.iter().flatten())
                {
                    self.sim.groups.list[fact.gid].num = 0;
                    self.sim.groups.list[fact.gid].army = -1;
                }
                self.sim.armies = *staged.armies;
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
                victory_receipts: victory_receipts.0,
                defeat_cleanup: victory_receipts.1,
                army_process_receipts,
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
    /// canonical Sim owner. Reached object authority, broader Army processing, and unsupported
    /// Victory cleanup remain fail-closed.
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
