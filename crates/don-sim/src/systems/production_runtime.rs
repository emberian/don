//! Authoritative Sim adapter for the recovered production queue transaction.
//!
//! This module is kept separate from `tick.rs` so the object-step call site can remain a
//! small, conflict-free hunk. The adapter and its runtime facts are implemented here; the
//! tick only needs to invoke the phase once from `Build::process`.

use super::*;
use crate::objects::Band;
use crate::order::{Order, OrderIndex};
use crate::systems::tech_cities::{
    GainTechCohortContext, TechAutoUnlockHost, TechAutoUnlockMutation,
    TechAutoUnlockMutationReceipt, TechOneShotHost, TechOneShotMutation,
    TechOneShotMutationReceipt, TechState, NUM_RES, TECH_AUTO_UNLOCK_BUILD_FLAG,
    TECH_AUTO_UNLOCK_EXCLUDED_OBJ_MASK, TECH_AUTO_UNLOCK_UNIT_FLAG, TECH_RESOURCE_SELL_FLOOR,
};
use crate::tick::Sim;

/// Runtime classification of one installed global type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTypeClass {
    Unit,
    Building,
    Research,
    Spell,
}

/// Unit placement cohorts the live Sim adapter can currently execute without guessing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveUnitPlacement {
    /// A normal ground unit leaves a non-air, non-University producer through
    /// `come_out(0)`, then reaches the common launching-bit tail.
    OrdinaryGround,
    /// A true plane trained by a Holds-Air producer. Multi-point rally installs one live
    /// AirPatrol order and its exact dynamic waypoint payload; empty rally remains inside
    /// or executes the recovered capacity-destruction tail.
    HostedAir,
    /// Carrier payload, University Scholar, missile/Helicopter single-rally, and strafe
    /// effects still require facts outside the bounded runtime cohort.
    Unsupported,
}

/// Exact effect profile admitted for an ordinary building-shaped completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveBuildingCompletion {
    /// Same-footprint, uncaptured in-place type replacement. `mask_me(1,0)` has no
    /// additional Sim-owned terrain delta for this explicitly installed profile.
    InPlaceUncaptured,
    /// The build-shaped type falls through to `Leader::gain_tech` (`build_flags & 4`).
    GainTech,
    Unsupported,
}

/// Tech-specific regions surrounding the generic gain transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTechEffects {
    /// The installer has identified a type with no special branch, resource-sale arm,
    /// Town check, or recursive auto-unlock in the installed table.
    GenericOnly,
    Unsupported,
}

/// The production projection of one installed `TypeData` row. Missing rows are not
/// defaulted: the phase rejects them before touching queue progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveProductionType {
    pub type_index: i32,
    pub class: LiveTypeClass,
    pub train_time: i32,
    pub prerequisites: Vec<i32>,
    pub can_make: bool,
    pub control_cost: i32,
    pub uses_caravan_limit: bool,
    pub unit_flags: u32,
    pub object_masks: u32,
    pub stance_type: i32,
    pub unit_placement: LiveUnitPlacement,
    pub building_completion: LiveBuildingCompletion,
    pub city_pop_value: i32,
    pub build_flags: u32,
    pub is_town: bool,
    pub type_eligible: bool,
    pub parallel_slots: usize,
    pub is_capitol: bool,
    pub holds_air: bool,
    pub is_university: bool,
    pub gather_inside: bool,
    pub garrison_limit: i32,
    pub is_aircraft_carrier: bool,
    pub tech_effects: LiveTechEffects,
    /// Exact price for `action_queue(type,0)` after a repeat completion. `None` makes the
    /// repeat payment fail and preserves retail's repeat latch.
    pub repeat_cost: Option<[i32; NUM_RES]>,
}

impl LiveProductionType {
    pub fn research(type_index: i32, train_time: i32) -> Self {
        Self {
            type_index,
            class: LiveTypeClass::Research,
            train_time: train_time.max(1),
            prerequisites: Vec::new(),
            can_make: false,
            control_cost: 0,
            uses_caravan_limit: false,
            unit_flags: 0,
            object_masks: 0,
            stance_type: 0,
            unit_placement: LiveUnitPlacement::Unsupported,
            building_completion: LiveBuildingCompletion::Unsupported,
            city_pop_value: 0,
            build_flags: 0,
            is_town: false,
            type_eligible: true,
            parallel_slots: 1,
            is_capitol: false,
            holds_air: false,
            is_university: false,
            gather_inside: false,
            garrison_limit: 10,
            is_aircraft_carrier: false,
            tech_effects: LiveTechEffects::GenericOnly,
            repeat_cost: None,
        }
    }

    pub fn ordinary_unit(type_index: i32, train_time: i32, control_cost: i32) -> Self {
        Self {
            class: LiveTypeClass::Unit,
            can_make: true,
            control_cost,
            unit_placement: LiveUnitPlacement::OrdinaryGround,
            ..Self::research(type_index, train_time)
        }
    }

    pub fn hosted_air_unit(type_index: i32, train_time: i32, control_cost: i32) -> Self {
        Self {
            unit_placement: LiveUnitPlacement::HostedAir,
            ..Self::ordinary_unit(type_index, train_time, control_cost)
        }
    }

    pub fn in_place_building(type_index: i32, train_time: i32) -> Self {
        Self {
            class: LiveTypeClass::Building,
            building_completion: LiveBuildingCompletion::InPlaceUncaptured,
            tech_effects: LiveTechEffects::Unsupported,
            ..Self::research(type_index, train_time)
        }
    }
}

/// Sim-owned per-player state needed by queue completion but absent from `BuildData`.
#[derive(Clone, Debug)]
pub struct LiveProductionLeader {
    pub tech: TechState,
    pub resources: [i32; NUM_RES],
    pub resource_unlock_tech: [i32; NUM_RES],
    pub resource_unlock_amount: [i32; NUM_RES],
    pub resource_sell_tech: [i32; NUM_RES],
    pub control: i32,
    pub control_cap: i32,
    pub caravan_limit: i32,
    pub aircraft_limit: i32,
    pub ai_speed: i32,
    pub unit_counts: Vec<i32>,
    pub queued_counts: Vec<i32>,
    pub last_unit_built: i32,
    pub last_unit_finished: Vec<i32>,
    pub age_stamp: [i32; 7],
    pub gain_context: GainTechCohortContext,
    pub queue_dirty: bool,
}

impl Default for LiveProductionLeader {
    fn default() -> Self {
        Self {
            tech: TechState::default(),
            resources: [0; NUM_RES],
            resource_unlock_tech: [-1; NUM_RES],
            resource_unlock_amount: [0; NUM_RES],
            resource_sell_tech: [-1; NUM_RES],
            control: 0,
            control_cap: 200,
            caravan_limit: i32::MAX,
            aircraft_limit: i32::MAX,
            ai_speed: 1,
            unit_counts: vec![0; crate::systems::tech_cities::ty::NUM_TYPES],
            queued_counts: vec![0; crate::systems::tech_cities::ty::NUM_TYPES],
            last_unit_built: -1,
            last_unit_finished: vec![-1; crate::systems::tech_cities::ty::NUM_TYPES],
            age_stamp: [-1; 7],
            gain_context: GainTechCohortContext::default(),
            queue_dirty: false,
        }
    }
}

/// Sidecar installed on `Sim` by the eventual one-line tick call site.
#[derive(Clone, Debug)]
pub struct LiveProductionRuntime {
    pub types: Vec<Option<LiveProductionType>>,
    pub leaders: Vec<LiveProductionLeader>,
    /// Current build type by `Sim::builds` row. `BuildData::orig_type` is not a substitute.
    pub build_types: Vec<Option<i32>>,
    pub local_player: u8,
    pub scenario_presentation_count: i32,
    pub game_tech_dirty: bool,
    pub world_population: i32,
    pub mask_effects: u64,
    /// Dynamic payloads behind generic `OrderIndex::AirPatrol` records installed by
    /// `Build::train`. One record is created for the first valid rally point; later
    /// points mutate it in place.
    pub air_patrol_orders: Vec<LiveAirPatrolOrder>,
    /// Concrete simulation-side receipts for the UI/event boundary. The deterministic
    /// stage and placement identity remain available even when no product UI is attached.
    pub unit_presentations: Vec<LiveUnitPresentation>,
}

