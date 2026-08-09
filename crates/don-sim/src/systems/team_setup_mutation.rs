//! Atomic deterministic mutation seam for the retail `Game::init_teams` setup pass.
//!
//! The read-only team predicates live in [`super::setup_diplomacy`].  This module owns the
//! smallest instruction-derived mutation path which can consume those predicates without
//! inventing a second lobby/team model: the non-ranked, non-random path through
//! `Game::init_teams` (`0x0058AE70`, 4,909 bytes).  The admitted path updates the setup team
//! bytes, victory team counts, team semaphore, and live leaders' diplomacy-chat gates as one
//! stale-state-checked plan.  Script callbacks remain an explicit ordered product boundary.

#![allow(dead_code)]

use super::setup_diplomacy::{
    IsTeamArg, SetupDiplomacy, TeamQueryError, PLAYER_PRESENT, SETUP_SLOTS,
};

/// `Game::init_teams` treats signed team byte 5 as the unresolved/random-team sentinel.
/// This is distinct from the value 8 special-cased by `LeaderData::get_team`.
pub const RANDOM_TEAM: i8 = 5;

/// First callback-table offset used to publish a player's current setup team.
pub const SCRIPT_PLAYER_TEAM: u32 = 0xD8E0;
/// Callback-table offset used to publish each of the eight `Game::on_team` cells.
pub const SCRIPT_TEAM_OCCUPANCY: u32 = 0xD8F4;
/// Callback-table offset used to publish one live leader's team-member count.
pub const SCRIPT_TEAM_MEMBER_COUNT: u32 = 0xD930;
/// Callback-table offset emitted once when the first multi-player team is found.
pub const SCRIPT_TEAM_MODE_ENABLED: u32 = 0xD944;
/// Callback-table offset emitted after seeding one `LeaderData::chat_status` cell to 1.
pub const SCRIPT_CHAT_STATUS_SEEDED: u32 = 0xD958;
/// Callback-table offset emitted before clearing one admitted chat-status cell to 0.
pub const SCRIPT_CHAT_STATUS_CLEARED: u32 = 0xD96C;

/// The checksum-visible state owned by the deterministic setup/team mutation seam.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamSetupState {
    /// Canonical setup players, leaders, team-style, frame, and semaphore byte.
    pub setup: SetupDiplomacy,
    /// `Game::on_team[8]` at `Game+0x5E0`.
    pub on_team: [i32; SETUP_SLOTS],
    /// `Game::num_teams` at `Game+0x6A4`.
    pub num_teams: i32,
    /// `Game::num_sides` at `Game+0x6A8`.
    pub num_sides: i32,
    /// `Game::semaphore.flags` at `Game+0x81C`.
    pub semaphore_flags: i32,
    /// `LeaderData::chat_status[8]` at leader offset `+0x54`.
    pub chat_status: [[i32; SETUP_SLOTS]; SETUP_SLOTS],
}

impl Default for TeamSetupState {
    fn default() -> Self {
        Self {
            setup: SetupDiplomacy::default(),
            on_team: [0; SETUP_SLOTS],
            num_teams: 0,
            num_sides: 0,
            semaphore_flags: 0,
            chat_status: [[0; SETUP_SLOTS]; SETUP_SLOTS],
        }
    }
}

/// Facts read outside the state image by the admitted retail path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InitTeamsRequest {
    /// `Net+0x2A0`, compared with the resolved `Player` slot in team style 3.
    pub local_player_setup_slot: usize,
    /// `GameInfo::is_ranked`.  The ranked ELO/balancing tail is deliberately rejected.
    pub ranked: bool,
}

/// Phase of the retail player-team callback.  Retail publishes the setup bytes once before
/// any deterministic forced-team write and once after the assignment section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerTeamPhase {
    BeforeAssignment,
    AfterAssignment,
}

/// Ordered script callback facts produced by the admitted `Game::init_teams` path.
///
/// These are plans, not claims that the BHS/script host ran.  Keeping the callback-table
/// offsets in the variants makes the remaining integration boundary mechanically auditable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitTeamsScriptCall {
    PlayerTeam {
        table_offset: u32,
        phase: PlayerTeamPhase,
        player_slot: usize,
        team: i32,
    },
    TeamOccupancy {
        table_offset: u32,
        team_slot: usize,
        count: i32,
    },
    TeamMemberCount {
        table_offset: u32,
        leader_slot: usize,
        count: i32,
    },
    TeamModeEnabled {
        table_offset: u32,
    },
    ChatStatusSeeded {
        table_offset: u32,
        leader_slot: usize,
        target_slot: usize,
    },
    ChatStatusCleared {
        table_offset: u32,
        leader_slot: usize,
        target_slot: usize,
    },
}

/// Observable result of one fully validated deterministic setup mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitTeamsReceipt {
    pub on_team: [i32; SETUP_SLOTS],
    pub num_teams: i32,
    pub num_sides: i32,
    pub team_mode_enabled: bool,
    pub script_calls: Vec<InitTeamsScriptCall>,
}

