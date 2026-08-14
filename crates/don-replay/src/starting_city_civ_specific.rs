//! Exact empty `Setup::build_civ_specific` cohort for ordinary starting Cities.
//!
//! `Setup::build_cities` calls `build_civ_specific` after every starting center, including
//! starting-town mode 1.  A missing call is harmless only when all seven retail
//! `LeaderData::has_tribe_bonus` probes produce no admitted `Leader::free_build` call.  This
//! module proves that empty schedule from the replay-selected Leader, the SHA-bound Rules
//! Tribe row, and the exact setup-time Leader state.  It refuses a non-empty schedule rather
//! than constructing partial Build/City state.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::leader_tribe_bonus_runtime::{
    has_tribe_bonus, CanonicalConquestRacialPowers, TribeBonusInputError, TribeBonusInputs,
    TribeBonusReceipt,
};

use crate::cities_runtime::{check_sim_owned_cities, CitiesChannelValue, CitiesRuntimeError};
use crate::city_build_constructor_runtime::{
    civ_specific_free_build_plan, CivSpecificBuildingsRequest,
};
use crate::groups_pre_pair_unit_authority::{
    replay_tribe_type_facts, PrePairUnitAuthorityError, UNIT_TYPE_FIRST,
};
use crate::initial::{ReplayByteSpan, SHIPPED_TYPES_SERIALIZED_BYTES};
use crate::replay::Replay;
use crate::rules_channel::RULES_BLOCK_BYTES;
use crate::setup_cities_builds::StartingSetupState;
use crate::world_owner_frontier::sha256;

pub const SETUP_BUILD_CITIES_VA: u32 = 0x005a_b910;
pub const SETUP_BUILD_CIV_SPECIFIC_VA: u32 = 0x005a_b760;
pub const LEADER_HAS_TRIBE_BONUS_VA: u32 = 0x006e_1370;
pub const LEADER_FREE_BUILD_VA: u32 = 0x006e_1400;

/// `LeaderData +0x04` after the inactive `Leader::init` reset and before setup scripts.
/// The nearby `0x02000000` OR at `0x006e3b28` targets `leader_flags +0x00`, not flags2.
pub const LEADER_INIT_FLAGS2: u32 = 0;
pub const CIV_SPECIFIC_BONUS_ORDER: [i32; 7] = [4, 22, 5, 16, 7, 10, 18];