impl Default for LiveProductionRuntime {
    fn default() -> Self {
        Self {
            types: vec![None; crate::systems::tech_cities::ty::NUM_TYPES],
            leaders: (0..crate::objects::OWNER_SLOTS)
                .map(|_| LiveProductionLeader::default())
                .collect(),
            build_types: Vec::new(),
            local_player: u8::MAX,
            scenario_presentation_count: 0,
            game_tech_dirty: false,
            world_population: 0,
            mask_effects: 0,
            air_patrol_orders: Vec::new(),
            unit_presentations: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveAirPatrolOrder {
    pub unit: UnitIdentity,
    pub order: crate::systems::patrol::AirPatrolOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveUnitPresentation {
    pub placement: UnitPlacementRequest,
    pub stage: UnitPresentationStage,
}

impl LiveProductionRuntime {
    pub fn install_type(&mut self, facts: LiveProductionType) {
        let type_index = facts.type_index as usize;
        if self.types.len() <= type_index {
            self.types.resize(type_index + 1, None);
        }
        self.types[type_index] = Some(facts);
    }

    pub fn register_build(&mut self, row: usize, type_index: i32) {
        if self.build_types.len() <= row {
            self.build_types.resize(row + 1, None);
        }
        self.build_types[row] = Some(type_index);
    }

    fn facts(&self, type_index: i32) -> Option<&LiveProductionType> {
        usize::try_from(type_index)
            .ok()
            .and_then(|index| self.types.get(index))
            .and_then(Option::as_ref)
    }

    /// Live `LeaderData::has_preq(type_index)` for an installed ordinary prerequisite
    /// row.
    ///
    /// This is the exact bounded query needed by `GameDaemon::process_victory` for bonus
    /// type `0x2B9` (World Government's instant-timer effect): the queried type's complete
    /// prerequisite list must be held by the leader. Missing type/leader facts fail closed
    /// rather than manufacturing the countdown bypass.
    pub fn leader_has_prerequisites(&self, owner: usize, type_index: i32) -> bool {
        let Some(leader) = self.leaders.get(owner) else {
            return false;
        };
        let Some(facts) = self.facts(type_index) else {
            return false;
        };
        facts
            .prerequisites
            .iter()
            .all(|&preq| prerequisite_held(&leader.tech, preq))
    }

    /// Execute the concrete Build-state half of terminal `Build::clean_queue(0)` for one
    /// owner.
    ///
    /// `Leader::victory` (`0x006ECA04..0x006ECA5C`) and `Leader::defeat`
    /// (`0x006ECBB7..0x006ECC1C`) traverse valid objects in the owner's Build band and call
    /// no-refund cleanup. Repeated `Build::unqueue(last, 0)` leaves the allocated records
    /// in place, zeroes each former logical entry's progress, decrements positive queued
    /// counters, takes the logical length to zero, and clears `REPEAT_QUEUE`. This adapter
    /// reproduces that terminal net transition directly over the Sim's concrete Build rows.
    pub fn clean_terminal_build_queues(
        &mut self,
        builds: &mut [BuildData],
        owner: usize,
    ) -> TerminalQueueCleanupReceipt {
        let mut receipt = TerminalQueueCleanupReceipt {
            owner,
            ..Default::default()
        };
        let Ok(owner_byte) = u8::try_from(owner) else {
            return receipt;
        };
        let Some(leader) = self.leaders.get_mut(owner) else {
            return receipt;
        };

        for build in builds
            .iter_mut()
            .filter(|build| build.is_valid() && build.who == owner_byte)
        {
            receipt.builds_visited += 1;
            let logical = build.queue.queued as usize;
            if logical != 0 {
                receipt.queues_cleaned += 1;
                receipt.entries_removed += logical;
                leader.queue_dirty = true;
            }
            for entry in build.queue.entries.iter_mut().take(logical) {
                entry.elapsed = 0;
                if let Ok(type_index) = usize::try_from(entry.type_index) {
                    if let Some(queued) = leader.queued_counts.get_mut(type_index) {
                        if *queued != 0 {
                            *queued -= 1;
                        }
                    }
                }
            }
            build.queue.queued = 0;
            build.build_masks &= !mask::REPEAT_QUEUE;
        }
        receipt
    }
}

/// Concrete terminal queue-cleanup work performed for one leader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalQueueCleanupReceipt {
    pub owner: usize,
    /// Valid owned Build rows reached by the retail band traversal.
    pub builds_visited: usize,
    /// Reached Build rows whose logical queue was non-empty.
    pub queues_cleaned: usize,
    /// Logical slots removed. This can exceed allocated records for a malformed retail
    /// image because `BuildData::queued` and `BuildQueue::num` are independent fields.
    pub entries_removed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveProductionError {
    MissingBuildRow(usize),
    InactiveProducer(usize),
    MissingBuildType(usize),
    MissingType(i32),
    InvalidOwner(u8),
    UnsupportedUnitPlacement(i32),
    UnsupportedBuildingCompletion(i32),
    UnsupportedTechEffects(i32),
    UnsupportedSpell(i32),
    CapturedBuildingCompletion(i32),
    RecursiveTechUnlock(i32),
    MissingQueuedCounter(i32),
    UnsupportedCallback(&'static str),
    Queue(QueueTransactionError),
    Finished(FinishedEffectError),
}

impl From<QueueTransactionError> for LiveProductionError {
    fn from(value: QueueTransactionError) -> Self {
        Self::Queue(value)
    }
}

fn prerequisite_held(tech: &TechState, type_index: i32) -> bool {
    match type_index {
        -1 => true,
        -2 => false,
        value if value < 50 => true,
        value => tech.tech.get(value),
    }
}

fn prerequisites_met_after_gain(
    facts: &LiveProductionType,
    tech: &TechState,
    gained_type: i32,
) -> bool {
    facts
        .prerequisites
        .iter()
        .all(|&preq| preq == gained_type || prerequisite_held(tech, preq))
}

fn has_recursive_unlock(
    runtime: &LiveProductionRuntime,
    leader: &LiveProductionLeader,
    gained_type: i32,
) -> bool {
    runtime.types.iter().flatten().any(|candidate| {
        if !candidate.prerequisites.contains(&gained_type)
            || !prerequisites_met_after_gain(candidate, &leader.tech, gained_type)
        {
            return false;
        }
        match candidate.class {
            LiveTypeClass::Building => {
                candidate.is_town
                    || (candidate.build_flags & TECH_AUTO_UNLOCK_BUILD_FLAG != 0
                        && candidate.type_eligible)
            }
            LiveTypeClass::Unit => {
                candidate.unit_flags & TECH_AUTO_UNLOCK_UNIT_FLAG != 0
                    && candidate.object_masks & TECH_AUTO_UNLOCK_EXCLUDED_OBJ_MASK == 0
                    && candidate.type_eligible
                    && !leader.tech.tech.get(candidate.type_index)
            }
            _ => false,
        }
    })
}

fn rebuild_queued_counts(sim: &Sim, runtime: &mut LiveProductionRuntime, owner: usize) {
    runtime.leaders[owner].queued_counts.fill(0);
    for build in sim
        .builds
        .iter()
        .filter(|build| build.who as usize == owner)
    {
        for entry in build.queue.entries.iter().take(build.queue.queued as usize) {
            if let Some(count) = runtime.leaders[owner]
                .queued_counts
                .get_mut(entry.type_index as usize)
            {
                *count = count.wrapping_add(1);
            }
        }
    }
}

fn preflight(
    sim: &Sim,
    runtime: &LiveProductionRuntime,
    row: usize,
) -> Result<(usize, i32), LiveProductionError> {
    let build = sim
        .builds
        .get(row)
        .ok_or(LiveProductionError::MissingBuildRow(row))?;
    let owner = build.who as usize;
    let leader = runtime
        .leaders
        .get(owner)
        .ok_or(LiveProductionError::InvalidOwner(build.who))?;
    let producer_type = runtime
        .build_types
        .get(row)
        .and_then(|value| *value)
        .ok_or(LiveProductionError::MissingBuildType(row))?;
    let producer = runtime
        .facts(producer_type)
        .ok_or(LiveProductionError::MissingType(producer_type))?;
    if producer.parallel_slots == 0 {
        return Err(LiveProductionError::UnsupportedCallback(
            "zero parallel-slot limit",
        ));
    }

    for entry in build.queue.entries.iter().take(build.queue.queued as usize) {
        let type_index = entry.type_index as i32;
        let facts = runtime
            .facts(type_index)
            .ok_or(LiveProductionError::MissingType(type_index))?;
        match facts.class {
            LiveTypeClass::Unit if facts.can_make => {
                let carrier_unit = facts.is_aircraft_carrier
                    || facts.type_index == UNIT_PLACEMENT_TYPE_AIRCRAFT_CARRIER
                    || producer.is_aircraft_carrier
                    || producer.type_index == UNIT_PLACEMENT_TYPE_AIRCRAFT_CARRIER;
                let university =
                    producer.is_university || producer.type_index == UNIT_PLACEMENT_TYPE_UNIVERSITY;
                let placement_supported = match facts.unit_placement {
                    LiveUnitPlacement::OrdinaryGround => !producer.holds_air,
                    LiveUnitPlacement::HostedAir => {
                        if !producer.holds_air {
                            false
                        } else if build.gather.is_empty() {
                            true
                        } else {
                            facts.object_masks & UNIT_PLACEMENT_OBJ_MISSILE == 0
                                && facts.unit_flags & UNIT_PLACEMENT_FLAG_HELICOPTER == 0
                        }
                    }
                    LiveUnitPlacement::Unsupported => false,
                };
                if carrier_unit || university || !placement_supported {
                    return Err(LiveProductionError::UnsupportedUnitPlacement(type_index));
                }
            }
            LiveTypeClass::Building
                if facts.building_completion == LiveBuildingCompletion::InPlaceUncaptured =>
            {
                if build.flags & flag::CAPTURED != 0 {
                    return Err(LiveProductionError::CapturedBuildingCompletion(type_index));
                }
            }
            LiveTypeClass::Spell => {
                return Err(LiveProductionError::UnsupportedSpell(type_index));
            }
            _ => {
                if facts.tech_effects != LiveTechEffects::GenericOnly
                    || runtime
                        .leaders
                        .get(owner)
                        .is_some_and(|leader| has_recursive_unlock(runtime, leader, type_index))
                    || leader
                        .resource_sell_tech
                        .iter()
                        .any(|&sell_tech| sell_tech == type_index)
                    || producer.is_capitol
                {
                    return Err(LiveProductionError::UnsupportedTechEffects(type_index));
                }
            }
        }
    }
    Ok((owner, producer_type))
}

/// World/type adapter used only after [`preflight`] has proved that every queue entry
/// belongs to the bounded live cohort. Callbacks outside that cohort still record an
/// error; they never become accidental no-ops if the classifier is later widened.
struct SimFinishedHost<'a> {
    sim: &'a mut Sim,
    runtime: &'a mut LiveProductionRuntime,
    owner: usize,
    producer_row: usize,
    producer_type: i32,
    producer_position: (i32, i32),
    finishing_type: i32,
    live_tech: TechState,
    error: Option<LiveProductionError>,
}

impl SimFinishedHost<'_> {
    fn owner(&self) -> usize {
        self.owner
    }

    fn type_facts(&self, type_index: i32) -> Option<&LiveProductionType> {
        self.runtime.facts(type_index)
    }

    fn unit_row(&self, unit: UnitIdentity) -> Option<usize> {
        let object_id = usize::try_from(unit.object_id).ok()?;
        self.sim
            .world
            .objects
            .slot(unit.owner as usize)
            .band(Band::Unit)
            .get(object_id)
            .copied()
            .map(|row| row as usize)
    }

    fn unsupported(&mut self, callback: &'static str) {
        self.error
            .get_or_insert(LiveProductionError::UnsupportedCallback(callback));
    }

    fn matching_placement(
        &mut self,
        callback: &'static str,
        mutation: UnitPlacementMutation,
    ) -> UnitPlacementMutationReceipt {
        self.unsupported(callback);
        UnitPlacementMutationReceipt { mutation }
    }

    fn destroy_just_allocated_unit(
        &mut self,
        placement: UnitPlacementRequest,
        callback: &'static str,
    ) {
        let unit = UnitIdentity {
            owner: placement.owner,
            object_id: placement.object_id,
        };
        let Some(row) = self.unit_row(unit) else {
            self.unsupported(callback);
            return;
        };
        if row + 1 != self.sim.world.live_count() as usize {
            self.unsupported("capacity destruction target is not the new tail unit");
            return;
        }
        let Some(handle) = self.sim.world.handle_at_row(row) else {
            self.unsupported(callback);
            return;
        };
        if !self.sim.world.despawn(handle) {
            self.unsupported(callback);
            return;
        }
        self.sim.unit_type.pop();
        self.sim.paths.pop();
        self.sim.path_unit.pop();
        self.sim.crash_units.pop();

        let control = self
            .type_facts(placement.type_index)
            .map_or(0, |facts| facts.control_cost);
        let leader = &mut self.runtime.leaders[placement.owner as usize];
        leader.control = leader.control.wrapping_sub(control);
        if let Some(count) = leader.unit_counts.get_mut(placement.type_index as usize) {
            *count = count.wrapping_sub(1);
        }
    }
}

impl UnitCompletionHost for SimFinishedHost<'_> {
    fn unit_control_cost(&mut self, type_index: i32) -> i32 {
        self.type_facts(type_index)
            .map_or(0, |facts| facts.control_cost)
    }

    fn leader_control(&mut self, build: &BuildData) -> i32 {
        self.runtime.leaders[build.who as usize].control
    }

    fn leader_control_cap(&mut self, build: &BuildData) -> i32 {
        self.runtime.leaders[build.who as usize].control_cap
    }

    fn uses_caravan_limit(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index)
            .is_some_and(|facts| facts.uses_caravan_limit)
    }

    fn trained_unit_count(&mut self, build: &BuildData, type_index: i32, queued: i32) -> i32 {
        self.runtime.leaders[build.who as usize]
            .unit_counts
            .get(type_index as usize)
            .copied()
            .unwrap_or(0)
            .wrapping_add(queued)
    }

    fn caravan_limit(&mut self, build: &BuildData, _queued: i32) -> i32 {
        self.runtime.leaders[build.who as usize].caravan_limit
    }

    fn producer_is_aircraft_carrier(&mut self, _build: &BuildData) -> bool {
        self.type_facts(self.producer_type)
            .is_some_and(|facts| facts.is_aircraft_carrier)
    }

    fn unit_is_helicopter(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index).is_some_and(|facts| {
            facts.unit_flags & UNIT_PLACEMENT_FLAG_HELICOPTER != 0
                || facts.type_index == UNIT_PLACEMENT_TYPE_HELICOPTER
        })
    }

