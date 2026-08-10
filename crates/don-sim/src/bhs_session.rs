//! Opaque production ownership for one simulation and its BHS type state.
//!
//! [`crate::script_runtime::ScriptRuntime`] historically lived beside [`crate::tick::Sim`].
//! That shape allowed callers to execute a script-free frame, call the legacy v6 save writer,
//! or compute the partial simulation digest while a canonical mutable type owner existed in the
//! adjacent runtime.  `BhsSession` consumes both values, admits one synchronized factory image,
//! and deliberately exposes neither value again.  Every reachable frame, save, and digest path
//! therefore crosses the same installed owner.

use std::fmt;

use crate::script_runtime::{ScriptRunError, ScriptRuntime};
use crate::systems::bhs_type_factory::{
    produce_type_builtin_state, TypeBuiltinFactoryError, TypeBuiltinFactoryInput,
    TypeBuiltinProvenance,
};
use crate::systems::bhs_type_channel13_frontier::{
    ProjectedTypeRules, TypeChannel13Error, TypePersistenceOwner, TypeWalkSource,
};
use crate::systems::bhs_type_runtime::{
    TypeBuiltinBoundaryError, TypeBuiltinReceipt, TypeBuiltinRuntime,
};
use crate::systems::bhs_type_table::TypeBuiltinState;
use crate::systems::save_load::{save_sim_with_scripts, SaveError};
use crate::tick::{Sim, TickTrace};

/// A setup failure before an opaque session can exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BhsSessionSetupError {
    /// The synchronized rules/mod composition did not satisfy the canonical type factory.
    Factory(TypeBuiltinFactoryError),
    /// The supplied runtime already owns an unprovenanced or independently produced type state.
    PreinstalledTypeOwner,
    /// Retail step 4 was already entered, so installation would occur after the first script
    /// frame rather than during production setup.
    ScriptRuntimeAlreadyStarted,
    /// The witnessed type owner and consumed simulation disagree about the retail Leader gate
    /// bytes that select immediate stat-recalculation slots.
    LeaderFlagsMismatch {
        slot: usize,
        type_owner: i32,
        sim: u32,
    },
    /// The immutable normalized Type walk does not admit the produced canonical owner.
    Channel13(TypeChannel13Error),
}

impl fmt::Display for BhsSessionSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Factory(error) => write!(f, "BHS type setup failed: {error}"),
            Self::PreinstalledTypeOwner => {
                write!(
                    f,
                    "BHS runtime already contains an independently installed type owner"
                )
            }
            Self::ScriptRuntimeAlreadyStarted => {
                write!(f, "BHS runtime has already entered a script frame")
            }
            Self::LeaderFlagsMismatch {
                slot,
                type_owner,
                sim,
            } => write!(
                f,
                "BHS type owner Leader flags at slot {slot} are {type_owner:#x}, simulation has {sim:#x}"
            ),
            Self::Channel13(error) => write!(f, "BHS channel-13 setup failed: {error}"),
        }
    }
}

impl std::error::Error for BhsSessionSetupError {}

impl From<TypeBuiltinFactoryError> for BhsSessionSetupError {
    fn from(value: TypeBuiltinFactoryError) -> Self {
        Self::Factory(value)
    }
}

/// Read-only scalar evidence from the joined owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BhsSessionStatus {
    pub frame: i32,
    pub seconds: i32,
    pub script_calls: u64,
    pub script_bytecodes: u64,
    pub type_mutation_revision: u64,
    pub type_state_dirty: bool,
    pub type_channel13_owned: bool,
}

/// One non-cloneable production session.
///
/// There is intentionally no `Deref`, `AsRef<Sim>`, `sim()` accessor, or `into_parts` method.
/// `Sim` is not cloneable, so consuming it here removes the legacy script-free frame/save/digest
/// surface for the lifetime of the installed BHS owner.  Read-only type access is safe because it
/// cannot recover the enclosed `Sim` or replace the immutable provenance witness.
pub struct BhsSession {
    sim: Sim,
    scripts: ScriptRuntime,
    type_provenance: TypeBuiltinProvenance,
}

impl BhsSession {
    /// Build and install the canonical type owner before the session can execute its first frame.
    ///
    /// The input is admitted by [`produce_type_builtin_state`] inside this constructor.  A runtime
    /// that was populated through the older state-only installation API is rejected: it cannot be
    /// paired after the fact with a provenance value that may describe different backups.
    pub fn new(
        sim: Sim,
        scripts: ScriptRuntime,
        type_input: TypeBuiltinFactoryInput,
    ) -> Result<Self, BhsSessionSetupError> {
        Self::build(sim, scripts, type_input, None)
    }

    /// Build a session whose canonical owner also retains the exact normalized Type prefix of
    /// checksum channel 13. The source is admitted while the owner is pristine and then remains
    /// immutable while script calls mutate only the canonical table.
    pub fn new_with_channel13(
        sim: Sim,
        scripts: ScriptRuntime,
        type_input: TypeBuiltinFactoryInput,
        channel13_source: TypeWalkSource,
    ) -> Result<Self, BhsSessionSetupError> {
        Self::build(sim, scripts, type_input, Some(channel13_source))
    }