const BONUS_5_RULE_OFFSET: usize = 0x5e0;
const BONUS_16_RULE_OFFSET: usize = 0x7d0;
const BONUS_7_RULE_OFFSET: usize = 0x640;
const BONUS_10_RULE_OFFSET: usize = 0x6c0;
const BONUS_18_RULE_OFFSET: usize = 0x810;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivSpecificBonusProbe {
    pub bonus: i32,
    pub granted: bool,
    pub receipt: TribeBonusReceipt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivSpecificRuleRead {
    pub bonus: i32,
    pub runtime_offset: usize,
    pub value: i32,
    pub source: ReplayByteSpan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyCivSpecificCityReceipt {
    pub owner: u8,
    pub replay_player_slot: u8,
    pub city_slot: i16,
    pub center_object_id: i16,
    pub tribe_selector: u8,
    pub player_tribe_source: ReplayByteSpan,
    pub tribe_default_bonus: i32,
    pub tribe_rules_source: ReplayByteSpan,
    pub leader_flags2: u32,
    pub city_num: i32,
    pub conquest_racial_powers: [u8; 3],
    pub probes: Vec<CivSpecificBonusProbe>,
    pub rule_reads: Vec<CivSpecificRuleRead>,
    pub free_build_types: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyStartingCivSpecificCohort {
    pub replay_payload_sha256: [u8; 32],
    pub rules_serialized_sha256: [u8; 32],
    pub cities_before: CitiesChannelValue,
    pub cities_after: CitiesChannelValue,
    pub cities: Vec<EmptyCivSpecificCityReceipt>,
    /// An empty schedule owns absence of a City mutation; it does not produce walked bytes.
    pub source_produced_city_bytes: u64,
    pub city_mutations: u32,
    pub installed_in_scoreboard: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartingCivSpecificError {
    PayloadSha256Mismatch,
    SetupPayloadSha256Mismatch,
    MissingRules,
    SetupCityCountMismatch {
        active_players: usize,
        receipts: usize,
    },
    ReplayPlayerSlotOutOfRange {
        slot: usize,
    },
    ReplayPlayerMismatch {
        slot: usize,
        expected_owner: u8,
        actual_owner: u8,
        present: bool,
    },
    ReplayPlayerSourceMismatch {
        slot: usize,
    },
    OwnerOutOfRange {
        owner: usize,
    },
    DuplicateOwner {
        owner: usize,
    },
    CityCountMismatch {
        owner: usize,
        count: i32,
    },
    CityJoinMismatch {
        owner: usize,
        slot: usize,
    },
    TribeSelectorOutOfRange {
        owner: usize,
        tribe: u8,
    },
    RulesConstantOutsideBlock {
        offset: usize,
    },
    RulesConstantOutsidePayload {
        offset: usize,
    },
    Tribe(PrePairUnitAuthorityError),
    Bonus(TribeBonusInputError),
    Cities(CitiesRuntimeError),
    NonEmptyFreeBuildSchedule {
        owner: usize,
        types: Vec<i32>,
    },
}

impl fmt::Display for StartingCivSpecificError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "starting City civ-specific cohort refused: {self:?}")
    }
}

impl std::error::Error for StartingCivSpecificError {}

impl From<PrePairUnitAuthorityError> for StartingCivSpecificError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::Tribe(value)
    }
}

impl From<TribeBonusInputError> for StartingCivSpecificError {
    fn from(value: TribeBonusInputError) -> Self {
        Self::Bonus(value)
    }
}

impl From<CitiesRuntimeError> for StartingCivSpecificError {
    fn from(value: CitiesRuntimeError) -> Self {
        Self::Cities(value)
    }
}

fn read_rule(
    payload: &[u8],
    serialized_offset: usize,
    bonus: i32,
    runtime_offset: usize,
) -> Result<CivSpecificRuleRead, StartingCivSpecificError> {
    if runtime_offset
        .checked_add(4)
        .is_none_or(|end| end > RULES_BLOCK_BYTES)
    {
        return Err(StartingCivSpecificError::RulesConstantOutsideBlock {
            offset: runtime_offset,
        });
    }
    let offset = serialized_offset
        .checked_add(1 + SHIPPED_TYPES_SERIALIZED_BYTES)
        .and_then(|offset| offset.checked_add(runtime_offset))
        .ok_or(StartingCivSpecificError::RulesConstantOutsidePayload {
            offset: runtime_offset,
        })?;
    let bytes = payload.get(offset..offset + 4).ok_or(
        StartingCivSpecificError::RulesConstantOutsidePayload {
            offset: runtime_offset,
        },
    )?;
    Ok(CivSpecificRuleRead {
        bonus,
        runtime_offset,
        value: i32::from_le_bytes(bytes.try_into().expect("four-byte slice")),
        source: ReplayByteSpan { offset, bytes: 4 },
    })
}

fn probe(
    inputs: TribeBonusInputs,
    bonus: i32,
    probes: &mut Vec<CivSpecificBonusProbe>,
) -> Result<bool, StartingCivSpecificError> {
    let (granted, receipt) = has_tribe_bonus(inputs, bonus)?;
    probes.push(CivSpecificBonusProbe {
        bonus,
        granted,
        receipt,
    });
    Ok(granted)
}