    fn hosted_aircraft(&mut self, build: &BuildData, _queued: i32) -> i32 {
        let producer_o = build.object_id();
        self.sim
            .world
            .units
            .inside_up()
            .iter()
            .zip(self.sim.world.units.inside_up_who())
            .filter(|(inside, who)| **inside == producer_o && **who == build.who as i8)
            .count() as i32
    }

    fn aircraft_limit(&mut self, build: &BuildData) -> i32 {
        self.runtime.leaders[build.who as usize].aircraft_limit
    }

    fn allocate_unit(&mut self, request: UnitAllocationRequest) -> UnitAllocationReceipt {
        let object_id = self
            .sim
            .world
            .allocate_typed_at(request.owner, request.type_index, request.x, request.y)
            .and_then(|handle| self.sim.world.row_of(handle))
            .map_or(-1, |row| {
                self.sim.world.units.mylos_mut()[row] = 0;
                self.sim.world.units.guy_mark_mut()[row] = 1;
                while self.sim.unit_type.len() <= row {
                    self.sim.unit_type.push(0);
                    self.sim
                        .paths
                        .push(crate::systems::movement::PathStack::new());
                    self.sim
                        .path_unit
                        .push(crate::systems::movement::PathUnit::default());
                    self.sim.crash_units.push(None);
                }
                self.sim.unit_type[row] = request.type_index;
                self.sim.world.units.o()[row] as i32
            });
        if object_id >= 0 {
            let owner = request.owner as usize;
            let control = self
                .type_facts(request.type_index)
                .map_or(0, |facts| facts.control_cost);
            let leader = &mut self.runtime.leaders[owner];
            leader.control = leader.control.wrapping_add(control);
            if let Some(count) = leader.unit_counts.get_mut(request.type_index as usize) {
                *count = count.wrapping_add(1);
            }
        }
        UnitAllocationReceipt { request, object_id }
    }

    fn publish_last_unit_built(&mut self, owner: u8, object_id: i32) {
        self.runtime.leaders[owner as usize].last_unit_built = object_id;
    }

    fn publish_last_unit_finished(&mut self, owner: u8, type_index: i32, object_id: i32) {
        if let Some(last) = self.runtime.leaders[owner as usize]
            .last_unit_finished
            .get_mut(type_index as usize)
        {
            *last = object_id;
        }
    }

    fn unit_stance_type(&mut self, _owner: u8, object_id: i32) -> i32 {
        let unit = UnitIdentity {
            owner: self.owner() as u8,
            object_id,
        };
        self.unit_row(unit)
            .and_then(|row| self.sim.unit_type.get(row).copied())
            .and_then(|type_index| self.type_facts(type_index))
            .map_or(0, |facts| facts.stance_type)
    }

    fn producer_stance_type(&mut self, _build: &BuildData) -> i32 {
        self.type_facts(self.producer_type)
            .map_or(0, |facts| facts.stance_type)
    }

