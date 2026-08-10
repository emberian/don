// SPDX-License-Identifier: GPL-3.0-or-later
//! Complete diplomacy/shared-vision loop from `Leader::init`.
//!
//! This source-only owner covers `0x006E3BF9..0x006E3D93` in the shipped
//! `Leader::init(int who, int tribe, int local)` body. It initializes one receiver's
//! eight raw diplomacy rows, treaty/interaction arrays, aggression flags, and
//! `LeaderData::ally_mask`. The body is modeled in instruction order because later
//! `is_ally` and `is_team` calls observe earlier writes in the same loop.
//!
//! The frame-zero PlayerSetup owner consumes this loop in sequential Leader order. That
//! integration supplies every option, semaphore, and prerequisite fact explicitly so a
//! host cannot publish a plausible-looking team table without the shipped row effects.

use super::setup_diplomacy::{IsTeamArg, SetupDiplomacy, TeamQueryError, DIPLO_ALLY, SETUP_SLOTS};

pub const LEADER_INIT_LOOP_BEGIN_VA: u32 = 0x006e_3bf9;
pub const LEADER_INIT_LOOP_END_VA: u32 = 0x006e_3d93;
pub const LEADER_STARTING_AGE_VA: u32 = 0x006d_7320;
pub const GAME_TEAMS_LOCKED_VA: u32 = 0x0059_4880;
pub const LEADER_IS_TEAM_VA: u32 = 0x006e_bd30;
pub const LEADER_IS_ALLY_VA: u32 = 0x006e_db50;
pub const LEADER_HAS_PREQ_VA: u32 = 0x006d_b810;
pub const SHARED_VISION_PREQ: i32 = 0x2b0;

pub const DIPLO_WAR: i32 = 0;
pub const DIPLO_PEACE: i32 = 1;

/// Exact `Game`/`GameInfo` facts read by the recovered loop and its two small callees.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderInitDiplomacyFacts {
    /// `Game+0x30` = `GameInfo+0x24` (`reveal_map`).
    pub reveal_map: u8,
    /// `Game+0x2A` = `GameInfo+0x1E` (`game_rules`).
    pub game_rules: u8,
    /// `Game+0x32` = `GameInfo+0x26` (`rush_rules`).
    pub rush_rules: u8,
    /// `Game+0x34` = `GameInfo+0x28`.
    pub starting_technology: u8,
    /// `Game+0x35` = `GameInfo+0x29`.
    pub starting_technology2: u8,
    /// `Game+0x36` = `GameInfo+0x2A`.
    pub ending_technology: u8,
    /// `Game+0x822 & 0x02`, semaphore bit 17.
    pub scenario_rules: bool,
    /// `Game+0x821 & 0x02`, semaphore bit 9.
    pub check_victory_mode: bool,
    /// Exact result of `LeaderData::has_preq(0x2B0)` for the receiver.
    pub has_shared_vision_preq: bool,
}

/// Non-diplomacy cells written by the same eight-target loop. Raw declarations remain in
/// [`SetupDiplomacy::leaders`], where the team/ally callees can observe them immediately.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeaderInitDiplomacyRow {
    pub treaties: [i32; SETUP_SLOTS],
    pub agendas: [i32; SETUP_SLOTS],
    pub good_deeds: [i32; SETUP_SLOTS],
    pub attack_stamp: [i32; SETUP_SLOTS],
    pub raid_stamp: [i32; SETUP_SLOTS],
    pub capital_stamp: [i32; SETUP_SLOTS],
    pub ally_stamp: [i32; SETUP_SLOTS],
    pub tribute_stamp: [i32; SETUP_SLOTS],
    pub gift_stamp: [i32; SETUP_SLOTS],
    pub hire_stamp: [i32; SETUP_SLOTS],
    pub hire_who: [i32; SETUP_SLOTS],
    pub aggression: [i32; SETUP_SLOTS],
    pub strong: [i32; SETUP_SLOTS],
    pub weak: [i32; SETUP_SLOTS],
    pub dow: [i32; SETUP_SLOTS],
    pub invaders: [i32; SETUP_SLOTS],
    pub broke_alliance: [i32; SETUP_SLOTS],
    pub made_peace: [i32; SETUP_SLOTS],
    pub got_diplo_message: i32,
    pub last_spoke: [i32; SETUP_SLOTS],
    pub counteroffer: [i32; SETUP_SLOTS],
    pub tribute_demanded: [i32; SETUP_SLOTS],
    pub last_taunt: [i32; SETUP_SLOTS],
    pub taunt_frame: [i32; SETUP_SLOTS],
    /// `LeaderData+0x6929`.
    pub ally_mask: u8,
}

