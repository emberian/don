// SPDX-License-Identifier: GPL-3.0-or-later

//! `Leader::plan_strategy` `0x006B9620` and `Leader::production_ai` `0x006C1960` —
//! the two functions tick step 11 reaches on its way into retail's compiled AI.
//!
//! ```text
//! Game::do_frame                            0x00591EF0
//!  +- [11] Leaders::strategy_all            0x006ED430   systems/leaders.rs
//!       for slot in 0..8, leader_flags & 3 == 3:
//!         Leader::check_explore             0x006BC860   systems/leaders.rs
//!         Leader::plan_strategy             0x006B9620   <- THIS FILE (11,108 B)
//!         |   phase = (who*25 + frame) % (200 / ai_speed)
//!         |   if production_step != 0:
//!         |     +- Leader::production_ai    0x006C1960   <- THIS FILE (628 B, whole)
//!         |          +- RunTimeEnv::run_script          0x0043D0E0  boundary (BHS)
//!         |          +- Leader::production_ai_setup     0x006C83E0  boundary (1,807 B)
//!         |          +- MakeList::clear                 0x006C9DB0  boundary (210 B)
//!         |          +- Leader::found_cities            0x006C7A60  boundary (1,708 B)
//!         |          +- Leader::research_techs          0x006C6BA0  boundary (3,776 B)
//!         |          +- Leader::upgrade_units           0x006C6430  boundary (1,902 B)
//!         |          +- Leader::create_units            0x006C40A0  boundary (9,104 B)
//!         |          +- Leader::create_buildings        0x006C1BE0  boundary (9,405 B)
//!         |          +- Leader::make_stuff              0x006C8AF0  boundary (1,732 B)
//!         |          +- Leader::queued_units            0x006CE000  host answer (394 B)
//!         |   else if frame != 0 && phase != 0:
//!         |     if phase % 30 != 0: RETURN having touched nothing
//!         |     else the MakeList head tech fast lane:
//!         |       +- Leader::can_pay        0x006C9B90  boundary (79 B)
//!         |       +- LeaderData::has_tech   0x006E0C80  boundary (176 B)
//!         |       +- LeaderData::researching 0x006DB510 boundary (364 B)
//!         |       +- Leader::make_this      0x006C94F0  boundary (1,309 B)
//!         |   else the 11,108-byte planning body                    boundary
//!         Leader::compute_score(0)          0x006EC560   victory_score.rs
//!         Leader::diplomacy                 0x006BC950   gate in systems/leaders.rs
//! ```
//!
//! Everything below is [measured] from a radare2 disassembly of `0x006B9620`,
//! `0x006C1960`, `0x006C9B90`, `0x006BC860`, `0x009FF5E0`, `0x009FF7A0`, `0x009FF820`,
//! `0x009FF8A0`, `0x009FFBA0`, `0x009E5D20` and `0x005930C0` against
//! `ron-bin/riseofnations.exe`, cross-read against `re/decomp-all/006b9620.c` and
//! `re/decomp-all/006c1960.c` and the PDB layouts in `schema/pdb-types.json`.
//! **Tier C**: structure and constants are read from the binary; nothing here has been
//! executed against retail.
//!
//! # Six things this file establishes
//!
//! 1. **Step 11 is the only door into retail's production AI.** `Leaders::strategy_all`
//!    `0x006ED452` is the *only* call site of `Leader::plan_strategy`, and
//!    `Leader::plan_strategy` `0x006B9662` is the *only* call site of
//!    `Leader::production_ai`, which is the *only* caller of the eight production stage
//!    functions. Until step 11 runs the machine, the AI's whole build/research pipeline
//!    is unreachable, not merely unported.
//! 2. **`plan_strategy`'s phase is not `check_explore`'s.** Both use
//!    `(who * 25 + frame) % (200 / ai_speed)`, but `check_explore` `0x006BC890` biases the
//!    dividend by `+ 12` (`lea eax, [esi + 0xc]`) and `plan_strategy` `0x006B9650` does
//!    not. The two children of one dispatcher fire on different frames.
//! 3. **`production_step != 0` bypasses the phase entirely** (`0x006B9655`). The phase is
//!    computed first but only consulted once the production cycle is idle, so a running
//!    cycle advances one stage *every frame* of the leader's turn, not once per period.
//! 4. **196 of every 200 `plan_strategy` calls provably return having touched nothing.**
//!    With `production_step == 0` the body is `if (frame != 0 && phase != 0) { if (phase %
//!    30 != 0) return; ... }`, so at `ai_speed = 1` only `phase == 0` (the planning body)
//!    and the six `phase in {30,60,90,120,150,180}` frames get past the gate. Charging a
//!    coverage gap on the other 193 over-reports what retail did.
//! 5. **`LeaderData::leader_flags2`'s low bits are the four AI-subsystem kill switches**,
//!    read straight off the shipped scenario-script API — see [`flags2`]. Retail's
//!    `production_ai` gate at `0x006C198A` is exactly `leader_flags2 &
//!    PRODUCTION_AI_DISABLED`.
//! 6. **`leader_flags & 4` is the human bit.** `ScenarioFuncSet::change_to_ai`
//!    `0x009E5D57` requires it set and clears it (`and dword [...], 0xFFFFFFFB`) while
//!    converting a player to AI. `Leader::production_ai` `0x006C1970` and
//!    `Leader::diplomacy` `0x006BC99A` both refuse a leader that carries it.
//!
//! # What is deliberately not here
//!
//! No stage body. All eight are between 210 and 9,405 bytes of AI policy and every one of
//! them is recorded as a named [`Stage`] the machine reached, with its retail VA, rather
//! than approximated. No `MakeList`: the fast lane's `[this + 0x6ED8]` read is the
//! `Array<MakeObject>::list` pointer and the head element's `TypeIndex`, supplied as an
//! explicit host answer, because nothing in `don-sim` owns the AI's want-list.

use std::fmt;

// ===============================================================================================
// Retail addresses
// ===============================================================================================

