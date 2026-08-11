// SPDX-License-Identifier: GPL-3.0-or-later
//! Coverage and provenance accounting for the sparse setup-time Leader prefix.
//!
//! [`crate::leader_initial_prefix`] owns exact checksum-visible spans, but it is
//! intentionally not a complete `leaders` checksum producer.  This module turns
//! those row-local spans into an auditable ledger and a compact report without
//! installing them in [`crate::state::SimState`].  In particular, an Adler-32
//! over the complete channel cannot validate a partial image, so every report
//! emitted here remains ineligible for the substantive checksum scoreboard.

#![forbid(unsafe_code)]

use crate::initial::InitialState;
use crate::leader_initial_prefix::{
    derive, InitialLeaderPrefix, InitialLeaderPrefixError, LeaderOwnedSpan, LeaderPrefixProvenance,
    ACTIVE_OWNED_BYTES, CHECKSUM_LEADER_SLOTS, FIXED_WALK_END, INACTIVE_OWNED_BYTES,
};
use crate::world_owner_frontier::sha256;

/// The fixed header always visited by `LeaderData::walk_data`.
pub const LEADER_HEADER_BYTES: usize = 8;

/// Content-addressed ownership for one checksum-relative interval in one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderPrefixLedgerSpan {
    pub slot: u8,
    pub begin: usize,
    pub end: usize,
    pub provenance: LeaderPrefixProvenance,
    pub value_sha256: [u8; 32],
}

impl LeaderPrefixLedgerSpan {
    pub const fn bytes(self) -> usize {
        self.end - self.begin
    }

    const fn row_span(self) -> LeaderOwnedSpan {
        LeaderOwnedSpan {
            begin: self.begin,
            end: self.end,
            provenance: self.provenance,
        }
    }
}

/// Byte totals grouped by the exact setup routine which owns them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LeaderPrefixProvenanceCoverage {
    pub init_rules_and_teams_flags: usize,
    pub leader_init_identity: usize,
    pub leader_init_self_diplomacy: usize,
    pub leader_init_diplomacy_reset: usize,
}

impl LeaderPrefixProvenanceCoverage {
    pub const fn total(self) -> usize {
        self.init_rules_and_teams_flags
            + self.leader_init_identity
            + self.leader_init_self_diplomacy
            + self.leader_init_diplomacy_reset
    }

    fn add(&mut self, provenance: LeaderPrefixProvenance, bytes: usize) {
        match provenance {
            LeaderPrefixProvenance::InitRulesAndTeamsFlags => {
                self.init_rules_and_teams_flags += bytes;
            }
            LeaderPrefixProvenance::LeaderInitIdentity => {
                self.leader_init_identity += bytes;
            }
            LeaderPrefixProvenance::LeaderInitSelfDiplomacy => {
                self.leader_init_self_diplomacy += bytes;
            }
            LeaderPrefixProvenance::LeaderInitDiplomacyReset => {
                self.leader_init_diplomacy_reset += bytes;
            }
        }
    }
}

/// Honest coverage ceiling for one replay's setup-time Leader owner.
///
/// `fixed_traversal_bytes` excludes all dynamic children after the fixed body;
/// it is therefore a lower bound on the full channel walk, not a denominator
/// from which whole-channel compatibility may be inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaderPrefixCoverageReport {
    pub rows: usize,
    pub active_rows: usize,
    pub inactive_rows: usize,
    pub human_rows: usize,
    pub nonhuman_rows: usize,
    pub spans: usize,
    pub fixed_traversal_bytes: usize,
    pub exact_owned_checksum_bytes: usize,
    pub unknown_fixed_traversal_bytes: usize,
    pub dynamic_children_owned_bytes: usize,
    pub human_only_first_checksum_candidate: bool,
    /// Sparse ownership is deliberately never a complete channel walk.
    pub walk_complete: bool,
    /// Sparse ownership is deliberately never an exact whole-channel producer.
    pub exact_channel_producer: bool,
    /// Must remain false until the prefix is merged into a complete live owner.
    pub substantive_scoreboard_eligible: bool,
    pub provenance: LeaderPrefixProvenanceCoverage,
}

/// Validated sparse ownership for one replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderPrefixSpanLedger {
    prefix: InitialLeaderPrefix,
    spans: Vec<LeaderPrefixLedgerSpan>,
    coverage: LeaderPrefixCoverageReport,
}

impl LeaderPrefixSpanLedger {
    pub fn derive(initial: &InitialState) -> Result<Self, LeaderPrefixLedgerError> {
        Self::from_prefix(derive(initial)?)
    }