/// A plan binds every mutation to the exact state image from which it was derived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitTeamsPlan {
    expected: TeamSetupState,
    next: TeamSetupState,
    receipt: InitTeamsReceipt,
}

impl InitTeamsPlan {
    pub fn receipt(&self) -> &InitTeamsReceipt {
        &self.receipt
    }

    pub fn next_state(&self) -> &TeamSetupState {
        &self.next
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitTeamsError {
    RankedPathUnsupported,
    SetupFrameIsNotZero { frame: i32 },
    LocalPlayerSlotOutOfRange { slot: usize },
    LeaderIdentityMismatch { slot: usize, who: i32 },
    AbsentPlayerSetup { leader_slot: usize, who: i32 },
    UnsafeSignedTeam { player_slot: usize, team: i8 },
    UnresolvedRandomTeam { player_slot: usize },
    CooperativeWhoOutOfRange { player_slot: usize, who: u8 },
    NoLiveLeaders,
    TeamQuery(TeamQueryError),
    StaleState,
}

impl From<TeamQueryError> for InitTeamsError {
    fn from(value: TeamQueryError) -> Self {
        Self::TeamQuery(value)
    }
}

/// Reconstruct `Game::is_cooperative` (`0x00594810`) for the fields consumed by the chat
/// setup tail.  Styles 8..=10 are always cooperative.  Styles 11 and 12 are cooperative only
/// when two present `Player` records name the same in-range `who` byte.
fn is_cooperative(setup: &SetupDiplomacy) -> Result<bool, InitTeamsError> {
    if matches!(setup.team_style, 8..=10) {
        return Ok(true);
    }
    if !matches!(setup.team_style, 11 | 12) {
        return Ok(false);
    }

    let mut seen = [false; SETUP_SLOTS];
    for (player_slot, player) in setup.players.iter().copied().enumerate() {
        if player.flags & PLAYER_PRESENT == 0 {
            continue;
        }
        let who = usize::from(player.who);
        if who >= SETUP_SLOTS {
            return Err(InitTeamsError::CooperativeWhoOutOfRange {
                player_slot,
                who: player.who,
            });
        }
        if seen[who] {
            return Ok(true);
        }
        seen[who] = true;
    }
    Ok(false)
}

/// Validate the retail identity invariants and resolve each live leader to its exact setup
/// player.  Retail falls back to setup slot zero when no match exists; the executable product
/// seam deliberately fails closed instead of mutating from that absent `PlayerSetup`.
fn resolve_live_players(
    setup: &SetupDiplomacy,
) -> Result<([Option<usize>; SETUP_SLOTS], usize), InitTeamsError> {
    let mut resolved = [None; SETUP_SLOTS];
    let mut live = 0usize;
    for (leader_slot, leader) in setup.leaders.iter().enumerate() {
        if !leader.is_present() {
            continue;
        }
        live += 1;
        if leader.who != leader_slot as i32 {
            return Err(InitTeamsError::LeaderIdentityMismatch {
                slot: leader_slot,
                who: leader.who,
            });
        }
        let player_slot = setup.player_index_for_leader(leader_slot)?;
        let player = setup.players[player_slot];
        if !player.is_present() || i32::from(player.who) != leader.who {
            return Err(InitTeamsError::AbsentPlayerSetup {
                leader_slot,
                who: leader.who,
            });
        }
        resolved[leader_slot] = Some(player_slot);
    }
    if live == 0 {
        return Err(InitTeamsError::NoLiveLeaders);
    }
    Ok((resolved, live))
}

fn push_player_team_calls(
    calls: &mut Vec<InitTeamsScriptCall>,
    setup: &SetupDiplomacy,
    phase: PlayerTeamPhase,
) {
    for (player_slot, player) in setup.players.iter().copied().enumerate() {
        if player.is_present() {
            calls.push(InitTeamsScriptCall::PlayerTeam {
                table_offset: SCRIPT_PLAYER_TEAM,
                phase,
                player_slot,
                team: i32::from(player.team),
            });
        }
    }
}

/// Plan the safe deterministic path through `Game::init_teams` without mutating `before`.
///
/// Random-team assignment (team byte 5), ranked ELO balancing, nonzero-frame diplomacy, and
/// malformed signed team bytes are explicit red gates.  Every fallible read happens before an
/// [`InitTeamsPlan`] can be applied.
pub fn plan_init_teams(
    before: &TeamSetupState,
    request: InitTeamsRequest,
) -> Result<InitTeamsPlan, InitTeamsError> {
    if request.ranked {
        return Err(InitTeamsError::RankedPathUnsupported);
    }
    if before.setup.frame != 0 {
        return Err(InitTeamsError::SetupFrameIsNotZero {
            frame: before.setup.frame,
        });
    }

    let (resolved, _) = resolve_live_players(&before.setup)?;
    let mut next = before.clone();
    let mut script_calls = Vec::new();
    push_player_team_calls(
        &mut script_calls,
        &next.setup,
        PlayerTeamPhase::BeforeAssignment,
    );

    // 0x0058B020..0x0058B047: this setup style forces the local setup player to team 0 and
    // every other live leader's resolved setup player to team 1 unless semaphore bit 2 is set.
    if next.setup.team_style == 3 && next.setup.semaphore_820 & 0x04 == 0 {
        if request.local_player_setup_slot >= SETUP_SLOTS {
            return Err(InitTeamsError::LocalPlayerSlotOutOfRange {
                slot: request.local_player_setup_slot,
            });
        }
        for player_slot in resolved.iter().copied().flatten() {
            let is_remote = player_slot != request.local_player_setup_slot;
            next.setup.players[player_slot].team = if is_remote { 1 } else { 0 };
        }
    }

    let mut on_team = [0i32; SETUP_SLOTS];
    let mut non_team_sides = 0i32;
    for player_slot in resolved.iter().copied().flatten() {
        let team = next.setup.players[player_slot].team;
        if team == RANDOM_TEAM {
            return Err(InitTeamsError::UnresolvedRandomTeam { player_slot });
        }
        if team < 0 {
            return Err(InitTeamsError::UnsafeSignedTeam { player_slot, team });
        }
        if team < 4 {
            on_team[team as usize] += 1;
        } else {
            non_team_sides += 1;
        }
    }

    next.on_team = on_team;
    next.num_teams = on_team[..4].iter().filter(|&&count| count != 0).count() as i32;
    next.num_sides = next.num_teams + non_team_sides;

    for (team_slot, count) in on_team.iter().copied().enumerate() {
        script_calls.push(InitTeamsScriptCall::TeamOccupancy {
            table_offset: SCRIPT_TEAM_OCCUPANCY,
            team_slot,
            count,
        });
    }
    push_player_team_calls(
        &mut script_calls,
        &next.setup,
        PlayerTeamPhase::AfterAssignment,
    );

    // Retail clears the team bit before proving it again from the live leader roster.
    next.setup.semaphore_820 &= 0x7F;
    if next.semaphore_flags == 0 {
        next.semaphore_flags = 2;
    }

    let mut team_mode_enabled = false;
    for (leader_slot, leader) in next.setup.leaders.iter().enumerate() {
        if !leader.is_present() {
            continue;
        }
        let mut members = 0i32;
        for (target_slot, target) in next.setup.leaders.iter().enumerate() {
            if target_slot == leader_slot {
                members += 1;
            } else if target.is_present()
                && next
                    .setup
                    .is_team(leader_slot, target_slot, IsTeamArg::Zero)?
            {
                members += 1;
            }
        }
        script_calls.push(InitTeamsScriptCall::TeamMemberCount {
            table_offset: SCRIPT_TEAM_MEMBER_COUNT,
            leader_slot,
            count: members,
        });
        if members > 1 {
            team_mode_enabled = true;
            next.setup.semaphore_820 |= 0x80;
            next.semaphore_flags = 0;
            script_calls.push(InitTeamsScriptCall::TeamModeEnabled {
                table_offset: SCRIPT_TEAM_MODE_ENABLED,
            });
            break;
        }
    }

    let cooperative = is_cooperative(&next.setup)?;
    for leader_slot in 0..SETUP_SLOTS {
        if !next.setup.leaders[leader_slot].is_present() {
            continue;
        }
        for target_slot in 0..SETUP_SLOTS {
            next.chat_status[leader_slot][target_slot] = 1;
            script_calls.push(InitTeamsScriptCall::ChatStatusSeeded {
                table_offset: SCRIPT_CHAT_STATUS_SEEDED,
                leader_slot,
                target_slot,
            });

            if !next.setup.leaders[target_slot].is_present() {
                continue;
            }
            let clear = (!team_mode_enabled && !cooperative)
                || next
                    .setup
                    .is_team(leader_slot, target_slot, IsTeamArg::Zero)?;
            if clear {
                script_calls.push(InitTeamsScriptCall::ChatStatusCleared {
                    table_offset: SCRIPT_CHAT_STATUS_CLEARED,
                    leader_slot,
                    target_slot,
                });
                next.chat_status[leader_slot][target_slot] = 0;
            }
        }
    }

    let receipt = InitTeamsReceipt {
        on_team,
        num_teams: next.num_teams,
        num_sides: next.num_sides,
        team_mode_enabled,
        script_calls,
    };
    Ok(InitTeamsPlan {
        expected: before.clone(),
        next,
        receipt,
    })
}

/// Apply a previously validated plan only to the exact state image it was derived from.
pub fn apply_init_teams_plan(
    state: &mut TeamSetupState,
    plan: InitTeamsPlan,
) -> Result<InitTeamsReceipt, InitTeamsError> {
    if *state != plan.expected {
        return Err(InitTeamsError::StaleState);
    }
    *state = plan.next;
    Ok(plan.receipt)
}

/// Convenience atomic entry point.  Planning occurs against a clone, so every validation or
/// query failure leaves `state` byte-for-byte unchanged.
pub fn init_teams_atomic(
    state: &mut TeamSetupState,
    request: InitTeamsRequest,
) -> Result<InitTeamsReceipt, InitTeamsError> {
    let plan = plan_init_teams(state, request)?;
    apply_init_teams_plan(state, plan)
}
