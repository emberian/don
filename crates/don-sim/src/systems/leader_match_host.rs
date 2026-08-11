// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical atomic host for terminal `Leader`/`Game` state transitions.
//!
//! [`victory_score::Leaders`] and [`victory_score::Match`] already contain the recovered
//! bodies for `Leader::victory` `0x006EC9B0` and `Leader::defeat` `0x006ECB00`.  Before
//! this module, callers either invoked those bodies directly or stopped at an authority
//! boundary.  This host gives the pair one transaction contract: validate the target,
//! execute against a staged clone, publish both owners together, and return the exact
//! checksum-channel change and cleanup requests produced by the body.
//!
//! The pair transaction deliberately does not consume Build/Unit cleanup requests.  A
//! concrete product owns those object stores. [`Sim::apply_leader_match_transaction`]
//! publishes the pair and immediately drains them through the executable tick adapter;
//! the Arena consumes the same masks against its own queues.

use super::victory_score::{
    adler32, DefeatType, Leaders, Match, MatchEvent, VictoryType, NUM_LEADERS,
};
use super::Sim;

/// One recovered terminal state call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderMatchRequest {
    /// `Leader::victory(int victory_type, int instant)` `0x006EC9B0`.
    Victory {
        who: usize,
        victory_type: VictoryType,
        instant: i32,
    },
    /// `Leader::defeat(int defeat_type, int by, int instant)` `0x006ECB00`.
    Defeat {
        who: usize,
        defeat_type: DefeatType,
        by: i32,
        instant: i32,
    },
}

impl LeaderMatchRequest {
    #[inline]
    pub const fn who(self) -> usize {
        match self {
            Self::Victory { who, .. } | Self::Defeat { who, .. } => who,
        }
    }
}

/// A request that cannot name a retail `leaders[8]` receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderMatchError {
    LeaderOutOfRange { who: usize },
}

/// Exact state evidence for one committed pair transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderMatchReceipt {
    pub request: LeaderMatchRequest,
    /// Adler-32 over the owned `LeaderData::walk_data` projection, before/after.
    pub leaders_before: u32,
    pub leaders_after: u32,
    /// Adler-32 over the owned `Game::walk_data` projection, before/after.
    pub match_before: u32,
    pub match_after: u32,
    /// Events appended by this call, including recursive allied wins/enemy defeats.
    pub events: Vec<MatchEvent>,
    /// Owners whose live Build queues must receive `Build::clean_queue(0)`.
    pub terminal_queue_cleanup: u8,
    /// Owners whose Unit bands must receive the defeated-player cleanup transaction.
    pub defeat_unit_cleanup: u8,
}

fn leader_digest(leaders: &Leaders) -> u32 {
    let mut bytes = Vec::new();
    leaders.walk_bytes(&mut bytes);
    adler32(1, &bytes)
}

fn match_digest(game: &Match) -> u32 {
    let mut bytes = Vec::new();
    game.walk_bytes(&mut bytes);
    adler32(1, &bytes)
}

/// Execute one terminal call against the authoritative pair.
///
/// All validation happens before the staged clone is touched. The recovered bodies have
/// no fallible host callbacks; after they finish, both owners are swapped together.
pub fn apply_leader_match(
    leaders: &mut Leaders,
    game: &mut Match,
    request: LeaderMatchRequest,
) -> Result<LeaderMatchReceipt, LeaderMatchError> {
    if request.who() >= NUM_LEADERS || request.who() >= leaders.slots.len() {
        return Err(LeaderMatchError::LeaderOutOfRange { who: request.who() });
    }

    let leaders_before = leader_digest(leaders);
    let match_before = match_digest(game);
    let old_event_count = leaders.events.len();
    let mut staged_leaders = leaders.clone();
    let mut staged_game = game.clone();
    match request {
        LeaderMatchRequest::Victory {
            who,
            victory_type,
            instant,
        } => staged_leaders.victory(&mut staged_game, who, victory_type, instant),
        LeaderMatchRequest::Defeat {
            who,
            defeat_type,
            by,
            instant,
        } => staged_leaders.defeat(&mut staged_game, who, defeat_type, by, instant),
    }

    let events = staged_leaders.events[old_event_count..].to_vec();
    let (terminal_queue_cleanup, defeat_unit_cleanup) = staged_leaders.pending_cleanup_masks();
    let receipt = LeaderMatchReceipt {
        request,
        leaders_before,
        leaders_after: leader_digest(&staged_leaders),
        match_before,
        match_after: match_digest(&staged_game),
        events,
        terminal_queue_cleanup,
        defeat_unit_cleanup,
    };
    *leaders = staged_leaders;
    *game = staged_game;
    Ok(receipt)
}

/// Receipt from the full Sim adapter. A non-empty cleanup error is retained on
/// [`Sim::defeat_cleanup_error`] and its owner request remains armed for retry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimLeaderMatchReceipt {
    pub pair: LeaderMatchReceipt,
    pub cleanup_error: Option<super::defeat_cleanup::DefeatCleanupError>,
}

impl Sim {
    /// Execute the canonical pair transaction and immediately adapt its object cleanup to
    /// the live Sim stores.
    pub fn apply_leader_match_transaction(
        &mut self,
        request: LeaderMatchRequest,
    ) -> Result<SimLeaderMatchReceipt, LeaderMatchError> {
        let pair = apply_leader_match(&mut self.vic_leaders, &mut self.vic_match, request)?;
        self.flush_terminal_queue_cleanup();
        Ok(SimLeaderMatchReceipt {
            pair,
            cleanup_error: self.defeat_cleanup_error,
        })
    }
}
