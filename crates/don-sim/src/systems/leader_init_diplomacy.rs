// SPDX-License-Identifier: GPL-3.0-or-later
//! Option-independent diplomacy prefix from `Leader::init`.
//!
//! Retail `Leader::init(int who, int slot, int local)` is 6,102 bytes at
//! `0x006E3930`. Its eight-target initialization loop calls
//! `LeaderData::is_team(target, 0)` at `0x006E3C65..0x006E3C6D`; a true result selects
//! diplomacy value 2 at `0x006E3C76..0x006E3C79` and stores it in
//! `LeaderData::diplos[target]` at `0x006E3CB4`.
//!
//! The false arm depends on `Game+0x32` (`GameInfo+0x26`, `rush_rules`),
//! `LeaderData::starting_age`, and `Game::teams_locked`, while the remainder of
//! `Leader::init` owns substantially more state. This module therefore admits only active
//! player pairs for which the recovered frame-zero team query is true. The complete
//! detached loop is reconstructed in `leader_init_diplomacy_loop`; product integration
//! here never changes a non-team declaration or claims the rest of `Leader::init`.

use super::setup_diplomacy::{IsTeamArg, SetupDiplomacy, TeamQueryError, DIPLO_ALLY, SETUP_SLOTS};

/// Exact active-target alliance writes admitted from the option-independent branch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderInitTeamAllianceReceipt {
    /// Bit `target` is set when the corresponding active row was initialized to ally.
    pub ally_masks: [u8; SETUP_SLOTS],
    /// Number of directional declaration cells written, including active self cells.
    pub writes: usize,
}

/// Whole-state-bound plan. The expected image prevents setup/team changes from racing the
/// diplomacy initialization prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderInitTeamAlliancePlan {
    expected: SetupDiplomacy,
    next: SetupDiplomacy,
    receipt: LeaderInitTeamAllianceReceipt,
}

impl LeaderInitTeamAlliancePlan {
    pub fn next_state(&self) -> &SetupDiplomacy {
        &self.next
    }

    pub fn receipt(&self) -> LeaderInitTeamAllianceReceipt {
        self.receipt
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaderInitTeamAllianceError {
    SetupFrameIsNotZero { frame: i32 },
    ActiveLeaderMissing { slot: usize },
    TeamQuery(TeamQueryError),
    StaleState,
}

impl From<TeamQueryError> for LeaderInitTeamAllianceError {
    fn from(value: TeamQueryError) -> Self {
        Self::TeamQuery(value)
    }
}

/// Plan the exact, option-independent `is_team(target, 0) -> diplos[target] = 2` branch
/// for the active setup cohort.
pub fn plan_active_team_alliances(
    before: &SetupDiplomacy,
    active_mask: u8,
) -> Result<LeaderInitTeamAlliancePlan, LeaderInitTeamAllianceError> {
    if before.frame != 0 {
        return Err(LeaderInitTeamAllianceError::SetupFrameIsNotZero {
            frame: before.frame,
        });
    }
    for slot in 0..SETUP_SLOTS {
        if active_mask & (1u8 << slot) != 0 && !before.leaders[slot].is_present() {
            return Err(LeaderInitTeamAllianceError::ActiveLeaderMissing { slot });
        }
    }

    let mut next = before.clone();
    let mut receipt = LeaderInitTeamAllianceReceipt::default();
    for actor in 0..SETUP_SLOTS {
        if active_mask & (1u8 << actor) == 0 {
            continue;
        }
        for target in 0..SETUP_SLOTS {
            if active_mask & (1u8 << target) == 0
                || !before.is_team(actor, target, IsTeamArg::Zero)?
            {
                continue;
            }
            next.leaders[actor].diplos[target] = DIPLO_ALLY;
            receipt.ally_masks[actor] |= 1u8 << target;
            receipt.writes += 1;
        }
    }
    Ok(LeaderInitTeamAlliancePlan {
        expected: before.clone(),
        next,
        receipt,
    })
}

/// Apply a plan only to the exact setup image from which it was derived.
pub fn apply_active_team_alliances(
    state: &mut SetupDiplomacy,
    plan: LeaderInitTeamAlliancePlan,
) -> Result<LeaderInitTeamAllianceReceipt, LeaderInitTeamAllianceError> {
    if *state != plan.expected {
        return Err(LeaderInitTeamAllianceError::StaleState);
    }
    *state = plan.next;
    Ok(plan.receipt)
}

/// Plan and atomically apply the bounded initialization prefix.
pub fn init_active_team_alliances(
    state: &mut SetupDiplomacy,
    active_mask: u8,
) -> Result<LeaderInitTeamAllianceReceipt, LeaderInitTeamAllianceError> {
    let plan = plan_active_team_alliances(state, active_mask)?;
    apply_active_team_alliances(state, plan)
}
