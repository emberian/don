//! Load the shipped BHS program image selected by a standard replay.
//!
//! The replay prefix does not serialize the production-script string stored at
//! `LeaderData+0x6ea4`.  It does serialize the player flags which decide whether a
//! production AI exists.  For the stock, non-scenario/no-mod setup supported here,
//! live retail evidence identifies that production script as `economic`; the seven
//! corpus recordings without an AI have an empty script-file registry, while all
//! fourteen recordings with an AI have three files (economic, its include, then
//! general powers).  Unsupported setup shapes fail closed instead of guessing a
//! custom or mod-selected program.

use std::path::Path;

use don_bhs::program::{Program, ProgramWalkMeta};
use don_bhs::ScriptTimers;
use don_bhs_cc::load;
use don_sim::script_runtime::{ScriptBindError, ScriptBinding, ScriptRuntime};

use crate::initial::InitialState;
use crate::script_channel::{checksum_program, ScriptChannelChecksum, ScriptChannelError};

/// `LeaderData::leader_flags & 4`, the retail HUMAN bit.
pub const LEADER_FLAG_HUMAN: u16 = 4;

/// Production script observed in a live ordinary skirmish AI at `LeaderData+0x6ea4`.
pub const STANDARD_AI_SCRIPT_FILE: &str = "ai/scripts/economic.bhs";

/// Why this adapter selected a particular global ScriptFile registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayBhsSelection {
    /// A stock recording with no active non-human player.  This is the exact shape of
    /// the seven corpus recordings whose channel 15 is the empty count word forever.
    EmptyNoAi,
    /// A stock skirmish containing the listed active AI slots.  Retail compiles one
    /// `economic` unit (which includes `aibestbuildlibrary`) and then general powers.
    StandardAiEconomic { ai_slots: Vec<u8> },
}

/// Name and root-file index of one entry in the loaded global registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayBhsBinding {
    pub file: usize,
    pub name: String,
}

/// Replay-owned compiled BHS state, ready for `ScriptRuntime` and channel 15.
#[derive(Debug)]
pub struct ReplayBhsProgram {
    pub selection: ReplayBhsSelection,
    pub program: Program,
    pub checksum: ScriptChannelChecksum,
    /// The per-leader production entry.  This is not the zero-argument
    /// `Game::do_frame` game-script slot: the AI stage calls it with four arguments.
    pub production: Option<ReplayBhsBinding>,
    pub general_powers: Option<ReplayBhsBinding>,
}

impl ReplayBhsProgram {
    /// Transfer the loaded image into the one persistent runtime used by `don-sim`.
    ///
    /// Only general powers is a zero-argument step-4 binding.  The `economic` program
    /// remains in the same registry for the production-AI stage to invoke with its four
    /// live arguments once that caller is connected.
    pub fn into_script_runtime(
        self,
    ) -> Result<(ScriptRuntime, Option<ReplayBhsBinding>), ScriptBindError> {
        self.into_script_runtime_with_timers(ScriptTimers::default())
    }

    /// Transfer the loaded image while retaining an exact caller-supplied timer owner.
    ///
    /// Production AI invokes timer builtins through the same persistent runtime as channel 15;
    /// replay integration must therefore install its admitted timer image at construction rather
    /// than mutating a detached timer container after the program has been bound.
    pub fn into_script_runtime_with_timers(
        self,
        timers: ScriptTimers,
    ) -> Result<(ScriptRuntime, Option<ReplayBhsBinding>), ScriptBindError> {
        let binding = |value: ReplayBhsBinding| ScriptBinding::new(value.file, value.name);
        let runtime = ScriptRuntime::new_with_timers(
            self.program,
            None,
            self.general_powers.map(binding),
            timers,
        )?;
        Ok((runtime, self.production))
    }
}

#[derive(Debug)]
pub enum ReplayBhsLoadError {
    /// Custom scenarios, non-default script modes and mods can select a different root
    /// script.  The replay prefix currently supplies no exact filename for those modes.
    UnsupportedSetup {
        scenario_type: u8,
        script_type: u8,
        mods: u8,
    },
    Compile {
        script: &'static str,
        source: load::LoadError,
    },
    MissingWalkMetadata {
        script: &'static str,
    },
    NonDefaultTypeRegistry {
        script: &'static str,
    },
    InvalidEntry {
        script: &'static str,
        entry: String,
        expected_arity: usize,
        actual_arity: Option<usize>,
    },
    Walk(ScriptChannelError),
}

impl std::fmt::Display for ReplayBhsLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSetup {
                scenario_type,
                script_type,
                mods,
            } => write!(
                f,
                "unsupported replay BHS selection: scenario_type={scenario_type}, \
                 script_type={script_type}, mods={mods}"
            ),
            Self::Compile { script, source } => write!(f, "compile {script}: {source}"),
            Self::MissingWalkMetadata { script } => {
                write!(f, "compiled {script} has no channel-15 sidecar")
            }
            Self::NonDefaultTypeRegistry { script } => {
                write!(
                    f,
                    "compiled {script} changed the process-global type registry"
                )
            }
            Self::InvalidEntry {
                script,
                entry,
                expected_arity,
                actual_arity,
            } => write!(
                f,
                "compiled {script} entry `{entry}` has arity {actual_arity:?}, expected \
                 {expected_arity}"
            ),
            Self::Walk(source) => write!(f, "walk loaded BHS program: {source}"),
        }
    }
}

