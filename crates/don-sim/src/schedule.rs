//! `Game::do_frame` — the sim tick, as an ordered, addressable schedule.
//!
//! # Where this comes from
//!
//! `Game::do_frame` `0x00591EF0` (game.cpp:1840, 1,879 bytes) was disassembled and every
//! direct call symbolised in address order [measured]. That list is [`DO_FRAME`], with
//! the retail VA and source line on every entry. `Game::do_frame` has exactly one caller
//! (`Game::loop` `0x00591570`) and appears in no vtable or function-pointer table, so
//! this list is the whole tick.
//!
//! Keeping the schedule as *data* rather than as a hand-written sequence of calls buys
//! three things: the order cannot drift from the derivation without the table changing,
//! coverage is countable per subsystem instead of estimated, and a reader can diff this
//! file against a fresh disassembly.
//!
//! # The clock, which is two facts and not one
//!
//! `TurnControl::timings` `0x00AFC4A4` is five ints of **milliseconds per sim frame**:
//! `{200, 125, 67, 50, 1}` for Very Slow / Slow / **Normal** / Fast / Hyper Fast
//! [measured]. Normal is therefore 14.925 Hz of wall clock.
//!
//! Separately, `Game::do_frame` itself increments `Game::seconds` every **15** frames
//! (`0x005924CF`: `idiv 15`, `test edx,edx`). The *simulation* defines one second as
//! exactly 15 frames — that is the unit every `"450 frames"` rule value is denominated
//! in. Both are true. A headless environment steps frames and uses 15; only replay
//! timing and wall-clock comparisons need the 67 ms.

/// `TurnControl::timings` `0x00AFC4A4`, milliseconds per sim frame [measured].
pub const TIMINGS_MS: [i32; 5] = [200, 125, 67, 50, 1];
/// Index of Normal in `rules.xml`'s `<CATEGORIES id="gamespeeds">`.
pub const SPEED_NORMAL: usize = 2;
/// Sim frames per game second — `Game::do_frame`'s own `idiv 15` at `0x005924CF`.
pub const FRAMES_PER_SECOND: i32 = 15;

/// How faithfully this port covers one subsystem call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepStatus {
    /// Some derived behaviour runs here.
    Implemented,
    /// The step exists and is entered, but its body is not ported. Counted, never faked.
    Stub,
    /// Presentation, telemetry, or session management: correctly absent from a headless
    /// deterministic core. Not a gap.
    OutOfScope,
}

/// One entry of `Game::do_frame`'s ordered call list.
#[derive(Clone, Copy, Debug)]
pub struct SubsystemStep {
    /// Position in the retail call order.
    pub idx: u8,
    pub name: &'static str,
    /// Retail VA, or `None` for an inline block rather than a call.
    pub va: Option<&'static str>,
    pub source: &'static str,
    pub status: StepStatus,
    pub note: &'static str,
}

