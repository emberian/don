//! `do_frame` — the retail `Game::do_frame` `0x00591ef0` step schedule over
//! the canonical `Save` state.
//!
//! The 29-step order is the direct-call list inside `0x00591ef0` (decomp
//! `re/decomp-all/00591ef0.c`), cross-checked against the schedule already
//! derived in `crates/don-sim/src/schedule.rs`. Each entry is `Ported`
//! (whole body transcribed from the disassembly), `Partial` (at least one
//! write transcribed and executed, rest stubbed), or `Stub` (no-op, fields
//! untouched). Nothing here is derived from observed diffs — the retail
//! decompilation is the only source; the diff is the oracle.
//!
//! State writes are indexed into `Game::scalars` (the walked block
//! `Game+0x550..+0x6e4`), so `scalars[0..4]` is `Game::frame` (+0x550),
//! `scalars[0x10..0x14]` is `Game::tick` (+0x560), and
//! `scalars[0x14..0x18]` is `Game::market_tick` (+0x564).

use crate::Save;

/// Offsets inside `Game::scalars` (image offset minus 0x550).
pub const FRAME: usize = 0x00; // Game+0x550
pub const TICK: usize = 0x10; // Game+0x560
pub const MARKET_TICK: usize = 0x14; // Game+0x564

/// Whether a step's body has been transcribed from the disassembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    /// Whole body transcribed and executed.
    Ported,
    /// Some writes transcribed and executed; the rest of the body is a
    /// no-op. Not counted as Ported for the burn-down gates.
    Partial,
    /// No-op: the step runs in the schedule but touches nothing.
    Stub,
}

#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub idx: usize,
    pub name: &'static str,
    pub va: Option<u32>,
    pub status: StepStatus,
    pub note: &'static str,
}

