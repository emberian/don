//! The persistent BHS runtime consumed by [`crate::tick::Sim`] at step 4.
//!
//! Retail's `Game::do_frame` calls `RunTimeEnv::run_script` twice at step 4: first the
//! selected game script every frame, then the general-powers script only when
//! `Game::frame > 0`. Both are zero-argument entry points and both run before leaders,
//! objects, and the frame increment. This module supplies that exact producer boundary
//! over [`don_bhs::Vm`].
//!
//! Supported utility builtins execute here, and their RNG operations share the main
//! simulation stream. Scenario calls require a [`ScenarioHost`]; the exact timer,
//! clock, map, age, and resource cohort recovered below performs real persistent work.
//! Every other ScenarioFuncSet entry remains fail-closed, so an incomplete host cannot
//! silently turn a script-bearing tick green.

use std::fmt;

use don_bhs::{
    call_util, BuiltinDecl, Host, HostError, HostResult, Program, RuntimeError, Value, Vm, VmError,
};

use crate::systems::{economy, leaders};
use crate::tick::Sim;

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

/// The mandatory simulation surface for a step-4 script run.
///
/// Retail routes utility RNG calls and every `ScenarioFuncSet` handler through the live
/// `Game`/`World` singletons. Keeping those operations on one required host prevents a
/// caller from executing bytecode against a detached clock or private random stream.
pub trait ScenarioHost {
    fn script_frame(&self) -> i32;
    fn game_seconds(&self) -> i32;
    fn map_size(&self) -> i32;
    fn call_scenario(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult;
    fn game_random(&mut self, lo: i32, hi: i32) -> Result<i32, HostError>;
    fn game_random_step(&mut self) -> Result<u32, HostError>;
    fn game_random_seed(&self) -> Result<u32, HostError>;
    fn set_game_random_seed(&mut self, seed: u32) -> Result<(), HostError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScriptTimer {
    name: String,
    expires_at: i32,
}

/// `ScenarioData::timers`, a `ScriptTimers : LinkList<String,int>` with at most 100
/// entries. `ordered_insert` sorts by the expiry value and inserts before equal values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ScriptTimers {
    entries: Vec<ScriptTimer>,
}

impl ScriptTimers {
    fn position(&self, name: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|timer| timer.name.eq_ignore_ascii_case(name))
    }

    /// `ScenarioFuncSet::set_timer` `0x009e4bc0` ->
    /// `ScriptTimers::add_timer` `0x00a049e0`.
    fn set(&mut self, name: &str, duration: i32, now: i32) -> Result<i32, HostError> {
        if !name.is_ascii() {
            // Retail uses locale-sensitive `_wcsicmp`. The shipped reachable timer IDs
            // are ASCII; do not invent a Unicode case-fold for the unresolved domain.
            return Err(HostError::Unimplemented);
        }
        if duration <= 0 || name.is_empty() {
            return Ok(-1);
        }
        // The count gate precedes the seek/remove sequence in the shipped function, so
        // even replacement fails when the list already contains 100 timers.
        if self.entries.len() >= 100 {
            return Ok(-1);
        }
        if let Some(index) = self.position(name) {
            self.entries.remove(index);
        }
        let expires_at = now.wrapping_add(duration);
        let insert_at = self
            .entries
            .iter()
            .position(|timer| timer.expires_at >= expires_at)
            .unwrap_or(self.entries.len());
        self.entries.insert(
            insert_at,
            ScriptTimer {
                name: name.to_string(),
                expires_at,
            },
        );
        Ok(1)
    }

    /// `ScriptTimers::remove_timer` `0x00a04b20`.
    fn stop(&mut self, name: &str) -> Result<i32, HostError> {
        if !name.is_ascii() {
            return Err(HostError::Unimplemented);
        }
        let Some(index) = self.position(name) else {
            return Ok(-1);
        };
        self.entries.remove(index);
        Ok(1)
    }

    /// `ScriptTimers::check` `0x00a04b80`. An expired timer is removed before the
    /// function returns 1; a live timer remains and returns 0; absence returns -1.
    fn expired(&mut self, name: &str, now: i32) -> Result<i32, HostError> {
        if !name.is_ascii() {
            return Err(HostError::Unimplemented);
        }
        let Some(index) = self.position(name) else {
            return Ok(-1);
        };
        if self.entries[index].expires_at <= now {
            self.entries.remove(index);
            Ok(1)
        } else {
            Ok(0)
        }
    }
}

/// Persistent compiled code plus its cross-frame statics and trigger bits.
pub struct ScriptRuntime {
    program: Program,
    game: Option<ScriptBinding>,
    general_powers: Option<ScriptBinding>,
    output: Vec<ScriptOutput>,
    timers: ScriptTimers,
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
            timers: ScriptTimers::default(),
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
    pub(crate) fn run_frame<H: ScenarioHost>(
        &mut self,
        host: &mut H,
    ) -> Result<ScriptFrameRun, ScriptRunError> {
        let frame = host.script_frame();
        let mut work = ScriptFrameRun::default();
        if let Some(binding) = self.game.clone() {
            let bytecodes = self.run_one(ScriptSlot::Game, &binding, host)?;
            work.calls += 1;
            work.bytecodes += bytecodes;
            self.calls += 1;
            self.bytecodes += bytecodes;
        }
        if frame > 0 {
            if let Some(binding) = self.general_powers.clone() {
                let bytecodes = self.run_one(ScriptSlot::GeneralPowers, &binding, host)?;
                work.calls += 1;
                work.bytecodes += bytecodes;
                self.calls += 1;
                self.bytecodes += bytecodes;
            }
        }
        Ok(work)
    }

