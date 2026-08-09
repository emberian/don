//! Canonical frame-zero player/team setup owner for executable Sim adapters.
//!
//! [`super::team_setup_mutation`] recovers the deterministic, non-ranked body of
//! `Game::init_teams`, but deliberately stops at a representation-neutral plan.  This
//! module is the narrow product owner: it materializes exact [`PlayerSetup`] records,
//! applies that plan once at frame zero, synchronizes the victory fields already owned by
//! [`Sim`], and only then activates the requested leader cohort.  No adapter-side roster or
//! team table survives the call.

use super::setup_diplomacy::{
    LeaderTeamState, PlayerSetup, PLAYER_PRESENT, SETUP_SLOTS, TEAM_AUTO,
};
use super::team_setup_mutation::{
    init_teams_atomic, InitTeamsError, InitTeamsReceipt, InitTeamsRequest, TeamSetupState,
    RANDOM_TEAM,
};
use super::victory_score::game_sem;
use crate::tick::Sim;

/// The shipped `gamestyles` category has recovered uses for indices 0 through 12.
pub const MAX_TEAM_STYLE: u8 = 12;

/// One complete manual setup request.  Inactive slots must retain [`TEAM_AUTO`]; this
/// prevents an adapter from smuggling a second, latent roster into the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualPlayerSetup {
    pub active_mask: u8,
    pub teams: [i8; SETUP_SLOTS],
    pub team_style: u8,
    pub local_player_setup_slot: usize,
    pub ranked: bool,
}

impl Default for ManualPlayerSetup {
    fn default() -> Self {
        Self {
            active_mask: 0,
            teams: [TEAM_AUTO; SETUP_SLOTS],
            team_style: 0,
            local_player_setup_slot: 0,
            ranked: false,
        }
    }
}

/// Canonical, Sim-owned result retained after setup.  The ordered script calls remain in
/// the receipt instead of being silently presented as executed by headless/browser hosts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedPlayerSetup {
    pub active_mask: u8,
    pub state: TeamSetupState,
    pub receipt: InitTeamsReceipt,
}

/// Persistent setup owner embedded in the authoritative victory leader table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerSetupOwner {
    applied: Option<AppliedPlayerSetup>,
}

impl PlayerSetupOwner {
    #[inline]
    pub fn applied(&self) -> Option<&AppliedPlayerSetup> {
        self.applied.as_ref()
    }

    #[inline]
    pub fn configured_mask(&self) -> u8 {
        self.applied.as_ref().map_or(0, |setup| setup.active_mask)
    }

    #[inline]
    pub fn is_configured(&self, who: usize) -> bool {
        who < SETUP_SLOTS && self.configured_mask() & (1u8 << who) != 0
    }

