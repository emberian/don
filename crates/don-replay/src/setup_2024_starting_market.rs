//! Golden 2024 Dutch starting-Market chronology and City after-image authority.
//!
//! `Setup::build_cities` calls `Setup::build_civ_specific` after the owner-0 Village and
//! before `Setup::build_units`. The golden replay selects Dutch bonus 22, so retail issues
//! exactly one `Leader::produce_building(436, 2000, 0)` call. This module derives that call
//! from replay/Rules bytes, joins the exact Market `blocked_site` footprint owner, executes exact
//! `find_friends` Object lookups under explicit scratch authority, consumes the bounded found-Build
//! type predicates from canonical type/production owners, and binds a
//! supported-retail pre/post capture to every City checksum byte written by the success suffix.
//! It never uses a recorded checksum and does not manufacture the missing generated World needed
//! to select the site.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::rng::Random;
use don_sim::systems::air_patrol_building_search_frontier::WORLD_CELL_SEARCH_OFFSETS;
use don_sim::systems::bhs_type_table::{TypeBody, TypeBuiltinState, TypeDomain};
use don_sim::systems::build_type_find_friends::{
    produce_build_type_find_friends_prefix, BuildTypeFindFriendsError, BuildTypeFindFriendsReceipt,
    BuildTypeFindFriendsRequest, BuildTypeFindFriendsStop, ObjectsFindBuildingPlacedAtBoundary,
    BUILD_TYPE_FIND_FRIENDS_FIRST_OBJECT_CALL_VA, BUILD_TYPE_FIND_FRIENDS_VA,
    OBJECTS_FIND_BUILDING_PLACED_AT_VA, OBJECTS_SELECTED_OWNER_OFFSET,
};
use don_sim::systems::leader_market_build_accounting::{
    MarketLeaderAccountingError, MarketLeaderAccountingReceipt, MarketLeaderRegionAuthority,
};
use don_sim::systems::leader_produce_building_blocked_site_prefix::{
    apply_sim_leader_produce_building_blocked_site_prefix,
    apply_sim_leader_produce_building_blocked_site_raw_zero_footprint,
    apply_sim_leader_produce_building_market_blocked_location_tail,
    plan_sim_leader_produce_building_market_blocked_location_request,
    LeaderProduceBuildingBlockedSitePrefixError,
    LeaderProduceBuildingBlockedSiteRawZeroFootprintError,
    LeaderProduceBuildingBlockedSiteRawZeroFootprintReceipt,
    LeaderProduceBuildingFindFriendsBoundary, LeaderProduceBuildingMarketBlockedLocationError,
    LeaderProduceBuildingMarketBlockedLocationReceipt,
    LeaderProduceBuildingMarketBlockedLocationRequest,
    LeaderProduceBuildingMarketBlockedLocationRequestError,
    BUILD_TYPE_BLOCKED_LOCATION_GET_TREGION_CALL_VA, LEADER_PRODUCE_BUILDING_FINE_RANDOM_CALL_VA,
    MARKET_BUILD_FLAGS, RANDOM_GET_VA,
};
use don_sim::systems::leader_produce_building_candidate_prefix::{
    LeaderProduceBuildingCandidatePrefixReceipt, LeaderProduceBuildingCandidatePrefixStatus,
};
use don_sim::systems::leader_produce_building_prefix::{
    LeaderProduceBuildingPrefixRequest, LEADER_PRODUCE_BUILDING_VA,
};
use don_sim::systems::leader_tribe_bonus_runtime::{
    has_tribe_bonus, CanonicalConquestRacialPowers, TribeBonusInputError, TribeBonusInputs,
    TribeBonusReceipt,
};
use don_sim::systems::map_terrain::{Coord, WCoord, WorldTregionError, WorldTregionQuery};
use don_sim::systems::objects_find_building_placed_at::{
    execute_objects_find_building_placed_at, ObjectsFindBuildingPlacedAtExecuteError,
    ObjectsFindBuildingPlacedAtReceipt, ObjectsFindBuildingPlacedAtStop,
    ObjectsFindBuildingPlacedAtWallBoundary, ObjectsSelectedOwnerAuthority,
    ObjectsSelectedOwnerCommitError, ObjectsSpatialBand,
};
use don_sim::systems::production::{
    flag,
    runtime::{LiveProductionRuntime, LiveTypeClass},
    Footprint,
};
use don_sim::systems::save_load::SaveError;
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::systems::tech_cities::CityRecord;
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;
use don_sim::world::WorldObjectIdentity;

use crate::cities_runtime::{check_sim_owned_cities, CitiesChannelValue, CitiesRuntimeError};
use crate::city_build_constructor_runtime::{
    civ_specific_free_build_plan, CivSpecificBuildingsRequest,
};
use crate::groups_pre_pair_unit_authority::{
    replay_build_type_facts, replay_tribe_type_facts, PrePairUnitAuthorityError,
    ReplayBuildTypeFacts, ReplayTribeTypeFacts, UNIT_TYPE_FIRST,
};
use crate::initial::ReplayByteSpan;
use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame379::{
    frame379_setup_snapshot_sha256, MAP_SIZE, MAP_STYLE, OWNER, PLAY, REPLAY_FILE_SHA256,
    REPLAY_SEED, TRIBE,
};
use crate::starting_city_civ_specific::{CIV_SPECIFIC_BONUS_ORDER, LEADER_INIT_FLAGS2};
use crate::world_owner_frontier::sha256;

pub const DUTCH_STARTING_MARKET_O: i32 = 2001;
pub const DUTCH_STARTING_MARKET_TYPE: i32 = 436;
pub const STARTING_VILLAGE_O: i32 = 2000;
pub const STARTING_CITY_SLOT: i16 = 0;
pub const BUILD_ACTIVATE_VA: u32 = 0x0062_3e20;
pub const CITY_FLAGS_OFFSET: u16 = 4;
pub const CITY_FILLED_OFFSET: u16 = 100;
pub const CITY_SPACE_OFFSET: u16 = 105;
pub const MARKET_CITY_FLAG: u16 = 0x0800;
pub const FRESH_CITY_FLAGS: u16 = 0x4011;
pub const MARKET_CITY_FLAGS: u16 = FRESH_CITY_FLAGS | MARKET_CITY_FLAG;
pub const MARKET_BUILD_QUEUE_SLOTS: usize = 20;
pub const MARKET_DOMAIN: i32 = 0;
/// First instruction after the initial `ObjectsData::find_building_placed_at` call returns to
/// `BuildTypeData::find_friends`.
pub const MARKET_FIND_FRIENDS_AFTER_FIRST_LOOKUP_VA: u32 = 0x0063_933a;
/// `BuildTypeData::is_gather_type` vtable slot reached after a found Build passes the City gate.
pub const MARKET_FIND_FRIENDS_FIRST_FOUND_TYPE_VIRTUAL_SLOT: u32 = 0x90;
pub const MARKET_FIND_FRIENDS_WONDER_VIRTUAL_SLOT: u32 = 0x1c;
pub const BUILD_TYPE_BASIC_TYPE_VA: u32 = 0x0063_9970;
pub const UNIVERSITY_TYPE: usize = 420;
pub const GATHER_ENHANCER_TYPES: [usize; 4] = [423, 424, 425, 426];
pub const BUILD_GATHER_TYPE_MASK: u32 = 0x40;
pub const BUILD_MILITARY_TRAINER_MASK: u32 = 0x4000_0000;
pub const OBJECT_CAPTURED_MASK: u8 = 0x20;
pub const WONDER_TYPE_FIRST: i32 = 0x20e;
pub const WONDER_TYPE_END: i32 = 0x21f;
/// Independent schema for the two-image pre/post Market lifecycle contract.
pub const GOLDEN_STARTING_MARKET_SETUP_ENTRY_SCHEMA_VERSION: u64 = 1;
pub const MARKET_FOOTPRINT: Footprint = Footprint {
    x_size: 4,
    y_size: 4,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenMarketBonusProbe {
    pub bonus: i32,
    pub granted: bool,
    pub receipt: TribeBonusReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketPlan {
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub rules_serialized_sha256: [u8; 32],
    pub owner: u8,
    pub replay_player_slot: u8,
    pub play: u8,
    pub tribe: u8,
    pub player_tribe_source: ReplayByteSpan,
    pub tribe_facts: ReplayTribeTypeFacts,
    pub market_type: ReplayBuildTypeFacts,
    pub probes: Vec<GoldenMarketBonusProbe>,
    pub calls: Vec<LeaderProduceBuildingPrefixRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketPlanError {
    ReplayRead(String),
    PayloadRead(String),
    WrongReplayFile,
    WrongReplaySettings,
    MissingPlayer,
    WrongPlayer,
    MissingPlayerSource,
    MissingRules,
    Rules(PrePairUnitAuthorityError),
    Bonus(TribeBonusInputError),
    WrongDutchBonusSchedule,
    WrongMarketTypeFacts,
}

impl fmt::Display for GoldenStartingMarketPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "golden starting Market plan refused: {self:?}")
    }
}

impl std::error::Error for GoldenStartingMarketPlanError {}

impl From<PrePairUnitAuthorityError> for GoldenStartingMarketPlanError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::Rules(value)
    }
}

impl From<TribeBonusInputError> for GoldenStartingMarketPlanError {
    fn from(value: TribeBonusInputError) -> Self {
        Self::Bonus(value)
    }
}

fn bonus_probe(
    inputs: TribeBonusInputs,
    bonus: i32,
    probes: &mut Vec<GoldenMarketBonusProbe>,
) -> Result<bool, GoldenStartingMarketPlanError> {
    let (granted, receipt) = has_tribe_bonus(inputs, bonus)?;
    probes.push(GoldenMarketBonusProbe {
        bonus,
        granted,
        receipt,
    });
    Ok(granted)
}