/// Retail `Game::do_frame` `0x00591ef0` direct-call order (29 steps),
/// names/VAs as documented in `crates/don-sim/src/schedule.rs`.
pub const STEPS: [Step; 29] = [
    Step { idx: 0, name: "AutoSave::restore", va: Some(0x005A20C0), status: StepStatus::Stub,
        note: "only if Game+0x821 & 0x20" },
    Step { idx: 1, name: "GameLog::begin_frame", va: Some(0x00932A70), status: StepStatus::Stub,
        note: "desync-log frame marker" },
    Step { idx: 2, name: "Random::get (artificial lag)", va: Some(0x00A39D70), status: StepStatus::Stub,
        note: "burns LCG draws for lag smoothing" },
    Step { idx: 3, name: "CommandManager::issue_player_speed", va: Some(0x00943100), status: StepStatus::Stub,
        note: "every 8 frames, phase-offset per player" },
    Step { idx: 4, name: "RunTimeEnv::run_script", va: Some(0x0043D0E0), status: StepStatus::Stub,
        note: "scenario/trigger script step, called twice" },
    Step { idx: 5, name: "ConquestGame::place_reinforcements", va: Some(0x00798880), status: StepStatus::Stub,
        note: "Conquer-the-World only" },
    Step { idx: 6, name: "TutorialPromptWin::exec", va: Some(0x007C2810), status: StepStatus::Stub,
        note: "tutorial only" },
    Step { idx: 7, name: "SteamLeaderboards::UploadScore", va: Some(0x00A36190), status: StepStatus::Stub,
        note: "" },
    Step { idx: 8, name: "Leaders::process_all", va: Some(0x006ED2A0), status: StepStatus::Stub,
        note: "" },
    Step { idx: 9, name: "NetDaemon::process_all", va: Some(0x00951300), status: StepStatus::Stub,
        note: "socket pump; retail calls it 5x inside one tick" },
    Step { idx: 10, name: "rush-rules timer expiry", va: None, status: StepStatus::Stub,
        note: "inline block 0x0059225E..0x0059241C: when frame == rush_rules[i].minutes*900, \
               zeroes LeaderData::attrition_stamp and set_diplo(pair, WAR) per neutral pair" },
    Step { idx: 11, name: "Leaders::strategy_all", va: Some(0x006ED430), status: StepStatus::Stub,
        note: "" },
    Step { idx: 12, name: "GameDaemon::process_all", va: Some(0x00732700), status: StepStatus::Partial,
        note: "FUN_00732700 body: market decay + FUN_00732180 market update (writes \
               Game::market_tick +0x564 and market_flux/next_flux/flux_length) + \
               Groups::process FUN_006fa210 (advances Groups::proc_group mod 64). \
               Ported so far: proc_group advance only — the market update needs the \
               rules constants at [c061f0]+0xcd8..0xcec which live in the still-opaque \
               rules region" },
    Step { idx: 13, name: "Armies::process_all", va: Some(0x006F3B00), status: StepStatus::Stub,
        note: "army aggregation" },
    Step { idx: 14, name: "Objects::process_all", va: Some(0x0065DCE0), status: StepStatus::Stub,
        note: "" },
    Step { idx: 15, name: "Objects::inc_time", va: Some(0x0065DB70), status: StepStatus::Stub,
        note: "animation/time advance for all objects" },
    Step { idx: 16, name: "GraphicEvents::process", va: Some(0x008E50A0), status: StepStatus::Stub,
        note: "FX event queue" },
    Step { idx: 17, name: "Leaders::end_process_all", va: Some(0x006ED070), status: StepStatus::Stub,
        note: "" },
    Step { idx: 18, name: "Achieve::capture_data", va: Some(0x007AF980), status: StepStatus::Stub,
        note: "achievements" },
    Step { idx: 19, name: "Leader::process_event_frame", va: Some(0x006EC180), status: StepStatus::Stub,
        note: "" },
    Step { idx: 20, name: "Game::frame++", va: Some(0x005924BF), status: StepStatus::Ported,
        note: "Game+0x550 += 1 (decomp 00591ef0.c:234); runs AFTER Objects::process_all, \
               so rotations use the pre-increment frame" },
    Step { idx: 21, name: "OrdersMemManager::cycle", va: Some(0x00730E20), status: StepStatus::Stub,
        note: "flips 28 SafeRecycler pools" },
    Step { idx: 22, name: "Roads::scan_and_kill_stray_roads", va: Some(0x008956A0), status: StepStatus::Stub,
        note: "" },
    Step { idx: 23, name: "frame % 15 -> Game::tick++", va: Some(0x005924CF), status: StepStatus::Ported,
        note: "if frame % 15 == 0: Game+0x560 += 1 (decomp 00591ef0.c:237-238) — this is \
               `Game::tick`, one game second per 15 sim frames; NOT a second per-frame counter. \
               The sibling +0x564 `market_tick` is written by FUN_00732180 inside step 12" },
    Step { idx: 24, name: "TurnControl::check_cannon_time", va: Some(0x009579E0), status: StepStatus::Stub,
        note: "child of step 23's frame%15==0 branch (call at 0x005924E7), not every frame" },
    Step { idx: 25, name: "SaveGame::save_game / LoadGame::load_game", va: Some(0x005A8220), status: StepStatus::Stub,
        note: "autosave, conditional" },
    Step { idx: 26, name: "GameLog::end_frame", va: Some(0x009329D0), status: StepStatus::Stub,
        note: "" },
    Step { idx: 27, name: "Game::process_end_game", va: Some(0x00591CE0), status: StepStatus::Stub,
        note: "" },
    Step { idx: 28, name: "Scene::process_capture_sequence", va: Some(0x008C13C0), status: StepStatus::Stub,
        note: "cinematic capture" },
];

/// Walked-but-nondeterministic fields: excluded from the burn-down oracle
/// because retail derives them from wall-clock, not simulation state.
/// Each entry must cite the retail writer that proves it.
pub const NONDETERMINISTIC_EXCLUSIONS: &[NondeterministicField] = &[NondeterministicField {
    path: "Game.graphic_tick",
    writer_va: 0x00591570,
    justification: "FUN_00591570 (Game::loop tail): reads timeGetTime(), advances \
                    Game+0x844 once per elapsed 66ms quantum, and updates the \
                    Game+0x840 wall-clock baseline. Wall-clock derived; not \
                    deterministic sim state.",
}];

#[derive(Clone, Copy, Debug)]
pub struct NondeterministicField {
    /// Span path the exclusion applies to.
    pub path: &'static str,
    /// VA of the retail writer function.
    pub writer_va: u32,
    /// Why the writer is nondeterministic.
    pub justification: &'static str,
}