impl LeaderInitDiplomacyRow {
    /// Emit the owned fields from `LeaderData+0x94..+0x394` in exact PDB order.
    /// `diplos` immediately precedes this range and remains owned by `LeaderState`.
    pub fn walk_prefix_bytes(&self, out: &mut Vec<u8>) {
        for values in [
            &self.treaties,
            &self.agendas,
            &self.good_deeds,
            &self.attack_stamp,
            &self.raid_stamp,
            &self.capital_stamp,
            &self.ally_stamp,
            &self.tribute_stamp,
            &self.gift_stamp,
            &self.hire_stamp,
            &self.hire_who,
            &self.aggression,
            &self.strong,
            &self.weak,
            &self.dow,
            &self.invaders,
            &self.broke_alliance,
            &self.made_peace,
        ] {
            for value in values {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
        out.extend_from_slice(&self.got_diplo_message.to_le_bytes());
        for values in [
            &self.last_spoke,
            &self.counteroffer,
            &self.tribute_demanded,
            &self.last_taunt,
            &self.taunt_frame,
        ] {
            for value in values {
                out.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
}

/// Complete state needed to preserve the loop's read-after-write ordering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeaderInitDiplomacyLoopImage {
    pub setup: SetupDiplomacy,
    pub row: LeaderInitDiplomacyRow,
}

/// `receiver_slot` identifies `this`; the function has already stored its `who` argument
/// into `setup.leaders[receiver_slot].who` before this loop begins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderInitDiplomacyLoopRequest {
    pub receiver_slot: usize,
    pub tribe: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartingAgePath {
    Primary,
    PrimaryForNonzeroTeam {
        team: i32,
    },
    TeamZeroCombined {
        primary: i32,
        secondary: i32,
        ending: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartingAgeReceipt {
    pub call_va: u32,
    pub path: StartingAgePath,
    pub value: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationDecision {
    NegativeTribe,
    ScenarioPreserved,
    Team,
    NonTeam {
        rush_rules: u8,
        starting_age: Option<StartingAgeReceipt>,
        teams_locked: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedVisionDecision {
    NegativeTribeSkipped,
    NotAllied,
    Prerequisite,
    RevealMap,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderInitDiplomacyTargetReceipt {
    pub target: usize,
    pub relation_before: i32,
    pub relation_decision: RelationDecision,
    /// First `is_team(target, 0)` result. Absent when scenario rules bypass that call or
    /// when the negative-tribe arm skips the ordinary body.
    pub initialization_team: Option<bool>,
    pub forced_war: bool,
    pub relation_after: i32,
    pub shared_vision: SharedVisionDecision,
    /// Second `is_team(target, 0)` result used to OR treaty bit zero.
    pub treaty_team: Option<bool>,
    pub treaty_after: i32,
    pub aggression_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderInitDiplomacyLoopReceipt {
    pub receiver_slot: usize,
    pub who: usize,
    pub targets: Vec<LeaderInitDiplomacyTargetReceipt>,
    pub ally_mask: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderInitDiplomacyLoopPlan {
    expected: LeaderInitDiplomacyLoopImage,
    next: LeaderInitDiplomacyLoopImage,
    receipt: LeaderInitDiplomacyLoopReceipt,
}

impl LeaderInitDiplomacyLoopPlan {
    pub fn next_state(&self) -> &LeaderInitDiplomacyLoopImage {
        &self.next
    }

    pub fn receipt(&self) -> &LeaderInitDiplomacyLoopReceipt {
        &self.receipt
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderInitDiplomacyLoopError {
    ReceiverSlotOutOfRange { slot: usize },
    ReceiverWhoOutOfRange { slot: usize, who: i32 },
    TeamQuery(TeamQueryError),
    StaleState,
}

impl From<TeamQueryError> for LeaderInitDiplomacyLoopError {
    fn from(value: TeamQueryError) -> Self {
        Self::TeamQuery(value)
    }
}

/// Exact `Game::teams_locked` body at `0x00594880`: team styles 0, 8, and 11 are
/// unlocked; every other byte is locked.
#[inline]
pub const fn teams_locked(team_style: u8) -> bool {
    !matches!(team_style, 0 | 8 | 11)
}

fn starting_age(
    setup: &SetupDiplomacy,
    receiver_slot: usize,
    facts: LeaderInitDiplomacyFacts,
) -> Result<StartingAgeReceipt, LeaderInitDiplomacyLoopError> {
    let primary = i32::from(facts.starting_technology.min(7));
    if facts.game_rules != 8 {
        return Ok(StartingAgeReceipt {
            call_va: LEADER_STARTING_AGE_VA,
            path: StartingAgePath::Primary,
            value: primary,
        });
    }

    let team = setup.team_of(receiver_slot)?;
    if team != 0 {
        return Ok(StartingAgeReceipt {
            call_va: LEADER_STARTING_AGE_VA,
            path: StartingAgePath::PrimaryForNonzeroTeam { team },
            value: primary,
        });
    }

    let secondary = i32::from(facts.starting_technology2.min(7));
    let ending = i32::from(facts.ending_technology);
    let value = primary.wrapping_add(secondary).min(ending);
    Ok(StartingAgeReceipt {
        call_va: LEADER_STARTING_AGE_VA,
        path: StartingAgePath::TeamZeroCombined {
            primary,
            secondary,
            ending,
        },
        value,
    })
}

fn reset_opening_cells(row: &mut LeaderInitDiplomacyRow, target: usize, treaty: i32) {
    row.agendas[target] = 0;
    row.good_deeds[target] = 0;
    row.attack_stamp[target] = 0;
    row.raid_stamp[target] = 0;
    row.capital_stamp[target] = 0;
    row.ally_stamp[target] = 0;
    row.tribute_stamp[target] = 0;
    row.gift_stamp[target] = 0;
    row.hire_stamp[target] = 0;
    row.hire_who[target] = -1;
    row.treaties[target] = treaty;
}

fn reset_closing_cells(row: &mut LeaderInitDiplomacyRow, target: usize, relation: i32) {
    row.aggression[target] = i32::from(relation == DIPLO_WAR);
    row.dow[target] = 0;
    row.weak[target] = 0;
    row.strong[target] = 0;
    row.invaders[target] = 0;
    row.broke_alliance[target] = 0;
    row.made_peace[target] = 0;
    row.got_diplo_message = 0;
    row.last_spoke[target] = 0;
    row.counteroffer[target] = 0;
    row.tribute_demanded[target] = 0;
    row.last_taunt[target] = 0;
    row.taunt_frame[target] = 0;
}

/// Plan the complete eight-target loop without publishing any mutation.
pub fn plan_leader_init_diplomacy_loop(
    before: &LeaderInitDiplomacyLoopImage,
    request: LeaderInitDiplomacyLoopRequest,
    facts: LeaderInitDiplomacyFacts,
) -> Result<LeaderInitDiplomacyLoopPlan, LeaderInitDiplomacyLoopError> {
    let receiver = before.setup.leaders.get(request.receiver_slot).ok_or(
        LeaderInitDiplomacyLoopError::ReceiverSlotOutOfRange {
            slot: request.receiver_slot,
        },
    )?;
    let who = usize::try_from(receiver.who)
        .ok()
        .filter(|who| *who < SETUP_SLOTS)
        .ok_or(LeaderInitDiplomacyLoopError::ReceiverWhoOutOfRange {
            slot: request.receiver_slot,
            who: receiver.who,
        })?;

    let mut next = before.clone();
    next.row.ally_mask = 1u8 << who;
    let mut targets = Vec::with_capacity(SETUP_SLOTS);

    for target in 0..SETUP_SLOTS {
        let base_treaty = i32::from(facts.reveal_map == 3);
        reset_opening_cells(&mut next.row, target, base_treaty);
        let relation_before = next.setup.leaders[request.receiver_slot].diplos[target];
        let mut initialization_team = None;
        let mut forced_war = false;
        let relation_decision;
        let shared_vision;
        let treaty_team;

        if request.tribe < 0 {
            let relation = if target == who { DIPLO_ALLY } else { DIPLO_WAR };
            next.setup.leaders[request.receiver_slot].diplos[target] = relation;
            relation_decision = RelationDecision::NegativeTribe;
            shared_vision = SharedVisionDecision::NegativeTribeSkipped;
            treaty_team = None;
        } else {
            if facts.scenario_rules {
                relation_decision = RelationDecision::ScenarioPreserved;
            } else {
                let is_team = next.setup.is_team(who, target, IsTeamArg::Zero)?;
                initialization_team = Some(is_team);
                if is_team {
                    next.setup.leaders[request.receiver_slot].diplos[target] = DIPLO_ALLY;
                    relation_decision = RelationDecision::Team;
                } else {
                    let starting_age = if facts.rush_rules == 0 {
                        None
                    } else {
                        Some(starting_age(&next.setup, request.receiver_slot, facts)?)
                    };
                    let locked = teams_locked(next.setup.team_style);
                    let before_lock_peace =
                        starting_age.is_some_and(|age| i32::from(facts.rush_rules) > age.value);
                    let relation = if before_lock_peace || !locked {
                        DIPLO_PEACE
                    } else {
                        DIPLO_WAR
                    };
                    next.setup.leaders[request.receiver_slot].diplos[target] = relation;
                    relation_decision = RelationDecision::NonTeam {
                        rush_rules: facts.rush_rules,
                        starting_age,
                        teams_locked: locked,
                    };
                }
            }

            if facts.check_victory_mode && target != who {
                next.setup.leaders[request.receiver_slot].diplos[target] = DIPLO_WAR;
                forced_war = true;
            }

            let allied = next.setup.is_ally(request.receiver_slot, target)?;
            shared_vision = if !allied {
                SharedVisionDecision::NotAllied
            } else if facts.has_shared_vision_preq {
                next.row.ally_mask |= 1u8 << target;
                SharedVisionDecision::Prerequisite
            } else if facts.reveal_map >= 1 {
                next.row.ally_mask |= 1u8 << target;
                SharedVisionDecision::RevealMap
            } else {
                SharedVisionDecision::Unavailable
            };

            let is_team = next.setup.is_team(who, target, IsTeamArg::Zero)?;
            if is_team {
                next.row.treaties[target] |= 1;
            }
            treaty_team = Some(is_team);
        }

        let relation_after = next.setup.leaders[request.receiver_slot].diplos[target];
        reset_closing_cells(&mut next.row, target, relation_after);
        targets.push(LeaderInitDiplomacyTargetReceipt {
            target,
            relation_before,
            relation_decision,
            initialization_team,
            forced_war,
            relation_after,
            shared_vision,
            treaty_team,
            treaty_after: next.row.treaties[target],
            aggression_after: next.row.aggression[target],
        });
    }

    let receipt = LeaderInitDiplomacyLoopReceipt {
        receiver_slot: request.receiver_slot,
        who,
        targets,
        ally_mask: next.row.ally_mask,
    };
    Ok(LeaderInitDiplomacyLoopPlan {
        expected: before.clone(),
        next,
        receipt,
    })
}

/// Apply only to the exact image from which the plan was derived.
pub fn apply_leader_init_diplomacy_loop(
    state: &mut LeaderInitDiplomacyLoopImage,
    plan: LeaderInitDiplomacyLoopPlan,
) -> Result<LeaderInitDiplomacyLoopReceipt, LeaderInitDiplomacyLoopError> {
    if *state != plan.expected {
        return Err(LeaderInitDiplomacyLoopError::StaleState);
    }
    *state = plan.next;
    Ok(plan.receipt)
}

/// Plan and atomically apply the complete recovered loop.
pub fn init_diplomacy_loop(
    state: &mut LeaderInitDiplomacyLoopImage,
    request: LeaderInitDiplomacyLoopRequest,
    facts: LeaderInitDiplomacyFacts,
) -> Result<LeaderInitDiplomacyLoopReceipt, LeaderInitDiplomacyLoopError> {
    let plan = plan_leader_init_diplomacy_loop(state, request, facts)?;
    apply_leader_init_diplomacy_loop(state, plan)
}
