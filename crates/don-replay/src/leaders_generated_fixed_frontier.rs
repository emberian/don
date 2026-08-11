// SPDX-License-Identifier: GPL-3.0-or-later
//! Long exact-prefix extension for the materialised `LeaderCols` fixed body.
//!
//! `LeaderData::walk_data` walks `[+0x08,+0x692a)` as one byte interval.  The PDB-generated
//! [`LeaderCols`] owner materialises every field from the existing `gov` boundary at `+0x14`
//! through `reg_terr[64]`, ending at `+0x14de`.  The sole representation hole inside that
//! prefix is `TauntRequest last_taunt[8]` at `[+0x354,+0x374)`; the established runtime
//! frontier already owns those exact bytes.  The next PDB field, `reg_buildings[129][64]`,
//! is deliberately deferred by the generated owner and remains the new refusal boundary.
//!
//! A caller must explicitly pass a canonical, current `LeaderCols` instance. This is a
//! conditional admission boundary; it is not evidence that `Sim` currently owns or populates
//! those columns. Every byte duplicated by the victory/step-8/taunt/type owners is compared
//! before any extended frontier is returned, including the live tribe resolved from replay
//! selector 24. This module has no
//! `SimState` installation hook and never issues a checksum, even for a vacuous roster.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_runtime_frontier::{
    LeadersWalkBoundary, LeadersWalkFrontier, RuntimeCoveredRange, LEADER_FIXED_BODY_BEGIN,
};
use crate::leaders_runtime_tribe_frontier::{
    RuntimeLeadersTribeFrontier, LEADER_TRIBE_OFFSET, NEXT_FIXED_BODY_GAP,
};
use don_sim::generated::state::{leader, FieldDesc, LeaderCols, Pool, Repr};

