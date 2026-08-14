//! Exclusive builtin-357/386/436/455/520 bridge over canonical BHS, Sim, and production owners.
//!
//! The generic production-prefix host intentionally leaves stateful research red. This
//! adapter adds the measured single-Library path, builtin 332's immediate stock continuation,
//! builtin 386's City/Build census, builtin 436's Build-queue census, and builtin 455's live
//! idle-Unit census. It also composes builtin 520's exact frame-zero Farm placement through one
//! outer rollback boundary. Every other builtin is delegated to that strict prefix host.

use crate::builds_runtime::BuildsWalkAuthority;
use crate::leader_produce_building_farm_frame_zero_tail::{
    complete_leader_produce_building_farm_frame_zero_tail, FarmBuildInitAuthority,
    FarmFrameZeroSourceFacts, FarmFrameZeroTailReceipt,
};
use crate::replay_bhs_live_bindings::{
    ProductionBuiltinCall, ProductionBuiltinImage, ProductionBuiltinValue, ProductionDisposition,
    ProductionRunError, ProductionRunFailure, ProductionRunReceipt, ReplayProductionBuiltinHost,
    ReplayProductionCall,
};
use crate::replay_bhs_runtime::ReplayBhsBinding;
use crate::terrain_height_runtime::TerrainHeightAuthority;
use don_bhs::{BuiltinDecl, Host, HostError, HostResult, Value};
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::script_runtime::{
    ExternalGameSeconds, ExternalScriptFailure, ExternalTimerHost, ScriptRuntime,
};
use don_sim::systems::bhs_city_building_runtime::{
    apply_sim_city_building_count_transaction, CityBuildingCountReceipt, CityBuildingCountRequest,
    NUM_CITY_BUILDINGS_BUILTIN,
};
use don_sim::systems::bhs_create_unit_runtime::BhsCreateUnitRuntime;
use don_sim::systems::bhs_idle_unit_runtime::{
    apply_sim_idle_unit_census_transaction, IdleUnitCensusReceipt, IdleUnitCensusRequest,
    FIND_NUM_IDLE_UNIT_BUILTIN,
};
use don_sim::systems::bhs_place_building_runtime::{
    apply_sim_place_building_with_cost_prefix, PlaceBuildingCostAuthority, PlaceBuildingReceipt,
    PlaceBuildingRequest, PLACE_BUILDING_WITH_COST_BUILTIN,
};
use don_sim::systems::bhs_type_queue_runtime::{
    apply_sim_type_queue_count_transaction, TypeQueueCountReceipt, TypeQueueCountRequest,
    NUM_TYPE_QUEUED_BUILTIN,
};
use don_sim::systems::bhs_type_table::TypeBuiltinState;
use don_sim::systems::gather_terrain::GatherTerrainMaterialization;
use don_sim::systems::leader_produce_building_blocked_site_prefix::{
    apply_sim_build_type_blocked_tcoord_land_prefix,
    apply_sim_leader_produce_building_blocked_site_farm_owned_tail,
    apply_sim_leader_produce_building_blocked_site_land_footprint,
    apply_sim_leader_produce_building_blocked_site_prefix,
    apply_sim_leader_produce_building_farm_success_preflight, BuildTypeBlockedTcoordPrefixReceipt,
    LeaderProduceBuildingBlockedSiteFarmOwnedReceipt,
    LeaderProduceBuildingBlockedSiteFootprintReceipt,
    LeaderProduceBuildingBlockedSitePrefixReceipt,
    LeaderProduceBuildingFarmSuccessPreflightReceipt,
};
use don_sim::systems::leader_produce_building_candidate_prefix::{
    apply_sim_leader_produce_building_candidate_prefix, LeaderProduceBuildingCandidatePrefixReceipt,
};
use don_sim::systems::leader_produce_building_prefix::{
    apply_sim_leader_produce_building_prefix, LeaderProduceBuildingPrefixReceipt,
    LeaderProduceBuildingPrefixRequest,
};
use don_sim::systems::leader_produce_building_search_setup::{
    apply_sim_leader_produce_building_search_setup, LeaderProduceBuildingSearchSetupReceipt,
};
use don_sim::systems::production::flag;
use don_sim::systems::production::runtime::{
    apply_sim_single_library_research_transaction, LiveProductionRuntime,
    SingleLibraryResearchReceipt, SingleLibraryResearchRequest, SingleLibraryResearchStatus,
};
use don_sim::tick::Sim;

pub const RESEARCH_TECH_WITH_COST_BUILTIN: u32 = 357;
pub const AT_LEAST_TYPE_BUILTIN: u32 = 332;
pub const RESEARCH_FIND_COUNTER: usize = 30;
pub const LEADER_PRODUCE_BUILDING_FRAME_PAYMENT_GATE_VA: u32 = 0x006e_2c66;
pub const LEADER_PRODUCE_BUILDING_PAY_COST_CALL_VA: u32 = 0x006e_2c89;
pub const TYPE_PAY_COST_VA: u32 = 0x0066_81f0;

