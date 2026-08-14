// SPDX-License-Identifier: GPL-3.0-or-later
//! Conditional continuation across the deferred Leader regional-building history.
//!
//! Retail walks `[+0x08,+0x692a)` as one fixed byte range, then walks eight
//! 92-byte `Diplomacy` children before reaching `Personality` at `+0x6dd4`.
//! [`RuntimeLeadersGeneratedFixedFrontier`] already reaches the first generated
//! representation hole, `reg_buildings` at `+0x14de`.  This module admits an
//! explicit caller-supplied current matrix and the two otherwise unnamed padding
//! words inside the remainder of the fixed range.  Materialised fields continue
//! to come from the same caller-supplied [`LeaderCols`] snapshot; the established
//! runtime frontier supplies generated-deferred `num_queued` and `Diplomacy[8]`.
//!
//! This is deliberately not a `Sim` ownership claim.  The inputs have no same-frame
//! join to canonical simulation state, so source-produced coverage, checksum
//! issuance, and scoreboard installation remain pinned off.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
use crate::leaders_generated_fixed_frontier::{
    RuntimeLeadersGeneratedFixedFrontier, GENERATED_FIXED_PREFIX_END,
};
use crate::leaders_runtime_frontier::{
    LeadersWalkBoundary, LeadersWalkFrontier, RuntimeCoveredRange, LEADER_DIPLOMACY_BEGIN,
    LEADER_DIPLOMACY_BYTES, LEADER_FIXED_BODY_BEGIN, LEADER_FIXED_BODY_END,
};
use don_sim::generated::state::{leader, FieldDesc, LeaderCols, Pool, Repr};

pub const REG_BUILDING_REGIONS: usize = 64;
pub const REG_BUILDING_TYPE_SLOTS: usize = 129;
pub const REG_BUILDING_VALUES: usize = REG_BUILDING_REGIONS * REG_BUILDING_TYPE_SLOTS;
pub const REG_BUILDINGS_BEGIN: usize = GENERATED_FIXED_PREFIX_END;
pub const REG_BUILDINGS_END: usize = 0x555e;
pub const NUM_QUEUED_BEGIN: usize = 0x5a22;
pub const NUM_QUEUED_VALUES: usize = 806;
pub const NUM_QUEUED_END: usize = 0x606e;
pub const QUEUE_PADDING_BEGIN: usize = NUM_QUEUED_END;
pub const QUEUE_PADDING_END: usize = 0x6070;
pub const LAST_BUILDING_FINISHED_BEGIN: usize = QUEUE_PADDING_END;
pub const LAST_BUILDING_FINISHED_VALUES: usize = 129;
pub const LAST_BUILDING_FINISHED_END: usize = 0x6274;
pub const GOV_BEGIN: usize = 0x14;
pub const GOV_END: usize = 0x18;
pub const SETUP_STAMPS_BEGIN: usize = 0x7ac;
pub const SETUP_STAMPS_END: usize = 0x7d8;
pub const SETUP_STAMP_DWORDS: usize = (SETUP_STAMPS_END - SETUP_STAMPS_BEGIN) / 4;
pub const REG_CITIES_BEGIN: usize = 0x125e;
pub const REG_FORTS_BEGIN: usize = 0x12de;
pub const REG_DOCKS_BEGIN: usize = 0x135e;
pub const REG_BUILD_REGISTRY_END: usize = 0x13de;
pub const REG_BUILD_REGISTRY_VALUES: usize = 3 * REG_BUILDING_REGIONS;
pub const HIGH_BUILDINGS_BEGIN: usize = 0x5660;
pub const HIGH_BUILDINGS_VALUES: usize = REG_BUILDING_TYPE_SLOTS;
pub const HIGH_BUILDINGS_END: usize = 0x5762;
pub const REG_ATTACKED_BEGIN: usize = 0x67f6;
pub const REG_WARS_BEGIN: usize = 0x6836;
pub const REG_NEUTRALS_BEGIN: usize = 0x6876;
pub const REG_ALLIES_BEGIN: usize = 0x68b6;
pub const REG_STRATEGY_HISTORY_END: usize = 0x68f6;
pub const REG_STRATEGY_HISTORY_VALUES: usize = 4 * REG_BUILDING_REGIONS;
pub const CTW_PADDING_BEGIN: usize = 0x6922;
pub const CTW_PADDING_END: usize = 0x6924;
pub const LEADER_DIPLOMACY_END: usize =
    LEADER_DIPLOMACY_BEGIN + LEADER_DIPLOMACY_BYTES * CHECKSUM_LEADER_SLOTS;