/// Derive the sole golden civ-specific Build call from replay-selected player and Rules bytes.
pub fn derive_golden_starting_market_plan(
    replay: &Replay,
) -> Result<GoldenStartingMarketPlan, GoldenStartingMarketPlanError> {
    let raw = std::fs::read(&replay.path)
        .map_err(|error| GoldenStartingMarketPlanError::ReplayRead(error.to_string()))?;
    let replay_file_sha256 = sha256(&raw);
    if replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(GoldenStartingMarketPlanError::WrongReplayFile);
    }
    if replay.initial.info.seed != REPLAY_SEED
        || replay.initial.info.settings.map_style != MAP_STYLE
        || replay.initial.info.settings.map_size != MAP_SIZE
        || replay.initial.info.settings.starting_town != 1
    {
        return Err(GoldenStartingMarketPlanError::WrongReplaySettings);
    }
    let player = replay
        .initial
        .info
        .players
        .iter()
        .find(|player| player.present && player.who == OWNER)
        .ok_or(GoldenStartingMarketPlanError::MissingPlayer)?;
    if player.play != PLAY || player.tribe != TRIBE {
        return Err(GoldenStartingMarketPlanError::WrongPlayer);
    }
    let payload = load_payload(&replay.path)
        .map_err(|error| GoldenStartingMarketPlanError::PayloadRead(error.to_string()))?;
    if sha256(&payload) != replay.initial.payload_sha256 {
        return Err(GoldenStartingMarketPlanError::WrongReplayFile);
    }
    let player_span = replay.initial.worldgen_sources.player_bodies[player.slot as usize]
        .filter(|span| span.bytes == 0x39)
        .ok_or(GoldenStartingMarketPlanError::MissingPlayerSource)?;
    let player_body = payload
        .get(player_span.offset..player_span.end())
        .ok_or(GoldenStartingMarketPlanError::MissingPlayerSource)?;
    if player_body[0x32] != TRIBE || player_body[0x33] != OWNER {
        return Err(GoldenStartingMarketPlanError::MissingPlayerSource);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(GoldenStartingMarketPlanError::MissingRules)?;
    let tribe_facts =
        replay_tribe_type_facts(&payload, &rules, usize::from(TRIBE), UNIT_TYPE_FIRST)?;
    if tribe_facts.tribe_id != i32::from(TRIBE) {
        return Err(GoldenStartingMarketPlanError::WrongDutchBonusSchedule);
    }
    let inputs = TribeBonusInputs {
        no_nation_powers: replay.initial.info.flags & 4 != 0,
        victory: replay.initial.info.settings.victory,
        city_num: 1,
        tribe: i32::from(TRIBE),
        leader_flags2: LEADER_INIT_FLAGS2,
        conquest_racial_powers: CanonicalConquestRacialPowers::default(),
        tribe_default_bonus: Some(tribe_facts.tribe_id),
    };
    let mut probes = Vec::with_capacity(CIV_SPECIFIC_BONUS_ORDER.len());
    let bonus_4 = bonus_probe(inputs, 4, &mut probes)?;
    let bonus_22 = if bonus_4 {
        false
    } else {
        bonus_probe(inputs, 22, &mut probes)?
    };
    let bonus_5 = bonus_probe(inputs, 5, &mut probes)?;
    let bonus_16 = bonus_probe(inputs, 16, &mut probes)?;
    let bonus_7 = bonus_probe(inputs, 7, &mut probes)?;
    let bonus_10 = bonus_probe(inputs, 10, &mut probes)?;
    let bonus_18 = bonus_probe(inputs, 18, &mut probes)?;
    let schedule = civ_specific_free_build_plan(CivSpecificBuildingsRequest {
        bonus_4,
        bonus_22,
        bonus_5,
        bonus_5_rule: 0,
        bonus_16,
        bonus_16_rule: 0,
        bonus_7,
        bonus_7_rule: 0,
        bonus_10,
        bonus_10_rule: 0,
        bonus_18,
        bonus_18_rule: 0,
    });
    if probes.iter().map(|probe| probe.bonus).collect::<Vec<_>>() != CIV_SPECIFIC_BONUS_ORDER
        || probes.iter().map(|probe| probe.granted).collect::<Vec<_>>()
            != [false, true, false, false, false, false, false]
        || schedule != [DUTCH_STARTING_MARKET_TYPE]
    {
        return Err(GoldenStartingMarketPlanError::WrongDutchBonusSchedule);
    }
    let market_type = replay_build_type_facts(&payload, &rules, DUTCH_STARTING_MARKET_TYPE)?;
    if market_type.type_index != DUTCH_STARTING_MARKET_TYPE
        || market_type.build_flags != MARKET_BUILD_FLAGS
        || market_type.domain != MARKET_DOMAIN
        || [market_type.x_size, market_type.y_size]
            != [MARKET_FOOTPRINT.x_size, MARKET_FOOTPRINT.y_size]
    {
        return Err(GoldenStartingMarketPlanError::WrongMarketTypeFacts);
    }

    Ok(GoldenStartingMarketPlan {
        replay_file_sha256,
        replay_payload_sha256: replay.initial.payload_sha256,
        rules_serialized_sha256: rules.serialized_sha256,
        owner: OWNER,
        replay_player_slot: player.slot,
        play: PLAY,
        tribe: TRIBE,
        player_tribe_source: ReplayByteSpan {
            offset: player_span.offset + 0x32,
            bytes: 1,
        },
        tribe_facts,
        market_type,
        probes,
        calls: vec![LeaderProduceBuildingPrefixRequest {
            owner: OWNER,
            type_index: DUTCH_STARTING_MARKET_TYPE,
            origin_build_object: STARTING_VILLAGE_O as i16,
            mode: 0,
        }],
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketPlacementReceipt {
    pub plan: GoldenStartingMarketPlan,
    pub candidate: LeaderProduceBuildingCandidatePrefixReceipt,
    pub footprint: LeaderProduceBuildingBlockedSiteRawZeroFootprintReceipt,
    pub blocked_location: LeaderProduceBuildingMarketBlockedLocationRequest,
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketPlacementError {
    Plan(GoldenStartingMarketPlanError),
    InvalidCandidate,
    BlockedSite(LeaderProduceBuildingBlockedSitePrefixError),
    Footprint(LeaderProduceBuildingBlockedSiteRawZeroFootprintError),
    BlockedLocation(LeaderProduceBuildingMarketBlockedLocationRequestError),
}

impl fmt::Display for GoldenStartingMarketPlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "golden starting Market placement refused: {self:?}")
    }
}

impl std::error::Error for GoldenStartingMarketPlacementError {}

/// Join one generated-World candidate to the source-derived golden Market call and advance
/// through all sixteen direct-zero footprint children.
pub fn advance_golden_starting_market_candidate(
    replay: &Replay,
    sim: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    candidate: LeaderProduceBuildingCandidatePrefixReceipt,
) -> Result<GoldenStartingMarketPlacementReceipt, GoldenStartingMarketPlacementError> {
    let plan = derive_golden_starting_market_plan(replay)
        .map_err(GoldenStartingMarketPlacementError::Plan)?;
    let Some(boundary) = candidate.continuation else {
        return Err(GoldenStartingMarketPlacementError::InvalidCandidate);
    };
    if !candidate.validates()
        || candidate.status != LeaderProduceBuildingCandidatePrefixStatus::ReadyForBlockedSite
        || boundary.owner != OWNER
        || boundary.type_index != DUTCH_STARTING_MARKET_TYPE
        || boundary.origin_build_object != STARTING_VILLAGE_O as i16
        || boundary.mode != 0
        || boundary.target_footprint != MARKET_FOOTPRINT
        || boundary.target_domain != MARKET_DOMAIN
        || boundary.space_grade != 4
    {
        return Err(GoldenStartingMarketPlacementError::InvalidCandidate);
    }
    let entry = apply_sim_leader_produce_building_blocked_site_prefix(production, types, boundary)
        .map_err(GoldenStartingMarketPlacementError::BlockedSite)?;
    let footprint = apply_sim_leader_produce_building_blocked_site_raw_zero_footprint(
        sim, production, types, entry,
    )
    .map_err(GoldenStartingMarketPlacementError::Footprint)?;
    let blocked_location = plan_sim_leader_produce_building_market_blocked_location_request(
        production,
        types,
        footprint.clone(),
    )
    .map_err(GoldenStartingMarketPlacementError::BlockedLocation)?;
    Ok(GoldenStartingMarketPlacementReceipt {
        plan,
        candidate,
        footprint,
        blocked_location,
        source_produced_city_bytes: 0,
        installed_in_scoreboard: false,
    })
}

/// Execution-backed accepted coarse Market site over one canonical pre-Market Sim.
///
/// The World query and every later read-only `blocked_location` predicate are replayed from the
/// retained preimage. The full Sim hash prevents this authority from being transplanted onto a
/// different City/Build image. Candidate scoring, fine RNG and all construction mutations remain
/// outside this receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketAcceptedPlacementReceipt {
    pub placement: GoldenStartingMarketPlacementReceipt,
    pub blocked_location: LeaderProduceBuildingMarketBlockedLocationReceipt,
    /// Exact prefix of the first scoring child. The receipt stops before the mutating
    /// `ObjectsData::find_building_placed_at` call and therefore does not authorize a
    /// `find_friends` return value or any coarse score.
    pub find_friends: BuildTypeFindFriendsReceipt,
    pub before_sim_sha256: [u8; 32],
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketAcceptedPlacementError {
    InvalidPlacementReceipt,
    Snapshot(SaveError),
    Tregion(WorldTregionError),
    BlockedLocation(LeaderProduceBuildingMarketBlockedLocationError),
    FindFriends(BuildTypeFindFriendsError),
    UnexpectedFindFriendsStop,
}

impl fmt::Display for GoldenStartingMarketAcceptedPlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "golden starting Market accepted placement refused: {self:?}"
        )
    }
}

impl std::error::Error for GoldenStartingMarketAcceptedPlacementError {}

fn accepted_placement_receipt_shape_validates(
    accepted: &GoldenStartingMarketAcceptedPlacementReceipt,
) -> bool {
    accepted.placement.footprint.validates()
        && accepted.placement.candidate.continuation
            == Some(accepted.placement.footprint.entry.input)
        && accepted.placement.blocked_location.validates()
        && accepted.placement.blocked_location.input == accepted.placement.footprint
        && accepted.placement.source_produced_city_bytes == 0
        && !accepted.placement.installed_in_scoreboard
        && accepted.blocked_location.validates()
        && accepted.blocked_location.input == accepted.placement.blocked_location
        && accepted.find_friends.validates()
        && accepted.find_friends.request
            == market_find_friends_request(accepted.blocked_location.next_child)
        && accepted.find_friends.stop == BuildTypeFindFriendsStop::FirstObjectLookup
        && accepted.find_friends.returned.is_none()
        && accepted.find_friends.first_child.is_some()
        && accepted.source_produced_city_bytes == 0
        && !accepted.installed_in_scoreboard
}

/// Exact local continuation after the first placed-Build lookup returns to
/// `BuildTypeData::find_friends`.
///
/// This is a resume boundary, not a `find_friends` result. The generic tail must still consume
/// the returned object, City/type predicates, remaining ring probes, and final accumulator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFindFriendsAfterFirstLookupBoundary {
    pub instruction_va: u32,
    pub function_va: u32,
    pub request: BuildTypeFindFriendsRequest,
    pub circle_offset: i32,
    pub accumulator_before: i32,
    pub effective_city_filter: i32,
    pub returned: i32,
    pub returned_owner: Option<i32>,
}

impl GoldenStartingMarketFindFriendsAfterFirstLookupBoundary {
    fn validates_against(
        &self,
        find_friends: &BuildTypeFindFriendsReceipt,
        lookup: &ObjectsFindBuildingPlacedAtReceipt,
    ) -> bool {
        self.instruction_va == MARKET_FIND_FRIENDS_AFTER_FIRST_LOOKUP_VA
            && self.function_va == BUILD_TYPE_FIND_FRIENDS_VA
            && self.request == find_friends.request
            && self.circle_offset == lookup.request.circle_offset
            && self.accumulator_before == 0
            && find_friends.effective_city_filter == Some(self.effective_city_filter)
            && lookup.returned == Some(self.returned)
            && lookup.returned_owner == self.returned_owner
            && ((self.returned == -1 && self.returned_owner.is_none())
                || (self.returned >= 0 && self.returned_owner == Some(find_friends.request.owner)))
    }
}

/// One complete execution-backed first Object lookup, committed against the separately installed
/// `ObjectsData+0x200` authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFirstObjectLookupReceipt {
    pub accepted: GoldenStartingMarketAcceptedPlacementReceipt,
    pub object_lookup: ObjectsFindBuildingPlacedAtReceipt,
    pub scratch_before: ObjectsSelectedOwnerAuthority,
    pub scratch_after: ObjectsSelectedOwnerAuthority,
    pub next: GoldenStartingMarketFindFriendsAfterFirstLookupBoundary,
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

impl GoldenStartingMarketFirstObjectLookupReceipt {
    pub fn validates(&self) -> bool {
        accepted_placement_receipt_shape_validates(&self.accepted)
            && self.accepted.find_friends.first_child == Some(self.object_lookup.request)
            && self.object_lookup.validates()
            && self.object_lookup.returned.is_some()
            && self.object_lookup.stop != ObjectsFindBuildingPlacedAtStop::WallBandIdentityBoundary
            && self.object_lookup.scratch.before == self.scratch_before
            && self.object_lookup.scratch.after == Some(self.scratch_after)
            && self
                .next
                .validates_against(&self.accepted.find_friends, &self.object_lookup)
            && self.source_produced_city_bytes == 0
            && !self.installed_in_scoreboard
    }

    pub fn validates_against(
        &self,
        before: &Sim,
        production: &LiveProductionRuntime,
        types: &TypeBuiltinState,
    ) -> bool {
        self.validates()
            && self
                .accepted
                .blocked_location
                .tregion
                .validates_against(&before.map.world)
            && self
                .accepted
                .find_friends
                .validates_against(types, production, &before.map.world)
            && self.object_lookup.validates_against(before, production)
            && frame379_setup_snapshot_sha256(before)
                .is_ok_and(|digest| digest == self.accepted.before_sim_sha256)
    }
}

