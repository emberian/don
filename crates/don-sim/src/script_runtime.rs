//! The persistent BHS runtime consumed by [`crate::tick::Sim`] at step 4.
//!
//! Retail's `Game::do_frame` calls `RunTimeEnv::run_script` twice at step 4: first the
//! selected game script every frame, then the general-powers script only when
//! `Game::frame > 0`. Both are zero-argument entry points and both run before leaders,
//! objects, and the frame increment. This module supplies that exact producer boundary
//! over [`don_bhs::Vm`].
//!
//! The 31 recovered utility builtins execute here. Their RNG operations share
//! [`crate::rng::Random`] with the rest of the simulation. The 842 ScenarioFuncSet
//! entries are deliberately not stubbed: reaching one returns [`ScriptRunError`] and
//! the tick stops before any later subsystem runs.

use std::fmt;

use don_bhs::{
    call_util, BuiltinDecl, Host, HostError, HostResult, Program, RuntimeError, Value, Vm, VmError,
};

use crate::rng::Random;

/// Which of the two measured `Game::do_frame` script slots is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptSlot {
    Game,
    GeneralPowers,
}

/// A zero-argument script selected from one loaded [`Program`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptBinding {
    pub file: usize,
    pub name: String,
}

impl ScriptBinding {
    pub fn new(file: usize, name: impl Into<String>) -> Self {
        ScriptBinding {
            file,
            name: name.into(),
        }
    }
}

/// Why a binding cannot represent retail's zero-argument tick call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptBindError {
    BadFile(usize),
    MissingScript {
        file: usize,
        name: String,
    },
    NonzeroArity {
        file: usize,
        name: String,
        arity: usize,
    },
}

impl fmt::Display for ScriptBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScriptBindError::BadFile(file) => write!(f, "script file index {file} is absent"),
            ScriptBindError::MissingScript { file, name } => {
                write!(f, "script `{name}` is absent from file {file}")
            }
            ScriptBindError::NonzeroArity { file, name, arity } => write!(
                f,
                "step-4 script `{name}` in file {file} has arity {arity}, expected zero"
            ),
        }
    }
}

impl std::error::Error for ScriptBindError {}

/// The exact failure which stopped a script and therefore the whole tick.
#[derive(Debug)]
pub struct ScriptRunError {
    pub slot: ScriptSlot,
    pub file: usize,
    pub name: String,
    pub bytecodes_executed: u64,
    pub failure: ScriptFailure,
}

#[derive(Debug)]
pub enum ScriptFailure {
    /// A DON implementation gap or invalid compiled-program boundary.
    Vm(VmError),
    /// A runtime error the retail VM itself would raise.
    Runtime(RuntimeError),
}

impl fmt::Display for ScriptRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "step-4 {:?} script `{}` (file {}) failed after {} bytecodes: {:?}",
            self.slot, self.name, self.file, self.bytecodes_executed, self.failure
        )
    }
}

impl std::error::Error for ScriptRunError {}

/// Successful work performed by the two retail call sites in one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptFrameRun {
    pub calls: u32,
    pub bytecodes: u64,
}

/// One recovered `print` / `print_line` event. Newline identity is retained rather
/// than flattened into presentation text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptOutput {
    pub text: String,
    pub newline: bool,
}

/// Persistent compiled code plus its cross-frame statics and trigger bits.
pub struct ScriptRuntime {
    program: Program,
    game: Option<ScriptBinding>,
    general_powers: Option<ScriptBinding>,
    output: Vec<ScriptOutput>,
    calls: u64,
    bytecodes: u64,
}