/// Prove that the complete `Setup::build_civ_specific` continuation issued no free Build.
///
/// The input setup must already contain one canonical fresh center per active owner.  Retail
/// reaches this function after `City::init` increments that owner's `city_num`, so the query
/// image is exactly: `city_num=1`, the still-zero `leader_flags2` reset, and the zero
/// conquest-power payload inherited from the constructor reset. A non-empty free-build plan
/// is an explicit refusal because its Build initializer and City link are not executed here.
pub fn prove_empty_starting_civ_specific_cohort(
    replay: &Replay,
    payload: &[u8],
    setup: &StartingSetupState,
) -> Result<EmptyStartingCivSpecificCohort, StartingCivSpecificError> {
    if sha256(payload) != replay.initial.payload_sha256 {
        return Err(StartingCivSpecificError::PayloadSha256Mismatch);
    }
    if setup.receipt.replay_payload_sha256 != replay.initial.payload_sha256 {
        return Err(StartingCivSpecificError::SetupPayloadSha256Mismatch);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(StartingCivSpecificError::MissingRules)?;
    if setup.receipt.active_players != setup.receipt.cities.len() {
        return Err(StartingCivSpecificError::SetupCityCountMismatch {
            active_players: setup.receipt.active_players,
            receipts: setup.receipt.cities.len(),
        });
    }

    let cities_before = check_sim_owned_cities(&setup.sim)?;
    let mut seen_owner = [false; 8];
    let mut receipts = Vec::with_capacity(setup.receipt.cities.len());
    for city in &setup.receipt.cities {
        let owner = usize::from(city.owner);
        if owner >= seen_owner.len() {
            return Err(StartingCivSpecificError::OwnerOutOfRange { owner });
        }
        if std::mem::replace(&mut seen_owner[owner], true) {
            return Err(StartingCivSpecificError::DuplicateOwner { owner });
        }
        let player_slot = usize::from(city.replay_player_slot);
        let player =
            replay.initial.info.players.get(player_slot).ok_or(
                StartingCivSpecificError::ReplayPlayerSlotOutOfRange { slot: player_slot },
            )?;
        if !player.present || player.who != city.owner {
            return Err(StartingCivSpecificError::ReplayPlayerMismatch {
                slot: player_slot,
                expected_owner: city.owner,
                actual_owner: player.who,
                present: player.present,
            });
        }
        let player_body = replay.initial.worldgen_sources.player_bodies[player_slot]
            .filter(|span| span.bytes == 0x39)
            .and_then(|span| {
                payload
                    .get(span.offset..span.end())
                    .map(|body| (span, body))
            })
            .ok_or(StartingCivSpecificError::ReplayPlayerSourceMismatch { slot: player_slot })?;
        let source_flags = u16::from_le_bytes([player_body.1[0x30], player_body.1[0x31]]);
        if source_flags != player.flags
            || player_body.1[0x32] != player.tribe
            || player_body.1[0x33] != player.who
        {
            return Err(StartingCivSpecificError::ReplayPlayerSourceMismatch { slot: player_slot });
        }
        let city_num = setup.sim.cities.count(owner);
        if city_num != 1 {
            return Err(StartingCivSpecificError::CityCountMismatch {
                owner,
                count: city_num,
            });
        }
        let city_slot = usize::try_from(city.city_slot).map_err(|_| {
            StartingCivSpecificError::CityJoinMismatch {
                owner,
                slot: usize::MAX,
            }
        })?;
        let current = setup
            .sim
            .cities
            .slots
            .get(owner)
            .and_then(|slots| slots.get(city_slot))
            .ok_or(StartingCivSpecificError::CityJoinMismatch {
                owner,
                slot: city_slot,
            })?;
        if current != &city.constructor.city
            || !current.active()
            || current.city != city.city_slot
            || current.o != city.constructor.object_id
            || current.who != city.owner as i8
        {
            return Err(StartingCivSpecificError::CityJoinMismatch {
                owner,
                slot: city_slot,
            });
        }

        let tribe = usize::from(player.tribe);
        if tribe >= 24 {
            return Err(StartingCivSpecificError::TribeSelectorOutOfRange {
                owner,
                tribe: player.tribe,
            });
        }
        // Type 50 is used only to traverse to the selected Tribe row.  The returned
        // nation-graft word is not consumed by this authority.
        let tribe_facts = replay_tribe_type_facts(payload, &rules, tribe, UNIT_TYPE_FIRST)?;
        let inputs = TribeBonusInputs {
            no_nation_powers: replay.initial.info.flags & 4 != 0,
            victory: replay.initial.info.settings.victory,
            city_num,
            tribe: i32::from(player.tribe),
            leader_flags2: LEADER_INIT_FLAGS2,
            conquest_racial_powers: CanonicalConquestRacialPowers::default(),
            tribe_default_bonus: Some(tribe_facts.tribe_id),
        };

        let mut probes = Vec::with_capacity(CIV_SPECIFIC_BONUS_ORDER.len());
        let bonus_4 = probe(inputs, 4, &mut probes)?;
        let bonus_22 = if bonus_4 {
            false
        } else {
            probe(inputs, 22, &mut probes)?
        };
        let bonus_5 = probe(inputs, 5, &mut probes)?;
        let bonus_16 = probe(inputs, 16, &mut probes)?;
        let bonus_7 = probe(inputs, 7, &mut probes)?;
        let bonus_10 = probe(inputs, 10, &mut probes)?;
        let bonus_18 = probe(inputs, 18, &mut probes)?;

        let mut rule_reads = Vec::new();
        let mut conditional_rule =
            |granted: bool, bonus: i32, offset: usize| -> Result<i32, StartingCivSpecificError> {
                if granted {
                    let read = read_rule(payload, rules.serialized_offset, bonus, offset)?;
                    let value = read.value;
                    rule_reads.push(read);
                    Ok(value)
                } else {
                    Ok(0)
                }
            };
        let request = CivSpecificBuildingsRequest {
            bonus_4,
            bonus_22,
            bonus_5,
            bonus_5_rule: conditional_rule(bonus_5, 5, BONUS_5_RULE_OFFSET)?,
            bonus_16,
            bonus_16_rule: conditional_rule(bonus_16, 16, BONUS_16_RULE_OFFSET)?,
            bonus_7,
            bonus_7_rule: conditional_rule(bonus_7, 7, BONUS_7_RULE_OFFSET)?,
            bonus_10,
            bonus_10_rule: conditional_rule(bonus_10, 10, BONUS_10_RULE_OFFSET)?,
            bonus_18,
            bonus_18_rule: conditional_rule(bonus_18, 18, BONUS_18_RULE_OFFSET)?,
        };
        let free_build_types = civ_specific_free_build_plan(request);
        if !free_build_types.is_empty() {
            return Err(StartingCivSpecificError::NonEmptyFreeBuildSchedule {
                owner,
                types: free_build_types,
            });
        }

        receipts.push(EmptyCivSpecificCityReceipt {
            owner: city.owner,
            replay_player_slot: city.replay_player_slot,
            city_slot: city.city_slot,
            center_object_id: city.constructor.object_id,
            tribe_selector: player.tribe,
            player_tribe_source: ReplayByteSpan {
                offset: player_body.0.offset + 0x32,
                bytes: 1,
            },
            tribe_default_bonus: tribe_facts.tribe_id,
            tribe_rules_source: tribe_facts.tribe_row,
            leader_flags2: LEADER_INIT_FLAGS2,
            city_num,
            conquest_racial_powers: CanonicalConquestRacialPowers::default().payload(),
            probes,
            rule_reads,
            free_build_types: Vec::new(),
        });
    }

    // No `Leader::free_build` call means this routine has no mutation of the City owner.
    let cities_after = check_sim_owned_cities(&setup.sim)?;
    debug_assert_eq!(cities_before, cities_after);
    Ok(EmptyStartingCivSpecificCohort {
        replay_payload_sha256: replay.initial.payload_sha256,
        rules_serialized_sha256: rules.serialized_sha256,
        cities_before,
        cities_after,
        cities: receipts,
        source_produced_city_bytes: 0,
        city_mutations: 0,
        installed_in_scoreboard: false,
    })
}