/// Typed fail-closed stop when the generic lookup reaches the unresolved Wall-band identity
/// projection. The scratch authority is unchanged and no parent `find_friends` instruction ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFirstObjectLookupWallBoundaryReceipt {
    pub accepted: GoldenStartingMarketAcceptedPlacementReceipt,
    pub object_lookup: ObjectsFindBuildingPlacedAtReceipt,
    pub scratch_authority: ObjectsSelectedOwnerAuthority,
    pub wall_boundary: ObjectsFindBuildingPlacedAtWallBoundary,
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

impl GoldenStartingMarketFirstObjectLookupWallBoundaryReceipt {
    pub fn validates(&self) -> bool {
        accepted_placement_receipt_shape_validates(&self.accepted)
            && self.accepted.find_friends.first_child == Some(self.object_lookup.request)
            && self.object_lookup.validates()
            && self.object_lookup.stop == ObjectsFindBuildingPlacedAtStop::WallBandIdentityBoundary
            && self.object_lookup.returned.is_none()
            && self.object_lookup.scratch.before == self.scratch_authority
            && self.object_lookup.scratch.after.is_none()
            && self.object_lookup.wall_boundary == Some(self.wall_boundary)
            && self.source_produced_city_bytes == 0
            && !self.installed_in_scoreboard
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketFirstObjectLookupAdvance {
    Complete(GoldenStartingMarketFirstObjectLookupReceipt),
    WallBandIdentityBoundary(GoldenStartingMarketFirstObjectLookupWallBoundaryReceipt),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketFirstObjectLookupError {
    InvalidAcceptedPlacementReceipt,
    Snapshot(SaveError),
    BeforeSnapshotMismatch,
    Lookup(ObjectsFindBuildingPlacedAtExecuteError),
    InvalidLookupReceipt,
    Rollback(ObjectsSelectedOwnerCommitError),
}

impl fmt::Display for GoldenStartingMarketFirstObjectLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "golden starting Market first Object lookup refused: {self:?}"
        )
    }
}

impl std::error::Error for GoldenStartingMarketFirstObjectLookupError {}

fn market_find_friends_request(
    boundary: LeaderProduceBuildingFindFriendsBoundary,
) -> BuildTypeFindFriendsRequest {
    BuildTypeFindFriendsRequest {
        call_va: boundary.call_va,
        callee_va: boundary.callee_va,
        type_index: boundary.type_index,
        candidate_world_cell: boundary.candidate_world_cell,
        city_filter: boundary.origin_city_filter,
        owner: i32::from(boundary.owner),
    }
}

/// Consume the generic `WorldData::get_tregion` sibling authority and execute the rest of the
/// source-exact read-only Market placement verdict on the same pre-Market Sim.
pub fn advance_golden_starting_market_blocked_location(
    before: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    placement: GoldenStartingMarketPlacementReceipt,
) -> Result<GoldenStartingMarketAcceptedPlacementReceipt, GoldenStartingMarketAcceptedPlacementError>
{
    if !placement.footprint.validates()
        || placement.candidate.continuation != Some(placement.footprint.entry.input)
        || !placement.blocked_location.validates()
        || placement.blocked_location.input != placement.footprint
        || placement.source_produced_city_bytes != 0
        || placement.installed_in_scoreboard
    {
        return Err(GoldenStartingMarketAcceptedPlacementError::InvalidPlacementReceipt);
    }
    let request = placement.blocked_location.clone();
    let tregion = before
        .map
        .world
        .read_tregion(WorldTregionQuery {
            call_va: BUILD_TYPE_BLOCKED_LOCATION_GET_TREGION_CALL_VA,
            callee_va: request.first_world_child_callee_va,
            tcoord: request.placement_tcoord,
        })
        .map_err(GoldenStartingMarketAcceptedPlacementError::Tregion)?;
    let blocked_location = apply_sim_leader_produce_building_market_blocked_location_tail(
        before, production, types, request, tregion,
    )
    .map_err(GoldenStartingMarketAcceptedPlacementError::BlockedLocation)?;
    let find_friends = produce_build_type_find_friends_prefix(
        types,
        production,
        &before.map.world,
        market_find_friends_request(blocked_location.next_child),
    )
    .map_err(GoldenStartingMarketAcceptedPlacementError::FindFriends)?;
    if find_friends.stop != BuildTypeFindFriendsStop::FirstObjectLookup
        || find_friends.returned.is_some()
        || find_friends.first_child.is_none()
    {
        return Err(GoldenStartingMarketAcceptedPlacementError::UnexpectedFindFriendsStop);
    }
    let before_sim_sha256 = frame379_setup_snapshot_sha256(before)
        .map_err(GoldenStartingMarketAcceptedPlacementError::Snapshot)?;
    Ok(GoldenStartingMarketAcceptedPlacementReceipt {
        placement,
        blocked_location,
        find_friends,
        before_sim_sha256,
        source_produced_city_bytes: 0,
        installed_in_scoreboard: false,
    })
}

/// Execute the first generic placed-Build lookup and advance only the independently installed
/// Objects scratch word. The canonical Sim remains read-only. A complete lookup publishes the
/// exact `+0x200` after-image; a Wall identity boundary publishes nothing and is returned as a
/// typed residual.
pub fn advance_golden_starting_market_first_object_lookup(
    before: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    scratch: &mut ObjectsSelectedOwnerAuthority,
    accepted: GoldenStartingMarketAcceptedPlacementReceipt,
) -> Result<GoldenStartingMarketFirstObjectLookupAdvance, GoldenStartingMarketFirstObjectLookupError>
{
    if !accepted_placement_receipt_shape_validates(&accepted) {
        return Err(GoldenStartingMarketFirstObjectLookupError::InvalidAcceptedPlacementReceipt);
    }
    let before_sim_sha256 = frame379_setup_snapshot_sha256(before)
        .map_err(GoldenStartingMarketFirstObjectLookupError::Snapshot)?;
    if before_sim_sha256 != accepted.before_sim_sha256
        || !accepted
            .blocked_location
            .tregion
            .validates_against(&before.map.world)
        || !accepted
            .find_friends
            .validates_against(types, production, &before.map.world)
    {
        return Err(GoldenStartingMarketFirstObjectLookupError::BeforeSnapshotMismatch);
    }

    let request = accepted
        .find_friends
        .first_child
        .expect("validated first Object child");
    let scratch_before = *scratch;
    let object_lookup =
        execute_objects_find_building_placed_at(before, production, scratch, request)
            .map_err(GoldenStartingMarketFirstObjectLookupError::Lookup)?;
    if object_lookup.request != request
        || object_lookup.scratch.before != scratch_before
        || !object_lookup.validates_against(before, production)
    {
        if object_lookup.returned.is_some() {
            object_lookup
                .scratch
                .rollback(scratch)
                .map_err(GoldenStartingMarketFirstObjectLookupError::Rollback)?;
        }
        return Err(GoldenStartingMarketFirstObjectLookupError::InvalidLookupReceipt);
    }

    if object_lookup.stop == ObjectsFindBuildingPlacedAtStop::WallBandIdentityBoundary {
        if *scratch != scratch_before || object_lookup.returned.is_some() {
            return Err(GoldenStartingMarketFirstObjectLookupError::InvalidLookupReceipt);
        }
        let wall_boundary = object_lookup
            .wall_boundary
            .expect("validated Wall boundary receipt");
        let receipt = GoldenStartingMarketFirstObjectLookupWallBoundaryReceipt {
            accepted,
            object_lookup,
            scratch_authority: scratch_before,
            wall_boundary,
            source_produced_city_bytes: 0,
            installed_in_scoreboard: false,
        };
        debug_assert!(receipt.validates());
        return Ok(GoldenStartingMarketFirstObjectLookupAdvance::WallBandIdentityBoundary(receipt));
    }

    let returned = object_lookup.returned.expect("validated complete lookup");
    let scratch_after = object_lookup
        .scratch
        .after
        .expect("validated complete lookup journal");
    if *scratch != scratch_after {
        object_lookup
            .scratch
            .rollback(scratch)
            .map_err(GoldenStartingMarketFirstObjectLookupError::Rollback)?;
        return Err(GoldenStartingMarketFirstObjectLookupError::InvalidLookupReceipt);
    }
    let next = GoldenStartingMarketFindFriendsAfterFirstLookupBoundary {
        instruction_va: MARKET_FIND_FRIENDS_AFTER_FIRST_LOOKUP_VA,
        function_va: BUILD_TYPE_FIND_FRIENDS_VA,
        request: accepted.find_friends.request,
        circle_offset: request.circle_offset,
        accumulator_before: 0,
        effective_city_filter: accepted
            .find_friends
            .effective_city_filter
            .expect("validated Market find_friends prefix"),
        returned,
        returned_owner: object_lookup.returned_owner,
    };
    let receipt = GoldenStartingMarketFirstObjectLookupReceipt {
        accepted,
        object_lookup,
        scratch_before,
        scratch_after,
        next,
        source_produced_city_bytes: 0,
        installed_in_scoreboard: false,
    };
    debug_assert!(receipt.validates());
    Ok(GoldenStartingMarketFirstObjectLookupAdvance::Complete(
        receipt,
    ))
}

/// Exact Build scalar reached after a nonnegative placed-Build lookup and before retail's first
/// found-type virtual. A City mismatch skips every type predicate and resumes the ring unchanged;
/// a match is the fail-closed boundary owned by this tranche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFindFriendsFoundBuildRead {
    pub owner: i32,
    pub object: i32,
    pub build_row: usize,
    pub type_index: i32,
    pub city: i16,
    pub effective_city_filter: i32,
}

/// One exact continuation step after the first lookup. Out-of-bounds offsets have no child and
/// do not touch Objects scratch. Complete lookup steps are either misses or City mismatches; a
/// City match is retained separately as a typed boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFindFriendsRingStep {
    pub circle_offset: i32,
    pub world_cell: [i32; 2],
    pub child: Option<ObjectsFindBuildingPlacedAtBoundary>,
    pub object_lookup: Option<ObjectsFindBuildingPlacedAtReceipt>,
    pub city_mismatch: Option<GoldenStartingMarketFindFriendsFoundBuildRead>,
    pub accumulator_before: i32,
    pub accumulator_after: i32,
}

/// Exact all-miss/City-mismatch native return. No found-type predicate was guessed and therefore
/// the retail accumulator is provably still zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFindFriendsReturnedZeroReceipt {
    pub first_lookup: GoldenStartingMarketFirstObjectLookupReceipt,
    pub remaining_steps: Vec<GoldenStartingMarketFindFriendsRingStep>,
    pub scratch_entry: ObjectsSelectedOwnerAuthority,
    pub scratch_after: ObjectsSelectedOwnerAuthority,
    pub returned: i32,
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

/// Typed stop after a lookup returned a Build in the effective City. The first unowned operation
/// is the found BuildType's `is_gather_type` virtual; no relation, flag, Wonder, or accumulator
/// result is claimed here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFindFriendsTypeBoundaryReceipt {
    pub first_lookup: GoldenStartingMarketFirstObjectLookupReceipt,
    pub completed_steps: Vec<GoldenStartingMarketFindFriendsRingStep>,
    pub scratch_entry: ObjectsSelectedOwnerAuthority,
    pub scratch_at_boundary: ObjectsSelectedOwnerAuthority,
    pub object_lookup: ObjectsFindBuildingPlacedAtReceipt,
    pub found: GoldenStartingMarketFindFriendsFoundBuildRead,
    pub accumulator_before: i32,
    pub first_unowned_virtual_slot: u32,
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