    pub fn from_prefix(prefix: InitialLeaderPrefix) -> Result<Self, LeaderPrefixLedgerError> {
        let active_rows = prefix.active_mask.count_ones() as usize;
        let human_rows = prefix.human_mask.count_ones() as usize;
        let nonhuman_rows = prefix.nonhuman_mask.count_ones() as usize;
        if prefix.human_mask & !prefix.active_mask != 0
            || prefix.nonhuman_mask != prefix.active_mask & !prefix.human_mask
        {
            return Err(LeaderPrefixLedgerError::RosterMaskMismatch);
        }

        let mut spans = Vec::new();
        let mut provenance = LeaderPrefixProvenanceCoverage::default();
        let mut exact_owned_checksum_bytes = 0usize;
        for (slot, row) in prefix.rows.iter().enumerate() {
            if usize::from(row.slot) != slot
                || row.active != (prefix.active_mask & (1u8 << slot) != 0)
                || row.human != (prefix.human_mask & (1u8 << slot) != 0)
            {
                return Err(LeaderPrefixLedgerError::RowRosterMismatch { slot });
            }

            let row_limit = if row.active {
                FIXED_WALK_END
            } else {
                LEADER_HEADER_BYTES
            };
            let mut previous_end = 0usize;
            let mut row_bytes = 0usize;
            for span in row.owned_spans().iter().copied() {
                if span.begin >= span.end || span.end > row_limit {
                    return Err(LeaderPrefixLedgerError::SpanOutOfBounds {
                        slot,
                        begin: span.begin,
                        end: span.end,
                        row_limit,
                    });
                }
                if span.begin < previous_end {
                    return Err(LeaderPrefixLedgerError::OverlappingOrUnsortedSpan {
                        slot,
                        previous_end,
                        begin: span.begin,
                    });
                }
                let bytes = row.owned_slice(span).ok_or(
                    LeaderPrefixLedgerError::OwnedSliceUnavailable {
                        slot,
                        begin: span.begin,
                        end: span.end,
                    },
                )?;
                let ledger_span = LeaderPrefixLedgerSpan {
                    slot: row.slot,
                    begin: span.begin,
                    end: span.end,
                    provenance: span.provenance,
                    value_sha256: sha256(bytes),
                };
                provenance.add(span.provenance, span.bytes());
                row_bytes += span.bytes();
                previous_end = span.end;
                spans.push(ledger_span);
            }

            let expected = if row.active {
                ACTIVE_OWNED_BYTES
            } else {
                INACTIVE_OWNED_BYTES
            };
            if row_bytes != expected || row.claimed_walked_bytes() != expected {
                return Err(LeaderPrefixLedgerError::RowOwnedByteCount {
                    slot,
                    expected,
                    got: row_bytes,
                });
            }
            exact_owned_checksum_bytes += row_bytes;
        }

        if exact_owned_checksum_bytes != prefix.claimed_walked_bytes()
            || provenance.total() != exact_owned_checksum_bytes
        {
            return Err(LeaderPrefixLedgerError::TotalOwnedByteCount {
                prefix: prefix.claimed_walked_bytes(),
                ledger: exact_owned_checksum_bytes,
                provenance: provenance.total(),
            });
        }

        let fixed_traversal_bytes = active_rows * FIXED_WALK_END
            + (CHECKSUM_LEADER_SLOTS - active_rows) * LEADER_HEADER_BYTES;
        let coverage = LeaderPrefixCoverageReport {
            rows: CHECKSUM_LEADER_SLOTS,
            active_rows,
            inactive_rows: CHECKSUM_LEADER_SLOTS - active_rows,
            human_rows,
            nonhuman_rows,
            spans: spans.len(),
            fixed_traversal_bytes,
            exact_owned_checksum_bytes,
            unknown_fixed_traversal_bytes: fixed_traversal_bytes - exact_owned_checksum_bytes,
            dynamic_children_owned_bytes: 0,
            human_only_first_checksum_candidate: prefix.is_human_only_first_checksum_candidate(),
            walk_complete: false,
            exact_channel_producer: false,
            substantive_scoreboard_eligible: false,
            provenance,
        };
        Ok(Self {
            prefix,
            spans,
            coverage,
        })
    }

    pub fn prefix(&self) -> &InitialLeaderPrefix {
        &self.prefix
    }

    pub fn spans(&self) -> &[LeaderPrefixLedgerSpan] {
        &self.spans
    }

    pub const fn coverage(&self) -> LeaderPrefixCoverageReport {
        self.coverage
    }