/// The tick, in retail's own order [measured, `0x00591EF0`].
pub const DO_FRAME: [SubsystemStep; 29] = [
    SubsystemStep { idx: 0, name: "AutoSave::restore", va: Some("0x005A20C0"), source: "autosave.cpp:115",
        status: StepStatus::OutOfScope, note: "only if Game+0x821 & 0x20" },
    SubsystemStep { idx: 1, name: "GameLog::begin_frame", va: Some("0x00932A70"), source: "gamelog.cpp:333",
        status: StepStatus::OutOfScope, note: "desync-log frame marker" },
    SubsystemStep { idx: 2, name: "Random::get (artificial lag)", va: Some("0x00A39D70"), source: "random.cpp:28",
        status: StepStatus::OutOfScope,
        note: "debug artificial-lag only, gated on Game+0x9E0; NOT a sim draw" },
    SubsystemStep { idx: 3, name: "CommandManager::issue_player_speed", va: Some("0x00943100"), source: "commandmanager.cpp:119",
        status: StepStatus::OutOfScope, note: "every 8 frames, phase-offset per player" },
    SubsystemStep { idx: 4, name: "RunTimeEnv::run_script", va: Some("0x0043D0E0"), source: "runtimeenv.h:113",
        status: StepStatus::Stub, note: "scenario/trigger script step, called twice" },
    SubsystemStep { idx: 5, name: "ConquestGame::place_reinforcements", va: Some("0x00798880"), source: "conquestgame.cpp:6202",
        status: StepStatus::OutOfScope, note: "Conquer-the-World only" },
    SubsystemStep { idx: 6, name: "TutorialPromptWin::exec", va: Some("0x007C2810"), source: "tutorialpromptwin.cpp:14",
        status: StepStatus::OutOfScope, note: "tutorial only" },
    SubsystemStep { idx: 7, name: "SteamLeaderboards::UploadScore", va: Some("0x00A36190"), source: "steamleaderboards.cpp:129",
        status: StepStatus::OutOfScope, note: "" },
    SubsystemStep { idx: 8, name: "Leaders::process_all", va: Some("0x006ED2A0"), source: "leaders.cpp:28778",
        status: StepStatus::Stub,
        note: "per-player economy: gather, calc_wall_stats, calc_unit_stats, process_elimination, process_taunt" },
    SubsystemStep { idx: 9, name: "NetDaemon::process_all", va: Some("0x00951300"), source: "netdaemon.cpp:34",
        status: StepStatus::OutOfScope, note: "socket pump; retail calls it 5x inside one tick" },
    SubsystemStep { idx: 10, name: "AI diplomacy chat", va: None, source: "leaders.cpp",
        status: StepStatus::OutOfScope, note: "Leader::set_diplo -> chat_to_local" },
    SubsystemStep { idx: 11, name: "Leaders::strategy_all", va: Some("0x006ED430"), source: "leaders.cpp:28747",
        status: StepStatus::Stub,
        note: "check_explore, plan_strategy, compute_score, diplomacy (20,348 B), check_victory" },
    SubsystemStep { idx: 12, name: "GameDaemon::process_all", va: Some("0x00732700"), source: "gamedaemon.cpp:1403",
        status: StepStatus::Stub,
        note: "victory, danger map, fog update_all_seen, markets, borders, coll blocks, Groups::process" },
    SubsystemStep { idx: 13, name: "Armies::process_all", va: Some("0x006F3B00"), source: "armies.cpp:695",
        status: StepStatus::Stub, note: "army aggregation" },
    SubsystemStep { idx: 14, name: "Objects::process_all", va: Some("0x0065DCE0"), source: "objects.cpp:5287",
        status: StepStatus::Implemented,
        note: "owner-slot rotation (frame+i)%10 plus the 2000/3000 bands; see crate::objects" },
    SubsystemStep { idx: 15, name: "Objects::inc_time", va: Some("0x0065DB70"), source: "objects.cpp:5434",
        status: StepStatus::Stub, note: "animation/time advance for all objects" },
    SubsystemStep { idx: 16, name: "GraphicEvents::process", va: Some("0x008E50A0"), source: "graphicevent.cpp:434",
        status: StepStatus::OutOfScope, note: "FX event queue" },
    SubsystemStep { idx: 17, name: "Leaders::end_process_all", va: Some("0x006ED070"), source: "leaders.cpp:28794",
        status: StepStatus::Implemented,
        note: "complete eight-Leader deterministic body; localized feedback/audio are emitted as typed presentation requests" },
    SubsystemStep { idx: 18, name: "Achieve::capture_data", va: Some("0x007AF980"), source: "achieve.cpp:244",
        status: StepStatus::OutOfScope, note: "achievements" },
    SubsystemStep { idx: 19, name: "Leader::process_event_frame", va: Some("0x006EC180"), source: "leaders.cpp:28958",
        status: StepStatus::Implemented,
        note: "complete eight-Leader body; exact ordered product outbox owns reached JukeBox and achievement calls" },
    SubsystemStep { idx: 20, name: "Game::frame++", va: Some("0x005924BF"), source: "game.cpp",
        status: StepStatus::Implemented,
        note: "THE frame counter increments HERE, after Objects::process_all -- so the rotation uses the pre-increment frame" },
    SubsystemStep { idx: 21, name: "OrdersMemManager::cycle", va: Some("0x00730E20"), source: "ordmemmgr.cpp:43",
        status: StepStatus::OutOfScope,
        note: "flips 28 SafeRecycler pools; freed orders stay readable until this runs" },
    SubsystemStep { idx: 22, name: "Roads::scan_and_kill_stray_roads", va: Some("0x008956A0"), source: "roads.cpp:3647",
        status: StepStatus::Stub, note: "" },
    SubsystemStep { idx: 23, name: "frame % 15 -> Game::seconds++", va: Some("0x005924CF"), source: "game.cpp",
        status: StepStatus::Implemented, note: "15 sim frames is one game second, exactly" },
    SubsystemStep { idx: 24, name: "TurnControl::check_cannon_time", va: Some("0x009579E0"), source: "turncontrol.cpp:116",
        status: StepStatus::Implemented, note: "75-frame cannon-time expiry and pending speed transition" },
    SubsystemStep { idx: 25, name: "SaveGame::save_game / LoadGame::load_game", va: Some("0x005A8220"), source: "save.cpp:1081",
        status: StepStatus::OutOfScope, note: "autosave, conditional" },
    SubsystemStep { idx: 26, name: "GameLog::end_frame", va: Some("0x009329D0"), source: "gamelog.cpp:354",
        status: StepStatus::OutOfScope, note: "" },
    SubsystemStep { idx: 27, name: "Game::process_end_game", va: Some("0x00591CE0"), source: "game.cpp:2182",
        status: StepStatus::Implemented, note: "consumes semaphore bit 22; statistics/UI/menu effects stay outside the headless core" },
    SubsystemStep { idx: 28, name: "Scene::process_capture_sequence", va: Some("0x008C13C0"), source: "scene.cpp:137",
        status: StepStatus::OutOfScope, note: "cinematic capture" },
];