    fn set_unit_stance(&mut self, owner: u8, object_id: i32, stance: i32, _secondary: i32) {
        if let Some(row) = self.unit_row(UnitIdentity { owner, object_id }) {
            self.sim.world.units.stance_mut()[row] = stance as i8;
        } else {
            self.unsupported("set stance for missing allocated unit");
        }
    }

    fn put_unit_inside(
        &mut self,
        owner: u8,
        object_id: i32,
        producer_object_id: i32,
        producer_owner: u8,
        _secondary: i32,
    ) {
        if let Some(row) = self.unit_row(UnitIdentity { owner, object_id }) {
            self.sim.world.units.inside_up_mut()[row] = producer_object_id as i16;
            self.sim.world.units.inside_up_who_mut()[row] = producer_owner as i8;
        } else {
            self.unsupported("put missing allocated unit inside");
        }
    }

    fn producer_holds_air(&mut self, _build: &BuildData) -> bool {
        self.type_facts(self.producer_type)
            .is_some_and(|facts| facts.holds_air)
    }

    fn unit_object_masks(&mut self, unit: UnitIdentity) -> u32 {
        self.unit_row(unit)
            .and_then(|row| self.sim.unit_type.get(row).copied())
            .and_then(|type_index| self.type_facts(type_index))
            .map_or(0, |facts| facts.object_masks)
    }

    fn unit_type_flags(&mut self, unit: UnitIdentity) -> u32 {
        self.unit_row(unit)
            .and_then(|row| self.sim.unit_type.get(row).copied())
            .and_then(|type_index| self.type_facts(type_index))
            .map_or(0, |facts| facts.unit_flags)
    }

    fn unit_is_type(&mut self, unit: UnitIdentity, type_index: i32, _strict: i32) -> bool {
        self.unit_row(unit)
            .and_then(|row| self.sim.unit_type.get(row))
            .is_some_and(|&installed| installed == type_index)
    }

    fn validate_unit_rally_point(
        &mut self,
        build: &BuildData,
        request: UnitRallyPointRequest,
    ) -> UnitRallyPointReceipt {
        let valid = build
            .gather
            .get(request.index as usize)
            .is_some_and(|point| self.sim.map.world.valid_coord(point.x, point.y));
        UnitRallyPointReceipt { request, valid }
    }

    fn find_building_at_rally(
        &mut self,
        request: UnitRallyTargetRequest,
    ) -> UnitRallyTargetReceipt {
        self.unsupported("air rally target lookup");
        UnitRallyTargetReceipt {
            request,
            target: None,
        }
    }

    fn owners_are_enemies(&mut self, _owner: u8, _other_owner: u8) -> bool {
        self.unsupported("air rally diplomacy lookup");
        false
    }

    fn leader_has_prerequisite(&mut self, owner: u8, type_index: i32) -> bool {
        prerequisite_held(&self.runtime.leaders[owner as usize].tech, type_index)
    }

    fn target_can_carry(&mut self, _target: UnitIdentity, _unit: UnitIdentity) -> bool {
        self.unsupported("air carry admission");
        false
    }

    fn target_is_valid(&mut self, _target: UnitIdentity) -> bool {
        self.unsupported("air rally target validity");
        false
    }

    fn producer_garrison_limit(&mut self, _build: &BuildData) -> i32 {
        self.type_facts(self.producer_type)
            .map_or(10, |facts| facts.garrison_limit)
    }

    fn producer_is_type(&mut self, _build: &BuildData, type_index: i32, _strict: i32) -> bool {
        self.producer_type == type_index
    }

    fn trained_current_type(&mut self, type_index: i32) -> i32 {
        type_index
    }

    fn producer_num_inside(&mut self, build: &BuildData, _mode: i32) -> i32 {
        let producer_o = build.object_id();
        self.sim
            .world
            .units
            .inside_up()
            .iter()
            .zip(self.sim.world.units.inside_up_who())
            .filter(|(inside, who)| **inside == producer_o && **who == build.who as i8)
            .count() as i32
    }

    fn producer_gather_inside(&mut self, _build: &BuildData) -> bool {
        self.type_facts(self.producer_type)
            .is_some_and(|facts| facts.gather_inside)
    }

    fn local_player_owner(&mut self) -> u8 {
        self.runtime.local_player
    }

    fn scenario_presentation_count(&mut self) -> i32 {
        self.runtime.scenario_presentation_count
    }

    fn unit_has_orders(&mut self, request: UnitOrderStateRequest) -> UnitOrderStateReceipt {
        let has_orders = self
            .unit_row(request.unit)
            .is_some_and(|row| !self.sim.world.orders(row).is_empty());
        UnitOrderStateReceipt {
            request,
            has_orders,
        }
    }

    fn add_air_patrol_order(
        &mut self,
        request: UnitAirPatrolOrderRequest,
    ) -> UnitPlacementMutationReceipt {
        let mutation = UnitPlacementMutation::AddAirPatrol(request);
        let Some(row) = self.unit_row(request.unit) else {
            self.unsupported("air patrol for missing allocated unit");
            return UnitPlacementMutationReceipt { mutation };
        };
        let order = crate::systems::patrol::new_air_patrol(
            request.point.x,
            request.point.y,
            request.producer.object_id,
            request.producer.owner as i32,
            Some(self.producer_position),
        );
        self.sim.world.orders_mut(row).push(Order {
            kind: OrderIndex::AirPatrol,
            x: request.point.x,
            y: request.point.y,
            target_who: request.producer.owner as i8,
            target_o: request.producer.object_id as i16,
            ..Order::default()
        });
        self.runtime.air_patrol_orders.push(LiveAirPatrolOrder {
            unit: request.unit,
            order,
        });
        UnitPlacementMutationReceipt { mutation }
    }

    fn append_air_patrol_waypoint(
        &mut self,
        request: UnitAppendPatrolWaypointRequest,
    ) -> UnitPlacementMutationReceipt {
        let mutation = UnitPlacementMutation::AppendAirPatrolWaypoint(request);
        if let Some(order) = self
            .runtime
            .air_patrol_orders
            .iter_mut()
            .rev()
            .find(|order| order.unit == request.unit)
        {
            order.order.points.push(request.point.x, request.point.y);
        } else {
            self.unsupported("air patrol waypoint without live patrol order");
        }
        UnitPlacementMutationReceipt { mutation }
    }

    fn add_strafe_order(
        &mut self,
        request: UnitStrafeOrderRequest,
    ) -> UnitPlacementMutationReceipt {
        self.matching_placement(
            "strafe placement",
            UnitPlacementMutation::AddStrafe(request),
        )
    }

    fn come_out(&mut self, request: UnitComeOutRequest) -> UnitComeOutReceipt {
        let returned = if let Some(row) = self.unit_row(request.unit) {
            self.sim.world.units.inside_up_mut()[row] = -1;
            self.sim.world.units.inside_up_who_mut()[row] = -1;
            1
        } else {
            self.unsupported("come_out for missing allocated unit");
            0
        };
        UnitComeOutReceipt {
            mutation: UnitPlacementMutation::ComeOut(request),
            returned,
        }
    }

    fn check_producer_gatherers(
        &mut self,
        request: UnitPlacementRequest,
    ) -> UnitPlacementMutationReceipt {
        self.matching_placement(
            "University gatherer placement",
            UnitPlacementMutation::CheckProducerGatherers(request),
        )
    }

    fn destroy_at_air_capacity(
        &mut self,
        request: UnitPlacementRequest,
    ) -> UnitPlacementMutationReceipt {
        self.destroy_just_allocated_unit(request, "air-capacity destruction");
        UnitPlacementMutationReceipt {
            mutation: UnitPlacementMutation::DestroyAtAirCapacity(request),
        }
    }

    fn destroy_after_garrison_overflow(
        &mut self,
        request: UnitPlacementRequest,
    ) -> UnitPlacementMutationReceipt {
        self.destroy_just_allocated_unit(request, "garrison-overflow destruction");
        UnitPlacementMutationReceipt {
            mutation: UnitPlacementMutation::DestroyAfterGarrisonOverflow(request),
        }
    }

    fn clear_unit_launching(
        &mut self,
        request: UnitPlacementRequest,
    ) -> UnitPlacementMutationReceipt {
        if let Some(row) = self.unit_row(UnitIdentity {
            owner: request.owner,
            object_id: request.object_id,
        }) {
            self.sim.world.units.unit_masks_mut()[row] &= 0xfbff_ffffu32 as i32;
        } else {
            self.unsupported("clear launching for missing allocated unit");
        }
        UnitPlacementMutationReceipt {
            mutation: UnitPlacementMutation::ClearLaunching(request),
        }
    }

    fn complete_carrier_tail(
        &mut self,
        request: UnitPlacementRequest,
    ) -> UnitPlacementMutationReceipt {
        self.matching_placement(
            "aircraft-carrier completion tail",
            UnitPlacementMutation::CompleteCarrierTail(request),
        )
    }

