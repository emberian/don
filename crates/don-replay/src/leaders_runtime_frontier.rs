// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact, fail-closed projection of current `don-sim` Leader owners into checksum channel 8.
//!
//! `LeaderData::walk_data` (`0x006D6750`) is not a struct hash.  It always walks the
//! eight-byte header and, for a valid row, walks a 26,914-byte fixed body, eight
//! `Diplomacy` rows, several length-bearing children, and a decoded
//! `LeaderDataEncrypt` transcript.  This module projects every current field owned by the
//! existing victory, step-8, taunt, production-AI, tech, and economy states, but refuses
//! to fill any gap with zero.  Consequently it is useful as a merge frontier and cannot
//! accidentally become a retail checksum producer before the remaining owners exist.
//!
//! The setup-time [`crate::leader_initial_prefix::InitialLeaderPrefix`] is retained as a
//! separate evidence ledger.  Its flags and diplomacy cells are mutable match state, and
//! even its four-dword identity span mixes immutable-looking fields with `defeated_by`
//! and `gov`.  No setup span is silently promoted into a live claim.

#![forbid(unsafe_code)]

use crate::leader_initial_prefix::{InitialLeaderPrefix, CHECKSUM_LEADER_SLOTS};
use don_sim::systems::{leader_process_taunt, leaders, victory_score};

pub const LEADER_WALK_DATA_VA: u32 = 0x006d_6750;
pub const LEADER_FIXED_BODY_BEGIN: usize = 8;
pub const LEADER_FIXED_BODY_END: usize = 0x692a;
pub const LEADER_DIPLOMACY_BEGIN: usize = 0x692c;
pub const LEADER_DIPLOMACY_BYTES: usize = 0x5c;
pub const LEADER_PERSONALITY_BEGIN: usize = 0x6dd4;
pub const LEADER_PERSONALITY_END: usize = 0x6e34;
pub const LEADER_DATA_SIZE: usize = 0x6ee4;
pub const LEADER_DATA_ENCRYPT_DWORDS: usize = 62;