/// How many times each `DO_FRAME` step ran.
#[derive(Clone, Copy, Debug)]
pub struct ScheduleCoverage {
    pub entered: [u64; DO_FRAME.len()],
}

impl Default for ScheduleCoverage {
    fn default() -> Self {
        ScheduleCoverage {
            entered: [0; DO_FRAME.len()],
        }
    }
}

impl PartialEq for ScheduleCoverage {
    fn eq(&self, other: &Self) -> bool {
        self.entered == other.entered
    }
}
impl Eq for ScheduleCoverage {}

impl ScheduleCoverage {
    #[inline]
    pub fn enter(&mut self, step: usize) {
        self.entered[step] += 1;
    }

    pub fn merge(&mut self, other: &ScheduleCoverage) {
        for (a, b) in self.entered.iter_mut().zip(other.entered.iter()) {
            *a += *b;
        }
    }

    /// (implemented, stub, out-of-scope) counts over the whole schedule.
    pub fn tally() -> (usize, usize, usize) {
        let mut t = (0, 0, 0);
        for s in DO_FRAME.iter() {
            match s.status {
                StepStatus::Implemented => t.0 += 1,
                StepStatus::Stub => t.1 += 1,
                StepStatus::OutOfScope => t.2 += 1,
            }
        }
        t
    }
}

/// Milliseconds of wall clock one sim frame is paced at, by speed index.
pub fn frame_ms(speed: usize) -> i32 {
    TIMINGS_MS[speed.min(TIMINGS_MS.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schedule_is_in_retail_order_and_complete() {
        for (i, s) in DO_FRAME.iter().enumerate() {
            assert_eq!(s.idx as usize, i, "DO_FRAME[{i}] carries the wrong index");
        }
        assert_eq!(DO_FRAME.len(), 29);
    }

    /// The frame counter must increment after `Objects::process_all`, not before. If
    /// these two ever swap, every owner-slot rotation is off by one frame.
    #[test]
    fn frame_increment_follows_objects_process_all() {
        let objs = DO_FRAME
            .iter()
            .position(|s| s.name == "Objects::process_all")
            .unwrap();
        let inc = DO_FRAME
            .iter()
            .position(|s| s.name == "Game::frame++")
            .unwrap();
        assert!(objs < inc);
    }

    #[test]
    fn normal_speed_is_67ms_not_15hz() {
        assert_eq!(frame_ms(SPEED_NORMAL), 67);
        assert_eq!(FRAMES_PER_SECOND, 15);
        // The two numbers disagree by design; a "30 second" 450-frame rule really takes
        // 30.15 s of wall clock at Normal.
        let wall_ms = 450 * frame_ms(SPEED_NORMAL);
        assert_eq!(wall_ms, 30_150);
    }

    #[test]
    fn coverage_tally_adds_up() {
        let (i, s, o) = ScheduleCoverage::tally();
        assert_eq!(i + s + o, DO_FRAME.len());
        assert!(i > 0, "at least one step must actually be implemented");
    }
}