impl std::error::Error for ReplayBhsLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compile { source, .. } => Some(source),
            Self::Walk(source) => Some(source),
            _ => None,
        }
    }
}

fn compile(
    inc: &don_bhs_cc::IncludePath,
    script: &'static str,
) -> Result<load::LoadedScript, ReplayBhsLoadError> {
    load::load_script(inc, script).map_err(|source| ReplayBhsLoadError::Compile { script, source })
}

fn require_entry_arity(
    loaded: &load::LoadedScript,
    script: &'static str,
    expected_arity: usize,
) -> Result<(), ReplayBhsLoadError> {
    let actual_arity = loaded
        .program
        .files
        .first()
        .and_then(|file| file.find_script(&loaded.entry))
        .map(|index| loaded.program.files[0].scripts[index].arity);
    if actual_arity != Some(expected_arity) {
        return Err(ReplayBhsLoadError::InvalidEntry {
            script,
            entry: loaded.entry.clone(),
            expected_arity,
            actual_arity,
        });
    }
    Ok(())
}

/// Merge two fresh source-compiler results while retaining their exact walk sidecars.
///
/// `Program::append` deliberately rejects a sidecar because arbitrary metadata may use
/// global linked-file indices.  These two programs came directly from `load_script`, so
/// offsetting every non-negative index in the later sidecar is the complete relocation.
fn merge_compiled_units(
    mut first: Program,
    first_name: &'static str,
    mut second: Program,
    second_name: &'static str,
) -> Result<Program, ReplayBhsLoadError> {
    let defaults = Program::default();
    if first.global_type_names() != defaults.global_type_names() {
        return Err(ReplayBhsLoadError::NonDefaultTypeRegistry { script: first_name });
    }
    if second.global_type_names() != defaults.global_type_names() {
        return Err(ReplayBhsLoadError::NonDefaultTypeRegistry {
            script: second_name,
        });
    }

    let base = first.files.len();
    let mut first_meta = first
        .walk_meta()
        .cloned()
        .ok_or(ReplayBhsLoadError::MissingWalkMetadata { script: first_name })?;
    let mut second_meta =
        second
            .walk_meta()
            .cloned()
            .ok_or(ReplayBhsLoadError::MissingWalkMetadata {
                script: second_name,
            })?;
    for file in &mut second_meta.files {
        for index in &mut file.linked_file_indices {
            if *index >= 0 {
                *index += base as i32;
            }
        }
    }

    first.files.append(&mut second.files);
    first_meta.files.append(&mut second_meta.files);
    first.set_walk_meta(ProgramWalkMeta {
        files: first_meta.files,
    });
    Ok(first)
}

/// Select and load the replay's stock BHS registry from an explicit shipped-content root.
///
/// `content_root` is the install-shaped directory containing `ai/` and `scenario/`; in
/// this repository's private extraction it is `ron-data/bhs-corpus`.  Keeping it explicit
/// prevents a replay filename from silently selecting an unrelated install.
pub fn load_replay_bhs_program(
    initial: &InitialState,
    content_root: &Path,
) -> Result<ReplayBhsProgram, ReplayBhsLoadError> {
    let settings = initial.info.settings;
    if settings.scenario_type != 0 || settings.script_type != 0 || settings.mods != 0 {
        return Err(ReplayBhsLoadError::UnsupportedSetup {
            scenario_type: settings.scenario_type,
            script_type: settings.script_type,
            mods: settings.mods,
        });
    }

    let ai_slots = initial
        .active_players()
        .filter(|player| player.flags & LEADER_FLAG_HUMAN == 0)
        .map(|player| player.slot)
        .collect::<Vec<_>>();
    if ai_slots.is_empty() {
        let program = Program::default().with_walk_meta(ProgramWalkMeta::default());
        let checksum = checksum_program(&program).map_err(ReplayBhsLoadError::Walk)?;
        return Ok(ReplayBhsProgram {
            selection: ReplayBhsSelection::EmptyNoAi,
            program,
            checksum,
            production: None,
            general_powers: None,
        });
    }

    let inc = load::install_include_path(content_root.to_path_buf());
    let economic = compile(&inc, STANDARD_AI_SCRIPT_FILE)?;
    let general = compile(&inc, load::GENERAL_POWERS_SCRIPT_FILE)?;
    require_entry_arity(&economic, STANDARD_AI_SCRIPT_FILE, 4)?;
    require_entry_arity(&general, load::GENERAL_POWERS_SCRIPT_FILE, 0)?;
    let general_base = economic.program.files.len();
    let production = ReplayBhsBinding {
        file: 0,
        name: economic.entry.clone(),
    };
    let general_powers = ReplayBhsBinding {
        file: general_base,
        name: general.entry.clone(),
    };
    let program = merge_compiled_units(
        economic.program,
        STANDARD_AI_SCRIPT_FILE,
        general.program,
        load::GENERAL_POWERS_SCRIPT_FILE,
    )?;
    let checksum = checksum_program(&program).map_err(ReplayBhsLoadError::Walk)?;
    Ok(ReplayBhsProgram {
        selection: ReplayBhsSelection::StandardAiEconomic { ai_slots },
        program,
        checksum,
        production: Some(production),
        general_powers: Some(general_powers),
    })
}