/// Typed stop when a later generic lookup reaches its impossible-retail Wall-band identity seam.
/// The boundary lookup does not publish scratch; earlier complete lookups remain exact and are
/// retained in program order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFindFriendsWallBoundaryReceipt {
    pub first_lookup: GoldenStartingMarketFirstObjectLookupReceipt,
    pub completed_steps: Vec<GoldenStartingMarketFindFriendsRingStep>,
    pub scratch_entry: ObjectsSelectedOwnerAuthority,
    pub scratch_at_boundary: ObjectsSelectedOwnerAuthority,
    pub object_lookup: ObjectsFindBuildingPlacedAtReceipt,
    pub wall_boundary: ObjectsFindBuildingPlacedAtWallBoundary,
    pub accumulator_before: i32,
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketFindFriendsRingAdvance {
    ReturnedZero(GoldenStartingMarketFindFriendsReturnedZeroReceipt),
    FoundBuildTypeBoundary(GoldenStartingMarketFindFriendsTypeBoundaryReceipt),
    WallBandIdentityBoundary(GoldenStartingMarketFindFriendsWallBoundaryReceipt),
}

impl GoldenStartingMarketFindFriendsRingAdvance {
    /// Re-run the bounded adapter from its retained first-lookup/scratch entry and require the
    /// complete result and published scratch state to be identical.
    pub fn validates_against(
        &self,
        before: &Sim,
        production: &LiveProductionRuntime,
        types: &TypeBuiltinState,
    ) -> bool {
        let (first_lookup, scratch_entry, expected_scratch) = match self {
            Self::ReturnedZero(receipt) => (
                &receipt.first_lookup,
                receipt.scratch_entry,
                receipt.scratch_after,
            ),
            Self::FoundBuildTypeBoundary(receipt) => (
                &receipt.first_lookup,
                receipt.scratch_entry,
                receipt.scratch_at_boundary,
            ),
            Self::WallBandIdentityBoundary(receipt) => (
                &receipt.first_lookup,
                receipt.scratch_entry,
                receipt.scratch_at_boundary,
            ),
        };
        let mut scratch = scratch_entry;
        advance_golden_starting_market_find_friends_ring(
            before,
            production,
            types,
            &mut scratch,
            first_lookup.clone(),
        )
        .is_ok_and(|expected| expected == *self && scratch == expected_scratch)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketFindFriendsRingError {
    InvalidFirstLookupReceipt,
    ScratchDoesNotExtendFirstLookup,
    InvalidLookupReceipt,
    InvalidFoundBuild,
    Lookup(ObjectsFindBuildingPlacedAtExecuteError),
    Rollback(ObjectsSelectedOwnerCommitError),
}

impl fmt::Display for GoldenStartingMarketFindFriendsRingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "golden starting Market find_friends ring refused: {self:?}"
        )
    }
}

impl std::error::Error for GoldenStartingMarketFindFriendsRingError {}

fn market_find_friends_object_child(
    request: BuildTypeFindFriendsRequest,
    circle_offset: i32,
    world_cell: [i32; 2],
) -> ObjectsFindBuildingPlacedAtBoundary {
    let tile = [
        world_cell[0].wrapping_mul(4).wrapping_add(2),
        world_cell[1].wrapping_mul(4).wrapping_add(2),
    ];
    ObjectsFindBuildingPlacedAtBoundary {
        call_va: BUILD_TYPE_FIND_FRIENDS_FIRST_OBJECT_CALL_VA,
        callee_va: OBJECTS_FIND_BUILDING_PLACED_AT_VA,
        native_push_order: [-1, -1, request.owner, tile[1], tile[0]],
        circle_offset,
        world_cell,
        tile,
        owner_filter: request.owner,
        excluded_object: -1,
        excluded_owner: -1,
        selected_owner_offset: OBJECTS_SELECTED_OWNER_OFFSET,
        selected_owner_first_write: -1,
    }
}

fn market_find_friends_found_build(
    sim: &Sim,
    effective_city_filter: i32,
    lookup: &ObjectsFindBuildingPlacedAtReceipt,
) -> Option<GoldenStartingMarketFindFriendsFoundBuildRead> {
    let object = lookup.returned.filter(|&value| value >= 0)?;
    let owner = lookup.returned_owner?;
    let object_i16 = i16::try_from(object).ok()?;
    let owner_i16 = i16::try_from(owner).ok()?;
    let read = lookup.objects.iter().rev().find(|read| {
        read.band == ObjectsSpatialBand::Build
            && read.key.who == owner_i16
            && read.key.o == object_i16
            && read.valid_build == Some(true)
            && read.contains_query_tile == Some(true)
    })?;
    let build = sim.builds.get(read.row)?;
    if u8::try_from(owner).ok() != Some(build.who) || build.object_id() != object_i16 {
        return None;
    }
    Some(GoldenStartingMarketFindFriendsFoundBuildRead {
        owner,
        object,
        build_row: read.row,
        type_index: read.type_index?,
        city: build.city,
        effective_city_filter,
    })
}

fn rollback_market_find_friends_steps(
    steps: &[GoldenStartingMarketFindFriendsRingStep],
    scratch: &mut ObjectsSelectedOwnerAuthority,
) -> Result<(), ObjectsSelectedOwnerCommitError> {
    for step in steps.iter().rev() {
        if let Some(lookup) = &step.object_lookup {
            lookup.scratch.rollback(scratch)?;
        }
    }
    Ok(())
}