    fn present_trained_unit(
        &mut self,
        request: UnitPlacementRequest,
        stage: UnitPresentationStage,
    ) -> UnitPlacementMutationReceipt {
        self.runtime.unit_presentations.push(LiveUnitPresentation {
            placement: request,
            stage,
        });
        UnitPlacementMutationReceipt {
            mutation: UnitPlacementMutation::Present {
                placement: request,
                stage,
            },
        }
    }
}

impl BuildingCompletionHost for SimFinishedHost<'_> {
    fn city_pop_value(&mut self, _build: &BuildData) -> i32 {
        self.type_facts(self.producer_type)
            .map_or(0, |facts| facts.city_pop_value)
    }

    fn set_building_type(&mut self, _build: &BuildData, type_index: i32, _secondary: i32) {
        self.runtime.register_build(self.producer_row, type_index);
    }

    fn mask_building(&mut self, _build: &BuildData, _mask: i32, _regen_roads: i32) {
        self.runtime.mask_effects = self.runtime.mask_effects.wrapping_add(1);
    }

    fn mark_leader_building_completed(&mut self, build: &BuildData, flag: u32) {
        self.sim.step8.leaders[build.who as usize].flags |= flag;
    }

    fn find_city_buildings(&mut self, _build: &BuildData) {
        self.unsupported("captured-building city scan");
    }

    fn adjust_leader_population(&mut self, _build: &BuildData, _delta: i32) {
        self.unsupported("captured-building leader population");
    }

    fn adjust_world_population(&mut self, delta: i32) {
        self.unsupported("captured-building world population");
        self.runtime.world_population = self.runtime.world_population.wrapping_add(delta);
    }

    fn building_region_index(&mut self, _build: &BuildData) -> i32 {
        self.unsupported("captured-building region lookup");
        64
    }

    fn adjust_region_population(&mut self, _build: &BuildData, _region: i32, _delta: i16) {
        self.unsupported("captured-building region population");
    }

    fn calc_population_cap(&mut self, _build: &BuildData) {
        self.unsupported("captured-building population-cap calculation");
    }

    fn fix_region_borders(&mut self) {
        self.unsupported("captured-building border repair");
    }
}

impl TechOneShotHost for SimFinishedHost<'_> {
    fn apply_tech_mutation(
        &mut self,
        state: &TechState,
        mutation: TechOneShotMutation,
    ) -> TechOneShotMutationReceipt {
        self.live_tech = state.clone();
        let owner = self.owner();
        match mutation {
            TechOneShotMutation::MarkGameTechDirty => self.runtime.game_tech_dirty = true,
            TechOneShotMutation::RecordAgeStamp { slot, frame, .. } => {
                if let Some(stamp) = self.runtime.leaders[owner].age_stamp.get_mut(slot) {
                    *stamp = frame;
                } else {
                    self.unsupported("age-stamp slot");
                }
            }
            TechOneShotMutation::GrantResource { resource, amount } => {
                self.runtime.leaders[owner].resources[resource] =
                    self.runtime.leaders[owner].resources[resource].wrapping_add(amount);
            }
            TechOneShotMutation::SetResourceAmount { resource, amount } => {
                self.runtime.leaders[owner].resources[resource] = amount;
            }
            TechOneShotMutation::SellResource { resource, .. } => {
                let bucket = &mut self.runtime.leaders[owner].resources[resource];
                *bucket = (*bucket).min(TECH_RESOURCE_SELL_FLOOR);
            }
            TechOneShotMutation::CompleteGainPreBitEffects(type_index)
            | TechOneShotMutation::CompleteGainAfterBitEffects(type_index)
            | TechOneShotMutation::CompleteGainBeforeAutoUnlockEffects(type_index)
            | TechOneShotMutation::CompleteGainAfterAutoUnlockEffects(type_index)
                if self
                    .type_facts(type_index)
                    .is_none_or(|facts| facts.tech_effects != LiveTechEffects::GenericOnly) =>
            {
                self.unsupported("opaque tech-specific gain effects");
            }
            _ => {}
        }
        TechOneShotMutationReceipt { mutation }
    }

    fn resource_prerequisite_held(&mut self, resource: usize) -> bool {
        prerequisite_held(
            &self.live_tech,
            self.runtime.leaders[self.owner()].resource_unlock_tech[resource],
        )
    }

    fn resource_unlock_tech(&mut self, resource: usize) -> i32 {
        self.runtime.leaders[self.owner()].resource_unlock_tech[resource]
    }

    fn resource_unlock_amount(&mut self, resource: usize) -> i32 {
        self.runtime.leaders[self.owner()].resource_unlock_amount[resource]
    }

    fn resource_sell_tech(&mut self, resource: usize) -> i32 {
        self.runtime.leaders[self.owner()].resource_sell_tech[resource]
    }

    fn resource_amount(&mut self, resource: usize) -> i32 {
        self.runtime.leaders[self.owner()].resources[resource]
    }

    fn lose_type_is_resource(&mut self, _type_index: i32) -> bool {
        false
    }

    fn lose_type_is_removable(&mut self, _type_index: i32) -> bool {
        false
    }
}

impl TechAutoUnlockHost for SimFinishedHost<'_> {
    fn has_prerequisite(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index).is_some_and(|facts| {
            facts
                .prerequisites
                .iter()
                .all(|&preq| prerequisite_held(&self.live_tech, preq))
        })
    }

    fn effective_prerequisite_count(&mut self, type_index: i32) -> i32 {
        self.type_facts(type_index)
            .map_or(0, |facts| facts.prerequisites.len() as i32)
    }

    fn effective_prerequisite(&mut self, type_index: i32, slot: i32) -> i32 {
        self.type_facts(type_index)
            .and_then(|facts| facts.prerequisites.get(slot as usize))
            .copied()
            .unwrap_or(-2)
    }

    fn candidate_is_town(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index)
            .is_some_and(|facts| facts.is_town)
    }

    fn building_flags(&mut self, type_index: i32) -> u32 {
        self.type_facts(type_index)
            .map_or(0, |facts| facts.build_flags)
    }

    fn type_eligible(&mut self, type_index: i32, _strict: i32) -> bool {
        self.type_facts(type_index)
            .is_some_and(|facts| facts.type_eligible)
    }

    fn has_tech_live(&mut self, type_index: i32) -> bool {
        self.live_tech.tech.get(type_index)
    }

    fn unit_flags(&mut self, type_index: i32) -> u32 {
        self.type_facts(type_index)
            .map_or(0, |facts| facts.unit_flags)
    }

    fn unit_object_masks(&mut self, type_index: i32) -> u32 {
        self.type_facts(type_index)
            .map_or(0, |facts| facts.object_masks)
    }

    fn apply_auto_unlock(
        &mut self,
        mutation: TechAutoUnlockMutation,
    ) -> TechAutoUnlockMutationReceipt {
        self.error
            .get_or_insert(LiveProductionError::RecursiveTechUnlock(
                self.finishing_type,
            ));
        TechAutoUnlockMutationReceipt { mutation }
    }
}

impl FinishedEffectHost for SimFinishedHost<'_> {
    fn is_unit_type(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index)
            .is_some_and(|facts| facts.class == LiveTypeClass::Unit)
    }

    fn can_make_unit(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index).is_some_and(|facts| {
            facts.can_make
                && facts
                    .prerequisites
                    .iter()
                    .all(|&preq| prerequisite_held(&self.live_tech, preq))
        })
    }

    fn is_spell_type(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index)
            .is_some_and(|facts| facts.class == LiveTypeClass::Spell)
    }

    fn has_spell(&mut self, _type_index: i32) -> bool {
        false
    }

    fn cast_spell(&mut self, _build: &BuildData, _type_index: i32) {
        self.unsupported("spell completion");
    }

    fn is_build_type(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index)
            .is_some_and(|facts| facts.class == LiveTypeClass::Building)
    }

    fn building_completion_gains_tech(&mut self, type_index: i32) -> bool {
        self.type_facts(type_index)
            .is_some_and(|facts| facts.building_completion == LiveBuildingCompletion::GainTech)
    }

    fn gain_tech_context(&mut self, _build: &BuildData, _type_index: i32) -> GainTechCohortContext {
        let mut context = self.runtime.leaders[self.owner()].gain_context;
        context.game_frame = self.sim.world.frame;
        context.local_player = self.owner() as u8 == self.runtime.local_player;
        context
    }

    fn producer_is_capitol(&mut self, _build: &BuildData) -> bool {
        self.type_facts(self.producer_type)
            .is_some_and(|facts| facts.is_capitol)
    }

    fn government_hero_type(&mut self) -> Option<i32> {
        self.unsupported("government-hero completion");
        None
    }

    fn complete_government_hero(&mut self, _build: &BuildData, _hero_type: i32) {
        self.unsupported("government-hero train or upgrade");
    }
}

