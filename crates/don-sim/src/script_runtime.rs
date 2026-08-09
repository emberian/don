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

use crate::objects::{Band, BUILD_BAND_BASE, WALL_BAND_BASE};
use crate::systems::{economy, leaders, order_dispatch, production, victory_score};
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
    fn map_tile_width(&self) -> i32;
    fn map_tile_height(&self) -> i32;
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
                // `world_{x,y}_size` `0x009e4ee0` / `0x009e4ef0`: direct reads of
                // `WorldData::tile_xs` / `tile_ys` at +0x18 / +0x1c.
                86 => Ok(Value::Int(self.scenario.map_tile_width())),
                87 => Ok(Value::Int(self.scenario.map_tile_height())),
                // `time` and `time_min` are instruction-identical signed divisions of
                // `Game::seconds` by 60. `time_sec` returns that field unmodified.
                296 | 297 => Ok(Value::Int(self.scenario.game_seconds() / 60)),
                298 => Ok(Value::Int(self.scenario.game_seconds())),
                // `time_later_than` `0x009ee120`: Game::seconds / 60 >= argument.
                351 => Ok(Value::Int(
                    (self.scenario.game_seconds() / 60 >= args[0].as_int()) as i32,
                )),
                // `time_earlier_than` `0x009ee160`: the complementary strict compare.
                352 => Ok(Value::Int(
                    (self.scenario.game_seconds() / 60 < args[0].as_int()) as i32,
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

/// A resolved entry in retail's `(who, o)` object table.
///
/// `ObjectRegistry` carries the same owner-local band identity while the simulation's
/// actual records live in their respective dense stores. Keeping the kind and row
/// together prevents a script lookup from accidentally treating a band offset as a
/// dense row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScriptObject {
    Unit { who: usize, o: i32, row: usize },
    Build { who: usize, o: i32, row: usize },
    Wall { who: usize, o: i32, row: usize },
}

const SCRIPT_NO_CITY_DEFEAT: i32 = 0x01;
const SCRIPT_UNIT_AI_OFF: i32 = 0x02;
const SCRIPT_PRODUCTION_AI_OFF: i32 = 0x04;
const SCRIPT_COMBAT_AI_OFF: i32 = 0x08;
const SCRIPT_NO_EXPANSION: i32 = 0x10;
const SCRIPT_UNIT_MASK_AI_OFF: u32 = 0x0100_0000;

impl Sim {
    /// The `flags & 1` Leader gate used by retail reads that remain valid while the
    /// slot is in-game but temporarily not processing.
    fn in_game_script_leader(&self, who: i32) -> Option<usize> {
        let who = who.wrapping_sub(1) as u32 as usize;
        let flags = self.step8.leaders.get(who)?.flags;
        (flags & leaders::flag::IN_GAME != 0).then_some(who)
    }

    /// The `(flags & 3) == 3` leader gate shared by the retail player-state readers.
    fn active_script_leader(&self, who: i32) -> Option<usize> {
        let who = self.in_game_script_leader(who)?;
        (self.step8.leaders[who].flags & leaders::flag::PROCESS != 0).then_some(who)
    }

    /// `ScenarioFuncSet::set_explored(who,x,y,radius)` `0x009e44b0`.
    ///
    /// The rectangular walk is asymmetric in retail: x includes its clipped upper
    /// bound while y excludes it. Each surviving tile is then filtered through a
    /// strict f32 Euclidean-radius comparison and converted to FCoord by `>> 1`.
    fn script_set_explored(&mut self, who: i32, x: i32, y: i32, radius: i32) -> i32 {
        let who = who.wrapping_sub(1);
        let tile_xs = self.map.world.tile_xs;
        let tile_ys = self.map.world.tile_ys;
        if x < 0 || y < 0 || x >= tile_xs || y >= tile_ys || radius < 0 {
            return -1;
        }
        if who >= 0 {
            let Some(flags) = self
                .step8
                .leaders
                .get(who as usize)
                .map(|leader| leader.flags)
            else {
                return -1;
            };
            if flags & (leaders::flag::IN_GAME | leaders::flag::PROCESS)
                != (leaders::flag::IN_GAME | leaders::flag::PROCESS)
            {
                return -1;
            }
        }

        let x_start = x.wrapping_sub(radius).max(0);
        let y_start = y.wrapping_sub(radius).max(0);
        let x_sum = x.wrapping_add(radius);
        let y_sum = y.wrapping_add(radius);
        let x_end = if x_sum >= tile_xs {
            tile_xs.wrapping_sub(1)
        } else {
            x_sum
        };
        let y_end = if y_sum >= tile_ys {
            tile_ys.wrapping_sub(1)
        } else {
            y_sum
        };
        let radius = radius as f32;

        let mut tile_x = x_start;
        while tile_x <= x_end {
            let mut tile_y = y_start;
            while tile_y < y_end {
                let dx = tile_x.wrapping_sub(x) as f32;
                let dy = tile_y.wrapping_sub(y) as f32;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance < radius {
                    let fog_x = tile_x >> 1;
                    let fog_y = tile_y >> 1;
                    if who < 0 {
                        // The all-player branch uses the weaker one-bit in-game gate and
                        // stamps player slots in ascending order for every selected tile.
                        for player in 0..self.map.fog.leaders.len() {
                            if self.step8.leaders[player].flags & leaders::flag::IN_GAME != 0 {
                                let map = &mut self.map;
                                map.fog
                                    .set_seen(&mut map.world, fog_x, fog_y, player as i32, true);
                            }
                        }
                    } else {
                        let map = &mut self.map;
                        map.fog.set_seen(&mut map.world, fog_x, fog_y, who, true);
                    }
                }
                tile_y += 1;
            }
            tile_x += 1;
        }
        1
    }

    /// The simulation-visible body shared by `set_explored(who)` `0x009e46f0` and
    /// `show_all_map_{enable,disable}` `0x009e4a30` / `0x009e4a90`. Their remaining
    /// writes only invalidate presentation caches.
    fn script_set_show_all(&mut self, who: i32, enabled: bool) -> i32 {
        let Some(who) = self.active_script_leader(who) else {
            return -1;
        };
        self.map.fog.leaders[who].see_all = enabled;
        1
    }

    /// The ten direct `LeaderData::leader_flags2` scenario toggles at
    /// `0x009ff5e0..0x009ffcba`. `enabled` clears the named "off" bit and
    /// `disabled` sets it. The three gate shapes are retained explicitly: the broad
    /// AI pairs need only VALID, city AI also needs ACTIVE and rejects HUMAN, while
    /// city defeat needs VALID|ACTIVE but remains legal for humans.
    fn script_set_leader_policy(
        &mut self,
        who: i32,
        bit: i32,
        enabled: bool,
        require_active: bool,
        reject_human: bool,
    ) -> i32 {
        let who = if require_active {
            self.active_script_leader(who)
        } else {
            self.in_game_script_leader(who)
        };
        let Some(who) = who else {
            return -1;
        };
        let Some(leader) = self.vic_leaders.slots.get_mut(who) else {
            return -1;
        };
        if reject_human && leader.leader_flags & victory_score::leader_flag::HUMAN != 0 {
            return -1;
        }
        if enabled {
            leader.leader_flags2 &= !bit;
        } else {
            leader.leader_flags2 |= bit;
        }
        1
    }

    /// `ScenarioFuncSet::{enable,disable}_unit_ai` (`0x009ff920` / `0x009ffa10`).
    ///
    /// A live addressed unit redirects to its captain and mutates every member in
    /// captain-to-`o_down` order. A negative object id is a distinct retail sentinel:
    /// it accepts the weaker one-bit Leader gate and toggles the all-unit policy bit.
    /// Formation links are preflighted before the first write so absent host facts fail
    /// closed instead of leaving a partially mutated chain.
    fn script_set_unit_ai(&mut self, who: i32, o: i32, enabled: bool) -> Result<i32, HostError> {
        let slot = who.wrapping_sub(1) as u32 as usize;
        let active_unit = self.step8.leaders.get(slot).and_then(|leader| {
            (leader.flags & (leaders::flag::IN_GAME | leaders::flag::PROCESS)
                == (leaders::flag::IN_GAME | leaders::flag::PROCESS))
                .then(|| self.script_object(slot, o))
                .flatten()
        });
        let active_unit = active_unit.and_then(|object| match object {
            object @ ScriptObject::Unit { row, .. }
                if self.world.units.get_flags(row) & 1 != 0
                    || self.world.units.o_up()[row] >= 0 =>
            {
                Some(object)
            }
            _ => None,
        });

        if let Some(object) = active_unit {
            let captain = self.script_captain(object)?;
            let ScriptObject::Unit {
                who: captain_who,
                row: captain_row,
                ..
            } = captain
            else {
                return Err(HostError::Unimplemented);
            };
            if captain_who != slot {
                return Err(HostError::Unimplemented);
            }

            let limit = self.world.objects.total_objects().saturating_add(1);
            let mut rows = Vec::new();
            let mut row = captain_row;
            let mut terminated = false;
            for _ in 0..limit {
                if rows.contains(&row) {
                    return Err(HostError::Unimplemented);
                }
                rows.push(row);
                let down = self.world.units.o_down()[row] as i32;
                if down < 0 {
                    terminated = true;
                    break;
                }
                let Some(ScriptObject::Unit {
                    who: next_who,
                    row: next_row,
                    ..
                }) = self.script_object(slot, down)
                else {
                    return Err(HostError::Unimplemented);
                };
                if next_who != slot {
                    return Err(HostError::Unimplemented);
                }
                row = next_row;
            }
            if !terminated {
                return Err(HostError::Unimplemented);
            }

            for row in rows {
                let masks = self.world.units.get_unit_masks(row);
                self.world.units.set_unit_masks(
                    row,
                    if enabled {
                        masks & !SCRIPT_UNIT_MASK_AI_OFF
                    } else {
                        masks | SCRIPT_UNIT_MASK_AI_OFF
                    },
                );
            }
            return Ok(1);
        }

        let Some(slot) = self.in_game_script_leader(who) else {
            return Ok(-1);
        };
        if o >= 0 {
            return Ok(-1);
        }
        if enabled {
            self.vic_leaders.slots[slot].leader_flags2 &= !SCRIPT_UNIT_AI_OFF;
        } else {
            self.vic_leaders.slots[slot].leader_flags2 |= SCRIPT_UNIT_AI_OFF;
        }
        Ok(1)
    }

    fn script_object(&self, who: usize, o: i32) -> Option<ScriptObject> {
        if who >= crate::objects::OWNER_SLOTS || o < 0 {
            return None;
        }
        let slot = self.world.objects.slot(who);
        if o < BUILD_BAND_BASE as i32 {
            let row = *slot.band(Band::Unit).get(o as usize)? as usize;
            (row < self.world.units.len()).then_some(ScriptObject::Unit { who, o, row })
        } else if o < WALL_BAND_BASE as i32 {
            let index = o.checked_sub(BUILD_BAND_BASE as i32)? as usize;
            let row = *slot.band(Band::Build).get(index)? as usize;
            (row < self.builds.len()).then_some(ScriptObject::Build { who, o, row })
        } else {
            let index = o.checked_sub(WALL_BAND_BASE as i32)? as usize;
            let row = *slot.band(Band::Wall).get(index)? as usize;
            (row < self.walls.len()).then_some(ScriptObject::Wall { who, o, row })
        }
    }

    /// The `active_unit_slot` / `valid_build_o` gate shared by the two retail handlers.
    fn valid_script_position_object(&self, who: usize, o: i32) -> Option<ScriptObject> {
        let flags = self.step8.leaders.get(who)?.flags;
        if flags & (leaders::flag::IN_GAME | leaders::flag::PROCESS)
            != (leaders::flag::IN_GAME | leaders::flag::PROCESS)
        {
            return None;
        }
        match self.script_object(who, o)? {
            object @ ScriptObject::Unit { row, .. }
                if self.world.units.get_flags(row) & 1 != 0
                    || self.world.units.o_up()[row] >= 0 =>
            {
                Some(object)
            }
            object @ ScriptObject::Build { row, .. } if self.builds[row].is_valid() => Some(object),
            _ => None,
        }
    }

    /// `ScenarioFuncSet::valid_object_o` `0x009e32a0`: both Leader bits, the
    /// owner-local object band below 3000, a non-null object slot, then `flags & 1`.
    /// Unlike the position readers' `active_unit_slot`, this gate does not preserve
    /// an inactive formation member solely because it has an `o_up` captain.
    fn valid_script_object(&self, who: usize, o: i32) -> Option<ScriptObject> {
        let flags = self.step8.leaders.get(who)?.flags;
        if flags & (leaders::flag::IN_GAME | leaders::flag::PROCESS)
            != (leaders::flag::IN_GAME | leaders::flag::PROCESS)
            || !(0..WALL_BAND_BASE as i32).contains(&o)
        {
            return None;
        }
        match self.script_object(who, o)? {
            object @ ScriptObject::Unit { row, .. } if self.world.units.get_flags(row) & 1 != 0 => {
                Some(object)
            }
            object @ ScriptObject::Build { row, .. } if self.builds[row].is_valid() => Some(object),
            _ => None,
        }
    }

    /// `UnitData::get_captain` `0x00610ab0`. Non-unit objects inherit
    /// `ObjectData::get_captain` and return their own object index.
    fn script_captain(&self, mut object: ScriptObject) -> Result<ScriptObject, HostError> {
        let limit = self.world.objects.total_objects().saturating_add(1);
        for _ in 0..limit {
            let ScriptObject::Unit { who, row, .. } = object else {
                return Ok(object);
            };
            let captain = self.world.units.o_up()[row] as i32;
            if captain < 0 {
                return Ok(object);
            }
            let next = self
                .script_object(who, captain)
                .ok_or(HostError::Unimplemented)?;
            if next == object {
                return Err(HostError::Unimplemented);
            }
            object = next;
        }
        Err(HostError::Unimplemented)
    }

    /// `ObjectData::get_inside` `0x00651a80`: follow `inside_up` while the container is
    /// itself a unit, stopping on the outermost unit or the first non-unit object.
    fn script_container(&self, object: ScriptObject) -> Result<Option<ScriptObject>, HostError> {
        let ScriptObject::Unit { row, .. } = object else {
            return Ok(None);
        };
        let inside = self.world.units.inside_up()[row] as i32;
        if inside < 0 {
            return Ok(None);
        }
        let inside_who = self.world.units.inside_up_who()[row] as u8 as usize;
        let mut container = self
            .script_object(inside_who, inside)
            .ok_or(HostError::Unimplemented)?;
        let limit = self.world.objects.total_objects().saturating_add(1);
        for _ in 0..limit {
            let ScriptObject::Unit { row, .. } = container else {
                return Ok(Some(container));
            };
            let next_o = self.world.units.inside_up()[row] as i32;
            if next_o < 0 {
                return Ok(Some(container));
            }
            let next_who = self.world.units.inside_up_who()[row] as u8 as usize;
            let next = self
                .script_object(next_who, next_o)
                .ok_or(HostError::Unimplemented)?;
            if next == container {
                return Err(HostError::Unimplemented);
            }
            container = next;
        }
        Err(HostError::Unimplemented)
    }

    fn script_outer_container(&self, object: ScriptObject) -> Result<ScriptObject, HostError> {
        Ok(self.script_container(object)?.unwrap_or(object))
    }

    /// The `BuildData::hits(0)` virtual used by both health readers. Ordinary construction
    /// exposes `construct_hits`; the two razing pseudo-items scale that value by queue
    /// progress. Their total train time is not present in `BuildData`, so the installed
    /// production type row is mandatory whenever that branch is live.
    fn script_build_max_health(&self, row: usize) -> Result<i32, HostError> {
        let build = self.builds.get(row).ok_or(HostError::Unimplemented)?;
        let razing = if build.is_active() && build.queue.queued != 0 {
            match build.queue.entries.first() {
                Some(entry) if matches!(i32::from(entry.type_index), 0x29a | 0x286) => {
                    let type_index =
                        usize::try_from(entry.type_index).map_err(|_| HostError::Unimplemented)?;
                    let facts = self
                        .production_runtime
                        .types
                        .get(type_index)
                        .and_then(Option::as_ref)
                        .filter(|facts| facts.type_index == i32::from(entry.type_index))
                        .ok_or(HostError::Unimplemented)?;
                    if facts.train_time <= 0 {
                        return Err(HostError::Unimplemented);
                    }
                    Some((entry.elapsed, facts.train_time))
                }
                _ => None,
            }
        } else {
            None
        };
        Ok(production::build_hits(
            false,
            build.myhits,
            build.construct_hits,
            razing,
        ))
    }

    /// The virtual `ObjectData::hits(0)` call shared by `object_health` and
    /// `object_max_health`.
    fn script_object_max_health(&self, object: ScriptObject) -> Result<i32, HostError> {
        match object {
            ScriptObject::Unit { row, .. } => self
                .world
                .units
                .myhits()
                .get(row)
                .copied()
                .ok_or(HostError::Unimplemented),
            ScriptObject::Build { row, .. } => self.script_build_max_health(row),
            // `valid_object_o` rejects the 3000 wall band before either handler reaches
            // its virtual call. Retain a fail-closed arm for corrupt internal callers.
            ScriptObject::Wall { .. } => Err(HostError::Unimplemented),
        }
    }

    /// `UnitData::total_damage(0, nullptr)` for the ordinary `UnitData` layout represented
    /// by the generated columns. It walks the live `o_down` formation chain, accumulates
    /// whole and sixteenth damage, then charges absent squad members their share of the
    /// captain's maximum health.
    fn script_unit_total_damage(&self, object: ScriptObject) -> Result<i32, HostError> {
        let ScriptObject::Unit {
            who,
            row: captain_row,
            ..
        } = object
        else {
            return Err(HostError::Unimplemented);
        };
        if self.world.units.get_who(captain_row) as usize != who
            || self.world.units.o_up()[captain_row] >= 0
        {
            // The caller must have applied UnitData::get_captain first.
            return Err(HostError::Unimplemented);
        }

        let type_id = *self
            .unit_type
            .get(captain_row)
            .ok_or(HostError::Unimplemented)?;
        let uber_size = self
            .shooter_rules
            .iter()
            .find(|(candidate, _)| *candidate == type_id)
            .map(|(_, rules)| rules.uber_size)
            .filter(|&size| size > 0)
            .ok_or(HostError::Unimplemented)?;

        let mut row = captain_row;
        let mut current_size = 0usize;
        let mut damage = 0i32;
        let mut damage_frac = 0i32;
        let mut terminated = false;
        let limit = self.world.objects.total_objects().saturating_add(1);
        for _ in 0..limit {
            current_size += 1;
            damage = damage.wrapping_add(self.world.units.damage()[row]);
            damage_frac = damage_frac.wrapping_add(i32::from(self.world.units.damage_frac()[row]));

            let down = self.world.units.o_down()[row] as i32;
            if down < 0 {
                terminated = true;
                break;
            }
            let next = self
                .script_object(who, down)
                .ok_or(HostError::Unimplemented)?;
            let ScriptObject::Unit { row: next_row, .. } = next else {
                return Err(HostError::Unimplemented);
            };
            if self.world.units.get_who(next_row) as usize != who {
                return Err(HostError::Unimplemented);
            }
            if self.world.units.get_flags(next_row) & 1 == 0 {
                terminated = true;
                break;
            }
            row = next_row;
        }
        if !terminated || current_size > uber_size as usize {
            return Err(HostError::Unimplemented);
        }

        let max = self.script_object_max_health(object)?;
        let absent = uber_size.wrapping_sub(current_size as i32);
        let absent_damage = absent
            .wrapping_mul(max)
            .checked_div(uber_size)
            .ok_or(HostError::Unimplemented)?;
        damage = damage.wrapping_add(absent_damage);
        if damage_frac >= 16 {
            damage = damage.wrapping_add((damage_frac as u32 >> 4) as i32);
        }
        Ok(damage)
    }

    /// `ObjectData::hits_left()` over the dynamic `hits(0)` answer.
    fn script_nonunit_hits_left(&self, object: ScriptObject) -> Result<i32, HostError> {
        let max = self.script_object_max_health(object)?;
        let damage = match object {
            ScriptObject::Build { row, .. } => self
                .builds
                .get(row)
                .map(|build| build.damage)
                .ok_or(HostError::Unimplemented)?,
            ScriptObject::Wall { row, .. } => self
                .walls
                .get(row)
                .map(|wall| wall.damage)
                .ok_or(HostError::Unimplemented)?,
            ScriptObject::Unit { .. } => return Err(HostError::Unimplemented),
        };
        let left = max.wrapping_sub(damage);
        Ok(if left < 0 || max < 0 {
            0
        } else {
            left.min(max)
        })
    }

    /// SSE `cvttss2si`: unlike Rust's saturating float cast, NaN and every out-of-range
    /// input produce the integer-indefinite value `0x80000000`.
    fn script_cvttss2si(value: f32) -> i32 {
        if value.is_nan() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
            i32::MIN
        } else {
            value.trunc() as i32
        }
    }

    /// `ScenarioFuncSet::object_health` `0x009f5910`.
    fn script_object_health(&self, who: i32, o: i32) -> Result<i32, HostError> {
        let who = who.wrapping_sub(1) as u32 as usize;
        let Some(object) = self.valid_script_object(who, o) else {
            return Ok(-1);
        };
        let object = self.script_captain(object)?;
        let max = self.script_object_max_health(object)?;
        let current = match object {
            ScriptObject::Unit { .. } => max.wrapping_sub(self.script_unit_total_damage(object)?),
            ScriptObject::Build { .. } | ScriptObject::Wall { .. } => {
                self.script_nonunit_hits_left(object)?
            }
        };
        let ratio = current as f32 / max as f32;
        Ok(Self::script_cvttss2si(ratio * 100.0))
    }

    /// `ScenarioFuncSet::object_max_health` `0x009f5f60`. Unlike `object_health`, this
    /// handler does not resolve a unit's captain before calling `hits(0)`.
    fn script_object_max_health_read(&self, who: i32, o: i32) -> Result<i32, HostError> {
        let who = who.wrapping_sub(1) as u32 as usize;
        let Some(object) = self.valid_script_object(who, o) else {
            return Ok(-1);
        };
        self.script_object_max_health(object)
    }

    /// The point used by the addressed-object proximity readers. These handlers call
    /// `UnitData::get_captain`, but unlike `object_position_{x,y}` they do not redirect
    /// through `ObjectData::get_inside` before reading the encrypted coordinates.
    fn script_object_tile_point(&self, object: ScriptObject) -> Result<(i32, i32), HostError> {
        let object = self.script_captain(object)?;
        let (x, y) = match object {
            ScriptObject::Unit { row, .. } => (
                self.world.units.x_internal()[row],
                self.world.units.y_internal()[row],
            ),
            ScriptObject::Build { row, .. } => self.builds[row].position(),
            // Both callers apply `valid_object_o`, whose owner-local domain stops
            // before the wall band at 3000.
            ScriptObject::Wall { .. } => return Err(HostError::Unimplemented),
        };
        if x < 0 || y < 0 {
            // Retail indexes `div_3_table` after an arithmetic shift. Negative host
            // coordinates fall outside that recovered finite lookup domain.
            return Err(HostError::Unimplemented);
        }
        Ok((
            x / production::COORD_PER_TILE,
            y / production::COORD_PER_TILE,
        ))
    }

    /// Integer point distance at `0x0046cff0`. This is the engine's fast approximation:
    /// `major + minor^2 / (2*major)`, switching to `(minor + 2*major) / 2` before the
    /// square can overflow. Its callers compare the result strictly below the radius.
    fn script_point_distance(dx: i32, dy: i32) -> Result<i32, HostError> {
        let dx = dx.checked_abs().ok_or(HostError::Unimplemented)?;
        let dy = dy.checked_abs().ok_or(HostError::Unimplemented)?;
        let (major, minor) = if dx >= dy { (dx, dy) } else { (dy, dx) };
        if major == 0 {
            return Ok(0);
        }
        if minor >= 60_000 {
            let distance = (minor as u32).wrapping_add((major as u32).wrapping_mul(2)) >> 1;
            return Ok(distance as i32);
        }
        let correction = (minor as u32).wrapping_mul(minor as u32) / (major as u32).wrapping_mul(2);
        Ok(correction.wrapping_add(major as u32) as i32)
    }

    /// `ScenarioFuncSet::is_object_at` `0x009f0b70`.
    fn script_is_object_at(&self, who: i32, o: i32, x: i32, y: i32) -> Result<i32, HostError> {
        let who = who.wrapping_sub(1) as u32 as usize;
        let Some(object) = self.valid_script_object(who, o) else {
            return Ok(-1);
        };
        let point = self.script_object_tile_point(object)?;
        Ok((point == (x, y)) as i32)
    }

    /// `ScenarioFuncSet::object_near` `0x009f0c30`.
    fn script_object_near(
        &self,
        who: i32,
        o: i32,
        x: i32,
        y: i32,
        radius: i32,
    ) -> Result<i32, HostError> {
        let who = who.wrapping_sub(1) as u32 as usize;
        let Some(object) = self.valid_script_object(who, o) else {
            return Ok(-1);
        };
        if radius <= 0 {
            return Ok(-1);
        }
        let (object_x, object_y) = self.script_object_tile_point(object)?;
        let distance =
            Self::script_point_distance(object_x.wrapping_sub(x), object_y.wrapping_sub(y))?;
        Ok((distance < radius) as i32)
    }

    /// `ScenarioFuncSet::object_position_{x,y}` (`0x009f1360` / `0x009f1470`).
    fn script_object_position(&self, who: i32, o: i32, y_axis: bool) -> Result<i32, HostError> {
        let who = who.wrapping_sub(1) as u32 as usize;
        let Some(object) = self.valid_script_position_object(who, o) else {
            return Ok(-1);
        };
        let object = self.script_captain(object)?;
        let object = self.script_outer_container(object)?;
        let coord = match object {
            ScriptObject::Unit { row, .. } => {
                if y_axis {
                    self.world.units.y_internal()[row]
                } else {
                    self.world.units.x_internal()[row]
                }
            }
            ScriptObject::Build { row, .. } => {
                let (x, y) = self.builds[row].position();
                if y_axis {
                    y
                } else {
                    x
                }
            }
            ScriptObject::Wall { row, .. } => {
                if y_axis {
                    self.walls[row].y()
                } else {
                    self.walls[row].x()
                }
            }
        };
        if coord < 0 {
            // Retail indexes the finite non-negative `div_3_table` after an arithmetic
            // shift. A negative coordinate is corrupt host state, not a value to invent.
            return Err(HostError::Unimplemented);
        }
        Ok(coord / crate::systems::production::COORD_PER_TILE)
    }
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

    fn map_tile_width(&self) -> i32 {
        self.map.world.tile_xs
    }

    fn map_tile_height(&self) -> i32 {
        self.map.world.tile_ys
    }

    fn call_scenario(&mut self, decl: &BuiltinDecl, args: &[Value]) -> HostResult {
        match decl.index {
            // The fog-effect cohort uses global builtin indices. The compiler resolves
            // the two `set_explored` overloads by exact arity: #67 is the disc stamp;
            // #68 is the whole-map reveal bit also written by #74.
            67 => Ok(Value::Int(self.script_set_explored(
                args[0].as_int(),
                args[1].as_int(),
                args[2].as_int(),
                args[3].as_int(),
            ))),
            68 | 74 => Ok(Value::Int(self.script_set_show_all(args[0].as_int(), true))),
            75 => Ok(Value::Int(
                self.script_set_show_all(args[0].as_int(), false),
            )),
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
            // `map_is_passable` `0x009e4de0`: tile bounds, convert to the containing
            // WCoord cell, then test `(WData::flags & 0x70) == 0`.
            84 => {
                let x = args[0].as_int();
                let y = args[1].as_int();
                if x < 0 || y < 0 || x >= self.map.world.tile_xs || y >= self.map.world.tile_ys {
                    return Ok(Value::Int(-1));
                }
                Ok(Value::Int(self.map.world.is_passable(x >> 2, y >> 2) as i32))
            }
            // `map_is_buildable` `0x009e4e50`: the same tile/WCoord conversion, then
            // reject mountains, forest, ocean, rocks, or the unattributed 0x40 bit.
            85 => {
                let x = args[0].as_int();
                let y = args[1].as_int();
                if x < 0 || y < 0 || x >= self.map.world.tile_xs || y >= self.map.world.tile_ys {
                    return Ok(Value::Int(-1));
                }
                Ok(Value::Int(self.map.world.is_flat(x >> 2, y >> 2) as i32))
            }
            // `territory_owner` `0x009e4f00`: tile bounds, containing WData cell,
            // then the signed `who` byte plus one for the script player convention.
            88 => {
                let x = args[0].as_int();
                let y = args[1].as_int();
                if x < 0 || y < 0 || x >= self.map.world.tile_xs || y >= self.map.world.tile_ys {
                    return Ok(Value::Int(-1));
                }
                Ok(Value::Int(
                    self.map.world.get_who(x >> 2, y >> 2).wrapping_add(1),
                ))
            }
            // `is_victory_*` (`0x009e5250..0x009e52e0`): each handler is a direct
            // byte comparison against `GameInfo::victory` (`Game +0x38`).
            96..=105 => {
                let expected = match decl.index {
                    96 => victory_score::Victory::Standard,
                    97 => victory_score::Victory::Conquest,
                    98 => victory_score::Victory::Economic,
                    99 => victory_score::Victory::MusicalChairs,
                    100 => victory_score::Victory::Score,
                    101 => victory_score::Victory::SuddenDeath,
                    102 => victory_score::Victory::TechRace,
                    103 => victory_score::Victory::Population,
                    104 => victory_score::Victory::TimeLimit,
                    105 => victory_score::Victory::Wonder,
                    _ => unreachable!(),
                };
                Ok(Value::Int(
                    (self.vic_match.options.victory == expected as u8) as i32,
                ))
            }
            // `get_time_limit` `0x009e5bf0`: only Time Limit victory is valid. The
            // ordinary category indices read `time_limits[index].data[0]` directly.
            // Index 8 is retail's scenario override and depends on two ScenarioData
            // flags plus the separate `0x00cc21b4` custom-limit global, none of which
            // Sim owns; that branch therefore remains deliberately fail-closed.
            137 => {
                if self.vic_match.options.victory != victory_score::Victory::TimeLimit as u8 {
                    return Ok(Value::Int(-1));
                }
                let index = self.vic_match.options.time_limit as usize;
                if index >= 8 {
                    return Err(HostError::Unimplemented);
                }
                let value = self
                    .vic_match
                    .victory_options
                    .time_limits
                    .get(index)
                    .copied()
                    .ok_or(HostError::Unimplemented)?;
                Ok(Value::Int(value))
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
            // `population` `0x009e8e70`: both Leader flags, then the direct
            // `LeaderData::control` read at +0x940.
            245 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let leader = self
                    .production_runtime
                    .leaders
                    .get(who)
                    .ok_or(HostError::Unimplemented)?;
                Ok(Value::Int(leader.control))
            }
            // `population_cap` `0x009e8eb0`: both Leader flags, then the direct
            // `LeaderData::pop_cap` read at +0x7e4.
            246 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(self.step8.leaders[who].pop_cap))
            }
            // `score` `0x009e8fa0`: the same gate, then `LeaderData::score` at +0x18.
            249 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(self.vic_leaders.slots[who].score))
            }
            // `get_territory` `0x009e8fe0`: both Leader flags, then the direct owned-tile
            // count at +0x9d8 scaled by the World's authoritative land-size field.
            250 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let scaled = self.vic_leaders.slots[who].territory.wrapping_mul(100);
                let percent = scaled
                    .checked_div(self.map.world.land_size)
                    .ok_or(HostError::Unimplemented)?;
                Ok(Value::Int(percent))
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
            // `gather_rate` `0x009e90b0`: both Leader flags, primary-resource type,
            // then the displayed income slot divided by 16 with signed truncation.
            253 => {
                let who = args[0].as_int();
                let Some(who) = self.active_script_leader(who) else {
                    return Ok(Value::Int(-1));
                };
                let Some(resource) = resource_index(string_arg(args, 1)?)? else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(self.leaders[who].econ.displayed[resource] / 16))
            }
            // `get_starting_loc_{x,y}` (`0x009e9250` / `0x009e92a0`): the one-bit
            // in-game Leader gate, array-length gate, then WCoord-to-tile `<< 2`.
            256 | 257 => {
                let Some(who) = self.in_game_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let coords = if decl.index == 256 {
                    &self.map.world.start_x.items
                } else {
                    &self.map.world.start_y.items
                };
                let Some(&coord) = coords.get(who) else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(coord.wrapping_shl(2)))
            }
            // `get_last_unit_built` `0x009e9a70`: the one-bit in-game Leader gate,
            // then ScenarioData's owner-local last completed unit object id.
            267 => {
                let Some(who) = self.in_game_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let leader = self
                    .production_runtime
                    .leaders
                    .get(who)
                    .ok_or(HostError::Unimplemented)?;
                Ok(Value::Int(leader.last_unit_built))
            }
            // `num_buildings` `0x009e9bf0`: both Leader flags, then sum the exact 129
            // unsigned-short counters at LeaderData +0x555e.
            270 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(
                    self.vic_leaders.slots[who]
                        .num_buildings
                        .iter()
                        .map(|&count| i32::from(count))
                        .sum(),
                ))
            }
            // `num_units` `0x009e9d60`: sum all 352 unsigned-short unit counters at
            // LeaderData +0x5762. The paired retail loop only unrolls that exact sum.
            273 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(
                    self.vic_leaders.slots[who]
                        .num_units
                        .iter()
                        .map(|&count| i32::from(count))
                        .sum(),
                ))
            }
            // The addressed-object location cohort shares `valid_object_o`, direct
            // captain-coordinate reads, and the retail integer distance approximation.
            // Radius comparisons are strict; a non-positive radius is an invalid call.
            402 => Ok(Value::Int(self.script_is_object_at(
                args[0].as_int(),
                args[1].as_int(),
                args[2].as_int(),
                args[3].as_int(),
            )?)),
            403 => Ok(Value::Int(self.script_object_near(
                args[0].as_int(),
                args[1].as_int(),
                args[2].as_int(),
                args[3].as_int(),
                args[4].as_int(),
            )?)),
            // Both handlers share the exact address validation, captain resolution,
            // outer-container walk, coordinate deobfuscation, and `div_3_table` tile
            // conversion recovered at `0x009f1360` / `0x009f1470`.
            411 => Ok(Value::Int(self.script_object_position(
                args[0].as_int(),
                args[1].as_int(),
                false,
            )?)),
            412 => Ok(Value::Int(self.script_object_position(
                args[0].as_int(),
                args[1].as_int(),
                true,
            )?)),
            // `is_garrisoned` `0x009f2f20`: exact `valid_object_o`, then
            // `ObjectData::get_inside`; only a resolved outer building container counts.
            446 => {
                let who = args[0].as_int().wrapping_sub(1) as u32 as usize;
                let Some(object) = self.valid_script_object(who, args[1].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(matches!(
                    self.script_container(object)?,
                    Some(ScriptObject::Build { .. })
                ) as i32))
            }
            // `object_health` `0x009f5910`: exact `valid_object_o`, unit captain and
            // `total_damage` resolution, dynamic construction maximum, then the retail
            // f32 percentage and truncating SSE conversion.
            525 => Ok(Value::Int(
                self.script_object_health(args[0].as_int(), args[1].as_int())?,
            )),
            // `object_max_health` `0x009f5f60`: the same object gate followed directly
            // by virtual `hits(0)`; notably, no unit-captain redirection occurs here.
            530 => Ok(Value::Int(self.script_object_max_health_read(
                args[0].as_int(),
                args[1].as_int(),
            )?)),
            // `is_idle` `0x009f9370`: an empty order list is not enough; retail calls
            // `is_captain` on the originally addressed unit, without resolving `o_up`.
            583 => {
                let who = args[0].as_int().wrapping_sub(1) as u32 as usize;
                let Some(object) = self.valid_script_object(who, args[1].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let ScriptObject::Unit { row, .. } = object else {
                    // The registered contract says `unit_o`; retail's direct UnitData
                    // method call leaves valid non-unit input outside the recovered domain.
                    return Err(HostError::Unimplemented);
                };
                Ok(Value::Int(
                    (self.world.orders(row).is_empty() && self.world.units.o_up()[row] < 0) as i32,
                ))
            }
            // `has_move_order` `0x009f9840`: a non-idle UnitData has a current order;
            // dispatch its virtual `is_move` classification. The exact seven concrete
            // overrides are the measured `MOVE_LIKE` set.
            592 => {
                let who = args[0].as_int().wrapping_sub(1) as u32 as usize;
                let Some(object) = self.valid_script_object(who, args[1].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let ScriptObject::Unit { row, .. } = object else {
                    return Err(HostError::Unimplemented);
                };
                let is_move = self
                    .world
                    .orders(row)
                    .current()
                    .is_some_and(|order| order_dispatch::is_move_like(order.kind));
                Ok(Value::Int(is_move as i32))
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
            // `take_good` `0x009fb630`: active Leader and primary resource, wrapping
            // subtraction in the decoded stockpile followed by a clamp at zero.
            662 => {
                let (who, resource, amount) = leader_resource_args(args)?;
                let Some(flags) = self.step8.leaders.get(who).map(|leader| leader.flags) else {
                    return Ok(Value::Int(-1));
                };
                if flags & leaders::flag::IN_GAME == 0 {
                    return Ok(Value::Int(-1));
                }
                let Some(leader) = self.leaders.get_mut(who) else {
                    return Ok(Value::Int(-1));
                };
                let Some(resource) = resource else {
                    return Ok(Value::Int(-1));
                };
                let remaining = leader.econ.stockpile[resource].wrapping_sub(amount);
                leader.econ.stockpile[resource] = remaining.max(0);
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
            // `get_base_rate` `0x009fbc20`: both Leader flags and a primary-resource
            // type, then signed `/ 16` over the exact +0x4b0 base-rate term.
            670 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let Some(resource) = resource_index(string_arg(args, 1)?)? else {
                    return Ok(Value::Int(-1));
                };
                let leader = self.leaders.get(who).ok_or(HostError::Unimplemented)?;
                Ok(Value::Int(leader.gather_ctx.extra_income[resource] / 16))
            }
            // `have_alliance` `0x009fcf50`: both players pass the two-bit active gate,
            // then the first player's directed diplomacy slot is exactly value 2.
            706 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let Some(other) = self.active_script_leader(args[1].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(
                    (self.step8.leaders[who].diplo[other] == victory_score::Diplo::Ally as i32)
                        as i32,
                ))
            }
            // `have_peace` `0x009fcfc0`: values 1 (peace) and 2 (alliance) both count.
            707 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let Some(other) = self.active_script_leader(args[1].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let diplo = self.step8.leaders[who].diplo[other];
                Ok(Value::Int(
                    (diplo == victory_score::Diplo::Peace as i32
                        || diplo == victory_score::Diplo::Ally as i32) as i32,
                ))
            }
            // `have_war` `0x009fd040`: the same two active gates, then directed value 0.
            708 => {
                let Some(who) = self.active_script_leader(args[0].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                let Some(other) = self.active_script_leader(args[1].as_int()) else {
                    return Ok(Value::Int(-1));
                };
                Ok(Value::Int(
                    (self.step8.leaders[who].diplo[other] == victory_score::Diplo::War as i32)
                        as i32,
                ))
            }
            // Direct LeaderData+0x04 policy toggles. These are global builtin indices,
            // not ScenarioFuncSet-local ordinals. Every successful body performs exactly
            // one AND/OR mutation after its retail leader gate and then returns 1.
            785 | 787 | 789 | 790 | 791 | 792 | 796 | 797 | 798 | 799 => {
                let (bit, enabled, require_active, reject_human) = match decl.index {
                    785 => (SCRIPT_PRODUCTION_AI_OFF, true, false, false),
                    787 => (SCRIPT_PRODUCTION_AI_OFF, false, false, false),
                    789 => (SCRIPT_COMBAT_AI_OFF, true, false, false),
                    790 => (SCRIPT_COMBAT_AI_OFF, false, false, false),
                    791 => (SCRIPT_UNIT_AI_OFF, true, false, false),
                    792 => (SCRIPT_UNIT_AI_OFF, false, false, false),
                    796 => (SCRIPT_NO_EXPANSION, true, true, true),
                    797 => (SCRIPT_NO_EXPANSION, false, true, true),
                    798 => (SCRIPT_NO_CITY_DEFEAT, true, true, false),
                    799 => (SCRIPT_NO_CITY_DEFEAT, false, true, false),
                    _ => unreachable!(),
                };
                Ok(Value::Int(self.script_set_leader_policy(
                    args[0].as_int(),
                    bit,
                    enabled,
                    require_active,
                    reject_human,
                )))
            }
            // The unit-specific pair first tries retail's active-unit address path,
            // then falls back to the negative-id all-unit Leader policy sentinel.
            793 | 794 => Ok(Value::Int(self.script_set_unit_ai(
                args[0].as_int(),
                args[1].as_int(),
                decl.index == 793,
            )?)),
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