    /// Read the installed retail setup query.  An unconfigured slot retains the victory
    /// lane's historical own-slot fallback; malformed installed state is impossible because
    /// installation is private and validation precedes the owner swap.
    pub fn team_of(&self, who: usize) -> i32 {
        self.applied
            .as_ref()
            .and_then(|setup| setup.state.setup.team_of(who).ok())
            .unwrap_or(who as i32)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManualPlayerSetupError {
    WorldFrameIsNotZero { frame: i32 },
    VictoryFrameMismatch { world: i32, victory: i32 },
    MatchAlreadyStarted,
    TeamStyleOutOfRange { team_style: u8 },
    LocalPlayerInactive { slot: usize },
    InactiveSlotHasTeam { slot: usize, team: i8 },
    UnsupportedManualTeam { slot: usize, team: i8 },
    InitTeams(InitTeamsError),
}

impl From<InitTeamsError> for ManualPlayerSetupError {
    fn from(value: InitTeamsError) -> Self {
        Self::InitTeams(value)
    }
}

fn plan_manual_setup(
    sim: &Sim,
    request: ManualPlayerSetup,
) -> Result<AppliedPlayerSetup, ManualPlayerSetupError> {
    if sim.world.frame != 0 {
        return Err(ManualPlayerSetupError::WorldFrameIsNotZero {
            frame: sim.world.frame,
        });
    }
    if sim.vic_match.frame != sim.world.frame {
        return Err(ManualPlayerSetupError::VictoryFrameMismatch {
            world: sim.world.frame,
            victory: sim.vic_match.frame,
        });
    }
    if sim.vic_leaders.setup_owner.applied().is_some()
        || sim
            .vic_leaders
            .slots
            .iter()
            .any(|leader| leader.is_active())
    {
        return Err(ManualPlayerSetupError::MatchAlreadyStarted);
    }
    if request.team_style > MAX_TEAM_STYLE {
        return Err(ManualPlayerSetupError::TeamStyleOutOfRange {
            team_style: request.team_style,
        });
    }
    if request.local_player_setup_slot >= SETUP_SLOTS
        || request.active_mask & (1u8 << request.local_player_setup_slot) == 0
    {
        return Err(ManualPlayerSetupError::LocalPlayerInactive {
            slot: request.local_player_setup_slot,
        });
    }

    let mut state = TeamSetupState::default();
    state.setup.team_style = request.team_style;
    state.setup.frame = sim.world.frame;
    if sim.vic_match.sem(game_sem::NET_OR_RECORDING) {
        state.setup.semaphore_820 |= 0x04;
    }

    for slot in 0..SETUP_SLOTS {
        let active = request.active_mask & (1u8 << slot) != 0;
        let team = request.teams[slot];
        // `Player::team` exists independently of `Player::flags & 1`. Preserve the complete
        // validated setup image even though retail queries ignore this byte while absent.
        state.setup.players[slot] = PlayerSetup {
            flags: 0,
            who: slot as u8,
            team,
        };
        if !active {
            if team != TEAM_AUTO {
                return Err(ManualPlayerSetupError::InactiveSlotHasTeam { slot, team });
            }
            continue;
        }
        // The random sentinel is rejected even when team style 3 would overwrite it.
        // Product callers must never rely on an omitted RNG arm being unreachable by luck.
        if team == RANDOM_TEAM || !matches!(team, 0..=3 | TEAM_AUTO) {
            return Err(ManualPlayerSetupError::UnsupportedManualTeam { slot, team });
        }
        state.setup.players[slot].flags = PLAYER_PRESENT;
        state.setup.leaders[slot] = LeaderTeamState {
            leader_flags: 1,
            who: slot as i32,
            diplos: sim.vic_leaders.slots[slot].diplos,
        };
    }

    let receipt = init_teams_atomic(
        &mut state,
        InitTeamsRequest {
            local_player_setup_slot: request.local_player_setup_slot,
            ranked: request.ranked,
        },
    )?;
    Ok(AppliedPlayerSetup {
        active_mask: request.active_mask,
        state,
        receipt,
    })
}

impl Sim {
    /// Atomically configure the exact setup players, apply the deterministic team body,
    /// synchronize victory aggregation, and activate the cohort.  All fallible work is
    /// planned first; an error leaves every Sim channel unchanged.
    pub fn start_manual_player_setup(
        &mut self,
        request: ManualPlayerSetup,
    ) -> Result<&AppliedPlayerSetup, ManualPlayerSetupError> {
        let applied = plan_manual_setup(self, request)?;

        self.vic_match.options.team_style = applied.state.setup.team_style;
        self.vic_match.on_team = applied.state.on_team;
        self.vic_match.num_sides = applied.state.num_sides;
        if applied.receipt.team_mode_enabled {
            self.vic_match.set_sem(game_sem::TEAM_SCORING);
        } else {
            self.vic_match.clear_sem(game_sem::TEAM_SCORING);
        }
        self.vic_leaders.setup_owner.applied = Some(applied);

        for who in 0..SETUP_SLOTS {
            if request.active_mask & (1u8 << who) != 0 {
                self.activate(who);
            }
        }

        Ok(self.vic_leaders.setup_owner.applied().unwrap())
    }
}
