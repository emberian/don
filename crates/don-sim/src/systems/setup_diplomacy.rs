// SPDX-License-Identifier: GPL-3.0-or-later
//! Retail setup-team lookup and the runtime/static team predicate.
//!
//! This module is deliberately detached from [`crate::world`] and the tick.  The replay
//! prefix, Arena, browser setup, and the authoritative leader table all hold different
//! concrete representations today; [`SetupDiplomacy`] is the small typed image they can
//! materialize without copying the retail decision tree into four adapters.
//!
//! # Provenance and boundary
//!
//! Tier C, instruction-derived; the shipped function has not been called by an oracle.
//! The supported executable is PE32 SHA-256
//! `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
//!
//! * `LeaderData::get_player` `0x006EC0F0..0x006EC12B` supplies
//!   [`SetupDiplomacy::player_index_for_leader`].
//! * `LeaderData::get_team` `0x006EC040..0x006EC0EB` supplies
//!   [`SetupDiplomacy::team_of`].
//! * `LeaderData::is_team` `0x006EBD30..0x006EBE3F` supplies
//!   [`SetupDiplomacy::is_team`].
//! * `LeaderData::is_ally` `0x006EDB50` supplies the mutual-alliance leaf reached by
//!   `is_team` after frame zero.
//!
//! It does **not** assign random teams, initialize diplomacy declarations, execute a
//! diplomacy command, or reproduce `Leader::set_diplo`'s retarget/vision/event tail.

/// Retail has eight `Player` setup records and eight `Leader` slots.
pub const SETUP_SLOTS: usize = 8;

/// `Player::flags & 1` is the presence gate used by all three recovered functions.
pub const PLAYER_PRESENT: u16 = 0x01;

/// A matching `Player` with either of these bits is retained as a fallback but does not
/// stop `LeaderData::get_player`'s scan.  The PDB does not give this combined mask a name,
/// so this constant names only the measured lookup behavior.
pub const PLAYER_LOOKUP_DEFER: u16 = 0x50;

/// Raw `Player::team` sentinel recognized by `LeaderData::get_team` and the team-style-7
/// special case in `LeaderData::is_team`.
pub const TEAM_AUTO: i8 = 8;

/// Only setup team bytes 0 through 3 form a configured team in `is_team`.
pub const CONFIGURED_TEAMS: std::ops::Range<i8> = 0..4;

/// The `GameInfo::team_style` branch which rejects distinct auto-team players.
pub const TEAM_STYLE_NEUTRAL: u8 = 7;

/// Team styles whose nonzero `is_team` argument still requires a runtime alliance once
/// `Game::frame != 0`.
pub const ALLIANCE_GATED_TEAM_STYLES: [u8; 3] = [0, 8, 11];

/// `DiploButtonCats::DIPLO_ALLY`, compared directly by `LeaderData::is_ally`.
pub const DIPLO_ALLY: i32 = 2;

/// The setup fields read from one PDB `Player` record (stride `0x8C`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerSetup {
    /// `Player+0x30`, read as a `u16`.
    pub flags: u16,
    /// `Player+0x33`.
    pub who: u8,
    /// `Player+0x34`, read and returned with signed-byte semantics.
    pub team: i8,
}

impl PlayerSetup {
    #[inline]
    pub const fn is_present(self) -> bool {
        self.flags & PLAYER_PRESENT != 0
    }

    #[inline]
    pub const fn is_immediate_lookup_match(self) -> bool {
        self.flags & PLAYER_LOOKUP_DEFER == 0
    }
}

/// The `LeaderData` fields read by the recovered team query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderTeamState {
    /// `LeaderData+0x00`; `get_team` scans candidates with bit zero set.
    pub leader_flags: i32,
    /// `LeaderData+0x08`.  Retail compares this dword with `Player::who`.
    pub who: i32,
    /// Directional `LeaderData::diplos[8]` at `+0x74`.
    pub diplos: [i32; SETUP_SLOTS],
}

impl Default for LeaderTeamState {
    fn default() -> Self {
        Self {
            leader_flags: 0,
            who: -1,
            diplos: [0; SETUP_SLOTS],
        }
    }
}

impl LeaderTeamState {
    #[inline]
    pub const fn is_present(self) -> bool {
        self.leader_flags & 1 != 0
    }
}

/// The raw second argument to `LeaderData::is_team(int, int)`.
///
/// No name for that argument survives in the PDB.  Keeping the zero/nonzero distinction
/// explicit avoids assigning invented semantics to it.  At frame zero both modes use the
/// configured setup team; after frame zero `Zero` always uses mutual diplomacy, while
/// `NonZero` retains the configured team except for team styles 0, 8, and 11.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IsTeamArg {
    #[default]
    Zero,
    NonZero,
}

/// A bounded, representation-neutral image for the three recovered retail queries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupDiplomacy {
    pub players: [PlayerSetup; SETUP_SLOTS],
    pub leaders: [LeaderTeamState; SETUP_SLOTS],
    /// `GameInfo::team_style` (`Game+0x24`).
    pub team_style: u8,
    /// `Game::frame` (`Game+0x550`).
    pub frame: i32,
    /// The first byte of `Game::semaphore`'s bit array (`Game+0x820`).  `get_team`
    /// tests it as signed and therefore branches only on bit `0x80`.
    pub semaphore_820: u8,
}