/// Result of one live `Build::process -> Build::do_queue(0)` phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveProductionReceipt {
    pub build_row: usize,
    pub producer_type: i32,
    pub queue: RoutedQueueTransaction,
}

/// Execute the recovered routed queue and completion transactions against an ordinary
/// [`Sim`] building row. The caller is the `Build::process` arm at retail step 14.
///
/// Every installed queue type is preflighted before progress changes. Hosted true-plane
/// patrol and held-inside/gather-inside routes, including their capacity-destruction tails,
/// are executable. Carrier payload, University Scholar, single-rally missile/Helicopter,
/// spell, captured-building, recursive-tech, or opaque special-tech effects return an
/// error with the queue and RNG untouched.
pub fn process_sim_build_queue(
    sim: &mut Sim,
    runtime: &mut LiveProductionRuntime,
    row: usize,
) -> Result<LiveProductionReceipt, LiveProductionError> {
    let build = sim
        .builds
        .get(row)
        .ok_or(LiveProductionError::MissingBuildRow(row))?;
    if build.queue.queued == 0 {
        return Ok(LiveProductionReceipt {
            build_row: row,
            // No type query occurs in retail's empty `do_queue` return. Keep that
            // absence explicit when the installer has not yet registered this row.
            producer_type: runtime
                .build_types
                .get(row)
                .and_then(|type_index| *type_index)
                .unwrap_or(-1),
            queue: RoutedQueueTransaction::default(),
        });
    }
    if !build.is_active() {
        return Err(LiveProductionError::InactiveProducer(row));
    }
    let (owner, producer_type) = preflight(sim, runtime, row)?;
    rebuild_queued_counts(sim, runtime, owner);
    let rules = sim.prod_rules.clone();
    let ai_speed = runtime.leaders[owner].ai_speed;
    let mut build = std::mem::take(&mut sim.builds[row]);
    let snapshot = build.clone();
    let mut host = SimQueueHost {
        sim,
        runtime,
        producer_row: row,
        producer_type,
        producer_snapshot: snapshot,
        error: None,
    };
    let queue_result = execute_routed_queue_slots(&mut build, 0, ai_speed, &rules, &mut host);
    let callback_error = host.error;
    host.sim.builds[row] = build;
    let queue = queue_result?;
    if let Some(error) = callback_error {
        return Err(error);
    }
    Ok(LiveProductionReceipt {
        build_row: row,
        producer_type,
        queue,
    })
}

struct SimQueueHost<'a> {
    sim: &'a mut Sim,
    runtime: &'a mut LiveProductionRuntime,
    producer_row: usize,
    producer_type: i32,
    producer_snapshot: BuildData,
    error: Option<LiveProductionError>,
}

impl QueueCompletionHost for SimQueueHost<'_> {
    fn train_time(&mut self, type_index: i32) -> i32 {
        self.runtime
            .facts(type_index)
            .map_or(1, |facts| facts.train_time.max(1))
    }

    fn classify(&mut self, type_index: i32) -> QueueKind {
        let facts = self.runtime.facts(type_index);
        let (is_build, is_unit, can_make) = facts.map_or((false, false, false), |facts| {
            let owner = self.producer_snapshot.who as usize;
            let can_make = facts.can_make
                && self.runtime.leaders.get(owner).is_some_and(|leader| {
                    facts
                        .prerequisites
                        .iter()
                        .all(|&preq| prerequisite_held(&leader.tech, preq))
                });
            (
                facts.class == LiveTypeClass::Building,
                facts.class == LiveTypeClass::Unit,
                can_make,
            )
        });
        QueueKind::classify(type_index, is_build, is_unit, can_make)
    }

    fn finished(&mut self, type_index: i32, _queue: &BuildQueue, _slot: usize) -> bool {
        let owner = self.producer_snapshot.who as usize;
        let mut tech = std::mem::take(&mut self.runtime.leaders[owner].tech);
        let mut host = SimFinishedHost {
            sim: self.sim,
            runtime: self.runtime,
            owner,
            producer_row: self.producer_row,
            producer_type: self.producer_type,
            producer_position: self.producer_snapshot.position(),
            finishing_type: type_index,
            live_tech: tech.clone(),
            error: None,
        };
        let result =
            execute_finished_effect(&mut tech, &self.producer_snapshot, type_index, &mut host);
        let nested_error = host.error;
        host.runtime.leaders[owner].tech = tech;
        if let Some(error) = nested_error {
            self.error = Some(error);
            return false;
        }
        match result {
            Ok(effect) => {
                if !effect.allows_unqueue() {
                    self.sim.step8.leaders[owner].pop_issues =
                        self.sim.step8.leaders[owner].pop_issues.wrapping_add(1);
                }
                effect.allows_unqueue()
            }
            Err(error) => {
                self.error = Some(LiveProductionError::Finished(error));
                false
            }
        }
    }

    fn mark_queue_dirty(&mut self) {
        self.runtime.leaders[self.producer_snapshot.who as usize].queue_dirty = true;
    }

    fn completed_unqueue(&mut self, type_index: i32, _queue: &BuildQueue, _slot: usize) {
        let owner = self.producer_snapshot.who as usize;
        let Some(count) = self.runtime.leaders[owner]
            .queued_counts
            .get_mut(type_index as usize)
        else {
            self.error = Some(LiveProductionError::MissingQueuedCounter(type_index));
            return;
        };
        if *count <= 0 {
            self.error = Some(LiveProductionError::MissingQueuedCounter(type_index));
        } else {
            *count -= 1;
        }
    }

    fn repeat_unit(&mut self, build: &mut BuildData, type_index: i32) -> bool {
        let owner = self.producer_snapshot.who as usize;
        let Some(cost) = self
            .runtime
            .facts(type_index)
            .and_then(|facts| facts.repeat_cost)
        else {
            return false;
        };
        if cost
            .iter()
            .enumerate()
            .any(|(resource, &amount)| self.runtime.leaders[owner].resources[resource] < amount)
        {
            return false;
        }
        for (resource, amount) in cost.into_iter().enumerate() {
            self.runtime.leaders[owner].resources[resource] -= amount;
        }
        let slot = build.queue.queued as usize;
        let entry = BuildQueueEntry {
            type_index: type_index as i16,
            res: [-1; 3],
            ..BuildQueueEntry::default()
        };
        if slot < build.queue.entries.len() {
            build.queue.entries[slot] = entry;
        } else {
            build.queue.entries.push(entry);
        }
        build.queue.queued = build.queue.queued.saturating_add(1);
        self.runtime.leaders[owner].queued_counts[type_index as usize] += 1;
        true
    }
}