/// Returns true when `path` is in the nondeterministic exclusion list.
pub fn is_nondeterministic(path: &str) -> bool {
    NONDETERMINISTIC_EXCLUSIONS.iter().any(|e| path == e.path)
}

/// Per-step record of what `do_frame` did.
#[derive(Clone, Debug)]
pub struct StepRecord {
    pub idx: usize,
    pub name: &'static str,
    pub va: Option<u32>,
    pub status: StepStatus,
    /// Writes the step performed (empty for pure stubs).
    pub effects: Vec<String>,
}

/// What `do_frame` did to a `Save`.
#[derive(Clone, Debug)]
pub struct FrameReport {
    pub steps: Vec<StepRecord>,
    /// `Game::frame` after the tick (the new frame number).
    pub frame: i32,
    /// `Game::tick` after the tick (game seconds; advances every 15 frames).
    pub tick: i32,
    /// `Groups::proc_group` after the tick (0..63 rotation).
    pub proc_group: i32,
}

fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

fn put_i32(buf: &mut [u8], off: usize, v: i32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// Advance the simulation one frame in retail `Game::do_frame` order.
/// Stub and Partial steps run their (possibly empty) transcribed effects
/// at their retail position so ordering is faithful when more systems land.
pub fn do_frame(save: &mut Save) -> FrameReport {
    let mut steps: Vec<StepRecord> = Vec::with_capacity(STEPS.len());
    for s in &STEPS {
        let mut effects = Vec::new();
        match s.idx {
            12 => {
                // Groups::process FUN_006fa210 tail (re/decomp-all/006fa210.c:88-92):
                //   DAT_00e85f50 += 1; if (DAT_00e85f50 > 0x3f) DAT_00e85f50 = 0;
                // Runs once per leader-slot pass completes — once per frame.
                save.groups.proc_group += 1;
                if save.groups.proc_group > 0x3f {
                    save.groups.proc_group = 0;
                }
                effects.push("Groups.proc_group = (proc_group + 1) % 64".into());
            }
            20 => {
                // 00591ef0.c:234 — *(Game+0x550) += 1.
                let f = get_i32(&save.game.scalars, FRAME) + 1;
                put_i32(&mut save.game.scalars, FRAME, f);
                effects.push(format!("Game.frame -> {f}"));
            }
            23 => {
                // 00591ef0.c:237-239 — if frame % 15 == 0: Game+0x560 += 1;
                // then FUN_009579e0 (cannon-time expiry; stub, no walked state).
                let f = get_i32(&save.game.scalars, FRAME);
                if f % 15 == 0 {
                    let t = get_i32(&save.game.scalars, TICK) + 1;
                    put_i32(&mut save.game.scalars, TICK, t);
                    effects.push(format!("Game.tick -> {t} (frame {f} % 15 == 0)"));
                }
            }
            8 => crate::systems::leaders_process::run(save, &mut effects),
            14 => crate::systems::objects_process::run(save, &mut effects),
            15 => crate::systems::objects_inc_time::run(save, &mut effects),
            _ => {}
        }
        let status = match s.idx {
            8 => crate::systems::leaders_process::STATUS,
            14 => crate::systems::objects_process::STATUS,
            15 => crate::systems::objects_inc_time::STATUS,
            _ => s.status,
        };
        steps.push(StepRecord {
            idx: s.idx,
            name: s.name,
            va: s.va,
            status,
            effects,
        });
    }
    FrameReport {
        steps,
        frame: get_i32(&save.game.scalars, FRAME),
        tick: get_i32(&save.game.scalars, TICK),
        proc_group: save.groups.proc_group,
    }
}

/// The retail LCG (`FUN_00a39d70` Random::get): `s = s*1664525 + 1013904223`.
/// `get` consumes one step per call and returns bits 16..31 scaled.
pub fn rng_step(s: u32) -> u32 {
    s.wrapping_mul(1664525).wrapping_add(1013904223)
}

/// Number of LCG applications taking `from` to `to`, capped at `1 << 20`.
/// `None` when the chain does not reach `to` within the cap.
pub fn rng_draws(from: u32, to: u32) -> Option<u32> {
    let mut s = from;
    for i in 0..(1u32 << 20) {
        if s == to {
            return Some(i);
        }
        s = rng_step(s);
    }
    if s == to {
        return Some(1 << 20);
    }
    None
}