impl Default for SetupDiplomacy {
    fn default() -> Self {
        Self {
            players: [PlayerSetup::default(); SETUP_SLOTS],
            leaders: [LeaderTeamState::default(); SETUP_SLOTS],
            team_style: 0,
            frame: 0,
            semaphore_820: 0,
        }
    }
}

/// The retail functions assume a valid leader index.  Product adapters fail closed at
/// that boundary rather than indexing a different leader after integer wrapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamQueryError {
    LeaderSlotOutOfRange { slot: usize },
    LeaderWhoOutOfRange { slot: usize, who: i32 },
}

impl SetupDiplomacy {
    /// `LeaderData::get_player` `0x006EC0F0`.
    ///
    /// The scan is subtler than `find`: a present record with matching `who` and neither
    /// `0x50` flag stops immediately.  A matching deferred record replaces the fallback
    /// and the scan continues.  If nothing stops the scan, the most recent deferred match
    /// is returned, or slot zero when there was no match at all.
    pub fn player_index_for_leader(&self, leader_slot: usize) -> Result<usize, TeamQueryError> {
        let leader = self.leader(leader_slot)?;
        let mut fallback = 0usize;
        for (slot, player) in self.players.iter().copied().enumerate() {
            if !player.is_present() || i32::from(player.who) != leader.who {
                continue;
            }
            if player.is_immediate_lookup_match() {
                return Ok(slot);
            }
            fallback = slot;
        }
        Ok(fallback)
    }

    /// `LeaderData::get_team` `0x006EC040`.
    ///
    /// Ordinary configured bytes are returned with `i8` sign extension.  Team byte 8 is
    /// special only while `Game+0x820 & 0x80 == 0`: retail scans present leader slots and
    /// returns the first slot for which `is_team(candidate, 0)` succeeds.  With the high
    /// semaphore bit set, or with no matching candidate, the raw sentinel 8 survives.
    pub fn team_of(&self, leader_slot: usize) -> Result<i32, TeamQueryError> {
        self.leader(leader_slot)?;
        let player_slot = self.player_index_for_leader(leader_slot)?;
        let player = self.players[player_slot];
        if !player.is_present() {
            return Ok(i32::from(TEAM_AUTO));
        }

        if player.team != TEAM_AUTO || self.semaphore_820 & 0x80 != 0 {
            return Ok(i32::from(player.team));
        }

        for candidate in 0..SETUP_SLOTS {
            if self.leaders[candidate].is_present()
                && self.is_team(leader_slot, candidate, IsTeamArg::Zero)?
            {
                return Ok(candidate as i32);
            }
        }
        Ok(i32::from(TEAM_AUTO))
    }

    /// `LeaderData::is_team(int target, int mode)` `0x006EBD30`.
    pub fn is_team(
        &self,
        leader_slot: usize,
        target: usize,
        arg: IsTeamArg,
    ) -> Result<bool, TeamQueryError> {
        let leader = self.leader(leader_slot)?;
        self.leader(target)?;

        // 0x006EBD39: compares target with LeaderData::who, not the receiver's slot.
        if target as i32 == leader.who {
            return Ok(true);
        }

        // 0x006EBD53..0x006EBDA0: in team style 7, a present auto-team setup record on
        // either side makes two distinct leaders non-teammates before any frame branch.
        if self.team_style == TEAM_STYLE_NEUTRAL {
            let mine = self.players[self.player_index_for_leader(leader_slot)?];
            if mine.is_present() && mine.team == TEAM_AUTO {
                return Ok(false);
            }
            let theirs = self.players[self.player_index_for_leader(target)?];
            if theirs.is_present() && theirs.team == TEAM_AUTO {
                return Ok(false);
            }
        }

        // 0x006EBDA6..0x006EBDC4.
        if self.frame != 0 && arg == IsTeamArg::Zero {
            return self.is_ally(leader_slot, target);
        }

        // 0x006EBDC7..0x006EBDFD. Team bytes are signed; only 0..3 qualify.
        let mine = self.players[self.player_index_for_leader(leader_slot)?].team;
        if !CONFIGURED_TEAMS.contains(&mine) {
            return Ok(false);
        }
        let theirs = self.players[self.player_index_for_leader(target)?].team;
        if mine != theirs {
            return Ok(false);
        }

        // 0x006EBDFF..0x006EBE2A.
        if self.frame != 0
            && arg == IsTeamArg::NonZero
            && ALLIANCE_GATED_TEAM_STYLES.contains(&self.team_style)
        {
            return self.is_ally(leader_slot, target);
        }
        Ok(true)
    }

    /// The `LeaderData::is_ally` leaf used by `is_team`: self is allied, otherwise both
    /// directional declarations must be exactly `DIPLO_ALLY`.
    pub fn is_ally(&self, leader_slot: usize, target: usize) -> Result<bool, TeamQueryError> {
        let leader = self.leader(leader_slot)?;
        let other = self.leader(target)?;
        if target as i32 == leader.who {
            return Ok(true);
        }
        let who = usize::try_from(leader.who)
            .ok()
            .filter(|&who| who < SETUP_SLOTS)
            .ok_or(TeamQueryError::LeaderWhoOutOfRange {
                slot: leader_slot,
                who: leader.who,
            })?;
        Ok(leader.diplos[target] == DIPLO_ALLY && other.diplos[who] == DIPLO_ALLY)
    }

    #[inline]
    fn leader(&self, slot: usize) -> Result<&LeaderTeamState, TeamQueryError> {
        self.leaders
            .get(slot)
            .ok_or(TeamQueryError::LeaderSlotOutOfRange { slot })
    }
}