/// Every retail address this file names, so a reader never has to trust a comment.
pub mod va {
    /// `Leader::plan_strategy`, 11,108 bytes, `leaders.cpp:26880`.
    pub const PLAN_STRATEGY: u32 = 0x006b_9620;
    /// `Leader::production_ai`, 628 bytes, `leaders.cpp:23401`.
    pub const PRODUCTION_AI: u32 = 0x006c_1960;
    /// The single `call 0x6C1960` inside `plan_strategy`.
    pub const PLAN_STRATEGY_PRODUCTION_CALL: u32 = 0x006b_9662;
    /// `jne 0x6BC0F7` — the `phase % 30` early return.
    pub const PLAN_STRATEGY_SUB_PHASE_MISS: u32 = 0x006b_9698;
    /// The plain epilogue every early return jumps to.
    pub const PLAN_STRATEGY_RETURN: u32 = 0x006b_c0f7;
    /// `mov eax, [ebx + 0x6ED8]` — the `MakeList::list` read that opens the fast lane.
    pub const PLAN_STRATEGY_MAKE_LIST_READ: u32 = 0x006b_969e;
    /// The 11,108-byte planning body proper (`frame == 0 || phase == 0`).
    pub const PLAN_STRATEGY_BODY: u32 = 0x006b_96f9;

    /// `Leader::can_pay(int)`, 79 bytes.
    pub const CAN_PAY: u32 = 0x006c_9b90;
    /// `LeaderData::has_tech(TypeIndex) const`, 176 bytes.
    pub const HAS_TECH: u32 = 0x006e_0c80;
    /// `LeaderData::researching(TypeIndex, int, int, int) const`, 364 bytes.
    pub const RESEARCHING: u32 = 0x006d_b510;
    /// `Leader::make_this(int)`, 1,309 bytes.
    pub const MAKE_THIS: u32 = 0x006c_94f0;

    /// `Leader::queued_units()`, 394 bytes.
    pub const QUEUED_UNITS: u32 = 0x006c_e000;
    /// `RunTimeEnv::run_script(const String&, int, ...)` — the BHS entry point.
    pub const RUN_SCRIPT: u32 = 0x0043_d0e0;
    /// `RunTimeEnv::get_ret_int()`, 20 bytes.
    pub const GET_RET_INT: u32 = 0x0047_d9e0;
    /// `Leader::production_ai_setup()`, 1,807 bytes.
    pub const PRODUCTION_AI_SETUP: u32 = 0x006c_83e0;
    /// `MakeList::clear()`, 210 bytes.
    pub const MAKE_LIST_CLEAR: u32 = 0x006c_9db0;
    /// `Leader::found_cities()`, 1,708 bytes.
    pub const FOUND_CITIES: u32 = 0x006c_7a60;
    /// `Leader::research_techs()`, 3,776 bytes.
    pub const RESEARCH_TECHS: u32 = 0x006c_6ba0;
    /// `Leader::upgrade_units()`, 1,902 bytes.
    pub const UPGRADE_UNITS: u32 = 0x006c_6430;
    /// `Leader::create_units()`, 9,104 bytes.
    pub const CREATE_UNITS: u32 = 0x006c_40a0;
    /// `Leader::create_buildings()`, 9,405 bytes.
    pub const CREATE_BUILDINGS: u32 = 0x006c_1be0;
    /// `Leader::make_stuff()`, 1,732 bytes.
    pub const MAKE_STUFF: u32 = 0x006c_8af0;

    /// The 11-entry `jmp dword [ecx*4 + 0x6C1BA8]` table, indexed `production_step - 1`.
    pub const PRODUCTION_STEP_TABLE: u32 = 0x006c_1ba8;
    /// `mov dword [edi + 0x788], 0` — every refusal and the end of the cycle land here.
    pub const PRODUCTION_STEP_RESET: u32 = 0x006c_1b96;

    /// `GameAccess::ai_speed`, the pointer slot at `0x00C061C0`.
    pub const AI_SPEED_PTR: u32 = 0x00c0_61c0;
    /// `GameAccess::ai_off`, the pointer slot at `0x00C061C4`. `Game::action_cheat_ai_toggle`
    /// `0x005930C0` flips it.
    pub const AI_OFF_PTR: u32 = 0x00c0_61c4;
    /// `ScenarioFuncSet::enable_production_ai` / `disable_production_ai`.
    pub const ENABLE_PRODUCTION_AI: u32 = 0x009f_f5e0;
    pub const DISABLE_PRODUCTION_AI: u32 = 0x009f_f7a0;
    /// `ScenarioFuncSet::change_to_ai` — the writer that names `leader_flags & 4`.
    pub const CHANGE_TO_AI: u32 = 0x009e_5d20;
}

/// Byte offsets inside `LeaderData` this machine reads or writes, each recovered from the
/// instruction that touches it and cross-checked against `schema/pdb-types.json`.
pub mod offsets {
    /// `0x006C196E` reads it as a byte for the human/AI-assist gate. PDB `leader_flags`.
    pub const LEADER_FLAGS: usize = 0x000;
    /// `0x006C198A` (`test byte [edi + 4], 4`). PDB `leader_flags2`.
    pub const LEADER_FLAGS2: usize = 0x004;
    /// `0x006B9636` (`imul esi, dword [ebx + 8], 0x19`). PDB `who`.
    pub const WHO: usize = 0x008;
    /// `0x006C1999`. PDB `control`.
    pub const CONTROL: usize = 0x940;
    /// `0x006B9655` / `0x006C19A2`. PDB `production_step`.
    pub const PRODUCTION_STEP: usize = 0x788;
    /// `0x006C19B1`. PDB `prod_script_run`.
    pub const PROD_SCRIPT_RUN: usize = 0x78c;
    /// `0x006C19E8`. PDB `script_step`.
    pub const SCRIPT_STEP: usize = 0x790;
    /// `0x006C19A9`. PDB `effective_pop`.
    pub const EFFECTIVE_POP: usize = 0x9e0;
    /// `0x006C1A15`. PDB `pers`, a `Personality`.
    pub const PERS: usize = 0x6dd4;
    /// `0x006C1A4F`. PDB `prod_script`, a `String`.
    pub const PROD_SCRIPT: usize = 0x6ea4;
    /// `0x006C1AC4` (`lea ecx, [edi + 0x6EC8]`). PDB `make_list`, a `MakeList`.
    pub const MAKE_LIST: usize = 0x6ec8;
    /// `0x006B969E` / `0x006C9BA6`. `make_list + 0x10` is `ArrayBase<MakeObject>::list`.
    pub const MAKE_LIST_LIST: usize = 0x6ed8;
}