    pub fn owner_at(&self, slot: usize, offset: usize) -> Option<&LeaderPrefixLedgerSpan> {
        self.spans.iter().find(|span| {
            usize::from(span.slot) == slot && span.begin <= offset && offset < span.end
        })
    }

    /// Recover the exact bytes behind a content-addressed ledger span.
    pub fn owned_slice(&self, span: LeaderPrefixLedgerSpan) -> Option<&[u8]> {
        let row = self.prefix.rows.get(usize::from(span.slot))?;
        let bytes = row.owned_slice(span.row_span())?;
        (sha256(bytes) == span.value_sha256).then_some(bytes)
    }
}

/// Aggregate coverage for a replay set, with checksum-bearing totals kept
/// separate because only those files participate in the `0x39` scoreboard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LeaderPrefixCorpusCoverage {
    pub files: usize,
    pub checksum_bearing_files: usize,
    pub all_exact_owned_checksum_bytes: usize,
    pub checksum_bearing_exact_owned_checksum_bytes: usize,
    pub checksum_bearing_active_rows: usize,
    /// Present replay Player rows before duplicate `Player::who` values collapse
    /// onto one checksum Leader row.
    pub checksum_bearing_present_player_rows: usize,
    /// The historical `8 + 704 * present_players` projection. This is an upper
    /// bound, not ownership, because duplicate `who` rows write the same Leader.
    pub checksum_bearing_player_row_projection_bytes: usize,
    pub checksum_bearing_duplicate_who_collapsed_rows: usize,
    pub checksum_bearing_player_row_projection_overcount_bytes: usize,
    pub checksum_bearing_human_only_candidates: usize,
    pub checksum_bearing_expired_by_nonhuman: usize,
    /// A sparse prefix never contributes a substantive Leaders compare.
    pub prefix_contributed_substantive_leaders_compares: usize,
    /// A sparse prefix never contributes a substantive Leaders match.
    pub prefix_contributed_substantive_leaders_matches: usize,
}

impl LeaderPrefixCorpusCoverage {
    pub fn observe(
        &mut self,
        checksum_bearing: bool,
        present_player_rows: usize,
        coverage: LeaderPrefixCoverageReport,
    ) {
        self.files += 1;
        self.all_exact_owned_checksum_bytes += coverage.exact_owned_checksum_bytes;
        if checksum_bearing {
            debug_assert!(present_player_rows >= coverage.active_rows);
            self.checksum_bearing_files += 1;
            self.checksum_bearing_exact_owned_checksum_bytes += coverage.exact_owned_checksum_bytes;
            self.checksum_bearing_active_rows += coverage.active_rows;
            self.checksum_bearing_present_player_rows += present_player_rows;
            self.checksum_bearing_player_row_projection_bytes +=
                CHECKSUM_LEADER_SLOTS + 704 * present_player_rows;
            self.checksum_bearing_duplicate_who_collapsed_rows +=
                present_player_rows.saturating_sub(coverage.active_rows);
            self.checksum_bearing_player_row_projection_overcount_bytes +=
                704 * present_player_rows.saturating_sub(coverage.active_rows);
            if coverage.human_only_first_checksum_candidate {
                self.checksum_bearing_human_only_candidates += 1;
            } else {
                self.checksum_bearing_expired_by_nonhuman += 1;
            }
        }
        // Intentionally no scoreboard mutation.  These fields make that
        // invariant explicit in any aggregate report which carries the ledger.
        debug_assert!(!coverage.substantive_scoreboard_eligible);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaderPrefixLedgerError {
    Prefix(InitialLeaderPrefixError),
    RosterMaskMismatch,
    RowRosterMismatch {
        slot: usize,
    },
    SpanOutOfBounds {
        slot: usize,
        begin: usize,
        end: usize,
        row_limit: usize,
    },
    OverlappingOrUnsortedSpan {
        slot: usize,
        previous_end: usize,
        begin: usize,
    },
    OwnedSliceUnavailable {
        slot: usize,
        begin: usize,
        end: usize,
    },
    RowOwnedByteCount {
        slot: usize,
        expected: usize,
        got: usize,
    },
    TotalOwnedByteCount {
        prefix: usize,
        ledger: usize,
        provenance: usize,
    },
}

impl From<InitialLeaderPrefixError> for LeaderPrefixLedgerError {
    fn from(error: InitialLeaderPrefixError) -> Self {
        Self::Prefix(error)
    }
}

impl std::fmt::Display for LeaderPrefixLedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Leader prefix ownership refused: {self:?}")
    }
}

impl std::error::Error for LeaderPrefixLedgerError {}