impl ScriptRuntime {
    pub fn new(
        program: Program,
        game: Option<ScriptBinding>,
        general_powers: Option<ScriptBinding>,
    ) -> Result<Self, ScriptBindError> {
        if let Some(binding) = &game {
            validate_binding(&program, binding)?;
        }
        if let Some(binding) = &general_powers {
            validate_binding(&program, binding)?;
        }
        Ok(ScriptRuntime {
            program,
            game,
            general_powers,
            output: Vec::new(),
            calls: 0,
            bytecodes: 0,
        })
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn output(&self) -> &[ScriptOutput] {
        &self.output
    }

    pub fn calls(&self) -> u64 {
        self.calls
    }

    pub fn bytecodes(&self) -> u64 {
        self.bytecodes
    }

    /// Execute the two `Game::do_frame` call sites in recovered order.
    pub(crate) fn run_frame(
        &mut self,
        frame: i32,
        random: &mut Random,
    ) -> Result<ScriptFrameRun, ScriptRunError> {
        let mut work = ScriptFrameRun::default();
        if let Some(binding) = self.game.clone() {
            let bytecodes = self.run_one(ScriptSlot::Game, &binding, random)?;
            work.calls += 1;
            work.bytecodes += bytecodes;
            self.calls += 1;
            self.bytecodes += bytecodes;
        }
        if frame > 0 {
            if let Some(binding) = self.general_powers.clone() {
                let bytecodes = self.run_one(ScriptSlot::GeneralPowers, &binding, random)?;
                work.calls += 1;
                work.bytecodes += bytecodes;
                self.calls += 1;
                self.bytecodes += bytecodes;
            }
        }
        Ok(work)
    }

    fn run_one(
        &mut self,
        slot: ScriptSlot,
        binding: &ScriptBinding,
        random: &mut Random,
    ) -> Result<u64, ScriptRunError> {
        let mut host = SimScriptHost {
            random,
            output: &mut self.output,
        };
        let mut vm = Vm::new(&mut self.program, &mut host);
        let result = vm.run_script(binding.file, &binding.name);
        let bytecodes_executed = vm.bytecodes_executed;
        match result {
            Err(failure) => Err(ScriptRunError {
                slot,
                file: binding.file,
                name: binding.name.clone(),
                bytecodes_executed,
                failure: ScriptFailure::Vm(failure),
            }),
            Ok(outcome) => {
                if let Some(failure) = outcome.error {
                    return Err(ScriptRunError {
                        slot,
                        file: binding.file,
                        name: binding.name.clone(),
                        bytecodes_executed: outcome.bytecodes_executed,
                        failure: ScriptFailure::Runtime(failure),
                    });
                }
                Ok(outcome.bytecodes_executed)
            }
        }
    }
}

fn validate_binding(program: &Program, binding: &ScriptBinding) -> Result<(), ScriptBindError> {
    let file = program
        .files
        .get(binding.file)
        .ok_or(ScriptBindError::BadFile(binding.file))?;
    let index = file
        .find_script(&binding.name)
        .ok_or_else(|| ScriptBindError::MissingScript {
            file: binding.file,
            name: binding.name.clone(),
        })?;
    let arity = file.scripts[index].arity;
    if arity != 0 {
        return Err(ScriptBindError::NonzeroArity {
            file: binding.file,
            name: binding.name.clone(),
            arity,
        });
    }
    Ok(())
}

struct SimScriptHost<'a> {
    random: &'a mut Random,
    output: &'a mut Vec<ScriptOutput>,
}

impl Host for SimScriptHost<'_> {
    fn call(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
        match call_util(self, decl, args) {
            Some(result) => result,
            None => Err(HostError::Unimplemented),
        }
    }

    fn game_random(&mut self, lo: i32, hi: i32) -> Result<i32, HostError> {
        Ok(self.random.get(lo, hi))
    }

    fn game_random_step(&mut self) -> Result<u32, HostError> {
        Ok(self.random.advance() as u32)
    }

    fn game_random_seed(&self) -> Result<u32, HostError> {
        Ok(self.random.state() as u32)
    }

    fn set_game_random_seed(&mut self, seed: u32) -> Result<(), HostError> {
        self.random.reseed(seed as i32);
        Ok(())
    }

    fn script_print(&mut self, value: &str, newline: bool) -> Result<(), HostError> {
        self.output.push(ScriptOutput {
            text: value.to_string(),
            newline,
        });
        Ok(())
    }

    // `rand_seed(-1)` reads wall-clock state in retail. A deterministic headless sim
    // has no grounded substitute, so the Host default deliberately fails closed.
}