/// `LeaderData::leader_flags` bits this machine reads.
///
/// Named from their *writers*, not from the PDB, which does not decompose the word.
pub mod flags {
    /// `& 4`. `ScenarioFuncSet::change_to_ai` `0x009E5D46` requires it and `0x009E5D57`
    /// clears it while converting the player to AI, so it is the human bit.
    /// `ScenarioFuncSet::enable_city_ai` `0x009FFBC8` also refuses a leader carrying it.
    pub const HUMAN: u32 = 0x0000_0004;
    /// `& 8`. `Leader::production_ai` `0x006C1974` runs the production cycle for a
    /// [`HUMAN`] leader when this is *also* set, and refuses when it is clear. No writer
    /// was located in this lane, so it is named for the gate it forms and nothing more.
    pub const PRODUCTION_DESPITE_HUMAN: u32 = 0x0000_0008;
}

/// `LeaderData::leader_flags2`'s low bits — the four AI-subsystem kill switches, read off
/// the shipped scenario-script API, which is the only code in the image that writes them.
///
/// | script function | VA | instruction |
/// |---|---|---|
/// | `disable_production_ai` | `0x009FF7A0` | `or  dword [leader_flags2], 4` |
/// | `enable_production_ai`  | `0x009FF5E0` | `and dword [leader_flags2], ~4` |
/// | `enable_combat_ai`      | `0x009FF820` | `and dword [leader_flags2], ~8` |
/// | `enable_all_unit_ai`    | `0x009FF8A0` | `and dword [leader_flags2], ~2` |
/// | `enable_city_ai`        | `0x009FFBA0` | `and dword [leader_flags2], ~0x10` |
///
/// Every one of them first requires `leader_flags & 1` on the target leader and returns
/// `-1` otherwise; `enable_city_ai` additionally requires `& 2` and refuses `& 4`.
pub mod flags2 {
    pub const UNIT_AI_DISABLED: u32 = 0x0000_0002;
    pub const PRODUCTION_AI_DISABLED: u32 = 0x0000_0004;
    pub const COMBAT_AI_DISABLED: u32 = 0x0000_0008;
    pub const CITY_AI_DISABLED: u32 = 0x0000_0010;
}

/// `mov eax, 200` at `0x006B9629`, divided by `GameAccess::ai_speed`.
pub const PERIOD_BASE: i32 = 200;
/// `imul esi, dword [ebx + 8], 0x19` at `0x006B9636`.
pub const SLOT_PHASE: i32 = 25;
/// The `esi % 30` at `0x006B967A..0x006B9698`, computed with the `0x88888889` magic
/// sequence, which is a truncating signed remainder and therefore Rust's `%`.
pub const SUB_PHASE: i32 = 30;
/// `lea eax, [esi - 0x220]` / `cmp eax, 0x54` at `0x006B96A6` — the fast lane only fires
/// when the `MakeList` head's `TypeIndex` is inside `[0x220, 0x274]` inclusive.
pub const TECH_TYPE_FIRST: i32 = 0x220;
pub const TECH_TYPE_LAST: i32 = 0x274;
/// `LeaderData::researching(head, -1, 0, 0)` — the argument triple pushed at
/// `0x006B96D6..0x006B96DA`.
pub const RESEARCHING_ARGS: (i32, i32, i32) = (-1, 0, 0);
/// The 11 arms of the `production_step` jump table, `production_step - 1` in `0..=10`.
pub const PRODUCTION_STEP_ARMS: i32 = 11;
/// `cmp byte [game + 0x2D], 8`. `Game + 0x2D` is `GameInfo + 0x21`, `starting_resources`,
/// and 8 is the Infinite setting — the same encoding `victory_score::MatchOptions` uses.
pub const INFINITE_RESOURCES: u8 = 8;
/// `RunTimeEnv::get_ret_int` values `Leader::production_ai` branches on. 1 resets the whole
/// cycle (`production_step = 0`, `0x006C1A89`); 3 retires the script for good
/// (`prod_script_run = 0`, `0x006C1A9F`), which is the one-way `SCRIPT_DONE` latch.
pub const SCRIPT_BLOCK_ON_THIS: i32 = 1;
pub const SCRIPT_DONE: i32 = 3;

// ===============================================================================================
// State
// ===============================================================================================

/// The `LeaderData` slice `Leader::plan_strategy` and `Leader::production_ai` own.
///
/// Every field is a recovered offset (see [`offsets`]); the two `Option`s are host answers
/// this port does not compute, and `None` means "this host does not answer", never zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ProductionState {
    /// `+0x004` `leader_flags2` — the four kill switches in [`flags2`].
    pub flags2: u32,
    /// `+0x788` `production_step`, the 12-stage counter. Zero means the cycle is idle and
    /// `plan_strategy` consults its phase instead.
    pub production_step: i32,
    /// `+0x78C` `prod_script_run`. Set once by `Leader::init` and cleared once, here, by a
    /// script that returns `SCRIPT_DONE` or fails to run.
    pub prod_script_run: i32,
    /// `+0x790` `script_step`, the BHS script's own `ref int step`. Passed into
    /// `RunTimeEnv::run_script` and rewritten from the first `ScriptInt` on return.
    pub script_step: i32,
    /// `+0x940` `control`.
    pub control: i32,
    /// `+0x9E0` `effective_pop`, written unconditionally before the jump table at
    /// `0x006C19A9` as `queued_units() + control + 1`.
    pub effective_pop: i32,
    /// `+0x6DD4` `pers`. Only its *address* is used here: `pers + 2` is the third
    /// `ScriptInt` handed to the BHS script (`0x006C1A15`). Held as the decoded value so a
    /// host that owns `Personality` can supply it; `None` keeps the script argument
    /// explicit rather than invented.
    pub pers_arg: Option<i32>,
    /// `Leader::queued_units()` `0x006CE000`'s answer, 394 bytes of build-queue walking
    /// this port does not perform.
    pub queued_units: Option<i32>,
    /// `*(TypeIndex *)(make_list + 0x10)` — the `TypeIndex` of `MakeList::list[0]`, read
    /// unconditionally at `0x006B96A4` without consulting `length`. `None` because nothing
    /// in `don-sim` owns the AI want-list.
    pub make_list_head: Option<i32>,
    /// `RunTimeEnv::get_ret_int()`'s answer for a script invocation this port cannot
    /// perform. `None` stalls the step-1 arm instead of guessing a control-flow branch.
    pub script_result: Option<i32>,
    /// `Leader::make_stuff()`'s `int` return for the step-8 arm, which branches on it.
    /// `None` stalls instead of guessing whether the cycle ends at 8 or runs on to 11.
    pub make_stuff_result: Option<i32>,
}

