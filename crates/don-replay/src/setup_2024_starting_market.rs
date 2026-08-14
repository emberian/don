//! Golden 2024 Dutch starting-Market chronology and City after-image authority.
//!
//! `Setup::build_cities` calls `Setup::build_civ_specific` after the owner-0 Village and
//! before `Setup::build_units`. The golden replay selects Dutch bonus 22, so retail issues
//! exactly one `Leader::produce_building(436, 2000, 0)` call. This module derives that call
//! from replay/Rules bytes, joins the exact Market `blocked_site` footprint owner, executes the
//! first `find_friends` Object lookup under explicit scratch authority, and binds a
//! supported-retail pre/post capture to every City checksum byte written by the success suffix.
//! It never uses a recorded checksum and does not manufacture the missing generated World needed
//! to select the site.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::rng::Random;
use don_sim::systems::bhs_type_table::TypeBuiltinState;
use don_sim::systems::build_type_find_friends::{
    produce_build_type_find_friends_prefix, BuildTypeFindFriendsError, BuildTypeFindFriendsReceipt,
    BuildTypeFindFriendsRequest, BuildTypeFindFriendsStop, BUILD_TYPE_FIND_FRIENDS_VA,
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
    ObjectsSelectedOwnerCommitError,
};
use don_sim::systems::production::{flag, runtime::LiveProductionRuntime, Footprint};
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