pub const DEFERRED_FIXED_EXTENSION_BYTES: usize =
    LEADER_FIXED_BODY_END - GENERATED_FIXED_PREFIX_END;
pub const DIPLOMACY_EXTENSION_BYTES: usize = LEADER_DIPLOMACY_BYTES * CHECKSUM_LEADER_SLOTS;

pub const LEADER_INIT_VA: u32 = 0x006e_3930;
pub const LEADER_INIT_REG_BUILDINGS_VA: u32 = 0x006e_4acd;
pub const LEADER_GET_REG_BUILDINGS_VA: u32 = 0x006d_5470;
pub const LEADER_WALK_DIPLOMACY_CALL_VA: u32 = 0x006d_67ac;
pub const LEADER_WALK_PERSONALITY_CALL_VA: u32 = 0x006d_67c7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredLeaderFixedRow {
    /// Retail storage order recovered at `Leader::init` `0x006e4acd`: 64 region
    /// slices, each containing 129 building-type `u16` counters.  The PDB spells
    /// the declaration `unsigned short[129][64]`; this vector intentionally names
    /// the executable's observed indexing order instead of repeating that ambiguity.
    pub reg_buildings_by_region_then_type: Vec<u16>,
    /// Walked PDB padding `[+0x606e,+0x6070)` after `num_queued[806]`.
    pub queue_padding: [u8; 2],
    /// Walked PDB padding `[+0x6922,+0x6924)` before `defeat_stamp`.
    pub ctw_padding: [u8; 2],
}

impl Default for DeferredLeaderFixedRow {
    fn default() -> Self {
        Self {
            reg_buildings_by_region_then_type: vec![0; REG_BUILDING_VALUES],
            queue_padding: [0; 2],
            ctw_padding: [0; 2],
        }
    }
}

impl DeferredLeaderFixedRow {
    pub const fn reg_building_index(region: usize, building_slot: usize) -> Option<usize> {
        if region < REG_BUILDING_REGIONS && building_slot < REG_BUILDING_TYPE_SLOTS {
            Some(region * REG_BUILDING_TYPE_SLOTS + building_slot)
        } else {
            None
        }
    }

    pub fn reg_building(&self, region: usize, building_slot: usize) -> Option<u16> {
        Self::reg_building_index(region, building_slot)
            .and_then(|index| self.reg_buildings_by_region_then_type.get(index).copied())
    }