/// The globals both functions read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AiEnv {
    /// `Game::frame`, `[game + 0x550]`. Step 11 runs before the step-20 increment, so this
    /// is the pre-increment frame, exactly as `check_explore` sees it.
    pub frame: i32,
    /// `*GameAccess::ai_speed`, `[[0x00C061C0]]`.
    pub ai_speed: i32,
    /// `*GameAccess::ai_off != 0`, `[[0x00C061C4]]`. `Game::action_cheat_ai_toggle`
    /// `0x005930C0` flips it, gated on game semaphore bit 2 (`game[0x820] & 4`).
    pub ai_off: bool,
    /// `GameInfo::starting_resources`, read as `byte [game + 0x2D]`. `None` is a host that
    /// did not answer; the arms that compare it against [`INFINITE_RESOURCES`] then stall.
    pub starting_resources: Option<u8>,
}

impl Default for AiEnv {
    /// `ai_speed` defaults to 1 because zero is a divide fault in retail, not a value.
    fn default() -> Self {
        AiEnv {
            frame: 0,
            ai_speed: 1,
            ai_off: false,
            starting_resources: None,
        }
    }
}

// ===============================================================================================
// Traces
// ===============================================================================================

/// A retail call this port reached but does not perform, named by function and VA.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// `0x006C19E8`. `RunTimeEnv::run_script(prod_script, 4, {who + 1}, {script_step,
    /// kind 3}, {pers + 2}, {5})` — the four `ScriptInt` arguments in push order.
    RunProductionScript {
        script_step: i32,
        pers_arg: Option<i32>,
        who_plus_one: i32,
    },
    /// `0x006C1ABF`, step 2.
    ProductionAiSetup,
    /// `0x006C1ACA` / `0x006C1B00`, step 2 and the Infinite-resources tail.
    MakeListClear,
    /// `0x006C1ADF`, step 3.
    FoundCities,
    /// `0x006C1B15`, step 4.
    ResearchTechs,
    /// `0x006C1B25`, step 5.
    UpgradeUnits,
    /// `0x006C1B35`, steps 6 and 9.
    CreateUnits,
    /// `0x006C1B45` (steps 7) / `0x006C1B83` (step 10).
    CreateBuildings,
    /// `0x006C1AF5` (Infinite tail), `0x006C1B55` (step 8), `0x006C1B91` (step 11).
    MakeStuff,
}

impl Stage {
    /// The retail function this boundary would have entered.
    pub fn callee_va(self) -> u32 {
        match self {
            Stage::RunProductionScript { .. } => va::RUN_SCRIPT,
            Stage::ProductionAiSetup => va::PRODUCTION_AI_SETUP,
            Stage::MakeListClear => va::MAKE_LIST_CLEAR,
            Stage::FoundCities => va::FOUND_CITIES,
            Stage::ResearchTechs => va::RESEARCH_TECHS,
            Stage::UpgradeUnits => va::UPGRADE_UNITS,
            Stage::CreateUnits => va::CREATE_UNITS,
            Stage::CreateBuildings => va::CREATE_BUILDINGS,
            Stage::MakeStuff => va::MAKE_STUFF,
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Stage::RunProductionScript { .. } => "RunTimeEnv::run_script",
            Stage::ProductionAiSetup => "Leader::production_ai_setup",
            Stage::MakeListClear => "MakeList::clear",
            Stage::FoundCities => "Leader::found_cities",
            Stage::ResearchTechs => "Leader::research_techs",
            Stage::UpgradeUnits => "Leader::upgrade_units",
            Stage::CreateUnits => "Leader::create_units",
            Stage::CreateBuildings => "Leader::create_buildings",
            Stage::MakeStuff => "Leader::make_stuff",
        };
        write!(f, "{name} {:#010x}", self.callee_va())
    }
}

/// Why `Leader::production_ai` did not reach its jump table. Every one of these resets
/// `production_step` to zero at `0x006C1B96` on the way out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProductionGate {
    /// `0x006C1970`/`0x006C1974`: `leader_flags & 4` set and `& 8` clear.
    Human,
    /// `0x006C197C`: `*GameAccess::ai_off != 0`.
    AiOff,
    /// `0x006C198A`: `leader_flags2 & 4`.
    ProductionAiDisabled,
    /// `0x006C19D8`: `(unsigned)(production_step - 1) > 10`.
    StepOutsideTable { step: i32 },
    /// Past all four; the jump table ran.
    Entered,
}

/// A fact the machine needed and the host did not supply. The step is left exactly as
/// retail left it before the read, and nothing downstream of the read is claimed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stall {
    /// `byte [game + 0x2D]` — needed by the step-1 pre-check and by every arm that reaches
    /// the Infinite-resources tail.
    StartingResources { at_va: u32 },
    /// `RunTimeEnv::get_ret_int()` after `run_script`. Decides between `production_step =
    /// 0`, `prod_script_run = 0`, and a plain advance.
    ScriptResult { at_va: u32 },
    /// `Leader::make_stuff()`'s return in the step-8 arm. Decides whether the cycle ends
    /// at step 8 or continues into steps 9, 10 and 11.
    MakeStuffResult { at_va: u32 },
    /// `Leader::queued_units()` `0x006CE000`, 394 bytes of build-queue walking. Retail
    /// calls it unconditionally at `0x006C1994`; without its answer `effective_pop` is
    /// left alone rather than written wrong.
    QueuedUnits { at_va: u32 },
}

/// What one `Leader::production_ai` call did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductionTrace {
    pub gate: ProductionGate,
    pub step_before: i32,
    pub step_after: i32,
    /// `queued_units() + control + 1`, or `None` when `Leader::queued_units` had no
    /// answer, in which case `effective_pop` is left alone rather than written wrong.
    pub effective_pop: Option<i32>,
    /// Every unported retail call the arm reached, in instruction order.
    pub stages: Vec<Stage>,
    /// Every fact the machine needed and the host did not supply, in read order.
    pub stalls: Vec<Stall>,
}