impl QueueRoutingHost for SimQueueHost<'_> {
    fn is_parallel_producer(&mut self, _build: &BuildData, _slot: usize) -> bool {
        self.runtime
            .facts(self.producer_type)
            .is_some_and(|facts| facts.parallel_slots > 1)
    }

    fn parallel_slot_limit(&mut self, _build: &BuildData, _slot: usize) -> usize {
        self.runtime
            .facts(self.producer_type)
            .map_or(1, |facts| facts.parallel_slots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRODUCER_TYPE: i32 = 430;

    fn queue_build(type_indices: &[i32]) -> BuildData {
        let mut build = BuildData {
            flags: flag::VALID | flag::ACTIVE,
            queue: BuildQueue {
                queued: type_indices.len() as u8,
                entries: type_indices
                    .iter()
                    .map(|&type_index| BuildQueueEntry {
                        elapsed: 1,
                        type_index: type_index as i16,
                        res: [-1; 3],
                        ..BuildQueueEntry::default()
                    })
                    .collect(),
            },
            ..BuildData::default()
        };
        let object_id = crate::objects::BUILD_BAND_BASE as i16;
        build.other[off::OBJECT_ID..off::OBJECT_ID + 2].copy_from_slice(&object_id.to_le_bytes());
        build.other[off::X_INTERNAL..off::X_INTERNAL + 4]
            .copy_from_slice(&(768i32 ^ 0x63637).to_le_bytes());
        build.other[off::Y_INTERNAL..off::Y_INTERNAL + 4]
            .copy_from_slice(&(960i32 ^ 0x63637).to_le_bytes());
        build
    }

    fn harness(type_indices: &[i32]) -> (Sim, LiveProductionRuntime, usize) {
        let mut sim = Sim::new(0x51de, 16);
        sim.activate(0);
        let row = sim.spawn_build(0, queue_build(type_indices));
        let mut runtime = LiveProductionRuntime::default();
        runtime.register_build(row, PRODUCER_TYPE);
        runtime.install_type(LiveProductionType::in_place_building(PRODUCER_TYPE, 1));
        (sim, runtime, row)
    }

    #[test]
    fn research_queue_completion_commits_tech_then_paid_unqueue_state() {
        let research_type = 600;
        let (mut sim, mut runtime, row) = harness(&[research_type]);
        runtime.install_type(LiveProductionType::research(research_type, 1));

        let receipt = process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        assert!(matches!(
            receipt.queue.slots.as_slice(),
            [RoutedQueueSlot {
                transaction: QueueTransaction::Completed {
                    type_index: 600,
                    repeat_attempted: false,
                    ..
                },
                ..
            }]
        ));
        assert!(runtime.leaders[0].tech.tech.get(research_type));
        assert_eq!(runtime.leaders[0].tech.counters.discovered, 1);
        assert!(runtime.game_tech_dirty);
        assert!(runtime.leaders[0].queue_dirty);
        assert_eq!(runtime.leaders[0].queued_counts[research_type as usize], 0);
        assert_eq!(sim.builds[row].queue.queued, 0);
        assert_eq!(sim.builds[row].queue.entries[0].elapsed, 0);
    }

    #[test]
    fn age_research_completion_updates_the_exact_age_counter_and_frame_stamp() {
        let age = crate::systems::tech_cities::ty::CLASSICAL_AGE;
        let (mut sim, mut runtime, row) = harness(&[age]);
        runtime.install_type(LiveProductionType::research(age, 1));
        sim.world.frame = 321;

        process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        assert!(runtime.leaders[0].tech.tech.get(age));
        assert_eq!(runtime.leaders[0].tech.counters.ages, 1);
        assert_eq!(runtime.leaders[0].tech.counters.discovered, 0);
        assert_eq!(runtime.leaders[0].age_stamp[0], 321);
    }

    #[test]
    fn ordinary_building_completion_retypes_the_live_build_before_unqueue() {
        let completed_type = 500;
        let (mut sim, mut runtime, row) = harness(&[completed_type]);
        runtime.install_type(LiveProductionType::in_place_building(completed_type, 1));

        process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        assert_eq!(runtime.build_types[row], Some(completed_type));
        assert_eq!(runtime.mask_effects, 1);
        assert_ne!(
            sim.step8.leaders[0].flags & LEADER_BUILDING_COMPLETED_FLAG,
            0
        );
        assert_eq!(sim.builds[row].queue.queued, 0);
    }

    #[test]
    fn ordinary_ground_unit_completion_allocates_places_and_publishes_identity() {
        let unit_type = 60;
        let (mut sim, mut runtime, row) = harness(&[unit_type]);
        runtime.install_type(LiveProductionType::ordinary_unit(unit_type, 1, 2));
        let before = sim.world.live_count();
        let rng_before = sim.world.random.state();

        process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        assert_eq!(sim.world.live_count(), before + 1);
        assert_eq!(sim.world.random.state(), rng_before);
        let unit_row = sim.world.objects.slot(0).band(Band::Unit)[0] as usize;
        assert_eq!(sim.unit_type[unit_row], unit_type);
        assert_eq!(sim.world.units.inside_up()[unit_row], -1);
        assert_eq!(sim.world.units.inside_up_who()[unit_row], -1);
        assert_eq!(sim.world.units.unit_masks()[unit_row] & 0x0400_0000, 0);
        assert_eq!(runtime.leaders[0].control, 2);
        assert_eq!(runtime.leaders[0].unit_counts[unit_type as usize], 1);
        assert_eq!(runtime.leaders[0].last_unit_built, 0);
        assert_eq!(runtime.leaders[0].last_unit_finished[unit_type as usize], 0);
        assert_eq!(sim.builds[row].queue.queued, 0);
    }

    #[test]
    fn hosted_air_completion_installs_one_live_patrol_and_appends_valid_waypoints() {
        let unit_type = 100;
        let (mut sim, mut runtime, row) = harness(&[unit_type]);
        runtime.install_type(LiveProductionType::hosted_air_unit(unit_type, 1, 1));
        runtime.types[PRODUCER_TYPE as usize]
            .as_mut()
            .unwrap()
            .holds_air = true;
        sim.builds[row].gather = vec![
            GatherPoint {
                x: 1_536,
                y: 1_920,
                ..GatherPoint::default()
            },
            GatherPoint {
                x: 2_304,
                y: 2_688,
                ..GatherPoint::default()
            },
        ];

        process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        let unit_row = sim.world.objects.slot(0).band(Band::Unit)[0] as usize;
        let generic = sim.world.orders(unit_row).current().unwrap();
        assert_eq!(sim.world.orders(unit_row).len(), 1);
        assert_eq!(generic.kind, OrderIndex::AirPatrol);
        assert_eq!((generic.x, generic.y), (1_536, 1_920));
        assert_eq!((generic.target_who, generic.target_o), (0, 2_000));
        assert_eq!(runtime.air_patrol_orders.len(), 1);
        assert_eq!(
            runtime.air_patrol_orders[0].order.points.x,
            vec![768, 2_304]
        );
        assert_eq!(
            runtime.air_patrol_orders[0].order.points.y,
            vec![960, 2_688]
        );
        assert_eq!(sim.world.units.inside_up()[unit_row], 2_000);
        assert_eq!(sim.builds[row].queue.queued, 0);
    }

    #[test]
    fn hosted_helicopter_remains_inside_or_is_destroyed_at_live_capacity() {
        let unit_type = UNIT_PLACEMENT_TYPE_HELICOPTER;
        let (mut sim, mut runtime, row) = harness(&[unit_type]);
        runtime.install_type(LiveProductionType::hosted_air_unit(unit_type, 1, 1));
        runtime.types[PRODUCER_TYPE as usize]
            .as_mut()
            .unwrap()
            .holds_air = true;
        runtime.leaders[0].aircraft_limit = 1;

        process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        let unit_row = sim.world.objects.slot(0).band(Band::Unit)[0] as usize;
        assert_eq!(sim.world.units.inside_up()[unit_row], 2_000);
        assert!(sim.world.orders(unit_row).is_empty());

        let (mut blocked_sim, mut blocked_runtime, blocked_row) = harness(&[unit_type]);
        blocked_runtime.install_type(LiveProductionType::hosted_air_unit(unit_type, 1, 1));
        blocked_runtime.types[PRODUCER_TYPE as usize]
            .as_mut()
            .unwrap()
            .holds_air = true;
        blocked_runtime.leaders[0].aircraft_limit = 0;
        let rng_before = blocked_sim.world.random.state();
        process_sim_build_queue(&mut blocked_sim, &mut blocked_runtime, blocked_row).unwrap();
        assert_eq!(blocked_sim.world.random.state(), rng_before);
        assert_eq!(blocked_sim.world.live_count(), 0);
        assert_eq!(blocked_sim.builds[blocked_row].queue.queued, 0);
        assert_eq!(blocked_runtime.leaders[0].control, 0);
        assert_eq!(
            blocked_runtime.leaders[0].unit_counts[unit_type as usize],
            0
        );
    }

    #[test]
    fn gather_inside_keeps_the_unit_contained_and_pins_both_presentation_stages() {
        let unit_type = 101;
        let (mut sim, mut runtime, row) = harness(&[unit_type]);
        runtime.install_type(LiveProductionType::ordinary_unit(unit_type, 1, 1));
        runtime.types[PRODUCER_TYPE as usize]
            .as_mut()
            .unwrap()
            .gather_inside = true;
        runtime.local_player = 0;
        runtime.scenario_presentation_count = 1;

        process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        let unit_row = sim.world.objects.slot(0).band(Band::Unit)[0] as usize;
        assert_eq!(sim.world.units.inside_up()[unit_row], 2_000);
        assert!(sim.world.orders(unit_row).is_empty());
        assert_eq!(
            runtime
                .unit_presentations
                .iter()
                .map(|presentation| presentation.stage)
                .collect::<Vec<_>>(),
            vec![
                UnitPresentationStage::HeldInside,
                UnitPresentationStage::Completed
            ]
        );
        assert_eq!(sim.builds[row].queue.queued, 0);
    }

    #[test]
    fn gather_inside_overflow_destroys_only_the_new_tail_unit_and_unqueues() {
        let unit_type = 103;
        let (mut sim, mut runtime, row) = harness(&[unit_type]);
        runtime.install_type(LiveProductionType::ordinary_unit(unit_type, 1, 2));
        let producer = runtime.types[PRODUCER_TYPE as usize].as_mut().unwrap();
        producer.gather_inside = true;
        producer.garrison_limit = 1;
        for _ in 0..2 {
            let handle = sim.spawn_unit(0, 0, 500, 500, 0).unwrap();
            let existing_row = sim.world.row_of(handle).unwrap();
            sim.world.units.inside_up_mut()[existing_row] = 2_000;
            sim.world.units.inside_up_who_mut()[existing_row] = 0;
        }
        let rng_before = sim.world.random.state();

        process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        assert_eq!(sim.world.random.state(), rng_before);
        assert_eq!(sim.world.live_count(), 2);
        assert_eq!(sim.unit_type.len(), 2);
        assert_eq!(runtime.leaders[0].control, 0);
        assert_eq!(runtime.leaders[0].unit_counts[unit_type as usize], 0);
        assert_eq!(sim.builds[row].queue.queued, 0);
    }

    #[test]
    fn single_rally_air_profiles_still_fail_before_allocation_or_rng() {
        let unit_type = 102;
        let (mut sim, mut runtime, row) = harness(&[unit_type]);
        let mut air = LiveProductionType::hosted_air_unit(unit_type, 1, 1);
        air.unit_flags |= UNIT_PLACEMENT_FLAG_HELICOPTER;
        runtime.install_type(air);
        runtime.types[PRODUCER_TYPE as usize]
            .as_mut()
            .unwrap()
            .holds_air = true;
        sim.builds[row].gather.push(GatherPoint {
            x: 1_536,
            y: 1_920,
            ..GatherPoint::default()
        });
        let rng_before = sim.world.random.state();

        assert_eq!(
            process_sim_build_queue(&mut sim, &mut runtime, row),
            Err(LiveProductionError::UnsupportedUnitPlacement(unit_type))
        );
        assert_eq!(sim.world.random.state(), rng_before);
        assert_eq!(sim.world.live_count(), 0);
        assert_eq!(sim.builds[row].queue.queued, 1);
    }

    #[test]
    fn unsupported_unit_route_blocks_before_queue_rng_or_world_mutation() {
        let unit_type = 61;
        let (mut sim, mut runtime, row) = harness(&[unit_type]);
        let mut facts = LiveProductionType::ordinary_unit(unit_type, 1, 2);
        facts.unit_placement = LiveUnitPlacement::Unsupported;
        runtime.install_type(facts);
        let queued_before = sim.builds[row].queue.queued;
        let entries_before = sim.builds[row].queue.entries.clone();
        let rng_before = sim.world.random.state();
        let live_before = sim.world.live_count();

        assert_eq!(
            process_sim_build_queue(&mut sim, &mut runtime, row),
            Err(LiveProductionError::UnsupportedUnitPlacement(unit_type))
        );

        assert_eq!(sim.builds[row].queue.queued, queued_before);
        assert_eq!(sim.builds[row].queue.entries, entries_before);
        assert_eq!(sim.world.random.state(), rng_before);
        assert_eq!(sim.world.live_count(), live_before);
        assert!(!runtime.leaders[0].queue_dirty);
    }

    #[test]
    fn carrier_identity_cannot_bypass_the_unsupported_placement_preflight() {
        let unit_type = UNIT_PLACEMENT_TYPE_AIRCRAFT_CARRIER;
        let (mut sim, mut runtime, row) = harness(&[unit_type]);
        runtime.install_type(LiveProductionType::ordinary_unit(unit_type, 1, 2));
        let rng_before = sim.world.random.state();

        assert_eq!(
            process_sim_build_queue(&mut sim, &mut runtime, row),
            Err(LiveProductionError::UnsupportedUnitPlacement(unit_type))
        );
        assert_eq!(sim.world.random.state(), rng_before);
        assert_eq!(sim.world.live_count(), 0);
        assert_eq!(sim.builds[row].queue.queued, 1);
    }

    #[test]
    fn parallel_live_completion_preserves_deepest_first_routing() {
        let first = 600;
        let second = 601;
        let (mut sim, mut runtime, row) = harness(&[first, second]);
        runtime.install_type(LiveProductionType::research(first, 1));
        runtime.install_type(LiveProductionType::research(second, 1));
        runtime.types[PRODUCER_TYPE as usize]
            .as_mut()
            .unwrap()
            .parallel_slots = 2;

        let receipt = process_sim_build_queue(&mut sim, &mut runtime, row).unwrap();

        assert_eq!(
            receipt
                .queue
                .slots
                .iter()
                .map(|slot| slot.slot)
                .collect::<Vec<_>>(),
            vec![1, 0]
        );
        assert!(runtime.leaders[0].tech.tech.get(first));
        assert!(runtime.leaders[0].tech.tech.get(second));
        assert_eq!(runtime.leaders[0].tech.counters.discovered, 2);
        assert_eq!(sim.builds[row].queue.queued, 0);
    }

    #[test]
    fn live_prerequisite_query_reads_installed_type_requirements_and_tech_bits() {
        let mut runtime = LiveProductionRuntime::default();
        let mut instant_timer_bonus = LiveProductionType::research(0x2b9, 1);
        instant_timer_bonus.prerequisites = vec![600, -1, 12];
        runtime.install_type(instant_timer_bonus);

        // Owning the queried row's raw bit is not the `has_preq` result: its installed
        // prerequisite still has to be held.
        runtime.leaders[0].tech.tech.set(0x2b9, true);
        assert!(!runtime.leader_has_prerequisites(0, 0x2b9));
        runtime.leaders[0].tech.tech.set(600, true);
        assert!(runtime.leader_has_prerequisites(0, 0x2b9));

        assert!(!runtime.leader_has_prerequisites(1, 0x2b9));
        assert!(!runtime.leader_has_prerequisites(0, 0x2ba));
        assert!(!runtime.leader_has_prerequisites(NUM_LEADERS, 0x2b9));
    }

    #[test]
    fn terminal_cleanup_traverses_only_valid_owned_builds_without_refunds() {
        let mut runtime = LiveProductionRuntime::default();
        runtime.leaders[0].queued_counts[60] = 1;
        runtime.leaders[0].queued_counts[61] = 1;
        runtime.leaders[0].resources = [100, 200, 300, 400, 500, 600];

        let mut owned = queue_build(&[60, 61]);
        owned.queue.entries[0].elapsed = 17;
        owned.queue.entries[1].elapsed = 29;
        owned.build_masks |= mask::REPEAT_QUEUE;
        let mut owned_empty = queue_build(&[]);
        owned_empty.build_masks |= mask::REPEAT_QUEUE;
        let mut invalid = queue_build(&[62]);
        invalid.flags &= !flag::VALID;
        invalid.build_masks |= mask::REPEAT_QUEUE;
        let mut enemy = queue_build(&[63]);
        enemy.who = 1;
        enemy.build_masks |= mask::REPEAT_QUEUE;
        let mut builds = vec![owned, owned_empty, invalid, enemy];

        let receipt = runtime.clean_terminal_build_queues(&mut builds, 0);

        assert_eq!(
            receipt,
            TerminalQueueCleanupReceipt {
                owner: 0,
                builds_visited: 2,
                queues_cleaned: 1,
                entries_removed: 2,
            }
        );
        assert_eq!(builds[0].queue.queued, 0);
        assert_eq!(builds[0].queue.entries.len(), 2);
        assert_eq!(builds[0].queue.entries[0].type_index, 60);
        assert_eq!(builds[0].queue.entries[1].type_index, 61);
        assert_eq!(builds[0].queue.entries[0].elapsed, 0);
        assert_eq!(builds[0].queue.entries[1].elapsed, 0);
        assert_eq!(builds[0].build_masks & mask::REPEAT_QUEUE, 0);
        assert_eq!(builds[1].build_masks & mask::REPEAT_QUEUE, 0);
        assert_eq!(runtime.leaders[0].queued_counts[60], 0);
        assert_eq!(runtime.leaders[0].queued_counts[61], 0);
        assert!(runtime.leaders[0].queue_dirty);
        assert_eq!(runtime.leaders[0].resources, [100, 200, 300, 400, 500, 600]);

        // Invalid and other-owner rows are outside retail's leader Build-band sweep.
        assert_eq!(builds[2].queue.queued, 1);
        assert_ne!(builds[2].build_masks & mask::REPEAT_QUEUE, 0);
        assert_eq!(builds[3].queue.queued, 1);
        assert_ne!(builds[3].build_masks & mask::REPEAT_QUEUE, 0);
    }

    #[test]
    fn ordinary_do_frame_executes_the_installed_live_production_phase() {
        let research_type = 602;
        let (mut sim, mut runtime, row) = harness(&[research_type]);
        runtime.install_type(LiveProductionType::research(research_type, 1));
        sim.production_runtime = runtime;

        let _trace = sim.do_frame();

        assert!(sim.production_runtime.leaders[0]
            .tech
            .tech
            .get(research_type));
        assert!(sim.production_runtime.game_tech_dirty);
        assert_eq!(sim.builds[row].queue.queued, 0);
    }
}