/// Continue the golden Market's ring after the exact first lookup.
///
/// Retail offsets remain clockwise `1..=8`. Each in-bounds child commits the separately owned
/// `ObjectsData+0x200` journal before the parent inspects its result. Misses and exact City
/// mismatches leave the accumulator at zero and may advance. A same-City Build stops before the
/// first type virtual. Thus only the all-miss/City-mismatch cohort can return zero; this function
/// never executes found-type predicates, coarse scoring, fine RNG, allocation, or City writes.
pub fn advance_golden_starting_market_find_friends_ring(
    before: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    scratch: &mut ObjectsSelectedOwnerAuthority,
    first_lookup: GoldenStartingMarketFirstObjectLookupReceipt,
) -> Result<GoldenStartingMarketFindFriendsRingAdvance, GoldenStartingMarketFindFriendsRingError> {
    if !first_lookup.validates_against(before, production, types) {
        return Err(GoldenStartingMarketFindFriendsRingError::InvalidFirstLookupReceipt);
    }
    if *scratch != first_lookup.scratch_after {
        return Err(GoldenStartingMarketFindFriendsRingError::ScratchDoesNotExtendFirstLookup);
    }
    let scratch_entry = *scratch;
    let request = first_lookup.next.request;
    let effective_city_filter = first_lookup.next.effective_city_filter;
    let first_circle_offset = first_lookup.next.circle_offset;
    if !(1..=8).contains(&first_circle_offset) {
        return Err(GoldenStartingMarketFindFriendsRingError::InvalidFirstLookupReceipt);
    }

    if first_lookup.next.returned >= 0 {
        let found = market_find_friends_found_build(
            before,
            effective_city_filter,
            &first_lookup.object_lookup,
        )
        .ok_or(GoldenStartingMarketFindFriendsRingError::InvalidFoundBuild)?;
        if i32::from(found.city) == effective_city_filter {
            return Ok(
                GoldenStartingMarketFindFriendsRingAdvance::FoundBuildTypeBoundary(
                    GoldenStartingMarketFindFriendsTypeBoundaryReceipt {
                        first_lookup: first_lookup.clone(),
                        completed_steps: Vec::new(),
                        scratch_entry,
                        scratch_at_boundary: *scratch,
                        object_lookup: first_lookup.object_lookup.clone(),
                        found,
                        accumulator_before: 0,
                        first_unowned_virtual_slot:
                            MARKET_FIND_FRIENDS_FIRST_FOUND_TYPE_VIRTUAL_SLOT,
                        source_produced_city_bytes: 0,
                        installed_in_scoreboard: false,
                    },
                ),
            );
        }
    }

    let mut completed_steps = Vec::new();
    for circle_offset in first_circle_offset + 1..=8 {
        let (dx, dy) = WORLD_CELL_SEARCH_OFFSETS[circle_offset as usize];
        let world_cell = [
            request.candidate_world_cell[0].wrapping_add(dx),
            request.candidate_world_cell[1].wrapping_add(dy),
        ];
        if !before.map.world.valid_w(world_cell[0], world_cell[1]) {
            completed_steps.push(GoldenStartingMarketFindFriendsRingStep {
                circle_offset,
                world_cell,
                child: None,
                object_lookup: None,
                city_mismatch: None,
                accumulator_before: 0,
                accumulator_after: 0,
            });
            continue;
        }

        let child = market_find_friends_object_child(request, circle_offset, world_cell);
        let scratch_before = *scratch;
        let lookup =
            match execute_objects_find_building_placed_at(before, production, scratch, child) {
                Ok(receipt) => receipt,
                Err(error) => {
                    rollback_market_find_friends_steps(&completed_steps, scratch)
                        .map_err(GoldenStartingMarketFindFriendsRingError::Rollback)?;
                    return Err(GoldenStartingMarketFindFriendsRingError::Lookup(error));
                }
            };
        if lookup.request != child
            || lookup.scratch.before != scratch_before
            || !lookup.validates_against(before, production)
        {
            if lookup.returned.is_some() {
                lookup
                    .scratch
                    .rollback(scratch)
                    .map_err(GoldenStartingMarketFindFriendsRingError::Rollback)?;
            }
            rollback_market_find_friends_steps(&completed_steps, scratch)
                .map_err(GoldenStartingMarketFindFriendsRingError::Rollback)?;
            return Err(GoldenStartingMarketFindFriendsRingError::InvalidLookupReceipt);
        }
        if lookup.stop == ObjectsFindBuildingPlacedAtStop::WallBandIdentityBoundary {
            if *scratch != scratch_before || lookup.returned.is_some() {
                rollback_market_find_friends_steps(&completed_steps, scratch)
                    .map_err(GoldenStartingMarketFindFriendsRingError::Rollback)?;
                return Err(GoldenStartingMarketFindFriendsRingError::InvalidLookupReceipt);
            }
            let wall_boundary = lookup
                .wall_boundary
                .expect("validated Wall-band identity boundary");
            return Ok(
                GoldenStartingMarketFindFriendsRingAdvance::WallBandIdentityBoundary(
                    GoldenStartingMarketFindFriendsWallBoundaryReceipt {
                        first_lookup,
                        completed_steps,
                        scratch_entry,
                        scratch_at_boundary: *scratch,
                        object_lookup: lookup,
                        wall_boundary,
                        accumulator_before: 0,
                        source_produced_city_bytes: 0,
                        installed_in_scoreboard: false,
                    },
                ),
            );
        }

        let returned = lookup.returned.expect("validated complete Object lookup");
        if returned == -1 {
            completed_steps.push(GoldenStartingMarketFindFriendsRingStep {
                circle_offset,
                world_cell,
                child: Some(child),
                object_lookup: Some(lookup),
                city_mismatch: None,
                accumulator_before: 0,
                accumulator_after: 0,
            });
            continue;
        }
        let Some(found) = market_find_friends_found_build(before, effective_city_filter, &lookup)
        else {
            lookup
                .scratch
                .rollback(scratch)
                .map_err(GoldenStartingMarketFindFriendsRingError::Rollback)?;
            rollback_market_find_friends_steps(&completed_steps, scratch)
                .map_err(GoldenStartingMarketFindFriendsRingError::Rollback)?;
            return Err(GoldenStartingMarketFindFriendsRingError::InvalidFoundBuild);
        };
        if i32::from(found.city) == effective_city_filter {
            return Ok(
                GoldenStartingMarketFindFriendsRingAdvance::FoundBuildTypeBoundary(
                    GoldenStartingMarketFindFriendsTypeBoundaryReceipt {
                        first_lookup,
                        completed_steps,
                        scratch_entry,
                        scratch_at_boundary: *scratch,
                        object_lookup: lookup,
                        found,
                        accumulator_before: 0,
                        first_unowned_virtual_slot:
                            MARKET_FIND_FRIENDS_FIRST_FOUND_TYPE_VIRTUAL_SLOT,
                        source_produced_city_bytes: 0,
                        installed_in_scoreboard: false,
                    },
                ),
            );
        }
        completed_steps.push(GoldenStartingMarketFindFriendsRingStep {
            circle_offset,
            world_cell,
            child: Some(child),
            object_lookup: Some(lookup),
            city_mismatch: Some(found),
            accumulator_before: 0,
            accumulator_after: 0,
        });
    }

    Ok(GoldenStartingMarketFindFriendsRingAdvance::ReturnedZero(
        GoldenStartingMarketFindFriendsReturnedZeroReceipt {
            first_lookup,
            remaining_steps: completed_steps,
            scratch_entry,
            scratch_after: *scratch,
            returned: 0,
            source_produced_city_bytes: 0,
            installed_in_scoreboard: false,
        },
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketFoundBuildRejectReason {
    GatherTypeOutsideUniversity,
    GatherEnhancer { relation_type: i32 },
    UncapturedMilitaryTrainer,
    Wonder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketFoundBuildPredicateOutcome {
    Rejected(GoldenStartingMarketFoundBuildRejectReason),
    Counted { weight: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketAfterFoundBuildContinuation {
    NextRingOffset {
        circle_offset: i32,
        accumulator: i32,
    },
    Returned {
        value: i32,
    },
}

/// Exact lazy Type/Build reads after a same-City found Build reaches vtable slot `+0x90`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFoundBuildTypeReads {
    pub type_rows: usize,
    pub mutation_revision: u64,
    pub found_type_index: i32,
    pub found_build_flags: u32,
    pub is_gather_type: bool,
    /// Reached only for a gather type.
    pub university_relation: Option<bool>,
    /// Non-strict relations 423..=426 in native short-circuit order. Entries after the first
    /// true result remain absent.
    pub gather_enhancer_relations: [Option<bool>; 4],
    /// Recursive `BuildTypeData::basic_type` chain, reached only after the preceding rejects pass.
    pub basic_type_call_va: Option<u32>,
    pub basic_type_chain: Vec<i32>,
    pub basic_type: Option<i32>,
    pub basic_type_build_flags: Option<u32>,
    pub military_trainer: Option<bool>,
    /// Live Object flag `CAPTURED`; lazily read only for a military trainer.
    pub captured: Option<bool>,
    /// `TypeData::is_wonder_type`, reached only after the military-trainer gate passes.
    pub wonder_virtual_slot: Option<u32>,
    pub is_wonder_type: Option<bool>,
}

/// Complete exact predicate sequence for one same-City found Build. This receipt owns no Object,
/// scratch, Sim, or City mutation and stops before the next ring lookup or caller scoring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketFoundBuildPredicateReceipt {
    pub input: GoldenStartingMarketFindFriendsTypeBoundaryReceipt,
    pub reads: GoldenStartingMarketFoundBuildTypeReads,
    pub outcome: GoldenStartingMarketFoundBuildPredicateOutcome,
    pub accumulator_before: i32,
    pub accumulator_after: i32,
    pub continuation: GoldenStartingMarketAfterFoundBuildContinuation,
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

impl GoldenStartingMarketFoundBuildPredicateReceipt {
    pub fn validates_against(
        &self,
        before: &Sim,
        production: &LiveProductionRuntime,
        types: &TypeBuiltinState,
    ) -> bool {
        produce_golden_starting_market_found_build_predicates(
            before,
            production,
            types,
            self.input.clone(),
        )
        .is_ok_and(|expected| expected == *self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketFoundBuildPredicateError {
    InvalidTypeBoundary,
    MissingFoundType { type_index: i32 },
    FoundTypeIsNotBuilding { type_index: i32 },
    MissingFoundProductionType { type_index: i32 },
    FoundProductionTypeMismatch { type_index: i32 },
    InvalidBasicTypeChain { type_index: i32 },
    MissingBasicProductionType { type_index: i32 },
    BasicProductionTypeMismatch { type_index: i32 },
}

impl fmt::Display for GoldenStartingMarketFoundBuildPredicateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "golden starting Market found-Build predicates refused: {self:?}"
        )
    }
}

impl std::error::Error for GoldenStartingMarketFoundBuildPredicateError {}

fn market_type_relation(types: &TypeBuiltinState, type_index: usize, query: usize) -> bool {
    types.types.rows()[type_index]
        .is_list
        .iter()
        .any(|&candidate| usize::from(candidate) == query)
}

fn market_basic_type_chain(
    types: &TypeBuiltinState,
    type_index: i32,
) -> Result<Vec<i32>, GoldenStartingMarketFoundBuildPredicateError> {
    let mut chain = Vec::new();
    let mut current = type_index;
    loop {
        let index = usize::try_from(current)
            .ok()
            .filter(|&index| index < types.types.rows().len())
            .ok_or(
                GoldenStartingMarketFoundBuildPredicateError::InvalidBasicTypeChain {
                    type_index: current,
                },
            )?;
        if chain.contains(&current) || types.types.rows()[index].domain() != TypeDomain::Build {
            return Err(
                GoldenStartingMarketFoundBuildPredicateError::InvalidBasicTypeChain {
                    type_index: current,
                },
            );
        }
        chain.push(current);
        let from = types.types.rows()[index].from;
        if from < 0 {
            return Ok(chain);
        }
        current = from;
    }
}

/// Execute only the source-exact found-Build predicate chain after a same-City ring hit.
///
/// Type relations come from the canonical mutable `TypeBuiltinState`; Build flags come from the
/// exact live production projection; Object `CAPTURED` comes from the same hashed Sim used by the
/// ring. The native short-circuit shape is retained. Missing rows or basic-type facts fail closed.
/// The result stops before another Object lookup or any Market score calculation.
pub fn produce_golden_starting_market_found_build_predicates(
    before: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    input: GoldenStartingMarketFindFriendsTypeBoundaryReceipt,
) -> Result<
    GoldenStartingMarketFoundBuildPredicateReceipt,
    GoldenStartingMarketFoundBuildPredicateError,
> {
    if input.first_unowned_virtual_slot != MARKET_FIND_FRIENDS_FIRST_FOUND_TYPE_VIRTUAL_SLOT
        || input.source_produced_city_bytes != 0
        || input.installed_in_scoreboard
        || !GoldenStartingMarketFindFriendsRingAdvance::FoundBuildTypeBoundary(input.clone())
            .validates_against(before, production, types)
    {
        return Err(GoldenStartingMarketFoundBuildPredicateError::InvalidTypeBoundary);
    }

    let found_index = usize::try_from(input.found.type_index)
        .ok()
        .filter(|&index| index < types.types.rows().len())
        .ok_or(
            GoldenStartingMarketFoundBuildPredicateError::MissingFoundType {
                type_index: input.found.type_index,
            },
        )?;
    if types.types.rows()[found_index].domain() != TypeDomain::Build
        || !matches!(
            &types.types.rows()[found_index].body,
            TypeBody::Build { .. }
        )
    {
        return Err(
            GoldenStartingMarketFoundBuildPredicateError::FoundTypeIsNotBuilding {
                type_index: input.found.type_index,
            },
        );
    }
    let found_live = production
        .types
        .get(found_index)
        .and_then(Option::as_ref)
        .ok_or(
            GoldenStartingMarketFoundBuildPredicateError::MissingFoundProductionType {
                type_index: input.found.type_index,
            },
        )?;
    if found_live.type_index != input.found.type_index
        || found_live.class != LiveTypeClass::Building
    {
        return Err(
            GoldenStartingMarketFoundBuildPredicateError::FoundProductionTypeMismatch {
                type_index: input.found.type_index,
            },
        );
    }

    let accumulator_before = input.accumulator_before;
    let found_build_flags = found_live.build_flags;
    let is_gather_type = found_build_flags & BUILD_GATHER_TYPE_MASK != 0;
    let university_relation =
        is_gather_type.then(|| market_type_relation(types, found_index, UNIVERSITY_TYPE));
    let mut gather_enhancer_relations = [None; 4];
    let mut basic_type_call_va = None;
    let mut basic_type_chain = Vec::new();
    let mut basic_type = None;
    let mut basic_type_build_flags = None;
    let mut military_trainer = None;
    let mut captured = None;
    let mut wonder_virtual_slot = None;
    let mut is_wonder_type = None;

    let outcome = if university_relation == Some(false) {
        GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(
            GoldenStartingMarketFoundBuildRejectReason::GatherTypeOutsideUniversity,
        )
    } else {
        let mut enhancer = None;
        for (slot, &relation_type) in GATHER_ENHANCER_TYPES.iter().enumerate() {
            let related = market_type_relation(types, found_index, relation_type);
            gather_enhancer_relations[slot] = Some(related);
            if related {
                enhancer = Some(relation_type);
                break;
            }
        }
        if let Some(relation_type) = enhancer {
            GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(
                GoldenStartingMarketFoundBuildRejectReason::GatherEnhancer {
                    relation_type: relation_type as i32,
                },
            )
        } else {
            basic_type_call_va = Some(BUILD_TYPE_BASIC_TYPE_VA);
            basic_type_chain = market_basic_type_chain(types, input.found.type_index)?;
            let resolved_basic_type = *basic_type_chain
                .last()
                .expect("basic-type chain contains the found type");
            basic_type = Some(resolved_basic_type);
            let basic_index = usize::try_from(resolved_basic_type).map_err(|_| {
                GoldenStartingMarketFoundBuildPredicateError::InvalidBasicTypeChain {
                    type_index: resolved_basic_type,
                }
            })?;
            let basic_live = production
                .types
                .get(basic_index)
                .and_then(Option::as_ref)
                .ok_or(
                    GoldenStartingMarketFoundBuildPredicateError::MissingBasicProductionType {
                        type_index: resolved_basic_type,
                    },
                )?;
            if basic_live.type_index != resolved_basic_type
                || basic_live.class != LiveTypeClass::Building
            {
                return Err(
                    GoldenStartingMarketFoundBuildPredicateError::BasicProductionTypeMismatch {
                        type_index: resolved_basic_type,
                    },
                );
            }
            basic_type_build_flags = Some(basic_live.build_flags);
            let is_military_trainer = basic_live.build_flags & BUILD_MILITARY_TRAINER_MASK != 0;
            military_trainer = Some(is_military_trainer);
            if is_military_trainer {
                let build = before
                    .builds
                    .get(input.found.build_row)
                    .ok_or(GoldenStartingMarketFoundBuildPredicateError::InvalidTypeBoundary)?;
                let is_captured = build.flags & OBJECT_CAPTURED_MASK != 0;
                captured = Some(is_captured);
                if !is_captured {
                    GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(
                        GoldenStartingMarketFoundBuildRejectReason::UncapturedMilitaryTrainer,
                    )
                } else {
                    wonder_virtual_slot = Some(MARKET_FIND_FRIENDS_WONDER_VIRTUAL_SLOT);
                    let wonder =
                        (WONDER_TYPE_FIRST..WONDER_TYPE_END).contains(&input.found.type_index);
                    is_wonder_type = Some(wonder);
                    if wonder {
                        GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(
                            GoldenStartingMarketFoundBuildRejectReason::Wonder,
                        )
                    } else {
                        GoldenStartingMarketFoundBuildPredicateOutcome::Counted {
                            weight: if input.object_lookup.request.circle_offset & 1 == 0 {
                                2
                            } else {
                                1
                            },
                        }
                    }
                }
            } else {
                wonder_virtual_slot = Some(MARKET_FIND_FRIENDS_WONDER_VIRTUAL_SLOT);
                let wonder = (WONDER_TYPE_FIRST..WONDER_TYPE_END).contains(&input.found.type_index);
                is_wonder_type = Some(wonder);
                if wonder {
                    GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(
                        GoldenStartingMarketFoundBuildRejectReason::Wonder,
                    )
                } else {
                    GoldenStartingMarketFoundBuildPredicateOutcome::Counted {
                        weight: if input.object_lookup.request.circle_offset & 1 == 0 {
                            2
                        } else {
                            1
                        },
                    }
                }
            }
        }
    };

    let accumulator_after = match outcome {
        GoldenStartingMarketFoundBuildPredicateOutcome::Rejected(_) => accumulator_before,
        GoldenStartingMarketFoundBuildPredicateOutcome::Counted { weight } => {
            accumulator_before.wrapping_add(weight)
        }
    };
    let circle_offset = input.object_lookup.request.circle_offset;
    let continuation = if circle_offset < 8 {
        GoldenStartingMarketAfterFoundBuildContinuation::NextRingOffset {
            circle_offset: circle_offset + 1,
            accumulator: accumulator_after,
        }
    } else {
        GoldenStartingMarketAfterFoundBuildContinuation::Returned {
            value: accumulator_after,
        }
    };
    Ok(GoldenStartingMarketFoundBuildPredicateReceipt {
        input,
        reads: GoldenStartingMarketFoundBuildTypeReads {
            type_rows: types.types.rows().len(),
            mutation_revision: types.mutation_revision(),
            found_type_index: found_index as i32,
            found_build_flags,
            is_gather_type,
            university_relation,
            gather_enhancer_relations,
            basic_type_call_va,
            basic_type_chain,
            basic_type,
            basic_type_build_flags,
            military_trainer,
            captured,
            wonder_virtual_slot,
            is_wonder_type,
        },
        outcome,
        accumulator_before,
        accumulator_after,
        continuation,
        source_produced_city_bytes: 0,
        installed_in_scoreboard: false,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketCaptureSource {
    /// A supported retail process was captured at `Leader::produce_building` entry/return,
    /// with every fine-grid `Random::get` call recorded in program order.
    CompleteRetailLeaderProduceBuildingReturn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenMarketFineRandomDraw {
    pub call_va: u32,
    pub callee_va: u32,
    pub placement_coord: [i32; 2],
    pub state_before: i32,
    pub raw: i32,
    pub remainder: i32,
    pub state_after: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketCapture {
    pub revision: u64,
    pub source: GoldenStartingMarketCaptureSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub before_sim_sha256: [u8; 32],
    pub after_sim_sha256: [u8; 32],
    pub native_trace_sha256: [u8; 32],
    pub footprint_receipt_sha256: [u8; 32],
    /// Market skips the Farm-only coarse `% 500` draw.
    pub coarse_random_draws: u32,
    pub fine_random_draws: Vec<GoldenMarketFineRandomDraw>,
    pub selected_placement_coord: [i32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenMarketCityChecksumByteWrite {
    pub offset: u16,
    pub before: u8,
    pub after: u8,
    pub writer_va: u32,
    pub value_changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketCityReceipt {
    pub placement: GoldenStartingMarketPlacementReceipt,
    /// Execution-derived read-only placement acceptance on the hashed pre-Market Sim.
    pub blocked_location: LeaderProduceBuildingMarketBlockedLocationReceipt,
    /// Execution-derived type/World prefix of the first scoring child. Its typed
    /// `find_building_placed_at` child is retained separately below.
    pub find_friends: BuildTypeFindFriendsReceipt,
    /// Complete execution-derived first spatial lookup on the same pre-Market Sim.
    pub first_object_lookup: ObjectsFindBuildingPlacedAtReceipt,
    pub objects_selected_owner_before: ObjectsSelectedOwnerAuthority,
    pub objects_selected_owner_after: ObjectsSelectedOwnerAuthority,
    /// The exact unexecuted `find_friends` resume boundary after the first child returns.
    pub find_friends_after_first_lookup: GoldenStartingMarketFindFriendsAfterFirstLookupBoundary,
    pub capture_revision: u64,
    pub source: GoldenStartingMarketCaptureSource,
    pub executable_sha256: [u8; 32],
    pub native_trace_sha256: [u8; 32],
    pub footprint_receipt_sha256: [u8; 32],
    pub before_sim_sha256: [u8; 32],
    pub after_sim_sha256: [u8; 32],
    pub before_cities: CitiesChannelValue,
    pub after_cities: CitiesChannelValue,
    pub center_build_row: usize,
    pub market_build_row: usize,
    pub market_build_o: i32,
    pub market_city_slot: i16,
    pub space_grade: i32,
    pub city_checksum_writes: Vec<GoldenMarketCityChecksumByteWrite>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub fine_random_draws: Vec<GoldenMarketFineRandomDraw>,
    pub selected_placement_coord: [i32; 2],
    /// One complete active empty-caravan City remains a 114-byte walk; this receipt owns
    /// the exact changed image, not a second traversal.
    pub source_produced_city_bytes: u64,
    pub installed_in_scoreboard: bool,
}

/// Immutable join between the separate schema-v1 Market lifecycle capture, the exact Market
/// transaction receipt, and the schema-v2 oracle's post-Market `Setup::build_units` entry image.
/// Setup production consumes this authority instead of accepting an object-shaped post-image as
/// proof that the Market transaction ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenStartingMarketSetupEntryAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub schema_version: u64,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub before_sim_sha256: [u8; 32],
    pub entry_sim_sha256: [u8; 32],
    pub native_trace_sha256: [u8; 32],
    pub footprint_receipt_sha256: [u8; 32],
    pub center_build_row: usize,
    pub market_build_row: usize,
    pub market_build_o: i32,
    pub market_city_slot: i16,
    pub space_grade: i32,
    pub random_state_after: i32,
    pub source_produced_city_bytes: u64,
}

/// Stable digest over every public setup-entry join claim other than the digest itself.
pub fn golden_starting_market_setup_entry_digest(
    authority: &GoldenStartingMarketSetupEntryAuthority,
) -> [u8; 32] {
    let mut image = b"don-golden-starting-market-setup-entry-v1".to_vec();
    image.extend_from_slice(&authority.revision.to_le_bytes());
    image.extend_from_slice(&authority.schema_version.to_le_bytes());
    image.extend_from_slice(&authority.replay_file_sha256);
    image.extend_from_slice(&authority.executable_sha256);
    image.extend_from_slice(&authority.before_sim_sha256);
    image.extend_from_slice(&authority.entry_sim_sha256);
    image.extend_from_slice(&authority.native_trace_sha256);
    image.extend_from_slice(&authority.footprint_receipt_sha256);
    image.extend_from_slice(&(authority.center_build_row as u64).to_le_bytes());
    image.extend_from_slice(&(authority.market_build_row as u64).to_le_bytes());
    image.extend_from_slice(&authority.market_build_o.to_le_bytes());
    image.extend_from_slice(&authority.market_city_slot.to_le_bytes());
    image.extend_from_slice(&authority.space_grade.to_le_bytes());
    image.extend_from_slice(&authority.random_state_after.to_le_bytes());
    image.extend_from_slice(&authority.source_produced_city_bytes.to_le_bytes());
    sha256(&image)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketBindError {
    Plan(GoldenStartingMarketPlanError),
    InvalidPlacementReceipt,
    MissingCaptureRevision,
    ReplayMismatch,
    UnsupportedExecutable,
    MissingTraceIdentity,
    FootprintReceiptMismatch,
    Snapshot(SaveError),
    BeforeSnapshotMismatch,
    AfterSnapshotMismatch,
    WrongFrame { before: i32, after: i32 },
    WrongRandomTrace,
    MissingCenterBuild,
    CenterBuildMismatch,
    MarketAlreadyPresent,
    MissingMarketBuild,
    MarketBuildMismatch,
    BuildAllocationMismatch,
    UnitAllocationNotFresh,
    MissingCity,
    CityBeforeMismatch,
    CityMutationMismatch,
    OtherCityMutation,
    Cities(CitiesRuntimeError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenStartingMarketLeaderAccountingError {
    InvalidSetupEntryAuthority,
    Snapshot(SaveError),
    EntrySnapshotMismatch,
    WorldRegionUnavailable,
    Accounting(MarketLeaderAccountingError),
}

impl fmt::Display for GoldenStartingMarketLeaderAccountingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "golden starting Market Leader accounting refused: {self:?}"
        )
    }
}

impl std::error::Error for GoldenStartingMarketLeaderAccountingError {}

impl From<MarketLeaderAccountingError> for GoldenStartingMarketLeaderAccountingError {
    fn from(value: MarketLeaderAccountingError) -> Self {
        Self::Accounting(value)
    }
}

impl fmt::Display for GoldenStartingMarketBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "golden starting Market City receipt refused: {self:?}")
    }
}

impl std::error::Error for GoldenStartingMarketBindError {}

pub fn golden_market_footprint_receipt_sha256(
    receipt: &LeaderProduceBuildingBlockedSiteRawZeroFootprintReceipt,
) -> [u8; 32] {
    let mut image = b"don-golden-market-footprint-v1".to_vec();
    let input = receipt.entry.input;
    image.push(input.owner);
    image.extend_from_slice(&input.type_index.to_le_bytes());
    image.extend_from_slice(&input.origin_build_object.to_le_bytes());
    image.extend_from_slice(&input.mode.to_le_bytes());
    image.extend_from_slice(&input.circle_offset.to_le_bytes());
    image.extend_from_slice(&input.space_grade.to_le_bytes());
    for value in input
        .candidate_world_cell
        .into_iter()
        .chain(input.placement_coord)
        .chain(receipt.entry.footprint_corner)
    {
        image.extend_from_slice(&value.to_le_bytes());
    }
    for tile in &receipt.tiles {
        for value in tile.input.tile {
            image.extend_from_slice(&value.to_le_bytes());
        }
        image.push(u8::from(tile.was_seen.unwrap_or(false)));
        image.extend_from_slice(&tile.world_region.unwrap_or(-1).to_le_bytes());
        image.extend_from_slice(&tile.terrain_mask.unwrap_or_default().to_le_bytes());
        image.extend_from_slice(
            &tile
                .raw_returned_to_blocked_site
                .unwrap_or(-1)
                .to_le_bytes(),
        );
    }
    sha256(&image)
}

fn expected_city_after(before: &CityRecord, grade: i32) -> Option<CityRecord> {
    let last = usize::try_from(grade.checked_sub(2)?).ok()?;
    if last >= before.space.len() {
        return None;
    }
    let mut expected = before.clone();
    expected.city_flags |= MARKET_CITY_FLAG;
    expected.filled = expected.filled.wrapping_add(1);
    for value in &mut expected.space[..=last] {
        *value = value.saturating_sub(1);
    }
    Some(expected)
}

fn city_checksum_writes(
    before: &CityRecord,
    after: &CityRecord,
    grade: i32,
) -> Vec<GoldenMarketCityChecksumByteWrite> {
    let mut writes = Vec::new();
    for (byte, (before, after)) in before
        .city_flags
        .to_le_bytes()
        .into_iter()
        .zip(after.city_flags.to_le_bytes())
        .enumerate()
    {
        writes.push(GoldenMarketCityChecksumByteWrite {
            offset: CITY_FLAGS_OFFSET + byte as u16,
            before,
            after,
            writer_va: BUILD_ACTIVATE_VA,
            value_changed: before != after,
        });
    }
    writes.push(GoldenMarketCityChecksumByteWrite {
        offset: CITY_FILLED_OFFSET,
        before: before.filled,
        after: after.filled,
        writer_va: LEADER_PRODUCE_BUILDING_VA,
        value_changed: before.filled != after.filled,
    });
    for index in 0..=usize::try_from(grade - 2).expect("validated Market space grade") {
        writes.push(GoldenMarketCityChecksumByteWrite {
            offset: CITY_SPACE_OFFSET + index as u16,
            before: before.space[index],
            after: after.space[index],
            writer_va: LEADER_PRODUCE_BUILDING_VA,
            value_changed: before.space[index] != after.space[index],
        });
    }
    writes
}

fn validate_fine_random_trace(
    before_state: i32,
    after_state: i32,
    corner: [i32; 2],
    selected: [i32; 2],
    capture: &GoldenStartingMarketCapture,
) -> bool {
    if capture.coarse_random_draws != 0
        || capture.fine_random_draws.is_empty()
        || capture.fine_random_draws.len() > 4
    {
        return false;
    }
    let allowed: Vec<[i32; 2]> = (corner[0]..=corner[0] + 1)
        .flat_map(|tx| {
            (corner[1]..=corner[1] + 1)
                .map(move |ty| [tx.wrapping_mul(192) + 384, ty.wrapping_mul(192) + 384])
        })
        .collect();
    let mut previous_index = None;
    let mut random = Random::new(before_state);
    let mut best = None;
    for (draw_ordinal, draw) in capture.fine_random_draws.iter().enumerate() {
        let Some(index) = allowed
            .iter()
            .position(|&site| site == draw.placement_coord)
        else {
            return false;
        };
        // The first 2x2 probe is the accepted coarse site. Retail therefore reaches the fine
        // RNG call for index zero before it can consider any later fine site.
        if draw_ordinal == 0 && index != 0 {
            return false;
        }
        if previous_index.is_some_and(|previous| index <= previous) {
            return false;
        }
        previous_index = Some(index);
        if draw.call_va != LEADER_PRODUCE_BUILDING_FINE_RANDOM_CALL_VA
            || draw.callee_va != RANDOM_GET_VA
            || draw.state_before != random.state()
        {
            return false;
        }
        let raw = random.get(0, 0xffff);
        if draw.raw != raw || draw.remainder != raw % 100 || draw.state_after != random.state() {
            return false;
        }
        if best.is_none_or(|(score, _)| draw.remainder >= score) {
            best = Some((draw.remainder, draw.placement_coord));
        }
    }
    random.state() == after_state
        && best.is_some_and(|(_, site)| site == selected)
        && capture.selected_placement_coord == selected
}

fn build_row(sim: &Sim, object: i32) -> Option<usize> {
    let identity = sim
        .world
        .object_bands()
        .live_identity(RetailObjectAddress::new(OWNER, RetailBand::Build, object))?;
    let WorldObjectIdentity::BuildRow(row) = identity else {
        return None;
    };
    Some(row as usize)
}

/// Bind a complete supported-retail Market call to the exact golden City checksum after-image.
/// The placement and first Object-lookup receipts are mandatory: an object-shaped after-image
/// without its generated-World and Objects scratch provenance is refused.
pub fn bind_golden_starting_market_city(
    replay: &Replay,
    before: &Sim,
    after: &Sim,
    production: &LiveProductionRuntime,
    types: &TypeBuiltinState,
    first_lookup: GoldenStartingMarketFirstObjectLookupReceipt,
    capture: &GoldenStartingMarketCapture,
) -> Result<GoldenStartingMarketCityReceipt, GoldenStartingMarketBindError> {
    if !first_lookup.validates() {
        return Err(GoldenStartingMarketBindError::InvalidPlacementReceipt);
    }
    let GoldenStartingMarketFirstObjectLookupReceipt {
        accepted,
        object_lookup: first_object_lookup,
        scratch_before: objects_selected_owner_before,
        scratch_after: objects_selected_owner_after,
        next: find_friends_after_first_lookup,
        source_produced_city_bytes: first_lookup_city_bytes,
        installed_in_scoreboard: first_lookup_installed,
    } = first_lookup;
    let GoldenStartingMarketAcceptedPlacementReceipt {
        placement,
        blocked_location,
        find_friends,
        before_sim_sha256: accepted_before_sim_sha256,
        source_produced_city_bytes: accepted_city_bytes,
        installed_in_scoreboard: accepted_installed,
    } = accepted;
    let plan =
        derive_golden_starting_market_plan(replay).map_err(GoldenStartingMarketBindError::Plan)?;
    if placement.plan != plan
        || !placement.footprint.validates()
        || placement.candidate.continuation != Some(placement.footprint.entry.input)
        || !placement.blocked_location.validates()
        || placement.blocked_location.input != placement.footprint
        || placement.source_produced_city_bytes != 0
        || placement.installed_in_scoreboard
        || !blocked_location.validates()
        || blocked_location.input != placement.blocked_location
        || !find_friends.validates()
        || find_friends.request != market_find_friends_request(blocked_location.next_child)
        || find_friends.stop != BuildTypeFindFriendsStop::FirstObjectLookup
        || find_friends.returned.is_some()
        || find_friends.first_child.is_none()
        || accepted_city_bytes != 0
        || accepted_installed
        || first_lookup_city_bytes != 0
        || first_lookup_installed
    {
        return Err(GoldenStartingMarketBindError::InvalidPlacementReceipt);
    }
    if capture.revision == 0 {
        return Err(GoldenStartingMarketBindError::MissingCaptureRevision);
    }
    if capture.replay_file_sha256 != plan.replay_file_sha256 {
        return Err(GoldenStartingMarketBindError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(GoldenStartingMarketBindError::UnsupportedExecutable);
    }
    if capture.native_trace_sha256 == [0; 32] {
        return Err(GoldenStartingMarketBindError::MissingTraceIdentity);
    }
    if capture.footprint_receipt_sha256
        != golden_market_footprint_receipt_sha256(&placement.footprint)
    {
        return Err(GoldenStartingMarketBindError::FootprintReceiptMismatch);
    }
    let before_sim_sha256 =
        frame379_setup_snapshot_sha256(before).map_err(GoldenStartingMarketBindError::Snapshot)?;
    let after_sim_sha256 =
        frame379_setup_snapshot_sha256(after).map_err(GoldenStartingMarketBindError::Snapshot)?;
    if capture.before_sim_sha256 != before_sim_sha256
        || accepted_before_sim_sha256 != before_sim_sha256
        || !blocked_location
            .tregion
            .validates_against(&before.map.world)
        || !find_friends.validates_against(types, production, &before.map.world)
        || !first_object_lookup.validates_against(before, production)
        || first_object_lookup.scratch.before != objects_selected_owner_before
        || first_object_lookup.scratch.after != Some(objects_selected_owner_after)
        || !find_friends_after_first_lookup.validates_against(&find_friends, &first_object_lookup)
    {
        return Err(GoldenStartingMarketBindError::BeforeSnapshotMismatch);
    }
    if capture.after_sim_sha256 != after_sim_sha256 {
        return Err(GoldenStartingMarketBindError::AfterSnapshotMismatch);
    }
    if before.world.frame != 0 || after.world.frame != 0 {
        return Err(GoldenStartingMarketBindError::WrongFrame {
            before: before.world.frame,
            after: after.world.frame,
        });
    }

    let site = placement.footprint.entry.input;
    if !validate_fine_random_trace(
        before.world.random.state(),
        after.world.random.state(),
        placement.footprint.entry.footprint_corner,
        capture.selected_placement_coord,
        capture,
    ) {
        return Err(GoldenStartingMarketBindError::WrongRandomTrace);
    }

    let center_build_row = build_row(before, STARTING_VILLAGE_O)
        .ok_or(GoldenStartingMarketBindError::MissingCenterBuild)?;
    let center_before = before
        .builds
        .get(center_build_row)
        .ok_or(GoldenStartingMarketBindError::MissingCenterBuild)?;
    if center_before.who != OWNER
        || i32::from(center_before.object_id()) != STARTING_VILLAGE_O
        || center_before.city != STARTING_CITY_SLOT
        || center_before.city_down != -1
        || center_before.flags & (flag::VALID | flag::STARTED | flag::ACTIVE | 0x20)
            != (flag::VALID | flag::STARTED | flag::ACTIVE | 0x20)
    {
        return Err(GoldenStartingMarketBindError::CenterBuildMismatch);
    }
    if build_row(before, DUTCH_STARTING_MARKET_O).is_some() {
        return Err(GoldenStartingMarketBindError::MarketAlreadyPresent);
    }
    let market_build_row = build_row(after, DUTCH_STARTING_MARKET_O)
        .ok_or(GoldenStartingMarketBindError::MissingMarketBuild)?;
    let center_after = after
        .builds
        .get(center_build_row)
        .ok_or(GoldenStartingMarketBindError::MissingCenterBuild)?;
    let market = after
        .builds
        .get(market_build_row)
        .ok_or(GoldenStartingMarketBindError::MissingMarketBuild)?;
    let required_market_flags = flag::VALID | flag::STARTED | flag::ACTIVE;
    if market.who != OWNER
        || i32::from(market.object_id()) != DUTCH_STARTING_MARKET_O
        || market.city != STARTING_CITY_SLOT
        || market.city_down != -1
        || market.position()
            != (
                capture.selected_placement_coord[0],
                capture.selected_placement_coord[1],
            )
        || market.flags & required_market_flags != required_market_flags
        || market.flags & 0x20 != 0
        || market.queue.num() != MARKET_BUILD_QUEUE_SLOTS
        || market.queue.queued != 0
        || after
            .production_runtime
            .build_types
            .get(market_build_row)
            .and_then(|value| *value)
            != Some(DUTCH_STARTING_MARKET_TYPE)
        || center_after.city_down != DUTCH_STARTING_MARKET_O as i16
    {
        return Err(GoldenStartingMarketBindError::MarketBuildMismatch);
    }
    if before.world.objects.slot(OWNER as usize).mark(Band::Build) != DUTCH_STARTING_MARKET_O as u32
        || after.world.objects.slot(OWNER as usize).mark(Band::Build)
            != DUTCH_STARTING_MARKET_O as u32 + 1
        || before.builds.len() + 1 != after.builds.len()
        || market_build_row + 1 != after.builds.len()
        || market_build_row != before.builds.len()
        || DUTCH_STARTING_MARKET_O < BUILD_BAND_BASE as i32
    {
        return Err(GoldenStartingMarketBindError::BuildAllocationMismatch);
    }
    if before.world.unit_mark(OWNER as usize) != Some(0)
        || after.world.unit_mark(OWNER as usize) != Some(0)
    {
        return Err(GoldenStartingMarketBindError::UnitAllocationNotFresh);
    }

    let before_city = before
        .cities
        .slots
        .get(OWNER as usize)
        .and_then(|cities| cities.get(STARTING_CITY_SLOT as usize))
        .ok_or(GoldenStartingMarketBindError::MissingCity)?;
    let after_city = after
        .cities
        .slots
        .get(OWNER as usize)
        .and_then(|cities| cities.get(STARTING_CITY_SLOT as usize))
        .ok_or(GoldenStartingMarketBindError::MissingCity)?;
    if before_city.city_flags != FRESH_CITY_FLAGS
        || before_city.city != STARTING_CITY_SLOT
        || before_city.o != STARTING_VILLAGE_O as i16
        || before_city.who != OWNER as i8
        || before_city.filled != 1
    {
        return Err(GoldenStartingMarketBindError::CityBeforeMismatch);
    }
    let expected = expected_city_after(before_city, site.space_grade)
        .ok_or(GoldenStartingMarketBindError::CityMutationMismatch)?;
    if site.space_grade != 4
        || expected.city_flags != MARKET_CITY_FLAGS
        || expected.filled != 2
        || after_city != &expected
    {
        return Err(GoldenStartingMarketBindError::CityMutationMismatch);
    }
    if before.cities.city_mark != after.cities.city_mark
        || before.cities.slots.len() != after.cities.slots.len()
    {
        return Err(GoldenStartingMarketBindError::OtherCityMutation);
    }
    for owner in 0..before.cities.slots.len() {
        let before_slots = &before.cities.slots[owner];
        let after_slots = after
            .cities
            .slots
            .get(owner)
            .ok_or(GoldenStartingMarketBindError::OtherCityMutation)?;
        if before_slots.len() != after_slots.len() {
            return Err(GoldenStartingMarketBindError::OtherCityMutation);
        }
        for slot in 0..before_slots.len() {
            if owner == OWNER as usize && slot == STARTING_CITY_SLOT as usize {
                continue;
            }
            if before_slots[slot] != after_slots[slot] {
                return Err(GoldenStartingMarketBindError::OtherCityMutation);
            }
        }
    }

    let before_cities =
        check_sim_owned_cities(before).map_err(GoldenStartingMarketBindError::Cities)?;
    let after_cities =
        check_sim_owned_cities(after).map_err(GoldenStartingMarketBindError::Cities)?;
    let city_checksum_writes = city_checksum_writes(before_city, after_city, site.space_grade);
    Ok(GoldenStartingMarketCityReceipt {
        placement,
        blocked_location,
        find_friends,
        first_object_lookup,
        objects_selected_owner_before,
        objects_selected_owner_after,
        find_friends_after_first_lookup,
        capture_revision: capture.revision,
        source: capture.source,
        executable_sha256: capture.executable_sha256,
        native_trace_sha256: capture.native_trace_sha256,
        footprint_receipt_sha256: capture.footprint_receipt_sha256,
        before_sim_sha256,
        after_sim_sha256,
        before_cities,
        after_cities,
        center_build_row,
        market_build_row,
        market_build_o: DUTCH_STARTING_MARKET_O,
        market_city_slot: STARTING_CITY_SLOT,
        space_grade: site.space_grade,
        city_checksum_writes,
        random_state_before: before.world.random.state(),
        random_state_after: after.world.random.state(),
        fine_random_draws: capture.fine_random_draws.clone(),
        selected_placement_coord: capture.selected_placement_coord,
        source_produced_city_bytes: after_cities.bytes_walked,
        installed_in_scoreboard: false,
    })
}

/// Consume the digest-bound lifecycle/setup-entry join and mount the missing Leader-owned half
/// of the same retail Market call. The current Sim must still equal the authority's post-Market
/// setup-entry snapshot; the commit changes only canonical Leader accounting and its exact
/// step-8 flag mirror.
pub fn commit_golden_starting_market_leader_accounting(
    sim: &mut Sim,
    authority: &GoldenStartingMarketSetupEntryAuthority,
) -> Result<MarketLeaderAccountingReceipt, GoldenStartingMarketLeaderAccountingError> {
    if authority.revision == 0
        || authority.composition_digest == [0; 32]
        || authority.composition_digest != golden_starting_market_setup_entry_digest(authority)
        || authority.schema_version != GOLDEN_STARTING_MARKET_SETUP_ENTRY_SCHEMA_VERSION
        || authority.replay_file_sha256 != REPLAY_FILE_SHA256
        || authority.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || authority.before_sim_sha256 == [0; 32]
        || authority.entry_sim_sha256 == [0; 32]
        || authority.before_sim_sha256 == authority.entry_sim_sha256
        || authority.native_trace_sha256 == [0; 32]
        || authority.footprint_receipt_sha256 == [0; 32]
        || authority.market_build_o != DUTCH_STARTING_MARKET_O
        || authority.market_city_slot != STARTING_CITY_SLOT
        || authority.space_grade != 4
        || authority.source_produced_city_bytes == 0
        || sim.world.random.state() != authority.random_state_after
        || build_row(sim, STARTING_VILLAGE_O) != Some(authority.center_build_row)
        || build_row(sim, DUTCH_STARTING_MARKET_O) != Some(authority.market_build_row)
    {
        return Err(GoldenStartingMarketLeaderAccountingError::InvalidSetupEntryAuthority);
    }
    let snapshot = frame379_setup_snapshot_sha256(sim)
        .map_err(GoldenStartingMarketLeaderAccountingError::Snapshot)?;
    if snapshot != authority.entry_sim_sha256 {
        return Err(GoldenStartingMarketLeaderAccountingError::EntrySnapshotMismatch);
    }

    let market = sim
        .builds
        .get(authority.market_build_row)
        .ok_or(GoldenStartingMarketLeaderAccountingError::InvalidSetupEntryAuthority)?;
    let (x, y) = market.position();
    let wx = WCoord::from_coord(Coord(x)).0;
    let wy = WCoord::from_coord(Coord(y)).0;
    if !sim.map.world.valid_w(wx, wy) {
        return Err(GoldenStartingMarketLeaderAccountingError::WorldRegionUnavailable);
    }
    let region = u8::try_from(sim.map.world.wdata(wx, wy).region)
        .ok()
        .filter(|region| usize::from(*region) < 64)
        .ok_or(GoldenStartingMarketLeaderAccountingError::WorldRegionUnavailable)?;
    let source = MarketLeaderRegionAuthority {
        capture_revision: authority.revision,
        native_trace_sha256: authority.native_trace_sha256,
        after_sim_sha256: authority.entry_sim_sha256,
        owner: usize::from(OWNER),
        object_id: DUTCH_STARTING_MARKET_O as i16,
        type_index: DUTCH_STARTING_MARKET_TYPE,
        region,
    };
    let prepared = sim.prepare_golden_market_leader_accounting(source)?;
    sim.commit_golden_market_leader_accounting(source, prepared)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::systems::bhs_type_table::{
        LeaderTypeMasks, TribeRoster, TypeBackup, TypeRow, TypeTable, BUILD_BEGIN, BUILD_END,
        NUM_TRIBES, NUM_TYPES, REGULAR_UNIT_BEGIN, REGULAR_UNIT_END,
    };

    fn predicate_types(cycle: bool) -> TypeBuiltinState {
        let mut rows = (0..NUM_TYPES).map(TypeRow::empty).collect::<Vec<_>>();
        rows[415].from = 414;
        rows[415].is_list.push(423);
        if cycle {
            rows[414].from = 415;
        }
        let backups = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                ((REGULAR_UNIT_BEGIN..REGULAR_UNIT_END).contains(&index)
                    || (BUILD_BEGIN..BUILD_END).contains(&index))
                .then(|| TypeBackup::capture_pristine(row))
            })
            .collect();
        TypeBuiltinState::new(
            TypeTable::new(rows, backups).unwrap(),
            TribeRoster::new((0..NUM_TRIBES).map(|i| format!("Tribe {i}")).collect()).unwrap(),
            std::array::from_fn(|_| LeaderTypeMasks::default()),
        )
    }

    #[test]
    fn market_find_friends_request_preserves_the_native_parent_arguments() {
        let boundary = LeaderProduceBuildingFindFriendsBoundary {
            call_va: 0x006e_1f5f,
            callee_va: 0x0063_9270,
            owner: OWNER,
            type_index: DUTCH_STARTING_MARKET_TYPE,
            candidate_world_cell: [23, 31],
            origin_city_filter: STARTING_CITY_SLOT.into(),
        };
        let request = market_find_friends_request(boundary);
        assert_eq!(request.call_va, boundary.call_va);
        assert_eq!(request.callee_va, boundary.callee_va);
        assert_eq!(request.type_index, DUTCH_STARTING_MARKET_TYPE);
        assert_eq!(request.candidate_world_cell, [23, 31]);
        assert_eq!(request.city_filter, 0);
        assert_eq!(request.owner, 0);
        assert_eq!(request.native_push_order(), [0, 0, 31, 23]);
    }

    #[test]
    fn market_find_friends_ring_children_preserve_clockwise_offsets_and_pushes() {
        let request = BuildTypeFindFriendsRequest {
            call_va: 0x006e_1f5f,
            callee_va: BUILD_TYPE_FIND_FRIENDS_VA,
            type_index: DUTCH_STARTING_MARKET_TYPE,
            candidate_world_cell: [23, 31],
            city_filter: 0,
            owner: 0,
        };
        let children = (1..=8)
            .map(|circle_offset| {
                let (dx, dy) = WORLD_CELL_SEARCH_OFFSETS[circle_offset as usize];
                let world_cell = [
                    request.candidate_world_cell[0] + dx,
                    request.candidate_world_cell[1] + dy,
                ];
                market_find_friends_object_child(request, circle_offset, world_cell)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            children
                .iter()
                .map(|child| child.world_cell)
                .collect::<Vec<_>>(),
            [
                [22, 30],
                [23, 30],
                [24, 30],
                [24, 31],
                [24, 32],
                [23, 32],
                [22, 32],
                [22, 31],
            ]
        );
        assert_eq!(children[1].tile, [94, 122]);
        assert_eq!(children[1].native_push_order, [-1, -1, 0, 122, 94]);
        assert!(children.iter().enumerate().all(|(index, child)| {
            child.circle_offset == index as i32 + 1
                && child.call_va == BUILD_TYPE_FIND_FRIENDS_FIRST_OBJECT_CALL_VA
                && child.callee_va == OBJECTS_FIND_BUILDING_PLACED_AT_VA
                && child.selected_owner_offset == OBJECTS_SELECTED_OWNER_OFFSET
                && child.selected_owner_first_write == -1
        }));
    }

    #[test]
    fn market_found_build_relations_and_basic_type_chain_use_canonical_rows() {
        let types = predicate_types(false);
        assert!(market_type_relation(&types, 415, 423));
        assert!(!market_type_relation(&types, 415, 424));
        assert_eq!(market_basic_type_chain(&types, 415).unwrap(), [415, 414]);

        let cycle = predicate_types(true);
        assert_eq!(
            market_basic_type_chain(&cycle, 415),
            Err(
                GoldenStartingMarketFoundBuildPredicateError::InvalidBasicTypeChain {
                    type_index: 415,
                }
            )
        );
    }

    #[test]
    fn market_city_writes_are_exact_and_grade_bounded() {
        let before = CityRecord {
            city_flags: FRESH_CITY_FLAGS,
            filled: 1,
            space: [0, 2, 9],
            ..CityRecord::default()
        };
        let after = expected_city_after(&before, 4).unwrap();
        assert_eq!(after.city_flags, MARKET_CITY_FLAGS);
        assert_eq!(after.filled, 2);
        assert_eq!(after.space, [0, 1, 8]);
        assert!(expected_city_after(&before, 5).is_none());

        let writes = city_checksum_writes(&before, &after, 4);
        assert_eq!(
            writes.iter().map(|write| write.offset).collect::<Vec<_>>(),
            [4, 5, 100, 105, 106, 107]
        );
        assert!(!writes[0].value_changed);
        assert!(writes[1].value_changed);
        assert!(writes[2].value_changed);
        assert!(
            !writes[3].value_changed,
            "clamped zero is still an exact write"
        );
        assert!(writes[4].value_changed);
        assert!(writes[5].value_changed);
    }

    #[test]
    fn fine_random_trace_is_exact_and_rejects_coarse_draws() {
        let mut random = Random::new(7);
        let first_before = random.state();
        let first_raw = random.get(0, 0xffff);
        let first_after = random.state();
        let second_before = random.state();
        let second_raw = random.get(0, 0xffff);
        let second_after = random.state();
        let corner = [10, 20];
        let sites = [[2_304, 4_224], [2_304, 4_416]];
        let selected = if second_raw % 100 >= first_raw % 100 {
            sites[1]
        } else {
            sites[0]
        };
        let capture = GoldenStartingMarketCapture {
            revision: 1,
            source: GoldenStartingMarketCaptureSource::CompleteRetailLeaderProduceBuildingReturn,
            replay_file_sha256: REPLAY_FILE_SHA256,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            before_sim_sha256: [1; 32],
            after_sim_sha256: [2; 32],
            native_trace_sha256: [3; 32],
            footprint_receipt_sha256: [4; 32],
            coarse_random_draws: 0,
            fine_random_draws: vec![
                GoldenMarketFineRandomDraw {
                    call_va: LEADER_PRODUCE_BUILDING_FINE_RANDOM_CALL_VA,
                    callee_va: RANDOM_GET_VA,
                    placement_coord: sites[0],
                    state_before: first_before,
                    raw: first_raw,
                    remainder: first_raw % 100,
                    state_after: first_after,
                },
                GoldenMarketFineRandomDraw {
                    call_va: LEADER_PRODUCE_BUILDING_FINE_RANDOM_CALL_VA,
                    callee_va: RANDOM_GET_VA,
                    placement_coord: sites[1],
                    state_before: second_before,
                    raw: second_raw,
                    remainder: second_raw % 100,
                    state_after: second_after,
                },
            ],
            selected_placement_coord: selected,
        };
        assert!(validate_fine_random_trace(
            7,
            second_after,
            corner,
            selected,
            &capture,
        ));
        let mut skipped_first = capture.clone();
        skipped_first.fine_random_draws.truncate(1);
        skipped_first.fine_random_draws[0].placement_coord = sites[1];
        skipped_first.selected_placement_coord = sites[1];
        assert!(!validate_fine_random_trace(
            7,
            first_after,
            corner,
            sites[1],
            &skipped_first,
        ));

        let mut bad = capture;
        bad.coarse_random_draws = 1;
        assert!(!validate_fine_random_trace(
            7,
            second_after,
            corner,
            selected,
            &bad,
        ));
    }
}
