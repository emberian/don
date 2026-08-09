//! Deterministic RL episode ownership over the retail-ordered [`don_sim::tick::Sim`].
//!
//! `VecEnv` currently owns a compact [`don_sim::World`] plus environment-local columns.
//! That remains useful as a throughput surface, but it cannot become the full-game fidelity
//! environment by accumulating more parallel state.  This module proves the opposite
//! ownership direction: a scenario creates the authoritative `Sim`, observations may borrow
//! its stores, actions must enter its command/order paths, and one environment frame is
//! `Sim::do_frame`.
//!
//! This tranche intentionally does not provide observations or actions.  Those adapters must
//! be read/write views over this owner; putting either policy-facing surface here before that
//! boundary exists would merely create a second model again.

use don_sim::systems::map_terrain::COORD_PER_WCELL;
use don_sim::tick::{Sim, StepRun, TickTrace, NUM_LEADERS, NUM_STEPS};
use don_sim::{Handle, MAX_UNITS};
use std::fmt;

/// One explicit unit in scenario allocation order.
///
/// Allocation order is observable through retail `(who, o, uid)` identity and object-band
/// traversal, so callers must not sort or otherwise canonicalise this list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioUnit {
    pub who: u8,
    pub type_id: i32,
    /// Position in retail `Coord` units (192 per terrain tile).
    pub x: i32,
    pub y: i32,
    /// Exact initial `ObjectData::mylos` input.  Scenario content owns this rule value.
    pub los_tiles: i8,
}

/// A bounded deterministic scenario declaration.
///
/// This is not a replacement for retail map/start generation.  It is the scenario-pack
/// primitive requested by the RL gate: generated or captured setup code must resolve its
/// results into these explicit inputs, after which ordinary training advances the same
/// `Sim` used by replay/save-load and the tick integration tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioSpec {
    pub seed: u64,
    /// Number of WCoord cells on each side; one WCoord cell is four terrain tiles.
    pub map_wcells: u16,
    /// Player slots to activate (`0..NUM_LEADERS`).
    pub active_players: Vec<u8>,
    pub units: Vec<ScenarioUnit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EpisodeError {
    EmptyMap,
    InvalidPlayer(u8),
    DuplicatePlayer(u8),
    UnitOwnerInactive { unit: usize, who: u8 },
    UnitOffMap { unit: usize, x: i32, y: i32 },
    UnitCapacity { requested: usize, capacity: usize },
    SpawnRefused { unit: usize },
}

impl fmt::Display for EpisodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMap => write!(f, "scenario map must contain at least one WCoord cell"),
            Self::InvalidPlayer(who) => write!(f, "scenario player slot {who} is not playable"),
            Self::DuplicatePlayer(who) => {
                write!(f, "scenario activates player slot {who} more than once")
            }
            Self::UnitOwnerInactive { unit, who } => {
                write!(f, "scenario unit {unit} belongs to inactive player {who}")
            }
            Self::UnitOffMap { unit, x, y } => {
                write!(f, "scenario unit {unit} is off-map at ({x}, {y})")
            }
            Self::UnitCapacity {
                requested,
                capacity,
            } => write!(
                f,
                "scenario requests {requested} units but the core capacity is {capacity}"
            ),
            Self::SpawnRefused { unit } => {
                write!(f, "authoritative core refused scenario unit {unit}")
            }
        }
    }
}

impl std::error::Error for EpisodeError {}

/// Aggregate proof of which retail tick stages a policy step actually reached.
///
/// Counts are indexed by the 29-entry `Game::do_frame` schedule.  This is deliberately
/// richer than an elapsed-frame count: training/evaluation can reject a scenario whose
/// supposedly reachable system remained vacuous or unimplemented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepReceipt {
    pub start_frame: i32,
    pub end_frame: i32,
    pub frames: u32,
    pub executed: [u64; NUM_STEPS],
    pub vacuous: [u64; NUM_STEPS],
    pub unimplemented: [u64; NUM_STEPS],
    pub out_of_scope: [u64; NUM_STEPS],
    pub work: [u64; NUM_STEPS],
}

impl StepReceipt {
    fn new(start_frame: i32) -> Self {
        Self {
            start_frame,
            end_frame: start_frame,
            frames: 0,
            executed: [0; NUM_STEPS],
            vacuous: [0; NUM_STEPS],
            unimplemented: [0; NUM_STEPS],
            out_of_scope: [0; NUM_STEPS],
            work: [0; NUM_STEPS],
        }
    }