/// Source owners required to cross builtin 520's exact frame-zero Farm success arm.
///
/// The dynamic-cost authority stays the runner's canonical argument. Repeating its revision
/// and digest here prevents accidental composition against a different installed cost image.
pub struct FarmBuiltin520Authority<'a> {
    pub expected_cost_revision: u64,
    pub expected_cost_composition_digest: [u8; 32],
    pub gather_terrain: &'a GatherTerrainMaterialization,
    pub terrain_height: &'a TerrainHeightAuthority,
    pub source: FarmFrameZeroSourceFacts,
    pub build_authority: &'a mut FarmBuildInitAuthority,
    pub builds_walk: &'a mut BuildsWalkAuthority,
    pub expected_build_authority_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FarmBuiltin520CostReceipt {
    pub owner: u8,
    pub authority_revision: u64,
    pub composition_digest: [u8; 32],
    pub possible_goods: [bool; 6],
    pub resolved_costs: [i32; 6],
    pub resources_before: [i32; 6],
    pub resources_after: [i32; 6],
    /// The sole `Type::pay_cost` call is guarded by `Game::frame != 0`. The reached
    /// production witness is frame zero, so retail intentionally leaves stock unchanged.
    pub payment_skipped_at_frame_zero: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProductionResearchRunReceipt {
    pub production: ProductionRunReceipt,
    pub research: Vec<SingleLibraryResearchReceipt>,
    pub idle_units: Vec<IdleUnitCensusReceipt>,
    pub city_buildings: Vec<CityBuildingCountReceipt>,
    pub type_queues: Vec<TypeQueueCountReceipt>,
    pub place_buildings: Vec<PlaceBuildingReceipt>,
    pub produce_buildings: Vec<LeaderProduceBuildingPrefixReceipt>,
    pub produce_building_search_setups: Vec<LeaderProduceBuildingSearchSetupReceipt>,
    pub produce_building_candidate_prefixes: Vec<LeaderProduceBuildingCandidatePrefixReceipt>,
    pub produce_building_blocked_site_prefixes: Vec<LeaderProduceBuildingBlockedSitePrefixReceipt>,
    pub produce_building_blocked_tcoord_prefixes: Vec<BuildTypeBlockedTcoordPrefixReceipt>,
    pub produce_building_land_footprints: Vec<LeaderProduceBuildingBlockedSiteFootprintReceipt>,
    pub produce_building_owned_sites: Vec<LeaderProduceBuildingBlockedSiteFarmOwnedReceipt>,
    pub produce_building_success_preflights: Vec<LeaderProduceBuildingFarmSuccessPreflightReceipt>,
    pub farm_builtin_520_costs: Vec<FarmBuiltin520CostReceipt>,
    pub farm_frame_zero_tails: Vec<FarmFrameZeroTailReceipt>,
}

struct ResearchHost<'a, 'farm> {
    prefix: ReplayProductionBuiltinHost<'a>,
    image: &'a ProductionBuiltinImage,
    types: &'a TypeBuiltinState,
    upgrades: &'a BhsCreateUnitRuntime,
    place_building_costs: &'a PlaceBuildingCostAuthority,
    farm_520: Option<FarmBuiltin520Authority<'farm>>,
    sim: &'a mut Sim,
    production: &'a mut LiveProductionRuntime,
    trace: Vec<ProductionBuiltinCall>,
    research: Vec<SingleLibraryResearchReceipt>,
    idle_units: Vec<IdleUnitCensusReceipt>,
    city_buildings: Vec<CityBuildingCountReceipt>,
    type_queues: Vec<TypeQueueCountReceipt>,
    place_buildings: Vec<PlaceBuildingReceipt>,
    produce_buildings: Vec<LeaderProduceBuildingPrefixReceipt>,
    produce_building_search_setups: Vec<LeaderProduceBuildingSearchSetupReceipt>,
    produce_building_candidate_prefixes: Vec<LeaderProduceBuildingCandidatePrefixReceipt>,
    produce_building_blocked_site_prefixes: Vec<LeaderProduceBuildingBlockedSitePrefixReceipt>,
    produce_building_blocked_tcoord_prefixes: Vec<BuildTypeBlockedTcoordPrefixReceipt>,
    produce_building_land_footprints: Vec<LeaderProduceBuildingBlockedSiteFootprintReceipt>,
    produce_building_owned_sites: Vec<LeaderProduceBuildingBlockedSiteFarmOwnedReceipt>,
    produce_building_success_preflights: Vec<LeaderProduceBuildingFarmSuccessPreflightReceipt>,
    farm_builtin_520_costs: Vec<FarmBuiltin520CostReceipt>,
    farm_frame_zero_tails: Vec<FarmFrameZeroTailReceipt>,
    /// The replay setup byte is compared with the canonical mutable owner on
    /// the first global-difficulty access. Later accesses in the same atomic
    /// script call observe earlier admitted setter writes.
    difficulty_bound: bool,
}

impl<'a, 'farm> ResearchHost<'a, 'farm> {
    fn new(
        image: &'a ProductionBuiltinImage,
        types: &'a TypeBuiltinState,
        upgrades: &'a BhsCreateUnitRuntime,
        place_building_costs: &'a PlaceBuildingCostAuthority,
        farm_520: Option<FarmBuiltin520Authority<'farm>>,
        sim: &'a mut Sim,
        production: &'a mut LiveProductionRuntime,
    ) -> Self {
        Self {
            prefix: ReplayProductionBuiltinHost::new(image),
            image,
            types,
            upgrades,
            place_building_costs,
            farm_520,
            sim,
            production,
            trace: Vec::new(),
            research: Vec::new(),
            idle_units: Vec::new(),
            city_buildings: Vec::new(),
            type_queues: Vec::new(),
            place_buildings: Vec::new(),
            produce_buildings: Vec::new(),
            produce_building_search_setups: Vec::new(),
            produce_building_candidate_prefixes: Vec::new(),
            produce_building_blocked_site_prefixes: Vec::new(),
            produce_building_blocked_tcoord_prefixes: Vec::new(),
            produce_building_land_footprints: Vec::new(),
            produce_building_owned_sites: Vec::new(),
            produce_building_success_preflights: Vec::new(),
            farm_builtin_520_costs: Vec::new(),
            farm_frame_zero_tails: Vec::new(),
            difficulty_bound: false,
        }
    }

    fn who0(args: &[Value], at: usize) -> Result<Option<usize>, HostError> {
        let raw = match args.get(at) {
            Some(Value::Int(value)) => *value,
            _ => return Err(HostError::BadArgs("production who argument is not int")),
        };
        let who0 = raw.wrapping_sub(1);
        Ok((0..=7).contains(&who0).then_some(who0 as usize))
    }

    fn int_arg(args: &[Value], at: usize) -> Result<i32, HostError> {
        match args.get(at) {
            Some(Value::Int(value)) => Ok(*value),
            _ => Err(HostError::BadArgs("production argument is not int")),
        }
    }

    fn str_arg(args: &[Value], at: usize) -> Result<&str, HostError> {
        match args.get(at) {
            Some(Value::Str(value)) => Ok(value.as_str()),
            _ => Err(HostError::BadArgs("production argument is not string")),
        }
    }

    fn first_type(&self, query: &str) -> Result<Option<usize>, HostError> {
        if !query.is_ascii() {
            return Err(HostError::Unimplemented);
        }
        if query.is_empty() {
            return Ok(None);
        }
        Ok(self
            .types
            .types
            .rows()
            .iter()
            .position(|row| row.name.len() == query.len() && row.name.eq_ignore_ascii_case(query)))
    }

    fn reconciled_active_owner(&self, who0: usize) -> Result<bool, HostError> {
        let type_flags = self.types.leaders[who0].leader_flags;
        let image_flags = self.image.leaders[who0].flags;
        let victory_flags = self.sim.vic_leaders.slots[who0].leader_flags;
        let step8_flags = self.sim.step8.leaders[who0].flags;
        if image_flags != type_flags as u32
            || victory_flags != type_flags
            || step8_flags != type_flags as u32
        {
            return Err(HostError::Unimplemented);
        }
        Ok(type_flags & 3 == 3)
    }

    fn bind_global_difficulty(&mut self) -> Result<(), HostError> {
        if !self.difficulty_bound {
            let replay = self
                .image
                .setup
                .as_ref()
                .ok_or(HostError::Unimplemented)?
                .difficulty;
            if self.sim.vic_match.options.difficulty != replay {
                return Err(HostError::Unimplemented);
            }
            self.difficulty_bound = true;
        }
        Ok(())
    }

    /// `set_difficulty`, `0x009e52f0`: accept script ordinals 1..=6,
    /// publish the zero-based byte at `Game+0x2B`, and return one. Refusals
    /// return -1 without reading or writing the canonical owner.
    fn set_difficulty(&mut self, args: &[Value]) -> HostResult {
        let difficulty = Self::int_arg(args, 0)?;
        if !(1..=6).contains(&difficulty) {
            return Ok(Value::Int(-1));
        }
        self.bind_global_difficulty()?;
        self.sim.vic_match.options.difficulty = (difficulty - 1) as u8;
        Ok(Value::Int(1))
    }

    /// `set_leader_difficulty`, `0x009e5350`: unlike the global setter,
    /// retail stores the validated script value itself in `LeaderData+0x50`.
    fn set_leader_difficulty(&mut self, args: &[Value]) -> HostResult {
        let difficulty = Self::int_arg(args, 1)?;
        if !(1..=6).contains(&difficulty) {
            return Ok(Value::Int(-1));
        }
        let Some(who0) = Self::who0(args, 0)? else {
            return Ok(Value::Int(-1));
        };
        if !self.reconciled_active_owner(who0)? {
            return Ok(Value::Int(-1));
        }
        self.sim.vic_leaders.slots[who0].multi_diff = difficulty;
        Ok(Value::Int(1))
    }

    /// `get_leader_difficulty`, `0x009e53b0`: the same one-based Leader and
    /// both-low-flags guards, followed by the raw `LeaderData+0x50` dword.
    fn get_leader_difficulty(&self, args: &[Value]) -> HostResult {
        let Some(who0) = Self::who0(args, 0)? else {
            return Ok(Value::Int(-1));
        };
        if !self.reconciled_active_owner(who0)? {
            return Ok(Value::Int(-1));
        }
        Ok(Value::Int(self.sim.vic_leaders.slots[who0].multi_diff))
    }

    fn population(&self, builtin: u32, args: &[Value]) -> HostResult {
        let Some(who0) = Self::who0(args, 0)? else {
            return Ok(Value::Int(-1));
        };
        if !self.reconciled_active_owner(who0)? {
            return Ok(Value::Int(-1));
        }
        match builtin {
            245 => {
                let production = self.production.leaders[who0].control;
                let step8 = self.sim.step8.leaders[who0].ai.control;
                if production != step8 {
                    return Err(HostError::Unimplemented);
                }
                Ok(Value::Int(production))
            }
            246 => {
                let victory = self.sim.vic_leaders.slots[who0].population_cap;
                let step8 = self.sim.step8.leaders[who0].pop_cap;
                if victory != step8 {
                    return Err(HostError::Unimplemented);
                }
                Ok(Value::Int(victory))
            }
            _ => unreachable!("population helper owns only builtins 245 and 246"),
        }
    }

    fn reconciled_has_tech(&self, who0: usize, type_index: usize) -> Result<bool, HostError> {
        let from_types = self.types.leaders[who0].tech.get(type_index);
        let from_production = self.production.leaders[who0]
            .tech
            .tech
            .get(type_index as i32);
        let from_victory = self.sim.vic_leaders.slots[who0]
            .has_tech
            .get(type_index)
            .copied()
            .ok_or(HostError::Unimplemented)?;
        if from_types != from_production || from_types != from_victory {
            return Err(HostError::Unimplemented);
        }
        Ok(from_types)
    }

    fn object_count(&self, who0: usize) -> i32 {
        let slot = self.sim.world.objects.slot(who0);
        if !slot.band(Band::Wall).is_empty() {
            slot.mark(Band::Wall) as i32
        } else if !slot.band(Band::Build).is_empty() {
            slot.mark(Band::Build) as i32
        } else {
            slot.mark(Band::Unit) as i32
        }
    }

    fn completed_build_after(&self, who0: usize, producer_type: i32, cursor: i32) -> Option<i16> {
        let count = self.object_count(who0);
        let cursor = if cursor < 0 || cursor >= count {
            BUILD_BAND_BASE as i32
        } else {
            cursor
        };
        let start = cursor.wrapping_add(1);
        let candidates = (start..count).chain(BUILD_BAND_BASE as i32..start);
        for object in candidates {
            let Some(build_slot) = object
                .checked_sub(BUILD_BAND_BASE as i32)
                .and_then(|slot| usize::try_from(slot).ok())
            else {
                continue;
            };
            let Some(&row) = self
                .sim
                .world
                .objects
                .slot(who0)
                .band(Band::Build)
                .get(build_slot)
            else {
                continue;
            };
            let row = row as usize;
            let Some(build) = self.sim.builds.get(row) else {
                continue;
            };
            if build.flags & (flag::VALID | flag::ACTIVE) == (flag::VALID | flag::ACTIVE)
                && build.who as usize == who0
                && i32::from(build.object_id()) == object
                && self.production.build_types.get(row).copied().flatten() == Some(producer_type)
            {
                return i16::try_from(object).ok();
            }
        }
        None
    }

    fn research_tech_with_cost(&mut self, args: &[Value]) -> HostResult {
        let prior_cursor = self.sim.scenario_data.find_counters[RESEARCH_FIND_COUNTER];
        if prior_cursor < 0 {
            self.sim.scenario_data.find_counters[RESEARCH_FIND_COUNTER] = BUILD_BAND_BASE as i32;
        }
        let query = Self::str_arg(args, 1)?;
        let Some(type_index) = self.first_type(query)? else {
            return Ok(Value::Int(-1));
        };
        let Some(who0) = Self::who0(args, 0)? else {
            return Ok(Value::Int(-1));
        };
        if !self.reconciled_active_owner(who0)? {
            return Ok(Value::Int(-1));
        }
        if self.reconciled_has_tech(who0, type_index)? {
            return Ok(Value::Int(1));
        }
        let row = &self.types.types.rows()[type_index];
        let Ok(where_type) = usize::try_from(row.where_type) else {
            return Ok(Value::Int(-1));
        };
        let Some(producer_type) = self
            .upgrades
            .current_upgrade_for_production(who0, where_type)
        else {
            return Ok(Value::Int(-1));
        };
        if !self.reconciled_active_owner(who0)? {
            return Ok(Value::Int(0));
        }
        let cursor = self.sim.scenario_data.find_counters[RESEARCH_FIND_COUNTER];
        let Some(object_index) = self.completed_build_after(who0, producer_type, cursor) else {
            return Ok(Value::Int(0));
        };

        // Retail stores the successful find cursor before constructing/pushing the
        // transient singleton. Restore it only when the shared can_queue transaction
        // refuses before that chronology becomes observable.
        let cursor_before_find = self.sim.scenario_data.find_counters[RESEARCH_FIND_COUNTER];
        self.sim.scenario_data.find_counters[RESEARCH_FIND_COUNTER] = i32::from(object_index);
        let request = SingleLibraryResearchRequest {
            owner: who0 as u8,
            object_index,
            producer_type,
            research_type: type_index as i32,
            cost: row.common.costs,
        };
        let receipt =
            apply_sim_single_library_research_transaction(self.sim, self.production, request);
        let applied = receipt.status == SingleLibraryResearchStatus::Applied;
        self.research.push(receipt);
        if !applied {
            self.sim.scenario_data.find_counters[RESEARCH_FIND_COUNTER] = cursor_before_find;
            return Ok(Value::Int(0));
        }
        Ok(Value::Int(i32::from(object_index)))
    }

    fn at_least_type(&self, args: &[Value]) -> HostResult {
        let Some(who0) = Self::who0(args, 0)? else {
            return Ok(Value::Int(-1));
        };
        if !self.reconciled_active_owner(who0)? {
            return Ok(Value::Int(-1));
        }
        let amount = Self::int_arg(args, 1)?;
        let query = Self::str_arg(args, 2)?;
        let Some(type_index) = self.first_type(query)? else {
            return Ok(Value::Int(-1));
        };
        if type_index >= don_sim::systems::tech_cities::NUM_RES {
            return Ok(Value::Int(-1));
        }
        let runtime = self.production.leaders[who0].resources;
        if runtime != self.sim.leaders[who0].econ.stockpile
            || runtime != self.sim.step8.leaders[who0].econ.stockpile
            || runtime != self.sim.vic_leaders.slots[who0].economy.bucket
        {
            return Err(HostError::Unimplemented);
        }
        Ok(Value::Int((runtime[type_index] >= amount) as i32))
    }

    fn find_num_idle_unit(&mut self, args: &[Value]) -> HostResult {
        let who = Self::int_arg(args, 0)?;
        let type_name = Self::str_arg(args, 1)?.to_owned();
        let receipt = apply_sim_idle_unit_census_transaction(
            self.sim,
            self.types,
            IdleUnitCensusRequest { who, type_name },
        )
        .map_err(|_| HostError::Unimplemented)?;
        let returned = receipt.returned;
        self.idle_units.push(receipt);
        Ok(Value::Int(returned))
    }

    fn num_city_buildings(&mut self, args: &[Value]) -> HostResult {
        let request = CityBuildingCountRequest {
            who: Self::int_arg(args, 0)?,
            city_name: Self::str_arg(args, 1)?.to_owned(),
            type_name: Self::str_arg(args, 2)?.to_owned(),
            include_unfinished: Self::int_arg(args, 3)?,
        };
        let receipt = apply_sim_city_building_count_transaction(
            self.sim,
            self.production,
            self.types,
            request,
        )
        .map_err(|_| HostError::Unimplemented)?;
        let returned = receipt.returned;
        self.city_buildings.push(receipt);
        Ok(Value::Int(returned))
    }

    fn num_type_queued(&mut self, args: &[Value]) -> HostResult {
        let request = TypeQueueCountRequest {
            who: Self::int_arg(args, 0)?,
            build_object: Self::int_arg(args, 1)?,
            type_name: Self::str_arg(args, 2)?.to_owned(),
        };
        let receipt = apply_sim_type_queue_count_transaction(self.sim, self.types, request)
            .map_err(|_| HostError::Unimplemented)?;
        let returned = receipt.returned;
        self.type_queues.push(receipt);
        Ok(Value::Int(returned))
    }

    fn place_building_with_cost(&mut self, args: &[Value]) -> HostResult {
        let request = PlaceBuildingRequest {
            who: Self::int_arg(args, 0)?,
            type_name: Self::str_arg(args, 1)?.to_owned(),
            city_name: Self::str_arg(args, 2)?.to_owned(),
        };
        let receipt = apply_sim_place_building_with_cost_prefix(
            self.sim,
            self.production,
            self.types,
            self.place_building_costs,
            request,
        )
        .map_err(|_| HostError::Unimplemented)?;
        let returned = receipt.returned;
        let continuation = receipt.continuation;
        let cost_revision = receipt.authority_revision;
        let possible_goods = receipt.possible_goods;
        let resolved_costs = receipt.resolved_costs;
        let resources_before = receipt.resources;
        self.place_buildings.push(receipt);
        if let Some(returned) = returned {
            return Ok(Value::Int(returned));
        }
        let continuation = continuation.ok_or(HostError::Unimplemented)?;
        let produce = apply_sim_leader_produce_building_prefix(
            self.sim,
            self.production,
            LeaderProduceBuildingPrefixRequest {
                owner: continuation.owner,
                type_index: continuation.type_index,
                origin_build_object: continuation.origin_build_object,
                mode: continuation.mode,
            },
        )
        .map_err(|_| HostError::Unimplemented)?;
        let returned = produce.scenario_returned;
        let search_boundary = produce.continuation;
        self.produce_buildings.push(produce);
        if let Some(returned) = returned {
            return Ok(Value::Int(returned));
        }
        let search_boundary = search_boundary.ok_or(HostError::Unimplemented)?;
        let search = apply_sim_leader_produce_building_search_setup(
            self.sim,
            self.production,
            self.types,
            search_boundary,
        )
        .map_err(|_| HostError::Unimplemented)?;
        let returned = search.scenario_returned;
        let candidate_boundary = search.continuation;
        self.produce_building_search_setups.push(search);
        if let Some(returned) = returned {
            return Ok(Value::Int(returned));
        }
        let candidate_boundary = candidate_boundary.ok_or(HostError::Unimplemented)?;
        let candidate = apply_sim_leader_produce_building_candidate_prefix(
            self.sim,
            self.production,
            self.types,
            candidate_boundary,
        )
        .map_err(|_| HostError::Unimplemented)?;
        let returned = candidate.scenario_returned;
        let blocked_site_boundary = candidate.continuation;
        self.produce_building_candidate_prefixes
            .push(candidate.clone());
        if let Some(returned) = returned {
            return Ok(Value::Int(returned));
        }
        let blocked_site_boundary = blocked_site_boundary.ok_or(HostError::Unimplemented)?;
        let blocked_site = apply_sim_leader_produce_building_blocked_site_prefix(
            self.production,
            self.types,
            blocked_site_boundary,
        )
        .map_err(|_| HostError::Unimplemented)?;
        let returned = blocked_site.native_returned;
        let blocked_tcoord_boundary = blocked_site.continuation;
        self.produce_building_blocked_site_prefixes
            .push(blocked_site.clone());
        if let Some(returned) = returned {
            return Ok(Value::Int(returned));
        }
        let blocked_tcoord = apply_sim_build_type_blocked_tcoord_land_prefix(
            self.sim,
            self.production,
            self.types,
            blocked_tcoord_boundary,
        )
        .map_err(|_| HostError::Unimplemented)?;
        self.produce_building_blocked_tcoord_prefixes
            .push(blocked_tcoord);

        let farm = self.farm_520.as_mut().ok_or(HostError::Unimplemented)?;
        if farm.expected_cost_revision != self.place_building_costs.revision
            || farm.expected_cost_composition_digest != self.place_building_costs.composition_digest
            || cost_revision != Some(farm.expected_cost_revision)
        {
            return Err(HostError::Unimplemented);
        }
        let possible_goods = possible_goods.ok_or(HostError::Unimplemented)?;
        let resolved_costs = resolved_costs.ok_or(HostError::Unimplemented)?;
        let resources_before = resources_before.ok_or(HostError::Unimplemented)?;

        let footprint = apply_sim_leader_produce_building_blocked_site_land_footprint(
            self.sim,
            self.production,
            self.types,
            farm.gather_terrain,
            blocked_site,
        )
        .map_err(|_| HostError::Unimplemented)?;
        self.produce_building_land_footprints
            .push(footprint.clone());
        let owned_site = apply_sim_leader_produce_building_blocked_site_farm_owned_tail(
            self.sim,
            self.production,
            self.types,
            footprint,
        )
        .map_err(|_| HostError::Unimplemented)?;
        self.produce_building_owned_sites.push(owned_site.clone());
        let success = apply_sim_leader_produce_building_farm_success_preflight(
            self.sim,
            self.production,
            self.types,
            farm.gather_terrain,
            candidate,
            owned_site,
        )
        .map_err(|_| HostError::Unimplemented)?;
        self.produce_building_success_preflights
            .push(success.clone());

        let tail = complete_leader_produce_building_farm_frame_zero_tail(
            self.sim,
            self.production,
            self.types,
            farm.terrain_height,
            farm.build_authority,
            farm.builds_walk,
            farm.expected_build_authority_revision,
            farm.source,
            &success,
        )
        .map_err(|_| HostError::Unimplemented)?;

        // `Leader::produce_building`'s sole Type::pay_cost virtual is at 0x006E2C89 and
        // guarded by `Game::frame != 0` at 0x006E2C66. Search setup already proves this exact
        // witness is frame zero. Reconcile every live stock owner again after construction so
        // the no-payment branch cannot conceal a partial resource publication.
        let owner = tail.owner as usize;
        let resources_after = self.production.leaders[owner].resources;
        if resources_after != resources_before
            || resources_after != self.sim.leaders[owner].econ.stockpile
            || resources_after != self.sim.step8.leaders[owner].econ.stockpile
            || resources_after != self.sim.vic_leaders.slots[owner].economy.bucket
        {
            return Err(HostError::Unimplemented);
        }
        self.farm_builtin_520_costs.push(FarmBuiltin520CostReceipt {
            owner: tail.owner,
            authority_revision: farm.expected_cost_revision,
            composition_digest: farm.expected_cost_composition_digest,
            possible_goods,
            resolved_costs,
            resources_before,
            resources_after,
            payment_skipped_at_frame_zero: true,
        });
        self.farm_frame_zero_tails.push(tail);

        // Native produce_building returns zero on success; builtin 521 normalizes that to one.
        Ok(Value::Int(1))
    }

    fn record(&mut self, decl: &BuiltinDecl, args: &[Value], returned: &Value) {
        self.trace.push(ProductionBuiltinCall {
            index: decl.index,
            name: decl.name,
            args: args.iter().map(ProductionBuiltinValue::from).collect(),
            returned: ProductionBuiltinValue::from(returned),
        });
    }
}

impl Host for ResearchHost<'_, '_> {
    fn call(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
        let returned = match decl.index {
            RESEARCH_TECH_WITH_COST_BUILTIN => self.research_tech_with_cost(args),
            FIND_NUM_IDLE_UNIT_BUILTIN => self.find_num_idle_unit(args),
            NUM_CITY_BUILDINGS_BUILTIN => self.num_city_buildings(args),
            NUM_TYPE_QUEUED_BUILTIN => self.num_type_queued(args),
            PLACE_BUILDING_WITH_COST_BUILTIN => self.place_building_with_cost(args),
            AT_LEAST_TYPE_BUILTIN => self.at_least_type(args),
            106 => self.set_difficulty(args),
            108 => self.set_leader_difficulty(args),
            109 => self.get_leader_difficulty(args),
            245 | 246 => self.population(decl.index, args),
            _ => {
                let before = self.prefix.trace().len();
                let returned = self.prefix.call(decl, args);
                if returned.is_ok() && self.prefix.trace().len() == before + 1 {
                    self.trace.push(self.prefix.trace()[before].clone());
                }
                return returned;
            }
        }?;
        self.record(decl, args, &returned);
        Ok(returned)
    }
}

impl ExternalTimerHost for ResearchHost<'_, '_> {
    fn timer_builtin_returned(&mut self, decl: &BuiltinDecl, args: &[Value], returned: &Value) {
        self.record(decl, args, returned);
    }
}

/// Run one production-script call over one atomic Program/ref/timer/production candidate.
pub fn run_production_research_call(
    script_runtime: &mut ScriptRuntime,
    binding: &ReplayBhsBinding,
    call: &mut ReplayProductionCall,
    image: &ProductionBuiltinImage,
    types: &TypeBuiltinState,
    upgrades: &BhsCreateUnitRuntime,
    place_building_costs: &PlaceBuildingCostAuthority,
    farm_520: Option<FarmBuiltin520Authority<'_>>,
    sim: &mut Sim,
    production: &mut LiveProductionRuntime,
    game_seconds: ExternalGameSeconds,
) -> Result<ProductionResearchRunReceipt, ProductionRunError> {
    let Some(file) = script_runtime.program().files.get(binding.file) else {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::BadFile(binding.file),
            bytecodes_executed: 0,
            trace: Vec::new(),
        });
    };
    let Some(script) = file.find_script(&binding.name) else {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::MissingScript {
                file: binding.file,
                name: binding.name.clone(),
            },
            bytecodes_executed: 0,
            trace: Vec::new(),
        });
    };
    let arity = file.scripts[script].arity;
    if arity != 4 {
        return Err(ProductionRunError {
            failure: ProductionRunFailure::BadArity { actual: arity },
            bytecodes_executed: 0,
            trace: Vec::new(),
        });
    }

    let before = *call;
    let args = [
        Value::Int(call.who),
        Value::Int(call.step),
        Value::Int(call.boom_vs_rush),
        Value::Int(call.num_loops),
    ];

    let scenario_before = sim.scenario_data.clone();
    let groups_before = sim.groups.clone();
    let world_before = sim.world.clone();
    let map_world_before = sim.map.world.clone();
    let builds_before = sim.builds.clone();
    let farms_before = sim.farms.clone();
    let cities_before = sim.cities.clone();
    let production_before = production.clone();
    let leader_resources_before =
        std::array::from_fn::<_, 8, _>(|who| sim.leaders[who].econ.stockpile);
    let step8_resources_before =
        std::array::from_fn::<_, 8, _>(|who| sim.step8.leaders[who].econ.stockpile);
    let step8_flags_before = std::array::from_fn::<_, 8, _>(|who| sim.step8.leaders[who].flags);
    let victory_before = sim.vic_leaders.clone();
    let match_options_before = sim.vic_match.options;
    let farm_build_authority_before = farm_520
        .as_ref()
        .map(|authority| authority.build_authority.clone());
    let builds_walk_before = farm_520
        .as_ref()
        .map(|authority| authority.builds_walk.clone());

    let (
        result,
        trace,
        research,
        idle_units,
        city_buildings,
        type_queues,
        place_buildings,
        produce_buildings,
        produce_building_search_setups,
        produce_building_candidate_prefixes,
        produce_building_blocked_site_prefixes,
        produce_building_blocked_tcoord_prefixes,
        produce_building_land_footprints,
        produce_building_owned_sites,
        produce_building_success_preflights,
        farm_builtin_520_costs,
        farm_frame_zero_tails,
        mut farm_520,
    ) = {
        let mut host = ResearchHost::new(
            image,
            types,
            upgrades,
            place_building_costs,
            farm_520,
            sim,
            production,
        );
        let result = script_runtime.run_external_timer_transaction(
            binding.file,
            script,
            &args,
            game_seconds,
            &mut host,
            |outcome, candidate_args| {
                if let Some(failure) = outcome.error.clone() {
                    return Err(ProductionRunFailure::Runtime(failure));
                }
                let Some(Value::Int(after_step)) = candidate_args.get(1) else {
                    return Err(ProductionRunFailure::StepWasNotInt);
                };
                let Some(Value::Int(returned)) = outcome.returned.as_ref() else {
                    return Err(ProductionRunFailure::ReturnWasNotInt);
                };
                Ok((*after_step, *returned))
            },
        );
        (
            result,
            host.trace,
            host.research,
            host.idle_units,
            host.city_buildings,
            host.type_queues,
            host.place_buildings,
            host.produce_buildings,
            host.produce_building_search_setups,
            host.produce_building_candidate_prefixes,
            host.produce_building_blocked_site_prefixes,
            host.produce_building_blocked_tcoord_prefixes,
            host.produce_building_land_footprints,
            host.produce_building_owned_sites,
            host.produce_building_success_preflights,
            host.farm_builtin_520_costs,
            host.farm_frame_zero_tails,
            host.farm_520,
        )
    };

    let committed = match result {
        Ok(committed) => committed,
        Err(error) => {
            sim.scenario_data = scenario_before;
            sim.groups = groups_before;
            sim.world = world_before;
            sim.map.world = map_world_before;
            sim.builds = builds_before;
            sim.farms = farms_before;
            sim.cities = cities_before;
            *production = production_before;
            for who in 0..8 {
                sim.leaders[who].econ.stockpile = leader_resources_before[who];
                sim.step8.leaders[who].econ.stockpile = step8_resources_before[who];
                sim.step8.leaders[who].flags = step8_flags_before[who];
            }
            sim.vic_leaders = victory_before;
            sim.vic_match.options = match_options_before;
            if let (Some(authority), Some(before)) =
                (farm_520.as_mut(), farm_build_authority_before)
            {
                *authority.build_authority = before;
            }
            if let (Some(authority), Some(before)) = (farm_520.as_mut(), builds_walk_before) {
                *authority.builds_walk = before;
            }
            let failure = match error.failure {
                ExternalScriptFailure::Vm(failure) => ProductionRunFailure::Vm(failure),
                ExternalScriptFailure::Rejected(failure) => failure,
            };
            return Err(ProductionRunError {
                failure,
                bytecodes_executed: error.bytecodes_executed,
                trace,
            });
        }
    };

    let (after_step, returned) = committed.validated;
    call.step = after_step;
    Ok(ProductionResearchRunReceipt {
        production: ProductionRunReceipt {
            before,
            after_step,
            returned,
            disposition: ProductionDisposition::from(returned),
            bytecodes_executed: committed.bytecodes_executed,
            trace,
        },
        research,
        idle_units,
        city_buildings,
        type_queues,
        place_buildings,
        produce_buildings,
        produce_building_search_setups,
        produce_building_candidate_prefixes,
        produce_building_blocked_site_prefixes,
        produce_building_blocked_tcoord_prefixes,
        produce_building_land_footprints,
        produce_building_owned_sites,
        produce_building_success_preflights,
        farm_builtin_520_costs,
        farm_frame_zero_tails,
    })
}