impl ProductionTrace {
    fn refused(gate: ProductionGate, step_before: i32) -> ProductionTrace {
        ProductionTrace {
            gate,
            step_before,
            step_after: 0,
            effective_pop: None,
            stages: Vec::new(),
            stalls: Vec::new(),
        }
    }

    /// True when this call reached retail code the port does not execute, or could not
    /// decide a branch. A refusal reaches nothing at all and is therefore *not* charged.
    pub fn charges(&self) -> bool {
        !self.stages.is_empty() || !self.stalls.is_empty()
    }
}

/// Which arm of `Leader::plan_strategy` ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanArm {
    /// `production_step != 0` at `0x006B9655`: the whole call is `Leader::production_ai`.
    ProductionAi(ProductionTrace),
    /// `frame == 0 || phase == 0`: the 11,108-byte planning body at `0x006B96F9`.
    PlanningBody { first_unowned_va: u32 },
    /// `phase % 30 != 0` at `0x006B9698`: retail returns having read nothing but its own
    /// phase. Fully owned, and the reason step 11's planner gap over-reported.
    NotDue { phase: i32 },
    /// The fast lane opened but the `MakeList` head is not a `TypeIndex` in
    /// `[0x220, 0x274]`, so retail returns at `0x006B96AF` having touched nothing.
    TechQueueHeadRejected { head: i32 },
    /// The fast lane opened on a tech head and reached `Leader::can_pay` `0x006C9B90`.
    TechQueueReached { head: i32, first_call_va: u32 },
    /// `ai_speed == 0`, or `200 / ai_speed == 0`: retail takes an `idiv` fault here, which
    /// is not a behaviour to reproduce. It is a missing fact about the host.
    MissingPeriod { ai_speed: i32 },
    /// The fast lane opened and the host owns no `MakeList`.
    MissingMakeList,
}

/// What one `Leader::plan_strategy` call did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanTrace {
    /// `(who * 25 + frame) % (200 / ai_speed)`, or `None` when the period is unusable.
    /// Computed before the `production_step` test, exactly as retail computes it.
    pub phase: Option<i32>,
    pub arm: PlanArm,
}

impl PlanTrace {
    /// True when the call reached a retail boundary this port does not execute.
    ///
    /// This is the whole point of the file: three of the seven arms reach nothing, and one
    /// of those three is where retail spends 193 frames out of every 200.
    pub fn charges(&self) -> bool {
        match &self.arm {
            PlanArm::ProductionAi(trace) => trace.charges(),
            PlanArm::PlanningBody { .. }
            | PlanArm::TechQueueReached { .. }
            | PlanArm::MissingPeriod { .. }
            | PlanArm::MissingMakeList => true,
            PlanArm::NotDue { .. } | PlanArm::TechQueueHeadRejected { .. } => false,
        }
    }
}

// ===============================================================================================
// Leader::plan_strategy 0x006B9620
// ===============================================================================================

/// `200 / ai_speed`, the `mov eax, 200; cdq; idiv dword [ecx]` at `0x006B9629`.
///
/// Retail faults on `ai_speed == 0`. `None` is that impossible state, not a period of zero.
#[inline]
pub fn ai_period(ai_speed: i32) -> Option<i32> {
    let period = PERIOD_BASE.checked_div(ai_speed)?;
    if period == 0 {
        return None;
    }
    Some(period)
}

/// `(who * 25 + frame) % (200 / ai_speed)` — `0x006B9636`..`0x006B9653`.
///
/// Note the missing `+ 12`: `Leader::check_explore` `0x006BC890` biases the same dividend
/// and this does not, so the two children of step 11 are on different phases.
#[inline]
pub fn plan_phase(who: i32, env: AiEnv) -> Option<i32> {
    let period = ai_period(env.ai_speed)?;
    let dividend = who.wrapping_mul(SLOT_PHASE).wrapping_add(env.frame);
    dividend.checked_rem(period)
}

/// `Leader::plan_strategy` `0x006B9620`, the entry and dispatch skeleton, whole.
///
/// The 11,108-byte planning body itself is not here; reaching it is
/// [`PlanArm::PlanningBody`], which names `0x006B96F9`.
pub fn plan_strategy(st: &mut ProductionState, leader_flags: u32, who: i32, env: AiEnv) -> PlanTrace {
    let phase = plan_phase(who, env);

    // 0x006B9655: the production cycle bypasses the phase entirely.
    if st.production_step != 0 {
        let trace = production_ai(st, leader_flags, who, env);
        return PlanTrace {
            phase,
            arm: PlanArm::ProductionAi(trace),
        };
    }

    let Some(phase_value) = phase else {
        return PlanTrace {
            phase,
            arm: PlanArm::MissingPeriod {
                ai_speed: env.ai_speed,
            },
        };
    };

    // 0x006B966E / 0x006B9676: frame zero and phase zero both fall through to the body.
    if env.frame == 0 || phase_value == 0 {
        return PlanTrace {
            phase,
            arm: PlanArm::PlanningBody {
                first_unowned_va: va::PLAN_STRATEGY_BODY,
            },
        };
    }

    // 0x006B9698: the sub-phase. This is the arm retail takes on 193 frames in 200.
    if phase_value % SUB_PHASE != 0 {
        return PlanTrace {
            phase,
            arm: PlanArm::NotDue { phase: phase_value },
        };
    }

    // 0x006B969E: MakeList::list[0].type, read without consulting length.
    let Some(head) = st.make_list_head else {
        return PlanTrace {
            phase,
            arm: PlanArm::MissingMakeList,
        };
    };
    // 0x006B96A6: `lea eax, [esi - 0x220]; cmp eax, 0x54; ja` — an unsigned window test.
    if !(TECH_TYPE_FIRST..=TECH_TYPE_LAST).contains(&head) {
        return PlanTrace {
            phase,
            arm: PlanArm::TechQueueHeadRejected { head },
        };
    }
    PlanTrace {
        phase,
        arm: PlanArm::TechQueueReached {
            head,
            first_call_va: va::CAN_PAY,
        },
    }
}

// ===============================================================================================
// Leader::production_ai 0x006C1960
// ===============================================================================================