    fn build(
        sim: Sim,
        mut scripts: ScriptRuntime,
        type_input: TypeBuiltinFactoryInput,
        channel13_source: Option<TypeWalkSource>,
    ) -> Result<Self, BhsSessionSetupError> {
        if scripts.type_builtins().is_some() {
            return Err(BhsSessionSetupError::PreinstalledTypeOwner);
        }
        if scripts.has_started() {
            return Err(BhsSessionSetupError::ScriptRuntimeAlreadyStarted);
        }

        let produced = produce_type_builtin_state(type_input)?;
        let type_provenance = produced.provenance();
        let (state, retained_provenance) = produced.into_parts();
        debug_assert_eq!(type_provenance, retained_provenance);
        for (slot, (type_leader, sim_leader)) in state
            .leaders
            .iter()
            .zip(sim.step8.leaders.iter())
            .enumerate()
        {
            if type_leader.leader_flags as u32 != sim_leader.flags {
                return Err(BhsSessionSetupError::LeaderFlagsMismatch {
                    slot,
                    type_owner: type_leader.leader_flags,
                    sim: sim_leader.flags,
                });
            }
        }
        match channel13_source {
            Some(source) => {
                let runtime = TypeBuiltinRuntime::new_with_channel13(
                    state,
                    type_provenance,
                    source,
                )
                .map_err(BhsSessionSetupError::Channel13)?;
                scripts
                    .install_type_builtin_runtime(runtime)
                    .map_err(|_| BhsSessionSetupError::PreinstalledTypeOwner)?;
            }
            None => scripts
                .install_type_builtins(state)
                .map_err(|_| BhsSessionSetupError::PreinstalledTypeOwner)?,
        }

        Ok(Self {
            sim,
            scripts,
            type_provenance,
        })
    }

    /// Immutable rules/mod identity paired with the installed mutable owner.
    pub fn type_provenance(&self) -> TypeBuiltinProvenance {
        self.type_provenance
    }

    /// Immutable inspection of the installed type rows and Leader masks.
    pub fn type_state(&self) -> &TypeBuiltinState {
        self.type_runtime().state()
    }

    pub fn last_type_builtin_receipt(&self) -> Option<&TypeBuiltinReceipt> {
        self.scripts.last_type_builtin_receipt()
    }

    pub fn status(&self) -> BhsSessionStatus {
        let state = self.type_state();
        BhsSessionStatus {
            frame: self.sim.world.frame,
            seconds: self.sim.world.seconds,
            script_calls: self.scripts.calls(),
            script_bytecodes: self.scripts.bytecodes(),
            type_mutation_revision: state.mutation_revision(),
            type_state_dirty: state.is_dirty(),
            type_channel13_owned: self.type_runtime().has_channel13_source(),
        }
    }

    /// Live revision-bound projection of the exact Type prefix of retail checksum channel 13.
    pub fn projected_type_rules(&self) -> Result<ProjectedTypeRules<'_>, TypeChannel13Error> {
        self.type_runtime().projected_type_rules()
    }

    /// Cumulative Adler-32 after the 806 Type virtual walks, before Constants/Balance/Tribes.
    pub fn type_channel13_checkpoint(&self) -> Result<u32, TypeChannel13Error> {
        self.type_runtime().type_channel13_checkpoint()
    }

    /// Borrow the full canonical owner, exact provenance, live revision, and admitted checkpoint.
    /// DoNSave v6 remains red because it has no encoding/restoration path for this contract.
    pub fn type_persistence_owner(
        &self,
    ) -> Result<TypePersistenceOwner<'_>, TypeChannel13Error> {
        self.type_runtime().type_persistence_owner()
    }

    /// Execute retail step 4 and the remaining frame against the joined owners.
    pub fn do_frame(&mut self) -> Result<TickTrace, ScriptRunError> {
        self.sim.do_frame_with_scripts(&mut self.scripts)
    }

    /// DoNSave v6 remains red for every installed owner, including a pristine one.
    ///
    /// This method cannot fall through to `save_sim(&Sim)`: admission runs first and currently
    /// refuses because v6 has no provenance/type-state section or reconstruction path.
    pub fn save_v6(&self) -> Result<Vec<u8>, SaveError> {
        save_sim_with_scripts(&self.sim, &self.scripts)
    }

    /// Admit the existing non-retail partial Sim digest only after the live Type projection passes.
    /// The exact Types checkpoint is exposed separately by [`Self::type_channel13_checkpoint`].
    pub fn partial_channel_digest(&self) -> Result<u64, TypeBuiltinBoundaryError> {
        self.scripts.admitted_sim_channel_digest(&self.sim)
    }

    fn type_runtime(&self) -> &TypeBuiltinRuntime {
        self.scripts
            .type_builtins()
            .expect("BhsSession constructor installs exactly one canonical type owner")
    }
}