pub const GENERATED_FIXED_PREFIX_END: usize = 0x14de;
pub const NEXT_DEFERRED_FIELD_NAME: &str = "reg_buildings";
pub const NEXT_DEFERRED_FIELD: &str = "reg_buildings[129][64]";
pub const NEXT_DEFERRED_FIELD_BYTES: usize = 129 * 64 * 2;
pub const LAST_TAUNT_BEGIN: usize = 0x354;
pub const LAST_TAUNT_END: usize = 0x374;
pub const LEADER_WALK_FIXED_RANGE_CALL_VA: u32 = 0x006d_6796;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedFixedProvenance {
    /// A scalar/array plane materialised by the PDB-generated `LeaderCols` owner.
    PdbGeneratedLeaderCols,
    /// The enum array which `LeaderCols` conservatively leaves aggregate, supplied by the
    /// already-established taunt/runtime owner after all adjacent duplicate checks pass.
    ExistingRuntimeFrontierGapFill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedFixedClaim {
    pub field: &'static str,
    pub begin: usize,
    pub end: usize,
    pub provenance: GeneratedFixedProvenance,
}

impl GeneratedFixedClaim {
    pub const fn bytes(self) -> usize {
        self.end - self.begin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFixedPrefixRow {
    pub slot: u8,
    pub active: bool,
    image: Vec<u8>,
    claims: Vec<GeneratedFixedClaim>,
    conditionally_admitted_walked_bytes: usize,
    duplicate_checked_walked_bytes: usize,
}

impl GeneratedFixedPrefixRow {
    pub fn claims(&self) -> &[GeneratedFixedClaim] {
        &self.claims
    }

    pub fn conditionally_admitted_slice(&self, range: RuntimeCoveredRange) -> Option<&[u8]> {
        let owned_end = if self.active {
            GENERATED_FIXED_PREFIX_END
        } else {
            LEADER_FIXED_BODY_BEGIN
        };
        (range.begin <= range.end && range.end <= owned_end)
            .then(|| &self.image[range.begin..range.end])
    }

    pub const fn conditionally_admitted_walked_bytes(&self) -> usize {
        self.conditionally_admitted_walked_bytes
    }

    pub const fn duplicate_checked_walked_bytes(&self) -> usize {
        self.duplicate_checked_walked_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeadersGeneratedFixedFrontier {
    previous: RuntimeLeadersTribeFrontier,
    rows: [GeneratedFixedPrefixRow; CHECKSUM_LEADER_SLOTS],
}

impl RuntimeLeadersGeneratedFixedFrontier {
    pub fn previous(&self) -> &RuntimeLeadersTribeFrontier {
        &self.previous
    }

    pub fn rows(&self) -> &[GeneratedFixedPrefixRow; CHECKSUM_LEADER_SLOTS] {
        &self.rows
    }

    pub fn conditionally_admitted_walked_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(GeneratedFixedPrefixRow::conditionally_admitted_walked_bytes)
            .sum()
    }

    pub fn duplicate_checked_walked_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(GeneratedFixedPrefixRow::duplicate_checked_walked_bytes)
            .sum()
    }

    /// No same-frame canonical `Sim` join installs these caller-supplied columns yet.
    /// Conditional admission is deliberately not reported as source-produced coverage.
    pub const fn source_produced_walked_bytes(&self) -> usize {
        0
    }

    /// Execute only the longest contiguous exact prefix.  The deferred `reg_buildings`
    /// matrix remains inside retail's single fixed-body byte walk, so no later sparse field
    /// or child is reachable yet.
    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        let mut checksum = 1u32;
        let mut bytes_walked = 0u64;
        for (slot, row) in self.rows.iter().enumerate() {
            let header = &row.image[..LEADER_FIXED_BODY_BEGIN];
            checksum = don_sim::checksum::adler32(checksum, header);
            bytes_walked += header.len() as u64;
            if !row.active {
                continue;
            }

            let body = &row.image[LEADER_FIXED_BODY_BEGIN..GENERATED_FIXED_PREFIX_END];
            checksum = don_sim::checksum::adler32(checksum, body);
            bytes_walked += body.len() as u64;
            return LeadersWalkFrontier {
                checksum,
                bytes_walked,
                boundary: LeadersWalkBoundary::FixedBody {
                    slot,
                    offset: GENERATED_FIXED_PREFIX_END,
                },
            };
        }
        LeadersWalkFrontier {
            checksum,
            bytes_walked,
            boundary: LeadersWalkBoundary::Complete,
        }
    }

    /// Full-channel authority is still absent.  Keep the issuance gate red even if a
    /// synthetic roster happens to contain no active rows.
    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedFixedFrontierError {
    ColumnRowCount {
        expected: usize,
        got: usize,
    },
    PreviousBoundaryChanged {
        expected: LeadersWalkBoundary,
        got: LeadersWalkBoundary,
    },
    InvalidGeneratedFieldLayout {
        field: &'static str,
        offset: usize,
        size: usize,
        count: usize,
        pool: Pool,
        repr: Repr,
    },
    ColumnDisagreement {
        slot: usize,
        offset: usize,
        field: &'static str,
        established_owner: &'static str,
        established: u8,
        column: u8,
    },
    MissingPrefixByte {
        slot: usize,
        offset: usize,
    },
    UnexpectedRuntimeGapFill {
        slot: usize,
        offset: usize,
    },
    DeferredBoundaryChanged,
}

impl std::fmt::Display for GeneratedFixedFrontierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "generated Leader fixed frontier refused: {self:?}")
    }
}

impl std::error::Error for GeneratedFixedFrontierError {}

fn invalid_layout(field: &FieldDesc) -> GeneratedFixedFrontierError {
    GeneratedFixedFrontierError::InvalidGeneratedFieldLayout {
        field: field.name,
        offset: field.offset as usize,
        size: field.size as usize,
        count: field.count as usize,
        pool: field.pool,
        repr: field.repr,
    }
}