    fn run_one<H: ScenarioHost>(
        &mut self,
        slot: ScriptSlot,
        binding: &ScriptBinding,
        scenario: &mut H,
    ) -> Result<u64, ScriptRunError> {
        let mut host = SimScriptHost {
            scenario,
            timers: &mut self.timers,
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

struct SimScriptHost<'a, H> {
    scenario: &'a mut H,
    timers: &'a mut ScriptTimers,
    output: &'a mut Vec<ScriptOutput>,
}

fn string_arg(args: &[Value], index: usize) -> Result<&str, HostError> {
    match args.get(index) {
        Some(Value::Str(value)) => Ok(value.as_str()),
        _ => Err(HostError::BadArgs(
            "scenario string argument has wrong type",
        )),
    }
}

impl<H: ScenarioHost> Host for SimScriptHost<'_, H> {
    fn call(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
        match call_util(self, decl, args) {
            Some(result) => result,
            None => match decl.index {
                // `ScenarioFuncSet::{set,stop}_timer/timer_expired`, indices 77..79.
                77 => Ok(Value::Int(self.timers.set(
                    string_arg(args, 0)?,
                    args[1].as_int(),
                    self.scenario.game_seconds(),
                )?)),
                78 => Ok(Value::Int(self.timers.stop(string_arg(args, 0)?)?)),
                79 => Ok(Value::Int(
                    self.timers
                        .expired(string_arg(args, 0)?, self.scenario.game_seconds())?,
                )),
                // `get_map_size` `0x009e4cb0`: `WorldData::xs << 2`.
                80 => Ok(Value::Int(self.scenario.map_size())),
                // `time_later_than` `0x009ee120`: Game::seconds / 60 >= argument.
                351 => Ok(Value::Int(
                    (self.scenario.game_seconds() / 60 >= args[0].as_int()) as i32,
                )),
                _ => self.scenario.call_scenario(decl, args),
            },
        }
    }

    fn game_random(&mut self, lo: i32, hi: i32) -> Result<i32, HostError> {
        self.scenario.game_random(lo, hi)
    }

    fn game_random_step(&mut self) -> Result<u32, HostError> {
        self.scenario.game_random_step()
    }

    fn game_random_seed(&self) -> Result<u32, HostError> {
        self.scenario.game_random_seed()
    }

    fn set_game_random_seed(&mut self, seed: u32) -> Result<(), HostError> {
        self.scenario.set_game_random_seed(seed)
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

fn resource_index(name: &str) -> Result<Option<usize>, HostError> {
    // Type indices 0..5 are fixed by the PDB enum and the first six shipped
    // `resourcerules.xml` entries. `String::operator==` `0x00a1f140` compares names
    // case-insensitively, hence the ASCII-insensitive spelling match here.
    if !name.is_ascii() {
        return Err(HostError::Unimplemented);
    }
    const NAMES: [&str; economy::NUM_RESOURCES] =
        ["Food", "Timber", "Wealth", "Knowledge", "Metal", "Oil"];
    Ok(NAMES
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name)))
}

fn leader_resource_args(args: &[Value]) -> Result<(usize, Option<usize>, i32), HostError> {
    let who = args[0].as_int().wrapping_sub(1) as u32;
    let resource = resource_index(string_arg(args, 1)?)?;
    Ok((who as usize, resource, args[2].as_int()))
}

impl ScenarioHost for Sim {
    fn script_frame(&self) -> i32 {
        self.world.frame
    }

    fn game_seconds(&self) -> i32 {
        self.world.seconds
    }

    fn map_size(&self) -> i32 {
        self.map.world.xs.wrapping_shl(2)
    }

