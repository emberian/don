//! Replay-carried immutable content for the exact `UnitData::speed` authority.
//!
//! The admitted Rules section already serializes every shipped Unit type and the complete
//! `Constants +0..+0xd40` image. This module projects the fields consumed by
//! [`don_sim::systems::land_speed_authority`] directly from that SHA-bound section. It does not
//! substitute `MOVES`, use the repository live capture, or read a recorded checksum command.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;

use don_sim::systems::land_speed_authority::{
    LandSpeedConstants, LandSpeedContent, LandSpeedTypeFacts,
};

use crate::groups_pre_pair_unit_authority::{
    replay_unit_type_facts, PrePairUnitAuthorityError, ReplayUnitTypeFacts, UNIT_TYPE_FIRST,
    UNIT_TYPE_LAST,
};
use crate::initial::{InitialRules, SHIPPED_TYPES_SERIALIZED_BYTES};
use crate::replay::{load_payload, Replay};
use crate::world_owner_frontier::sha256;

pub const REPLAY_LAND_SPEED_CONTENT_SCHEMA: u64 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayLandSpeedContent {
    replay_file_sha256: [u8; 32],
    replay_payload_sha256: [u8; 32],
    rules_serialized_sha256: [u8; 32],
    revision: u64,
    constants: LandSpeedConstants,
    types: BTreeMap<i32, LandSpeedTypeFacts>,
}

impl ReplayLandSpeedContent {
    pub const fn replay_file_sha256(&self) -> [u8; 32] {
        self.replay_file_sha256
    }

    pub const fn replay_payload_sha256(&self) -> [u8; 32] {
        self.replay_payload_sha256
    }

    pub const fn rules_serialized_sha256(&self) -> [u8; 32] {
        self.rules_serialized_sha256
    }

    pub const fn constants(&self) -> LandSpeedConstants {
        self.constants
    }

    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    pub fn type_fact(&self, type_id: i32) -> Option<LandSpeedTypeFacts> {
        self.types.get(&type_id).copied()
    }
}

impl LandSpeedContent for ReplayLandSpeedContent {
    fn land_speed_revision(&self) -> u64 {
        self.revision
    }

    fn land_speed_composition_digest(&self) -> [u8; 32] {
        self.rules_serialized_sha256
    }

    fn land_speed_type(&self, type_id: i32) -> Option<LandSpeedTypeFacts> {
        self.type_fact(type_id)
    }

    fn land_speed_constants(&self) -> Option<LandSpeedConstants> {
        Some(self.constants)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayLandSpeedContentError {
    ReplayRead(String),
    PayloadRead(String),
    PayloadSha256Mismatch,
    MissingRules,
    RulesSpanOutsidePayload,
    ZeroRevision,
    UnitType(PrePairUnitAuthorityError),
}

impl fmt::Display for ReplayLandSpeedContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "replay land-speed content refused: {self:?}")
    }
}

impl std::error::Error for ReplayLandSpeedContentError {}

impl From<PrePairUnitAuthorityError> for ReplayLandSpeedContentError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::UnitType(value)
    }
}

fn read_constant(
    payload: &[u8],
    rules: &InitialRules,
    runtime_offset: usize,
) -> Result<i32, ReplayLandSpeedContentError> {
    let at = rules
        .serialized_offset
        .checked_add(1 + SHIPPED_TYPES_SERIALIZED_BYTES)
        .and_then(|at| at.checked_add(runtime_offset))
        .ok_or(ReplayLandSpeedContentError::RulesSpanOutsidePayload)?;
    let bytes = payload
        .get(at..at + 4)
        .ok_or(ReplayLandSpeedContentError::RulesSpanOutsidePayload)?;
    Ok(i32::from_le_bytes(
        bytes.try_into().expect("four-byte slice"),
    ))
}