/// `Leader::production_ai` `0x006C1960`, the complete 628-byte step machine.
///
/// The eight stage bodies are boundaries; every `production_step` transition, the two
/// refusal resets, the `effective_pop` write and the Infinite-resources tail are executed.
pub fn production_ai(
    st: &mut ProductionState,
    leader_flags: u32,
    who: i32,
    env: AiEnv,
) -> ProductionTrace {
    let step_before = st.production_step;

    // 0x006C1970: a human leader only runs the cycle with the companion bit set.
    if leader_flags & flags::HUMAN != 0 && leader_flags & flags::PRODUCTION_DESPITE_HUMAN == 0 {
        st.production_step = 0;
        return ProductionTrace::refused(ProductionGate::Human, step_before);
    }
    // 0x006C197C.
    if env.ai_off {
        st.production_step = 0;
        return ProductionTrace::refused(ProductionGate::AiOff, step_before);
    }
    // 0x006C198A.
    if st.flags2 & flags2::PRODUCTION_AI_DISABLED != 0 {
        st.production_step = 0;
        return ProductionTrace::refused(ProductionGate::ProductionAiDisabled, step_before);
    }

    let mut trace = ProductionTrace {
        gate: ProductionGate::Entered,
        step_before,
        step_after: step_before,
        effective_pop: None,
        stages: Vec::new(),
        stalls: Vec::new(),
    };

    // 0x006C1994..0x006C19A9: effective_pop = queued_units() + control + 1, unconditional.
    match st.queued_units {
        Some(queued) => {
            let value = queued.wrapping_add(st.control).wrapping_add(1);
            st.effective_pop = value;
            trace.effective_pop = Some(value);
        }
        None => trace.stalls.push(Stall::QueuedUnits {
            at_va: va::QUEUED_UNITS,
        }),
    }

    // 0x006C19A2: step 1 is skipped outright when the script is already retired or the
    // match runs on Infinite resources.
    if st.production_step == 1 {
        if st.prod_script_run == 0 {
            st.production_step = 2;
        } else {
            match env.starting_resources {
                Some(value) => {
                    if value == INFINITE_RESOURCES {
                        st.production_step = 2;
                    }
                }
                None => {
                    trace.step_after = st.production_step;
                    trace.stalls.push(Stall::StartingResources {
                        at_va: 0x006c_19bf,
                    });
                    return trace;
                }
            }
        }
    }

    // 0x006C19CF..0x006C19E1: the 11-entry jump table, indexed production_step - 1.
    let step = st.production_step;
    let arm = step.wrapping_sub(1);
    if !(0..PRODUCTION_STEP_ARMS).contains(&arm) {
        st.production_step = 0;
        trace.gate = ProductionGate::StepOutsideTable { step };
        trace.step_after = 0;
        return trace;
    }

    match step {
        1 => run_script_arm(st, &mut trace, who),
        2 => {
            st.production_step = step + 1;
            trace.stages.push(Stage::ProductionAiSetup);
            trace.stages.push(Stage::MakeListClear);
        }
        3 => {
            st.production_step = step + 1;
            trace.stages.push(Stage::FoundCities);
            infinite_tail(&mut trace, env);
        }
        4 => {
            st.production_step = step + 1;
            trace.stages.push(Stage::ResearchTechs);
            infinite_tail(&mut trace, env);
        }
        5 => {
            st.production_step = step + 1;
            trace.stages.push(Stage::UpgradeUnits);
            infinite_tail(&mut trace, env);
        }
        6 | 9 => {
            st.production_step = step + 1;
            trace.stages.push(Stage::CreateUnits);
            infinite_tail(&mut trace, env);
        }
        7 => {
            st.production_step = step + 1;
            trace.stages.push(Stage::CreateBuildings);
            infinite_tail(&mut trace, env);
        }
        8 => make_stuff_arm(st, &mut trace, env),
        10 => {
            st.production_step = step + 1;
            trace.stages.push(Stage::CreateBuildings);
        }
        11 => {
            // 0x006C1B8F: no pre-increment; the cycle ends at 0x006C1B96.
            trace.stages.push(Stage::MakeStuff);
            st.production_step = 0;
        }
        _ => unreachable!("the jump-table window is 1..=11"),
    }

    trace.step_after = st.production_step;
    trace
}

/// Step 1, `0x006C19E8` — the BHS opening book.
fn run_script_arm(st: &mut ProductionState, trace: &mut ProductionTrace, who: i32) {
    trace.stages.push(Stage::RunProductionScript {
        script_step: st.script_step,
        pers_arg: st.pers_arg,
        who_plus_one: who.wrapping_add(1),
    });
    let Some(result) = st.script_result else {
        trace.stalls.push(Stall::ScriptResult {
            at_va: va::GET_RET_INT,
        });
        return;
    };
    // 0x006C1A89: BLOCK_ON_THIS resets the whole cycle, so stages 2..11 never run.
    if result == SCRIPT_BLOCK_ON_THIS {
        st.production_step = 0;
        return;
    }
    // 0x006C1A9F: SCRIPT_DONE retires the script. Nothing in the image re-arms it.
    if result == SCRIPT_DONE {
        st.prod_script_run = 0;
    }
    // 0x006C1AA9.
    st.production_step = st.production_step.wrapping_add(1);
}

/// Step 8, `0x006C1B4C` — the one arm whose continuation depends on a callee's return.
fn make_stuff_arm(st: &mut ProductionState, trace: &mut ProductionTrace, env: AiEnv) {
    st.production_step = st.production_step.wrapping_add(1);
    trace.stages.push(Stage::MakeStuff);
    let Some(result) = st.make_stuff_result else {
        trace.stalls.push(Stall::MakeStuffResult {
            at_va: va::MAKE_STUFF,
        });
        return;
    };
    // 0x006C1B5C: work remained, so steps 9..11 get a turn.
    if result != 0 {
        return;
    }
    // 0x006C1B63: Infinite resources also keeps the cycle alive.
    match env.starting_resources {
        Some(value) => {
            if value != INFINITE_RESOURCES {
                st.production_step = 0;
            }
        }
        None => trace.stalls.push(Stall::StartingResources {
            at_va: 0x006c_1b63,
        }),
    }
}