    fn record(&mut self, trace: &TickTrace) {
        for stage in 0..NUM_STEPS {
            match trace.steps[stage] {
                StepRun::Executed => self.executed[stage] += 1,
                StepRun::Vacuous => self.vacuous[stage] += 1,
                StepRun::Unimplemented(_) => self.unimplemented[stage] += 1,
                StepRun::OutOfScope => self.out_of_scope[stage] += 1,
            }
            self.work[stage] += u64::from(trace.work[stage]);
        }
        self.frames += 1;
    }

    /// True only when no top-level stage reported an unimplemented branch during this step.
    /// Inner system fidelity still needs its own coverage gate.
    pub fn top_level_complete(&self) -> bool {
        self.unimplemented.iter().all(|&count| count == 0)
    }
}

/// An RL episode whose sole mutable game-state owner is [`Sim`].
pub struct AuthoritativeEpisode {
    spec: ScenarioSpec,
    sim: Sim,
    spawned: Vec<Handle>,
}

impl AuthoritativeEpisode {
    /// Validate all declarative inputs before allocating or mutating the simulation.
    pub fn from_spec(spec: ScenarioSpec) -> Result<Self, EpisodeError> {
        validate(&spec)?;

        let mut sim = Sim::new(spec.seed, spec.map_wcells);
        // `Sim::new` seeds the main Random stream. The terrain owner retains the same setup
        // seed for the future world-generation continuation and for save/replay state.
        sim.map.world.seed = spec.seed as i32;

        // Activation order is slot order, independent of declaration ordering.
        let mut active = [false; NUM_LEADERS];
        for &who in &spec.active_players {
            active[usize::from(who)] = true;
        }
        for (who, enabled) in active.iter().copied().enumerate() {
            if enabled {
                sim.activate(who);
            }
        }

        let mut spawned = Vec::with_capacity(spec.units.len());
        for (index, unit) in spec.units.iter().copied().enumerate() {
            let Some(handle) = sim.spawn_unit(
                usize::from(unit.who),
                unit.type_id,
                unit.x,
                unit.y,
                unit.los_tiles,
            ) else {
                return Err(EpisodeError::SpawnRefused { unit: index });
            };
            spawned.push(handle);
        }

        Ok(Self { spec, sim, spawned })
    }

    pub fn spec(&self) -> &ScenarioSpec {
        &self.spec
    }

    /// Stable handles in scenario allocation order.
    pub fn spawned(&self) -> &[Handle] {
        &self.spawned
    }

    /// Read-only observation/reward adapters borrow the authoritative owner here.
    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// Crate-internal ownership seam for the side-by-side fidelity backend. Policy callers
    /// never receive this mutable reference; the backend admits only typed transactions.
    pub(crate) fn sim_mut_for_backend(&mut self) -> &mut Sim {
        &mut self.sim
    }

    /// Rebuild the declared scenario from its seed and ordered inputs.
    pub fn reset(&mut self) -> Result<(), EpisodeError> {
        let replacement = Self::from_spec(self.spec.clone())?;
        *self = replacement;
        Ok(())
    }

    /// Advance the authoritative 29-stage frame driver, aggregating reachability evidence.
    pub fn step_frames(&mut self, frames: u32) -> StepReceipt {
        let mut receipt = StepReceipt::new(self.sim.world.frame);
        for _ in 0..frames {
            let trace = self.sim.do_frame();
            receipt.record(&trace);
        }
        receipt.end_frame = self.sim.world.frame;
        receipt
    }
}

fn validate(spec: &ScenarioSpec) -> Result<(), EpisodeError> {
    if spec.map_wcells == 0 {
        return Err(EpisodeError::EmptyMap);
    }
    if spec.units.len() > MAX_UNITS {
        return Err(EpisodeError::UnitCapacity {
            requested: spec.units.len(),
            capacity: MAX_UNITS,
        });
    }

    let mut active = [false; NUM_LEADERS];
    for &who in &spec.active_players {
        let slot = usize::from(who);
        if slot >= NUM_LEADERS {
            return Err(EpisodeError::InvalidPlayer(who));
        }
        if std::mem::replace(&mut active[slot], true) {
            return Err(EpisodeError::DuplicatePlayer(who));
        }
    }

    let extent = i32::from(spec.map_wcells) * COORD_PER_WCELL;
    for (index, unit) in spec.units.iter().enumerate() {
        if usize::from(unit.who) >= NUM_LEADERS || !active[usize::from(unit.who)] {
            return Err(EpisodeError::UnitOwnerInactive {
                unit: index,
                who: unit.who,
            });
        }
        if !(0..extent).contains(&unit.x) || !(0..extent).contains(&unit.y) {
            return Err(EpisodeError::UnitOffMap {
                unit: index,
                x: unit.x,
                y: unit.y,
            });
        }
    }
    Ok(())
}