fn constants(
    payload: &[u8],
    rules: &InitialRules,
) -> Result<LandSpeedConstants, ReplayLandSpeedContentError> {
    Ok(LandSpeedConstants {
        coord_scale: read_constant(payload, rules, 0x004)?,
        irq_spear_bonus: read_constant(payload, rules, 0x838)?,
        irq_mo_spear_bonus: read_constant(payload, rules, 0x83c)?,
        irq_hmo_spear_bonus: read_constant(payload, rules, 0x840)?,
        irq_emo_spear_bonus: read_constant(payload, rules, 0x844)?,
        alexander_napoleon_aura_256: read_constant(payload, rules, 0xb4c)?,
        spitamenes_stable_256: read_constant(payload, rules, 0xb78)?,
        porus_elephant_256: read_constant(payload, rules, 0xb7c)?,
        napoleon_siege_percent: read_constant(payload, rules, 0xbb0)?,
        charles_percent: read_constant(payload, rules, 0xbc0)?,
        blucher_stable_percent: read_constant(payload, rules, 0xbd4)?,
        hero_aura_speed: read_constant(payload, rules, 0xc50)?,
    })
}

fn land_speed_fact(facts: ReplayUnitTypeFacts) -> LandSpeedTypeFacts {
    LandSpeedTypeFacts {
        type_id: facts.type_index,
        from: facts.from_type,
        where_type: facts.where_type,
        graft: facts.graft,
        domain: facts.domain,
        unit_flags: facts.unit_flags,
        unit_flags2: facts.unit_flags2,
    }
}

/// Project exact speed content from a payload already decoded for `replay`.
///
/// The payload SHA must still equal the one proven by `Replay::open`; this overload exists so a
/// caller that is already walking Rules need not decompress the same recording twice.
pub fn produce_replay_land_speed_content_from_payload(
    replay: &Replay,
    payload: &[u8],
) -> Result<ReplayLandSpeedContent, ReplayLandSpeedContentError> {
    let replay_payload_sha256 = sha256(payload);
    if replay_payload_sha256 != replay.initial.payload_sha256 {
        return Err(ReplayLandSpeedContentError::PayloadSha256Mismatch);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(ReplayLandSpeedContentError::MissingRules)?;

    let mut types = BTreeMap::new();
    for type_id in UNIT_TYPE_FIRST..=UNIT_TYPE_LAST {
        let fact = land_speed_fact(replay_unit_type_facts(payload, &rules, type_id)?);
        if types.insert(type_id, fact).is_some() {
            unreachable!("the closed Unit TypeIndex range has unique keys");
        }
    }

    let constants = constants(payload, &rules)?;
    let section_end = rules
        .serialized_offset
        .checked_add(rules.serialized_bytes)
        .ok_or(ReplayLandSpeedContentError::RulesSpanOutsidePayload)?;
    let section = payload
        .get(rules.serialized_offset..section_end)
        .ok_or(ReplayLandSpeedContentError::RulesSpanOutsidePayload)?;
    let mut revision = 0xcbf2_9ce4_8422_2325u64 ^ REPLAY_LAND_SPEED_CONTENT_SCHEMA;
    for byte in section {
        revision = (revision ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    if revision == 0 {
        return Err(ReplayLandSpeedContentError::ZeroRevision);
    }

    let replay_bytes = std::fs::read(&replay.path)
        .map_err(|error| ReplayLandSpeedContentError::ReplayRead(error.to_string()))?;
    Ok(ReplayLandSpeedContent {
        replay_file_sha256: sha256(&replay_bytes),
        replay_payload_sha256,
        rules_serialized_sha256: rules.serialized_sha256,
        revision,
        constants,
        types,
    })
}

/// Load and project the recording's exact admitted Rules content.
pub fn produce_replay_land_speed_content(
    replay: &Replay,
) -> Result<ReplayLandSpeedContent, ReplayLandSpeedContentError> {
    let payload = load_payload(&replay.path)
        .map_err(|error| ReplayLandSpeedContentError::PayloadRead(error.to_string()))?;
    produce_replay_land_speed_content_from_payload(replay, &payload)
}