const TECH_BITS: usize = 806;
const TECH_BYTES: usize = TECH_BITS.div_ceil(8);
const TECH_PAYLOAD_BEGIN: usize = 0x6c18;
const TECH_AT_START_PAYLOAD_BEGIN: usize = 0x6c8c;
const RARE_EFFECTIVE_PAYLOAD_BEGIN: usize = 0x6da4;
const RARE_A_PAYLOAD_BEGIN: usize = 0x6db8;
const RARE_B_PAYLOAD_BEGIN: usize = 0x6dcc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLeaderProvenance {
    VictoryScore,
    Step8,
    TauntRuntime,
    ProductionAi,
    TechRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLeaderClaim {
    pub begin: usize,
    pub end: usize,
    pub provenance: RuntimeLeaderProvenance,
}

impl RuntimeLeaderClaim {
    pub const fn bytes(self) -> usize {
        self.end - self.begin
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeCoveredRange {
    pub begin: usize,
    pub end: usize,
}

impl RuntimeCoveredRange {
    pub const fn bytes(self) -> usize {
        self.end - self.begin
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeadersRuntimeError {
    VictorySlotCount {
        count: usize,
    },
    SlotIdentity {
        slot: usize,
        victory_who: i32,
        step8_who: i32,
    },
    SetupRosterDrift {
        slot: usize,
        setup_valid: bool,
        runtime_valid: bool,
    },
    SourceLength {
        slot: usize,
        source: &'static str,
        expected: usize,
        got: usize,
    },
    SourceDisagreement {
        slot: usize,
        offset: usize,
        existing: u8,
        incoming: u8,
        incoming_provenance: RuntimeLeaderProvenance,
    },
    EconomyDisagreement {
        slot: usize,
        field: &'static str,
        index: usize,
        victory: i32,
        step8: i32,
    },
    RareDisagreement {
        slot: usize,
        victory: u64,
        step8: [u8; leaders::RARE_MASK_BYTES],
    },
}

impl std::fmt::Display for LeadersRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Leader runtime projection refused: {self:?}")
    }
}

impl std::error::Error for LeadersRuntimeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeadersWalkBoundary {
    Header {
        slot: usize,
        offset: usize,
    },
    FixedBody {
        slot: usize,
        offset: usize,
    },
    /// Reached only after the complete fixed body becomes owned.  This revision does not
    /// pretend that a sparse child payload also owns its length-bearing header.
    DynamicChildren {
        slot: usize,
        child: &'static str,
    },
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeadersWalkFrontier {
    pub checksum: u32,
    pub bytes_walked: u64,
    pub boundary: LeadersWalkBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeaderRow {
    pub slot: u8,
    pub active: bool,
    pub setup_claimed_walked_bytes: usize,
    image: Vec<u8>,
    owned: Vec<bool>,
    claims: Vec<RuntimeLeaderClaim>,
    /// The exact plaintext dwords passed to `DataWalk::walk` by
    /// `LeaderDataEncrypt::walk_data`.  The engine decodes each stored word before the
    /// visitor sees it, so these are deliberately separate from the Leader image.
    econ_plaintext: [Option<i32>; LEADER_DATA_ENCRYPT_DWORDS],
}

impl RuntimeLeaderRow {
    pub fn claims(&self) -> &[RuntimeLeaderClaim] {
        &self.claims
    }

    pub fn covered_ranges(&self) -> Vec<RuntimeCoveredRange> {
        let mut ranges = Vec::new();
        let mut begin = None;
        for (offset, owned) in self.owned.iter().copied().enumerate() {
            match (begin, owned) {
                (None, true) => begin = Some(offset),
                (Some(start), false) => {
                    ranges.push(RuntimeCoveredRange {
                        begin: start,
                        end: offset,
                    });
                    begin = None;
                }
                _ => {}
            }
        }
        if let Some(start) = begin {
            ranges.push(RuntimeCoveredRange {
                begin: start,
                end: self.owned.len(),
            });
        }
        ranges
    }

    pub fn owned_slice(&self, range: RuntimeCoveredRange) -> Option<&[u8]> {
        (range.begin <= range.end
            && range.end <= self.image.len()
            && self.owned[range.begin..range.end]
                .iter()
                .all(|owned| *owned))
        .then(|| &self.image[range.begin..range.end])
    }

    pub fn runtime_claimed_walked_bytes(&self) -> usize {
        if self.active {
            self.owned.iter().filter(|owned| **owned).count()
                + self
                    .econ_plaintext
                    .iter()
                    .filter(|value| value.is_some())
                    .count()
                    * 4
        } else {
            self.owned[..LEADER_FIXED_BODY_BEGIN]
                .iter()
                .filter(|owned| **owned)
                .count()
        }
    }

    pub fn econ_plaintext(&self) -> &[Option<i32>; LEADER_DATA_ENCRYPT_DWORDS] {
        &self.econ_plaintext
    }

    pub fn first_unowned_fixed_offset(&self) -> Option<usize> {
        self.active
            .then(|| {
                self.owned[LEADER_FIXED_BODY_BEGIN..LEADER_FIXED_BODY_END]
                    .iter()
                    .position(|owned| !owned)
                    .map(|relative| LEADER_FIXED_BODY_BEGIN + relative)
            })
            .flatten()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLeadersFrontier {
    pub rows: [RuntimeLeaderRow; CHECKSUM_LEADER_SLOTS],
    pub setup_claimed_walked_bytes: usize,
    pub setup_human_only_first_checksum_candidate: bool,
}

impl RuntimeLeadersFrontier {
    pub fn runtime_claimed_walked_bytes(&self) -> usize {
        self.rows
            .iter()
            .map(RuntimeLeaderRow::runtime_claimed_walked_bytes)
            .sum()
    }

    /// Execute the longest checksum prefix for which every byte is current and owned.
    /// Call boundaries do not alter Adler-32, so stopping inside retail's large fixed-body
    /// call is still an exact prefix checkpoint.  It is not a channel match.
    pub fn walk_frontier(&self) -> LeadersWalkFrontier {
        let mut checksum = 1u32;
        let mut bytes_walked = 0u64;
        for (slot, row) in self.rows.iter().enumerate() {
            let header_end = row.owned[..LEADER_FIXED_BODY_BEGIN]
                .iter()
                .position(|owned| !owned)
                .unwrap_or(LEADER_FIXED_BODY_BEGIN);
            if header_end != 0 {
                checksum = don_sim::checksum::adler32(checksum, &row.image[..header_end]);
                bytes_walked += header_end as u64;
            }
            if header_end != LEADER_FIXED_BODY_BEGIN {
                return LeadersWalkFrontier {
                    checksum,
                    bytes_walked,
                    boundary: LeadersWalkBoundary::Header {
                        slot,
                        offset: header_end,
                    },
                };
            }
            if !row.active {
                continue;
            }

            let body_owned = &row.owned[LEADER_FIXED_BODY_BEGIN..LEADER_FIXED_BODY_END];
            let body_end = body_owned
                .iter()
                .position(|owned| !owned)
                .map(|relative| LEADER_FIXED_BODY_BEGIN + relative)
                .unwrap_or(LEADER_FIXED_BODY_END);
            if body_end != LEADER_FIXED_BODY_BEGIN {
                checksum = don_sim::checksum::adler32(
                    checksum,
                    &row.image[LEADER_FIXED_BODY_BEGIN..body_end],
                );
                bytes_walked += (body_end - LEADER_FIXED_BODY_BEGIN) as u64;
            }
            if body_end != LEADER_FIXED_BODY_END {
                return LeadersWalkFrontier {
                    checksum,
                    bytes_walked,
                    boundary: LeadersWalkBoundary::FixedBody {
                        slot,
                        offset: body_end,
                    },
                };
            }

            return LeadersWalkFrontier {
                checksum,
                bytes_walked,
                boundary: LeadersWalkBoundary::DynamicChildren {
                    slot,
                    child: "Diplomacy[8] and length-bearing LeaderData children",
                },
            };
        }
        LeadersWalkFrontier {
            checksum,
            bytes_walked,
            boundary: LeadersWalkBoundary::Complete,
        }
    }

    /// A full value is issued only when the traversal is complete.  With any active row,
    /// this revision necessarily returns the frontier instead of a plausible checksum.
    pub fn checksum(&self) -> Result<(u32, u64), LeadersWalkFrontier> {
        let frontier = self.walk_frontier();
        if frontier.boundary == LeadersWalkBoundary::Complete {
            Ok((frontier.checksum, frontier.bytes_walked))
        } else {
            Err(frontier)
        }
    }
}

struct RowBuilder {
    slot: usize,
    image: Vec<u8>,
    owned: Vec<bool>,
    claims: Vec<RuntimeLeaderClaim>,
}

impl RowBuilder {
    fn new(slot: usize) -> Self {
        Self {
            slot,
            image: vec![0; LEADER_DATA_SIZE],
            owned: vec![false; LEADER_DATA_SIZE],
            claims: Vec::new(),
        }
    }

    fn put(
        &mut self,
        begin: usize,
        bytes: &[u8],
        provenance: RuntimeLeaderProvenance,
    ) -> Result<(), LeadersRuntimeError> {
        let end = begin + bytes.len();
        debug_assert!(end <= self.image.len());
        for (relative, incoming) in bytes.iter().copied().enumerate() {
            let offset = begin + relative;
            if self.owned[offset] && self.image[offset] != incoming {
                return Err(LeadersRuntimeError::SourceDisagreement {
                    slot: self.slot,
                    offset,
                    existing: self.image[offset],
                    incoming,
                    incoming_provenance: provenance,
                });
            }
            self.image[offset] = incoming;
            self.owned[offset] = true;
        }
        self.claims.push(RuntimeLeaderClaim {
            begin,
            end,
            provenance,
        });
        Ok(())
    }

    fn put_i32(
        &mut self,
        offset: usize,
        value: i32,
        provenance: RuntimeLeaderProvenance,
    ) -> Result<(), LeadersRuntimeError> {
        self.put(offset, &value.to_le_bytes(), provenance)
    }

    fn put_u32(
        &mut self,
        offset: usize,
        value: u32,
        provenance: RuntimeLeaderProvenance,
    ) -> Result<(), LeadersRuntimeError> {
        self.put(offset, &value.to_le_bytes(), provenance)
    }

    fn put_i32s(
        &mut self,
        offset: usize,
        values: &[i32],
        provenance: RuntimeLeaderProvenance,
    ) -> Result<(), LeadersRuntimeError> {
        for (index, value) in values.iter().copied().enumerate() {
            self.put_i32(offset + index * 4, value, provenance)?;
        }
        Ok(())
    }

    fn put_u16s(
        &mut self,
        offset: usize,
        values: &[u16],
        provenance: RuntimeLeaderProvenance,
    ) -> Result<(), LeadersRuntimeError> {
        for (index, value) in values.iter().copied().enumerate() {
            self.put(offset + index * 2, &value.to_le_bytes(), provenance)?;
        }
        Ok(())
    }
}

fn check_len(
    slot: usize,
    source: &'static str,
    got: usize,
    expected: usize,
) -> Result<(), LeadersRuntimeError> {
    if got == expected {
        Ok(())
    } else {
        Err(LeadersRuntimeError::SourceLength {
            slot,
            source,
            expected,
            got,
        })
    }
}

fn pack_bits(bits: &[bool]) -> [u8; TECH_BYTES] {
    let mut packed = [0u8; TECH_BYTES];
    for (bit, present) in bits.iter().copied().enumerate() {
        if present {
            packed[bit >> 3] |= 1u8 << (bit & 7);
        }
    }
    packed
}

fn put_init_diplomacy(
    builder: &mut RowBuilder,
    row: &don_sim::systems::leader_init_diplomacy_loop::LeaderInitDiplomacyRow,
) -> Result<(), LeadersRuntimeError> {
    let provenance = RuntimeLeaderProvenance::VictoryScore;
    for (offset, values) in [
        (0x094, &row.treaties),
        (0x0b4, &row.agendas),
        (0x0d4, &row.good_deeds),
        (0x0f4, &row.attack_stamp),
        (0x114, &row.raid_stamp),
        (0x134, &row.capital_stamp),
        (0x154, &row.ally_stamp),
        (0x174, &row.tribute_stamp),
        (0x194, &row.gift_stamp),
        (0x1b4, &row.hire_stamp),
        (0x1d4, &row.hire_who),
        (0x210, &row.aggression),
        (0x230, &row.strong),
        (0x250, &row.weak),
        (0x270, &row.dow),
        (0x290, &row.invaders),
        (0x2b0, &row.broke_alliance),
        (0x2d0, &row.made_peace),
    ] {
        builder.put_i32s(offset, values, provenance)?;
    }
    builder.put_i32(0x2f0, row.got_diplo_message, provenance)?;
    for (offset, values) in [
        (0x2f4, &row.last_spoke),
        (0x314, &row.counteroffer),
        (0x334, &row.tribute_demanded),
        (0x354, &row.last_taunt),
        (0x374, &row.taunt_frame),
    ] {
        builder.put_i32s(offset, values, provenance)?;
    }
    builder.put(0x6929, &[row.ally_mask], provenance)
}

fn put_taunt(
    builder: &mut RowBuilder,
    row: &leader_process_taunt::TauntLeaderState,
) -> Result<(), LeadersRuntimeError> {
    let provenance = RuntimeLeaderProvenance::TauntRuntime;
    builder.put_i32s(0x194, &row.gift_stamp, provenance)?;
    builder.put_i32s(0x354, &row.last_taunt, provenance)?;
    builder.put_i32s(0x374, &row.last_taunt_frame, provenance)?;
    builder.put_i32s(0x498, &row.tributes, provenance)?;
    builder.put_i32s(
        0x794,
        &[
            row.mods.wonder,
            row.mods.ground,
            row.mods.air,
            row.mods.sea,
            row.mods.infra,
            row.mods.defense,
        ],
        provenance,
    )?;
    for (target, diplomacy) in row.dip.iter().enumerate() {
        let begin = LEADER_DIPLOMACY_BEGIN + target * LEADER_DIPLOMACY_BYTES;
        builder.put_i32(begin, diplomacy.agree, provenance)?;
        builder.put_i32(begin + 4, diplomacy.any_offer, provenance)?;
        builder.put_i32(begin + 8, diplomacy.treaty, provenance)?;
        builder.put_i32s(begin + 0x0c, &diplomacy.offers, provenance)?;
        builder.put_i32s(begin + 0x24, &diplomacy.dows, provenance)?;
        builder.put_i32s(begin + 0x3c, &diplomacy.attacks, provenance)?;
    }
    debug_assert!(LEADER_PERSONALITY_BEGIN + 0x18 + 4 <= LEADER_PERSONALITY_END);
    builder.put_i32(
        LEADER_PERSONALITY_BEGIN + 0x18,
        row.personality_raid,
        provenance,
    )
}

fn project_econ(
    slot: usize,
    victory: &victory_score::LeaderState,
    step8: &leaders::Leader,
) -> Result<[Option<i32>; LEADER_DATA_ENCRYPT_DWORDS], LeadersRuntimeError> {
    for resource in 0..victory_score::NUM_RESOURCES {
        for (field, victory_value, step8_value) in [
            (
                "bucket",
                victory.economy.bucket[resource],
                step8.econ.stockpile[resource],
            ),
            (
                "income",
                victory.economy.income[resource],
                step8.econ.displayed[resource],
            ),
        ] {
            if victory_value != step8_value {
                return Err(LeadersRuntimeError::EconomyDisagreement {
                    slot,
                    field,
                    index: resource,
                    victory: victory_value,
                    step8: step8_value,
                });
            }
        }
    }

    let mut out = [None; LEADER_DATA_ENCRYPT_DWORDS];
    for resource in 0..victory_score::NUM_RESOURCES {
        let base = resource * 9;
        out[base] = Some(step8.econ.stockpile[resource]);
        out[base + 1] = Some(step8.econ.accumulator[resource]);
        out[base + 2] = Some(step8.econ.commerce_cap[resource]);
        out[base + 3] = Some(step8.econ.capped_flag[resource]);
        out[base + 4] = Some(step8.econ.gross[resource]);
        out[base + 5] = Some(step8.econ.expense[resource]);
        out[base + 6] = Some(step8.econ.displayed[resource]);
        // `LeaderDataEncrypt::rate[resource]` is not represented by LeaderEcon.
        out[base + 8] = Some(step8.econ.breakdown[resource]);
    }
    // After 6 * 9 resource values: cap[6], epoch[4], ages, epochs, discovered.
    // Only `ages` has an unambiguous current owner in LeaderEcon (`age_alt`).
    out[59] = Some(step8.econ.age_alt);
    Ok(out)
}

fn project_row(
    slot: usize,
    setup: &crate::leader_initial_prefix::InitialLeaderRow,
    victory: &victory_score::LeaderState,
    step8: &leaders::Leader,
) -> Result<RuntimeLeaderRow, LeadersRuntimeError> {
    if victory.who != slot as i32 || step8.slot != slot as i32 {
        return Err(LeadersRuntimeError::SlotIdentity {
            slot,
            victory_who: victory.who,
            step8_who: step8.slot,
        });
    }
    let runtime_valid = victory.leader_flags & victory_score::leader_flag::VALID != 0;
    if runtime_valid != setup.active {
        return Err(LeadersRuntimeError::SetupRosterDrift {
            slot,
            setup_valid: setup.active,
            runtime_valid,
        });
    }

    check_len(
        slot,
        "num_buildings",
        victory.num_buildings.len(),
        victory_score::NUM_BUILD_SLOTS,
    )?;
    check_len(
        slot,
        "num_units",
        victory.num_units.len(),
        victory_score::NUM_UNIT_SLOTS,
    )?;
    check_len(
        slot,
        "num_queued",
        victory.num_queued.len(),
        victory_score::NUM_TYPES,
    )?;
    check_len(slot, "has_tech", victory.has_tech.len(), TECH_BITS)?;
    check_len(
        slot,
        "tech_at_start",
        victory.tech_at_start.len(),
        TECH_BYTES,
    )?;

    let mut builder = RowBuilder::new(slot);
    let v = RuntimeLeaderProvenance::VictoryScore;
    builder.put_i32(0x000, victory.leader_flags, v)?;
    builder.put_i32(0x004, victory.leader_flags2, v)?;
    builder.put_i32(0x008, victory.who, v)?;
    builder.put_i32(0x010, victory.defeated_by, v)?;
    builder.put_i32s(
        0x018,
        &[
            victory.score,
            victory.score_explored,
            victory.score_territory,
            victory.score_units,
            victory.score_units_2,
            victory.score_buildings,
            victory.score_economy,
            victory.score_pop,
            victory.score_unit_upgrades,
            victory.score_research,
            victory.score_wonders,
            victory.score_combat,
        ],
        v,
    )?;
    builder.put_i32(0x050, victory.multi_diff, v)?;
    builder.put_i32s(0x074, &victory.diplos, v)?;
    put_init_diplomacy(&mut builder, &victory.init_diplomacy)?;
    for (offset, value) in [
        (0x414, victory.lost_capital_stamp),
        (0x418, victory.lost_capital_timer),
        (0x440, victory.popwin_stamp),
        (0x444, victory.popwin_timer),
        (0x448, victory.wonderwin_stamp),
        (0x44c, victory.wonderwin_timer),
        (0x7d8, victory.victory_type),
        (0x7dc, victory.defeat_type),
        (0x7e4, victory.population_cap),
        (0x7ec, victory.misery),
        (0x7f8, victory.give_attrition_disabled),
        (0x7fc, victory.take_attrition_disabled),
        (0x800, victory.neutral_attrition),
        (0x804, victory.building_attrition_disabled),
        (0x9d8, victory.territory),
    ] {
        builder.put_i32(offset, value, v)?;
    }
    builder.put_u16s(0x555e, &victory.num_buildings, v)?;
    builder.put_u16s(0x5762, &victory.num_units, v)?;
    builder.put_u16s(0x5a22, &victory.num_queued, v)?;

    let s = RuntimeLeaderProvenance::Step8;
    builder.put_u32(0x000, step8.flags, s)?;
    builder.put_i32(0x008, step8.slot, s)?;
    builder.put_i32s(0x074, &step8.diplo, s)?;
    builder.put_i32s(0x394, &step8.taunt_kind, s)?;
    builder.put_i32s(0x3b4, &step8.taunt_arg, s)?;
    builder.put_i32s(0x3d4, &step8.taunt_frame, s)?;
    builder.put_i32(0x3f8, step8.city_num, s)?;
    for (index, (value_offset, frozen_offset)) in [(0x414, 0x418), (0x440, 0x444), (0x448, 0x44c)]
        .into_iter()
        .enumerate()
    {
        builder.put_i32(value_offset, step8.timers[index].value, s)?;
        builder.put_i32(frozen_offset, step8.timers[index].frozen, s)?;
    }
    builder.put_i32(0x41c, step8.retake_scale, s)?;
    builder.put_i32(0x7e4, step8.pop_cap, s)?;
    builder.put_i32(0x7e8, step8.pop_issues, s)?;
    builder.put_i32(0x7f0, step8.attrition, s)?;
    builder.put_u32(0x7f4, step8.anti_attrition.to_bits(), s)?;
    builder.put_i32(0x7f8, step8.attrition_off, s)?;
    builder.put_i32(0x7fc, step8.anti_attrition_off, s)?;
    builder.put_i32(0x800, step8.neutral_attrition, s)?;
    builder.put_i32(0x804, step8.building_attrition_off, s)?;
    builder.put_i32(0x9d4, step8.explored, s)?;
    builder.put_i32(0x9f4, step8.frame_counter_b, s)?;
    builder.put_i32(0xa4c, step8.event_frame.frame_battle, s)?;
    for (offset, value) in [
        (0xa50, step8.event_frame.average_death_rate),
        (0xa52, step8.event_frame.average_kill_rate),
        (0xa54, step8.event_frame.average_damage_rate),
        (0xa56, step8.event_frame.average_hit_rate),
        (0xa58, step8.event_frame.deaths_current_frame),
        (0xa5a, step8.event_frame.kills_current_frame),
        (0xa5c, step8.event_frame.hits_current_frame),
        (0xa5e, step8.event_frame.damage_current_frame),
        (0xa60, step8.event_frame.deaths_fifteen_seconds),
        (0xa62, step8.event_frame.kills_fifteen_seconds),
        (0xa64, step8.event_frame.hits_fifteen_seconds),
        (0xa66, step8.event_frame.damage_fifteen_seconds),
    ] {
        builder.put(offset, &value.to_le_bytes(), s)?;
    }
    builder.put(0x6900, &[step8.conquest_byte], s)?;

    let ai = RuntimeLeaderProvenance::ProductionAi;
    builder.put_u32(0x004, step8.ai.flags2, ai)?;
    for (offset, value) in [
        (0x788, step8.ai.production_step),
        (0x78c, step8.ai.prod_script_run),
        (0x790, step8.ai.script_step),
        (0x940, step8.ai.control),
        (0x9e0, step8.ai.effective_pop),
    ] {
        builder.put_i32(offset, value, ai)?;
    }
    put_taunt(&mut builder, &step8.taunt)?;

    let victory_rare = victory.rare;
    let rare_bytes = victory_rare.to_le_bytes();
    if victory_rare >> leaders::RareMask::BITS != 0
        || rare_bytes[..leaders::RARE_MASK_BYTES] != step8.rare_effective.bytes
    {
        return Err(LeadersRuntimeError::RareDisagreement {
            slot,
            victory: victory_rare,
            step8: step8.rare_effective.bytes,
        });
    }
    let tech = RuntimeLeaderProvenance::TechRuntime;
    builder.put(TECH_PAYLOAD_BEGIN, &pack_bits(&victory.has_tech), tech)?;
    builder.put(TECH_AT_START_PAYLOAD_BEGIN, &victory.tech_at_start, tech)?;
    builder.put(
        RARE_EFFECTIVE_PAYLOAD_BEGIN,
        &step8.rare_effective.bytes,
        tech,
    )?;
    builder.put(RARE_A_PAYLOAD_BEGIN, &step8.rare_a.bytes, tech)?;
    builder.put(RARE_B_PAYLOAD_BEGIN, &step8.rare_b.bytes, tech)?;

    let econ_plaintext = project_econ(slot, victory, step8)?;
    Ok(RuntimeLeaderRow {
        slot: slot as u8,
        active: runtime_valid,
        setup_claimed_walked_bytes: setup.claimed_walked_bytes(),
        image: builder.image,
        owned: builder.owned,
        claims: builder.claims,
        econ_plaintext,
    })
}

/// Bind the replay setup evidence to the two current `don-sim` Leader owners.
///
/// The adapter requires duplicate owners to agree byte-for-byte.  It never mutates either
/// source and never installs itself into the replay harness; the future hook is a
/// `SimState` direct-channel producer after every missing walk operation is owned.
pub fn bind_live(
    setup: &InitialLeaderPrefix,
    victory: &victory_score::Leaders,
    step8: &leaders::Leaders,
) -> Result<RuntimeLeadersFrontier, LeadersRuntimeError> {
    if victory.slots.len() != CHECKSUM_LEADER_SLOTS {
        return Err(LeadersRuntimeError::VictorySlotCount {
            count: victory.slots.len(),
        });
    }
    let mut rows = Vec::with_capacity(CHECKSUM_LEADER_SLOTS);
    for slot in 0..CHECKSUM_LEADER_SLOTS {
        rows.push(project_row(
            slot,
            &setup.rows[slot],
            &victory.slots[slot],
            &step8.leaders[slot],
        )?);
    }
    Ok(RuntimeLeadersFrontier {
        rows: rows.try_into().expect("exact eight-row projection"),
        setup_claimed_walked_bytes: setup.claimed_walked_bytes(),
        setup_human_only_first_checksum_candidate: setup.is_human_only_first_checksum_candidate(),
    })
}