/// `0x006C1AE4` — the tail shared by steps 3, 4, 5, 6, 7 and 9.
fn infinite_tail(trace: &mut ProductionTrace, env: AiEnv) {
    match env.starting_resources {
        Some(value) => {
            if value == INFINITE_RESOURCES {
                trace.stages.push(Stage::MakeStuff);
                trace.stages.push(Stage::MakeListClear);
            }
        }
        None => trace.stalls.push(Stall::StartingResources {
            at_va: 0x006c_1ae9,
        }),
    }
}

// ===============================================================================================
// Tests
// ===============================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn env(frame: i32) -> AiEnv {
        AiEnv {
            frame,
            ai_speed: 1,
            ai_off: false,
            starting_resources: Some(0),
        }
    }

    #[test]
    fn plan_strategy_phase_has_no_check_explore_bias() {
        // check_explore fires when (who*25 + frame + 12) % 200 == 0, i.e. frame 188 for
        // slot 0. plan_strategy has no such bias, so 188 is emphatically not its phase.
        assert_eq!(plan_phase(0, env(188)), Some(188));
        assert_eq!(plan_phase(0, env(200)), Some(0));
        assert_eq!(plan_phase(2, env(150)), Some(0));
    }

    #[test]
    fn a_zero_ai_speed_is_a_missing_fact_not_a_period() {
        let mut st = ProductionState::default();
        let trace = plan_strategy(
            &mut st,
            0,
            0,
            AiEnv {
                ai_speed: 0,
                frame: 7,
                ..env(7)
            },
        );
        assert_eq!(trace.phase, None);
        assert_eq!(trace.arm, PlanArm::MissingPeriod { ai_speed: 0 });
        assert!(trace.charges());
    }

    #[test]
    fn the_sub_phase_arm_reaches_nothing_and_is_not_charged() {
        let mut st = ProductionState::default();
        let before = st;
        let trace = plan_strategy(&mut st, 0, 0, env(7));
        assert_eq!(trace.arm, PlanArm::NotDue { phase: 7 });
        assert!(!trace.charges());
        assert_eq!(st, before, "retail returns without touching the leader");
    }

    #[test]
    fn one_leader_takes_the_planning_body_once_per_two_hundred_frames() {
        let mut charged = 0;
        let mut body = 0;
        let mut fast_lane = 0;
        for frame in 1..=200 {
            let mut st = ProductionState::default();
            let trace = plan_strategy(&mut st, 0, 0, env(frame));
            if trace.charges() {
                charged += 1;
            }
            match trace.arm {
                PlanArm::PlanningBody { .. } => body += 1,
                PlanArm::MissingMakeList => fast_lane += 1,
                _ => {}
            }
        }
        assert_eq!(body, 1, "phase == 0 exactly once per period");
        assert_eq!(fast_lane, 6, "phase in {{30,60,90,120,150,180}}");
        assert_eq!(charged, 7);
    }

    #[test]
    fn the_tech_window_is_the_unsigned_compare_at_0x006b96a6() {
        let mut st = ProductionState::default();
        st.make_list_head = Some(0x21f);
        let trace = plan_strategy(&mut st, 0, 0, env(30));
        assert_eq!(trace.arm, PlanArm::TechQueueHeadRejected { head: 0x21f });
        assert!(!trace.charges());

        st.make_list_head = Some(0x274);
        let trace = plan_strategy(&mut st, 0, 0, env(30));
        assert_eq!(
            trace.arm,
            PlanArm::TechQueueReached {
                head: 0x274,
                first_call_va: va::CAN_PAY
            }
        );
        assert!(trace.charges());

        st.make_list_head = Some(0x275);
        let trace = plan_strategy(&mut st, 0, 0, env(30));
        assert_eq!(trace.arm, PlanArm::TechQueueHeadRejected { head: 0x275 });

        // The `lea`/`ja` pair is unsigned, so a negative head is far above the window.
        st.make_list_head = Some(-1);
        let trace = plan_strategy(&mut st, 0, 0, env(30));
        assert_eq!(trace.arm, PlanArm::TechQueueHeadRejected { head: -1 });
    }

    #[test]
    fn a_running_cycle_ignores_the_phase_entirely() {
        let mut st = ProductionState::default();
        st.production_step = 2;
        st.queued_units = Some(4);
        st.control = 10;
        // frame 7 is a sub-phase miss; the production arm must run anyway.
        let trace = plan_strategy(&mut st, 0, 0, env(7));
        assert_eq!(trace.phase, Some(7));
        let PlanArm::ProductionAi(production) = trace.arm else {
            panic!("a non-zero production_step must delegate");
        };
        assert_eq!(production.gate, ProductionGate::Entered);
        assert_eq!(production.step_before, 2);
        assert_eq!(production.step_after, 3);
        assert_eq!(production.effective_pop, Some(15));
        assert_eq!(st.effective_pop, 15);
        assert_eq!(
            production.stages,
            vec![Stage::ProductionAiSetup, Stage::MakeListClear]
        );
        assert!(production.stalls.is_empty());
    }

    #[test]
    fn every_refusal_resets_the_step_and_reaches_nothing() {
        for (flags, flags2_bits, ai_off, expect) in [
            (flags::HUMAN, 0, false, ProductionGate::Human),
            (0, 0, true, ProductionGate::AiOff),
            (
                0,
                flags2::PRODUCTION_AI_DISABLED,
                false,
                ProductionGate::ProductionAiDisabled,
            ),
        ] {
            let mut st = ProductionState {
                production_step: 5,
                flags2: flags2_bits,
                queued_units: Some(3),
                ..ProductionState::default()
            };
            let trace = production_ai(
                &mut st,
                flags,
                0,
                AiEnv {
                    ai_off,
                    ..env(1)
                },
            );
            assert_eq!(trace.gate, expect);
            assert_eq!(st.production_step, 0, "0x006C1B96 resets the counter");
            assert!(trace.stages.is_empty());
            assert!(!trace.charges());
            assert_eq!(st.effective_pop, 0, "the write is after the gate");
        }
    }

    #[test]
    fn a_human_with_the_companion_bit_still_runs_the_cycle() {
        let mut st = ProductionState {
            production_step: 2,
            queued_units: Some(0),
            ..ProductionState::default()
        };
        let trace = production_ai(
            &mut st,
            flags::HUMAN | flags::PRODUCTION_DESPITE_HUMAN,
            0,
            env(1),
        );
        assert_eq!(trace.gate, ProductionGate::Entered);
        assert_eq!(st.production_step, 3);
    }

    #[test]
    fn a_step_outside_the_jump_table_resets_to_zero() {
        for step in [-3, 12, 99] {
            let mut st = ProductionState {
                production_step: step,
                queued_units: Some(1),
                ..ProductionState::default()
            };
            let trace = production_ai(&mut st, 0, 0, env(1));
            assert_eq!(trace.gate, ProductionGate::StepOutsideTable { step });
            assert_eq!(st.production_step, 0);
            assert!(!trace.charges());
        }
    }

    #[test]
    fn step_one_is_skipped_when_the_script_is_already_retired() {
        let mut st = ProductionState {
            production_step: 1,
            prod_script_run: 0,
            queued_units: Some(0),
            ..ProductionState::default()
        };
        let trace = production_ai(&mut st, 0, 0, env(1));
        assert_eq!(st.production_step, 3, "1 -> 2 pre-check, then the step-2 arm");
        assert!(trace.stages.contains(&Stage::ProductionAiSetup));
    }

    #[test]
    fn infinite_resources_skips_the_script_and_arms_the_shared_tail() {
        let mut st = ProductionState {
            production_step: 1,
            prod_script_run: 1,
            queued_units: Some(0),
            ..ProductionState::default()
        };
        let infinite = AiEnv {
            starting_resources: Some(INFINITE_RESOURCES),
            ..env(1)
        };
        let trace = production_ai(&mut st, 0, 0, infinite);
        assert_eq!(st.production_step, 3);
        assert!(trace.stages.contains(&Stage::ProductionAiSetup));

        // Step 3's arm then also runs make_stuff + MakeList::clear.
        let trace = production_ai(&mut st, 0, 0, infinite);
        assert_eq!(st.production_step, 4);
        assert_eq!(
            trace.stages,
            vec![Stage::FoundCities, Stage::MakeStuff, Stage::MakeListClear]
        );
    }

    #[test]
    fn a_blocking_script_resets_the_cycle_and_script_done_retires_it() {
        let mut st = ProductionState {
            production_step: 1,
            prod_script_run: 1,
            queued_units: Some(0),
            script_result: Some(SCRIPT_BLOCK_ON_THIS),
            ..ProductionState::default()
        };
        production_ai(&mut st, 0, 0, env(1));
        assert_eq!(st.production_step, 0, "BLOCK_ON_THIS starves stages 2..11");
        assert_eq!(st.prod_script_run, 1, "and does not retire the script");

        st.production_step = 1;
        st.script_result = Some(SCRIPT_DONE);
        production_ai(&mut st, 0, 0, env(1));
        assert_eq!(st.production_step, 2);
        assert_eq!(st.prod_script_run, 0, "the one-way latch");
    }

    #[test]
    fn an_unanswered_script_stalls_at_step_one_without_guessing() {
        let mut st = ProductionState {
            production_step: 1,
            prod_script_run: 1,
            queued_units: Some(0),
            ..ProductionState::default()
        };
        let trace = production_ai(&mut st, 0, 0, env(1));
        assert_eq!(st.production_step, 1, "the arm did not advance");
        assert_eq!(
            trace.stalls,
            vec![Stall::ScriptResult {
                at_va: va::GET_RET_INT
            }]
        );
        assert!(trace.charges());
    }

    #[test]
    fn step_eight_ends_the_cycle_only_when_make_stuff_found_nothing() {
        let mut st = ProductionState {
            production_step: 8,
            queued_units: Some(0),
            make_stuff_result: Some(0),
            ..ProductionState::default()
        };
        production_ai(&mut st, 0, 0, env(1));
        assert_eq!(st.production_step, 0, "cycle ends at eight");

        st.production_step = 8;
        st.make_stuff_result = Some(1);
        production_ai(&mut st, 0, 0, env(1));
        assert_eq!(st.production_step, 9, "work remained, so 9..11 get a turn");
    }

    #[test]
    fn the_whole_twelve_stage_cycle_walks_two_through_eleven() {
        let mut st = ProductionState {
            production_step: 2,
            queued_units: Some(0),
            make_stuff_result: Some(1),
            ..ProductionState::default()
        };
        let mut seen = Vec::new();
        for _ in 0..10 {
            if st.production_step == 0 {
                break;
            }
            let trace = production_ai(&mut st, 0, 0, env(1));
            assert_eq!(trace.gate, ProductionGate::Entered);
            seen.extend(trace.stages.iter().copied());
        }
        assert_eq!(st.production_step, 0, "step 11 ends the cycle");
        assert_eq!(
            seen,
            vec![
                Stage::ProductionAiSetup,
                Stage::MakeListClear,
                Stage::FoundCities,
                Stage::ResearchTechs,
                Stage::UpgradeUnits,
                Stage::CreateUnits,
                Stage::CreateBuildings,
                Stage::MakeStuff,
                Stage::CreateUnits,
                Stage::CreateBuildings,
                Stage::MakeStuff,
            ],
            "steps 6 and 9 are the same arm, and so are 7 and 10"
        );
    }

    #[test]
    fn an_unanswered_starting_resources_stalls_instead_of_choosing_a_branch() {
        let mut st = ProductionState {
            production_step: 3,
            queued_units: Some(0),
            ..ProductionState::default()
        };
        let trace = production_ai(
            &mut st,
            0,
            0,
            AiEnv {
                starting_resources: None,
                ..env(1)
            },
        );
        assert_eq!(
            trace.stalls,
            vec![Stall::StartingResources { at_va: 0x006c_1ae9 }]
        );
        assert!(trace.charges());
        assert_eq!(st.production_step, 4, "the arm's own store still happened");
    }

    #[test]
    fn an_absent_queued_units_answer_leaves_effective_pop_alone() {
        let mut st = ProductionState {
            production_step: 2,
            control: 9,
            effective_pop: 1234,
            ..ProductionState::default()
        };
        let trace = production_ai(&mut st, 0, 0, env(1));
        assert_eq!(trace.effective_pop, None);
        assert_eq!(st.effective_pop, 1234, "no invented population");
        assert_eq!(
            trace.stalls,
            vec![Stall::QueuedUnits {
                at_va: va::QUEUED_UNITS
            }]
        );
    }
}