fn field_bytes(
    columns: &LeaderCols,
    row: usize,
    field: &FieldDesc,
) -> Result<Vec<u8>, GeneratedFixedFrontierError> {
    let count = field.count as usize;
    let plane = field.plane as usize;
    let (width, planes) = match field.pool {
        Pool::W4 => (4, leader::W4_PLANES),
        Pool::W2 => (2, leader::W2_PLANES),
        Pool::W1 => (1, leader::W1_PLANES),
        Pool::WF => (4, leader::WF_PLANES),
        Pool::None => return Err(invalid_layout(field)),
    };
    if count == 0
        || count.checked_mul(width) != Some(field.size as usize)
        || plane.checked_add(count).is_none_or(|end| end > planes)
    {
        return Err(invalid_layout(field));
    }

    let mut bytes = Vec::with_capacity(field.size as usize);
    match field.pool {
        Pool::W4 => {
            if count == 1 {
                bytes.extend_from_slice(&columns.w4_plane(plane)[row].to_le_bytes());
            } else {
                for value in columns.w4_arr(plane, row, count) {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        Pool::W2 => {
            if count == 1 {
                bytes.extend_from_slice(&columns.w2_plane(plane)[row].to_le_bytes());
            } else {
                for value in columns.w2_arr(plane, row, count) {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        Pool::W1 => {
            if count == 1 {
                bytes.push(columns.w1_plane(plane)[row] as u8);
            } else {
                bytes.extend(
                    columns
                        .w1_arr(plane, row, count)
                        .iter()
                        .map(|value| *value as u8),
                );
            }
        }
        Pool::WF => {
            if count == 1 {
                bytes.extend_from_slice(&columns.wf_slice(plane)[row].to_bits().to_le_bytes());
            } else {
                for value in columns.wf_arr(plane, row, count) {
                    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
                }
            }
        }
        Pool::None => unreachable!("validated materialised pool"),
    }
    Ok(bytes)
}

fn disagree(
    slot: usize,
    offset: usize,
    field_for_offset: &[Option<&'static str>],
    established_owner: &'static str,
    established: u8,
    column: u8,
) -> GeneratedFixedFrontierError {
    GeneratedFixedFrontierError::ColumnDisagreement {
        slot,
        offset,
        field: field_for_offset[offset].unwrap_or("PDB padding"),
        established_owner,
        established,
        column,
    }
}

/// Join a canonical live PDB-column owner to the established tribe frontier.
///
/// All overlapping bytes are duplicate-owner gates, not last-writer-wins merges.  Only the
/// PDB generator's explicitly materialised planes are admitted.  The first deferred field
/// remains a hard boundary even though later fields have storage.
pub fn bind_generated_fixed_prefix(
    previous: RuntimeLeadersTribeFrontier,
    columns: &LeaderCols,
) -> Result<RuntimeLeadersGeneratedFixedFrontier, GeneratedFixedFrontierError> {
    if columns.len() != CHECKSUM_LEADER_SLOTS {
        return Err(GeneratedFixedFrontierError::ColumnRowCount {
            expected: CHECKSUM_LEADER_SLOTS,
            got: columns.len(),
        });
    }

    let deferred_boundary_is_exact = leader::FIELDS.iter().any(|field| {
        field.name == NEXT_DEFERRED_FIELD_NAME
            && field.offset as usize == GENERATED_FIXED_PREFIX_END
            && field.size as usize == NEXT_DEFERRED_FIELD_BYTES
            && field.count as usize == 129 * 64
            && field.repr == Repr::Deferred
            && field.pool == Pool::None
    });
    if !deferred_boundary_is_exact {
        return Err(GeneratedFixedFrontierError::DeferredBoundaryChanged);
    }

    let first_active = previous.base().rows.iter().position(|row| row.active);
    let expected_previous = first_active.map_or(LeadersWalkBoundary::Complete, |slot| {
        LeadersWalkBoundary::FixedBody {
            slot,
            offset: NEXT_FIXED_BODY_GAP,
        }
    });
    let got_previous = previous.walk_frontier().boundary;
    if got_previous != expected_previous {
        return Err(GeneratedFixedFrontierError::PreviousBoundaryChanged {
            expected: expected_previous,
            got: got_previous,
        });
    }

    let mut rows = Vec::with_capacity(CHECKSUM_LEADER_SLOTS);
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let base_row = &previous.base().rows[slot];
        let walk_end = if base_row.active {
            GENERATED_FIXED_PREFIX_END
        } else {
            LEADER_FIXED_BODY_BEGIN
        };
        let mut image = vec![0u8; GENERATED_FIXED_PREFIX_END];
        let mut owned = vec![false; GENERATED_FIXED_PREFIX_END];
        let mut field_for_offset = vec![None; GENERATED_FIXED_PREFIX_END];
        let mut claims = Vec::new();

        for field in &leader::FIELDS {
            let begin = field.offset as usize;
            let end = begin + field.size as usize;
            if begin >= walk_end || end > walk_end || field.alias_of.is_some() {
                continue;
            }
            if !field.repr.materialised() {
                continue;
            }
            let bytes = field_bytes(columns, slot, field)?;
            image[begin..end].copy_from_slice(&bytes);
            owned[begin..end].fill(true);
            field_for_offset[begin..end].fill(Some(field.name));
            claims.push(GeneratedFixedClaim {
                field: field.name,
                begin,
                end,
                provenance: GeneratedFixedProvenance::PdbGeneratedLeaderCols,
            });
        }

        let mut prior = vec![None; walk_end];
        for range in base_row.covered_ranges() {
            let begin = range.begin.min(walk_end);
            let end = range.end.min(walk_end);
            if begin >= end {
                continue;
            }
            let source = base_row
                .owned_slice(RuntimeCoveredRange { begin, end })
                .expect("covered range remains owned");
            for (relative, value) in source.iter().copied().enumerate() {
                prior[begin + relative] = Some((value, "RuntimeLeadersFrontier"));
            }
        }
        if base_row.active {
            let tribe = previous
                .tribe_for(slot)
                .expect("previous binding issued one tribe per active row")
                .to_le_bytes();
            for (relative, value) in tribe.iter().copied().enumerate() {
                prior[LEADER_TRIBE_OFFSET + relative] =
                    Some((value, "RuntimeLeadersTribeFrontier"));
            }
        }

        let mut conditionally_admitted = 0usize;
        let mut duplicate_checked = 0usize;
        let mut fallback_begin = None;
        for offset in 0..walk_end {
            match (owned[offset], prior[offset]) {
                (true, Some((established, established_owner))) => {
                    duplicate_checked += 1;
                    if image[offset] != established {
                        return Err(disagree(
                            slot,
                            offset,
                            &field_for_offset,
                            established_owner,
                            established,
                            image[offset],
                        ));
                    }
                }
                (true, None) => conditionally_admitted += 1,
                (false, Some((established, _))) => {
                    if !(LAST_TAUNT_BEGIN..LAST_TAUNT_END).contains(&offset) {
                        return Err(GeneratedFixedFrontierError::UnexpectedRuntimeGapFill {
                            slot,
                            offset,
                        });
                    }
                    image[offset] = established;
                    owned[offset] = true;
                    if fallback_begin.is_none() {
                        fallback_begin = Some(offset);
                    }
                }
                (false, None) => {
                    return Err(GeneratedFixedFrontierError::MissingPrefixByte { slot, offset });
                }
            }
            if fallback_begin.is_some() && (offset + 1 == walk_end || owned[offset + 1]) {
                let begin = fallback_begin.take().expect("checked Some");
                claims.push(GeneratedFixedClaim {
                    field: "last_taunt",
                    begin,
                    end: offset + 1,
                    provenance: GeneratedFixedProvenance::ExistingRuntimeFrontierGapFill,
                });
            }
        }

        rows.push(GeneratedFixedPrefixRow {
            slot: slot as u8,
            active: base_row.active,
            image,
            claims,
            conditionally_admitted_walked_bytes: conditionally_admitted,
            duplicate_checked_walked_bytes: duplicate_checked,
        });
    }

    Ok(RuntimeLeadersGeneratedFixedFrontier {
        previous,
        rows: rows.try_into().expect("exact eight-row projection"),
    })
}