    pub fn set_reg_building(&mut self, region: usize, building_slot: usize, value: u16) -> bool {
        let Some(index) = Self::reg_building_index(region, building_slot) else {
            return false;
        };
        let Some(cell) = self.reg_buildings_by_region_then_type.get_mut(index) else {
            return false;
        };
        *cell = value;
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredLeadersFixedAuthority {
    pub rows: [DeferredLeaderFixedRow; CHECKSUM_LEADER_SLOTS],
}

impl Default for DeferredLeadersFixedAuthority {
    fn default() -> Self {
        Self {
            rows: std::array::from_fn(|_| DeferredLeaderFixedRow::default()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredHistoryProvenance {
    ExistingGeneratedFixedPrefix,
    DeferredRegionalBuildingAuthority,
    PdbGeneratedLeaderCols,
    ExistingRuntimeNumQueued,
    ExplicitWalkPaddingAuthority,
    ExistingRuntimeDiplomacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeferredHistoryClaim {
    pub field: &'static str,
    pub begin: usize,
    pub end: usize,
    pub provenance: DeferredHistoryProvenance,
}

impl DeferredHistoryClaim {
    pub const fn bytes(self) -> usize {
        self.end - self.begin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredHistoryRow {
    pub slot: u8,
    pub active: bool,
    image: Vec<u8>,
    claims: Vec<DeferredHistoryClaim>,
    conditionally_admitted_walked_bytes: usize,
    duplicate_checked_walked_bytes: usize,
}

impl DeferredHistoryRow {
    pub fn claims(&self) -> &[DeferredHistoryClaim] {
        &self.claims
    }

    /// Return a slice only when it belongs to one contiguous retail visitor call.
    /// The two layout bytes `[+0x692a,+0x692c)` are deliberately unavailable because
    /// neither the fixed-body nor a Diplomacy visitor walks them.
    pub fn conditionally_admitted_slice(&self, range: RuntimeCoveredRange) -> Option<&[u8]> {
        let inside_call = range.begin <= range.end
            && ((range.end <= LEADER_FIXED_BODY_END)
                || (range.begin >= LEADER_DIPLOMACY_BEGIN && range.end <= LEADER_DIPLOMACY_END));
        let owned_end = if self.active {
            LEADER_DIPLOMACY_END
        } else {
            LEADER_FIXED_BODY_BEGIN
        };
        (inside_call && range.end <= owned_end).then(|| &self.image[range.begin..range.end])
    }

    pub const fn conditionally_admitted_walked_bytes(&self) -> usize {
        self.conditionally_admitted_walked_bytes
    }

    pub const fn duplicate_checked_walked_bytes(&self) -> usize {
        self.duplicate_checked_walked_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeadersDeferredHistoryFrontier {
    previous: RuntimeLeadersGeneratedFixedFrontier,
    rows: [DeferredHistoryRow; CHECKSUM_LEADER_SLOTS],
}

impl RuntimeLeadersDeferredHistoryFrontier {
    pub fn previous(&self) -> &RuntimeLeadersGeneratedFixedFrontier {
        &self.previous
    }

    pub fn rows(&self) -> &[DeferredHistoryRow; CHECKSUM_LEADER_SLOTS] {
        &self.rows
    }

    pub fn conditionally_admitted_walked_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(DeferredHistoryRow::conditionally_admitted_walked_bytes)
            .sum()
    }

    pub fn duplicate_checked_walked_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(DeferredHistoryRow::duplicate_checked_walked_bytes)
            .sum()
    }

    pub const fn source_produced_walked_bytes(&self) -> usize {
        0
    }

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

            let fixed = &row.image[LEADER_FIXED_BODY_BEGIN..LEADER_FIXED_BODY_END];
            checksum = don_sim::checksum::adler32(checksum, fixed);
            bytes_walked += fixed.len() as u64;
            for target in 0..CHECKSUM_LEADER_SLOTS {
                let begin = LEADER_DIPLOMACY_BEGIN + target * LEADER_DIPLOMACY_BYTES;
                let end = begin + LEADER_DIPLOMACY_BYTES;
                checksum = don_sim::checksum::adler32(checksum, &row.image[begin..end]);
                bytes_walked += LEADER_DIPLOMACY_BYTES as u64;
            }

            return LeadersWalkFrontier {
                checksum,
                bytes_walked,
                boundary: LeadersWalkBoundary::DynamicChildren {
                    slot,
                    child: "Personality at +0x6dd4",
                },
            };
        }
        LeadersWalkFrontier {
            checksum,
            bytes_walked,
            boundary: LeadersWalkBoundary::Complete,
        }
    }

    /// Conditional caller-supplied authority never becomes a channel producer.
    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        Err(self.walk_frontier())
    }

    pub const fn installed_in_scoreboard(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeferredHistoryFrontierError {
    ColumnRowCount {
        expected: usize,
        got: usize,
    },
    RegionalBuildingLength {
        slot: usize,
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
    ColumnSnapshotDisagreement {
        slot: usize,
        offset: usize,
        field: &'static str,
        established: u8,
        column: u8,
    },
    RuntimeDisagreement {
        slot: usize,
        offset: usize,
        field: &'static str,
        runtime: u8,
        conditional: u8,
    },
    MissingExistingRuntimeRange {
        slot: usize,
        field: &'static str,
        begin: usize,
        end: usize,
    },
    UnsupportedGeneratedGap {
        field: &'static str,
        offset: usize,
        repr: Repr,
    },
    MissingFixedByte {
        slot: usize,
        offset: usize,
    },
}

impl std::fmt::Display for DeferredHistoryFrontierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "deferred Leader history frontier refused: {self:?}")
    }
}

impl std::error::Error for DeferredHistoryFrontierError {}

fn invalid_layout(field: &FieldDesc) -> DeferredHistoryFrontierError {
    DeferredHistoryFrontierError::InvalidGeneratedFieldLayout {
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
) -> Result<Vec<u8>, DeferredHistoryFrontierError> {
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

fn put(
    image: &mut [u8],
    owned: &mut [bool],
    conditional: &mut [bool],
    field_for_offset: &mut [Option<&'static str>],
    begin: usize,
    bytes: &[u8],
    field: &'static str,
    is_conditional: bool,
) {
    let end = begin + bytes.len();
    image[begin..end].copy_from_slice(bytes);
    owned[begin..end].fill(true);
    conditional[begin..end].fill(is_conditional);
    field_for_offset[begin..end].fill(Some(field));
}

/// Bind the exact deferred fixed-body inputs and advance through all eight already-owned
/// Diplomacy children.  `columns` must be the same logical current snapshot used to build
/// `previous`; every materialised prefix field is compared again before any extension is
/// returned.
pub fn bind_deferred_history_frontier(
    previous: RuntimeLeadersGeneratedFixedFrontier,
    columns: &LeaderCols,
    authority: &DeferredLeadersFixedAuthority,
) -> Result<RuntimeLeadersDeferredHistoryFrontier, DeferredHistoryFrontierError> {
    if columns.len() != CHECKSUM_LEADER_SLOTS {
        return Err(DeferredHistoryFrontierError::ColumnRowCount {
            expected: CHECKSUM_LEADER_SLOTS,
            got: columns.len(),
        });
    }

    let reg_layout = leader::FIELDS
        .iter()
        .find(|field| field.name == "reg_buildings");
    let queue_layout = leader::FIELDS
        .iter()
        .find(|field| field.name == "num_queued");
    for (field, begin, bytes, count) in [
        (
            reg_layout,
            REG_BUILDINGS_BEGIN,
            REG_BUILDING_VALUES * 2,
            REG_BUILDING_VALUES,
        ),
        (
            queue_layout,
            NUM_QUEUED_BEGIN,
            NUM_QUEUED_VALUES * 2,
            NUM_QUEUED_VALUES,
        ),
    ] {
        let Some(field) = field else {
            return Err(DeferredHistoryFrontierError::UnsupportedGeneratedGap {
                field: "missing deferred descriptor",
                offset: begin,
                repr: Repr::Deferred,
            });
        };
        if field.offset as usize != begin
            || field.size as usize != bytes
            || field.count as usize != count
            || field.repr != Repr::Deferred
            || field.pool != Pool::None
        {
            return Err(invalid_layout(field));
        }
    }

    let first_active = previous
        .previous()
        .base()
        .rows
        .iter()
        .position(|row| row.active);
    let expected_previous = first_active.map_or(LeadersWalkBoundary::Complete, |slot| {
        LeadersWalkBoundary::FixedBody {
            slot,
            offset: GENERATED_FIXED_PREFIX_END,
        }
    });
    let got_previous = previous.walk_frontier().boundary;
    if got_previous != expected_previous {
        return Err(DeferredHistoryFrontierError::PreviousBoundaryChanged {
            expected: expected_previous,
            got: got_previous,
        });
    }

    let mut rows = Vec::with_capacity(CHECKSUM_LEADER_SLOTS);
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        let previous_row = &previous.rows()[slot];
        let base_row = &previous.previous().base().rows[slot];
        let active = base_row.active;
        if active
            && authority.rows[slot].reg_buildings_by_region_then_type.len() != REG_BUILDING_VALUES
        {
            return Err(DeferredHistoryFrontierError::RegionalBuildingLength {
                slot,
                expected: REG_BUILDING_VALUES,
                got: authority.rows[slot].reg_buildings_by_region_then_type.len(),
            });
        }

        let mut image = vec![0u8; LEADER_DIPLOMACY_END];
        let mut owned = vec![false; LEADER_DIPLOMACY_END];
        let mut conditional = vec![false; LEADER_DIPLOMACY_END];
        let mut runtime_owned = vec![false; LEADER_DIPLOMACY_END];
        let mut field_for_offset = vec![None; LEADER_DIPLOMACY_END];
        let mut claims = Vec::new();
        let prefix_end = if active {
            GENERATED_FIXED_PREFIX_END
        } else {
            LEADER_FIXED_BODY_BEGIN
        };
        let prefix = previous_row
            .conditionally_admitted_slice(RuntimeCoveredRange {
                begin: 0,
                end: prefix_end,
            })
            .expect("previous frontier owns its advertised prefix");
        put(
            &mut image,
            &mut owned,
            &mut conditional,
            &mut field_for_offset,
            0,
            prefix,
            "existing generated fixed prefix",
            false,
        );
        claims.push(DeferredHistoryClaim {
            field: "existing generated fixed prefix",
            begin: 0,
            end: prefix_end,
            provenance: DeferredHistoryProvenance::ExistingGeneratedFixedPrefix,
        });

        let mut duplicate_checked = 0usize;
        for field in &leader::FIELDS {
            let begin = field.offset as usize;
            let end = begin + field.size as usize;
            if begin >= prefix_end
                || end > prefix_end
                || field.alias_of.is_some()
                || !field.repr.materialised()
            {
                continue;
            }
            let rendered = field_bytes(columns, slot, field)?;
            let established = &image[begin..end];
            if let Some(relative) = established
                .iter()
                .zip(&rendered)
                .position(|(left, right)| left != right)
            {
                let offset = begin + relative;
                return Err(DeferredHistoryFrontierError::ColumnSnapshotDisagreement {
                    slot,
                    offset,
                    field: field.name,
                    established: established[relative],
                    column: rendered[relative],
                });
            }
            duplicate_checked += rendered.len();
        }

        if active {
            let mut reg_bytes = Vec::with_capacity(REG_BUILDING_VALUES * 2);
            for value in &authority.rows[slot].reg_buildings_by_region_then_type {
                reg_bytes.extend_from_slice(&value.to_le_bytes());
            }
            put(
                &mut image,
                &mut owned,
                &mut conditional,
                &mut field_for_offset,
                REG_BUILDINGS_BEGIN,
                &reg_bytes,
                "reg_buildings",
                true,
            );
            claims.push(DeferredHistoryClaim {
                field: "reg_buildings",
                begin: REG_BUILDINGS_BEGIN,
                end: REG_BUILDINGS_END,
                provenance: DeferredHistoryProvenance::DeferredRegionalBuildingAuthority,
            });

            for field in &leader::FIELDS {
                let begin = field.offset as usize;
                let end = begin + field.size as usize;
                if begin < REG_BUILDINGS_END
                    || end > LEADER_FIXED_BODY_END
                    || field.alias_of.is_some()
                {
                    continue;
                }
                if field.name == "num_queued" {
                    let bytes = base_row
                        .owned_slice(RuntimeCoveredRange { begin, end })
                        .ok_or(DeferredHistoryFrontierError::MissingExistingRuntimeRange {
                            slot,
                            field: "num_queued",
                            begin,
                            end,
                        })?;
                    put(
                        &mut image,
                        &mut owned,
                        &mut conditional,
                        &mut field_for_offset,
                        begin,
                        bytes,
                        field.name,
                        false,
                    );
                    claims.push(DeferredHistoryClaim {
                        field: field.name,
                        begin,
                        end,
                        provenance: DeferredHistoryProvenance::ExistingRuntimeNumQueued,
                    });
                } else if field.repr.materialised() {
                    let bytes = field_bytes(columns, slot, field)?;
                    put(
                        &mut image,
                        &mut owned,
                        &mut conditional,
                        &mut field_for_offset,
                        begin,
                        &bytes,
                        field.name,
                        true,
                    );
                    claims.push(DeferredHistoryClaim {
                        field: field.name,
                        begin,
                        end,
                        provenance: DeferredHistoryProvenance::PdbGeneratedLeaderCols,
                    });
                } else {
                    return Err(DeferredHistoryFrontierError::UnsupportedGeneratedGap {
                        field: field.name,
                        offset: begin,
                        repr: field.repr,
                    });
                }
            }

            for (field, begin, bytes) in [
                (
                    "padding after num_queued",
                    QUEUE_PADDING_BEGIN,
                    authority.rows[slot].queue_padding.as_slice(),
                ),
                (
                    "padding before defeat_stamp",
                    CTW_PADDING_BEGIN,
                    authority.rows[slot].ctw_padding.as_slice(),
                ),
            ] {
                put(
                    &mut image,
                    &mut owned,
                    &mut conditional,
                    &mut field_for_offset,
                    begin,
                    bytes,
                    field,
                    true,
                );
                claims.push(DeferredHistoryClaim {
                    field,
                    begin,
                    end: begin + bytes.len(),
                    provenance: DeferredHistoryProvenance::ExplicitWalkPaddingAuthority,
                });
            }

            for range in base_row.covered_ranges() {
                let begin = range.begin.max(REG_BUILDINGS_BEGIN);
                let end = range.end.min(LEADER_FIXED_BODY_END);
                if begin >= end {
                    continue;
                }
                let runtime = base_row
                    .owned_slice(RuntimeCoveredRange { begin, end })
                    .expect("covered runtime range remains owned");
                for (relative, value) in runtime.iter().copied().enumerate() {
                    let offset = begin + relative;
                    runtime_owned[offset] = true;
                    duplicate_checked += usize::from(conditional[offset]);
                    if image[offset] != value {
                        return Err(DeferredHistoryFrontierError::RuntimeDisagreement {
                            slot,
                            offset,
                            field: field_for_offset[offset].unwrap_or("PDB padding"),
                            runtime: value,
                            conditional: image[offset],
                        });
                    }
                }
            }

            if let Some(relative) = owned[LEADER_FIXED_BODY_BEGIN..LEADER_FIXED_BODY_END]
                .iter()
                .position(|is_owned| !is_owned)
            {
                return Err(DeferredHistoryFrontierError::MissingFixedByte {
                    slot,
                    offset: LEADER_FIXED_BODY_BEGIN + relative,
                });
            }

            let diplomacy = base_row
                .owned_slice(RuntimeCoveredRange {
                    begin: LEADER_DIPLOMACY_BEGIN,
                    end: LEADER_DIPLOMACY_END,
                })
                .ok_or(DeferredHistoryFrontierError::MissingExistingRuntimeRange {
                    slot,
                    field: "Diplomacy[8]",
                    begin: LEADER_DIPLOMACY_BEGIN,
                    end: LEADER_DIPLOMACY_END,
                })?;
            put(
                &mut image,
                &mut owned,
                &mut conditional,
                &mut field_for_offset,
                LEADER_DIPLOMACY_BEGIN,
                diplomacy,
                "Diplomacy[8]",
                false,
            );
            claims.push(DeferredHistoryClaim {
                field: "Diplomacy[8]",
                begin: LEADER_DIPLOMACY_BEGIN,
                end: LEADER_DIPLOMACY_END,
                provenance: DeferredHistoryProvenance::ExistingRuntimeDiplomacy,
            });
        }

        let conditionally_admitted = conditional
            .iter()
            .zip(&runtime_owned)
            .enumerate()
            .filter(|&(offset, (is_conditional, is_runtime))| {
                let walked = offset < LEADER_FIXED_BODY_END
                    || (offset >= LEADER_DIPLOMACY_BEGIN && offset < LEADER_DIPLOMACY_END);
                walked && *is_conditional && !*is_runtime
            })
            .count();
        rows.push(DeferredHistoryRow {
            slot: slot as u8,
            active,
            image,
            claims,
            conditionally_admitted_walked_bytes: conditionally_admitted,
            duplicate_checked_walked_bytes: duplicate_checked,
        });
    }

    Ok(RuntimeLeadersDeferredHistoryFrontier {
        previous,
        rows: rows.try_into().expect("exact eight-row projection"),
    })
}