    fn call_scenario(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
        match decl.index {
            // `map_is_land` `0x009e4d90`: bounds against tile dimensions, then
            // `(low_byte(tdata[y * tile_xs + x]) & 0x30) != 0x20`.
            83 => {
                let x = args[0].as_int();
                let y = args[1].as_int();
                if x < 0 || y < 0 || x >= self.map.world.tile_xs || y >= self.map.world.tile_ys {
                    return Ok(Value::Int(-1));
                }
                let index = (y * self.map.world.tile_xs + x) as usize;
                Ok(Value::Int(
                    ((self.map.world.tdata[index] as u8 & 0x30) != 0x20) as i32,
                ))
            }
            // `num_players` `0x009e5df0`: count `Leader::flags & 1` across all slots.
            142 => Ok(Value::Int(
                self.step8
                    .leaders
                    .iter()
                    .filter(|leader| leader.flags & leaders::flag::IN_GAME != 0)
                    .count() as i32,
            )),
            // `age` `0x009e8f50`: both Leader flags are required, then the alternate
            // encrypted age slot at econ +0xdc is returned.
            248 => {
                let who = args[0].as_int().wrapping_sub(1) as u32;
                let Some(flags) = self
                    .step8
                    .leaders
                    .get(who as usize)
                    .map(|leader| leader.flags)
                else {
                    return Ok(Value::Int(-1));
                };
                if flags & (leaders::flag::IN_GAME | leaders::flag::PROCESS)
                    != (leaders::flag::IN_GAME | leaders::flag::PROCESS)
                {
                    return Ok(Value::Int(-1));
                }
                let leader = &self.leaders[who as usize];
                Ok(Value::Int(leader.econ.age_alt))
            }
            // `is_defeated` `0x009e9070`: active slot, then bit 6 of the low flags byte.
            252 => {
                let who = args[0].as_int().wrapping_sub(1) as u32;
                let Some(flags) = self
                    .step8
                    .leaders
                    .get(who as usize)
                    .map(|leader| leader.flags)
                else {
                    return Ok(Value::Int(-1));
                };
                if flags & leaders::flag::IN_GAME == 0 {
                    return Ok(Value::Int(-1));
                }
                Ok(Value::Int(((flags >> 6) & 1) as i32))
            }
            // `give_good` `0x009fb590`: leader active, resource type index < 6,
            // wrapping add to the decoded stockpile.
            661 => {
                let (who, resource, amount) = leader_resource_args(args)?;
                let Some(leader) = self.leaders.get_mut(who) else {
                    return Ok(Value::Int(-1));
                };
                let Some(resource) = resource else {
                    return Ok(Value::Int(-1));
                };
                if !leader.active {
                    return Ok(Value::Int(-1));
                }
                leader.econ.stockpile[resource] =
                    leader.econ.stockpile[resource].wrapping_add(amount);
                Ok(Value::Int(1))
            }
            // `set_good` `0x009fb6f0`: the same gates plus a non-negative value.
            663 => {
                let (who, resource, amount) = leader_resource_args(args)?;
                let Some(leader) = self.leaders.get_mut(who) else {
                    return Ok(Value::Int(-1));
                };
                let Some(resource) = resource else {
                    return Ok(Value::Int(-1));
                };
                if !leader.active || amount < 0 {
                    return Ok(Value::Int(-1));
                }
                leader.econ.stockpile[resource] = amount;
                Ok(Value::Int(1))
            }
            // `set_base_rate` `0x009fbb80`: both Leader flags, then `num << 4` at
            // LeaderData +0x4b0. `DoGatherContext::extra_income` owns that exact term.
            669 => {
                let (who, resource, amount) = leader_resource_args(args)?;
                let Some(leader) = self.leaders.get_mut(who) else {
                    return Ok(Value::Int(-1));
                };
                let Some(resource) = resource else {
                    return Ok(Value::Int(-1));
                };
                let flags = self.step8.leaders[who].flags;
                if flags & (leaders::flag::IN_GAME | leaders::flag::PROCESS)
                    != (leaders::flag::IN_GAME | leaders::flag::PROCESS)
                {
                    return Ok(Value::Int(-1));
                }
                leader.gather_ctx.extra_income[resource] = amount.wrapping_shl(4);
                Ok(Value::Int(1))
            }
            _ => Err(HostError::Unimplemented),
        }
    }

    fn game_random(&mut self, lo: i32, hi: i32) -> Result<i32, HostError> {
        Ok(self.world.random.get(lo, hi))
    }

    fn game_random_step(&mut self) -> Result<u32, HostError> {
        Ok(self.world.random.advance() as u32)
    }

    fn game_random_seed(&self) -> Result<u32, HostError> {
        Ok(self.world.random.state() as u32)
    }

    fn set_game_random_seed(&mut self, seed: u32) -> Result<(), HostError> {
        self.world.random.reseed(seed as i32);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptTimers;

    #[test]
    fn timer_capacity_gate_precedes_replacement_and_expiry_consumes() {
        let mut timers = ScriptTimers::default();
        for i in 0..100 {
            assert_eq!(timers.set(&format!("timer-{i}"), 100 - i, 0).unwrap(), 1);
        }
        assert!(
            timers
                .entries
                .windows(2)
                .all(|pair| pair[0].expires_at <= pair[1].expires_at),
            "ordered_insert must retain expiry ordering"
        );

        // Retail checks count == 100 before seeking and removing a duplicate.
        assert_eq!(timers.set("TIMER-0", 1, 0).unwrap(), -1);
        assert_eq!(timers.expired("timer-0", 99).unwrap(), 0);
        assert_eq!(timers.expired("TIMER-0", 100).unwrap(), 1);
        assert_eq!(timers.expired("timer-0", 100).unwrap(), -1);

        // Once expiry consumed the old entry, replacement has capacity again.
        assert_eq!(timers.set("timer-0", 1, 100).unwrap(), 1);
        assert_eq!(timers.expired("timer-0", 101).unwrap(), 1);
    }
}
